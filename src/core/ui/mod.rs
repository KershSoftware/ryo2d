//! Hybrid UI System
//!
//! A two-layer UI system combining immediate-mode and retained-mode approaches:
//!
//! - **Immediate mode** (`immediate/`): Stateless, per-frame drawing for HUDs and debug overlays
//! - **Retained mode** (`retained/`): Stateful widgets with layout containers for complex UIs
//! - **Theme system** (`theme.rs`): Centralized styling with hot-reload support
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │              Game Code (Scene::ui callback)             │
//! ├─────────────────────────────────────────────────────────┤
//! │   Retained Mode Layer                                   │
//! │   ├─ Layout Containers (HBox, VBox, Stack)              │
//! │   ├─ Stateful Widgets (TextInput, Dropdown, ScrollView) │
//! │   └─ Automatic state management                         │
//! ├─────────────────────────────────────────────────────────┤
//! │   Immediate Mode Layer                                  │
//! │   ├─ UiContext drawing API                              │
//! │   ├─ Stateless widgets (Button, Label, ProgressBar)     │
//! │   └─ Draw commands, drawn by the GPU                    │
//! ├─────────────────────────────────────────────────────────┤
//! │   Theme System                                          │
//! │   ├─ Widget styles (colors, spacing, borders)           │
//! │   └─ Hot-reload from JSON files                         │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! # When to Use Each Mode
//!
//! ## Immediate Mode
//! - Simple HUDs (health bars, score displays)
//! - Debug overlays
//! - Dynamic UI that changes frequently
//! - Prototyping
//!
//! ## Retained Mode
//! - Complex forms with text input
//! - Menus with many options
//! - Inventory screens
//! - Settings screens
//!
//! # Example
//!
//! ```ignore
//! fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
//!     // Immediate mode: draw directly
//!     ui.draw_progress_bar(
//!         Rect::new(10.0, 10.0, 200.0, 20.0),
//!         self.health / 100.0,
//!         Color::HEALTH_RED,
//!         Color::DARK_GRAY,
//!     );
//!
//!     // Button returns true when clicked
//!     if ui.button("Pause", Rect::new(700.0, 10.0, 80.0, 30.0)) {
//!         return SceneAction::Push(Box::new(PauseMenu));
//!     }
//!     SceneAction::Continue
//! }
//! ```

// Submodules
pub(crate) mod id;
pub mod immediate;
pub(crate) mod layout;
pub(crate) mod memory;
pub mod retained;
#[cfg(test)]
pub(crate) mod test_util;
pub mod theme;
pub mod tween;
mod types;

// Re-export core types at module root for convenience
pub use id::UiId;
pub use layout::{Align, Edges, Justify, Length};
pub use types::{Anchor, Color, Rect};

// Re-export immediate mode API (stateless per-frame drawing)
pub use immediate::{HAlign, HBox, Stack, UiConsumed, UiContext, UiInput, UiText, VAlign, VBox};

// Re-export retained mode API (stateful widgets)
pub use retained::{DropdownState, FocusTrap, Focusable, ScrollViewState, TextInputState};

// Re-export theme types
pub use theme::{
  ButtonStyle, CheckboxStyle, HotReloadTheme, LabelStyle, PanelStyle, ProgressBarStyle,
  ScrollbarStyle, SliderStyle, TextInputStyle, Theme, ThemeError, ToggleStyle, TooltipStyle,
};

// Re-export tween types
pub use tween::{Ease, Lerp, TweenColor, TweenFloat, TweenRect, TweenVec2, UiTween, UiTweenLoop};
