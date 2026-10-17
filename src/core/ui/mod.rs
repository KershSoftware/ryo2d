//! Hybrid UI System
//!
//! A UI built each frame in `Scene::ui`, laid out and resolved by the engine:
//!
//! - **Drawing** (`immediate/`): the `UiContext`, absolute `draw_*` calls for HUDs and debug overlays
//! - **Widgets and containers** (`widgets/`, `containers.rs`, `popup.rs`): Laid out with flexbox and grid; widget state (open lists, carets, scroll offsets) lives in the UI's memory
//! - **Theme system** (`theme/`): Looks per widget kind and state, variants and skins, from RON files with hot reload
//!
//! # Overview
//!
//! - Widgets are builders that answer from the UI's memory: `ui.button("Play").clicked()`.
//! - Containers are laid out by Taffy (flexbox and grid).
//! - Focus lives in scopes, and keyboard and gamepad navigation move through them.
//! - Absolute `draw_*` calls remain for HUDs and custom drawing.
//! - The theme is a world resource.
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
pub(crate) mod popup;
mod response;
#[cfg(test)]
pub(crate) mod test_util;
pub mod theme;
pub(crate) mod tree;
pub mod tween;
mod types;
pub(crate) mod widgets;

// Re-export core types at module root for convenience
pub use containers::Container;
pub use id::UiId;
pub use layout::{Align, Edges, Justify, Length};
pub use response::{Response, Sense};
pub use types::{Anchor, Color, Rect};
pub use widgets::{
  Button, Checkbox, ChoiceList, Custom, Dropdown, Image, Modal, Painter, Progress, Prompt, Radio,
  Rebind, Scroll, Slider, Slot, Tabs, TextInput, Toggle,
};

// Re-export immediate mode API (stateless per-frame drawing)
pub use immediate::{UiConsumed, UiContext, UiInput, UiText};

// Re-export theme types
pub use theme::{
  FocusRing, Look, LookPatch, Looks, Metrics, Palette, Sizes, Skin, TextLook, TextRoles, Theme,
  ThemeError, ThemeFont, UiSounds, UiTheme, Variant, WidgetKind, WidgetLooks, WidgetState,
};

// Re-export tween types
pub use tween::{Ease, Lerp, TweenColor, TweenFloat, TweenRect, TweenVec2, UiTween, UiTweenLoop};
