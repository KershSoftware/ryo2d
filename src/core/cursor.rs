//! Mouse cursor customization
//!
//! Provides control over the mouse cursor appearance including:
//! - System cursor icons (pointer, crosshair, grab, etc.)
//! - Custom sprite cursors from game assets
//! - Cursor visibility control
//! - Cursor confinement to window
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
//!     // Change to crosshair when aiming
//!     if ctx.input.mouse_down(MouseButton::Right) {
//!         ctx.set_cursor(CursorStyle::Crosshair);
//!     } else {
//!         ctx.set_cursor(CursorStyle::Default);
//!     }
//!
//!     // Use a custom sprite cursor
//!     let crosshair = ctx.assets.load_sprite("crosshair.png").unwrap();
//!     ctx.set_custom_cursor(&crosshair, 16, 16);  // hotspot at center
//!
//!     // Hide cursor during gameplay
//!     ctx.set_cursor_visible(false);
//!     SceneAction::Continue
//! }
//! ```

use crate::core::ecs::Resource;

/// System cursor icons available on most platforms.
///
/// These map to standard OS cursor icons. Not all icons may be available
/// on all platforms - unavailable icons fall back to `Default`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CursorStyle {
  /// Standard arrow cursor (default)
  #[default]
  Default,
  /// Text input cursor (I-beam)
  Text,
  /// Crosshair for precise selection
  Crosshair,
  /// Hand/pointer for clickable elements
  Pointer,
  /// Resize horizontally (left-right)
  ResizeHorizontal,
  /// Resize vertically (up-down)
  ResizeVertical,
  /// Resize diagonally (top-left to bottom-right)
  ResizeNeSw,
  /// Resize diagonally (top-right to bottom-left)
  ResizeNwSe,
  /// Move/drag cursor (four arrows)
  Move,
  /// Not allowed / forbidden
  NotAllowed,
  /// Grabbing hand (open)
  Grab,
  /// Grabbing hand (closed/grabbed)
  Grabbing,
  /// Waiting/busy spinner
  Wait,
  /// Progress indicator (arrow with spinner)
  Progress,
  /// Help cursor (arrow with question mark)
  Help,
  /// Cell/table selection
  Cell,
  /// Zoom in
  ZoomIn,
  /// Zoom out
  ZoomOut,
}

impl CursorStyle {
  /// Convert to winit CursorIcon
  pub(crate) fn to_winit(self) -> winit::window::CursorIcon {
    use winit::window::CursorIcon;
    match self {
      CursorStyle::Default => CursorIcon::Default,
      CursorStyle::Text => CursorIcon::Text,
      CursorStyle::Crosshair => CursorIcon::Crosshair,
      CursorStyle::Pointer => CursorIcon::Pointer,
      CursorStyle::ResizeHorizontal => CursorIcon::EwResize,
      CursorStyle::ResizeVertical => CursorIcon::NsResize,
      CursorStyle::ResizeNeSw => CursorIcon::NeswResize,
      CursorStyle::ResizeNwSe => CursorIcon::NwseResize,
      CursorStyle::Move => CursorIcon::Move,
      CursorStyle::NotAllowed => CursorIcon::NotAllowed,
      CursorStyle::Grab => CursorIcon::Grab,
      CursorStyle::Grabbing => CursorIcon::Grabbing,
      CursorStyle::Wait => CursorIcon::Wait,
      CursorStyle::Progress => CursorIcon::Progress,
      CursorStyle::Help => CursorIcon::Help,
      CursorStyle::Cell => CursorIcon::Cell,
      CursorStyle::ZoomIn => CursorIcon::ZoomIn,
      CursorStyle::ZoomOut => CursorIcon::ZoomOut,
    }
  }
}

/// Data for a custom sprite cursor.
///
/// Contains the RGBA pixel data and hotspot position.
#[derive(Debug, Clone)]
pub struct CustomCursorData {
  /// RGBA pixel data (4 bytes per pixel, not premultiplied alpha)
  pub rgba: Vec<u8>,
  /// Width in pixels
  pub width: u16,
  /// Height in pixels
  pub height: u16,
  /// X coordinate of the hotspot (click point)
  pub hotspot_x: u16,
  /// Y coordinate of the hotspot (click point)
  pub hotspot_y: u16,
  /// Unique identifier for caching (sprite path)
  pub id: String,
}

/// Hotspot position presets for custom cursors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorHotspot {
  /// Top-left corner (0, 0) - default for arrow cursors
  #[default]
  TopLeft,
  /// Center of the cursor - good for crosshairs
  Center,
  /// Custom position
  Custom(u16, u16),
}

/// Pending cursor changes to be applied to the window.
#[derive(Resource, Debug, Default, Clone)]
pub struct CursorRequest {
  /// New system cursor style to set (None = no change)
  pub cursor: Option<CursorStyle>,
  /// Custom cursor data (takes precedence over cursor style)
  pub custom_cursor: Option<CustomCursorData>,
  /// New visibility state (None = no change)
  pub visible: Option<bool>,
  /// Whether to confine cursor to window (None = no change)
  pub confined: Option<bool>,
  /// Whether to lock cursor in place for FPS-style input (None = no change)
  ///
  /// When locked, the cursor is hidden and held in place. Raw mouse motion
  /// is reported via `mouse_delta()` instead of position-based deltas.
  pub locked: Option<bool>,
}

impl CursorRequest {
  pub fn has_changes(&self) -> bool {
    self.cursor.is_some()
      || self.custom_cursor.is_some()
      || self.visible.is_some()
      || self.confined.is_some()
      || self.locked.is_some()
  }
}

/// What the window shows: a system cursor, or a sprite's by its id
#[derive(Debug, Clone)]
pub(crate) enum CursorShape {
  Style(CursorStyle),
  Custom(CustomCursorData),
}

impl CursorShape {
  /// Whether the window already shows it: styles by value, sprites by id
  fn is_shown_as(&self, other: &CursorShape) -> bool {
    match (self, other) {
      (CursorShape::Style(a), CursorShape::Style(b)) => a == b,
      (CursorShape::Custom(a), CursorShape::Custom(b)) => a.id == b.id,
      _ => false,
    }
  }
}

/// Which cursor the window shows: the engine's per-frame override (the
/// console's), else the game's last choice, which persists without being
/// asked for again. The window is told only when the result differs from
/// what it last showed, or on a refresh (macOS applies a new icon lazily).
#[derive(Debug)]
pub(crate) struct CursorResolver {
  game: CursorShape,
  shown: CursorShape,
}

impl Default for CursorResolver {
  fn default() -> Self {
    Self {
      game: CursorShape::Style(CursorStyle::Default),
      shown: CursorShape::Style(CursorStyle::Default),
    }
  }
}

impl CursorResolver {
  /// This frame's cursor change, if any. `style` and `custom` are what the
  /// game asked for this frame (a sprite wins over a style), `over` the
  /// override, and `refresh` shows the cursor again even if unchanged.
  pub(crate) fn resolve(
    &mut self,
    style: Option<CursorStyle>,
    custom: Option<CustomCursorData>,
    over: Option<CursorStyle>,
    refresh: bool,
  ) -> Option<CursorShape> {
    if let Some(custom) = custom {
      self.game = CursorShape::Custom(custom);
    } else if let Some(style) = style {
      self.game = CursorShape::Style(style);
    }
    let wanted = match over {
      Some(style) => CursorShape::Style(style),
      None => self.game.clone(),
    };
    if !refresh && wanted.is_shown_as(&self.shown) {
      return None;
    }
    self.shown = wanted.clone();
    Some(wanted)
  }

  /// The window was made afresh: it shows its default, whatever was shown
  pub(crate) fn window_reset(&mut self) {
    self.shown = CursorShape::Style(CursorStyle::Default);
  }
}

/// Whether the cursor can be confined to the window here: winit cannot on
/// macOS, iOS, Android or the web
pub(crate) fn confinement_supported() -> bool {
  !cfg!(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_arch = "wasm32"
  ))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn cursor_style_default() {
    let style = CursorStyle::default();
    assert_eq!(style, CursorStyle::Default);
  }

  #[test]
  fn cursor_request_changes() {
    let mut req = CursorRequest::default();
    assert!(!req.has_changes());

    req.visible = Some(false);
    assert!(req.has_changes());

    req.cursor = Some(CursorStyle::Pointer);
    assert!(req.has_changes());

    // Reset to default (no changes)
    req = CursorRequest::default();
    assert!(!req.has_changes());
  }

  #[test]
  fn cursor_request_locked_triggers_changes() {
    let mut req = CursorRequest::default();
    assert!(!req.has_changes());

    req.locked = Some(true);
    assert!(req.has_changes());

    req = CursorRequest::default();
    req.locked = Some(false);
    assert!(req.has_changes());
  }

  #[test]
  fn macos_and_the_web_cannot_confine_the_cursor() {
    let unsupported = cfg!(any(
      target_os = "macos",
      target_os = "ios",
      target_os = "android",
      target_arch = "wasm32"
    ));
    assert_eq!(confinement_supported(), !unsupported);
  }

  #[test]
  fn cursor_request_confined_triggers_changes() {
    let req = CursorRequest {
      confined: Some(true),
      ..Default::default()
    };
    assert!(req.has_changes());
  }

  #[test]
  fn cursor_request_default_has_no_fields_set() {
    let req = CursorRequest::default();
    assert!(req.cursor.is_none());
    assert!(req.custom_cursor.is_none());
    assert!(req.visible.is_none());
    assert!(req.confined.is_none());
    assert!(req.locked.is_none());
  }

  #[test]
  fn cursor_request_all_fields_trigger_changes() {
    // Each field individually should trigger has_changes
    let req = CursorRequest {
      cursor: Some(CursorStyle::Crosshair),
      ..Default::default()
    };
    assert!(req.has_changes());

    let req = CursorRequest {
      visible: Some(true),
      ..Default::default()
    };
    assert!(req.has_changes());

    let req = CursorRequest {
      confined: Some(false),
      ..Default::default()
    };
    assert!(req.has_changes());

    let req = CursorRequest {
      locked: Some(true),
      ..Default::default()
    };
    assert!(req.has_changes());

    let req = CursorRequest {
      custom_cursor: Some(CustomCursorData {
        rgba: vec![0; 16],
        width: 2,
        height: 2,
        hotspot_x: 0,
        hotspot_y: 0,
        id: "test".into(),
      }),
      ..Default::default()
    };
    assert!(req.has_changes());
  }

  #[test]
  fn all_cursor_styles_convert_to_winit() {
    // Ensure all our cursor styles can convert without panicking
    let styles = [
      CursorStyle::Default,
      CursorStyle::Text,
      CursorStyle::Crosshair,
      CursorStyle::Pointer,
      CursorStyle::ResizeHorizontal,
      CursorStyle::ResizeVertical,
      CursorStyle::ResizeNeSw,
      CursorStyle::ResizeNwSe,
      CursorStyle::Move,
      CursorStyle::NotAllowed,
      CursorStyle::Grab,
      CursorStyle::Grabbing,
      CursorStyle::Wait,
      CursorStyle::Progress,
      CursorStyle::Help,
      CursorStyle::Cell,
      CursorStyle::ZoomIn,
      CursorStyle::ZoomOut,
    ];
    for style in styles {
      let _ = style.to_winit();
    }
  }

  fn style(shape: Option<CursorShape>) -> Option<CursorStyle> {
    match shape {
      Some(CursorShape::Style(style)) => Some(style),
      _ => None,
    }
  }

  fn sprite(id: &str) -> CustomCursorData {
    CustomCursorData {
      rgba: vec![0; 4],
      width: 1,
      height: 1,
      hotspot_x: 0,
      hotspot_y: 0,
      id: id.to_string(),
    }
  }

  #[test]
  fn the_override_wins_and_the_game_cursor_comes_back() {
    let mut cursors = CursorResolver::default();
    let shown = cursors.resolve(Some(CursorStyle::Crosshair), None, None, false);
    assert_eq!(style(shown), Some(CursorStyle::Crosshair));
    let shown = cursors.resolve(None, None, Some(CursorStyle::ResizeNwSe), false);
    assert_eq!(style(shown), Some(CursorStyle::ResizeNwSe));
    // Nobody asks any more: the game's cursor is back, without a request
    let shown = cursors.resolve(None, None, None, false);
    assert_eq!(style(shown), Some(CursorStyle::Crosshair));
  }

  #[test]
  fn the_override_beats_a_cursor_set_in_the_same_frame() {
    let mut cursors = CursorResolver::default();
    let shown = cursors.resolve(
      Some(CursorStyle::Crosshair),
      None,
      Some(CursorStyle::Move),
      false,
    );
    assert_eq!(style(shown), Some(CursorStyle::Move));
    let shown = cursors.resolve(None, None, None, false);
    assert_eq!(style(shown), Some(CursorStyle::Crosshair), "kept meanwhile");
  }

  #[test]
  fn the_game_cursor_persists_and_a_change_applies_once() {
    let mut cursors = CursorResolver::default();
    assert!(
      cursors.resolve(None, None, None, false).is_none(),
      "the window starts on the default"
    );
    assert!(
      cursors
        .resolve(Some(CursorStyle::Default), None, None, false)
        .is_none()
    );
    let shown = cursors.resolve(Some(CursorStyle::Pointer), None, None, false);
    assert_eq!(style(shown), Some(CursorStyle::Pointer));
    for _ in 0..3 {
      assert!(
        cursors.resolve(None, None, None, false).is_none(),
        "persists"
      );
      assert!(
        cursors
          .resolve(Some(CursorStyle::Pointer), None, None, false)
          .is_none(),
        "no change, nothing applied"
      );
    }
  }

  #[test]
  fn a_button_release_applies_the_cursor_again() {
    let mut cursors = CursorResolver::default();
    cursors.resolve(Some(CursorStyle::Pointer), None, None, false);
    let shown = cursors.resolve(None, None, None, true);
    assert_eq!(style(shown), Some(CursorStyle::Pointer));
    let shown = cursors.resolve(None, None, Some(CursorStyle::Move), true);
    assert_eq!(style(shown), Some(CursorStyle::Move));
  }

  #[test]
  fn a_custom_cursor_is_the_games_and_comes_back_too() {
    let mut cursors = CursorResolver::default();
    let shown = cursors.resolve(None, Some(sprite("a")), None, false);
    assert!(matches!(shown, Some(CursorShape::Custom(c)) if c.id == "a"));
    assert!(
      cursors
        .resolve(None, Some(sprite("a")), None, false)
        .is_none()
    );
    cursors.resolve(None, None, Some(CursorStyle::ResizeNwSe), false);
    let shown = cursors.resolve(None, None, None, false);
    assert!(matches!(shown, Some(CursorShape::Custom(c)) if c.id == "a"));
    // A style set later replaces it, and a sprite wins over a style set with it
    let shown = cursors.resolve(Some(CursorStyle::Text), None, None, false);
    assert_eq!(style(shown), Some(CursorStyle::Text));
    let shown = cursors.resolve(Some(CursorStyle::Text), Some(sprite("b")), None, false);
    assert!(matches!(shown, Some(CursorShape::Custom(c)) if c.id == "b"));
  }
}
