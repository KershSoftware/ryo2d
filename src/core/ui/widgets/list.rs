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
/// The spacers before and after the built rows
const LEAD_SALT: u64 = 0x6c65_6164;
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
  /// The item whose row holds the focus
  #[expect(dead_code, reason = "kept for the list's focus handling")]
  pub(crate) focused: Option<usize>,
  /// An item to give the focus once its row is built
  #[expect(dead_code, reason = "kept for the list's focus handling")]
  pub(crate) pending_focus: Option<usize>,
  /// The ids made while building the pending item's row
  #[expect(dead_code, reason = "kept for the list's focus handling")]
  pub(crate) focus_ids: Vec<UiId>,
}

/// Where the list `list` keeps its [`ListState`]
pub(crate) fn state_id(list: UiId) -> UiId {
  list.derived(STATE_SALT)
}

/// The rows to build: those covering the view from wherever the area shows
/// to wherever it is heading (`from` and `to`, the offset and the target,
/// in the rows' own units), widened by `overscan` rows each side and kept
/// to `0..rows`
pub(crate) fn built_rows(
  rows: usize,
  stride: f32,
  view: f32,
  from: f32,
  to: f32,
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
  let stride = f64::from(stride);
  let (from, to) = (clean(from), clean(to));
  // Float-to-integer casts saturate, so a far offset stays a far row
  let first = (from.min(to) / stride).floor() as usize;
  let end = ((from.max(to) + clean(view)) / stride).ceil() as usize;
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
  /// The items built this frame
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
  /// it scrolls. What a row's widgets remember (a caret, an open dropdown)
  /// is dropped when the row leaves the built rows: keep what must last in
  /// the game's data.
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
  /// shows item `index`'s row, from the next frame. An index past the end
  /// is the last item; a list not drawn yet ignores it.
  pub fn list_scroll_to(&mut self, list: UiId, index: usize) {
    let Some(target) = row_target(self.memory, list, index) else {
      return;
    };
    let (scene, _) = self.scene();
    if let Some(state) = self.memory.state::<ScrollState>(list, scene) {
      state.target.y = target;
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
    let sized = row_height.is_finite() && row_height > 0.0;
    if !sized {
      warn_once("ui-list-row-height".into(), || {
        format!("a list's row height must be finite and above 0 (got {row_height}); drawn empty")
      });
    }
    let shape = Shape {
      count,
      row_height,
      columns: columns.max(1),
      // The layout cleans the gap it is given the same way, and warns
      gap: if gap.is_finite() { gap.max(0.0) } else { 0.0 },
      overscan: overscan.max(1),
      top,
    };
    let mut range = 0..0;
    let response = scroll.gap(gap).show(|ui| {
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
/// (the current scope); the items built
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
  let rows = count.div_ceil(per_row);
  let stride = row_height + gap;
  let (from, to, view) = ui
    .memory
    .peek_state::<ScrollState>(list)
    .map_or((0.0, 0.0, 0.0), |s| (s.offset.y, s.target.y, s.view.y));
  // Before its first layout the list has no view: the screen's stands in
  let view = if view > 0.0 { view } else { ui.ui_size().y };
  let built = built_rows(rows, stride, view, from - top, to - top, overscan);
  if let Some(state) = ui.memory.state::<ListState>(state_id(list), scene) {
    state.stride = stride;
    state.row_height = row_height;
    state.top = top;
    state.rows = rows;
    state.columns = columns;
    state.count = count;
  }

  // The area's gap is the list's, so a spacer is a gap short of the rows it
  // stands for
  let (stride64, gap64) = (f64::from(stride), f64::from(gap));
  if built.start > 0 {
    let height = built.start as f64 * stride64 - gap64;
    blank(ui, list.derived(LEAD_SALT), height_style(height));
  }
  for r in built.clone() {
    if per_row == 1 {
      cell(ui, item_site(), r, item_style(row_height), |ui| row(ui, r));
      continue;
    }
    let style = UiStyle {
      flow: Flow::Row,
      gap,
      ..item_style(row_height)
    };
    cell(ui, row_site(), r, style, |ui| {
      let cells = UiStyle {
        width: Length::Fill,
        ..UiStyle::default()
      };
      for c in 0..per_row {
        let index = r.saturating_mul(per_row).saturating_add(c);
        if index < count {
          cell(ui, item_site(), index, cells.clone(), |ui| row(ui, index));
        } else {
          // The last row's missing items keep the cells equal
          let id = ui.memory.scopes.current().derived(BLANK_SALT ^ c as u64);
          blank(ui, id, cells.clone());
        }
      }
    });
  }
  if built.end < rows {
    let height = (rows - built.end) as f64 * stride64 - gap64;
    blank(ui, list.derived(TRAIL_SALT), height_style(height));
  }
  built.start.saturating_mul(per_row)..built.end.saturating_mul(per_row).min(count)
}

/// A built row's container: the whole width, a row tall
fn item_style(row_height: f32) -> UiStyle {
  UiStyle {
    width: Length::Fill,
    height: Length::Px(row_height),
    ..UiStyle::default()
  }
}

/// A spacer's style: `height` tall, kept finite for the layout
fn height_style(height: f64) -> UiStyle {
  UiStyle {
    height: Length::Px(height.clamp(0.0, f64::from(f32::MAX)) as f32),
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

#[cfg(test)]
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
    assert_eq!(built_rows(1000, 22.0, 200.0, 0.0, 0.0, 2), 0..12); // 200/22 = 9.1 -> 10 rows, + 2
    assert_eq!(built_rows(1000, 22.0, 200.0, 2200.0, 2200.0, 2), 98..112);
    assert_eq!(
      built_rows(1000, 22.0, 200.0, 21_800.0, 21_800.0, 2),
      988..1000
    );
    assert_eq!(
      built_rows(1000, 22.0, 200.0, 0.0, 2200.0, 2),
      0..112,
      "spans offset to target"
    );
    assert_eq!(built_rows(0, 22.0, 200.0, 0.0, 0.0, 2), 0..0);
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
  fn the_content_length_is_exact() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 100, 1);
    board(&mut h, 100, 1);
    assert_eq!(h.scroll_content(list.id).y, 100.0 * 22.0 - 2.0);
    let grid = |h: &mut UiHarness| {
      h.frame(|ui| {
        // Shorter than its rows: an area's contents are never less than its view
        ui.list(10)
          .row_height(20.0)
          .gap(2.0)
          .columns(3)
          .height(50.0)
          .width(300.0)
          .id("grid")
          // A button is 28 tall at least; one taller than its row overflows it
          .show(|ui, i| {
            ui.button(format!("{i}")).id(i).height(20.0);
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
    let zero = h.frame(|ui| {
      ui.list(5)
        .row_height(20.0)
        .columns(0)
        .height(200.0)
        .id("z")
        .show(|ui, i| {
          ui.text(format!("{i}"));
        })
    });
    assert_eq!(zero.range, 0..5, "columns(0) is one column");
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
