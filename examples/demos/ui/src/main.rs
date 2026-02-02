//! UI Demo - Comprehensive demonstration of Ryo2D's UI system
//!
//! This example showcases all UI features available in the engine,
//! organized into themed demo scenes that you can navigate between.

mod scenes;

use ryo2d::prelude::*;
use scenes::MainMenu;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  EngineBuilder::new()
    .title("Ryo2D UI Demo")
    .resolution(800, 600)
    .window_mode(WindowMode::ResizableAspectRatio)
    .with_scene(MainMenu::new())
    .build()
    .run()?;

  Ok(())
}
