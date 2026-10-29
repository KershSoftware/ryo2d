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

/// A frame of real time (`audio_step`): the clock, the registry's pump and
/// the audio engine's frame, which renders it on the mock backend, and the
/// change trackers cleared, as the engine's `Last` clears them
fn frame(world: &mut World) {
  audio_step(world, FRAME);
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
  // As a game runs it: the audio log is off but for headless engines, and
  // a mixer is set (every engine sets the game's), which each frame looks
  // up and checks for a change
  world.resource_mut::<AudioEngine>().set_logging(false);
  AudioView::new(&mut world).set_mixer(Mixer {
    buses: vec![BusDef::new("sfx/footsteps")],
    ..Default::default()
  });
  for _ in 0..10 {
    frame(&mut world);
  }
  let buses = world.resource::<AudioEngine>().buses();
  assert!(
    buses.iter().any(|bus| bus.name == "sfx/footsteps"),
    "the mixer was applied"
  );

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
    .add(AudioClip::from_frames(48_000, &frames).unwrap());
  let hum = world.resource_mut::<Assets<Sound>>().add(Sound {
    clips: vec![SoundClip::new(blip.clone())],
    looping: true,
    ..Default::default()
  });
  AudioView::new(&mut world).play(&blip);
  // A clip starts once a frame
  frame(&mut world);
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

  // Idle with virtual voices: loops a limit keeps out and one out of
  // range, each frame moved on through their clips and ranked against the
  // loops heard (none moves: they are as loud)
  let beds = world.resource_mut::<Assets<Sound>>().add(Sound {
    clips: vec![SoundClip::new(blip.clone())],
    looping: true,
    per_frame: 8,
    limit: Some(VoiceLimit {
      max: 2,
      rule: LimitRule::StopQuietest,
    }),
    ..Default::default()
  });
  let far = world.resource_mut::<Assets<Sound>>().add(Sound {
    clips: vec![SoundClip::new(blip)],
    looping: true,
    spatial: Some(Spatial {
      min_distance: 10.0,
      max_distance: 100.0,
      curve: Falloff::Linear,
      pan_width: 100.0,
      low_pass_at_max: None,
    }),
    ..Default::default()
  });
  let mut audio = AudioView::new(&mut world);
  let played: Vec<Voice> = (0..5).map(|_| audio.play(&beds)).collect();
  let distant = audio.play_at(&far, Vec2::new(500.0, 0.0));
  // Past the 250 ms a voice stays as it is, so each frame weighs them all
  for _ in 0..30 {
    frame(&mut world);
  }
  let kept = played.iter().filter(|voice| voice.is_virtual()).count();
  assert_eq!(kept, 3, "{played:?}");
  assert!(distant.is_virtual(), "{distant:?}");
  let virtuals = hundred_frames(&mut world);
  assert_eq!(
    virtuals, 0,
    "100 idle frames with virtual voices allocated {virtuals} times"
  );
  assert_eq!(AudioView::new(&mut world).virtual_voices_on("sfx"), 4);
}
