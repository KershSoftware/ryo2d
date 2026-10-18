//! Game UI, built each frame in `Scene::ui`
//!
//! [`UiContext`] is what a scene's `ui` hook draws with:
//!
//! - **Widgets are builders** (`ui.button("Play").clicked()`): each answers,
//!   through a [`Response`], from what the player did to what was drawn last
//!   frame, resolved once a frame before any scene's `ui`. The game's values
//!   stay with the game; what a widget remembers of itself (an open list, a
//!   caret, a scroll offset, the focus) lives in the UI's memory under its
//!   [`UiId`].
//! - **Containers** (`column`, `row`, `grid`, `stack`, `panel`, `scroll`)
//!   lay their children out with flexbox and grid; popups, modals and
//!   `confirm` draw above the page.
//! - **Focus scopes** (`.focus_scope()` containers, open lists, popups and
//!   modals) carry keyboard and gamepad navigation from the `ui.*` actions,
//!   so a HUD outside any scope never takes the arrows from gameplay.
//! - **The theme** ([`UiTheme`], a world resource) gives each widget kind its
//!   looks per state, with variants, skins and sounds, from code or RON
//!   files reloaded in debug builds.
//! - **Absolute drawing** (`draw_*`, `ui.text(..).at(..)`, `immediate/`)
//!   remains for HUDs, overlays and custom shapes.
//!
//! The `ryo-ui` skill is the guide; `ryo2d::testing::UiHarness` runs UI
//! frames without a GPU.
//!
//! # Example
//!
//! ```ignore
//! fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
//!     let mut action = SceneAction::Continue;
//!     ui.panel().focus_scope().anchor(Anchor::Center).gap(8.0).show(|ui| {
//!         if ui.button("Resume").default_focus().clicked() {
//!             action = SceneAction::Pop;
//!         }
//!         ui.slider(&mut self.volume, 0.0..=1.0).label("Volume").show_value();
//!         if ui.button("Quit").variant("danger").clicked() {
//!             action = SceneAction::Quit;
//!         }
//!     });
//!     // Back (Escape, or B on a pad) closes the menu
//!     if ui.back() {
//!         action = SceneAction::Pop;
//!     }
//!     action
//! }
//! ```

// Submodules
mod containers;
pub(crate) mod debug;
pub(crate) mod file;
pub(crate) mod id;
pub mod immediate;
pub(crate) mod interact;
pub(crate) mod layout;
pub(crate) mod memory;
pub(crate) mod nav;
pub mod nodes;
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

// The context, its absolute drawing and its input snapshot
pub use immediate::{UiConsumed, UiContext, UiInput, UiText};

// Retained nodes
pub use nodes::{
  TextRole, UiChecked, UiDisabled, UiEvent, UiEventKind, UiHidden, UiImage, UiIndex, UiInteraction,
  UiLabel, UiName, UiNode, UiNodeKind, UiOptions, UiRoot, UiSlot, UiTextValue, UiTooltip, UiValue,
};

// Re-export theme types
pub use theme::{
  FocusRing, Look, LookPatch, Looks, Metrics, Palette, Sizes, Skin, TextLook, TextRoles, Theme,
  ThemeError, ThemeFont, UiSounds, UiTheme, Variant, WidgetKind, WidgetLooks, WidgetState,
};

// Re-export tween types
pub use tween::{Ease, Lerp, TweenColor, TweenFloat, TweenRect, TweenVec2, UiTween, UiTweenLoop};
