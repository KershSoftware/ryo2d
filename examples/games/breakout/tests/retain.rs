//! Breakout's menus and HUD with UI render retention on and off: two engines
//! driven by the same input must draw the same pixels every frame, and the
//! UI renderer draws them in one or two draws

use std::path::PathBuf;
use std::sync::OnceLock;

use breakout::scenes::{GameScene, MenuScene};
use ryo2d::prelude::*;
use ryo2d::testing::PixelPair;

/// A folder holding only the game's levels to run from, so the high score
/// is saved there and the player's is never touched, and no UI file is on
/// disk, so the embedded copies load. The working directory is the
/// process's, so the tests share one folder.
fn game_folder() {
  static FOLDER: OnceLock<PathBuf> = OnceLock::new();
  FOLDER.get_or_init(|| {
    let folder = std::env::temp_dir().join(format!("breakout-retain-{}", std::process::id()));
    let levels = folder.join("levels");
    std::fs::create_dir_all(&levels).unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("levels");
    for entry in std::fs::read_dir(source).unwrap() {
      let path = entry.unwrap().path();
      std::fs::copy(&path, levels.join(path.file_name().unwrap())).unwrap();
    }
    std::env::set_current_dir(&folder).unwrap();
    folder
  });
}

/// Most UI draws a frame may make: the UI renderer draws once per texture
/// run, and the game's text sits on one glyph page
const MAX_DRAWS: u32 = 2;

fn pair<S: Scene + 'static>(start: fn() -> S) -> Option<PixelPair> {
  game_folder();
  PixelPair::new(move || {
    EngineBuilder::new()
      .resolution(1280, 720)
      .fixed_hz(120)
      .start_scene(start())
  })
}

#[test]
fn the_main_menu_looks_the_same_retained() {
  let Some(mut pair) = pair(MenuScene::new) else {
    return;
  };
  pair.frames(30, "the menu opening");
  for key in [KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::ArrowUp] {
    pair.each(|engine| engine.tap_key(key));
    pair.frames(5, "moving through the menu");
  }
  pair.frames(30, "the menu still");
  eprintln!(
    "menu: {} compared, {} kept, {} reused, at most {} UI draws",
    pair.compared(),
    pair.kept(),
    pair.reused(),
    pair.most_draws()
  );
  assert!(pair.kept() > 0);
  let draws = pair.most_draws();
  assert!((1..=MAX_DRAWS).contains(&draws), "{draws} UI draws");
}

#[test]
fn the_pause_menu_and_the_hud_look_the_same_retained() {
  let Some(mut pair) = pair(|| GameScene::new(0, None)) else {
    return;
  };
  pair.each(|engine| engine.tap_key(KeyCode::Space));
  pair.frames(30, "the ball launched");
  for _ in 0..3 {
    pair.each(|engine| engine.press_key(KeyCode::KeyA));
    pair.frames(40, "the paddle left");
    pair.each(|engine| engine.release_key(KeyCode::KeyA));
    pair.each(|engine| engine.press_key(KeyCode::KeyD));
    pair.frames(60, "the paddle right");
    pair.each(|engine| engine.release_key(KeyCode::KeyD));
  }
  pair.each(|engine| engine.tap_key(KeyCode::Escape));
  pair.frames(20, "paused");
  pair.each(|engine| engine.tap_key(KeyCode::ArrowDown));
  pair.frames(10, "paused, moving");
  pair.each(|engine| engine.tap_key(KeyCode::Escape));
  pair.frames(30, "playing again");
  eprintln!(
    "play: {} compared, {} kept, {} reused, at most {} UI draws",
    pair.compared(),
    pair.kept(),
    pair.reused(),
    pair.most_draws()
  );
  assert!(pair.kept() > 0, "the paused menu kept the image");
  assert!(pair.reused() > 0, "the HUD reused its still texts");
  let draws = pair.most_draws();
  assert!((1..=MAX_DRAWS).contains(&draws), "{draws} UI draws");
}
