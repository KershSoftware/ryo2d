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
use crate::core::schedule::{EngineSchedules, FixedSet, Phase};

/// Add the gameplay systems to `FixedSet::Movement`, chained.
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
      .in_set(FixedSet::Movement),
  );
}
