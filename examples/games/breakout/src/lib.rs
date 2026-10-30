//! Breakout - A Ryo2D Example Game
//!
//! This crate demonstrates how to structure a Ryo2D game project:
//!
//! - `components/` - ECS components specific to this game
//! - `scenes/` - Game scenes (menu, gameplay, pause, etc.)
//! - `levels/` - Level loading (Tiled maps through the asset registry)
//! - `save/` - Save data management
//! - `constants.rs` - Game configuration values
//! - `look.rs` - The game's font and panel spacing
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
//! ├── assets/              # Audio, fonts, UI files and the Tiled levels
//! └── data/                # Save files (git-ignored)
//! ```

#![allow(clippy::new_without_default)]

pub mod components;
pub mod constants;
mod embedded;
pub mod levels;
pub mod look;
pub mod save;
pub mod scenes;

pub use embedded::{EMBEDDED, embedded};

/// Prelude - commonly used items
pub mod prelude {
  pub use super::components::*;
  pub use super::constants::*;
  pub use super::levels::{BRICK_CLASS, LEVEL_ORIGIN, level_path, read_brick};
  pub use super::look::{TEXT_SIZE, text_style, use_game_look};
  pub use super::save::{HighScoreData, load_high_score, save_high_score};
  pub use super::scenes::*;
}
