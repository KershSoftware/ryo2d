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
//! fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
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
//!     if ui.button("Pause").at(Rect::new(700.0, 10.0, 80.0, 30.0)).clicked() {
//!         return SceneAction::Push(Box::new(PauseMenu));
//!     }
//!
//!     // Layout containers for positioning (stateless helpers)
//!     let mut row = HBox::new(10.0, 100.0, 8.0);
//!     ui.button("A").at(row.next(60.0, 30.0));
//!     ui.button("B").at(row.next(60.0, 30.0));
//!     ui.button("C").at(row.next(60.0, 30.0));
//!     SceneAction::Continue
//! }
//! ```

mod commands;
mod context;
mod input;
mod layout;

pub(crate) use commands::{UiCommand, UiCommands};
pub(crate) use context::{OVERLAY_LAYER, PAGE_LAYER, POPUP_LAYER, text_seed};
pub use context::{UiContext, UiText};
pub use input::{UiConsumed, UiInput};
pub use layout::{HAlign, HBox, Stack, VAlign, VBox};
