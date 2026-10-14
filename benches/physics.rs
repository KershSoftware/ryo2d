//! Physics benchmark: 1,000 bodies bouncing in a closed box with no gravity
//! and elastic, frictionless contacts. One headless frame runs exactly one
//! fixed step; the empty scene is the frame overhead.
//!
//! The bodies still lose a little energy at every contact, and over
//! thousands of frames they slow down and fall asleep, after which a frame
//! costs almost nothing. So no measurement lets the scene age: each one
//! starts a fresh engine, runs 30 warm-up frames and times a window of at
//! most 200 frames, as many windows as criterion asks frames for. After
//! every bouncing window at most 1% of the bodies may be asleep, or the
//! benchmark stops.

use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const SIZE: f32 = 600.0;
const BODIES: usize = 1000;
/// Frames run before a window is timed
const WARM_UP: u32 = 30;
/// The most frames one engine is timed for
const WINDOW: u64 = 200;

struct Bouncing;

impl Scene for Bouncing {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    for (x, y, w, h) in [
      (SIZE / 2.0, -10.0, SIZE + 40.0, 20.0),
      (SIZE / 2.0, SIZE + 10.0, SIZE + 40.0, 20.0),
      (-10.0, SIZE / 2.0, 20.0, SIZE + 40.0),
      (SIZE + 10.0, SIZE / 2.0, 20.0, SIZE + 40.0),
    ] {
      ctx.world.spawn((
        Transform::from_xy(x, y),
        RigidBody::fixed(),
        Collider::rect(w, h)
          .with_restitution(1.0)
          .with_friction(0.0),
      ));
    }
    for i in 0..BODIES {
      let (col, row) = ((i % 40) as f32, (i / 40) as f32);
      let angle = i as f32 * 2.399;
      let collider = if i % 2 == 0 {
        Collider::circle(5.0)
      } else {
        Collider::rect(8.0, 8.0)
      };
      ctx.world.spawn((
        Transform::from_xy(20.0 + col * 14.0, 20.0 + row * 14.0),
        Velocity {
          dx: angle.cos() * 120.0,
          dy: angle.sin() * 120.0,
        },
        RigidBody::dynamic().with_gravity_scale(0.0).lock_rotation(),
        collider.with_restitution(1.0).with_friction(0.0),
      ));
    }
  }
}

struct Empty;

impl Scene for Empty {}

fn bouncing() -> EngineBuilder {
  EngineBuilder::new()
    .resolution(300, 300)
    .start_scene(Bouncing)
}

fn empty() -> EngineBuilder {
  EngineBuilder::new().resolution(300, 300).start_scene(Empty)
}

/// Panic unless the bouncing scene's bodies are all there and at most 1% of
/// them asleep.
fn check_awake(engine: &mut HeadlessEngine) {
  let world = engine.world_mut();
  let bodies: Vec<Entity> = world
    .query::<(Entity, &RigidBody)>()
    .iter(world)
    .filter(|(_, body)| body.kind == BodyKind::Dynamic)
    .map(|(entity, _)| entity)
    .collect();
  let physics = world.resource::<Physics>();
  let asleep = bodies.iter().filter(|e| physics.is_sleeping(**e)).count();
  assert_eq!(bodies.len(), BODIES, "the scene's bodies");
  assert!(
    asleep * 100 <= BODIES,
    "{asleep} of {BODIES} bodies asleep: the benchmark would time a sleeping world"
  );
}

/// The time `iters` frames take, in windows on fresh engines.
fn time_frames(builder: fn() -> EngineBuilder, iters: u64, live: bool) -> Duration {
  let mut total = Duration::ZERO;
  let mut done = 0;
  while done < iters {
    let mut engine = HeadlessEngine::new(builder()).expect("a headless engine");
    engine.step_frames(WARM_UP).expect("warm-up frames");
    let frames = WINDOW.min(iters - done);
    let start = Instant::now();
    for _ in 0..frames {
      engine.step().expect("frame");
    }
    total += start.elapsed();
    if live {
      check_awake(&mut engine);
    }
    done += frames;
  }
  total
}

fn frame(c: &mut Criterion) {
  // Needs a GPU; without one this benchmark is skipped
  if HeadlessEngine::new_or_skip(empty()).is_none() {
    return;
  }
  c.bench_function("physics frame, 1000 bouncing bodies", |b| {
    b.iter_custom(|iters| time_frames(bouncing, iters, true))
  });
  c.bench_function("physics frame, empty scene", |b| {
    b.iter_custom(|iters| time_frames(empty, iters, false))
  });
}

criterion_group!(benches, frame);
criterion_main!(benches);
