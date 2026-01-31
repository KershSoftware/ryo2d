//! Basic UI types: Color, Rect, Anchor
//!
//! These are the foundational types used throughout the UI system.

use serde::{Deserialize, Serialize};

// ============================================================================
// Color
// ============================================================================

/// RGBA color with 8-bit components.
///
/// Provides common color constants and utilities for UI rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Color {
  pub r: u8,
  pub g: u8,
  pub b: u8,
  pub a: u8,
}

impl Color {
  /// Create a new color from RGBA components.
  pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
    Self { r, g, b, a }
  }

  /// Create a fully opaque color from RGB components.
  pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
    Self { r, g, b, a: 255 }
  }

  /// Create a color from a hex value (0xRRGGBB or 0xRRGGBBAA).
  pub const fn hex(value: u32) -> Self {
    if value > 0xFFFFFF {
      // 0xRRGGBBAA format
      Self {
        r: ((value >> 24) & 0xFF) as u8,
        g: ((value >> 16) & 0xFF) as u8,
        b: ((value >> 8) & 0xFF) as u8,
        a: (value & 0xFF) as u8,
      }
    } else {
      // 0xRRGGBB format (fully opaque)
      Self {
        r: ((value >> 16) & 0xFF) as u8,
        g: ((value >> 8) & 0xFF) as u8,
        b: (value & 0xFF) as u8,
        a: 255,
      }
    }
  }

  /// Create a color with modified alpha.
  pub const fn with_alpha(self, a: u8) -> Self {
    Self { a, ..self }
  }

  /// Convert to RGBA array.
  pub const fn to_array(self) -> [u8; 4] {
    [self.r, self.g, self.b, self.a]
  }

  // Common color constants
  pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);
  pub const BLACK: Self = Self::rgb(0, 0, 0);
  pub const WHITE: Self = Self::rgb(255, 255, 255);
  pub const RED: Self = Self::rgb(255, 0, 0);
  pub const GREEN: Self = Self::rgb(0, 255, 0);
  pub const BLUE: Self = Self::rgb(0, 0, 255);
  pub const YELLOW: Self = Self::rgb(255, 255, 0);
  pub const CYAN: Self = Self::rgb(0, 255, 255);
  pub const MAGENTA: Self = Self::rgb(255, 0, 255);
  pub const GRAY: Self = Self::rgb(128, 128, 128);
  pub const DARK_GRAY: Self = Self::rgb(64, 64, 64);
  pub const LIGHT_GRAY: Self = Self::rgb(192, 192, 192);

  // Game-friendly colors
  pub const HEALTH_RED: Self = Self::rgb(220, 50, 50);
  pub const MANA_BLUE: Self = Self::rgb(50, 100, 220);
  pub const STAMINA_GREEN: Self = Self::rgb(50, 180, 80);
  pub const XP_GOLD: Self = Self::rgb(255, 200, 50);
  pub const PANEL_BG: Self = Self::new(20, 20, 30, 200);
  pub const PANEL_BORDER: Self = Self::rgb(80, 80, 100);

  /// Convert to normalized RGBA (0.0-1.0 range) for shader uniforms.
  pub fn to_normalized(self) -> [f32; 4] {
    [
      self.r as f32 / 255.0,
      self.g as f32 / 255.0,
      self.b as f32 / 255.0,
      self.a as f32 / 255.0,
    ]
  }
}

// ============================================================================
// Rect
// ============================================================================

/// A rectangle defined by position and size.
///
/// Used for UI element positioning and hit testing.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
}

impl Rect {
  /// Create a new rectangle.
  pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
    Self {
      x,
      y,
      width,
      height,
    }
  }

  /// Create a rectangle from position and size tuples.
  pub fn from_pos_size(pos: (f32, f32), size: (f32, f32)) -> Self {
    Self {
      x: pos.0,
      y: pos.1,
      width: size.0,
      height: size.1,
    }
  }

  /// Create a rectangle centered at a point.
  pub fn centered_at(cx: f32, cy: f32, width: f32, height: f32) -> Self {
    Self {
      x: cx - width / 2.0,
      y: cy - height / 2.0,
      width,
      height,
    }
  }

  /// Get the right edge (x + width).
  pub fn right(&self) -> f32 {
    self.x + self.width
  }

  /// Get the bottom edge (y + height).
  pub fn bottom(&self) -> f32 {
    self.y + self.height
  }

  /// Get the center point.
  pub fn center(&self) -> (f32, f32) {
    (self.x + self.width / 2.0, self.y + self.height / 2.0)
  }

  /// Check if a point is inside the rectangle.
  pub fn contains(&self, px: f32, py: f32) -> bool {
    px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
  }

  /// Create a smaller rectangle with padding removed.
  pub fn shrink(&self, padding: f32) -> Self {
    Self {
      x: self.x + padding,
      y: self.y + padding,
      width: (self.width - padding * 2.0).max(0.0),
      height: (self.height - padding * 2.0).max(0.0),
    }
  }

  /// Create a larger rectangle with padding added.
  pub fn expand(&self, padding: f32) -> Self {
    Self {
      x: self.x - padding,
      y: self.y - padding,
      width: self.width + padding * 2.0,
      height: self.height + padding * 2.0,
    }
  }
}

// ============================================================================
// Anchor
// ============================================================================

/// Anchor point for positioning UI elements relative to screen edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Anchor {
  /// Top-left corner (default)
  #[default]
  TopLeft,
  /// Top-center
  TopCenter,
  /// Top-right corner
  TopRight,
  /// Center-left
  CenterLeft,
  /// Center of screen
  Center,
  /// Center-right
  CenterRight,
  /// Bottom-left corner
  BottomLeft,
  /// Bottom-center
  BottomCenter,
  /// Bottom-right corner
  BottomRight,
}

impl Anchor {
  /// Calculate the screen position for an element with this anchor.
  ///
  /// # Arguments
  /// * `offset_x`, `offset_y` - Offset from anchor point
  /// * `element_width`, `element_height` - Size of the element
  /// * `screen_width`, `screen_height` - Size of the screen
  ///
  /// # Returns
  /// The top-left corner position of the element in screen coordinates.
  pub fn calculate_position(
    &self,
    offset_x: f32,
    offset_y: f32,
    element_width: f32,
    element_height: f32,
    screen_width: f32,
    screen_height: f32,
  ) -> (f32, f32) {
    let (anchor_x, anchor_y) = match self {
      Anchor::TopLeft => (0.0, 0.0),
      Anchor::TopCenter => (screen_width / 2.0 - element_width / 2.0, 0.0),
      Anchor::TopRight => (screen_width - element_width, 0.0),
      Anchor::CenterLeft => (0.0, screen_height / 2.0 - element_height / 2.0),
      Anchor::Center => (
        screen_width / 2.0 - element_width / 2.0,
        screen_height / 2.0 - element_height / 2.0,
      ),
      Anchor::CenterRight => (
        screen_width - element_width,
        screen_height / 2.0 - element_height / 2.0,
      ),
      Anchor::BottomLeft => (0.0, screen_height - element_height),
      Anchor::BottomCenter => (
        screen_width / 2.0 - element_width / 2.0,
        screen_height - element_height,
      ),
      Anchor::BottomRight => (screen_width - element_width, screen_height - element_height),
    };

    (anchor_x + offset_x, anchor_y + offset_y)
  }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn color_from_hex_rgb() {
    let c = Color::hex(0xFF8000);
    assert_eq!(c.r, 255);
    assert_eq!(c.g, 128);
    assert_eq!(c.b, 0);
    assert_eq!(c.a, 255);
  }

  #[test]
  fn color_from_hex_rgba() {
    let c = Color::hex(0xFF800080);
    assert_eq!(c.r, 255);
    assert_eq!(c.g, 128);
    assert_eq!(c.b, 0);
    assert_eq!(c.a, 128);
  }

  #[test]
  fn color_with_alpha() {
    let c = Color::RED.with_alpha(128);
    assert_eq!(c.r, 255);
    assert_eq!(c.g, 0);
    assert_eq!(c.b, 0);
    assert_eq!(c.a, 128);
  }

  #[test]
  fn rect_contains_point() {
    let r = Rect::new(10.0, 10.0, 100.0, 50.0);
    assert!(r.contains(10.0, 10.0)); // Top-left corner
    assert!(r.contains(50.0, 30.0)); // Inside
    assert!(!r.contains(110.0, 30.0)); // Right edge (exclusive)
    assert!(!r.contains(5.0, 30.0)); // Left of rect
  }

  #[test]
  fn rect_center() {
    let r = Rect::new(0.0, 0.0, 100.0, 50.0);
    assert_eq!(r.center(), (50.0, 25.0));
  }

  #[test]
  fn rect_shrink_expand() {
    let r = Rect::new(10.0, 10.0, 100.0, 50.0);
    let shrunk = r.shrink(5.0);
    assert_eq!(shrunk.x, 15.0);
    assert_eq!(shrunk.y, 15.0);
    assert_eq!(shrunk.width, 90.0);
    assert_eq!(shrunk.height, 40.0);

    let expanded = r.expand(5.0);
    assert_eq!(expanded.x, 5.0);
    assert_eq!(expanded.y, 5.0);
    assert_eq!(expanded.width, 110.0);
    assert_eq!(expanded.height, 60.0);
  }

  #[test]
  fn anchor_positions() {
    let (x, y) = Anchor::TopLeft.calculate_position(10.0, 10.0, 100.0, 50.0, 800.0, 600.0);
    assert_eq!((x, y), (10.0, 10.0));

    let (x, y) = Anchor::BottomRight.calculate_position(-10.0, -10.0, 100.0, 50.0, 800.0, 600.0);
    assert_eq!((x, y), (690.0, 540.0));

    let (x, y) = Anchor::Center.calculate_position(0.0, 0.0, 100.0, 50.0, 800.0, 600.0);
    assert_eq!((x, y), (350.0, 275.0));
  }
}
