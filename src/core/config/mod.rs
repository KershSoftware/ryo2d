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
    InfoConfig, SystemDefaults, Workspace, WorkspaceConfig,
    DebugConfig as WorkspaceDebugConfig, CONFIG_FILENAME,
};

// Save system types
pub use save::{
    compress_data, decompress_data, DisplaySettings, EntitySnapshot, GameSaveData,
    SaveManager, SystemState, WorldState,
};

// Debug utilities (re-export with prefix to avoid conflict with workspace::DebugConfig)
pub use debug::{
    add_gpu_rect, add_gpu_text, draw_circle_outline, draw_horizontal_line, draw_line,
    draw_rect_outline, draw_vertical_line, render_colliders, render_fps_counter,
    render_fps_counter_gpu, render_frame_timing, render_frame_timing_gpu, render_origins,
    render_velocities, set_pixel, time_block, upload_debug_font, DebugConfig, FrameTimings,
    SmoothedFrameTimings,
};
