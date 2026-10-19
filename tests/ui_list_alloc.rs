//! Allocations per virtual list frame, counted by a global allocator in this
//! test binary
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

/// A row's height: the view of 400 is a whole number of rows
const ROW: f32 = 25.0;

/// A list of `labels`, one text per row
fn board(ui: &mut UiContext<'_>, labels: &[String]) -> UiId {
  ui.list(labels.len())
    .row_height(ROW)
    .height(400.0)
    .width(300.0)
    .id("board")
    .show(|ui, i| {
      ui.text(labels[i].as_str());
    })
    .id
}

/// The allocations of one unchanged frame of a list of `count` rows,
/// scrolled to its middle
fn frame_allocations(count: usize) -> usize {
  let labels: Vec<String> = (0..count).map(|i| format!("Player {i}")).collect();
  let mut harness = UiHarness::new(640.0, 480.0);
  let list = harness.frame(|ui| board(ui, &labels));
  // A whole number of rows down, so both lists build as many
  let middle = ROW * (count / 2) as f32;
  harness.frame(|ui| ui.set_scroll_offset(list, Vec2::new(0.0, middle)));
  for _ in 0..5 {
    let offset = harness.frame(|ui| {
      board(ui, &labels);
      ui.scroll_offset(list)
    });
    assert_eq!(offset.y, middle);
  }
  let before = ALLOCATIONS.load(Ordering::Relaxed);
  harness.frame(|ui| board(ui, &labels));
  ALLOCATIONS.load(Ordering::Relaxed) - before
}

#[test]
fn a_list_frame_allocates_as_much_at_10_000_rows_as_at_100() {
  let short = frame_allocations(100);
  let long = frame_allocations(10_000);
  println!("allocations in one frame: {short} at 100 rows, {long} at 10,000");
  // A list costs its screenful whatever its count: the same rows are built
  assert!(
    long.abs_diff(short) <= 4,
    "10,000 rows made {long} allocations, 100 rows {short}"
  );
}
