//! Configuration and persistence systems.
//!
//! This module provides:
//! - [`Workspace`] - Project discovery and Ryo.toml parsing
//! - [`SaveManager`] - 3-tier save system with compression
//! - Debug visualization and frame timing utilities

pub mod debug;
pub(crate) mod resolve;
pub mod save;
pub(crate) mod save_location;
pub mod workspace;

// Workspace types
pub use workspace::{
  CONFIG_FILENAME, DebugConfig as WorkspaceDebugConfig, ReleaseConfig, SystemDefaults, Workspace,
  WorkspaceConfig,
};

// Save system types
pub use save::{
  DisplayMode, GameSaveData, Resolution, SaveManager, SystemState, UserPreference,
  UserPreferenceFilter, compress_data, decompress_data, parse_resolution,
};

// Debug utilities (re-export with prefix to avoid conflict with workspace::DebugConfig)
pub use debug::{
  DebugConfig, DebugStats, FrameTimings, OverlayMode, SmoothedDebugStats, SmoothedFrameTimings,
  render_fps_counter, render_frame_timing, render_stats, time_block,
};
