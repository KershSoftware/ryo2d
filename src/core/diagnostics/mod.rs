//! Debug visualization and rendering utilities
//!
//! This module provides engine-level debug visualization for development.
//! All debug features can be configured via `Ryo.toml` or the `EngineBuilder`.
//!
//! # Available Debug Options
//!
//! | Option | Description |
//! |--------|-------------|
//! | `show_colliders` | Draw the simulation's colliders, joints and contacts, coloured by body kind |
//! | `show_velocities` | Draw velocity vectors on physics entities |
//! | `show_origins` | Draw entity origin points (transform position) |
//! | `enable_profiler` | Open flamegraph profiler window |
//! | `ui_cache` | Replay UI node trees' unchanged subtrees (on by default; `EngineBuilder::ui_cache`) |
//! | `ui_fragments` | Keep the layout and resolved output of UI node trees' unchanged subtrees between frames (on by default; `EngineBuilder::ui_fragments`) |
//! | `ui_retain` | Keep the UI image while the UI is unchanged; reuse unchanged texts' glyphs (on by default; `EngineBuilder::ui_retain`) |
//!
//! # Available Info Overlays
//!
//! | Option | Description |
//! |--------|-------------|
//! | `show_fps` | Display FPS counter overlay |
//! | `show_frame_timing` | Display frame timing breakdown |
//! | `show_stats` | Display entity/sprite/batch statistics |
//!
//! # Runtime Debug Key Toggles
//!
//! The keys below are developer tools, in a build with them (debug builds,
//! or the `dev-tools` feature); a build without them leaves every key to the
//! game but F5, and keeps the FPS counter as the player's setting.
//!
//! **Game Debug Keys (F1-F5)** - On unless `disable_debug_keys = true`:
//!
//! | Key | Action |
//! |-----|--------|
//! | F1 | Cycle overlay mode (Off → FPS → Frame Timing → Full Stats → Off) |
//! | F2 | Toggle collision boundaries |
//! | F3 | Toggle velocity vectors |
//! | F4 | Toggle origin points |
//! | F5 | Toggle borderless fullscreen |
//!
//! **Dev Keys (F9, F10, F12)** - On unless `dev_window_keys = false`:
//!
//! | Key | Action |
//! |-----|--------|
//! | F9 | Toggle logger window |
//! | F10 | Toggle profiler window (requires `profiler` feature) |
//! | F12 | Capture the next frame to `captures/` (or `RYO_CAPTURE_DIR`) |
//!
//! Set `disable_debug_keys = true` in `[debug]` to disable the F1-F5 toggles,
//! and `dev_window_keys = false` to disable F9, F10 and F12.
//!
//! # Profiling Workflow
//!
//! Frame timing is always captured with negligible overhead. Use the F1 key or
//! `show_frame_timing = true` to display timing breakdown:
//!
//! | Metric | High Value Indicates |
//! |--------|---------------------|
//! | Update | Heavy game logic, too many entities, expensive AI |
//! | Physics | Too many colliders, complex shapes, small timestep |
//! | Render | Too many draw calls, shader complexity, large textures |
//! | Present | GPU-bound, vsync waiting, driver overhead |
//!
//! **Debug build features:**
//! - Scene `on_enter()` timing warnings to console (>5ms)
//! - Slow frame warnings to console (>33ms)
//!
//! # Ryo.toml Configuration
//!
//! ```toml
//! [debug]
//! show_colliders = true
//! show_velocities = false
//! show_origins = false
//! disable_debug_keys = false
//! enable_profiler = true   # Open flamegraph profiler window
//!
//! [system]
//! show_fps = true
//! ```
//!
//! # Programmatic Configuration
//!
//! ```rust,ignore
//! EngineBuilder::new()
//!     .show_colliders(true)
//!     .show_fps(true)
//!     .build()
//! ```
//!
//! Debug settings from `Ryo.toml` are automatically loaded during `build()`.
//! Values set programmatically will override config file settings.

mod flags;
mod overlays;
mod timing;

pub use flags::DebugConfig;
pub use overlays::{DebugStats, OverlayMode, SmoothedDebugStats, colors};
pub use timing::{FrameCost, FrameTimings, SmoothedFrameTimings, time_block};

pub(crate) use overlays::{
  OverlayFlags, overlay_label, overlay_shapes, overlay_stack_top, render_fps_counter,
  render_frame_timing, render_stats, row_pitch,
};
