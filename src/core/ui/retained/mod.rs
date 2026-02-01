//! Retained-mode UI widgets
//!
//! This module provides stateful widgets for complex UIs. Unlike immediate-mode
//! widgets that are purely per-frame, retained widgets maintain state across frames:
//!
//! - **TextInputState** - Text buffer, cursor, selection, blink state
//! - **DropdownState** - Selected index, open/closed, focus, hover index  
//! - **ScrollViewState** - Scroll offset, velocity, drag state
//!
//! # When to Use Retained Mode
//!
//! - Text input fields (cursor, selection, editing state)
//! - Dropdowns/select boxes (open/closed, keyboard navigation)
//! - Scrollable areas (scroll position, momentum)
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
//!     settings_scroll: ScrollViewState,
//! }
//!
//! fn render_ui(&mut self, ui: &mut UiContext) {
//!     ui.text_input(&mut self.username, rect, "Enter name...", dt);
//!     ui.dropdown(&mut self.difficulty, rect, &["Easy", "Medium", "Hard"]);
//!     ui.scroll_view(&mut self.settings_scroll, rect, content_height, |ui, offset| {
//!         // Draw scrollable content
//!     });
//!     ui.finish_dropdowns(); // Render dropdown overlays on top
//! }
//! ```

mod dropdown;
mod focus;
mod scroll_view;
mod text_input;

pub use dropdown::DropdownState;
pub use focus::{Focusable, FocusTrap};
pub use scroll_view::ScrollViewState;
pub use text_input::TextInputState;
