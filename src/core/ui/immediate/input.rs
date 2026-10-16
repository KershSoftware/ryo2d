//! What the UI pass reads of input: a snapshot that records what it consumes.

use std::cell::Cell;
use std::collections::HashSet;

use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use crate::core::input::{GamepadButton, Input, InputView};

/// The frame's input as the UI sees it.
///
/// The UI pass runs before gameplay. Widgets read this snapshot and consume
/// what they use (a clicked button takes the click, a focused text field the
/// typed text); the engine then applies the consumption to `Input`, so
/// gameplay never sees input the UI took. A consumed key or button reads as
/// not pressed, held or released for the rest of the frame.
#[derive(Debug, Clone)]
pub struct UiInput {
  view: InputView,
  consumed_keys: HashSet<KeyCode>,
  consumed_mouse: HashSet<MouseButton>,
  text_consumed: Cell<bool>,
  scroll_consumed: bool,
  /// UI units per screen unit (1 unless the game set a UI scale)
  mouse_scale: f32,
}

impl Default for UiInput {
  fn default() -> Self {
    Self {
      view: InputView::default(),
      consumed_keys: HashSet::new(),
      consumed_mouse: HashSet::new(),
      text_consumed: Cell::new(false),
      scroll_consumed: false,
      mouse_scale: 1.0,
    }
  }
}

/// What a UI pass consumed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiConsumed {
  pub keys: Vec<KeyCode>,
  pub mouse: Vec<MouseButton>,
  pub text: bool,
  pub scroll: bool,
}

impl UiInput {
  /// The frame's input, read without consuming anything.
  pub fn snapshot(input: &Input) -> Self {
    Self::from_view(input.ui_view())
  }

  /// The input `view` describes, nothing consumed (the UI test driver
  /// scripts its frames with it)
  pub(crate) fn from_view(view: InputView) -> Self {
    Self {
      view,
      ..Self::default()
    }
  }

  /// Read mouse positions in UI units: screen positions times `scale` (UI
  /// units per screen unit). A non-finite or non-positive scale is ignored.
  pub fn with_mouse_scale(mut self, scale: f32) -> Self {
    if scale.is_finite() && scale > 0.0 {
      self.mouse_scale = scale;
    }
    self
  }

  // --- mouse ---

  /// The pointer, in UI units
  pub fn mouse_position(&self) -> (f32, f32) {
    (self.mouse_x(), self.mouse_y())
  }

  pub fn mouse_x(&self) -> f32 {
    self.view.mouse.0 * self.mouse_scale
  }

  pub fn mouse_y(&self) -> f32 {
    self.view.mouse.1 * self.mouse_scale
  }

  pub fn mouse_down(&self, button: MouseButton) -> bool {
    !self.consumed_mouse.contains(&button) && self.view.mouse_down.contains(&button)
  }

  pub fn mouse_pressed(&self, button: MouseButton) -> bool {
    !self.consumed_mouse.contains(&button) && self.view.mouse_pressed.contains(&button)
  }

  pub fn mouse_released(&self, button: MouseButton) -> bool {
    !self.consumed_mouse.contains(&button) && self.view.mouse_released.contains(&button)
  }

  pub fn scroll_delta(&self) -> (f32, f32) {
    if self.scroll_consumed {
      (0.0, 0.0)
    } else {
      self.view.scroll
    }
  }

  // --- keyboard ---

  pub fn key_down(&self, key: KeyCode) -> bool {
    !self.consumed_keys.contains(&key) && self.view.keys_down.contains(&key)
  }

  pub fn key_pressed(&self, key: KeyCode) -> bool {
    !self.consumed_keys.contains(&key) && self.view.keys_pressed.contains(&key)
  }

  pub fn key_released(&self, key: KeyCode) -> bool {
    !self.consumed_keys.contains(&key) && self.view.keys_released.contains(&key)
  }

  /// The key fired this frame: initial press or repeat.
  pub fn key_repeat(&self, key: KeyCode) -> bool {
    !self.consumed_keys.contains(&key) && self.view.keys_repeated.contains(&key)
  }

  /// The frame's typed text. The first non-empty read consumes it.
  pub fn text_input(&self) -> &str {
    if self.text_consumed.get() {
      return "";
    }
    if !self.view.text.is_empty() {
      self.text_consumed.set(true);
    }
    &self.view.text
  }

  pub fn ime_preedit(&self) -> &str {
    &self.view.ime_preedit
  }

  pub fn ime_preedit_cursor(&self) -> Option<(usize, usize)> {
    self.view.ime_preedit_cursor
  }

  pub fn ime_composing(&self) -> bool {
    self.view.ime_composing
  }

  pub fn ime_enabled(&self) -> bool {
    self.view.ime_enabled
  }

  // --- gamepad ---

  pub fn gamepad_button_pressed(&self, button: GamepadButton) -> bool {
    self.view.gamepad_pressed.contains(&button)
  }

  pub fn left_stick(&self) -> (f32, f32) {
    self.view.left_stick
  }

  // --- consumption ---

  /// Take `key` for the rest of the frame, from the UI and from gameplay.
  pub fn consume_key(&mut self, key: KeyCode) {
    self.consumed_keys.insert(key);
  }

  /// Take `button` for the rest of the frame, from the UI and from gameplay.
  pub fn consume_mouse(&mut self, button: MouseButton) {
    self.consumed_mouse.insert(button);
  }

  /// Take the frame's typed text.
  pub fn consume_text(&self) {
    self.text_consumed.set(true);
  }

  /// Take the frame's mouse wheel, from the UI and from gameplay.
  pub fn consume_scroll(&mut self) {
    self.scroll_consumed = true;
  }

  pub fn is_key_consumed(&self, key: KeyCode) -> bool {
    self.consumed_keys.contains(&key)
  }

  pub fn is_mouse_consumed(&self, button: MouseButton) -> bool {
    self.consumed_mouse.contains(&button)
  }

  /// Everything consumed so far.
  pub fn consumed(&self) -> UiConsumed {
    UiConsumed {
      keys: self.consumed_keys.iter().copied().collect(),
      mouse: self.consumed_mouse.iter().copied().collect(),
      text: self.text_consumed.get(),
      scroll: self.scroll_consumed,
    }
  }

  /// Hide what the UI consumed from `input` for the rest of the frame.
  /// Take the whole keyboard for the frame: every key down, pressed or
  /// released, and the typed text (a focused text field does this).
  pub fn consume_keyboard(&mut self) {
    let keys: Vec<KeyCode> = self
      .view
      .keys_down
      .iter()
      .chain(&self.view.keys_pressed)
      .chain(&self.view.keys_released)
      .copied()
      .collect();
    self.consumed_keys.extend(keys);
    self.consume_text();
  }

  /// Mouse buttons pressed this frame that the UI took.
  pub(crate) fn mouse_presses_taken(&self) -> impl Iterator<Item = MouseButton> + '_ {
    self
      .consumed_mouse
      .iter()
      .copied()
      .filter(|button| self.view.mouse_pressed.contains(button))
  }

  /// Whether `button` is physically down, consumed or not.
  pub(crate) fn mouse_held_raw(&self, button: MouseButton) -> bool {
    self.view.mouse_down.contains(&button)
  }

  pub fn apply_consumed(&self, input: &mut Input) {
    for &key in &self.consumed_keys {
      input.consume_key(key);
    }
    for &button in &self.consumed_mouse {
      input.consume_mouse(button);
    }
    if self.text_consumed.get() {
      input.consume_text();
    }
    if self.scroll_consumed {
      input.consume_scroll();
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::input::{Input, InputEvent};
  use winit::keyboard::KeyCode;

  #[test]
  fn snapshot_reads_and_consumption_flows_back() {
    let mut input = Input::new();
    input.queue(InputEvent::Key {
      key: KeyCode::Escape,
      pressed: true,
    });
    input.queue(InputEvent::MouseMove { x: 10.0, y: 20.0 });
    input.begin_frame();
    let mut ui = UiInput::snapshot(&input);
    assert!(ui.key_pressed(KeyCode::Escape));
    assert_eq!(ui.mouse_position(), (10.0, 20.0));
    ui.consume_key(KeyCode::Escape);
    assert!(!ui.key_pressed(KeyCode::Escape));
    ui.apply_consumed(&mut input);
    assert!(
      !input.key_pressed(KeyCode::Escape),
      "gameplay no longer sees it"
    );
  }

  #[test]
  fn typed_text_read_by_the_ui_is_gone_for_gameplay() {
    let mut input = Input::new();
    input.queue(InputEvent::Text("hi".into()));
    input.begin_frame();
    let ui = UiInput::snapshot(&input);
    assert_eq!(ui.text_input(), "hi");
    assert_eq!(ui.text_input(), "", "the first read consumes");
    ui.apply_consumed(&mut input);
    assert_eq!(input.text_input(), "");
  }

  #[test]
  fn a_mouse_scale_converts_positions_to_ui_units() {
    let mut input = Input::new();
    input.queue(InputEvent::MouseMove { x: 100.0, y: 40.0 });
    input.begin_frame();
    let ui = UiInput::snapshot(&input);
    assert_eq!(ui.mouse_position(), (100.0, 40.0));
    let ui = ui.with_mouse_scale(0.5);
    assert_eq!(ui.mouse_position(), (50.0, 20.0));
    assert_eq!((ui.mouse_x(), ui.mouse_y()), (50.0, 20.0));
    assert_eq!(ui.with_mouse_scale(f32::NAN).mouse_x(), 50.0, "ignored");
  }

  #[test]
  fn taking_a_snapshot_consumes_nothing() {
    let mut input = Input::new();
    input.queue(InputEvent::Text("hi".into()));
    input.queue(InputEvent::MouseButton {
      button: MouseButton::Left,
      pressed: true,
    });
    input.begin_frame();
    let ui = UiInput::snapshot(&input);
    assert!(ui.mouse_pressed(MouseButton::Left));
    ui.apply_consumed(&mut input);
    assert!(input.mouse_pressed(MouseButton::Left));
    assert_eq!(input.text_input(), "hi");
  }
}
