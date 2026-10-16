//! The pointer resolve: once per engine frame, before any scene's `ui`, the
//! pointer is hit-tested against last frame's laid-out records

use glam::Vec2;

use super::memory::{Flags, Record, UiMemory};
use super::widgets::scroll::ScrollState;
use super::{Rect, UiId};

/// Two clicks on one widget this close together (seconds) are a double click
const DOUBLE_CLICK: f64 = 0.3;

/// The pointer as the resolve reads it: in UI units, with the left (0) and
/// right (1) buttons
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PointerInput {
  pub(crate) position: Vec2,
  /// Went down this frame
  pub(crate) pressed: [bool; 2],
  /// Went up this frame
  pub(crate) released: [bool; 2],
  /// Held
  pub(crate) down: [bool; 2],
  /// Real time in seconds (for double clicks)
  pub(crate) time: f64,
}

/// What the resolve took from the frame's input
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PointerOutcome {
  /// The left (0) or right (1) press was over the UI: gameplay must not see it
  pub(crate) consume_press: [bool; 2],
  /// A scroll area took the wheel: gameplay must not see it
  pub(crate) consume_wheel: bool,
}

/// Resolve the frame's pointer against last frame's records of the top
/// scene (`top_scene`, its scope id) into `memory.interact`: what is hot,
/// active, clicked or dragged. The pointer inside a `blockers` rect (the
/// console) is nobody's. A press must move more than `drag_threshold` UI
/// units to start a drag.
pub(crate) fn resolve_pointer(
  memory: &mut UiMemory,
  pointer: &PointerInput,
  top_scene: Option<u64>,
  blockers: &[Rect],
  drag_threshold: f32,
) -> PointerOutcome {
  let position = pointer.position;
  let blocked = blockers.iter().any(|b| b.contains(position.x, position.y));
  let hit = match top_scene {
    Some(scene) if !blocked => hit_test(memory, position, scene),
    _ => Hit::default(),
  };
  let hot = hit.hot;
  let over_ui = hit.over_ui;

  // Worked on as a copy, so last frame's records stay readable
  let mut i = memory.interact;
  // One-frame results last until the next resolve
  i.clicked = UiId::NONE;
  i.secondary = UiId::NONE;
  i.double = UiId::NONE;
  i.drag_started = UiId::NONE;
  i.drag_released = UiId::NONE;
  i.prev_pointer = i.pointer;
  i.pointer = position;
  i.hot = hot;
  i.hovered = hit.hovered;
  i.over_ui = over_ui;

  // A widget gone since its press reports nothing more
  if i.active != UiId::NONE && memory.prev(i.active).is_none() {
    i.active = UiId::NONE;
    i.dragging = false;
  }

  let mut outcome = PointerOutcome::default();
  for button in 0..2 {
    if !pointer.pressed[button] {
      continue;
    }
    outcome.consume_press[button] = over_ui;
    // A second button's press leaves the first's widget active
    if i.active == UiId::NONE || usize::from(i.active_button) == button {
      i.active = hot;
      i.active_button = button as u8;
      i.press_pos = position;
      i.dragging = false;
    }
  }

  // Only a widget that senses drags is dragged: a click-only one still
  // clicks when released over itself, however far it moved within it
  let drags = memory
    .prev(i.active)
    .is_some_and(|record| record.flags.contains(Flags::DRAG));
  if drags && !i.dragging && position.distance(i.press_pos) > drag_threshold {
    i.dragging = true;
    i.drag_started = i.active;
  }

  let button = usize::from(i.active_button).min(1);
  if i.active != UiId::NONE && pointer.released[button] {
    if i.dragging {
      i.drag_released = i.active;
    } else if hot == i.active {
      if button == 0 {
        i.clicked = i.active;
        let (last, at) = i.last_click;
        if last == i.active && pointer.time - at <= DOUBLE_CLICK {
          i.double = i.active;
          i.last_click = (UiId::NONE, 0.0);
        } else {
          i.last_click = (i.active, pointer.time);
        }
      } else {
        i.secondary = i.active;
      }
    }
    i.active = UiId::NONE;
    i.dragging = false;
  } else if i.active != UiId::NONE && !pointer.down[button] {
    // The release was missed (the window lost focus while held)
    i.active = UiId::NONE;
    i.dragging = false;
  }
  memory.interact = i;
  outcome
}

/// Give the frame's wheel (`wheel`, in lines, +y up) to the scroll areas of
/// `top_scene` under `pointer`, from last frame's records. Per axis, the
/// innermost area that can still move toward the wheel takes it (its
/// `target` moves `line` units per line); an area at its edge passes the
/// wheel to the one around it. Shift turns a vertical-only wheel sideways.
/// Whether anything took the wheel, so gameplay must not see it.
pub(crate) fn route_wheel(
  memory: &mut UiMemory,
  pointer: Vec2,
  wheel: Vec2,
  shift: bool,
  top_scene: Option<u64>,
  line: f32,
) -> bool {
  let Some(scene) = top_scene else {
    return false;
  };
  let mut wheel = if shift && wheel.x == 0.0 {
    Vec2::new(wheel.y, 0.0)
  } else {
    wheel
  };
  if !wheel.is_finite() || wheel == Vec2::ZERO {
    return false;
  }
  // The topmost thing under the pointer decides, as for clicks, so a panel
  // or popup over a scroll area keeps the wheel from it. Its scroll chain,
  // innermost area first, takes the wheel in turn.
  let Some((top, record)) = topmost(memory, pointer, scene) else {
    return false;
  };
  let mut chain = Vec::new();
  let mut next = if record.flags.contains(Flags::SCROLLABLE) {
    top
  } else {
    record.scroll
  };
  while next != UiId::NONE && chain.len() < MAX_SCROLL_CHAIN && !chain.contains(&next) {
    chain.push(next);
    next = memory.prev(next).map_or(UiId::NONE, |r| r.scroll);
  }

  let mut taken = false;
  for id in chain {
    let Some(state) = memory.state::<ScrollState>(id, scene) else {
      continue;
    };
    let max = state.max();
    for axis in 0..2 {
      if wheel[axis] == 0.0 {
        continue;
      }
      let delta = -wheel[axis] * line;
      let room = if delta < 0.0 {
        state.target[axis] > 0.0
      } else {
        state.target[axis] < max[axis]
      };
      if room {
        state.target[axis] = (state.target[axis] + delta).clamp(0.0, max[axis]);
        wheel[axis] = 0.0;
        taken = true;
      }
    }
    if wheel == Vec2::ZERO {
      break;
    }
  }
  taken
}

/// Carry out the `scroll_to` requests made last frame: each scroll area
/// around a requested widget moves its `target` the least that brings the
/// widget's rect (as last frame laid it out) into view.
pub(crate) fn apply_scroll_requests(memory: &mut UiMemory) {
  for id in memory.take_scroll_requests() {
    let Some(record) = memory.prev(id).copied() else {
      continue;
    };
    let Some(area) = memory.prev(record.scroll).copied() else {
      continue;
    };
    let Some(state) = memory.state::<ScrollState>(record.scroll, area.scene) else {
      continue;
    };
    let max = state.max();
    // Last frame's rects were laid out at the offset the state still holds
    let from = Vec2::new(record.rect.x - area.rect.x, record.rect.y - area.rect.y) + state.offset;
    let size = Vec2::new(record.rect.width, record.rect.height);
    for axis in 0..2 {
      let (start, end) = (from[axis], from[axis] + size[axis]);
      let (seen, view) = (state.offset[axis], state.view[axis]);
      let wanted = if start < seen {
        start
      } else if end > seen + view {
        end - view
      } else {
        continue;
      };
      state.target[axis] = wanted.clamp(0.0, max[axis]);
    }
  }
}

/// The longest scroll chain the wheel walks
const MAX_SCROLL_CHAIN: usize = 64;

/// The topmost record of `scene` under `at` that takes the pointer (layer,
/// then order; depth and id only break ties)
fn topmost(memory: &UiMemory, at: Vec2, scene: u64) -> Option<(UiId, Record)> {
  let takes =
    Flags::CLICK | Flags::DRAG | Flags::BLOCKING | Flags::SCROLLABLE | Flags::DROP_TARGET;
  memory
    .prev_records()
    .filter(|(_, r)| {
      r.scene == scene
        && r.flags.intersects(takes)
        && r.rect.contains(at.x, at.y)
        && r.clip.is_none_or(|clip| clip.contains(at.x, at.y))
    })
    .max_by_key(|(id, r)| (r.layer, r.order, r.depth, *id))
    .map(|(id, r)| (id, *r))
}

/// What is under the pointer
#[derive(Debug, Default)]
struct Hit {
  /// The topmost sensing, enabled widget, unless something above blocks it
  hot: UiId,
  /// The topmost record, hover-only ones included
  hovered: UiId,
  /// Any record that takes the pointer is under it
  over_ui: bool,
}

/// What is under `at` among `scene`'s records from last frame. The topmost
/// record (layer, then order) that takes the pointer decides what is hot:
/// one that senses and is enabled is; a panel, scroll area or disabled
/// widget blocks what is under it. Hover-only records take no part in that:
/// they are only ever `hovered`, the topmost of everything under `at`.
fn hit_test(memory: &UiMemory, at: Vec2, scene: u64) -> Hit {
  // What senses the pointer, or stops it
  let takes = Flags::CLICK | Flags::DRAG | Flags::BLOCKING | Flags::SCROLLABLE | Flags::DROP_TARGET;
  // Depth and id only break ties, so the result never depends on map order
  let mut top: Option<((u8, u32, u16, UiId), Flags)> = None;
  let mut hovered: Option<(u8, u32, u16, UiId)> = None;
  for (id, r) in memory.prev_records() {
    let under = r.scene == scene
      && r.rect.contains(at.x, at.y)
      && r.clip.is_none_or(|clip| clip.contains(at.x, at.y));
    if !under {
      continue;
    }
    let key = (r.layer, r.order, r.depth, id);
    if r.flags.intersects(takes) && top.is_none_or(|(above, _)| key > above) {
      top = Some((key, r.flags));
    }
    if r.flags.intersects(takes | Flags::HOVER) && hovered.is_none_or(|above| key > above) {
      hovered = Some(key);
    }
  }
  let hot = top.and_then(|((.., id), flags)| {
    let senses = flags.intersects(Flags::CLICK | Flags::DRAG);
    (senses && !flags.contains(Flags::DISABLED)).then_some(id)
  });
  Hit {
    hot: hot.unwrap_or(UiId::NONE),
    hovered: hovered.map_or(UiId::NONE, |(.., id)| id),
    over_ui: top.is_some(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::memory::{Flags, Record, UiMemory};
  use crate::core::ui::{Rect, UiId};
  use glam::Vec2;

  fn rec(rect: Rect, layer: u8, order: u32, flags: Flags, scene: u64) -> Record {
    Record {
      rect,
      clip: None,
      layer,
      order,
      depth: 0,
      scene,
      flags,
      scope: UiId::NONE,
      scroll: UiId::NONE,
      content: Vec2::ZERO,
      nav: [UiId::NONE; 4],
    }
  }

  /// Records drawn "last frame", then a fresh frame to resolve against
  fn memory(records: &[(u64, Record)]) -> UiMemory {
    let mut memory = UiMemory::new();
    memory.begin_frame();
    for (id, r) in records {
      memory.record(UiId(*id), *r);
    }
    memory.end_frame();
    memory.begin_frame();
    memory
  }

  fn at(x: f32, y: f32) -> PointerInput {
    PointerInput {
      position: Vec2::new(x, y),
      pressed: [false; 2],
      released: [false; 2],
      down: [false; 2],
      time: 0.0,
    }
  }

  fn press(x: f32, y: f32) -> PointerInput {
    PointerInput {
      pressed: [true, false],
      down: [true, false],
      ..at(x, y)
    }
  }

  fn release(x: f32, y: f32) -> PointerInput {
    PointerInput {
      released: [true, false],
      ..at(x, y)
    }
  }

  const BUTTON: Rect = Rect::new(10.0, 10.0, 50.0, 20.0);

  #[test]
  fn the_topmost_widget_under_the_pointer_is_hot() {
    let mut m = memory(&[
      (1, rec(BUTTON, 0, 1, Flags::CLICK, 1)),
      (2, rec(BUTTON, 0, 2, Flags::CLICK, 1)),
    ]);
    resolve_pointer(&mut m, &at(20.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.hot, UiId(2));
  }

  #[test]
  fn a_blocking_panel_hides_widgets_under_it() {
    let mut m = memory(&[
      (1, rec(BUTTON, 0, 1, Flags::CLICK, 1)),
      (
        2,
        rec(Rect::new(0.0, 0.0, 100.0, 100.0), 0, 2, Flags::BLOCKING, 1),
      ),
    ]);
    let out = resolve_pointer(&mut m, &press(20.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.hot, UiId::NONE);
    assert!(m.interact.over_ui);
    assert!(out.consume_press[0], "a press on a panel is the UI's");
  }

  #[test]
  fn a_clipped_out_widget_is_not_hit() {
    let mut clipped = rec(BUTTON, 0, 1, Flags::CLICK, 1);
    clipped.clip = Some(Rect::new(0.0, 0.0, 15.0, 15.0));
    let mut m = memory(&[(1, clipped)]);
    resolve_pointer(&mut m, &at(40.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.hot, UiId::NONE);
    resolve_pointer(&mut m, &at(12.0, 12.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.hot, UiId(1));
  }

  #[test]
  fn a_disabled_widget_is_never_hot_and_blocks() {
    let mut m = memory(&[
      (1, rec(BUTTON, 0, 1, Flags::CLICK, 1)),
      (2, rec(BUTTON, 0, 2, Flags::CLICK | Flags::DISABLED, 1)),
    ]);
    resolve_pointer(&mut m, &at(20.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.hot, UiId::NONE);
  }

  #[test]
  fn a_click_fires_on_release_over_the_pressed_widget() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    resolve_pointer(&mut m, &press(20.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.active, UiId(1));
    assert_eq!(m.interact.clicked, UiId::NONE, "not on press");
    m.end_frame();
    m.begin_frame();
    m.record(UiId(1), rec(BUTTON, 0, 1, Flags::CLICK, 1));
    m.end_frame();
    m.begin_frame();
    resolve_pointer(&mut m, &release(21.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.clicked, UiId(1));
    assert_eq!(m.interact.active, UiId::NONE);
  }

  #[test]
  fn dragging_off_before_release_cancels_the_click() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    resolve_pointer(&mut m, &press(20.0, 20.0), Some(1), &[], 4.0);
    // Still recorded next frame
    let r = *m.prev(UiId(1)).unwrap();
    m.record(UiId(1), r);
    m.end_frame();
    m.begin_frame();
    resolve_pointer(&mut m, &release(200.0, 200.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.clicked, UiId::NONE);
  }

  #[test]
  fn a_drag_starts_past_four_units() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::DRAG, 1))]);
    resolve_pointer(&mut m, &press(20.0, 20.0), Some(1), &[], 4.0);
    let held = |x| PointerInput {
      down: [true, false],
      ..at(x, 20.0)
    };
    resolve_pointer(&mut m, &held(23.0), Some(1), &[], 4.0);
    assert!(!m.interact.dragging);
    resolve_pointer(&mut m, &held(25.0), Some(1), &[], 4.0);
    assert!(m.interact.dragging);
    assert_eq!(m.interact.drag_started, UiId(1));
  }

  #[test]
  fn a_click_only_button_clicks_after_moving_within_it() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    resolve_pointer(&mut m, &press(15.0, 15.0), Some(1), &[], 4.0);
    let held = PointerInput {
      down: [true, false],
      ..at(25.0, 20.0)
    };
    resolve_pointer(&mut m, &held, Some(1), &[], 4.0);
    assert!(!m.interact.dragging, "it does not sense drags");
    assert_eq!(m.interact.drag_started, UiId::NONE);
    resolve_pointer(&mut m, &release(30.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.clicked, UiId(1), "released over itself");
  }

  #[test]
  fn two_quick_clicks_are_a_double_click() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    for (t, double) in [(0.0, false), (0.2, true)] {
      resolve_pointer(
        &mut m,
        &PointerInput {
          time: t,
          ..press(20.0, 20.0)
        },
        Some(1),
        &[],
        4.0,
      );
      resolve_pointer(
        &mut m,
        &PointerInput {
          time: t + 0.05,
          ..release(20.0, 20.0)
        },
        Some(1),
        &[],
        4.0,
      );
      assert_eq!(m.interact.double == UiId(1), double);
    }
  }

  #[test]
  fn a_secondary_click_is_its_own() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    resolve_pointer(
      &mut m,
      &PointerInput {
        pressed: [false, true],
        down: [false, true],
        ..at(20.0, 20.0)
      },
      Some(1),
      &[],
      4.0,
    );
    resolve_pointer(
      &mut m,
      &PointerInput {
        released: [false, true],
        ..at(20.0, 20.0)
      },
      Some(1),
      &[],
      4.0,
    );
    assert_eq!(m.interact.secondary, UiId(1));
    assert_eq!(m.interact.clicked, UiId::NONE);
  }

  #[test]
  fn a_lower_scene_gets_no_pointer() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 7))]);
    resolve_pointer(&mut m, &at(20.0, 20.0), Some(8), &[], 4.0);
    assert_eq!(m.interact.hot, UiId::NONE);
  }

  #[test]
  fn a_widget_gone_this_frame_reports_nothing() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    resolve_pointer(&mut m, &press(20.0, 20.0), Some(1), &[], 4.0);
    m.end_frame();
    m.begin_frame(); // the button was not recorded: it is gone
    resolve_pointer(&mut m, &release(20.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.clicked, UiId::NONE);
    assert_eq!(m.interact.active, UiId::NONE);
  }

  #[test]
  fn a_click_lands_on_what_was_drawn_last_frame() {
    // The button moved this frame, but the player clicked where they saw it
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    resolve_pointer(&mut m, &press(12.0, 12.0), Some(1), &[], 4.0);
    m.record(
      UiId(1),
      rec(Rect::new(200.0, 200.0, 50.0, 20.0), 0, 1, Flags::CLICK, 1),
    );
    m.end_frame();
    m.begin_frame();
    m.record(UiId(1), rec(BUTTON, 0, 1, Flags::CLICK, 1));
    m.end_frame();
    m.begin_frame();
    resolve_pointer(&mut m, &release(12.0, 12.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.clicked, UiId(1));
  }

  #[test]
  fn the_console_blocks_the_pointer() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    let out = resolve_pointer(
      &mut m,
      &press(20.0, 20.0),
      Some(1),
      &[Rect::new(0.0, 0.0, 100.0, 100.0)],
      4.0,
    );
    assert_eq!(m.interact.hot, UiId::NONE);
    assert!(!out.consume_press[0], "the console takes its own clicks");
  }

  fn scrollable(rect: Rect, depth: u16, id: u64, m: &mut UiMemory, content: Vec2) {
    let mut r = rec(rect, 0, id as u32, Flags::SCROLLABLE, 1);
    r.depth = depth;
    // A deeper area sits inside the one with the id before it
    if depth > 0 {
      r.scroll = UiId(id - 1);
    }
    r.content = content;
    m.record(UiId(id), r);
    let state = m
      .state::<crate::core::ui::widgets::scroll::ScrollState>(UiId(id), 1)
      .unwrap();
    state.view = Vec2::new(rect.width, rect.height);
    state.content = content;
  }

  fn target(m: &UiMemory, id: u64) -> Vec2 {
    m.peek_state::<crate::core::ui::widgets::scroll::ScrollState>(UiId(id))
      .unwrap()
      .target
  }

  #[test]
  fn the_innermost_scrollable_takes_the_wheel() {
    let mut m = UiMemory::new();
    m.begin_frame();
    scrollable(
      Rect::new(0.0, 0.0, 200.0, 200.0),
      0,
      1,
      &mut m,
      Vec2::new(200.0, 1000.0),
    );
    scrollable(
      Rect::new(10.0, 10.0, 100.0, 100.0),
      3,
      2,
      &mut m,
      Vec2::new(100.0, 500.0),
    );
    m.end_frame();
    m.begin_frame();
    assert!(route_wheel(
      &mut m,
      Vec2::new(20.0, 20.0),
      Vec2::new(0.0, -1.0),
      false,
      Some(1),
      40.0
    ));
    assert_eq!((target(&m, 2).y, target(&m, 1).y), (40.0, 0.0));
  }

  #[test]
  fn a_scrollable_at_its_edge_passes_the_wheel_out() {
    let mut m = UiMemory::new();
    m.begin_frame();
    scrollable(
      Rect::new(0.0, 0.0, 200.0, 200.0),
      0,
      1,
      &mut m,
      Vec2::new(200.0, 1000.0),
    );
    scrollable(
      Rect::new(10.0, 10.0, 100.0, 100.0),
      3,
      2,
      &mut m,
      Vec2::new(100.0, 500.0),
    );
    m.end_frame();
    m.begin_frame();
    // The inner one is at its top; scrolling up goes to the outer one, which is at its top too
    assert!(!route_wheel(
      &mut m,
      Vec2::new(20.0, 20.0),
      Vec2::new(0.0, 1.0),
      false,
      Some(1),
      40.0
    ));
    m.state::<crate::core::ui::widgets::scroll::ScrollState>(UiId(1), 1)
      .unwrap()
      .target
      .y = 100.0;
    assert!(route_wheel(
      &mut m,
      Vec2::new(20.0, 20.0),
      Vec2::new(0.0, 1.0),
      false,
      Some(1),
      40.0
    ));
    assert_eq!(target(&m, 1).y, 60.0);
  }

  #[test]
  fn a_scrollable_whose_content_fits_leaves_the_wheel() {
    let mut m = UiMemory::new();
    m.begin_frame();
    scrollable(
      Rect::new(0.0, 0.0, 200.0, 200.0),
      0,
      1,
      &mut m,
      Vec2::new(200.0, 150.0),
    );
    m.end_frame();
    m.begin_frame();
    assert!(!route_wheel(
      &mut m,
      Vec2::new(20.0, 20.0),
      Vec2::new(0.0, -1.0),
      false,
      Some(1),
      40.0
    ));
  }

  #[test]
  fn shift_turns_the_wheel_sideways() {
    let mut m = UiMemory::new();
    m.begin_frame();
    scrollable(
      Rect::new(0.0, 0.0, 200.0, 50.0),
      0,
      1,
      &mut m,
      Vec2::new(800.0, 50.0),
    );
    m.end_frame();
    m.begin_frame();
    assert!(route_wheel(
      &mut m,
      Vec2::new(20.0, 20.0),
      Vec2::new(0.0, -2.0),
      true,
      Some(1),
      40.0
    ));
    assert_eq!(target(&m, 1).x, 80.0);
  }
}
