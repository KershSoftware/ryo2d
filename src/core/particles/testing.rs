//! Test worlds for the particle step.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use glam::Vec2;

use super::collision::ParticleCollision;
use super::contact::{ColliderQuery, ParticleCollider, gather_colliders};
use super::draw::instance_for;
use super::effect::ParticleEffect;
use super::emitter::{EmitterFinished, ParticleEmitter};
use super::pool::{EmitterPool, Pool, STUCK, SubPool, TOUCHING};
use super::rng::ParticleRng;
use super::space::LocalFrame;
use super::step::{Compiled, step_particles, step_unscaled_particles};
use super::store::{DEFAULT_PARTICLE_BUDGET, ParticleStore};
use crate::core::asset::{Assets, Handle};
use crate::core::ecs::interpolate::snapshot_previous_transforms;
use crate::core::ecs::kernel::SingleThreadedExecutor;
use crate::core::ecs::propagate::propagate_transforms;
use crate::core::ecs::{
  ChildOf, DrawLift, Entity, GlobalTransform, IVec2, IntoScheduleConfigs, Messages, Mut,
  PreviousGlobalTransform, QueryState, Resource, Schedule, Transform, World,
};
use crate::core::graphics::Image;
use crate::core::messages::register_engine_messages;
use crate::core::physics::{Collider, SlopeCorner, TileCell, TileColliders, TilePart};
use crate::core::time::Time;

/// The systems `step` runs, kept so their state lasts from step to step
#[derive(Resource)]
struct StepSchedule(Schedule);

/// The systems `frame` runs, kept as `StepSchedule` is
#[derive(Resource)]
struct FrameSchedule(Schedule);

/// A world the particle step runs in: a 60 Hz clock, the engine's
/// messages, the image and effect stores, and a store with the default
/// budget seeded with 1
pub(crate) fn world() -> World {
  let mut world = World::new();
  world.insert_resource(Time::new(60));
  register_engine_messages(&mut world);
  world.insert_resource(Assets::<Image>::new());
  world.insert_resource(Assets::<ParticleEffect>::new());
  world.insert_resource(ParticleStore::new(DEFAULT_PARTICLE_BUDGET, 1));
  let mut schedule = Schedule::default();
  schedule.set_executor(SingleThreadedExecutor::new());
  schedule.add_systems(
    (
      propagate_transforms,
      crate::core::tilemap::lift_floored,
      step_particles,
    )
      .chain(),
  );
  world.insert_resource(StepSchedule(schedule));
  let mut schedule = Schedule::default();
  schedule.set_executor(SingleThreadedExecutor::new());
  schedule.add_systems((propagate_transforms, step_unscaled_particles).chain());
  world.insert_resource(FrameSchedule(schedule));
  world
}

/// An 8 x 8 white image
pub(crate) fn image(world: &mut World) -> Handle<Image> {
  world
    .resource_mut::<Assets<Image>>()
    .add(Image::solid_rect(8, 8, 255, 255, 255, 255))
}

/// `effect` added to the world's store
pub(crate) fn effect(world: &mut World, effect: ParticleEffect) -> Handle<ParticleEffect> {
  world.resource_mut::<Assets<ParticleEffect>>().add(effect)
}

/// One fixed step of particles: transforms propagate, floors lift, the
/// particle step runs, then queued commands apply
pub(crate) fn step(world: &mut World) {
  world.resource_scope(|world, mut schedule: Mut<StepSchedule>| {
    schedule.0.run(world);
  });
  world.flush();
}

/// `n` steps
pub(crate) fn steps(world: &mut World, n: usize) {
  for _ in 0..n {
    step(world);
  }
}

/// One frame of `real` time for the unscaled emitters: the clock advances,
/// transforms propagate, the unscaled particles step, then queued commands
/// apply (no fixed step runs)
pub(crate) fn frame(world: &mut World, real: Duration) {
  world.resource_mut::<Time>().advance(real);
  world.resource_scope(|world, mut schedule: Mut<FrameSchedule>| {
    schedule.0.run(world);
  });
  world.flush();
}

/// How far `emitter`'s pool is through its next unscaled step
pub(crate) fn fraction(world: &World, emitter: Entity) -> f32 {
  world
    .get::<ParticleEmitter>(emitter)
    .and_then(|emitter| world.resource::<ParticleStore>().pool(emitter.slot))
    .map_or(0.0, |pool| pool.fraction)
}

/// Where interpolation's snapshot leaves an emitter at the start of a
/// fixed step: a normal step, not a teleport (the engine snapshots before
/// a scene can call `skip_interpolation`)
pub(crate) fn snapshot(world: &mut World) {
  let mut schedule = Schedule::default();
  schedule.set_executor(SingleThreadedExecutor::new());
  schedule.add_systems(snapshot_previous_transforms);
  schedule.run(world);
}

/// One particle as a test sees it
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Seen {
  pub x: f32,
  pub y: f32,
  pub vx: f32,
  pub vy: f32,
  pub age: f32,
  /// As drawn now: the start size times the effect's size curve
  pub size: f32,
  /// As drawn now: the start colour times the effect's gradient
  pub color: [f32; 4],
  /// Stuck where it hit a collider
  pub stuck: bool,
  /// Touched a collider in its last step
  pub touching: bool,
}

/// `emitter`'s particles, oldest first, as its pool keeps them: in the
/// world, or in the emitter's frame for a `Local` effect
pub(crate) fn particles(world: &World, emitter: Entity) -> Vec<Seen> {
  let Some(emitter) = world.get::<ParticleEmitter>(emitter) else {
    return Vec::new();
  };
  let store = world.resource::<ParticleStore>();
  let Some(pool) = store.pool(emitter.slot) else {
    return Vec::new();
  };
  seen(&pool.main, pool.effect.and_then(|id| store.compiled(id)))
}

/// The particles `emitter`'s effect's `on_death` played, oldest first, as
/// its pool keeps them: in the world, or in the emitter's frame for a
/// `Local` sub-effect
pub(crate) fn death_particles(world: &World, emitter: Entity) -> Vec<Seen> {
  sub_particles(world, emitter, |pool| &pool.death)
}

/// The particles `emitter`'s effect's `on_collision` played, as
/// [`death_particles`]
pub(crate) fn collision_particles(world: &World, emitter: Entity) -> Vec<Seen> {
  sub_particles(world, emitter, |pool| &pool.collision)
}

fn sub_particles(
  world: &World,
  emitter: Entity,
  which: impl Fn(&EmitterPool) -> &SubPool,
) -> Vec<Seen> {
  let Some(emitter) = world.get::<ParticleEmitter>(emitter) else {
    return Vec::new();
  };
  let store = world.resource::<ParticleStore>();
  let Some(sub) = store.pool(emitter.slot).map(which) else {
    return Vec::new();
  };
  seen(&sub.particles, sub.effect.and_then(|id| store.compiled(id)))
}

/// `p`'s particles as a test sees them, drawn by `compiled`
fn seen(p: &Pool, compiled: Option<&Compiled>) -> Vec<Seen> {
  (0..p.len())
    .map(|i| {
      let t = p.age[i] / p.lifetime[i];
      let (size, tint) = compiled.map_or((1.0, [1.0; 4]), |c| {
        (
          c.effect.size_over_life.lookup(t),
          c.effect.color_over_life.lookup(t),
        )
      });
      Seen {
        x: p.x[i],
        y: p.y[i],
        vx: p.vx[i],
        vy: p.vy[i],
        age: p.age[i],
        size: p.size[i] * size,
        color: std::array::from_fn(|c| p.color[i][c] * tint[c]),
        stuck: p.flags[i] & STUCK != 0,
        touching: p.flags[i] & TOUCHING != 0,
      }
    })
    .collect()
}

/// Where `emitter`'s particles draw, `alpha` of the way between their last
/// two steps, oldest first: a `Local` effect's through the emitter's frame
/// as it draws, as the drawer places them
pub(crate) fn drawn(world: &World, emitter: Entity, alpha: f32) -> Vec<Vec2> {
  let Some(slot) = world.get::<ParticleEmitter>(emitter).map(|e| e.slot) else {
    return Vec::new();
  };
  let store = world.resource::<ParticleStore>();
  let Some(pool) = store.pool(slot) else {
    return Vec::new();
  };
  let Some(compiled) = pool.effect.and_then(|id| store.compiled(id)) else {
    return Vec::new();
  };
  let local = pool.local.and_then(|_| {
    let global = world.get::<GlobalTransform>(emitter)?;
    let previous = world.get::<PreviousGlobalTransform>(emitter);
    let lift = world.get::<DrawLift>(emitter);
    Some(LocalFrame::drawn(global, previous, lift, alpha))
  });
  (0..pool.main.len())
    .map(|i| {
      let instance = instance_for(&pool.main, i, compiled, alpha, local.as_ref());
      Vec2::new(instance.transform[2][0], instance.transform[2][1])
    })
    .collect()
}

/// How many particles `emitter` has
pub(crate) fn count(world: &World, emitter: Entity) -> usize {
  particles(world, emitter).len()
}

/// Changes `emitter`'s particles as a test places them (index 0 the
/// oldest); nothing if it has no pool
pub(crate) fn edit(world: &mut World, emitter: Entity, f: impl FnOnce(&mut Pool)) {
  let Some(slot) = world.get::<ParticleEmitter>(emitter).map(|e| e.slot) else {
    return;
  };
  let mut store = world.resource_mut::<ParticleStore>();
  if let Some(pool) = store.pools.get_mut(slot as usize).filter(|p| p.used) {
    f(&mut pool.main);
  }
}

/// The particle collisions written so far, in order (a test world never
/// advances its message buffers)
pub(crate) fn collisions(world: &World) -> Vec<ParticleCollision> {
  world
    .get_resource::<Messages<ParticleCollision>>()
    .map_or_else(Vec::new, |messages| {
      messages.get_cursor().read(messages).cloned().collect()
    })
}

/// The emitters reported finished so far, in order (a test world never
/// advances its message buffers)
pub(crate) fn finished(world: &World) -> Vec<Entity> {
  world
    .get_resource::<Messages<EmitterFinished>>()
    .map_or_else(Vec::new, |messages| {
      messages
        .get_cursor()
        .read(messages)
        .map(|finished| finished.entity)
        .collect()
    })
}

/// The dev log's particle warnings are shared by every test in the
/// process: a test that writes one (a clamped effect, a full budget, a
/// nested sub-effect) or counts them, expecting some or none, holds this
pub(crate) fn log_lock() -> MutexGuard<'static, ()> {
  static LOCK: Mutex<()> = Mutex::new(());
  LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The particle warnings in the dev log (which keeps its last 10,000
/// entries, from every test in the process)
pub(crate) fn particle_warnings() -> Vec<String> {
  crate::core::dev_log::get_entries()
    .into_iter()
    .filter(|entry| entry.module.as_deref() == Some("Particles"))
    .map(|entry| entry.message)
    .collect()
}

/// The warnings the particle file loader gives (its target is the asset
/// module's, so the old log labels them `Asset`)
pub(crate) fn particle_loader_warnings() -> Vec<String> {
  crate::core::dev_log::get_entries()
    .into_iter()
    .filter(|entry| entry.module.as_deref() == Some("Asset"))
    .map(|entry| entry.message)
    .collect()
}

/// Spawns a busy level's colliders, for comparing ways of meeting them, and
/// propagates their transforms: a tile map's parts (64 x 48 cells of 16
/// px from (40, 24), its bottom rows ground with holes and the rest a
/// scatter of solid, sloped and one-way cells), circles and boxes (some
/// turned) among them, an open zigzag, and a game's closed polyline arena
/// round it all. It spans about (-48, -48) to (1100, 840), and repeats
pub(crate) fn busy_level(world: &mut World) {
  let mut rng = ParticleRng::new(0xB05_1E7E1);
  let mut tiles = TileColliders::new(64, 48, Vec2::splat(16.0));
  let corners = [
    SlopeCorner::BottomLeft,
    SlopeCorner::BottomRight,
    SlopeCorner::TopLeft,
    SlopeCorner::TopRight,
  ];
  for y in 0..48 {
    for x in 0..64 {
      let roll = rng.next_f32();
      let ground = y >= 44 && roll < 0.9;
      let cell = if ground || roll < 0.22 {
        TileCell::Solid
      } else if roll < 0.26 {
        TileCell::Slope(corners[(rng.next_u64() % 4) as usize])
      } else if roll < 0.30 {
        TileCell::OneWay
      } else {
        TileCell::Empty
      };
      tiles.set(IVec2::new(x, y), cell);
    }
  }
  let parts = tiles.traced_parts();
  let map = world.spawn((Transform::from_xy(40.0, 24.0), tiles)).id();
  for (at, part) in parts {
    world.spawn((Transform::from_xy(at.x, at.y), part, TilePart, ChildOf(map)));
  }
  for n in 0..16 {
    let at = Vec2::new(rng.range(0.0, 1100.0), rng.range(0.0, 800.0));
    let shape = if n % 2 == 0 {
      Collider::circle(rng.range(6.0, 40.0))
    } else {
      let turn = if n % 4 == 1 { rng.range(0.0, 3.0) } else { 0.0 };
      Collider::rect(rng.range(8.0, 80.0), rng.range(8.0, 80.0)).with_rotation(turn)
    };
    world.spawn((Transform::from_xy(at.x, at.y), shape));
  }
  let zigzag = (0..12).map(|i| {
    Vec2::new(
      60.0 + i as f32 * 90.0,
      if i % 2 == 0 { 380.0 } else { 440.0 },
    )
  });
  world.spawn((Transform::IDENTITY, Collider::polyline(zigzag)));
  let (x0, y0, x1, y1) = (-48.0, -48.0, 1100.0, 840.0);
  let arena = [(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)].map(|(x, y)| Vec2::new(x, y));
  world.spawn((Transform::IDENTITY, Collider::polyline(arena)));
  step(world);
}

/// `world`'s colliders and their edges, gathered as a step gathers them
pub(crate) fn gathered(world: &mut World) -> (Vec<ParticleCollider>, Vec<(Vec2, Vec2)>) {
  let mut query: ColliderQuery = QueryState::new(world);
  let (mut colliders, mut edges) = (Vec::new(), Vec::new());
  gather_colliders(world, &mut query, &mut colliders, &mut edges);
  (colliders, edges)
}
