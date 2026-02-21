//! UI Demo - Comprehensive demonstration of Ryo2D's UI system
//!
//! This example showcases all UI features available in the engine,
//! organized into themed demo scenes that you can navigate between.

mod scenes;

use ryo2d::prelude::*;
use scenes::MainMenu;

fn main() {
  let engine = EngineBuilder::new()
    .title("Ryo2D UI Demo")
    .resolution(1024, 768)
    .window_mode(WindowMode::ResizableAspectRatio)
    .start_scene(MainMenu::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
