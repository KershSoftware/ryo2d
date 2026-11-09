//! Particles against an empty frame, with a GPU (`HeadlessEngine`, 1920 x
//! 1080): 10,000 particles from one emitter, 100 emitters of 100 particles
//! in a grid, and 2,000 particles bouncing on a 64 x 64 tile map's colliders.
//!
//! Each scene adds its effect once and places it with emitters seeded with
//! `ParticleEmitter::with_seed(7)`; the engine steps and draws them, and the
//! scene reads the live count in `Scene::update`. Before criterion runs, a
//! guard steps every scene's engine and an empty engine in turn, interleaved
//! (an engine stepping evicts the others' data), the GPU drained outside the
//! measure, and prints each scene's step over the empty frame (median and
//! 95th percentile). There are no targets yet. Criterion then steps each
//! scene's engine back to back, and the medians are read back from where it
//! writes them (`guards::results_dir`).

mod guards;

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use criterion::Criterion;
use ryo2d::core::ecs::kernel::ChildOf;
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, shared_gpu};

/// Every engine's view
const RESOLUTION: (u32, u32) = (1920, 1080);
/// Rounds of the guard: one frame of every engine a round
const ROUNDS: usize = 1000;
/// Frames each engine runs, at least, before it is measured
const WARM: usize = 240;
/// Frames the warm-up waits at most for the live count to come within 2% of
/// its steady state (an emitter at its cap counts a few short of it)
const WARM_MAX: usize = 900;
/// A tile's side in pixels, and the map's side in tiles
const TILE: f32 = 16.0;
const MAP: u32 = 64;

/// The benches: criterion's names, the live particles each holds, and what
/// each median is of
const BENCHES: [(&str, usize, &str); 3] = [
  (
    "particles_10k",
    10_000,
    "a step of one emitter holding 10,000 particles (back to back, GPU-paced)",
  ),
  (
    "particles_100x100",
    10_000,
    "a step of 100 emitters of 100 particles in a grid (back to back, GPU-paced)",
  ),
  (
    "particles_collide_2k",
    2_000,
    "a step of 2,000 particles bouncing on a 64 x 64 tile map (back to back, GPU-paced)",
  ),
];

/// A scene of the bench: what it spawns, in one function each
#[derive(Clone, Copy, PartialEq, Eq)]
enum Show {
  Empty,
  Ten,
  Grid,
  Collide,
}

impl Show {
  fn name(self) -> &'static str {
    match self {
      Show::Empty => "empty",
      Show::Ten => "particles_10k",
      Show::Grid => "particles_100x100",
      Show::Collide => "particles_collide_2k",
    }
  }

  /// The particles live once warmed up
  fn live(self) -> usize {
    match self {
      Show::Empty => 0,
      Show::Ten | Show::Grid => 10_000,
      Show::Collide => 2_000,
    }
  }
}

/// A scene of the bench, and the live count it last saw
struct ParticleScene {
  show: Show,
  live: Arc<AtomicUsize>,
}

/// An effect of the bench: `rate` a second from a point in every direction,
/// living 2 s, up to `max` alive, each particle shrinking to nothing and
/// fading out over its life (as the scene-driven system's defaults did)
fn effect(sprite: Handle<Image>, rate: f32, max: u32) -> ParticleEffect {
  ParticleEffect::new(sprite)
    .with_rate(rate)
    .with_lifetime(2.0)
    .with_max_particles(max)
    .with_size_over_life(Curve::new([(0.0, 1.0), (1.0, 0.0)]))
}

/// An emitter of `effect` at `(x, y)`, seeded so every run repeats
fn emitter(x: f32, y: f32, effect: &Handle<ParticleEffect>) -> (Transform, ParticleEmitter) {
  (
    Transform::from_xy(x, y),
    ParticleEmitter::new(effect.clone()).with_seed(7),
  )
}

/// One emitter: 5,000 a second living 2 s, up to 10,000
fn spawn_10k(ctx: &mut SceneContext, sprite: Handle<Image>) {
  let effect = ctx.assets().add(effect(sprite, 5_000.0, 10_000));
  ctx.world.spawn(emitter(960.0, 540.0, &effect));
}

/// 100 emitters in a 10 x 10 grid: 50 a second living 2 s, up to 100 each
fn spawn_100x100(ctx: &mut SceneContext, sprite: Handle<Image>) {
  let effect = ctx.assets().add(effect(sprite, 50.0, 100));
  for row in 0..10 {
    for column in 0..10 {
      ctx.world.spawn(emitter(
        210.0 + 150.0 * column as f32,
        90.0 + 90.0 * row as f32,
        &effect,
      ));
    }
  }
}

/// A 64 x 64 map of 16 px tiles with a solid border and 8 platforms, its
/// `TileColliders` built by hand, and one emitter above it: 1,000 a second
/// living 2 s (the effect's cap raised from the default 1,000 to hold them),
/// bouncing, falling at 400 px/s^2
fn spawn_collide_2k(ctx: &mut SceneContext, sprite: Handle<Image>) {
  let mut tiles = TileColliders::new(MAP, MAP, Vec2::splat(TILE));
  let last = MAP as i32 - 1;
  for i in 0..=last {
    for cell in [
      IVec2::new(i, 0),
      IVec2::new(i, last),
      IVec2::new(0, i),
      IVec2::new(last, i),
    ] {
      tiles.set(cell, TileCell::Solid);
    }
  }
  for platform in 0..8 {
    let y = 10 + platform * 6;
    let from = if platform % 2 == 0 { 8 } else { 32 };
    for x in from..from + 24 {
      tiles.set(IVec2::new(x, y), TileCell::Solid);
    }
  }
  let side = MAP as f32 * TILE;
  let origin = Vec2::new(
    (RESOLUTION.0 as f32 - side) / 2.0,
    (RESOLUTION.1 as f32 - side) / 2.0,
  );
  ctx
    .world
    .spawn((Transform::from_xy(origin.x, origin.y), tiles));
  let effect = ctx.assets().add(
    effect(sprite, 1_000.0, 2_000)
      .with_gravity(0.0, 400.0)
      .with_collision(CollisionConfig::bouncy(2.0, 0.5)),
  );
  ctx.world.spawn(emitter(
    origin.x + side / 2.0,
    origin.y + 3.0 * TILE,
    &effect,
  ));
}

impl Scene for ParticleScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .camera()
      .set_position(RESOLUTION.0 as f32 / 2.0, RESOLUTION.1 as f32 / 2.0);
    let sprite = ctx
      .assets()
      .add(Image::solid_circle(16, 255, 255, 255, 255));
    match self.show {
      Show::Empty => {}
      Show::Ten => spawn_10k(ctx, sprite),
      Show::Grid => spawn_100x100(ctx, sprite),
      Show::Collide => spawn_collide_2k(ctx, sprite),
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let live = ctx.particles().count();
    self.live.store(live as usize, Ordering::Relaxed);
    SceneAction::Continue
  }
}

/// An engine of the bench and the live count its scene reports
struct Probe {
  show: Show,
  engine: HeadlessEngine,
  live: Arc<AtomicUsize>,
}

/// An engine of `show`, warmed up until its particles are all live. None
/// without a GPU
fn engine_of(show: Show) -> Option<Probe> {
  let live = Arc::new(AtomicUsize::new(0));
  let builder = EngineBuilder::new()
    .resolution(RESOLUTION.0, RESOLUTION.1)
    .start_scene(ParticleScene {
      show,
      live: live.clone(),
    });
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  let mut frames = 0;
  while frames < WARM
    || (live.load(Ordering::Relaxed) * 100 < show.live() * 98 && frames < WARM_MAX)
  {
    engine.step().expect("a warm-up frame");
    frames += 1;
  }
  let count = live.load(Ordering::Relaxed);
  eprintln!(
    "{}: {count} particles live after {frames} warm-up frames",
    show.name()
  );
  assert!(
    count * 100 >= show.live() * 98,
    "{}: warmed up to {} particles, found {count}",
    show.name(),
    show.live()
  );
  if show == Show::Collide {
    // The tile map's colliders, the parts the engine builds as children of
    // its `TileColliders`: without one the particles fall through the map
    let world = engine.world_mut();
    let mut colliders = world.query_filtered::<&ChildOf, With<Collider>>();
    let parts = colliders
      .iter(world)
      .filter(|child_of| world.get::<TileColliders>(child_of.parent()).is_some())
      .count();
    assert!(parts > 0, "{}: the tile map has no colliders", show.name());
  }
  Some(Probe { show, engine, live })
}

/// Wait for the GPU, so a frame's work is not the last one's
fn settle_gpu() {
  if let Ok((device, _)) = shared_gpu() {
    device
      .poll(wgpu::PollType::wait_indefinitely())
      .expect("the GPU finishes its work");
  }
}

fn us(time: Duration) -> f64 {
  time.as_secs_f64() * 1e6
}

/// The median and 95th percentile of `times`
fn spread(mut times: Vec<Duration>) -> (Duration, Duration) {
  times.sort_unstable();
  let at = |percent: usize| times[(times.len() * percent / 100).min(times.len() - 1)];
  (times[times.len() / 2], at(95))
}

/// The guard: every scene's engine and an empty one stepped in turn,
/// `ROUNDS` times, each scene's step less the empty frame's median
fn guard() {
  let mut probes = Vec::new();
  for show in [Show::Empty, Show::Ten, Show::Grid, Show::Collide] {
    let Some(probe) = engine_of(show) else {
      return;
    };
    probes.push((probe, Vec::with_capacity(ROUNDS)));
  }
  // Round robin: each round starts at the next engine, every other round in
  // reverse, so no engine steps twice in a row
  let count = probes.len();
  for round in 0..ROUNDS {
    for i in 0..count {
      let index = if round.is_multiple_of(2) {
        (round + i) % count
      } else {
        (round + 1 + count - i) % count
      };
      let (probe, times) = &mut probes[index];
      settle_gpu();
      let start = Instant::now();
      probe.engine.step().expect("a guard frame");
      times.push(start.elapsed());
    }
  }
  let (empty, _) = spread(probes[0].1.clone());
  eprintln!(
    "particles guard: {ROUNDS} interleaved frames at {}x{}. An empty frame: step median {:.1} µs",
    RESOLUTION.0,
    RESOLUTION.1,
    us(empty)
  );
  for (probe, times) in &probes[1..] {
    let over: Vec<Duration> = times.iter().map(|t| t.saturating_sub(empty)).collect();
    let (median, p95) = spread(over);
    eprintln!(
      "{} (no target), {} particles live at the end: the step over the empty frame's median, \
       median {:.1} µs, 95th percentile {:.1}",
      probe.show.name(),
      probe.live.load(Ordering::Relaxed),
      us(median),
      us(p95)
    );
  }
}

/// A scene's bench: its engine stepped back to back
fn scene_bench(c: &mut Criterion, show: Show) {
  let mut made: Option<Probe> = None;
  c.bench_function(show.name(), |b| {
    if made.is_none() {
      made = engine_of(show);
    }
    let Some(probe) = made.as_mut() else {
      return;
    };
    b.iter(|| probe.engine.step().expect("a frame"));
  });
}

fn main() {
  let started = SystemTime::now();
  let load = Command::new("uptime")
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_else(|_| "uptime unavailable".into());
  println!("machine: {load}");
  println!("particles bench: 10,000 particles, 100 emitters, and 2,000 colliding");
  let results = guards::results_dir();
  let mut criterion = Criterion::default()
    .sample_size(20)
    .output_directory(&results)
    .configure_from_args();
  if guards::runs(&["particles_10k", "particles_100x100", "particles_collide_2k"]) {
    guard();
  }
  for show in [Show::Ten, Show::Grid, Show::Collide] {
    scene_bench(&mut criterion, show);
  }
  criterion.final_summary();
  for (bench, _, of) in BENCHES {
    guards::print_median(&results, bench, of, started, 1);
  }
  guards::verdict("particles", "No guard has a target yet.");
}
