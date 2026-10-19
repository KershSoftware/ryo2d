//! Allocations per UI frame of a cached node tree, counted by a global
//! allocator in this test binary
// This test binary must hold exactly one test: the allocator is global, and a second test on another thread would count into it

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use ryo2d::testing::UiHarness;
use ryo2d::testing::fixtures::settings_tree;

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

/// The settings node tree, warmed up with caching `on` or off
fn settings(on: bool) -> UiHarness {
  let mut harness = UiHarness::new(640.0, 480.0);
  harness.set_ui_cache(on);
  harness.spawn_ui(settings_tree());
  for _ in 0..10 {
    harness.frame(|_| {});
  }
  harness
}

/// The allocations of one unchanged frame
fn frame(harness: &mut UiHarness) -> usize {
  let before = ALLOCATIONS.load(Ordering::Relaxed);
  harness.frame(|_| {});
  ALLOCATIONS.load(Ordering::Relaxed) - before
}

#[test]
fn an_unchanged_cached_node_frame_allocates_no_more_than_an_uncached_one() {
  let mut cached = settings(true);
  let mut uncached = settings(false);
  let on = frame(&mut cached);
  let again = frame(&mut cached);
  let off = frame(&mut uncached);
  let stats = cached.ui_cache_stats();
  println!(
    "allocations in one unchanged frame: {on} cached, then {again}; {off} uncached \
     ({} built, {} replayed)",
    stats.built, stats.replayed
  );
  assert!(stats.replayed > 0, "the cached tree replays");
  assert!(
    on <= off,
    "an unchanged cached frame made {on} allocations, an uncached one {off}"
  );
  // A replay allocates nothing once warm: what is left is the frame's own
  // (the focused path's builds, resolve, commands), well under an uncached
  // frame's
  assert!(
    on <= 30,
    "an unchanged cached frame made {on} allocations (at most 30)"
  );
  assert_eq!(
    on, again,
    "two unchanged cached frames allocate the same: replay allocates nothing"
  );
}
