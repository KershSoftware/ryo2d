//! Named, rebindable actions: a game binds "jump" to keys, mouse buttons,
//! pad buttons and stick directions, and reads `input.action_pressed("jump")`.
//! The UI's own actions (`ui.*`) are bound here too.

use std::collections::BTreeMap;

use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use super::{GamepadButton, Stick};
use crate::engine_warn;

macro_rules! key_table {
  ($($key:ident),* $(,)?) => {
    &[$((stringify!($key), KeyCode::$key)),*]
  };
}

/// The keys that bindings and `RYO_INPUT` can name, by winit `KeyCode` name
pub(crate) const KEY_NAMES: &[(&str, KeyCode)] = key_table![
  KeyA,
  KeyB,
  KeyC,
  KeyD,
  KeyE,
  KeyF,
  KeyG,
  KeyH,
  KeyI,
  KeyJ,
  KeyK,
  KeyL,
  KeyM,
  KeyN,
  KeyO,
  KeyP,
  KeyQ,
  KeyR,
  KeyS,
  KeyT,
  KeyU,
  KeyV,
  KeyW,
  KeyX,
  KeyY,
  KeyZ,
  Digit0,
  Digit1,
  Digit2,
  Digit3,
  Digit4,
  Digit5,
  Digit6,
  Digit7,
  Digit8,
  Digit9,
  ArrowUp,
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  Space,
  Enter,
  Tab,
  Escape,
  Backspace,
  Delete,
  Insert,
  Home,
  End,
  PageUp,
  PageDown,
  Minus,
  Equal,
  BracketLeft,
  BracketRight,
  Backslash,
  Semicolon,
  Quote,
  Backquote,
  Comma,
  Period,
  Slash,
  ShiftLeft,
  ShiftRight,
  ControlLeft,
  ControlRight,
  AltLeft,
  AltRight,
  F1,
  F2,
  F3,
  F4,
  F5,
  F6,
  F7,
  F8,
  F9,
  F10,
  F11,
  F12,
  Numpad0,
  Numpad1,
  Numpad2,
  Numpad3,
  Numpad4,
  Numpad5,
  Numpad6,
  Numpad7,
  Numpad8,
  Numpad9,
  NumpadAdd,
  NumpadSubtract,
  NumpadMultiply,
  NumpadDivide,
  NumpadDecimal,
  NumpadEnter,
];

/// Gamepad buttons by `GamepadButton` variant name
pub(crate) const PAD_NAMES: &[(&str, GamepadButton)] = &[
  ("South", GamepadButton::South),
  ("East", GamepadButton::East),
  ("West", GamepadButton::West),
  ("North", GamepadButton::North),
  ("LeftBumper", GamepadButton::LeftBumper),
  ("RightBumper", GamepadButton::RightBumper),
  ("LeftTrigger", GamepadButton::LeftTrigger),
  ("RightTrigger", GamepadButton::RightTrigger),
  ("LeftStick", GamepadButton::LeftStick),
  ("RightStick", GamepadButton::RightStick),
  ("Select", GamepadButton::Select),
  ("Start", GamepadButton::Start),
  ("Guide", GamepadButton::Guide),
  ("DPadUp", GamepadButton::DPadUp),
  ("DPadDown", GamepadButton::DPadDown),
  ("DPadLeft", GamepadButton::DPadLeft),
  ("DPadRight", GamepadButton::DPadRight),
];

/// The key named `name` (a winit `KeyCode` name, any case)
pub(crate) fn key_from_name(name: &str) -> Option<KeyCode> {
  KEY_NAMES
    .iter()
    .find(|(known, _)| known.eq_ignore_ascii_case(name))
    .map(|&(_, key)| key)
}

/// The name of `key` in [`KEY_NAMES`], if it has one
pub(crate) fn key_name(key: KeyCode) -> Option<&'static str> {
  KEY_NAMES
    .iter()
    .find(|&&(_, known)| known == key)
    .map(|&(name, _)| name)
}

/// The pad button named `name` (a `GamepadButton` variant name, any case)
pub(crate) fn pad_button_from_name(name: &str) -> Option<GamepadButton> {
  PAD_NAMES
    .iter()
    .find(|(known, _)| known.eq_ignore_ascii_case(name))
    .map(|&(_, button)| button)
}

fn pad_button_name(button: GamepadButton) -> Option<&'static str> {
  PAD_NAMES
    .iter()
    .find(|&&(_, known)| known == button)
    .map(|&(name, _)| name)
}

const MOUSE_NAMES: &[(&str, MouseButton)] = &[
  ("Left", MouseButton::Left),
  ("Right", MouseButton::Right),
  ("Middle", MouseButton::Middle),
  ("Back", MouseButton::Back),
  ("Forward", MouseButton::Forward),
];

const STICK_NAMES: &[(&str, Stick)] = &[("Left", Stick::Left), ("Right", Stick::Right)];

const DIRECTION_NAMES: &[(&str, Direction)] = &[
  ("Up", Direction::Up),
  ("Down", Direction::Down),
  ("Left", Direction::Left),
  ("Right", Direction::Right),
];

/// A direction on the screen or a stick
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
  Up,
  Down,
  Left,
  Right,
}

/// One stick pushed one way, read as a button past half deflection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StickDirection {
  pub stick: Stick,
  pub direction: Direction,
}

/// What can trigger an action
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Binding {
  Key(KeyCode),
  Mouse(MouseButton),
  Pad(GamepadButton),
  Stick(StickDirection),
}

impl Binding {
  /// Read a binding from its config text: "Key:Space", "Mouse:Left",
  /// "Pad:South", "Stick:LeftUp"
  pub fn parse(text: &str) -> Option<Binding> {
    let (kind, name) = text.split_once(':')?;
    match kind {
      "Key" => key_from_name(name).map(Binding::Key),
      "Mouse" => {
        if let Some(&(_, button)) = MOUSE_NAMES.iter().find(|(known, _)| *known == name) {
          return Some(Binding::Mouse(button));
        }
        let index = name.strip_prefix("Other")?.parse().ok()?;
        Some(Binding::Mouse(MouseButton::Other(index)))
      }
      "Pad" => pad_button_from_name(name).map(Binding::Pad),
      "Stick" => STICK_NAMES.iter().find_map(|&(stick_name, stick)| {
        let direction = name.strip_prefix(stick_name)?;
        let &(_, direction) = DIRECTION_NAMES
          .iter()
          .find(|(known, _)| *known == direction)?;
        Some(Binding::Stick(StickDirection { stick, direction }))
      }),
      _ => None,
    }
  }

  /// The binding as config text, the form [`parse`](Self::parse) reads
  pub fn to_config(&self) -> String {
    match self {
      Binding::Key(key) => match key_name(*key) {
        Some(name) => format!("Key:{name}"),
        None => format!("Key:{key:?}"),
      },
      Binding::Mouse(MouseButton::Other(index)) => format!("Mouse:Other{index}"),
      Binding::Mouse(button) => {
        let name = MOUSE_NAMES
          .iter()
          .find(|&&(_, known)| known == *button)
          .map_or("Left", |&(name, _)| name);
        format!("Mouse:{name}")
      }
      Binding::Pad(button) => format!("Pad:{}", pad_button_name(*button).unwrap_or("South")),
      Binding::Stick(StickDirection { stick, direction }) => {
        let stick = STICK_NAMES
          .iter()
          .find(|&&(_, known)| known == *stick)
          .map_or("Left", |&(name, _)| name);
        let direction = DIRECTION_NAMES
          .iter()
          .find(|&&(_, known)| known == *direction)
          .map_or("Up", |&(name, _)| name);
        format!("Stick:{stick}{direction}")
      }
    }
  }
}

#[derive(Debug, Clone)]
struct Entry {
  name: String,
  defaults: Vec<Binding>,
  current: Vec<Binding>,
}

/// The game's actions and their bindings. Lives in `Input`; reach it with
/// `ctx.actions()`
#[derive(Debug, Clone, Default)]
pub struct Actions {
  entries: Vec<Entry>,
  saved: BTreeMap<String, Vec<Binding>>,
  dirty: bool,
}

impl Actions {
  /// Bind `name` to `bindings` as its defaults, replacing earlier defaults. A
  /// saved override (the player's settings) takes precedence
  pub fn bind(&mut self, name: &str, bindings: impl IntoIterator<Item = Binding>) -> &mut Self {
    let defaults: Vec<Binding> = bindings.into_iter().collect();
    let current = self
      .saved
      .get(name)
      .cloned()
      .unwrap_or_else(|| defaults.clone());
    match self.entries.iter_mut().find(|entry| entry.name == name) {
      Some(entry) => {
        entry.defaults = defaults;
        entry.current = current;
      }
      None => self.entries.push(Entry {
        name: name.to_string(),
        defaults,
        current,
      }),
    }
    self
  }

  /// What triggers `name`; empty for an unknown action
  pub fn bindings(&self, name: &str) -> &[Binding] {
    self
      .entries
      .iter()
      .find(|entry| entry.name == name)
      .map_or(&[], |entry| &entry.current)
  }

  /// Replace all of `name`'s current bindings (saved if they differ from the
  /// defaults)
  pub fn set_bindings(&mut self, name: &str, bindings: Vec<Binding>) {
    if let Some(entry) = self.entries.iter_mut().find(|entry| entry.name == name) {
      entry.current = bindings;
      self.dirty = true;
    }
  }

  /// Bind `binding` in `slot` (appending past the end). If another action in
  /// the same group has it, the two swap; returns that action's name
  pub fn rebind(&mut self, name: &str, slot: usize, binding: Binding) -> Option<String> {
    let index = self.entries.iter().position(|entry| entry.name == name)?;
    let group = Self::group(name);
    let old = self.entries[index].current.get(slot).copied();
    let mut swapped = None;
    for (other, entry) in self.entries.iter_mut().enumerate() {
      if other == index || Self::group(&entry.name) != group {
        continue;
      }
      if let Some(at) = entry.current.iter().position(|b| *b == binding) {
        match old {
          Some(old) => entry.current[at] = old,
          None => {
            entry.current.remove(at);
          }
        }
        swapped = Some(entry.name.clone());
        break;
      }
    }
    let current = &mut self.entries[index].current;
    if slot < current.len() {
      current[slot] = binding;
    } else {
      current.push(binding);
    }
    self.dirty = true;
    swapped
  }

  /// Put every action back to its defaults and forget the saved overrides
  pub fn reset_defaults(&mut self) {
    for entry in &mut self.entries {
      entry.current = entry.defaults.clone();
    }
    self.saved.clear();
    self.dirty = true;
  }

  /// The action names, in the order they were first bound
  pub fn names(&self) -> impl Iterator<Item = &str> {
    self.entries.iter().map(|entry| entry.name.as_str())
  }

  /// "ui" for `ui.*` actions, "game" for the rest
  pub fn group(name: &str) -> &str {
    match name.split_once('.') {
      Some(("ui", _)) => "ui",
      _ => "game",
    }
  }

  /// An `Actions` with the UI's navigation actions bound
  pub(crate) fn with_ui_defaults() -> Actions {
    use Direction::{Down, Left, Right, Up};
    let stick = |direction| {
      Binding::Stick(StickDirection {
        stick: Stick::Left,
        direction,
      })
    };
    let key = Binding::Key;
    let pad = Binding::Pad;
    let mut actions = Actions::default();
    actions
      .bind(
        "ui.up",
        [key(KeyCode::ArrowUp), pad(GamepadButton::DPadUp), stick(Up)],
      )
      .bind(
        "ui.down",
        [
          key(KeyCode::ArrowDown),
          pad(GamepadButton::DPadDown),
          stick(Down),
        ],
      )
      .bind(
        "ui.left",
        [
          key(KeyCode::ArrowLeft),
          pad(GamepadButton::DPadLeft),
          stick(Left),
        ],
      )
      .bind(
        "ui.right",
        [
          key(KeyCode::ArrowRight),
          pad(GamepadButton::DPadRight),
          stick(Right),
        ],
      )
      .bind(
        "ui.confirm",
        [
          key(KeyCode::Enter),
          key(KeyCode::NumpadEnter),
          key(KeyCode::Space),
          pad(GamepadButton::South),
        ],
      )
      .bind("ui.back", [key(KeyCode::Escape), pad(GamepadButton::East)])
      .bind("ui.next", [key(KeyCode::Tab)])
      .bind(
        "ui.tab_next",
        [key(KeyCode::KeyE), pad(GamepadButton::RightBumper)],
      )
      .bind(
        "ui.tab_prev",
        [key(KeyCode::KeyQ), pad(GamepadButton::LeftBumper)],
      );
    actions
  }

  /// Take saved bindings (action name to binding texts) as overrides. Actions
  /// already bound switch to them at once; the rest when the game binds them
  pub(crate) fn load_saved(&mut self, saved: &BTreeMap<String, Vec<String>>) {
    for (name, texts) in saved {
      let mut bindings = Vec::with_capacity(texts.len());
      for text in texts {
        match Binding::parse(text) {
          Some(binding) => bindings.push(binding),
          None => engine_warn!(
            module: "input",
            "saved binding {text:?} for {name:?} is not a binding; skipped"
          ),
        }
      }
      if let Some(entry) = self.entries.iter_mut().find(|entry| entry.name == *name) {
        entry.current = bindings.clone();
      }
      self.saved.insert(name.clone(), bindings);
    }
  }

  /// The actions whose bindings differ from their defaults, as binding texts
  pub(crate) fn saved_map(&self) -> BTreeMap<String, Vec<String>> {
    self
      .entries
      .iter()
      .filter(|entry| entry.current != entry.defaults)
      .map(|entry| {
        let texts = entry.current.iter().map(Binding::to_config).collect();
        (entry.name.clone(), texts)
      })
      .collect()
  }

  /// Whether bindings changed since the last call
  pub(crate) fn take_dirty(&mut self) -> bool {
    std::mem::take(&mut self.dirty)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use winit::keyboard::KeyCode;

  #[test]
  fn bindings_print_and_parse_back() {
    for binding in [
      Binding::Key(KeyCode::Space),
      Binding::Mouse(MouseButton::Right),
      Binding::Pad(GamepadButton::DPadUp),
      Binding::Stick(StickDirection {
        stick: Stick::Right,
        direction: Direction::Left,
      }),
    ] {
      assert_eq!(Binding::parse(&binding.to_config()), Some(binding));
    }
    assert_eq!(Binding::Key(KeyCode::Space).to_config(), "Key:Space");
    assert_eq!(
      Binding::parse("Stick:LeftUp"),
      Some(Binding::Stick(StickDirection {
        stick: Stick::Left,
        direction: Direction::Up
      }))
    );
    for bad in [
      "",
      "Key:",
      "Key:Nope",
      "Pad:Nope",
      "Joystick:Up",
      "Stick:Middle",
    ] {
      assert_eq!(Binding::parse(bad), None, "{bad}");
    }
  }

  #[test]
  fn rebinding_to_a_key_another_action_has_swaps_them() {
    let mut actions = Actions::default();
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
    actions.bind("dash", [Binding::Key(KeyCode::ShiftLeft)]);
    let swapped = actions.rebind("jump", 0, Binding::Key(KeyCode::ShiftLeft));
    assert_eq!(swapped.as_deref(), Some("dash"));
    assert_eq!(
      actions.bindings("jump"),
      &[Binding::Key(KeyCode::ShiftLeft)]
    );
    assert_eq!(actions.bindings("dash"), &[Binding::Key(KeyCode::Space)]);
    assert!(actions.take_dirty());
    assert!(!actions.take_dirty());
  }

  #[test]
  fn groups_keep_ui_and_game_bindings_apart() {
    let mut actions = Actions::with_ui_defaults();
    actions.bind("jump", [Binding::Key(KeyCode::KeyZ)]);
    // Space is ui.confirm's, but the UI and the game are separate groups
    assert_eq!(
      actions.rebind("jump", 0, Binding::Key(KeyCode::Space)),
      None
    );
    assert!(
      actions
        .bindings("ui.confirm")
        .contains(&Binding::Key(KeyCode::Space))
    );
    assert_eq!(Actions::group("ui.confirm"), "ui");
    assert_eq!(Actions::group("jump"), "game");
  }

  #[test]
  fn saved_bindings_override_what_the_game_binds() {
    let mut actions = Actions::default();
    let mut saved = BTreeMap::new();
    saved.insert(
      "jump".to_string(),
      vec!["Key:KeyJ".to_string(), "Nope".to_string()],
    );
    actions.load_saved(&saved);
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
    assert_eq!(
      actions.bindings("jump"),
      &[Binding::Key(KeyCode::KeyJ)],
      "unparsable entries are skipped"
    );
    assert_eq!(
      actions.saved_map().get("jump"),
      Some(&vec!["Key:KeyJ".to_string()])
    );
    actions.reset_defaults();
    assert_eq!(actions.bindings("jump"), &[Binding::Key(KeyCode::Space)]);
    assert!(actions.saved_map().is_empty(), "defaults are not saved");
  }

  #[test]
  fn rebinding_past_the_end_adds_a_binding() {
    let mut actions = Actions::default();
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
    actions.rebind("jump", 5, Binding::Pad(GamepadButton::South));
    assert_eq!(
      actions.bindings("jump"),
      &[
        Binding::Key(KeyCode::Space),
        Binding::Pad(GamepadButton::South)
      ]
    );
  }

  #[test]
  fn every_ui_default_survives_a_save_round_trip() {
    let actions = Actions::with_ui_defaults();
    for name in actions.names() {
      for binding in actions.bindings(name) {
        assert_eq!(
          Binding::parse(&binding.to_config()),
          Some(*binding),
          "{name}"
        );
      }
    }
  }
}
