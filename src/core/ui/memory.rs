//! What the UI remembers between frames: last frame's laid-out records (for
//! hit testing and navigation), widgets' own state and per-scope focus

use std::any::Any;
use std::collections::HashMap;
use std::ops::{BitOr, BitOrAssign};

use glam::Vec2;

use super::id::{IdScopes, UiId};
use super::types::Rect;

/// What a widget senses and how navigation treats it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Flags(u32);

impl Flags {
  pub(crate) const CLICK: Flags = Flags(1 << 0);
  pub(crate) const DRAG: Flags = Flags(1 << 1);
  #[allow(dead_code)] // Used from Task 8 (navigation)
  pub(crate) const FOCUSABLE: Flags = Flags(1 << 2);
  pub(crate) const BLOCKING: Flags = Flags(1 << 3);
  pub(crate) const SCROLLABLE: Flags = Flags(1 << 4);
  pub(crate) const DROP_TARGET: Flags = Flags(1 << 5);
  pub(crate) const DISABLED: Flags = Flags(1 << 6);
  #[allow(dead_code)] // Used from Task 8 (navigation)
  pub(crate) const TAKES_HORIZONTAL: Flags = Flags(1 << 7);
  #[allow(dead_code)] // Used from Task 8 (navigation)
  pub(crate) const TAKES_VERTICAL: Flags = Flags(1 << 8);
  pub(crate) const FOCUS_SCOPE: Flags = Flags(1 << 9);
  pub(crate) const TRAP: Flags = Flags(1 << 10);
  pub(crate) const WRAP: Flags = Flags(1 << 11);
  #[allow(dead_code)] // Used from Task 8 (navigation)
  pub(crate) const MODAL: Flags = Flags(1 << 12);
  #[allow(dead_code)] // Used from Task 8 (navigation)
  pub(crate) const TEXT_FIELD: Flags = Flags(1 << 13);
  #[allow(dead_code)] // Used from Task 8 (navigation)
  pub(crate) const DEFAULT_FOCUS: Flags = Flags(1 << 14);
  #[allow(dead_code)] // Used from Task 8 (navigation)
  pub(crate) const USES_BACK: Flags = Flags(1 << 15);
  #[allow(dead_code)] // Used from Task 13 (drag and drop)
  pub(crate) const DRAG_SOURCE: Flags = Flags(1 << 16);
  #[allow(dead_code)] // Used from Task 8 (navigation)
  pub(crate) const NAV_SKIP: Flags = Flags(1 << 17);
  /// Reports hovering only: never hot, blocks nothing
  pub(crate) const HOVER: Flags = Flags(1 << 18);

  /// Every flag in `other` is set
  pub(crate) fn contains(self, other: Flags) -> bool {
    self.0 & other.0 == other.0
  }

  /// At least one flag in `other` is set
  pub(crate) fn intersects(self, other: Flags) -> bool {
    self.0 & other.0 != 0
  }
}

impl BitOr for Flags {
  type Output = Flags;

  fn bitor(self, rhs: Flags) -> Flags {
    Flags(self.0 | rhs.0)
  }
}

impl BitOrAssign for Flags {
  fn bitor_assign(&mut self, rhs: Flags) {
    self.0 |= rhs.0;
  }
}

/// One widget as laid out in a frame
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Record {
  pub(crate) rect: Rect,
  pub(crate) clip: Option<Rect>,
  pub(crate) layer: u8,
  pub(crate) order: u32,
  pub(crate) depth: u16,
  pub(crate) scene: u64,
  pub(crate) flags: Flags,
  /// Nearest focus-scope ancestor (`NONE` outside scopes)
  pub(crate) scope: UiId,
  /// Nearest scrollable ancestor (`NONE`)
  pub(crate) scroll: UiId,
  /// Scrollables: content size
  pub(crate) content: Vec2,
  /// Explicit Up, Down, Left, Right neighbours (`NONE`)
  pub(crate) nav: [UiId; 4],
}

/// The pointer's interaction, resolved once per engine frame against last
/// frame's records (`interact::resolve_pointer`)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Interaction {
  /// The topmost sensing widget under the pointer (`NONE`)
  pub(crate) hot: UiId,
  /// The topmost record under the pointer, hover-only widgets included:
  /// what reports `hovered()` (`NONE`)
  pub(crate) hovered: UiId,
  /// The widget a press started on, until that button's release (`NONE`)
  pub(crate) active: UiId,
  /// The button that made it active: 0 left, 1 right
  pub(crate) active_button: u8,
  /// Where that press was, in UI units
  pub(crate) press_pos: Vec2,
  /// The pointer this frame, in UI units
  pub(crate) pointer: Vec2,
  /// The pointer last frame
  pub(crate) prev_pointer: Vec2,
  /// The active widget's press moved past the drag threshold
  pub(crate) dragging: bool,
  /// This frame's left click (`NONE`)
  pub(crate) clicked: UiId,
  /// This frame's right click
  pub(crate) secondary: UiId,
  /// This frame's double click
  pub(crate) double: UiId,
  /// The last left click's widget and time, for double clicks
  pub(crate) last_click: (UiId, f64),
  /// The widget whose drag started this frame
  pub(crate) drag_started: UiId,
  /// The widget whose drag ended this frame
  pub(crate) drag_released: UiId,
  /// The pointer is over a widget or panel of the top scene
  pub(crate) over_ui: bool,
}

struct Slot {
  scene: u64,
  seen: u64,
  value: Box<dyn Any>,
}

/// What the UI remembers between frames: last frame's laid-out records,
/// widgets' own state (scroll offsets, carets, open lists) and focus
pub(crate) struct UiMemory {
  prev: HashMap<UiId, Record>,
  next: HashMap<UiId, Record>,
  states: HashMap<UiId, Slot>,
  focus_memory: HashMap<UiId, (u64, UiId)>,
  frame: u64,
  pub(crate) scopes: IdScopes,
  /// The pointer's hot, active, click and drag state
  pub(crate) interact: Interaction,
}

impl UiMemory {
  pub(crate) fn new() -> UiMemory {
    UiMemory {
      prev: HashMap::new(),
      next: HashMap::new(),
      states: HashMap::new(),
      focus_memory: HashMap::new(),
      frame: 0,
      scopes: IdScopes::default(),
      interact: Interaction::default(),
    }
  }

  /// Last frame's records become readable; this frame starts empty
  pub(crate) fn begin_frame(&mut self) {
    std::mem::swap(&mut self.prev, &mut self.next);
    self.next.clear();
    self.frame += 1;
    self.scopes.begin_frame();
  }

  /// The UI pass runs again this frame (a scene switched from `ui`): the
  /// records it made so far are discarded and ids start over
  pub(crate) fn restart_pass(&mut self) {
    self.next.clear();
    self.scopes.begin_frame();
  }

  /// Widget state not touched this frame is dropped
  pub(crate) fn end_frame(&mut self) {
    let frame = self.frame;
    self.states.retain(|_, slot| slot.seen == frame);
  }

  #[allow(dead_code)] // Part of Task 3's interface; no task reads it yet
  pub(crate) fn frame(&self) -> u64 {
    self.frame
  }

  pub(crate) fn record(&mut self, id: UiId, record: Record) {
    self.next.insert(id, record);
  }

  /// A widget's record from last frame
  pub(crate) fn prev(&self, id: UiId) -> Option<&Record> {
    self.prev.get(&id)
  }

  /// Every record from last frame
  pub(crate) fn prev_records(&self) -> impl Iterator<Item = (UiId, &Record)> {
    self.prev.iter().map(|(id, record)| (*id, record))
  }

  /// A widget's record from this frame so far
  pub(crate) fn current(&self, id: UiId) -> Option<&Record> {
    self.next.get(&id)
  }

  /// A widget's own state, created with `T::default()` when absent or of
  /// another type. Always `Some`; the `Option` keeps library code panic-free
  #[allow(dead_code)] // Used from Task 7 (scroll state)
  pub(crate) fn state<T: Default + 'static>(&mut self, id: UiId, scene: u64) -> Option<&mut T> {
    let frame = self.frame;
    let slot = self.states.entry(id).or_insert_with(|| Slot {
      scene,
      seen: frame,
      value: Box::new(T::default()),
    });
    if !slot.value.is::<T>() {
      slot.value = Box::new(T::default());
    }
    slot.scene = scene;
    slot.seen = frame;
    slot.value.downcast_mut::<T>()
  }

  #[allow(dead_code)] // Used from Task 7 (scroll state)
  pub(crate) fn peek_state<T: 'static>(&self, id: UiId) -> Option<&T> {
    self.states.get(&id)?.value.downcast_ref::<T>()
  }

  #[allow(dead_code)] // Used from Task 8 (focus memory)
  pub(crate) fn remember_focus(&mut self, scope: UiId, scene: u64, focused: UiId) {
    self.focus_memory.insert(scope, (scene, focused));
  }

  #[allow(dead_code)] // Used from Task 8 (focus memory)
  pub(crate) fn recall_focus(&self, scope: UiId) -> Option<UiId> {
    self.focus_memory.get(&scope).map(|(_, focused)| *focused)
  }

  /// A scene exited: drop everything it left behind
  pub(crate) fn forget_scene(&mut self, scene: u64) {
    self.states.retain(|_, slot| slot.scene != scene);
    self.focus_memory.retain(|_, (owner, _)| *owner != scene);
    self.prev.retain(|_, record| record.scene != scene);
    self.next.retain(|_, record| record.scene != scene);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn record(scene: u64) -> Record {
    Record {
      rect: Rect::new(0.0, 0.0, 10.0, 10.0),
      clip: None,
      layer: 0,
      order: 0,
      depth: 0,
      scene,
      flags: Flags::CLICK,
      scope: UiId::NONE,
      scroll: UiId::NONE,
      content: glam::Vec2::ZERO,
      nav: [UiId::NONE; 4],
    }
  }

  #[test]
  fn records_from_last_frame_are_read_this_frame() {
    let mut memory = UiMemory::new();
    memory.begin_frame();
    memory.record(UiId(5), record(1));
    assert!(memory.prev(UiId(5)).is_none(), "not until the next frame");
    memory.end_frame();
    memory.begin_frame();
    assert!(memory.prev(UiId(5)).is_some());
    memory.end_frame();
    memory.begin_frame();
    assert!(
      memory.prev(UiId(5)).is_none(),
      "gone once a frame did not record it"
    );
  }

  #[test]
  fn widget_state_is_dropped_after_a_frame_unseen() {
    let mut memory = UiMemory::new();
    memory.begin_frame();
    *memory.state::<u32>(UiId(9), 1).unwrap() = 42;
    memory.end_frame();
    memory.begin_frame();
    assert_eq!(
      *memory.state::<u32>(UiId(9), 1).unwrap(),
      42,
      "seen again: kept"
    );
    memory.end_frame();
    memory.begin_frame();
    memory.end_frame();
    memory.begin_frame();
    assert_eq!(
      *memory.state::<u32>(UiId(9), 1).unwrap(),
      0,
      "a frame unseen: dropped"
    );
  }

  #[test]
  fn focus_memory_lasts_until_the_scene_is_forgotten() {
    let mut memory = UiMemory::new();
    memory.remember_focus(UiId(100), 3, UiId(7));
    for _ in 0..5 {
      memory.begin_frame();
      memory.end_frame();
    }
    assert_eq!(memory.recall_focus(UiId(100)), Some(UiId(7)));
    memory.forget_scene(3);
    assert_eq!(memory.recall_focus(UiId(100)), None);
  }

  #[test]
  fn a_restarted_pass_drops_its_records_and_makes_the_same_ids() {
    let site = std::panic::Location::caller();
    let mut memory = UiMemory::new();
    memory.begin_frame();
    memory.record(UiId(5), record(1));
    memory.end_frame();
    memory.begin_frame();
    memory.scopes.enter_root(UiId::root(1));
    let first = memory.scopes.make(site, None);
    memory.record(UiId(6), record(1));
    memory.restart_pass();
    memory.scopes.enter_root(UiId::root(1));
    assert!(memory.current(UiId(6)).is_none(), "the first pass's record");
    assert!(memory.prev(UiId(5)).is_some(), "last frame's stay");
    assert_eq!(
      memory.scopes.make(site, None),
      first,
      "ids start over, so the rerun's widgets keep theirs"
    );
  }
}
