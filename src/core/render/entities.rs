//! Entities on the GPU world path
//!
//! Collects what the CPU renderer drew pixel by pixel (sprites, shadows,
//! tiles and composite tiles) into the frame's [`DrawList`] as sprite
//! instances. Placement follows the CPU renderer exactly for unrotated
//! sprites: the top-left corner is the screen position truncated to a whole
//! pixel, the size is `ceil(width * scale)` by `ceil(height * scale)`, and
//! nearest sampling maps whole-number scales onto the same source pixels.
//!
//! Everything an entity emits (its shadows first, then the sprite or its
//! tiles) is a [`DrawKind::Entity`] item at the entity's `z_order`, submitted
//! in render cache order, so the shadow draws under its entity and YSort
//! orders entities within their layer. Composite tiles draw at
//! their own `z_order`; on equal z they follow the entities.

use super::{DrawKind, DrawList, SpriteShading, TextureRef};
use crate::core::asset::SpriteHandle;
use crate::core::ecs::{
  Entity, Inactive, Material, PreviousTransform, RenderStyle, Renderable, ShaderHandle,
  ShaderParams, SpriteEffect, SpriteShader, Transform, World, WorldTransform, YSort,
};
use crate::core::graphics::sprite_gpu::effective_shader;
use crate::core::graphics::{Camera, CompositeTile, Sprite, SpriteGpuRenderer, SpriteInstance};
use crate::core::graphics::{TileDirection, effect_padding};
use crate::core::ui::Color;

/// Side of the one white ellipse texture every blob and contact shadow is
/// drawn from, scaled and tinted.
const ELLIPSE_SIZE: u32 = 128;

/// Id of the shared shadow ellipse texture.
const ELLIPSE_ID: &str = "\u{1}shadow-ellipse";

/// Rotation (degrees) below which a sprite counts as axis-aligned, as in the
/// CPU renderer.
const AXIS_ALIGNED_DEGREES: f32 = 0.01;

/// Where an entity falls in the draw order: by z layer, then in each layer
/// plain sprites before y-sorted ones, and y-sorted ones by their sort y.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DrawOrder {
  z: i32,
  /// The sort y of a y-sorted entity
  sort_y: Option<f32>,
}

impl DrawOrder {
  pub(crate) fn plain(z: i32) -> Self {
    Self { z, sort_y: None }
  }

  pub(crate) fn y_sorted(z: i32, sort_y: f32) -> Self {
    Self {
      z,
      sort_y: Some(sort_y),
    }
  }

  /// Total order; sort with a stable sort so plain sprites keep their order
  pub(crate) fn cmp(&self, other: &Self) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    self
      .z
      .cmp(&other.z)
      .then_with(|| match (self.sort_y, other.sort_y) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(a), Some(b)) => a.total_cmp(&b),
      })
  }
}

/// What [`EntityDrawer::collect`] reads from the engine.
pub(crate) struct EntityFrame<'a> {
  pub(crate) world: &'a World,
  pub(crate) camera: &'a Camera,
  /// The render cache: entities in draw order
  pub(crate) cache: &'a [(Entity, DrawOrder)],
  /// Render interpolation factor
  pub(crate) alpha: f32,
  /// Game resolution, to skip sprites entirely off screen
  pub(crate) size: (u32, u32),
}

/// Turns the world's renderable entities into draw list sprites, keeping the
/// shadow ellipse texture between frames.
#[derive(Default)]
pub(crate) struct EntityDrawer {
  /// The shared shadow ellipse texture, once uploaded
  ellipse: Option<TextureRef>,
}

impl EntityDrawer {
  /// Push the frame's entities onto `list`.
  pub(crate) fn collect(
    &mut self,
    frame: &EntityFrame<'_>,
    gpu: &mut SpriteGpuRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    list: &mut DrawList,
  ) {
    let mut out = Emitter {
      gpu,
      device,
      queue,
      list,
      ellipse: &mut self.ellipse,
      size: frame.size,
    };
    // The draw list sorts by each entity's own z; the cache's order (y-sorted
    // after plain within a layer) becomes the submission sequence
    for &(entity, _) in frame.cache {
      collect_entity(frame, entity, &mut out);
    }
    collect_composites(frame, &mut out);
  }
}

/// Where and how one texture is drawn, in screen pixels.
#[derive(Clone, Copy)]
struct Placement {
  /// Top-left corner (the CPU truncates the float position)
  x: i32,
  y: i32,
  scale_x: f32,
  scale_y: f32,
  rotation_deg: f32,
  flip_x: bool,
  flip_y: bool,
}

impl Placement {
  fn at(x: f32, y: f32, scale_x: f32, scale_y: f32) -> Self {
    Self {
      x: x as i32,
      y: y as i32,
      scale_x,
      scale_y,
      rotation_deg: 0.0,
      flip_x: false,
      flip_y: false,
    }
  }

  fn rotated(mut self, degrees: f32) -> Self {
    self.rotation_deg = degrees;
    self
  }

  fn flipped(mut self, flip_x: bool, flip_y: bool) -> Self {
    self.flip_x = flip_x;
    self.flip_y = flip_y;
    self
  }
}

/// `color` with full alpha: shadow opacity travels separately.
fn opaque(color: Color) -> Color {
  Color::new(color.r, color.g, color.b, 255)
}

/// What a draw samples: an entity sprite, or the shared shadow ellipse
/// standing in at a size, with its texture resolved only when something is
/// actually drawn (so culled or failed sprites cost no upload).
struct Source<'a> {
  /// `None` for the shadow ellipse
  sprite: Option<&'a SpriteHandle>,
  width: u32,
  height: u32,
  /// Resolved lazily: `Some(None)` is a failed upload
  id: Option<Option<TextureRef>>,
}

impl<'a> Source<'a> {
  fn sprite(sprite: &'a SpriteHandle) -> Self {
    Self {
      sprite: Some(sprite),
      width: sprite.width(),
      height: sprite.height(),
      id: None,
    }
  }

  /// The shadow ellipse drawn `width` by `height` pixels (before zoom).
  fn ellipse(width: u32, height: u32) -> Source<'static> {
    Source {
      sprite: None,
      width,
      height,
      id: None,
    }
  }
}

/// How a texture is coloured.
#[derive(Clone, Copy)]
struct Shading<'a> {
  tint: Color,
  alpha: f32,
  effect: Option<&'a SpriteEffect>,
  /// A game shader and its parameters (instead of `effect`)
  custom: Option<(&'a ShaderHandle, &'a ShaderParams)>,
}

impl<'a> Shading<'a> {
  const PLAIN: Shading<'static> = Shading {
    tint: Color::WHITE,
    alpha: 1.0,
    effect: None,
    custom: None,
  };

  fn tinted(tint: Color, alpha: f32) -> Self {
    Self {
      tint,
      alpha,
      effect: None,
      custom: None,
    }
  }

  /// Shade with `shader`, whichever kind it is.
  fn shaded(tint: Color, alpha: f32, shader: Option<&'a SpriteShader>) -> Self {
    let mut shading = Self::tinted(tint, alpha);
    match shader {
      Some(SpriteShader::BuiltIn(effect)) => shading.effect = Some(effect),
      Some(SpriteShader::Custom { shader, params }) => shading.custom = Some((shader, params)),
      None => {}
    }
    shading
  }
}

/// Where collected sprites go.
struct Emitter<'a> {
  gpu: &'a mut SpriteGpuRenderer,
  device: &'a wgpu::Device,
  queue: &'a wgpu::Queue,
  list: &'a mut DrawList,
  ellipse: &'a mut Option<TextureRef>,
  size: (u32, u32),
}

impl Emitter<'_> {
  /// The shared white ellipse texture, uploaded if need be. One texture for
  /// every shadow: size comes from scaling, colour and opacity from the tint.
  fn ellipse_texture(&mut self) -> TextureRef {
    if let Some(id) = self.ellipse.as_ref()
      && self.gpu.has_texture(id)
    {
      return id.clone();
    }
    let id = TextureRef::from(ELLIPSE_ID);
    let sprite = Sprite::solid_ellipse(ELLIPSE_SIZE, ELLIPSE_SIZE, 255, 255, 255, 255);
    self.gpu.upload_texture(
      self.device,
      self.queue,
      &id,
      ELLIPSE_SIZE,
      ELLIPSE_SIZE,
      sprite.data(),
    );
    *self.ellipse = Some(id.clone());
    id
  }

  /// The texture behind `source`, uploaded on first use.
  fn resolve(&mut self, source: &mut Source<'_>) -> Option<TextureRef> {
    if source.id.is_none() {
      let id = match source.sprite {
        Some(sprite) => self
          .gpu
          .upload_entity_sprite(self.device, self.queue, sprite),
        None => Some(self.ellipse_texture()),
      };
      source.id = Some(id);
    }
    source.id.clone().flatten()
  }

  /// Draw `source` at `at` with `shading` as an entity item at `z`.
  fn draw(&mut self, z: i32, source: &mut Source<'_>, at: Placement, shading: Shading<'_>) {
    if shading.alpha <= 0.0 {
      return;
    }
    let mut effect_params = SpriteInstance::new();
    let mut at = at;
    let (mut width, mut height) = (source.width, source.height);
    let mut pad = 0;
    if let Some(effect) = shading.effect {
      effect_params = effect_params.with_effect(effect);
      // Outline and glow draw a bigger image, offset to stay centred: the CPU
      // halves the growth in whole pixels, scales it and rounds
      pad = effect_padding(&effect_params.effect_params).max(0.0) as u32;
      width += 2 * pad;
      height += 2 * pad;
      let offset_x = ((width - source.width) as i32 / 2) as f32 * at.scale_x;
      let offset_y = ((height - source.height) as i32 / 2) as f32 * at.scale_y;
      at.x -= offset_x.round() as i32;
      at.y -= offset_y.round() as i32;
    }
    let Some(mut instance) = instance_for(width, height, at, self.size) else {
      return;
    };
    if pad > 0 {
      // The renderer grows the quad by the padding again: start smaller so it
      // ends up the grown size placed above
      let (shrink_x, shrink_y) = (
        source.width as f32 / width as f32,
        source.height as f32 / height as f32,
      );
      for c in 0..2 {
        instance.transform[0][c] *= shrink_x;
        instance.transform[1][c] *= shrink_y;
      }
    }
    let Some(texture) = self.resolve(source) else {
      return;
    };
    let mut instance = instance.with_tint(shading.tint, shading.alpha);
    if shading.effect.is_some() {
      instance.effect_params = effect_params.effect_params;
      instance.effect_extra = effect_params.effect_extra;
    }
    let shading = match (shading.effect, shading.custom) {
      (Some(_), _) => SpriteShading::Effect,
      (None, Some((shader, params))) => {
        // Params are packed into the instance now; a failed shader draws plain
        match self
          .gpu
          .shade_custom(self.device, shader, params, &mut instance)
        {
          Some(key) => SpriteShading::Custom(key),
          None => SpriteShading::Plain,
        }
      }
      (None, None) => SpriteShading::Plain,
    };
    self
      .list
      .push_sprite(DrawKind::Entity, z, texture, shading, instance);
  }
}

/// The screen-space quad for `texture` placed at `at`, or `None` when nothing
/// would be drawn.
///
/// Unrotated sprites cover `ceil(width * scale)` by `ceil(height * scale)`
/// pixels from the top-left corner. Rotated ones turn about the middle of
/// that box (integer halves, like the CPU) and keep the unrounded size.
fn instance_for(
  width: u32,
  height: u32,
  at: Placement,
  size: (u32, u32),
) -> Option<SpriteInstance> {
  if !(at.scale_x > 0.0 && at.scale_y > 0.0) || !at.scale_x.is_finite() || !at.scale_y.is_finite() {
    return None;
  }
  let scaled_w = (width as f32 * at.scale_x).ceil();
  let scaled_h = (height as f32 * at.scale_y).ceil();
  let (u0, u1) = if at.flip_x { (1.0, 0.0) } else { (0.0, 1.0) };
  let (v0, v1) = if at.flip_y { (1.0, 0.0) } else { (0.0, 1.0) };

  if at.rotation_deg.abs() < AXIS_ALIGNED_DEGREES {
    let (x, y) = (at.x as f32, at.y as f32);
    // Entirely outside the frame
    if x >= size.0 as f32 || y >= size.1 as f32 || x + scaled_w <= 0.0 || y + scaled_h <= 0.0 {
      return None;
    }
    return Some(
      SpriteInstance::new()
        .with_transform(
          x + scaled_w / 2.0,
          y + scaled_h / 2.0,
          0.0,
          scaled_w,
          scaled_h,
        )
        .with_uv(u0, v0, u1, v1),
    );
  }

  // The sprite quad is centred on its translation, so that is the pivot
  let center_x = at.x as f32 + (scaled_w as i32 / 2) as f32;
  let center_y = at.y as f32 + (scaled_h as i32 / 2) as f32;
  let quad_w = width as f32 * at.scale_x;
  let quad_h = height as f32 * at.scale_y;
  let instance = SpriteInstance::new()
    .with_transform(
      center_x,
      center_y,
      at.rotation_deg.to_radians(),
      quad_w,
      quad_h,
    )
    .with_uv(u0, v0, u1, v1);
  Some(instance)
}

/// Where the first tile of a run starts, relative to the entity, for the
/// direction (Up and Left grow backwards).
fn tile_start(direction: TileDirection, tile_count: u32, tile_size: f32) -> (f32, f32) {
  match direction {
    TileDirection::Up => (0.0, -(tile_count as f32 - 1.0) * tile_size),
    TileDirection::Left => (-(tile_count as f32 - 1.0) * tile_size, 0.0),
    TileDirection::Down | TileDirection::Right => (0.0, 0.0),
  }
}

/// One renderable entity: its shadows, then its sprite or tiles.
fn collect_entity(frame: &EntityFrame<'_>, entity: Entity, out: &mut Emitter<'_>) {
  let world = frame.world;
  let camera = frame.camera;
  if world.has::<Inactive>(entity) {
    return;
  }
  let Some(renderable) = world.get::<Renderable>(entity) else {
    return;
  };
  if !renderable.visible {
    return;
  }
  let z = renderable.z_order;
  let material = world.get::<Material>(entity);

  // WorldTransform for hierarchical entities, else the local Transform
  let (
    base_x,
    base_y,
    base_rotation,
    base_scale_x,
    base_scale_y,
    pivot_x,
    pivot_y,
    offset_x,
    offset_y,
  ) = if let Some(wt) = world.get::<WorldTransform>(entity) {
    (
      wt.x,
      wt.y,
      wt.rotation,
      wt.scale_x,
      wt.scale_y,
      wt.pivot_x,
      wt.pivot_y,
      wt.offset_x,
      wt.offset_y,
    )
  } else if let Some(t) = world.get::<Transform>(entity) {
    (
      t.x, t.y, t.rotation, t.scale_x, t.scale_y, t.pivot_x, t.pivot_y, t.offset_x, t.offset_y,
    )
  } else {
    return;
  };

  let interpolated = if renderable.interpolate {
    world
      .get::<PreviousTransform>(entity)
      .filter(|prev| !prev.skip)
  } else {
    None
  };
  let (x, y, rotation, scale_x, scale_y) = match interpolated {
    Some(prev) => {
      let target = Transform {
        x: base_x,
        y: base_y,
        z: 0.0,
        rotation: base_rotation,
        scale_x: base_scale_x,
        scale_y: base_scale_y,
        pivot_x: 0.0,
        pivot_y: 0.0,
        offset_x: 0.0,
        offset_y: 0.0,
      };
      prev.lerp(&target, frame.alpha)
    }
    None => (base_x, base_y, base_rotation, base_scale_x, base_scale_y),
  };

  // The offset is a pixel-level adjustment after the anchor
  let x = x + offset_x;
  let y = y + offset_y;

  let (scale_x, scale_y) = if let Some(y_sort) = world.get::<YSort>(entity) {
    let (depth_x, depth_y) = y_sort.calculate_depth_scale(y);
    (scale_x * depth_x, scale_y * depth_y)
  } else {
    (scale_x, scale_y)
  };

  // Transform.rotation is in radians, placement takes degrees
  let rotation_deg = rotation.to_degrees();
  let alpha = renderable.alpha;
  let tint = renderable.tint;
  let mut source = Source::sprite(&renderable.sprite);

  match &renderable.style {
    RenderStyle::Single => {
      let final_scale_x = scale_x * camera.zoom;
      let final_scale_y = scale_y * camera.zoom;
      let sprite_w = source.width as f32 * final_scale_x;
      let sprite_h = source.height as f32 * final_scale_y;

      // Placement takes the top-left corner and turns about the middle; a
      // custom pivot moves the corner so the pivot stays at (x, y)
      let (draw_x, draw_y) = if pivot_x.abs() < 0.001 && pivot_y.abs() < 0.001 {
        (x, y)
      } else {
        let pivot_to_center_x = (0.5 - pivot_x) * sprite_w;
        let pivot_to_center_y = (0.5 - pivot_y) * sprite_h;
        let (rotated_offset_x, rotated_offset_y) = if rotation_deg.abs() > 0.01 {
          let angle_rad = rotation_deg.to_radians();
          let cos_a = angle_rad.cos();
          let sin_a = angle_rad.sin();
          (
            pivot_to_center_x * cos_a - pivot_to_center_y * sin_a,
            pivot_to_center_x * sin_a + pivot_to_center_y * cos_a,
          )
        } else {
          (pivot_to_center_x, pivot_to_center_y)
        };
        let center_x = x + rotated_offset_x;
        let center_y = y + rotated_offset_y;
        (center_x - sprite_w / 2.0, center_y - sprite_h / 2.0)
      };

      let (screen_x, screen_y) = camera.world_to_screen(draw_x, draw_y);
      if !camera.is_visible(draw_x, draw_y, sprite_w, sprite_h) {
        return;
      }

      if let Some(shadow) = &renderable.drop_shadow {
        let (shadow_screen_x, shadow_screen_y) =
          camera.world_to_screen(draw_x + shadow.offset.x, draw_y + shadow.offset.y);
        let shadow_alpha = (shadow.color.a as f32 / 255.0) * alpha;
        // The shadow colour tints; its alpha goes in with the entity's
        let shadow_tint = Color::new(shadow.color.r, shadow.color.g, shadow.color.b, 255);
        out.draw(
          z,
          &mut source,
          Placement::at(
            shadow_screen_x,
            shadow_screen_y,
            final_scale_x * shadow.scale.x,
            final_scale_y * shadow.scale.y,
          )
          .rotated(rotation_deg)
          .flipped(renderable.flip_x, renderable.flip_y),
          Shading::tinted(shadow_tint, shadow_alpha),
        );
      }

      if let Some(blob) = &renderable.blob_shadow {
        let (eff_radius_x, eff_radius_y) = blob.effective_radius();
        let blob_alpha = blob.effective_alpha() * alpha;
        if eff_radius_x >= 1.0 && eff_radius_y >= 1.0 && blob_alpha > 0.01 {
          // Centred under the sprite's base position, not the rotated corner
          let blob_x = x - eff_radius_x;
          let blob_y = y + blob.offset_y - eff_radius_y;
          let (blob_screen_x, blob_screen_y) = camera.world_to_screen(blob_x, blob_y);
          let blob_w = (eff_radius_x * 2.0) as u32;
          let blob_h = (eff_radius_y * 2.0) as u32;
          out.draw(
            z,
            &mut Source::ellipse(blob_w, blob_h),
            Placement::at(blob_screen_x, blob_screen_y, camera.zoom, camera.zoom),
            Shading::tinted(opaque(blob.color), blob_alpha),
          );
        }
      }

      if let Some(contact) = &renderable.contact_shadow {
        let contact_alpha = contact.intensity * alpha;
        if contact.radius >= 1.0 && contact_alpha > 0.01 {
          // A squashed ellipse at the base of the sprite
          let contact_radius_x = contact.radius;
          let contact_radius_y = contact.radius * 0.4;
          let contact_x = x - contact_radius_x;
          let contact_y = y + contact.offset_y - contact_radius_y;
          let (contact_screen_x, contact_screen_y) = camera.world_to_screen(contact_x, contact_y);
          let contact_w = (contact_radius_x * 2.0) as u32;
          let contact_h = (contact_radius_y * 2.0) as u32;
          out.draw(
            z,
            &mut Source::ellipse(contact_w.max(2), contact_h.max(2)),
            Placement::at(contact_screen_x, contact_screen_y, camera.zoom, camera.zoom),
            Shading::tinted(opaque(contact.color), contact_alpha),
          );
        }
      }

      // A Material component wins over the inline shader
      let shader = effective_shader(material.as_deref(), renderable.shader.as_ref());
      out.draw(
        z,
        &mut source,
        Placement::at(screen_x, screen_y, final_scale_x, final_scale_y)
          .rotated(rotation_deg)
          .flipped(renderable.flip_x, renderable.flip_y),
        Shading::shaded(tint, alpha, shader),
      );
    }

    RenderStyle::Tiled {
      direction,
      target_size,
    } => {
      let tile_size = if direction.is_horizontal() {
        source.width as f32
      } else {
        source.height as f32
      };
      if tile_size <= 0.0 {
        return;
      }

      let tile_count = (target_size / tile_size).ceil() as u32;
      let (offset_dx, offset_dy) = direction.offset(tile_size);
      let (start_x, start_y) = tile_start(*direction, tile_count, tile_size);

      let rotation_rad = rotation_deg.to_radians();
      let cos_r = rotation_rad.cos();
      let sin_r = rotation_rad.sin();
      let final_scale_x = scale_x * camera.zoom;
      let final_scale_y = scale_y * camera.zoom;

      // A tile's world position, relative to the entity, rotated and scaled
      let tile_position = |i: u32| {
        let local_x = start_x + offset_dx * i as f32;
        let local_y = start_y + offset_dy * i as f32;
        let rotated_x = local_x * cos_r - local_y * sin_r;
        let rotated_y = local_x * sin_r + local_y * cos_r;
        (x + rotated_x * scale_x, y + rotated_y * scale_y)
      };

      // Every tile's shadow first
      if let Some(shadow) = &renderable.drop_shadow {
        let shadow_alpha = (shadow.color.a as f32 / 255.0) * alpha;
        let shadow_tint = Color::new(shadow.color.r, shadow.color.g, shadow.color.b, 255);
        for i in 0..tile_count {
          let (world_x, world_y) = tile_position(i);
          let (screen_x, screen_y) =
            camera.world_to_screen(world_x + shadow.offset.x, world_y + shadow.offset.y);
          out.draw(
            z,
            &mut source,
            Placement::at(
              screen_x,
              screen_y,
              final_scale_x * shadow.scale.x,
              final_scale_y * shadow.scale.y,
            )
            .rotated(rotation_deg)
            .flipped(renderable.flip_x, renderable.flip_y),
            Shading::tinted(shadow_tint, shadow_alpha),
          );
        }
      }

      for i in 0..tile_count {
        let (world_x, world_y) = tile_position(i);
        let (screen_x, screen_y) = camera.world_to_screen(world_x, world_y);
        out.draw(
          z,
          &mut source,
          Placement::at(screen_x, screen_y, final_scale_x, final_scale_y)
            .rotated(rotation_deg)
            .flipped(renderable.flip_x, renderable.flip_y),
          Shading::tinted(tint, alpha),
        );
      }
    }
  }
}

/// The multi-part tiles: static parts once, tiled parts repeated, each part
/// at the composite's `z_order`.
fn collect_composites(frame: &EntityFrame<'_>, out: &mut Emitter<'_>) {
  let world = frame.world;
  let camera = frame.camera;
  for entity in world.entities_with_two::<Transform, CompositeTile>() {
    let Some(transform) = world.get::<Transform>(entity) else {
      continue;
    };
    let Some(composite) = world.get::<CompositeTile>(entity) else {
      continue;
    };
    if !composite.visible {
      continue;
    }

    let z = composite.z_order;
    let final_scale_x = transform.scale_x * camera.zoom;
    let final_scale_y = transform.scale_y * camera.zoom;

    for part in &composite.parts {
      let mut source = Source::sprite(&part.sprite);
      let placement = |world_x: f32, world_y: f32| {
        let (screen_x, screen_y) = camera.world_to_screen(world_x, world_y);
        Placement::at(screen_x, screen_y, final_scale_x, final_scale_y)
          .flipped(part.flip_x, part.flip_y)
      };

      match &part.tiling {
        None => {
          let at = placement(transform.x + part.offset_x, transform.y + part.offset_y);
          out.draw(z, &mut source, at, Shading::PLAIN);
        }
        Some((direction, target_size)) => {
          let tile_size = if direction.is_horizontal() {
            source.width as f32
          } else {
            source.height as f32
          };
          if tile_size <= 0.0 || *target_size <= 0.0 {
            continue;
          }

          let tile_count = (target_size / tile_size).ceil() as u32;
          let (offset_dx, offset_dy) = direction.offset(tile_size);
          let (start_x, start_y) = tile_start(*direction, tile_count, tile_size);
          for i in 0..tile_count {
            let at = placement(
              transform.x + part.offset_x + start_x + offset_dx * i as f32,
              transform.y + part.offset_y + start_y + offset_dy * i as f32,
            );
            out.draw(z, &mut source, at, Shading::PLAIN);
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod draw_order_tests {
  use super::DrawOrder;

  #[test]
  fn layers_then_plain_then_sort_y() {
    let mut orders = [
      DrawOrder::y_sorted(0, 50.0),
      DrawOrder::plain(1),
      DrawOrder::y_sorted(0, -20.0),
      DrawOrder::plain(0),
      DrawOrder::y_sorted(i32::MAX, 1e9),
      DrawOrder::plain(i32::MIN),
    ];
    orders.sort_by(|a, b| a.cmp(b));
    assert_eq!(
      orders,
      [
        DrawOrder::plain(i32::MIN),
        DrawOrder::plain(0),
        DrawOrder::y_sorted(0, -20.0),
        DrawOrder::y_sorted(0, 50.0),
        DrawOrder::plain(1),
        DrawOrder::y_sorted(i32::MAX, 1e9),
      ]
    );
  }
}
