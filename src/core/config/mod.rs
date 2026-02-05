//! Configuration and persistence systems.
//!
//! This module provides:
//! - [`Workspace`] - Project discovery and Ryo.toml parsing
//! - [`SaveManager`] - 3-tier save system with compression
//! - Debug visualization and frame timing utilities

pub mod debug;
pub mod save;
pub mod workspace;

// Workspace types
pub use workspace::{
  CONFIG_FILENAME, DebugConfig as WorkspaceDebugConfig, ReleaseConfig, SystemDefaults, Workspace,
  WorkspaceConfig,
};

// Save system types
pub use save::{
  DisplayMode, DisplaySettings, EntitySnapshot, GameSaveData, Resolution, SaveManager, SystemField,
  SystemFieldFilter, SystemState, WorldState, compress_data, decompress_data,
};

// Debug utilities (re-export with prefix to avoid conflict with workspace::DebugConfig)
pub use debug::{
  DebugConfig, DebugStats, FrameTimings, OverlayMode, SmoothedDebugStats, SmoothedFrameTimings,
  add_gpu_rect, add_gpu_text, draw_circle_outline, draw_horizontal_line, draw_line,
  draw_rect_outline, draw_vertical_line, render_colliders, render_fps_counter,
  render_fps_counter_gpu, render_frame_timing, render_frame_timing_gpu, render_origins,
  render_stats, render_stats_gpu, render_velocities, set_pixel, time_block, upload_debug_font,
};
