//! What `UiContext` records: drawn by the engine's UI pass through the 2D
//! batch, by layer, then in the order the draws were recorded

#[cfg(test)]
use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;

use glam::Vec2;

use crate::core::asset::SpriteHandle;
use crate::core::render::batch::Filter;
use crate::core::text::TextLayout;
use crate::core::ui::{Color, Rect, UiId};

/// One UI draw, in UI units
#[derive(Debug, Clone)]
pub(crate) enum UiCommand {
  Rect {
    rect: Rect,
    fill: Color,
    radius: f32,
    border: Option<(f32, Color)>,
    clip: Option<Rect>,
    layer: u8,
  },
  Sprite {
    sprite: SpriteHandle,
    rect: Rect,
    uv: [f32; 4],
    tint: Color,
    /// How the texture is sampled (`Filter::Default` for absolute draws)
    filter: Filter,
    clip: Option<Rect>,
    layer: u8,
  },
  Line {
    points: Arc<[Vec2]>,
    /// Added to every point where it is drawn: a node's local line is
    /// placed so, sharing its points with the draw it was made from
    at: Vec2,
    width: f32,
    color: Color,
    closed: bool,
    clip: Option<Rect>,
    layer: u8,
  },
  Text {
    layout: Arc<TextLayout>,
    origin: Vec2,
    reveal: Option<(f32, f32)>,
    opacity: f32,
    /// Seeds the text's effects; made from the text alone, so it does not
    /// change with what else was drawn
    seed: u64,
    clip: Option<Rect>,
    layer: u8,
  },
}

impl UiCommand {
  /// Whether it draws what `other` does: shared layouts by address, line
  /// points by value (immediate widgets allocate them anew every frame),
  /// numbers by their bits, sprites by handle (the renderer stamps their
  /// versions apart)
  pub(crate) fn draws_as(&self, other: &UiCommand) -> bool {
    let clips = |a: &Option<Rect>, b: &Option<Rect>| a.map(bits) == b.map(bits);
    match (self, other) {
      (
        UiCommand::Rect {
          rect,
          fill,
          radius,
          border,
          clip,
          layer,
        },
        UiCommand::Rect {
          rect: rect_b,
          fill: fill_b,
          radius: radius_b,
          border: border_b,
          clip: clip_b,
          layer: layer_b,
        },
      ) => {
        bits(*rect) == bits(*rect_b)
          && fill == fill_b
          && radius.to_bits() == radius_b.to_bits()
          && border.map(|(width, color)| (width.to_bits(), color))
            == border_b.map(|(width, color)| (width.to_bits(), color))
          && clips(clip, clip_b)
          && layer == layer_b
      }
      (
        UiCommand::Sprite {
          sprite,
          rect,
          uv,
          tint,
          filter,
          clip,
          layer,
        },
        UiCommand::Sprite {
          sprite: sprite_b,
          rect: rect_b,
          uv: uv_b,
          tint: tint_b,
          filter: filter_b,
          clip: clip_b,
          layer: layer_b,
        },
      ) => {
        sprite.serial() == sprite_b.serial()
          && bits(*rect) == bits(*rect_b)
          && uv.map(f32::to_bits) == uv_b.map(f32::to_bits)
          && tint == tint_b
          && filter == filter_b
          && clips(clip, clip_b)
          && layer == layer_b
      }
      (
        UiCommand::Line {
          points,
          at,
          width,
          color,
          closed,
          clip,
          layer,
        },
        UiCommand::Line {
          points: points_b,
          at: at_b,
          width: width_b,
          color: color_b,
          closed: closed_b,
          clip: clip_b,
          layer: layer_b,
        },
      ) => {
        (Arc::ptr_eq(points, points_b)
          || (points.len() == points_b.len()
            && points
              .iter()
              .zip(points_b.iter())
              .all(|(p, q)| p.to_array().map(f32::to_bits) == q.to_array().map(f32::to_bits))))
          && at.to_array().map(f32::to_bits) == at_b.to_array().map(f32::to_bits)
          && width.to_bits() == width_b.to_bits()
          && color == color_b
          && closed == closed_b
          && clips(clip, clip_b)
          && layer == layer_b
      }
      (
        UiCommand::Text {
          layout,
          origin,
          reveal,
          opacity,
          seed,
          clip,
          layer,
        },
        UiCommand::Text {
          layout: layout_b,
          origin: origin_b,
          reveal: reveal_b,
          opacity: opacity_b,
          seed: seed_b,
          clip: clip_b,
          layer: layer_b,
        },
      ) => {
        Arc::ptr_eq(layout, layout_b)
          && origin.to_array().map(f32::to_bits) == origin_b.to_array().map(f32::to_bits)
          && reveal.map(|(shown, fade)| (shown.to_bits(), fade.to_bits()))
            == reveal_b.map(|(shown, fade)| (shown.to_bits(), fade.to_bits()))
          && opacity.to_bits() == opacity_b.to_bits()
          && seed == seed_b
          && clips(clip, clip_b)
          && layer == layer_b
      }
      _ => false,
    }
  }

  /// Its draw layer (higher layers draw over lower ones)
  pub(crate) fn draw_layer(&self) -> u8 {
    match self {
      Self::Rect { layer, .. }
      | Self::Sprite { layer, .. }
      | Self::Line { layer, .. }
      | Self::Text { layer, .. } => *layer,
    }
  }

  /// The clip it was recorded under
  pub(crate) fn clip_rect(&self) -> Option<Rect> {
    match self {
      Self::Rect { clip, .. }
      | Self::Sprite { clip, .. }
      | Self::Line { clip, .. }
      | Self::Text { clip, .. } => *clip,
    }
  }

  #[cfg(test)]
  fn clip_mut(&mut self) -> &mut Option<Rect> {
    match self {
      Self::Rect { clip, .. }
      | Self::Sprite { clip, .. }
      | Self::Line { clip, .. }
      | Self::Text { clip, .. } => clip,
    }
  }

  /// The box it is culled by: a rect's or sprite's rect, a text's block (its
  /// layout's size at the scale it was laid out at); `None` for a line,
  /// which is never culled
  pub(crate) fn cull_box(&self) -> Option<Rect> {
    match self {
      Self::Rect { rect, .. } | Self::Sprite { rect, .. } => Some(*rect),
      Self::Text { layout, origin, .. } => {
        let size = layout.size / layout.scale;
        Some(Rect::new(origin.x, origin.y, size.x, size.y))
      }
      Self::Line { .. } => None,
    }
  }

  /// The same draw moved by `by` (UI units): its rect, a line's offset, a
  /// text's origin and its own clip. A line's points and a text's layout are
  /// shared, not copied.
  #[cfg(test)]
  pub(crate) fn moved(&self, by: Vec2) -> UiCommand {
    let shift = |rect: Rect| shifted(rect, by);
    let mut command = match self {
      Self::Rect {
        rect,
        fill,
        radius,
        border,
        clip,
        layer,
      } => Self::Rect {
        rect: shift(*rect),
        fill: *fill,
        radius: *radius,
        border: *border,
        clip: *clip,
        layer: *layer,
      },
      Self::Sprite {
        sprite,
        rect,
        uv,
        tint,
        filter,
        clip,
        layer,
      } => Self::Sprite {
        sprite: sprite.clone(),
        rect: shift(*rect),
        uv: *uv,
        tint: *tint,
        filter: *filter,
        clip: *clip,
        layer: *layer,
      },
      Self::Line {
        points,
        at,
        width,
        color,
        closed,
        clip,
        layer,
      } => Self::Line {
        points: Arc::clone(points),
        at: *at + by,
        width: *width,
        color: *color,
        closed: *closed,
        clip: *clip,
        layer: *layer,
      },
      Self::Text {
        layout,
        origin,
        reveal,
        opacity,
        seed,
        clip,
        layer,
      } => Self::Text {
        layout: Arc::clone(layout),
        origin: *origin + by,
        reveal: *reveal,
        opacity: *opacity,
        seed: *seed,
        clip: *clip,
        layer: *layer,
      },
    };
    let clip = command.clip_mut();
    *clip = clip.map(shift);
    command
  }
}

#[cfg(test)]
impl UiCommand {
  pub(crate) fn kind_name(&self) -> &'static str {
    match self {
      Self::Rect { .. } => "rect",
      Self::Sprite { .. } => "sprite",
      Self::Line { .. } => "line",
      Self::Text { .. } => "text",
    }
  }

  pub(crate) fn clip(&self) -> Option<Rect> {
    self.clip_rect()
  }

  pub(crate) fn text_origin(&self) -> Option<Vec2> {
    match self {
      Self::Text { origin, .. } => Some(*origin),
      _ => None,
    }
  }
}

/// A scroll area's slot in a frame: what the area shows is placed relative
/// to it, so moving the slot moves its contents exactly
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollSlot {
  /// Where the contents' origin is drawn, in whole render pixels
  pub(crate) offset: [i32; 2],
  /// The clip the contents draw under (UI units, absolute)
  pub(crate) clip: Option<Rect>,
  /// That clip in the contents' own coordinates: what a draw in the slot
  /// is culled by, with its own clip
  pub(crate) cull: Option<Rect>,
}

impl ScrollSlot {
  /// Slot 0: no scroll area, so positions and clips are absolute
  pub(crate) const NONE: ScrollSlot = ScrollSlot {
    offset: [0, 0],
    clip: None,
    cull: None,
  };

  /// Its offset in UI units, at `px_scale` render pixels per UI unit
  pub(crate) fn offset_units(&self, px_scale: f32) -> Vec2 {
    Vec2::new(self.offset[0] as f32, self.offset[1] as f32) / px_scale
  }
}

/// One UI draw: its place in the painter's order, the slot its positions
/// and clip are relative to (0: none, positions absolute), and the command
#[derive(Debug, Clone)]
pub(crate) struct UiDraw {
  pub(crate) order: u32,
  pub(crate) slot: u16,
  pub(crate) command: UiCommand,
}

impl UiDraw {
  /// Where it is drawn: what it is moved by, its slot's offset (at
  /// `px_scale` render pixels per UI unit; `None` in slot 0), and its clip
  /// moved with it and met with the slot's. `None` when its slot is not in
  /// `slots`. Its rect, a line's offset and a text's origin move by
  /// [`shifted`]'s sums, as `UiCommand::moved` moves them.
  pub(crate) fn placement(
    &self,
    slots: &[ScrollSlot],
    px_scale: f32,
  ) -> Option<(Option<Vec2>, Option<Rect>)> {
    let clip = self.command.clip_rect();
    if self.slot == 0 {
      return Some((None, clip));
    }
    let slot = slots.get(usize::from(self.slot))?;
    let by = slot.offset_units(px_scale);
    Some((
      Some(by),
      meet(slot.clip, clip.map(|clip| shifted(clip, by))),
    ))
  }

  /// The command as it is drawn (`placement`): one in a slot moved by the
  /// slot's offset, its clip moved with it and met with the slot's. `None`
  /// when its slot is not in `slots`.
  #[cfg(test)]
  pub(crate) fn placed(&self, slots: &[ScrollSlot], px_scale: f32) -> Option<Cow<'_, UiCommand>> {
    let (by, clip) = self.placement(slots, px_scale)?;
    let Some(by) = by else {
      return Some(Cow::Borrowed(&self.command));
    };
    let mut command = self.command.moved(by);
    *command.clip_mut() = clip;
    Some(Cow::Owned(command))
  }

  /// Whether it can show a pixel: its box (`UiCommand::cull_box`) meets
  /// its own clip, met in a slot with the slot's clip in its coordinates
  /// (`ScrollSlot::cull`). This is the test resolve culls the draws outside
  /// fragments by, so a full rebuild sends only draws that pass it; a line
  /// always does.
  pub(crate) fn shows(&self, slots: &[ScrollSlot]) -> bool {
    let Some(seen) = self.command.cull_box() else {
      return true;
    };
    let clip = self.command.clip_rect();
    let cull = match self.slot {
      0 => clip,
      k => meet(clip, slots.get(usize::from(k)).and_then(|slot| slot.cull)),
    };
    !outside(cull, seen)
  }
}

/// A rect's numbers' bits
pub(crate) fn bits(rect: Rect) -> [u32; 4] {
  [rect.x, rect.y, rect.width, rect.height].map(f32::to_bits)
}

/// `rect` moved by `by` (UI units)
pub(crate) fn shifted(rect: Rect, by: Vec2) -> Rect {
  Rect::new(rect.x + by.x, rect.y + by.y, rect.width, rect.height)
}

/// The overlap of two rects (empty when they do not meet)
pub(crate) fn intersect(a: Rect, b: Rect) -> Rect {
  let (x, y) = (a.x.max(b.x), a.y.max(b.y));
  let (right, bottom) = (a.right().min(b.right()), a.bottom().min(b.bottom()));
  Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
}

/// Two clips together: their overlap, either one alone, or none
pub(crate) fn meet(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
  match (a, b) {
    (Some(a), Some(b)) => Some(intersect(a, b)),
    (a, b) => a.or(b),
  }
}

/// Whether `rect` lies wholly outside `clip`: nothing of it can show
pub(crate) fn outside(clip: Option<Rect>, rect: Rect) -> bool {
  clip.is_some_and(|clip| {
    clip.width <= 0.0
      || clip.height <= 0.0
      || rect.right() <= clip.x
      || rect.x >= clip.right()
      || rect.bottom() <= clip.y
      || rect.y >= clip.bottom()
  })
}

/// The edges of the box a run's draws' boxes (`UiCommand::cull_box`) lie
/// in, grown draw by draw: none once a draw has no box (a line) or one not
/// finite
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Bounds {
  edges: Option<[f32; 4]>,
  open: bool,
}

impl Bounds {
  /// Grown by a draw's box
  pub(crate) fn add(&mut self, cull_box: Option<Rect>) {
    let Some(rect) = cull_box else {
      self.open = true;
      return;
    };
    let [x0, y0, x1, y1] = [rect.x, rect.y, rect.right(), rect.bottom()];
    if ![x0, y0, x1, y1].iter().all(|edge| edge.is_finite()) {
      self.open = true;
      return;
    }
    self.edges = Some(match self.edges {
      Some([a, b, c, d]) => [a.min(x0), b.min(y0), c.max(x1), d.max(y1)],
      None => [x0, y0, x1, y1],
    });
  }

  /// Its edges, x0, y0, x1, y1: `None` when a draw had no box, or none was
  /// added
  pub(crate) fn edges(&self) -> Option<[f32; 4]> {
    self.edges.filter(|_| !self.open)
  }
}

/// A fragment's commands in the frame: `draws` is their range in
/// `UiCommands::as_slice()` (contiguous, in painter's order, one layer, one
/// slot), and the rest says when they are the same as before
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FragmentRun {
  /// Its root's id
  pub(crate) id: UiId,
  /// Changes whenever its commands may (`Fragment::version`)
  pub(crate) version: u64,
  /// Its first order: the run's orders are `base..base + count`
  pub(crate) base: u32,
  pub(crate) layer: u8,
  pub(crate) slot: u16,
  pub(crate) draws: Range<u32>,
  /// The edges of the box its draws' boxes lie in (`Bounds`), in its
  /// slot's coordinates; `None` when one has none. They follow from its
  /// version, as its commands do.
  pub(crate) bounds: Option<[f32; 4]>,
}

/// The frame's UI draws (cleared each UI pass), each with its place in the
/// painter's order and its slot, the frame's slot table and its fragment
/// runs. A container's draws are pushed when it is laid out, after absolute
/// draws recorded later, so the order (taken when a draw was recorded), not
/// the position in the list, is what orders them.
///
/// A pass writes its draws over the last pass's, which stay past what it
/// has pushed: a kept fragment's run that the last pass left where this one
/// pushes it next, with the same tag, is taken as it is (`reuse_run`),
/// its draws neither cloned nor written.
#[derive(Debug)]
pub(crate) struct UiCommands {
  /// This pass's draws, then, past `len`, the rest of the last pass's
  draws: Vec<UiDraw>,
  /// The draws this pass has pushed
  len: usize,
  /// Slot 0 (no scroll area), then each scroll area's, in the order they
  /// were laid out
  slots: Vec<ScrollSlot>,
  /// Each fragment's draws, tagged, in the order of their ranges
  runs: Vec<FragmentRun>,
  /// The last pass's runs: those from `len` on still hold their draws
  runs_was: Vec<FragmentRun>,
  /// The first of `runs_was` that may still be where it was
  was_at: usize,
}

impl Default for UiCommands {
  fn default() -> UiCommands {
    UiCommands {
      draws: Vec::new(),
      len: 0,
      slots: vec![ScrollSlot::NONE],
      runs: Vec::new(),
      runs_was: Vec::new(),
      was_at: 0,
    }
  }
}

impl UiCommands {
  /// Each draw, in the order they were pushed
  pub(crate) fn as_slice(&self) -> &[UiDraw] {
    self.draws.get(..self.len).unwrap_or_default()
  }
  /// The slot table: slot 0, then the scroll areas'
  pub(crate) fn slots(&self) -> &[ScrollSlot] {
    &self.slots
  }
  /// The fragment runs, in the order of their ranges: a range is
  /// contiguous and holds one fragment's draws, which no other run holds
  pub(crate) fn runs(&self) -> &[FragmentRun] {
    &self.runs
  }
  /// A new pass: no draws, slots or runs. The last pass's draws stay to
  /// be written over, or taken as they are by `reuse_run`.
  pub(crate) fn clear(&mut self) {
    self.end_pass();
    self.len = 0;
    self.slots.clear();
    self.slots.push(ScrollSlot::NONE);
    std::mem::swap(&mut self.runs, &mut self.runs_was);
    self.runs.clear();
    self.was_at = 0;
  }
  /// The pass is over: what is left of the last pass's draws past this
  /// pass's goes (with what it holds of sprites and text layouts)
  pub(crate) fn end_pass(&mut self) {
    self.draws.truncate(self.len);
  }
  pub(crate) fn push(&mut self, order: u32, slot: u16, command: UiCommand) {
    let draw = UiDraw {
      order,
      slot,
      command,
    };
    match self.draws.get_mut(self.len) {
      Some(at) => *at = draw,
      None => self.draws.push(draw),
    }
    self.len += 1;
  }
  /// Take a kept fragment's run where the last pass left it: when the last
  /// pass's draws from where the next one goes are the run of fragment
  /// `id` at `version`, from order `base` on `layer` in `slot`, they are
  /// this pass's as they are, and the run is pushed. A run of the same
  /// fragment, version, base, layer and slot holds the same draws. False,
  /// with nothing pushed, when they are not there.
  pub(crate) fn reuse_run(
    &mut self,
    id: UiId,
    version: u64,
    base: u32,
    layer: u8,
    slot: u16,
  ) -> bool {
    let at = self.position();
    // Runs before here have been written over
    while self
      .runs_was
      .get(self.was_at)
      .is_some_and(|was| was.draws.start < at)
    {
      self.was_at += 1;
    }
    let Some(was) = self.runs_was.get(self.was_at) else {
      return false;
    };
    let here = was.draws.start == at && was.draws.end as usize <= self.draws.len();
    let same = was.id == id
      && was.version == version
      && was.base == base
      && was.layer == layer
      && was.slot == slot;
    if !(here && same) {
      return false;
    }
    let run = was.clone();
    self.len = run.draws.end as usize;
    self.was_at += 1;
    self.runs.push(run);
    true
  }
  /// Add a scroll area's slot: its index, or `u16::MAX` when the table is
  /// full (its contents are then placed absolute)
  pub(crate) fn push_slot(&mut self, slot: ScrollSlot) -> u16 {
    let Ok(index) = u16::try_from(self.slots.len()) else {
      return u16::MAX;
    };
    if index == u16::MAX {
      return u16::MAX;
    }
    self.slots.push(slot);
    index
  }
  /// Where the next draw pushed goes in the list: a run's range starts or
  /// ends here
  pub(crate) fn position(&self) -> u32 {
    u32::try_from(self.len).unwrap_or(u32::MAX)
  }
  /// Tag the draws `run.draws` as one fragment's: the caller pushed them
  /// just before, each in `run.slot` on `run.layer`, from order `run.base`
  /// on. A range past the draws is dropped.
  pub(crate) fn push_run(&mut self, run: FragmentRun) {
    let Range { start, end } = run.draws;
    let held = self.as_slice().get(start as usize..end as usize);
    debug_assert!(held.is_some(), "a run holds draws pushed: {run:?}");
    debug_assert!(
      held.into_iter().flatten().all(|draw| {
        draw.slot == run.slot && draw.order >= run.base && draw.command.draw_layer() == run.layer
      }),
      "a run's draws share its slot and layer, from its base on: {run:?}"
    );
    debug_assert!(
      run.bounds.is_none_or(|[x0, y0, x1, y1]| {
        held.into_iter().flatten().all(|draw| {
          draw.command.cull_box().is_some_and(|rect| {
            rect.x >= x0 && rect.y >= y0 && rect.right() <= x1 && rect.bottom() <= y1
          })
        })
      }),
      "a run's bounds hold its draws' boxes: {run:?}"
    );
    if held.is_some() {
      self.runs.push(run);
    }
  }
  /// Put the runs from `from` on in the order of their ranges (an empty one
  /// by its base, where its copy goes): resolve pushes a captured
  /// fragment's run after the kept ones of its subtree
  pub(crate) fn sort_runs(&mut self, from: usize) {
    if let Some(runs) = self.runs.get_mut(from..) {
      runs.sort_unstable_by_key(|run| (run.draws.start, run.draws.end, run.base));
    }
    debug_assert!(self.runs_hold(), "runs never overlap: {:?}", self.runs);
  }
  /// Whether the runs are in the order of their ranges, which do not
  /// overlap and hold draws pushed
  pub(crate) fn runs_hold(&self) -> bool {
    let mut at = 0;
    self.runs.iter().all(|run| {
      let Range { start, end } = run.draws;
      let holds = at <= start && start <= end && end as usize <= self.len;
      at = end;
      holds
    })
  }
  pub(crate) fn is_empty(&self) -> bool {
    self.len == 0
  }
  /// How many commands there are (the UI harness)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn len(&self) -> usize {
    self.len
  }

  /// The draws that can show a pixel (`UiDraw::shows`), in the order they
  /// were pushed. With fragments off this keeps every draw; a kept
  /// fragment's draws in a slot are culled by the slot's wider interest,
  /// and some cannot show. What two runs with fragments on and off compare.
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn visible(&self) -> impl Iterator<Item = &UiDraw> {
    self
      .as_slice()
      .iter()
      .filter(|draw| draw.shows(&self.slots))
  }
}

#[cfg(test)]
impl UiCommands {
  /// Each draw, in the order they were pushed
  pub(crate) fn iter(&self) -> impl Iterator<Item = &UiDraw> {
    self.as_slice().iter()
  }
  /// The commands as stored (relative to their slots), in the order they
  /// were pushed
  pub(crate) fn commands(&self) -> impl Iterator<Item = &UiCommand> {
    self.as_slice().iter().map(|draw| &draw.command)
  }
  /// The commands where they are drawn at `px_scale` render pixels per UI
  /// unit (each moved by its slot and under its slot's clip), in the order
  /// they were pushed
  pub(crate) fn placed(&self, px_scale: f32) -> Vec<UiCommand> {
    self
      .as_slice()
      .iter()
      .filter_map(|draw| draw.placed(&self.slots, px_scale))
      .map(Cow::into_owned)
      .collect()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn rect(rect: Rect, clip: Option<Rect>) -> UiCommand {
    UiCommand::Rect {
      rect,
      fill: Color::WHITE,
      radius: 0.0,
      border: None,
      clip,
      layer: 0,
    }
  }

  #[test]
  fn a_runs_bounds_hold_its_draws_boxes_while_each_has_one() {
    let mut bounds = Bounds::default();
    assert_eq!(bounds.edges(), None, "nothing added");
    bounds.add(Some(Rect::new(10.0, 20.0, 5.0, 5.0)));
    bounds.add(Some(Rect::new(-3.0, 30.0, 4.0, 10.0)));
    assert_eq!(bounds.edges(), Some([-3.0, 20.0, 15.0, 40.0]));
    let (mut line, mut nan) = (bounds, bounds);
    line.add(None);
    assert_eq!(line.edges(), None, "a line has no box");
    nan.add(Some(Rect::new(f32::NAN, 0.0, 1.0, 1.0)));
    assert_eq!(nan.edges(), None, "nor has a box not finite");
  }

  #[test]
  fn a_draw_is_visible_where_its_box_meets_its_clip_and_its_slots() {
    let mut commands = UiCommands::default();
    // A slot whose view is 100 by 100 at (100, 50), its contents drawn 20
    // render pixels up at scale 2: in its own coordinates the view starts
    // at (0, 15)
    let clip = Rect::new(100.0, 50.0, 100.0, 100.0);
    let slot = commands.push_slot(ScrollSlot {
      offset: [200, 70],
      clip: Some(clip),
      cull: Some(Rect::new(0.0, 15.0, 100.0, 100.0)),
    });
    // Order 1: above the slot's view in its coordinates, though not once
    // moved by the offset as a placed draw would be
    commands.push(1, slot, rect(Rect::new(10.0, 0.0, 10.0, 10.0), None));
    // 2: meets the view
    commands.push(2, slot, rect(Rect::new(10.0, 10.0, 10.0, 10.0), None));
    // 3: meets the view, outside its own clip (relative to the slot)
    let own = Some(Rect::new(0.0, 0.0, 5.0, 200.0));
    commands.push(3, slot, rect(Rect::new(10.0, 10.0, 10.0, 10.0), own));
    // 4: touches the view's edge only
    commands.push(4, slot, rect(Rect::new(10.0, 5.0, 10.0, 10.0), None));
    // 5, 6: text, by its block (layout pixels at its layout's scale)
    let layout = Arc::new(TextLayout {
      size: Vec2::new(40.0, 20.0),
      scale: 2.0,
      ..TextLayout::default()
    });
    let text = |y: f32| UiCommand::Text {
      layout: Arc::clone(&layout),
      origin: Vec2::new(0.0, y),
      reveal: None,
      opacity: 1.0,
      seed: 0,
      clip: Some(Rect::new(0.0, 100.0, 50.0, 50.0)),
      layer: 0,
    };
    commands.push(5, 0, text(95.0));
    commands.push(6, 0, text(90.0));
    // 7: a line, never culled (resolve never culls one)
    let line = UiCommand::Line {
      points: Arc::from([Vec2::ZERO, Vec2::new(5.0, 0.0)]),
      at: Vec2::ZERO,
      width: 1.0,
      color: Color::WHITE,
      closed: false,
      clip: Some(Rect::new(300.0, 300.0, 10.0, 10.0)),
      layer: 0,
    };
    commands.push(7, 0, line);
    let visible: Vec<u32> = commands.visible().map(|draw| draw.order).collect();
    assert_eq!(visible, [2, 5, 7]);
  }
}
