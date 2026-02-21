//! UI Animations Demo - Comprehensive demonstration of Ryo2D's UI tweening system
//!
//! This example showcases the UiTween system for animating UI properties,
//! organized into themed demo scenes that you can navigate between.

mod scenes;

use ryo2d::prelude::*;
use scenes::MainMenu;

fn main() {
  let engine = EngineBuilder::new()
    .title("Ryo2D UI Animations Demo")
    .resolution(800, 600)
    .window_mode(WindowMode::ResizableAspectRatio)
    .start_scene(MainMenu::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
