//! Which texture each UI instance samples, and the runs of instances that
//! can share one bind group

use std::ops::Range;

use crate::core::graphics::{GlyphPage, SlotPage, TextureId};

/// What an instance samples
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TexNeed {
  None,
  Glyph(GlyphPage),
  Sprite(TextureId),
}

/// The bind group's texture slots, in binding order
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Slot {
  Mask,
  Color,
  Msdf,
  Sprite,
}

impl Slot {
  fn index(self) -> usize {
    self as usize
  }
}

/// A texture in a slot, as the frame resolved it (a glyph page, or the page a
/// sprite texture sits on this frame)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TexKey {
  Glyph(GlyphPage),
  Sprite(SlotPage),
}

/// Instances that draw with one bind group: the textures in its slots
/// (`None`: the placeholder)
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Run {
  pub(crate) range: Range<u32>,
  pub(crate) textures: [Option<TexKey>; 4],
}

/// Split instances into runs: a run ends when an instance needs a texture in a
/// slot that already holds another one. The next run starts with only that
/// texture; its other slots fill as later instances need them (carrying the
/// last run's textures over could only make more breaks).
pub(crate) fn plan_runs(
  needs: &[TexNeed],
  slot_of: impl Fn(TexNeed) -> Option<(Slot, TexKey)>,
) -> Vec<Run> {
  let mut runs = Vec::new();
  let mut start = 0u32;
  let mut textures: [Option<TexKey>; 4] = [None; 4];
  for (index, need) in needs.iter().enumerate() {
    let Some((slot, key)) = slot_of(*need) else {
      continue;
    };
    match textures[slot.index()] {
      Some(current) if current != key => {
        let index = index as u32;
        runs.push(Run {
          range: start..index,
          textures,
        });
        start = index;
        textures = [None; 4];
        textures[slot.index()] = Some(key);
      }
      Some(_) => {}
      None => textures[slot.index()] = Some(key),
    }
  }
  let end = needs.len() as u32;
  if end > start {
    runs.push(Run {
      range: start..end,
      textures,
    });
  }
  runs
}
