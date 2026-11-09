//! Allocations of a steady frame of the engine's particles, counted by a
//! global allocator in this test binary. The scene: 20 steady emitters, 18
//! of them plain (with an `on_death` sub-effect) and two colliding over a
//! tile map's parts (with `on_collision` and `on_death` sub-effects), and a
//! burst effect played with `play_at` every 10 frames, its emitter
//! despawning itself when finished, for 120 frames of warm-up and 120 of
//! measure.
//!
//! A frame's allocations are told apart by their stacks, as in
//! `tests/tilemap_alloc.rs`: those made inside the GPU API (wgpu and the
//! Metal bindings, which allocate as they record any frame's draw calls)
//! are the GPU's, and those the ECS makes spawning, despawning or applying
//! commands (a one-shot's entity arriving and going) are the ECS's; both are
//! printed. Every other one is the engine's own, and a frame makes none but
//! what an empty frame makes, site by site. Nor may the particle code make
//! a GPU resource every frame (a GPU allocation whose nearest engine
//! function is the particles').
// Each thread counts its own allocations, so tests never count each other's

use std::alloc::{GlobalAlloc, Layout, System};
use std::backtrace::Backtrace;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use ryo2d::ecs::kernel::{MessageCursor, Messages};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

thread_local! {
  static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
  /// Whether each allocation's stack is kept, and whether one is being
  /// taken (its own allocations are not)
  static TRACING: Cell<bool> = const { Cell::new(false) };
  static IN_TRACE: Cell<bool> = const { Cell::new(false) };
  static TRACES: RefCell<Vec<Backtrace>> = const { RefCell::new(Vec::new()) };
}

/// Counts an allocation, and keeps its stack while tracing (taking a stack
/// allocates, and those are not counted)
fn note() {
  if IN_TRACE.with(Cell::get) {
    return;
  }
  ALLOCATIONS.with(|c| c.set(c.get() + 1));
  if TRACING.with(Cell::get) {
    IN_TRACE.with(|c| c.set(true));
    let trace = Backtrace::force_capture();
    TRACES.with(|t| t.borrow_mut().push(trace));
    IN_TRACE.with(|c| c.set(false));
  }
}

/// Counts the allocations each thread makes
struct Counting;

// SAFETY: every call goes to `System`. The counters are const-initialised
// thread locals without destructors, so touching them never allocates; a
// stack is only taken while tracing, and its own allocations (and the
// list's) happen with `IN_TRACE` set, so they are neither counted nor
// traced
unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    note();
    unsafe { System.alloc(layout) }
  }
  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) }
  }
  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
    note();
    unsafe { System.realloc(ptr, layout, new) }
  }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// The allocations this thread has made
fn allocations() -> usize {
  ALLOCATIONS.with(Cell::get)
}

/// Frames before the measure, and measured
const WARM: usize = 120;
const MEASURED: usize = 120;
/// A one-shot burst every this many frames
const ONE_SHOT_EVERY: u32 = 10;
/// The view, the camera on its middle
const VIEW: (u32, u32) = (640, 360);
/// Where the one-shots are played, in turn
const PLACES: [Vec2; 4] = [
  Vec2::new(100.0, 60.0),
  Vec2::new(540.0, 60.0),
  Vec2::new(100.0, 180.0),
  Vec2::new(540.0, 180.0),
];

/// The scene measured (or nothing, `empty`, whose frame is the baseline): 20
/// steady emitters and a burst played every 10 frames
struct Steady {
  empty: bool,
  frame: u32,
  burst: Option<Handle<ParticleEffect>>,
}

impl Scene for Steady {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .camera()
      .set_position(VIEW.0 as f32 / 2.0, VIEW.1 as f32 / 2.0);
    if self.empty {
      return;
    }
    let sprite = ctx.assets().add(Image::solid_circle(8, 255, 255, 255, 255));
    // A spark: what a particle becomes when it dies or lands
    let spark = ctx.assets().add(
      ParticleEffect::new(sprite.clone())
        .with_rate(0.0)
        .with_lifetime(0.1)
        .with_speed(10.0)
        .with_size(3.0),
    );
    // The burst `play_at` plays: its particles die into sparks
    let burst = ctx.assets().add(
      ParticleEffect::new(sprite.clone())
        .with_rate(0.0)
        .with_burst(Burst::at(0.0, 16.0))
        .with_duration(0.2)
        .with_lifetime(0.3)
        .with_speed(40.0)
        .with_size(4.0)
        .on_death(SubEmitter::new(spark.clone(), 1.0)),
    );
    self.burst = Some(burst);
    // 18 plain steady emitters, in a grid above the floor
    let plain = ctx.assets().add(
      ParticleEffect::new(sprite.clone())
        .with_rate(60.0)
        .with_lifetime(0.4)
        .with_speed(30.0)
        .with_size(4.0)
        .with_gravity(0.0, 40.0)
        .with_size_over_life(Curve::new([(0.0, 1.0), (1.0, 0.2)]))
        .with_noise(NoiseForce::new(4.0, 0.02))
        .on_death(SubEmitter::new(spark.clone(), 1.0)),
    );
    for row in 0..3 {
      for column in 0..6 {
        ctx.world.spawn((
          Transform::from_xy(120.0 + 80.0 * column as f32, 40.0 + 60.0 * row as f32),
          ParticleEmitter::new(plain.clone()).with_seed(7),
        ));
      }
    }
    // Two emitters colliding over a tile map's parts: its colliders are the
    // parts the engine builds as the children of its `TileColliders`
    let mut tiles = TileColliders::new(40, 8, Vec2::splat(16.0));
    for x in 0..40 {
      tiles.set(IVec2::new(x, 2), TileCell::Solid);
    }
    for x in (0..40).step_by(5) {
      tiles.set(IVec2::new(x, 1), TileCell::Solid);
    }
    ctx.world.spawn((Transform::from_xy(0.0, 300.0), tiles));
    let falls = ParticleEffect::new(sprite.clone())
      .with_rate(120.0)
      .with_lifetime(1.0)
      .with_speed(40.0)
      .with_direction(90.0, 40.0)
      .with_size(4.0)
      .with_gravity(0.0, 300.0);
    let die = ctx.assets().add(
      falls
        .clone()
        .with_collision(CollisionConfig::die_on_hit(2.0).with_events())
        .on_death(SubEmitter::new(spark.clone(), 1.0))
        .on_collision(SubEmitter::new(spark.clone(), 2.0)),
    );
    let bounce = ctx.assets().add(
      falls
        .with_collision(CollisionConfig::bouncy(2.0, 0.5).with_events())
        .on_collision(SubEmitter::new(spark, 1.0)),
    );
    ctx.world.spawn((
      Transform::from_xy(200.0, 220.0),
      ParticleEmitter::new(die).with_seed(7),
    ));
    ctx.world.spawn((
      Transform::from_xy(440.0, 220.0),
      ParticleEmitter::new(bounce).with_seed(7),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.frame += 1;
    if let Some(burst) = &self.burst
      && self.frame.is_multiple_of(ONE_SHOT_EVERY)
    {
      let burst = burst.clone();
      let at = PLACES[(self.frame / ONE_SHOT_EVERY) as usize % PLACES.len()];
      ctx.particles().play_at(&burst, at);
    }
    SceneAction::Continue
  }
}

/// Where a stack's frames come from: the GPU API's crates
const GPU: [&str; 5] = ["wgpu", "naga::", "objc2", "metal::", "block2"];
/// A stack's frames, on its leaf side of the engine's nearest function,
/// that make an allocation the ECS's own: spawning, despawning or applying
/// commands
const ECS: [&str; 5] = ["spawn", "despawn", "command", "flush", "apply_deferred"];

/// One frame's allocations: how many; those made inside the GPU API (how
/// many, and whether any were the particle code's); those the ECS made
/// spawning, despawning or applying commands, by site; and the engine's own
/// (the rest), by site. Sites name the nearest engine function and the one
/// it called, as `tests/tilemap_alloc.rs` does
#[derive(Default)]
struct Frame {
  count: usize,
  gpu: usize,
  particle_gpu: Vec<String>,
  ecs: BTreeMap<String, usize>,
  own: BTreeMap<String, usize>,
}

/// A stack's function names, leaf first
fn functions(text: &str) -> Vec<&str> {
  text
    .lines()
    .map(str::trim)
    .filter(|line| !line.starts_with("at "))
    .filter_map(|line| line.split_once(": ").map(|(_, name)| name))
    .collect()
}

/// Whether an allocation of the stack `frames`, whose nearest engine
/// function is at `ours`, was made by the ECS spawning, despawning or
/// applying commands (the ECS code runs on the leaf side of that function:
/// an `on_insert` hook of the engine's, with engine frames nearer the leaf
/// than the spawn it runs in, is the engine's)
fn in_ecs(frames: &[&str], ours: Option<usize>) -> bool {
  frames[..ours.unwrap_or(frames.len())]
    .iter()
    .any(|frame| frame.contains("bevy_ecs") && ECS.iter().any(|name| frame.contains(name)))
}

/// A step of `engine`, its allocations traced and told apart
fn traced(engine: &mut HeadlessEngine) -> Frame {
  TRACES.with(|t| t.borrow_mut().clear());
  let before = allocations();
  TRACING.with(|c| c.set(true));
  engine.step().expect("a frame");
  TRACING.with(|c| c.set(false));
  let count = allocations() - before;
  let traces = TRACES.with(|t| std::mem::take(&mut *t.borrow_mut()));
  let mut frame = Frame {
    count,
    ..Frame::default()
  };
  for trace in traces {
    let text = trace.to_string();
    let frames = functions(&text);
    let ours = frames.iter().position(|f| f.contains("ryo2d::"));
    if GPU.iter().any(|gpu| text.contains(gpu)) {
      frame.gpu += 1;
      if let Some(at) = ours
        && frames[at].contains("core::particles")
      {
        frame.particle_gpu.push(frames[at].to_string());
      }
      continue;
    }
    let site = match ours {
      Some(at) => format!("{} <- {}", frames[at.saturating_sub(1)], frames[at]),
      None => frames.first().copied().unwrap_or("?").to_string(),
    };
    if in_ecs(&frames, ours) {
      *frame.ecs.entry(site).or_insert(0) += 1;
    } else {
      *frame.own.entry(site).or_insert(0) += 1;
    }
  }
  frame
}

/// The sites where `drawn` makes more allocations than `nothing`
fn more(drawn: &BTreeMap<String, usize>, nothing: &BTreeMap<String, usize>) -> Vec<String> {
  drawn
    .iter()
    .filter(|(site, n)| nothing.get(*site).copied().unwrap_or(0) < **n)
    .map(|(site, n)| format!("{n} at {site}"))
    .collect()
}

/// How many colliders `engine`'s tile grids built
fn tile_colliders(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  let grids: Vec<Entity> = world
    .query_filtered::<Entity, With<TileColliders>>()
    .iter(world)
    .collect();
  world
    .query::<(&ChildOf, &Collider)>()
    .iter(world)
    .filter(|(child_of, _)| grids.contains(&child_of.parent()))
    .count()
}

/// How many emitters are in `engine`'s world
fn emitters(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  world.query::<&ParticleEmitter>().iter(world).count()
}

#[test]
fn a_steady_frame_of_emitters_and_one_shots_makes_no_allocation_of_the_engine_s_own() {
  let scene = |empty| Steady {
    empty,
    frame: 0,
    burst: None,
  };
  let make = |empty| {
    let builder = EngineBuilder::new()
      .resolution(VIEW.0, VIEW.1)
      .start_scene(scene(empty));
    HeadlessEngine::new_or_skip(builder)
  };
  let (Some(mut empty), Some(mut steady)) = (make(true), make(false)) else {
    return;
  };
  let mut hits = steady
    .world()
    .resource::<Messages<ParticleCollision>>()
    .get_cursor_current();
  let mut done = steady
    .world()
    .resource::<Messages<EmitterFinished>>()
    .get_cursor_current();
  for _ in 0..WARM {
    empty.step().expect("a warm-up frame");
    steady.step().expect("a warm-up frame");
    drain(&mut steady, &mut hits, &mut done);
  }
  // The scene is what the test says: the map built its parts, the steady
  // emitters and the one-shots hold particles
  assert!(tile_colliders(&mut steady) > 0, "the map built its parts");
  let live = ParticlesView::new(steady.world_mut()).count();
  assert!(live > 200, "only {live} particles are alive after warm-up");

  // The empty frame's sites: how many each makes at most
  let mut baseline: BTreeMap<String, usize> = BTreeMap::new();
  for _ in 0..MEASURED {
    let frame = traced(&mut empty);
    for (site, n) in frame.own {
      let most = baseline.entry(site).or_insert(0);
      *most = (*most).max(n);
    }
  }

  let (mut count, mut gpu) = (0, 0);
  let mut ecs: BTreeMap<String, usize> = BTreeMap::new();
  let mut own: BTreeMap<String, usize> = BTreeMap::new();
  let mut failures = Vec::new();
  let (mut collisions, mut finished) = (0, 0);
  let (mut least, mut most) = (usize::MAX, 0);
  let mut emitters_seen = Vec::new();
  for frame in 0..MEASURED {
    let drawn = traced(&mut steady);
    let extra = more(&drawn.own, &baseline);
    if !extra.is_empty() {
      failures.push(format!(
        "frame {frame}: allocations of the engine's own an empty frame did not make: {extra:#?}"
      ));
    }
    if !drawn.particle_gpu.is_empty() {
      failures.push(format!(
        "frame {frame}: the particle code made GPU allocations: {:#?}",
        drawn.particle_gpu
      ));
    }
    count += drawn.count;
    gpu += drawn.gpu;
    least = least.min(drawn.count);
    most = most.max(drawn.count);
    for (site, n) in drawn.ecs {
      *ecs.entry(site).or_insert(0) += n;
    }
    for (site, n) in drawn.own {
      *own.entry(site).or_insert(0) += n;
    }
    let (new_hits, new_done) = drain(&mut steady, &mut hits, &mut done);
    collisions += new_hits;
    finished += new_done;
    emitters_seen.push(emitters(&mut steady));
  }
  let live = ParticlesView::new(steady.world_mut()).count();
  println!(
    "measured {MEASURED} frames: {count} allocations ({least} to {most} a frame), {gpu} inside \
     the GPU API; {live} particles alive at the end, {collisions} collisions told, {finished} \
     one-shots finished, {} to {} emitters",
    emitters_seen.iter().min().unwrap_or(&0),
    emitters_seen.iter().max().unwrap_or(&0),
  );
  println!(
    "the ECS's allocations spawning, despawning or applying commands, over the measured frames \
     (printed, not asserted): {ecs:#?}"
  );
  println!("the engine's own allocations over the measured frames: {own:#?}");
  println!("an empty frame's own allocations, at most a frame: {baseline:#?}");
  assert!(collisions > 0, "no particle met the tile map's parts");
  assert!(finished > 0, "no one-shot finished in the measured frames");
  assert!(
    emitters_seen.iter().min() != emitters_seen.iter().max(),
    "the one-shots' emitters did not come and go in the measured frames"
  );
  assert!(failures.is_empty(), "{failures:#?}");
}

/// The collisions and one-shots finished since the last call
fn drain(
  engine: &mut HeadlessEngine,
  hits: &mut MessageCursor<ParticleCollision>,
  done: &mut MessageCursor<EmitterFinished>,
) -> (usize, usize) {
  let world = engine.world();
  let collisions = hits
    .read(world.resource::<Messages<ParticleCollision>>())
    .count();
  let finished = done
    .read(world.resource::<Messages<EmitterFinished>>())
    .count();
  (collisions, finished)
}
