//! Theme files: RON in (read by the registry's `ThemeLoader`) and out

use std::path::{Path, PathBuf};

use ron::extensions::Extensions;

use super::Theme;

/// Why a theme could not be written
#[derive(Debug, Clone, PartialEq)]
pub enum ThemeError {
  /// The file could not be written
  Io { path: PathBuf, error: String },
  /// The theme could not be turned into text
  Serialize { error: String },
}

impl std::fmt::Display for ThemeError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ThemeError::Io { path, error } => write!(f, "IO error for {}: {error}", path.display()),
      ThemeError::Serialize { error } => write!(f, "Serialization error: {error}"),
    }
  }
}

impl std::error::Error for ThemeError {}

impl Theme {
  /// Read a theme file's RON `text`. It is merged over the dark theme map by
  /// map (a struct, or the variants and skins), so it states only what it
  /// changes; a list or an option replaces whole, and an option can be
  /// written without `Some(..)`. A variant's `base`, a text role's `font`
  /// and a skin's `filter` are strings with the variant's name (`"Button"`,
  /// `"Pixel"`, `"Nearest"`); an unknown name is an error naming the type
  /// and the name. Skins are not given their images here
  pub(crate) fn parse(text: &str) -> Result<Theme, String> {
    // The file is read as plain data and merged over the dark theme's, map
    // by map, so it states only what it changes
    let changes: serde_json::Value = ron::Options::default()
      .with_default_extension(Extensions::IMPLICIT_SOME)
      .from_str(text)
      .map_err(|e| e.to_string())?;
    let mut merged = serde_json::to_value(Theme::dark()).map_err(|e| e.to_string())?;
    merge(&mut merged, changes);
    serde_json::from_value(merged).map_err(|e| e.to_string())
  }

  /// Save the theme to the RON file `path`, which the registry loads back
  /// when it is named `.theme.ron`.
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

/// Lay `changes` over `base`: maps merge key by key, anything else replaces
fn merge(base: &mut serde_json::Value, changes: serde_json::Value) {
  match (base, changes) {
    (serde_json::Value::Object(base), serde_json::Value::Object(changes)) => {
      for (key, change) in changes {
        match base.get_mut(&key) {
          Some(slot) => merge(slot, change),
          None => {
            base.insert(key, change);
          }
        }
      }
    }
    (slot, change) => *slot = change,
  }
}
