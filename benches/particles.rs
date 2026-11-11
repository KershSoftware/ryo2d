//! Particles against an empty frame, with a GPU (`HeadlessEngine`, 1920 x
//! 1080): 10,000 particles from one emitter, 100 emitters of 100 particles
//! in a grid, and 2,000 particles bouncing on a 64 x 64 tile map's colliders.
//!
//! Each scene adds its effect once and places it with emitters seeded with
//! `ParticleEmitter::with_seed(7)`; the engine steps and draws them, and the
//! scene reads the live count in `Scene::update`. Before criterion runs,
//! four things are measured:
//! - the frame guard: every scene's engine and an empty engine stepped in
//!   turn, interleaved (an engine stepping evicts the others' data), the
//!   GPU drained outside the measure, each scene's median step over the
//!   empty frame's median held to its target (`TEN_K_TARGET`,
//!   `HUNDRED_TARGET`, `COLLIDE_TARGET`), the spec's, and to the
//!   scene-driven system's figure for the same scene (`TEN_K_BEFORE`,
//!   `HUNDRED_BEFORE`, `COLLIDE_BEFORE`), so a regression against it fails
//!   the run. Beside each it prints the 95th percentile and, with no
//!   target, the frame's particle systems' own time
//!   (`HeadlessEngine::particle_systems_time`, the profiler's
//!   `cpu:particles` and `cpu:render:particles` scopes) and its render step over
//!   the empty frame's;
//! - `particles_idle`: the particle systems' own work with no emitters
//!   (`step_particles`, `step_unscaled_particles` and the drawer, run outside
//!   a frame, so their frame timers are not in it, through
//!   `HeadlessEngine::idle_particle_systems`), 100 batches of 10,000 runs,
//!   the median batch's time a run held to `IDLE_TARGET`, the spec's. The
//!   criterion bench of the same name times a run for the profile;
//! - `particles_prewarm`, recorded with no target: a prewarmed effect of
//!   1,000 particles (rate 500, lifetime 2, its cycle 2 s) added to an
//!   engine with no particles, `cpu:particles` in the frame it starts and
//!   the whole frame over an ordinary one;
//! - `particles_reload`, recorded with no target: a sample `.particles.ron`
//!   (the demo's sparks file, its rate changed each time) reloaded, from the
//!   reload to its new version in the store, in a world with the asset
//!   registry and no engine, the asset workers running.
//!
//! A missed target fails the run once criterion's benches have run. Each
//! guard runs only where criterion measures the bench of its name: with no
//! filter or one naming it, and not for `--list`, `--test` or
//! `--profile-time`. Criterion then steps each scene's engine back to back,
//! and the medians are read back from where it writes them
//! (`guards::results_dir`).

mod guards;

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use criterion::Criterion;
use ryo2d::core::asset::pump;
use ryo2d::core::ecs::kernel::ChildOf;
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, asset_world_over, shared_gpu};

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
/// The spec's targets: each scene's median frame over the empty frame's
const TEN_K_TARGET: Duration = Duration::from_micros(1_000);
const HUNDRED_TARGET: Duration = Duration::from_micros(1_200);
const COLLIDE_TARGET: Duration = Duration::from_micros(1_000);
/// The scene-driven system this one replaced, on the same scenes: the
/// medians of three rounds of its bench (at `d127c8ab`), each scene's median
/// step over the empty frame's, run alternately with this bench in one
/// session on the record machine (`docs/backlog/particles.md`, "After").
/// That bench no longer exists here, so its figures are held as constants;
/// like the targets, they are for that machine
const TEN_K_BEFORE: Duration = Duration::from_nanos(669_300);
const HUNDRED_BEFORE: Duration = Duration::from_nanos(704_800);
const COLLIDE_BEFORE: Duration = Duration::from_nanos(285_300);
/// The spec's no-emitters frame: the particle systems' own time
const IDLE_TARGET: Duration = Duration::from_micros(1);
/// The no-emitters guard: batches, and runs in each
const IDLE_BATCHES: usize = 100;
const IDLE_CALLS: u32 = 10_000;
/// Times a prewarmed effect is added and a sample file reloaded, each
/// median of these and a few first ones left out
const PREWARMS: usize = 30;
const RELOADS: usize = 30;
const LEFT_OUT: usize = 3;
/// A prewarmed effect holds this many particles
const PREWARMED: usize = 1_000;
/// The sample effect file reloaded, under the bench's source of files
const RELOAD_FILE: &str = "fx/sample.particles.ron";

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

  /// Its median frame's target over the empty frame's (the empty frame has
  /// none)
  fn target(self) -> Option<Duration> {
    match self {
      Show::Empty => None,
      Show::Ten => Some(TEN_K_TARGET),
      Show::Grid => Some(HUNDRED_TARGET),
      Show::Collide => Some(COLLIDE_TARGET),
    }
  }

  /// The scene-driven system's median frame over the empty frame's on this
  /// scene (the empty frame has none)
  fn before(self) -> Option<Duration> {
    match self {
      Show::Empty => None,
      Show::Ten => Some(TEN_K_BEFORE),
      Show::Grid => Some(HUNDRED_BEFORE),
      Show::Collide => Some(COLLIDE_BEFORE),
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

/// An engine of the bench and the live count its scene reports, with what
/// the guard measures of its frames
struct Probe {
  show: Show,
  engine: HeadlessEngine,
  live: Arc<AtomicUsize>,
  /// Each frame's step, the particle systems' CPU time in it (the
  /// `cpu:particles` and `cpu:render:particles` scopes), and its render step
  steps: Vec<Duration>,
  cpu: Vec<Duration>,
  draw: Vec<Duration>,
  render: Vec<Duration>,
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
  Some(Probe {
    show,
    engine,
    live,
    steps: Vec::with_capacity(ROUNDS),
    cpu: Vec::with_capacity(ROUNDS),
    draw: Vec::with_capacity(ROUNDS),
    render: Vec::with_capacity(ROUNDS),
  })
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

fn median(times: &[Duration]) -> Duration {
  spread(times.to_vec()).0
}

/// The guard: every scene's engine and an empty one stepped in turn,
/// `ROUNDS` times, each scene's median step over the empty frame's median
/// held to its target and to the scene-driven system's figure
fn guard() {
  let mut probes = Vec::new();
  for show in [Show::Empty, Show::Ten, Show::Grid, Show::Collide] {
    let Some(probe) = engine_of(show) else {
      return;
    };
    probes.push(probe);
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
      let probe = &mut probes[index];
      settle_gpu();
      let start = Instant::now();
      probe.engine.step().expect("a guard frame");
      probe.steps.push(start.elapsed());
      let (cpu, draw) = probe.engine.particle_systems_time();
      probe.cpu.push(cpu);
      probe.draw.push(draw);
      probe.render.push(probe.engine.render_time());
    }
  }
  let empty_probe = &probes[0];
  let empty = median(&empty_probe.steps);
  let empty_render = median(&empty_probe.render);
  eprintln!(
    "particles guard: {ROUNDS} interleaved frames at {}x{}. An empty frame: step median {:.1} \
     µs, the particle systems' own time median {:.2} µs (the frame's timers in it)",
    RESOLUTION.0,
    RESOLUTION.1,
    us(empty),
    us(median(&empty_probe.cpu)) + us(median(&empty_probe.draw)),
  );
  for probe in &probes[1..] {
    let over: Vec<Duration> = probe
      .steps
      .iter()
      .map(|t| t.saturating_sub(empty))
      .collect();
    let (held, p95) = spread(over);
    let target = probe.show.target().expect("every scene has a target");
    let before = probe.show.before().expect("every scene has a before");
    let word = |limit: Duration| if held <= limit { "met" } else { "missed" };
    let line = format!(
      "{}, {} particles live: the step over the empty frame's median, median {:.1} µs, 95th \
       percentile {:.1}, target {} µs: {}, the scene-driven system's {:.1} µs: {}; \
       cpu:particles median {:.1} µs, cpu:render:particles median {:.1} µs, the render step over \
       the empty frame's {:+.1} µs",
      probe.show.name(),
      probe.live.load(Ordering::Relaxed),
      us(held),
      us(p95),
      target.as_micros(),
      word(target),
      us(before),
      word(before),
      us(median(&probe.cpu)),
      us(median(&probe.draw)),
      us(median(&probe.render)) - us(empty_render),
    );
    eprintln!("{line}");
    if held > target || held > before {
      guards::miss(line);
    }
  }
}

/// A batch of the particle systems' work with no emitters: the time a run
fn idle_batch(engine: &mut HeadlessEngine) -> Duration {
  engine.idle_particle_systems(IDLE_CALLS) / IDLE_CALLS
}

/// The no-emitters guard: the median batch's time a run, held to the
/// target. The first batch, which makes the systems' kept states, is left
/// out
fn idle_guard(engine: &mut HeadlessEngine) {
  idle_batch(engine);
  let mut batches: Vec<Duration> = (0..IDLE_BATCHES).map(|_| idle_batch(engine)).collect();
  batches.sort_unstable();
  let mid = batches[batches.len() / 2];
  let (min, max) = (batches[0], batches[batches.len() - 1]);
  let ns = |time: Duration| time.as_secs_f64() * 1e9;
  println!(
    "particles_idle: median {:.1} ns a run over {IDLE_BATCHES} batches of {IDLE_CALLS} (min \
     {:.1}, max {:.1}), target {} ns",
    ns(mid),
    ns(min),
    ns(max),
    IDLE_TARGET.as_nanos(),
  );
  if mid > IDLE_TARGET {
    guards::miss(format!(
      "particles_idle median {:.1} ns over {} ns",
      ns(mid),
      IDLE_TARGET.as_nanos()
    ));
  }
}

/// The prewarm's cost: an effect of 1,000 particles added to an engine with
/// no particles, `cpu:particles` in the frame it starts (its prewarm
/// included: 2 s of steps) and the whole frame over an ordinary one, the
/// median of `PREWARMS` (the first, which compile it, left out)
fn prewarm_record() {
  let Some(Probe { mut engine, .. }) = engine_of(Show::Empty) else {
    return;
  };
  let sprite = engine
    .assets()
    .add(Image::solid_circle(16, 255, 255, 255, 255));
  let warmed = effect(sprite, 500.0, PREWARMED as u32)
    .with_duration(2.0)
    .with_prewarm(true);
  let handle = engine.assets().add(warmed);
  let mut ordinary = Vec::with_capacity(PREWARMS + LEFT_OUT);
  let (mut started, mut frames) = (Vec::new(), Vec::new());
  let mut live = 0;
  for round in 0..PREWARMS + LEFT_OUT {
    settle_gpu();
    let start = Instant::now();
    engine.step().expect("an ordinary frame");
    ordinary.push(start.elapsed());
    let (transform, emitter) = emitter(960.0, 540.0, &handle);
    let entity = engine.world_mut().spawn((transform, emitter)).id();
    settle_gpu();
    let start = Instant::now();
    engine.step().expect("the frame it starts");
    let took = start.elapsed();
    if round >= LEFT_OUT {
      started.push(engine.particle_systems_time().0);
      frames.push(took);
    }
    live = ParticlesView::new(engine.world_mut()).count();
    engine.world_mut().despawn(entity);
  }
  let plain = median(&ordinary[LEFT_OUT..]);
  let (held, p95) = spread(started);
  println!(
    "particles_prewarm (no target): {live} particles alive after it, {:.1} µs of cpu:particles \
     in the frame it starts, median over {PREWARMS} (95th percentile {:.1}), the frame over an \
     ordinary one {:.1} µs",
    us(held),
    us(p95),
    us(median(&frames).saturating_sub(plain)),
  );
}

/// The sample file's reload time: from the reload to its new version in
/// the store, the asset workers running and the main thread pumping
fn reload_record() {
  let files = MemorySource::new();
  files.insert(
    "fx/spark.png",
    include_bytes!("../examples/demos/particles/assets/fx/spark.png").to_vec(),
  );
  let text = include_str!("../examples/demos/particles/assets/fx/sparks.particles.ron");
  let versions = [text.to_string(), text.replace("rate: 0", "rate: 1")];
  files.insert(RELOAD_FILE, versions[0].clone());
  let mut world = asset_world_over(files.clone());
  let server = world.resource::<AssetServer>().clone();
  let effect = server
    .load_now::<ParticleEffect>(&mut world, RELOAD_FILE)
    .expect("the sample effect file");
  let mut times = Vec::with_capacity(RELOADS);
  for reload in 1..=RELOADS + LEFT_OUT {
    let before = world.resource::<Assets<ParticleEffect>>().version(&effect);
    files.insert(RELOAD_FILE, versions[reload % 2].clone());
    let start = Instant::now();
    server.reload(RELOAD_FILE);
    loop {
      pump(&mut world);
      if world.resource::<Assets<ParticleEffect>>().version(&effect) != before {
        break;
      }
      assert!(
        start.elapsed() < Duration::from_secs(5),
        "the sample file's reload landed"
      );
      std::hint::spin_loop();
    }
    let took = start.elapsed();
    if reload > LEFT_OUT {
      times.push(took);
    }
  }
  let bytes = text.len();
  let (min, max) = (
    times.iter().min().copied().unwrap_or_default(),
    times.iter().max().copied().unwrap_or_default(),
  );
  let (held, p95) = spread(times);
  println!(
    "particles_reload (no target): the sample file ({bytes} B of RON) reloaded, from the reload \
     to its new version in the store, median {:.1} µs over {RELOADS} reloads (95th percentile \
     {:.1}, min {:.1}, max {:.1})",
    us(held),
    us(p95),
    us(min),
    us(max),
  );
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

fn idle_bench(c: &mut Criterion, engine: &mut HeadlessEngine) {
  c.bench_function("particles_idle", |b| {
    b.iter(|| engine.idle_particle_systems(1));
  });
}

fn main() {
  let started = SystemTime::now();
  let load = Command::new("uptime")
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_else(|_| "uptime unavailable".into());
  println!("machine: {load}");
  println!(
    "particles bench: 10,000 particles, 100 emitters, 2,000 colliding, no emitters, a prewarm, a \
     reload"
  );
  let results = guards::results_dir();
  let mut criterion = Criterion::default()
    .sample_size(20)
    .output_directory(&results)
    .configure_from_args();
  if guards::runs(&["particles_10k", "particles_100x100", "particles_collide_2k"]) {
    guard();
  }
  let idle = guards::runs(&["particles_idle"]);
  let mut idle_engine = if idle { engine_of(Show::Empty) } else { None };
  if let Some(probe) = &mut idle_engine {
    idle_guard(&mut probe.engine);
  }
  if guards::runs(&["particles_prewarm"]) {
    prewarm_record();
  }
  if guards::runs(&["particles_reload"]) {
    reload_record();
  }
  for show in [Show::Ten, Show::Grid, Show::Collide] {
    scene_bench(&mut criterion, show);
  }
  if let Some(probe) = &mut idle_engine {
    idle_bench(&mut criterion, &mut probe.engine);
  }
  criterion.final_summary();
  for (bench, _, of) in BENCHES {
    guards::print_median(&results, bench, of, started, 1);
  }
  guards::print_median(
    &results,
    "particles_idle",
    "the particle systems' runs with no emitters",
    started,
    3,
  );
  guards::verdict(
    "particles",
    "Profile a miss (the bench binary is the one `cargo bench --bench particles --no-run` names) \
     with\n  xcrun xctrace record --template 'Time Profiler' --output particles.trace --launch -- \
     target/release/deps/particles-<hash> --bench --profile-time 20 '^particles_10k$'",
  );
  eprintln!("particles guards: every guard that ran met its target");
}
