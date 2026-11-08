//! Configuration: project discovery and the player-preference types.
//!
//! This module provides:
//! - [`Workspace`] - Project discovery and Ryo.toml parsing
//! - [`UserPreference`] and [`UserPreferenceFilter`] - which of the player's saved
//!   settings a game applies
//!
//! The player's settings and the game's saves live in `core::settings` and `core::saves`.

pub(crate) mod preferences;
pub(crate) mod project;
mod project_file;
pub(crate) mod resolve;
pub(crate) mod save_location;
pub mod workspace;

pub use project_file::ProjectFile;

pub use workspace::{CONFIG_FILENAME, ReleaseConfig, Workspace, WorkspaceConfig};

pub use preferences::{
  DisplayMode, Resolution, UserPreference, UserPreferenceFilter, parse_resolution,
};
