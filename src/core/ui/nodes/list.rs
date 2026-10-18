//! The pooled node list: `UiNode::list(count, row_height)` keeps about a
//! screenful of row entities, each given an item by index, reused as the
//! list scrolls and filled by the game when a `UiListBind` names it
//!
//! `update_lists` brings each pool in line once a frame, before any tree is
//! drawn; the walk's `Kind::List` arm draws the pooled rows through the
//! closure list's placement (`widgets/list.rs`), so both kinds of list place
//! their rows alike.

use std::ops::Range;
use std::sync::Arc;

use glam::Vec2;

use crate::core::ecs::kernel::{Component, Entity, Messages, World};
use crate::core::messages::register_message;
use crate::core::ui::memory::UiMemory;
use crate::core::ui::theme::warn_once;
use crate::core::ui::widgets::list::{
  Fill, FocusWatch, Shape, built_rows, place, row_target_in, runs,
};
use crate::core::ui::widgets::scroll::{Scroll, ScrollState};
use crate::core::ui::{Response, UiContext, UiId};

use super::bindings;
use super::components::{UiInteraction, UiNodes};
use super::draw::{Walk, draw_child};
use super::events::UiListBind;
use super::node::{Kind, ListOpts, UiNode, UiNodeKind};
use super::spawn::spawn_list_row;

/// What a list's pooled rows are spawned from
#[derive(Clone)]
pub(crate) enum RowShape {
  /// `UiNode::list(..).row_shape(f)`
  Code(Arc<dyn Fn() -> UiNode + Send + Sync>),
}

impl RowShape {
  /// One row's tree
  fn build(&self) -> UiNode {
    match self {
      RowShape::Code(shape) => shape(),
    }
  }
}

/// A list node's row shape
#[derive(Component)]
pub(crate) struct UiListShape(pub(crate) RowShape);

/// A list node's pool: its item count and the row entities given items
#[derive(Component, Debug, Default)]
pub(crate) struct UiList {
  /// The items: state, as `UiValue` is (a reconcile writes it only when the
  /// tree's count changed)
  pub(crate) count: usize,
  /// (item, row) for each pooled row, by item
  pub(crate) rows: Vec<(usize, Entity)>,
  /// The item whose row held the focus when the list was last drawn: its
  /// row, and one either side, stay pooled
  pub(crate) focused: Option<usize>,
  /// An item to scroll to (and whether to focus it): its row is pooled and
  /// the list heads there at the next update
  pub(crate) jump: Option<(usize, bool)>,
}

/// A pooled row of a list node: which item it shows. Each row is its own
/// binding scope (`ctx.ui_set(row, ..)`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiListRow {
  pub(crate) index: usize,
  /// The frame the row was given its item: it is drawn from the next
  pub(crate) bound_frame: u64,
}

impl UiListRow {
  /// The index of the item the row shows
  pub fn index(&self) -> usize {
    self.index
  }
}

/// Bring every list node's pool in line, once a frame: after the pointer,
/// the wheel, the scroll requests and navigation are resolved and before any
/// node tree is drawn. A list keeps a row for each item it needs (the rows
/// in view at its scroll area's offset and at its target, every row between
/// when the two are within two views, the overscan, the focused item's row
/// and one either side, and a row being jumped to), reuses the rows of the
/// items it no longer needs for the ones that have none, and spawns or
/// despawns the difference. Each row given an item sends a `UiListBind`.
///
/// A world with no list is left exactly as it was: nothing is spawned,
/// inserted or registered.
pub(crate) fn update_lists(world: &mut World, memory: &mut UiMemory, frame: u64, screen: Vec2) {
  if world
    .get_resource::<UiNodes>()
    .is_none_or(|nodes| nodes.lists.is_empty())
  {
    return;
  }
  let mut lists = world
    .get_resource_mut::<UiNodes>()
    .map(|mut nodes| std::mem::take(&mut nodes.lists))
    .unwrap_or_default();
  lists.retain(|&list| world.get::<UiList>(list).is_some());
  let mut binds = Vec::new();
  for &list in &lists {
    update_list(world, memory, frame, screen, list, &mut binds);
  }
  // A row may hold a list of its own: one spawned now joins from next frame
  if let Some(mut nodes) = world.get_resource_mut::<UiNodes>() {
    let spawned = std::mem::replace(&mut nodes.lists, lists);
    nodes.lists.extend(spawned);
  }
  deliver(world, binds);
}

/// Bring the pool of `list` in line; each row given an item joins `binds`
fn update_list(
  world: &mut World,
  memory: &mut UiMemory,
  frame: u64,
  screen: Vec2,
  list: Entity,
  binds: &mut Vec<UiListBind>,
) {
  let Some(node) = world.get::<UiNodeKind>(list) else {
    return;
  };
  let Kind::List(opts) = node.kind else {
    return;
  };
  let gap = node.layout.gap.unwrap_or(0.0);
  let top = node.layout.padding.map_or(0.0, |padding| padding.top);
  let id = world
    .get::<UiInteraction>(list)
    .map_or(UiId::NONE, |state| state.id);
  let shaped = world.get::<UiListShape>(list).is_some();
  let Some((count, focused, jump)) = world
    .get::<UiList>(list)
    .map(|pool| (pool.count, pool.focused, pool.jump))
  else {
    return;
  };
  let shape = Shape::new(
    count,
    opts.row_height,
    opts.columns,
    gap,
    usize::from(opts.overscan),
    top,
  );
  // No row shape, or no size to place rows at: no rows
  let Some(shape) = shape.filter(|_| shaped) else {
    release(world, list);
    return;
  };

  let per_row = usize::from(shape.columns);
  let rows = shape.rows();
  let stride = shape.stride();
  let row_of = |index: usize| index / per_row;
  let jump_to = jump.and_then(|(index, _)| count.checked_sub(1).map(|last| index.min(last)));
  let mut aimed = false;
  let area = if id == UiId::NONE {
    None
  } else {
    memory.peek_state_mut::<ScrollState>(id)
  };
  // Before its first draw the list has no view: the screen's stands in
  let (offset, target, view) = match area {
    Some(area) => {
      let view = if area.view.y > 0.0 {
        area.view.y
      } else {
        screen.y
      };
      if let Some(index) = jump_to {
        // The least movement; a jump further than a view lands one view
        // short before the rows are chosen, and the area eases the rest
        if let Some(to) = row_target_in(area, shape.top, stride, shape.row_height, row_of(index)) {
          area.target.y = to;
          area.approach(1, view);
        }
        aimed = true;
      }
      (area.offset.y, area.target.y, view)
    }
    None => (0.0, 0.0, screen.y),
  };

  let at = built_rows(rows, stride, view, offset - shape.top, shape.overscan);
  let to = built_rows(rows, stride, view, target - shape.top, shape.overscan);
  // Within two views of each other, the rows the ease passes are kept too
  let near = (f64::from(target) - f64::from(offset)).abs() <= 2.0 * f64::from(view);
  let (at, to) = if near {
    (at.start.min(to.start)..at.end.max(to.end), 0..0)
  } else {
    (at, to)
  };
  // The focused item's row and one either side, so the pad steps off it
  let focus = focused
    .filter(|&index| index < count)
    .map_or(0..0, |index| {
      row_of(index).saturating_sub(1)..row_of(index).saturating_add(2).min(rows)
    });
  let jumped = jump_to.map_or(0..0, |index| {
    row_of(index)..row_of(index).saturating_add(1).min(rows)
  });
  let needed = runs([at, to, focus, jumped]).map(|run| shape.items(run));

  let Some(mut pool) = world.get_mut::<UiList>(list) else {
    return;
  };
  let mut held = std::mem::take(&mut pool.rows);
  // The jump's row is pooled below; a jump to nothing has nowhere to go
  if aimed || jump_to.is_none() {
    pool.jump = None;
  }
  // A row despawned elsewhere is gone from the pool
  held.retain(|&(_, row)| world.get::<UiListRow>(row).is_some());
  let mut free = Vec::new();
  held.retain(|&(index, row)| {
    let keep = needed.iter().any(|items| items.contains(&index));
    if !keep {
      free.push(row);
    }
    keep
  });
  for items in &needed {
    for index in items.clone() {
      let Err(at) = held.binary_search_by_key(&index, |&(item, _)| item) else {
        continue;
      };
      let row = match free.pop() {
        Some(row) => {
          if let Some(mut pooled) = world.get_mut::<UiListRow>(row) {
            *pooled = UiListRow {
              index,
              bound_frame: frame,
            };
          }
          row
        }
        None => {
          let Some(shape) = world.get::<UiListShape>(list).map(|shape| shape.0.clone()) else {
            continue;
          };
          spawn_list_row(world, list, shape.build(), index, frame)
        }
      };
      held.insert(at, (index, row));
      binds.push(UiListBind { list, row, index });
    }
  }
  for row in free {
    if world.get_entity(row).is_ok() {
      world.despawn(row);
    }
  }
  if let Some(mut pool) = world.get_mut::<UiList>(list) {
    pool.rows = held;
  }
}

/// Despawn every row of `list` and drop its jump, when it can have none
fn release(world: &mut World, list: Entity) {
  let pooled = world
    .get::<UiList>(list)
    .is_some_and(|pool| !pool.rows.is_empty() || pool.jump.is_some());
  if !pooled {
    return;
  }
  let Some(mut pool) = world.get_mut::<UiList>(list) else {
    return;
  };
  pool.jump = None;
  let rows = std::mem::take(&mut pool.rows);
  for (_, row) in rows {
    if world.get_entity(row).is_ok() {
      world.despawn(row);
    }
  }
}

/// Write `binds` as messages, in order, and trigger each on its row (a row
/// an earlier observer despawned gets its message and no trigger)
fn deliver(world: &mut World, binds: Vec<UiListBind>) {
  if binds.is_empty() {
    return;
  }
  register_message::<UiListBind>(world);
  if let Some(mut messages) = world.get_resource_mut::<Messages<UiListBind>>() {
    for bind in &binds {
      messages.write(*bind);
    }
  }
  for bind in binds {
    if world.get_entity(bind.row).is_ok() {
      world.trigger(bind);
    }
  }
}

/// Warn once that a list node has no row shape, so builds no rows
pub(crate) fn warn_shapeless() {
  warn_once("ui-list-no-row-shape".to_string(), || {
    "a list node has no row shape (UiNode::row_shape); it builds no rows".to_string()
  });
}

/// Draw the list node `node` in `scroll`, its scroll area configured from
/// the node: its pooled rows in item order, each in its item's container
/// keyed by the index, with spacers between, placed as a closure list
/// places its rows. A row given its item this frame is drawn from the next;
/// until then its container is drawn empty, at full size. The item whose
/// row holds the focus becomes the list's `focused`.
pub(super) fn draw_list(
  world: &mut World,
  node: Entity,
  scroll: Scroll<'_, '_>,
  opts: ListOpts,
  gap: f32,
  top: f32,
  walk: &mut Walk,
) -> Response {
  let count = world.get::<UiList>(node).map_or(0, |pool| pool.count);
  let shape = Shape::new(
    count,
    opts.row_height,
    opts.columns,
    gap,
    usize::from(opts.overscan),
    top,
  );
  // Taken while the rows draw, so the world is free for them
  let rows = world
    .get_mut::<UiList>(node)
    .map(|mut pool| std::mem::take(&mut pool.rows))
    .unwrap_or_default();
  let mut scroll = scroll;
  if let Some(shape) = shape {
    scroll = scroll.content_height(shape.length());
  }
  let mut found = None;
  let response = scroll.show(|ui| {
    let Some(shape) = shape else {
      return;
    };
    let list = ui.memory.scopes.current();
    let runs = PoolRuns {
      items: &rows,
      per_row: usize::from(shape.columns),
      rows: shape.rows(),
    };
    let mut pool = Pool {
      frame: ui.memory.frame(),
      watch: FocusWatch::new(ui),
      world,
      walk,
      rows: &rows,
    };
    place(ui, list, shape, runs, &mut pool);
    found = pool.watch.found;
  });
  if let Some(mut pool) = world.get_mut::<UiList>(node) {
    pool.rows = rows;
    if pool.focused != found {
      pool.focused = found;
    }
  }
  response
}

/// A list node's items as they are drawn: each pooled row given its item
/// before this frame, in that item's container
struct Pool<'p, 'w> {
  world: &'w mut World,
  walk: &'p mut Walk,
  /// (item, row), by item
  rows: &'p [(usize, Entity)],
  frame: u64,
  watch: FocusWatch,
}

impl Fill for Pool<'_, '_> {
  fn contents(&mut self, ui: &mut UiContext<'_>, index: usize) {
    let Some(&(_, row)) = self
      .rows
      .binary_search_by_key(&index, |&(item, _)| item)
      .ok()
      .and_then(|at| self.rows.get(at))
    else {
      return;
    };
    let bound = self
      .world
      .get::<UiListRow>(row)
      .is_some_and(|pooled| pooled.bound_frame < self.frame);
    if bound {
      // A bound widget the game changed sets its name in the row's scope
      bindings::sync_root(self.world, row);
      draw_child(self.world, ui, row, self.walk);
    }
  }

  fn closed(&mut self, ui: &mut UiContext<'_>, index: usize) {
    self.watch.note(ui, index);
  }
}

/// The runs of rows a pool's items (by item) are in, in order; items past
/// the list's rows are left out
struct PoolRuns<'p> {
  items: &'p [(usize, Entity)],
  /// At least 1
  per_row: usize,
  rows: usize,
}

impl Iterator for PoolRuns<'_> {
  type Item = Range<usize>;

  fn next(&mut self) -> Option<Range<usize>> {
    let (&(first, _), rest) = self.items.split_first()?;
    let start = first / self.per_row;
    if start >= self.rows {
      self.items = &[];
      return None;
    }
    self.items = rest;
    let mut end = start + 1;
    while let Some((&(index, _), rest)) = self.items.split_first() {
      let row = index / self.per_row;
      if row > end || row >= self.rows {
        break;
      }
      end = end.max(row + 1);
      self.items = rest;
    }
    Some(start..end)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::*;
  use crate::prelude::*;

  fn row() -> UiNode {
    UiNode::row().child(UiNode::text("{rank}. {name}").bound().name("line"))
  }

  fn board(h: &mut UiHarness, count: usize) -> Entity {
    h.spawn_ui(
      UiNode::list(count, 20.0)
        .gap(2.0)
        .height(200.0)
        .width(300.0)
        .row_shape(row)
        .name("board"),
    )
  }

  /// Bind every row the frame asked for, as a game would in its `ui`
  fn bind(h: &mut UiHarness) -> usize {
    let binds: Vec<UiListBind> = h.messages::<UiListBind>().collect();
    for b in &binds {
      h.ui_set(b.row, "rank", b.index + 1);
      h.ui_set(b.row, "name", format!("Player {}", b.index + 1));
    }
    binds.len()
  }

  fn line_of(h: &UiHarness, row: Entity) -> String {
    let line = crate::core::ui::nodes::find_ui_in(h.world(), row, "line").unwrap();
    h.world()
      .get::<UiLabel>(line)
      .map(|l| l.0.clone())
      .unwrap_or_default()
  }

  fn rows(h: &UiHarness, list: Entity) -> Vec<(usize, Entity)> {
    h.world().get::<UiList>(list).unwrap().rows.clone()
  }

  #[test]
  fn a_pool_holds_about_a_screenful_and_binds_each_row() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 10_000);
    h.frame(|_| {});
    let first = bind(&mut h);
    assert!(
      first > 0 && first < 40,
      "{first} rows for a 200-unit view (screen height before the first draw)"
    );
    h.frame(|_| {});
    bind(&mut h);
    h.frame(|_| {});
    let pool = rows(&h, list);
    assert!(pool.len() < 20, "{} rows pooled", pool.len());
    let (index, row) = pool[0];
    assert_eq!(
      line_of(&h, row),
      format!("{}. Player {}", index + 1, index + 1)
    );
  }

  #[test]
  fn scrolling_reuses_rows_and_binds_them_for_their_new_items() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 10_000);
    for _ in 0..3 {
      h.frame(|_| {});
      bind(&mut h);
    }
    let before: Vec<Entity> = rows(&h, list).iter().map(|r| r.1).collect();
    let id = h.ui_id(list).unwrap();
    h.frame(|ui| ui.set_scroll_offset(id, Vec2::new(0.0, 22.0 * 50.0)));
    // The scene's `ui` scrolled after this frame's pool update: the next
    // frame's binds the rows
    assert_eq!(bind(&mut h), 0);
    h.frame(|_| {});
    let rebound = bind(&mut h);
    assert!(rebound > 0);
    for _ in 0..2 {
      h.frame(|_| {});
      bind(&mut h);
    }
    let after = rows(&h, list);
    assert!(after.iter().any(|(i, _)| *i == 52));
    assert!(
      after.iter().all(|(_, e)| before.contains(e)) || after.len() <= before.len() + 2,
      "rows are reused, not respawned"
    );
    let (_, row) = *after.iter().find(|(i, _)| *i == 52).unwrap();
    assert_eq!(line_of(&h, row), "53. Player 53");
  }

  #[test]
  fn a_row_bound_this_frame_is_drawn_from_the_next() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 100);
    h.frame(|_| {});
    let (_, row) = rows(&h, list)[0];
    // A text node has no id of its own: the row's container has
    assert!(
      h.ui_id(row).is_none(),
      "not drawn in the frame it was bound"
    );
    bind(&mut h);
    h.frame(|_| {});
    assert!(h.ui_id(row).is_some(), "drawn the next frame");
  }

  #[test]
  fn each_row_is_its_own_binding_scope() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 10);
    h.frame(|_| {});
    bind(&mut h);
    let pool = rows(&h, list);
    assert_ne!(line_of(&h, pool[0].1), line_of(&h, pool[1].1));
  }

  #[test]
  fn hostile_sizes_and_a_missing_shape_build_no_rows() {
    let mut h = UiHarness::new(640.0, 480.0);
    let bad = h.spawn_ui(UiNode::list(10, f32::NAN).row_shape(row));
    let shapeless = h.spawn_ui(UiNode::list(10, 20.0));
    h.frame(|_| {});
    assert!(rows(&h, bad).is_empty());
    assert!(rows(&h, shapeless).is_empty());
    assert_eq!(
      crate::core::ui::test_util::warnings_containing("row shape"),
      1
    );
  }

  #[test]
  fn a_despawned_list_takes_its_rows_and_spawn_ui_in_refuses_it() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 10);
    h.frame(|_| {});
    let pool = rows(&h, list);
    assert!(!pool.is_empty());
    assert!(spawn_ui_in(h.world_mut(), list, UiNode::text("x")).is_none());
    h.world_mut().despawn(list);
    assert!(pool.iter().all(|(_, e)| h.world().get_entity(*e).is_err()));
  }

  #[test]
  fn a_reconcile_keeps_the_rows_and_writes_the_count_only_when_the_tree_changed_it() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 100);
    h.frame(|_| {});
    let pool = rows(&h, list);
    h.world_mut().get_mut::<UiList>(list).unwrap().count = 500; // the game's count
    h.update_ui(
      list,
      UiNode::list(100, 20.0)
        .gap(4.0)
        .height(200.0)
        .width(300.0)
        .row_shape(row)
        .name("board"),
    );
    assert_eq!(
      h.world().get::<UiList>(list).unwrap().count,
      500,
      "the tree did not change the count"
    );
    assert_eq!(rows(&h, list), pool, "rows are the pool's");
    h.update_ui(
      list,
      UiNode::list(42, 20.0)
        .gap(4.0)
        .height(200.0)
        .width(300.0)
        .row_shape(row)
        .name("board"),
    );
    assert_eq!(h.world().get::<UiList>(list).unwrap().count, 42);
  }

  /// The frame the harness drew last
  fn frame_of(h: &UiHarness) -> u64 {
    h.memory.frame()
  }

  #[test]
  fn a_far_jump_keeps_the_pool_to_a_few_views_on_every_frame_and_lands() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 10_000);
    for _ in 0..3 {
      h.frame(|_| {});
      bind(&mut h);
    }
    let id = h.ui_id(list).unwrap();
    h.world_mut().get_mut::<UiList>(list).unwrap().jump = Some((5000, false));
    // A view is 10 rows of 22 (200 units)
    let views = 3 * 10;
    for frame in 0..60 {
      h.frame(|_| {});
      bind(&mut h);
      let pool = rows(&h, list);
      assert!(
        pool.len() < views + 4,
        "frame {frame}: {} rows pooled",
        pool.len()
      );
      assert!(
        pool.iter().all(|(i, _)| *i > 4900),
        "frame {frame}: no sweep from the top"
      );
      // From the frame after the jump, every row in view is drawn
      if frame > 0 {
        assert_view_drawn(&h, list, id, &format!("frame {frame}"));
      }
    }
    assert_eq!(h.world().get::<UiList>(list).unwrap().jump, None);
    let offset = h.scroll_offset(id).y;
    let bottom = 5000.0 * 22.0 + 20.0;
    assert!(
      offset <= 5000.0 * 22.0 && offset + 200.0 >= bottom,
      "item 5000 in view at {offset}"
    );
  }

  #[test]
  fn ui_passes_over_trees_without_a_list_spawn_nothing() {
    // The same tree in two harnesses, one never drawn: the next entity takes
    // the same id (`update_lists_leaves_a_world_with_no_list_as_it_was`
    // checks the archetypes and resources)
    let tree = || {
      UiNode::column()
        .focus_scope()
        .child(UiNode::button("Play"))
        .child(UiNode::scroll().height(40.0).child(UiNode::text("x")))
    };
    let mut h = UiHarness::new(320.0, 240.0);
    let mut still = UiHarness::new(320.0, 240.0);
    h.spawn_ui(tree());
    still.spawn_ui(tree());
    for _ in 0..3 {
      h.frame(|_| {});
    }
    assert_eq!(
      h.world_mut().spawn_empty().id(),
      still.world_mut().spawn_empty().id()
    );
  }

  #[test]
  fn update_lists_leaves_a_world_with_no_list_as_it_was() {
    let mut world = World::new();
    let mut untouched = World::new();
    let mut memory = UiMemory::new();
    let screen = Vec2::new(320.0, 240.0);
    // No UI at all, then trees with no list
    update_lists(&mut world, &mut memory, 1, screen);
    for w in [&mut world, &mut untouched] {
      spawn_ui(w, 1, UiNode::column().child(UiNode::button("A")));
    }
    update_lists(&mut world, &mut memory, 2, screen);
    update_lists(&mut world, &mut memory, 3, screen);
    assert!(!world.contains_resource::<Messages<UiListBind>>());
    assert_eq!(world.archetypes().len(), untouched.archetypes().len());
    assert_eq!(world.components().len(), untouched.components().len());
    assert_eq!(world.spawn_empty().id(), untouched.spawn_empty().id());
  }

  /// A closure list as `board` makes it, drawn from one call site
  fn closure_board(ui: &mut UiContext<'_>, columns: u16, offset: Option<f32>) -> UiId {
    let list = ui
      .list(10_000)
      .row_height(20.0)
      .gap(2.0)
      .columns(columns)
      .height(200.0)
      .width(300.0)
      .show(|ui, i| {
        ui.text(format!("{i}")).draw();
      });
    if let Some(offset) = offset {
      ui.set_scroll_offset(list.id, Vec2::new(0.0, offset));
    }
    list.id
  }

  #[test]
  fn rows_are_placed_as_a_closure_list_places_them() {
    for columns in [1, 3] {
      let mut closure = UiHarness::new(640.0, 480.0);
      let id = closure.frame(|ui| closure_board(ui, columns, None));
      closure.frame(|ui| closure_board(ui, columns, Some(1000.0)));
      for _ in 0..2 {
        closure.frame(|ui| closure_board(ui, columns, None));
      }

      let mut h = UiHarness::new(640.0, 480.0);
      let list = h.spawn_ui(
        UiNode::list(10_000, 20.0)
          .gap(2.0)
          .columns(columns)
          .height(200.0)
          .width(300.0)
          .row_shape(row),
      );
      h.frame(|_| {});
      let node_id = h.ui_id(list).unwrap();
      h.frame(|ui| ui.set_scroll_offset(node_id, Vec2::new(0.0, 1000.0)));
      for _ in 0..3 {
        h.frame(|_| {});
        bind(&mut h);
      }
      assert_eq!(h.scroll_offset(node_id), closure.scroll_offset(id));
      let per_row = usize::from(columns);
      let items = (1000 / 22) * per_row..(1200 / 22 + 1) * per_row;
      for index in items {
        let drawn = closure.record_of_key(id, index).map(|r| r.rect);
        let item = crate::core::ui::widgets::list::item_id(node_id, columns, index);
        let pooled = h.record_of(item).map(|r| r.rect);
        assert!(drawn.is_some(), "{columns} columns: item {index} drawn");
        assert_eq!(pooled, drawn, "{columns} columns: item {index}");
      }
    }
  }

  fn pick() -> UiNode {
    UiNode::button("{name}").bound().name("pick")
  }

  #[test]
  fn the_focused_row_stays_pooled_when_scrolled_away_and_the_pad_steps_on_from_it() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::column().focus_scope().child(
        UiNode::list(1000, 30.0)
          .height(120.0)
          .width(200.0)
          .row_shape(pick)
          .name("list"),
      ),
    );
    let list = h.find_ui("list").unwrap();
    for _ in 0..4 {
      h.frame(|_| {});
      bind(&mut h);
    }
    // The scope appeared before its rows were drawn: the first Down focuses
    // its first item
    h.action("ui.down");
    h.frame(|_| {});
    assert_eq!(h.world().get::<UiList>(list).unwrap().focused, Some(0));
    for _ in 0..3 {
      h.action("ui.down");
      h.frame(|_| {});
      bind(&mut h);
    }
    let focused = h.focused();
    assert!(focused.is_some());
    assert_eq!(h.world().get::<UiList>(list).unwrap().focused, Some(3));
    let id = h.ui_id(list).unwrap();
    h.frame(|ui| ui.set_scroll_offset(id, Vec2::new(0.0, 15_000.0)));
    for _ in 0..4 {
      h.frame(|_| {});
      bind(&mut h);
    }
    let pool = rows(&h, list);
    for index in 2..5 {
      assert!(pool.iter().any(|(i, _)| *i == index), "item {index} pooled");
    }
    assert!(pool.len() < 20, "{} rows", pool.len());
    assert_eq!(h.focused(), focused, "the focus stays on its item");
    h.action("ui.down");
    h.frame(|_| {});
    h.frame(|_| {});
    assert_eq!(h.world().get::<UiList>(list).unwrap().focused, Some(4));
  }

  #[test]
  fn rows_are_scopes_of_their_own_for_bindings_and_names() {
    let mut h = UiHarness::new(640.0, 480.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::text("{name}").bound().name("title"))
        .child(
          UiNode::list(10, 20.0)
            .height(100.0)
            .row_shape(row)
            .name("board"),
        ),
    );
    let list = h.find_ui("board").unwrap();
    h.frame(|_| {});
    bind(&mut h);
    h.ui_set(root, "name", "Root");
    let title = h.find_ui("title").unwrap();
    assert_eq!(h.world().get::<UiLabel>(title).unwrap().0, "Root");
    let (_, first) = rows(&h, list)[0];
    assert_eq!(line_of(&h, first), "1. Player 1");
    assert_eq!(
      h.ui_get::<String>(first, "name").as_deref(),
      Some("Player 1")
    );
    assert_eq!(h.ui_get::<String>(root, "name").as_deref(), Some("Root"));
    // Names too: a row's nodes are found from the row
    assert_eq!(h.find_ui("line"), None);
    assert_eq!(
      crate::core::ui::test_util::warnings_containing("named \"line\""),
      0
    );
  }

  #[test]
  fn an_observer_hears_each_bind_on_its_row() {
    let mut h = UiHarness::new(640.0, 480.0);
    let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&heard);
    h.world_mut()
      .add_observer(move |bind: crate::core::ecs::kernel::On<UiListBind>| {
        if let Ok(mut log) = log.lock() {
          log.push((bind.row, bind.index));
        }
      });
    let list = board(&mut h, 30);
    h.frame(|_| {});
    let sent: Vec<(Entity, usize)> = h
      .messages::<UiListBind>()
      .map(|b| (b.row, b.index))
      .collect();
    assert!(!sent.is_empty());
    assert_eq!(*heard.lock().unwrap(), sent);
    assert!(
      sent
        .iter()
        .all(|&(row, _)| h.world().get::<ChildOf>(row).map(ChildOf::parent) == Some(list))
    );
  }

  #[test]
  fn a_count_the_game_changes_releases_and_grows_the_rows() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 100);
    for _ in 0..3 {
      h.frame(|_| {});
      bind(&mut h);
    }
    h.world_mut().get_mut::<UiList>(list).unwrap().count = 5;
    h.frame(|_| {});
    assert_eq!(
      rows(&h, list).iter().map(|r| r.0).collect::<Vec<_>>(),
      [0, 1, 2, 3, 4]
    );
    h.world_mut().get_mut::<UiList>(list).unwrap().count = 0;
    h.frame(|_| {});
    assert!(rows(&h, list).is_empty());
    h.world_mut().get_mut::<UiList>(list).unwrap().count = 50;
    h.frame(|_| {});
    assert_eq!(bind(&mut h), 12, "a view of rows and the overscan");
  }

  #[test]
  fn hostile_counts_and_columns_never_panic_and_stay_bounded() {
    let mut h = UiHarness::new(640.0, 480.0);
    let huge = h.spawn_ui(
      UiNode::list(usize::MAX, 20.0)
        .height(100.0)
        .columns(0)
        .row_shape(row),
    );
    let mut bad = Vec::new();
    for height in [0.0, -1.0, f32::INFINITY, f32::MAX] {
      bad.push(h.spawn_ui(UiNode::list(10, height).gap(f32::MAX).row_shape(row)));
    }
    for _ in 0..3 {
      h.frame(|_| {});
      bind(&mut h);
    }
    assert!(!rows(&h, huge).is_empty() && rows(&h, huge).len() < 20);
    for list in bad {
      assert!(rows(&h, list).is_empty());
    }
    let id = h.ui_id(huge).unwrap();
    h.world_mut().get_mut::<UiList>(huge).unwrap().jump = Some((usize::MAX, false));
    for _ in 0..40 {
      h.frame(|_| {});
      bind(&mut h);
    }
    let offset = h.scroll_offset(id).y;
    assert!(offset.is_finite() && offset > 1e18, "{offset}");
    assert!(rows(&h, huge).len() < 20);
    assert!(rows(&h, huge).iter().any(|(i, _)| *i == usize::MAX - 1));
  }

  /// Every item in the view the list was drawn at last frame has a row given
  /// its item before that frame
  fn assert_view_drawn(h: &UiHarness, list: Entity, id: UiId, when: &str) {
    let offset = h.scroll_offset(id).y;
    let pool = rows(h, list);
    let first = (offset / 22.0).floor() as usize;
    let end = ((offset + 200.0) / 22.0).ceil() as usize;
    for index in first..end {
      let row = pool.iter().find(|(i, _)| *i == index).map(|r| r.1);
      let row = row.unwrap_or_else(|| panic!("{when}: item {index} in view at {offset}"));
      let pooled = h.world().get::<UiListRow>(row).unwrap();
      assert!(
        pooled.bound_frame < frame_of(h),
        "{when}: item {index} drawn at {offset}"
      );
    }
  }

  #[test]
  fn a_wheel_within_two_views_draws_every_row_it_eases_past() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 10_000);
    for _ in 0..3 {
      h.frame(|_| {});
      bind(&mut h);
    }
    let id = h.ui_id(list).unwrap();
    h.move_to(150.0, 100.0);
    // Nine lines of 40: under two views
    h.wheel(0.0, -9.0);
    h.frame(|_| {});
    bind(&mut h);
    for frame in 1..40 {
      h.frame(|_| {});
      bind(&mut h);
      assert_view_drawn(&h, list, id, &format!("frame {frame}"));
    }
    assert_eq!(h.scroll_offset(id).y, 360.0);
  }
}
