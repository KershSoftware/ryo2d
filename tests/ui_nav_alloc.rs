//! Allocations of a UI frame whose focus moves down the rows of a scroll
//! area, counted by a global allocator in this test binary
// This test binary must hold exactly one test: the allocator is global, and a second test on another thread would count into it

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use ryo2d::prelude::*;
use ryo2d::testing::UiHarness;

struct Counting;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    unsafe { System.alloc(layout) }
  }
  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) }
  }
  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    unsafe { System.realloc(ptr, layout, size) }
  }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// A focus scope around a scroll area of `rows` buttons, on a screen tall
/// enough that the area never scrolls, warmed up with its first row
/// focused
fn list(rows: usize) -> UiHarness {
  let mut harness = UiHarness::new(640.0, 2400.0);
  let buttons = (0..rows).map(|i| UiNode::button(format!("Row {i}")));
  harness.spawn_ui(
    UiNode::panel()
      .focus_scope()
      .child(UiNode::scroll().children(buttons)),
  );
  for _ in 0..5 {
    harness.frame(|_| {});
  }
  assert!(harness.focused().is_some(), "the scope took the focus");
  harness
}

/// The allocations of each of `steps` frames that move the focus down a row
fn steps(harness: &mut UiHarness, steps: usize) -> Vec<usize> {
  (0..steps)
    .map(|_| {
      let focused = harness.focused();
      let before = ALLOCATIONS.load(Ordering::Relaxed);
      harness.action("ui.down");
      harness.frame(|_| {});
      let made = ALLOCATIONS.load(Ordering::Relaxed) - before;
      assert_ne!(harness.focused(), focused, "ui.down moved the focus");
      made
    })
    .collect()
}

#[test]
fn a_focus_step_allocates_nothing_for_each_row_it_chooses_among() {
  let mut few = list(10);
  let mut many = list(40);
  let (a, b) = (steps(&mut few, 6), steps(&mut many, 6));
  println!("allocations of each focus step: 10 rows {a:?}, 40 rows {b:?}");
  assert_eq!(
    a, b,
    "a focus step among 40 rows allocates as much as one among 10"
  );
}
