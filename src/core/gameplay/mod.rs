//! Gameplay on top of physics: movement presets and the input that drives
//! them, trigger behaviours and knockback. Nothing here names Rapier; this
//! module depends on `physics`, never the reverse.

mod events;
mod intent;
mod knockback;
mod platformer;
#[cfg(test)]
pub(crate) mod test_util;
mod top_down;
mod zones;

pub use events::{CheckpointReached, Killed, MovementEvent, MovementEventKind, PickedUp};
pub use intent::{MoveBindings, MoveIntent, read_move_bindings};
pub use knockback::update_knockback;
pub use platformer::{Dash, PlatformerMovement, PlatformerState, WallJump, platformer_movement};
pub use top_down::{TopDownMovement, TopDownState, top_down_movement};
pub use zones::{Checkpoint, KillZone, Pickup, Respawn};
use zones::{
  apply_checkpoints, apply_kill_zones, collect_pickups, place_respawn_points, respawn_pickups,
  warn_untriggered,
};

use crate::core::ecs::IntoScheduleConfigs;
use crate::core::physics::physics_enabled;
use crate::core::schedule::{EngineSchedules, FixedSet, Phase};

/// Add the gameplay systems to `FixedSet::Movement`, chained. They run only
/// while physics does: under a pause overlay (`PhysicsPaused`) a preset would
/// otherwise pile gravity into `Velocity` and act on the menu's input.
pub fn register_gameplay_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::FixedUpdate,
    (
      place_respawn_points,
      warn_untriggered,
      collect_pickups,
      respawn_pickups,
      apply_kill_zones,
      apply_checkpoints,
      read_move_bindings,
      update_knockback,
      platformer_movement,
      top_down_movement,
    )
      .chain()
      .in_set(FixedSet::Movement)
      .run_if(physics_enabled),
  );
}

#[cfg(test)]
mod tests {
  use super::platformer::jump_physics;
  use super::test_util::*;
  use super::{MoveIntent, PlatformerMovement};
  use crate::core::ecs::Velocity;
  use crate::core::scene::PhysicsPaused;

  #[test]
  fn a_character_mid_jump_keeps_its_velocity_while_physics_is_paused() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = player(&mut world, 0.0, 79.0);
    world.entity_mut(e).insert(PlatformerMovement::default());
    run(&mut world, &mut s, 30);
    {
      let mut intent = world.get_mut::<MoveIntent>(e).unwrap();
      intent.jump_pressed = true;
      intent.jump_held = true;
    }
    run(&mut world, &mut s, 5);
    let (pose, velocity) = (at(&world, e), *world.get::<Velocity>(e).unwrap());
    assert!(velocity.dy < 0.0, "rising: {velocity:?}");

    // A pause overlay: physics stops, and so does the preset (no gravity
    // piling up, no input acted on)
    world.resource_mut::<PhysicsPaused>().0 = true;
    for _ in 0..60 {
      world.get_mut::<MoveIntent>(e).unwrap().jump_pressed = true;
      run(&mut world, &mut s, 1);
    }
    assert_eq!(*world.get::<Velocity>(e).unwrap(), velocity);
    assert_eq!(at(&world, e), pose);

    // Resumed, the jump carries on from where it stopped
    world.resource_mut::<PhysicsPaused>().0 = false;
    world.get_mut::<MoveIntent>(e).unwrap().jump_pressed = false;
    run(&mut world, &mut s, 1);
    let (gravity, _) = jump_physics(64.0, 0.38, 1.0 / 60.0);
    let after = world.get::<Velocity>(e).unwrap().dy;
    assert!(
      (after - (velocity.dy + gravity / 60.0)).abs() < 1e-3,
      "{after} after {velocity:?}"
    );
    assert!(
      at(&world, e).translation.y < pose.translation.y,
      "still rising"
    );
  }
}
