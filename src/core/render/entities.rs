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
//! Everything an entity emits (its shadows first, then the sprite, its
//! tiles or its skeleton's regions) is a [`DrawKind::Entity`] item at the
//! entity's `z_order`, submitted in draw order, so the shadow draws under
//! its entity and YSort orders entities within their layer; ties go by
//! entity index (`entity_order`).
//! Draws from outside the entity query take their places in the same order
//! ([`OrderedDraw`]: tile layers' chunks and y-sorted rows, tile objects).
//! Composite tiles draw at their own `z_order`; on equal z they follow the
//! entities.
//!
//! A renderable showing a render target draws the target's texture at the
//! target's size; the camera drawing into that target skips it.
//!
//! A renderable drawing its entity's skeleton (`RenderStyle::Skeleton`) is
//! one drawable like any other: it sorts as a sprite does, and emits the
//! regions its slots show, back to front in the skeleton's draw order,
//! each stretch of one image and blend as one run item
//! ([`DrawList::push_run_as`]), after its shadows.
//!
//! Positions come from `GlobalTransform`, interpolated by the frame's alpha
//! for entities with `PreviousGlobalTransform`, and lifted by `DrawLift` on
//! a floor (sorted where they stand). A single sprite is placed by its
//! `Renderable::anchor`, and mirrored when its transform is (a negative
//! scale on one axis, on it or above it); tiled runs and composite parts
//! are laid out from the entity's position.

use std::collections::HashSet;

use super::batch::{BlendMode, Filter};
use super::scratch::recycle;
use super::{CameraDraw, DrawChunk, DrawKind, DrawList, DrawRun, SpriteLook, SpriteShading};
use crate::core::asset::{AssetId, Assets};
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::kernel::QueryState;
use crate::core::ecs::{
  DrawLift, Entity, GlobalTransform, Material, PreviousGlobalTransform, RenderStyle, Renderable,
  Shader, ShaderParams, SpriteEffect, SpriteShader, Vec2, World, YSort, entity_order, shaders_of,
};
use crate::core::graphics::TileDirection;
use crate::core::graphics::atlas::Showing;
use crate::core::graphics::sprite_gpu::{ShownImage, effective_shader};
use crate::core::graphics::{
  CompositeTile, Image, RenderLayers, RenderTarget, RenderTargets, SpriteGpuRenderer,
  SpriteInstance, TextureId, images_of,
};
use crate::core::skeleton::{
  Carrier, RegionQuad, Skeleton, SkeletonData, SkeletonStore, SlotBlend,
};
use crate::core::ui::Color;
use glam::Affine2;

/// Side of the one white ellipse texture every blob and contact shadow is
/// drawn from, scaled and tinted.
const ELLIPSE_SIZE: u32 = 128;

/// Id of the shared shadow ellipse texture.
const ELLIPSE_ID: &str = "\u{1}shadow-ellipse";

/// Where an entity falls in the draw order: by z layer, then in each layer
/// plain sprites before y-sorted ones, y-sorted ones by their sort y, then
/// the lower floor (storey) first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DrawOrder {
  pub z: i32,
  /// The sort y of a y-sorted entity
  pub sort_y: Option<f32>,
  /// The floor it is drawn on, 0 the ground floor
  pub floor: u8,
}

impl DrawOrder {
  pub(crate) fn plain(z: i32, floor: u8) -> Self {
    Self {
      z,
      sort_y: None,
      floor,
    }
  }

  pub(crate) fn y_sorted(z: i32, sort_y: f32, floor: u8) -> Self {
    Self {
      z,
      sort_y: Some(sort_y),
      floor,
    }
  }

  /// By layer, sort y, then floor, so at one sort y (where a character
  /// stands under a bridge) the lower floor draws first. Ties (a layer's
  /// plain sprites, y-sorted ones at one sort y, on one floor) are broken
  /// by `entity_order` where the drawables are sorted
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
      .then_with(|| self.floor.cmp(&other.floor))
  }
}

/// Whether what is at `a` (its order, and the entity its ties go by) draws
/// before what is at `b`
fn draws_before(a: (DrawOrder, Entity), b: (DrawOrder, Entity)) -> bool {
  a.0
    .cmp(&b.0)
    .then_with(|| entity_order(&a.1).cmp(&entity_order(&b.1)))
    .is_lt()
}

/// A draw from outside the entity query that takes its place among the
/// entities' draws: by its order, ties by its entity's index, as an entity
/// at that order would. Tile layers make them (`render/tiles.rs`): a plain
/// layer's chunks at its z, a y-sorted layer's rows by their bottom edges,
/// and tile objects
#[derive(Clone, Copy, Debug)]
pub(crate) struct OrderedDraw {
  pub(crate) order: DrawOrder,
  /// What ties at one order go by: the layer or the object
  pub(crate) entity: Entity,
  /// Its place among draws at one order and entity (the order made)
  pub(crate) index: u32,
  /// Drawn before lighting; otherwise after it, at full brightness
  pub(crate) lit: bool,
  /// Drawn by this camera alone (a parallax layer's)
  pub(crate) only_camera: Option<Entity>,
  pub(crate) payload: OrderedPayload,
}

/// What an [`OrderedDraw`] draws
#[derive(Clone, Copy, Debug)]
pub(crate) enum OrderedPayload {
  Chunk(DrawChunk),
  Sprite {
    texture: TextureId,
    look: SpriteLook,
    instance: SpriteInstance,
    layers: RenderLayers,
  },
}

impl OrderedDraw {
  /// Sorts `draws` into draw order, without allocating
  pub(crate) fn sort(draws: &mut [OrderedDraw]) {
    draws.sort_unstable_by(|a, b| {
      a.order
        .cmp(&b.order)
        .then_with(|| entity_order(&a.entity).cmp(&entity_order(&b.entity)))
        .then_with(|| a.index.cmp(&b.index))
    });
  }

  fn push(&self, list: &mut DrawList) {
    let z = self.order.z;
    match self.payload {
      OrderedPayload::Chunk(chunk) => list.push_chunk(self.lit, z, self.only_camera, chunk),
      OrderedPayload::Sprite {
        texture,
        look,
        instance,
        layers,
      } => {
        let seen = (layers, None, self.only_camera);
        list.push_sprite_in(DrawKind::Entity, z, texture, look, instance, seen);
      }
    }
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
  Option<&'static DrawLift>,
  Option<&'static Skeleton>,
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
  /// How far up it draws from where it stands (on a floor)
  lift: Vec2,
  /// The skeleton a `RenderStyle::Skeleton` renderable draws
  skeleton: Option<&'w Skeleton>,
}

/// Turns the world's renderable entities into draw list sprites.
#[derive(Default)]
pub(crate) struct EntityDrawer {
  /// Entities already warned about showing a camera its own target
  warned: HashSet<Entity>,
  /// The shadow ellipse's texture, kept while the renderer keeps it
  ellipse: Option<TextureId>,
  /// The frame's drawables in order (empty between frames; its allocation
  /// is reused)
  items: Vec<(DrawOrder, Drawable<'static>)>,
  /// The regions of the skeleton being drawn (its allocation is reused)
  quads: Vec<RegionQuad>,
}

impl EntityDrawer {
  /// Push the frame's entities onto `list` as world-space quads, with
  /// `ordered` (sorted by [`OrderedDraw::sort`]) each in its place among
  /// them, returning how many visible renderables there were. `alpha` is
  /// the render interpolation factor; quads none of `cameras` can see are
  /// skipped.
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
    ordered: &[OrderedDraw],
  ) -> usize {
    let mut drawables = world.query::<DrawableItem>();
    let mut composites = world.query::<(
      &GlobalTransform,
      Option<&PreviousGlobalTransform>,
      &CompositeTile,
      Option<&RenderLayers>,
      Option<&DrawLift>,
    )>();
    let world: &World = world;
    let images = images_of(world);
    let mut items: Vec<(DrawOrder, Drawable<'_>)> = recycle(std::mem::take(&mut self.items));
    in_draw_order(
      &mut drawables,
      world,
      (images, gpu.showing_old()),
      &mut items,
    );

    let frame = EntityFrame { alpha };
    let mut out = Emitter {
      gpu,
      device,
      queue,
      list,
      cameras,
      targets: world.get_resource::<RenderTargets>(),
      images,
      shaders: shaders_of(world),
      layers: RenderLayers::default(),
      shows: None,
      filter: Filter::Default,
      lit: true,
      ellipse: &mut self.ellipse,
      rigs: world.get_resource::<Assets<SkeletonData>>(),
      skeleton_step: world
        .get_resource::<SkeletonStore>()
        .map(SkeletonStore::step),
      quads: &mut self.quads,
    };
    let mut ordered = ordered.iter().peekable();
    for (order, drawable) in &items {
      // The draws from outside the query that come before this entity
      while let Some(draw) =
        ordered.next_if(|draw| draws_before((draw.order, draw.entity), (*order, drawable.entity)))
      {
        draw.push(out.list);
      }
      let shows = drawable.renderable.target;
      if let Some(target) = shows
        && cameras.iter().any(|camera| camera.target == Some(target))
        && self.warned.insert(drawable.entity)
      {
        log::warn!(
          "{:?} shows the render target its camera draws into; that camera skips it",
          drawable.entity
        );
      }
      out.layers = drawable.layers;
      out.shows = shows;
      out.filter = drawable.renderable.filter;
      out.lit = drawable.renderable.lighting;
      collect_entity(&frame, drawable, &mut out);
    }
    for draw in ordered {
      draw.push(out.list);
    }
    out.shows = None;
    out.filter = Filter::Default;
    out.lit = true;
    for (global, previous, composite, layers, lift) in composites.iter(world) {
      out.layers = layers.copied().unwrap_or_default();
      let lift = lift.map_or(Vec2::ZERO, |drawn| drawn.lift);
      collect_composite(&frame, global, previous, lift, composite, &mut out);
    }
    let count = items.len();
    self.items = recycle(items);
    count
  }
}

/// Add the visible drawables to `items` in draw order. The draw list sorts
/// by each entity's own z; this order (y-sorted after plain within a layer)
/// becomes the submission sequence.
///
/// Ties go by `entity_order` (index order), never by query order: a query
/// visits archetypes in an order that changes whenever an archetype is made,
/// so adding a resource anywhere would reorder overlapping sprites.
fn in_draw_order<'w>(
  drawables: &mut QueryState<DrawableItem>,
  world: &'w World,
  images: (&Assets<Image>, &[Showing]),
  items: &mut Vec<(DrawOrder, Drawable<'w>)>,
) {
  items.extend(
    drawables
      .iter(world)
      .filter(|(_, renderable, ..)| renderable.visible)
      .map(
        |(entity, renderable, global, previous, material, y_sort, layers, lift, skeleton)| {
          // Sorted where it stands, its floor breaking ties
          let floor = lift.map_or(0, DrawLift::draw_floor);
          let order = match y_sort {
            Some(y_sort) => DrawOrder::y_sorted(
              renderable.z_order,
              y_sort_key(y_sort, renderable, global.translation().y, images),
              floor,
            ),
            None => DrawOrder::plain(renderable.z_order, floor),
          };
          let drawable = Drawable {
            entity,
            renderable,
            global,
            previous,
            material,
            y_sort,
            layers: layers.copied().unwrap_or_default(),
            lift: lift.map_or(Vec2::ZERO, |drawn| drawn.lift),
            skeleton,
          };
          (order, drawable)
        },
      ),
  );
  items.sort_unstable_by(|(a, x), (b, y)| {
    a.cmp(b)
      .then_with(|| entity_order(&x.entity).cmp(&entity_order(&y.entity)))
  });
}

/// Each visible renderable of `world` with its draw order, in the order
/// the frame draws them
#[cfg(test)]
pub(crate) fn draw_orders(world: &mut World) -> Vec<(Entity, DrawOrder)> {
  let mut drawables = world.query::<DrawableItem>();
  let world: &World = world;
  let mut items = Vec::new();
  in_draw_order(&mut drawables, world, (images_of(world), &[]), &mut items);
  items
    .iter()
    .map(|(order, drawable)| (drawable.entity, *order))
    .collect()
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

/// What a draw samples: an entity's image, or the shared shadow ellipse
/// standing in at a size, with its texture resolved only when something is
/// actually drawn (so culled or failed images cost no upload).
struct Source<'a> {
  /// `None` for the shadow ellipse and render targets
  image: Option<ShownImage<'a>>,
  width: u32,
  height: u32,
  /// Resolved lazily: `Some(None)` is a failed upload
  id: Option<Option<TextureId>>,
}

impl<'a> Source<'a> {
  /// What `image` draws: itself, or the checker in its place
  fn image(image: ShownImage<'a>) -> Self {
    let (width, height) = image.size();
    Self {
      image: Some(image),
      width,
      height,
      id: None,
    }
  }

  /// A render target's texture `id`, `width` by `height` pixels.
  fn target(id: TextureId, (width, height): (u32, u32)) -> Source<'static> {
    Source {
      image: None,
      width,
      height,
      id: Some(Some(id)),
    }
  }

  /// The shadow ellipse drawn `width` by `height` pixels (before zoom).
  fn ellipse(width: u32, height: u32) -> Source<'static> {
    Source {
      image: None,
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
  custom: Option<(AssetId<Shader>, &'a ShaderParams)>,
  /// Shadows blend over what is below whatever their entity's blend
  blend: BlendMode,
  /// Multiplies the light (see `Renderable::emission`)
  emission: f32,
}

impl<'a> Shading<'a> {
  const PLAIN: Shading<'static> = Shading {
    tint: Color::WHITE,
    alpha: 1.0,
    effect: None,
    custom: None,
    blend: BlendMode::Alpha,
    emission: 1.0,
  };

  fn tinted(tint: Color, alpha: f32) -> Self {
    Self {
      tint,
      alpha,
      effect: None,
      custom: None,
      blend: BlendMode::Alpha,
      emission: 1.0,
    }
  }

  fn blended(self, blend: BlendMode) -> Self {
    Self { blend, ..self }
  }

  fn emitting(self, emission: f32) -> Self {
    Self { emission, ..self }
  }

  /// Shade with `shader`, whichever kind it is.
  fn shaded(tint: Color, alpha: f32, shader: Option<&'a SpriteShader>) -> Self {
    let mut shading = Self::tinted(tint, alpha);
    match shader {
      Some(SpriteShader::BuiltIn(effect)) => shading.effect = Some(effect),
      Some(SpriteShader::Custom { shader, params }) => {
        shading.custom = Some((shader.id(), params));
      }
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
  /// The images entities draw
  images: &'a Assets<Image>,
  /// The game shaders they draw with
  shaders: &'a Assets<Shader>,
  /// The layers of the entity being drawn, and the render target it shows
  layers: RenderLayers,
  shows: Option<RenderTarget>,
  /// The entity's filter, and whether it is lit (its shadows follow it)
  filter: Filter,
  lit: bool,
  /// The shadow ellipse's texture once known (kept across frames)
  ellipse: &'a mut Option<TextureId>,
  /// The skeletons' data, and the number of their last fixed step
  rigs: Option<&'a Assets<SkeletonData>>,
  skeleton_step: Option<u64>,
  /// The regions of the skeleton being drawn (scratch, kept across frames)
  quads: &'a mut Vec<RegionQuad>,
}

impl<'a> Emitter<'a> {
  /// What drawing image `id` draws from: the image, or the checker in its
  /// place (`None`: nothing, in release builds)
  fn image_source(&self, id: crate::core::asset::AssetId<Image>) -> Option<Source<'a>> {
    self.gpu.shown_image(id, self.images).map(Source::image)
  }

  /// What showing `target` draws from, once its texture exists.
  fn target_source(&self, target: RenderTarget) -> Option<Source<'static>> {
    let size = self.targets?.size(target)?;
    Some(Source::target(self.gpu.target_texture(target)?, size))
  }

  /// The shared white ellipse texture, uploaded if need be. One texture for
  /// every shadow: size comes from scaling, colour and opacity from the tint.
  fn ellipse_texture(&mut self) -> Option<TextureId> {
    if let Some(id) = *self.ellipse
      && self.gpu.is_texture(id)
    {
      return Some(id);
    }
    let id = match self.gpu.named_texture(ELLIPSE_ID) {
      Some(id) => id,
      None => {
        let sprite = Image::solid_ellipse(ELLIPSE_SIZE, ELLIPSE_SIZE, 255, 255, 255, 255);
        self.gpu.upload_named(
          self.device,
          self.queue,
          ELLIPSE_ID,
          ELLIPSE_SIZE,
          ELLIPSE_SIZE,
          sprite.data(),
        )?
      }
    };
    *self.ellipse = Some(id);
    Some(id)
  }

  /// The texture behind `source`, uploaded on first use.
  fn resolve(&mut self, source: &mut Source<'_>) -> Option<TextureId> {
    if source.id.is_none() {
      let id = match source.image {
        Some(image) => self.gpu.upload_entity_image(self.device, self.queue, image),
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
      .with_tint(shading.tint, shading.alpha)
      .with_emission(shading.emission);
    if let Some(effect) = shading.effect {
      let effect = SpriteInstance::new().with_effect(effect);
      instance.effect_params = effect.effect_params;
      instance.effect_extra = effect.effect_extra;
    }
    let blend = shading.blend;
    let shading = match (shading.effect, shading.custom) {
      (Some(_), _) => SpriteShading::Effect,
      (None, Some((shader, params))) => {
        // Params are packed into the instance now; a failed shader draws plain
        match self
          .gpu
          .shade_custom(shader, self.shaders, params, &mut instance)
        {
          Some(key) => SpriteShading::Custom(key),
          None => SpriteShading::Plain,
        }
      }
      (None, None) => SpriteShading::Plain,
    };
    let look = SpriteLook {
      shading,
      blend,
      filter: self.filter,
      lit: self.lit,
    };
    self.list.push_sprite_in(
      DrawKind::Entity,
      z,
      texture,
      look,
      instance,
      (self.layers, self.shows, None),
    );
  }
}

/// One quad of a skeleton as a run draws it: the image it shows a
/// rectangle of, how it blends, where it draws, and its tint (the sRGB
/// colour with straight alpha)
struct RunQuad {
  image: AssetId<Image>,
  rect: [u32; 4],
  blend: BlendMode,
  quad: Affine2,
  tint: [f32; 4],
}

/// How every quad of a skeleton's runs is drawn
#[derive(Clone, Copy)]
struct RunLook<'a> {
  /// A built-in effect on every quad
  effect: Option<&'a SpriteEffect>,
  /// A game shader and its parameters on every quad (instead of `effect`)
  custom: Option<(AssetId<Shader>, &'a ShaderParams)>,
  emission: f32,
}

impl RunQuad {
  /// The quad as an instance drawing its rectangle of `shown`, the image
  /// or what stands in for it, with the box it stays inside
  fn instance(&self, shown: &ShownImage<'_>, emission: f32) -> (SpriteInstance, Vec2, Vec2) {
    // Its rectangle of the image as the texture shows it; all of the
    // checker standing in for an image that is not loaded
    let uv = if shown.id == self.image {
      let (width, height) = shown.size();
      let (width, height) = (width.max(1) as f32, height.max(1) as f32);
      let [x, y, w, h] = self.rect.map(|texels| texels as f32);
      [x / width, y / height, (x + w) / width, (y + h) / height]
    } else {
      [0.0, 0.0, 1.0, 1.0]
    };
    let (x, y, at) = (
      self.quad.matrix2.x_axis,
      self.quad.matrix2.y_axis,
      self.quad.translation,
    );
    let mut instance = SpriteInstance::new().with_emission(emission);
    instance.transform[0][..2].copy_from_slice(&[x.x, x.y]);
    instance.transform[1][..2].copy_from_slice(&[y.x, y.y]);
    instance.transform[2][..2].copy_from_slice(&[at.x, at.y]);
    instance.uv_rect = uv;
    instance.tint = self.tint;
    let half = (x.abs() + y.abs()) / 2.0;
    (instance, at - half, at + half)
  }

  /// Whether it draws anything: it has some alpha, and is somewhere
  fn shows(&self) -> bool {
    let alpha = self.tint[3];
    !alpha.is_nan() && alpha > 0.0 && self.quad.is_finite()
  }
}

/// A stretch of a skeleton's quads of one image and blend, as it is added
struct OpenRun<'i> {
  image: AssetId<Image>,
  blend: BlendMode,
  /// What the image draws: itself, or the checker in its place
  shown: ShownImage<'i>,
  /// Where its instances start in the list's run instances
  start: usize,
  /// The world box its quads stay inside
  min: Vec2,
  max: Vec2,
}

impl<'a> Emitter<'a> {
  /// Draw a skeleton's `quads`, in order, as entity items at `z`: each
  /// stretch of quads that share an image and a blend is one run, so a
  /// skeleton drawn from one atlas is one item. A run no camera on the
  /// entity's layers can see is not pushed (and its image never uploaded);
  /// an image that draws nothing (not loaded, in a release build) leaves
  /// its quads out, as does a quad that is not finite or has no alpha.
  ///
  /// With a game shader each quad is a sprite item of its own instead, as
  /// an entity's sprite is: a run is shaded plain or by a built-in effect.
  fn draw_runs(&mut self, z: i32, quads: impl Iterator<Item = RunQuad>, look: RunLook<'_>) {
    if let Some(custom) = look.custom {
      for quad in quads {
        self.draw_shaded(z, &quad, custom, look.emission);
      }
      return;
    }
    let effect = look
      .effect
      .map(|effect| SpriteInstance::new().with_effect(effect));
    let shading = match effect {
      Some(_) => SpriteShading::Effect,
      None => SpriteShading::Plain,
    };
    let mut open: Option<OpenRun<'a>> = None;
    for quad in quads {
      if !quad.shows() {
        continue;
      }
      if open
        .as_ref()
        .is_some_and(|run| run.image != quad.image || run.blend != quad.blend)
        && let Some(run) = open.take()
      {
        self.push_run(z, run, shading);
      }
      let run = match &mut open {
        Some(run) => run,
        None => {
          let Some(shown) = self.gpu.shown_image(quad.image, self.images) else {
            continue;
          };
          open.insert(OpenRun {
            image: quad.image,
            blend: quad.blend,
            shown,
            start: self.list.run_instances().len(),
            min: Vec2::INFINITY,
            max: Vec2::NEG_INFINITY,
          })
        }
      };
      let (mut instance, min, max) = quad.instance(&run.shown, look.emission);
      if let Some(effect) = &effect {
        instance.effect_params = effect.effect_params;
        instance.effect_extra = effect.effect_extra;
      }
      run.min = run.min.min(min);
      run.max = run.max.max(max);
      self.list.run_instances().push(instance);
    }
    if let Some(run) = open {
      self.push_run(z, run, shading);
    }
  }

  /// Draw one quad of a skeleton through a game shader, as an entity's
  /// sprite is drawn through one: a sprite item, its parameters packed in
  /// the instance, plain while the shader has failed or is not compiled
  fn draw_shaded(
    &mut self,
    z: i32,
    quad: &RunQuad,
    (shader, params): (AssetId<Shader>, &ShaderParams),
    emission: f32,
  ) {
    if !quad.shows() {
      return;
    }
    let Some(shown) = self.gpu.shown_image(quad.image, self.images) else {
      return;
    };
    let (mut instance, min, max) = quad.instance(&shown, emission);
    let seen = self
      .cameras
      .iter()
      .any(|camera| camera.layers.intersects(&self.layers) && camera.view.is_visible(min, max));
    if !seen {
      return;
    }
    let Some(texture) = self.gpu.upload_entity_image(self.device, self.queue, shown) else {
      return;
    };
    let shading = match self
      .gpu
      .shade_custom(shader, self.shaders, params, &mut instance)
    {
      Some(key) => SpriteShading::Custom(key),
      None => SpriteShading::Plain,
    };
    let look = SpriteLook {
      shading,
      blend: quad.blend,
      filter: self.filter,
      lit: self.lit,
    };
    self.list.push_sprite_in(
      DrawKind::Entity,
      z,
      texture,
      look,
      instance,
      (self.layers, None, None),
    );
  }

  /// Push `run`, whose instances are the list's run instances from its
  /// start on, if a camera on the entity's layers can see it
  fn push_run(&mut self, z: i32, run: OpenRun<'_>, shading: SpriteShading) {
    let end = self.list.run_instances().len();
    let seen = self.cameras.iter().any(|camera| {
      camera.layers.intersects(&self.layers) && camera.view.is_visible(run.min, run.max)
    });
    let (Ok(first), Ok(last), true) = (u32::try_from(run.start), u32::try_from(end), seen) else {
      self.list.drop_run_instances(run.start);
      return;
    };
    if first == last {
      return;
    }
    let texture = self.gpu.shown_texture(run.shown);
    let item = DrawRun {
      texture,
      shading,
      blend: run.blend,
      filter: self.filter,
      layers: self.layers,
      min: run.min,
      max: run.max,
      instances: first..last,
    };
    self.list.push_run_as(DrawKind::Entity, self.lit, z, item);
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
    lift,
    ..
  } = *drawable;
  let z = renderable.z_order;
  if renderable.is_skeleton() {
    collect_skeleton(frame, drawable, out);
    return;
  }

  // Between the last two fixed steps for interpolated entities, lifted by
  // its floor's height; its depth scale goes by where it stands
  let drawn = render_global(global, previous, frame.alpha);
  let ground = drawn.translation();
  let position = ground + lift;
  let rotation = drawn.rotation();
  let mut scale = drawn.scale();
  if let Some(y_sort) = y_sort {
    let (depth_x, depth_y) = y_sort.calculate_depth_scale(ground.y);
    scale *= Vec2::new(depth_x, depth_y);
  }
  // A mirrored transform mirrors a single sprite. Its turn is read from
  // its x axis, so the mirror is across that axis: the anchor's offset and
  // the image flip top to bottom. With the renderable's own flip of that
  // axis that makes one flip or none
  let mirrored = drawn.is_mirrored();
  let mirrored_flip_y = renderable.flip_y != mirrored;
  let (pivot_x, pivot_y) = renderable.anchor.to_pivot();
  let alpha = renderable.alpha;
  let tint = renderable.tint;
  let mut source = match renderable.target {
    // A missing target (removed, or not made yet) draws nothing
    Some(target) => match out.target_source(target) {
      Some(source) => source,
      None => return,
    },
    None => match out.image_source(renderable.image.id()) {
      Some(source) => source,
      None => return,
    },
  };
  let texels = Vec2::new(source.width as f32, source.height as f32);

  match &renderable.style {
    RenderStyle::Single => {
      // The anchor point sits on the position; the sprite turns about it
      let size = texels * scale;
      let mut pivot_to_center = Vec2::new(0.5 - pivot_x, 0.5 - pivot_y) * size;
      if mirrored {
        pivot_to_center.y = -pivot_to_center.y;
      }
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
            .flipped(renderable.flip_x, mirrored_flip_y),
          Shading::tinted(shadow_tint, shadow_alpha),
        );
      }

      ground_shadows(renderable, position, out);

      // A Material component wins over the inline shader
      let shader = effective_shader(material, renderable.shader.as_ref());
      out.draw(
        z,
        &mut source,
        Placement::centered(center, scale)
          .rotated(rotation)
          .flipped(renderable.flip_x, mirrored_flip_y),
        Shading::shaded(tint, alpha, shader)
          .blended(renderable.blend)
          .emitting(renderable.emission),
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
          Shading::tinted(tint, alpha)
            .blended(renderable.blend)
            .emitting(renderable.emission),
        );
      }
    }

    // Drawn above, before an image is looked for
    RenderStyle::Skeleton => {}
  }
}

/// An entity's blob and contact shadows: ellipses under `position`, where
/// the entity draws (not its sprite's turned corner).
fn ground_shadows(renderable: &Renderable, position: Vec2, out: &mut Emitter<'_>) {
  let (z, alpha) = (renderable.z_order, renderable.alpha);
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
}

/// A skeleton entity: its regions' drop shadows, its blob and contact
/// shadows, then its regions back to front in the skeleton's draw order,
/// all at the entity's place among the entities' draws.
///
/// The skeleton is where its entity draws, as a sprite is
/// ([`Skeleton::drawn`]): interpolated, lifted on a floor, scaled with its
/// depth, and mirrored about the entity's origin by the renderable's
/// flips. Each region's colour is its slot's times its attachment's times
/// the renderable's tint, and its alpha those three's times the
/// renderable's; its blend is its slot's, a slot that blends normally
/// taking the renderable's. The renderable's emission and sprite shader
/// (a built-in effect or a game's own, its `Material`'s first) apply to
/// each region on its own: a flash tints every part, an outline outlines
/// each part.
///
/// The drop shadow is each region's own silhouette, moved by the shadow's
/// offset and scaled about where the skeleton stands in its entity's
/// turned axes (a sprite's is scaled from its box's corner; a skeleton has
/// no box, and its pose's bounds move with every step). Every shadow is
/// under every region. The blob and contact shadows are a sprite's: under
/// where the entity draws.
fn collect_skeleton(frame: &EntityFrame, drawable: &Drawable<'_>, out: &mut Emitter<'_>) {
  let Drawable {
    renderable,
    global,
    previous,
    material,
    y_sort,
    lift,
    skeleton,
    ..
  } = *drawable;
  // Nothing without a skeleton, or until its data lands
  let (Some(skeleton), Some(rigs)) = (skeleton, out.rigs) else {
    return;
  };
  let id = skeleton.data().id();
  let (Some(data), Some(version)) = (rigs.get(id), rigs.version(id)) else {
    return;
  };
  // Nor with a pose made for another version of its data than this: it
  // has no body to draw, so no shadow of one either
  if skeleton.seen() != Some((id, version)) {
    return;
  }
  let (z, alpha) = (renderable.z_order, renderable.alpha);
  let carrier = Carrier {
    global,
    previous,
    lift,
    y_sort,
  };
  let drawn = skeleton.drawn(&carrier, frame.alpha, out.skeleton_step);
  let mut quads = std::mem::take(out.quads);
  quads.clear();
  skeleton.regions(data, (id, version), drawn.root, drawn.between, &mut quads);

  if let Some(shadow) = &renderable.drop_shadow {
    let color = shadow.color;
    let strength = (color.a as f32 / 255.0) * alpha;
    let [r, g, b] = [color.r, color.g, color.b].map(|c| c as f32 / 255.0);
    // Scaled in the entity's turned axes about where it draws, then moved
    let cast = if shadow.scale == Vec2::ONE {
      Affine2::from_translation(shadow.offset)
    } else {
      Affine2::from_translation(drawn.position + shadow.offset)
        * Affine2::from_angle(drawn.rotation)
        * Affine2::from_scale(shadow.scale)
        * Affine2::from_angle(-drawn.rotation)
        * Affine2::from_translation(-drawn.position)
    };
    let shadows = quads.iter().map(|quad| RunQuad {
      image: quad.image,
      rect: quad.rect,
      // Shadows blend over what is below whatever their region's blend
      blend: BlendMode::Alpha,
      quad: cast * quad.quad,
      // A region faded out by its slot or attachment casts less
      tint: [r, g, b, strength * quad.color[3]],
    });
    let plain = RunLook {
      effect: None,
      custom: None,
      emission: 1.0,
    };
    out.draw_runs(z, shadows, plain);
  }

  ground_shadows(renderable, drawn.position, out);

  // A Material component wins over the inline shader. Either kind is each
  // region's own: a built-in effect on every quad of a run, a game shader
  // on a sprite item a quad
  let (effect, custom) = match effective_shader(material, renderable.shader.as_ref()) {
    Some(SpriteShader::BuiltIn(effect)) => (Some(effect), None),
    Some(SpriteShader::Custom { shader, params }) => (None, Some((shader.id(), params))),
    None => (None, None),
  };
  let tint = renderable.tint;
  let tint = [tint.r, tint.g, tint.b, tint.a].map(|c| c as f32 / 255.0);
  let regions = quads.iter().map(|quad| RunQuad {
    image: quad.image,
    rect: quad.rect,
    blend: match quad.blend {
      SlotBlend::Normal => renderable.blend,
      SlotBlend::Additive => BlendMode::Additive,
      SlotBlend::Multiply => BlendMode::Multiply,
      SlotBlend::Screen => BlendMode::Screen,
    },
    quad: quad.quad,
    tint: [
      quad.color[0] * tint[0],
      quad.color[1] * tint[1],
      quad.color[2] * tint[2],
      quad.color[3] * tint[3] * alpha,
    ],
  });
  let look = RunLook {
    effect,
    custom,
    emission: renderable.emission,
  };
  out.draw_runs(z, regions, look);
  *out.quads = quads;
}

/// A multi-part tile: static parts once, tiled parts repeated, each part at
/// the composite's `z_order`, laid out from the entity's position (lifted
/// by `lift`, on a floor).
fn collect_composite(
  frame: &EntityFrame,
  global: &GlobalTransform,
  previous: Option<&PreviousGlobalTransform>,
  lift: Vec2,
  composite: &CompositeTile,
  out: &mut Emitter<'_>,
) {
  if !composite.visible {
    return;
  }
  // Blended for interpolated entities, like single sprites
  let global = &render_global(global, previous, frame.alpha);

  let z = composite.z_order;
  let origin = global.translation() + lift;
  let scale = global.scale();

  for part in &composite.parts {
    let Some(mut source) = out.image_source(part.sprite.id()) else {
      continue;
    };
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
/// its sprite (0 top, 1 bottom), plus `sort_offset`; for a skeleton, its
/// entity's own y plus `sort_offset`, however it is posed or flipped.
/// `showing` gives the size of an image whose texture shows an older
/// version.
fn y_sort_key(
  y_sort: &YSort,
  renderable: &Renderable,
  y: f32,
  (images, showing): (&Assets<Image>, &[Showing]),
) -> f32 {
  // A skeleton has no image to measure down: it sorts where it stands, its
  // origin, where a rig's feet are
  if renderable.is_skeleton() {
    return y_sort.calculate_sort_y(y, 0.0);
  }
  // The height it draws at: an older version's while a new one is written
  let id = renderable.image.id();
  let height = match showing.iter().find(|showing| showing.image == id) {
    Some(showing) => showing.size.1 as f32,
    None => images.get(id).map_or(0.0, |image| image.height() as f32),
  };
  // `y` is the anchor point; `calculate_sort_y` measures from the top
  let (_, pivot_y) = renderable.anchor.to_pivot();
  y_sort.calculate_sort_y(y - pivot_y * height, height)
}

#[cfg(test)]
mod draw_order_tests {
  use super::{DrawOrder, DrawableItem, draws_before, in_draw_order, y_sort_key};
  use crate::core::asset::Assets;
  use crate::core::ecs::test_util::{MAX_UNRELATED, Other, insert_unrelated};
  use crate::core::ecs::{Anchor, Entity, GlobalTransform, Renderable, Vec2, World, YSort};
  use crate::core::graphics::Image;
  use crate::core::graphics::atlas::Showing;

  /// Catches ties left in query order (`in_draw_order` breaks them by
  /// `entity_order`): a query visits archetypes in an order that changes
  /// when an unrelated resource is added.
  #[test]
  fn tied_sprites_draw_in_entity_order_whatever_the_resources() {
    let mut images = Assets::<Image>::new();
    let image = images.add(Image::solid_rect(4, 4, 255, 255, 255, 255));
    for k in 0..=MAX_UNRELATED {
      let mut world = World::new();
      insert_unrelated(&mut world, k);
      // One layer, every y-sorted sprite at one sort y, in four archetypes
      let (mut plain, mut sorted) = (Vec::new(), Vec::new());
      for i in 0..16 {
        let mut entity = world.spawn((Renderable::image(image.clone()), GlobalTransform::IDENTITY));
        if i % 4 >= 2 {
          entity.insert(Other);
        }
        if i % 2 == 1 {
          entity.insert(YSort::new());
          sorted.push(entity.id());
        } else {
          plain.push(entity.id());
        }
      }
      let mut drawables = world.query::<DrawableItem>();
      let mut items = Vec::new();
      in_draw_order(&mut drawables, &world, (&images, &[]), &mut items);
      let drawn: Vec<Entity> = items.iter().map(|(_, drawable)| drawable.entity).collect();
      let expected: Vec<Entity> = plain.into_iter().chain(sorted).collect();
      assert_eq!(drawn, expected, "with {k} unrelated resources");
    }
  }

  #[test]
  fn the_sort_key_is_the_feet_wherever_the_anchor_is() {
    let mut images = Assets::<Image>::new();
    let tall = images.add(Image::solid_rect(10, 20, 255, 255, 255, 255));
    let sprite_20_high = |anchor| Renderable::image(tall.clone()).with_anchor(anchor);
    let y_sort_key = |y_sort: &YSort, renderable: &Renderable, y| {
      y_sort_key(y_sort, renderable, y, (&images, &[]))
    };
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
  fn a_mirrored_sprite_and_a_skeleton_sort_where_they_stand() {
    use crate::core::ecs::Transform;
    let mut images = Assets::<Image>::new();
    let tall = images.add(Image::solid_rect(10, 20, 255, 255, 255, 255));
    let mut world = World::new();
    // Three y-sorted sprites standing on y 100 by their feet: as it is,
    // mirrored left to right (a character turned round), and mirrored top
    // to bottom; and a skeleton standing there
    let at = |scale: Vec2| {
      GlobalTransform::from_transform(&Transform::from_xy(0.0, 100.0).with_scale(scale))
    };
    let sprite = || Renderable::image(tall.clone()).with_anchor(Anchor::BottomCenter);
    for scale in [Vec2::ONE, Vec2::new(-1.0, 1.0), Vec2::new(1.0, -1.0)] {
      world.spawn((sprite(), at(scale), YSort::new()));
    }
    world.spawn((
      Renderable::skeleton(),
      at(Vec2::new(-1.0, 1.0)),
      YSort::new(),
    ));
    let mut drawables = world.query::<DrawableItem>();
    let mut items = Vec::new();
    in_draw_order(&mut drawables, &world, (&images, &[]), &mut items);
    // A mirror moves no sort key: as the key takes no account of a
    // sprite's turn, it takes none of its mirroring
    let keys: Vec<Option<f32>> = items.iter().map(|(order, _)| order.sort_y).collect();
    assert_eq!(keys, [Some(100.0); 4]);
  }

  #[test]
  fn an_image_being_replaced_sorts_by_the_height_it_draws_at() {
    let mut images = Assets::<Image>::new();
    let tall = images.add(Image::solid_rect(10, 20, 255, 255, 255, 255));
    // Its texture still shows an older version, 40 px high
    let showing = [Showing {
      key: crate::core::graphics::TextureId::for_test(0),
      image: tall.id(),
      version: 1,
      size: (10, 40),
    }];
    let sprite = Renderable::image(tall.clone()).with_anchor(Anchor::TopLeft);
    let key = y_sort_key(&YSort::new(), &sprite, 100.0, (&images, &showing));
    assert_eq!(
      key, 140.0,
      "the feet of the 40 px it draws, not the 20 px it is"
    );
  }

  #[test]
  fn layers_then_plain_then_sort_y() {
    let mut orders = [
      DrawOrder::y_sorted(0, 50.0, 0),
      DrawOrder::plain(1, 0),
      DrawOrder::y_sorted(0, -20.0, 0),
      DrawOrder::plain(0, 0),
      DrawOrder::y_sorted(i32::MAX, 1e9, 0),
      DrawOrder::plain(i32::MIN, 0),
    ];
    orders.sort_by(|a, b| a.cmp(b));
    assert_eq!(
      orders,
      [
        DrawOrder::plain(i32::MIN, 0),
        DrawOrder::plain(0, 0),
        DrawOrder::y_sorted(0, -20.0, 0),
        DrawOrder::y_sorted(0, 50.0, 0),
        DrawOrder::plain(1, 0),
        DrawOrder::y_sorted(i32::MAX, 1e9, 0),
      ]
    );
  }

  #[test]
  fn equal_sort_y_draws_the_lower_floor_first() {
    let mut orders = [
      DrawOrder::y_sorted(0, 10.0, 1),
      DrawOrder::y_sorted(0, 10.0, 0),
      DrawOrder::y_sorted(0, 5.0, 2),
      DrawOrder::plain(0, 1),
      DrawOrder::plain(0, 0),
      DrawOrder::y_sorted(0, 10.0, 255),
    ];
    orders.sort_by(|a, b| a.cmp(b));
    // The sort y first: a higher floor further up draws before a lower one
    // further down; at one sort y the lower floor first
    assert_eq!(
      orders,
      [
        DrawOrder::plain(0, 0),
        DrawOrder::plain(0, 1),
        DrawOrder::y_sorted(0, 5.0, 2),
        DrawOrder::y_sorted(0, 10.0, 0),
        DrawOrder::y_sorted(0, 10.0, 1),
        DrawOrder::y_sorted(0, 10.0, 255),
      ]
    );
    // The floor goes before the entities' order: a row of an upper floor
    // tied on an older entity still draws after a newer one on the ground
    let mut world = World::new();
    let (older, newer) = (world.spawn_empty().id(), world.spawn_empty().id());
    let ground = (DrawOrder::y_sorted(0, 10.0, 0), newer);
    let upper = (DrawOrder::y_sorted(0, 10.0, 1), older);
    assert!(draws_before(ground, upper));
    assert!(!draws_before(upper, ground));
  }
}
