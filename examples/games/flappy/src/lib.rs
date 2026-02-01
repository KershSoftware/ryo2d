//! Flappy - Ryo2D Example Game
//!
//! This example demonstrates:
//! - Image asset loading and rendering
//! - Camera/parallax scrolling effects
//! - Physics simulation (gravity, collision)
//! - Procedural content generation (pipes)
//! - Simple state machine (menu, playing, game over)

pub mod components;
pub mod constants;
pub mod save;
pub mod scenes;

pub mod prelude {
  pub use crate::components::*;
  pub use crate::constants::*;
  pub use crate::save::*;
  pub use crate::scenes::*;
}
