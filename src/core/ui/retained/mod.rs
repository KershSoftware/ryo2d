//! Retained-mode UI widgets
//!
//! This module provides stateful widgets for complex UIs. Unlike immediate-mode
//! widgets that are purely per-frame, retained widgets maintain state across frames:
//!
//! - **TextInputState** - Text buffer, cursor, selection, blink state
//! - **DropdownState** - Selected index, open/closed, focus, hover index  
//!
//! # When to Use Retained Mode
//!
//! - Text input fields (cursor, selection, editing state)
//! - Dropdowns/select boxes (open/closed, keyboard navigation)
//! - Any widget that needs state across frames
//!
//! # Usage Pattern
//!
//! 1. Store the state struct in your scene/game struct
//! 2. Pass it mutably to UiContext each frame
//! 3. The widget renders and updates based on input
//!
//! ```ignore
//! struct MyScene {
//!     username: TextInputState,
//!     difficulty: DropdownState,
//! }
//!
//! fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
//!     let dt = ui.dt();
//!     ui.text_input(&mut self.username, rect, "Enter name...", dt);
//!     ui.dropdown(&mut self.difficulty, rect, &["Easy", "Medium", "Hard"]);
//!     SceneAction::Continue
//! }
//! ```

mod dropdown;
mod focus;
mod text_input;

pub use dropdown::DropdownState;
pub use focus::{FocusTrap, Focusable};
pub use text_input::TextInputState;
