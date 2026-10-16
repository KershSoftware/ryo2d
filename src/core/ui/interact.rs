//! The pointer resolve: once per engine frame, before any scene's `ui`, the
//! pointer is hit-tested against last frame's laid-out records

use glam::Vec2;

use super::memory::{Flags, UiMemory};
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
  let (hot, over_ui) = match top_scene {
    Some(scene) if !blocked => hit_test(memory, position, scene),
    _ => (UiId::NONE, false),
  };
  let active_exists = memory.prev(memory.interact.active).is_some();

  let i = &mut memory.interact;
  // One-frame results last until the next resolve
  i.clicked = UiId::NONE;
  i.secondary = UiId::NONE;
  i.double = UiId::NONE;
  i.drag_started = UiId::NONE;
  i.drag_released = UiId::NONE;
  i.prev_pointer = i.pointer;
  i.pointer = position;
  i.hot = hot;
  i.over_ui = over_ui;

  // A widget gone since its press reports nothing more
  if i.active != UiId::NONE && !active_exists {
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

  if i.active != UiId::NONE && !i.dragging && position.distance(i.press_pos) > drag_threshold {
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
  outcome
}

/// The hot widget at `at` among `scene`'s records from last frame, and
/// whether any of them is under it. The topmost record (layer, then order)
/// decides: one that senses and is enabled is hot; a panel, scroll area or
/// disabled widget blocks what is under it.
fn hit_test(memory: &UiMemory, at: Vec2, scene: u64) -> (UiId, bool) {
  // What senses the pointer, or stops it
  let hits = Flags::CLICK | Flags::DRAG | Flags::BLOCKING | Flags::SCROLLABLE | Flags::DROP_TARGET;
  let top = memory
    .prev_records()
    .filter(|(_, r)| {
      r.scene == scene
        && r.flags.intersects(hits)
        && r.rect.contains(at.x, at.y)
        && r.clip.is_none_or(|clip| clip.contains(at.x, at.y))
    })
    // Depth and id only break ties, so the result never depends on map order
    .max_by_key(|(id, r)| (r.layer, r.order, r.depth, *id));
  match top {
    Some((id, r)) => {
      let senses = r.flags.intersects(Flags::CLICK | Flags::DRAG);
      let hot = if senses && !r.flags.contains(Flags::DISABLED) {
        id
      } else {
        UiId::NONE
      };
      (hot, true)
    }
    None => (UiId::NONE, false),
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
}
