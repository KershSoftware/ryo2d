//! Shared helpers for the physics tests.

use super::sync::register_physics_systems;
use super::{Collider, Physics, PhysicsConfig, RigidBody};
use crate::core::ecs::{Entity, Transform, World};
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
      Collider::box_collider(400.0, 20.0),
    ))
    .id()
}

pub(crate) fn ball(world: &mut World, x: f32, y: f32) -> Entity {
  world
    .spawn((
      Transform::from_xy(x, y),
      RigidBody::dynamic(),
      Collider::circle_collider(10.0),
    ))
    .id()
}
