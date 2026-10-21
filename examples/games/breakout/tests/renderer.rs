//! Breakout's menus and HUD on the old UI path and the UI renderer: two engines
//! driven by the same input must draw the same pixels every frame

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
    let folder = std::env::temp_dir().join(format!("breakout-renderer-{}", std::process::id()));
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

/// Run `n` frames of the pair, noting the most draws the new path made in a
/// frame it drew
fn frames(pair: &mut PixelPair, most: &mut u32, n: u32, what: &str) {
  for _ in 0..n {
    pair.frames(1, what);
    *most = (*most).max(pair.b().ui_draw_count());
  }
}

/// Most draws a frame may make: the UI atlas plus one
const MAX_DRAWS: u32 = 2;

fn pair<S: Scene + 'static>(start: fn() -> S) -> Option<PixelPair> {
  game_folder();
  PixelPair::between(
    move || {
      EngineBuilder::new()
        .resolution(1280, 720)
        .fixed_hz(120)
        .start_scene(start())
    },
    |b| b.ui_dedicated(false),
    |b| b.ui_dedicated(true),
  )
}

#[test]
fn the_main_menu_looks_the_same_on_the_ui_renderer() {
  let mut most = 0;
  let Some(mut pair) = pair(MenuScene::new) else {
    return;
  };
  frames(&mut pair, &mut most, 30, "the menu opening");
  for key in [KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::ArrowUp] {
    pair.each(|engine| engine.tap_key(key));
    frames(&mut pair, &mut most, 5, "moving through the menu");
  }
  frames(&mut pair, &mut most, 30, "the menu still");
  eprintln!(
    "menu: {} compared, at most {most} UI draws",
    pair.compared()
  );
  assert!((1..=MAX_DRAWS).contains(&most), "{most} UI draws");
}

#[test]
fn the_pause_menu_and_the_hud_look_the_same_on_the_ui_renderer() {
  let mut most = 0;
  let Some(mut pair) = pair(|| GameScene::new(0, None)) else {
    return;
  };
  pair.each(|engine| engine.tap_key(KeyCode::Space));
  frames(&mut pair, &mut most, 30, "the ball launched");
  for _ in 0..3 {
    pair.each(|engine| engine.press_key(KeyCode::KeyA));
    frames(&mut pair, &mut most, 40, "the paddle left");
    pair.each(|engine| engine.release_key(KeyCode::KeyA));
    pair.each(|engine| engine.press_key(KeyCode::KeyD));
    frames(&mut pair, &mut most, 60, "the paddle right");
    pair.each(|engine| engine.release_key(KeyCode::KeyD));
  }
  pair.each(|engine| engine.tap_key(KeyCode::Escape));
  frames(&mut pair, &mut most, 20, "paused");
  pair.each(|engine| engine.tap_key(KeyCode::ArrowDown));
  frames(&mut pair, &mut most, 10, "paused, moving");
  pair.each(|engine| engine.tap_key(KeyCode::Escape));
  frames(&mut pair, &mut most, 30, "playing again");
  eprintln!(
    "play: {} compared, at most {most} UI draws",
    pair.compared()
  );
  assert!((1..=MAX_DRAWS).contains(&most), "{most} UI draws");
}
