//! Flappy Bird - Ryo2D Example Game
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
  let engine = EngineBuilder::new()
    .title("Flappy Bird - Ryo2D Demo")
    .resolution(SCREEN_WIDTH, SCREEN_HEIGHT)
    .fps(60)
    .physics_hz(120)
    .with_workspace_debug() // Load debug settings from Ryo.toml
    .with_scene(MenuScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
