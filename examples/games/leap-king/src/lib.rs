//! Leap King - Multiplayer Platformer
//!
//! A game where players can't walk - they leap in arc trajectories
//! via charge-and-release mechanics. Features procedurally generated
//! infinite vertical platforms and multiplayer support.

#![allow(clippy::new_without_default)]

pub mod components;
pub mod constants;
pub mod generation;
pub mod look;
pub mod network;
pub mod scenes;

pub mod prelude {
  pub use super::components::*;
  pub use super::constants::*;
  pub use super::generation::*;
  pub use super::network::*;
  pub use super::scenes::*;
}
