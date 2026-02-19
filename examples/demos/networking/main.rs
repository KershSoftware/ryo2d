//! Netplay - Multiplayer Platformer Demo
//!
//! A simple 2-player networked game demonstrating Ryo2D's networking features.
//!
//! ## Features
//! - Listen server (host) and client modes
//! - 5-character room codes for easy joining
//! - Player movement, jumping, and collision
//! - Real-time position synchronization
//!
//! ## Controls
//! - A/D or Arrow keys: Move left/right
//! - Space or W: Jump

use networking::prelude::*;
use ryo2d::prelude::*;

fn main() {
  let engine = EngineBuilder::new()
    .title("Netplay - Multiplayer Demo")
    .resolution(800, 600)
    .fps(60)
    .physics_hz(60)
    .with_scene(MenuScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
