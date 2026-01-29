//! Immediate-mode UI drawing context
//!
//! The `UiContext` provides a simple API for drawing shapes, text, and
//! interactive elements. Create one per frame and pass it to render functions.

use winit::event::MouseButton;

use crate::core::input::InputState;

use super::font::BitmapFont;
use super::types::{Anchor, Color, Rect};

// ============================================================================
// UiContext - Immediate Mode Drawing
// ============================================================================

/// Context for immediate-mode UI rendering.
///
/// Provides methods for drawing shapes, text, and interactive elements.
/// Create one per frame and pass it to your render functions.
///
/// # Example
///
/// ```ignore
/// fn render_ui(&mut self, ui: &mut UiContext) {
///     // Draw a simple label
///     ui.draw_text("Score: 100", 10.0, 10.0, Color::WHITE);
///     
///     // Draw a button that returns true when clicked
///     if ui.button("Start", Rect::new(100.0, 100.0, 120.0, 40.0)) {
///         // Handle button click
///     }
/// }
/// ```
pub struct UiContext<'a> {
  /// Frame buffer (RGBA, 4 bytes per pixel)
  frame: &'a mut [u8],
  /// Frame width in pixels
  frame_width: u32,
  /// Frame height in pixels
  frame_height: u32,
  /// Current input state (for click detection)
  input: &'a InputState,
  /// Font for text rendering
  font: &'a BitmapFont,
}

impl<'a> UiContext<'a> {
  /// Create a new UI context for the current frame.
  pub fn new(
    frame: &'a mut [u8],
    frame_width: u32,
    frame_height: u32,
    input: &'a InputState,
    font: &'a BitmapFont,
  ) -> Self {
    Self {
      frame,
      frame_width,
      frame_height,
      input,
      font,
    }
  }

  /// Get the screen dimensions.
  pub fn screen_size(&self) -> (u32, u32) {
    (self.frame_width, self.frame_height)
  }

  // =========================================================================
  // Shape Drawing
  // =========================================================================

  /// Draw a filled rectangle.
  pub fn draw_rect(&mut self, rect: Rect, color: Color) {
    let x0 = rect.x.max(0.0) as i32;
    let y0 = rect.y.max(0.0) as i32;
    let x1 = rect.right().min(self.frame_width as f32) as i32;
    let y1 = rect.bottom().min(self.frame_height as f32) as i32;

    if x0 >= x1 || y0 >= y1 || color.a == 0 {
      return;
    }

    if color.a == 255 {
      // Opaque - direct write
      for y in y0..y1 {
        for x in x0..x1 {
          let idx = (y as usize * self.frame_width as usize + x as usize) * 4;
          self.frame[idx] = color.r;
          self.frame[idx + 1] = color.g;
          self.frame[idx + 2] = color.b;
          self.frame[idx + 3] = 255;
        }
      }
    } else {
      // Alpha blending
      let alpha = color.a as u16;
      let inv_alpha = 255 - alpha;
      for y in y0..y1 {
        for x in x0..x1 {
          let idx = (y as usize * self.frame_width as usize + x as usize) * 4;
          self.frame[idx] =
            ((color.r as u16 * alpha + self.frame[idx] as u16 * inv_alpha) / 255) as u8;
          self.frame[idx + 1] =
            ((color.g as u16 * alpha + self.frame[idx + 1] as u16 * inv_alpha) / 255) as u8;
          self.frame[idx + 2] =
            ((color.b as u16 * alpha + self.frame[idx + 2] as u16 * inv_alpha) / 255) as u8;
          self.frame[idx + 3] = 255;
        }
      }
    }
  }

  /// Draw a rectangle outline (border only).
  pub fn draw_rect_outline(&mut self, rect: Rect, color: Color, thickness: f32) {
    let t = thickness;
    // Top
    self.draw_rect(Rect::new(rect.x, rect.y, rect.width, t), color);
    // Bottom
    self.draw_rect(
      Rect::new(rect.x, rect.bottom() - t, rect.width, t),
      color,
    );
    // Left
    self.draw_rect(
      Rect::new(rect.x, rect.y + t, t, rect.height - t * 2.0),
      color,
    );
    // Right
    self.draw_rect(
      Rect::new(rect.right() - t, rect.y + t, t, rect.height - t * 2.0),
      color,
    );
  }

  /// Draw a panel with background and optional border.
  pub fn draw_panel(&mut self, rect: Rect, bg_color: Color, border_color: Option<Color>) {
    self.draw_rect(rect, bg_color);
    if let Some(border) = border_color {
      self.draw_rect_outline(rect, border, 1.0);
    }
  }

  /// Draw a horizontal progress bar.
  ///
  /// # Arguments
  /// * `rect` - Bounding rectangle for the progress bar
  /// * `progress` - Fill amount (0.0 to 1.0)
  /// * `fill_color` - Color of the filled portion
  /// * `bg_color` - Color of the background
  pub fn draw_progress_bar(
    &mut self,
    rect: Rect,
    progress: f32,
    fill_color: Color,
    bg_color: Color,
  ) {
    // Background
    self.draw_rect(rect, bg_color);

    // Fill
    let progress = progress.clamp(0.0, 1.0);
    if progress > 0.0 {
      let fill_width = rect.width * progress;
      self.draw_rect(
        Rect::new(rect.x, rect.y, fill_width, rect.height),
        fill_color,
      );
    }
  }

  /// Draw a progress bar with border.
  pub fn draw_progress_bar_with_border(
    &mut self,
    rect: Rect,
    progress: f32,
    fill_color: Color,
    bg_color: Color,
    border_color: Color,
  ) {
    self.draw_progress_bar(rect, progress, fill_color, bg_color);
    self.draw_rect_outline(rect, border_color, 1.0);
  }

  // =========================================================================
  // Text Drawing
  // =========================================================================

  /// Draw text at the given position.
  pub fn draw_text(&mut self, text: &str, x: f32, y: f32, color: Color) {
    self.font.draw_text(
      self.frame,
      self.frame_width,
      self.frame_height,
      text,
      x as i32,
      y as i32,
      color,
    );
  }

  /// Draw text with anchor-based positioning.
  pub fn draw_text_anchored(
    &mut self,
    text: &str,
    anchor: Anchor,
    offset_x: f32,
    offset_y: f32,
    color: Color,
  ) {
    let (text_w, text_h) = self.font.text_size(text);
    let (x, y) = anchor.calculate_position(
      offset_x,
      offset_y,
      text_w as f32,
      text_h as f32,
      self.frame_width as f32,
      self.frame_height as f32,
    );
    self.draw_text(text, x, y, color);
  }

  /// Draw centered text within a rectangle.
  pub fn draw_text_centered(&mut self, text: &str, rect: Rect, color: Color) {
    let (text_w, text_h) = self.font.text_size(text);
    let x = rect.x + (rect.width - text_w as f32) / 2.0;
    let y = rect.y + (rect.height - text_h as f32) / 2.0;
    self.draw_text(text, x, y, color);
  }

  /// Calculate the pixel size of text.
  pub fn text_size(&self, text: &str) -> (u32, u32) {
    self.font.text_size(text)
  }

  // =========================================================================
  // Interactive Widgets
  // =========================================================================

  /// Check if a rectangle is being clicked this frame.
  ///
  /// Returns `true` if the mouse button was just pressed while over the rectangle.
  pub fn is_clicked(&self, rect: Rect) -> bool {
    if !self.input.mouse_pressed(MouseButton::Left) {
      return false;
    }
    let (mx, my) = self.input.mouse_position();
    rect.contains(mx, my)
  }

  /// Check if a rectangle is being hovered.
  pub fn is_hovered(&self, rect: Rect) -> bool {
    let (mx, my) = self.input.mouse_position();
    rect.contains(mx, my)
  }

  /// Check if a rectangle is being held (mouse down while hovering).
  pub fn is_held(&self, rect: Rect) -> bool {
    if !self.input.mouse_down(MouseButton::Left) {
      return false;
    }
    let (mx, my) = self.input.mouse_position();
    rect.contains(mx, my)
  }

  /// Draw an interactive button and return whether it was clicked.
  ///
  /// Draws a button with hover/pressed states and returns `true` on click.
  pub fn button(&mut self, text: &str, rect: Rect) -> bool {
    let hovered = self.is_hovered(rect);
    let held = self.is_held(rect);
    let clicked = self.is_clicked(rect);

    // Button colors based on state
    let bg_color = if held {
      Color::rgb(60, 60, 80)
    } else if hovered {
      Color::rgb(50, 50, 70)
    } else {
      Color::rgb(40, 40, 60)
    };
    let border_color = if hovered {
      Color::rgb(100, 100, 140)
    } else {
      Color::rgb(80, 80, 100)
    };
    let text_color = if held {
      Color::LIGHT_GRAY
    } else {
      Color::WHITE
    };

    self.draw_panel(rect, bg_color, Some(border_color));
    self.draw_text_centered(text, rect, text_color);

    clicked
  }

  /// Draw an interactive button with custom colors.
  pub fn button_styled(
    &mut self,
    text: &str,
    rect: Rect,
    normal_bg: Color,
    hover_bg: Color,
    pressed_bg: Color,
    text_color: Color,
  ) -> bool {
    let hovered = self.is_hovered(rect);
    let held = self.is_held(rect);
    let clicked = self.is_clicked(rect);

    let bg_color = if held {
      pressed_bg
    } else if hovered {
      hover_bg
    } else {
      normal_bg
    };

    self.draw_rect(rect, bg_color);
    self.draw_text_centered(text, rect, text_color);

    clicked
  }

  /// Draw a label with optional background panel.
  pub fn label(&mut self, text: &str, x: f32, y: f32, text_color: Color, bg_color: Option<Color>) {
    if let Some(bg) = bg_color {
      let (w, h) = self.text_size(text);
      let padding = 4.0;
      self.draw_rect(
        Rect::new(
          x - padding,
          y - padding,
          w as f32 + padding * 2.0,
          h as f32 + padding * 2.0,
        ),
        bg,
      );
    }
    self.draw_text(text, x, y, text_color);
  }
}
