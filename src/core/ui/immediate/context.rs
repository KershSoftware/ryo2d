//! Immediate-mode UI drawing context
//!
//! The `UiContext` provides a simple API for drawing shapes, text, and
//! interactive elements. Create one per frame and pass it to render functions.
//!
//! # Clipping
//!
//! The UI context supports nested clip rects for scroll views and overflow handling.
//! Use `push_clip()` and `pop_clip()` to manage the clip stack:
//!
//! ```ignore
//! // Set up a scroll region
//! ui.push_clip(scroll_area);
//!
//! // Draw content - anything outside scroll_area is clipped
//! ui.draw_text("Item 1", 10.0, 10.0, Color::WHITE);
//! ui.draw_text("Item 2", 10.0, 30.0, Color::WHITE);
//!
//! // Restore previous clip state
//! ui.pop_clip();
//! ```

use winit::event::MouseButton;

use crate::core::input::InputState;

use super::font::BitmapFont;
use crate::core::ui::retained::{DropdownState, Focusable, ScrollViewState, TextInputState};
use crate::core::ui::theme::Theme;
use crate::core::ui::types::{Anchor, Color, Rect};

/// Maximum depth of nested clip rects.
const MAX_CLIP_STACK_DEPTH: usize = 16;

/// Pending dropdown overlay to be drawn at the end of the frame.
struct PendingDropdown {
  /// The trigger button rect
  trigger_rect: Rect,
  /// The dropdown list rect
  list_rect: Rect,
  /// Currently hovered index (for keyboard navigation)
  hover_index: usize,
  /// Options to display (stored as owned strings for lifetime management)
  options: Vec<String>,
}

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
  /// Optional theme for styled widgets
  theme: Option<&'a Theme>,
  /// Stack of clip rects for nested clipping
  clip_stack: Vec<Rect>,
  /// Current effective clip rect (intersection of all stacked clips)
  current_clip: Option<Rect>,
  /// Pending dropdown overlays to draw at end of frame
  pending_dropdowns: Vec<PendingDropdown>,
  /// Whether escape was consumed by a widget this frame
  escape_consumed: bool,
  /// Whether the primary mouse click was consumed by a widget this frame
  click_consumed: bool,
  /// ID of the currently focused dropdown (0 = none)
  focused_dropdown_id: u32,
  /// UI scale factor for HiDPI displays (1 = normal, 2 = double size, etc.)
  ui_scale: u32,
  /// Whether the mouse is occluded by an overlay (e.g. developer console).
  /// When true, `is_hovered`, `is_held`, and `is_clicked` all return false.
  mouse_occluded: bool,
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
      theme: None,
      clip_stack: Vec::with_capacity(MAX_CLIP_STACK_DEPTH),
      current_clip: None,
      pending_dropdowns: Vec::new(),
      escape_consumed: false,
      click_consumed: false,
      focused_dropdown_id: 0,
      ui_scale: 1,
      mouse_occluded: false,
    }
  }

  /// Create a new UI context with a specific scale factor.
  ///
  /// Use this for HiDPI displays where UI should be scaled up for readability.
  /// Scale of 1 = normal size, 2 = double size (crisp pixel scaling), etc.
  pub fn new_scaled(
    frame: &'a mut [u8],
    frame_width: u32,
    frame_height: u32,
    input: &'a InputState,
    font: &'a BitmapFont,
    ui_scale: u32,
  ) -> Self {
    Self {
      frame,
      frame_width,
      frame_height,
      input,
      font,
      theme: None,
      clip_stack: Vec::with_capacity(MAX_CLIP_STACK_DEPTH),
      current_clip: None,
      pending_dropdowns: Vec::new(),
      escape_consumed: false,
      click_consumed: false,
      focused_dropdown_id: 0,
      ui_scale: ui_scale.max(1),
      mouse_occluded: false,
    }
  }

  /// Create a new UI context with a theme.
  pub fn with_theme(
    frame: &'a mut [u8],
    frame_width: u32,
    frame_height: u32,
    input: &'a InputState,
    font: &'a BitmapFont,
    theme: &'a Theme,
  ) -> Self {
    Self {
      frame,
      frame_width,
      frame_height,
      input,
      font,
      theme: Some(theme),
      clip_stack: Vec::with_capacity(MAX_CLIP_STACK_DEPTH),
      current_clip: None,
      pending_dropdowns: Vec::new(),
      escape_consumed: false,
      click_consumed: false,
      focused_dropdown_id: 0,
      ui_scale: 1,
      mouse_occluded: false,
    }
  }

  /// Create a new UI context with both a theme and scale factor.
  pub fn with_theme_scaled(
    frame: &'a mut [u8],
    frame_width: u32,
    frame_height: u32,
    input: &'a InputState,
    font: &'a BitmapFont,
    theme: &'a Theme,
    ui_scale: u32,
  ) -> Self {
    Self {
      frame,
      frame_width,
      frame_height,
      input,
      font,
      theme: Some(theme),
      clip_stack: Vec::with_capacity(MAX_CLIP_STACK_DEPTH),
      current_clip: None,
      pending_dropdowns: Vec::new(),
      escape_consumed: false,
      click_consumed: false,
      focused_dropdown_id: 0,
      ui_scale: ui_scale.max(1),
      mouse_occluded: false,
    }
  }

  /// Get the current theme, if any.
  pub fn theme(&self) -> Option<&Theme> {
    self.theme
  }

  /// Get the current UI scale factor.
  ///
  /// Returns 1 for normal size, 2 for double size (HiDPI), etc.
  pub fn ui_scale(&self) -> u32 {
    self.ui_scale
  }

  /// Get the screen dimensions.
  pub fn screen_size(&self) -> (u32, u32) {
    (self.frame_width, self.frame_height)
  }

  // =========================================================================
  // Input Consumption
  // =========================================================================

  /// Check if the Escape key was consumed by a widget this frame.
  ///
  /// Use this after calling your UI methods to know whether to handle
  /// Escape yourself (e.g., for scene navigation).
  ///
  /// # Example
  /// ```ignore
  /// // In update():
  /// if ctx.input.key_pressed(KeyCode::Escape) && !self.escape_consumed {
  ///     return SceneAction::Pop;
  /// }
  ///
  /// // In render_ui():
  /// ui.dropdown(&mut self.dropdown, rect, &options);
  /// self.escape_consumed = ui.escape_consumed();
  /// ```
  pub fn escape_consumed(&self) -> bool {
    self.escape_consumed
  }

  /// Check if a mouse click was consumed by a widget this frame.
  ///
  /// Use this to avoid processing clicks on game elements when UI
  /// handled the click.
  pub fn click_consumed(&self) -> bool {
    self.click_consumed
  }

  /// Consume the mouse click for this frame, preventing other widgets from responding.
  pub fn consume_click(&mut self) {
    self.click_consumed = true;
  }

  /// Consume the escape key for this frame, preventing other widgets from responding.
  pub fn consume_escape(&mut self) {
    self.escape_consumed = true;
  }

  /// Mark the mouse as occluded by an overlay (e.g. developer console).
  ///
  /// When set, `is_hovered`, `is_held`, and `is_clicked` all return false,
  /// preventing widgets underneath the overlay from responding to the mouse.
  pub fn set_mouse_occluded(&mut self) {
    self.mouse_occluded = true;
  }

  /// Check if any dropdown is currently open.
  ///
  /// Useful for knowing whether to handle scene-level input.
  pub fn any_dropdown_open(&self) -> bool {
    !self.pending_dropdowns.is_empty()
  }

  /// Get the currently focused dropdown ID (0 = none focused).
  pub fn focused_dropdown(&self) -> u32 {
    self.focused_dropdown_id
  }

  /// Set the currently focused dropdown ID.
  pub fn set_focused_dropdown(&mut self, id: u32) {
    self.focused_dropdown_id = id;
  }

  // =========================================================================
  // Clipping
  // =========================================================================

  /// Push a clip rect onto the stack.
  ///
  /// All subsequent drawing will be clipped to the intersection of the
  /// current clip and this new rect. Use `pop_clip()` to restore the
  /// previous clip state.
  ///
  /// # Panics
  /// Panics if the clip stack exceeds `MAX_CLIP_STACK_DEPTH` (16 levels).
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Create a scroll region
  /// let scroll_area = Rect::new(10.0, 10.0, 200.0, 150.0);
  /// ui.push_clip(scroll_area);
  ///
  /// // Draw clipped content
  /// for (i, item) in items.iter().enumerate() {
  ///     let y = 10.0 + i as f32 * 20.0 - scroll_offset;
  ///     ui.draw_text(item, 15.0, y, Color::WHITE);
  /// }
  ///
  /// ui.pop_clip();
  /// ```
  pub fn push_clip(&mut self, rect: Rect) {
    assert!(
      self.clip_stack.len() < MAX_CLIP_STACK_DEPTH,
      "Clip stack overflow: exceeded {} nested clips",
      MAX_CLIP_STACK_DEPTH
    );

    self.clip_stack.push(rect);
    self.update_current_clip();
  }

  /// Pop the most recent clip rect from the stack.
  ///
  /// Restores the clip state to what it was before the corresponding
  /// `push_clip()` call.
  ///
  /// # Panics
  /// Panics if called when the clip stack is empty.
  pub fn pop_clip(&mut self) {
    assert!(
      !self.clip_stack.is_empty(),
      "Clip stack underflow: pop_clip() called with empty stack"
    );

    self.clip_stack.pop();
    self.update_current_clip();
  }

  /// Get the current effective clip rect, if any.
  ///
  /// Returns `None` if no clip is active (full screen is visible).
  pub fn current_clip(&self) -> Option<Rect> {
    self.current_clip
  }

  /// Check if a clip is currently active.
  pub fn has_clip(&self) -> bool {
    self.current_clip.is_some()
  }

  /// Get the current clip stack depth.
  pub fn clip_depth(&self) -> usize {
    self.clip_stack.len()
  }

  /// Clear all clips from the stack.
  ///
  /// Useful for drawing modal overlays that should ignore parent clips.
  pub fn clear_clips(&mut self) {
    self.clip_stack.clear();
    self.current_clip = None;
  }

  /// Temporarily clear clips, run a closure, then restore.
  ///
  /// Useful for drawing modal backgrounds or overlays.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Draw a dimmed background over everything
  /// ui.with_no_clip(|ui| {
  ///     let (w, h) = ui.screen_size();
  ///     ui.draw_rect(Rect::new(0.0, 0.0, w as f32, h as f32),
  ///                  Color::rgba(0, 0, 0, 128));
  /// });
  /// ```
  pub fn with_no_clip<F, R>(&mut self, f: F) -> R
  where
    F: FnOnce(&mut Self) -> R,
  {
    let saved_stack = std::mem::take(&mut self.clip_stack);
    let saved_clip = self.current_clip.take();

    let result = f(self);

    self.clip_stack = saved_stack;
    self.current_clip = saved_clip;

    result
  }

  /// Recalculate the current effective clip rect.
  fn update_current_clip(&mut self) {
    if self.clip_stack.is_empty() {
      self.current_clip = None;
    } else {
      // Start with first clip rect
      let mut result = self.clip_stack[0];

      // Intersect with all subsequent clips
      for clip in &self.clip_stack[1..] {
        result = Self::intersect_rects(result, *clip);
      }

      // Check if the intersection is valid (non-empty)
      if result.width > 0.0 && result.height > 0.0 {
        self.current_clip = Some(result);
      } else {
        // Empty intersection - nothing visible
        self.current_clip = Some(Rect::new(0.0, 0.0, 0.0, 0.0));
      }
    }
  }

  /// Calculate the intersection of two rectangles.
  fn intersect_rects(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());

    let width = (right - x).max(0.0);
    let height = (bottom - y).max(0.0);

    Rect::new(x, y, width, height)
  }

  /// Apply clipping to a rectangle and return the visible portion.
  ///
  /// Returns `None` if the rect is completely outside the clip area.
  fn clip_rect(&self, rect: Rect) -> Option<Rect> {
    // First clip to screen bounds
    let screen = Rect::new(0.0, 0.0, self.frame_width as f32, self.frame_height as f32);
    let clipped = Self::intersect_rects(rect, screen);

    if clipped.width <= 0.0 || clipped.height <= 0.0 {
      return None;
    }

    // Then clip to current clip rect if any
    if let Some(clip) = self.current_clip {
      let final_rect = Self::intersect_rects(clipped, clip);
      if final_rect.width > 0.0 && final_rect.height > 0.0 {
        Some(final_rect)
      } else {
        None
      }
    } else {
      Some(clipped)
    }
  }

  // =========================================================================
  // Shape Drawing
  // =========================================================================

  /// Draw a filled rectangle.
  ///
  /// The rectangle is automatically clipped to the current clip rect if one
  /// is active, and to screen bounds.
  pub fn draw_rect(&mut self, rect: Rect, color: Color) {
    // Apply clipping
    let Some(clipped) = self.clip_rect(rect) else {
      return; // Completely outside clip area
    };

    let x0 = clipped.x as i32;
    let y0 = clipped.y as i32;
    let x1 = clipped.right() as i32;
    let y1 = clipped.bottom() as i32;

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

  /// Draw a themed panel using the current theme.
  ///
  /// Falls back to default colors if no theme is set.
  pub fn draw_themed_panel(&mut self, rect: Rect) {
    if let Some(theme) = self.theme {
      self.draw_rect(rect, theme.panel.background);
      if let Some(border) = theme.panel.border {
        self.draw_rect_outline(rect, border, theme.panel.border_thickness);
      }
    } else {
      self.draw_panel(rect, Color::PANEL_BG, Some(Color::PANEL_BORDER));
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

  /// Draw a themed progress bar using the current theme.
  ///
  /// Falls back to default colors if no theme is set.
  pub fn draw_themed_progress_bar(&mut self, rect: Rect, progress: f32) {
    if let Some(theme) = self.theme {
      self.draw_progress_bar(
        rect,
        progress,
        theme.progress_bar.fill,
        theme.progress_bar.background,
      );
      if let Some(border) = theme.progress_bar.border {
        self.draw_rect_outline(rect, border, theme.progress_bar.border_thickness);
      }
    } else {
      self.draw_progress_bar(rect, progress, Color::rgb(80, 150, 255), Color::DARK_GRAY);
    }
  }

  // =========================================================================
  // Text Drawing
  // =========================================================================

  /// Draw text at the given position.
  ///
  /// Text is clipped per-pixel to the current clip rect if one is active.
  /// If UI scaling is enabled (ui_scale > 1), text is rendered at the scaled size
  /// using nearest-neighbor interpolation for crisp pixel-art aesthetics.
  pub fn draw_text(&mut self, text: &str, x: f32, y: f32, color: Color) {
    // Quick bounds check if clip is active
    if let Some(clip) = self.current_clip {
      let (text_w, text_h) = self.text_size(text);
      let text_rect = Rect::new(x, y, text_w as f32, text_h as f32);

      // Skip if text is completely outside clip region
      if text_rect.right() < clip.x
        || text_rect.x > clip.right()
        || text_rect.bottom() < clip.y
        || text_rect.y > clip.bottom()
      {
        return;
      }

      // Use per-pixel clipped rendering with scaling
      self.font.draw_text_scaled_clipped(
        self.frame,
        self.frame_width,
        self.frame_height,
        text,
        x as i32,
        y as i32,
        color,
        self.ui_scale,
        clip.x as i32,
        clip.y as i32,
        clip.width as i32,
        clip.height as i32,
      );
    } else {
      self.font.draw_text_scaled(
        self.frame,
        self.frame_width,
        self.frame_height,
        text,
        x as i32,
        y as i32,
        color,
        self.ui_scale,
      );
    }
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

  /// Calculate the pixel size of text (accounting for UI scale).
  pub fn text_size(&self, text: &str) -> (u32, u32) {
    let (w, h) = self.font.text_size(text);
    (w * self.ui_scale, h * self.ui_scale)
  }

  /// Calculate the pixel size of text at base scale (ignoring UI scale).
  pub fn text_size_unscaled(&self, text: &str) -> (u32, u32) {
    self.font.text_size(text)
  }

  /// Wrap text to fit within a maximum width.
  ///
  /// Returns a vector of lines that fit within `max_width` pixels.
  pub fn wrap_text(&self, text: &str, max_width: u32) -> Vec<String> {
    self.font.wrap_text(text, max_width)
  }

  /// Calculate the size of wrapped text.
  pub fn wrapped_text_size(&self, text: &str, max_width: u32) -> (u32, u32) {
    self.font.wrapped_text_size(text, max_width)
  }

  /// Draw text with word wrapping within a maximum width.
  ///
  /// Text is wrapped at word boundaries to fit within `max_width` pixels.
  /// Long words that don't fit are broken across lines.
  pub fn draw_text_wrapped(&mut self, text: &str, x: f32, y: f32, max_width: u32, color: Color) {
    let lines = self.font.wrap_text(text, max_width);
    let line_height = self.font.char_height() as f32;

    for (i, line) in lines.iter().enumerate() {
      let line_y = y + (i as f32 * line_height);
      self.draw_text(line, x, line_y, color);
    }
  }

  /// Draw wrapped text centered within a rectangle.
  ///
  /// Text is word-wrapped to fit the rectangle width, then centered both
  /// horizontally and vertically within the bounds.
  pub fn draw_text_wrapped_centered(&mut self, text: &str, rect: Rect, color: Color) {
    let max_width = rect.width as u32;
    let lines = self.font.wrap_text(text, max_width);
    let line_height = self.font.char_height() as f32;
    let total_height = lines.len() as f32 * line_height;

    let start_y = rect.y + (rect.height - total_height) / 2.0;

    for (i, line) in lines.iter().enumerate() {
      let line_width = self.font.text_width(line) as f32;
      let line_x = rect.x + (rect.width - line_width) / 2.0;
      let line_y = start_y + (i as f32 * line_height);
      self.draw_text(line, line_x, line_y, color);
    }
  }

  // =========================================================================
  // Interactive Widgets
  // =========================================================================

  /// Check if a rectangle is being clicked this frame.
  ///
  /// Returns `true` if the mouse button was just pressed while over the rectangle.
  /// Returns `false` if the mouse is occluded by an overlay.
  pub fn is_clicked(&self, rect: Rect) -> bool {
    if self.mouse_occluded || !self.input.mouse_pressed(MouseButton::Left) {
      return false;
    }
    let (mx, my) = self.input.mouse_position();
    rect.contains(mx, my)
  }

  /// Check if a rectangle is being hovered.
  ///
  /// Returns `false` if the mouse is occluded by an overlay.
  pub fn is_hovered(&self, rect: Rect) -> bool {
    if self.mouse_occluded {
      return false;
    }
    let (mx, my) = self.input.mouse_position();
    rect.contains(mx, my)
  }

  /// Check if a rectangle is being held (mouse down while hovering).
  ///
  /// Returns `false` if the mouse is occluded by an overlay.
  pub fn is_held(&self, rect: Rect) -> bool {
    if self.mouse_occluded || !self.input.mouse_down(MouseButton::Left) {
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
    let clicked = !self.click_consumed && self.is_clicked(rect);
    if clicked {
      self.click_consumed = true;
    }

    // Use theme colors if available, otherwise use defaults
    let (bg_color, border_color, text_color) = if let Some(theme) = self.theme {
      let bg = if held {
        theme.button.background_pressed
      } else if hovered {
        theme.button.background_hovered
      } else {
        theme.button.background
      };
      let border = if hovered {
        theme.button.border_hovered
      } else {
        theme.button.border
      };
      let text = if held {
        Color::LIGHT_GRAY
      } else {
        theme.button.text
      };
      (bg, border, text)
    } else {
      let bg = if held {
        Color::rgb(60, 60, 80)
      } else if hovered {
        Color::rgb(50, 50, 70)
      } else {
        Color::rgb(40, 40, 60)
      };
      let border = if hovered {
        Color::rgb(100, 100, 140)
      } else {
        Color::rgb(80, 80, 100)
      };
      let text = if held {
        Color::LIGHT_GRAY
      } else {
        Color::WHITE
      };
      (bg, border, text)
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
    let clicked = !self.click_consumed && self.is_clicked(rect);
    if clicked {
      self.click_consumed = true;
    }

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
  pub fn draw_sprite(&mut self, sprite: &crate::core::graphics::Sprite, x: f32, y: f32) {
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
    sprite: &crate::core::graphics::Sprite,
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
    // Get colors from theme or use defaults
    let (track_color, fill_color, handle_color, hover_color) = if let Some(theme) = self.theme {
      (
        theme.slider.track_background,
        theme.slider.track_fill,
        theme.slider.handle,
        theme.slider.handle_hovered,
      )
    } else {
      (
        Color::rgb(60, 60, 80),
        Color::rgb(80, 150, 255),
        Color::WHITE,
        Color::rgb(200, 220, 255),
      )
    };

    let mouse_pos = self.input.mouse_position();

    // Interaction area - must click here to start dragging, and stay roughly here to continue
    // Horizontal is generous (allow overshooting ends), vertical is tight (avoid other widgets)
    let interaction_rect = Rect::new(
      rect.x - 20.0,
      rect.y - 15.0,
      rect.width + 40.0,
      rect.height + 30.0,
    );
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

    // Handle dragging - mouse must be within the interaction area
    // (In pure immediate mode, we can't track "which widget started the drag",
    // so we require staying near the slider to avoid affecting other widgets)
    let is_dragging = self.input.mouse_down(MouseButton::Left) && mouse_in_interaction;

    if is_dragging {
      // Calculate new value based on mouse position (clamped to slider bounds)
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
    let handle_visual_color = if mouse_in_interaction || is_dragging {
      hover_color
    } else {
      handle_color
    };
    self.draw_rect(handle_rect, handle_visual_color);

    changed
  }

  // =========================================================================
  // Checkbox Widget
  // =========================================================================

  /// Draw an interactive checkbox.
  ///
  /// Returns true if the checkbox was toggled this frame.
  ///
  /// # Arguments
  /// * `rect` - The bounding box for the checkbox (typically square)
  /// * `checked` - Mutable reference to the checked state
  /// * `label` - Optional label text to display next to the checkbox
  pub fn checkbox(&mut self, rect: Rect, checked: &mut bool, label: Option<&str>) -> bool {
    let hovered = self.is_hovered(rect);
    let clicked = !self.click_consumed && self.is_clicked(rect);
    if clicked {
      self.click_consumed = true;
    }

    // Get colors from theme or use defaults
    let (bg_color, border_color, checkmark_color) = if let Some(theme) = self.theme {
      let bg = if *checked {
        theme.checkbox.background_checked
      } else if hovered {
        theme.checkbox.background_hovered
      } else {
        theme.checkbox.background
      };
      (bg, theme.checkbox.border, theme.checkbox.checkmark)
    } else {
      let bg = if *checked {
        Color::rgb(60, 120, 200)
      } else if hovered {
        Color::rgb(50, 50, 70)
      } else {
        Color::rgb(40, 40, 60)
      };
      (bg, Color::rgb(80, 80, 100), Color::WHITE)
    };

    // Toggle on click
    if clicked {
      *checked = !*checked;
    }

    // Draw checkbox background
    self.draw_panel(rect, bg_color, Some(border_color));

    // Draw checkmark if checked - simple filled inner square
    if *checked {
      let padding = (rect.width.min(rect.height) * 0.25).max(3.0);
      let inner_rect = rect.shrink(padding);
      self.draw_rect(inner_rect, checkmark_color);
    }

    // Draw label if provided
    if let Some(label_text) = label {
      let text_color = self.theme.map(|t| t.label.text).unwrap_or(Color::WHITE);
      let text_x = rect.right() + 8.0;
      let (_, text_h) = self.text_size(label_text);
      let text_y = rect.y + (rect.height - text_h as f32) / 2.0;
      self.draw_text(label_text, text_x, text_y, text_color);
    }

    clicked
  }

  // =========================================================================
  // Toggle Widget
  // =========================================================================

  /// Draw an interactive toggle switch.
  ///
  /// Returns true if the toggle was changed this frame.
  ///
  /// # Arguments
  /// * `rect` - The bounding box for the toggle (width should be ~2x height)
  /// * `on` - Mutable reference to the toggle state
  pub fn toggle(&mut self, rect: Rect, on: &mut bool) -> bool {
    let hovered = self.is_hovered(rect);
    let clicked = !self.click_consumed && self.is_clicked(rect);
    if clicked {
      self.click_consumed = true;
    }

    // Get colors from theme or use defaults
    let (track_color, knob_color) = if let Some(theme) = self.theme {
      let track = if *on {
        theme.toggle.track_on
      } else if hovered {
        theme.toggle.track_hovered
      } else {
        theme.toggle.track_off
      };
      (track, theme.toggle.knob)
    } else {
      let track = if *on {
        Color::rgb(60, 150, 80)
      } else if hovered {
        Color::rgb(70, 70, 90)
      } else {
        Color::rgb(60, 60, 80)
      };
      (track, Color::WHITE)
    };

    // Toggle on click
    if clicked {
      *on = !*on;
    }

    // Draw track (rounded appearance via rect)
    self.draw_rect(rect, track_color);

    // Draw knob
    let knob_size = rect.height - 4.0;
    let knob_x = if *on {
      rect.right() - knob_size - 2.0
    } else {
      rect.x + 2.0
    };
    let knob_rect = Rect::new(knob_x, rect.y + 2.0, knob_size, knob_size);
    self.draw_rect(knob_rect, knob_color);

    clicked
  }

  // =========================================================================
  // Text Input Widget
  // =========================================================================

  /// Draw an interactive text input field.
  ///
  /// This is a retained-mode widget - the `state` persists across frames and
  /// holds the text buffer, cursor position, and selection state.
  ///
  /// Returns `true` if the text was modified this frame.
  ///
  /// # Arguments
  /// * `state` - Mutable reference to the text input state
  /// * `rect` - The bounding box for the text field
  /// * `placeholder` - Text to show when the field is empty
  /// * `dt` - Delta time for cursor blinking
  ///
  /// # Example
  ///
  /// ```ignore
  /// // In your scene struct:
  /// struct MyScene {
  ///     username: TextInputState,
  /// }
  ///
  /// // Each frame:
  /// let rect = Rect::new(100.0, 100.0, 200.0, 30.0);
  /// if ui.text_input(&mut self.username, rect, "Enter name...", dt) {
  ///     println!("Text changed: {}", self.username.text());
  /// }
  /// ```
  pub fn text_input(
    &mut self,
    state: &mut TextInputState,
    rect: Rect,
    placeholder: &str,
    dt: f32,
  ) -> bool {
    // Update blink timer
    state.update_blink(dt);

    // Get padding for click position calculation
    let (pad_x, _pad_y) = self
      .theme
      .map(|t| t.text_input.padding)
      .unwrap_or((8.0, 4.0));
    let char_width = self.font.char_width() as f32;
    let text_x = rect.x + pad_x;

    // Handle focus on click
    let clicked = !self.click_consumed && self.is_clicked(rect);
    let hovered = self.is_hovered(rect);

    if clicked {
      self.click_consumed = true;
      state.set_focused(true);

      // Calculate which character was clicked, accounting for scroll offset
      let mouse_x = self.input.mouse_x();
      let relative_x = mouse_x - text_x + state.scroll_offset();

      if relative_x <= 0.0 {
        // Click before text - position at start
        state.set_cursor_char(0, false);
      } else {
        // Calculate character position from click
        let char_index = (relative_x / char_width).round() as usize;
        let text_char_count = state.text().chars().count();
        state.set_cursor_char(char_index.min(text_char_count), false);
      }
    } else if state.is_focused()
      && self.input.mouse_down(MouseButton::Left)
      && !self.input.mouse_pressed(MouseButton::Left)
    {
      // Drag-to-select: mouse held after initial click, extend selection
      let mouse_x = self.input.mouse_x();
      let relative_x = mouse_x - text_x + state.scroll_offset();
      let char_index = if relative_x <= 0.0 {
        0
      } else {
        let idx = (relative_x / char_width).round() as usize;
        idx.min(state.text().chars().count())
      };
      state.set_cursor_char(char_index, true); // true = extend selection from anchor
    } else if self.input.mouse_pressed(MouseButton::Left) && !hovered {
      // Click outside always unfocuses, even if click was consumed by another widget.
      // This prevents dual-focus when clicking between text inputs rendered in any order.
      state.set_focused(false);
    }

    // Handle keyboard input
    let modified = state.handle_input(self.input);

    // Get colors from theme or use defaults
    let (bg_color, border_color, text_color, placeholder_color, cursor_color, selection_color) =
      if let Some(theme) = self.theme {
        let bg = if state.is_focused() {
          theme.text_input.background_focused
        } else {
          theme.text_input.background
        };
        let border = if state.is_focused() {
          theme.text_input.border_focused
        } else {
          theme.text_input.border
        };
        (
          bg,
          border,
          theme.text_input.text,
          theme.text_input.placeholder,
          theme.text_input.cursor,
          theme.text_input.selection,
        )
      } else {
        let bg = if state.is_focused() {
          Color::rgb(35, 35, 50)
        } else {
          Color::rgb(30, 30, 45)
        };
        let border = if state.is_focused() {
          Color::rgb(100, 140, 200)
        } else {
          Color::rgb(70, 70, 90)
        };
        (
          bg,
          border,
          Color::WHITE,
          Color::GRAY,
          Color::WHITE,
          Color::new(100, 140, 200, 100),
        )
      };

    // Get padding from theme or default
    let (pad_x, pad_y) = self
      .theme
      .map(|t| t.text_input.padding)
      .unwrap_or((8.0, 4.0));
    let border_thickness = self
      .theme
      .map(|t| t.text_input.border_thickness)
      .unwrap_or(1.0);

    // Draw background and border
    self.draw_rect(rect, bg_color);
    self.draw_rect_outline(rect, border_color, border_thickness);

    // Calculate text area (inside padding)
    let text_x = rect.x + pad_x;
    let text_y = rect.y + (rect.height - self.font.char_height() as f32) / 2.0;
    let text_width = rect.width - pad_x * 2.0;

    // Clip to text area for overflow handling
    let text_area = Rect::new(
      text_x,
      rect.y + pad_y,
      text_width,
      rect.height - pad_y * 2.0,
    );
    self.push_clip(text_area);

    let char_width = self.font.char_width() as f32;

    // Ensure cursor is visible (adjusts scroll offset for overflow)
    state.ensure_cursor_visible(text_width, char_width);
    let scroll = state.scroll_offset();
    let text = state.text();

    // Draw selection highlight if any
    if let Some((sel_start, sel_end)) = state.selection_range() {
      // Calculate pixel positions for selection
      let start_chars = text[..sel_start].chars().count();
      let end_chars = text[..sel_end].chars().count();
      let sel_x = text_x + start_chars as f32 * char_width - scroll;
      let sel_width = (end_chars - start_chars) as f32 * char_width;
      let sel_rect = Rect::new(sel_x, text_y, sel_width, self.font.char_height() as f32);
      self.draw_rect(sel_rect, selection_color);
    }

    // Draw text or placeholder
    if text.is_empty() {
      self.draw_text(placeholder, text_x, text_y, placeholder_color);
    } else {
      // Draw the text content, shifted by scroll offset
      self.draw_text(text, text_x - scroll, text_y, text_color);

      // Draw IME preedit text at cursor if composing
      let preedit = self.input.ime_preedit();
      if !preedit.is_empty() {
        let cursor_chars = text[..state.cursor()].chars().count();
        let preedit_x = text_x + cursor_chars as f32 * char_width - scroll;
        // Draw preedit with underline-style indicator
        self.draw_text(preedit, preedit_x, text_y, text_color);
        // Draw underline for preedit text
        let preedit_width = preedit.chars().count() as f32 * char_width;
        self.draw_rect(
          Rect::new(
            preedit_x,
            text_y + self.font.char_height() as f32 - 1.0,
            preedit_width,
            1.0,
          ),
          cursor_color,
        );
      }
    }

    // Draw cursor if focused and visible (blinking)
    if state.should_draw_cursor() && !state.has_selection() {
      let cursor_chars = text[..state.cursor()].chars().count();
      // Account for preedit text when positioning cursor
      let preedit_chars = self.input.ime_preedit().chars().count();
      let cursor_x = text_x + (cursor_chars + preedit_chars) as f32 * char_width - scroll;
      let cursor_rect = Rect::new(cursor_x, text_y, 2.0, self.font.char_height() as f32);
      self.draw_rect(cursor_rect, cursor_color);
    }

    self.pop_clip();

    modified
  }

  // =========================================================================
  // Scroll View Widget
  // =========================================================================

  /// Draw a scrollable content area.
  ///
  /// This widget provides a scrollable container with automatic scrollbar.
  /// The content is drawn by the provided closure, which receives the
  /// current scroll offset.
  ///
  /// # Arguments
  /// * `state` - Mutable reference to the scroll view state (retained across frames)
  /// * `rect` - The visible area of the scroll view
  /// * `content_height` - Total height of the scrollable content
  /// * `dt` - Delta time for momentum/animation
  /// * `content_fn` - Closure that draws the content, receives (ui, scroll_offset)
  ///
  /// # Returns
  /// `true` if the scroll position changed this frame
  ///
  /// # Example
  ///
  /// ```ignore
  /// let content_height = items.len() as f32 * 30.0;
  /// let view_rect = Rect::new(20.0, 50.0, 300.0, 400.0);
  ///
  /// ui.scroll_view(&mut self.scroll_state, view_rect, content_height, dt, |ui, scroll_y| {
  ///     for (i, item) in items.iter().enumerate() {
  ///         let y = 50.0 + i as f32 * 30.0 - scroll_y;
  ///         ui.draw_text(item, 30.0, y, Color::WHITE);
  ///     }
  /// });
  /// ```
  pub fn scroll_view<F>(
    &mut self,
    state: &mut ScrollViewState,
    rect: Rect,
    content_height: f32,
    dt: f32,
    content_fn: F,
  ) -> bool
  where
    F: FnOnce(&mut Self, f32),
  {
    // Constants for scrollbar
    const SCROLLBAR_WIDTH: f32 = 12.0;
    const SCROLLBAR_PADDING: f32 = 2.0;
    const SCROLLBAR_THUMB_MIN: f32 = 20.0;

    let view_height = rect.height;
    let needs_scrollbar = content_height > view_height;

    // Calculate content area (leave room for scrollbar if needed)
    let content_width = if needs_scrollbar {
      rect.width - SCROLLBAR_WIDTH
    } else {
      rect.width
    };
    let content_rect = Rect::new(rect.x, rect.y, content_width, rect.height);

    // Scrollbar dimensions
    let scrollbar_rect = Rect::new(rect.x + content_width, rect.y, SCROLLBAR_WIDTH, rect.height);
    let track_rect = Rect::new(
      scrollbar_rect.x + SCROLLBAR_PADDING,
      scrollbar_rect.y + SCROLLBAR_PADDING,
      SCROLLBAR_WIDTH - SCROLLBAR_PADDING * 2.0,
      scrollbar_rect.height - SCROLLBAR_PADDING * 2.0,
    );

    // Calculate thumb position
    let (thumb_y, thumb_height) =
      state.calculate_thumb(view_height, content_height, track_rect.height);
    let thumb_height = thumb_height.max(SCROLLBAR_THUMB_MIN);
    let thumb_rect = (
      track_rect.x,
      track_rect.y + thumb_y,
      track_rect.width,
      thumb_height,
    );

    // Check hover states
    let content_hovered = self.is_hovered(content_rect);
    let scrollbar_hovered = self.is_hovered(scrollbar_rect);

    // Handle input
    let scrollbar_thumb_rect = if needs_scrollbar {
      Some(thumb_rect)
    } else {
      None
    };
    let changed = state.handle_input(
      self.input,
      view_height,
      content_height,
      content_hovered || scrollbar_hovered,
      scrollbar_hovered,
      scrollbar_thumb_rect,
      dt,
    );

    // Get colors from theme or use defaults
    let (bg_color, scrollbar_bg, scrollbar_thumb, scrollbar_thumb_hover) =
      if let Some(theme) = self.theme {
        (
          theme.scrollbar.track,
          theme.scrollbar.track,
          theme.scrollbar.thumb,
          theme.scrollbar.thumb_hovered,
        )
      } else {
        (
          Color::rgb(30, 30, 40),
          Color::rgb(40, 40, 55),
          Color::rgb(80, 80, 100),
          Color::rgb(100, 100, 130),
        )
      };

    // Draw background
    self.draw_rect(rect, bg_color);

    // Draw content with clipping
    self.push_clip(content_rect);
    content_fn(self, state.scroll_y());
    self.pop_clip();

    // Draw scrollbar if needed
    if needs_scrollbar {
      // Scrollbar track
      self.draw_rect(scrollbar_rect, scrollbar_bg);

      // Scrollbar thumb
      let thumb_color = if state.is_dragging() || scrollbar_hovered {
        scrollbar_thumb_hover
      } else {
        scrollbar_thumb
      };
      let thumb_draw_rect = Rect::new(thumb_rect.0, thumb_rect.1, thumb_rect.2, thumb_rect.3);
      self.draw_rect(thumb_draw_rect, thumb_color);
    }

    changed
  }

  /// A scrollable container widget with horizontal scrolling.
  ///
  /// Similar to `scroll_view` but scrolls horizontally instead of vertically.
  /// The scrollbar appears at the bottom of the view.
  ///
  /// # Arguments
  /// * `state` - The retained scroll state (must be stored between frames)
  /// * `rect` - The visible viewport rectangle
  /// * `content_width` - Total width of the scrollable content
  /// * `dt` - Delta time for momentum physics
  /// * `content_fn` - Closure that draws content, receives `scroll_x` offset
  ///
  /// # Returns
  /// `true` if the scroll position changed this frame
  ///
  /// # Example
  /// ```ignore
  /// ui.scroll_view_horizontal(&mut scroll_state, rect, 2000.0, dt, |ui, scroll_x| {
  ///     for i in 0..20 {
  ///         let x = 10.0 + i as f32 * 100.0 - scroll_x;
  ///         ui.draw_rect(Rect::new(x, 10.0, 80.0, 80.0), Color::BLUE);
  ///     }
  /// });
  /// ```
  pub fn scroll_view_horizontal<F>(
    &mut self,
    state: &mut ScrollViewState,
    rect: Rect,
    content_width: f32,
    dt: f32,
    content_fn: F,
  ) -> bool
  where
    F: FnOnce(&mut Self, f32),
  {
    // Constants for scrollbar
    const SCROLLBAR_HEIGHT: f32 = 12.0;
    const SCROLLBAR_PADDING: f32 = 2.0;
    const SCROLLBAR_THUMB_MIN: f32 = 20.0;

    let view_width = rect.width;
    let needs_scrollbar = content_width > view_width;

    // Calculate content area (leave room for scrollbar if needed)
    let content_height = if needs_scrollbar {
      rect.height - SCROLLBAR_HEIGHT
    } else {
      rect.height
    };
    let content_rect = Rect::new(rect.x, rect.y, rect.width, content_height);

    // Scrollbar dimensions (at bottom)
    let scrollbar_rect = Rect::new(
      rect.x,
      rect.y + content_height,
      rect.width,
      SCROLLBAR_HEIGHT,
    );
    let track_rect = Rect::new(
      scrollbar_rect.x + SCROLLBAR_PADDING,
      scrollbar_rect.y + SCROLLBAR_PADDING,
      scrollbar_rect.width - SCROLLBAR_PADDING * 2.0,
      SCROLLBAR_HEIGHT - SCROLLBAR_PADDING * 2.0,
    );

    // Calculate thumb position (using scroll_x for horizontal)
    let scroll_x = state.scroll_x();
    let thumb_ratio = view_width / content_width;
    let thumb_width = (track_rect.width * thumb_ratio).max(SCROLLBAR_THUMB_MIN);
    let max_scroll = (content_width - view_width).max(0.0);
    let scroll_ratio = if max_scroll > 0.0 {
      scroll_x / max_scroll
    } else {
      0.0
    };
    let thumb_x = scroll_ratio * (track_rect.width - thumb_width);
    let thumb_rect = (
      track_rect.x + thumb_x,
      track_rect.y,
      thumb_width,
      track_rect.height,
    );

    // Check hover states
    let content_hovered = self.is_hovered(content_rect);
    let scrollbar_hovered = self.is_hovered(scrollbar_rect);

    // Handle input for horizontal scrolling
    let scrollbar_thumb_rect = if needs_scrollbar {
      Some(thumb_rect)
    } else {
      None
    };
    let changed = state.handle_input_horizontal(
      self.input,
      view_width,
      content_width,
      content_hovered || scrollbar_hovered,
      scrollbar_hovered,
      scrollbar_thumb_rect,
      dt,
    );

    // Get colors from theme or use defaults
    let (bg_color, scrollbar_bg, scrollbar_thumb, scrollbar_thumb_hover) =
      if let Some(theme) = self.theme {
        (
          theme.scrollbar.track,
          theme.scrollbar.track,
          theme.scrollbar.thumb,
          theme.scrollbar.thumb_hovered,
        )
      } else {
        (
          Color::rgb(30, 30, 40),
          Color::rgb(40, 40, 55),
          Color::rgb(80, 80, 100),
          Color::rgb(100, 100, 130),
        )
      };

    // Draw background
    self.draw_rect(rect, bg_color);

    // Draw content with clipping
    self.push_clip(content_rect);
    content_fn(self, state.scroll_x());
    self.pop_clip();

    // Draw scrollbar if needed
    if needs_scrollbar {
      // Scrollbar track
      self.draw_rect(scrollbar_rect, scrollbar_bg);

      // Scrollbar thumb
      let thumb_color = if state.is_dragging() || scrollbar_hovered {
        scrollbar_thumb_hover
      } else {
        scrollbar_thumb
      };
      let thumb_draw_rect = Rect::new(thumb_rect.0, thumb_rect.1, thumb_rect.2, thumb_rect.3);
      self.draw_rect(thumb_draw_rect, thumb_color);
    }

    changed
  }

  // =========================================================================
  // Dropdown Widget
  // =========================================================================

  /// A dropdown selector widget for choosing from a list of options.
  ///
  /// This is a retained-mode widget - the state persists between frames.
  /// When open, the dropdown draws above other content using the modal overlay.
  ///
  /// # Arguments
  /// * `state` - The retained dropdown state (must be stored between frames)
  /// * `rect` - The trigger button rectangle (closed dropdown size)
  /// * `options` - Slice of option labels to display
  ///
  /// # Returns
  /// `true` if the selection changed this frame
  ///
  /// # Input Consumption
  /// When a dropdown is open:
  /// - Escape key is consumed (check `ui.escape_consumed()` afterwards)
  /// - Clicks on the dropdown list are consumed (check `ui.click_consumed()`)
  ///
  /// # Tab Navigation
  /// If the dropdown has an ID > 0, Tab/Shift+Tab can navigate between dropdowns.
  /// Space or Enter opens a focused dropdown.
  ///
  /// # Note
  /// Call `finish_dropdowns()` at the end of your `render_ui` to draw all open
  /// dropdown lists on top of other content.
  ///
  /// # Example
  /// ```ignore
  /// let options = ["Easy", "Medium", "Hard"];
  /// if ui.dropdown(&mut self.difficulty, rect, &options) {
  ///     println!("Selected: {}", options[self.difficulty.selected()]);
  /// }
  ///
  /// // At end of render_ui:
  /// ui.finish_dropdowns();
  /// ```
  pub fn dropdown(&mut self, state: &mut DropdownState, rect: Rect, options: &[&str]) -> bool {
    if options.is_empty() {
      return false;
    }

    let old_selected = state.selected();

    // Get colors from theme or use defaults
    let (bg_color, bg_hover, text_color, border_color, focus_color) =
      if let Some(theme) = self.theme {
        (
          theme.button.background,
          theme.button.background_hovered,
          theme.button.text,
          theme.panel.border.unwrap_or(Color::rgb(60, 60, 80)),
          theme.text_input.border_focused,
        )
      } else {
        (
          Color::rgb(50, 50, 65),
          Color::rgb(65, 65, 85),
          Color::WHITE,
          Color::rgb(80, 80, 100),
          Color::rgb(100, 150, 255),
        )
      };

    let trigger_hovered = self.is_hovered(rect);
    let mouse_pressed = self.input.mouse_pressed(MouseButton::Left) && !self.click_consumed;
    let mouse_x = self.input.mouse_x();
    let mouse_y = self.input.mouse_y();

    // Handle focus via click on trigger - this dropdown becomes focused
    if trigger_hovered && mouse_pressed {
      state.set_focused(true);
      // Track this as the focused dropdown so we can unfocus others
      if state.id() > 0 {
        self.focused_dropdown_id = state.id();
      }
    }

    // Sync focus: if another dropdown was clicked this frame, unfocus this one
    // This ensures only one dropdown has focus at a time
    if state.id() > 0 && self.focused_dropdown_id > 0 && self.focused_dropdown_id != state.id() {
      state.set_focused(false);
    }

    // Determine if we should show focus ring
    let show_focus = state.is_focused();

    // Draw the trigger button
    let trigger_bg = if trigger_hovered || state.is_open() {
      bg_hover
    } else {
      bg_color
    };
    self.draw_rect(rect, trigger_bg);

    // Draw focus ring if focused
    if show_focus {
      self.draw_rect_outline(rect, focus_color, 2.0);
    } else {
      self.draw_rect_outline(rect, border_color, 1.0);
    }

    // Draw selected option text
    let selected_text = if state.selected() < options.len() {
      options[state.selected()]
    } else {
      ""
    };
    let text_x = rect.x + 8.0;
    let text_y = rect.y + (rect.height - 8.0) / 2.0; // Center vertically (assuming ~8px font)
    self.draw_text(selected_text, text_x, text_y, text_color);

    // Draw dropdown arrow
    let arrow_size = 6.0;
    let arrow_x = rect.x + rect.width - 16.0;
    let arrow_y = rect.y + rect.height / 2.0;
    self.draw_dropdown_arrow(arrow_x, arrow_y, arrow_size, text_color, state.is_open());

    // Handle trigger click to toggle dropdown
    if trigger_hovered && mouse_pressed {
      state.toggle();
      self.click_consumed = true;
    }

    // Note: Space/Enter to open focused dropdown is handled in handle_dropdown_tab_navigation()
    // which runs after all dropdowns are processed, ensuring focus state is correct.

    // Handle keyboard when open
    if state.is_open() {
      let max_index = options.len().saturating_sub(1);

      if self.input.key_pressed(winit::keyboard::KeyCode::ArrowUp) {
        state.hover_up();
      }
      if self.input.key_pressed(winit::keyboard::KeyCode::ArrowDown) {
        state.hover_down(max_index);
      }
      // Both Enter and Space select the hovered item
      if self.input.key_pressed(winit::keyboard::KeyCode::Enter)
        || self.input.key_pressed(winit::keyboard::KeyCode::Space)
      {
        state.select_hovered();
      }
      if self.input.key_pressed(winit::keyboard::KeyCode::Escape) {
        state.close();
        self.escape_consumed = true;
      }

      // Calculate list rect for click-outside detection
      let item_height = rect.height;
      let list_height = options.len() as f32 * item_height;
      let list_rect = Rect::new(rect.x, rect.y + rect.height, rect.width, list_height);

      // Check if click is outside the dropdown entirely
      let full_dropdown = Rect::new(rect.x, rect.y, rect.width, rect.height + list_height);
      if mouse_pressed && !full_dropdown.contains(mouse_x, mouse_y) {
        state.close();
      }

      // Handle clicks on options (must be done here for state mutation)
      for (i, _) in options.iter().enumerate() {
        let option_rect = Rect::new(
          rect.x,
          rect.y + rect.height + i as f32 * item_height,
          rect.width,
          item_height,
        );
        let option_hovered = option_rect.contains(mouse_x, mouse_y);

        if option_hovered && mouse_pressed {
          state.set_selected(i);
          state.close();
          self.click_consumed = true;
        }
      }

      // Consume click if anywhere on the dropdown to prevent passthrough
      if mouse_pressed && full_dropdown.contains(mouse_x, mouse_y) {
        self.click_consumed = true;
      }

      // Queue the dropdown list to be drawn later (on top of everything)
      // Only add if still open after handling clicks
      if state.is_open() {
        self.pending_dropdowns.push(PendingDropdown {
          trigger_rect: rect,
          list_rect,
          hover_index: state.hover_index(),
          options: options.iter().map(|s| s.to_string()).collect(),
        });
      }
    }

    state.selected() != old_selected
  }

  /// Handle Tab navigation between multiple dropdowns.
  ///
  /// Call this once per frame after all dropdowns to enable Tab/Shift+Tab
  /// navigation. Provide all dropdown states in tab order.
  ///
  /// # Example
  /// ```ignore
  /// ui.dropdown(&mut self.dropdown1, rect1, &options1);
  /// ui.dropdown(&mut self.dropdown2, rect2, &options2);
  /// ui.dropdown(&mut self.dropdown3, rect3, &options3);
  ///
  /// // Enable tab navigation
  /// ui.handle_dropdown_tab_navigation(&mut [
  ///     &mut self.dropdown1,
  ///     &mut self.dropdown2,
  ///     &mut self.dropdown3,
  /// ]);
  /// ```
  pub fn handle_dropdown_tab_navigation(&mut self, dropdowns: &mut [&mut DropdownState]) {
    if dropdowns.is_empty() {
      return;
    }

    // Clear just_closed flags at the start of each frame's handling
    // This must happen BEFORE we check for open key, so a dropdown closed
    // on a previous frame doesn't block opening on this frame
    for dropdown in dropdowns.iter_mut() {
      if dropdown.was_just_closed() {
        // Don't clear yet - we'll use this to prevent re-open this frame
      }
    }

    // If a dropdown was clicked this frame (focused_dropdown_id was set),
    // ensure only that one has focus
    if self.focused_dropdown_id > 0 {
      for dropdown in dropdowns.iter_mut() {
        let should_focus = dropdown.id() == self.focused_dropdown_id;
        dropdown.set_focused(should_focus);
      }
    }

    // Don't do keyboard navigation if any dropdown is open
    if dropdowns.iter().any(|d| d.is_open()) {
      return;
    }

    // Handle Space/Enter to open the focused dropdown
    // This must be done HERE because focus may have been set by Tab on a previous frame
    let open_key = self.input.key_pressed(winit::keyboard::KeyCode::Space)
      || self.input.key_pressed(winit::keyboard::KeyCode::Enter);
    if open_key {
      for dropdown in dropdowns.iter_mut() {
        // Don't re-open if this dropdown was just closed this frame (by selecting an item)
        if dropdown.is_focused() && !dropdown.was_just_closed() {
          dropdown.open();
          return; // Only open one
        }
      }
    }

    // Clear just_closed flags at the END so they persist for one frame
    for dropdown in dropdowns.iter_mut() {
      dropdown.clear_just_closed();
    }

    let tab_pressed = self.input.key_pressed(winit::keyboard::KeyCode::Tab);
    if !tab_pressed {
      return;
    }

    let shift_held = self.input.key_down(winit::keyboard::KeyCode::ShiftLeft)
      || self.input.key_down(winit::keyboard::KeyCode::ShiftRight);

    // Find currently focused dropdown index
    let current_idx = dropdowns.iter().position(|d| d.is_focused());

    let next_idx = match current_idx {
      Some(idx) => {
        if shift_held {
          // Shift+Tab: go backwards
          if idx == 0 {
            dropdowns.len() - 1
          } else {
            idx - 1
          }
        } else {
          // Tab: go forwards
          (idx + 1) % dropdowns.len()
        }
      }
      None => {
        // Nothing focused, start at first (or last if shift)
        if shift_held { dropdowns.len() - 1 } else { 0 }
      }
    };

    // Update focus
    for (i, dropdown) in dropdowns.iter_mut().enumerate() {
      dropdown.set_focused(i == next_idx);
    }
  }

  /// Draw all pending dropdown lists on top of other content.
  ///
  /// Call this at the end of your `render_ui` function to ensure dropdown
  /// lists appear above other UI elements.
  ///
  /// # Example
  /// ```ignore
  /// fn render_ui(&mut self, ui: &mut UiContext) {
  ///     // Draw all your UI...
  ///     ui.dropdown(&mut self.dropdown1, rect1, &options1);
  ///     ui.dropdown(&mut self.dropdown2, rect2, &options2);
  ///     
  ///     // Draw dropdown overlays last
  ///     ui.finish_dropdowns();
  /// }
  /// ```
  pub fn finish_dropdowns(&mut self) {
    // Take pending dropdowns to avoid borrow issues
    let dropdowns = std::mem::take(&mut self.pending_dropdowns);

    if dropdowns.is_empty() {
      return;
    }

    // Get colors from theme
    let (text_color, border_color, option_bg, option_hover) = if let Some(theme) = self.theme {
      (
        theme.button.text,
        theme.panel.border.unwrap_or(Color::rgb(60, 60, 80)),
        theme.panel.background,
        theme.button.background_hovered,
      )
    } else {
      (
        Color::WHITE,
        Color::rgb(80, 80, 100),
        Color::rgb(40, 40, 55),
        Color::rgb(60, 60, 80),
      )
    };

    let mouse_x = self.input.mouse_x();
    let mouse_y = self.input.mouse_y();

    // Draw each pending dropdown list
    for dropdown in dropdowns {
      let item_height = dropdown.trigger_rect.height;

      // Draw list background
      self.draw_rect(dropdown.list_rect, option_bg);
      self.draw_rect_outline(dropdown.list_rect, border_color, 1.0);

      // Draw options
      for (i, option) in dropdown.options.iter().enumerate() {
        let option_rect = Rect::new(
          dropdown.trigger_rect.x,
          dropdown.trigger_rect.y + dropdown.trigger_rect.height + i as f32 * item_height,
          dropdown.trigger_rect.width,
          item_height,
        );

        let option_hovered = option_rect.contains(mouse_x, mouse_y);
        let is_keyboard_hover = i == dropdown.hover_index;

        // Highlight hovered or keyboard-selected option
        if option_hovered || is_keyboard_hover {
          self.draw_rect(option_rect, option_hover);
        }

        // Draw option text
        let opt_text_y = option_rect.y + (option_rect.height - 8.0) / 2.0;
        self.draw_text(option, option_rect.x + 8.0, opt_text_y, text_color);
      }
    }
  }

  /// Draw a dropdown arrow indicator.
  fn draw_dropdown_arrow(&mut self, x: f32, y: f32, size: f32, color: Color, pointing_up: bool) {
    // Draw a simple triangle arrow
    // For now, we'll draw it using small rectangles (since we don't have triangle primitives)
    let half = size / 2.0;
    if pointing_up {
      // Arrow pointing up (dropdown is open)
      self.draw_rect(Rect::new(x - half, y, size, 2.0), color);
      self.draw_rect(Rect::new(x - half + 1.0, y - 2.0, size - 2.0, 2.0), color);
      self.draw_rect(Rect::new(x - half + 2.0, y - 4.0, size - 4.0, 2.0), color);
    } else {
      // Arrow pointing down (dropdown is closed)
      self.draw_rect(Rect::new(x - half, y - 2.0, size, 2.0), color);
      self.draw_rect(Rect::new(x - half + 1.0, y, size - 2.0, 2.0), color);
      self.draw_rect(Rect::new(x - half + 2.0, y + 2.0, size - 4.0, 2.0), color);
    }
  }

  // =========================================================================
  // Modal & Overlay Support
  // =========================================================================

  /// Draw a modal overlay that dims the background.
  ///
  /// This clears all current clips and draws a semi-transparent rectangle
  /// over the entire screen. Useful as a backdrop for modal dialogs.
  ///
  /// # Arguments
  /// * `dim_color` - The color to overlay (typically semi-transparent black)
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Dim the background
  /// ui.draw_modal_overlay(Color::rgba(0, 0, 0, 128));
  ///
  /// // Draw the modal dialog on top
  /// let dialog = Rect::centered_at(center_x, center_y, 300.0, 200.0);
  /// ui.draw_panel(dialog, Color::rgb(40, 40, 50), Some(Color::WHITE));
  /// ui.draw_text_centered("Confirm?", dialog, Color::WHITE);
  /// ```
  pub fn draw_modal_overlay(&mut self, dim_color: Color) {
    self.with_no_clip(|ui| {
      let (w, h) = ui.screen_size();
      ui.draw_rect(Rect::new(0.0, 0.0, w as f32, h as f32), dim_color);
    });
  }

  /// Check if a point is within the current clip region.
  ///
  /// Returns `true` if no clip is active or if the point is inside the clip.
  pub fn point_in_clip(&self, x: f32, y: f32) -> bool {
    match self.current_clip {
      Some(clip) => clip.contains(x, y),
      None => {
        // Check screen bounds
        x >= 0.0 && y >= 0.0 && x < self.frame_width as f32 && y < self.frame_height as f32
      }
    }
  }

  // =========================================================================
  // Focus Management
  // =========================================================================

  /// Handle Tab/Shift+Tab navigation between focusable widgets.
  ///
  /// Call this once per frame after all widgets to enable Tab navigation
  /// across different widget types (text inputs, dropdowns, etc.).
  ///
  /// # Arguments
  /// * `widgets` - Slice of focusable widgets in tab order
  ///
  /// # Example
  /// ```ignore
  /// ui.text_input(&mut self.username, rect1, "Username", dt);
  /// ui.text_input(&mut self.password, rect2, "Password", dt);
  /// ui.dropdown(&mut self.role, rect3, &roles);
  ///
  /// // Enable tab navigation across all widgets
  /// ui.handle_tab_navigation(&mut [
  ///     &mut self.username,
  ///     &mut self.password,
  ///     &mut self.role,
  /// ]);
  /// ```
  pub fn handle_tab_navigation(&mut self, widgets: &mut [&mut dyn Focusable]) {
    if widgets.is_empty() {
      return;
    }

    // Don't do keyboard navigation if any widget is capturing input
    // (e.g., open dropdown handling arrow keys)
    if widgets.iter().any(|w| w.is_capturing()) {
      return;
    }

    let tab_pressed = self.input.key_pressed(winit::keyboard::KeyCode::Tab);
    if !tab_pressed {
      return;
    }

    let shift_held = self.input.key_down(winit::keyboard::KeyCode::ShiftLeft)
      || self.input.key_down(winit::keyboard::KeyCode::ShiftRight);

    // Find currently focused widget index
    let current_idx = widgets.iter().position(|w| w.is_focused());

    let next_idx = match current_idx {
      Some(idx) => {
        if shift_held {
          // Shift+Tab: go backwards (wrap around)
          if idx == 0 { widgets.len() - 1 } else { idx - 1 }
        } else {
          // Tab: go forwards (wrap around)
          (idx + 1) % widgets.len()
        }
      }
      None => {
        // Nothing focused, start at first (or last if shift)
        if shift_held { widgets.len() - 1 } else { 0 }
      }
    };

    // Blur current widget
    if let Some(idx) = current_idx {
      widgets[idx].set_focused(false);
      widgets[idx].on_blur();
    }

    // Focus next widget
    widgets[next_idx].set_focused(true);
    widgets[next_idx].on_focus();
  }

  /// Draw a focus ring (outline) around a rectangle.
  ///
  /// Use this to indicate which widget currently has keyboard focus.
  /// The ring is drawn outside the widget bounds.
  ///
  /// # Arguments
  /// * `rect` - The widget bounds to draw around
  /// * `color` - The focus ring color (typically a bright accent color)
  /// * `thickness` - Ring thickness in pixels (typically 2.0)
  /// * `offset` - Gap between widget and ring (typically 1.0-2.0)
  ///
  /// # Example
  /// ```ignore
  /// if self.button_focused {
  ///     ui.draw_focus_ring(button_rect, Color::rgb(100, 150, 255), 2.0, 2.0);
  /// }
  /// ui.button("Submit", button_rect);
  /// ```
  pub fn draw_focus_ring(&mut self, rect: Rect, color: Color, thickness: f32, offset: f32) {
    let expanded = rect.expand(offset + thickness);
    let inner = rect.expand(offset);

    // Draw the four sides of the ring
    // Top
    self.draw_rect(
      Rect::new(expanded.x, expanded.y, expanded.width, thickness),
      color,
    );
    // Bottom
    self.draw_rect(
      Rect::new(
        expanded.x,
        inner.y + inner.height,
        expanded.width,
        thickness,
      ),
      color,
    );
    // Left
    self.draw_rect(
      Rect::new(expanded.x, inner.y, thickness, inner.height),
      color,
    );
    // Right
    self.draw_rect(
      Rect::new(inner.x + inner.width, inner.y, thickness, inner.height),
      color,
    );
  }

  /// Draw a focus ring using theme colors.
  ///
  /// Convenience method that uses the theme's text input focus color.
  pub fn draw_focus_ring_themed(&mut self, rect: Rect) {
    let color = self
      .theme
      .map(|t| t.text_input.border_focused)
      .unwrap_or(Color::rgb(100, 150, 255));
    self.draw_focus_ring(rect, color, 2.0, 1.0);
  }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
  use super::*;
  use winit::event::ElementState;

  fn create_test_context(width: u32, height: u32, frame: &mut [u8]) -> UiContext<'_> {
    let input = InputState::new();
    // Leak the input to get a static reference for testing
    let input_ref: &'static InputState = Box::leak(Box::new(input));
    let font = super::super::font::create_debug_font();
    let font_ref: &'static BitmapFont = Box::leak(Box::new(font));

    UiContext::new(frame, width, height, input_ref, font_ref)
  }

  #[test]
  fn clip_stack_push_pop() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let mut ui = create_test_context(100, 100, &mut frame);

    assert_eq!(ui.clip_depth(), 0);
    assert!(!ui.has_clip());

    ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
    assert_eq!(ui.clip_depth(), 1);
    assert!(ui.has_clip());
    assert_eq!(ui.current_clip(), Some(Rect::new(10.0, 10.0, 50.0, 50.0)));

    ui.pop_clip();
    assert_eq!(ui.clip_depth(), 0);
    assert!(!ui.has_clip());
  }

  #[test]
  fn nested_clips_intersect() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let mut ui = create_test_context(100, 100, &mut frame);

    // First clip: 10,10 to 60,60
    ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
    // Second clip: 30,30 to 80,80
    ui.push_clip(Rect::new(30.0, 30.0, 50.0, 50.0));

    // Intersection should be: 30,30 to 60,60 (30x30)
    let clip = ui.current_clip().unwrap();
    assert_eq!(clip.x, 30.0);
    assert_eq!(clip.y, 30.0);
    assert_eq!(clip.width, 30.0);
    assert_eq!(clip.height, 30.0);
  }

  #[test]
  fn clear_clips_resets_stack() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let mut ui = create_test_context(100, 100, &mut frame);

    ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
    ui.push_clip(Rect::new(20.0, 20.0, 30.0, 30.0));
    assert_eq!(ui.clip_depth(), 2);

    ui.clear_clips();
    assert_eq!(ui.clip_depth(), 0);
    assert!(!ui.has_clip());
  }

  #[test]
  fn with_no_clip_restores_state() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let mut ui = create_test_context(100, 100, &mut frame);

    ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
    let original_clip = ui.current_clip();

    let inside_clip = ui.with_no_clip(|ui| {
      assert!(!ui.has_clip());
      42
    });

    assert_eq!(inside_clip, 42);
    assert_eq!(ui.current_clip(), original_clip);
    assert_eq!(ui.clip_depth(), 1);
  }

  #[test]
  fn point_in_clip_checks_bounds() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let mut ui = create_test_context(100, 100, &mut frame);

    // No clip - check screen bounds
    assert!(ui.point_in_clip(50.0, 50.0));
    assert!(!ui.point_in_clip(-1.0, 50.0));
    assert!(!ui.point_in_clip(100.0, 50.0));

    // With clip
    ui.push_clip(Rect::new(20.0, 20.0, 40.0, 40.0));
    assert!(ui.point_in_clip(30.0, 30.0)); // Inside clip
    assert!(!ui.point_in_clip(10.0, 30.0)); // Left of clip
    assert!(!ui.point_in_clip(70.0, 30.0)); // Right of clip
  }

  #[test]
  fn empty_clip_intersection() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let mut ui = create_test_context(100, 100, &mut frame);

    // Two non-overlapping clips
    ui.push_clip(Rect::new(10.0, 10.0, 20.0, 20.0)); // 10,10 to 30,30
    ui.push_clip(Rect::new(50.0, 50.0, 20.0, 20.0)); // 50,50 to 70,70

    // Intersection is empty
    let clip = ui.current_clip().unwrap();
    assert!(clip.width == 0.0 || clip.height == 0.0);
  }

  #[test]
  fn rect_intersection_calculation() {
    // Overlapping rects
    let a = Rect::new(10.0, 10.0, 50.0, 50.0);
    let b = Rect::new(30.0, 30.0, 50.0, 50.0);
    let result = UiContext::intersect_rects(a, b);
    assert_eq!(result.x, 30.0);
    assert_eq!(result.y, 30.0);
    assert_eq!(result.width, 30.0);
    assert_eq!(result.height, 30.0);

    // Non-overlapping rects
    let c = Rect::new(0.0, 0.0, 10.0, 10.0);
    let d = Rect::new(20.0, 20.0, 10.0, 10.0);
    let result2 = UiContext::intersect_rects(c, d);
    assert!(result2.width == 0.0 || result2.height == 0.0);
  }

  #[test]
  fn wrap_text_delegates_to_font() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let ui = create_test_context(100, 100, &mut frame);

    // Basic wrapping test
    let lines = ui.wrap_text("Hello World", 50);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "Hello");
    assert_eq!(lines[1], "World");
  }

  #[test]
  fn wrapped_text_size_delegates_to_font() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let ui = create_test_context(100, 100, &mut frame);

    // 8px char width, "Hello" = 40px, wraps to 2 lines at 50px width
    let (w, h) = ui.wrapped_text_size("Hello World", 50);
    assert_eq!(w, 40); // "Hello" or "World" width
    assert_eq!(h, 16); // 2 lines * 8px
  }

  #[test]
  fn text_size_calculates_correctly() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let ui = create_test_context(100, 100, &mut frame);

    let (w, h) = ui.text_size("Test");
    assert_eq!(w, 32); // 4 chars * 8px
    assert_eq!(h, 8); // 1 line * 8px
  }

  // ==========================================================================
  // Click Consumption Tests
  // ==========================================================================

  /// Create a test context with a mouse click at the given position.
  fn create_test_context_with_click(
    width: u32,
    height: u32,
    frame: &mut [u8],
    click_x: f32,
    click_y: f32,
  ) -> UiContext<'_> {
    let mut input = InputState::new();
    input.handle_mouse_move(click_x as f64, click_y as f64);
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    let input_ref: &'static InputState = Box::leak(Box::new(input));
    let font = super::super::font::create_debug_font();
    let font_ref: &'static BitmapFont = Box::leak(Box::new(font));
    UiContext::new(frame, width, height, input_ref, font_ref)
  }

  #[test]
  fn button_consumes_click() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context_with_click(200, 200, &mut frame, 50.0, 50.0);

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let clicked = ui.button("Test", rect);

    assert!(clicked, "Button should register click");
    assert!(ui.click_consumed(), "Click should be consumed after button");
  }

  #[test]
  fn second_button_does_not_fire_after_consumed() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context_with_click(200, 200, &mut frame, 50.0, 50.0);

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let first = ui.button("First", rect);
    let second = ui.button("Second", rect);

    assert!(first, "First button should fire");
    assert!(!second, "Second overlapping button should NOT fire");
  }

  #[test]
  fn button_styled_consumes_click() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context_with_click(200, 200, &mut frame, 50.0, 50.0);

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let clicked = ui.button_styled(
      "Test",
      rect,
      Color::BLACK,
      Color::GRAY,
      Color::WHITE,
      Color::WHITE,
    );

    assert!(clicked);
    assert!(ui.click_consumed());
  }

  #[test]
  fn checkbox_consumes_click() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context_with_click(200, 200, &mut frame, 50.0, 50.0);

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let mut checked = false;
    let toggled = ui.checkbox(rect, &mut checked, None);

    assert!(toggled, "Checkbox should register click");
    assert!(checked, "Checkbox should be checked");
    assert!(ui.click_consumed(), "Click should be consumed");
  }

  #[test]
  fn toggle_consumes_click() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context_with_click(200, 200, &mut frame, 50.0, 50.0);

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let mut on = false;
    let toggled = ui.toggle(rect, &mut on);

    assert!(toggled, "Toggle should register click");
    assert!(on, "Toggle should be on");
    assert!(ui.click_consumed(), "Click should be consumed");
  }

  #[test]
  fn text_input_consumes_click() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context_with_click(200, 200, &mut frame, 50.0, 50.0);

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let mut state = TextInputState::new();
    ui.text_input(&mut state, rect, "placeholder", 0.016);

    assert!(state.is_focused(), "Text input should be focused");
    assert!(ui.click_consumed(), "Click should be consumed");
  }

  #[test]
  fn consume_click_prevents_button() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context_with_click(200, 200, &mut frame, 50.0, 50.0);

    ui.consume_click();

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let clicked = ui.button("Test", rect);

    assert!(!clicked, "Button should not fire after consume_click");
  }

  #[test]
  fn consume_escape_sets_flag() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context(200, 200, &mut frame);

    assert!(!ui.escape_consumed());
    ui.consume_escape();
    assert!(ui.escape_consumed());
  }

  #[test]
  fn checkbox_not_toggled_when_click_consumed() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context_with_click(200, 200, &mut frame, 50.0, 50.0);

    // First widget consumes the click
    ui.consume_click();

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let mut checked = false;
    let toggled = ui.checkbox(rect, &mut checked, None);

    assert!(!toggled, "Checkbox should not toggle when click consumed");
    assert!(!checked, "Checkbox state should be unchanged");
  }

  #[test]
  fn text_input_unfocuses_even_when_click_consumed() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    // Click at position outside the text input rect
    let mut ui = create_test_context_with_click(200, 200, &mut frame, 180.0, 180.0);

    let rect = Rect::new(10.0, 10.0, 100.0, 30.0);
    let mut state = TextInputState::new();
    state.set_focused(true);

    // First, consume the click (simulating another widget handled it)
    ui.consume_click();

    // Text input should STILL unfocus — clicking outside always unfocuses,
    // regardless of consumption. This prevents dual-focus when clicking
    // between text inputs rendered in any order.
    ui.text_input(&mut state, rect, "placeholder", 0.016);
    assert!(
      !state.is_focused(),
      "Text input should unfocus when clicked outside, even if click was consumed"
    );
  }

  #[test]
  fn clicking_second_text_input_unfocuses_first() {
    // Simulate two text inputs where field B is rendered before field A.
    // Clicking on field B should focus B and unfocus A.
    let mut frame = vec![0u8; 400 * 200 * 4];
    // Click at position inside field B (right side)
    let mut ui = create_test_context_with_click(400, 200, &mut frame, 250.0, 20.0);

    let rect_a = Rect::new(10.0, 10.0, 150.0, 30.0);
    let rect_b = Rect::new(200.0, 10.0, 150.0, 30.0);

    let mut state_a = TextInputState::new();
    let mut state_b = TextInputState::new();
    state_a.set_focused(true); // A was previously focused

    // Render B first (it gets the click), then A
    ui.text_input(&mut state_b, rect_b, "Field B", 0.016);
    ui.text_input(&mut state_a, rect_a, "Field A", 0.016);

    assert!(state_b.is_focused(), "Field B should be focused");
    assert!(!state_a.is_focused(), "Field A should be unfocused");
  }

  // ==========================================================================
  // DPI/UI Scale Tests
  // ==========================================================================

  fn create_test_context_scaled(
    width: u32,
    height: u32,
    frame: &mut [u8],
    scale: u32,
  ) -> UiContext<'_> {
    let input = InputState::new();
    let input_ref: &'static InputState = Box::leak(Box::new(input));
    let font = super::super::font::create_debug_font();
    let font_ref: &'static BitmapFont = Box::leak(Box::new(font));

    UiContext::new_scaled(frame, width, height, input_ref, font_ref, scale)
  }

  #[test]
  fn ui_context_default_scale_is_1() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let ui = create_test_context(100, 100, &mut frame);

    assert_eq!(ui.ui_scale(), 1, "Default UI scale should be 1");
  }

  #[test]
  fn ui_context_scaled_stores_scale() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let ui = create_test_context_scaled(100, 100, &mut frame, 2);

    assert_eq!(ui.ui_scale(), 2, "UI scale should be 2");
  }

  #[test]
  fn ui_context_scaled_3x() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let ui = create_test_context_scaled(100, 100, &mut frame, 3);

    assert_eq!(ui.ui_scale(), 3, "UI scale should be 3");
  }

  #[test]
  fn text_size_respects_ui_scale_2x() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let ui = create_test_context_scaled(200, 200, &mut frame, 2);

    let (w, h) = ui.text_size("Test");
    // At 2x scale, text should be 2x larger
    assert_eq!(w, 64); // 4 chars * 8px * 2 = 64
    assert_eq!(h, 16); // 1 line * 8px * 2 = 16
  }

  #[test]
  fn text_size_respects_ui_scale_1x() {
    let mut frame = vec![0u8; 100 * 100 * 4];
    let ui = create_test_context_scaled(100, 100, &mut frame, 1);

    let (w, h) = ui.text_size("Test");
    // At 1x scale, text should be normal size
    assert_eq!(w, 32); // 4 chars * 8px = 32
    assert_eq!(h, 8); // 1 line * 8px = 8
  }

  #[test]
  fn draw_text_at_2x_scale_draws_larger() {
    let mut frame_1x = vec![0u8; 100 * 100 * 4];
    let mut frame_2x = vec![0u8; 100 * 100 * 4];

    {
      let mut ui_1x = create_test_context_scaled(100, 100, &mut frame_1x, 1);
      ui_1x.draw_text("A", 10.0, 10.0, Color::WHITE);
    }
    {
      let mut ui_2x = create_test_context_scaled(100, 100, &mut frame_2x, 2);
      ui_2x.draw_text("A", 10.0, 10.0, Color::WHITE);
    }

    // Count non-zero pixels
    let pixels_1x = frame_1x.iter().filter(|&&p| p != 0).count();
    let pixels_2x = frame_2x.iter().filter(|&&p| p != 0).count();

    // 2x scale should have approximately 4x the pixels (2x width * 2x height)
    assert!(pixels_2x > pixels_1x, "2x scale should draw more pixels");
    // Approximately 4x, give some tolerance for anti-aliasing
    let ratio = pixels_2x as f64 / pixels_1x as f64;
    assert!(
      ratio > 3.0 && ratio < 5.0,
      "2x scale should have ~4x pixels, got {}",
      ratio
    );
  }

  // =========================================================================
  // Mouse Occlusion Tests
  // =========================================================================

  fn create_test_context_with_hover(
    width: u32,
    height: u32,
    frame: &mut [u8],
    hover_x: f32,
    hover_y: f32,
  ) -> UiContext<'_> {
    let mut input = InputState::new();
    input.handle_mouse_move(hover_x as f64, hover_y as f64);
    let input_ref: &'static InputState = Box::leak(Box::new(input));
    let font = super::super::font::create_debug_font();
    let font_ref: &'static BitmapFont = Box::leak(Box::new(font));
    UiContext::new(frame, width, height, input_ref, font_ref)
  }

  #[test]
  fn mouse_occluded_blocks_hover() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context_with_hover(200, 200, &mut frame, 50.0, 50.0);

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    assert!(ui.is_hovered(rect), "Should be hovered before occlusion");

    ui.set_mouse_occluded();
    assert!(
      !ui.is_hovered(rect),
      "Should not be hovered after occlusion"
    );
  }

  #[test]
  fn mouse_occluded_blocks_click() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context_with_click(200, 200, &mut frame, 50.0, 50.0);

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    assert!(ui.is_clicked(rect), "Should be clicked before occlusion");

    ui.set_mouse_occluded();
    assert!(
      !ui.is_clicked(rect),
      "Should not be clicked after occlusion"
    );
  }

  #[test]
  fn mouse_occluded_blocks_held() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    // Use click helper — mouse_pressed implies mouse_down
    let mut ui = create_test_context_with_click(200, 200, &mut frame, 50.0, 50.0);

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    assert!(ui.is_held(rect), "Should be held before occlusion");

    ui.set_mouse_occluded();
    assert!(!ui.is_held(rect), "Should not be held after occlusion");
  }

  #[test]
  fn mouse_occluded_prevents_button_click() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context_with_click(200, 200, &mut frame, 50.0, 50.0);

    ui.set_mouse_occluded();

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let clicked = ui.button("Test", rect);

    assert!(!clicked, "Button should not register click when occluded");
    // click_consumed should still be false (button didn't fire)
    assert!(
      !ui.click_consumed(),
      "Click should not be consumed by occluded button"
    );
  }

  #[test]
  fn mouse_not_occluded_by_default() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let ui = create_test_context_with_hover(200, 200, &mut frame, 50.0, 50.0);

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    assert!(
      ui.is_hovered(rect),
      "Mouse should not be occluded by default"
    );
  }

  #[test]
  fn mouse_occluded_does_not_affect_rects_outside_mouse() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context_with_hover(200, 200, &mut frame, 50.0, 50.0);

    ui.set_mouse_occluded();

    // Rect that doesn't contain the mouse — is_hovered is false regardless
    let far_rect = Rect::new(150.0, 150.0, 30.0, 30.0);
    assert!(!ui.is_hovered(far_rect));
  }
}
