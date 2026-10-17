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
  Backquote,
  Backslash,
  BracketLeft,
  BracketRight,
  Comma,
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
  Equal,
  IntlBackslash,
  IntlRo,
  IntlYen,
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
  Minus,
  Period,
  Quote,
  Semicolon,
  Slash,
  AltLeft,
  AltRight,
  Backspace,
  CapsLock,
  ContextMenu,
  ControlLeft,
  ControlRight,
  Enter,
  SuperLeft,
  SuperRight,
  ShiftLeft,
  ShiftRight,
  Space,
  Tab,
  Convert,
  KanaMode,
  Lang1,
  Lang2,
  Lang3,
  Lang4,
  Lang5,
  NonConvert,
  Delete,
  End,
  Help,
  Home,
  Insert,
  PageDown,
  PageUp,
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  NumLock,
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
  NumpadBackspace,
  NumpadClear,
  NumpadClearEntry,
  NumpadComma,
  NumpadDecimal,
  NumpadDivide,
  NumpadEnter,
  NumpadEqual,
  NumpadHash,
  NumpadMemoryAdd,
  NumpadMemoryClear,
  NumpadMemoryRecall,
  NumpadMemoryStore,
  NumpadMemorySubtract,
  NumpadMultiply,
  NumpadParenLeft,
  NumpadParenRight,
  NumpadStar,
  NumpadSubtract,
  Escape,
  Fn,
  FnLock,
  PrintScreen,
  ScrollLock,
  Pause,
  BrowserBack,
  BrowserFavorites,
  BrowserForward,
  BrowserHome,
  BrowserRefresh,
  BrowserSearch,
  BrowserStop,
  Eject,
  LaunchApp1,
  LaunchApp2,
  LaunchMail,
  MediaPlayPause,
  MediaSelect,
  MediaStop,
  MediaTrackNext,
  MediaTrackPrevious,
  Power,
  Sleep,
  AudioVolumeDown,
  AudioVolumeMute,
  AudioVolumeUp,
  WakeUp,
  Meta,
  Hyper,
  Turbo,
  Abort,
  Resume,
  Suspend,
  Again,
  Copy,
  Cut,
  Find,
  Open,
  Paste,
  Props,
  Select,
  Undo,
  Hiragana,
  Katakana,
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
  F13,
  F14,
  F15,
  F16,
  F17,
  F18,
  F19,
  F20,
  F21,
  F22,
  F23,
  F24,
  F25,
  F26,
  F27,
  F28,
  F29,
  F30,
  F31,
  F32,
  F33,
  F34,
  F35,
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

  /// Whether the binding survives being saved and loaded again
  pub fn is_saveable(&self) -> bool {
    Binding::parse(&self.to_config()) == Some(*self)
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
  /// The player's overrides: every action whose bindings differ from its
  /// defaults, including saved ones for actions the game has not bound yet
  saved: BTreeMap<String, Vec<Binding>>,
  dirty: bool,
}

impl Actions {
  /// Bind `name` to `bindings` as its defaults, replacing earlier defaults.
  /// Bindings the player changed this session stay, and a saved override (the
  /// player's settings) takes precedence over the new defaults
  pub fn bind(&mut self, name: &str, bindings: impl IntoIterator<Item = Binding>) -> &mut Self {
    let defaults: Vec<Binding> = bindings.into_iter().collect();
    let index = match self.entries.iter().position(|entry| entry.name == name) {
      Some(index) => {
        let entry = &mut self.entries[index];
        // Bindings changed this session (rebound, set or loaded) stay
        if entry.current == entry.defaults {
          entry.current = self
            .saved
            .get(name)
            .cloned()
            .unwrap_or_else(|| defaults.clone());
        }
        entry.defaults = defaults;
        index
      }
      None => {
        let current = self
          .saved
          .get(name)
          .cloned()
          .unwrap_or_else(|| defaults.clone());
        self.entries.push(Entry {
          name: name.to_string(),
          defaults,
          current,
        });
        self.entries.len() - 1
      }
    };
    self.sync_saved(index);
    self
  }

  /// Keep the overrides in step with entry `index`: its bindings while they
  /// differ from its defaults, nothing once they are the defaults again
  fn sync_saved(&mut self, index: usize) {
    let Some(entry) = self.entries.get(index) else {
      return;
    };
    if entry.current == entry.defaults {
      self.saved.remove(&entry.name);
    } else {
      self.saved.insert(entry.name.clone(), entry.current.clone());
    }
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
    if let Some(index) = self.entries.iter().position(|entry| entry.name == name) {
      self.entries[index].current = bindings;
      self.sync_saved(index);
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
        swapped = Some((other, entry.name.clone()));
        break;
      }
    }
    let current = &mut self.entries[index].current;
    if slot < current.len() {
      current[slot] = binding;
    } else {
      current.push(binding);
    }
    self.sync_saved(index);
    if let Some((other, _)) = swapped {
      self.sync_saved(other);
    }
    self.dirty = true;
    swapped.map(|(_, name)| name)
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
  /// already bound switch to them at once; the rest when the game binds them.
  /// A text that does not parse is skipped, and an override none of whose
  /// texts parse is ignored, so the action keeps its defaults.
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
      if bindings.is_empty() && !texts.is_empty() {
        continue;
      }
      self.saved.insert(name.clone(), bindings.clone());
      if let Some(index) = self.entries.iter().position(|entry| entry.name == *name) {
        self.entries[index].current = bindings;
        self.sync_saved(index);
      }
    }
  }

  /// What to save, as binding texts: the overrides of actions not bound yet,
  /// kept as they were loaded, and every bound action whose bindings differ
  /// from its defaults
  pub(crate) fn saved_map(&self) -> BTreeMap<String, Vec<String>> {
    let texts = |bindings: &[Binding]| bindings.iter().map(Binding::to_config).collect();
    let unbound = self
      .saved
      .iter()
      .filter(|(name, _)| !self.entries.iter().any(|entry| entry.name == **name))
      .map(|(name, bindings)| (name.clone(), texts(bindings)));
    let changed = self
      .entries
      .iter()
      .filter(|entry| entry.current != entry.defaults)
      .map(|entry| (entry.name.clone(), texts(&entry.current)));
    unbound.chain(changed).collect()
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

  #[test]
  fn binding_again_keeps_a_rebind() {
    let mut actions = Actions::default();
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
    actions.rebind("jump", 0, Binding::Key(KeyCode::KeyJ));
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
    assert_eq!(actions.bindings("jump"), &[Binding::Key(KeyCode::KeyJ)]);
    assert_eq!(
      actions.saved_map().get("jump"),
      Some(&vec!["Key:KeyJ".to_string()])
    );
  }

  #[test]
  fn binding_new_defaults_applies_when_the_player_never_rebound() {
    let mut actions = Actions::default();
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
    actions.bind("jump", [Binding::Key(KeyCode::KeyZ)]);
    assert_eq!(actions.bindings("jump"), &[Binding::Key(KeyCode::KeyZ)]);
  }

  #[test]
  fn every_named_key_round_trips() {
    for &(_, key) in KEY_NAMES {
      let binding = Binding::Key(key);
      assert_eq!(Binding::parse(&binding.to_config()), Some(binding));
    }
  }

  #[test]
  fn previously_unnamed_keys_round_trip() {
    for key in [
      KeyCode::CapsLock,
      KeyCode::IntlBackslash,
      KeyCode::F13,
      KeyCode::F24,
      KeyCode::ContextMenu,
      KeyCode::NumpadEnter,
      KeyCode::MediaPlayPause,
    ] {
      let binding = Binding::Key(key);
      assert!(binding.is_saveable(), "{key:?}");
      assert_eq!(Binding::parse(&binding.to_config()), Some(binding));
    }
  }

  #[test]
  fn a_save_keeps_the_saved_bindings_of_actions_not_bound_yet() {
    // Last session the player rebound "jump"; this session the game has
    // not bound it yet (its level binds it) when the player rebinds a UI
    // action from the title screen, which saves
    let mut saved = BTreeMap::new();
    saved.insert("jump".to_string(), vec!["Key:KeyJ".to_string()]);
    let mut actions = Actions::with_ui_defaults();
    actions.load_saved(&saved);
    actions.rebind("ui.confirm", 0, Binding::Key(KeyCode::KeyK));
    let written = actions.saved_map();
    assert_eq!(
      written.get("jump"),
      Some(&vec!["Key:KeyJ".to_string()]),
      "the player's jump binding survives the save: {written:?}"
    );
    assert!(written.contains_key("ui.confirm"), "{written:?}");
  }

  #[test]
  fn rebinding_back_to_the_defaults_drops_the_saved_override() {
    let mut saved = BTreeMap::new();
    saved.insert("jump".to_string(), vec!["Key:KeyJ".to_string()]);
    let mut actions = Actions::default();
    actions.load_saved(&saved);
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
    assert_eq!(actions.bindings("jump"), &[Binding::Key(KeyCode::KeyJ)]);
    // The player puts Space back, then the game binds jump again (a level
    // entered once more)
    actions.rebind("jump", 0, Binding::Key(KeyCode::Space));
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
    assert_eq!(
      actions.bindings("jump"),
      &[Binding::Key(KeyCode::Space)],
      "the startup override is not applied again"
    );
    assert!(
      !actions.saved_map().contains_key("jump"),
      "defaults are not saved"
    );
  }

  #[test]
  fn an_override_that_does_not_parse_keeps_the_defaults() {
    let mut saved = BTreeMap::new();
    saved.insert("jump".to_string(), vec!["Nope".to_string()]);
    saved.insert("fire".to_string(), vec!["Key:Nope".to_string()]);
    let mut actions = Actions::default();
    actions.bind("fire", [Binding::Key(KeyCode::KeyF)]);
    actions.load_saved(&saved);
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
    assert_eq!(
      actions.bindings("jump"),
      &[Binding::Key(KeyCode::Space)],
      "bound after the load"
    );
    assert_eq!(
      actions.bindings("fire"),
      &[Binding::Key(KeyCode::KeyF)],
      "bound before it"
    );
    assert!(actions.saved_map().is_empty());
  }

  #[test]
  fn extra_mouse_buttons_are_saveable() {
    // winit's KeyCode has no Unidentified variant (that lives on PhysicalKey), so
    // every key is nameable; other buttons keep their number
    let binding = Binding::Mouse(MouseButton::Other(7));
    assert!(binding.is_saveable());
    assert_eq!(binding.to_config(), "Mouse:Other7");
  }
}
