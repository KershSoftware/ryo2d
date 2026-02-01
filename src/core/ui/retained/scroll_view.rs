//! ScrollView widget for scrollable content areas
//!
//! A retained-mode widget that provides scrollable containers. The user stores
//! a `ScrollViewState` and passes it each frame to render scrollable content.
//!
//! # Example
//!
//! ```ignore
//! // In your scene/game struct:
//! scroll_state: ScrollViewState,
//!
//! // In render_ui:
//! let content_height = 800.0; // Total height of all content
//! let view_rect = Rect::new(20.0, 50.0, 300.0, 400.0);
//!
//! ui.scroll_view(&mut self.scroll_state, view_rect, content_height, |ui, scroll_offset| {
//!     // Draw content - y positions are relative to scroll offset
//!     for i in 0..20 {
//!         let y = i as f32 * 35.0 - scroll_offset;
//!         ui.draw_text(&format!("Item {}", i), 10.0, y, Color::WHITE);
//!     }
//! });
//! ```

use crate::core::input::InputState;

/// State for a scroll view widget.
///
/// This is a retained-mode widget - you store this in your scene/game and
/// pass it to the UI context each frame.
#[derive(Debug, Clone)]
pub struct ScrollViewState {
  /// Current vertical scroll offset (0 = top)
  scroll_y: f32,
  /// Current horizontal scroll offset (0 = left)
  scroll_x: f32,
  /// Velocity for smooth scrolling/momentum
  velocity_y: f32,
  velocity_x: f32,
  /// Whether the scrollbar is being dragged
  dragging_scrollbar: bool,
  /// Whether the content area is being dragged (click and drag scrolling)
  dragging_content: bool,
  /// Last mouse Y when dragging started
  drag_start_mouse_y: f32,
  /// Scroll offset when dragging started
  drag_start_scroll_y: f32,
}

impl Default for ScrollViewState {
  fn default() -> Self {
    Self::new()
  }
}

impl ScrollViewState {
  /// Create a new scroll view state starting at the top.
  pub fn new() -> Self {
    Self {
      scroll_y: 0.0,
      scroll_x: 0.0,
      velocity_y: 0.0,
      velocity_x: 0.0,
      dragging_scrollbar: false,
      dragging_content: false,
      drag_start_mouse_y: 0.0,
      drag_start_scroll_y: 0.0,
    }
  }

  /// Create a scroll view state with an initial scroll position.
  pub fn with_scroll(scroll_x: f32, scroll_y: f32) -> Self {
    Self {
      scroll_y,
      scroll_x,
      ..Self::new()
    }
  }

  /// Get the current vertical scroll offset.
  pub fn scroll_y(&self) -> f32 {
    self.scroll_y
  }

  /// Get the current horizontal scroll offset.
  pub fn scroll_x(&self) -> f32 {
    self.scroll_x
  }

  /// Set the vertical scroll position directly.
  pub fn set_scroll_y(&mut self, y: f32) {
    self.scroll_y = y;
    self.velocity_y = 0.0;
  }

  /// Set the horizontal scroll position directly.
  pub fn set_scroll_x(&mut self, x: f32) {
    self.scroll_x = x;
    self.velocity_x = 0.0;
  }

  /// Scroll to the top of the content.
  pub fn scroll_to_top(&mut self) {
    self.scroll_y = 0.0;
    self.velocity_y = 0.0;
  }

  /// Scroll to the bottom of the content.
  pub fn scroll_to_bottom(&mut self, content_height: f32, view_height: f32) {
    self.scroll_y = (content_height - view_height).max(0.0);
    self.velocity_y = 0.0;
  }

  /// Check if scrollbar is currently being dragged.
  pub fn is_dragging(&self) -> bool {
    self.dragging_scrollbar || self.dragging_content
  }

  /// Update scroll state based on input.
  ///
  /// Returns `true` if the scroll position changed.
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn handle_input(
    &mut self,
    input: &InputState,
    view_height: f32,
    content_height: f32,
    hovered: bool,
    scrollbar_hovered: bool,
    scrollbar_thumb_rect: Option<(f32, f32, f32, f32)>, // (x, y, w, h)
    dt: f32,
  ) -> bool {
    let max_scroll = (content_height - view_height).max(0.0);
    let old_scroll = self.scroll_y;

    // Handle mouse wheel scrolling when hovered
    if hovered {
      let (_, scroll_y) = input.scroll_delta();
      if scroll_y != 0.0 {
        // Scroll 40 pixels per scroll line
        self.scroll_y -= scroll_y * 40.0;
        self.velocity_y = 0.0; // Cancel momentum when using wheel
      }
    }

    // Handle scrollbar dragging
    if let Some((thumb_x, thumb_y, thumb_w, thumb_h)) = scrollbar_thumb_rect {
      let mouse_x = input.mouse_x();
      let mouse_y = input.mouse_y();

      // Check if mouse is over thumb
      let thumb_hovered = mouse_x >= thumb_x
        && mouse_x <= thumb_x + thumb_w
        && mouse_y >= thumb_y
        && mouse_y <= thumb_y + thumb_h;

      if input.mouse_pressed(winit::event::MouseButton::Left) {
        if thumb_hovered {
          self.dragging_scrollbar = true;
          self.drag_start_mouse_y = mouse_y;
          self.drag_start_scroll_y = self.scroll_y;
        } else if scrollbar_hovered && content_height > view_height {
          // Click on scrollbar track - jump to that position
          // Calculate what scroll position corresponds to this mouse Y
          let track_start = thumb_y - self.scroll_y * (view_height / content_height);
          let relative_y = mouse_y - track_start;
          self.scroll_y = (relative_y / view_height) * content_height - view_height / 2.0;
          self.velocity_y = 0.0;
        }
      }
    }

    // Continue scrollbar drag
    if self.dragging_scrollbar {
      if input.mouse_down(winit::event::MouseButton::Left) {
        let mouse_y = input.mouse_y();
        let delta_mouse = mouse_y - self.drag_start_mouse_y;
        // Convert mouse delta to scroll delta based on ratio
        let scroll_ratio = if view_height > 0.0 {
          content_height / view_height
        } else {
          1.0
        };
        self.scroll_y = self.drag_start_scroll_y + delta_mouse * scroll_ratio;
        self.velocity_y = 0.0;
      } else {
        self.dragging_scrollbar = false;
      }
    }

    // Handle content drag scrolling (click and drag in content area)
    if hovered && !scrollbar_hovered && input.mouse_pressed(winit::event::MouseButton::Left) {
      self.dragging_content = true;
      self.drag_start_mouse_y = input.mouse_y();
      self.drag_start_scroll_y = self.scroll_y;
    }

    if self.dragging_content {
      if input.mouse_down(winit::event::MouseButton::Left) {
        let mouse_y = input.mouse_y();
        let delta = self.drag_start_mouse_y - mouse_y;
        self.scroll_y = self.drag_start_scroll_y + delta;

        // Track velocity for momentum
        self.velocity_y = delta / dt.max(0.001);
      } else {
        self.dragging_content = false;
        // Apply momentum decay
        self.velocity_y *= 0.95;
      }
    }

    // Apply momentum scrolling (when not dragging)
    if !self.dragging_content && !self.dragging_scrollbar {
      if self.velocity_y.abs() > 0.5 {
        self.scroll_y += self.velocity_y * dt;
        self.velocity_y *= 0.92; // Friction
      } else {
        self.velocity_y = 0.0;
      }
    }

    // Clamp scroll position
    self.scroll_y = self.scroll_y.clamp(0.0, max_scroll);

    self.scroll_y != old_scroll
  }

  /// Update scroll state based on input for horizontal scrolling.
  ///
  /// Returns `true` if the scroll position changed.
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn handle_input_horizontal(
    &mut self,
    input: &InputState,
    view_width: f32,
    content_width: f32,
    hovered: bool,
    scrollbar_hovered: bool,
    scrollbar_thumb_rect: Option<(f32, f32, f32, f32)>, // (x, y, w, h)
    dt: f32,
  ) -> bool {
    let max_scroll = (content_width - view_width).max(0.0);
    let old_scroll = self.scroll_x;

    // Handle mouse wheel scrolling when hovered (horizontal uses vertical wheel)
    if hovered {
      let (_, scroll_y) = input.scroll_delta();
      if scroll_y != 0.0 {
        // Scroll 40 pixels per scroll line (use vertical wheel for horizontal)
        self.scroll_x -= scroll_y * 40.0;
        self.velocity_x = 0.0; // Cancel momentum when using wheel
      }
    }

    // Handle scrollbar dragging
    if let Some((thumb_x, thumb_y, thumb_w, thumb_h)) = scrollbar_thumb_rect {
      let mouse_x = input.mouse_x();
      let mouse_y = input.mouse_y();

      // Check if mouse is over thumb
      let thumb_hovered = mouse_x >= thumb_x
        && mouse_x <= thumb_x + thumb_w
        && mouse_y >= thumb_y
        && mouse_y <= thumb_y + thumb_h;

      if input.mouse_pressed(winit::event::MouseButton::Left) {
        if thumb_hovered {
          self.dragging_scrollbar = true;
          self.drag_start_mouse_y = mouse_x; // Using Y field for X coord
          self.drag_start_scroll_y = self.scroll_x; // Using Y field for X scroll
        } else if scrollbar_hovered && content_width > view_width {
          // Click on scrollbar track - jump to that position
          let track_start = thumb_x - self.scroll_x * (view_width / content_width);
          let relative_x = mouse_x - track_start;
          self.scroll_x = (relative_x / view_width) * content_width - view_width / 2.0;
          self.velocity_x = 0.0;
        }
      }
    }

    // Continue scrollbar drag
    if self.dragging_scrollbar {
      if input.mouse_down(winit::event::MouseButton::Left) {
        let mouse_x = input.mouse_x();
        let delta_mouse = mouse_x - self.drag_start_mouse_y;
        let scroll_ratio = if view_width > 0.0 {
          content_width / view_width
        } else {
          1.0
        };
        self.scroll_x = self.drag_start_scroll_y + delta_mouse * scroll_ratio;
        self.velocity_x = 0.0;
      } else {
        self.dragging_scrollbar = false;
      }
    }

    // Handle content drag scrolling (click and drag in content area)
    if hovered && !scrollbar_hovered && input.mouse_pressed(winit::event::MouseButton::Left) {
      self.dragging_content = true;
      self.drag_start_mouse_y = input.mouse_x(); // Using Y field for X
      self.drag_start_scroll_y = self.scroll_x;
    }

    if self.dragging_content {
      if input.mouse_down(winit::event::MouseButton::Left) {
        let mouse_x = input.mouse_x();
        let delta = self.drag_start_mouse_y - mouse_x;
        self.scroll_x = self.drag_start_scroll_y + delta;

        // Track velocity for momentum
        self.velocity_x = delta / dt.max(0.001);
      } else {
        self.dragging_content = false;
        self.velocity_x *= 0.95;
      }
    }

    // Apply momentum scrolling (when not dragging)
    if !self.dragging_content && !self.dragging_scrollbar {
      if self.velocity_x.abs() > 0.5 {
        self.scroll_x += self.velocity_x * dt;
        self.velocity_x *= 0.92; // Friction
      } else {
        self.velocity_x = 0.0;
      }
    }

    // Clamp scroll position
    self.scroll_x = self.scroll_x.clamp(0.0, max_scroll);

    self.scroll_x != old_scroll
  }

  /// Calculate the scrollbar thumb position and size.
  ///
  /// Returns `(thumb_y, thumb_height)` relative to the track.
  pub(crate) fn calculate_thumb(
    &self,
    view_height: f32,
    content_height: f32,
    track_height: f32,
  ) -> (f32, f32) {
    if content_height <= view_height {
      // No scrolling needed - thumb fills track
      return (0.0, track_height);
    }

    // Thumb size is proportional to visible area
    let thumb_height = (view_height / content_height * track_height).max(20.0);

    // Thumb position is proportional to scroll position
    let max_scroll = content_height - view_height;
    let scroll_ratio = if max_scroll > 0.0 {
      self.scroll_y / max_scroll
    } else {
      0.0
    };
    let max_thumb_y = track_height - thumb_height;
    let thumb_y = scroll_ratio * max_thumb_y;

    (thumb_y, thumb_height)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn new_creates_default_state() {
    let state = ScrollViewState::new();
    assert_eq!(state.scroll_y(), 0.0);
    assert_eq!(state.scroll_x(), 0.0);
    assert!(!state.is_dragging());
  }

  #[test]
  fn with_scroll_sets_initial_position() {
    let state = ScrollViewState::with_scroll(10.0, 50.0);
    assert_eq!(state.scroll_x(), 10.0);
    assert_eq!(state.scroll_y(), 50.0);
  }

  #[test]
  fn set_scroll_resets_velocity() {
    let mut state = ScrollViewState::new();
    state.velocity_y = 100.0;
    state.set_scroll_y(50.0);
    assert_eq!(state.scroll_y(), 50.0);
    assert_eq!(state.velocity_y, 0.0);
  }

  #[test]
  fn scroll_to_top() {
    let mut state = ScrollViewState::with_scroll(0.0, 100.0);
    state.scroll_to_top();
    assert_eq!(state.scroll_y(), 0.0);
  }

  #[test]
  fn scroll_to_bottom() {
    let mut state = ScrollViewState::new();
    state.scroll_to_bottom(500.0, 200.0);
    assert_eq!(state.scroll_y(), 300.0); // 500 - 200
  }

  #[test]
  fn scroll_to_bottom_capped_when_content_fits() {
    let mut state = ScrollViewState::new();
    state.scroll_to_bottom(100.0, 200.0); // Content smaller than view
    assert_eq!(state.scroll_y(), 0.0);
  }

  #[test]
  fn calculate_thumb_no_scroll_needed() {
    let state = ScrollViewState::new();
    let (thumb_y, thumb_height) = state.calculate_thumb(200.0, 100.0, 200.0);
    assert_eq!(thumb_y, 0.0);
    assert_eq!(thumb_height, 200.0); // Fills track
  }

  #[test]
  fn calculate_thumb_at_top() {
    let state = ScrollViewState::new();
    let (thumb_y, _thumb_height) = state.calculate_thumb(200.0, 400.0, 200.0);
    assert_eq!(thumb_y, 0.0);
  }

  #[test]
  fn calculate_thumb_at_bottom() {
    let mut state = ScrollViewState::new();
    state.set_scroll_y(200.0); // Max scroll for 400 content, 200 view
    let (thumb_y, thumb_height) = state.calculate_thumb(200.0, 400.0, 200.0);
    // Thumb should be at bottom of track
    assert!((thumb_y + thumb_height - 200.0).abs() < 0.1);
  }

  #[test]
  fn calculate_thumb_minimum_size() {
    let state = ScrollViewState::new();
    let (_thumb_y, thumb_height) = state.calculate_thumb(100.0, 10000.0, 200.0);
    assert!(thumb_height >= 20.0); // Minimum thumb size
  }
}
