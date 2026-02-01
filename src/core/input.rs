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
  /// Keys that fired this frame (includes initial press AND repeats)
  keys_repeated: HashSet<KeyCode>,

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

  // Text input (from IME or keyboard)
  /// Text committed this frame (ready to insert)
  text_input: String,
  /// IME preedit/composition text (in-progress input, display but don't commit)
  ime_preedit: String,
  /// IME preedit cursor position (start, end) in bytes
  ime_preedit_cursor: Option<(usize, usize)>,
  /// Whether IME is currently enabled
  ime_enabled: bool,
}

impl InputState {
  pub fn new() -> Self {
    InputState::default()
  }

  /// Call at the start of each frame to clear per-frame state
  pub fn begin_frame(&mut self) {
    self.keys_pressed.clear();
    self.keys_released.clear();
    self.keys_repeated.clear();
    self.mouse_buttons_pressed.clear();
    self.mouse_buttons_released.clear();
    self.mouse_delta_x = 0.0;
    self.mouse_delta_y = 0.0;
    self.scroll_delta_x = 0.0;
    self.scroll_delta_y = 0.0;
    // Clear committed text, but keep preedit (it persists until cleared by IME)
    self.text_input.clear();
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

  /// Returns true if the key fired this frame (initial press OR repeat).
  ///
  /// Use this for text editing keys like arrows, backspace, delete that
  /// should repeat when held down.
  pub fn key_repeat(&self, key: KeyCode) -> bool {
    self.keys_repeated.contains(&key)
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
        // Always register in keys_repeated (includes initial press and repeats)
        self.keys_repeated.insert(key);
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

  // ─────────────────────────────────────────────────────────────────────────────
  // Text Input (IME)
  // ─────────────────────────────────────────────────────────────────────────────

  /// Get text that was committed this frame (ready to insert into text fields).
  ///
  /// Returns an empty string if no text was input this frame.
  pub fn text_input(&self) -> &str {
    &self.text_input
  }

  /// Check if any text was input this frame.
  pub fn has_text_input(&self) -> bool {
    !self.text_input.is_empty()
  }

  /// Get the current IME preedit/composition text (in-progress input).
  ///
  /// This text should be displayed at the cursor but not committed until
  /// the IME confirms it. Returns an empty string if no composition is active.
  pub fn ime_preedit(&self) -> &str {
    &self.ime_preedit
  }

  /// Get the IME preedit cursor position as (start, end) byte offsets.
  ///
  /// Returns `None` if no cursor should be shown in the preedit text.
  pub fn ime_preedit_cursor(&self) -> Option<(usize, usize)> {
    self.ime_preedit_cursor
  }

  /// Check if IME composition is currently active.
  pub fn ime_composing(&self) -> bool {
    !self.ime_preedit.is_empty()
  }

  /// Check if IME is currently enabled.
  pub fn ime_enabled(&self) -> bool {
    self.ime_enabled
  }

  /// Handle IME enabled event
  pub(crate) fn handle_ime_enabled(&mut self) {
    self.ime_enabled = true;
  }

  /// Handle IME disabled event
  pub(crate) fn handle_ime_disabled(&mut self) {
    self.ime_enabled = false;
    self.ime_preedit.clear();
    self.ime_preedit_cursor = None;
  }

  /// Handle IME preedit (composition) text update
  pub(crate) fn handle_ime_preedit(&mut self, text: String, cursor: Option<(usize, usize)>) {
    self.ime_preedit = text;
    self.ime_preedit_cursor = cursor;
  }

  /// Handle IME commit (finalized text ready to insert)
  pub(crate) fn handle_ime_commit(&mut self, text: String) {
    self.text_input.push_str(&text);
    // Preedit should be cleared by winit before commit, but ensure it's clear
    self.ime_preedit.clear();
    self.ime_preedit_cursor = None;
  }

  /// Handle direct text input from keyboard events (non-IME).
  ///
  /// This is used for regular character input when IME is not composing.
  pub(crate) fn handle_text_input(&mut self, text: &str) {
    // Only add text if not currently in IME composition mode
    if self.ime_preedit.is_empty() {
      self.text_input.push_str(text);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn key_down_tracks_held_keys() {
    let mut input = InputState::new();

    input.handle_keyboard(KeyCode::KeyW, ElementState::Pressed);
    assert!(input.key_down(KeyCode::KeyW));
    assert!(!input.key_down(KeyCode::KeyA));

    input.handle_keyboard(KeyCode::KeyW, ElementState::Released);
    assert!(!input.key_down(KeyCode::KeyW));
  }

  #[test]
  fn key_pressed_only_fires_once() {
    let mut input = InputState::new();

    input.handle_keyboard(KeyCode::KeyW, ElementState::Pressed);
    assert!(input.key_pressed(KeyCode::KeyW));

    // Simulate holding the key (repeated press events)
    input.handle_keyboard(KeyCode::KeyW, ElementState::Pressed);
    assert!(input.key_pressed(KeyCode::KeyW)); // Still true this frame

    // Next frame - pressed should clear
    input.begin_frame();
    assert!(!input.key_pressed(KeyCode::KeyW));
    assert!(input.key_down(KeyCode::KeyW)); // But still held
  }

  #[test]
  fn key_released_fires_on_release() {
    let mut input = InputState::new();

    input.handle_keyboard(KeyCode::KeyW, ElementState::Pressed);
    input.handle_keyboard(KeyCode::KeyW, ElementState::Released);
    assert!(input.key_released(KeyCode::KeyW));

    input.begin_frame();
    assert!(!input.key_released(KeyCode::KeyW));
  }

  #[test]
  fn mouse_position_tracking() {
    let mut input = InputState::new();

    input.handle_mouse_move(100.0, 200.0);
    assert_eq!(input.mouse_position(), (100.0, 200.0));
    assert_eq!(input.mouse_delta(), (100.0, 200.0)); // Delta from (0,0)

    input.begin_frame();
    input.handle_mouse_move(150.0, 250.0);
    assert_eq!(input.mouse_delta(), (50.0, 50.0)); // Delta from previous position
  }

  #[test]
  fn mouse_button_tracking() {
    let mut input = InputState::new();

    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    assert!(input.mouse_down(MouseButton::Left));
    assert!(input.mouse_pressed(MouseButton::Left));

    input.begin_frame();
    assert!(input.mouse_down(MouseButton::Left));
    assert!(!input.mouse_pressed(MouseButton::Left));

    input.handle_mouse_button(MouseButton::Left, ElementState::Released);
    assert!(!input.mouse_down(MouseButton::Left));
    assert!(input.mouse_released(MouseButton::Left));
  }

  #[test]
  fn scroll_accumulates_within_frame() {
    let mut input = InputState::new();

    input.handle_scroll(MouseScrollDelta::LineDelta(1.0, 2.0));
    input.handle_scroll(MouseScrollDelta::LineDelta(0.5, 1.0));
    assert_eq!(input.scroll_delta(), (1.5, 3.0));

    input.begin_frame();
    assert_eq!(input.scroll_delta(), (0.0, 0.0));
  }

  #[test]
  fn scroll_pixel_delta_conversion() {
    let mut input = InputState::new();
    input.handle_scroll(MouseScrollDelta::PixelDelta(
      winit::dpi::PhysicalPosition::new(120.0, 240.0),
    ));
    assert_eq!(input.scroll_delta(), (1.0, 2.0)); // Divided by 120.0
  }

  #[test]
  fn mouse_x_y_getters() {
    let mut input = InputState::new();
    input.handle_mouse_move(150.0, 250.0);
    assert_eq!(input.mouse_x(), 150.0);
    assert_eq!(input.mouse_y(), 250.0);
  }

  #[test]
  fn mouse_delta_accumulates() {
    let mut input = InputState::new();
    input.handle_mouse_move(10.0, 20.0);
    input.handle_mouse_move(15.0, 25.0);
    assert_eq!(input.mouse_delta(), (15.0, 25.0)); // Total delta from origin
  }

  #[test]
  fn begin_frame_clears_one_frame_events() {
    let mut input = InputState::new();
    input.handle_keyboard(KeyCode::KeyW, ElementState::Pressed);
    input.handle_keyboard(KeyCode::KeyA, ElementState::Released);
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    input.handle_mouse_move(100.0, 200.0);
    input.handle_scroll(MouseScrollDelta::LineDelta(1.0, 2.0));

    assert!(input.key_pressed(KeyCode::KeyW));
    assert!(input.key_released(KeyCode::KeyA));
    assert!(input.mouse_pressed(MouseButton::Left));
    assert_eq!(input.mouse_delta(), (100.0, 200.0));
    assert_eq!(input.scroll_delta(), (1.0, 2.0));

    input.begin_frame();

    // One-frame events cleared
    assert!(!input.key_pressed(KeyCode::KeyW));
    assert!(!input.key_released(KeyCode::KeyA));
    assert!(!input.mouse_pressed(MouseButton::Left));
    assert_eq!(input.mouse_delta(), (0.0, 0.0));
    assert_eq!(input.scroll_delta(), (0.0, 0.0));

    // Held states persist
    assert!(input.key_down(KeyCode::KeyW));
    assert!(!input.key_down(KeyCode::KeyA));
    assert!(input.mouse_down(MouseButton::Left));
  }

  #[test]
  fn mouse_button_no_repeat_registration() {
    let mut input = InputState::new();
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    assert!(input.mouse_pressed(MouseButton::Left));

    // Repeated press (OS key repeat)
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    // Should still be registered as pressed this frame
    assert!(input.mouse_pressed(MouseButton::Left));
  }

  #[test]
  fn key_repeat_fires_on_initial_and_repeat() {
    let mut input = InputState::new();

    // Initial press - both key_pressed and key_repeat fire
    input.handle_keyboard(KeyCode::ArrowLeft, ElementState::Pressed);
    assert!(input.key_pressed(KeyCode::ArrowLeft));
    assert!(input.key_repeat(KeyCode::ArrowLeft));

    // Simulate next frame with key still held
    input.begin_frame();

    // Key repeat event (OS sends another Pressed event)
    input.handle_keyboard(KeyCode::ArrowLeft, ElementState::Pressed);

    // key_pressed should NOT fire (already held)
    assert!(!input.key_pressed(KeyCode::ArrowLeft));
    // key_repeat SHOULD fire (for continuous action)
    assert!(input.key_repeat(KeyCode::ArrowLeft));
    // key_down should still be true
    assert!(input.key_down(KeyCode::ArrowLeft));
  }

  #[test]
  fn key_repeat_clears_each_frame() {
    let mut input = InputState::new();

    input.handle_keyboard(KeyCode::Backspace, ElementState::Pressed);
    assert!(input.key_repeat(KeyCode::Backspace));

    input.begin_frame();

    // No new press event this frame
    assert!(!input.key_repeat(KeyCode::Backspace));
    // But key is still held
    assert!(input.key_down(KeyCode::Backspace));
  }
}
