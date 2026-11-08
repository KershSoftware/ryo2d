//! Configuration and persistence systems.
//!
//! This module provides:
//! - [`Workspace`] - Project discovery and Ryo.toml parsing
//! - [`SaveManager`] - saves: system state (`system.json` user settings) and
//!   compressed game data

pub(crate) mod project;
mod project_file;
pub(crate) mod resolve;
pub mod save;
pub(crate) mod save_location;
pub mod workspace;

pub use project_file::ProjectFile;

// Workspace types
pub use workspace::{CONFIG_FILENAME, ReleaseConfig, SystemDefaults, Workspace, WorkspaceConfig};

// Save system types
pub use save::{
  DisplayMode, GameSaveData, Resolution, SaveManager, SystemState, UserPreference,
  UserPreferenceFilter, compress_data, decompress_data, parse_resolution,
};
