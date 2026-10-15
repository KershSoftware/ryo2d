//! Splitting a camera's sorted drawables into draw calls.

use std::ops::Range;

use super::{DrawState, Stream};
use crate::core::render::DrawItem;

/// One draw call: a range of the quad instances or the triangle indices.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Draw {
  pub(crate) stream: Stream,
  pub(crate) state: DrawState,
  /// Instances for quads, indices for triangles
  pub(crate) range: Range<u32>,
}

/// Where one drawable went, as `place` reports it to [`plan_draws`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Placed {
  /// The next quad instance, drawn with this state
  Quad(DrawState),
  /// These triangle indices, drawn with this state
  Triangles {
    state: DrawState,
    indices: Range<u32>,
  },
}

/// Split sorted `items` into draw calls, into `out` (cleared first).
///
/// `place` is asked once per item, in draw order: it puts the item in its
/// stream and says where, or returns `None` to drop it. Quads are numbered
/// from `first_quad` in the order placed. Consecutive quads with equal
/// states merge into one draw, as do consecutive triangle items with equal
/// states and adjacent index ranges; anything else starts a new draw.
/// Returns the number the next quad would get.
pub(crate) fn plan_draws<'i>(
  items: impl IntoIterator<Item = &'i DrawItem>,
  out: &mut Vec<Draw>,
  first_quad: u32,
  mut place: impl FnMut(&DrawItem) -> Option<Placed>,
) -> u32 {
  out.clear();
  let mut next_quad = first_quad;
  for item in items {
    let (stream, state, range) = match place(item) {
      None => continue,
      Some(Placed::Quad(state)) => {
        next_quad += 1;
        (Stream::Quads, state, next_quad - 1..next_quad)
      }
      Some(Placed::Triangles { state, indices }) => (Stream::Triangles, state, indices),
    };
    match out.last_mut() {
      Some(draw)
        if draw.stream == stream && draw.state == state && draw.range.end == range.start =>
      {
        draw.range.end = range.end;
      }
      _ => out.push(Draw {
        stream,
        state,
        range,
      }),
    }
  }
  next_quad
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::{SlotPage, SpriteInstance, TextureId};
  use crate::core::render::batch::{BatchShader, BlendMode, Sampling};
  use crate::core::render::{DrawKind, DrawList, SpriteShading};

  fn state(page: usize, blend: BlendMode) -> DrawState {
    DrawState {
      page: Some(SlotPage::Atlas(page)),
      shader: BatchShader::Sprite,
      blend,
      sampling: Sampling::Nearest,
      scissor: None,
    }
  }

  /// A list of `n` sprites, sorted.
  fn list(n: u32) -> DrawList {
    let mut list = DrawList::default();
    for i in 0..n {
      let texture = TextureId::for_test(i);
      list.push_sprite(
        DrawKind::Sprite,
        0,
        texture,
        SpriteShading::Plain,
        SpriteInstance::new(),
      );
    }
    list.sorted();
    list
  }

  fn plan(list: &DrawList, mut place: impl FnMut(usize) -> Option<Placed>) -> Vec<Draw> {
    let mut out = Vec::new();
    let mut i = 0;
    plan_draws(list.items(), &mut out, 0, |_| {
      i += 1;
      place(i - 1)
    });
    out
  }

  fn quads(out: &[Draw]) -> Vec<(DrawState, std::ops::Range<u32>)> {
    out.iter().map(|d| (d.state, d.range.clone())).collect()
  }

  #[test]
  fn equal_states_merge_into_one_draw() {
    let out = plan(&list(3), |_| Some(Placed::Quad(state(0, BlendMode::Alpha))));
    assert_eq!(quads(&out), [(state(0, BlendMode::Alpha), 0..3)]);
  }

  #[test]
  fn a_blend_or_page_change_splits() {
    let states = [
      state(0, BlendMode::Alpha),
      state(0, BlendMode::Additive),
      state(1, BlendMode::Additive),
    ];
    let out = plan(&list(3), |i| Some(Placed::Quad(states[i])));
    assert_eq!(
      quads(&out),
      [(states[0], 0..1), (states[1], 1..2), (states[2], 2..3)]
    );
  }

  #[test]
  fn a_scissor_change_splits_draws() {
    let clip = Some(crate::core::render::PixelRect {
      x: 0,
      y: 0,
      width: 10,
      height: 10,
    });
    let open = DrawState {
      page: None,
      shader: BatchShader::UiRect,
      ..state(0, BlendMode::Alpha)
    };
    let clipped = DrawState {
      scissor: clip,
      ..open
    };
    let states = [open, open, clipped, clipped, open];
    let out = plan(&list(5), |i| Some(Placed::Quad(states[i])));
    assert_eq!(quads(&out), [(open, 0..2), (clipped, 2..4), (open, 4..5)]);
  }

  #[test]
  fn dropped_items_leave_no_gap() {
    let out = plan(&list(3), |i| {
      (i != 1).then_some(Placed::Quad(state(0, BlendMode::Alpha)))
    });
    assert_eq!(quads(&out), [(state(0, BlendMode::Alpha), 0..2)]);
  }

  #[test]
  fn adjacent_triangle_ranges_merge_and_a_new_state_splits() {
    let tri = DrawState {
      shader: BatchShader::Line,
      page: None,
      ..state(0, BlendMode::Alpha)
    };
    let additive = DrawState {
      blend: BlendMode::Additive,
      ..tri
    };
    let placed = [
      Placed::Triangles {
        state: tri,
        indices: 0..6,
      },
      Placed::Triangles {
        state: tri,
        indices: 6..12,
      },
      Placed::Triangles {
        state: additive,
        indices: 12..18,
      },
      Placed::Triangles {
        state: tri,
        indices: 18..24,
      },
    ];
    let out = plan(&list(4), |i| Some(placed[i].clone()));
    let ranges: Vec<_> = out.iter().map(|d| (d.stream, d.range.clone())).collect();
    assert_eq!(
      ranges,
      [
        (Stream::Triangles, 0..12),
        (Stream::Triangles, 12..18),
        (Stream::Triangles, 18..24)
      ]
    );
  }
}
