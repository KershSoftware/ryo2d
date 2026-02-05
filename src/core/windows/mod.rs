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
//! - **Logger** (`logger_window`) - Live log viewer with filtering by level/context
//!
//! # Enabling Windows
//!
//! Configure in `Ryo.toml`:
//!
//! ```toml
//! [debug]
//! enable_profiler = true  # Enable profiler window (requires --features profiler)
//! logger_window = true    # Enable logger window
//! ```
//!
//! Or via CLI:
//!
//! ```bash
//! ryo2d dev --profiler    # Run with profiler
//! ryo2d dev --logger      # Run with logger (planned)
//! ```
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

// Generic dev window framework
pub mod dev_window;

// Profiler window (requires profiler feature for puffin integration)
#[cfg(feature = "profiler")]
pub mod profiler_window;

// Logger window
pub mod logger_window;

// Re-export common types for convenience
pub use dev_window::{CHAR_HEIGHT, CHAR_WIDTH, DevWindowConfig, DevWindowRenderer, colors};
