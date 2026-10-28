//! Allocations of the audio engine's frame, counted by a global allocator
//! in this test binary. Only the test's own thread counts: the frame runs
//! there (the mock backend renders on it too), and kira's decoder threads
//! are not the frame's
// This test binary holds one test, so nothing else runs on its thread

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ryo2d::core::audio::AudioEngine;
use ryo2d::prelude::*;
use ryo2d::testing::{audio_step, audio_world};

struct Counting;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
  /// Whether this thread's allocations are being counted
  static COUNTING: Cell<bool> = const { Cell::new(false) };
}

fn note() {
  if COUNTING.try_with(Cell::get).unwrap_or(false) {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
  }
}

unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    note();
    unsafe { System.alloc(layout) }
  }
  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) }
  }
  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
    note();
    unsafe { System.realloc(ptr, layout, size) }
  }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// The allocations `f` makes on this thread
fn allocations(f: impl FnOnce()) -> usize {
  let before = ALLOCATIONS.load(Ordering::Relaxed);
  COUNTING.with(|counting| counting.set(true));
  f();
  COUNTING.with(|counting| counting.set(false));
  ALLOCATIONS.load(Ordering::Relaxed) - before
}

/// A 60 Hz frame
const FRAME: Duration = Duration::from_nanos(16_666_667);

/// A frame of real time: the clock, the registry's pump and the audio
/// engine's frame, which renders it on the mock backend; then the change
/// trackers cleared, as the engine's `Last` clears them each frame (the
/// audio frame takes its engine out of the world and puts it back, which
/// writes a removal message into a buffer only clearing reuses)
fn frame(world: &mut World) {
  audio_step(world, FRAME);
  world.clear_trackers();
}

/// A hundred frames' allocations
fn hundred_frames(world: &mut World) -> usize {
  allocations(|| {
    for _ in 0..100 {
      frame(world);
    }
  })
}

#[test]
fn idle_frame_audio_allocates_nothing() {
  let mut world = audio_world();
  // As a game runs it: the audio log is off but for headless engines
  world.resource_mut::<AudioEngine>().set_logging(false);
  for _ in 0..10 {
    frame(&mut world);
  }

  let idle = hundred_frames(&mut world);
  assert_eq!(
    idle, 0,
    "100 idle frames of a fresh engine allocated {idle} times"
  );

  // Idle again after sounds have played and ended: a one-shot, a loop
  // stopped with its fade, and a voice heard from a place, with the
  // listener the engine looks for
  world.spawn((AudioListener, Transform::default()));
  let frames = [[0.1, 0.1]; 2_400];
  let blip = world
    .resource_mut::<Assets<AudioClip>>()
    .add(AudioClip::from_frames(48_000, &frames));
  let hum = world.resource_mut::<Assets<Sound>>().add(Sound {
    clips: vec![SoundClip::new(blip.clone())],
    looping: true,
    ..Default::default()
  });
  AudioView::new(&mut world).play(&blip);
  AudioView::new(&mut world).play_at(&blip, Vec2::new(40.0, 0.0));
  let looped = AudioView::new(&mut world).play(&hum);
  for _ in 0..10 {
    frame(&mut world);
  }
  looped.stop(Duration::from_millis(50));
  for _ in 0..30 {
    frame(&mut world);
  }
  assert!(looped.is_stopped(), "the loop has faded out");
  assert_eq!(AudioView::new(&mut world).voices_on("master"), 0);

  let after = hundred_frames(&mut world);
  assert_eq!(
    after, 0,
    "100 idle frames after sounds played allocated {after} times"
  );
}
