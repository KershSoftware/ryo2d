//! Workspace management for Ryo2D projects
//!
//! A workspace represents a game project directory containing:
//! - `Ryo.toml` configuration file (developer settings)
//! - `data/system.json` user configuration file (player settings)
//! - Entry point (main.rs)
//! - Assets, save data, and other resources
//!
//! # Configuration Hierarchy
//!
//! Ryo2D uses a two-file configuration system with fallback chain:
//!
//! | File | Purpose | Who Edits |
//! |------|---------|-----------|
//! | `Ryo.toml` | Developer defaults, debug flags | Game developer |
//! | `system.json` | User preferences (resolution, audio) | Player |
//!
//! **Priority chain:** engine defaults → `Ryo.toml` → `EngineBuilder` calls → saved `system.json`
//! preferences (see `EngineBuilder::with_user_preferences`).
//!
//! # Ryo.toml Structure
//!
//! ```toml
//! [project]
//! name = "My Game"
//! title = "My Game - The Adventure"  # Window title (optional, defaults to name)
//! description = "An amazing game"    # Optional description for tooling
//!
//! [paths]
//! entry_point = "src/main.rs"
//! save_data = "data"
//! assets = "assets"
//!
//! [system]  # The game's defaults for the player's settings
//! resolution = "1920x1080"
//! vsync = true
//! display_mode = "windowed"
//! # shadow_quality = "high"  # low, medium, high or ultra
//! master_volume = 1.0
//! music_volume = 0.89
//! sfx_volume = 1.0
//! ui_volume = 1.0
//! voice_volume = 1.0
//! ambience_volume = 1.0
//! # audio_buffer_frames = 256  # the audio device's buffer (default: the device's)
//! show_fps = false
//!
//! [tilemap]
//! # tiled_project = "tiled/game.tiled-project"  # class and enum types for Tiled maps (default: project.tiled-project at the asset root)
//!
//! [debug]  # Developer debug flags (NOT in system.json; a build without the tools ignores them)
//! console = true  # the developer console (default: on in a build with the tools)
//! show_colliders = false
//! show_velocities = false
//! show_origins = false
//!
//! [env]  # Game-specific variables
//! debug_mode = false
//! ```
//!
//! A key that a known section doesn't have warns, naming the nearest known
//! key; `[env]` and the game's own sections are free-form.
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Auto-discover workspace from current directory
//! let workspace = Workspace::discover()?;
//!
//! // Or load from explicit path
//! let workspace = Workspace::from_path("./my_game")?;
//!
//! // Read what Ryo.toml names
//! println!("Title: {}", workspace.config().title);
//! ```

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::project::{ProjectLayer, toml_value, warn_unknown_keys};
use super::resolve::set;
use crate::core::diagnostics::DebugConfig;
use crate::core::error::EngineError;

/// Configuration filename (case-insensitive on lookup)
pub const CONFIG_FILENAME: &str = "Ryo.toml";

/// Maximum directory levels to search upward for config
const MAX_SEARCH_DEPTH: usize = 3;

/// A Ryo2D project workspace.
///
/// Represents the root directory of a game project and provides
/// access to configured paths and settings.
#[derive(Debug, Clone)]
pub struct Workspace {
  /// Root directory of the workspace
  root: PathBuf,
  /// Parsed configuration
  config: WorkspaceConfig,
  /// The project settings as Ryo.toml (or the embedded values) sets them,
  /// without defaults
  project: ProjectLayer,
  /// Save directory of a release build (`ryo2d build`); `None` in dev
  release_save_dir: Option<PathBuf>,
}

/// Configuration loaded from `ryo.toml`.
#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
  /// Project name (defaults to directory name)
  pub name: String,
  /// Window title (from `[project] title`, defaults to name)
  pub title: String,
  /// Project description (optional, for tooling)
  pub description: Option<String>,
  /// Project version (optional, from the `[project]` section)
  pub version: Option<String>,
  /// Path to entry point relative to workspace root
  pub entry_point: PathBuf,
  /// Path to save data directory relative to workspace root. This is the
  /// dev-build save folder; release builds use [`Workspace::save_location`].
  pub save_data: PathBuf,
  /// Path to assets directory relative to workspace root
  pub assets: PathBuf,
  /// The debug switches the project sets (`[debug]`, plus `[system]
  /// show_fps`), over the engine's defaults
  pub debug: DebugConfig,
  /// Release/distribution configuration (from the `[release]` section)
  pub release: ReleaseConfig,
  /// The Tiled project (an asset path) naming the class and enum types of
  /// Tiled maps, from `[tilemap] tiled_project`; `None`: look for
  /// `project.tiled-project` at the asset root
  pub tiled_project: Option<String>,
  /// Game-specific environment variables from [env] section
  pub env: std::collections::HashMap<String, toml::Value>,
}

/// Release/distribution configuration options.
///
/// Controls build-time settings for distributing the game.
#[derive(Debug, Clone, Default)]
pub struct ReleaseConfig {
  /// Path to custom application icon (PNG), relative to workspace root.
  /// If None, the Ryo2D default logo is used.
  pub logo: Option<PathBuf>,
  /// Keep save data in `data/` beside the game instead of the OS user data
  /// folder (`[release] portable`, default `false`).
  pub portable: bool,
  /// Folder under the OS user data folder for this game's saves
  /// (`[release] save_folder`); `None` uses `[project] name`.
  pub save_folder: Option<String>,
}

impl Workspace {
  /// Discover workspace by searching for `ryo.toml` from current directory.
  ///
  /// Searches the current directory and up to 3 parent directories.
  /// If no config file is found, uses the current directory as workspace root
  /// with default settings. In a release build made by `ryo2d build`, save
  /// data goes to the OS user data folder (see [`save_location`](Self::save_location)).
  ///
  /// # Errors
  ///
  /// Returns an error if the current directory cannot be determined.
  pub fn discover() -> Result<Self, EngineError> {
    // What reading Ryo.toml logs reaches the engine's outputs
    crate::core::logging::ensure();
    let cwd = std::env::current_dir().map_err(|e| EngineError::WorkspaceError(e.to_string()))?;
    let mut workspace = match Self::from_embedded(&cwd, super::project::compiled_embedded_value) {
      Some(release) => release,
      None => Self::discover_from(&cwd)?,
    };
    workspace.release_save_dir = super::save_location::compiled_release_save_dir();
    Ok(workspace)
  }

  /// The workspace of a release build (`ryo2d build`): the Ryo.toml values
  /// embedded at build time, rooted at `root`. `None` for dev builds.
  ///
  /// Ryo.toml files on disk are ignored, so `ryo2d run` sees exactly what a
  /// shipped game sees.
  pub(crate) fn from_embedded<'a>(
    root: &Path,
    lookup: impl Fn(&str) -> Option<&'a str>,
  ) -> Option<Self> {
    let project = ProjectLayer::from_embedded(&lookup)?;
    let mut config = WorkspaceConfig::default();
    set(&mut config.title, project.title.clone());
    config.apply_project(&project);

    config.release.portable = lookup("RYO_EMBED_PORTABLE") == Some("true");
    config.release.save_folder = lookup("RYO_EMBED_SAVE_FOLDER").map(str::to_string);

    let env = embedded_env(lookup("RYO_EMBED_ENV"));
    config.env = env
      .iter()
      .filter_map(|(key, value)| {
        serde_json::from_value::<toml::Value>(value.clone())
          .ok()
          .map(|value| (key.clone(), value))
      })
      .collect();

    Some(Workspace {
      root: root.to_path_buf(),
      config,
      project,
      release_save_dir: None,
    })
  }

  /// The workspace a Ryo.toml's text describes, rooted at `root`: the one
  /// reader of Ryo.toml text. A key a known section doesn't have, or a value
  /// of the wrong type or text, warns once a run; the value is left unset.
  ///
  /// # Errors
  ///
  /// [`EngineError::WorkspaceError`] when the text is not TOML.
  pub(crate) fn from_toml_str(text: &str, root: PathBuf) -> Result<Workspace, EngineError> {
    Self::read_toml(text, root, &mut warn_once)
  }

  /// [`from_toml_str`](Self::from_toml_str), telling `warn` of each problem.
  pub(crate) fn read_toml(
    text: &str,
    root: PathBuf,
    warn: &mut dyn FnMut(String),
  ) -> Result<Workspace, EngineError> {
    let table: toml::Table = text.parse().map_err(|e| {
      EngineError::WorkspaceError(format!("Failed to parse {CONFIG_FILENAME}: {e}"))
    })?;
    warn_unknown_keys(&table, warn);
    let project = ProjectLayer::from_toml(&table, warn);
    let config = Self::build_config(&root, &table, &project, warn)?;
    Ok(Workspace {
      root,
      config,
      project,
      release_save_dir: None,
    })
  }

  /// Discover workspace starting from a specific directory.
  ///
  /// Searches the given directory and up to 3 parent directories
  /// for `Ryo.toml` (case-insensitive).
  pub fn discover_from(start: &Path) -> Result<Self, EngineError> {
    // What reading Ryo.toml logs reaches the engine's outputs
    crate::core::logging::ensure();
    let mut current = start.to_path_buf();

    for _ in 0..MAX_SEARCH_DEPTH {
      // Check for config file (case-insensitive)
      if let Some(config_path) = Self::find_config_file(&current) {
        return Self::from_path_with_config(&current, &config_path);
      }

      match current.parent() {
        Some(parent) => current = parent.to_path_buf(),
        None => break,
      }
    }

    // No config found - use start directory with defaults
    Self::from_path_with_defaults(start)
  }

  /// Find the config file in a directory (case-insensitive).
  ///
  /// Searches for a file matching `CONFIG_FILENAME` case-insensitively.
  fn find_config_file(dir: &Path) -> Option<PathBuf> {
    // Fast path: exact match
    let exact = dir.join(CONFIG_FILENAME);
    if exact.exists() {
      return Some(exact);
    }

    // Case-insensitive search
    let target_lower = CONFIG_FILENAME.to_lowercase();
    if let Ok(entries) = fs::read_dir(dir) {
      for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str()
          && name.to_lowercase() == target_lower
        {
          return Some(entry.path());
        }
      }
    }

    None
  }

  /// Load workspace from an explicit path.
  ///
  /// If `Ryo.toml` exists in the directory (case-insensitive), it will be parsed.
  /// Otherwise, default settings are used.
  pub fn from_path(root: impl AsRef<Path>) -> Result<Self, EngineError> {
    // What reading Ryo.toml logs reaches the engine's outputs
    crate::core::logging::ensure();
    let root = root.as_ref().to_path_buf();

    // Check for config file (case-insensitive)
    if let Some(config_path) = Self::find_config_file(&root) {
      return Self::from_path_with_config(&root, &config_path);
    }

    Self::from_path_with_defaults(&root)
  }

  /// Load workspace from a path with an explicit config file.
  fn from_path_with_config(root: &Path, config_path: &Path) -> Result<Self, EngineError> {
    let config_name = config_path
      .file_name()
      .and_then(|n| n.to_str())
      .unwrap_or(CONFIG_FILENAME);

    let contents = fs::read_to_string(config_path)
      .map_err(|e| EngineError::WorkspaceError(format!("Failed to read {}: {}", config_name, e)))?;

    Self::from_toml_str(&contents, root.to_path_buf())
  }

  /// Create workspace with default settings (no config file).
  fn from_path_with_defaults(root: &Path) -> Result<Self, EngineError> {
    Self::from_toml_str("", root.to_path_buf())
  }

  /// Build configuration from a Ryo.toml's table and its project settings,
  /// applying defaults.
  fn build_config(
    root: &Path,
    table: &toml::Table,
    project: &ProjectLayer,
    warn: &mut dyn FnMut(String),
  ) -> Result<WorkspaceConfig, EngineError> {
    // Project name: from config, or directory name
    let name = toml_value::<String>(table, "project", "name", warn).unwrap_or_else(|| {
      root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unnamed")
        .to_string()
    });

    // Window title: from [project].title, fall back to name
    let title = project.title.clone().unwrap_or_else(|| name.clone());

    // Entry point: from config, or inferred
    let entry_point = match toml_value(table, "paths", "entry_point", warn) {
      Some(entry_point) => entry_point,
      None => Self::infer_entry_point(root)?,
    };

    let env: HashMap<String, toml::Value> = match table.get("env") {
      Some(toml::Value::Table(env)) => env.clone().into_iter().collect(),
      Some(_) => {
        warn("Ryo.toml: `env` is not a section, ignoring it".to_string());
        HashMap::new()
      }
      None => HashMap::new(),
    };

    let mut config = WorkspaceConfig {
      name,
      title,
      description: toml_value(table, "project", "description", warn),
      version: toml_value(table, "project", "version", warn),
      entry_point,
      save_data: toml_value(table, "paths", "save_data", warn)
        .unwrap_or_else(|| PathBuf::from("data")),
      assets: toml_value(table, "paths", "assets", warn).unwrap_or_else(|| PathBuf::from("assets")),
      release: ReleaseConfig {
        logo: project.icon.clone(),
        portable: toml_value(table, "release", "portable", warn).unwrap_or(false),
        save_folder: toml_value(table, "release", "save_folder", warn),
      },
      ..WorkspaceConfig::default()
    };
    config.apply_project(project);

    config.env = env;

    Ok(config)
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

  /// The project settings as Ryo.toml (or, in a release build, the embedded
  /// values) sets them, without defaults.
  pub(crate) fn project(&self) -> &ProjectLayer {
    &self.project
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

  /// Where save data lives, without creating it.
  ///
  /// Release builds (`ryo2d build`) use the OS user data folder - `%APPDATA%`,
  /// `~/Library/Application Support` or `~/.local/share`, plus
  /// `[release] save_folder` or `[project] name` - or `data/` beside the game
  /// when `[release] portable = true`. Dev builds use `[paths] save_data`
  /// under the workspace root.
  pub fn save_location(&self) -> PathBuf {
    self
      .release_save_dir
      .clone()
      .unwrap_or_else(|| self.root.join(&self.config.save_data))
  }

  /// Get the absolute path to the save data directory ([`save_location`](Self::save_location)).
  ///
  /// Creates the directory if it doesn't exist.
  pub fn save_data_dir(&self) -> Result<PathBuf, EngineError> {
    let path = self.save_location();
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

  /// Check if a `Ryo.toml` config file exists in the workspace (case-insensitive).
  pub fn has_config_file(&self) -> bool {
    Self::find_config_file(&self.root).is_some()
  }

  /// Change the save data directory at runtime.
  ///
  /// This updates the workspace configuration in memory. The change is not
  /// persisted to `ryo.toml` but affects all subsequent save operations.
  /// This replaces the release save location too.
  ///
  /// # Arguments
  ///
  /// * `path` - New path for save data. If relative, it's relative to workspace root.
  pub fn set_save_data_path(&mut self, path: impl Into<PathBuf>) {
    self.config.save_data = path.into();
    self.release_save_dir = None;
  }
}

impl Default for WorkspaceConfig {
  fn default() -> Self {
    WorkspaceConfig {
      name: "unnamed".to_string(),
      title: "Ryo2D".to_string(),
      description: None,
      version: None,
      entry_point: PathBuf::from("src/main.rs"),
      save_data: PathBuf::from("data"),
      assets: PathBuf::from("assets"),
      debug: DebugConfig::default(),
      release: ReleaseConfig::default(),
      tiled_project: None,
      env: std::collections::HashMap::new(),
    }
  }
}

impl WorkspaceConfig {
  /// Get an environment variable as a string.
  ///
  /// Returns `None` if the key doesn't exist or isn't a string.
  ///
  /// # Example
  ///
  /// ```toml
  /// [env]
  /// game_mode = "debug"
  /// ```
  ///
  /// ```rust,ignore
  /// if config.env_str("game_mode") == Some("debug") {
  ///     // Enable debug features
  /// }
  /// ```
  pub fn env_str(&self, key: &str) -> Option<&str> {
    self.env.get(key).and_then(|v| v.as_str())
  }

  /// Get an environment variable as a boolean.
  ///
  /// Returns `None` if the key doesn't exist or isn't a boolean.
  ///
  /// # Example
  ///
  /// ```toml
  /// [env]
  /// debug_scene = true
  /// ```
  ///
  /// ```rust,ignore
  /// if config.env_bool("debug_scene").unwrap_or(false) {
  ///     // Start with debug scene
  /// }
  /// ```
  pub fn env_bool(&self, key: &str) -> Option<bool> {
    self.env.get(key).and_then(|v| v.as_bool())
  }

  /// Get an environment variable as an integer.
  ///
  /// Returns `None` if the key doesn't exist or isn't an integer.
  pub fn env_int(&self, key: &str) -> Option<i64> {
    self.env.get(key).and_then(|v| v.as_integer())
  }

  /// Get an environment variable as a float.
  ///
  /// Returns `None` if the key doesn't exist or isn't a float.
  pub fn env_float(&self, key: &str) -> Option<f64> {
    self.env.get(key).and_then(|v| v.as_float())
  }

  /// `[env]` as JSON values, the form `system.json` once seeded it in and
  /// `ryo2d build` embeds it in.
  pub(crate) fn env_json(&self) -> HashMap<String, serde_json::Value> {
    self
      .env
      .iter()
      .filter_map(|(k, v)| serde_json::to_value(v).ok().map(|jv| (k.clone(), jv)))
      .collect()
  }

  /// Check if an environment variable exists.
  pub fn has_env(&self, key: &str) -> bool {
    self.env.contains_key(key)
  }
}

impl WorkspaceConfig {
  /// Take the settings the project sets, keeping the defaults for the rest.
  /// `[system] show_fps` also shows the FPS counter.
  fn apply_project(&mut self, project: &ProjectLayer) {
    let debug = &mut self.debug;
    set(&mut debug.show_fps, project.show_fps);
    set(&mut debug.dev_console, project.dev_console);
    set(&mut debug.show_colliders, project.show_colliders);
    set(&mut debug.show_velocities, project.show_velocities);
    set(&mut debug.show_origins, project.show_origins);
    set(&mut debug.show_frame_timing, project.show_frame_timing);
    set(&mut debug.disable_debug_keys, project.disable_debug_keys);
    set(&mut debug.enable_profiler, project.enable_profiler);
    set(&mut debug.logger_window, project.logger_window);
    set(&mut debug.dev_window_keys, project.dev_window_keys);

    // Empty is no project: the default one
    self.tiled_project = project
      .tiled_project
      .clone()
      .filter(|path| !path.is_empty());
  }
}

/// Log a Ryo.toml warning, once a run: games read the workspace again (a
/// menu that calls `Workspace::discover`), and one misspelt key needs one line.
fn warn_once(message: String) {
  static WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
  let first = WARNED
    .get_or_init(Default::default)
    .lock()
    .map(|mut warned| warned.insert(message.clone()))
    .unwrap_or(true);
  if first {
    log::warn!("{message}");
  }
}

/// `[env]` as embedded by `ryo2d build` (a JSON object); empty when absent or malformed.
fn embedded_env(raw: Option<&str>) -> HashMap<String, serde_json::Value> {
  let Some(raw) = raw else {
    return HashMap::new();
  };
  serde_json::from_str(raw).unwrap_or_else(|e| {
    log::warn!("Malformed embedded [env] ({e}), ignoring it");
    HashMap::new()
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::config::DisplayMode;
  use std::env;

  #[test]
  fn dev_save_location_is_inside_the_workspace() {
    let ws = Workspace::discover().unwrap();
    assert!(ws.save_location().starts_with(ws.root()));
  }

  #[test]
  fn set_save_data_path_replaces_the_release_location() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::from_path(dir.path()).unwrap();
    ws.release_save_dir = Some(PathBuf::from("/users/me/data/Hopper"));
    assert_eq!(ws.save_location(), PathBuf::from("/users/me/data/Hopper"));

    ws.set_save_data_path("saves");

    assert_eq!(ws.save_location(), dir.path().join("saves"));
  }

  #[test]
  fn release_section_reads_portable_and_save_folder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
      dir.path().join("Ryo.toml"),
      "[release]\nportable = true\nsave_folder = \"Studio/Game\"\n",
    )
    .unwrap();
    let ws = Workspace::from_path(dir.path()).unwrap();
    assert!(ws.config().release.portable);
    assert_eq!(
      ws.config().release.save_folder.as_deref(),
      Some("Studio/Game")
    );

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ryo.toml"), "").unwrap();
    let ws = Workspace::from_path(dir.path()).unwrap();
    assert!(!ws.config().release.portable);
    assert_eq!(ws.config().release.save_folder, None);
  }

  fn embedded_release(name: &str) -> Option<&'static str> {
    match name {
      "RYO_EMBED_RESOLUTION" => Some("800x600"),
      "RYO_EMBED_TITLE" => Some("Hopper"),
      "RYO_EMBED_MASTER_VOLUME" => Some("0.3"),
      "RYO_EMBED_DISPLAY_MODE" => Some("borderless_fullscreen"),
      "RYO_EMBED_SHOW_COLLIDERS" => Some("true"),
      "RYO_EMBED_PORTABLE" => Some("true"),
      "RYO_EMBED_SAVE_FOLDER" => Some("Studio/Hopper"),
      "RYO_EMBED_ENV" => Some(r#"{"difficulty":"hard","debug_scene":false,"lives":3}"#),
      _ => None,
    }
  }

  #[test]
  fn release_workspace_reads_embedded_values() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::from_embedded(dir.path(), embedded_release).unwrap();
    let config = ws.config();
    assert_eq!(config.env_str("difficulty"), Some("hard"));
    assert_eq!(config.env_bool("debug_scene"), Some(false));
    assert_eq!(config.env_int("lives"), Some(3));
    assert_eq!(ws.project.resolution, Some((800, 600)));
    assert_eq!(ws.project.master_volume, Some(0.3));
    assert_eq!(
      ws.project.display_mode,
      Some(DisplayMode::BorderlessFullscreen)
    );
    assert_eq!(config.title, "Hopper");
    assert!(config.debug.show_colliders);
    assert!(config.release.portable);
    assert_eq!(config.release.save_folder.as_deref(), Some("Studio/Hopper"));
    assert_eq!(ws.root(), dir.path());
  }

  #[test]
  fn release_workspace_ignores_ryo_toml_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
      dir.path().join("Ryo.toml"),
      "[env]\ndifficulty = \"easy\"\n",
    )
    .unwrap();
    let ws = Workspace::from_embedded(dir.path(), embedded_release).unwrap();
    assert_eq!(ws.config().env_str("difficulty"), Some("hard"));
  }

  #[test]
  fn dev_builds_have_no_embedded_workspace() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Workspace::from_embedded(dir.path(), |_| None).is_none());
  }

  #[test]
  fn malformed_embedded_env_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::from_embedded(dir.path(), |name| match name {
      "RYO_EMBED_ENV" => Some("not json"),
      other => embedded_release(other),
    })
    .unwrap();
    assert!(ws.config().env.is_empty());
    assert_eq!(ws.project.resolution, Some((800, 600)));
  }

  #[test]
  fn workspace_config_defaults() {
    let config = WorkspaceConfig::default();
    assert_eq!(config.name, "unnamed");
    assert_eq!(config.entry_point, PathBuf::from("src/main.rs"));
    assert_eq!(config.save_data, PathBuf::from("data"));
    assert_eq!(config.assets, PathBuf::from("assets"));
    assert!(config.env.is_empty());
  }

  #[test]
  fn workspace_config_env_helpers() {
    let mut config = WorkspaceConfig::default();
    config
      .env
      .insert("debug_scene".to_string(), toml::Value::Boolean(true));
    config
      .env
      .insert("level".to_string(), toml::Value::Integer(5));
    config
      .env
      .insert("name".to_string(), toml::Value::String("test".to_string()));
    config
      .env
      .insert("speed".to_string(), toml::Value::Float(1.5));

    assert_eq!(config.env_bool("debug_scene"), Some(true));
    assert_eq!(config.env_int("level"), Some(5));
    assert_eq!(config.env_str("name"), Some("test"));
    assert_eq!(config.env_float("speed"), Some(1.5));
    assert!(config.has_env("debug_scene"));
    assert!(!config.has_env("nonexistent"));
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

  #[test]
  fn debug_config_defaults() {
    let config = WorkspaceConfig::default().debug;
    assert!(!config.show_colliders);
    assert!(!config.show_velocities);
    assert!(!config.show_origins);
    assert!(!config.show_frame_timing);
    assert!(!config.disable_debug_keys);
    // enable_profiler defaults to false (opt-in)
    assert!(!config.enable_profiler);
  }

  #[test]
  fn workspace_config_debug_defaults_included() {
    let config = WorkspaceConfig::default();
    assert!(!config.debug.show_colliders);
    assert!(!config.debug.show_velocities);
    assert!(!config.debug.show_origins);
  }

  #[test]
  fn workspace_config_show_fps_in_system() {
    let config = WorkspaceConfig::default();
    assert!(!config.debug.show_fps);
  }

  #[test]
  fn workspace_config_debug_disable_keys_default() {
    let config = WorkspaceConfig::default();
    assert!(!config.debug.disable_debug_keys);
  }

  fn read(text: &str) -> (Workspace, Vec<String>) {
    let mut warnings = Vec::new();
    let workspace = Workspace::read_toml(text, PathBuf::from("game"), &mut |warning| {
      warnings.push(warning)
    })
    .unwrap();
    (workspace, warnings)
  }

  #[test]
  fn debug_section_parses_disable_debug_keys_true() {
    let (ws, _) = read("[debug]\ndisable_debug_keys = true\n");
    assert!(ws.config().debug.disable_debug_keys);
    assert_eq!(ws.project.disable_debug_keys, Some(true));
  }

  #[test]
  fn debug_section_parses_disable_debug_keys_false() {
    let (ws, _) = read("[debug]\ndisable_debug_keys = false\n");
    assert!(!ws.config().debug.disable_debug_keys);
    assert_eq!(ws.project.disable_debug_keys, Some(false));
  }

  #[test]
  fn debug_section_defaults_disable_debug_keys_to_none() {
    let (ws, _) = read("[debug]\nshow_colliders = true\n");
    assert_eq!(ws.project.disable_debug_keys, None);
    assert!(!ws.config().debug.disable_debug_keys);
    assert!(ws.config().debug.show_colliders);
  }

  #[test]
  fn debug_section_parses_enable_profiler() {
    let (ws, _) = read("[debug]\nenable_profiler = true\n");
    assert!(ws.config().debug.enable_profiler);
  }

  #[test]
  fn system_show_fps_also_shows_the_fps_counter() {
    let (ws, _) = read("[system]\nshow_fps = true\n");
    assert_eq!(ws.project.show_fps, Some(true));
    assert!(ws.config().debug.show_fps);
  }

  #[test]
  fn debug_console_is_the_key_and_system_console_warns() {
    let (ws, warnings) = read("[debug]\nconsole = false\n");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(ws.project.dev_console, Some(false));
    assert!(!ws.config().debug.dev_console);

    let (ws, warnings) = read("[system]\nconsole = true\n");
    assert_eq!(
      warnings,
      ["Ryo.toml: unknown key `console` in [system] (did you mean `debug.console`?)"]
    );
    assert_eq!(ws.project.dev_console, None, "the old key sets nothing");
  }

  #[test]
  fn a_project_without_a_console_setting_has_none() {
    let (ws, _) = read("");
    assert_eq!(ws.project.dev_console, None);
    // The build's own default: on with the tools
    assert_eq!(ws.config().debug.dev_console, crate::DEV_TOOLS);
  }

  #[test]
  fn workspace_config_enable_profiler_default_false() {
    let config = WorkspaceConfig::default();
    // enable_profiler should default to false (opt-in)
    assert!(!config.debug.enable_profiler);
  }

  // ==========================================================================
  // Resolution strings
  // ==========================================================================

  #[test]
  fn system_section_parses_resolution_string() {
    let (ws, warnings) = read("[system]\nresolution = \"800x600\"\nvsync = true\n");
    assert_eq!(ws.project.resolution, Some((800, 600)));
    assert!(warnings.is_empty(), "{warnings:?}");
  }

  #[test]
  fn system_section_resolution_absent_is_none() {
    let (ws, _) = read("[system]\nvsync = true\n");
    assert_eq!(ws.project.resolution, None);
  }

  #[test]
  fn system_defaults_resolution_parsed_from_toml() {
    let (ws, _) = read("[system]\nresolution = \"1920x1080\"\n");
    assert_eq!(ws.project.resolution, Some((1920, 1080)));
  }

  #[test]
  fn system_defaults_malformed_resolution_falls_back() {
    let (ws, warnings) = read("[system]\nresolution = \"garbage\"\n");
    assert_eq!(ws.project.resolution, None);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
  }

  // ==========================================================================
  // The Ryo.toml reader
  // ==========================================================================

  #[test]
  fn malformed_toml_is_an_error() {
    let result = Workspace::from_toml_str("[system\nvsync = true\n", PathBuf::from("game"));
    assert!(matches!(result, Err(EngineError::WorkspaceError(_))));
  }

  #[test]
  fn disk_and_text_read_the_same_way() {
    let text = "[project]\nname = \"Hopper\"\n\n[system]\nresolution = \"640x360\"\n";
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ryo.toml"), text).unwrap();

    let from_disk = Workspace::from_path(dir.path()).unwrap();
    let from_text = Workspace::from_toml_str(text, dir.path().to_path_buf()).unwrap();

    assert_eq!(from_disk.project, from_text.project);
    assert_eq!(from_disk.name(), "Hopper");
    assert_eq!(from_text.project.resolution, Some((640, 360)));
  }

  #[test]
  fn a_misspelt_key_warns_with_the_nearest_known_key() {
    let (ws, warnings) = read("[system]\nvsyn = true\n");
    assert_eq!(
      warnings,
      ["Ryo.toml: unknown key `vsyn` in [system] (did you mean `vsync`?)"]
    );
    assert!(warnings[0].contains("`vsync`"));
    assert_eq!(ws.project.vsync, None);
  }

  #[test]
  fn an_unknown_key_with_no_near_known_key_warns_without_a_guess() {
    let (_, warnings) = read("[system]\nwidth = 800\n");
    assert_eq!(warnings, ["Ryo.toml: unknown key `width` in [system]"]);
  }

  #[test]
  fn keys_outside_known_sections_do_not_warn() {
    let (_, warnings) = read("[env]\nfoo = 1\n\n[custom]\nx = 2\n");
    assert!(warnings.is_empty(), "{warnings:?}");
  }

  #[test]
  fn a_misspelt_key_warns_once_a_run() {
    let log = crate::core::testing::capture_log();
    let text = "[debug]\nshow_coliders_once_a_run = true\n";
    Workspace::from_toml_str(text, PathBuf::from("game")).unwrap();
    Workspace::from_toml_str(text, PathBuf::from("game")).unwrap();
    let warned = log.containing("`show_coliders_once_a_run`");
    assert_eq!(warned, 1);
  }

  /// Every `Ryo.toml` under `dir`, leaving out build output
  fn ryo_tomls(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
      let path = entry.path();
      let name = entry.file_name().to_string_lossy().to_lowercase();
      if path.is_dir() {
        if name != "target" && name != "dist" {
          ryo_tomls(&path, found);
        }
      } else if name == "ryo.toml" {
        found.push(path);
      }
    }
  }

  #[test]
  fn every_example_ryo_toml_parses_without_a_warning() {
    let mut files = Vec::new();
    ryo_tomls(
      &Path::new(env!("CARGO_MANIFEST_DIR")).join("examples"),
      &mut files,
    );
    assert!(files.len() > 20, "found only {files:?}");

    let mut warnings = Vec::new();
    for file in &files {
      let text = fs::read_to_string(file).unwrap();
      let root = file.parent().unwrap().to_path_buf();
      Workspace::read_toml(&text, root, &mut |warning| {
        warnings.push(format!("{}: {warning}", file.display()))
      })
      .unwrap();
    }

    assert!(warnings.is_empty(), "{warnings:#?}");
  }
}
