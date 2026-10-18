//! Virtual lists: `ui.list(count)` is a scroll area of the list's exact
//! length that builds only the rows in view (and a few either side), with
//! spacers standing in for the rest

use std::hash::Hash;
use std::ops::Range;
use std::panic::Location;

#[cfg(test)]
use crate::core::ui::id::IdScopes;
use crate::core::ui::id::key_hash;
use crate::core::ui::immediate::UiContext;
use crate::core::ui::layout::{Edges, Flow, Length, Measure, UiStyle};
use crate::core::ui::memory::{Flags, UiMemory};
use crate::core::ui::response::Response;
use crate::core::ui::theme::warn_once;
use crate::core::ui::tree::Node;
use crate::core::ui::widgets::scroll::{Scroll, ScrollState};
use crate::core::ui::{Anchor, Rect, UiId};

/// Rows built past each edge of the view unless `.overscan` says otherwise
const OVERSCAN: usize = 2;
/// Keeps a list's state apart from its scroll area's, which has its id
const STATE_SALT: u64 = 0x006c_6973;
/// The spacers before, between and after the runs of built rows
const LEAD_SALT: u64 = 0x6c65_6164;
const GAP_SALT: u64 = 0x6761_7073;
const TRAIL_SALT: u64 = 0x7472_6169;
/// The empty cells that fill a grid's last row
const BLANK_SALT: u64 = 0x626c_616e;

/// What a list remembers between frames, in `UiMemory` under
/// [`state_id`] of the list's id (its scroll area keeps that id)
#[derive(Debug, Clone, Default)]
pub(crate) struct ListState {
  /// A row's height and the gap after it
  pub(crate) stride: f32,
  /// A row's height
  pub(crate) row_height: f32,
  /// Where the first row starts in the contents: the area's top padding
  pub(crate) top: f32,
  /// Rows of `columns` items, the last one perhaps short
  pub(crate) rows: usize,
  /// Items per row, at least 1
  pub(crate) columns: u16,
  /// Items
  pub(crate) count: usize,
  /// `list_scroll_to` moved the target: the next build brings the offset to
  /// within a view of it first
  pub(crate) jump: bool,
  /// The item whose row holds the focus: its row (and one either side, so
  /// the pad can step off it) stays built wherever the list scrolls
  pub(crate) focused: Option<usize>,
  /// The widget with the focus, made in `focused`'s row
  pub(crate) focus_widget: UiId,
  /// An item to give the focus once its row is built (`list_focus`, or the
  /// new last item when the count shrank under the focus); its row stays
  /// built until then
  pub(crate) pending_focus: Option<usize>,
  /// The ids made while building the pending item's row
  pub(crate) focus_ids: Vec<UiId>,
  /// The focus was asked for one of `focus_ids`: the next build ends the
  /// pending focus, landed or not
  pub(crate) focus_asked: bool,
}

impl ListState {
  /// Give item `index` the focus once its row is built, starting over
  fn pend(&mut self, index: Option<usize>) {
    self.pending_focus = index;
    self.focus_ids.clear();
    self.focus_asked = false;
  }
}

/// Where the list `list` keeps its [`ListState`]
pub(crate) fn state_id(list: UiId) -> UiId {
  list.derived(STATE_SALT)
}

/// The rows to build: those covering the view shown from `offset` (the
/// offset the frame is drawn at, in the rows' own units), widened by
/// `overscan` rows each side and kept to `0..rows`. However far the area
/// is heading, a frame builds a view's worth.
pub(crate) fn built_rows(
  rows: usize,
  stride: f32,
  view: f32,
  offset: f32,
  overscan: usize,
) -> Range<usize> {
  if rows == 0 || !stride.is_finite() || stride <= 0.0 {
    return 0..0;
  }
  let clean = |v: f32| {
    if v.is_finite() {
      f64::from(v).max(0.0)
    } else {
      0.0
    }
  };
  let (stride, offset) = (f64::from(stride), clean(offset));
  // Float-to-integer casts saturate, so a far offset stays a far row
  let first = (offset / stride).floor() as usize;
  let end = ((offset + clean(view)) / stride).ceil() as usize;
  let first = first.saturating_sub(overscan).min(rows);
  let end = end.saturating_add(overscan).min(rows).max(first);
  first..end
}

/// A list being set up: `.show(|ui, index| ..)` builds the rows in view
#[must_use = "call .show(|ui, i| ..) to draw the list"]
pub struct List<'u, 'a> {
  scroll: Scroll<'u, 'a>,
  count: usize,
  row_height: f32,
  columns: u16,
  gap: f32,
  overscan: usize,
  /// The scroll area's top padding
  top: f32,
}

/// What a list did this frame
#[derive(Debug, Clone)]
pub struct ListResponse {
  /// The list's id, for [`list_scroll_to`](UiContext::list_scroll_to) and
  /// the scroll area calls ([`scroll_offset`](UiContext::scroll_offset))
  pub id: UiId,
  /// The items built for the view this frame: the rows in view and the
  /// overscan either side (not a focused row kept built out of view)
  pub range: Range<usize>,
  /// The scroll area's response
  pub response: Response,
}

impl<'a> UiContext<'a> {
  /// A list of `count` items in rows of one height, in a scroll area as
  /// long as the whole list. Only the rows in view are built, and a few
  /// either side ([`overscan`](List::overscan)), so a list of 10,000 costs
  /// what a screenful does. [`row_height`](List::row_height) is required;
  /// [`columns`](List::columns) lays the items out as a grid.
  ///
  /// Each item is built in a container keyed by its index, so its widgets
  /// keep their ids, and with them their hover, focus and memory, wherever
  /// it scrolls. The row with the keyboard or gamepad focus stays built
  /// (clipped) when the wheel or the scrollbar takes it out of view, so it
  /// keeps the focus and the pad steps on from it. What a row's widgets
  /// remember (a caret, an open dropdown) is dropped when the row leaves
  /// the built rows: keep what must last in the game's data.
  ///
  /// ```ignore
  /// let list = ui.list(scores.len()).row_height(28.0).gap(2.0).fill().show(|ui, i| {
  ///     ui.row().gap(8.0).show(|ui| {
  ///         ui.text(format!("{}", i + 1));
  ///         ui.text(scores[i].name.as_str());
  ///     });
  /// });
  /// // list.range: the items built this frame
  /// ```
  #[track_caller]
  pub fn list<'u>(&'u mut self, count: usize) -> List<'u, 'a> {
    List {
      scroll: self.scroll(),
      count,
      row_height: 0.0,
      columns: 1,
      gap: 0.0,
      overscan: OVERSCAN,
      top: 0.0,
    }
  }

  /// Scroll the list `list` (its [`ListResponse::id`]) by the least that
  /// shows item `index`'s row. A jump further than the view lands one view
  /// short at the list's next draw and eases the rest, so it never sweeps
  /// through the rows between. An index past the end is the last item; a
  /// list not drawn yet ignores it.
  pub fn list_scroll_to(&mut self, list: UiId, index: usize) {
    let Some(target) = row_target(self.memory, list, index) else {
      return;
    };
    let (scene, _) = self.scene();
    if let Some(state) = self.memory.state::<ScrollState>(list, scene) {
      state.target.y = target;
    }
    // The offset moves when the list next builds, never between its build
    // and its layout (a call after the list in the same container)
    if let Some(state) = self.memory.state::<ListState>(state_id(list), scene) {
      state.jump = true;
    }
  }

  /// Scroll the list `list` to item `index`, as
  /// [`list_scroll_to`](Self::list_scroll_to), and give the first focusable
  /// widget in its row the keyboard and gamepad focus. The row is built
  /// wherever the list is heading, and the focus lands up to two frames
  /// after the row is first built. An index past the end is the last item;
  /// a list not drawn yet, or empty, ignores it.
  ///
  /// ```ignore
  /// let list = ui.list(names.len()).row_height(28.0).fill().show(|ui, i| {
  ///     ui.button(names[i].as_str()).id(i);
  /// });
  /// if ui.button("Back to the top").clicked() {
  ///     ui.list_focus(list.id, 0);
  /// }
  /// ```
  pub fn list_focus(&mut self, list: UiId, index: usize) {
    let Some(last) = self
      .memory
      .peek_state::<ListState>(state_id(list))
      .and_then(|state| state.count.checked_sub(1))
    else {
      return;
    };
    let index = index.min(last);
    self.list_scroll_to(list, index);
    let (scene, _) = self.scene();
    if let Some(state) = self.memory.state::<ListState>(state_id(list), scene)
      // Asked again for the same item, it keeps its progress
      && state.pending_focus != Some(index)
    {
      state.pend(Some(index));
    }
  }
}

/// Where the list `list`'s scroll area should head to show item `index`'s
/// row, moving least; `None` when it shows already or the list was never
/// drawn
fn row_target(memory: &UiMemory, list: UiId, index: usize) -> Option<f32> {
  let shape = memory.peek_state::<ListState>(state_id(list))?;
  let area = memory.peek_state::<ScrollState>(list)?;
  let last = shape.count.checked_sub(1)?;
  let row = index.min(last) / usize::from(shape.columns.max(1));
  let top = f64::from(shape.top) + row as f64 * f64::from(shape.stride);
  let bottom = top + f64::from(shape.row_height);
  let (shown, view) = (f64::from(area.offset.y), f64::from(area.view.y));
  let wanted = if top < shown {
    top
  } else if bottom > shown + view {
    bottom - view
  } else {
    return None;
  };
  Some(wanted.min(f64::from(area.max().y)).max(0.0) as f32)
}

impl List<'_, '_> {
  /// Each row's height in UI units (required: above 0)
  pub fn row_height(mut self, height: f32) -> Self {
    self.row_height = height;
    self
  }

  /// Items side by side in each row, as a grid of equal cells (0 is 1)
  pub fn columns(mut self, columns: u16) -> Self {
    self.columns = columns;
    self
  }

  /// Room between rows, and between a grid's cells
  pub fn gap(mut self, gap: f32) -> Self {
    self.gap = gap;
    self
  }

  /// Rows built past each edge of the view (default 2, at least 1), so
  /// the pad can always step onto the next row
  pub fn overscan(mut self, rows: usize) -> Self {
    self.overscan = rows;
    self
  }

  /// Its id from `key` instead of how often this line ran
  pub fn id(mut self, key: impl Hash) -> Self {
    self.scroll = self.scroll.id(key);
    self
  }

  /// Its width: UI units (`120.0`), a fraction of its parent
  /// (`Length::Pct(0.5)`), `Length::Fill` or `Length::Fit`
  pub fn width(mut self, w: impl Into<Length>) -> Self {
    self.scroll = self.scroll.width(w);
    self
  }

  /// Its height, as [`width`](Self::width)
  pub fn height(mut self, h: impl Into<Length>) -> Self {
    self.scroll = self.scroll.height(h);
    self
  }

  /// Take the width its parent has left
  pub fn fill_width(mut self) -> Self {
    self.scroll = self.scroll.fill_width();
    self
  }

  /// Take the height its parent has left
  pub fn fill_height(mut self) -> Self {
    self.scroll = self.scroll.fill_height();
    self
  }

  /// Take the width and height its parent has left
  pub fn fill(mut self) -> Self {
    self.scroll = self.scroll.fill();
    self
  }

  /// At least this wide
  pub fn min_width(mut self, w: f32) -> Self {
    self.scroll = self.scroll.min_width(w);
    self
  }

  /// At most this wide
  pub fn max_width(mut self, w: f32) -> Self {
    self.scroll = self.scroll.max_width(w);
    self
  }

  /// At least this tall
  pub fn min_height(mut self, h: f32) -> Self {
    self.scroll = self.scroll.min_height(h);
    self
  }

  /// At most this tall
  pub fn max_height(mut self, h: f32) -> Self {
    self.scroll = self.scroll.max_height(h);
    self
  }

  /// Room between its edges and its rows
  pub fn padding(mut self, p: impl Into<Edges>) -> Self {
    let p = p.into();
    self.top = if p.top.is_finite() {
      p.top.max(0.0)
    } else {
      0.0
    };
    self.scroll = self.scroll.padding(p);
    self
  }

  /// Room around it, outside its edges
  pub fn margin(mut self, m: impl Into<Edges>) -> Self {
    self.scroll = self.scroll.margin(m);
    self
  }

  /// How much of the room its parent has left it takes, shared by weight
  pub fn grow(mut self, g: f32) -> Self {
    self.scroll = self.scroll.grow(g);
    self
  }

  /// Where it sits in a stack, or in the UI at the top level
  pub fn anchor(mut self, a: Anchor) -> Self {
    self.scroll = self.scroll.anchor(a);
    self
  }

  /// Moved by this much from where its anchor puts it
  pub fn offset(mut self, x: f32, y: f32) -> Self {
    self.scroll = self.scroll.offset(x, y);
    self
  }

  /// Placed at `rect`, its size too, out of the layout (inside a container too)
  pub fn at(mut self, rect: Rect) -> Self {
    self.scroll = self.scroll.at(rect);
    self
  }

  /// No scrollbar
  pub fn no_bar(mut self) -> Self {
    self.scroll = self.scroll.no_bar();
    self
  }
}

/// A list's measures for one frame, cleaned
#[derive(Clone, Copy)]
struct Shape {
  count: usize,
  row_height: f32,
  /// At least 1
  columns: u16,
  gap: f32,
  overscan: usize,
  top: f32,
}

impl Shape {
  /// Rows of `columns` items, the last one perhaps short
  fn rows(&self) -> usize {
    self.count.div_ceil(usize::from(self.columns))
  }

  /// The whole list's height: every row and the gaps between
  fn length(&self) -> f32 {
    let rows = self.rows() as f64;
    let length = rows * f64::from(self.row_height + self.gap) - f64::from(self.gap);
    to_units(length)
  }
}

/// A length in `f64` as the layout takes it: finite, never negative
fn to_units(length: f64) -> f32 {
  length.clamp(0.0, f64::from(f32::MAX)) as f32
}

impl<'a> List<'_, 'a> {
  /// Draw the list, calling `row(ui, index)` for each item built this frame
  /// inside its own container. The [`ListResponse`] names the list and the
  /// items built.
  pub fn show(self, mut row: impl FnMut(&mut UiContext<'_>, usize)) -> ListResponse {
    let List {
      scroll,
      count,
      row_height,
      columns,
      gap,
      overscan,
      top,
    } = self;
    // The layout cleans the gap it is given the same way, and warns
    let clean_gap = if gap.is_finite() { gap.max(0.0) } else { 0.0 };
    let stride = row_height + clean_gap;
    let sized = row_height.is_finite() && row_height > 0.0 && stride.is_finite();
    if !sized {
      warn_once("ui-list-row-height".into(), || {
        format!(
          "a list's row height must be finite and above 0, and its row height and gap finite together (got {row_height} and {gap}); drawn empty"
        )
      });
    }
    let shape = Shape {
      count,
      row_height,
      columns: columns.max(1),
      gap: clean_gap,
      overscan: overscan.max(1),
      top,
    };
    let mut range = 0..0;
    let mut scroll = scroll.gap(gap);
    if sized {
      scroll = scroll.content_height(shape.length());
    }
    let response = scroll.show(|ui| {
      if sized {
        range = build(ui, shape, &mut row);
      }
    });
    ListResponse {
      id: response.id,
      range,
      response,
    }
  }
}

/// Build the rows in view between their spacers, in the list's scroll area
/// (the current scope), and the rows the focus needs wherever they are; the
/// items built for the view
fn build<'a>(
  ui: &mut UiContext<'a>,
  shape: Shape,
  row: &mut impl FnMut(&mut UiContext<'_>, usize),
) -> Range<usize> {
  let list = ui.memory.scopes.current();
  let (scene, _) = ui.scene();
  let Shape {
    count,
    row_height,
    columns,
    gap,
    overscan,
    top,
  } = shape;
  let per_row = usize::from(columns);
  let rows = shape.rows();
  let stride = row_height + gap;
  let view = ui
    .memory
    .peek_state::<ScrollState>(list)
    .map_or(0.0, |s| s.view.y);
  // Before its first layout the list has no view: the screen's stands in
  let view = if view > 0.0 { view } else { ui.ui_size().y };
  let jump = ui
    .memory
    .peek_state::<ListState>(state_id(list))
    .is_some_and(|s| s.jump);
  // The area has eased this frame's offset already; a jump moves it on
  // before the rows are chosen, so the frame draws at the offset built for
  let mut offset = 0.0;
  if let Some(state) = ui.memory.state::<ScrollState>(list, scene) {
    if jump {
      state.approach(1, view);
    }
    offset = state.offset.y;
  }
  let built = built_rows(rows, stride, view, offset - top, overscan);
  // The focus this frame's resolve left, unless it was made before the list
  let focus = ui
    .focused()
    .filter(|id| !ui.memory.scopes.made(*id))
    .unwrap_or(UiId::NONE);
  let plan = plan_focus(ui, list, count, focus);
  if let Some(state) = ui.memory.state::<ListState>(state_id(list), scene) {
    state.stride = stride;
    state.row_height = row_height;
    state.top = top;
    state.rows = rows;
    state.columns = columns;
    state.count = count;
    state.jump = false;
  }

  // The focused item's row and one either side, so the pad can step off it
  // in either direction wherever it has scrolled to; the pending item's row
  let row_of = |index: usize| index / per_row;
  let focused = plan.focused.map_or(0..0, |i| {
    row_of(i).saturating_sub(1)..row_of(i).saturating_add(2).min(rows)
  });
  let pending = plan
    .pending
    .map_or(0..0, |i| row_of(i)..row_of(i).saturating_add(1).min(rows));
  let mut watch = Watch {
    focus,
    found: None,
    capture: plan.capture,
    ids: plan.ids,
  };
  // The area's gap is the list's, so a spacer is a gap short of the rows it
  // stands for
  let spacer = |ui: &mut UiContext<'_>, salt: u64, standing_for: usize| {
    let height = standing_for as f64 * f64::from(stride) - f64::from(gap);
    blank(ui, list.derived(salt), height_style(height));
  };
  let mut next = 0;
  for (k, run) in runs([built.clone(), focused, pending]).iter().enumerate() {
    if run.is_empty() {
      continue;
    }
    if run.start > next {
      let salt = if k == 0 {
        LEAD_SALT
      } else {
        GAP_SALT ^ k as u64
      };
      spacer(ui, salt, run.start - next);
    }
    for r in run.clone() {
      build_row(ui, shape, r, &mut watch, row);
    }
    next = run.end;
  }
  if next < rows {
    spacer(ui, TRAIL_SALT, rows - next);
  }

  if let Some(state) = ui.memory.state::<ListState>(state_id(list), scene) {
    state.focused = watch.found;
    state.focus_widget = if watch.found.is_some() {
      focus
    } else {
      UiId::NONE
    };
    // Unless a row asked for another item meanwhile (`list_focus` there)
    let same = state.pending_focus == plan.pending;
    if plan.capture.is_some() && same && !state.focus_asked {
      state.focus_ids = watch.ids;
    } else if plan.landing && same && state.focus_asked {
      state.pend(None);
    }
  }
  built.start.saturating_mul(per_row)..built.end.saturating_mul(per_row).min(count)
}

/// What a list's build does for the focus this frame
#[derive(Default)]
struct FocusPlan {
  /// The item whose row held the focus last frame, still in the list
  focused: Option<usize>,
  /// The item to give the focus, whose row is built
  pending: Option<usize>,
  /// The pending item, when its ids are to be captured this frame
  capture: Option<usize>,
  /// The focus was asked for last frame: this build ends the pending focus
  landing: bool,
  /// The buffer the capture fills (the state's own, so it keeps its room)
  ids: Vec<UiId>,
}

/// Bring the list's focus state in line with this frame's `count` and move
/// a pending focus on a step: its row's ids are captured, then the first of
/// them that can take the focus is asked for, then it ends
fn plan_focus(ui: &mut UiContext<'_>, list: UiId, count: usize, focus: UiId) -> FocusPlan {
  let (scene, _) = ui.scene();
  let Some(state) = ui.memory.state::<ListState>(state_id(list), scene) else {
    return FocusPlan::default();
  };
  // The count shrank under the focused item: the focus moves to the new
  // last item if it is still on the widget that item made, else it goes
  let gone = state.focused.is_some_and(|i| i >= count);
  if gone {
    state.focused = None;
  }
  if state.pending_focus.is_some_and(|i| i >= count) {
    state.pend(count.checked_sub(1));
  }
  if gone && state.pending_focus.is_none() && count > 0 && focus == state.focus_widget {
    state.pend(Some(count - 1));
  }
  let pending = state.pending_focus;
  let asked = state.focus_asked;
  let ids = std::mem::take(&mut state.focus_ids);
  let mut plan = FocusPlan {
    focused: state.focused,
    pending,
    capture: pending.filter(|_| !asked && ids.is_empty()),
    landing: pending.is_some() && asked,
    ids: Vec::new(),
  };
  if plan.capture.is_some() {
    plan.ids = ids;
    return plan;
  }
  if pending.is_none() || asked {
    state.focus_ids = ids;
    return plan;
  }
  // The row was built last frame: its first widget that can take the focus
  let target = ids.iter().copied().find(|id| {
    ui.memory
      .prev(*id)
      .is_some_and(|r| r.flags.contains(Flags::FOCUSABLE) && !r.flags.contains(Flags::DISABLED))
  });
  if let Some(id) = target {
    ui.request_focus(id);
  }
  if let Some(state) = ui.memory.state::<ListState>(state_id(list), scene) {
    state.focus_ids = ids;
    if target.is_some() {
      state.focus_asked = true;
    } else {
      // Nothing in the row takes the focus
      state.pend(None);
      plan.pending = None;
    }
  }
  plan
}

/// Up to three runs of rows, in order, merged where they touch; the unused
/// ones are empty
fn runs(mut ranges: [Range<usize>; 3]) -> [Range<usize>; 3] {
  ranges.sort_unstable_by_key(|r| r.start);
  let mut out = [0..0, 0..0, 0..0];
  let mut n: usize = 0;
  for r in ranges.into_iter().filter(|r| !r.is_empty()) {
    match n.checked_sub(1).and_then(|last| out.get_mut(last)) {
      Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
      _ => {
        if let Some(slot) = out.get_mut(n) {
          *slot = r;
          n += 1;
        }
      }
    }
  }
  out
}

/// What a list watches for while it builds its items
struct Watch {
  /// The focused widget, when it was not made before the list
  focus: UiId,
  /// The item that made it
  found: Option<usize>,
  /// The item whose ids are captured, into `ids`
  capture: Option<usize>,
  ids: Vec<UiId>,
}

/// Build row `r`: one item's container, or a grid row of cells
fn build_row<'a>(
  ui: &mut UiContext<'a>,
  shape: Shape,
  r: usize,
  watch: &mut Watch,
  row: &mut impl FnMut(&mut UiContext<'_>, usize),
) {
  let per_row = usize::from(shape.columns);
  if per_row == 1 {
    item(ui, r, item_style(shape.row_height), watch, row);
    return;
  }
  let style = UiStyle {
    flow: Flow::Row,
    gap: shape.gap,
    ..item_style(shape.row_height)
  };
  cell(ui, row_site(), r, style, |ui| {
    // No floor of their contents' width: the cells stay equal
    let cells = UiStyle {
      width: Length::Fill,
      min_width: Some(0.0),
      ..UiStyle::default()
    };
    for c in 0..per_row {
      let index = r.saturating_mul(per_row).saturating_add(c);
      if index < shape.count {
        item(ui, index, cells.clone(), watch, row);
      } else {
        // The last row's missing items keep the cells equal
        let id = ui.memory.scopes.current().derived(BLANK_SALT ^ c as u64);
        blank(ui, id, cells.clone());
      }
    }
  });
}

/// Build item `index` in its container, noting whether it made the focused
/// widget, and capturing its ids when it is the one to focus
fn item<'a>(
  ui: &mut UiContext<'a>,
  index: usize,
  style: UiStyle,
  watch: &mut Watch,
  row: &mut impl FnMut(&mut UiContext<'_>, usize),
) {
  let mark = (watch.capture == Some(index)).then(|| ui.memory.scopes.start_capture());
  cell(ui, item_site(), index, style, |ui| row(ui, index));
  if let Some(mark) = mark {
    ui.memory.scopes.take_capture(mark, &mut watch.ids);
  }
  if watch.found.is_none() && watch.focus != UiId::NONE && ui.memory.scopes.made(watch.focus) {
    watch.found = Some(index);
  }
}

/// A built row's container: the whole width, a row tall
fn item_style(row_height: f32) -> UiStyle {
  UiStyle {
    width: Length::Fill,
    height: Length::Px(row_height),
    ..UiStyle::default()
  }
}

/// A spacer's style: `height` tall
fn height_style(height: f64) -> UiStyle {
  UiStyle {
    height: Length::Px(to_units(height)),
    ..UiStyle::default()
  }
}

/// Where items' containers are made: one site, so an item's id is its
/// index under its parent
fn item_site() -> &'static Location<'static> {
  Location::caller()
}

/// Where a grid's row containers are made
fn row_site() -> &'static Location<'static> {
  Location::caller()
}

/// A container keyed `key` at `site` in the current scope, recorded as a
/// plain column is, with `contents` built in its scope
fn cell<'a>(
  ui: &mut UiContext<'a>,
  site: &'static Location<'static>,
  key: usize,
  style: UiStyle,
  contents: impl FnOnce(&mut UiContext<'a>),
) {
  let id = ui.memory.scopes.make(site, Some(key_hash(&key)));
  let node = ui.open_node(id, style, None, Flags::default());
  ui.memory.scopes.push(id);
  contents(ui);
  ui.memory.scopes.pop();
  ui.close_node(node);
}

/// Empty room of `style` that records nothing: no hit, hover or focus
fn blank(ui: &mut UiContext<'_>, id: UiId, style: UiStyle) {
  let node = ui.push_node(Node::new(id, style, Measure::None), None);
  ui.close_node(node);
}

/// The id of item `index`'s container in the list `list` of `columns`
#[cfg(test)]
pub(crate) fn item_id(list: UiId, columns: u16, index: usize) -> UiId {
  let keyed = |parent: UiId, site: &'static Location<'static>, key: usize| {
    let mut scopes = IdScopes::default();
    scopes.enter_root(parent);
    scopes.make(site, Some(key_hash(&key)))
  };
  let columns = usize::from(columns.max(1));
  if columns == 1 {
    return keyed(list, item_site(), index);
  }
  keyed(keyed(list, row_site(), index / columns), item_site(), index)
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::prelude::*;

  fn board(h: &mut UiHarness, count: usize, columns: u16) -> ListResponse {
    h.frame(|ui| {
      ui.list(count)
        .row_height(20.0)
        .gap(2.0)
        .columns(columns)
        .height(200.0)
        .width(300.0)
        .show(|ui, i| {
          ui.button(format!("Item {i}")).id(i).fill_width();
        })
    })
  }

  #[test]
  fn the_built_rows_cover_the_view_and_the_overscan() {
    assert_eq!(built_rows(1000, 22.0, 200.0, 0.0, 2), 0..12); // 200/22 = 9.1 -> 10 rows, + 2
    assert_eq!(built_rows(1000, 22.0, 200.0, 2200.0, 2), 98..112);
    assert_eq!(built_rows(1000, 22.0, 200.0, 21_800.0, 2), 988..1000);
    assert_eq!(built_rows(0, 22.0, 200.0, 0.0, 2), 0..0);
  }

  #[test]
  fn only_the_rows_in_view_are_built_and_rows_sit_at_their_place() {
    let mut h = UiHarness::new(640.0, 480.0);
    let first = board(&mut h, 10_000, 1);
    let built = board(&mut h, 10_000, 1);
    assert!(
      built.range.end - built.range.start < 20,
      "{:?}",
      built.range
    );
    assert_eq!(built.range.start, 0);
    h.frame(|ui| ui.list_scroll_to(first.id, 5000));
    for _ in 0..60 {
      board(&mut h, 10_000, 1);
    }
    let middle = board(&mut h, 10_000, 1);
    assert!(middle.range.contains(&5000), "{:?}", middle.range);
    let row = h
      .record_of_key(middle.id, 5000)
      .expect("item 5000 is drawn");
    let offset = h.scroll_offset(first.id).y;
    assert!(
      (row.rect.y - (5000.0 * 22.0 - offset)).abs() < 1.0,
      "row 5000 sits at its place"
    );
  }

  #[test]
  fn a_far_jump_builds_a_view_a_frame_and_lands_in_view() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 10_000, 1);
    board(&mut h, 10_000, 1);
    h.frame(|ui| ui.list_scroll_to(list.id, 5000));
    let views = (3.0 * 200.0 / 22.0) as usize;
    for frame in 0..60 {
      let r = board(&mut h, 10_000, 1);
      assert!(r.range.len() < views, "frame {frame}: {:?}", r.range);
      assert!(
        r.range.start > 4900,
        "frame {frame} swept through {:?}",
        r.range
      );
      // The rows built cover the offset the frame was drawn at
      let offset = h.scroll_offset(list.id).y;
      let (top, bottom) = (r.range.start as f32 * 22.0, r.range.end as f32 * 22.0);
      assert!(
        top <= offset && offset + 200.0 <= bottom,
        "frame {frame}: {offset} in {:?}",
        r.range
      );
    }
    let row = h
      .record_of_key(list.id, 5000)
      .expect("item 5000 is drawn")
      .rect;
    assert!(
      row.y >= 0.0 && row.y + row.height <= 200.0,
      "item 5000 in view: {row:?}"
    );
  }

  #[test]
  fn a_jump_to_the_end_of_a_huge_list_settles() {
    let mut h = UiHarness::new(640.0, 480.0);
    let huge = |h: &mut UiHarness| {
      h.frame(|ui| {
        ui.list(usize::MAX)
          .row_height(20.0)
          .height(200.0)
          .show(|ui, i| {
            ui.text(format!("{i}"));
          })
      })
    };
    let list = huge(&mut h);
    huge(&mut h);
    h.frame(|ui| ui.list_scroll_to(list.id, usize::MAX));
    for _ in 0..30 {
      huge(&mut h);
    }
    let end = huge(&mut h);
    let state = *h.memory.peek_state::<ScrollState>(list.id).expect("drawn");
    assert!(
      state.offset.y.is_finite() && state.offset.y > 1e20,
      "{state:?}"
    );
    assert_eq!(state.offset.y, state.target.y, "the ease has arrived");
    assert_eq!(end.range.end, usize::MAX, "the last items are built");
    assert!(end.range.len() < 20, "{:?}", end.range);
  }

  #[test]
  fn the_content_length_is_exact() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 100, 1);
    board(&mut h, 100, 1);
    assert_eq!(h.scroll_content(list.id).y, 100.0 * 22.0 - 2.0);
    // One call site, so one list across the two frames
    let grid = |h: &mut UiHarness| {
      h.frame(|ui| {
        ui.list(10)
          .row_height(20.0)
          .gap(2.0)
          .columns(3)
          .height(200.0)
          .width(300.0)
          .id("grid")
          .show(|ui, i| {
            ui.button(format!("{i}")).id(i);
          })
      })
    };
    let first = grid(&mut h);
    grid(&mut h);
    assert_eq!(
      h.scroll_content(first.id).y,
      4.0 * 22.0 - 2.0,
      "10 items in 3 columns are 4 rows"
    );
  }

  #[test]
  fn rows_taller_than_their_height_leave_the_length_exact() {
    // A button is at least 28 tall, so the board's rows of 20 overflow
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 100, 1);
    board(&mut h, 100, 1);
    h.frame(|ui| ui.list_scroll_to(list.id, 99));
    for _ in 0..60 {
      board(&mut h, 100, 1);
    }
    let end = board(&mut h, 100, 1);
    assert_eq!(end.range.end, 100, "the last row is built");
    assert_eq!(h.scroll_content(list.id).y, 100.0 * 22.0 - 2.0);
    assert_eq!(h.scroll_offset(list.id).y, 100.0 * 22.0 - 2.0 - 200.0);
  }

  #[test]
  fn an_item_keeps_its_id_wherever_it_is_built() {
    let mut h = UiHarness::new(640.0, 480.0);
    // One call site, so one list: the one scrolled is the one drawn
    let id_at = |h: &mut UiHarness| {
      let mut id = None;
      let list = h.frame(|ui| {
        ui.list(1000)
          .row_height(20.0)
          .gap(2.0)
          .height(200.0)
          .width(300.0)
          .show(|ui, i| {
            let r = ui.button(format!("Item {i}")).id(i).show();
            if i == 8 {
              id = Some(r.id);
            }
          })
      });
      (list, id)
    };
    let (list, before) = id_at(&mut h);
    // Far enough that the rows before item 8 are not all built
    h.frame(|ui| ui.set_scroll_offset(list.id, Vec2::new(0.0, 120.0)));
    let (scrolled, after) = id_at(&mut h);
    assert!(scrolled.range.start > 0, "{:?}", scrolled.range);
    assert!(
      before.is_some() && before == after,
      "item 8 has one id at any scroll position"
    );
  }

  #[test]
  fn hostile_sizes_warn_and_draw_nothing() {
    let mut h = UiHarness::new(640.0, 480.0);
    for height in [0.0, -5.0, f32::NAN, f32::INFINITY] {
      let r = h.frame(|ui| {
        ui.list(10).row_height(height).show(|ui, i| {
          ui.text(format!("{i}"));
        })
      });
      assert_eq!(r.range, 0..0);
    }
    // A row height and gap that overflow together are as bad
    let r = h.frame(|ui| {
      ui.list(10)
        .row_height(f32::MAX)
        .gap(f32::MAX)
        .show(|ui, i| {
          ui.text(format!("{i}"));
        })
    });
    assert_eq!(r.range, 0..0);
    assert_eq!(
      crate::core::ui::test_util::warnings_containing("row height"),
      1
    );
    let huge = |h: &mut UiHarness| {
      h.frame(|ui| {
        ui.list(usize::MAX)
          .row_height(20.0)
          .height(200.0)
          .show(|ui, i| {
            ui.text(format!("{i}"));
          })
      })
    };
    let first = huge(&mut h);
    assert!(first.range.end > 0);
    huge(&mut h);
    assert!(h.scroll_content(first.id).y.is_finite());
    let zero = |h: &mut UiHarness| {
      h.frame(|ui| {
        ui.list(5)
          .row_height(20.0)
          .columns(0)
          .height(200.0)
          .id("z")
          .show(|ui, i| {
            ui.text(format!("{i}"));
          })
      })
    };
    let first = zero(&mut h);
    assert_eq!(first.range, 0..5, "columns(0) is one column");
    zero(&mut h);
    assert_eq!(
      h.scroll_content(first.id).y,
      5.0 * 20.0,
      "five rows of one item"
    );
    let (a, b) = (
      h.record_of_key(first.id, 0).expect("built"),
      h.record_of_key(first.id, 1).expect("built"),
    );
    assert_eq!(
      (b.rect.x, b.rect.y),
      (a.rect.x, a.rect.y + 20.0),
      "item 1 is under item 0"
    );
  }

  #[test]
  fn an_empty_and_a_short_list_draw_without_scrolling() {
    let mut h = UiHarness::new(640.0, 480.0);
    let empty = h.frame(|ui| {
      ui.list(0)
        .row_height(20.0)
        .height(200.0)
        .id("e")
        .show(|_, _| {})
    });
    assert_eq!(empty.range, 0..0);
    let short = h.frame(|ui| {
      ui.list(3)
        .row_height(20.0)
        .height(200.0)
        .id("s")
        .show(|ui, i| {
          ui.text(format!("{i}"));
        })
    });
    assert_eq!(short.range, 0..3);
  }

  #[test]
  fn list_scroll_to_moves_by_the_least_amount() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 1000, 1);
    board(&mut h, 1000, 1);
    h.frame(|ui| ui.list_scroll_to(list.id, 100));
    for _ in 0..60 {
      board(&mut h, 1000, 1);
    }
    let offset = h.scroll_offset(list.id).y;
    assert!(
      (offset - (101.0 * 22.0 - 2.0 - 200.0)).abs() < 1.0,
      "row 100 at the bottom: {offset}"
    );
    h.frame(|ui| ui.list_scroll_to(list.id, 5_000_000));
    for _ in 0..60 {
      board(&mut h, 1000, 1);
    }
    let end = board(&mut h, 1000, 1);
    assert_eq!(end.range.end, 1000, "past the end clamps to the last item");
  }

  #[test]
  fn a_jump_after_the_list_in_its_container_draws_no_empty_frame() {
    let mut h = UiHarness::new(640.0, 480.0);
    let page = |h: &mut UiHarness, jump: bool| {
      h.frame(|ui| {
        let mut list = None;
        ui.row().show(|ui| {
          let drawn = ui
            .list(10_000)
            .row_height(20.0)
            .gap(2.0)
            .height(200.0)
            .width(300.0)
            .show(|ui, i| {
              ui.button(format!("Item {i}")).id(i).fill_width();
            });
          // A jump button beside the list, still inside the row
          if jump {
            ui.list_scroll_to(drawn.id, 5000);
          }
          list = Some(drawn);
        });
        list.expect("drawn")
      })
    };
    let list = page(&mut h, false);
    page(&mut h, false);
    let jumped = page(&mut h, true);
    let offset = h.scroll_offset(list.id).y;
    let (top, bottom) = (
      jumped.range.start as f32 * 22.0,
      jumped.range.end as f32 * 22.0,
    );
    assert!(
      top <= offset && offset + 200.0 <= bottom,
      "drawn at {offset}, built {:?}",
      jumped.range
    );
    let next = page(&mut h, false);
    assert!(
      next.range.start > 4900,
      "the next frame lands near: {:?}",
      next.range
    );
  }

  #[test]
  fn a_grid_fills_equal_cells_row_by_row() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 10, 3);
    board(&mut h, 10, 3);
    let cell = |i: usize| h.record_of_key(list.id, i).expect("built").rect;
    assert_eq!(cell(4).x, cell(1).x, "item 4 is under item 1");
    assert_eq!(cell(4).y, cell(1).y + 22.0, "a row and a gap down");
    assert!(cell(2).x > cell(1).x && cell(1).x > cell(0).x);
    assert_eq!(
      cell(9).width,
      cell(0).width,
      "the short last row keeps its cells' width"
    );
    assert_eq!(cell(9).height, 20.0);
  }

  #[test]
  fn a_wide_item_leaves_its_row_s_cells_equal() {
    let mut h = UiHarness::new(640.0, 480.0);
    let draw = |h: &mut UiHarness| {
      h.frame(|ui| {
        ui.list(6)
          .row_height(20.0)
          .gap(2.0)
          .columns(3)
          .height(200.0)
          .width(300.0)
          .show(|ui, i| {
            let label = if i == 1 {
              "An item with a label far wider than its cell".to_string()
            } else {
              format!("{i}")
            };
            ui.button(label).id(i);
          })
      })
    };
    let list = draw(&mut h);
    draw(&mut h);
    let width = |i: usize| h.record_of_key(list.id, i).expect("built").rect.width;
    // Snapped to pixels, cells of 98.67 are 98 or 99 wide
    for i in 1..6 {
      assert!(
        (width(i) - width(0)).abs() <= 1.0,
        "item {i}: {} against {}",
        width(i),
        width(0)
      );
    }
  }

  #[test]
  fn overscan_is_at_least_one_row() {
    let mut h = UiHarness::new(640.0, 480.0);
    let draw = |h: &mut UiHarness| {
      h.frame(|ui| {
        ui.list(1000)
          .row_height(20.0)
          .gap(2.0)
          .overscan(0)
          .height(200.0)
          .width(300.0)
          .show(|ui, i| {
            ui.text(format!("{i}"));
          })
      })
    };
    draw(&mut h);
    assert_eq!(
      draw(&mut h).range,
      0..11,
      "10 rows in view, and one past the edge"
    );
  }

  #[test]
  fn spacers_record_nothing() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 1000, 1);
    h.frame(|ui| ui.set_scroll_offset(list.id, Vec2::new(0.0, 5000.0)));
    let middle = board(&mut h, 1000, 1);
    assert!(
      middle.range.start > 0 && middle.range.end < 1000,
      "{:?}",
      middle.range
    );
    let inside = h
      .memory
      .drawn_records()
      .filter(|(_, r)| r.scroll == list.id)
      .count();
    assert_eq!(
      inside,
      2 * middle.range.len(),
      "each item's container and button, nothing else"
    );
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod focus_tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::prelude::*;

  fn scope_list(h: &mut UiHarness, count: usize) -> ListResponse {
    h.frame(|ui| {
      let mut out = None;
      ui.column().focus_scope().show(|ui| {
        out = Some(
          ui.list(count)
            .row_height(20.0)
            .height(100.0)
            .width(200.0)
            .id("l")
            .show(|ui, i| {
              let b = ui.button(format!("Item {i}")).id(i);
              if i == 0 {
                b.default_focus().show();
              } else {
                b.show();
              }
            }),
        );
      });
      out.unwrap()
    })
  }

  #[test]
  fn the_pad_walks_to_the_end_of_a_long_list() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = scope_list(&mut h, 300);
    scope_list(&mut h, 300);
    for _ in 0..299 {
      h.action("ui.down");
      scope_list(&mut h, 300);
      scope_list(&mut h, 300);
    }
    assert_eq!(h.list_state(list.id).unwrap().focused, Some(299));
    let max = h.scroll_content(list.id).y - 100.0;
    for _ in 0..60 {
      scope_list(&mut h, 300);
    }
    assert!(
      (h.scroll_offset(list.id).y - max).abs() < 1.0,
      "scrolled to the end"
    );
  }

  #[test]
  fn a_focused_row_scrolled_away_stays_built_and_keeps_the_focus() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = scope_list(&mut h, 300);
    scope_list(&mut h, 300);
    let focused = h.focused();
    assert!(focused.is_some());
    h.frame(|ui| ui.set_scroll_offset(list.id, Vec2::new(0.0, 3000.0)));
    for _ in 0..3 {
      scope_list(&mut h, 300);
    }
    assert_eq!(
      h.focused(),
      focused,
      "item 0 is still built, so it keeps the focus"
    );
    h.action("ui.down");
    scope_list(&mut h, 300);
    scope_list(&mut h, 300);
    assert_eq!(
      h.list_state(list.id).unwrap().focused,
      Some(1),
      "navigation resumes from item 0"
    );
  }

  #[test]
  fn a_focused_row_wheeled_thousands_of_rows_away_is_kept_and_the_pad_returns_to_it() {
    let mut h = UiHarness::new(640.0, 480.0);
    let draw = |h: &mut UiHarness| {
      h.frame(|ui| {
        let mut out = None;
        ui.column().focus_scope().show(|ui| {
          out = Some(
            ui.list(10_000)
              .row_height(20.0)
              .height(100.0)
              .width(200.0)
              .id("far")
              .show(|ui, i| {
                let b = ui.button(format!("Item {i}")).id(i);
                if i == 0 {
                  b.default_focus().show();
                } else {
                  b.show();
                }
              }),
          );
        });
        out.expect("drawn")
      })
    };
    // The items built: each is its container and its button
    let built = |h: &UiHarness, list: UiId| {
      let records = h.memory.drawn_records().filter(|(_, r)| r.scroll == list);
      records.count() / 2
    };
    let list = draw(&mut h);
    draw(&mut h);
    // The pointer rests on the list beside the buttons, so the focus never
    // follows it
    let rect = h.record_of(list.id).expect("drawn").rect;
    h.move_to(rect.x + 150.0, rect.y + 50.0);
    draw(&mut h);
    for _ in 0..2 {
      h.action("ui.down");
      draw(&mut h);
      draw(&mut h);
    }
    assert_eq!(h.list_state(list.id).expect("drawn").focused, Some(2));
    let focused = h.focused();
    // 2,000 lines of 40 units are 4,000 rows of 20
    h.wheel(0.0, -2000.0);
    for frame in 0..90 {
      let r = draw(&mut h);
      assert!(r.range.len() <= 10, "frame {frame}: {:?}", r.range);
      assert!(
        built(&h, list.id) <= r.range.len() + 3,
        "frame {frame}: the view's rows and the focused row with its neighbours"
      );
      assert_eq!(h.focused(), focused, "frame {frame}: the focus is kept");
    }
    let far = h.scroll_offset(list.id).y;
    assert!(far > 79_000.0, "wheeled far away: {far}");
    assert_eq!(h.list_state(list.id).expect("drawn").focused, Some(2));
    h.action("ui.down");
    for frame in 0..90 {
      let r = draw(&mut h);
      assert!(
        built(&h, list.id) <= r.range.len() + 3,
        "frame {frame} back: {:?}",
        r.range
      );
    }
    assert_eq!(
      h.list_state(list.id).expect("drawn").focused,
      Some(3),
      "the pad steps on from item 2"
    );
    let offset = h.scroll_offset(list.id).y;
    assert!(
      (offset - 60.0).abs() < 1.0,
      "the view came back to item 3: {offset}"
    );
  }

  #[test]
  fn list_focus_jumps_to_an_item_and_focuses_it() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = scope_list(&mut h, 300);
    scope_list(&mut h, 300);
    h.frame(|ui| ui.list_focus(list.id, 250));
    for _ in 0..4 {
      scope_list(&mut h, 300);
    }
    assert_eq!(h.list_state(list.id).unwrap().focused, Some(250));
  }

  #[test]
  fn a_count_that_shrinks_under_the_focus_moves_it_to_the_last_item() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = scope_list(&mut h, 300);
    scope_list(&mut h, 300);
    h.frame(|ui| ui.list_focus(list.id, 250));
    for _ in 0..4 {
      scope_list(&mut h, 300);
    }
    for _ in 0..4 {
      scope_list(&mut h, 100);
    }
    assert_eq!(h.list_state(list.id).unwrap().focused, Some(99));
  }

  #[test]
  fn a_list_in_a_modal_inside_a_scroll_area_takes_the_wheel_and_the_pad() {
    let mut h = UiHarness::new(640.0, 480.0);
    let draw = |h: &mut UiHarness| {
      h.frame(|ui| {
        let mut out = None;
        ui.scroll()
          .height(300.0)
          .width(400.0)
          .id("outer")
          .show(|ui| {
            ui.modal().show(|ui| {
              out = Some(
                ui.list(500)
                  .row_height(20.0)
                  .height(100.0)
                  .width(200.0)
                  .id("inner")
                  .show(|ui, i| {
                    let b = ui.button(format!("{i}")).id(i);
                    if i == 0 {
                      b.default_focus().show();
                    } else {
                      b.show();
                    }
                  }),
              );
            });
          });
        out.unwrap()
      })
    };
    let list = draw(&mut h);
    draw(&mut h);
    let rect = h.record_of(list.id).unwrap().rect;
    h.move_to(rect.x + 10.0, rect.y + 10.0);
    h.wheel(0.0, -3.0);
    for _ in 0..30 {
      draw(&mut h);
    }
    assert!(
      h.scroll_offset(list.id).y > 0.0,
      "the inner list took the wheel"
    );
    for _ in 0..10 {
      h.action("ui.down");
      draw(&mut h);
      draw(&mut h);
    }
    assert_eq!(h.list_state(list.id).unwrap().focused, Some(10));
  }
}
