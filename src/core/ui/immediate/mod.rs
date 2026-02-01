//! Immediate-mode UI system
//!
//! This module provides stateless, per-frame UI drawing. Every frame you
//! call draw functions to render UI elements - no widget state is persisted.
//!
//! # When to Use Immediate Mode
//!
//! - Simple HUDs (health bars, score displays, ammo counters)
//! - Debug overlays and developer tools
//! - Dynamic UI that changes frequently
//! - Prototyping new UI elements
//!
//! # Example
//!
//! ```ignore
//! fn render_ui(&mut self, ui: &mut UiContext) {
//!     // Draw a health bar
//!     ui.draw_progress_bar(
//!         Rect::new(10.0, 10.0, 200.0, 20.0),
//!         self.health / 100.0,
//!         Color::HEALTH_RED,
//!         Color::DARK_GRAY,
//!     );
//!
//!     // Draw score text
//!     ui.draw_text(&format!("Score: {}", self.score), 10.0, 40.0, Color::WHITE);
//!
//!     // Button returns true when clicked (immediate - no state persisted)
//!     if ui.button("Pause", Rect::new(700.0, 10.0, 80.0, 30.0)) {
//!         self.paused = true;
//!     }
//!
//!     // Layout containers for positioning (stateless helpers)
//!     let mut row = HBox::new(10.0, 100.0, 8.0);
//!     ui.button("A", row.next(60.0, 30.0));
//!     ui.button("B", row.next(60.0, 30.0));
//!     ui.button("C", row.next(60.0, 30.0));
//! }
//! ```

mod context;
mod font;
mod layout;

pub use context::UiContext;
pub use font::{BitmapFont, create_debug_font};
pub use layout::{HAlign, HBox, Stack, VAlign, VBox};
