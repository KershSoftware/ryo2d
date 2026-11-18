//! Configuration: project discovery and the player-preference types.
//!
//! This module provides:
//! - [`Workspace`] - Project discovery and Ryo.toml parsing
//! - [`UserPreference`] and [`UserPreferenceFilter`] - which of the player's saved
//!   settings a game applies
//!
//! The player's settings and the game's saves live in `core::settings` and `core::saves`.

pub(crate) mod preferences;
/// The project layer: what a Ryo.toml sets, and what `ryo2d build` embeds.
pub mod project;
mod project_file;
/// The layered settings: builder calls over the project's values over defaults.
pub mod resolve;
/// Where a game keeps its saves, and the folder name a release uses.
pub mod save_location;
pub mod workspace;

pub use project_file::ProjectFile;

pub use workspace::{CONFIG_FILENAME, ReleaseConfig, Workspace, WorkspaceConfig};

pub use preferences::{
  DisplayMode, Resolution, UserPreference, UserPreferenceFilter, parse_resolution,
};
