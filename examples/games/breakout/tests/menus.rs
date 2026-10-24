use std::path::PathBuf;
use std::sync::OnceLock;

use breakout::scenes::{GameScene, PauseScene};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// The game under the pause menu, so popping the menu (Resume) comes back
/// here instead of emptying the stack, which would quit too
struct Under {
  pushed: bool,
}

impl Scene for Under {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.pushed {
      return SceneAction::Continue;
    }
    self.pushed = true;
    SceneAction::Push(Box::new(PauseScene::new(0, 0)))
  }
}

/// An empty folder to run from, so saves are written there and the player's
/// are never touched, and no UI file is on disk, so the embedded copies load.
/// The working directory is the process's, so both tests share one folder.
fn empty_folder() {
  static FOLDER: OnceLock<PathBuf> = OnceLock::new();
  FOLDER.get_or_init(|| {
    let folder = std::env::temp_dir().join(format!("breakout-menus-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::env::set_current_dir(&folder).unwrap();
    folder
  });
}

#[test]
fn the_hud_shows_the_score_through_its_bindings() {
  empty_folder();
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(800, 600)
      .start_scene(GameScene::new(0, None)),
  ) else {
    return;
  };
  engine.step_frames(5).unwrap();
  let world = engine.world_mut();
  let mut labels = world.query::<&UiLabel>();
  let hud: Vec<String> = labels
    .iter(world)
    .map(|label| label.0.clone())
    .filter(|text| text.starts_with("Score:"))
    .collect();
  assert_eq!(hud, ["Score: 0"]);
}

/// What the web build reads its assets from: no folder at all, so every
/// read is `Unsupported`
struct NoFolder;

impl AssetSource for NoFolder {
  fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
    Err(AssetError::Io {
      path: path.clone(),
      source: std::io::ErrorKind::Unsupported.into(),
    })
  }
}

#[test]
fn the_hud_shows_where_there_is_no_folder_to_read_as_on_the_web() {
  empty_folder();
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(800, 600)
      .asset_source(NoFolder)
      .start_scene(GameScene::new(0, None)),
  ) else {
    return;
  };
  engine.step_frames(5).unwrap();
  let world = engine.world_mut();
  let mut labels = world.query::<&UiLabel>();
  let hud = labels
    .iter(world)
    .filter(|label| label.0.starts_with("Score:"))
    .count();
  assert_eq!(hud, 1, "the embedded HUD");
}

#[test]
fn the_pause_menu_is_navigable_by_pad() {
  empty_folder();

  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(800, 600)
      .start_scene(Under { pushed: false }),
  ) else {
    return;
  };
  engine.step_frames(3).unwrap();
  // Resume is the default; down twice reaches Quit to Desktop
  for _ in 0..2 {
    engine.tap_pad(GamepadButton::DPadDown);
    engine.step_frames(2).unwrap();
  }
  engine.tap_pad(GamepadButton::South);
  let outcome = engine.step_frames(3).unwrap();
  assert_eq!(outcome, ryo2d::testing::StepOutcome::Quit);
}
