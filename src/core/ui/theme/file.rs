//! Theme files: RON in and out, and the debug-build watch that reloads a
//! theme when its file changes

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ron::extensions::Extensions;

use super::{Theme, UiTheme};

/// Why a theme file could not be read or written
#[derive(Debug, Clone, PartialEq)]
pub enum ThemeError {
  /// The file could not be read or written
  Io { path: PathBuf, error: String },
  /// The file is not a valid theme
  Parse { path: PathBuf, error: String },
  /// The theme could not be turned into text
  Serialize { error: String },
}

impl std::fmt::Display for ThemeError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ThemeError::Io { path, error } => write!(f, "IO error for {}: {error}", path.display()),
      ThemeError::Parse { path, error } => {
        write!(f, "Parse error in {}: {error}", path.display())
      }
      ThemeError::Serialize { error } => write!(f, "Serialization error: {error}"),
    }
  }
}

impl std::error::Error for ThemeError {}

impl Theme {
  /// Load a theme from the RON file `path`. The file states only what it
  /// changes from the dark theme, and a field that is an option can be
  /// written without `Some(..)`. The theme remembers `path` as its source.
  ///
  /// # Errors
  /// The file cannot be read, or is not a valid theme.
  pub fn load(path: impl AsRef<Path>) -> Result<Theme, ThemeError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|e| ThemeError::Io {
      path: path.to_path_buf(),
      error: e.to_string(),
    })?;
    let mut theme: Theme = ron::Options::default()
      .with_default_extension(Extensions::IMPLICIT_SOME)
      .from_str(&text)
      .map_err(|e| ThemeError::Parse {
        path: path.to_path_buf(),
        error: e.to_string(),
      })?;
    theme.source = Some(path.to_path_buf());
    Ok(theme)
  }

  /// Save the theme to the RON file `path`.
  ///
  /// # Errors
  /// The theme cannot be written as RON, or the file cannot be written.
  pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ThemeError> {
    let path = path.as_ref();
    let text =
      ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(|e| {
        ThemeError::Serialize {
          error: e.to_string(),
        }
      })?;
    std::fs::write(path, text).map_err(|e| ThemeError::Io {
      path: path.to_path_buf(),
      error: e.to_string(),
    })
  }
}

/// The seconds between looks at a watched file
const POLL_INTERVAL: f64 = 1.0;

/// Debug-build hot reload of the theme's source file: it looks at the file's
/// modification time about once a second and reloads the theme when it moved
#[derive(Default)]
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub(crate) struct ThemeWatch {
  /// When the file was last looked at (seconds of real time)
  checked: Option<f64>,
  /// The file being watched and its modification time when last seen
  seen: Option<(PathBuf, Option<SystemTime>)>,
  /// The modification time a failed reload was last warned about
  warned: Option<Option<SystemTime>>,
}

#[cfg_attr(not(debug_assertions), allow(dead_code))]
impl ThemeWatch {
  /// Look at `theme`'s source file at `now` (seconds of real time), at most
  /// once a second. The first look only remembers the file's time. A changed
  /// file is loaded and `set`; one that does not load warns once and leaves
  /// the old theme.
  pub(crate) fn poll(&mut self, theme: &mut UiTheme, now: f64) {
    if self.checked.is_some_and(|last| now - last < POLL_INTERVAL) {
      return;
    }
    self.checked = Some(now);
    let Some(path) = theme.get().source().map(Path::to_path_buf) else {
      self.seen = None;
      return;
    };
    let modified = std::fs::metadata(&path)
      .and_then(|meta| meta.modified())
      .ok();
    match &mut self.seen {
      Some((watched, seen)) if *watched == path => {
        if *seen == modified {
          return;
        }
        *seen = modified;
      }
      // A new file, from a theme set in code: remember it, reload nothing
      seen => {
        *seen = Some((path, modified));
        return;
      }
    }
    match Theme::load(&path) {
      Ok(loaded) => theme.set(loaded),
      Err(error) => {
        if self.warned != Some(modified) {
          crate::engine_warn!(
            module: "ui",
            "could not reload the UI theme {}: {error}; keeping the old one",
            path.display()
          );
        }
        self.warned = Some(modified);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::Color;

  #[test]
  fn a_theme_round_trips_through_ron() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("theme.ron");
    let theme = Theme::light();
    theme.save(&path).unwrap();
    let loaded = Theme::load(&path).unwrap();
    assert_eq!(loaded.palette, theme.palette);
    assert_eq!(loaded.widgets, theme.widgets);
    assert_eq!(loaded.source(), Some(path.as_path()));
  }

  #[test]
  fn a_partial_file_fills_the_rest_from_dark() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("theme.ron");
    std::fs::write(
      &path,
      "(name: \"mine\", palette: (accent: (r: 1, g: 2, b: 3, a: 255)))",
    )
    .unwrap();
    let theme = Theme::load(&path).unwrap();
    assert_eq!(theme.name, "mine");
    assert_eq!(theme.palette.accent, Color::new(1, 2, 3, 255));
    assert_eq!(theme.widgets, Theme::dark().widgets);
  }

  #[test]
  fn a_bad_file_is_an_error_naming_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.ron");
    std::fs::write(&path, "(palette: (accent: nonsense").unwrap();
    let error = Theme::load(&path).unwrap_err();
    assert!(error.to_string().contains("broken.ron"), "{error}");
  }

  /// Write `text` and move the file's time forward, so the change shows on any filesystem
  fn edit(path: &std::path::Path, text: &str, seconds: u64) {
    std::fs::write(path, text).unwrap();
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file
      .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(seconds))
      .unwrap();
  }

  #[test]
  fn a_changed_file_reloads_at_most_once_a_second() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("theme.ron");
    Theme::dark().save(&path).unwrap();
    let mut theme = UiTheme::new(Theme::load(&path).unwrap());
    let mut watch = ThemeWatch::default();
    watch.poll(&mut theme, 0.0);
    edit(&path, "(name: \"edited\")", 5);
    watch.poll(&mut theme, 0.5);
    assert_eq!(theme.get().name, "dark", "checked at most once a second");
    watch.poll(&mut theme, 1.5);
    assert_eq!(theme.get().name, "edited");
    assert_eq!(
      theme.get().source(),
      Some(path.as_path()),
      "the reloaded theme still watches its file"
    );
  }

  #[test]
  fn a_broken_edit_keeps_the_old_theme_and_warns_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("watched-broken.ron");
    Theme::light().save(&path).unwrap();
    let mut theme = UiTheme::new(Theme::load(&path).unwrap());
    let mut watch = ThemeWatch::default();
    watch.poll(&mut theme, 0.0);
    edit(&path, "(palette: nonsense", 5);
    for second in 1..5 {
      watch.poll(&mut theme, f64::from(second) + 0.5);
    }
    assert_eq!(theme.get().name, "light");
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("watched-broken.ron"))
      .count();
    assert_eq!(warnings, 1, "one warning for one bad change");
  }
}
