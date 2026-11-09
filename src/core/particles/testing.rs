//! Test worlds for the particle step.

use std::sync::{Mutex, MutexGuard, PoisonError};

use super::effect::ParticleEffect;
use super::emitter::ParticleEmitter;
use super::step::step_particles;
use super::store::{DEFAULT_PARTICLE_BUDGET, ParticleStore};
use crate::core::asset::{Assets, Handle};
use crate::core::ecs::interpolate::snapshot_previous_transforms;
use crate::core::ecs::kernel::SingleThreadedExecutor;
use crate::core::ecs::propagate::propagate_transforms;
use crate::core::ecs::{Entity, IntoScheduleConfigs, Mut, Resource, Schedule, World};
use crate::core::graphics::Image;
use crate::core::messages::register_engine_messages;
use crate::core::time::Time;

/// The systems `step` runs, kept so their state lasts from step to step
#[derive(Resource)]
struct StepSchedule(Schedule);

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
}

/// `emitter`'s particles, oldest first
pub(crate) fn particles(world: &World, emitter: Entity) -> Vec<Seen> {
  let Some(emitter) = world.get::<ParticleEmitter>(emitter) else {
    return Vec::new();
  };
  let store = world.resource::<ParticleStore>();
  let Some(pool) = store.pool(emitter.slot) else {
    return Vec::new();
  };
  let compiled = pool.effect.and_then(|id| store.compiled(id));
  let p = &pool.main;
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
      }
    })
    .collect()
}

/// How many particles `emitter` has
pub(crate) fn count(world: &World, emitter: Entity) -> usize {
  particles(world, emitter).len()
}

/// The dev log's particle warnings are shared by every test in the
/// process: a test that counts them, or expects none, holds this
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
