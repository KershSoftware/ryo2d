// Windows: Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Nigh_t
//!
//! A dark atmospheric game built with the Ryo2D engine.

use ryo2d::prelude::*;

mod scenes;
use scenes::MainMenuScene;

fn main() {
  let engine = EngineBuilder::new()
    .title("Nigh_t")
    .resolution(1920, 1080)
    .display_mode(DisplayMode::BorderlessFullscreen)
    .fps(60)
    .with_scene(MainMenuScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
    std::process::exit(1);
  }
}
