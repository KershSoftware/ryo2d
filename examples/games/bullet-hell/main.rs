//! Bullet Hell - Ryo2D GPU Batching Demo
//!
//! This demo showcases:
//! - GPU sprite batching with hundreds of entities
//! - Particle explosion systems
//! - Dynamic bullet patterns
//! - Collision detection with platforms
//! - WASD/arrow key movement
//!
//! The goal is to survive as long as possible while dodging bullets
//! fired from turrets on the left and right walls.

use bullet_hell::scenes::MenuScene;
use ryo2d::prelude::*;

fn main() {
  // Resolution, title, and volumes come from Ryo.toml (embedded at build time or read at runtime)
  let builder = EngineBuilder::new()
    .window_mode(WindowMode::ResizableAspectRatio)
    .fps(144)
    .physics_hz(60);

  let game = builder.with_scene(MenuScene::new()).build();

  let _ = game.run();
}
