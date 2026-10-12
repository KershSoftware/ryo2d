//! The engine's built-in systems.

mod animation;
mod knockback;
mod physics;
mod skeleton;
mod squash_stretch;
mod trail;
mod tween;

pub use animation::animate_sprites;
pub use knockback::update_knockback;
pub use physics::{
  emit_collision_messages, physics_enabled, register_new_bodies, register_physics_systems,
  register_restored_bodies, step_physics, sync_from_physics, sync_to_physics,
  unregister_removed_bodies,
};
pub use skeleton::update_skeletons;
pub use squash_stretch::update_squash_stretch;
pub use trail::record_trail_points;
pub use tween::{update_property_tweens, update_tween_sequences, update_tweens};

use crate::core::ecs::IntoScheduleConfigs;
use crate::core::schedule::{EngineSchedules, FixedSet, Phase};

/// Add the juice systems to `FixedSet::Juice`, in this order: animation,
/// tweens, tween sequences, property tweens, knockback, squash and stretch.
pub fn register_juice_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::FixedUpdate,
    (
      animate_sprites,
      update_tweens,
      update_tween_sequences,
      update_property_tweens,
      update_knockback,
      update_squash_stretch,
      update_skeletons,
    )
      .chain()
      .in_set(FixedSet::Juice),
  );
}

#[cfg(test)]
pub(crate) mod test_util {
  use crate::core::ecs::{
    IntoScheduleConfigs, Message, Messages, Schedule, ScheduleLabel, ScheduleSystem, World,
  };
  use crate::core::messages::register_engine_messages;
  use crate::core::time::Time;

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Step;

  /// A world with a `Time` whose fixed step is `1 / hz` and the engine messages.
  pub(crate) fn world(hz: u32) -> World {
    let mut world = World::new();
    world.insert_resource(Time::new(hz));
    register_engine_messages(&mut world);
    world
  }

  /// Change the fixed step for the next run.
  pub(crate) fn set_hz(world: &mut World, hz: u32) {
    world.resource_mut::<Time>().fixed.set_hz(hz);
  }

  /// Run `systems` once.
  pub(crate) fn run<M>(world: &mut World, systems: impl IntoScheduleConfigs<ScheduleSystem, M>) {
    let mut schedule = Schedule::new(Step);
    schedule.add_systems(systems);
    schedule.run(world);
  }

  /// Every `T` in the buffers.
  pub(crate) fn messages<T: Message + Clone>(world: &World) -> Vec<T> {
    let buffer = world.resource::<Messages<T>>();
    buffer.get_cursor().read(buffer).cloned().collect()
  }

  /// Drop every buffered message.
  pub(crate) fn clear_messages(world: &mut World) {
    crate::core::messages::clear_messages(world);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Transform, Tween};
  use crate::core::messages::TweenStarted;

  #[test]
  fn the_juice_systems_run_in_the_fixed_update() {
    let mut world = test_util::world(10);
    let entity = world
      .spawn((Transform::IDENTITY, Tween::to_position(100.0, 0.0, 1.0)))
      .id();
    let mut schedules = EngineSchedules::new();
    register_juice_systems(&mut schedules);
    schedules.run(Phase::FixedUpdate, &mut world);
    let x = world.get::<Transform>(entity).unwrap().translation.x;
    assert!(
      (x - 10.0).abs() < 1e-3,
      "one 0.1 s step of a 1 s tween to 100, got {x}"
    );
    assert_eq!(
      test_util::messages::<TweenStarted>(&world),
      [TweenStarted { entity }]
    );
  }

  #[test]
  fn the_juice_systems_place_skeletons() {
    use crate::core::skeleton::{Skeleton, SkeletonDef};
    use glam::Vec2;
    use std::sync::Arc;

    let def = Arc::new(
      SkeletonDef::builder()
        .bone("root", None, Vec2::ZERO, 0.0, 10.0)
        .build(),
    );
    let mut world = test_util::world(10);
    let entity = world
      .spawn((Transform::from_xy(12.0, 34.0), Skeleton::new(def)))
      .id();
    let mut schedules = EngineSchedules::new();
    register_juice_systems(&mut schedules);
    schedules.run(Phase::FixedUpdate, &mut world);
    let skeleton = world.get::<Skeleton>(entity).unwrap();
    assert_eq!(skeleton.world_transforms[0].position, Vec2::new(12.0, 34.0));
  }
}
