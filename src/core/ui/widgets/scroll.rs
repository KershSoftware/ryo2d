//! Scroll areas: `ui.scroll()` clips and translates its contents by an
//! eased offset, with a scrollbar. The wheel is routed to the innermost area
//! that can take it (`interact::route_wheel`).

use std::hash::Hash;
use std::panic::Location;

use glam::Vec2;

use crate::core::ui::id::key_hash;
use crate::core::ui::immediate::UiContext;
use crate::core::ui::layout::{Edges, Flow, Length, Measure, UiStyle};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::Response;
use crate::core::ui::tree::{DrawKind, Node, RecordSpec};
use crate::core::ui::{Anchor, Color, Rect, UiId};

/// The scrollbar's width until the theme owns it
const BAR_WIDTH: f32 = 10.0;
/// The thumb's width, centred in the track
const THUMB_WIDTH: f32 = 6.0;
const THUMB_RADIUS: f32 = 3.0;
/// The shortest a thumb gets
const MIN_THUMB: f32 = 20.0;
/// How fast the offset catches up with its target (seconds)
const EASE_TIME: f32 = 0.08;
/// An offset this close to its target is there
const SNAP: f32 = 0.5;
const TRACK: Color = Color::new(40, 40, 50, 180);
const THUMB: Color = Color::new(90, 90, 110, 255);
const THUMB_HOVERED: Color = Color::new(120, 120, 140, 255);

/// What a scroll area remembers between frames
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ScrollState {
  /// Where the contents are shown from, eased toward `target`
  pub(crate) offset: Vec2,
  /// Where the wheel, the scrollbar and `scroll_to` want it
  pub(crate) target: Vec2,
  /// The size of its contents, as last laid out
  pub(crate) content: Vec2,
  /// The size of its view, as last laid out
  pub(crate) view: Vec2,
  /// A scrollbar drag in progress: the axis and where in the thumb it holds
  grab: Option<(usize, f32)>,
}

impl ScrollState {
  /// The furthest each axis scrolls (0 until the first layout)
  pub(crate) fn max(&self) -> Vec2 {
    (self.content - self.view).max(Vec2::ZERO)
  }

  /// Ease `offset` toward `target` over `dt` seconds, keeping both inside
  /// the contents once those are known
  fn ease(&mut self, dt: f32) {
    let known = self.view.x > 0.0 || self.view.y > 0.0;
    if known {
      self.target = self.target.clamp(Vec2::ZERO, self.max());
    }
    let step = if dt.is_finite() && dt > 0.0 {
      1.0 - (-dt / EASE_TIME).exp()
    } else {
      0.0
    };
    for axis in 0..2 {
      let left = self.target[axis] - self.offset[axis];
      self.offset[axis] = if self.grab.is_some() || left.abs() < SNAP {
        self.target[axis]
      } else {
        self.offset[axis] + left * step
      };
    }
    if known {
      self.offset = self.offset.clamp(Vec2::ZERO, self.max());
    }
  }
}

/// One scrollbar, in the scroll area's own units
struct Bar {
  axis: usize,
  /// Room for the thumb to travel
  track: Rect,
  /// How far the thumb's start is from the track's
  thumb_at: f32,
  thumb_len: f32,
}

impl Bar {
  /// The track's length along its axis
  fn len(&self) -> f32 {
    if self.axis == 1 {
      self.track.height
    } else {
      self.track.width
    }
  }

  /// Where the track starts along its axis
  fn start(&self) -> f32 {
    if self.axis == 1 {
      self.track.y
    } else {
      self.track.x
    }
  }

  /// The thumb's rect
  fn thumb(&self) -> Rect {
    if self.axis == 1 {
      Rect::new(
        self.track.x,
        self.track.y + self.thumb_at,
        self.track.width,
        self.thumb_len,
      )
    } else {
      Rect::new(
        self.track.x + self.thumb_at,
        self.track.y,
        self.thumb_len,
        self.track.height,
      )
    }
  }
}

/// The bars `state` shows on the axes `enabled` (sideways, down)
fn bars(state: &ScrollState, enabled: [bool; 2], visible: bool) -> [Option<Bar>; 2] {
  let needs = |axis: usize| {
    visible && enabled[axis] && state.view[axis] > 0.0 && state.content[axis] > state.view[axis]
  };
  let max = state.max();
  let make = |axis: usize| {
    if !needs(axis) {
      return None;
    }
    // The corner is not shared when both bars show
    let len = state.view[axis] - if needs(1 - axis) { BAR_WIDTH } else { 0.0 };
    let thumb_len = (state.view[axis] / state.content[axis] * len)
      .max(MIN_THUMB)
      .min(len);
    let thumb_at = if max[axis] > 0.0 {
      state.offset[axis] / max[axis] * (len - thumb_len)
    } else {
      0.0
    };
    let track = if axis == 1 {
      Rect::new(state.view.x - BAR_WIDTH, 0.0, BAR_WIDTH, len)
    } else {
      Rect::new(0.0, state.view.y - BAR_WIDTH, len, BAR_WIDTH)
    };
    Some(Bar {
      axis,
      track,
      thumb_at,
      thumb_len,
    })
  };
  [make(0), make(1)]
}

/// Follow the scrollbar the press began on while it is held, or let go.
/// `origin` is the area's top-left as last laid out.
fn drag_bar(
  state: &mut ScrollState,
  bars: &[Option<Bar>; 2],
  held: bool,
  origin: Option<Vec2>,
  press: Vec2,
  pointer: Vec2,
) {
  let Some(origin) = origin.filter(|_| held) else {
    state.grab = None;
    return;
  };
  if state.grab.is_none() {
    let local = press - origin;
    state.grab = bars.iter().flatten().find_map(|bar| {
      if !bar.track.contains(local.x, local.y) {
        return None;
      }
      let thumb = bar.thumb();
      // Pressed on the thumb, it keeps its place under the pointer; pressed
      // on the track, it is centred there
      let hold = if thumb.contains(local.x, local.y) {
        local[bar.axis] - (bar.start() + bar.thumb_at)
      } else {
        bar.thumb_len / 2.0
      };
      Some((bar.axis, hold))
    });
  }
  if let Some((axis, hold)) = state.grab
    && let Some(bar) = bars[axis].as_ref()
  {
    let travel = bar.len() - bar.thumb_len;
    if travel > 0.0 {
      let along = pointer[axis] - origin[axis] - bar.start() - hold;
      state.target[axis] = (along / travel * state.max()[axis]).clamp(0.0, state.max()[axis]);
    }
  }
}

/// A scroll area being set up: `.show(|ui| ..)` draws its contents
/// translated by the offset and clipped to its rect
#[must_use = "call .show(|ui| ..) to draw the scroll area"]
pub struct Scroll<'u, 'a> {
  ui: &'u mut UiContext<'a>,
  site: &'static Location<'static>,
  style: UiStyle,
  key: Option<u64>,
  at: Option<Rect>,
  /// Scrolls sideways, down
  axes: [bool; 2],
  bar: bool,
}

impl<'a> UiContext<'a> {
  /// A scroll area: its contents are laid out as a column and shown through
  /// a view of the size you give it (`width`, `height` or `at`). The wheel
  /// scrolls the innermost area under the pointer that can move, and Shift
  /// scrolls sideways. A scrollbar shows when the contents do not fit.
  ///
  /// ```ignore
  /// ui.scroll().width(300.0).height(200.0).show(|ui| {
  ///     for line in &self.log {
  ///         ui.text(line);
  ///     }
  /// });
  /// ```
  #[track_caller]
  pub fn scroll<'u>(&'u mut self) -> Scroll<'u, 'a> {
    Scroll {
      ui: self,
      site: Location::caller(),
      style: UiStyle::default(),
      key: None,
      at: None,
      axes: [false, true],
      bar: true,
    }
  }

  /// Bring the widget `id` into view in the scroll area around it, from the
  /// next frame
  pub fn scroll_to(&mut self, id: UiId) {
    self.memory.request_scroll(id);
  }

  /// Where the scroll area `scroll` shows its contents from (zero until it
  /// is drawn)
  pub fn scroll_offset(&self, scroll: UiId) -> Vec2 {
    self
      .memory
      .peek_state::<ScrollState>(scroll)
      .map_or(Vec2::ZERO, |state| state.offset)
  }

  /// Show the scroll area `scroll` from `offset` at once, without easing
  /// (clamped to its contents once they are laid out)
  pub fn set_scroll_offset(&mut self, scroll: UiId, offset: Vec2) {
    let (scene, _) = self.scene();
    if let Some(state) = self.memory.state::<ScrollState>(scroll, scene) {
      let offset = if offset.is_finite() {
        offset.max(Vec2::ZERO)
      } else {
        Vec2::ZERO
      };
      state.offset = offset;
      state.target = offset;
    }
  }
}

impl Scroll<'_, '_> {
  /// Scroll up and down (the default)
  pub fn vertical(mut self) -> Self {
    self.axes = [false, true];
    self.style.flow = Flow::Column;
    self
  }

  /// Scroll left and right: the contents flow in a row
  pub fn horizontal(mut self) -> Self {
    self.axes = [true, false];
    self.style.flow = Flow::Row;
    self
  }

  /// Scroll both ways
  pub fn both(mut self) -> Self {
    self.axes = [true, true];
    self.style.flow = Flow::Column;
    self
  }

  /// No scrollbar
  pub fn no_bar(mut self) -> Self {
    self.bar = false;
    self
  }

  /// Its width: UI units (`120.0`), a fraction of its parent
  /// (`Length::Pct(0.5)`), `Length::Fill` or `Length::Fit`
  pub fn width(mut self, w: impl Into<Length>) -> Self {
    self.style.width = w.into();
    self
  }

  /// Its height, as [`width`](Self::width)
  pub fn height(mut self, h: impl Into<Length>) -> Self {
    self.style.height = h.into();
    self
  }

  /// Take the width its parent has left
  pub fn fill_width(self) -> Self {
    self.width(Length::Fill)
  }

  /// Take the height its parent has left
  pub fn fill_height(self) -> Self {
    self.height(Length::Fill)
  }

  /// Take the width and height its parent has left
  pub fn fill(self) -> Self {
    self.fill_width().fill_height()
  }

  /// At least this wide
  pub fn min_width(mut self, w: f32) -> Self {
    self.style.min_width = Some(w);
    self
  }

  /// At most this wide
  pub fn max_width(mut self, w: f32) -> Self {
    self.style.max_width = Some(w);
    self
  }

  /// At least this tall
  pub fn min_height(mut self, h: f32) -> Self {
    self.style.min_height = Some(h);
    self
  }

  /// At most this tall
  pub fn max_height(mut self, h: f32) -> Self {
    self.style.max_height = Some(h);
    self
  }

  /// Room between its edges and its contents
  pub fn padding(mut self, p: impl Into<Edges>) -> Self {
    self.style.padding = p.into();
    self
  }

  /// Room around it, outside its edges
  pub fn margin(mut self, m: impl Into<Edges>) -> Self {
    self.style.margin = m.into();
    self
  }

  /// Room between its contents
  pub fn gap(mut self, g: f32) -> Self {
    self.style.gap = g;
    self
  }

  /// How much of the room its parent has left it takes, shared by weight
  pub fn grow(mut self, g: f32) -> Self {
    self.style.grow = g;
    self
  }

  /// Where it sits in a stack, or in the UI at the top level
  pub fn anchor(mut self, a: Anchor) -> Self {
    self.style.anchor = a;
    self
  }

  /// Moved by this much from where its anchor puts it
  pub fn offset(mut self, x: f32, y: f32) -> Self {
    self.style.offset = Vec2::new(x, y);
    self
  }

  /// Placed at `rect`, its size too (at the top level)
  pub fn at(mut self, rect: Rect) -> Self {
    self.at = Some(rect);
    self
  }

  /// Its id from `key` instead of how often this line ran
  pub fn id(mut self, key: impl Hash) -> Self {
    self.key = Some(key_hash(&key));
    self
  }
}

impl<'a> Scroll<'_, 'a> {
  /// Draw the scroll area with `contents` inside. The [`Response`]'s `id`
  /// names it for [`scroll_offset`](UiContext::scroll_offset) and
  /// [`set_scroll_offset`](UiContext::set_scroll_offset).
  pub fn show(self, contents: impl FnOnce(&mut UiContext<'a>)) -> Response {
    let Scroll {
      ui,
      site,
      mut style,
      key,
      at,
      axes,
      bar,
    } = self;
    let id = ui.memory.scopes.make(site, key);
    let (scene, _) = ui.scene();
    let dt = ui.dt();
    let interact = ui.memory.interact;
    let origin = ui.memory.prev(id).map(|r| Vec2::new(r.rect.x, r.rect.y));
    let mut scratch = ScrollState::default();
    let state = ui
      .memory
      .state::<ScrollState>(id, scene)
      .unwrap_or(&mut scratch);
    drag_bar(
      state,
      &bars(state, axes, bar),
      interact.active == id,
      origin,
      interact.press_pos,
      interact.pointer,
    );
    state.ease(dt);
    let shown = bars(state, axes, bar);
    let grabbed = state.grab.map(|(axis, _)| axis);

    // A scrollbar takes room, so contents never lie under it
    style.padding.right += if shown[1].is_some() { BAR_WIDTH } else { 0.0 };
    style.padding.bottom += if shown[0].is_some() { BAR_WIDTH } else { 0.0 };
    style.scroll_x = axes[0];
    style.scroll_y = axes[1];
    let mut node = Node::new(id, style, Measure::None);
    node.record = Some(RecordSpec {
      flags: Flags::SCROLLABLE | Flags::DRAG,
      nav: [UiId::NONE; 4],
    });
    node.clips = true;
    let index = ui.push_node(node, at);
    ui.memory.scopes.push(id);
    contents(ui);
    ui.memory.scopes.pop();

    let local = origin.map(|o| interact.pointer - o);
    for bar in shown.iter().flatten() {
      let hovered =
        grabbed == Some(bar.axis) || local.is_some_and(|p| bar.thumb().contains(p.x, p.y));
      let gap = (BAR_WIDTH - THUMB_WIDTH) / 2.0;
      let (track_frac, track_size, thumb_frac, thumb_size, thumb_inset) = if bar.axis == 1 {
        (
          Vec2::new(1.0, 0.0),
          Vec2::new(BAR_WIDTH, bar.track.height),
          Vec2::new(1.0, 0.0),
          Vec2::new(THUMB_WIDTH, bar.thumb_len),
          Edges::new(0.0, bar.thumb_at, gap, 0.0),
        )
      } else {
        (
          Vec2::new(0.0, 1.0),
          Vec2::new(bar.track.width, BAR_WIDTH),
          Vec2::new(0.0, 1.0),
          Vec2::new(bar.thumb_len, THUMB_WIDTH),
          Edges::new(bar.thumb_at, 0.0, 0.0, gap),
        )
      };
      ui.push_draw(
        index,
        DrawKind::Box {
          inset: Edges::ZERO,
          frac: track_frac,
          size: track_size,
          fill: TRACK,
          radius: 0.0,
          border: None,
        },
      );
      ui.push_draw(
        index,
        DrawKind::Box {
          inset: thumb_inset,
          frac: thumb_frac,
          size: thumb_size,
          fill: if hovered { THUMB_HOVERED } else { THUMB },
          radius: THUMB_RADIUS,
          border: None,
        },
      );
    }
    ui.close_node(index)
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::ScrollState;
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext, UiId};
  use glam::Vec2;

  fn list(
    ui: &mut UiContext<'_>,
    rows: usize,
    ids: &mut Vec<UiId>,
    clicked: &mut Option<usize>,
  ) -> UiId {
    ui.scroll()
      .at(Rect::new(0.0, 0.0, 120.0, 100.0))
      .show(|ui| {
        for i in 0..rows {
          let r = ui
            .button(format!("Row {i}"))
            .id(i)
            .height(30.0)
            .fill_width()
            .show();
          if r.clicked() {
            *clicked = Some(i);
          }
          ids.push(r.id);
        }
      })
      .id
  }

  #[test]
  fn the_wheel_scrolls_rows_and_clipped_rows_take_no_clicks() {
    let mut d = UiDriver::new((200.0, 200.0));
    let mut scroll = UiId::NONE;
    for _ in 0..3 {
      d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    }
    d.move_to(50.0, 50.0);
    d.wheel(0.0, -1.0);
    for _ in 0..30 {
      d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    }
    let offset = d.frame(|ui| ui.scroll_offset(scroll));
    assert!(
      (offset.y - 40.0).abs() < 0.5,
      "eased to the target: {offset:?}"
    );
    // Row 0 is above the view now; a click at the very top (y 2) is on row 1
    d.move_to(50.0, 2.0);
    d.press();
    d.frame(|ui| list(ui, 10, &mut Vec::new(), &mut None));
    d.release();
    let mut clicked = None;
    d.frame(|ui| list(ui, 10, &mut Vec::new(), &mut clicked));
    assert_eq!(clicked, Some(1));
    // Below the scroll area (y 150) nothing is clicked, though row 6 lies there unclipped
    d.move_to(50.0, 150.0);
    d.press();
    d.frame(|ui| list(ui, 10, &mut Vec::new(), &mut None));
    d.release();
    let mut clicked = None;
    d.frame(|ui| list(ui, 10, &mut Vec::new(), &mut clicked));
    assert_eq!(clicked, None);
  }

  #[test]
  fn scroll_to_brings_a_row_into_view() {
    let mut d = UiDriver::new((200.0, 200.0));
    let mut ids = Vec::new();
    let mut scroll = UiId::NONE;
    d.frame(|ui| scroll = list(ui, 10, &mut ids, &mut None));
    let target = ids[8];
    // The request is read against this frame's records, so the list is drawn too
    d.frame(|ui| {
      scroll = list(ui, 10, &mut Vec::new(), &mut None);
      ui.scroll_to(target);
    });
    for _ in 0..30 {
      d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    }
    let offset = d.frame(|ui| ui.scroll_offset(scroll));
    // Row 8 spans 240..270; the view is 100 tall
    assert!((offset.y - 170.0).abs() < 0.5, "{offset:?}");
  }

  #[test]
  fn ten_thousand_rows_lay_out_and_scroll() {
    let mut d = UiDriver::new((200.0, 200.0));
    let mut ids = Vec::new();
    d.frame(|ui| {
      list(ui, 10_000, &mut ids, &mut None);
    });
    let last = *ids.last().unwrap();
    let mut scroll = UiId::NONE;
    d.frame(|ui| {
      scroll = list(ui, 10_000, &mut Vec::new(), &mut None);
      ui.scroll_to(last);
    });
    for _ in 0..40 {
      d.frame(|ui| scroll = list(ui, 10_000, &mut Vec::new(), &mut None));
    }
    let offset = d.frame(|ui| ui.scroll_offset(scroll));
    assert!(
      (offset.y - (10_000.0 * 30.0 - 100.0)).abs() < 1.0,
      "{offset:?}"
    );
  }

  #[test]
  fn rows_are_placed_by_the_offset_and_clipped_to_the_view() {
    let mut d = UiDriver::new((200.0, 200.0));
    let mut ids = Vec::new();
    let mut scroll = UiId::NONE;
    d.frame(|ui| scroll = list(ui, 10, &mut ids, &mut None));
    d.frame(|ui| {
      ui.set_scroll_offset(scroll, Vec2::new(0.0, 50.0));
      list(ui, 10, &mut Vec::new(), &mut None);
    });
    let view = Rect::new(0.0, 0.0, 120.0, 100.0);
    let row = |d: &UiDriver, i: usize| d.memory.current(ids[i]).copied().unwrap();
    // Row 1 is 30 down the contents and 50 are scrolled away
    assert_eq!(row(&d, 1).rect.y, -20.0);
    assert_eq!(row(&d, 3).rect.y, 40.0);
    assert_eq!(row(&d, 3).clip, Some(view));
    // Row 0 (-50..-20) lies wholly outside the view: nothing of it is drawn
    let drawn = d
      .commands
      .commands()
      .filter(|c| matches!(c, UiCommand::Rect { rect, .. } if rect.bottom() <= 0.0))
      .count();
    assert_eq!(drawn, 0);
  }

  #[test]
  fn a_wheel_beyond_the_contents_stops_at_the_end() {
    let mut d = UiDriver::new((200.0, 200.0));
    let mut scroll = UiId::NONE;
    d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    d.move_to(50.0, 50.0);
    for _ in 0..8 {
      d.wheel(0.0, -1.0);
      d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    }
    for _ in 0..30 {
      d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    }
    // 300 of contents in a view of 100
    assert_eq!(d.frame(|ui| ui.scroll_offset(scroll)).y, 200.0);
  }

  #[test]
  fn dragging_the_scrollbar_thumb_scrolls() {
    let mut d = UiDriver::new((200.0, 200.0));
    let mut scroll = UiId::NONE;
    for _ in 0..3 {
      d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    }
    // The bar is the right 10 units of the 120 wide view; the thumb is a
    // third of the 100 tall track, at its top
    d.move_to(115.0, 10.0);
    d.press();
    d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    // 33 units of thumb travel 67 units of track for 200 units of contents
    d.move_to(115.0, 10.0 + 33.5);
    for _ in 0..3 {
      d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    }
    let offset = d.frame(|ui| ui.scroll_offset(scroll));
    assert!((offset.y - 100.0).abs() < 2.0, "{offset:?}");
    d.release();
    d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    // Let go, it stays put
    d.move_to(115.0, 90.0);
    d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    let after = d.frame(|ui| ui.scroll_offset(scroll));
    assert!((after.y - offset.y).abs() < 2.0, "{after:?}");
  }

  #[test]
  fn a_press_on_the_contents_does_not_scroll() {
    let mut d = UiDriver::new((200.0, 200.0));
    let mut scroll = UiId::NONE;
    for _ in 0..3 {
      d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    }
    d.move_to(50.0, 90.0);
    d.press();
    d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    d.move_to(50.0, 10.0);
    for _ in 0..5 {
      d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    }
    assert_eq!(d.frame(|ui| ui.scroll_offset(scroll)), Vec2::ZERO);
  }

  fn nested(d: &mut UiDriver) -> (UiId, UiId) {
    d.frame(|ui| {
      let mut inner = UiId::NONE;
      let outer = ui
        .scroll()
        .at(Rect::new(0.0, 0.0, 200.0, 100.0))
        .show(|ui| {
          inner = ui
            .scroll()
            .height(60.0)
            .show(|ui| {
              for i in 0..4 {
                ui.custom(100.0, 30.0).id(i).show();
              }
            })
            .id;
          for i in 0..6 {
            ui.custom(100.0, 30.0).id(10 + i).show();
          }
        })
        .id;
      (outer, inner)
    })
  }

  fn target(d: &UiDriver, id: UiId) -> Vec2 {
    d.memory.peek_state::<ScrollState>(id).unwrap().target
  }

  #[test]
  fn the_wheel_bubbles_from_an_inner_area_at_its_edge() {
    let mut d = UiDriver::new((300.0, 300.0));
    let mut ids = (UiId::NONE, UiId::NONE);
    for _ in 0..3 {
      ids = nested(&mut d);
    }
    let (outer, inner) = ids;
    d.move_to(20.0, 20.0);
    // Down: the inner area (120 of contents in a view of 60) takes two steps
    // of 40 (to its end at 60) before the outer one gets any
    for _ in 0..2 {
      d.wheel(0.0, -1.0);
      nested(&mut d);
    }
    assert_eq!(target(&d, inner).y, 60.0);
    assert_eq!(target(&d, outer).y, 0.0);
    d.wheel(0.0, -1.0);
    nested(&mut d);
    assert_eq!(target(&d, outer).y, 40.0);
  }

  fn strip(d: &mut UiDriver) -> UiId {
    d.frame(|ui| {
      ui.scroll()
        .horizontal()
        .at(Rect::new(0.0, 0.0, 200.0, 60.0))
        .show(|ui| {
          for i in 0..6 {
            ui.custom(150.0, 40.0).id(i).show();
          }
        })
        .id
    })
  }

  #[test]
  fn a_horizontal_area_flows_in_a_row_and_ignores_a_vertical_wheel() {
    let mut d = UiDriver::new((300.0, 300.0));
    let mut scroll = UiId::NONE;
    for _ in 0..3 {
      scroll = strip(&mut d);
    }
    let state = *d.memory.peek_state::<ScrollState>(scroll).unwrap();
    assert_eq!(state.content, Vec2::new(900.0, 60.0));
    d.move_to(20.0, 20.0);
    d.wheel(0.0, -1.0);
    strip(&mut d);
    assert_eq!(target(&d, scroll), Vec2::ZERO);
  }
}
