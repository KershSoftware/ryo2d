//! Hybrid UI System
//!
//! A two-layer UI system combining immediate-mode and retained-mode approaches:
//!
//! - **Immediate mode** (`immediate/`): Stateless, per-frame drawing for HUDs and debug overlays
//! - **Retained mode** (`retained/`): Stateful widgets with layout containers for complex UIs
//! - **Theme system** (`theme/`): Looks per widget kind and state, variants and skins, from RON files with hot reload
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │              Game Code (Scene::ui callback)             │
//! ├─────────────────────────────────────────────────────────┤
//! │   Retained Mode Layer                                   │
//! │   ├─ Layout Containers (HBox, VBox, Stack)              │
//! │   ├─ Stateful Widgets (TextInput, Dropdown) │
//! │   └─ Automatic state management                         │
//! ├─────────────────────────────────────────────────────────┤
//! │   Immediate Mode Layer                                  │
//! │   ├─ UiContext drawing API                              │
//! │   ├─ Stateless widgets (Button, Label, ProgressBar)     │
//! │   └─ Draw commands, drawn by the GPU                    │
//! ├─────────────────────────────────────────────────────────┤
//! │   Theme System                                          │
//! │   ├─ Widget styles (colors, spacing, borders)           │
//! │   └─ Hot-reload from RON files                          │
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
//!     if ui.button("Pause").at(Rect::new(700.0, 10.0, 80.0, 30.0)).clicked() {
//!         return SceneAction::Push(Box::new(PauseMenu));
//!     }
//!     SceneAction::Continue
//! }
//! ```

// Submodules
mod containers;
pub(crate) mod id;
pub mod immediate;
pub(crate) mod interact;
pub(crate) mod layout;
pub(crate) mod memory;
pub(crate) mod nav;
mod response;
pub mod retained;
#[cfg(test)]
pub(crate) mod test_util;
pub mod theme;
pub(crate) mod tree;
pub mod tween;
mod types;
mod widgets;

// Re-export core types at module root for convenience
pub use containers::Container;
pub use id::UiId;
pub use layout::{Align, Edges, Justify, Length};
pub use response::{Response, Sense};
pub use types::{Anchor, Color, Rect};
pub use widgets::{
  Button, Checkbox, Custom, Image, Painter, Progress, Radio, Scroll, Slider, Toggle,
};

// Re-export immediate mode API (stateless per-frame drawing)
pub use immediate::{HAlign, HBox, Stack, UiConsumed, UiContext, UiInput, UiText, VAlign, VBox};

// Re-export retained mode API (stateful widgets)
pub use retained::{DropdownState, FocusTrap, Focusable, TextInputState};

// Re-export theme types
pub use theme::{
  FocusRing, Look, LookPatch, Looks, Metrics, Palette, Sizes, Skin, TextLook, TextRoles, Theme,
  ThemeError, ThemeFont, UiSounds, UiTheme, Variant, WidgetKind, WidgetLooks, WidgetState,
};

// Re-export tween types
pub use tween::{Ease, Lerp, TweenColor, TweenFloat, TweenRect, TweenVec2, UiTween, UiTweenLoop};
