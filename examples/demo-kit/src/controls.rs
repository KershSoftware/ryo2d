//! Page controls shown in the legend and the help overlay

use ryo2d::prelude::KeyCode;

/// A page's own control, listed in the legend and the help overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
  pub(crate) label: String,
  pub(crate) keys: Vec<KeyCode>,
  pub(crate) description: String,
}

impl Control {
  /// A single key, labelled from its name (`Space`, `Q`, `1`)
  pub fn key(key: KeyCode, description: impl Into<String>) -> Self {
    Self {
      label: key_label(key),
      keys: vec![key],
      description: description.into(),
    }
  }

  /// Several keys under one label, e.g. `Control::keys("WASD", &[...], "Move")`
  pub fn keys(label: impl Into<String>, keys: &[KeyCode], description: impl Into<String>) -> Self {
    Self {
      label: label.into(),
      keys: keys.to_vec(),
      description: description.into(),
    }
  }

  /// A mouse control
  pub fn mouse(description: impl Into<String>) -> Self {
    Self {
      label: "Mouse".to_string(),
      keys: Vec::new(),
      description: description.into(),
    }
  }
}

/// Keys the shell or the engine owns; pages may not list them
pub(crate) const RESERVED: &[KeyCode] = &[
  KeyCode::BracketLeft,
  KeyCode::BracketRight,
  KeyCode::PageUp,
  KeyCode::PageDown,
  KeyCode::ArrowUp,
  KeyCode::ArrowDown,
  KeyCode::ArrowLeft,
  KeyCode::ArrowRight,
  KeyCode::KeyH,
  KeyCode::KeyR,
  KeyCode::Escape,
  KeyCode::F1,
  KeyCode::F2,
  KeyCode::F3,
  KeyCode::F4,
  KeyCode::F5,
  KeyCode::F9,
  KeyCode::F10,
  KeyCode::F12,
  KeyCode::Backquote,
];

/// The first reserved key any control uses
pub(crate) fn reserved_key(controls: &[Control]) -> Option<KeyCode> {
  controls
    .iter()
    .flat_map(|control| control.keys.iter().copied())
    .find(|key| RESERVED.contains(key))
}

/// A short label for a key: `KeyQ` -> `Q`, `Digit3` -> `3` (the arrows are
/// the shell's, so pages never label them)
pub(crate) fn key_label(key: KeyCode) -> String {
  let name = format!("{key:?}");
  for prefix in ["Key", "Digit"] {
    if let Some(rest) = name.strip_prefix(prefix)
      && !rest.is_empty()
    {
      return rest.to_string();
    }
  }
  name
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn key_labels_are_short() {
    assert_eq!(key_label(KeyCode::KeyQ), "Q");
    assert_eq!(key_label(KeyCode::Digit3), "3");
    assert_eq!(key_label(KeyCode::Space), "Space");
    assert_eq!(key_label(KeyCode::Enter), "Enter");
  }

  #[test]
  fn reserved_keys_are_found_in_any_control() {
    let fine = [Control::key(KeyCode::Space, "Burst"), Control::mouse("Aim")];
    assert_eq!(reserved_key(&fine), None);
    let clash = [
      Control::key(KeyCode::Space, "Burst"),
      Control::keys("WASD + Up", &[KeyCode::KeyW, KeyCode::ArrowUp], "Move"),
    ];
    assert_eq!(reserved_key(&clash), Some(KeyCode::ArrowUp));
    assert_eq!(
      reserved_key(&[Control::key(KeyCode::KeyH, "Hop")]),
      Some(KeyCode::KeyH)
    );
  }
}
