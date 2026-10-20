use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::KeyCode;

use crate::core::ecs::kernel::Resource;
use crate::engine_info;

pub(crate) mod actions;
mod labels;

pub use actions::{Actions, Binding, Direction, StickDirection};
pub use labels::PadFamily;

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
/// A stick direction goes down past this deflection and up below the release one
const STICK_DIGITAL_PRESS: f32 = 0.5;
const STICK_DIGITAL_RELEASE: f32 = 0.4;

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

/// The kind of device the player used last: prompts and the focus ring follow it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InputDevice {
  #[default]
  Mouse,
  Keyboard,
  Gamepad,
}

/// A gamepad stick
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stick {
  Left,
  Right,
}

/// Trackpad pixels that make one line of [`Input::scroll_delta`]
const PIXELS_PER_LINE: f32 = 120.0;

/// A window scroll with its pixels in logical pixels: winit reports
/// trackpads in physical pixels, so they are divided by the window's
/// `scale_factor` (a bad factor leaves them as they are). Lines pass through.
pub(crate) fn logical_scroll(delta: MouseScrollDelta, scale_factor: f64) -> MouseScrollDelta {
  match delta {
    MouseScrollDelta::PixelDelta(pos) if scale_factor.is_finite() && scale_factor > 0.0 => {
      MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(
        pos.x / scale_factor,
        pos.y / scale_factor,
      ))
    }
    other => other,
  }
}

/// A window or gamepad event, applied at the next `begin_frame`.
#[derive(Debug, Clone)]
pub enum InputEvent {
  Key {
    key: KeyCode,
    pressed: bool,
  },
  Text(String),
  MouseButton {
    button: MouseButton,
    pressed: bool,
  },
  MouseMove {
    x: f64,
    y: f64,
  },
  RawMouseMotion {
    dx: f64,
    dy: f64,
  },
  /// A wheel step in lines, or a trackpad movement in logical pixels
  Scroll(MouseScrollDelta),
  ImeEnabled,
  ImeDisabled,
  ImePreedit {
    text: String,
    cursor: Option<(usize, usize)>,
  },
  ImeCommit(String),
  GamepadConnected {
    id: gilrs::GamepadId,
    name: String,
  },
  GamepadDisconnected(gilrs::GamepadId),
  GamepadButton {
    id: gilrs::GamepadId,
    button: gilrs::Button,
    pressed: bool,
  },
  GamepadTrigger {
    id: gilrs::GamepadId,
    button: gilrs::Button,
    value: f32,
  },
  GamepadAxis {
    id: gilrs::GamepadId,
    axis: gilrs::Axis,
    value: f32,
  },
  /// A gamepad button with no gilrs id: scripted and headless input. Applies
  /// to the tracked pad, or connects a virtual one
  PadButton {
    button: GamepadButton,
    pressed: bool,
  },
  /// A stick position (-1..1 each axis, +y up) with no gilrs id
  PadStick {
    stick: Stick,
    x: f32,
    y: f32,
  },
}

/// Input state for the frame, a resource and `SceneContext::input`.
///
/// Events are queued as they arrive and applied at `begin_frame`. One-shot
/// queries (`just pressed`, `just released`) have two views: the frame view,
/// true for exactly one frame, and the fixed view, true in the first fixed
/// step after the event even when that step runs in a later frame. Consuming
/// an input hides it from both for the rest of the frame.
#[derive(Debug, Default, Resource)]
pub struct Input {
  pending: Vec<InputEvent>,

  // Keyboard state
  keys_down: HashSet<KeyCode>,
  keys_pressed: HashSet<KeyCode>,
  keys_released: HashSet<KeyCode>,
  keys_pressed_fixed: HashSet<KeyCode>,
  keys_released_fixed: HashSet<KeyCode>,
  /// Keys that fired this frame (includes initial press AND repeats)
  keys_repeated: HashSet<KeyCode>,
  consumed_keys: HashSet<KeyCode>,

  // Mouse button state
  mouse_buttons_down: HashSet<MouseButton>,
  mouse_buttons_pressed: HashSet<MouseButton>,
  mouse_buttons_released: HashSet<MouseButton>,
  mouse_buttons_pressed_fixed: HashSet<MouseButton>,
  mouse_buttons_released_fixed: HashSet<MouseButton>,
  consumed_mouse: HashSet<MouseButton>,

  // Mouse position (in pixels, relative to window)
  mouse_x: f32,
  mouse_y: f32,
  mouse_delta_x: f32,
  mouse_delta_y: f32,

  // Mouse scroll
  /// Wheel lines this frame (`LineDelta` only)
  scroll_lines: (f32, f32),
  /// Trackpad movement this frame, in logical pixels (`PixelDelta` only)
  scroll_px: (f32, f32),
  /// Both for the fixed steps: kept until a step has seen them
  scroll_lines_fixed: (f32, f32),
  scroll_px_fixed: (f32, f32),

  /// The device behind the newest input
  last_device: InputDevice,

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
  text_input_consumed: AtomicBool,

  // Gamepad state (first connected gamepad only)
  gamepad_connected: bool,
  gamepad_name: String,
  gamepad_id: Option<gilrs::GamepadId>,
  gamepad_buttons_down: HashSet<GamepadButton>,
  gamepad_buttons_pressed: HashSet<GamepadButton>,
  gamepad_buttons_released: HashSet<GamepadButton>,
  gamepad_buttons_pressed_fixed: HashSet<GamepadButton>,
  gamepad_buttons_released_fixed: HashSet<GamepadButton>,
  consumed_gamepad: HashSet<GamepadButton>,
  /// Stick directions held past the digital threshold
  stick_dirs_down: HashSet<StickDirection>,
  stick_dirs_pressed: HashSet<StickDirection>,
  stick_dirs_released: HashSet<StickDirection>,
  stick_dirs_pressed_fixed: HashSet<StickDirection>,
  stick_dirs_released_fixed: HashSet<StickDirection>,
  consumed_stick_dirs: HashSet<StickDirection>,
  left_stick_x: f32,
  left_stick_y: f32,
  right_stick_x: f32,
  right_stick_y: f32,
  left_stick_x_raw: f32,
  left_stick_y_raw: f32,
  right_stick_x_raw: f32,
  right_stick_y_raw: f32,
  left_trigger: f32,
  right_trigger: f32,

  /// `Some(i)` while fixed step `i` of the frame runs; one-shots then read
  /// the fixed view (step 0) or nothing (later steps)
  fixed_step: Option<u32>,

  /// Input hidden from queries until `unblock` or the next frame
  block: InputBlock,

  actions: Actions,
}

/// What `Input::block_pointer` / `block_all` hide
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum InputBlock {
  #[default]
  None,
  Pointer,
  All,
}

/// Which one-shot view a query reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OneShots {
  Frame,
  Fixed,
  Suppressed,
}

impl Input {
  pub fn new() -> Self {
    Input {
      actions: Actions::with_ui_defaults(),
      ..Input::default()
    }
  }

  /// Queue an event; it takes effect at the next `begin_frame`.
  pub fn queue(&mut self, event: InputEvent) {
    self.pending.push(event);
  }

  /// Start a frame: forget last frame's one-shots and consumptions, then
  /// apply everything queued since the previous frame.
  pub fn begin_frame(&mut self) {
    self.keys_pressed.clear();
    self.keys_released.clear();
    self.keys_repeated.clear();
    self.consumed_keys.clear();
    self.mouse_buttons_pressed.clear();
    self.mouse_buttons_released.clear();
    self.consumed_mouse.clear();
    self.mouse_delta_x = 0.0;
    self.mouse_delta_y = 0.0;
    self.scroll_lines = (0.0, 0.0);
    self.scroll_px = (0.0, 0.0);
    // Clear committed text, but keep preedit (it persists until cleared by IME)
    self.text_input.clear();
    self.ime_committed_this_frame = false;
    self.keyboard_text_len = 0;
    self.text_input_consumed.store(false, Ordering::Relaxed);
    self.gamepad_buttons_pressed.clear();
    self.gamepad_buttons_released.clear();
    self.consumed_gamepad.clear();
    self.stick_dirs_pressed.clear();
    self.stick_dirs_released.clear();
    self.consumed_stick_dirs.clear();
    self.fixed_step = None;
    self.block = InputBlock::None;
    let pending = std::mem::take(&mut self.pending);
    for event in pending {
      self.apply(event);
    }
  }

  fn apply(&mut self, event: InputEvent) {
    match event {
      InputEvent::Key { key, pressed } => self.handle_keyboard(
        key,
        if pressed {
          ElementState::Pressed
        } else {
          ElementState::Released
        },
      ),
      InputEvent::Text(text) => self.handle_text_input(&text),
      InputEvent::MouseButton { button, pressed } => self.handle_mouse_button(
        button,
        if pressed {
          ElementState::Pressed
        } else {
          ElementState::Released
        },
      ),
      InputEvent::MouseMove { x, y } => self.handle_mouse_move(x, y),
      InputEvent::RawMouseMotion { dx, dy } => self.handle_raw_mouse_motion(dx, dy),
      InputEvent::Scroll(delta) => self.handle_scroll(delta),
      InputEvent::ImeEnabled => self.handle_ime_enabled(),
      InputEvent::ImeDisabled => self.handle_ime_disabled(),
      InputEvent::ImePreedit { text, cursor } => self.handle_ime_preedit(text, cursor),
      InputEvent::ImeCommit(text) => self.handle_ime_commit(text),
      InputEvent::GamepadConnected { id, name } => self.handle_gamepad_connected(id, name),
      InputEvent::GamepadDisconnected(id) => self.handle_gamepad_disconnected(id),
      InputEvent::GamepadButton {
        id,
        button,
        pressed,
      } => self.handle_gamepad_button(id, button, pressed),
      InputEvent::GamepadTrigger { id, button, value } => {
        self.handle_gamepad_trigger(id, button, value)
      }
      InputEvent::GamepadAxis { id, axis, value } => self.handle_gamepad_axis(id, axis, value),
      InputEvent::PadButton { button, pressed } => {
        self.connect_virtual_pad();
        self.set_gamepad_button(button, pressed);
      }
      InputEvent::PadStick { stick, x, y } => {
        self.connect_virtual_pad();
        self.set_stick_raw(stick, x, y);
      }
    }
  }

  /// Which fixed step of the frame is running: one-shots show in step 0
  /// (from the fixed view) and are suppressed in later steps.
  pub fn set_fixed_step(&mut self, index: u32) {
    self.fixed_step = Some(index);
  }

  /// The fixed loop is over. If a step ran, the fixed view's one-shots were
  /// seen and are cleared; otherwise they wait for the next frame's first step.
  pub fn end_fixed_steps(&mut self, ran_any: bool) {
    self.fixed_step = None;
    if ran_any {
      self.keys_pressed_fixed.clear();
      self.keys_released_fixed.clear();
      self.mouse_buttons_pressed_fixed.clear();
      self.mouse_buttons_released_fixed.clear();
      self.gamepad_buttons_pressed_fixed.clear();
      self.gamepad_buttons_released_fixed.clear();
      self.stick_dirs_pressed_fixed.clear();
      self.stick_dirs_released_fixed.clear();
      self.scroll_lines_fixed = (0.0, 0.0);
      self.scroll_px_fixed = (0.0, 0.0);
    }
  }

  /// The fixed step index while one runs, else 0.
  pub fn fixed_step(&self) -> u32 {
    self.fixed_step.unwrap_or(0)
  }

  #[cfg(test)]
  /// Old name for entering a later fixed step (one-shots suppressed).
  pub(crate) fn suppress_oneshots(&mut self) {
    self.fixed_step = Some(1);
  }

  #[cfg(test)]
  /// Old name for leaving the fixed loop without clearing the fixed view.
  pub(crate) fn unsuppress_oneshots(&mut self) {
    self.fixed_step = None;
  }

  fn oneshots(&self) -> OneShots {
    match self.fixed_step {
      None => OneShots::Frame,
      Some(0) => OneShots::Fixed,
      Some(_) => OneShots::Suppressed,
    }
  }

  fn one_shot<T: std::hash::Hash + Eq>(
    &self,
    frame: &HashSet<T>,
    fixed: &HashSet<T>,
    value: &T,
  ) -> bool {
    match self.oneshots() {
      OneShots::Frame => frame.contains(value),
      OneShots::Fixed => fixed.contains(value),
      OneShots::Suppressed => false,
    }
  }

  /// Hide mouse presses, held buttons, mouse motion and scrolling from every
  /// query until [`unblock`](Self::unblock) or the next frame. The cursor
  /// position still reads, so hover keeps working, and releases still report,
  /// so a drag begun before the block can end.
  pub fn block_pointer(&mut self) {
    self.block = InputBlock::Pointer;
  }

  /// Hide keyboard, text, mouse and gamepad input from every query until
  /// [`unblock`](Self::unblock) or the next frame. As with
  /// [`block_pointer`](Self::block_pointer), the cursor position and releases
  /// still report. Consuming calls do nothing while blocked, so the input is
  /// still there once unblocked.
  pub fn block_all(&mut self) {
    self.block = InputBlock::All;
  }

  /// Undo [`block_pointer`](Self::block_pointer) or [`block_all`](Self::block_all)
  pub fn unblock(&mut self) {
    self.block = InputBlock::None;
  }

  fn keys_blocked(&self) -> bool {
    self.block == InputBlock::All
  }

  fn pointer_blocked(&self) -> bool {
    self.block != InputBlock::None
  }

  #[cfg(test)]
  /// Old name: clears key repeats between frames (begin_frame does this now).
  pub(crate) fn post_render_cleanup(&mut self) {
    self.keys_repeated.clear();
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Input Consumption
  // ─────────────────────────────────────────────────────────────────────────────

  /// Hide a key from every query for the rest of the frame, including the
  /// fixed view, so gameplay never sees a press the UI took.
  pub fn consume_key(&mut self, key: KeyCode) {
    if self.keys_blocked() {
      return;
    }
    self.keys_pressed.remove(&key);
    self.keys_pressed_fixed.remove(&key);
    self.keys_released_fixed.remove(&key);
    self.consumed_keys.insert(key);
  }

  /// Hide a mouse button for the rest of the frame, including the fixed view.
  pub fn consume_mouse(&mut self, button: MouseButton) {
    if self.pointer_blocked() {
      return;
    }
    self.mouse_buttons_pressed.remove(&button);
    self.mouse_buttons_pressed_fixed.remove(&button);
    self.mouse_buttons_released_fixed.remove(&button);
    self.consumed_mouse.insert(button);
  }

  /// Hide a gamepad button for the rest of the frame, including the fixed view.
  pub fn consume_gamepad_button(&mut self, button: GamepadButton) {
    if self.keys_blocked() {
      return;
    }
    self.gamepad_buttons_pressed.remove(&button);
    self.gamepad_buttons_pressed_fixed.remove(&button);
    self.consumed_gamepad.insert(button);
  }

  /// Hide whatever a binding reads for the rest of the frame (the UI's
  /// navigation consumes the bindings it uses).
  pub(crate) fn consume_binding(&mut self, binding: &Binding) {
    match *binding {
      Binding::Key(key) => self.consume_key(key),
      Binding::Mouse(button) => self.consume_mouse(button),
      Binding::Pad(button) => self.consume_gamepad_button(button),
      Binding::Stick(dir) => {
        if self.keys_blocked() {
          return;
        }
        self.stick_dirs_pressed.remove(&dir);
        self.stick_dirs_pressed_fixed.remove(&dir);
        self.consumed_stick_dirs.insert(dir);
      }
    }
  }

  /// Hide this frame's mouse wheel for the rest of the frame.
  pub fn consume_scroll(&mut self) {
    if self.pointer_blocked() {
      return;
    }
    self.scroll_lines = (0.0, 0.0);
    self.scroll_px = (0.0, 0.0);
    self.scroll_lines_fixed = (0.0, 0.0);
    self.scroll_px_fixed = (0.0, 0.0);
  }

  /// Hide this frame's text from later readers.
  pub fn consume_text(&mut self) {
    self.consume_text_input();
  }

  /// Check if a key was pressed this frame AND consume it, preventing other systems from seeing it.
  pub fn key_pressed_and_consume(&mut self, key: KeyCode) -> bool {
    if self.key_pressed(key) {
      self.consume_key(key);
      true
    } else {
      false
    }
  }

  /// Consume a key press without checking if it was pressed.
  pub fn consume_key_press(&mut self, key: KeyCode) {
    self.consume_key(key);
  }

  /// Check if a mouse button was pressed this frame AND consume it.
  pub fn mouse_pressed_and_consume(&mut self, button: MouseButton) -> bool {
    if self.mouse_pressed(button) {
      self.consume_mouse(button);
      true
    } else {
      false
    }
  }

  /// Consume a mouse button press without checking if it was pressed.
  pub fn consume_mouse_press(&mut self, button: MouseButton) {
    self.consume_mouse(button);
  }

  /// Check if a gamepad button was pressed this frame AND consume it.
  pub fn gamepad_button_pressed_and_consume(&mut self, button: GamepadButton) -> bool {
    if self.gamepad_button_pressed(button) {
      self.consume_gamepad_button(button);
      true
    } else {
      false
    }
  }

  /// Consume a gamepad button press without checking if it was pressed.
  pub fn consume_gamepad_button_press(&mut self, button: GamepadButton) {
    self.consume_gamepad_button(button);
  }

  /// Clear all keyboard state for this frame.
  ///
  /// Used by the developer console to block game input while typing.
  pub fn clear_keyboard_state(&mut self) {
    self.keys_pressed.clear();
    self.keys_released.clear();
    self.keys_repeated.clear();
    self.keys_pressed_fixed.clear();
    self.keys_released_fixed.clear();
    self.keys_down.clear();
    self.text_input.clear();
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Actions
  // ─────────────────────────────────────────────────────────────────────────────

  /// The named actions and their bindings
  pub fn actions(&self) -> &Actions {
    &self.actions
  }

  /// Bind, rebind and reset actions
  pub fn actions_mut(&mut self) -> &mut Actions {
    &mut self.actions
  }

  /// Whether any binding of the action was just pressed (a one-shot)
  pub fn action_pressed(&self, name: &str) -> bool {
    self
      .actions
      .bindings(name)
      .iter()
      .any(|binding| self.binding_pressed(binding))
  }

  /// Whether any binding of the action is held
  pub fn action_held(&self, name: &str) -> bool {
    self
      .actions
      .bindings(name)
      .iter()
      .any(|binding| self.binding_held(binding))
  }

  /// Whether any binding of the action was just released (a one-shot)
  pub fn action_released(&self, name: &str) -> bool {
    self
      .actions
      .bindings(name)
      .iter()
      .any(|binding| self.binding_released(binding))
  }

  pub(crate) fn binding_pressed(&self, binding: &Binding) -> bool {
    match *binding {
      Binding::Key(key) => self.key_pressed(key),
      Binding::Mouse(button) => self.mouse_pressed(button),
      Binding::Pad(button) => self.gamepad_button_pressed(button),
      Binding::Stick(dir) => self.stick_direction_pressed(dir),
    }
  }

  pub(crate) fn binding_held(&self, binding: &Binding) -> bool {
    match *binding {
      Binding::Key(key) => self.key_down(key),
      Binding::Mouse(button) => self.mouse_down(button),
      Binding::Pad(button) => self.gamepad_button_down(button),
      Binding::Stick(dir) => self.stick_direction_down(dir),
    }
  }

  /// Whether a binding's key, button or stick direction is physically
  /// down, consumed or not
  pub(crate) fn binding_down_raw(&self, binding: &Binding) -> bool {
    match *binding {
      Binding::Key(key) => self.keys_down.contains(&key),
      Binding::Mouse(button) => self.mouse_buttons_down.contains(&button),
      Binding::Pad(button) => self.gamepad_buttons_down.contains(&button),
      Binding::Stick(dir) => self.stick_dirs_down.contains(&dir),
    }
  }

  fn binding_released(&self, binding: &Binding) -> bool {
    match *binding {
      Binding::Key(key) => self.key_released(key),
      Binding::Mouse(button) => self.mouse_released(button),
      Binding::Pad(button) => self.gamepad_button_released(button),
      Binding::Stick(dir) => self.stick_direction_released(dir),
    }
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Keyboard Input
  // ─────────────────────────────────────────────────────────────────────────────

  /// Returns true if the key is currently held down
  pub fn key_down(&self, key: KeyCode) -> bool {
    !self.keys_blocked() && !self.consumed_keys.contains(&key) && self.keys_down.contains(&key)
  }

  /// Returns true if the key was just pressed (once per frame, and once in
  /// the first fixed step after the press)
  pub fn key_pressed(&self, key: KeyCode) -> bool {
    if self.keys_blocked() || self.consumed_keys.contains(&key) {
      return false;
    }
    self.one_shot(&self.keys_pressed, &self.keys_pressed_fixed, &key)
  }

  /// Whether a press of `key` is still waiting for a fixed step to see it
  pub(crate) fn key_press_pending(&self, key: KeyCode) -> bool {
    self.keys_pressed_fixed.contains(&key)
  }

  /// Returns true if the key was just released
  pub fn key_released(&self, key: KeyCode) -> bool {
    self.one_shot(&self.keys_released, &self.keys_released_fixed, &key)
  }

  /// Returns true if the key fired this frame (initial press OR repeat).
  ///
  /// Use this for text editing keys like arrows, backspace, delete that
  /// should repeat when held down.
  pub fn key_repeat(&self, key: KeyCode) -> bool {
    if self.keys_blocked()
      || self.consumed_keys.contains(&key)
      || self.oneshots() == OneShots::Suppressed
    {
      return false;
    }
    self.keys_repeated.contains(&key)
  }

  /// Handle a keyboard event (applied at once)
  pub(crate) fn handle_keyboard(&mut self, key: KeyCode, state: ElementState) {
    match state {
      ElementState::Pressed => {
        // Only register as "pressed" if it wasn't already down (prevents key repeat)
        if !self.keys_down.contains(&key) {
          self.keys_pressed.insert(key);
          self.keys_pressed_fixed.insert(key);
          self.last_device = InputDevice::Keyboard;
        }
        self.keys_down.insert(key);
        // Always register in keys_repeated (includes initial press and repeats)
        self.keys_repeated.insert(key);
      }
      ElementState::Released => {
        self.keys_down.remove(&key);
        self.keys_released.insert(key);
        self.keys_released_fixed.insert(key);
      }
    }
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Mouse Button Input
  // ─────────────────────────────────────────────────────────────────────────────

  /// Returns true if the mouse button is currently held down
  pub fn mouse_down(&self, button: MouseButton) -> bool {
    !self.pointer_blocked()
      && !self.consumed_mouse.contains(&button)
      && self.mouse_buttons_down.contains(&button)
  }

  /// Whether a press of `button` is still waiting for a fixed step to see it
  pub(crate) fn mouse_press_pending(&self, button: MouseButton) -> bool {
    self.mouse_buttons_pressed_fixed.contains(&button)
  }

  /// Returns true if the mouse button was just pressed
  pub fn mouse_pressed(&self, button: MouseButton) -> bool {
    if self.pointer_blocked() || self.consumed_mouse.contains(&button) {
      return false;
    }
    self.one_shot(
      &self.mouse_buttons_pressed,
      &self.mouse_buttons_pressed_fixed,
      &button,
    )
  }

  /// Returns true if the mouse button was just released
  pub fn mouse_released(&self, button: MouseButton) -> bool {
    self.one_shot(
      &self.mouse_buttons_released,
      &self.mouse_buttons_released_fixed,
      &button,
    )
  }

  /// Handle a mouse button event (applied at once)
  pub(crate) fn handle_mouse_button(&mut self, button: MouseButton, state: ElementState) {
    match state {
      ElementState::Pressed => {
        if !self.mouse_buttons_down.contains(&button) {
          self.mouse_buttons_pressed.insert(button);
          self.mouse_buttons_pressed_fixed.insert(button);
        }
        self.last_device = InputDevice::Mouse;
        self.mouse_buttons_down.insert(button);
      }
      ElementState::Released => {
        self.mouse_buttons_down.remove(&button);
        self.mouse_buttons_released.insert(button);
        self.mouse_buttons_released_fixed.insert(button);
      }
    }
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Mouse Position
  // ─────────────────────────────────────────────────────────────────────────────

  /// The mouse X position in screen space (logical pixels, or the pixel-art
  /// target; window scaling and bars removed): over the bars it lies outside
  /// `0..screen_width`. `ctx.camera().screen_to_world` gives world space.
  /// Not updated while the cursor is locked.
  pub fn mouse_x(&self) -> f32 {
    self.mouse_x
  }

  /// The mouse Y position in screen space (see [`Input::mouse_x`]).
  pub fn mouse_y(&self) -> f32 {
    self.mouse_y
  }

  /// The mouse position `(x, y)` in screen space (see [`Input::mouse_x`]).
  pub fn mouse_position(&self) -> (f32, f32) {
    (self.mouse_x, self.mouse_y)
  }

  /// Get how much the mouse moved this frame (x, y)
  pub fn mouse_delta(&self) -> (f32, f32) {
    if self.pointer_blocked() {
      return (0.0, 0.0);
    }
    (self.mouse_delta_x, self.mouse_delta_y)
  }

  /// Handle mouse movement (applied at once)
  pub(crate) fn handle_mouse_move(&mut self, x: f64, y: f64) {
    let new_x = x as f32;
    let new_y = y as f32;
    if (new_x - self.mouse_x).hypot(new_y - self.mouse_y) >= 2.0 {
      self.last_device = InputDevice::Mouse;
    }
    self.mouse_delta_x += new_x - self.mouse_x;
    self.mouse_delta_y += new_y - self.mouse_y;
    self.mouse_x = new_x;
    self.mouse_y = new_y;
  }

  /// Handle raw mouse motion (from DeviceEvent when cursor is locked).
  pub(crate) fn handle_raw_mouse_motion(&mut self, dx: f64, dy: f64) {
    self.mouse_delta_x += dx as f32;
    self.mouse_delta_y += dy as f32;
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Mouse Scroll
  // ─────────────────────────────────────────────────────────────────────────────

  /// Get the scroll delta (x, y) in lines: a mouse wheel's lines plus a
  /// trackpad's pixels at 120 to the line.
  /// Positive y = scroll up, negative y = scroll down
  ///
  /// Outside the fixed steps this is the frame's scroll. Inside them the
  /// first step sees the frame's scroll and later steps see none, so a step
  /// never counts it twice; a frame with no step carries its scroll to the
  /// next first step, and a pause drops it.
  pub fn scroll_delta(&self) -> (f32, f32) {
    let (lines, px) = self.scroll_parts();
    (
      lines.0 + px.0 / PIXELS_PER_LINE,
      lines.1 + px.1 / PIXELS_PER_LINE,
    )
  }

  /// A trackpad's movement (x, y) in logical pixels this frame, 0 for a
  /// mouse wheel; follow the fingers with this. Same sign as
  /// [`Input::scroll_delta`] (positive y = scroll up) and the same
  /// fixed-step, consume and block rules.
  pub fn scroll_pixels(&self) -> (f32, f32) {
    self.scroll_parts().1
  }

  /// A mouse wheel's lines alone, trackpad pixels left out (what the UI
  /// eases; it follows the pixels at once).
  pub(crate) fn scroll_wheel_lines(&self) -> (f32, f32) {
    self.scroll_parts().0
  }

  /// This frame's scroll as (wheel lines, trackpad pixels), after the
  /// blocks and the fixed-step view
  fn scroll_parts(&self) -> ((f32, f32), (f32, f32)) {
    if self.pointer_blocked() {
      return ((0.0, 0.0), (0.0, 0.0));
    }
    match self.oneshots() {
      OneShots::Frame => (self.scroll_lines, self.scroll_px),
      OneShots::Fixed => (self.scroll_lines_fixed, self.scroll_px_fixed),
      OneShots::Suppressed => ((0.0, 0.0), (0.0, 0.0)),
    }
  }

  /// The player's last-used device (keyboard, mouse or gamepad)
  pub fn last_device(&self) -> InputDevice {
    self.last_device
  }

  /// A pad press waiting for the next fixed step (scripted input)
  pub(crate) fn gamepad_press_pending(&self, button: GamepadButton) -> bool {
    self.gamepad_buttons_pressed_fixed.contains(&button)
  }

  /// Handle mouse scroll (applied at once). A `PixelDelta` is in logical
  /// pixels (the window converts it, see [`logical_scroll`]).
  pub(crate) fn handle_scroll(&mut self, delta: MouseScrollDelta) {
    match delta {
      MouseScrollDelta::LineDelta(x, y) => {
        self.scroll_lines.0 += x;
        self.scroll_lines.1 += y;
        self.scroll_lines_fixed.0 += x;
        self.scroll_lines_fixed.1 += y;
      }
      MouseScrollDelta::PixelDelta(pos) => {
        let (x, y) = (pos.x as f32, pos.y as f32);
        self.scroll_px.0 += x;
        self.scroll_px.1 += y;
        self.scroll_px_fixed.0 += x;
        self.scroll_px_fixed.1 += y;
      }
    }
    self.last_device = InputDevice::Mouse;
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Text Input (IME)
  // ─────────────────────────────────────────────────────────────────────────────

  /// Get text that was committed this frame (ready to insert into text fields).
  ///
  /// The first caller to read text input in a frame gets the text; subsequent
  /// callers receive an empty string. This prevents duplicate processing when
  /// multiple systems (game update, UI, console) all read text input.
  pub fn text_input(&self) -> &str {
    if self.oneshots() == OneShots::Suppressed
      || self.keys_blocked()
      || self.text_input_consumed.load(Ordering::Relaxed)
    {
      return "";
    }
    if !self.text_input.is_empty() {
      self.text_input_consumed.store(true, Ordering::Relaxed);
    }
    &self.text_input
  }

  /// Check if any text was input this frame (and hasn't been consumed yet).
  pub fn has_text_input(&self) -> bool {
    if self.oneshots() == OneShots::Suppressed
      || self.keys_blocked()
      || self.text_input_consumed.load(Ordering::Relaxed)
    {
      return false;
    }
    !self.text_input.is_empty()
  }

  /// Explicitly consume text input, preventing subsequent readers from seeing it.
  pub fn consume_text_input(&self) {
    if self.keys_blocked() {
      return;
    }
    self.text_input_consumed.store(true, Ordering::Relaxed);
  }

  /// Get the current IME preedit/composition text (in-progress input).
  pub fn ime_preedit(&self) -> &str {
    &self.ime_preedit
  }

  /// Get the IME preedit cursor position as (start, end) byte offsets.
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

  pub(crate) fn handle_ime_enabled(&mut self) {
    self.ime_enabled = true;
  }

  pub(crate) fn handle_ime_disabled(&mut self) {
    self.ime_enabled = false;
    self.ime_preedit.clear();
    self.ime_preedit_cursor = None;
  }

  pub(crate) fn handle_ime_preedit(&mut self, text: String, cursor: Option<(usize, usize)>) {
    self.ime_preedit = text;
    self.ime_preedit_cursor = cursor;
  }

  pub(crate) fn handle_ime_commit(&mut self, text: String) {
    // When Ctrl/Cmd is held, the IME commit is likely from a keyboard shortcut
    // (e.g. Ctrl+V paste on Windows). Skip it; the app handles shortcuts directly.
    let ctrl_held = self.keys_down.contains(&KeyCode::ControlLeft)
      || self.keys_down.contains(&KeyCode::ControlRight)
      || self.keys_down.contains(&KeyCode::SuperLeft)
      || self.keys_down.contains(&KeyCode::SuperRight);
    if ctrl_held {
      return;
    }

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
    self.ime_preedit.clear();
    self.ime_preedit_cursor = None;
  }

  /// Handle direct text input from keyboard events (non-IME).
  pub(crate) fn handle_text_input(&mut self, text: &str) {
    // Skip keyboard text input when IME is enabled - let IME handle all text
    if self.ime_enabled {
      return;
    }
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

  /// The connected pad's family (from its name), which prompts label its
  /// buttons for: Xbox when none is connected or the name says nothing
  pub fn pad_family(&self) -> PadFamily {
    PadFamily::from_name(&self.gamepad_name)
  }

  /// Returns true if the gamepad button is currently held down.
  pub fn gamepad_button_down(&self, button: GamepadButton) -> bool {
    !self.keys_blocked()
      && !self.consumed_gamepad.contains(&button)
      && self.gamepad_buttons_down.contains(&button)
  }

  /// Returns true if the gamepad button was just pressed.
  pub fn gamepad_button_pressed(&self, button: GamepadButton) -> bool {
    if self.keys_blocked() || self.consumed_gamepad.contains(&button) {
      return false;
    }
    self.one_shot(
      &self.gamepad_buttons_pressed,
      &self.gamepad_buttons_pressed_fixed,
      &button,
    )
  }

  /// Returns true if the gamepad button was just released.
  pub fn gamepad_button_released(&self, button: GamepadButton) -> bool {
    self.one_shot(
      &self.gamepad_buttons_released,
      &self.gamepad_buttons_released_fixed,
      &button,
    )
  }

  /// Whether a stick is held past the digital threshold in a direction
  /// (down past 0.5 deflection, up again below 0.4)
  pub fn stick_direction_down(&self, direction: StickDirection) -> bool {
    !self.keys_blocked()
      && !self.consumed_stick_dirs.contains(&direction)
      && self.stick_dirs_down.contains(&direction)
  }

  /// Whether a stick direction just went down (a one-shot like a button press)
  pub fn stick_direction_pressed(&self, direction: StickDirection) -> bool {
    if self.keys_blocked() || self.consumed_stick_dirs.contains(&direction) {
      return false;
    }
    self.one_shot(
      &self.stick_dirs_pressed,
      &self.stick_dirs_pressed_fixed,
      &direction,
    )
  }

  /// Whether a stick direction just came up
  pub fn stick_direction_released(&self, direction: StickDirection) -> bool {
    self.one_shot(
      &self.stick_dirs_released,
      &self.stick_dirs_released_fixed,
      &direction,
    )
  }

  /// Get the left stick position as (x, y) with deadzone applied.
  pub fn left_stick(&self) -> (f32, f32) {
    if self.keys_blocked() {
      return (0.0, 0.0);
    }
    (self.left_stick_x, self.left_stick_y)
  }

  pub fn left_stick_x(&self) -> f32 {
    self.left_stick().0
  }

  pub fn left_stick_y(&self) -> f32 {
    self.left_stick().1
  }

  /// Get the right stick position as (x, y) with deadzone applied.
  pub fn right_stick(&self) -> (f32, f32) {
    if self.keys_blocked() {
      return (0.0, 0.0);
    }
    (self.right_stick_x, self.right_stick_y)
  }

  pub fn right_stick_x(&self) -> f32 {
    self.right_stick().0
  }

  pub fn right_stick_y(&self) -> f32 {
    self.right_stick().1
  }

  /// Get raw left stick position without deadzone (for debug visualization).
  pub fn left_stick_raw(&self) -> (f32, f32) {
    if self.keys_blocked() {
      return (0.0, 0.0);
    }
    (self.left_stick_x_raw, self.left_stick_y_raw)
  }

  /// Get raw right stick position without deadzone (for debug visualization).
  pub fn right_stick_raw(&self) -> (f32, f32) {
    if self.keys_blocked() {
      return (0.0, 0.0);
    }
    (self.right_stick_x_raw, self.right_stick_y_raw)
  }

  /// Get the left trigger value (0.0 to 1.0).
  pub fn left_trigger(&self) -> f32 {
    if self.keys_blocked() {
      return 0.0;
    }
    self.left_trigger
  }

  /// Get the right trigger value (0.0 to 1.0).
  pub fn right_trigger(&self) -> f32 {
    if self.keys_blocked() {
      return 0.0;
    }
    self.right_trigger
  }

  pub(crate) fn handle_gamepad_connected(&mut self, id: gilrs::GamepadId, name: String) {
    // Only track first gamepad if none connected
    if !self.gamepad_connected {
      self.gamepad_connected = true;
      self.gamepad_id = Some(id);
      self.gamepad_name = name;
      engine_info!(module: "Gamepad", "Connected: {}", self.gamepad_name);
    }
  }

  pub(crate) fn handle_gamepad_disconnected(&mut self, id: gilrs::GamepadId) {
    if self.gamepad_id == Some(id) {
      engine_info!(module: "Gamepad", "Disconnected: {}", self.gamepad_name);
      self.gamepad_connected = false;
      self.gamepad_id = None;
      self.gamepad_name.clear();
      self.gamepad_buttons_down.clear();
      self.gamepad_buttons_pressed.clear();
      self.gamepad_buttons_released.clear();
      self.gamepad_buttons_pressed_fixed.clear();
      self.gamepad_buttons_released_fixed.clear();
      self.stick_dirs_down.clear();
      self.stick_dirs_pressed.clear();
      self.stick_dirs_released.clear();
      self.stick_dirs_pressed_fixed.clear();
      self.stick_dirs_released_fixed.clear();
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

  pub(crate) fn handle_gamepad_button(
    &mut self,
    id: gilrs::GamepadId,
    button: gilrs::Button,
    pressed: bool,
  ) {
    if self.gamepad_id != Some(id) {
      return;
    }
    let Some(btn) = GamepadButton::from_gilrs(button) else {
      return;
    };
    self.set_gamepad_button(btn, pressed);
  }

  /// Apply a button change to the tracked pad's state (press sets, release clears)
  fn set_gamepad_button(&mut self, btn: GamepadButton, pressed: bool) {
    if pressed {
      if !self.gamepad_buttons_down.contains(&btn) {
        self.gamepad_buttons_pressed.insert(btn);
        self.gamepad_buttons_pressed_fixed.insert(btn);
      }
      self.gamepad_buttons_down.insert(btn);
      self.last_device = InputDevice::Gamepad;
    } else {
      self.gamepad_buttons_down.remove(&btn);
      self.gamepad_buttons_released.insert(btn);
      self.gamepad_buttons_released_fixed.insert(btn);
    }
  }

  /// Track a pad when a virtual event arrives and none is connected
  fn connect_virtual_pad(&mut self) {
    if !self.gamepad_connected {
      self.gamepad_connected = true;
      self.gamepad_name = "Virtual pad".to_string();
      self.gamepad_id = None;
    }
  }

  /// Store one stick's raw position and recompute its deadzoned value
  fn set_stick_raw(&mut self, stick: Stick, x: f32, y: f32) {
    let (dx, dy) = apply_deadzone(x, y);
    match stick {
      Stick::Left => {
        (self.left_stick_x_raw, self.left_stick_y_raw) = (x, y);
        (self.left_stick_x, self.left_stick_y) = (dx, dy);
      }
      Stick::Right => {
        (self.right_stick_x_raw, self.right_stick_y_raw) = (x, y);
        (self.right_stick_x, self.right_stick_y) = (dx, dy);
      }
    }
    if dx.hypot(dy) > 0.5 {
      self.last_device = InputDevice::Gamepad;
    }
    // Digital directions read the raw position, gated by the deadzone, so the
    // 0.5 and 0.4 thresholds are the stick's real deflection
    let (rx, ry) = if dx == 0.0 && dy == 0.0 {
      (0.0, 0.0)
    } else {
      (x, y)
    };
    for (direction, value) in [
      (Direction::Up, ry),
      (Direction::Down, -ry),
      (Direction::Right, rx),
      (Direction::Left, -rx),
    ] {
      self.set_stick_direction(StickDirection { stick, direction }, value);
    }
  }

  /// Update one stick direction's digital state: down past 0.5, up below 0.4
  fn set_stick_direction(&mut self, dir: StickDirection, value: f32) {
    let down = self.stick_dirs_down.contains(&dir);
    if !down && value > STICK_DIGITAL_PRESS {
      self.stick_dirs_down.insert(dir);
      self.stick_dirs_pressed.insert(dir);
      self.stick_dirs_pressed_fixed.insert(dir);
    } else if down && value < STICK_DIGITAL_RELEASE {
      self.stick_dirs_down.remove(&dir);
      self.stick_dirs_released.insert(dir);
      self.stick_dirs_released_fixed.insert(dir);
    }
  }

  pub(crate) fn handle_gamepad_axis(
    &mut self,
    id: gilrs::GamepadId,
    axis: gilrs::Axis,
    value: f32,
  ) {
    if self.gamepad_id != Some(id) {
      return;
    }
    use gilrs::Axis;
    match axis {
      Axis::LeftStickX => self.set_stick_raw(Stick::Left, value, self.left_stick_y_raw),
      Axis::LeftStickY => self.set_stick_raw(Stick::Left, self.left_stick_x_raw, value),
      Axis::RightStickX => self.set_stick_raw(Stick::Right, value, self.right_stick_y_raw),
      Axis::RightStickY => self.set_stick_raw(Stick::Right, self.right_stick_x_raw, value),
      // Triggers arrive as LeftTrigger2/RightTrigger2 button-changed events
      _ => {}
    }
  }

  pub(crate) fn handle_gamepad_trigger(
    &mut self,
    id: gilrs::GamepadId,
    button: gilrs::Button,
    value: f32,
  ) {
    if self.gamepad_id != Some(id) {
      return;
    }
    use gilrs::Button;
    match button {
      Button::LeftTrigger2 => self.left_trigger = value,
      Button::RightTrigger2 => self.right_trigger = value,
      _ => {}
    }
  }
}

/// This frame's input as the UI pass sees it: every set filtered through the
/// same blocks, suppression and consumption the read methods apply.
#[derive(Debug, Clone, Default)]
pub(crate) struct InputView {
  pub(crate) mouse: (f32, f32),
  pub(crate) scroll: (f32, f32),
  /// A wheel's lines alone, and a trackpad's logical pixels
  pub(crate) wheel: (f32, f32),
  pub(crate) pixels: (f32, f32),
  pub(crate) keys_down: HashSet<KeyCode>,
  pub(crate) keys_pressed: HashSet<KeyCode>,
  pub(crate) keys_released: HashSet<KeyCode>,
  pub(crate) keys_repeated: HashSet<KeyCode>,
  pub(crate) mouse_down: HashSet<MouseButton>,
  pub(crate) mouse_pressed: HashSet<MouseButton>,
  pub(crate) mouse_released: HashSet<MouseButton>,
  pub(crate) text: String,
  pub(crate) ime_preedit: String,
  pub(crate) ime_preedit_cursor: Option<(usize, usize)>,
  pub(crate) ime_composing: bool,
  pub(crate) ime_enabled: bool,
  pub(crate) gamepad_pressed: HashSet<GamepadButton>,
  pub(crate) left_stick: (f32, f32),
}

impl Input {
  /// What the UI pass reads; nothing here is consumed by taking it.
  pub(crate) fn ui_view(&self) -> InputView {
    fn keep<T: Copy + Eq + std::hash::Hash>(
      set: &HashSet<T>,
      read: impl Fn(T) -> bool,
    ) -> HashSet<T> {
      set.iter().copied().filter(|item| read(*item)).collect()
    }
    InputView {
      mouse: self.mouse_position(),
      scroll: self.scroll_delta(),
      wheel: self.scroll_wheel_lines(),
      pixels: self.scroll_pixels(),
      keys_down: keep(&self.keys_down, |key| self.key_down(key)),
      keys_pressed: keep(&self.keys_pressed, |key| self.key_pressed(key)),
      keys_released: keep(&self.keys_released, |key| self.key_released(key)),
      keys_repeated: keep(&self.keys_repeated, |key| self.key_repeat(key)),
      mouse_down: keep(&self.mouse_buttons_down, |button| self.mouse_down(button)),
      mouse_pressed: keep(&self.mouse_buttons_pressed, |button| {
        self.mouse_pressed(button)
      }),
      mouse_released: keep(&self.mouse_buttons_released, |button| {
        self.mouse_released(button)
      }),
      text: self.peek_text_input().to_string(),
      ime_preedit: self.ime_preedit().to_string(),
      ime_preedit_cursor: self.ime_preedit_cursor(),
      ime_composing: self.ime_composing(),
      ime_enabled: self.ime_enabled(),
      gamepad_pressed: keep(&self.gamepad_buttons_pressed, |button| {
        self.gamepad_button_pressed(button)
      }),
      left_stick: self.left_stick(),
    }
  }

  /// The frame's typed text, as `text_input` would return it, without
  /// consuming it.
  fn peek_text_input(&self) -> &str {
    if self.oneshots() == OneShots::Suppressed
      || self.keys_blocked()
      || self.text_input_consumed.load(Ordering::Relaxed)
    {
      return "";
    }
    &self.text_input
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn key_down_tracks_held_keys() {
    let mut input = Input::new();

    input.handle_keyboard(KeyCode::KeyW, ElementState::Pressed);
    assert!(input.key_down(KeyCode::KeyW));
    assert!(!input.key_down(KeyCode::KeyA));

    input.handle_keyboard(KeyCode::KeyW, ElementState::Released);
    assert!(!input.key_down(KeyCode::KeyW));
  }

  #[test]
  fn key_pressed_only_fires_once() {
    let mut input = Input::new();

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
    let mut input = Input::new();

    input.handle_keyboard(KeyCode::KeyW, ElementState::Pressed);
    input.handle_keyboard(KeyCode::KeyW, ElementState::Released);
    assert!(input.key_released(KeyCode::KeyW));

    input.begin_frame();
    assert!(!input.key_released(KeyCode::KeyW));
  }

  #[test]
  fn mouse_position_tracking() {
    let mut input = Input::new();

    input.handle_mouse_move(100.0, 200.0);
    assert_eq!(input.mouse_position(), (100.0, 200.0));
    assert_eq!(input.mouse_delta(), (100.0, 200.0)); // Delta from (0,0)

    input.begin_frame();
    input.handle_mouse_move(150.0, 250.0);
    assert_eq!(input.mouse_delta(), (50.0, 50.0)); // Delta from previous position
  }

  #[test]
  fn mouse_button_tracking() {
    let mut input = Input::new();

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
    let mut input = Input::new();

    input.handle_scroll(MouseScrollDelta::LineDelta(1.0, 2.0));
    input.handle_scroll(MouseScrollDelta::LineDelta(0.5, 1.0));
    assert_eq!(input.scroll_delta(), (1.5, 3.0));

    input.begin_frame();
    assert_eq!(input.scroll_delta(), (0.0, 0.0));
  }

  fn pixels(x: f64, y: f64) -> MouseScrollDelta {
    MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(x, y))
  }

  #[test]
  fn small_trackpad_events_add_up_to_exact_logical_pixels() {
    let mut input = Input::new();
    // 20 events of 0.75 physical pixels on a 2x display
    for _ in 0..20 {
      input.handle_scroll(logical_scroll(pixels(0.75, -1.5), 2.0));
    }
    assert_eq!(input.scroll_pixels(), (7.5, -15.0));
    assert_eq!(input.scroll_delta(), (7.5 / 120.0, -15.0 / 120.0));
    assert_eq!(input.scroll_wheel_lines(), (0.0, 0.0));
    input.begin_frame();
    assert_eq!(input.scroll_pixels(), (0.0, 0.0));
  }

  #[test]
  fn a_wheel_adds_no_pixels_and_a_trackpad_no_lines() {
    let mut input = Input::new();
    input.handle_scroll(MouseScrollDelta::LineDelta(1.0, -2.0));
    assert_eq!(input.scroll_pixels(), (0.0, 0.0));
    assert_eq!(input.scroll_wheel_lines(), (1.0, -2.0));
    input.handle_scroll(pixels(60.0, 120.0));
    assert_eq!(input.scroll_pixels(), (60.0, 120.0));
    assert_eq!(input.scroll_wheel_lines(), (1.0, -2.0));
    assert_eq!(input.scroll_delta(), (1.5, -1.0));
  }

  #[test]
  fn logical_scroll_scales_pixels_only() {
    let line = MouseScrollDelta::LineDelta(1.0, 2.0);
    assert!(matches!(
      logical_scroll(line, 2.0),
      MouseScrollDelta::LineDelta(1.0, 2.0)
    ));
    assert!(matches!(
      logical_scroll(pixels(4.0, 6.0), 2.0),
      MouseScrollDelta::PixelDelta(p) if (p.x, p.y) == (2.0, 3.0)
    ));
    // A bad factor leaves the pixels as they are
    assert!(matches!(
      logical_scroll(pixels(4.0, 6.0), 0.0),
      MouseScrollDelta::PixelDelta(p) if (p.x, p.y) == (4.0, 6.0)
    ));
  }

  #[test]
  fn trackpad_pixels_follow_the_blocked_consumed_and_fixed_rules() {
    let mut input = Input::new();
    frame(&mut input, vec![InputEvent::Scroll(pixels(0.0, 30.0))]);
    assert_eq!(input.scroll_pixels(), (0.0, 30.0));
    input.set_fixed_step(0);
    assert_eq!(input.scroll_pixels(), (0.0, 30.0));
    input.set_fixed_step(1);
    assert_eq!(input.scroll_pixels(), (0.0, 0.0), "once per frame");
    input.end_fixed_steps(true);
    input.begin_frame();
    input.set_fixed_step(0);
    assert_eq!(input.scroll_pixels(), (0.0, 0.0), "seen once, gone");

    // A frame with no step carries its pixels to the next first step
    let mut input = Input::new();
    frame(&mut input, vec![InputEvent::Scroll(pixels(0.0, 5.0))]);
    input.end_fixed_steps(false);
    frame(&mut input, vec![InputEvent::Scroll(pixels(0.0, 7.0))]);
    input.set_fixed_step(0);
    assert_eq!(input.scroll_pixels(), (0.0, 12.0));

    let mut input = Input::new();
    frame(&mut input, vec![InputEvent::Scroll(pixels(0.0, 5.0))]);
    input.consume_scroll();
    assert_eq!(input.scroll_pixels(), (0.0, 0.0));
    input.set_fixed_step(0);
    assert_eq!(input.scroll_pixels(), (0.0, 0.0));

    let mut input = Input::new();
    input.handle_scroll(pixels(0.0, 5.0));
    input.block_pointer();
    assert_eq!(input.scroll_pixels(), (0.0, 0.0));
    assert_eq!(input.scroll_wheel_lines(), (0.0, 0.0));
  }

  #[test]
  fn scroll_pixel_delta_conversion() {
    let mut input = Input::new();
    input.handle_scroll(MouseScrollDelta::PixelDelta(
      winit::dpi::PhysicalPosition::new(120.0, 240.0),
    ));
    assert_eq!(input.scroll_delta(), (1.0, 2.0)); // Divided by 120.0
  }

  #[test]
  fn mouse_x_y_getters() {
    let mut input = Input::new();
    input.handle_mouse_move(150.0, 250.0);
    assert_eq!(input.mouse_x(), 150.0);
    assert_eq!(input.mouse_y(), 250.0);
  }

  #[test]
  fn mouse_delta_accumulates() {
    let mut input = Input::new();
    input.handle_mouse_move(10.0, 20.0);
    input.handle_mouse_move(15.0, 25.0);
    assert_eq!(input.mouse_delta(), (15.0, 25.0)); // Total delta from origin
  }

  #[test]
  fn raw_mouse_motion_adds_to_delta() {
    let mut input = Input::new();
    input.handle_raw_mouse_motion(5.0, -3.0);
    assert_eq!(input.mouse_delta(), (5.0, -3.0));

    // Accumulates with additional raw motion
    input.handle_raw_mouse_motion(2.0, 1.0);
    assert_eq!(input.mouse_delta(), (7.0, -2.0));
  }

  #[test]
  fn raw_mouse_motion_cleared_by_begin_frame() {
    let mut input = Input::new();
    input.handle_raw_mouse_motion(10.0, 20.0);
    assert_eq!(input.mouse_delta(), (10.0, 20.0));

    input.begin_frame();
    assert_eq!(input.mouse_delta(), (0.0, 0.0));
  }

  #[test]
  fn raw_mouse_motion_does_not_affect_position() {
    let mut input = Input::new();
    input.handle_mouse_move(100.0, 200.0); // set position
    input.begin_frame();

    // Raw motion adds to delta but does NOT change mouse position
    input.handle_raw_mouse_motion(50.0, 50.0);
    assert_eq!(input.mouse_delta(), (50.0, 50.0));
    assert_eq!(input.mouse_position(), (100.0, 200.0));
  }

  #[test]
  fn begin_frame_clears_one_frame_events() {
    let mut input = Input::new();
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
    let mut input = Input::new();
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    assert!(input.mouse_pressed(MouseButton::Left));

    // Repeated press (OS key repeat)
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    // Should still be registered as pressed this frame
    assert!(input.mouse_pressed(MouseButton::Left));
  }

  #[test]
  fn key_repeat_fires_on_initial_and_repeat() {
    let mut input = Input::new();

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
    let mut input = Input::new();

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
    let input = Input::new();
    assert!(!input.gamepad_connected());
    assert!(input.gamepad_name().is_empty());
  }

  #[test]
  fn gamepad_stick_values_default_to_zero() {
    let input = Input::new();
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
    let mut input = Input::new();

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
    let mut input = Input::new();

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
    let mut input = Input::new();

    // Simulate: mouse button pressed
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);

    // First update sees the press
    assert!(
      input.mouse_pressed(MouseButton::Left),
      "First update should see mouse_pressed"
    );
    assert!(input.mouse_down(MouseButton::Left), "Button should be held");

    // Mouse pressed should persist until begin_frame
    assert!(
      input.mouse_pressed(MouseButton::Left),
      "Mouse pressed should persist for later readers in the frame"
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
    let mut input = Input::new();
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
    let mut input = Input::new();
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
    let mut input = Input::new();

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
  fn consume_key_removes_specific_key_only() {
    let mut input = Input::new();

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
    let mut input = Input::new();
    input.handle_keyboard(KeyCode::Escape, ElementState::Pressed);

    // First call returns true and consumes
    assert!(input.key_pressed_and_consume(KeyCode::Escape));

    // Second call returns false (already consumed)
    assert!(!input.key_pressed_and_consume(KeyCode::Escape));

    // Regular key_pressed also returns false
    assert!(!input.key_pressed(KeyCode::Escape));

    // Consumed for the rest of the frame, held state included
    assert!(!input.key_down(KeyCode::Escape));
  }

  #[test]
  fn key_pressed_and_consume_returns_false_if_not_pressed() {
    let mut input = Input::new();
    assert!(!input.key_pressed_and_consume(KeyCode::Escape));
  }

  #[test]
  fn consume_key_press_removes_without_return() {
    let mut input = Input::new();
    input.handle_keyboard(KeyCode::Space, ElementState::Pressed);

    assert!(input.key_pressed(KeyCode::Space));
    input.consume_key_press(KeyCode::Space);
    assert!(!input.key_pressed(KeyCode::Space));
  }

  #[test]
  fn mouse_pressed_and_consume_returns_true_and_consumes() {
    let mut input = Input::new();
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);

    // First call returns true and consumes
    assert!(input.mouse_pressed_and_consume(MouseButton::Left));

    // Second call returns false (already consumed)
    assert!(!input.mouse_pressed_and_consume(MouseButton::Left));

    // Regular mouse_pressed also returns false
    assert!(!input.mouse_pressed(MouseButton::Left));

    // Consumed for the rest of the frame, held state included
    assert!(!input.mouse_down(MouseButton::Left));
  }

  #[test]
  fn mouse_pressed_and_consume_returns_false_if_not_pressed() {
    let mut input = Input::new();
    assert!(!input.mouse_pressed_and_consume(MouseButton::Right));
  }

  #[test]
  fn consume_mouse_press_removes_without_return() {
    let mut input = Input::new();
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);

    assert!(input.mouse_pressed(MouseButton::Left));
    input.consume_mouse_press(MouseButton::Left);
    assert!(!input.mouse_pressed(MouseButton::Left));
  }

  #[test]
  fn gamepad_button_pressed_and_consume_returns_true_and_consumes() {
    let mut input = Input::new();
    // Manually insert a gamepad button press (since we can't easily simulate gilrs)
    input.gamepad_buttons_pressed.insert(GamepadButton::South);
    input.gamepad_buttons_down.insert(GamepadButton::South);

    // First call returns true and consumes
    assert!(input.gamepad_button_pressed_and_consume(GamepadButton::South));

    // Second call returns false (already consumed)
    assert!(!input.gamepad_button_pressed_and_consume(GamepadButton::South));

    // Regular gamepad_button_pressed also returns false
    assert!(!input.gamepad_button_pressed(GamepadButton::South));

    // Consumed for the rest of the frame, held state included
    assert!(!input.gamepad_button_down(GamepadButton::South));
  }

  #[test]
  fn consume_gamepad_button_press_removes_without_return() {
    let mut input = Input::new();
    input.gamepad_buttons_pressed.insert(GamepadButton::East);

    assert!(input.gamepad_button_pressed(GamepadButton::East));
    input.consume_gamepad_button_press(GamepadButton::East);
    assert!(!input.gamepad_button_pressed(GamepadButton::East));
  }

  #[test]
  fn consume_prevents_double_handling_pattern() {
    let mut input = Input::new();
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
  /// Keyboard events persist until begin_frame() is called, so that every
  /// reader in the frame sees them: the UI pass (`Scene::ui`, text inputs,
  /// dropdowns), the fixed steps and per-frame code.
  ///
  /// Frame flow should be:
  /// 1. Events received -> key_pressed() returns true
  /// 2. Several readers in the frame -> key_pressed() MUST still return true
  /// 3. begin_frame() clears events -> key_pressed() returns false
  #[test]
  fn keyboard_events_persist_until_begin_frame() {
    let mut input = Input::new();

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

    // Read again (simulating a UI read - the critical regression case)
    // This is where the bug manifested: UI couldn't see keyboard events
    assert!(
      input.key_pressed(KeyCode::KeyA),
      "key_pressed MUST be available to the UI (regression test)"
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
    let mut input = Input::new();
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
    let mut input = Input::new();
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
    let mut input = Input::new();
    input.handle_keyboard(KeyCode::ArrowLeft, ElementState::Pressed);

    assert!(input.key_repeat(KeyCode::ArrowLeft));

    input.suppress_oneshots();

    assert!(!input.key_repeat(KeyCode::ArrowLeft));

    input.unsuppress_oneshots();
    assert!(input.key_repeat(KeyCode::ArrowLeft));
  }

  #[test]
  fn oneshot_suppression_blocks_text_input() {
    let mut input = Input::new();
    input.handle_text_input("hello");

    assert!(input.has_text_input());
    assert_eq!(input.text_input(), "hello");

    input.suppress_oneshots();

    assert!(!input.has_text_input());
    assert_eq!(input.text_input(), "");
  }

  #[test]
  fn oneshot_suppression_does_not_affect_down_state() {
    let mut input = Input::new();
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
    let mut input = Input::new();
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
    let mut input = Input::new();
    input.handle_keyboard(KeyCode::Space, ElementState::Pressed);

    input.suppress_oneshots();
    assert!(!input.key_pressed(KeyCode::Space));

    input.unsuppress_oneshots();
    assert!(input.key_pressed(KeyCode::Space));
  }

  #[test]
  fn begin_frame_resets_suppression() {
    let mut input = Input::new();
    input.suppress_oneshots();

    input.begin_frame();
    input.handle_keyboard(KeyCode::Space, ElementState::Pressed);

    // After begin_frame, suppression should be cleared
    assert!(input.key_pressed(KeyCode::Space));
  }

  #[test]
  fn fixed_timestep_simulation() {
    let mut input = Input::new();

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

    // The fixed loop ends
    input.unsuppress_oneshots();

    // Per-frame code CAN see them
    assert!(input.key_pressed(KeyCode::Escape));
    assert!(input.mouse_pressed(MouseButton::Left));

    // Continuous state works throughout
    assert!(input.key_down(KeyCode::Escape));
    assert!(input.mouse_down(MouseButton::Left));
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Text Input Consume-on-Read Tests
  // Regression tests for text duplication where multiple readers (update,
  // the UI, the console) all read the same text_input in one frame.
  // ─────────────────────────────────────────────────────────────────────────────

  #[test]
  fn text_input_consumed_after_first_read() {
    let mut input = Input::new();
    input.handle_text_input("hello");

    // First read returns the text
    assert_eq!(input.text_input(), "hello");

    // Second read returns empty (consumed)
    assert_eq!(input.text_input(), "");
    assert!(!input.has_text_input());
  }

  #[test]
  fn text_input_has_check_does_not_consume() {
    let mut input = Input::new();
    input.handle_text_input("x");

    // has_text_input should NOT consume
    assert!(input.has_text_input());
    assert!(input.has_text_input());

    // text_input() still returns the text
    assert_eq!(input.text_input(), "x");

    // NOW it's consumed
    assert!(!input.has_text_input());
  }

  #[test]
  fn empty_text_input_does_not_set_consumed() {
    let mut input = Input::new();

    // Reading empty text should NOT set consumed flag
    assert_eq!(input.text_input(), "");

    // Later text should still be readable (simulates event arriving)
    input.handle_text_input("a");
    assert_eq!(input.text_input(), "a");
  }

  /// Regression test from the old frame loop, where a frame without an
  /// update skipped begin_frame() (`post_render_cleanup` is its test-only
  /// shim; begin_frame now runs every frame). Reading empty text_input must
  /// NOT set the consumed flag, otherwise text arriving later would be blocked.
  #[test]
  fn text_not_blocked_across_no_update_frames() {
    let mut input = Input::new();

    // Frame 1 (update): no text input
    // UI: widget reads text_input (empty)
    assert_eq!(input.text_input(), "");
    // post_render_cleanup + begin_frame
    input.post_render_cleanup();
    input.begin_frame();

    // Frame 2 (no update): still no text
    // UI: widget reads text_input (empty)
    assert_eq!(input.text_input(), "");
    // Only post_render_cleanup, NO begin_frame (no update ran)
    input.post_render_cleanup();

    // Frame 3 (no update): key pressed
    input.handle_text_input("1");
    // UI: widget should see the text (consumed flag must be false)
    assert!(input.has_text_input());
    assert_eq!(input.text_input(), "1");
  }

  #[test]
  fn consume_text_input_prevents_subsequent_reads() {
    let mut input = Input::new();
    input.handle_text_input("abc");

    // Explicitly consume
    input.consume_text_input();

    // text_input returns empty
    assert_eq!(input.text_input(), "");
    assert!(!input.has_text_input());
  }

  #[test]
  fn text_input_consumed_resets_on_begin_frame() {
    let mut input = Input::new();
    input.handle_text_input("x");
    assert_eq!(input.text_input(), "x"); // consumed

    input.begin_frame();
    input.handle_text_input("y");

    // After begin_frame, consumed is reset
    assert_eq!(input.text_input(), "y");
  }

  /// Once update reads the text, a later reader in the same frame (the UI,
  /// the console) should NOT see the same text again.
  #[test]
  fn text_input_not_duplicated_across_update_and_render() {
    let mut input = Input::new();
    input.handle_text_input("1");

    // Game update() reads the text
    let text = input.text_input();
    assert_eq!(text, "1");

    // Engine suppress/unsuppress cycle
    input.suppress_oneshots();
    input.unsuppress_oneshots();

    // A later reader should NOT see the text again (already consumed)
    assert!(!input.has_text_input());
    assert_eq!(input.text_input(), "");
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // IME Deduplication Tests
  // Regression tests for macOS where both KeyboardInput{text} and Ime::Commit
  // fire for the same keypress, causing "11" from a single "1" press.
  // ─────────────────────────────────────────────────────────────────────────────

  /// macOS sends keyboard text first, then IME commit for the same key.
  /// Result should be "1", not "11".
  #[test]
  fn ime_dedup_keyboard_then_commit() {
    let mut input = Input::new();

    // macOS event order: keyboard first, then IME commit
    input.handle_text_input("1");
    input.handle_ime_commit("1".to_string());

    assert_eq!(input.text_input(), "1");
  }

  /// macOS might send IME commit first, then keyboard text.
  /// Result should be "1", not "11".
  #[test]
  fn ime_dedup_commit_then_keyboard() {
    let mut input = Input::new();

    // Reverse order: IME commit first, then keyboard
    input.handle_ime_commit("1".to_string());
    input.handle_text_input("1");

    // IME committed, so keyboard text is tracked but IME already wrote "1"
    // keyboard_text_len=1 from handle_text_input, but text was already "1" from commit
    // The keyboard text adds to the buffer making it "11", but the next IME commit
    // would clean it. Since no more commits come, we get "11" in this contrived order.
    // In practice on macOS, if IME commits first, ime_enabled=true blocks keyboard text.
    // Test the realistic case: with ime_enabled=true (as macOS sets it)
    let mut input2 = Input::new();
    input2.handle_ime_enabled();
    input2.handle_ime_commit("1".to_string());
    input2.handle_text_input("1"); // blocked by ime_enabled

    assert_eq!(input2.text_input(), "1");
  }

  /// Fast typing: two keys in the same frame, both with keyboard + IME events.
  /// Should produce "ab", not "aabb" or "ab" with dropped characters.
  #[test]
  fn ime_dedup_fast_typing_two_keys() {
    let mut input = Input::new();

    // Fast typing: keyboard events for both keys, then IME commits for both
    input.handle_text_input("a");
    input.handle_text_input("b");
    input.handle_ime_commit("a".to_string());
    input.handle_ime_commit("b".to_string());

    assert_eq!(input.text_input(), "ab");
  }

  /// Fast typing with interleaved events (keyboard a, IME a, keyboard b, IME b).
  #[test]
  fn ime_dedup_fast_typing_interleaved() {
    let mut input = Input::new();

    input.handle_text_input("a");
    input.handle_ime_commit("a".to_string());
    input.handle_text_input("b");
    input.handle_ime_commit("b".to_string());

    assert_eq!(input.text_input(), "ab");
  }

  /// Windows with IME enabled: keyboard text is skipped, only IME commit matters.
  #[test]
  fn ime_windows_with_ime_enabled() {
    let mut input = Input::new();
    input.handle_ime_enabled();

    input.handle_text_input("1"); // skipped (ime_enabled)
    input.handle_ime_commit("1".to_string());

    assert_eq!(input.text_input(), "1");
  }

  /// Windows without IME: only keyboard events, no IME commits.
  /// All keyboard text should be preserved.
  #[test]
  fn ime_windows_no_ime_fast_typing() {
    let mut input = Input::new();

    // Pure keyboard input, no IME events at all
    input.handle_text_input("a");
    input.handle_text_input("b");
    input.handle_text_input("c");

    assert_eq!(input.text_input(), "abc");
  }

  /// IME commit should not drop text from a different key that arrived
  /// earlier in the same frame via keyboard events.
  #[test]
  fn ime_commit_preserves_prior_ime_text() {
    let mut input = Input::new();

    // First key arrived via IME (no keyboard duplicate)
    input.handle_ime_commit("a".to_string());

    // Second key arrives via keyboard then IME
    input.handle_text_input("b");
    input.handle_ime_commit("b".to_string());

    // Both "a" and "b" should be present
    assert_eq!(input.text_input(), "ab");
  }

  /// begin_frame resets keyboard_text_len so previous frame's tracking
  /// doesn't interfere with the next frame's IME dedup.
  #[test]
  fn ime_dedup_resets_across_frames() {
    let mut input = Input::new();

    // Frame 1: keyboard + IME
    input.handle_text_input("x");
    input.handle_ime_commit("x".to_string());
    assert_eq!(input.text_input(), "x");

    input.begin_frame();

    // Frame 2: fresh keyboard + IME
    input.handle_text_input("y");
    input.handle_ime_commit("y".to_string());
    assert_eq!(input.text_input(), "y");
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // IME Ctrl Guard Tests
  // On Windows, IME may commit clipboard text during Ctrl+V. The guard in
  // handle_ime_commit skips commits when Ctrl/Cmd is held.
  // ─────────────────────────────────────────────────────────────────────────────

  /// IME commit is blocked when Ctrl is held (e.g. Ctrl+V paste on Windows).
  #[test]
  fn ime_commit_blocked_when_ctrl_held() {
    let mut input = Input::new();
    input.handle_keyboard(KeyCode::ControlLeft, ElementState::Pressed);
    input.handle_ime_commit("clipboard text".to_string());

    assert_eq!(input.text_input(), "");
  }

  /// IME commit is blocked when Cmd/Super is held (e.g. Cmd+V on macOS).
  #[test]
  fn ime_commit_blocked_when_super_held() {
    let mut input = Input::new();
    input.handle_keyboard(KeyCode::SuperLeft, ElementState::Pressed);
    input.handle_ime_commit("clipboard text".to_string());

    assert_eq!(input.text_input(), "");
  }

  /// IME commit works normally when no modifier is held.
  #[test]
  fn ime_commit_allowed_without_modifiers() {
    let mut input = Input::new();
    input.handle_ime_commit("hello".to_string());

    assert_eq!(input.text_input(), "hello");
  }

  /// IME commit works after Ctrl is released.
  #[test]
  fn ime_commit_allowed_after_ctrl_released() {
    let mut input = Input::new();
    input.handle_keyboard(KeyCode::ControlLeft, ElementState::Pressed);
    input.handle_keyboard(KeyCode::ControlLeft, ElementState::Released);
    input.handle_ime_commit("hello".to_string());

    assert_eq!(input.text_input(), "hello");
  }

  // ─────────────────────────────────────────────────────────────────────────────
  // Post-Render Cleanup Tests
  // Regression tests from the old frame loop, where key_repeat fired 2-3x per
  // keypress when the UI drew on consecutive frames without an update
  // (begin_frame not called). `post_render_cleanup` is a test-only shim for
  // that loop; begin_frame now runs every frame.
  // ─────────────────────────────────────────────────────────────────────────────

  #[test]
  fn post_render_cleanup_clears_key_repeat() {
    let mut input = Input::new();
    input.handle_keyboard(KeyCode::Backspace, ElementState::Pressed);

    assert!(input.key_repeat(KeyCode::Backspace));

    input.post_render_cleanup();

    // key_repeat should be cleared
    assert!(!input.key_repeat(KeyCode::Backspace));
    // But key_down should persist (key is still held)
    assert!(input.key_down(KeyCode::Backspace));
    // And key_pressed should persist (for game logic on next update)
    assert!(input.key_pressed(KeyCode::Backspace));
  }

  /// Regression test: without post_render_cleanup, a single Backspace press
  /// would fire key_repeat on 2-3 consecutive render frames when begin_frame
  /// is not called (no-update frames at high fps).
  #[test]
  fn key_repeat_fires_once_across_no_update_frames() {
    let mut input = Input::new();

    // Key pressed during event processing
    input.handle_keyboard(KeyCode::Backspace, ElementState::Pressed);

    // Frame 1 (no update): the UI checks key_repeat
    assert!(
      input.key_repeat(KeyCode::Backspace),
      "First render frame should see key_repeat"
    );
    // Engine calls post_render_cleanup (no begin_frame because no update)
    input.post_render_cleanup();

    // Frame 2 (no update): the UI checks key_repeat again
    assert!(
      !input.key_repeat(KeyCode::Backspace),
      "Second render frame must NOT see key_repeat (already processed)"
    );
    input.post_render_cleanup();

    // Frame 3 (update): begin_frame clears everything
    input.begin_frame();

    // No key event this frame, so key_repeat should be false
    assert!(!input.key_repeat(KeyCode::Backspace));
  }

  /// Same test for arrow keys — they use key_repeat for cursor movement.
  #[test]
  fn arrow_key_repeat_fires_once_across_no_update_frames() {
    let mut input = Input::new();
    input.handle_keyboard(KeyCode::ArrowLeft, ElementState::Pressed);

    // First render sees it
    assert!(input.key_repeat(KeyCode::ArrowLeft));
    input.post_render_cleanup();

    // Second render does NOT see it
    assert!(!input.key_repeat(KeyCode::ArrowLeft));

    // key_down still works (for continuous movement in game logic)
    assert!(input.key_down(KeyCode::ArrowLeft));
  }

  /// When the OS sends a real key repeat (held key), it should fire again.
  #[test]
  fn held_key_repeat_fires_on_each_os_repeat_event() {
    let mut input = Input::new();
    input.handle_keyboard(KeyCode::Backspace, ElementState::Pressed);

    // First render sees initial press
    assert!(input.key_repeat(KeyCode::Backspace));
    input.post_render_cleanup();

    // OS sends repeat event (key still held)
    input.handle_keyboard(KeyCode::Backspace, ElementState::Pressed);

    // Next render sees the OS repeat
    assert!(
      input.key_repeat(KeyCode::Backspace),
      "OS key repeat event should fire key_repeat"
    );
    input.post_render_cleanup();

    // No more OS events — key_repeat should be false
    assert!(!input.key_repeat(KeyCode::Backspace));
  }

  /// Full frame simulation: event → update → suppress → unsuppress →
  /// render → post_render_cleanup → conditional begin_frame.
  /// Verifies key_repeat fires exactly once for the text input widget
  /// and doesn't duplicate across the update/render boundary.
  #[test]
  fn full_frame_key_repeat_no_duplication() {
    let mut input = Input::new();

    // Events arrive
    input.handle_keyboard(KeyCode::Backspace, ElementState::Pressed);
    input.handle_text_input("x");

    // --- Update phase ---
    // Game update (key_repeat visible)
    assert!(input.key_repeat(KeyCode::Backspace));

    // Suppress after first update
    input.suppress_oneshots();
    assert!(!input.key_repeat(KeyCode::Backspace)); // suppressed

    // Unsuppress before render
    input.unsuppress_oneshots();

    // --- Render phase ---
    // UI: widget sees key_repeat and text_input
    assert!(input.key_repeat(KeyCode::Backspace));
    let text = input.text_input();
    assert_eq!(text, "x");

    // Widget reads text again — should be consumed
    assert_eq!(input.text_input(), "");

    // Post-render cleanup
    input.post_render_cleanup();

    // key_repeat cleared by cleanup
    assert!(!input.key_repeat(KeyCode::Backspace));

    // begin_frame (update ran)
    input.begin_frame();

    // Clean slate for next frame
    assert!(!input.key_repeat(KeyCode::Backspace));
    assert_eq!(input.text_input(), "");
  }

  #[test]
  fn block_pointer_hides_mouse_buttons_motion_and_scroll() {
    let mut input = Input::new();
    input.handle_mouse_move(40.0, 30.0);
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    input.handle_mouse_button(MouseButton::Right, ElementState::Pressed);
    input.handle_mouse_button(MouseButton::Right, ElementState::Released);
    input.handle_scroll(MouseScrollDelta::LineDelta(0.0, 1.0));
    input.handle_keyboard(KeyCode::KeyW, ElementState::Pressed);

    input.block_pointer();

    assert!(!input.mouse_pressed(MouseButton::Left));
    assert!(!input.mouse_down(MouseButton::Left));
    // Releases still report, so a drag begun before the block can end
    assert!(input.mouse_released(MouseButton::Right));
    assert!(!input.mouse_pressed_and_consume(MouseButton::Left));
    assert_eq!(input.mouse_delta(), (0.0, 0.0));
    assert_eq!(input.scroll_delta(), (0.0, 0.0));
    // The cursor position still reads, for hover
    assert_eq!(input.mouse_position(), (40.0, 30.0));
    // Keys are not the pointer
    assert!(input.key_pressed(KeyCode::KeyW));
    assert!(input.key_down(KeyCode::KeyW));
  }

  #[test]
  fn block_all_hides_keys_text_and_gamepad() {
    let mut input = Input::new();
    input.handle_keyboard(KeyCode::KeyW, ElementState::Pressed);
    input.handle_keyboard(KeyCode::KeyS, ElementState::Pressed);
    input.handle_keyboard(KeyCode::KeyS, ElementState::Released);
    input.handle_text_input("w");
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    input.gamepad_buttons_pressed.insert(GamepadButton::South);
    input.gamepad_buttons_down.insert(GamepadButton::South);
    input.gamepad_buttons_released.insert(GamepadButton::East);
    input.left_stick_x = 0.5;
    input.right_stick_y = -0.5;
    input.left_trigger = 1.0;

    input.block_all();

    assert!(!input.key_pressed(KeyCode::KeyW));
    assert!(!input.key_down(KeyCode::KeyW));
    assert!(!input.key_repeat(KeyCode::KeyW));
    assert!(input.key_released(KeyCode::KeyS));
    assert!(!input.key_pressed_and_consume(KeyCode::KeyW));
    assert!(!input.has_text_input());
    assert_eq!(input.text_input(), "");
    assert!(!input.mouse_pressed(MouseButton::Left));
    assert!(!input.gamepad_button_pressed(GamepadButton::South));
    assert!(!input.gamepad_button_down(GamepadButton::South));
    assert!(input.gamepad_button_released(GamepadButton::East));
    assert!(!input.gamepad_button_pressed_and_consume(GamepadButton::South));
    assert_eq!(input.left_stick(), (0.0, 0.0));
    assert_eq!(input.left_stick_x(), 0.0);
    assert_eq!(input.right_stick(), (0.0, 0.0));
    assert_eq!(input.right_stick_y(), 0.0);
    assert_eq!(input.left_trigger(), 0.0);
  }

  #[test]
  fn unblock_restores_input_that_blocked_consumes_left_alone() {
    let mut input = Input::new();
    input.handle_keyboard(KeyCode::Space, ElementState::Pressed);
    input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    input.handle_text_input("a");

    input.block_all();
    assert!(!input.key_pressed_and_consume(KeyCode::Space));
    assert!(!input.mouse_pressed_and_consume(MouseButton::Left));
    assert_eq!(input.text_input(), "");

    input.unblock();
    assert!(input.key_pressed(KeyCode::Space));
    assert!(input.mouse_pressed(MouseButton::Left));
    assert_eq!(input.text_input(), "a");
  }

  #[test]
  fn begin_frame_clears_an_input_block() {
    let mut input = Input::new();
    input.block_all();

    input.begin_frame();
    input.handle_keyboard(KeyCode::Space, ElementState::Pressed);

    assert!(input.key_pressed(KeyCode::Space));
  }

  // ── Frame-scoped queue, fixed view and consumption ──────────────────────

  #[test]
  fn queued_events_become_visible_at_begin_frame() {
    let mut input = Input::new();
    input.queue(InputEvent::Key {
      key: KeyCode::Space,
      pressed: true,
    });
    assert!(
      !input.key_pressed(KeyCode::Space),
      "not until the frame begins"
    );
    input.begin_frame();
    assert!(input.key_pressed(KeyCode::Space));
    assert!(input.key_down(KeyCode::Space));
    input.begin_frame();
    assert!(
      !input.key_pressed(KeyCode::Space),
      "one-shots last one frame"
    );
    assert!(input.key_down(KeyCode::Space), "held state persists");
  }

  #[test]
  fn just_pressed_is_visible_in_the_first_fixed_step_only() {
    let mut input = Input::new();
    input.queue(InputEvent::Key {
      key: KeyCode::KeyA,
      pressed: true,
    });
    input.begin_frame();
    input.set_fixed_step(0);
    assert!(input.key_pressed(KeyCode::KeyA));
    input.set_fixed_step(1);
    assert!(!input.key_pressed(KeyCode::KeyA));
    assert!(input.key_down(KeyCode::KeyA));
    input.end_fixed_steps(true);
    input.begin_frame();
    assert!(!input.key_pressed(KeyCode::KeyA));
    input.set_fixed_step(0);
    assert!(
      !input.key_pressed(KeyCode::KeyA),
      "the fixed view was cleared once a step ran"
    );
  }

  #[test]
  fn a_press_in_a_frame_without_a_fixed_step_reaches_the_next_fixed_step() {
    let mut input = Input::new();
    input.queue(InputEvent::Key {
      key: KeyCode::Space,
      pressed: true,
    });
    input.begin_frame();
    assert!(input.key_pressed(KeyCode::Space), "frame view, frame 1");
    input.end_fixed_steps(false); // no fixed step ran this frame
    input.begin_frame();
    assert!(
      !input.key_pressed(KeyCode::Space),
      "the frame view lasts one frame"
    );
    input.set_fixed_step(0);
    assert!(
      input.key_pressed(KeyCode::Space),
      "the first fixed step after the press still sees it"
    );
    input.end_fixed_steps(true);
    input.begin_frame();
    input.set_fixed_step(0);
    assert!(!input.key_pressed(KeyCode::Space));
  }

  #[test]
  fn consumed_input_is_hidden_for_the_rest_of_the_frame() {
    let mut input = Input::new();
    input.queue(InputEvent::Key {
      key: KeyCode::Escape,
      pressed: true,
    });
    input.queue(InputEvent::MouseButton {
      button: MouseButton::Left,
      pressed: true,
    });
    input.begin_frame();
    assert!(input.key_pressed(KeyCode::Escape));
    input.consume_key(KeyCode::Escape);
    assert!(!input.key_pressed(KeyCode::Escape));
    assert!(
      !input.key_down(KeyCode::Escape),
      "consumed means not pressed at all this frame"
    );
    input.set_fixed_step(0);
    assert!(
      !input.key_pressed(KeyCode::Escape),
      "the fixed view is hidden too"
    );
    input.end_fixed_steps(true);
    input.consume_mouse(MouseButton::Left);
    assert!(!input.mouse_pressed(MouseButton::Left));
    input.begin_frame();
    assert!(
      input.key_down(KeyCode::Escape),
      "the key is still physically held next frame"
    );
  }

  #[test]
  fn text_and_release_events_round_trip() {
    let mut input = Input::new();
    input.queue(InputEvent::Text("hi".into()));
    input.queue(InputEvent::Key {
      key: KeyCode::KeyH,
      pressed: true,
    });
    input.queue(InputEvent::Key {
      key: KeyCode::KeyH,
      pressed: false,
    });
    input.begin_frame();
    assert_eq!(input.text_input(), "hi");
    assert!(input.key_pressed(KeyCode::KeyH));
    assert!(input.key_released(KeyCode::KeyH));
    assert!(!input.key_down(KeyCode::KeyH));
  }

  fn frame(input: &mut Input, events: Vec<InputEvent>) {
    for event in events {
      input.queue(event);
    }
    input.begin_frame();
  }

  #[test]
  fn the_last_device_follows_the_newest_input() {
    let mut input = Input::new();
    assert_eq!(input.last_device(), InputDevice::Mouse);
    frame(
      &mut input,
      vec![InputEvent::Key {
        key: KeyCode::KeyA,
        pressed: true,
      }],
    );
    assert_eq!(input.last_device(), InputDevice::Keyboard);
    frame(
      &mut input,
      vec![InputEvent::PadButton {
        button: GamepadButton::South,
        pressed: true,
      }],
    );
    assert_eq!(input.last_device(), InputDevice::Gamepad);
    // A one-unit nudge is jitter, not the mouse
    frame(&mut input, vec![InputEvent::MouseMove { x: 1.0, y: 0.0 }]);
    assert_eq!(input.last_device(), InputDevice::Gamepad);
    frame(&mut input, vec![InputEvent::MouseMove { x: 40.0, y: 30.0 }]);
    assert_eq!(input.last_device(), InputDevice::Mouse);
    frame(
      &mut input,
      vec![InputEvent::PadStick {
        stick: Stick::Left,
        x: 0.9,
        y: 0.0,
      }],
    );
    assert_eq!(input.last_device(), InputDevice::Gamepad);
    frame(
      &mut input,
      vec![InputEvent::Scroll(MouseScrollDelta::LineDelta(0.0, 1.0))],
    );
    assert_eq!(input.last_device(), InputDevice::Mouse);
  }

  #[test]
  fn the_wheel_shows_in_the_first_fixed_step_only() {
    let mut input = Input::new();
    frame(
      &mut input,
      vec![InputEvent::Scroll(MouseScrollDelta::LineDelta(0.0, 2.0))],
    );
    assert_eq!(
      input.scroll_delta(),
      (0.0, 2.0),
      "the frame view, as the UI pass reads it"
    );
    input.set_fixed_step(0);
    assert_eq!(input.scroll_delta(), (0.0, 2.0));
    input.set_fixed_step(1);
    assert_eq!(
      input.scroll_delta(),
      (0.0, 0.0),
      "a second step must not count it again"
    );
    input.end_fixed_steps(true);
    input.begin_frame();
    input.set_fixed_step(0);
    assert_eq!(input.scroll_delta(), (0.0, 0.0), "seen once, gone");
  }

  #[test]
  fn a_wheel_with_no_fixed_step_waits_for_the_next_frame() {
    let mut input = Input::new();
    frame(
      &mut input,
      vec![InputEvent::Scroll(MouseScrollDelta::LineDelta(0.0, -1.0))],
    );
    input.end_fixed_steps(false);
    frame(
      &mut input,
      vec![InputEvent::Scroll(MouseScrollDelta::LineDelta(0.0, -1.0))],
    );
    input.set_fixed_step(0);
    assert_eq!(
      input.scroll_delta(),
      (0.0, -2.0),
      "both frames' steps reach the first step that runs"
    );
  }

  #[test]
  fn a_paused_frame_drops_the_wheel() {
    let mut input = Input::new();
    frame(
      &mut input,
      vec![InputEvent::Scroll(MouseScrollDelta::LineDelta(0.0, 3.0))],
    );
    // The engine passes `step > 0 || paused`: a pause clears what no step saw
    input.end_fixed_steps(true);
    input.begin_frame();
    input.set_fixed_step(0);
    assert_eq!(input.scroll_delta(), (0.0, 0.0));
  }

  #[test]
  fn consuming_the_wheel_hides_it_from_the_fixed_steps() {
    let mut input = Input::new();
    frame(
      &mut input,
      vec![InputEvent::Scroll(MouseScrollDelta::LineDelta(0.0, 1.0))],
    );
    input.consume_scroll();
    input.set_fixed_step(0);
    assert_eq!(input.scroll_delta(), (0.0, 0.0));
  }

  #[test]
  fn a_virtual_pad_button_reads_like_a_real_one() {
    let mut input = Input::new();
    frame(
      &mut input,
      vec![InputEvent::PadButton {
        button: GamepadButton::South,
        pressed: true,
      }],
    );
    assert!(input.gamepad_connected());
    assert_eq!(input.gamepad_name(), "Virtual pad");
    assert!(input.gamepad_button_pressed(GamepadButton::South));
    assert!(input.gamepad_button_down(GamepadButton::South));
    input.set_fixed_step(0);
    assert!(
      input.gamepad_button_pressed(GamepadButton::South),
      "the fixed view sees the press too"
    );
    input.end_fixed_steps(true);
    frame(
      &mut input,
      vec![InputEvent::PadButton {
        button: GamepadButton::South,
        pressed: false,
      }],
    );
    assert!(input.gamepad_button_released(GamepadButton::South));
    assert!(!input.gamepad_button_down(GamepadButton::South));
  }

  #[test]
  fn a_virtual_stick_is_deadzoned() {
    let mut input = Input::new();
    frame(
      &mut input,
      vec![InputEvent::PadStick {
        stick: Stick::Right,
        x: 0.1,
        y: 0.05,
      }],
    );
    assert_eq!(input.right_stick(), (0.0, 0.0));
    frame(
      &mut input,
      vec![InputEvent::PadStick {
        stick: Stick::Right,
        x: 1.0,
        y: 0.0,
      }],
    );
    let (x, y) = input.right_stick();
    assert!((x - 1.0).abs() < 1e-5 && y.abs() < 1e-5);
    assert_eq!(input.right_stick_raw(), (1.0, 0.0));
  }

  #[test]
  fn actions_read_through_their_bindings() {
    let mut input = Input::new();
    input.actions_mut().bind(
      "jump",
      [
        Binding::Key(KeyCode::Space),
        Binding::Pad(GamepadButton::South),
      ],
    );
    frame(
      &mut input,
      vec![InputEvent::PadButton {
        button: GamepadButton::South,
        pressed: true,
      }],
    );
    assert!(input.action_pressed("jump"));
    assert!(input.action_held("jump"));
    input.set_fixed_step(0);
    assert!(input.action_pressed("jump"), "the first fixed step sees it");
    input.set_fixed_step(1);
    assert!(!input.action_pressed("jump"));
    assert!(input.action_held("jump"), "held is not a one-shot");
    input.end_fixed_steps(true);
    frame(
      &mut input,
      vec![InputEvent::PadButton {
        button: GamepadButton::South,
        pressed: false,
      }],
    );
    assert!(input.action_released("jump"));
    assert!(!input.action_pressed("nothing bound"));
  }

  #[test]
  fn stick_directions_press_past_half_and_release_below_forty_percent() {
    let mut input = Input::new();
    let up = StickDirection {
      stick: Stick::Left,
      direction: Direction::Up,
    };
    let push = |y| InputEvent::PadStick {
      stick: Stick::Left,
      x: 0.0,
      y,
    };
    frame(&mut input, vec![push(0.45)]);
    assert!(!input.stick_direction_down(up));
    frame(&mut input, vec![push(0.7)]);
    assert!(input.stick_direction_pressed(up) && input.stick_direction_down(up));
    frame(&mut input, vec![push(0.45)]);
    assert!(
      input.stick_direction_down(up),
      "hysteresis: still down above 0.4"
    );
    assert!(!input.stick_direction_pressed(up));
    frame(&mut input, vec![push(0.3)]);
    assert!(!input.stick_direction_down(up));
    assert!(!input.action_held("ui.up"));
  }

  #[test]
  fn the_ui_actions_are_bound_from_the_start() {
    let mut input = Input::new();
    frame(
      &mut input,
      vec![InputEvent::Key {
        key: KeyCode::Enter,
        pressed: true,
      }],
    );
    assert!(input.action_pressed("ui.confirm"));
    frame(
      &mut input,
      vec![InputEvent::PadButton {
        button: GamepadButton::East,
        pressed: true,
      }],
    );
    assert!(input.action_pressed("ui.back"));
  }

  #[test]
  fn consuming_a_binding_hides_it() {
    let mut input = Input::new();
    frame(
      &mut input,
      vec![InputEvent::Key {
        key: KeyCode::ArrowDown,
        pressed: true,
      }],
    );
    input.consume_binding(&Binding::Key(KeyCode::ArrowDown));
    assert!(!input.action_pressed("ui.down"));
    assert!(!input.key_pressed(KeyCode::ArrowDown));
  }
}
