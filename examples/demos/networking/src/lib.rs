//! Netplay - A Multiplayer Platformer Demo
//!
//! Demonstrates Ryo2D's networking features:
//! - ListenServer for host-based multiplayer
//! - NetworkClient for joining games
//! - Entity replication and state synchronization
//! - Simple platformer physics
//!
//! Runs as a single page on the demo-kit shell.

#![allow(clippy::new_without_default)]

pub mod components;
pub mod constants;
pub mod scenes;

pub mod prelude {
  pub use super::components::*;
  pub use super::constants::*;
  pub use super::scenes::*;
}
