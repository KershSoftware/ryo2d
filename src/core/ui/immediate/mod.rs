//! The UI context and its absolute drawing
//!
//! [`UiContext`] is what `Scene::ui` draws with. Widgets and containers are
//! builders on it (`ui.button("Play").clicked()`), laid out by the UI's layout
//! and answering from the UI's memory; this module holds the context itself,
//! the absolute `draw_*` calls, the clip stack and the input snapshot.
//!
//! # When to draw absolutely
//!
//! - HUD bars, counters and markers at fixed places
//! - Debug overlays and developer tools
//! - Custom shapes a widget does not cover
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
//!     // Widgets are builders: this one is placed at a rect and says
//!     // whether it was clicked
//!     if ui.button("Pause").at(Rect::new(700.0, 10.0, 80.0, 30.0)).clicked() {
//!         return SceneAction::Push(Box::new(PauseMenu));
//!     }
//!
//!     SceneAction::Continue
//! }
//! ```

mod commands;
mod context;
mod input;

pub(crate) use commands::{UiCommand, UiCommands};
pub(crate) use context::{OVERLAY_LAYER, PAGE_LAYER, POPUP_LAYER, PendingRebind, PromptContext};
pub use context::{UiContext, UiText};
pub use input::{UiConsumed, UiInput};
