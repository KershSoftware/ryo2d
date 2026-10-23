//! Shared helpers for the physics tests.

use super::sync::{PhysicsSchedule, register_physics_systems};
use super::{Collider, Physics, PhysicsConfig, RigidBody};
use crate::core::ecs::test_util::insert_unrelated;
use crate::core::ecs::{Entity, Mut, Transform, World};
use crate::core::scene::PhysicsPaused;
use crate::core::schedule::{EngineSchedules, Phase};
use crate::core::time::Time;

pub(crate) fn world(hz: u32) -> (World, EngineSchedules) {
  let mut world = World::new();
  world.insert_resource(Time::new(hz));
  world.insert_resource(Physics::new());
  world.insert_resource(PhysicsConfig::default());
  world.insert_resource(PhysicsPaused(false));
  crate::core::messages::register_engine_messages(&mut world);
  let mut schedules = EngineSchedules::new();
  register_physics_systems(&mut schedules);
  (world, schedules)
}

/// `world(60)` holding `k` unrelated resources before anything is spawned
/// (`insert_unrelated`): its entities and archetypes are numbered `k`
/// higher, and its queries visit archetypes in another order.
pub(crate) fn world_after(k: usize) -> (World, EngineSchedules) {
  let (mut world, schedules) = world(60);
  insert_unrelated(&mut world, k);
  (world, schedules)
}

/// `entities` in the order Rapier was given their bodies (a fresh set hands
/// out handles in order).
pub(crate) fn by_body_handle(world: &World, entities: &[Entity]) -> Vec<Entity> {
  let physics = world.resource::<Physics>();
  let mut sorted = entities.to_vec();
  sorted.sort_by_key(|e| physics.body_entries[e].handle.into_raw_parts());
  sorted
}

/// `entities` in the order Rapier was given their colliders.
pub(crate) fn by_collider_handle(world: &World, entities: &[Entity]) -> Vec<Entity> {
  let physics = world.resource::<Physics>();
  let mut sorted = entities.to_vec();
  sorted.sort_by_key(|e| physics.collider_entries[e].handle.into_raw_parts());
  sorted
}

/// `entities` in the order Rapier was given their joints.
pub(crate) fn by_joint_handle(world: &World, entities: &[Entity]) -> Vec<Entity> {
  let physics = world.resource::<Physics>();
  let mut sorted = entities.to_vec();
  sorted.sort_by_key(|e| physics.joint_entries[e].handle.map(|h| h.into_raw_parts()));
  sorted
}

/// Physics' own schedule, made if the step has not run yet: tests that
/// place systems inside the step add them here.
pub(crate) fn physics_schedule(world: &mut World) -> Mut<'_, PhysicsSchedule> {
  if !world.contains_resource::<PhysicsSchedule>() {
    world.insert_resource(PhysicsSchedule::new());
  }
  world.resource_mut::<PhysicsSchedule>()
}

pub(crate) fn run(world: &mut World, s: &mut EngineSchedules, steps: usize) {
  for _ in 0..steps {
    s.run(Phase::FixedUpdate, world);
  }
}

pub(crate) fn no_gravity(world: &mut World) {
  *world.resource_mut::<PhysicsConfig>() = PhysicsConfig::default().zero_gravity();
}

pub(crate) fn at(world: &World, e: Entity) -> Transform {
  *world.get::<Transform>(e).unwrap()
}

/// A static floor: a bare collider 400 wide whose top is at `top`.
pub(crate) fn floor(world: &mut World, top: f32) -> Entity {
  world
    .spawn((
      Transform::from_xy(0.0, top + 10.0),
      Collider::rect(400.0, 20.0),
    ))
    .id()
}

pub(crate) fn ball(world: &mut World, x: f32, y: f32) -> Entity {
  world
    .spawn((
      Transform::from_xy(x, y),
      RigidBody::dynamic(),
      Collider::circle(10.0),
    ))
    .id()
}
