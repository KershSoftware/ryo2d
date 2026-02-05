//! Shadow Demo
//!
//! Demonstrates various shadow techniques in Ryo2D.
//!
//! ## Scenes
//!
//! - **[1] Hard Shadows**: Real-time shadow volumes from light occluders
//! - **[2] Drop Shadows**: Artist-controlled offset shadows for sprites
//! - **[3] Blob Shadows**: Simple elliptical shadows beneath entities
//! - **[4] Soft Shadows**: Shadows with gradual edge fade
//! - **[5] Colored Shadows**: Tinted shadows for atmospheric effects
//! - **[6] Contact Shadows**: Concentrated shadows at object contact points
//!
//! ## Controls
//!
//! Press **1-6** to switch between shadow type demos.
//! Press **ESC** to quit.
//!
//! See each scene for specific controls.

mod scenes;

use ryo2d::prelude::*;
use scenes::HardShadowsScene;

fn main() {
  let engine = EngineBuilder::new()
    .title("Shadow Demo - Ryo2D")
    .resolution(1280, 720)
    .fps(60)
    .with_scene(HardShadowsScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
    std::process::exit(1);
  }
}
