//! Window presentation settings and the requests scenes make of the window.

use crate::core::ecs::Resource;

pub use crate::core::config::DisplayMode;

/// Pending window configuration change requested by the game.
///
/// Set through `ctx.window()` (`request_ui_scale`); the window's size and display
/// mode are the player's `Settings`. The engine applies a change after the current update.
#[derive(Resource, Debug, Clone, Default)]
pub struct WindowRequest {
  /// New resolution width (None = no change)
  pub(crate) width: Option<u32>,
  /// New resolution height (None = no change)
  pub(crate) height: Option<u32>,
  /// New display mode (None = no change)
  pub(crate) display_mode: Option<DisplayMode>,
  /// A new UI scale (`None`: no change): `Some(Some(s))` is `s` render
  /// pixels per UI unit, `Some(None)` the automatic scale
  pub ui_scale: Option<Option<f32>>,
}

impl WindowRequest {
  /// Create a new empty window request
  pub fn new() -> Self {
    Self::default()
  }

  /// Check if any change is requested
  pub fn has_changes(&self) -> bool {
    self.width.is_some()
      || self.height.is_some()
      || self.display_mode.is_some()
      || self.ui_scale.is_some()
  }

  /// Clear all pending requests
  pub fn clear(&mut self) {
    self.width = None;
    self.height = None;
    self.display_mode = None;
    self.ui_scale = None;
  }
}

/// What the game can read about its window, kept current by the engine.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct WindowInfo {
  /// DPI scale factor: 1.0 on standard displays, 2.0 on HiDPI
  pub scale_factor: f64,
  pub display_mode: DisplayMode,
}

impl Default for WindowInfo {
  fn default() -> Self {
    Self {
      scale_factor: 1.0,
      display_mode: DisplayMode::default(),
    }
  }
}
