//! The frame's drawables in draw order
//!
//! Sprites and trail/line triangles are pushed in any order; the world
//! passes draw them sorted by z, then by [`DrawKind`], then by submission, so
//! a trail can sit between two sprites in one render pass. Lights are kept
//! apart, for the lighting pass.

use super::batch::{BatchShader, BlendMode, Filter, Sampling};
use super::{CameraDraw, PixelRect};
use crate::core::ecs::Entity;
use crate::core::ecs::Vec2;
use std::ops::Range;

use crate::core::graphics::{GlyphPage, RenderLayers, RenderTarget, SpriteInstance, TextureId};

/// What a drawable is, which breaks ties between equal z (in declaration order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DrawKind {
  /// An entity (with its shadows, tiles and composite parts) drawn on the GPU
  Entity,
  /// Text and UI shapes: after the entities at equal z
  Text,
  /// A `Scene::render`, particle or parallax sprite
  Sprite,
  /// A trail or line
  Trail,
}

/// What a [`DrawItem`] draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrawPayload {
  /// Index into the list's sprite instances (atlas page and effects flag are
  /// resolved at draw time)
  Sprite(usize),
  /// Index into the list's triangle items (trails and lines)
  Triangles(usize),
  /// Index into the list's glyph items
  Glyph(usize),
  /// Index into the list's shape items
  #[allow(dead_code)] // drawn by the UI in a later task
  Shape(usize),
}

/// One drawable and where it sorts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DrawItem {
  pub(crate) z: i32,
  pub(crate) kind: DrawKind,
  /// Submission order across the whole list
  pub(crate) sequence: u32,
  pub(crate) payload: DrawPayload,
  /// Drawn by this camera alone (debug shapes built for one camera)
  pub(crate) only_camera: Option<Entity>,
}

/// How a sprite is shaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpriteShading {
  /// Texture times tint
  Plain,
  /// A built-in effect, already in the instance's effect parameters
  Effect,
  /// The game shader with this source key (from `shade_custom`); its
  /// parameters are already packed in the instance's effect parameters
  Custom(u64),
}

/// How a sprite is drawn, besides its texture and instance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SpriteLook {
  pub(crate) shading: SpriteShading,
  pub(crate) blend: BlendMode,
  pub(crate) filter: Filter,
  /// Drawn before lighting; otherwise after it, at full brightness
  pub(crate) lit: bool,
}

impl SpriteLook {
  /// `shading`, blended over what is below, with the presentation's filter,
  /// lit.
  #[cfg(test)]
  pub(crate) fn plain(shading: SpriteShading) -> Self {
    Self {
      shading,
      blend: BlendMode::default(),
      filter: Filter::default(),
      lit: true,
    }
  }
}

/// A sprite waiting to be drawn.
#[derive(Debug, Clone)]
pub(crate) struct DrawSprite {
  pub(crate) texture: TextureId,
  pub(crate) shading: SpriteShading,
  pub(crate) blend: BlendMode,
  pub(crate) filter: Filter,
  pub(crate) instance: SpriteInstance,
  /// The cameras on these layers draw it
  pub(crate) layers: RenderLayers,
  /// The render target it shows (or its entity does): never drawn by the
  /// camera drawing into that target
  pub(crate) shows: Option<RenderTarget>,
  /// The world box its quad stays inside at any rotation
  pub(crate) min: Vec2,
  pub(crate) max: Vec2,
  /// Pixels outside are not drawn
  pub(crate) scissor: Option<PixelRect>,
}

/// A trail or line waiting to be drawn: its triangles in the batch's
/// triangle stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DrawTriangles {
  /// The texture along it (textured trails)
  pub(crate) texture: Option<TextureId>,
  pub(crate) blend: BlendMode,
  pub(crate) filter: Filter,
  /// The cameras on these layers draw it
  pub(crate) layers: RenderLayers,
  /// The world box it stays inside
  pub(crate) min: Vec2,
  pub(crate) max: Vec2,
  pub(crate) indices: Range<u32>,
  /// Its vertices (a textured one is told its atlas slot when placed)
  pub(crate) vertices: Range<u32>,
  /// Pixels outside are not drawn
  pub(crate) scissor: Option<PixelRect>,
}

/// A glyph waiting to be drawn: its quad is already placed in its glyph
/// page, so the instance carries the atlas UVs.
#[derive(Debug, Clone)]
pub(crate) struct DrawGlyph {
  pub(crate) page: GlyphPage,
  pub(crate) instance: SpriteInstance,
  /// One of the text shaders
  pub(crate) shader: BatchShader,
  pub(crate) blend: BlendMode,
  pub(crate) sampling: Sampling,
  /// The cameras on these layers draw it
  pub(crate) layers: RenderLayers,
  /// The box its quad stays inside
  pub(crate) min: Vec2,
  pub(crate) max: Vec2,
  /// Pixels outside are not drawn
  pub(crate) scissor: Option<PixelRect>,
}

/// An untextured UI rectangle (rounded corners, border) waiting to be drawn.
#[derive(Debug, Clone)]
pub(crate) struct DrawShape {
  pub(crate) instance: SpriteInstance,
  pub(crate) blend: BlendMode,
  /// The cameras on these layers draw it
  pub(crate) layers: RenderLayers,
  /// The box its quad stays inside
  pub(crate) min: Vec2,
  pub(crate) max: Vec2,
  /// Pixels outside are not drawn
  pub(crate) scissor: Option<PixelRect>,
}

/// A light waiting to be drawn into the light buffers of the lit cameras
/// that see it.
#[derive(Debug, Clone)]
pub(crate) struct DrawLight {
  pub(crate) instance: SpriteInstance,
  /// The cameras on these layers draw it
  pub(crate) layers: RenderLayers,
  /// The world box it lights
  pub(crate) min: Vec2,
  pub(crate) max: Vec2,
  /// Its shadow volumes in the batch's triangle indices (empty: none)
  pub(crate) shadows: std::ops::Range<u32>,
  /// Its shadow is coloured: the light shows through it, tinted
  pub(crate) coloured: bool,
}

/// The frame's sprites, trails and lines, reused across frames.
///
/// Lit items are drawn before lighting; unlit ones (anything drawn
/// `with_lighting(false)`) after it.
#[derive(Debug, Default)]
pub(crate) struct DrawList {
  items: Vec<DrawItem>,
  unlit_items: Vec<DrawItem>,
  sprites: Vec<DrawSprite>,
  triangles: Vec<DrawTriangles>,
  glyphs: Vec<DrawGlyph>,
  shapes: Vec<DrawShape>,
  lights: Vec<DrawLight>,
  next_sequence: u32,
  sorted: bool,
}

impl DrawList {
  /// Forget the frame's items, keeping the allocations.
  pub(crate) fn clear(&mut self) {
    self.items.clear();
    self.unlit_items.clear();
    self.sprites.clear();
    self.triangles.clear();
    self.glyphs.clear();
    self.shapes.clear();
    self.lights.clear();
    self.next_sequence = 0;
    self.sorted = true;
  }

  /// Add a plain, lit sprite seen by every camera (tests).
  #[cfg(test)]
  pub(crate) fn push_sprite(
    &mut self,
    kind: DrawKind,
    z: i32,
    texture: TextureId,
    shading: SpriteShading,
    instance: SpriteInstance,
  ) {
    let look = SpriteLook::plain(shading);
    let layers = RenderLayers::default();
    self.push_sprite_in(kind, z, texture, look, instance, (layers, None, None));
  }

  /// Add a sprite drawn as `look` says, for the cameras on `layers` (or
  /// `only_camera` alone), from an entity that shows the render target
  /// `shows`.
  pub(crate) fn push_sprite_in(
    &mut self,
    kind: DrawKind,
    z: i32,
    texture: TextureId,
    look: SpriteLook,
    instance: SpriteInstance,
    (layers, shows, only_camera): (RenderLayers, Option<RenderTarget>, Option<Entity>),
  ) {
    let payload = DrawPayload::Sprite(self.sprites.len());
    let (min, max) = reach(&instance);
    self.sprites.push(DrawSprite {
      texture,
      shading: look.shading,
      blend: look.blend,
      filter: look.filter,
      instance,
      layers,
      shows,
      min,
      max,
      scissor: None,
    });
    self.push_item(look.lit, z, kind, payload, only_camera);
  }

  /// Clip the most recently pushed sprite to `scissor` (a no-op before any
  /// sprite).
  pub(crate) fn set_last_sprite_scissor(&mut self, scissor: Option<PixelRect>) {
    if let Some(sprite) = self.sprites.last_mut() {
      sprite.scissor = scissor;
    }
  }

  /// Add a trail or line at `z`, for `only_camera` or every camera; `lit`
  /// ones draw before lighting.
  pub(crate) fn push_triangles(
    &mut self,
    lit: bool,
    z: i32,
    only_camera: Option<Entity>,
    triangles: DrawTriangles,
  ) {
    let payload = DrawPayload::Triangles(self.triangles.len());
    self.triangles.push(triangles);
    self.push_item(lit, z, DrawKind::Trail, payload, only_camera);
  }

  /// Add a glyph at `z`, for `only_camera` or every camera.
  pub(crate) fn push_glyph(
    &mut self,
    kind: DrawKind,
    z: i32,
    lit: bool,
    only_camera: Option<Entity>,
    glyph: DrawGlyph,
  ) {
    let payload = DrawPayload::Glyph(self.glyphs.len());
    self.glyphs.push(glyph);
    self.push_item(lit, z, kind, payload, only_camera);
  }

  #[allow(dead_code)] // drawn by the UI in a later task
  /// Add a UI rectangle at `z`, for `only_camera` or every camera.
  pub(crate) fn push_shape(
    &mut self,
    kind: DrawKind,
    z: i32,
    lit: bool,
    only_camera: Option<Entity>,
    shape: DrawShape,
  ) {
    let payload = DrawPayload::Shape(self.shapes.len());
    self.shapes.push(shape);
    self.push_item(lit, z, kind, payload, only_camera);
  }

  /// Add a light for the lit cameras that see it.
  pub(crate) fn push_light(&mut self, light: DrawLight) {
    self.lights.push(light);
  }

  fn push_item(
    &mut self,
    lit: bool,
    z: i32,
    kind: DrawKind,
    payload: DrawPayload,
    only_camera: Option<Entity>,
  ) {
    let sequence = self.next_sequence;
    self.next_sequence = self.next_sequence.wrapping_add(1);
    let item = DrawItem {
      z,
      kind,
      sequence,
      payload,
      only_camera,
    };
    if lit {
      self.items.push(item);
    } else {
      self.unlit_items.push(item);
    }
    self.sorted = false;
  }

  /// The lit items sorted by (z, kind, sequence); stable.
  ///
  /// Sorts the unlit items too, so [`items`](Self::items) and
  /// [`unlit_items`](Self::unlit_items) are in draw order afterwards.
  pub(crate) fn sorted(&mut self) -> &[DrawItem] {
    if !self.sorted {
      let key = |item: &DrawItem| (item.z, item.kind, item.sequence);
      self.items.sort_by_key(key);
      self.unlit_items.sort_by_key(key);
      self.sorted = true;
    }
    &self.items
  }

  /// The lit items, in draw order once [`sorted`](Self::sorted) has run.
  pub(crate) fn items(&self) -> &[DrawItem] {
    &self.items
  }

  /// The unlit items, in draw order once [`sorted`](Self::sorted) has run.
  #[cfg(test)]
  pub(crate) fn unlit_items(&self) -> &[DrawItem] {
    &self.unlit_items
  }

  /// The sprite a [`DrawPayload::Sprite`] index refers to.
  pub(crate) fn sprite(&self, index: usize) -> Option<&DrawSprite> {
    self.sprites.get(index)
  }

  /// How many sprites the frame holds.
  pub(crate) fn sprite_count(&self) -> usize {
    self.sprites.len()
  }

  /// The trail or line a [`DrawPayload::Triangles`] index refers to.
  pub(crate) fn triangles(&self, index: usize) -> Option<&DrawTriangles> {
    self.triangles.get(index)
  }

  /// The glyph a [`DrawPayload::Glyph`] index refers to.
  pub(crate) fn glyph(&self, index: usize) -> Option<&DrawGlyph> {
    self.glyphs.get(index)
  }

  /// The rectangle a [`DrawPayload::Shape`] index refers to.
  pub(crate) fn shape(&self, index: usize) -> Option<&DrawShape> {
    self.shapes.get(index)
  }

  /// The textures the frame's trails and lines draw.
  pub(crate) fn triangle_textures(&self) -> impl Iterator<Item = TextureId> + '_ {
    self
      .triangles
      .iter()
      .filter_map(|triangles| triangles.texture)
  }

  /// The lit items `camera` draws, in order: those on its layers, and of
  /// the sprites only those inside its view that do not show its target.
  pub(crate) fn items_for<'s>(
    &'s self,
    camera: &'s CameraDraw,
  ) -> impl Iterator<Item = &'s DrawItem> + 's {
    self.items.iter().filter(|item| self.sees(camera, item))
  }

  /// The unlit items `camera` draws, in order.
  pub(crate) fn unlit_items_for<'s>(
    &'s self,
    camera: &'s CameraDraw,
  ) -> impl Iterator<Item = &'s DrawItem> + 's {
    self
      .unlit_items
      .iter()
      .filter(|item| self.sees(camera, item))
  }

  /// The lights `camera` draws: those on its layers whose box meets its view.
  pub(crate) fn lights_for<'s>(
    &'s self,
    camera: &'s CameraDraw,
  ) -> impl Iterator<Item = &'s DrawLight> + 's {
    self.lights.iter().filter(|light| {
      light.layers.intersects(&camera.layers) && camera.view.is_visible(light.min, light.max)
    })
  }

  fn sees(&self, camera: &CameraDraw, item: &DrawItem) -> bool {
    if item.only_camera.is_some_and(|only| only != camera.entity) {
      return false;
    }
    // A UI camera draws what is drawn for it alone
    if camera.ui && item.only_camera != Some(camera.entity) {
      return false;
    }
    match item.payload {
      DrawPayload::Sprite(index) => self.sprites.get(index).is_some_and(|sprite| {
        sprite.layers.intersects(&camera.layers)
          && camera.view.is_visible(sprite.min, sprite.max)
          && (sprite.shows.is_none() || sprite.shows != camera.target)
      }),
      DrawPayload::Triangles(index) => self.triangles.get(index).is_some_and(|triangles| {
        triangles.layers.intersects(&camera.layers)
          && camera.view.is_visible(triangles.min, triangles.max)
      }),
      DrawPayload::Glyph(index) => self.glyphs.get(index).is_some_and(|glyph| {
        glyph.layers.intersects(&camera.layers) && camera.view.is_visible(glyph.min, glyph.max)
      }),
      DrawPayload::Shape(index) => self.shapes.get(index).is_some_and(|shape| {
        shape.layers.intersects(&camera.layers) && camera.view.is_visible(shape.min, shape.max)
      }),
    }
  }
}

/// The world box `instance`'s quad stays inside at any rotation: its centre
/// give or take its half-diagonal.
fn reach(instance: &SpriteInstance) -> (Vec2, Vec2) {
  let [x, y, at] = [
    instance.transform[0],
    instance.transform[1],
    instance.transform[2],
  ];
  let diagonal = Vec2::new(x[0], x[1]).length_squared() + Vec2::new(y[0], y[1]).length_squared();
  let half = Vec2::splat(diagonal.sqrt() / 2.0);
  let center = Vec2::new(at[0], at[1]);
  (center - half, center + half)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::text::GlyphFormat;

  fn push(list: &mut DrawList, kind: DrawKind, z: i32, texture: u32) {
    list.push_sprite(
      kind,
      z,
      TextureId::for_test(texture),
      SpriteShading::Plain,
      SpriteInstance::new(),
    );
  }

  /// A line seen by every camera.
  fn line(list: &mut DrawList, lit: bool, z: i32) {
    let triangles = DrawTriangles {
      texture: None,
      blend: BlendMode::Alpha,
      filter: Filter::Default,
      layers: RenderLayers::default(),
      min: Vec2::ZERO,
      max: Vec2::ONE,
      indices: 0..3,
      vertices: 0..3,
      scissor: None,
    };
    list.push_triangles(lit, z, None, triangles);
  }

  fn glyph_item(min: Vec2, max: Vec2) -> DrawGlyph {
    DrawGlyph {
      page: GlyphPage {
        format: GlyphFormat::Mask,
        index: 0,
      },
      instance: SpriteInstance::new(),
      shader: BatchShader::TextMask,
      blend: BlendMode::Alpha,
      sampling: Sampling::Linear,
      layers: RenderLayers::default(),
      min,
      max,
      scissor: None,
    }
  }

  /// A camera over (0,0) to (100,100), a UI camera if `ui`.
  fn test_camera(entity: Entity, ui: bool) -> CameraDraw {
    use crate::core::graphics::{Camera2d, CameraView};
    let camera = Camera2d::default();
    let view = CameraView::compute(
      Vec2::new(50.0, 50.0),
      0.0,
      &camera,
      Vec2::ZERO,
      Vec2::new(100.0, 100.0),
    );
    let (mut cameras, mut targetless) = (Vec::new(), Vec::new());
    crate::core::render::plan_cameras(
      [(entity, &camera, &view, None)].into_iter(),
      (100, 100),
      true,
      None,
      &mut cameras,
      &mut targetless,
    );
    CameraDraw { ui, ..cameras[0] }
  }

  #[test]
  fn text_sorts_after_entities_at_equal_z() {
    let mut list = DrawList::default();
    let (min, max) = (Vec2::ZERO, Vec2::ONE);
    list.push_glyph(DrawKind::Text, 0, true, None, glyph_item(min, max));
    push(&mut list, DrawKind::Entity, 0, 0);
    list.sorted();
    let kinds: Vec<DrawKind> = list.items().iter().map(|i| i.kind).collect();
    assert_eq!(kinds, vec![DrawKind::Entity, DrawKind::Text]);
  }

  #[test]
  fn a_ui_camera_sees_only_its_own_items() {
    use crate::core::ecs::World;
    let mut world = World::new();
    let (ui, other) = (world.spawn_empty().id(), world.spawn_empty().id());
    let world_camera = test_camera(other, false);
    let ui_camera = test_camera(ui, true);
    let mut list = DrawList::default();
    let (min, max) = (Vec2::new(40.0, 40.0), Vec2::new(60.0, 60.0));
    list.push_glyph(DrawKind::Text, 0, false, Some(ui), glyph_item(min, max));
    list.push_glyph(DrawKind::Text, 0, false, None, glyph_item(min, max));
    list.sorted();
    assert_eq!(list.unlit_items_for(&ui_camera).count(), 1);
    assert_eq!(list.unlit_items_for(&world_camera).count(), 1);
  }

  fn order(items: &[DrawItem]) -> Vec<(i32, DrawKind)> {
    items.iter().map(|item| (item.z, item.kind)).collect()
  }

  #[test]
  fn sorts_by_z_then_kind_then_sequence() {
    let mut list = DrawList::default();
    push(&mut list, DrawKind::Entity, 1, 0);
    line(&mut list, true, 0);
    push(&mut list, DrawKind::Sprite, 1, 1);
    push(&mut list, DrawKind::Entity, 1, 2);

    let sorted = list.sorted();

    assert_eq!(
      order(sorted),
      vec![
        (0, DrawKind::Trail),
        (1, DrawKind::Entity),
        (1, DrawKind::Entity),
        (1, DrawKind::Sprite),
      ]
    );
    // The two entities keep their submission order
    assert!(sorted[1].sequence < sorted[2].sequence);
  }

  #[test]
  fn stable_within_kind() {
    let mut list = DrawList::default();
    for texture in 0..4 {
      push(&mut list, DrawKind::Sprite, 3, texture);
    }

    list.sorted();
    let textures: Vec<_> = list
      .items()
      .iter()
      .filter_map(|item| match item.payload {
        DrawPayload::Sprite(i) => list.sprite(i).map(|s| s.texture),
        _ => None,
      })
      .collect();

    assert_eq!(
      textures,
      (0..4).map(TextureId::for_test).collect::<Vec<_>>()
    );
  }

  #[test]
  fn unlit_groups_sort_apart_from_lit_ones() {
    let mut list = DrawList::default();
    line(&mut list, false, 2);
    line(&mut list, true, 5);
    line(&mut list, false, -1);

    list.sorted();

    assert_eq!(order(list.items()), vec![(5, DrawKind::Trail)]);
    assert_eq!(
      order(list.unlit_items()),
      vec![(-1, DrawKind::Trail), (2, DrawKind::Trail)]
    );
  }

  #[test]
  fn clear_empties_the_list_for_reuse() {
    let mut list = DrawList::default();
    push(&mut list, DrawKind::Sprite, 0, 0);
    line(&mut list, false, 0);

    list.clear();

    assert!(list.items().is_empty() && list.unlit_items().is_empty());
    assert_eq!(list.sprite_count(), 0);
    assert!(list.triangles(0).is_none());
    push(&mut list, DrawKind::Sprite, 0, 1);
    assert_eq!(list.sorted()[0].sequence, 0);
  }

  #[test]
  fn a_camera_sees_the_lights_on_its_layers_in_its_view() {
    use crate::core::ecs::World;
    use crate::core::graphics::{Camera2d, CameraView, RenderLayers};
    let mut world = World::new();
    let entity = world.spawn_empty().id();
    let camera = Camera2d::default();
    let view = CameraView::compute(
      Vec2::new(50.0, 50.0),
      0.0,
      &camera,
      Vec2::ZERO,
      Vec2::new(100.0, 100.0),
    );
    let (mut cameras, mut targetless) = (Vec::new(), Vec::new());
    crate::core::render::plan_cameras(
      [(entity, &camera, &view, None)].into_iter(),
      (100, 100),
      true,
      None,
      &mut cameras,
      &mut targetless,
    );
    let light = |at: Vec2, layers: RenderLayers| DrawLight {
      instance: SpriteInstance::new(),
      layers,
      min: at - 5.0,
      max: at + 5.0,
      shadows: 0..0,
      coloured: false,
    };
    let mut list = DrawList::default();
    list.push_light(light(Vec2::new(50.0, 50.0), RenderLayers::default()));
    list.push_light(light(Vec2::new(500.0, 50.0), RenderLayers::default()));
    list.push_light(light(Vec2::new(50.0, 50.0), RenderLayers::layer(1)));
    assert_eq!(list.lights_for(&cameras[0]).count(), 1);
  }
}
