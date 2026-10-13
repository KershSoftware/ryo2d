//! The frame's drawables in draw order
//!
//! Sprites and trail/line triangles are pushed in any order; the world
//! passes draw them sorted by z, then by [`DrawKind`], then by submission, so
//! a trail can sit between two sprites in one render pass.

use super::CameraDraw;
use super::batch::{BlendMode, Filter};
use crate::core::ecs::Vec2;
use std::ops::Range;

use crate::core::graphics::{RenderLayers, RenderTarget, SpriteInstance, TextureId};

/// What a drawable is, which breaks ties between equal z (in declaration order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DrawKind {
  /// An entity (with its shadows, tiles and composite parts) drawn on the GPU
  Entity,
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
}

/// One drawable and where it sorts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DrawItem {
  pub(crate) z: i32,
  pub(crate) kind: DrawKind,
  /// Submission order across the whole list
  pub(crate) sequence: u32,
  pub(crate) payload: DrawPayload,
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
    self.push_sprite_in(kind, z, texture, look, instance, (layers, None));
  }

  /// Add a sprite drawn as `look` says, for the cameras on `layers`, from
  /// an entity that shows the render target `shows`.
  pub(crate) fn push_sprite_in(
    &mut self,
    kind: DrawKind,
    z: i32,
    texture: TextureId,
    look: SpriteLook,
    instance: SpriteInstance,
    (layers, shows): (RenderLayers, Option<RenderTarget>),
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
    });
    self.push_item(look.lit, z, kind, payload);
  }

  /// Add a trail or line at `z`; `lit` ones draw before lighting.
  pub(crate) fn push_triangles(&mut self, lit: bool, z: i32, triangles: DrawTriangles) {
    let payload = DrawPayload::Triangles(self.triangles.len());
    self.triangles.push(triangles);
    self.push_item(lit, z, DrawKind::Trail, payload);
  }

  fn push_item(&mut self, lit: bool, z: i32, kind: DrawKind, payload: DrawPayload) {
    let sequence = self.next_sequence;
    self.next_sequence = self.next_sequence.wrapping_add(1);
    let item = DrawItem {
      z,
      kind,
      sequence,
      payload,
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

  fn sees(&self, camera: &CameraDraw, item: &DrawItem) -> bool {
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
    };
    list.push_triangles(lit, z, triangles);
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
        DrawPayload::Triangles(_) => None,
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
}
