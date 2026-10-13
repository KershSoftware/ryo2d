//! Entities on the GPU world path
//!
//! Collects the world's sprites, shadows, tiles and composite tiles into the
//! frame's [`DrawList`] as world-space quads: each instance carries its
//! quad's centre, size (texels times scale), rotation and flips, and the
//! camera's matrix takes it to the screen on the GPU (pixel art snaps
//! axis-aligned quads to whole pixels there). Quads no camera on the
//! entity's [`RenderLayers`] can see are skipped before their texture is
//! uploaded; each camera later keeps the ones it sees.
//!
//! Everything an entity emits (its shadows first, then the sprite or its
//! tiles) is a [`DrawKind::Entity`] item at the entity's `z_order`, submitted
//! in draw order, so the shadow draws under its entity and YSort orders
//! entities within their layer. Composite tiles draw at their own `z_order`;
//! on equal z they follow the entities.
//!
//! A renderable showing a render target draws the target's texture at the
//! target's size; the camera drawing into that target skips it.
//!
//! Positions come from `GlobalTransform`, interpolated by the frame's alpha
//! for entities with `PreviousGlobalTransform`. A single sprite is placed by
//! its `Renderable::anchor`; tiled runs and composite parts are laid out from
//! the entity's position.

use std::collections::HashSet;

use super::{CameraDraw, DrawKind, DrawList, SpriteShading};
use crate::core::asset::SpriteHandle;
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::{
  Entity, GlobalTransform, Material, PreviousGlobalTransform, RenderStyle, Renderable,
  ShaderHandle, ShaderParams, SpriteEffect, SpriteShader, Vec2, World, YSort,
};
use crate::core::graphics::TileDirection;
use crate::core::graphics::sprite_gpu::effective_shader;
use crate::core::graphics::{
  CompositeTile, RenderLayers, RenderTarget, RenderTargets, Sprite, SpriteGpuRenderer,
  SpriteInstance, TextureId,
};
use crate::core::ui::Color;

/// Side of the one white ellipse texture every blob and contact shadow is
/// drawn from, scaled and tinted.
const ELLIPSE_SIZE: u32 = 128;

/// Id of the shared shadow ellipse texture.
const ELLIPSE_ID: &str = "\u{1}shadow-ellipse";

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
struct EntityFrame {
  /// Render interpolation factor
  alpha: f32,
}

/// What a single-sprite entity is drawn from.
type DrawableItem = (
  Entity,
  &'static Renderable,
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
  Option<&'static Material>,
  Option<&'static YSort>,
  Option<&'static RenderLayers>,
);

/// One entity's components, borrowed from the world for the frame.
#[derive(Clone, Copy)]
struct Drawable<'w> {
  entity: Entity,
  renderable: &'w Renderable,
  global: &'w GlobalTransform,
  previous: Option<&'w PreviousGlobalTransform>,
  material: Option<&'w Material>,
  y_sort: Option<&'w YSort>,
  layers: RenderLayers,
}

/// Turns the world's renderable entities into draw list sprites.
#[derive(Default)]
pub(crate) struct EntityDrawer {
  /// Entities already warned about showing a camera its own target
  warned: HashSet<Entity>,
}

impl EntityDrawer {
  /// Push the frame's entities onto `list` as world-space quads, returning
  /// how many visible renderables there were. `alpha` is the render
  /// interpolation factor; quads none of `cameras` can see are skipped.
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn collect(
    &mut self,
    world: &mut World,
    cameras: &[CameraDraw],
    alpha: f32,
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
      Option<&RenderLayers>,
    )>();
    let world: &World = world;
    // The draw list sorts by each entity's own z; this order (y-sorted after
    // plain within a layer) becomes the submission sequence
    let mut items: Vec<(DrawOrder, Drawable<'_>)> = drawables
      .iter(world)
      .filter(|(_, renderable, ..)| renderable.visible)
      .map(
        |(entity, renderable, global, previous, material, y_sort, layers)| {
          let order = match y_sort {
            Some(y_sort) => DrawOrder::y_sorted(
              renderable.z_order,
              y_sort_key(y_sort, renderable, global.translation().y),
            ),
            None => DrawOrder::plain(renderable.z_order),
          };
          let drawable = Drawable {
            entity,
            renderable,
            global,
            previous,
            material,
            y_sort,
            layers: layers.copied().unwrap_or_default(),
          };
          (order, drawable)
        },
      )
      .collect();
    // Stable, so plain sprites keep their order
    items.sort_by(|(a, _), (b, _)| a.cmp(b));

    let frame = EntityFrame { alpha };
    let mut out = Emitter {
      gpu,
      device,
      queue,
      list,
      cameras,
      targets: world.get_resource::<RenderTargets>(),
      layers: RenderLayers::default(),
      shows: None,
    };
    for (_, drawable) in &items {
      let shows = drawable.renderable.target;
      if let Some(target) = shows
        && cameras.iter().any(|camera| camera.target == Some(target))
        && self.warned.insert(drawable.entity)
      {
        crate::engine_warn!(
          module: "Render",
          "{:?} shows the render target its camera draws into; that camera skips it",
          drawable.entity
        );
      }
      out.layers = drawable.layers;
      out.shows = shows;
      collect_entity(&frame, drawable, &mut out);
    }
    out.shows = None;
    for (global, previous, composite, layers) in composites.iter(world) {
      out.layers = layers.copied().unwrap_or_default();
      collect_composite(&frame, global, previous, composite, &mut out);
    }
    items.len()
  }
}

/// Where and how one texture is drawn, in world units: its quad's centre,
/// world units per texel, rotation (radians, about the centre) and flips.
#[derive(Clone, Copy)]
struct Placement {
  center: Vec2,
  scale: Vec2,
  rotation: f32,
  flip_x: bool,
  flip_y: bool,
}

impl Placement {
  fn centered(center: Vec2, scale: Vec2) -> Self {
    Self {
      center,
      scale,
      rotation: 0.0,
      flip_x: false,
      flip_y: false,
    }
  }

  /// The quad of `texels` at `scale` whose unturned top-left is `top_left`
  /// (it turns about its own centre).
  fn from_top_left(top_left: Vec2, texels: Vec2, scale: Vec2) -> Self {
    Self::centered(top_left + texels * scale / 2.0, scale)
  }

  fn rotated(mut self, radians: f32) -> Self {
    self.rotation = radians;
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

  /// A render target's texture `id`, `width` by `height` pixels.
  fn target(id: TextureId, (width, height): (u32, u32)) -> Source<'static> {
    Source {
      sprite: None,
      width,
      height,
      id: Some(Some(id)),
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
  /// Quads none of these cameras on `layers` can see are skipped (and
  /// never uploaded)
  cameras: &'a [CameraDraw],
  /// The render targets' sizes
  targets: Option<&'a RenderTargets>,
  /// The layers of the entity being drawn, and the render target it shows
  layers: RenderLayers,
  shows: Option<RenderTarget>,
}

impl Emitter<'_> {
  /// What showing `target` draws from, once its texture exists.
  fn target_source(&self, target: RenderTarget) -> Option<Source<'static>> {
    let size = self.targets?.size(target)?;
    Some(Source::target(self.gpu.target_texture(target)?, size))
  }

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
    let size = Vec2::new(source.width as f32, source.height as f32) * at.scale;
    if !(size.x > 0.0 && size.y > 0.0) || !size.is_finite() || !at.center.is_finite() {
      return;
    }
    // The quad's half-diagonal covers it at any rotation
    let reach = Vec2::splat(size.length() / 2.0);
    let (min, max) = (at.center - reach, at.center + reach);
    let seen = self
      .cameras
      .iter()
      .any(|camera| camera.layers.intersects(&self.layers) && camera.view.is_visible(min, max));
    if !seen {
      return;
    }
    let (u0, u1) = if at.flip_x { (1.0, 0.0) } else { (0.0, 1.0) };
    let (v0, v1) = if at.flip_y { (1.0, 0.0) } else { (0.0, 1.0) };
    let Some(texture) = self.resolve(source) else {
      return;
    };
    // Outline and glow draw a bigger image: the renderer grows the quad by
    // the effect's padding about its centre
    let mut instance = SpriteInstance::new()
      .with_transform(at.center.x, at.center.y, at.rotation, size.x, size.y)
      .with_uv(u0, v0, u1, v1)
      .with_tint(shading.tint, shading.alpha);
    if let Some(effect) = shading.effect {
      let effect = SpriteInstance::new().with_effect(effect);
      instance.effect_params = effect.effect_params;
      instance.effect_extra = effect.effect_extra;
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
    self.list.push_sprite_in(
      DrawKind::Entity,
      z,
      texture,
      shading,
      instance,
      (self.layers, self.shows),
    );
  }
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
fn collect_entity(frame: &EntityFrame, drawable: &Drawable<'_>, out: &mut Emitter<'_>) {
  let Drawable {
    renderable,
    global,
    previous,
    material,
    y_sort,
    ..
  } = *drawable;
  let z = renderable.z_order;

  // Between the last two fixed steps for interpolated entities
  let drawn = render_global(global, previous, frame.alpha);
  let position = drawn.translation();
  let rotation = drawn.rotation();
  let mut scale = drawn.scale();
  if let Some(y_sort) = y_sort {
    let (depth_x, depth_y) = y_sort.calculate_depth_scale(position.y);
    scale *= Vec2::new(depth_x, depth_y);
  }
  let (pivot_x, pivot_y) = renderable.anchor.to_pivot();
  let alpha = renderable.alpha;
  let tint = renderable.tint;
  let mut source = match renderable.target {
    // A missing target (removed, or not made yet) draws nothing
    Some(target) => match out.target_source(target) {
      Some(source) => source,
      None => return,
    },
    None => Source::sprite(&renderable.sprite),
  };
  let texels = Vec2::new(source.width as f32, source.height as f32);

  match &renderable.style {
    RenderStyle::Single => {
      // The anchor point sits on the position; the sprite turns about it
      let size = texels * scale;
      let pivot_to_center = Vec2::new(0.5 - pivot_x, 0.5 - pivot_y) * size;
      let center = position + Vec2::from_angle(rotation).rotate(pivot_to_center);
      // The unturned box's corner, which shadows are offset from
      let top_left = center - size / 2.0;

      if let Some(shadow) = &renderable.drop_shadow {
        let shadow_alpha = (shadow.color.a as f32 / 255.0) * alpha;
        // The shadow colour tints; its alpha goes in with the entity's
        let shadow_tint = Color::new(shadow.color.r, shadow.color.g, shadow.color.b, 255);
        out.draw(
          z,
          &mut source,
          Placement::from_top_left(top_left + shadow.offset, texels, scale * shadow.scale)
            .rotated(rotation)
            .flipped(renderable.flip_x, renderable.flip_y),
          Shading::tinted(shadow_tint, shadow_alpha),
        );
      }

      if let Some(blob) = &renderable.blob_shadow {
        let (eff_radius_x, eff_radius_y) = blob.effective_radius();
        let blob_alpha = blob.effective_alpha() * alpha;
        if eff_radius_x >= 1.0 && eff_radius_y >= 1.0 && blob_alpha > 0.01 {
          // Centred under the sprite's base position, not the rotated corner
          let blob_w = (eff_radius_x * 2.0) as u32;
          let blob_h = (eff_radius_y * 2.0) as u32;
          let corner = position + Vec2::new(-eff_radius_x, blob.offset_y - eff_radius_y);
          out.draw(
            z,
            &mut Source::ellipse(blob_w, blob_h),
            Placement::from_top_left(corner, Vec2::new(blob_w as f32, blob_h as f32), Vec2::ONE),
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
          let contact_w = ((contact_radius_x * 2.0) as u32).max(2);
          let contact_h = ((contact_radius_y * 2.0) as u32).max(2);
          let corner = position + Vec2::new(-contact_radius_x, contact.offset_y - contact_radius_y);
          out.draw(
            z,
            &mut Source::ellipse(contact_w, contact_h),
            Placement::from_top_left(
              corner,
              Vec2::new(contact_w as f32, contact_h as f32),
              Vec2::ONE,
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
        Placement::centered(center, scale)
          .rotated(rotation)
          .flipped(renderable.flip_x, renderable.flip_y),
        Shading::shaded(tint, alpha, shader),
      );
    }

    RenderStyle::Tiled {
      direction,
      target_size,
    } => {
      let tile_size = if direction.is_horizontal() {
        texels.x
      } else {
        texels.y
      };
      if tile_size <= 0.0 {
        return;
      }

      let tile_count = (target_size / tile_size).ceil() as u32;
      let (offset_dx, offset_dy) = direction.offset(tile_size);
      let (start_x, start_y) = tile_start(*direction, tile_count, tile_size);
      let turn = Vec2::from_angle(rotation);

      // A tile's top-left, relative to the entity, rotated and scaled
      let tile_corner = |i: u32| {
        let local = Vec2::new(
          start_x + offset_dx * i as f32,
          start_y + offset_dy * i as f32,
        );
        position + turn.rotate(local) * scale
      };

      // Every tile's shadow first
      if let Some(shadow) = &renderable.drop_shadow {
        let shadow_alpha = (shadow.color.a as f32 / 255.0) * alpha;
        let shadow_tint = Color::new(shadow.color.r, shadow.color.g, shadow.color.b, 255);
        for i in 0..tile_count {
          out.draw(
            z,
            &mut source,
            Placement::from_top_left(tile_corner(i) + shadow.offset, texels, scale * shadow.scale)
              .rotated(rotation)
              .flipped(renderable.flip_x, renderable.flip_y),
            Shading::tinted(shadow_tint, shadow_alpha),
          );
        }
      }

      for i in 0..tile_count {
        out.draw(
          z,
          &mut source,
          Placement::from_top_left(tile_corner(i), texels, scale)
            .rotated(rotation)
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
  frame: &EntityFrame,
  global: &GlobalTransform,
  previous: Option<&PreviousGlobalTransform>,
  composite: &CompositeTile,
  out: &mut Emitter<'_>,
) {
  if !composite.visible {
    return;
  }
  // Blended for interpolated entities, like single sprites
  let global = &render_global(global, previous, frame.alpha);

  let z = composite.z_order;
  let origin = global.translation();
  let scale = global.scale();

  for part in &composite.parts {
    let mut source = Source::sprite(&part.sprite);
    let texels = Vec2::new(source.width as f32, source.height as f32);
    let placement = |corner: Vec2| {
      Placement::from_top_left(corner, texels, scale).flipped(part.flip_x, part.flip_y)
    };
    let part_origin = origin + Vec2::new(part.offset_x, part.offset_y);

    match &part.tiling {
      None => {
        out.draw(z, &mut source, placement(part_origin), Shading::PLAIN);
      }
      Some((direction, target_size)) => {
        let tile_size = if direction.is_horizontal() {
          texels.x
        } else {
          texels.y
        };
        if tile_size <= 0.0 || *target_size <= 0.0 {
          continue;
        }

        let tile_count = (target_size / tile_size).ceil() as u32;
        let (offset_dx, offset_dy) = direction.offset(tile_size);
        let (start_x, start_y) = tile_start(*direction, tile_count, tile_size);
        for i in 0..tile_count {
          let corner = part_origin
            + Vec2::new(
              start_x + offset_dx * i as f32,
              start_y + offset_dy * i as f32,
            );
          out.draw(z, &mut source, placement(corner), Shading::PLAIN);
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
