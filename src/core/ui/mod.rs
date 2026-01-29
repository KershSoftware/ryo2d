//! Basic UI System
//!
//! Provides immediate-mode inspired UI for HUD elements, text rendering, and simple widgets.
//!
//! # Overview
//!
//! This module provides a minimal but functional UI system for game HUDs and debug overlays.
//! It uses an immediate-mode inspired design where you call draw functions each frame - no
//! persistent widget state is stored.
//!
//! # Features
//!
//! - **Text rendering** via bitmap fonts (fixed-width initially)
//! - **Shapes**: rectangles, bordered panels, progress bars
//! - **Positioning**: screen-space coordinates with optional anchoring
//! - **Input**: click detection for simple buttons/regions
//!
//! # Example
//!
//! ```ignore
//! fn render_ui(&mut self, ui: &mut UiContext) {
//!     // Draw a health bar
//!     ui.draw_progress_bar(
//!         Rect::new(10.0, 10.0, 200.0, 20.0),
//!         self.health / 100.0,
//!         Color::RED,
//!         Color::DARK_GRAY,
//!     );
//!
//!     // Draw score text
//!     ui.draw_text(&format!("Score: {}", self.score), 10.0, 40.0, Color::WHITE);
//!
//!     // Draw a button
//!     if ui.button("Pause", Rect::new(700.0, 10.0, 80.0, 30.0)) {
//!         self.paused = true;
//!     }
//! }
//! ```
//!
//! # Module Structure
//!
//! - [`types`] - Basic types: `Color`, `Rect`, `Anchor`
//! - [`font`] - Bitmap font rendering: `BitmapFont`, `create_debug_font`
//! - [`context`] - Immediate-mode drawing API: `UiContext`

mod context;
mod font;
mod types;

// Re-export all public types at the module root
pub use context::UiContext;
pub use font::{BitmapFont, create_debug_font};
pub use types::{Anchor, Color, Rect};
