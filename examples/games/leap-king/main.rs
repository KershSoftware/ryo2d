// Windows: Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Leap King - Multiplayer Platformer
//!
//! A game where players can't walk - they leap in arc trajectories
//! via charge-and-release mechanics.
//!
//! ## Controls
//! - Space or controller X / A: Charge leap (hold to charge, release to jump)
//! - Arrow keys or WASD or controller joysticks: Aim while charging
//! - ESC or controller Start: Menu/Quit

use leap_king::prelude::*;
use ryo2d::prelude::*;

fn main() {
  let engine = EngineBuilder::new()
    .resolution(800, 600)
    .fps(144)
    .with_scene(MenuScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
    std::process::exit(1);
  }
}
