// Windows: Hide the console window in release builds.
// In debug builds, the console remains visible for println! output.
// Remove this line if you need console output in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Breakout - Complete Ryo2D Example Game
//!
//! This example demonstrates all major engine features:
//! - Scene management (MenuScene, GameScene, PauseScene)
//! - Immediate-mode UI system for menus and HUD
//! - ECS architecture for game entities
//! - Physics with Rapier2D
//! - Collision events for game logic
//! - Save system for high score persistence
//! - Audio for sound effects
//! - Level loading from external files
//!
//! ## Project Structure
//!
//! This game is organized into modules following Ryo2D best practices:
//! - `components/` - ECS components (Paddle, Ball, Block, etc.)
//! - `scenes/` - Game scenes (Menu, Game, Pause, QuitConfirm)
//! - `levels/` - Level loading utilities
//! - `save/` - High score persistence
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
//! - Menu: Click "Start", Enter, or Space to begin
//! - Game: A/D or Arrow keys to move paddle
//! - Game: Any key or click to launch ball
//! - Pause: Escape to pause, Resume/Quit buttons

use breakout::prelude::*;
use ryo2d::prelude::*;

fn main() {
  // Load user preferences from system.json, but NOT resolution
  // (game uses fixed 1280x720 with hardcoded wall positions)
  let engine = EngineBuilder::new()
    .title("Breakout - Ryo2D Demo")
    .resolution(1280, 720) // Fixed resolution - game uses hardcoded wall positions
    .fps(144)
    .physics_hz(144)
    .with_scene(MenuScene::new())
    .with_user_preferences([
      UserPreference::MasterVolume,
      UserPreference::MusicVolume,
      UserPreference::SfxVolume,
    ])
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
