//! Retained-mode UI widgets
//!
//! Widgets whose state the caller keeps across frames:
//!
//! - **DropdownState** - Selected index, open/closed, focus, hover index
//!
//! # Usage Pattern
//!
//! 1. Store the state struct in your scene/game struct
//! 2. Pass it mutably to UiContext each frame
//! 3. The widget renders and updates based on input
//!
//! ```ignore
//! struct MyScene {
//!     difficulty: DropdownState,
//! }
//!
//! fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
//!     ui.dropdown(&mut self.difficulty, rect, &["Easy", "Medium", "Hard"]);
//!     SceneAction::Continue
//! }
//! ```

mod dropdown;

pub use dropdown::DropdownState;
