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
  let engine = EngineBuilder::new()
    .title("Breakout - Ryo2D Demo")
    .resolution(1280, 720) // Fixed resolution - game uses hardcoded wall positions
    .fps(144)
    .physics_hz(120)
    .with_scene(MenuScene::new())
    .with_system_state_overrides() // Apply audio overrides from system.json
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
