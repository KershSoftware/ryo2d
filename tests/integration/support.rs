//! Helpers for driving engine systems in integration tests.

use ryo2d::core::messages::register_engine_messages;
use ryo2d::core::schedule::EngineSchedules;
use ryo2d::core::systems::register_juice_systems;
use ryo2d::prelude::*;

/// A world with a `hz` fixed clock and the engine messages, plus the juice
/// systems (animation, tweens, knockback, squash and stretch, skeletons) in
/// the fixed update, as the engine registers them.
pub fn juice_world(hz: u32) -> (World, EngineSchedules) {
  let mut world = World::new();
  world.insert_resource(Time::new(hz));
  register_engine_messages(&mut world);
  let mut schedules = EngineSchedules::new();
  register_juice_systems(&mut schedules);
  (world, schedules)
}

/// Run `steps` fixed updates.
pub fn run_fixed(world: &mut World, schedules: &mut EngineSchedules, steps: u32) {
  for _ in 0..steps {
    schedules.run(Phase::FixedUpdate, world);
  }
}

/// Every `T` written so far: the buffers are never advanced in these tests,
/// so one cursor from the start sees them all.
pub fn messages<T: Message + Clone>(world: &World) -> Vec<T> {
  let buffer = world.resource::<Messages<T>>();
  buffer.get_cursor().read(buffer).cloned().collect()
}
