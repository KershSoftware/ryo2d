//! A physics world with the gameplay systems, for the gameplay tests.

use crate::core::ecs::{Entity, Transform, World};
use crate::core::input::Input;
use crate::core::physics::{CharacterController, Collider};
use crate::core::schedule::EngineSchedules;

pub(crate) use crate::core::physics::test_util::{at, floor, no_gravity, run};

pub(crate) fn world(hz: u32) -> (World, EngineSchedules) {
  let (mut world, mut schedules) = crate::core::physics::test_util::world(hz);
  world.insert_resource(Input::new());
  super::register_gameplay_systems(&mut schedules);
  (world, schedules)
}

/// A 20×40 platformer character centred at (x, y).
pub(crate) fn player(world: &mut World, x: f32, y: f32) -> Entity {
  world
    .spawn((
      Transform::from_xy(x, y),
      CharacterController::platformer(),
      Collider::rect(20.0, 40.0),
    ))
    .id()
}
