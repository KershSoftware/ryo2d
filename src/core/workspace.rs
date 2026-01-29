//! Workspace management for Ferrum2D projects
//!
//! A workspace represents a game project directory containing:
//! - `ryo.toml` configuration file (optional)
//! - Entry point (main.rs)
//! - Assets, save data, and other resources
//!
//! # Configuration File
//!
//! The `ryo.toml` file configures project settings:
//!
//! ```toml
//! [project]
//! name = "My Game"
//!
//! [paths]
//! entry_point = "src/main.rs"  # Default: inferred
//! save_data = "data"           # Default: "data"
//! assets = "assets"            # Default: "assets"
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use ferrum2d::prelude::*;
//!
//! // Auto-discover workspace from current directory
//! let workspace = Workspace::discover()?;
//!
//! // Or load from explicit path
//! let workspace = Workspace::from_path("./my_game")?;
//!
//! // Access paths
//! println!("Save data: {:?}", workspace.save_data_dir());
//! ```

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use super::error::EngineError;

/// The configuration filename to look for
pub const CONFIG_FILENAME: &str = "ryo.toml";

/// Maximum directory levels to search upward for config
const MAX_SEARCH_DEPTH: usize = 3;

/// A Ferrum2D project workspace.
///
/// Represents the root directory of a game project and provides
/// access to configured paths and settings.
#[derive(Debug, Clone)]
pub struct Workspace {
  /// Root directory of the workspace
  root: PathBuf,
  /// Parsed configuration
  config: WorkspaceConfig,
}

/// Configuration loaded from `ryo.toml`.
#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
  /// Project name (defaults to directory name)
  pub name: String,
  /// Path to entry point relative to workspace root
  pub entry_point: PathBuf,
  /// Path to save data directory relative to workspace root
  pub save_data: PathBuf,
  /// Path to assets directory relative to workspace root
  pub assets: PathBuf,
}

/// Raw TOML structure for deserialization
#[derive(Debug, Deserialize, Default)]
struct RyoToml {
  project: Option<ProjectSection>,
  paths: Option<PathsSection>,
}

#[derive(Debug, Deserialize, Default)]
struct ProjectSection {
  name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PathsSection {
  entry_point: Option<String>,
  save_data: Option<String>,
  assets: Option<String>,
}

impl Workspace {
  /// Discover workspace by searching for `ryo.toml` from current directory.
  ///
  /// Searches the current directory and up to 3 parent directories.
  /// If no config file is found, uses the current directory as workspace root
  /// with default settings.
  ///
  /// # Errors
  ///
  /// Returns an error if the current directory cannot be determined.
  pub fn discover() -> Result<Self, EngineError> {
    let cwd =
      std::env::current_dir().map_err(|e| EngineError::WorkspaceError(e.to_string()))?;

    Self::discover_from(&cwd)
  }

  /// Discover workspace starting from a specific directory.
  ///
  /// Searches the given directory and up to 3 parent directories.
  pub fn discover_from(start: &Path) -> Result<Self, EngineError> {
    let mut current = start.to_path_buf();

    for _ in 0..MAX_SEARCH_DEPTH {
      let config_path = current.join(CONFIG_FILENAME);
      if config_path.exists() {
        return Self::from_path(&current);
      }

      match current.parent() {
        Some(parent) => current = parent.to_path_buf(),
        None => break,
      }
    }

    // No config found - use start directory with defaults
    Self::from_path_with_defaults(start)
  }

  /// Load workspace from an explicit path.
  ///
  /// If `ryo.toml` exists in the directory, it will be parsed.
  /// Otherwise, default settings are used.
  pub fn from_path(root: impl AsRef<Path>) -> Result<Self, EngineError> {
    let root = root.as_ref().to_path_buf();
    let config_path = root.join(CONFIG_FILENAME);

    if config_path.exists() {
      let contents = fs::read_to_string(&config_path).map_err(|e| {
        EngineError::WorkspaceError(format!("Failed to read {}: {}", CONFIG_FILENAME, e))
      })?;

      let raw: RyoToml = toml::from_str(&contents).map_err(|e| {
        EngineError::WorkspaceError(format!("Failed to parse {}: {}", CONFIG_FILENAME, e))
      })?;

      let config = Self::build_config(&root, raw)?;
      Ok(Workspace { root, config })
    } else {
      Self::from_path_with_defaults(&root)
    }
  }

  /// Create workspace with default settings (no config file).
  fn from_path_with_defaults(root: &Path) -> Result<Self, EngineError> {
    let raw = RyoToml::default();
    let config = Self::build_config(root, raw)?;
    Ok(Workspace {
      root: root.to_path_buf(),
      config,
    })
  }

  /// Build configuration from raw TOML, applying defaults.
  fn build_config(root: &Path, raw: RyoToml) -> Result<WorkspaceConfig, EngineError> {
    let project = raw.project.unwrap_or_default();
    let paths = raw.paths.unwrap_or_default();

    // Project name: from config, or directory name
    let name = project.name.unwrap_or_else(|| {
      root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unnamed")
        .to_string()
    });

    // Entry point: from config, or inferred
    let entry_point = match paths.entry_point {
      Some(ep) => PathBuf::from(ep),
      None => Self::infer_entry_point(root)?,
    };

    // Save data: from config, or default
    let save_data = PathBuf::from(paths.save_data.unwrap_or_else(|| "data".to_string()));

    // Assets: from config, or default
    let assets = PathBuf::from(paths.assets.unwrap_or_else(|| "assets".to_string()));

    Ok(WorkspaceConfig {
      name,
      entry_point,
      save_data,
      assets,
    })
  }

  /// Infer entry point by checking common locations.
  fn infer_entry_point(root: &Path) -> Result<PathBuf, EngineError> {
    // Check ./main.rs first (sibling to ryo.toml)
    let sibling = root.join("main.rs");
    if sibling.exists() {
      return Ok(PathBuf::from("main.rs"));
    }

    // Check ./src/main.rs
    let src_main = root.join("src").join("main.rs");
    if src_main.exists() {
      return Ok(PathBuf::from("src/main.rs"));
    }

    // No entry point found - return default (may not exist)
    // This is okay for library-only projects
    Ok(PathBuf::from("src/main.rs"))
  }

  /// Get the workspace root directory.
  pub fn root(&self) -> &Path {
    &self.root
  }

  /// Get the project name.
  pub fn name(&self) -> &str {
    &self.config.name
  }

  /// Get the configuration.
  pub fn config(&self) -> &WorkspaceConfig {
    &self.config
  }

  /// Get the absolute path to the entry point.
  pub fn entry_point(&self) -> PathBuf {
    self.root.join(&self.config.entry_point)
  }

  /// Get the absolute path to the save data directory.
  ///
  /// Creates the directory if it doesn't exist.
  pub fn save_data_dir(&self) -> Result<PathBuf, EngineError> {
    let path = self.root.join(&self.config.save_data);
    if !path.exists() {
      fs::create_dir_all(&path).map_err(|e| {
        EngineError::WorkspaceError(format!("Failed to create save data directory: {}", e))
      })?;
    }
    Ok(path)
  }

  /// Get the absolute path to the assets directory.
  pub fn assets_dir(&self) -> PathBuf {
    self.root.join(&self.config.assets)
  }

  /// Check if a `ryo.toml` config file exists in the workspace.
  pub fn has_config_file(&self) -> bool {
    self.root.join(CONFIG_FILENAME).exists()
  }
}

impl Default for WorkspaceConfig {
  fn default() -> Self {
    WorkspaceConfig {
      name: "unnamed".to_string(),
      entry_point: PathBuf::from("src/main.rs"),
      save_data: PathBuf::from("data"),
      assets: PathBuf::from("assets"),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::env;

  #[test]
  fn workspace_config_defaults() {
    let config = WorkspaceConfig::default();
    assert_eq!(config.name, "unnamed");
    assert_eq!(config.entry_point, PathBuf::from("src/main.rs"));
    assert_eq!(config.save_data, PathBuf::from("data"));
    assert_eq!(config.assets, PathBuf::from("assets"));
  }

  #[test]
  fn workspace_discover_uses_cwd_when_no_config() {
    // This test just verifies discover doesn't panic
    // In a real workspace it would find ryo.toml
    let result = Workspace::discover();
    assert!(result.is_ok());
  }

  #[test]
  fn workspace_from_path_with_defaults() {
    let cwd = env::current_dir().unwrap();
    let workspace = Workspace::from_path_with_defaults(&cwd).unwrap();
    assert_eq!(workspace.root(), cwd);
  }
}
