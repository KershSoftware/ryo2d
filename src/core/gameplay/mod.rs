//! Gameplay on top of physics: movement presets and the input that drives
//! them, trigger behaviours and knockback. Nothing here names Rapier; this
//! module depends on `physics`, never the reverse.

mod events;
mod intent;
mod knockback;
mod platformer;
#[cfg(test)]
pub(crate) mod test_util;

pub use events::{MovementEvent, MovementEventKind};
pub use intent::{MoveBindings, MoveIntent, read_move_bindings};
pub use knockback::update_knockback;
pub use platformer::{PlatformerMovement, PlatformerState, platformer_movement};

use crate::core::ecs::IntoScheduleConfigs;
use crate::core::schedule::{EngineSchedules, FixedSet, Phase};

/// Add the gameplay systems to `FixedSet::Movement`, chained.
pub fn register_gameplay_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::FixedUpdate,
    (read_move_bindings, update_knockback, platformer_movement)
      .chain()
      .in_set(FixedSet::Movement),
  );
}
