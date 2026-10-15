//! Physics benchmark: 1,000 bodies bouncing in a closed box with no gravity
//! and elastic, frictionless contacts, and 100 characters running and
//! jumping on a tile level. One headless frame runs exactly one fixed step;
//! the empty scene is the frame overhead.
//!
//! The bouncing scene runs at two sizes. The first has 8 to 10 px bodies in
//! a 600 px box: at the default 100 px/m that is the worst case for Rapier
//! 0.36, whose broad phase pads every collider by 0.04 m (4 px), so pairs
//! are found long before bodies touch. The second is the same scene five
//! times larger (40 to 50 px bodies, a 3,000 px box, five times the speed),
//! the size of typical sprites.
//!
//! The bodies still lose a little energy at every contact, and over
//! thousands of frames they slow down and fall asleep, after which a frame
//! costs almost nothing. So no measurement lets the scene age: each one
//! starts a fresh engine, runs 30 warm-up frames and times a window of at
//! most 200 frames, as many windows as criterion asks frames for. After
//! every bouncing window at most 1% of the bodies may be asleep, or the
//! benchmark stops.
//!
//! The character scene is a 120 x 40 `TileColliders` level of 16 px tiles
//! (a floor, stairs and one-way rows) with 100 platformer characters
//! (`PlatformerMovement` on 12 x 24 px colliders) whose `MoveIntent` a scene
//! system drives: each turns round every 2 s and jumps every 1.5 s, offset
//! by its index. It is timed in the same fresh-engine windows; after every
//! window each character must still be inside the level.

use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const SIZE: f32 = 600.0;
const BODIES: usize = 1000;
/// The typical-size scene's scale
const TYPICAL: f32 = 5.0;
/// Frames run before a window is timed
const WARM_UP: u32 = 30;
/// The most frames one engine is timed for
const WINDOW: u64 = 200;
/// The character scene's level, in tiles of `TILE` px
const LEVEL: (u32, u32) = (120, 40);
const TILE: f32 = 16.0;
const CHARACTERS: u32 = 100;

/// The bouncing scene, every length and speed multiplied by its scale.
struct Bouncing(f32);

impl Scene for Bouncing {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let k = self.0;
    for (x, y, w, h) in [
      (SIZE / 2.0, -10.0, SIZE + 40.0, 20.0),
      (SIZE / 2.0, SIZE + 10.0, SIZE + 40.0, 20.0),
      (-10.0, SIZE / 2.0, 20.0, SIZE + 40.0),
      (SIZE + 10.0, SIZE / 2.0, 20.0, SIZE + 40.0),
    ] {
      let (x, y, w, h) = (x * k, y * k, w * k, h * k);
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
        Collider::circle(5.0 * k)
      } else {
        Collider::rect(8.0 * k, 8.0 * k)
      };
      ctx.world.spawn((
        Transform::from_xy((20.0 + col * 14.0) * k, (20.0 + row * 14.0) * k),
        Velocity {
          dx: angle.cos() * 120.0 * k,
          dy: angle.sin() * 120.0 * k,
        },
        RigidBody::dynamic().with_gravity_scale(0.0).lock_rotation(),
        collider.with_restitution(1.0).with_friction(0.0),
      ));
    }
  }
}

struct Empty;

impl Scene for Empty {}

/// A tile level: a floor two tiles deep, walls and a ceiling, a stair up two
/// tiles and down again every 20 columns, and one-way rows 3 and 7 tiles
/// above the floor.
fn level() -> TileColliders {
  let (width, height) = LEVEL;
  let mut tiles = TileColliders::new(width, height, Vec2::splat(TILE));
  for x in 0..width {
    for y in [0, height - 2, height - 1] {
      tiles.set(x, y, TileCell::Solid);
    }
  }
  for y in 0..height {
    tiles.set(0, y, TileCell::Solid);
    tiles.set(width - 1, y, TileCell::Solid);
  }
  for start in (10..width - 10).step_by(20) {
    for (i, steps) in [1, 2, 2, 1].into_iter().enumerate() {
      for step in 0..steps {
        tiles.set(start + i as u32, height - 3 - step, TileCell::Solid);
      }
    }
  }
  for (row, from, to) in [(35, 4, 50), (35, 64, 110), (31, 20, 60), (31, 76, 116)] {
    for x in from..to {
      tiles.set(x, row, TileCell::OneWay);
    }
  }
  tiles
}

/// A character the scene drives, by its index.
#[derive(Component)]
struct Bot(u32);

/// Turn every 2 s and jump every 1.5 s (holding it 0.4 s), each character
/// offset by its index.
fn drive_bots(time: Res<Time>, mut bots: Query<(&Bot, &mut MoveIntent)>) {
  let hz = (1.0 / time.fixed.timestep()).round() as u64;
  let (turn, jump, hold) = (2 * hz, 3 * hz / 2, 2 * hz / 5);
  for (bot, mut intent) in &mut bots {
    let step = time.fixed.step_count() + u64::from(bot.0) * 7;
    let right = (step / turn).is_multiple_of(2);
    intent.direction = Vec2::new(if right { 1.0 } else { -1.0 }, 0.0);
    intent.jump_pressed = step.is_multiple_of(jump);
    intent.jump_held = step % jump < hold;
  }
}

/// 100 characters spread along the level, each standing on the solid
/// ground below it.
struct Characters;

impl Scene for Characters {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let tiles = level();
    let height = LEVEL.1;
    for i in 0..CHARACTERS {
      let x = 40.0 + i as f32 * 18.4;
      let column = (x / TILE) as u32;
      let ground = (1..height)
        .find(|&row| tiles.get(column, row) == TileCell::Solid)
        .unwrap_or(height - 2);
      ctx.world.spawn((
        Bot(i),
        Transform::from_xy(x, ground as f32 * TILE - 13.0),
        CharacterController::platformer(),
        Collider::rect(12.0, 24.0),
        PlatformerMovement::default(),
      ));
    }
    ctx.world.spawn((Transform::IDENTITY, tiles));
    ctx
      .add_systems(Phase::FixedUpdate, drive_bots)
      .expect("a scene's fixed systems");
  }
}

fn bouncing() -> EngineBuilder {
  EngineBuilder::new()
    .resolution(300, 300)
    .start_scene(Bouncing(1.0))
}

fn typical() -> EngineBuilder {
  EngineBuilder::new()
    .resolution(300, 300)
    .start_scene(Bouncing(TYPICAL))
}

fn empty() -> EngineBuilder {
  EngineBuilder::new().resolution(300, 300).start_scene(Empty)
}

fn characters() -> EngineBuilder {
  EngineBuilder::new()
    .resolution(300, 300)
    .start_scene(Characters)
}

/// Panic unless a bouncing scene's bodies are all there and at most 1% of
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

/// Panic unless the character scene's characters are all there, inside the
/// level.
fn check_inside(engine: &mut HeadlessEngine) {
  let world = engine.world_mut();
  let size = Vec2::new(LEVEL.0 as f32, LEVEL.1 as f32) * TILE;
  let places: Vec<Vec2> = world
    .query::<(&Bot, &Transform)>()
    .iter(world)
    .map(|(_, transform)| transform.translation)
    .collect();
  assert_eq!(places.len(), CHARACTERS as usize, "the scene's characters");
  for at in places {
    assert!(
      at.x > TILE && at.x < size.x - TILE && at.y > TILE && at.y < size.y - TILE,
      "a character left the level: {at}"
    );
  }
}

/// The time `iters` frames take, in windows on fresh engines, each checked
/// by `check` afterwards.
fn time_frames(
  builder: fn() -> EngineBuilder,
  iters: u64,
  check: Option<fn(&mut HeadlessEngine)>,
) -> Duration {
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
    if let Some(check) = check {
      check(&mut engine);
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
    b.iter_custom(|iters| time_frames(bouncing, iters, Some(check_awake)))
  });
  c.bench_function("physics frame, 1000 bouncing 40-50 px bodies", |b| {
    b.iter_custom(|iters| time_frames(typical, iters, Some(check_awake)))
  });
  c.bench_function("physics frame, 100 characters on tiles", |b| {
    b.iter_custom(|iters| time_frames(characters, iters, Some(check_inside)))
  });
  c.bench_function("physics frame, empty scene", |b| {
    b.iter_custom(|iters| time_frames(empty, iters, None))
  });
}

criterion_group!(benches, frame);
criterion_main!(benches);
