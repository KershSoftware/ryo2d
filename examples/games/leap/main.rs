// Windows: Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Leap - Multiplayer Platformer
//!
//! A game where players can't walk - they leap in arc trajectories
//! via charge-and-release mechanics.
//!
//! ## Controls
//! - Space: Charge leap (hold to charge, release to jump)
//! - Arrow keys or WASD: Aim while charging
//! - ESC: Menu/Quit

use leap::prelude::*;
use ryo2d::prelude::*;

fn main() {
    let engine = EngineBuilder::new()
        .title("Leap - Multiplayer Platformer")
        .resolution(800, 600)
        .fps(60)
        .physics_hz(60)
        .with_scene(MenuScene::new())
        .build();

    if let Err(e) = engine.run() {
        eprintln!("Engine error: {}", e);
        std::process::exit(1);
    }
}
