//! `audio_system`: the frame's audio work

use super::AudioEngine;
use crate::core::asset::AssetServer;
use crate::core::ecs::World;
use crate::core::time::Time;

/// `PostUpdate`, after the cameras: applies the voices' commands queued this
/// frame (the scene's update has run), reports the device's stream errors,
/// advances the mock backend by the frame's real time, and retires the
/// voices that ended. A world without an [`AudioEngine`] is left alone
#[doc(hidden)]
pub fn audio_system(world: &mut World) {
  let real_delta = world
    .get_resource::<Time>()
    .map(|time| time.real.delta_duration())
    .unwrap_or_default();
  // Names clips in the log and in warnings; a clone shares the server
  let server = world.get_resource::<AssetServer>().cloned();
  if let Some(mut engine) = world.get_resource_mut::<AudioEngine>() {
    engine.update(real_delta, server.as_ref());
  }
}
