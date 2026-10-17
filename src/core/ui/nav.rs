//! Keyboard and gamepad focus: scopes, default focus, spatial navigation,
//! Tab order, activation and Back, resolved once per engine frame

use std::cmp::Ordering;

use glam::Vec2;

use super::memory::{Flags, Record, UiMemory};
use super::widgets::scroll::ScrollState;
use super::{Rect, UiId};
use crate::core::input::{Direction, InputDevice};

/// Rounding slack on the repeat timers, so 21 frames of 1/60 s make 0.35 s
const REPEAT_SLACK: f32 = 1e-4;
/// The right stick scrolls past this deflection
const STICK_SCROLL_THRESHOLD: f32 = 0.2;
/// UI units a second the right stick scrolls at full deflection
const STICK_SCROLL_SPEED: f32 = 600.0;
/// The deepest chain of nested scopes followed
const MAX_SCOPE_DEPTH: usize = 64;

/// Up, Down, Left, Right: the order of `NavInput`'s arrays
const DIRECTIONS: [Direction; 4] = [
  Direction::Up,
  Direction::Down,
  Direction::Left,
  Direction::Right,
];
/// The action of each direction, in the same order
const DIRECTION_ACTIONS: [&str; 4] = ["ui.up", "ui.down", "ui.left", "ui.right"];

/// The frame's navigation input, read from the UI actions
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NavInput {
  /// Up, Down, Left, Right went down this frame
  pub(crate) pressed: [bool; 4],
  /// Up, Down, Left, Right are held
  pub(crate) held: [bool; 4],
  pub(crate) confirm: bool,
  pub(crate) back: bool,
  /// Tab: the next focusable (the previous with `shift`)
  pub(crate) next: bool,
  pub(crate) shift: bool,
  pub(crate) tab_next: bool,
  pub(crate) tab_prev: bool,
  /// The right stick, with its deadzone applied (+y up)
  pub(crate) right_stick: Vec2,
  /// The last device the player used
  pub(crate) device: InputDevice,
  /// The real frame time (seconds)
  pub(crate) dt: f32,
  /// A held direction first repeats this long after its press (seconds)
  pub(crate) repeat_delay: f32,
  /// Then it repeats this often (seconds)
  pub(crate) repeat_rate: f32,
}

impl Default for NavInput {
  /// No input, with the theme's default repeat timing
  fn default() -> NavInput {
    NavInput {
      pressed: [false; 4],
      held: [false; 4],
      confirm: false,
      back: false,
      next: false,
      shift: false,
      tab_next: false,
      tab_prev: false,
      right_stick: Vec2::ZERO,
      device: InputDevice::default(),
      dt: 0.0,
      repeat_delay: 0.35,
      repeat_rate: 0.1,
    }
  }
}

/// What the focus resolve took from the frame's input
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct NavOutcome {
  /// The actions whose bindings the UI used: gameplay must not see them
  pub(crate) used: Vec<&'static str>,
}

/// The candidate in a direction: among those whose centre lies beyond
/// `from`'s centre along `direction`, the lowest score `p + 2s + 0.1c`,
/// where `p` is the gap along the direction from `from`'s leading edge to
/// the candidate's near edge, `s` the gap between them across it (0 when
/// they overlap) and `c` the distance between their centres across it. An
/// aligned candidate (`s == 0`) beats any other; ties go to the lower id.
pub(crate) fn spatial(
  from: Rect,
  direction: Direction,
  candidates: impl Iterator<Item = (UiId, Rect)>,
) -> Option<UiId> {
  let (along, sign) = axis(direction);
  let across = 1 - along;
  let (from_start, from_end) = span(from, along);
  let from_across = span(from, across);
  let mut best: Option<(bool, f32, UiId)> = None;
  for (id, rect) in candidates.filter(|(_, rect)| finite(*rect)) {
    if (centre(rect, along) - centre(from, along)) * sign <= 0.0 {
      continue;
    }
    let (start, end) = span(rect, along);
    let p = if sign > 0.0 {
      start - from_end
    } else {
      from_start - end
    }
    .max(0.0);
    let s = gap(from_across, span(rect, across));
    let score = p + 2.0 * s + 0.1 * (centre(rect, across) - centre(from, across)).abs();
    let key = (s > 0.0, score, id);
    let wins = best.is_none_or(|b| {
      key
        .0
        .cmp(&b.0)
        .then(key.1.total_cmp(&b.1))
        .then(key.2.cmp(&b.2))
        == Ordering::Less
    });
    if wins {
      best = Some(key);
    }
  }
  best.map(|(.., id)| id)
}

/// Where navigation wraps to: among the candidates beyond `from`'s centre
/// against `direction`, the farthest that way, aligned ones first, then
/// the nearest across; ties go to the lower id.
pub(crate) fn wrap_target(
  from: Rect,
  direction: Direction,
  candidates: impl Iterator<Item = (UiId, Rect)>,
) -> Option<UiId> {
  let (along, sign) = axis(direction);
  let across = 1 - along;
  let from_across = span(from, across);
  let mut best: Option<(bool, f32, f32, UiId)> = None;
  for (id, rect) in candidates.filter(|(_, rect)| finite(*rect)) {
    let behind = (centre(from, along) - centre(rect, along)) * sign;
    if behind <= 0.0 {
      continue;
    }
    let aligned = gap(from_across, span(rect, across)) <= 0.0;
    let key = (
      !aligned,
      -behind,
      (centre(rect, across) - centre(from, across)).abs(),
      id,
    );
    let wins = best.is_none_or(|b| {
      key
        .0
        .cmp(&b.0)
        .then(key.1.total_cmp(&b.1))
        .then(key.2.total_cmp(&b.2))
        .then(key.3.cmp(&b.3))
        == Ordering::Less
    });
    if wins {
      best = Some(key);
    }
  }
  best.map(|(.., id)| id)
}

/// The axis a direction moves along (0 across the screen, 1 down it) and
/// which way
fn axis(direction: Direction) -> (usize, f32) {
  match direction {
    Direction::Up => (1, -1.0),
    Direction::Down => (1, 1.0),
    Direction::Left => (0, -1.0),
    Direction::Right => (0, 1.0),
  }
}

/// A rect's start and end on an axis
fn span(rect: Rect, axis: usize) -> (f32, f32) {
  if axis == 0 {
    (rect.x, rect.right())
  } else {
    (rect.y, rect.bottom())
  }
}

fn centre(rect: Rect, axis: usize) -> f32 {
  let (start, end) = span(rect, axis);
  (start + end) / 2.0
}

/// The room between two spans (0 when they overlap or touch)
fn gap(a: (f32, f32), b: (f32, f32)) -> f32 {
  (b.0 - a.1).max(a.0 - b.1).max(0.0)
}

fn finite(rect: Rect) -> bool {
  rect.x.is_finite() && rect.y.is_finite() && rect.width.is_finite() && rect.height.is_finite()
}

/// Whether `rect` has positive area inside `clip` (anywhere, without one)
fn shows_in(rect: Rect, clip: Option<Rect>) -> bool {
  if !finite(rect) || rect.width <= 0.0 || rect.height <= 0.0 {
    return false;
  }
  clip.is_none_or(|clip| overlap(rect, clip))
}

/// The part of `rect` inside `clip`
fn clipped(rect: Rect, clip: Rect) -> Rect {
  let (x, y) = (rect.x.max(clip.x), rect.y.max(clip.y));
  let (right, bottom) = (
    rect.right().min(clip.right()),
    rect.bottom().min(clip.bottom()),
  );
  Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
}

/// Whether two rects share positive area
fn overlap(a: Rect, b: Rect) -> bool {
  a.x.max(b.x) < a.right().min(b.right()) && a.y.max(b.y) < a.bottom().min(b.bottom())
}

/// A modal or trapping scope keeps navigation inside it
fn traps(flags: Flags) -> bool {
  flags.intersects(Flags::TRAP | Flags::MODAL)
}

/// How focus is being moved: only a click (or a request) reaches a text
/// field outside any scope
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Via {
  Click,
  Nav,
}

/// The top scene's records from last frame, as navigation sees them
struct Nav<'m> {
  memory: &'m UiMemory,
  scene: Option<u64>,
  /// The focusables inside scopes, by order
  focusables: Vec<(UiId, Record)>,
  /// The focus scopes, by order
  scopes: Vec<(UiId, Record)>,
}

impl<'m> Nav<'m> {
  fn new(memory: &'m UiMemory, scene: Option<u64>) -> Nav<'m> {
    let mut nav = Nav {
      memory,
      scene,
      focusables: Vec::new(),
      scopes: Vec::new(),
    };
    let Some(scene) = scene else {
      return nav;
    };
    for (id, record) in memory.prev_records() {
      if record.scene != scene {
        continue;
      }
      if record.flags.contains(Flags::FOCUS_SCOPE) {
        nav.scopes.push((id, *record));
      }
      if nav.takes_focus(record, Via::Nav) {
        nav.focusables.push((id, *record));
      }
    }
    nav.focusables.sort_by_key(|(id, r)| (r.order, *id));
    nav.scopes.sort_by_key(|(id, r)| (r.order, *id));
    nav
  }

  /// A record of the top scene
  fn record(&self, id: UiId) -> Option<&'m Record> {
    let scene = self.scene?;
    if id == UiId::NONE {
      return None;
    }
    self.memory.prev(id).filter(|r| r.scene == scene)
  }

  /// Whether a record can take the focus `via` a click or navigation: it
  /// is focusable, enabled and shows (a record in a scroll area shows when
  /// its area does), inside a scope unless a click focuses a text field
  fn takes_focus(&self, record: &Record, via: Via) -> bool {
    let flags = record.flags;
    if !flags.contains(Flags::FOCUSABLE) || flags.contains(Flags::DISABLED) {
      return false;
    }
    if record.scope == UiId::NONE && !(via == Via::Click && flags.contains(Flags::TEXT_FIELD)) {
      return false;
    }
    if record.scroll == UiId::NONE {
      return shows_in(record.rect, record.clip);
    }
    // Rows scrolled out of view stay reachable, so the pad can walk a list
    finite(record.rect)
      && record.rect.width > 0.0
      && record.rect.height > 0.0
      && self
        .memory
        .prev(record.scroll)
        .is_some_and(|area| shows_in(area.rect, record.clip))
  }

  fn focusable(&self, id: UiId, via: Via) -> bool {
    self
      .record(id)
      .is_some_and(|record| self.takes_focus(record, via))
  }

  /// The scope a scope record sits in (`NONE` at the top)
  fn parent_scope(&self, scope: UiId) -> UiId {
    self.record(scope).map_or(UiId::NONE, |r| r.scope)
  }

  /// Whether `record` is inside `scope` (every record is inside `NONE`)
  fn inside(&self, record: &Record, scope: UiId) -> bool {
    let mut at = record.scope;
    for _ in 0..MAX_SCOPE_DEPTH {
      if at == scope {
        return true;
      }
      if at == UiId::NONE {
        return false;
      }
      at = self.parent_scope(at);
    }
    false
  }

  /// The focusables inside `scope`, by order
  fn members(&self, scope: UiId) -> impl Iterator<Item = &(UiId, Record)> {
    self
      .focusables
      .iter()
      .filter(move |(_, record)| self.inside(record, scope))
  }

  /// Where focus goes in `scope`: its remembered focus, else its member
  /// marked default, else its first member by order
  fn default_in(&self, scope: UiId) -> Option<UiId> {
    self
      .recalled_in(scope)
      .or_else(|| self.marked_default_in(scope))
      .or_else(|| self.members(scope).next().map(|(id, _)| *id))
  }

  /// Where focus goes in a modal or trapping scope that opens: its member
  /// marked default, else its remembered focus, else its first member. A
  /// dialog or list opens on its default (a confirm on No, a list on its
  /// selection) whatever had the focus when it last closed.
  fn trap_default_in(&self, scope: UiId) -> Option<UiId> {
    self
      .marked_default_in(scope)
      .or_else(|| self.recalled_in(scope))
      .or_else(|| self.members(scope).next().map(|(id, _)| *id))
  }

  /// `scope`'s remembered focus, while it is still one of its members
  fn recalled_in(&self, scope: UiId) -> Option<UiId> {
    let recalled = self.memory.recall_focus(scope);
    recalled.filter(|id| self.members(scope).any(|(m, _)| m == id))
  }

  /// `scope`'s first member marked `.default_focus()`
  fn marked_default_in(&self, scope: UiId) -> Option<UiId> {
    self
      .members(scope)
      .find(|(_, r)| r.flags.contains(Flags::DEFAULT_FOCUS))
      .map(|(id, _)| *id)
  }

  /// The scroll areas around `record`, innermost first
  fn scroll_chain(&self, record: &Record) -> Vec<UiId> {
    let mut chain = Vec::new();
    let mut at = record.scroll;
    while at != UiId::NONE && chain.len() < MAX_SCOPE_DEPTH && !chain.contains(&at) {
      chain.push(at);
      at = self.record(at).map_or(UiId::NONE, |r| r.scroll);
    }
    chain
  }

  /// The widget a move in `direction` from `from` lands on: its explicit
  /// neighbour, else the nearest that way in its scope, else (in a wrapping
  /// scope) the far end, else the same in each scope around it, until a
  /// trapping one. Rows scrolled out of view are candidates only from
  /// inside their own scroll area, so a move from outside a list never
  /// lands on (or past) one it cannot see.
  fn step(&self, from: UiId, direction: Direction) -> Option<UiId> {
    let record = self.record(from)?;
    let explicit = DIRECTIONS
      .iter()
      .position(|d| *d == direction)
      .map_or(UiId::NONE, |k| record.nav[k]);
    if explicit != from && self.focusable(explicit, Via::Nav) {
      return Some(explicit);
    }
    // A candidate in a scroll area `from` is not inside counts by the part
    // of it that shows
    let inside = self.scroll_chain(record);
    let seen = |r: &Record| match r.clip {
      Some(clip) if r.scroll != UiId::NONE && !inside.contains(&r.scroll) => {
        shows_in(r.rect, Some(clip)).then(|| clipped(r.rect, clip))
      }
      _ => Some(r.rect),
    };
    let mut scope = record.scope;
    for _ in 0..MAX_SCOPE_DEPTH {
      let candidates = || {
        self
          .members(scope)
          .filter(|(id, r)| *id != from && !r.flags.contains(Flags::NAV_SKIP))
          .filter_map(|(id, r)| Some((*id, seen(r)?)))
      };
      if let Some(id) = spatial(record.rect, direction, candidates()) {
        return Some(id);
      }
      if scope == UiId::NONE {
        return None;
      }
      let flags = self.record(scope).map_or(Flags::default(), |r| r.flags);
      if flags.contains(Flags::WRAP)
        && let Some(id) = wrap_target(record.rect, direction, candidates())
      {
        return Some(id);
      }
      if traps(flags) {
        return None;
      }
      scope = self.parent_scope(scope);
    }
    None
  }

  /// The next focusable by order after `from` (before it with `back`),
  /// wrapping, inside the innermost trapping scope of `stack` if any
  fn tab(&self, from: UiId, back: bool, stack: &[(UiId, UiId)]) -> Option<UiId> {
    let trap = stack.iter().rev().map(|(scope, _)| *scope).find(|scope| {
      self
        .record(*scope)
        .is_some_and(|record| traps(record.flags))
    });
    let pool: Vec<UiId> = match trap {
      Some(scope) => self.members(scope).map(|(id, _)| *id).collect(),
      None => self.focusables.iter().map(|(id, _)| *id).collect(),
    };
    let last = pool.len().checked_sub(1)?;
    let at = match pool.iter().position(|id| *id == from) {
      Some(at) if back => at.checked_sub(1).unwrap_or(last),
      Some(at) if at == last => 0,
      Some(at) => at + 1,
      None if back => last,
      None => 0,
    };
    pool.get(at).copied()
  }
}

/// Resolve the frame's keyboard and gamepad focus against last frame's
/// records of the top scene (`top_scene`, its scope id), after the pointer
/// resolve: scopes that appeared and closed, requests, the pointer, held
/// directions, Tab, Confirm, Back, tab switching and the right stick. What
/// it used of the input is the outcome, for the engine to hide from
/// gameplay.
pub(crate) fn resolve_focus(
  memory: &mut UiMemory,
  input: &NavInput,
  top_scene: Option<u64>,
  pointer_moved: bool,
) -> NavOutcome {
  let mut outcome = NavOutcome::default();
  let mut known = std::mem::take(&mut memory.known_scopes);
  let mut stack = std::mem::take(&mut memory.modal_stack);
  let request = memory.take_focus_request();
  let dt = if input.dt.is_finite() && input.dt > 0.0 {
    input.dt
  } else {
    0.0
  };

  // Worked on as a copy, so last frame's records stay readable
  let mut i = memory.interact.clone();
  let old = i.focused;
  i.gained = UiId::NONE;
  i.lost = UiId::NONE;
  i.activated = UiId::NONE;
  i.nav_step = None;
  i.back_pressed = false;
  i.back_used = false;
  i.back_taken = false;
  i.tab_step = 0;
  i.tab_taken = false;
  // A direction still held from another scene does nothing until pressed
  if i.nav_scene != top_scene {
    i.repeat = [None; 4];
    i.nav_scene = top_scene;
  }
  // Moved by the pointer (no scrolling into view), else by navigation
  let mut by_pointer = false;

  let (scope, scroll) = {
    let nav = Nav::new(memory, top_scene);

    // Focus on what is gone, disabled or clipped out is lost
    if !nav.focusable(i.focused, Via::Click) {
      i.focused = UiId::NONE;
    }

    // Scopes that appeared take focus: a modal or trap always (into it,
    // even when it has nothing to focus), another only when nothing has it
    // and no trap is open (a modal holding only text keeps the page under
    // it unfocused)
    for (scope, record) in &nav.scopes {
      if known.contains(scope) {
        continue;
      }
      if traps(record.flags) {
        stack.push((*scope, i.focused));
        i.focused = nav.trap_default_in(*scope).unwrap_or(UiId::NONE);
      } else if i.focused == UiId::NONE
        && stack.is_empty()
        && let Some(id) = nav.default_in(*scope)
      {
        i.focused = id;
      }
    }
    // A modal that closed gives focus back to where it was
    while let Some(&(scope, saved)) = stack.last() {
      if nav.scopes.iter().any(|(id, _)| *id == scope) {
        break;
      }
      stack.pop();
      if nav.focusable(saved, Via::Click) {
        i.focused = saved;
      }
    }
    known.clear();
    known.extend(nav.scopes.iter().map(|(id, _)| *id));

    // While a modal or trapping scope is open, neither a request nor the
    // pointer takes the focus out of the innermost one (a press outside an
    // open list closes it by its own rule)
    let trap = stack.last().map(|&(scope, _)| scope);
    let in_trap =
      |id: UiId| trap.is_none_or(|scope| nav.record(id).is_some_and(|r| nav.inside(r, scope)));

    if let Some(id) = request.filter(|id| nav.focusable(*id, Via::Click) && in_trap(*id)) {
      i.focused = id;
    }

    // Focus follows the pointer, but never onto or away from a text field:
    // a field takes the keys only by a press, the keys or the pad, and keeps
    // them while the pointer wanders. A press focuses what it lands on, and
    // a press elsewhere leaves a text field.
    let is_field = |id: UiId| {
      nav
        .record(id)
        .is_some_and(|r| r.flags.contains(Flags::TEXT_FIELD))
    };
    if pointer_moved
      && !is_field(i.focused)
      && !is_field(i.hot)
      && i.hot != i.focused
      && nav.focusable(i.hot, Via::Nav)
      && in_trap(i.hot)
    {
      i.focused = i.hot;
      by_pointer = true;
    }
    if i.pressed {
      if nav.focusable(i.hot, Via::Click) && in_trap(i.hot) {
        if i.hot != i.focused {
          i.focused = i.hot;
          by_pointer = true;
        }
      } else if nav
        .record(i.focused)
        .is_some_and(|r| r.flags.contains(Flags::TEXT_FIELD))
      {
        i.focused = UiId::NONE;
        by_pointer = true;
      }
    }

    // Directions: on press, then repeating while held. A press the UI uses
    // captures its direction until it is released: every held frame is the
    // UI's, so gameplay never sees it between repeats. A direction held from
    // before (gameplay's press) is never captured and never repeats.
    let scoped = !nav.scopes.is_empty();
    for (k, direction) in DIRECTIONS.into_iter().enumerate() {
      let fire = if input.pressed[k] {
        i.repeat[k] = None;
        true
      } else if input.held[k] && i.repeat[k].is_some() {
        repeats(&mut i.repeat[k], dt, input.repeat_rate)
      } else {
        i.repeat[k] = None;
        continue;
      };
      if i.focused == UiId::NONE && !scoped {
        i.repeat[k] = None;
        continue;
      }
      outcome.used.push(DIRECTION_ACTIONS[k]);
      if input.pressed[k] {
        i.repeat[k] = Some(input.repeat_delay);
      }
      if !fire {
        continue;
      }
      let Some(record) = nav.record(i.focused) else {
        // Nothing focused: the innermost modal's default, or the first
        // scope's; a modal with nothing to focus keeps focus off the page
        let target = match stack.last() {
          Some(&(scope, _)) => nav.trap_default_in(scope),
          None => nav.scopes.iter().find_map(|(id, _)| nav.default_in(*id)),
        };
        if let Some(id) = target {
          i.focused = id;
          by_pointer = false;
        }
        continue;
      };
      let takes = match direction {
        Direction::Up | Direction::Down => Flags::TAKES_VERTICAL,
        Direction::Left | Direction::Right => Flags::TAKES_HORIZONTAL,
      };
      if record.flags.contains(takes) {
        i.nav_step = Some((i.focused, direction));
      } else if record.scope == UiId::NONE {
        // A text field outside any scope has no neighbours to move to
      } else if let Some(id) = nav.step(i.focused, direction) {
        i.focused = id;
        by_pointer = false;
      }
    }

    // Tab walks every focusable in order, Shift+Tab back; a text field
    // outside any scope has no order to walk, so it keeps the focus
    let scope_less = nav.record(i.focused).is_some_and(|r| r.scope == UiId::NONE);
    if input.next && !scope_less && (i.focused != UiId::NONE || scoped) {
      outcome.used.push("ui.next");
      if let Some(id) = nav.tab(i.focused, input.shift, &stack)
        && id != i.focused
      {
        i.focused = id;
        by_pointer = false;
      }
    }

    // Confirm activates the focused widget; in a text field it is typing
    // (a space, Enter's own submit), so it activates nothing but stays used.
    // While a payload is held it does nothing, but put a pad's down on a
    // focused target that takes it (a drag source turns its activation into
    // a pick-up itself).
    let typing = is_field(i.focused);
    if input.confirm && i.focused != UiId::NONE {
      if let Some(drag) = i.drag.take() {
        let takes = drag.pad
          && nav.record(i.focused).is_some_and(|r| {
            r.flags.contains(Flags::DROP_TARGET) && r.accepts == Some(drag.type_id)
          });
        if takes {
          i.dropped = Some((i.focused, drag.payload));
        } else {
          i.drag = Some(drag);
        }
      } else if !typing {
        i.activated = i.focused;
      }
      outcome.used.push("ui.confirm");
    }

    // Back is used when a payload is held (it cancels the drag) or the
    // focused widget uses it; otherwise it is left for `ui.back()`
    let record = nav.record(i.focused);
    let dragging = i.drag.is_some();
    if input.back && dragging {
      i.drag = None;
    }
    i.back_pressed = input.back;
    i.back_used =
      input.back && (dragging || record.is_some_and(|r| r.flags.contains(Flags::USES_BACK)));
    if i.back_used {
      outcome.used.push("ui.back");
    }

    // Taken only by a tabs widget (`tab_taken`), so the keys stay the game's
    // with no tabs on screen; E or Q typed into a field switches nothing
    if scoped && !typing {
      i.tab_step = i8::from(input.tab_next) - i8::from(input.tab_prev);
    }

    (
      record.map_or(UiId::NONE, |r| r.scope),
      record.map_or(UiId::NONE, |r| r.scroll),
    )
  };

  if i.focused != old {
    i.gained = i.focused;
    i.lost = old;
    if let Some(scene) = top_scene.filter(|_| scope != UiId::NONE) {
      memory.remember_focus(scope, scene, i.focused);
    }
    // Navigation scrolls what it reaches into view; the pointer is on it
    if !by_pointer && i.focused != UiId::NONE {
      memory.request_scroll(i.focused);
    }
  }
  i.focus_visible = matches!(input.device, InputDevice::Keyboard | InputDevice::Gamepad);
  memory.interact = i;
  memory.known_scopes = known;
  memory.modal_stack = stack;

  // The right stick scrolls the area around the focused widget
  let stick = input.right_stick;
  let tilted = stick.is_finite() && stick.length() > STICK_SCROLL_THRESHOLD;
  if let Some(scene) = top_scene.filter(|_| tilted && scroll != UiId::NONE)
    && let Some(state) = memory.state::<ScrollState>(scroll, scene)
  {
    // +y is up on the stick and down in the UI
    let by = Vec2::new(stick.x, -stick.y) * STICK_SCROLL_SPEED * dt;
    state.target = (state.target + by).clamp(Vec2::ZERO, state.max());
  }
  outcome
}

/// Whether a captured, held direction fires again this frame: after the
/// repeat delay, then every interval. `timer` is the time left until it
/// fires, armed by the press the UI used.
fn repeats(timer: &mut Option<f32>, dt: f32, rate: f32) -> bool {
  let Some(left) = timer.as_mut() else {
    return false;
  };
  *left -= dt;
  if *left > REPEAT_SLACK {
    return false;
  }
  *left = (*left + rate).max(0.0);
  true
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::input::Direction;
  use crate::core::ui::{Rect, UiId};

  fn cands(rects: &[(u64, Rect)]) -> impl Iterator<Item = (UiId, Rect)> + '_ {
    rects.iter().map(|(id, r)| (UiId(*id), *r))
  }

  const ROW: f32 = 40.0;

  fn list() -> Vec<(u64, Rect)> {
    (0..4)
      .map(|i| (i + 1, Rect::new(0.0, i as f32 * ROW, 100.0, 30.0)))
      .collect()
  }

  #[test]
  fn down_in_a_list_picks_the_next_row() {
    let rows = list();
    assert_eq!(
      spatial(rows[0].1, Direction::Down, cands(&rows[1..])),
      Some(UiId(2))
    );
    assert_eq!(
      spatial(rows[2].1, Direction::Up, cands(&rows)),
      Some(UiId(2))
    );
    assert_eq!(
      spatial(rows[3].1, Direction::Down, cands(&rows)),
      None,
      "nothing beyond"
    );
  }

  #[test]
  fn right_in_a_grid_stays_on_the_row() {
    let cells: Vec<(u64, Rect)> = (0..12)
      .map(|i| {
        (
          i + 1,
          Rect::new((i % 4) as f32 * 52.0, (i / 4) as f32 * 52.0, 48.0, 48.0),
        )
      })
      .collect();
    assert_eq!(
      spatial(cells[5].1, Direction::Right, cands(&cells)),
      Some(UiId(7))
    );
    assert_eq!(
      spatial(cells[5].1, Direction::Down, cands(&cells)),
      Some(UiId(10))
    );
    assert_eq!(spatial(cells[4].1, Direction::Left, cands(&cells)), None);
  }

  #[test]
  fn an_aligned_candidate_beats_a_nearer_diagonal() {
    let from = Rect::new(0.0, 0.0, 50.0, 30.0);
    let far_aligned = (1, Rect::new(0.0, 200.0, 50.0, 30.0));
    let near_diagonal = (2, Rect::new(60.0, 40.0, 50.0, 30.0));
    assert_eq!(
      spatial(from, Direction::Down, cands(&[far_aligned, near_diagonal])),
      Some(UiId(1))
    );
  }

  #[test]
  fn wrapping_goes_to_the_far_end() {
    let rows = list();
    assert_eq!(
      wrap_target(rows[3].1, Direction::Down, cands(&rows)),
      Some(UiId(1))
    );
    assert_eq!(
      wrap_target(rows[0].1, Direction::Up, cands(&rows)),
      Some(UiId(4))
    );
  }

  use crate::core::ui::memory::{Flags, Record, UiMemory};

  const SCOPE: UiId = UiId(100);

  fn rec(rect: Rect, order: u32, flags: Flags, scope: UiId) -> Record {
    Record {
      rect,
      clip: None,
      layer: 0,
      order,
      depth: 1,
      scene: 1,
      flags,
      scope,
      scroll: UiId::NONE,
      content: Vec2::ZERO,
      nav: [UiId::NONE; 4],
      accepts: None,
    }
  }

  /// A scope (id 100) and the rows in it (ids from 1), a row's flags each
  fn menu(rows: &[Flags]) -> Vec<(u64, Record)> {
    let mut records = vec![(
      100,
      rec(
        Rect::new(0.0, 0.0, 200.0, 400.0),
        0,
        Flags::FOCUS_SCOPE,
        UiId::NONE,
      ),
    )];
    for (i, flags) in rows.iter().enumerate() {
      let rect = Rect::new(0.0, i as f32 * ROW, 100.0, 30.0);
      records.push((i as u64 + 1, rec(rect, i as u32 + 1, *flags, SCOPE)));
    }
    records
  }

  /// Make `records` last frame's
  fn draw(memory: &mut UiMemory, records: &[(u64, Record)]) {
    for (id, record) in records {
      memory.record(UiId(*id), *record);
    }
    memory.end_frame();
    memory.begin_frame();
  }

  fn resolve(memory: &mut UiMemory, input: NavInput) -> NavOutcome {
    resolve_focus(memory, &input, Some(1), false)
  }

  fn press(direction: usize) -> NavInput {
    let mut input = NavInput::default();
    input.pressed[direction] = true;
    input.held[direction] = true;
    input
  }

  const FOCUSABLE: Flags = Flags::FOCUSABLE;

  #[test]
  fn a_widget_that_takes_a_direction_gets_it_as_a_step() {
    let mut m = UiMemory::new();
    m.begin_frame();
    let slider = FOCUSABLE | Flags::TAKES_HORIZONTAL | Flags::DEFAULT_FOCUS;
    draw(&mut m, &menu(&[slider, FOCUSABLE]));
    resolve(&mut m, NavInput::default());
    assert_eq!(m.interact.focused, UiId(1));
    let out = resolve(&mut m, press(3));
    assert_eq!(m.interact.nav_step, Some((UiId(1), Direction::Right)));
    assert_eq!(m.interact.focused, UiId(1), "it stays");
    assert_eq!(out.used, vec!["ui.right"], "and gameplay does not see it");
    resolve(&mut m, press(1));
    assert_eq!(m.interact.nav_step, None);
    assert_eq!(m.interact.focused, UiId(2), "Down still moves");
  }

  #[test]
  fn back_a_focused_widget_uses_is_taken_from_gameplay() {
    let mut m = UiMemory::new();
    m.begin_frame();
    draw(
      &mut m,
      &menu(&[
        FOCUSABLE | Flags::USES_BACK | Flags::DEFAULT_FOCUS,
        FOCUSABLE,
      ]),
    );
    resolve(&mut m, NavInput::default());
    let back = NavInput {
      back: true,
      ..NavInput::default()
    };
    let out = resolve(&mut m, back);
    assert!(m.interact.back_pressed && m.interact.back_used);
    assert_eq!(out.used, vec!["ui.back"]);
    resolve(&mut m, press(1));
    let out = resolve(&mut m, back);
    assert!(m.interact.back_pressed && !m.interact.back_used);
    assert!(out.used.is_empty(), "free for `ui.back()`");
  }

  #[test]
  fn a_text_field_outside_a_scope_takes_focus_by_click_only() {
    let mut m = UiMemory::new();
    m.begin_frame();
    let field = Flags::CLICK | FOCUSABLE | Flags::TEXT_FIELD;
    draw(
      &mut m,
      &[
        (
          1,
          rec(Rect::new(0.0, 0.0, 100.0, 30.0), 1, field, UiId::NONE),
        ),
        (
          2,
          rec(
            Rect::new(0.0, 40.0, 100.0, 30.0),
            2,
            Flags::CLICK,
            UiId::NONE,
          ),
        ),
      ],
    );
    m.interact.hot = UiId(1);
    resolve_focus(&mut m, &NavInput::default(), Some(1), true);
    assert_eq!(m.interact.focused, UiId::NONE, "not by hovering");
    m.interact.pressed = true;
    resolve(&mut m, NavInput::default());
    assert_eq!(m.interact.focused, UiId(1), "by a click");
    m.interact.hot = UiId(2);
    resolve(&mut m, NavInput::default());
    assert_eq!(
      m.interact.focused,
      UiId::NONE,
      "a press elsewhere leaves it"
    );
  }

  #[test]
  fn a_closed_modal_gives_focus_back() {
    let mut m = UiMemory::new();
    m.begin_frame();
    let page = menu(&[FOCUSABLE, FOCUSABLE | Flags::DEFAULT_FOCUS]);
    draw(&mut m, &page);
    resolve(&mut m, NavInput::default());
    assert_eq!(m.interact.focused, UiId(2));
    let mut open = page.clone();
    let modal = Flags::FOCUS_SCOPE | Flags::TRAP | Flags::MODAL;
    open.push((
      200,
      rec(Rect::new(50.0, 50.0, 100.0, 100.0), 10, modal, UiId::NONE),
    ));
    open.push((
      7,
      rec(Rect::new(60.0, 60.0, 80.0, 30.0), 11, FOCUSABLE, UiId(200)),
    ));
    draw(&mut m, &open);
    resolve(&mut m, NavInput::default());
    assert_eq!(m.interact.focused, UiId(7), "into the modal");
    resolve(&mut m, press(1));
    assert_eq!(m.interact.focused, UiId(7), "the page is not a target");
    draw(&mut m, &page);
    resolve(&mut m, NavInput::default());
    assert_eq!(m.interact.focused, UiId(2), "back where it was");
    assert_eq!(m.interact.gained, UiId(2));
  }

  #[test]
  fn a_page_scope_appearing_under_an_empty_modal_takes_no_focus() {
    let mut m = UiMemory::new();
    m.begin_frame();
    let modal = Flags::FOCUS_SCOPE | Flags::TRAP | Flags::MODAL;
    // A modal holding only text: nothing in it takes the focus
    let dialog = (
      200,
      rec(Rect::new(50.0, 50.0, 100.0, 100.0), 10, modal, UiId::NONE),
    );
    draw(&mut m, &[dialog]);
    resolve(&mut m, NavInput::default());
    assert_eq!(m.modal_stack.len(), 1, "the modal is open");
    assert_eq!(m.interact.focused, UiId::NONE);
    let mut both = menu(&[FOCUSABLE | Flags::DEFAULT_FOCUS, FOCUSABLE]);
    both.push(dialog);
    draw(&mut m, &both);
    resolve(&mut m, NavInput::default());
    assert_eq!(
      m.interact.focused,
      UiId::NONE,
      "the page appearing under the modal takes no focus"
    );
    draw(&mut m, &both);
    let confirm = NavInput {
      confirm: true,
      ..NavInput::default()
    };
    resolve(&mut m, confirm);
    assert_eq!(
      m.interact.activated,
      UiId::NONE,
      "Confirm acts on nothing under it"
    );
  }

  #[test]
  fn an_open_trap_keeps_requests_presses_and_the_pointer_inside() {
    let mut m = UiMemory::new();
    m.begin_frame();
    let mut open = menu(&[FOCUSABLE, FOCUSABLE | Flags::DEFAULT_FOCUS]);
    let trap = Flags::FOCUS_SCOPE | Flags::TRAP;
    open.push((
      200,
      rec(Rect::new(150.0, 0.0, 100.0, 100.0), 10, trap, UiId::NONE),
    ));
    open.push((
      7,
      rec(Rect::new(160.0, 10.0, 80.0, 30.0), 11, FOCUSABLE, UiId(200)),
    ));
    draw(&mut m, &open);
    resolve(&mut m, NavInput::default());
    assert_eq!(m.interact.focused, UiId(7), "into the trap");
    m.request_focus(UiId(1));
    draw(&mut m, &open);
    resolve(&mut m, NavInput::default());
    assert_eq!(m.interact.focused, UiId(7), "a request outside is refused");
    m.interact.hot = UiId(1);
    draw(&mut m, &open);
    resolve_focus(&mut m, &NavInput::default(), Some(1), true);
    assert_eq!(
      m.interact.focused,
      UiId(7),
      "the pointer does not pull it out"
    );
    m.interact.pressed = true;
    draw(&mut m, &open);
    resolve(&mut m, NavInput::default());
    assert_eq!(m.interact.focused, UiId(7), "nor does a press");
  }

  #[test]
  fn a_trap_opens_on_its_default_not_its_memory() {
    let mut m = UiMemory::new();
    m.begin_frame();
    let page = menu(&[FOCUSABLE]);
    let mut open = page.clone();
    let trap = Flags::FOCUS_SCOPE | Flags::TRAP;
    open.push((
      200,
      rec(Rect::new(150.0, 0.0, 100.0, 100.0), 10, trap, UiId::NONE),
    ));
    for (id, y, flags) in [
      (7, 10.0, FOCUSABLE),
      (8, 50.0, FOCUSABLE | Flags::DEFAULT_FOCUS),
    ] {
      open.push((
        id,
        rec(
          Rect::new(160.0, y, 80.0, 30.0),
          id as u32 + 10,
          flags,
          UiId(200),
        ),
      ));
    }
    draw(&mut m, &open);
    resolve(&mut m, NavInput::default());
    assert_eq!(m.interact.focused, UiId(8));
    draw(&mut m, &open);
    resolve(&mut m, press(0));
    assert_eq!(m.interact.focused, UiId(7), "remembered");
    draw(&mut m, &page);
    resolve(&mut m, NavInput::default());
    draw(&mut m, &open);
    resolve(&mut m, NavInput::default());
    assert_eq!(m.interact.focused, UiId(8), "opened again on its default");
  }

  #[test]
  fn a_focused_text_field_switches_no_tabs_and_is_not_activated() {
    let mut m = UiMemory::new();
    m.begin_frame();
    let field = Flags::CLICK | FOCUSABLE | Flags::TEXT_FIELD | Flags::DEFAULT_FOCUS;
    draw(&mut m, &menu(&[field, FOCUSABLE]));
    resolve(&mut m, NavInput::default());
    assert_eq!(m.interact.focused, UiId(1));
    // E (or Q) typed into the field is not a tab switch
    let tab_next = NavInput {
      tab_next: true,
      ..NavInput::default()
    };
    resolve(&mut m, tab_next);
    assert_eq!(m.interact.tab_step, 0, "typing E switches no tab");
    // Confirm (Space, Enter) is typing too: used, but no click
    let confirm = NavInput {
      confirm: true,
      ..NavInput::default()
    };
    let out = resolve(&mut m, confirm);
    assert_eq!(m.interact.activated, UiId::NONE);
    assert!(out.used.contains(&"ui.confirm"));
    // Off the field, both work again
    resolve(&mut m, press(1));
    assert_eq!(m.interact.focused, UiId(2));
    resolve(&mut m, tab_next);
    assert_eq!(m.interact.tab_step, 1);
    resolve(&mut m, confirm);
    assert_eq!(m.interact.activated, UiId(2));
  }

  #[test]
  fn tab_switching_steps_only_while_a_scope_is_shown() {
    let mut m = UiMemory::new();
    m.begin_frame();
    let tab_next = NavInput {
      tab_next: true,
      ..NavInput::default()
    };
    draw(
      &mut m,
      &[(
        1,
        rec(Rect::new(0.0, 0.0, 100.0, 30.0), 1, FOCUSABLE, UiId::NONE),
      )],
    );
    resolve(&mut m, tab_next);
    assert_eq!(m.interact.tab_step, 0, "no scope: the keys are the game's");
    draw(&mut m, &menu(&[FOCUSABLE]));
    let out = resolve(&mut m, tab_next);
    assert_eq!(m.interact.tab_step, 1);
    assert!(out.used.is_empty(), "taken only by a tabs widget");
    let tab_prev = NavInput {
      tab_prev: true,
      ..NavInput::default()
    };
    resolve(&mut m, tab_prev);
    assert_eq!(m.interact.tab_step, -1);
  }
}
