//! Breakout - A Ryo2D Example Game
//!
//! This crate demonstrates how to structure a Ryo2D game project:
//!
//! - `components/` - ECS components specific to this game
//! - `scenes/` - Game scenes (menu, gameplay, pause, etc.)
//! - `levels/` - Level loading utilities
//! - `save/` - Save data management
//! - `constants.rs` - Game configuration values
//!
//! ## Project Structure
//!
//! ```text
//! breakout/
//! ├── main.rs              # Entry point
//! ├── src/
//! │   ├── lib.rs           # This file - game crate root
//! │   ├── constants.rs     # Game constants
//! │   ├── components/      # ECS components
//! │   ├── scenes/          # Game scenes
//! │   ├── levels/          # Level loading
//! │   └── save/            # Save data
//! ├── assets/              # Audio, sprites, fonts
//! ├── levels/              # Level definition files
//! └── data/                # Save files (git-ignored)
//! ```

#![allow(clippy::new_without_default)]

pub mod components;
pub mod constants;
pub mod levels;
pub mod save;
pub mod scenes;

/// Prelude - commonly used items
pub mod prelude {
  pub use super::components::*;
  pub use super::constants::*;
  pub use super::levels::{LevelData, level_path, load_level};
  pub use super::save::{HighScoreData, load_high_score, save_high_score};
  pub use super::scenes::*;
}
