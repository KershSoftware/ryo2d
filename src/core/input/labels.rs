//! What prompts show for a binding: its label for the player's pad family,
//! and the `TextIcons` name a game can register a glyph under

use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use super::actions::key_name;
use super::{Binding, Direction, GamepadButton, Stick};

/// The face-button layout a pad's labels follow, from its name
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PadFamily {
  #[default]
  Xbox,
  PlayStation,
  Nintendo,
}

impl PadFamily {
  /// The family a pad named `gamepad_name` belongs to: PlayStation for
  /// "dualsense", "dualshock", "playstation", "sony", "ps4" or "ps5",
  /// Nintendo for "nintendo", "switch", "pro controller" or "joy-con"
  /// (any case), Xbox otherwise
  pub fn from_name(gamepad_name: &str) -> PadFamily {
    let name = gamepad_name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| name.contains(word));
    if has(&[
      "dualsense",
      "dualshock",
      "playstation",
      "sony",
      "ps4",
      "ps5",
    ]) {
      PadFamily::PlayStation
    } else if has(&["nintendo", "switch", "pro controller", "joy-con"]) {
      PadFamily::Nintendo
    } else {
      PadFamily::Xbox
    }
  }

  /// Its part of a pad glyph's name: `prompt.pad.<key>.South`
  fn glyph_key(self) -> &'static str {
    match self {
      PadFamily::Xbox => "xbox",
      PadFamily::PlayStation => "playstation",
      PadFamily::Nintendo => "nintendo",
    }
  }
}

/// An arrow for a direction (U+2190-2193)
fn arrow(direction: Direction) -> char {
  match direction {
    Direction::Up => '↑',
    Direction::Down => '↓',
    Direction::Left => '←',
    Direction::Right => '→',
  }
}

/// A key's label: letters and digits as themselves, a short name for the
/// common keys, arrows and punctuation as their character, otherwise the
/// `KeyCode` name
fn key_label(key: KeyCode) -> String {
  let short = match key {
    KeyCode::Enter => "Enter",
    KeyCode::Space => "Space",
    KeyCode::Escape => "Esc",
    KeyCode::Tab => "Tab",
    KeyCode::ShiftLeft | KeyCode::ShiftRight => "Shift",
    KeyCode::ControlLeft | KeyCode::ControlRight => "Ctrl",
    KeyCode::AltLeft | KeyCode::AltRight => "Alt",
    KeyCode::Backspace => "Bksp",
    KeyCode::ArrowUp => "↑",
    KeyCode::ArrowDown => "↓",
    KeyCode::ArrowLeft => "←",
    KeyCode::ArrowRight => "→",
    KeyCode::Backquote => "`",
    KeyCode::Backslash => "\\",
    KeyCode::BracketLeft => "[",
    KeyCode::BracketRight => "]",
    KeyCode::Comma => ",",
    KeyCode::Equal => "=",
    KeyCode::Minus => "-",
    KeyCode::Period => ".",
    KeyCode::Quote => "'",
    KeyCode::Semicolon => ";",
    KeyCode::Slash => "/",
    _ => "",
  };
  if !short.is_empty() {
    return short.to_string();
  }
  let name = key_name(key).map_or_else(|| format!("{key:?}"), str::to_string);
  // KeyA is "A", Digit1 "1"; F1-F12 are their names already
  name
    .strip_prefix("Key")
    .or_else(|| name.strip_prefix("Digit"))
    .filter(|rest| rest.len() == 1)
    .map_or_else(|| name.clone(), str::to_string)
}

/// A pad button's label in `family`'s layout
fn pad_label(button: GamepadButton, family: PadFamily) -> &'static str {
  use GamepadButton as B;
  use PadFamily::{Nintendo, PlayStation, Xbox};
  match (button, family) {
    (B::South, Xbox) | (B::East, Nintendo) => "A",
    (B::East, Xbox) | (B::South, Nintendo) => "B",
    (B::West, Xbox) | (B::North, Nintendo) => "X",
    (B::North, Xbox) | (B::West, Nintendo) => "Y",
    (B::South, PlayStation) => "×",
    (B::East, PlayStation) => "○",
    (B::West, PlayStation) => "□",
    (B::North, PlayStation) => "△",
    (B::LeftBumper, Xbox) => "LB",
    (B::RightBumper, Xbox) => "RB",
    (B::LeftBumper, PlayStation) => "L1",
    (B::RightBumper, PlayStation) => "R1",
    (B::LeftBumper, Nintendo) => "L",
    (B::RightBumper, Nintendo) => "R",
    (B::LeftTrigger, Xbox) => "LT",
    (B::RightTrigger, Xbox) => "RT",
    (B::LeftTrigger, PlayStation) => "L2",
    (B::RightTrigger, PlayStation) => "R2",
    (B::LeftTrigger, Nintendo) => "ZL",
    (B::RightTrigger, Nintendo) => "ZR",
    (B::Start, Xbox) => "Menu",
    (B::Select, Xbox) => "View",
    (B::Start, PlayStation) => "Options",
    (B::Select, PlayStation) => "Share",
    (B::Guide, PlayStation) => "PS",
    (B::Start, Nintendo) => "+",
    (B::Select, Nintendo) => "−",
    (B::Guide, Xbox | Nintendo) => "Home",
    (B::LeftStick, PlayStation) => "L3",
    (B::RightStick, PlayStation) => "R3",
    (B::LeftStick, Xbox | Nintendo) => "LS",
    (B::RightStick, Xbox | Nintendo) => "RS",
    (B::DPadUp, _) => "D↑",
    (B::DPadDown, _) => "D↓",
    (B::DPadLeft, _) => "D←",
    (B::DPadRight, _) => "D→",
  }
}

impl Binding {
  /// What a prompt shows for the binding: "J", "Enter", "↑", "LMB", or a
  /// pad button as `family` labels it ("A" on Xbox, "×" on PlayStation),
  /// "L↑" for a stick direction
  pub fn label(&self, family: PadFamily) -> String {
    match *self {
      Binding::Key(key) => key_label(key),
      Binding::Mouse(button) => match button {
        MouseButton::Left => "LMB".to_string(),
        MouseButton::Right => "RMB".to_string(),
        MouseButton::Middle => "MMB".to_string(),
        MouseButton::Back => "Back".to_string(),
        MouseButton::Forward => "Fwd".to_string(),
        MouseButton::Other(index) => format!("M{index}"),
      },
      Binding::Pad(button) => pad_label(button, family).to_string(),
      Binding::Stick(direction) => {
        let stick = match direction.stick {
          Stick::Left => 'L',
          Stick::Right => 'R',
        };
        format!("{stick}{}", arrow(direction.direction))
      }
    }
  }

  /// The `TextIcons` name a game registers a glyph for the binding under:
  /// `prompt.key.Space`, `prompt.mouse.Left`, `prompt.pad.xbox.South`,
  /// `prompt.stick.LeftUp`
  pub(crate) fn glyph_name(&self, family: PadFamily) -> String {
    // The names are the config text's: "Key:Space", "Pad:South"
    let config = self.to_config();
    let (kind, name) = config.split_once(':').unwrap_or(("", config.as_str()));
    match self {
      Binding::Pad(_) => format!("prompt.pad.{}.{name}", family.glyph_key()),
      _ => format!("prompt.{}.{name}", kind.to_ascii_lowercase()),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::input::{Binding, Direction, GamepadButton, Stick, StickDirection};
  use winit::keyboard::KeyCode;

  #[test]
  fn pad_families_come_from_the_name() {
    assert_eq!(
      PadFamily::from_name("Sony DualSense Wireless Controller"),
      PadFamily::PlayStation
    );
    assert_eq!(
      PadFamily::from_name("Nintendo Switch Pro Controller"),
      PadFamily::Nintendo
    );
    assert_eq!(
      PadFamily::from_name("Xbox Wireless Controller"),
      PadFamily::Xbox
    );
    assert_eq!(PadFamily::from_name("Virtual pad"), PadFamily::Xbox);
  }

  #[test]
  fn binding_labels_follow_the_pad_family() {
    let south = Binding::Pad(GamepadButton::South);
    assert_eq!(south.label(PadFamily::Xbox), "A");
    assert_eq!(south.label(PadFamily::PlayStation), "×");
    assert_eq!(south.label(PadFamily::Nintendo), "B");
    assert_eq!(Binding::Key(KeyCode::Enter).label(PadFamily::Xbox), "Enter");
    assert_eq!(Binding::Key(KeyCode::ArrowUp).label(PadFamily::Xbox), "↑");
    assert_eq!(Binding::Key(KeyCode::KeyJ).label(PadFamily::Xbox), "J");
    assert_eq!(
      Binding::Stick(StickDirection {
        stick: Stick::Left,
        direction: Direction::Up
      })
      .label(PadFamily::Xbox),
      "L↑"
    );
    assert_eq!(
      Binding::Pad(GamepadButton::RightBumper).glyph_name(PadFamily::PlayStation),
      "prompt.pad.playstation.RightBumper"
    );
  }

  #[test]
  fn other_labels_and_glyph_names() {
    use winit::event::MouseButton;
    let east = Binding::Pad(GamepadButton::East);
    assert_eq!(east.label(PadFamily::PlayStation), "○");
    assert_eq!(east.label(PadFamily::Nintendo), "A");
    let select = Binding::Pad(GamepadButton::Select);
    assert_eq!(select.label(PadFamily::Nintendo), "−");
    assert_eq!(
      Binding::Pad(GamepadButton::DPadLeft).label(PadFamily::Xbox),
      "D←"
    );
    assert_eq!(Binding::Key(KeyCode::Digit7).label(PadFamily::Xbox), "7");
    assert_eq!(Binding::Key(KeyCode::Escape).label(PadFamily::Xbox), "Esc");
    assert_eq!(
      Binding::Key(KeyCode::ShiftRight).label(PadFamily::Xbox),
      "Shift"
    );
    assert_eq!(Binding::Key(KeyCode::F12).label(PadFamily::Xbox), "F12");
    assert_eq!(Binding::Key(KeyCode::Slash).label(PadFamily::Xbox), "/");
    assert_eq!(
      Binding::Key(KeyCode::NumpadAdd).label(PadFamily::Xbox),
      "NumpadAdd"
    );
    assert_eq!(
      Binding::Mouse(MouseButton::Left).label(PadFamily::Xbox),
      "LMB"
    );
    assert_eq!(
      Binding::Mouse(MouseButton::Forward).label(PadFamily::Xbox),
      "Fwd"
    );
    let right_down = Binding::Stick(StickDirection {
      stick: Stick::Right,
      direction: Direction::Down,
    });
    assert_eq!(right_down.label(PadFamily::Xbox), "R↓");
    assert_eq!(
      right_down.glyph_name(PadFamily::Xbox),
      "prompt.stick.RightDown"
    );
    assert_eq!(
      Binding::Key(KeyCode::Space).glyph_name(PadFamily::Xbox),
      "prompt.key.Space"
    );
    assert_eq!(
      Binding::Mouse(MouseButton::Right).glyph_name(PadFamily::Nintendo),
      "prompt.mouse.Right"
    );
    assert_eq!(
      Binding::Pad(GamepadButton::South).glyph_name(PadFamily::Xbox),
      "prompt.pad.xbox.South"
    );
    assert_eq!(
      PadFamily::from_name("PS5 Controller"),
      PadFamily::PlayStation
    );
    assert_eq!(PadFamily::from_name("Joy-Con (L)"), PadFamily::Nintendo);
  }
}
