use std::cell::Cell;
use std::collections::HashSet;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::KeyCode;

use crate::engine_info;

/// Standard gamepad buttons (mapped from gilrs).
///
/// Uses positional names (South, East, West, North) that work across controllers.
/// South = Cross (PS) / A (Xbox), etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamepadButton {
  /// Face button - bottom (Cross on PS, A on Xbox)
  South,
  /// Face button - right (Circle on PS, B on Xbox)
  East,
  /// Face button - left (Square on PS, X on Xbox)
  West,
  /// Face button - top (Triangle on PS, Y on Xbox)
  North,

  /// Left shoulder button (L1 / LB)
  LeftBumper,
  /// Right shoulder button (R1 / RB)
  RightBumper,

  /// Left trigger as button (L2 / LT when fully pressed)
  LeftTrigger,
  /// Right trigger as button (R2 / RT when fully pressed)
  RightTrigger,

  /// Left stick click (L3)
  LeftStick,
  /// Right stick click (R3)
  RightStick,

  /// Select/Share/Back button
  Select,
  /// Start/Options/Menu button
  Start,
  /// Guide/PS/Xbox button (may not be available on all controllers)
  Guide,

  /// D-pad up
  DPadUp,
  /// D-pad down
  DPadDown,
  /// D-pad left
  DPadLeft,
  /// D-pad right
  DPadRight,
}

impl GamepadButton {
  /// Convert from gilrs Button to our GamepadButton.
  pub(crate) fn from_gilrs(button: gilrs::Button) -> Option<Self> {
    use gilrs::Button as B;
    match button {
      B::South => Some(GamepadButton::South),
      B::East => Some(GamepadButton::East),
      B::West => Some(GamepadButton::West),
      B::North => Some(GamepadButton::North),
      B::LeftTrigger => Some(GamepadButton::LeftBumper),
      B::RightTrigger => Some(GamepadButton::RightBumper),
      B::LeftTrigger2 => Some(GamepadButton::LeftTrigger),
      B::RightTrigger2 => Some(GamepadButton::RightTrigger),
      B::LeftThumb => Some(GamepadButton::LeftStick),
      B::RightThumb => Some(GamepadButton::RightStick),
      B::Select => Some(GamepadButton::Select),
      B::Start => Some(GamepadButton::Start),
      B::Mode => Some(GamepadButton::Guide),
      B::DPadUp => Some(GamepadButton::DPadUp),
      B::DPadDown => Some(GamepadButton::DPadDown),
      B::DPadLeft => Some(GamepadButton::DPadLeft),
      B::DPadRight => Some(GamepadButton::DPadRight),
      _ => None,
    }
  }
}

/// Deadzone for analog sticks (radial deadzone).
const STICK_DEADZONE: f32 = 0.15;

/// Apply radial deadzone to stick input and rescale to use full range.
fn apply_deadzone(x: f32, y: f32) -> (f32, f32) {
  let magnitude = (x * x + y * y).sqrt();
  if magnitude < STICK_DEADZONE {
    (0.0, 0.0)
  } else {
    // Rescale to use full range after deadzone
    let scale = (magnitude - STICK_DEADZONE) / (1.0 - STICK_DEADZONE) / magnitude;
    (x * scale, y * scale)
  }
}

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
  /// Whether IME committed text this frame (used to prevent duplicate input from keyboard events)
  ime_committed_this_frame: bool,
  /// Bytes of text_input that came from handle_text_input (keyboard) this frame
  keyboard_text_len: usize,
  /// Whether text_input has been consumed this frame (first reader wins)
  text_input_consumed: Cell<bool>,

  // Gamepad state (first connected gamepad only)
  /// Whether a gamepad is currently connected
  gamepad_connected: bool,
  /// Name of the connected gamepad
  gamepad_name: String,
  /// ID of the active gamepad (for gilrs)
  gamepad_id: Option<gilrs::GamepadId>,

  /// Gamepad buttons currently held down
  gamepad_buttons_down: HashSet<GamepadButton>,
  /// Gamepad buttons that were just pressed this frame
  gamepad_buttons_pressed: HashSet<GamepadButton>,
  /// Gamepad buttons that were just released this frame
  gamepad_buttons_released: HashSet<GamepadButton>,

  /// Left stick X axis (after deadzone, -1.0 to 1.0)
  left_stick_x: f32,
  /// Left stick Y axis (after deadzone, -1.0 to 1.0)
  left_stick_y: f32,
  /// Right stick X axis (after deadzone, -1.0 to 1.0)
  right_stick_x: f32,
  /// Right stick Y axis (after deadzone, -1.0 to 1.0)
  right_stick_y: f32,

  /// Raw left stick X (before deadzone, for visualization)
  left_stick_x_raw: f32,
  /// Raw left stick Y (before deadzone, for visualization)
  left_stick_y_raw: f32,
  /// Raw right stick X (before deadzone, for visualization)
  right_stick_x_raw: f32,
  /// Raw right stick Y (before deadzone, for visualization)
  right_stick_y_raw: f32,

  /// Left trigger value (0.0 to 1.0)
  left_trigger: f32,
  /// Right trigger value (0.0 to 1.0)
  right_trigger: f32,

  /// When true, one-shot input queries return false/empty.
  /// Used by the engine to prevent events from firing in subsequent
  /// fixed timestep updates after the first.
  oneshots_suppressed: bool,
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
    self.ime_committed_this_frame = false;
    self.keyboard_text_len = 0;
    self.text_input_consumed.set(false);
    // Clear per-frame gamepad state
    self.gamepad_buttons_pressed.clear();
    self.gamepad_buttons_released.clear();
    // Reset oneshot suppression for the new frame
    self.oneshots_suppressed = false;
  }

  /// Suppress one-shot input queries (pressed/released/text).
  ///
  /// When suppressed, one-shot queries return false/empty so that events
  /// only fire in the first fixed timestep update, not subsequent ones.
  pub(crate) fn suppress_oneshots(&mut self) {
    self.oneshots_suppressed = true;
  }

  /// Restore one-shot input queries after suppression.
  ///
  /// Called before render so that `render_ui()` can still see one-shot events.
  pub(crate) fn unsuppress_oneshots(&mut self) {
    self.oneshots_suppressed = false;
  }

  /// Clean up input state that should not persist across renders.
  ///
  /// Called every frame after render_ui completes (unconditionally, unlike
  /// `begin_frame` which only runs on update frames). Clears `keys_repeated`
  /// so that held-key actions (backspace, arrow keys) don't fire multiple
  /// times when render_ui runs on consecutive no-update frames.
  pub(crate) fn post_render_cleanup(&mut self) {
    self.keys_repeated.clear();
  }

  /// Clear only keyboard one-shot events (pressed/released/repeated).
  ///
  /// Note: This function is no longer called in the main frame loop because
  /// UI widgets need keyboard events to persist until render_ui completes.
  /// Kept for potential future use and tested for correctness.
  #[allow(dead_code)]
  pub(crate) fn consume_keyboard_events(&mut self) {
    self.keys_pressed.clear();
    self.keys_released.clear();
    self.keys_repeated.clear();
  }

  /// Consume a specific key press, preventing it from being detected again this frame.
  ///
  /// Used by the engine for debug keys (F1-F4) which are processed outside
  /// the fixed timestep loop and need to fire exactly once.
  pub(crate) fn consume_key(&mut self, key: KeyCode) {
    self.keys_pressed.remove(&key);
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Input Consumption Utilities
  // ─────────────────────────────────────────────────────────────────────────────

  /// Check if a key was pressed this frame AND consume it, preventing other systems from seeing it.
  ///
  /// This is useful when multiple systems might react to the same key press and you want
  /// only one to handle it. Returns `true` if the key was pressed (and is now consumed),
  /// `false` if it wasn't pressed.
  ///
  /// # Example
  /// ```ignore
  /// // In a menu system - consume Escape so the game doesn't also pause
  /// if input.key_pressed_and_consume(KeyCode::Escape) {
  ///     self.close_menu();
  ///     // Other systems checking key_pressed(Escape) will now return false
  /// }
  /// ```
  pub fn key_pressed_and_consume(&mut self, key: KeyCode) -> bool {
    if self.oneshots_suppressed {
      return false;
    }
    self.keys_pressed.remove(&key)
  }

  /// Consume a key press without checking if it was pressed.
  ///
  /// Use this when you know a key was pressed and want to prevent other systems from seeing it.
  pub fn consume_key_press(&mut self, key: KeyCode) {
    self.keys_pressed.remove(&key);
  }

  /// Check if a mouse button was pressed this frame AND consume it.
  ///
  /// Returns `true` if the button was pressed (and is now consumed), `false` otherwise.
  ///
  /// # Example
  /// ```ignore
  /// // UI button consumes click so world interaction doesn't trigger
  /// if input.mouse_pressed_and_consume(MouseButton::Left) {
  ///     self.handle_button_click();
  /// }
  /// ```
  pub fn mouse_pressed_and_consume(&mut self, button: MouseButton) -> bool {
    if self.oneshots_suppressed {
      return false;
    }
    self.mouse_buttons_pressed.remove(&button)
  }

  /// Consume a mouse button press without checking if it was pressed.
  pub fn consume_mouse_press(&mut self, button: MouseButton) {
    self.mouse_buttons_pressed.remove(&button);
  }

  /// Check if a gamepad button was pressed this frame AND consume it.
  ///
  /// Returns `true` if the button was pressed (and is now consumed), `false` otherwise.
  pub fn gamepad_button_pressed_and_consume(&mut self, button: GamepadButton) -> bool {
    if self.oneshots_suppressed {
      return false;
    }
    self.gamepad_buttons_pressed.remove(&button)
  }

  /// Consume a gamepad button press without checking if it was pressed.
  pub fn consume_gamepad_button_press(&mut self, button: GamepadButton) {
    self.gamepad_buttons_pressed.remove(&button);
  }

  /// Clear all keyboard state for this frame.
  ///
  /// This prevents any keyboard input from reaching systems that check input after this call.
  /// Used by the developer console to block game input while typing.
  ///
  /// Clears: pressed keys, released keys, repeated keys, held keys, and text input.
  pub fn clear_keyboard_state(&mut self) {
    self.keys_pressed.clear();
    self.keys_released.clear();
    self.keys_repeated.clear();
    self.keys_down.clear();
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
    if self.oneshots_suppressed {
      return false;
    }
    self.keys_pressed.contains(&key)
  }

  /// Returns true if the key was just released this frame
  pub fn key_released(&self, key: KeyCode) -> bool {
    if self.oneshots_suppressed {
      return false;
    }
    self.keys_released.contains(&key)
  }

  /// Returns true if the key fired this frame (initial press OR repeat).
  ///
  /// Use this for text editing keys like arrows, backspace, delete that
  /// should repeat when held down.
  pub fn key_repeat(&self, key: KeyCode) -> bool {
    if self.oneshots_suppressed {
      return false;
    }
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
    if self.oneshots_suppressed {
      return false;
    }
    self.mouse_buttons_pressed.contains(&button)
  }

  /// Returns true if the mouse button was just released this frame
  pub fn mouse_released(&self, button: MouseButton) -> bool {
    if self.oneshots_suppressed {
      return false;
    }
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
  /// The first caller to read text input in a frame gets the text; subsequent
  /// callers receive an empty string. This prevents duplicate processing when
  /// multiple systems (game update, UI render, console) all read text input.
  ///
  /// Returns an empty string if no text was input or it was already consumed.
  pub fn text_input(&self) -> &str {
    if self.oneshots_suppressed || self.text_input_consumed.get() {
      return "";
    }
    // Only mark as consumed when there's actual text — reading an empty buffer
    // on a no-input frame must NOT set consumed, because begin_frame() only runs
    // on update frames and won't reset it before the next frame's events arrive.
    if !self.text_input.is_empty() {
      self.text_input_consumed.set(true);
    }
    &self.text_input
  }

  /// Check if any text was input this frame (and hasn't been consumed yet).
  pub fn has_text_input(&self) -> bool {
    if self.oneshots_suppressed || self.text_input_consumed.get() {
      return false;
    }
    !self.text_input.is_empty()
  }

  /// Explicitly consume text input, preventing subsequent readers from seeing it.
  ///
  /// Call this after reading text input to ensure no other system processes it.
  pub fn consume_text_input(&self) {
    self.text_input_consumed.set(true);
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
    // On macOS, both KeyboardInput{text} and Ime::Commit fire for the same keypress.
    // Remove only the keyboard-sourced portion (tracked by keyboard_text_len) so we
    // don't accidentally drop text from a *different* key that arrived in the same frame.
    if self.keyboard_text_len > 0 {
      let keep = self.text_input.len().saturating_sub(self.keyboard_text_len);
      self.text_input.truncate(keep);
      self.keyboard_text_len = 0;
    }
    self.text_input.push_str(&text);
    self.ime_committed_this_frame = true;
    // Preedit should be cleared by winit before commit, but ensure it's clear
    self.ime_preedit.clear();
    self.ime_preedit_cursor = None;
  }

  /// Handle direct text input from keyboard events (non-IME).
  ///
  /// This is used for regular character input when IME is not composing.
  pub(crate) fn handle_text_input(&mut self, text: &str) {
    // Skip keyboard text input when IME is enabled - let IME handle all text
    // This prevents double input on Windows where both keyboard and IME events fire
    if self.ime_enabled {
      return;
    }
    // Only add text if not currently in IME composition mode
    if self.ime_preedit.is_empty() {
      self.text_input.push_str(text);
      self.keyboard_text_len += text.len();
    }
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Gamepad Input
  // ─────────────────────────────────────────────────────────────────────────────

  /// Returns true if a gamepad is currently connected.
  pub fn gamepad_connected(&self) -> bool {
    self.gamepad_connected
  }

  /// Returns the name of the connected gamepad, or empty string if none.
  pub fn gamepad_name(&self) -> &str {
    &self.gamepad_name
  }

  /// Returns true if the gamepad button is currently held down.
  pub fn gamepad_button_down(&self, button: GamepadButton) -> bool {
    self.gamepad_buttons_down.contains(&button)
  }

  /// Returns true if the gamepad button was just pressed this frame.
  pub fn gamepad_button_pressed(&self, button: GamepadButton) -> bool {
    if self.oneshots_suppressed {
      return false;
    }
    self.gamepad_buttons_pressed.contains(&button)
  }

  /// Returns true if the gamepad button was just released this frame.
  pub fn gamepad_button_released(&self, button: GamepadButton) -> bool {
    if self.oneshots_suppressed {
      return false;
    }
    self.gamepad_buttons_released.contains(&button)
  }

  /// Get the left stick position as (x, y) with deadzone applied.
  /// Values range from -1.0 to 1.0.
  pub fn left_stick(&self) -> (f32, f32) {
    (self.left_stick_x, self.left_stick_y)
  }

  /// Get the left stick X axis with deadzone applied (-1.0 to 1.0).
  pub fn left_stick_x(&self) -> f32 {
    self.left_stick_x
  }

  /// Get the left stick Y axis with deadzone applied (-1.0 to 1.0).
  pub fn left_stick_y(&self) -> f32 {
    self.left_stick_y
  }

  /// Get the right stick position as (x, y) with deadzone applied.
  /// Values range from -1.0 to 1.0.
  pub fn right_stick(&self) -> (f32, f32) {
    (self.right_stick_x, self.right_stick_y)
  }

  /// Get the right stick X axis with deadzone applied (-1.0 to 1.0).
  pub fn right_stick_x(&self) -> f32 {
    self.right_stick_x
  }

  /// Get the right stick Y axis with deadzone applied (-1.0 to 1.0).
  pub fn right_stick_y(&self) -> f32 {
    self.right_stick_y
  }

  /// Get raw left stick position without deadzone (for debug visualization).
  pub fn left_stick_raw(&self) -> (f32, f32) {
    (self.left_stick_x_raw, self.left_stick_y_raw)
  }

  /// Get raw right stick position without deadzone (for debug visualization).
  pub fn right_stick_raw(&self) -> (f32, f32) {
    (self.right_stick_x_raw, self.right_stick_y_raw)
  }

  /// Get the left trigger value (0.0 to 1.0).
  pub fn left_trigger(&self) -> f32 {
    self.left_trigger
  }

  /// Get the right trigger value (0.0 to 1.0).
  pub fn right_trigger(&self) -> f32 {
    self.right_trigger
  }

  /// Handle gamepad connection event.
  pub(crate) fn handle_gamepad_connected(&mut self, id: gilrs::GamepadId, name: String) {
    // Only track first gamepad if none connected
    if !self.gamepad_connected {
      self.gamepad_connected = true;
      self.gamepad_id = Some(id);
      self.gamepad_name = name;
      engine_info!(module: "Gamepad", "Connected: {}", self.gamepad_name);
    }
  }

  /// Handle gamepad disconnection event.
  pub(crate) fn handle_gamepad_disconnected(&mut self, id: gilrs::GamepadId) {
    if self.gamepad_id == Some(id) {
      engine_info!(module: "Gamepad", "Disconnected: {}", self.gamepad_name);
      self.gamepad_connected = false;
      self.gamepad_id = None;
      self.gamepad_name.clear();
      // Clear all gamepad state
      self.gamepad_buttons_down.clear();
      self.gamepad_buttons_pressed.clear();
      self.gamepad_buttons_released.clear();
      self.left_stick_x = 0.0;
      self.left_stick_y = 0.0;
      self.right_stick_x = 0.0;
      self.right_stick_y = 0.0;
      self.left_stick_x_raw = 0.0;
      self.left_stick_y_raw = 0.0;
      self.right_stick_x_raw = 0.0;
      self.right_stick_y_raw = 0.0;
      self.left_trigger = 0.0;
      self.right_trigger = 0.0;
    }
  }

  /// Handle gamepad button event.
  pub(crate) fn handle_gamepad_button(
    &mut self,
    id: gilrs::GamepadId,
    button: gilrs::Button,
    pressed: bool,
  ) {
    // Only process events from the active gamepad
    if self.gamepad_id != Some(id) {
      return;
    }

    let Some(btn) = GamepadButton::from_gilrs(button) else {
      return;
    };

    if pressed {
      if !self.gamepad_buttons_down.contains(&btn) {
        self.gamepad_buttons_pressed.insert(btn);
      }
      self.gamepad_buttons_down.insert(btn);
    } else {
      self.gamepad_buttons_down.remove(&btn);
      self.gamepad_buttons_released.insert(btn);
    }
  }

  /// Handle gamepad axis event.
  pub(crate) fn handle_gamepad_axis(
    &mut self,
    id: gilrs::GamepadId,
    axis: gilrs::Axis,
    value: f32,
  ) {
    // Only process events from the active gamepad
    if self.gamepad_id != Some(id) {
      return;
    }

    use gilrs::Axis;
    match axis {
      Axis::LeftStickX => {
        self.left_stick_x_raw = value;
        let (x, y) = apply_deadzone(value, self.left_stick_y_raw);
        self.left_stick_x = x;
        self.left_stick_y = y;
      }
      Axis::LeftStickY => {
        self.left_stick_y_raw = value;
        let (x, y) = apply_deadzone(self.left_stick_x_raw, value);
        self.left_stick_x = x;
        self.left_stick_y = y;
      }
      Axis::RightStickX => {
        self.right_stick_x_raw = value;
        let (x, y) = apply_deadzone(value, self.right_stick_y_raw);
        self.right_stick_x = x;
        self.right_stick_y = y;
      }
      Axis::RightStickY => {
        self.right_stick_y_raw = value;
        let (x, y) = apply_deadzone(self.right_stick_x_raw, value);
        self.right_stick_x = x;
        self.right_stick_y = y;
      }
      Axis::LeftZ | Axis::RightZ => {
        // Some controllers report triggers as Z axes
        // Z is typically left trigger, RightZ is right trigger
        // However, gilrs normalizes this - we handle it via LeftTrigger2/RightTrigger2 buttons
        // and via dedicated trigger events (handled below)
      }
      _ => {}
    }
  }

  /// Handle gamepad trigger event (analog triggers).
  pub(crate) fn handle_gamepad_trigger(
    &mut self,
    id: gilrs::GamepadId,
    button: gilrs::Button,
    value: f32,
  ) {
    // Only process events from the active gamepad
    if self.gamepad_id != Some(id) {
      return;
    }

    use gilrs::Button;
    match button {
      Button::LeftTrigger2 => {
        self.left_trigger = value;
      }
      Button::RightTrigger2 => {
        self.right_trigger = value;
      }
      _ => {}
    }
  }

  /// Try to set the active gamepad if none is currently active.
  pub(crate) fn try_set_active_gamepad(&mut self, id: gilrs::GamepadId, name: String) {
    if !self.gamepad_connected {
      self.handle_gamepad_connected(id, name);
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

  // ─────────────────────────────────────────────────────────────────────────────
  // Gamepad Tests
  // ─────────────────────────────────────────────────────────────────────────────

  #[test]
  fn deadzone_filters_small_values() {
    let (x, y) = apply_deadzone(0.1, 0.05);
    assert_eq!((x, y), (0.0, 0.0));
  }

  #[test]
  fn deadzone_passes_large_values() {
    let (x, y) = apply_deadzone(1.0, 0.0);
    // Should be close to 1.0 after rescaling
    assert!((x - 1.0).abs() < 0.01);
    assert_eq!(y, 0.0);
  }

  #[test]
  fn deadzone_rescales_values() {
    // Value just above deadzone should be close to 0
    let (x, _) = apply_deadzone(STICK_DEADZONE + 0.01, 0.0);
    assert!(x > 0.0);
    assert!(x < 0.02); // Small value just above deadzone
  }

  #[test]
  fn gamepad_not_connected_by_default() {
    let input = InputState::new();
    assert!(!input.gamepad_connected());
    assert!(input.gamepad_name().is_empty());
  }

  #[test]
  fn gamepad_stick_values_default_to_zero() {
    let input = InputState::new();
    assert_eq!(input.left_stick(), (0.0, 0.0));
    assert_eq!(input.right_stick(), (0.0, 0.0));
    assert_eq!(input.left_trigger(), 0.0);
    assert_eq!(input.right_trigger(), 0.0);
  }

  #[test]
  fn gamepad_button_enum_conversion() {
    // Test a few button conversions
    assert_eq!(
      GamepadButton::from_gilrs(gilrs::Button::South),
      Some(GamepadButton::South)
    );
    assert_eq!(
      GamepadButton::from_gilrs(gilrs::Button::East),
      Some(GamepadButton::East)
    );
    assert_eq!(
      GamepadButton::from_gilrs(gilrs::Button::West),
      Some(GamepadButton::West)
    );
    assert_eq!(
      GamepadButton::from_gilrs(gilrs::Button::North),
      Some(GamepadButton::North)
    );
    assert_eq!(
      GamepadButton::from_gilrs(gilrs::Button::DPadUp),
      Some(GamepadButton::DPadUp)
    );
    assert_eq!(
      GamepadButton::from_gilrs(gilrs::Button::Start),
      Some(GamepadButton::Start)
    );
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Fixed Timestep Integration Tests
  // These tests simulate the engine's fixed timestep loop where multiple
  // updates can run in a single frame. Input should be consumed after
  // each update to prevent double-processing of one-shot events.
  // ─────────────────────────────────────────────────────────────────────────────

  #[test]
  fn key_pressed_only_fires_once_across_multiple_updates() {
    let mut input = InputState::new();

    // Simulate: key pressed before frame starts
    input.handle_keyboard(KeyCode::Escape, ElementState::Pressed);

    // First update sees the press
    assert!(
      input.key_pressed(KeyCode::Escape),
      "First update should see key_pressed"
    );
    assert!(input.key_down(KeyCode::Escape), "Key should be held");

    // After first update, clear per-frame input (as engine does)
    input.begin_frame();

    // Second update in same frame should NOT see the press again
    assert!(
      !input.key_pressed(KeyCode::Escape),
      "Second update should NOT see key_pressed"
    );
    assert!(input.key_down(KeyCode::Escape), "Key should still be held");

    // Third update - still no press event
    input.begin_frame();
    assert!(
      !input.key_pressed(KeyCode::Escape),
      "Third update should NOT see key_pressed"
    );
    assert!(input.key_down(KeyCode::Escape), "Key should still be held");
  }

  #[test]
  fn key_down_persists_across_multiple_updates() {
    let mut input = InputState::new();

    // Simulate: key pressed and held
    input.handle_keyboard(KeyCode::KeyW, ElementState::Pressed);

    // Simulate multiple fixed timestep updates (like movement input)
    for i in 0..5 {
      assert!(
        input.key_down(KeyCode::KeyW),
        "Update {} should see key_down for held key",
        i + 1
      );
      input.begin_frame();
    }

    // Key still held after all updates
    assert!(
      input.key_down(KeyCode::KeyW),
      "Key should remain held after all updates"
    );

    // Only when released does it stop
    input.handle_keyboard(KeyCode::KeyW, ElementState::Released);
    assert!(
      !input.key_down(KeyCode::KeyW),
      "Key should no longer be held after release"
    );
  }

  #[test]
  fn mouse_pressed_persists_for_ui_rendering() {
    let mut input = InputState::new();

    // Simulate: mouse button pressed
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);

    // First update sees the press
    assert!(
      input.mouse_pressed(MouseButton::Left),
      "First update should see mouse_pressed"
    );
    assert!(input.mouse_down(MouseButton::Left), "Button should be held");

    // After first update, only consume keyboard events (engine behavior)
    input.consume_keyboard_events();

    // Mouse pressed should STILL be visible for UI rendering
    assert!(
      input.mouse_pressed(MouseButton::Left),
      "Mouse pressed should persist for render_ui"
    );
    assert!(
      input.mouse_down(MouseButton::Left),
      "Button should still be held"
    );

    // Only begin_frame clears mouse state (at end of frame)
    input.begin_frame();
    assert!(
      !input.mouse_pressed(MouseButton::Left),
      "Mouse pressed should be cleared after begin_frame"
    );
  }

  #[test]
  fn toggle_action_only_toggles_once_per_keypress() {
    let mut input = InputState::new();
    let mut is_paused = false;

    // Simulate: user presses Escape to toggle pause
    input.handle_keyboard(KeyCode::Escape, ElementState::Pressed);

    // Simulate 3 fixed timestep updates in one frame (e.g., frame stutter)
    for _ in 0..3 {
      // Game logic: toggle pause on key_pressed
      if input.key_pressed(KeyCode::Escape) {
        is_paused = !is_paused;
      }
      input.begin_frame();
    }

    // Should only have toggled ONCE
    assert!(
      is_paused,
      "Pause should be toggled exactly once, not back to false"
    );
  }

  #[test]
  fn continuous_action_works_across_multiple_updates() {
    let mut input = InputState::new();
    let mut move_count = 0;

    // Simulate: user holds W key for movement
    input.handle_keyboard(KeyCode::KeyW, ElementState::Pressed);

    // Simulate 3 fixed timestep updates
    for _ in 0..3 {
      // Game logic: move while key is held
      if input.key_down(KeyCode::KeyW) {
        move_count += 1;
      }
      input.begin_frame();
    }

    // Should have moved in ALL 3 updates
    assert_eq!(
      move_count, 3,
      "Movement should occur in all updates while key is held"
    );
  }

  #[test]
  fn key_released_only_fires_once_across_multiple_updates() {
    let mut input = InputState::new();

    // Press and release key
    input.handle_keyboard(KeyCode::Space, ElementState::Pressed);
    input.handle_keyboard(KeyCode::Space, ElementState::Released);

    // First update sees the release
    assert!(
      input.key_released(KeyCode::Space),
      "First update should see key_released"
    );

    input.begin_frame();

    // Second update should NOT see the release again
    assert!(
      !input.key_released(KeyCode::Space),
      "Second update should NOT see key_released"
    );
  }

  #[test]
  fn consume_keyboard_events_only_clears_keyboard() {
    let mut input = InputState::new();

    // Set up both keyboard and mouse input
    input.handle_keyboard(KeyCode::Escape, ElementState::Pressed);
    input.handle_keyboard(KeyCode::Space, ElementState::Released);
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    input.handle_mouse_move(100.0, 200.0);
    input.handle_scroll(MouseScrollDelta::LineDelta(1.0, 2.0));

    // Verify all input is present
    assert!(input.key_pressed(KeyCode::Escape));
    assert!(input.key_released(KeyCode::Space));
    assert!(input.mouse_pressed(MouseButton::Left));
    assert_eq!(input.mouse_delta(), (100.0, 200.0));
    assert_eq!(input.scroll_delta(), (1.0, 2.0));

    // Consume only keyboard events
    input.consume_keyboard_events();

    // Keyboard one-shot events should be cleared
    assert!(
      !input.key_pressed(KeyCode::Escape),
      "key_pressed should be cleared"
    );
    assert!(
      !input.key_released(KeyCode::Space),
      "key_released should be cleared"
    );

    // But keyboard held state should persist
    assert!(input.key_down(KeyCode::Escape), "key_down should persist");

    // Mouse and scroll should be PRESERVED (for UI rendering)
    assert!(
      input.mouse_pressed(MouseButton::Left),
      "mouse_pressed should persist"
    );
    assert!(
      input.mouse_down(MouseButton::Left),
      "mouse_down should persist"
    );
    assert_eq!(
      input.mouse_delta(),
      (100.0, 200.0),
      "mouse_delta should persist"
    );
    assert_eq!(
      input.scroll_delta(),
      (1.0, 2.0),
      "scroll_delta should persist"
    );
  }

  #[test]
  fn consume_key_removes_specific_key_only() {
    let mut input = InputState::new();

    // Press multiple keys
    input.handle_keyboard(KeyCode::F1, ElementState::Pressed);
    input.handle_keyboard(KeyCode::F2, ElementState::Pressed);
    input.handle_keyboard(KeyCode::Space, ElementState::Pressed);

    // Verify all are pressed
    assert!(input.key_pressed(KeyCode::F1));
    assert!(input.key_pressed(KeyCode::F2));
    assert!(input.key_pressed(KeyCode::Space));

    // Consume only F1
    input.consume_key(KeyCode::F1);

    // F1 should be consumed, others should remain
    assert!(!input.key_pressed(KeyCode::F1), "F1 should be consumed");
    assert!(input.key_pressed(KeyCode::F2), "F2 should remain");
    assert!(input.key_pressed(KeyCode::Space), "Space should remain");
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Input Consumption Utility Tests
  // ─────────────────────────────────────────────────────────────────────────────

  #[test]
  fn key_pressed_and_consume_returns_true_and_consumes() {
    let mut input = InputState::new();
    input.handle_keyboard(KeyCode::Escape, ElementState::Pressed);

    // First call returns true and consumes
    assert!(input.key_pressed_and_consume(KeyCode::Escape));

    // Second call returns false (already consumed)
    assert!(!input.key_pressed_and_consume(KeyCode::Escape));

    // Regular key_pressed also returns false
    assert!(!input.key_pressed(KeyCode::Escape));

    // But key_down still works (key is held)
    assert!(input.key_down(KeyCode::Escape));
  }

  #[test]
  fn key_pressed_and_consume_returns_false_if_not_pressed() {
    let mut input = InputState::new();
    assert!(!input.key_pressed_and_consume(KeyCode::Escape));
  }

  #[test]
  fn consume_key_press_removes_without_return() {
    let mut input = InputState::new();
    input.handle_keyboard(KeyCode::Space, ElementState::Pressed);

    assert!(input.key_pressed(KeyCode::Space));
    input.consume_key_press(KeyCode::Space);
    assert!(!input.key_pressed(KeyCode::Space));
  }

  #[test]
  fn mouse_pressed_and_consume_returns_true_and_consumes() {
    let mut input = InputState::new();
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);

    // First call returns true and consumes
    assert!(input.mouse_pressed_and_consume(MouseButton::Left));

    // Second call returns false (already consumed)
    assert!(!input.mouse_pressed_and_consume(MouseButton::Left));

    // Regular mouse_pressed also returns false
    assert!(!input.mouse_pressed(MouseButton::Left));

    // But mouse_down still works (button is held)
    assert!(input.mouse_down(MouseButton::Left));
  }

  #[test]
  fn mouse_pressed_and_consume_returns_false_if_not_pressed() {
    let mut input = InputState::new();
    assert!(!input.mouse_pressed_and_consume(MouseButton::Right));
  }

  #[test]
  fn consume_mouse_press_removes_without_return() {
    let mut input = InputState::new();
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);

    assert!(input.mouse_pressed(MouseButton::Left));
    input.consume_mouse_press(MouseButton::Left);
    assert!(!input.mouse_pressed(MouseButton::Left));
  }

  #[test]
  fn gamepad_button_pressed_and_consume_returns_true_and_consumes() {
    let mut input = InputState::new();
    // Manually insert a gamepad button press (since we can't easily simulate gilrs)
    input.gamepad_buttons_pressed.insert(GamepadButton::South);
    input.gamepad_buttons_down.insert(GamepadButton::South);

    // First call returns true and consumes
    assert!(input.gamepad_button_pressed_and_consume(GamepadButton::South));

    // Second call returns false (already consumed)
    assert!(!input.gamepad_button_pressed_and_consume(GamepadButton::South));

    // Regular gamepad_button_pressed also returns false
    assert!(!input.gamepad_button_pressed(GamepadButton::South));

    // But gamepad_button_down still works (button is held)
    assert!(input.gamepad_button_down(GamepadButton::South));
  }

  #[test]
  fn consume_gamepad_button_press_removes_without_return() {
    let mut input = InputState::new();
    input.gamepad_buttons_pressed.insert(GamepadButton::East);

    assert!(input.gamepad_button_pressed(GamepadButton::East));
    input.consume_gamepad_button_press(GamepadButton::East);
    assert!(!input.gamepad_button_pressed(GamepadButton::East));
  }

  #[test]
  fn consume_prevents_double_handling_pattern() {
    let mut input = InputState::new();
    input.handle_keyboard(KeyCode::Escape, ElementState::Pressed);

    let mut menu_handled = false;
    let mut game_handled = false;

    // Menu system checks first and consumes
    if input.key_pressed_and_consume(KeyCode::Escape) {
      menu_handled = true;
    }

    // Game system checks after - should not see the press
    if input.key_pressed(KeyCode::Escape) {
      game_handled = true;
    }

    assert!(menu_handled, "Menu should have handled Escape");
    assert!(!game_handled, "Game should NOT have handled Escape");
  }

  /// Regression test: keyboard events must persist for the entire frame.
  ///
  /// Previously, consume_keyboard_events() was called after game update() but
  /// before render_ui(), causing UI widgets (text input, dropdowns) to never
  /// see keyboard events. The fix ensures keyboard events persist until
  /// begin_frame() is called at frame end.
  ///
  /// Frame flow should be:
  /// 1. Events received -> key_pressed() returns true
  /// 2. update() runs (possibly multiple times for fixed timestep)
  /// 3. render() + render_ui() runs -> key_pressed() MUST still return true
  /// 4. begin_frame() clears events -> key_pressed() returns false
  #[test]
  fn keyboard_events_persist_until_begin_frame() {
    let mut input = InputState::new();

    // Simulate key press event at start of frame
    input.handle_keyboard(KeyCode::KeyA, ElementState::Pressed);
    input.handle_keyboard(KeyCode::ControlLeft, ElementState::Pressed);

    // Verify events are present (simulating update phase)
    assert!(
      input.key_pressed(KeyCode::KeyA),
      "key_pressed should be true during update"
    );
    assert!(
      input.key_down(KeyCode::ControlLeft),
      "modifier should be held during update"
    );

    // Read again (simulating multiple fixed timestep updates)
    // Events should STILL be present
    assert!(
      input.key_pressed(KeyCode::KeyA),
      "key_pressed should persist across multiple update reads"
    );

    // Read again (simulating render_ui phase - the critical regression case)
    // This is where the bug manifested: UI couldn't see keyboard events
    assert!(
      input.key_pressed(KeyCode::KeyA),
      "key_pressed MUST be available during render_ui (regression test)"
    );
    assert!(
      input.key_down(KeyCode::ControlLeft),
      "modifier MUST be available for Ctrl+A detection in UI"
    );

    // Now simulate frame end
    input.begin_frame();

    // After begin_frame, one-shot events should be cleared
    assert!(
      !input.key_pressed(KeyCode::KeyA),
      "key_pressed should be cleared after begin_frame"
    );

    // But held state persists (key wasn't released)
    assert!(
      input.key_down(KeyCode::KeyA),
      "key_down should persist (key still held)"
    );
    assert!(
      input.key_down(KeyCode::ControlLeft),
      "modifier should persist (key still held)"
    );
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Oneshot Suppression Tests
  // ─────────────────────────────────────────────────────────────────────────────

  #[test]
  fn oneshot_suppression_blocks_pressed_events() {
    let mut input = InputState::new();
    input.handle_keyboard(KeyCode::Escape, ElementState::Pressed);
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    input.gamepad_buttons_pressed.insert(GamepadButton::South);

    input.suppress_oneshots();

    assert!(!input.key_pressed(KeyCode::Escape));
    assert!(!input.mouse_pressed(MouseButton::Left));
    assert!(!input.gamepad_button_pressed(GamepadButton::South));
  }

  #[test]
  fn oneshot_suppression_blocks_released_events() {
    let mut input = InputState::new();
    input.handle_keyboard(KeyCode::Escape, ElementState::Pressed);
    input.handle_keyboard(KeyCode::Escape, ElementState::Released);
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    input.handle_mouse_button(MouseButton::Left, ElementState::Released);
    input.gamepad_buttons_released.insert(GamepadButton::South);

    input.suppress_oneshots();

    assert!(!input.key_released(KeyCode::Escape));
    assert!(!input.mouse_released(MouseButton::Left));
    assert!(!input.gamepad_button_released(GamepadButton::South));
  }

  #[test]
  fn oneshot_suppression_blocks_key_repeat() {
    let mut input = InputState::new();
    input.handle_keyboard(KeyCode::ArrowLeft, ElementState::Pressed);

    assert!(input.key_repeat(KeyCode::ArrowLeft));

    input.suppress_oneshots();

    assert!(!input.key_repeat(KeyCode::ArrowLeft));

    input.unsuppress_oneshots();
    assert!(input.key_repeat(KeyCode::ArrowLeft));
  }

  #[test]
  fn oneshot_suppression_blocks_text_input() {
    let mut input = InputState::new();
    input.handle_text_input("hello");

    assert!(input.has_text_input());
    assert_eq!(input.text_input(), "hello");

    input.suppress_oneshots();

    assert!(!input.has_text_input());
    assert_eq!(input.text_input(), "");
  }

  #[test]
  fn oneshot_suppression_does_not_affect_down_state() {
    let mut input = InputState::new();
    input.handle_keyboard(KeyCode::KeyW, ElementState::Pressed);
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    input.gamepad_buttons_down.insert(GamepadButton::South);

    input.suppress_oneshots();

    assert!(input.key_down(KeyCode::KeyW));
    assert!(input.mouse_down(MouseButton::Left));
    assert!(input.gamepad_button_down(GamepadButton::South));
  }

  #[test]
  fn suppressed_and_consume_does_not_remove_event() {
    let mut input = InputState::new();
    input.handle_keyboard(KeyCode::Escape, ElementState::Pressed);
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    input.gamepad_buttons_pressed.insert(GamepadButton::South);

    input.suppress_oneshots();

    // _and_consume should return false WITHOUT removing events
    assert!(!input.key_pressed_and_consume(KeyCode::Escape));
    assert!(!input.mouse_pressed_and_consume(MouseButton::Left));
    assert!(!input.gamepad_button_pressed_and_consume(GamepadButton::South));

    // After unsuppressing, the events should still be there
    input.unsuppress_oneshots();
    assert!(input.key_pressed(KeyCode::Escape));
    assert!(input.mouse_pressed(MouseButton::Left));
    assert!(input.gamepad_button_pressed(GamepadButton::South));
  }

  #[test]
  fn unsuppress_restores_oneshot_visibility() {
    let mut input = InputState::new();
    input.handle_keyboard(KeyCode::Space, ElementState::Pressed);

    input.suppress_oneshots();
    assert!(!input.key_pressed(KeyCode::Space));

    input.unsuppress_oneshots();
    assert!(input.key_pressed(KeyCode::Space));
  }

  #[test]
  fn begin_frame_resets_suppression() {
    let mut input = InputState::new();
    input.suppress_oneshots();

    input.begin_frame();
    input.handle_keyboard(KeyCode::Space, ElementState::Pressed);

    // After begin_frame, suppression should be cleared
    assert!(input.key_pressed(KeyCode::Space));
  }

  #[test]
  fn fixed_timestep_simulation() {
    let mut input = InputState::new();

    // Simulate: key pressed before frame starts
    input.handle_keyboard(KeyCode::Escape, ElementState::Pressed);
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);

    // First update sees the events
    assert!(input.key_pressed(KeyCode::Escape));
    assert!(input.mouse_pressed(MouseButton::Left));

    // Engine suppresses after first update
    input.suppress_oneshots();

    // Second update does NOT see them
    assert!(!input.key_pressed(KeyCode::Escape));
    assert!(!input.mouse_pressed(MouseButton::Left));

    // Third update does NOT see them
    assert!(!input.key_pressed(KeyCode::Escape));
    assert!(!input.mouse_pressed(MouseButton::Left));

    // Engine unsuppresses before render
    input.unsuppress_oneshots();

    // render_ui CAN see them
    assert!(input.key_pressed(KeyCode::Escape));
    assert!(input.mouse_pressed(MouseButton::Left));

    // Continuous state works throughout
    assert!(input.key_down(KeyCode::Escape));
    assert!(input.mouse_down(MouseButton::Left));
  }
}
