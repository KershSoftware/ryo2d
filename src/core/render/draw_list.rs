//! The frame's drawables in draw order
//!
//! Sprites and trail/line geometry groups are pushed in any order; the world
//! passes draw them sorted by z, then by [`DrawKind`], then by submission, so
//! a trail can sit between two sprites in one render pass.

use std::sync::Arc;

use crate::core::graphics::{SlotPage, SpriteInstance};

/// A texture as the sprite renderer resolves it: the id the texture was
/// uploaded under. Shared, so every sprite of a batch holds one allocation.
pub(crate) type TextureRef = Arc<str>;

/// What a drawable is, which breaks ties between equal z (in declaration order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DrawKind {
  /// An entity (with its shadows, tiles and composite parts) drawn on the GPU
  Entity,
  /// A `render_gpu`, particle or parallax sprite
  Sprite,
  /// A trail or line geometry group
  Trail,
}

/// What a [`DrawItem`] draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrawPayload {
  /// Index into the list's sprite instances (atlas page and effects flag are
  /// resolved at draw time)
  Sprite(usize),
  /// A trail/line geometry group
  Trail { z: i32 },
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

/// The pipeline a run of sprites is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PipelineKind {
  Plain,
  Effect,
  /// The custom pipeline of the shader source with this key
  Custom(u64),
}

/// A sprite waiting to be drawn.
#[derive(Debug, Clone)]
pub(crate) struct DrawSprite {
  pub(crate) texture: TextureRef,
  pub(crate) shading: SpriteShading,
  pub(crate) instance: SpriteInstance,
}

/// The frame's sprites and trail groups, reused across frames.
///
/// Lit items are drawn before lighting; unlit ones (trail groups with
/// `with_lighting(false)`) after it.
#[derive(Debug, Default)]
pub(crate) struct DrawList {
  items: Vec<DrawItem>,
  unlit_items: Vec<DrawItem>,
  sprites: Vec<DrawSprite>,
  next_sequence: u32,
  sorted: bool,
}

impl DrawList {
  /// Forget the frame's items, keeping the allocations.
  pub(crate) fn clear(&mut self) {
    self.items.clear();
    self.unlit_items.clear();
    self.sprites.clear();
    self.next_sequence = 0;
    self.sorted = true;
  }

  /// Add a sprite to the lit world.
  pub(crate) fn push_sprite(
    &mut self,
    kind: DrawKind,
    z: i32,
    texture: TextureRef,
    shading: SpriteShading,
    instance: SpriteInstance,
  ) {
    let payload = DrawPayload::Sprite(self.sprites.len());
    self.sprites.push(DrawSprite {
      texture,
      shading,
      instance,
    });
    self.push_item(true, z, kind, payload);
  }

  /// Add the trail/line geometry of z `z`; `lit` groups draw before lighting.
  pub(crate) fn push_trail_group(&mut self, lit: bool, z: i32) {
    self.push_item(lit, z, DrawKind::Trail, DrawPayload::Trail { z });
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
}

/// One draw of the world pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorldRun {
  /// `count` consecutive placed instances from `start`, on one atlas page
  /// and pipeline
  Sprites {
    page: SlotPage,
    pipeline: PipelineKind,
    start: usize,
    count: usize,
  },
  /// The trail/line group of this z
  Trail { z: i32 },
}

/// Split sorted `items` into draw runs.
///
/// `place` is asked once per sprite, in draw order, with its index in the
/// list: it puts the sprite in the instance buffer and returns its atlas page
/// and the pipeline it draws with, or `None` to drop the sprite.
/// Consecutive sprites on one page and pipeline merge into one run; a trail
/// group splits runs.
pub(crate) fn plan_runs(
  items: &[DrawItem],
  runs: &mut Vec<WorldRun>,
  mut place: impl FnMut(usize) -> Option<(SlotPage, PipelineKind)>,
) {
  runs.clear();
  let mut placed = 0;
  for item in items {
    match item.payload {
      DrawPayload::Sprite(index) => {
        let Some((page, pipeline)) = place(index) else {
          continue;
        };
        match runs.last_mut() {
          Some(WorldRun::Sprites {
            page: run_page,
            pipeline: run_pipeline,
            count,
            ..
          }) if *run_page == page && *run_pipeline == pipeline => *count += 1,
          _ => runs.push(WorldRun::Sprites {
            page,
            pipeline,
            start: placed,
            count: 1,
          }),
        }
        placed += 1;
      }
      DrawPayload::Trail { z } => runs.push(WorldRun::Trail { z }),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn texture(name: &str) -> TextureRef {
    Arc::from(name)
  }

  fn push(list: &mut DrawList, kind: DrawKind, z: i32, name: &str) {
    list.push_sprite(
      kind,
      z,
      texture(name),
      SpriteShading::Plain,
      SpriteInstance::new(),
    );
  }

  fn order(items: &[DrawItem]) -> Vec<(i32, DrawKind)> {
    items.iter().map(|item| (item.z, item.kind)).collect()
  }

  #[test]
  fn sorts_by_z_then_kind_then_sequence() {
    let mut list = DrawList::default();
    push(&mut list, DrawKind::Entity, 1, "a");
    list.push_trail_group(true, 0);
    push(&mut list, DrawKind::Sprite, 1, "b");
    push(&mut list, DrawKind::Entity, 1, "c");

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
    for name in ["a", "b", "c", "d"] {
      push(&mut list, DrawKind::Sprite, 3, name);
    }

    list.sorted();
    let names: Vec<_> = list
      .items()
      .iter()
      .filter_map(|item| match item.payload {
        DrawPayload::Sprite(i) => list.sprite(i).map(|s| s.texture.to_string()),
        DrawPayload::Trail { .. } => None,
      })
      .collect();

    assert_eq!(names, vec!["a", "b", "c", "d"]);
  }

  #[test]
  fn unlit_groups_sort_apart_from_lit_ones() {
    let mut list = DrawList::default();
    list.push_trail_group(false, 2);
    list.push_trail_group(true, 5);
    list.push_trail_group(false, -1);

    list.sorted();

    assert_eq!(order(list.items()), vec![(5, DrawKind::Trail)]);
    assert_eq!(
      order(list.unlit_items()),
      vec![(-1, DrawKind::Trail), (2, DrawKind::Trail)]
    );
  }

  fn runs_for(list: &mut DrawList, pages: &[(usize, PipelineKind)]) -> Vec<WorldRun> {
    list.sorted();
    let mut runs = Vec::new();
    plan_runs(list.items(), &mut runs, |i| {
      let (page, pipeline) = pages[i];
      Some((SlotPage::Atlas(page), pipeline))
    });
    runs
  }

  fn sprites(page: usize, start: usize, count: usize) -> WorldRun {
    WorldRun::Sprites {
      page: SlotPage::Atlas(page),
      pipeline: PipelineKind::Plain,
      start,
      count,
    }
  }

  #[test]
  fn consecutive_sprites_on_one_page_merge_into_one_run() {
    let mut list = DrawList::default();
    for z in 0..3 {
      push(&mut list, DrawKind::Sprite, z, "a");
    }

    let runs = runs_for(&mut list, &[(0, PipelineKind::Plain); 3]);

    assert_eq!(runs, vec![sprites(0, 0, 3)]);
  }

  #[test]
  fn a_trail_between_sprites_splits_the_run() {
    let mut list = DrawList::default();
    push(&mut list, DrawKind::Sprite, 0, "a");
    list.push_trail_group(true, 1);
    push(&mut list, DrawKind::Sprite, 2, "a");

    let runs = runs_for(&mut list, &[(0, PipelineKind::Plain); 2]);

    assert_eq!(
      runs,
      vec![sprites(0, 0, 1), WorldRun::Trail { z: 1 }, sprites(0, 1, 1)]
    );
  }

  #[test]
  fn a_new_page_or_pipeline_starts_a_run() {
    let mut list = DrawList::default();
    for z in 0..3 {
      push(&mut list, DrawKind::Sprite, z, "a");
    }

    let runs = runs_for(
      &mut list,
      &[
        (0, PipelineKind::Plain),
        (1, PipelineKind::Plain),
        (1, PipelineKind::Effect),
      ],
    );

    assert_eq!(
      runs,
      vec![
        sprites(0, 0, 1),
        sprites(1, 1, 1),
        WorldRun::Sprites {
          page: SlotPage::Atlas(1),
          pipeline: PipelineKind::Effect,
          start: 2,
          count: 1,
        },
      ]
    );
  }

  #[test]
  fn each_custom_shader_starts_a_run() {
    let mut list = DrawList::default();
    for z in 0..4 {
      push(&mut list, DrawKind::Sprite, z, "a");
    }

    let runs = runs_for(
      &mut list,
      &[
        (0, PipelineKind::Custom(1)),
        (0, PipelineKind::Custom(1)),
        (0, PipelineKind::Custom(2)),
        (0, PipelineKind::Custom(1)),
      ],
    );

    let counts: Vec<_> = runs
      .iter()
      .map(|run| match run {
        WorldRun::Sprites { count, .. } => *count,
        WorldRun::Trail { .. } => 0,
      })
      .collect();
    assert_eq!(counts, vec![2, 1, 1]);
  }

  #[test]
  fn dropped_sprites_leave_no_gap() {
    let mut list = DrawList::default();
    for z in 0..3 {
      push(&mut list, DrawKind::Sprite, z, "a");
    }
    list.sorted();

    let mut runs = Vec::new();
    plan_runs(list.items(), &mut runs, |i| {
      (i != 1).then_some((SlotPage::Atlas(0), PipelineKind::Plain))
    });

    assert_eq!(runs, vec![sprites(0, 0, 2)]);
  }

  #[test]
  fn clear_empties_the_list_for_reuse() {
    let mut list = DrawList::default();
    push(&mut list, DrawKind::Sprite, 0, "a");
    list.push_trail_group(false, 0);

    list.clear();

    assert!(list.items().is_empty() && list.unlit_items().is_empty());
    assert_eq!(list.sprite_count(), 0);
    push(&mut list, DrawKind::Sprite, 0, "b");
    assert_eq!(list.sorted()[0].sequence, 0);
  }
}
