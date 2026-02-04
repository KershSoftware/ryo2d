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
  // Game uses fixed 1920x1080 design resolution from Ryo.toml
  // Window can be resized freely - engine handles scaling with letterboxing
  // Load display mode (windowed/fullscreen) from user settings while keeping fixed resolution
  let game = EngineBuilder::new()
    .window_mode(WindowMode::ResizableAspectRatio)
    .fps(144)
    .physics_hz(144)
    .ui_scale(None) // Auto-detect from DPI for HiDPI display support
    .with_saved_display_mode() // Load display mode from system.json (not resolution)
    .with_scene(MenuScene::new())
    .build();

  let _ = game.run();
}
