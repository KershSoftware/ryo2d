//! The theme page: its File theme follows the demo's theme file

use demo_kit::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// The Widgets & Themes page
const THEMES: usize = 4;

#[test]
fn the_file_theme_follows_the_demo_s_theme_file() {
  let builder = ui_demo::demo().start_page(THEMES).engine_builder();
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(4).unwrap();
  // T cycles Dark, Light, Pixel, Neon, File
  for _ in 0..4 {
    engine.tap_key(KeyCode::KeyT);
    engine.step_frames(2).unwrap();
  }
  let theme = engine.world().resource::<UiTheme>();
  assert_eq!(theme.get().name, "file", "the file's theme is in use");
  let themes = engine.world().resource::<Assets<Theme>>();
  assert_eq!(themes.len(), 1, "held while it is followed");
}
