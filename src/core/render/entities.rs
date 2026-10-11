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
//! tiles) is a [`DrawKind::Entity`] item at the entity's sort key, in render
//! cache order, so the shadow draws under its entity. Composite tiles draw at
//! their own `z_order`; on equal z they follow the entities, as the CPU path
//! drew them after all entities.

use std::collections::HashMap;

use super::{DrawKind, DrawList, TextureRef};
use crate::core::asset::SpriteHandle;
use crate::core::ecs::{
  Entity, Inactive, PreviousTransform, RenderStyle, Renderable, SpriteEffect, SpriteShader,
  Transform, World, WorldTransform, YSort,
};
use crate::core::graphics::TileDirection;
use crate::core::graphics::{Camera, CompositeTile, Sprite, SpriteGpuRenderer, SpriteInstance};
use crate::core::ui::Color;

/// Past this many cached shadow ellipses the cache starts over.
const MAX_ELLIPSES: usize = 512;

/// Rotation (degrees) below which a sprite counts as axis-aligned, as in the
/// CPU renderer.
const AXIS_ALIGNED_DEGREES: f32 = 0.01;

/// What [`EntityDrawer::collect`] reads from the engine.
pub(crate) struct EntityFrame<'a> {
  pub(crate) world: &'a World,
  pub(crate) camera: &'a Camera,
  /// The render cache: entities with their sort keys, in draw order
  pub(crate) cache: &'a [(Entity, i32)],
  /// Render interpolation factor
  pub(crate) alpha: f32,
  /// Game resolution, to skip sprites entirely off screen
  pub(crate) size: (u32, u32),
}

/// Turns the world's renderable entities into draw list sprites, keeping the
/// shadow ellipse textures between frames.
#[derive(Default)]
pub(crate) struct EntityDrawer {
  /// Shadow ellipse textures by (width, height, colour)
  ellipses: HashMap<(u32, u32, [u8; 3]), TextureRef>,
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
      ellipses: &mut self.ellipses,
      size: frame.size,
    };
    for &(entity, z) in frame.cache {
      collect_entity(frame, entity, z, &mut out);
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

/// A texture with its size in pixels.
#[derive(Clone)]
struct Texture {
  id: TextureRef,
  width: u32,
  height: u32,
}

/// How a texture is coloured.
#[derive(Clone, Copy)]
struct Shading<'a> {
  tint: Color,
  alpha: f32,
  effect: Option<&'a SpriteEffect>,
}

impl Shading<'_> {
  const PLAIN: Shading<'static> = Shading {
    tint: Color::WHITE,
    alpha: 1.0,
    effect: None,
  };

  fn tinted(tint: Color, alpha: f32) -> Self {
    Self {
      tint,
      alpha,
      effect: None,
    }
  }
}

/// Where collected sprites go.
struct Emitter<'a> {
  gpu: &'a mut SpriteGpuRenderer,
  device: &'a wgpu::Device,
  queue: &'a wgpu::Queue,
  list: &'a mut DrawList,
  ellipses: &'a mut HashMap<(u32, u32, [u8; 3]), TextureRef>,
  size: (u32, u32),
}

impl Emitter<'_> {
  /// The entity sprite's texture, uploaded if need be.
  fn texture(&mut self, sprite: &SpriteHandle) -> Option<Texture> {
    let id = self
      .gpu
      .upload_entity_sprite(self.device, self.queue, sprite)?;
    Some(Texture {
      id,
      width: sprite.width(),
      height: sprite.height(),
    })
  }

  /// A solid ellipse texture of this size and colour (the CPU built a fresh
  /// `Sprite::solid_ellipse` every frame; here it is kept).
  fn ellipse(&mut self, width: u32, height: u32, color: Color) -> Option<Texture> {
    if self.ellipses.len() >= MAX_ELLIPSES {
      self.ellipses.clear();
    }
    let key = (width, height, [color.r, color.g, color.b]);
    let id = match self.ellipses.get(&key) {
      Some(id) if self.gpu.has_texture(id) => id.clone(),
      _ => {
        let id = TextureRef::from(format!(
          "\u{1}ellipse:{width}x{height}:{},{},{}",
          color.r, color.g, color.b
        ));
        let sprite = Sprite::solid_ellipse(width, height, color.r, color.g, color.b, 255);
        self
          .gpu
          .upload_texture(self.device, self.queue, &id, width, height, sprite.data());
        self.ellipses.insert(key, id.clone());
        id
      }
    };
    Some(Texture { id, width, height })
  }

  /// Draw `texture` at `at` with `shading` as an entity item at `z`.
  fn draw(&mut self, z: i32, texture: &Texture, at: Placement, shading: Shading<'_>) {
    if shading.alpha <= 0.0 {
      return;
    }
    let Some(instance) = instance_for(texture, at, self.size) else {
      return;
    };
    let mut instance = instance.with_tint(shading.tint, shading.alpha);
    if let Some(effect) = shading.effect {
      instance = instance.with_effect(effect);
    }
    self.list.push_sprite(
      DrawKind::Entity,
      z,
      texture.id.clone(),
      shading.effect.is_some(),
      instance,
    );
  }
}

/// The screen-space quad for `texture` placed at `at`, or `None` when nothing
/// would be drawn.
///
/// Unrotated sprites cover `ceil(width * scale)` by `ceil(height * scale)`
/// pixels from the top-left corner. Rotated ones turn about the middle of
/// that box (integer halves, like the CPU) and keep the unrounded size.
fn instance_for(texture: &Texture, at: Placement, size: (u32, u32)) -> Option<SpriteInstance> {
  if !(at.scale_x > 0.0 && at.scale_y > 0.0) || !at.scale_x.is_finite() || !at.scale_y.is_finite() {
    return None;
  }
  let scaled_w = (texture.width as f32 * at.scale_x).ceil();
  let scaled_h = (texture.height as f32 * at.scale_y).ceil();
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
  let width = texture.width as f32 * at.scale_x;
  let height = texture.height as f32 * at.scale_y;
  let instance = SpriteInstance::new()
    .with_transform(
      center_x,
      center_y,
      at.rotation_deg.to_radians(),
      width,
      height,
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
fn collect_entity(frame: &EntityFrame<'_>, entity: Entity, z: i32, out: &mut Emitter<'_>) {
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
  let Some(texture) = out.texture(&renderable.sprite) else {
    return;
  };

  match &renderable.style {
    RenderStyle::Single => {
      let final_scale_x = scale_x * camera.zoom;
      let final_scale_y = scale_y * camera.zoom;
      let sprite_w = texture.width as f32 * final_scale_x;
      let sprite_h = texture.height as f32 * final_scale_y;

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
          &texture,
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
          if let Some(ellipse) = out.ellipse(blob_w, blob_h, blob.color) {
            out.draw(
              z,
              &ellipse,
              Placement::at(blob_screen_x, blob_screen_y, camera.zoom, camera.zoom),
              Shading::tinted(Color::WHITE, blob_alpha),
            );
          }
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
          if let Some(ellipse) = out.ellipse(contact_w.max(2), contact_h.max(2), contact.color) {
            out.draw(
              z,
              &ellipse,
              Placement::at(contact_screen_x, contact_screen_y, camera.zoom, camera.zoom),
              Shading::tinted(Color::WHITE, contact_alpha),
            );
          }
        }
      }

      let effect = match &renderable.shader {
        Some(SpriteShader::BuiltIn(effect)) => Some(effect),
        _ => None,
      };
      out.draw(
        z,
        &texture,
        Placement::at(screen_x, screen_y, final_scale_x, final_scale_y)
          .rotated(rotation_deg)
          .flipped(renderable.flip_x, renderable.flip_y),
        Shading {
          tint,
          alpha,
          effect,
        },
      );
    }

    RenderStyle::Tiled {
      direction,
      target_size,
    } => {
      let tile_size = if direction.is_horizontal() {
        texture.width as f32
      } else {
        texture.height as f32
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
            &texture,
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
          &texture,
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
      let Some(texture) = out.texture(&part.sprite) else {
        continue;
      };
      let placement = |world_x: f32, world_y: f32| {
        let (screen_x, screen_y) = camera.world_to_screen(world_x, world_y);
        Placement::at(screen_x, screen_y, final_scale_x, final_scale_y)
          .flipped(part.flip_x, part.flip_y)
      };

      match &part.tiling {
        None => {
          let at = placement(transform.x + part.offset_x, transform.y + part.offset_y);
          out.draw(z, &texture, at, Shading::PLAIN);
        }
        Some((direction, target_size)) => {
          let tile_size = if direction.is_horizontal() {
            texture.width as f32
          } else {
            texture.height as f32
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
            out.draw(z, &texture, at, Shading::PLAIN);
          }
        }
      }
    }
  }
}
