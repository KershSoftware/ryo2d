//! Where release builds (`ryo2d build`) keep save data, and which folder they run from.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use super::resolve::compiled_embedded_value;

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

/// The save directory of a release build, or `None` for a dev build.
///
/// `lookup` reads embedded values. Portable builds keep saves in `data/` beside
/// the executable; so do builds whose OS data directory or embedded save
/// folder is unusable, with a warning.
pub(crate) fn release_save_dir<'a>(
  lookup: impl Fn(&str) -> Option<&'a str>,
  exe_dir: &Path,
  user_data_dir: Option<PathBuf>,
) -> Option<PathBuf> {
  lookup("RYO_EMBED_RESOLUTION")?;
  let beside_game = exe_dir.join("data");
  if lookup("RYO_EMBED_PORTABLE") == Some("true") {
    return Some(beside_game);
  }
  let Some(folder) = lookup("RYO_EMBED_SAVE_FOLDER").filter(|folder| valid_save_folder(folder))
  else {
    eprintln!(
      "[ryo2d] Warning: No valid save folder was embedded, saving to {}",
      beside_game.display()
    );
    return Some(beside_game);
  };
  match user_data_dir {
    Some(base) => Some(base.join(folder)),
    None => {
      eprintln!(
        "[ryo2d] Warning: No user data folder on this system, saving to {}",
        beside_game.display()
      );
      Some(beside_game)
    }
  }
}

/// [`release_save_dir`] for this binary, computed once per process.
pub(crate) fn compiled_release_save_dir() -> Option<PathBuf> {
  static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
  DIR
    .get_or_init(|| {
      compiled_embedded_value("RYO_EMBED_RESOLUTION")?;
      let exe = std::env::current_exe().ok()?;
      release_save_dir(compiled_embedded_value, exe.parent()?, dirs::data_dir())
    })
    .clone()
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;

  fn embedded(
    values: &'static [(&'static str, &'static str)],
  ) -> impl Fn(&str) -> Option<&'static str> {
    move |name| {
      values
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
    }
  }

  fn game_dir() -> PathBuf {
    PathBuf::from("/games/hopper")
  }

  fn os_data() -> Option<PathBuf> {
    Some(PathBuf::from("/users/me/data"))
  }

  #[test]
  fn dev_builds_have_no_release_save_dir() {
    assert_eq!(release_save_dir(|_| None, &game_dir(), os_data()), None);
  }

  #[test]
  fn release_builds_save_in_the_os_user_folder() {
    let lookup = embedded(&[
      ("RYO_EMBED_RESOLUTION", "1280x720"),
      ("RYO_EMBED_PORTABLE", "false"),
      ("RYO_EMBED_SAVE_FOLDER", "Studio/Hopper"),
    ]);
    assert_eq!(
      release_save_dir(lookup, &game_dir(), os_data()),
      Some(PathBuf::from("/users/me/data").join("Studio/Hopper"))
    );
  }

  #[test]
  fn portable_release_builds_save_beside_the_game() {
    let lookup = embedded(&[
      ("RYO_EMBED_RESOLUTION", "1280x720"),
      ("RYO_EMBED_PORTABLE", "true"),
      ("RYO_EMBED_SAVE_FOLDER", "Hopper"),
    ]);
    assert_eq!(
      release_save_dir(lookup, &game_dir(), os_data()),
      Some(game_dir().join("data"))
    );
  }

  #[test]
  fn no_os_data_dir_falls_back_beside_the_game() {
    let lookup = embedded(&[
      ("RYO_EMBED_RESOLUTION", "1280x720"),
      ("RYO_EMBED_SAVE_FOLDER", "Hopper"),
    ]);
    assert_eq!(
      release_save_dir(lookup, &game_dir(), None),
      Some(game_dir().join("data"))
    );
  }

  #[test]
  fn missing_save_folder_falls_back_beside_the_game() {
    let lookup = embedded(&[("RYO_EMBED_RESOLUTION", "1280x720")]);
    assert_eq!(
      release_save_dir(lookup, &game_dir(), os_data()),
      Some(game_dir().join("data"))
    );
  }

  #[test]
  fn invalid_save_folder_falls_back_beside_the_game() {
    let lookup = embedded(&[
      ("RYO_EMBED_RESOLUTION", "1280x720"),
      ("RYO_EMBED_SAVE_FOLDER", "../escape"),
    ]);
    assert_eq!(
      release_save_dir(lookup, &game_dir(), os_data()),
      Some(game_dir().join("data"))
    );
  }

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
