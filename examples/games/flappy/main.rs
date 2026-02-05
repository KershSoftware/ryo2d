// Windows: Hide the console window in release builds.
// In debug builds, the console remains visible for println! output.
// Remove this line if you need console output in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Flappy - Ryo2D Example Game
//!
//! This example demonstrates:
//! - Image asset loading (sprites for bird, pipes, ground, background)
//! - Camera/parallax scrolling effects
//! - Physics simulation (gravity, collision detection)
//! - Procedural content generation (random pipe placement)
//! - Scene management (Menu, Game, GameOver)
//! - Save system for high score persistence
//!
//! ## Project Structure
//!
//! - `components/` - ECS components (Bird, Pipe, Ground, Scrolling)
//! - `scenes/` - Game scenes (Menu, Game, GameOver)
//! - `save.rs` - High score persistence
//! - `constants.rs` - Game configuration values
//!
//! ## Running
//!
//! From the game directory:
//! ```bash
//! cargo run
//! ```
//!
//! ## Controls
//!
//! - Menu: Space or Enter to start
//! - Game: Space to flap

use flappy::prelude::*;
use ryo2d::prelude::*;

fn main() {
  // Check if debug_scene is enabled in Ryo.toml [env] section
  let start_debug = Workspace::discover()
    .ok()
    .and_then(|ws| ws.config().env_bool("debug_scene"))
    .unwrap_or(false);

  let builder = EngineBuilder::new()
    .title("Flappy - Ryo2D Demo")
    .resolution(400, 600) // Fixed resolution - game uses hardcoded positions
    .fps(144)
    .physics_hz(144);

  // Start with debug scene or menu based on env config
  let engine = if start_debug {
    builder.with_scene(DebugScene::new()).build()
  } else {
    builder.with_scene(MenuScene::new()).build()
  };

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
