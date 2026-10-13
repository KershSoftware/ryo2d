//! What a scene switch or reset clears.

use crate::core::audio::Audio;
use crate::core::ecs::World;
use crate::core::ecs::world_ext::despawn_game_entities;
use crate::core::graphics::camera_mut::reset_main_camera;
use crate::core::lighting::LightingSettings;
use crate::core::messages::clear_messages;
use crate::core::shaders::EffectsManager;
use crate::core::tilemap::manager::forget_despawned_tilemaps;
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
    forget_despawned_tilemaps(world);
  }
  if cleanup.contains(SceneCleanup::CAMERA) {
    reset_main_camera(world);
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::camera_mut::spawn_main_camera;
  use crate::core::graphics::{Camera2d, CameraMut, Presentation, ScreenMapping};
  use glam::Vec2;

  fn cleanup_world() -> (World, Audio) {
    let mut world = World::new();
    crate::core::scene::init_world_resources_for_tests(&mut world);
    (world, Audio::new(None))
  }

  #[test]
  fn world_cleanup_keeps_the_main_camera_and_camera_cleanup_resets_it() {
    let (mut world, mut audio) = cleanup_world();
    world.insert_resource(ScreenMapping::new(
      Presentation::ResolutionIndependent,
      (800, 600),
      1.0,
    ));
    let entity = spawn_main_camera(&mut world, Vec2::new(800.0, 600.0));
    CameraMut::main(&mut world).unwrap().set_zoom(3.0);
    execute_cleanup(SceneCleanup::WORLD, &mut world, &mut audio);
    assert!(
      world.get_entity(entity).is_ok(),
      "the main camera survives a world cleanup"
    );
    execute_cleanup(SceneCleanup::CAMERA, &mut world, &mut audio);
    assert_eq!(world.get::<Camera2d>(entity).unwrap().zoom, 1.0);
  }
}
