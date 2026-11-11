//! Development tool windows for the Ryo2D engine.
//!
//! This module provides a generic framework for creating standalone development
//! tool windows that can communicate with the main game engine without affecting
//! its performance.
//!
//! # Architecture
//!
//! All dev windows share GPU resources (device/queue) with the main renderer via
//! `Arc`, allowing them to render independently without duplicating GPU context.
//! Each window uses CPU-side pixel buffer rendering for simplicity and full control
//! over the UI.
//!
//! # Available Windows
//!
//! - **Profiler** (`profiler_window`) - Real-time flamegraph showing CPU/GPU timing
//! - **Logger** (`logger_window`) - The log as it is written, filtered by level and origin
//!
//! # Opening Windows
//!
//! F9 toggles the logger and F10 the profiler. In `Ryo.toml`:
//!
//! ```toml
//! [debug]
//! logger_window = true    # open the logger window at start
//! enable_profiler = true  # profiler scopes on for the run; F10 opens the window
//! ```
//!
//! The profiler needs the engine's `profiler` feature, which `ryo2d dev`
//! adds when asked (no game feature needed):
//!
//! ```bash
//! ryo2d dev --profiler    # build with the profiler and open its window
//! ```
//!
//! The windows are developer tools: a build without the tools has none.
//!
//! # Creating Custom Windows
//!
//! Use `DevWindowRenderer` to create new dev tool windows:
//!
//! ```rust,ignore
//! use ryo2d::core::windows::{DevWindowConfig, DevWindowRenderer, colors};
//!
//! let config = DevWindowConfig {
//!   title: "My Tool".to_string(),
//!   width: 600,
//!   height: 400,
//!   render_throttle_ms: 100,
//! };
//!
//! let renderer = DevWindowRenderer::new(
//!   event_loop, device, queue, instance, adapter, config
//! )?;
//!
//! // In render loop:
//! if renderer.should_render() {
//!   renderer.clear(colors::BACKGROUND);
//!   renderer.draw_text("Hello", 10, 10, colors::TEXT);
//!   renderer.present();
//!   renderer.mark_rendered();
//! }
//! ```

// The whole module is a developer tool, native only (it is declared under
// `cfg(all(dev_tools, not(target_arch = "wasm32")))` in `core/mod.rs`)

// Generic dev window framework
pub mod dev_window;

// Profiler window (requires profiler feature for puffin integration)
#[cfg(feature = "profiler")]
pub mod profiler_window;

// The logger window, and its reading of the log
pub(crate) mod log_feed;
pub mod logger_window;

// Re-export common types for convenience
pub use dev_window::{CHAR_HEIGHT, CHAR_WIDTH, DevWindowConfig, DevWindowRenderer, colors};
