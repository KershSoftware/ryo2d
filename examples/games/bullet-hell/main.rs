// Windows: Hide the console window in release builds.
// In debug builds, the console remains visible for println! output.
// Remove this line if you need console output in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
  let engine = EngineBuilder::new()
    .resolution(1920, 1080)
    .target_fps(144)
    .physics_hz(144)
    .with_scene(MenuScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
