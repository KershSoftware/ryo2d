//! `audio_system`: the frame's audio work

use super::AudioEngine;
use crate::core::ecs::World;
use crate::core::ecs::kernel::Mut;
use crate::core::time::Time;

/// `PostUpdate`, after the cameras: applies the voices' commands queued this
/// frame (the scene's update has run), builds the bus tree from the mixer
/// when it lands or changes, reports the device's stream errors, advances
/// the mock backend by the frame's real time, and retires the voices that
/// ended. A world without an [`AudioEngine`] is left alone
#[doc(hidden)]
pub fn audio_system(world: &mut World) {
  let real_delta = world
    .get_resource::<Time>()
    .map(|time| time.real.delta_duration())
    .unwrap_or_default();
  // The engine is taken out of the world while it reads the stores
  world.try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
    engine.update(world, real_delta);
  });
}
