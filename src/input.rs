use std::collections::HashSet;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::KeyCode;

/// Tracks all input state for the current and previous frame
#[derive(Debug, Default)]
pub struct InputState {
  // Keyboard state
  keys_down: HashSet<KeyCode>,
  keys_pressed: HashSet<KeyCode>,
  keys_released: HashSet<KeyCode>,

  // Mouse button state
  mouse_buttons_down: HashSet<MouseButton>,
  mouse_buttons_pressed: HashSet<MouseButton>,
  mouse_buttons_released: HashSet<MouseButton>,

  // Mouse position (in pixels, relative to window)
  mouse_x: f32,
  mouse_y: f32,
  mouse_delta_x: f32,
  mouse_delta_y: f32,

  // Mouse scroll
  scroll_delta_x: f32,
  scroll_delta_y: f32,
}

impl InputState {
  pub fn new() -> Self {
    InputState::default()
  }

  /// Call at the start of each frame to clear per-frame state
  pub fn begin_frame(&mut self) {
    self.keys_pressed.clear();
    self.keys_released.clear();
    self.mouse_buttons_pressed.clear();
    self.mouse_buttons_released.clear();
    self.mouse_delta_x = 0.0;
    self.mouse_delta_y = 0.0;
    self.scroll_delta_x = 0.0;
    self.scroll_delta_y = 0.0;
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Keyboard Input
  // ─────────────────────────────────────────────────────────────────────────────

  /// Returns true if the key is currently held down
  pub fn key_down(&self, key: KeyCode) -> bool {
    self.keys_down.contains(&key)
  }

  /// Returns true if the key was just pressed this frame
  pub fn key_pressed(&self, key: KeyCode) -> bool {
    self.keys_pressed.contains(&key)
  }

  /// Returns true if the key was just released this frame
  pub fn key_released(&self, key: KeyCode) -> bool {
    self.keys_released.contains(&key)
  }

  /// Handle a keyboard event from winit
  pub(crate) fn handle_keyboard(&mut self, key: KeyCode, state: ElementState) {
    match state {
      ElementState::Pressed => {
        // Only register as "pressed" if it wasn't already down (prevents key repeat)
        if !self.keys_down.contains(&key) {
          self.keys_pressed.insert(key);
        }
        self.keys_down.insert(key);
      }
      ElementState::Released => {
        self.keys_down.remove(&key);
        self.keys_released.insert(key);
      }
    }
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Mouse Button Input
  // ─────────────────────────────────────────────────────────────────────────────

  /// Returns true if the mouse button is currently held down
  pub fn mouse_down(&self, button: MouseButton) -> bool {
    self.mouse_buttons_down.contains(&button)
  }

  /// Returns true if the mouse button was just pressed this frame
  pub fn mouse_pressed(&self, button: MouseButton) -> bool {
    self.mouse_buttons_pressed.contains(&button)
  }

  /// Returns true if the mouse button was just released this frame
  pub fn mouse_released(&self, button: MouseButton) -> bool {
    self.mouse_buttons_released.contains(&button)
  }

  /// Handle a mouse button event from winit
  pub(crate) fn handle_mouse_button(&mut self, button: MouseButton, state: ElementState) {
    match state {
      ElementState::Pressed => {
        if !self.mouse_buttons_down.contains(&button) {
          self.mouse_buttons_pressed.insert(button);
        }
        self.mouse_buttons_down.insert(button);
      }
      ElementState::Released => {
        self.mouse_buttons_down.remove(&button);
        self.mouse_buttons_released.insert(button);
      }
    }
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Mouse Position
  // ─────────────────────────────────────────────────────────────────────────────

  /// Get the current mouse X position in window pixels
  pub fn mouse_x(&self) -> f32 {
    self.mouse_x
  }

  /// Get the current mouse Y position in window pixels
  pub fn mouse_y(&self) -> f32 {
    self.mouse_y
  }

  /// Get the mouse position as a tuple (x, y)
  pub fn mouse_position(&self) -> (f32, f32) {
    (self.mouse_x, self.mouse_y)
  }

  /// Get how much the mouse moved this frame (x, y)
  pub fn mouse_delta(&self) -> (f32, f32) {
    (self.mouse_delta_x, self.mouse_delta_y)
  }

  /// Handle mouse movement from winit
  pub(crate) fn handle_mouse_move(&mut self, x: f64, y: f64) {
    let new_x = x as f32;
    let new_y = y as f32;
    self.mouse_delta_x += new_x - self.mouse_x;
    self.mouse_delta_y += new_y - self.mouse_y;
    self.mouse_x = new_x;
    self.mouse_y = new_y;
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Mouse Scroll
  // ─────────────────────────────────────────────────────────────────────────────

  /// Get the scroll wheel delta this frame (x, y)
  /// Positive y = scroll up, negative y = scroll down
  pub fn scroll_delta(&self) -> (f32, f32) {
    (self.scroll_delta_x, self.scroll_delta_y)
  }

  /// Handle mouse scroll from winit
  pub(crate) fn handle_scroll(&mut self, delta: MouseScrollDelta) {
    match delta {
      MouseScrollDelta::LineDelta(x, y) => {
        self.scroll_delta_x += x;
        self.scroll_delta_y += y;
      }
      MouseScrollDelta::PixelDelta(pos) => {
        // Convert pixel delta to approximate line delta
        self.scroll_delta_x += pos.x as f32 / 120.0;
        self.scroll_delta_y += pos.y as f32 / 120.0;
      }
    }
  }
}
