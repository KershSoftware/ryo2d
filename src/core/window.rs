//! Window presentation settings and the requests scenes make of the window.

use crate::core::ecs::Resource;

pub use crate::core::config::DisplayMode;

/// Window resize behavior.
///
/// Controls how the game viewport responds to window resizing and maximizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowMode {
  /// Fixed window size, no resizing or maximizing.
  ///
  /// The window cannot be resized by the user. Best for games that want
  /// complete control over presentation.
  Fixed,

  /// Resizable with aspect ratio preservation (letterbox/pillarbox).
  ///
  /// The game viewport scales to fit the window while maintaining its original
  /// aspect ratio. Black bars fill unused space. This is the default and most
  /// common approach for 2D games.
  ///
  /// Example: 800×600 game in a 1920×1080 window will scale up with black bars
  /// on the sides.
  #[default]
  ResizableAspectRatio,

  /// Resizable with integer scaling only (pixel-perfect).
  ///
  /// The game viewport scales in integer multiples (1×, 2×, 3×, 4×) to maintain
  /// perfect pixel crispness. Letterboxing fills remaining space. Best for
  /// retro-styled pixel art games.
  ///
  /// Example: 800×600 game in a 1000×800 window will scale to 1× (800×600) with
  /// black bars, not to fit the full window.
  ResizableIntegerScale,

  /// Resizable, stretch to fill (may distort aspect ratio).
  ///
  /// The game viewport stretches to fill the entire window, potentially distorting
  /// the aspect ratio. Rarely used as a default, but can be useful for UI-heavy
  /// games where distortion isn't noticeable.
  ResizableStretch,
}

/// Pending window configuration change requested by the game.
///
/// Set through `ctx.window()` or the `SceneContext::request_*` helpers.
/// The engine will apply these changes after the current update.
#[derive(Resource, Debug, Clone, Default)]
pub struct WindowRequest {
  /// New resolution width (None = no change)
  pub width: Option<u32>,
  /// New resolution height (None = no change)
  pub height: Option<u32>,
  /// New display mode (None = no change)
  pub display_mode: Option<DisplayMode>,
  /// New UI scale factor (None = no change)
  /// Use Some(0) to reset to auto-detect mode
  pub ui_scale: Option<u32>,
}

impl WindowRequest {
  /// Create a new empty window request
  pub fn new() -> Self {
    Self::default()
  }

  /// Request a resolution change
  pub fn with_resolution(mut self, width: u32, height: u32) -> Self {
    self.width = Some(width);
    self.height = Some(height);
    self
  }

  /// Request a display mode change
  pub fn with_display_mode(mut self, mode: DisplayMode) -> Self {
    self.display_mode = Some(mode);
    self
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
