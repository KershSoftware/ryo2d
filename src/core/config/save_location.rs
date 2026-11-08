//! Where release builds (`ryo2d build`) keep save data, and which folder they run from.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use super::project::compiled_embedded_value;

/// Whether `name` can name a per-game folder under the OS user data directory.
///
/// Valid names are non-empty relative paths of normal components (e.g.
/// `"MyGame"` or `"MyStudio/MyGame"`), without characters Windows rejects and
/// without components ending in `.` or a space or naming a reserved Windows
/// device (`CON`, `NUL`, `COM1`, ..., with or without an extension).
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
      Component::Normal(part) => part.to_str().is_some_and(|part| {
        !part.ends_with('.') && !part.ends_with(' ') && !is_reserved_name(part)
      }),
      _ => false,
    })
}

/// Whether `part` is a Windows device name, which can't be created as a file or folder.
fn is_reserved_name(part: &str) -> bool {
  let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
  if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
    return true;
  }
  let numbered = stem
    .strip_prefix("COM")
    .or_else(|| stem.strip_prefix("LPT"));
  numbered.is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9']))
}

/// This executable's path with symlinks resolved, as macOS can report the
/// path of the link a game was launched through.
fn game_exe() -> Option<PathBuf> {
  let exe = std::env::current_exe().ok()?;
  Some(std::fs::canonicalize(&exe).unwrap_or(exe))
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
      let game_dir = game_exe()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .or_else(|| {
          let cwd = std::env::current_dir().ok()?;
          eprintln!(
            "[ryo2d] Warning: Couldn't find the game's folder, using {}",
            cwd.display()
          );
          Some(cwd)
        })?;
      #[cfg(not(target_arch = "wasm32"))]
      let data_dir = dirs::data_dir();
      #[cfg(target_arch = "wasm32")]
      let data_dir = None;
      release_save_dir(compiled_embedded_value, &game_dir, data_dir)
    })
    .clone()
}

/// Folders `ryo2d build` puts beside the game binary.
const BUNDLE_DIRS: [&str; 3] = ["assets", "levels", "data"];

/// The folder a release build should run from: its executable's folder, when
/// that holds the bundle's `assets/`, `levels/` or `data/` and isn't already
/// the working directory. `None` for dev builds. Cargo target folders (with a
/// `deps/` folder or `.cargo-lock`) are never bundles, even once a portable
/// game has created `data/` in them.
pub(crate) fn bundle_dir_to_enter(is_release: bool, exe_dir: &Path, cwd: &Path) -> Option<PathBuf> {
  if !is_release || exe_dir == cwd {
    return None;
  }
  if exe_dir.join("deps").is_dir() || exe_dir.join(".cargo-lock").exists() {
    return None;
  }
  BUNDLE_DIRS
    .iter()
    .any(|dir| exe_dir.join(dir).is_dir())
    .then(|| exe_dir.to_path_buf())
}

/// Run this release build from its bundle folder, so relative paths (assets,
/// levels) resolve wherever the game was launched from.
pub(crate) fn enter_bundle_dir() {
  let is_release = compiled_embedded_value("RYO_EMBED_RESOLUTION").is_some();
  let (Some(exe), Ok(cwd)) = (game_exe(), std::env::current_dir()) else {
    return;
  };
  let Some(exe_dir) = exe.parent() else {
    return;
  };
  if let Some(dir) = bundle_dir_to_enter(is_release, exe_dir, &cwd)
    && let Err(e) = std::env::set_current_dir(&dir)
  {
    eprintln!(
      "[ryo2d] Warning: Couldn't run from the game folder {} ({e})",
      dir.display()
    );
  }
}

/// Make a project name one folder: path separators, control characters and
/// `<>:"|?*` become `-`, then surrounding whitespace and trailing dots go.
pub(crate) fn folder_from_project_name(name: &str) -> String {
  let replaced: String = name
    .chars()
    .map(|c| {
      if c.is_control() || "/\\<>:\"|?*".contains(c) {
        '-'
      } else {
        c
      }
    })
    .collect();
  replaced.trim().trim_end_matches(['.', ' ']).to_string()
}

/// The name the browser keeps a game's saves under: `[release] save_folder`
/// when it can name a folder, else `[project] name` as `ryo2d build` turns it
/// into a folder name, else the window title (warned once: games on one site
/// then share saves by title).
pub(crate) fn store_name(
  release_save_folder: Option<&str>,
  project_name: Option<&str>,
  title: &str,
) -> String {
  if let Some(folder) = release_save_folder.filter(|folder| valid_save_folder(folder)) {
    return folder.to_string();
  }
  if let Some(folder) = project_name
    .map(folder_from_project_name)
    .filter(|folder| valid_save_folder(folder))
  {
    return folder;
  }
  static WARNED: std::sync::Once = std::sync::Once::new();
  WARNED.call_once(|| {
    crate::engine_warn!(
      "web saves are named after the window title; set [project] name so other games on the same site don't share them"
    );
  });
  title.to_string()
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;

  fn folder_with(dirs: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in dirs {
      std::fs::create_dir(dir.path().join(name)).unwrap();
    }
    dir
  }

  #[test]
  fn dev_builds_stay_in_their_working_directory() {
    let bundle = folder_with(&["assets"]);
    assert_eq!(
      bundle_dir_to_enter(false, bundle.path(), Path::new("/home/me")),
      None
    );
  }

  #[test]
  fn release_builds_enter_their_bundle_folder() {
    for marker in ["assets", "levels", "data"] {
      let bundle = folder_with(&[marker]);
      assert_eq!(
        bundle_dir_to_enter(true, bundle.path(), Path::new("/home/me")),
        Some(bundle.path().to_path_buf()),
        "{marker}/ marks a bundle"
      );
    }
  }

  #[test]
  fn release_builds_already_in_their_folder_stay() {
    let bundle = folder_with(&["assets"]);
    assert_eq!(
      bundle_dir_to_enter(true, bundle.path(), bundle.path()),
      None
    );
  }

  #[test]
  fn cargo_target_folders_with_data_are_not_bundles() {
    let target = folder_with(&["deps", "build", "data"]);
    assert_eq!(
      bundle_dir_to_enter(true, target.path(), Path::new("/projects/hopper")),
      None
    );
  }

  #[test]
  fn target_release_folders_are_not_bundles() {
    let target = folder_with(&["deps", "build", ".fingerprint"]);
    assert_eq!(
      bundle_dir_to_enter(true, target.path(), Path::new("/projects/hopper")),
      None
    );
  }

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
  fn portable_without_a_save_folder_saves_beside_the_game() {
    let lookup = embedded(&[
      ("RYO_EMBED_RESOLUTION", "1280x720"),
      ("RYO_EMBED_PORTABLE", "true"),
    ]);
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
  fn project_names_become_a_single_folder() {
    assert_eq!(
      folder_from_project_name("Squash/Stretch Demo"),
      "Squash-Stretch Demo"
    );
    assert_eq!(folder_from_project_name("Game: Remix"), "Game- Remix");
    assert_eq!(folder_from_project_name("Breakout"), "Breakout");
    assert_eq!(folder_from_project_name("  Hopper. . "), "Hopper");
  }

  #[test]
  fn the_store_name_prefers_save_folder_then_name_then_title() {
    assert_eq!(
      store_name(Some("Studio/Hopper"), Some("Hopper"), "Title"),
      "Studio/Hopper"
    );
    // An invalid save folder falls through to the name
    assert_eq!(
      store_name(Some("../x"), Some("Star: Hopper"), "T"),
      "Star- Hopper"
    );
    assert_eq!(
      store_name(None, Some("Star Hopper"), "Title"),
      "Star Hopper"
    );
    assert_eq!(store_name(None, Some("  "), "Title"), "Title");
    assert_eq!(store_name(None, None, "Other"), "Other");
    // Warned once a process, however many games fall back
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|entry| {
        entry
          .message
          .contains("web saves are named after the window title")
      })
      .count();
    assert_eq!(warned, 1);
  }

  #[test]
  fn valid_save_folder_accepts_and_rejects() {
    for name in [
      "MyGame",
      "Star Hopper",
      "MyStudio/MyGame",
      "ゲーム",
      "game-2_final",
      "Console",
      "Comet",
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
      "CON",
      "nul",
      "Studio/aux",
      "com1.dat",
    ] {
      assert!(!valid_save_folder(name), "{name:?} should be rejected");
    }
  }
}
