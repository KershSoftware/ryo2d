//! The pooled node list: `UiNode::list(count, row_height)` keeps about a
//! screenful of row entities, each given an item by index, reused as the
//! list scrolls and filled by the game when a `UiListBind` names it
//!
//! `update_lists` brings each pool in line once a frame, before any tree is
//! drawn; the walk's `Kind::List` arm draws the pooled rows through the
//! closure list's placement (`widgets/list.rs`), so both kinds of list place
//! their rows alike. The game drives a list through `ctx.ui_list_*`
//! (`list_count`, `list_refresh`, `list_jump` and `list_index` here), and the
//! focus follows the closure list's rules: the focused row stays pooled, a
//! count that shrinks under it moves it to the new last item, and a focus
//! jump waits until the focus lands.

use std::ops::Range;
use std::sync::Arc;

use glam::Vec2;

use crate::core::ecs::kernel::{ChildOf, Children, Component, Entity, Messages, Mut, World};
use crate::core::messages::register_message;
use crate::core::ui::memory::UiMemory;
use crate::core::ui::theme::warn_once;
use crate::core::ui::widgets::list::{
  Fill, FocusWatch, Shape, cut_rows, place, row_target_in, runs, warn_cut,
};
use crate::core::ui::widgets::scroll::{Scroll, ScrollState};
use crate::core::ui::{Response, UiContext, UiId};

use super::bindings;
use super::components::{UiDisabled, UiHidden, UiInteraction, UiName, UiNodes, UiRoot};
use super::draw::{Walk, draw_child};
use super::events::UiListBind;
use super::node::{Kind, UiNode, UiNodeKind};
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
  /// The widget with the focus in `focused`'s row (`NONE` with none): a
  /// count that shrinks under the item moves the focus on only while the
  /// focus is still there
  pub(crate) focus_widget: UiId,
  /// An item to scroll to (and whether to focus it): its row is pooled and
  /// the list heads there at the next update
  pub(crate) jump: Option<(usize, bool)>,
  /// An item to give the focus once its row is drawn bound (a focus jump
  /// once aimed, or the new last item when the count shrank under the
  /// focus): its row stays pooled until the focus has been given
  pub(crate) pending: Option<PendingFocus>,
  /// Pooled rows to bind again at the next update (`ui_list_refresh`)
  pub(crate) rebind: Rebind,
  /// Whether the list's first rows give its focus scope its default focus
  pub(crate) first_focus: FirstFocus,
}

/// A focus on its way to an item of a list node
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingFocus {
  pub(crate) index: usize,
  /// The node asked for (through `UiNodes::pending_focus`) once its row was
  /// drawn bound: the update after the walk draws it ends the pending focus,
  /// landed or not
  pub(crate) asked: Option<Entity>,
}

/// The pooled rows a list node binds again: each keeps drawing its old data
/// until the game binds it
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum Rebind {
  #[default]
  None,
  All,
  /// The rows holding these items, if pooled
  Rows(Vec<usize>),
}

/// A new list's first rows and its focus scope's default focus
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum FirstFocus {
  /// Not drawn yet
  #[default]
  Undecided,
  /// First drawn with no row to show, in a focus scope that appeared with
  /// it: the scope found none of the rows' widgets for its default focus,
  /// so the rows give it when they first show
  Waiting,
  Done,
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

/// The pool of the list node `list`, to change; a node that is not a list
/// warns once
fn pool_mut(world: &mut World, list: Entity) -> Option<Mut<'_, UiList>> {
  let pool = world.get_mut::<UiList>(list);
  if pool.is_none() {
    warn_once("ui-list:not-a-list".to_string(), || {
      format!("{list} is not a list node (UiNode::list); a ui_list_* call on it does nothing")
    });
  }
  pool
}

/// `ctx.ui_list_count`: the list's item count. Rows past the end are
/// released at the next update, which also moves a focus past the end.
pub(crate) fn list_count(world: &mut World, list: Entity, count: usize) {
  if let Some(mut pool) = pool_mut(world, list)
    && pool.count != count
  {
    pool.count = count;
  }
}

/// `ctx.ui_list_refresh` (every pooled row, `index` `None`) and
/// `ctx.ui_list_refresh_row` (the row holding `index`, if pooled): bound
/// again at the next update
pub(crate) fn list_refresh(world: &mut World, list: Entity, index: Option<usize>) {
  let Some(mut pool) = pool_mut(world, list) else {
    return;
  };
  let rebind = &mut pool.rebind;
  match index {
    None => *rebind = Rebind::All,
    Some(index) => {
      if let Rebind::Rows(items) = rebind {
        if !items.contains(&index) {
          items.push(index);
        }
      } else if *rebind == Rebind::None {
        *rebind = Rebind::Rows(vec![index]);
      }
    }
  }
}

/// `ctx.ui_list_scroll_to` and, with `focus`, `ctx.ui_list_focus`: the list
/// heads for item `index` at the next update
pub(crate) fn list_jump(world: &mut World, list: Entity, index: usize, focus: bool) {
  if let Some(mut pool) = pool_mut(world, list) {
    pool.jump = Some((index, focus));
  }
}

/// `ctx.ui_list_index`: the item of the list row `node` belongs to (the
/// nearest row up from it, itself included)
pub(crate) fn list_index(world: &World, node: Entity) -> Option<usize> {
  let mut at = node;
  loop {
    let entity = world.get_entity(at).ok()?;
    if let Some(row) = entity.get::<UiListRow>() {
      return Some(row.index);
    }
    if entity.contains::<UiRoot>() {
      return None;
    }
    at = entity.get::<ChildOf>()?.parent();
  }
}

/// A list node's measures this frame: its options, gap and top padding with
/// `count` items, cleaned by `Shape::new` (`None`, warned once, when it can
/// place no rows; `None` for a node that is not a list)
pub(super) fn shape_of(kind: &UiNodeKind, count: usize) -> Option<Shape> {
  let Kind::List(opts) = kind.kind else {
    return None;
  };
  let gap = kind.layout.gap.unwrap_or(0.0);
  let top = kind.layout.padding.map_or(0.0, |padding| padding.top);
  Shape::new(
    count,
    opts.row_height,
    opts.columns,
    gap,
    usize::from(opts.overscan),
    top,
  )
}

/// Bring every list node's pool in line, once a frame: after the pointer,
/// the wheel, the scroll requests and navigation are resolved and before any
/// node tree is drawn. A list keeps a row for each item it needs (the rows
/// in view at its scroll area's offset and at its target, every row between
/// when the two are within two views, the overscan, the focused item's row
/// and one either side, a row being jumped to and a row waiting for the
/// focus), reuses the rows of the items it no longer needs for the ones that
/// have none, and spawns or despawns the difference. Each row given an item,
/// and each row the game asked to refresh, sends a `UiListBind`.
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

/// Bring the pool of `list` in line; each row bound joins `binds`
fn update_list(
  world: &mut World,
  memory: &mut UiMemory,
  frame: u64,
  screen: Vec2,
  list: Entity,
  binds: &mut Vec<UiListBind>,
) {
  let Some((count, focused, focus_widget, jump, pending)) = world.get::<UiList>(list).map(|pool| {
    (
      pool.count,
      pool.focused,
      pool.focus_widget,
      pool.jump,
      pool.pending,
    )
  }) else {
    return;
  };
  let shape = world
    .get::<UiNodeKind>(list)
    .and_then(|kind| shape_of(kind, count));
  let shaped = world.get::<UiListShape>(list).is_some();
  // No row shape, or no size to place rows at: no rows
  let Some(shape) = shape.filter(|_| shaped) else {
    release(world, list);
    return;
  };
  let id = world
    .get::<UiInteraction>(list)
    .map_or(UiId::NONE, |state| state.id);

  let (mut focused, mut jump, mut pending) = (focused, jump, pending);
  // The count shrank under the focused item: the focus moves to the new
  // last item while it is still on the widget that item made, else it goes
  if focused.is_some_and(|index| index >= count) {
    focused = None;
    let held = focus_widget != UiId::NONE && memory.interact.focused == focus_widget;
    let on_its_way = pending.is_some() || jump.is_some_and(|(_, focus)| focus);
    if held
      && !on_its_way
      && let Some(last) = count.checked_sub(1)
    {
      jump = Some((last, true));
    }
  }
  // A focus on its way past the end goes to the new last item, from the start
  if pending.is_some_and(|waiting| waiting.index >= count) {
    take_back(world, pending);
    pending = count
      .checked_sub(1)
      .map(|index| PendingFocus { index, asked: None });
  }

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
  // Aimed, a focus jump waits for its row; asked again for the same item,
  // it keeps its progress
  if aimed
    && let (Some(index), Some((_, true))) = (jump_to, jump)
    && pending.is_none_or(|waiting| waiting.index != index)
  {
    take_back(world, pending);
    pending = Some(PendingFocus { index, asked: None });
  }
  // The jump's row is pooled below; a jump to nothing has nowhere to go
  if aimed || jump_to.is_none() {
    jump = None;
  }

  // The view's rows at the offset and at the target, at most the cap
  // together; within two views of each other, the rows the ease passes too
  let cap = shape.row_cap();
  let (at, cut_at) = shape.built(view, offset);
  let (to, cut_to) = shape.built(view, target);
  let near = (f64::from(target) - f64::from(offset)).abs() <= 2.0 * f64::from(view);
  let (at, to, cut) = if near {
    let (both, cut) = cut_rows(at.start.min(to.start)..at.end.max(to.end), cap);
    (both, 0..0, cut)
  } else {
    let (to, cut) = cut_rows(to, cap.saturating_sub(at.len()));
    (at, to, cut)
  };
  if cut || cut_at || cut_to {
    warn_cut(list, || match world.get::<UiName>(list) {
      Some(name) => format!("the list node \"{}\"", name.0),
      None => format!("the list node {list}"),
    });
  }
  let one = |index: usize| row_of(index)..row_of(index).saturating_add(1).min(rows);
  // The focused item's row and one either side, so the pad steps off it
  let focus = focused.map_or(0..0, |index| {
    row_of(index).saturating_sub(1)..row_of(index).saturating_add(2).min(rows)
  });
  let jumped = jump_to.map_or(0..0, one);
  let waiting = pending.map_or(0..0, |waiting| one(waiting.index));
  let needed = runs([at, to, focus, jumped, waiting]).map(|run| shape.items(run));

  let Some(mut pool) = world.get_mut::<UiList>(list) else {
    return;
  };
  let mut held = std::mem::take(&mut pool.rows);
  let rebind = std::mem::take(&mut pool.rebind);
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

  // Rows the game asked to refresh are bound again and keep drawing
  // meanwhile; a row given its item this frame has been bound already
  let pooled = |index: usize| {
    held
      .binary_search_by_key(&index, |&(item, _)| item)
      .ok()
      .and_then(|at| held.get(at))
      .map(|&(_, row)| row)
  };
  match &rebind {
    Rebind::None => {}
    Rebind::All => {
      for &(index, row) in &held {
        if drawn(world, row, frame) {
          binds.push(UiListBind { list, row, index });
        }
      }
    }
    Rebind::Rows(items) => {
      for &index in items {
        if let Some(row) = pooled(index).filter(|&row| drawn(world, row, frame)) {
          binds.push(UiListBind { list, row, index });
        }
      }
    }
  }

  // The focus is asked for once its row is drawn bound, and ends the frame
  // after the walk drew the node asked for: this frame's resolve gave it
  let pending = match pending {
    Some(PendingFocus {
      asked: Some(node), ..
    }) => {
      let waiting = world
        .get_resource::<UiNodes>()
        .is_some_and(|nodes| nodes.pending_focus == Some(node));
      if waiting && world.get_entity(node).is_ok() {
        pending
      } else {
        take_back(world, pending);
        None
      }
    }
    Some(PendingFocus { index, asked: None }) => {
      match pooled(index).filter(|&row| drawn(world, row, frame)) {
        // Its row is not drawn yet
        None => pending,
        Some(row) => match first_focusable(world, row, false) {
          Some(node) => {
            if let Some(mut nodes) = world.get_resource_mut::<UiNodes>() {
              nodes.pending_focus = Some(node);
            }
            Some(PendingFocus {
              index,
              asked: Some(node),
            })
          }
          // Nothing in the row takes the focus
          None => None,
        },
      }
    }
    None => None,
  };

  if let Some(mut pool) = world.get_mut::<UiList>(list) {
    pool.rows = held;
    pool.jump = jump;
    pool.pending = pending;
    if pool.focused != focused {
      pool.focused = focused;
    }
  }
}

/// Whether the pooled row `row` is drawn this frame: given its item before it
fn drawn(world: &World, row: Entity, frame: u64) -> bool {
  world
    .get::<UiListRow>(row)
    .is_some_and(|pooled| pooled.bound_frame < frame)
}

/// A pending focus that ends before the walk drew the node it asked for
/// takes its request back
fn take_back(world: &mut World, pending: Option<PendingFocus>) {
  let Some(node) = pending.and_then(|waiting| waiting.asked) else {
    return;
  };
  if let Some(mut nodes) = world.get_resource_mut::<UiNodes>()
    && nodes.pending_focus == Some(node)
  {
    nodes.pending_focus = None;
  }
}

/// Whether a node of `kind` takes the focus when it is asked for (as
/// `focus_ui` asks): a widget focusable by default, unless made not
/// focusable, or one made focusable
fn takes_focus(kind: &UiNodeKind) -> bool {
  let by_default = matches!(
    kind.kind,
    Kind::Button(_)
      | Kind::Checkbox
      | Kind::Toggle
      | Kind::Slider(_)
      | Kind::TextInput(_)
      | Kind::Dropdown
      | Kind::Slot { .. }
      | Kind::Rebind(_)
  );
  let widget = by_default
    || matches!(
      kind.kind,
      Kind::Image(_) | Kind::Progress { .. } | Kind::Prompt { .. }
    );
  widget && kind.flags.focusable.unwrap_or(by_default)
}

/// The first node of `row`'s tree, depth first from the row itself, that
/// takes the focus when asked: neither hidden (nor under a hidden node) nor
/// disabled, and with `marked` one marked `.default_focus()`. The rows of a
/// list inside the row are that list's.
fn first_focusable(world: &World, row: Entity, marked: bool) -> Option<Entity> {
  let mut stack = vec![row];
  while let Some(node) = stack.pop() {
    let Ok(entity) = world.get_entity(node) else {
      continue;
    };
    if entity.contains::<UiHidden>() || (node != row && entity.contains::<UiListRow>()) {
      continue;
    }
    if let Some(kind) = entity.get::<UiNodeKind>()
      && takes_focus(kind)
      && !entity.contains::<UiDisabled>()
      && (!marked || kind.flags.default_focus)
    {
      return Some(node);
    }
    if let Some(children) = entity.get::<Children>() {
      stack.extend(children.iter().rev());
    }
  }
  None
}

/// Despawn every row of `list` and drop what waits for them, when it can
/// have none
fn release(world: &mut World, list: Entity) {
  let pooled = world.get::<UiList>(list).is_some_and(|pool| {
    !pool.rows.is_empty()
      || pool.jump.is_some()
      || pool.pending.is_some()
      || pool.rebind != Rebind::None
      || pool.focused.is_some()
  });
  if !pooled {
    return;
  }
  let Some(mut pool) = world.get_mut::<UiList>(list) else {
    return;
  };
  pool.jump = None;
  pool.rebind = Rebind::None;
  pool.focused = None;
  let pending = pool.pending.take();
  let rows = std::mem::take(&mut pool.rows);
  take_back(world, pending);
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

/// Draw the list node `node` of `shape` (from [`shape_of`]) in `scroll`, its
/// scroll area configured from the node: its pooled rows in item order, each
/// in its item's container keyed by the index, with spacers between, placed
/// as a closure list places its rows. A row given its item this frame is
/// drawn from the next; until then its container is drawn empty, at full
/// size. The item whose row holds the focus becomes the list's `focused`.
pub(super) fn draw_list(
  world: &mut World,
  node: Entity,
  scroll: Scroll<'_, '_>,
  shape: Option<Shape>,
  walk: &mut Walk,
) -> Response {
  // Taken while the rows draw, so the world is free for them
  let (rows, first, asking) = world
    .get_mut::<UiList>(node)
    .map(|mut pool| {
      let asking = pool.pending.is_some() || pool.jump.is_some_and(|(_, focus)| focus);
      (std::mem::take(&mut pool.rows), pool.first_focus, asking)
    })
    .unwrap_or_default();
  let mut scroll = scroll;
  if let Some(shape) = shape {
    scroll = scroll.content_height(shape.length());
  }
  let (mut found, mut focus, mut first_now) = (None, UiId::NONE, first);
  let response = scroll.show(|ui| {
    let Some(shape) = shape else {
      return;
    };
    let list = ui.memory.scopes.current();
    let frame = ui.memory.frame();
    first_now = first_focus(world, ui, &rows, frame, first, asking, walk);
    let runs = PoolRuns {
      items: &rows,
      per_row: usize::from(shape.columns),
      rows: shape.rows(),
    };
    let mut pool = Pool {
      frame,
      watch: FocusWatch::new(ui),
      world,
      walk,
      rows: &rows,
    };
    place(ui, list, shape, runs, &mut pool);
    found = pool.watch.found;
    focus = pool.watch.focus;
  });
  if let Some(mut pool) = world.get_mut::<UiList>(node) {
    pool.rows = rows;
    if pool.focused != found {
      pool.focused = found;
    }
    let widget = if found.is_some() { focus } else { UiId::NONE };
    if pool.focus_widget != widget {
      pool.focus_widget = widget;
    }
    if pool.first_focus != first_now {
      pool.first_focus = first_now;
    }
  }
  response
}

/// A new list's first rows give its focus scope the default focus the scope
/// could not take when it appeared without them, as a scope's default: the
/// first row node marked `.default_focus()`, else the first focusable node
/// of the first row shown. It is decided at the list's first draw (nothing to
/// show yet, in a focus scope that appears with it) and given at its next,
/// never over a focus that is already somewhere or asked for.
fn first_focus(
  world: &World,
  ui: &UiContext<'_>,
  rows: &[(usize, Entity)],
  frame: u64,
  state: FirstFocus,
  asking: bool,
  walk: &mut Walk,
) -> FirstFocus {
  let shown = || {
    rows
      .iter()
      .map(|&(_, row)| row)
      .filter(move |&row| drawn(world, row, frame))
  };
  match state {
    FirstFocus::Done => FirstFocus::Done,
    FirstFocus::Undecided => {
      let scope = ui.focus_scope_here();
      let appearing = scope != UiId::NONE && !ui.memory.known_scopes.contains(&scope);
      if appearing && shown().next().is_none() {
        FirstFocus::Waiting
      } else {
        FirstFocus::Done
      }
    }
    FirstFocus::Waiting => {
      let (_, top) = ui.scene();
      if top && ui.focused().is_none() && walk.pending_focus.is_none() && !asking {
        walk.pending_focus = shown()
          .find_map(|row| first_focusable(world, row, true))
          .or_else(|| shown().find_map(|row| first_focusable(world, row, false)));
      }
      FirstFocus::Done
    }
  }
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
    if drawn(self.world, row, self.frame) {
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
    // The scope appeared before its rows were drawn: its default focus, item
    // 0, came with the rows
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

#[cfg(test)]
mod api_tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::*;
  use crate::core::ui::test_util::warnings_containing;
  use crate::core::ui::widgets::list::BUILT_ITEMS;
  use crate::prelude::*;

  fn row() -> UiNode {
    UiNode::button("{name}").bound().name("pick")
  }

  fn setup(h: &mut UiHarness, count: usize) -> Entity {
    let root = h.spawn_ui(
      UiNode::column().focus_scope().child(
        UiNode::list(count, 20.0)
          .height(100.0)
          .width(200.0)
          .row_shape(row)
          .name("list"),
      ),
    );
    h.find_ui("list").unwrap_or(root)
  }

  fn step(h: &mut UiHarness) {
    h.frame(|_| {});
    let binds: Vec<UiListBind> = h.messages::<UiListBind>().collect();
    for b in binds {
      h.ui_set(b.row, "name", format!("P{}", b.index));
    }
  }

  fn focused_index(h: &UiHarness, list: Entity) -> Option<usize> {
    h.world().get::<UiList>(list).and_then(|l| l.focused)
  }

  #[test]
  fn ui_list_focus_jumps_and_focuses_and_ui_list_index_maps_events() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 1000);
    for _ in 0..3 {
      step(&mut h);
    }
    h.ui_list_focus(list, 700);
    for _ in 0..5 {
      step(&mut h);
    }
    assert_eq!(focused_index(&h, list), Some(700));
    h.action("ui.confirm");
    step(&mut h);
    let clicked = h
      .events()
      .iter()
      .find(|e| e.kind == UiEventKind::Clicked)
      .expect("a click");
    assert_eq!(h.ui_list_index(clicked.node), Some(700));
  }

  #[test]
  fn the_pad_walks_the_list_and_the_focused_row_stays_pooled() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 200);
    for _ in 0..3 {
      step(&mut h);
    }
    h.ui_list_focus(list, 0);
    for _ in 0..4 {
      step(&mut h);
    }
    for _ in 0..30 {
      h.action("ui.down");
      step(&mut h);
      step(&mut h);
    }
    assert_eq!(focused_index(&h, list), Some(30));
    let id = h.ui_id(list).unwrap();
    h.frame(|ui| ui.set_scroll_offset(id, Vec2::ZERO));
    for _ in 0..3 {
      step(&mut h);
    }
    assert!(
      h.world()
        .get::<UiList>(list)
        .unwrap()
        .rows
        .iter()
        .any(|(i, _)| *i == 30),
      "the focused row stays pooled"
    );
    assert_eq!(focused_index(&h, list), Some(30));
  }

  #[test]
  fn a_shrinking_count_moves_the_focus_to_the_last_item() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 1000);
    for _ in 0..3 {
      step(&mut h);
    }
    h.ui_list_focus(list, 900);
    for _ in 0..5 {
      step(&mut h);
    }
    h.ui_list_count(list, 50);
    for _ in 0..6 {
      step(&mut h);
    }
    assert_eq!(focused_index(&h, list), Some(49));
    assert!(
      h.world()
        .get::<UiList>(list)
        .unwrap()
        .rows
        .iter()
        .all(|(i, _)| *i < 50)
    );
  }

  #[test]
  fn refresh_rebinds_rows() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 10);
    for _ in 0..3 {
      step(&mut h);
    }
    h.ui_list_refresh(list);
    h.frame(|_| {});
    assert_eq!(
      h.messages::<UiListBind>().count(),
      h.world().get::<UiList>(list).unwrap().rows.len()
    );
    h.ui_list_refresh_row(list, 3);
    h.frame(|_| {});
    let again: Vec<UiListBind> = h.messages::<UiListBind>().collect();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].index, 3);
  }

  #[test]
  fn list_calls_on_a_plain_node_warn_once() {
    let mut h = UiHarness::new(640.0, 480.0);
    let plain = h.spawn_ui(UiNode::column());
    h.ui_list_count(plain, 3);
    h.ui_list_scroll_to(plain, 3);
    assert_eq!(warnings_containing("is not a list"), 1);
  }

  /// The row holding item `index`, if pooled
  fn row_of(h: &UiHarness, list: Entity, index: usize) -> Option<Entity> {
    let pool = h.world().get::<UiList>(list)?;
    pool.rows.iter().find(|r| r.0 == index).map(|r| r.1)
  }

  /// Whether item `index`'s button (its row) has the focus
  fn has_focus(h: &UiHarness, list: Entity, index: usize) -> bool {
    row_of(h, list, index)
      .and_then(|row| h.ui_id(row))
      .is_some_and(|id| h.focused() == Some(id))
  }

  #[test]
  fn a_focus_jump_stays_pending_until_the_focus_lands_two_frames_after_its_row_is_bound() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 1000);
    for _ in 0..3 {
      step(&mut h);
    }
    h.ui_list_focus(list, 700);
    let mut landed = None;
    for frame in 0..8 {
      step(&mut h);
      let pool = h.world().get::<UiList>(list).unwrap();
      if has_focus(&h, list, 700) {
        landed.get_or_insert(h.memory.frame());
        assert!(
          pool.jump.is_none() && pool.pending.is_none(),
          "frame {frame}: done once the focus is on it"
        );
      } else {
        assert!(
          pool.jump.is_some() || pool.pending.is_some(),
          "frame {frame}: pending until the focus lands"
        );
      }
    }
    let row = row_of(&h, list, 700).expect("pooled");
    let bound = h.world().get::<UiListRow>(row).unwrap().bound_frame;
    let landed = landed.expect("the focus landed");
    assert!(landed <= bound + 2, "landed at {landed}, bound at {bound}");
  }

  #[test]
  fn a_focus_jump_past_the_end_lands_on_the_last_item_and_an_empty_list_drops_one() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 1000);
    for _ in 0..3 {
      step(&mut h);
    }
    h.ui_list_focus(list, 5_000_000);
    for _ in 0..6 {
      step(&mut h);
    }
    assert_eq!(focused_index(&h, list), Some(999));
    assert!(has_focus(&h, list, 999));
    h.ui_list_focus(list, 10);
    h.ui_list_count(list, 0);
    step(&mut h);
    let pool = h.world().get::<UiList>(list).unwrap();
    assert!(pool.jump.is_none() && pool.pending.is_none() && pool.rows.is_empty());
    assert_eq!(focused_index(&h, list), None);
  }

  #[test]
  fn a_focus_jump_to_a_row_with_nothing_focusable_ends() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::column().focus_scope().child(
        UiNode::list(100, 20.0)
          .height(100.0)
          .row_shape(|| UiNode::row().child(UiNode::text("{name}").bound()))
          .name("list"),
      ),
    );
    let list = h.find_ui("list").unwrap();
    for _ in 0..3 {
      step(&mut h);
    }
    h.ui_list_focus(list, 50);
    for _ in 0..4 {
      step(&mut h);
    }
    let pool = h.world().get::<UiList>(list).unwrap();
    assert!(pool.jump.is_none() && pool.pending.is_none());
    assert_eq!(h.focused(), None);
  }

  #[test]
  fn a_focus_asked_of_a_node_the_game_despawned_ends() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 100);
    for _ in 0..3 {
      step(&mut h);
    }
    // Hidden, the list is not drawn: its row's button is asked for and waits
    h.ui_list_focus(list, 2);
    h.world_mut().entity_mut(list).insert(UiHidden);
    step(&mut h);
    let row = row_of(&h, list, 2).expect("pooled");
    let asked = h.world().get::<UiList>(list).unwrap().pending;
    assert_eq!(asked.and_then(|p| p.asked), Some(row));
    h.world_mut().despawn(row);
    h.world_mut().entity_mut(list).remove::<UiHidden>();
    step(&mut h);
    assert_eq!(h.world().get::<UiList>(list).unwrap().pending, None);
    assert_eq!(h.world().resource::<UiNodes>().pending_focus, None);
  }

  #[test]
  fn a_new_list_in_a_new_scope_takes_its_default_focus_once_its_rows_show() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 100);
    for _ in 0..3 {
      step(&mut h);
    }
    assert!(has_focus(&h, list, 0), "item 0's button has the focus");
    assert_eq!(focused_index(&h, list), Some(0));
  }

  #[test]
  fn a_row_node_marked_default_focus_takes_it_and_maps_to_its_item() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::column().focus_scope().child(
        UiNode::list(100, 20.0)
          .height(100.0)
          .width(200.0)
          .row_shape(|| {
            UiNode::row()
              .child(UiNode::button("A"))
              .child(UiNode::button("B").default_focus().name("b"))
          })
          .name("list"),
      ),
    );
    let list = h.find_ui("list").unwrap();
    for _ in 0..3 {
      step(&mut h);
    }
    let first = row_of(&h, list, 0).expect("pooled");
    let b = find_ui_in(h.world(), first, "b").expect("in the row");
    assert!(h.focused().is_some());
    assert_eq!(h.focused(), h.ui_id(b), "item 0's B");
    // A widget nested in a row maps to its item
    assert_eq!(h.ui_list_index(b), Some(0));
    assert_eq!(h.ui_list_index(list), None);
  }

  #[test]
  fn a_list_s_first_rows_never_take_a_focus_their_scope_gave_or_one_it_never_offered() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::button("Back").name("back"))
        .child(
          UiNode::list(100, 20.0)
            .height(100.0)
            .width(200.0)
            .row_shape(row),
        ),
    );
    let back = h.find_ui("back").unwrap();
    for _ in 0..4 {
      step(&mut h);
    }
    assert!(h.focused().is_some());
    assert_eq!(
      h.focused(),
      h.ui_id(back),
      "the scope's own default keeps it"
    );
    // A list added to a scope already shown takes nothing, as a closure
    // list's rows would not
    let mut quiet = UiHarness::new(640.0, 480.0);
    let page = quiet.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::text("Loading")),
    );
    for _ in 0..3 {
      step(&mut quiet);
    }
    spawn_ui_in(
      quiet.world_mut(),
      page,
      UiNode::list(100, 20.0).height(100.0).row_shape(row),
    );
    for _ in 0..4 {
      step(&mut quiet);
    }
    assert_eq!(quiet.focused(), None);
  }

  #[test]
  fn a_refresh_rebinds_without_drawing_a_row_empty() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 100);
    for _ in 0..3 {
      step(&mut h);
    }
    let pool = h.world().get::<UiList>(list).unwrap().rows.clone();
    h.ui_list_refresh(list);
    h.ui_list_refresh_row(list, 1);
    h.frame(|_| {});
    let sent: Vec<usize> = h.messages::<UiListBind>().map(|b| b.index).collect();
    assert_eq!(sent, pool.iter().map(|r| r.0).collect::<Vec<_>>());
    for &(index, row) in &pool {
      let id = h.ui_id(row).expect("drawn before");
      assert!(
        h.record_of(id).is_some(),
        "item {index} drawn the frame it was refreshed"
      );
      assert_eq!(
        h.world().get::<UiLabel>(row).map(|l| l.0.as_str()),
        Some(format!("P{index}").as_str()),
        "item {index} shows its old data until the game binds it"
      );
    }
  }

  #[test]
  fn a_row_the_game_despawned_is_skipped_and_its_item_given_a_new_row() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 100);
    for _ in 0..3 {
      step(&mut h);
    }
    let (index, dead) = h.world().get::<UiList>(list).unwrap().rows[2];
    h.world_mut().despawn(dead);
    h.ui_list_refresh_row(list, index);
    h.ui_list_refresh(list);
    h.ui_list_focus(list, index);
    assert_eq!(h.ui_list_index(dead), None);
    h.frame(|_| {});
    let sent: Vec<UiListBind> = h.messages::<UiListBind>().collect();
    assert!(sent.iter().all(|b| b.row != dead));
    assert!(sent.iter().any(|b| b.index == index), "its item rebound");
    for b in sent {
      h.ui_set(b.row, "name", format!("P{}", b.index));
    }
    for _ in 0..3 {
      step(&mut h);
    }
    let row = row_of(&h, list, index).expect("pooled");
    assert!(row != dead && h.world().get_entity(row).is_ok());
    assert!(has_focus(&h, list, index), "the focus lands on the new row");
    assert_eq!(warnings_containing(&format!("{dead}")), 0);
  }

  #[test]
  fn hostile_sizes_build_at_most_the_cap_and_warn_once() {
    let mut h = UiHarness::new(640.0, 480.0);
    let tiny = h.spawn_ui(
      UiNode::list(1_000_000, 0.001)
        .height(200.0)
        .width(100.0)
        .row_shape(UiNode::column)
        .name("tiny"),
    );
    // 256 columns at most, and 64 rows of overscan
    let wide = h.spawn_ui(
      UiNode::list(1_000_000, 20.0)
        .height(200.0)
        .width(100.0)
        .columns(u16::MAX)
        .overscan(u16::MAX)
        .row_shape(UiNode::column)
        .name("wide"),
    );
    for _ in 0..3 {
      h.frame(|_| {});
    }
    for list in [tiny, wide] {
      let pooled = h.world().get::<UiList>(list).unwrap().rows.len();
      assert_eq!(pooled, BUILT_ITEMS, "the cap bites: no more, no fewer");
    }
    let id = h.ui_id(wide).unwrap();
    // 1,000,000 items in rows of 256 are 3,907 rows
    assert_eq!(h.scroll_content(id).y, 3907.0 * 20.0);
    assert_eq!(
      warnings_containing("the list node \"tiny\" would build more than 4096"),
      1
    );
  }
}
