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
//! │              Game Code (render_ui callback)             │
//! ├─────────────────────────────────────────────────────────┤
//! │   Retained Mode Layer                                   │
//! │   ├─ Layout Containers (HBox, VBox, Stack)              │
//! │   ├─ Stateful Widgets (TextInput, Dropdown, ScrollView) │
//! │   └─ Automatic state management                         │
//! ├─────────────────────────────────────────────────────────┤
//! │   Immediate Mode Layer                                  │
//! │   ├─ UiContext drawing API                              │
//! │   ├─ Stateless widgets (Button, Label, ProgressBar)     │
//! │   └─ Direct frame buffer rendering                      │
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
//! ## Retained Mode (coming soon)
//! - Complex forms with text input
//! - Menus with many options
//! - Inventory screens
//! - Settings screens
//!
//! # Example
//!
//! ```ignore
//! fn render_ui(&mut self, ui: &mut UiContext) {
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
//!         self.paused = true;
//!     }
//! }
//! ```

// Submodules
pub mod immediate;
pub mod retained;
pub mod theme;
mod types;

// Re-export core types at module root for convenience
pub use types::{Anchor, Color, Rect};

// Re-export immediate mode API (most commonly used)
pub use immediate::{BitmapFont, HAlign, HBox, Stack, UiContext, VAlign, VBox, create_debug_font};

// Re-export theme types
pub use theme::{
    ButtonStyle, CheckboxStyle, HotReloadTheme, LabelStyle, PanelStyle, ProgressBarStyle,
    ScrollbarStyle, SliderStyle, TextInputStyle, Theme, ThemeError, ToggleStyle, TooltipStyle,
};
