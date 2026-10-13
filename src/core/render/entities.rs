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
//! in draw order, so the shadow draws under its entity and YSort orders
//! entities within their layer. Composite tiles draw at their own `z_order`;
//! on equal z they follow the entities.
//!
//! Positions come from `GlobalTransform`, interpolated by the frame's alpha
//! for entities with `PreviousGlobalTransform`. A single sprite is placed by
//! its `Renderable::anchor`; tiled runs and composite parts are laid out from
//! the entity's position.

use super::{DrawKind, DrawList, SpriteShading};
use crate::core::asset::SpriteHandle;
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::{
  GlobalTransform, Material, PreviousGlobalTransform, RenderStyle, Renderable, ShaderHandle,
  ShaderParams, SpriteEffect, SpriteShader, World, YSort,
};
use crate::core::graphics::sprite_gpu::effective_shader;
use crate::core::graphics::{
  CameraView, CompositeTile, Sprite, SpriteGpuRenderer, SpriteInstance, TextureId,
};
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

/// What one entity is drawn from.
struct EntityFrame<'a> {
  camera: &'a CameraView,
  /// Render interpolation factor
  alpha: f32,
}

/// What a single-sprite entity is drawn from.
type DrawableItem = (
  &'static Renderable,
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
  Option<&'static Material>,
  Option<&'static YSort>,
);

/// One entity's components, borrowed from the world for the frame.
#[derive(Clone, Copy)]
struct Drawable<'w> {
  renderable: &'w Renderable,
  global: &'w GlobalTransform,
  previous: Option<&'w PreviousGlobalTransform>,
  material: Option<&'w Material>,
  y_sort: Option<&'w YSort>,
}

/// Turns the world's renderable entities into draw list sprites.
#[derive(Default)]
pub(crate) struct EntityDrawer;

impl EntityDrawer {
  /// Push the frame's entities onto `list`, returning how many visible
  /// renderables there were. `alpha` is the render interpolation factor and
  /// `size` the game resolution (sprites entirely off it are skipped).
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn collect(
    &mut self,
    world: &mut World,
    camera: &CameraView,
    alpha: f32,
    size: (u32, u32),
    gpu: &mut SpriteGpuRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    list: &mut DrawList,
  ) -> usize {
    let mut drawables = world.query::<DrawableItem>();
    let mut composites = world.query::<(
      &GlobalTransform,
      Option<&PreviousGlobalTransform>,
      &CompositeTile,
    )>();
    let world: &World = world;
    // The draw list sorts by each entity's own z; this order (y-sorted after
    // plain within a layer) becomes the submission sequence
    let mut items: Vec<(DrawOrder, Drawable<'_>)> = drawables
      .iter(world)
      .filter(|(renderable, ..)| renderable.visible)
      .map(|(renderable, global, previous, material, y_sort)| {
        let order = match y_sort {
          Some(y_sort) => DrawOrder::y_sorted(
            renderable.z_order,
            y_sort_key(y_sort, renderable, global.translation().y),
          ),
          None => DrawOrder::plain(renderable.z_order),
        };
        let drawable = Drawable {
          renderable,
          global,
          previous,
          material,
          y_sort,
        };
        (order, drawable)
      })
      .collect();
    // Stable, so plain sprites keep their order
    items.sort_by(|(a, _), (b, _)| a.cmp(b));

    let frame = EntityFrame { camera, alpha };
    let mut out = Emitter {
      gpu,
      device,
      queue,
      list,
      size,
    };
    for (_, drawable) in &items {
      collect_entity(&frame, drawable, &mut out);
    }
    for (global, previous, composite) in composites.iter(world) {
      collect_composite(&frame, global, previous, composite, &mut out);
    }
    items.len()
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
  id: Option<Option<TextureId>>,
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
  size: (u32, u32),
}

impl Emitter<'_> {
  /// The shared white ellipse texture, uploaded if need be. One texture for
  /// every shadow: size comes from scaling, colour and opacity from the tint.
  fn ellipse_texture(&mut self) -> Option<TextureId> {
    if let Some(id) = self.gpu.named_texture(ELLIPSE_ID) {
      return Some(id);
    }
    let sprite = Sprite::solid_ellipse(ELLIPSE_SIZE, ELLIPSE_SIZE, 255, 255, 255, 255);
    self.gpu.upload_named(
      self.device,
      self.queue,
      ELLIPSE_ID,
      ELLIPSE_SIZE,
      ELLIPSE_SIZE,
      sprite.data(),
    )
  }

  /// The texture behind `source`, uploaded on first use.
  fn resolve(&mut self, source: &mut Source<'_>) -> Option<TextureId> {
    if source.id.is_none() {
      let id = match source.sprite {
        Some(sprite) => self
          .gpu
          .upload_entity_sprite(self.device, self.queue, sprite),
        None => self.ellipse_texture(),
      };
      source.id = Some(id);
    }
    source.id.flatten()
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
fn collect_entity(frame: &EntityFrame<'_>, drawable: &Drawable<'_>, out: &mut Emitter<'_>) {
  let camera = frame.camera;
  let Drawable {
    renderable,
    global,
    previous,
    material,
    y_sort,
  } = *drawable;
  let z = renderable.z_order;

  // Between the last two fixed steps for interpolated entities
  let drawn = render_global(global, previous, frame.alpha);
  let position = drawn.translation();
  let scale = drawn.scale();
  let (x, y, rotation, scale_x, scale_y) =
    (position.x, position.y, drawn.rotation(), scale.x, scale.y);
  let (pivot_x, pivot_y) = renderable.anchor.to_pivot();

  let (scale_x, scale_y) = if let Some(y_sort) = y_sort {
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
      let final_scale_x = scale_x * camera.scale;
      let final_scale_y = scale_y * camera.scale;
      // World size: the camera applies its zoom in `world_to_screen`
      let sprite_w = source.width as f32 * scale_x;
      let sprite_h = source.height as f32 * scale_y;

      // Placement takes the top-left corner and turns about the middle, so
      // the corner moves to keep the pivot at (x, y) through any rotation
      let unturned_top_left =
        pivot_x.abs() < 0.001 && pivot_y.abs() < 0.001 && rotation_deg.abs() <= 0.01;
      let (draw_x, draw_y) = if unturned_top_left {
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

      let (screen_x, screen_y) = camera.world_to_screen_xy(draw_x, draw_y);
      if !camera.is_visible_rect(draw_x, draw_y, sprite_w, sprite_h) {
        return;
      }

      if let Some(shadow) = &renderable.drop_shadow {
        let (shadow_screen_x, shadow_screen_y) =
          camera.world_to_screen_xy(draw_x + shadow.offset.x, draw_y + shadow.offset.y);
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
          let (blob_screen_x, blob_screen_y) = camera.world_to_screen_xy(blob_x, blob_y);
          let blob_w = (eff_radius_x * 2.0) as u32;
          let blob_h = (eff_radius_y * 2.0) as u32;
          out.draw(
            z,
            &mut Source::ellipse(blob_w, blob_h),
            Placement::at(blob_screen_x, blob_screen_y, camera.scale, camera.scale),
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
          let (contact_screen_x, contact_screen_y) =
            camera.world_to_screen_xy(contact_x, contact_y);
          let contact_w = (contact_radius_x * 2.0) as u32;
          let contact_h = (contact_radius_y * 2.0) as u32;
          out.draw(
            z,
            &mut Source::ellipse(contact_w.max(2), contact_h.max(2)),
            Placement::at(
              contact_screen_x,
              contact_screen_y,
              camera.scale,
              camera.scale,
            ),
            Shading::tinted(opaque(contact.color), contact_alpha),
          );
        }
      }

      // A Material component wins over the inline shader
      let shader = effective_shader(material, renderable.shader.as_ref());
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
      let final_scale_x = scale_x * camera.scale;
      let final_scale_y = scale_y * camera.scale;

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
            camera.world_to_screen_xy(world_x + shadow.offset.x, world_y + shadow.offset.y);
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
        let (screen_x, screen_y) = camera.world_to_screen_xy(world_x, world_y);
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

/// A multi-part tile: static parts once, tiled parts repeated, each part at
/// the composite's `z_order`, laid out from the entity's position.
fn collect_composite(
  frame: &EntityFrame<'_>,
  global: &GlobalTransform,
  previous: Option<&PreviousGlobalTransform>,
  composite: &CompositeTile,
  out: &mut Emitter<'_>,
) {
  let camera = frame.camera;
  if !composite.visible {
    return;
  }
  // Blended for interpolated entities, like single sprites
  let global = &render_global(global, previous, frame.alpha);

  let z = composite.z_order;
  let origin = global.translation();
  let scale = global.scale();
  let final_scale_x = scale.x * camera.scale;
  let final_scale_y = scale.y * camera.scale;

  for part in &composite.parts {
    let mut source = Source::sprite(&part.sprite);
    let placement = |world_x: f32, world_y: f32| {
      let (screen_x, screen_y) = camera.world_to_screen_xy(world_x, world_y);
      Placement::at(screen_x, screen_y, final_scale_x, final_scale_y)
        .flipped(part.flip_x, part.flip_y)
    };

    match &part.tiling {
      None => {
        let at = placement(origin.x + part.offset_x, origin.y + part.offset_y);
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
            origin.x + part.offset_x + start_x + offset_dx * i as f32,
            origin.y + part.offset_y + start_y + offset_dy * i as f32,
          );
          out.draw(z, &mut source, at, Shading::PLAIN);
        }
      }
    }
  }
}

/// The y a y-sorted entity sorts by: the point `foot_offset` of the way down
/// its sprite (0 top, 1 bottom), plus `sort_offset`.
fn y_sort_key(y_sort: &YSort, renderable: &Renderable, y: f32) -> f32 {
  let height = renderable.sprite.height() as f32;
  // `y` is the anchor point; `calculate_sort_y` measures from the top
  let (_, pivot_y) = renderable.anchor.to_pivot();
  y_sort.calculate_sort_y(y - pivot_y * height, height)
}

#[cfg(test)]
mod draw_order_tests {
  use super::{DrawOrder, y_sort_key};
  use crate::core::asset::SpriteHandle;
  use crate::core::ecs::{Anchor, Renderable, YSort};
  use crate::core::graphics::Sprite;

  fn sprite_20_high(anchor: Anchor) -> Renderable {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(10, 20, 255, 255, 255, 255), "tall");
    Renderable::sprite(sprite).with_anchor(anchor)
  }

  #[test]
  fn the_sort_key_is_the_feet_wherever_the_anchor_is() {
    let y_sort = YSort::new();
    // A 20 px sprite placed at y = 100: its feet are at 110 centred, 100
    // bottom-anchored, 120 top-anchored
    assert_eq!(
      y_sort_key(&y_sort, &sprite_20_high(Anchor::Center), 100.0),
      110.0
    );
    assert_eq!(
      y_sort_key(&y_sort, &sprite_20_high(Anchor::BottomCenter), 100.0),
      100.0
    );
    assert_eq!(
      y_sort_key(&y_sort, &sprite_20_high(Anchor::TopLeft), 100.0),
      120.0
    );
    // Foot offset 0 sorts by the top
    let by_top = YSort::new().with_foot_offset(0.0);
    assert_eq!(
      y_sort_key(&by_top, &sprite_20_high(Anchor::Center), 100.0),
      90.0
    );
  }

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
