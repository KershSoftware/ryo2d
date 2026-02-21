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
      // Alpha blending (Porter-Duff "over" operator)
      let alpha = color.a as u16;
      let inv_alpha = 255 - alpha;
      for y in y0..y1 {
        for x in x0..x1 {
          let idx = (y as usize * self.frame_width as usize + x as usize) * 4;
          let dst_alpha = self.frame[idx + 3] as u16;
          self.frame[idx] =
            ((color.r as u16 * alpha + self.frame[idx] as u16 * inv_alpha) / 255) as u8;
          self.frame[idx + 1] =
            ((color.g as u16 * alpha + self.frame[idx + 1] as u16 * inv_alpha) / 255) as u8;
          self.frame[idx + 2] =
            ((color.b as u16 * alpha + self.frame[idx + 2] as u16 * inv_alpha) / 255) as u8;
          // Proper alpha: out_alpha = src_alpha + dst_alpha * (1 - src_alpha)
          self.frame[idx + 3] = (alpha + dst_alpha * inv_alpha / 255).min(255) as u8;
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
    self.draw_rect(Rect::new(rect.x, rect.bottom() - t, rect.width, t), color);
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

  /// Draw centered text within a rectangle. Wraps automatically if text is wider than rect.
  pub fn draw_text_centered(&mut self, text: &str, rect: Rect, color: Color) {
    let (text_w, text_h) = self.font.text_size(text);
    if text_w as f32 > rect.width {
      let max_width = rect.width as u32;
      let line_height = self.font.char_height() as f32;
      let line_spacing = 2.0;
      let lines = self.wrap_text_simple(text, max_width);
      let total_height = lines.len() as f32 * line_height
        + (lines.len().saturating_sub(1)) as f32 * line_spacing;
      let start_y = rect.y + (rect.height - total_height) / 2.0;
      for (i, line) in lines.iter().enumerate() {
        let line_width = self.font.text_width(line) as f32;
        let line_x = rect.x + (rect.width - line_width) / 2.0;
        let line_y = start_y + i as f32 * (line_height + line_spacing);
        self.draw_text(line, line_x, line_y, color);
      }
      return;
    }
    let x = rect.x + (rect.width - text_w as f32) / 2.0;
    let y = rect.y + (rect.height - text_h as f32) / 2.0;
    self.draw_text(text, x, y, color);
  }

  /// Simple word wrapping for text that exceeds max_width.
  fn wrap_text_simple(&self, text: &str, max_width: u32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();
    for word in text.split_whitespace() {
      let test = if current_line.is_empty() {
        word.to_string()
      } else {
        format!("{} {}", current_line, word)
      };
      if self.font.text_width(&test) <= max_width {
        current_line = test;
      } else if current_line.is_empty() {
        // Single word wider than max — just add it
        lines.push(word.to_string());
      } else {
        lines.push(current_line);
        current_line = word.to_string();
      }
    }
    if !current_line.is_empty() {
      lines.push(current_line);
    }
    lines
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

  // =========================================================================
  // Sprite Drawing
  // =========================================================================

  /// Draw a sprite at the given position.
  pub fn draw_sprite(&mut self, sprite: &crate::core::sprite::Sprite, x: f32, y: f32) {
    sprite.draw(
      self.frame,
      self.frame_width,
      self.frame_height,
      x as i32,
      y as i32,
    );
  }

  /// Draw a sprite scaled uniformly.
  pub fn draw_sprite_scaled(
    &mut self,
    sprite: &crate::core::sprite::Sprite,
    x: f32,
    y: f32,
    scale: f32,
  ) {
    sprite.draw_scaled(
      self.frame,
      self.frame_width,
      self.frame_height,
      x as i32,
      y as i32,
      scale,
    );
  }

  /// Draw a sprite handle at the given position.
  pub fn draw_sprite_handle(&mut self, handle: &crate::core::asset::SpriteHandle, x: f32, y: f32) {
    handle.with_sprite(|sprite| {
      sprite.draw(
        self.frame,
        self.frame_width,
        self.frame_height,
        x as i32,
        y as i32,
      );
    });
  }

  /// Draw a sprite handle scaled uniformly.
  pub fn draw_sprite_handle_scaled(
    &mut self,
    handle: &crate::core::asset::SpriteHandle,
    x: f32,
    y: f32,
    scale: f32,
  ) {
    handle.with_sprite(|sprite| {
      sprite.draw_scaled(
        self.frame,
        self.frame_width,
        self.frame_height,
        x as i32,
        y as i32,
        scale,
      );
    });
  }

  // =========================================================================
  // Slider Widget
  // =========================================================================

  /// Draw an interactive horizontal slider.
  ///
  /// Returns true if the value was changed this frame.
  ///
  /// # Arguments
  /// * `rect` - The bounding box for the slider track
  /// * `value` - Mutable reference to the value being controlled (will be updated if dragged)
  /// * `min` - Minimum value
  /// * `max` - Maximum value
  pub fn slider(&mut self, rect: Rect, value: &mut f32, min: f32, max: f32) -> bool {
    let track_color = Color::rgb(60, 60, 80);
    let fill_color = Color::rgb(80, 150, 255);
    let handle_color = Color::rgb(255, 255, 255);
    let hover_color = Color::rgb(200, 220, 255);

    let mouse_pos = self.input.mouse_position();

    // Expand interaction area vertically for easier clicking
    let interaction_rect = Rect::new(rect.x, rect.y - 10.0, rect.width, rect.height + 20.0);
    let mouse_in_interaction = interaction_rect.contains(mouse_pos.0, mouse_pos.1);

    // Calculate handle position
    let normalized = (*value - min) / (max - min).max(0.0001);
    let handle_x = rect.x + normalized * rect.width;
    let handle_width = 12.0;
    let handle_height = rect.height + 8.0;
    let handle_rect = Rect::new(
      handle_x - handle_width / 2.0,
      rect.y - 4.0,
      handle_width,
      handle_height,
    );

    let mut changed = false;

    // Handle dragging
    if self.input.mouse_down(MouseButton::Left) && mouse_in_interaction {
      // Calculate new value based on mouse position
      let mouse_x = mouse_pos.0.clamp(rect.x, rect.right());
      let new_normalized = (mouse_x - rect.x) / rect.width;
      let new_value = min + new_normalized * (max - min);
      let clamped_value = new_value.clamp(min, max);

      if (*value - clamped_value).abs() > 0.001 {
        *value = clamped_value;
        changed = true;
      }
    }

    // Draw track background
    self.draw_rect(rect, track_color);

    // Draw filled portion
    let fill_rect = Rect::new(rect.x, rect.y, normalized * rect.width, rect.height);
    self.draw_rect(fill_rect, fill_color);

    // Draw handle
    let handle_visual_color = if mouse_in_interaction {
      hover_color
    } else {
      handle_color
    };
    self.draw_rect(handle_rect, handle_visual_color);

    changed
  }
}
