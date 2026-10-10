//! Where release builds (`ryo2d build`) keep save data, and which folder they run from.

use std::path::{Component, Path};

/// Whether `name` can name a per-game folder under the OS user data directory.
///
/// Valid names are non-empty relative paths of normal components (e.g.
/// `"MyGame"` or `"MyStudio/MyGame"`), without characters Windows rejects and
/// without components ending in `.` or a space.
pub(crate) fn valid_save_folder(name: &str) -> bool {
  const FORBIDDEN: &[char] = &['<', '>', ':', '"', '|', '?', '*', '\\'];
  if name.trim().is_empty()
    || name
      .chars()
      .any(|c| c.is_control() || FORBIDDEN.contains(&c))
  {
    return false;
  }
  Path::new(name)
    .components()
    .all(|component| match component {
      Component::Normal(part) => part
        .to_str()
        .is_some_and(|part| !part.ends_with('.') && !part.ends_with(' ')),
      _ => false,
    })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn valid_save_folder_accepts_and_rejects() {
    for name in [
      "MyGame",
      "Star Hopper",
      "MyStudio/MyGame",
      "ゲーム",
      "game-2_final",
    ] {
      assert!(valid_save_folder(name), "{name:?} should be accepted");
    }
    for name in [
      "",
      "   ",
      "/abs",
      "../up",
      "a/../b",
      ".",
      "C:\\Games",
      "a:b",
      "bad|name",
      "trailing.",
      "trailing ",
      "tab\tname",
    ] {
      assert!(!valid_save_folder(name), "{name:?} should be rejected");
    }
  }
}
