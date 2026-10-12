//! What a scene switch or reset clears.

use crate::core::audio::Audio;
use crate::core::ecs::World;
use crate::core::ecs::world_ext::despawn_game_entities;
use crate::core::graphics::Camera;
use crate::core::lighting::LightingSettings;
use crate::core::messages::clear_messages;
use crate::core::shaders::EffectsManager;
use crate::core::time::Time;

use super::SceneCleanup;

/// Reset the engine state `cleanup` names. Runs after the exiting scene's
/// `on_exit` on `Switch` and `Reset`.
pub(super) fn execute_cleanup(cleanup: SceneCleanup, world: &mut World, audio: &mut Audio) {
  if cleanup.is_empty() {
    return;
  }
  if cleanup.contains(SceneCleanup::WORLD) {
    despawn_game_entities(world);
  }
  if cleanup.contains(SceneCleanup::CAMERA)
    && let Some(mut camera) = world.get_resource_mut::<Camera>()
  {
    camera.reset();
  }
  if cleanup.contains(SceneCleanup::EFFECTS)
    && let Some(mut effects) = world.get_resource_mut::<EffectsManager>()
  {
    effects.clear();
  }
  if cleanup.contains(SceneCleanup::LIGHTING)
    && let Some(mut lighting) = world.get_resource_mut::<LightingSettings>()
  {
    *lighting = LightingSettings::default();
  }
  if cleanup.contains(SceneCleanup::TIME)
    && let Some(mut time) = world.get_resource_mut::<Time>()
  {
    time.game.set_speed(1.0);
    time.game.resume();
    time.game.clear_hitstop();
  }
  if cleanup.contains(SceneCleanup::MUSIC) {
    audio.stop_music();
  }
  if cleanup.contains(SceneCleanup::MESSAGES) {
    clear_messages(world);
  }
}
