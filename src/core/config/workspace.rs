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
//! **Priority chain:** `system.json` > `Ryo.toml [system]` > engine defaults
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
//! [system]  # Defaults for system.json (user-facing settings)
//! resolution_width = 1920
//! resolution_height = 1080
//! vsync = true
//! fullscreen = false
//! master_volume = 1.0
//! music_volume = 0.8
//! sfx_volume = 1.0
//! show_fps = false
//! console = true  # Enable developer console (works in debug and release)
//!
//! [debug]  # Developer debug flags (NOT in system.json)
//! show_colliders = false
//! show_velocities = false
//! show_origins = false
//!
//! [env]  # Game-specific variables
//! debug_mode = false
//! ```
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
//! // Access system defaults from Ryo.toml [system] section
//! let system_defaults = &workspace.config().system;
//! println!("Default resolution: {}x{}", system_defaults.resolution_width, system_defaults.resolution_height);
//! ```

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

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
}

/// Configuration loaded from `ryo.toml`.
#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
  /// Project name (defaults to directory name)
  pub name: String,
  /// Window title (from [project].title, defaults to name)
  pub title: String,
  /// Project description (optional, for tooling)
  pub description: Option<String>,
  /// Project version (optional, from [project] section)
  pub version: Option<String>,
  /// Path to entry point relative to workspace root
  pub entry_point: PathBuf,
  /// Path to save data directory relative to workspace root
  pub save_data: PathBuf,
  /// Path to assets directory relative to workspace root
  pub assets: PathBuf,
  /// Debug configuration options
  pub debug: DebugConfig,
  /// System defaults for user configuration (from [system] section)
  pub system: SystemDefaults,
  /// Release/distribution configuration (from [release] section)
  pub release: ReleaseConfig,
  /// Game-specific environment variables from [env] section
  pub env: std::collections::HashMap<String, toml::Value>,
}

/// Debug configuration options for development.
///
/// See [`crate::core::debug::DebugConfig`] for the engine-level config.
/// This struct mirrors those options for TOML deserialization.
#[derive(Debug, Clone)]
pub struct DebugConfig {
  /// Whether to render collision boundaries
  pub show_colliders: bool,
  /// Whether to draw velocity vectors on physics entities
  pub show_velocities: bool,
  /// Whether to draw entity origin points
  pub show_origins: bool,
  /// Whether to display frame timing breakdown
  pub show_frame_timing: bool,
  /// Whether to display debug statistics overlay
  pub show_stats: bool,
  /// Whether to display FPS counter
  pub show_fps: bool,
  /// Whether to disable F1-F4 debug key toggles
  pub disable_debug_keys: bool,
  /// Whether to open the flamegraph profiler window (requires --features profiler)
  pub enable_profiler: bool,
  /// Whether to open the logger dev window at startup (shows engine/game logs in real-time)
  pub logger_window: bool,
  /// Whether to enable dev window hotkeys (F8=profiler, F9=logger).
  /// Defaults to true in debug builds, false in release builds.
  pub dev_window_keys: bool,
}

impl Default for DebugConfig {
  fn default() -> Self {
    Self {
      show_colliders: false,
      show_velocities: false,
      show_origins: false,
      show_frame_timing: false,
      show_stats: false,
      show_fps: false,
      disable_debug_keys: false,
      enable_profiler: false, // Disabled by default (requires explicit opt-in)
      logger_window: false,   // Disabled by default (requires explicit opt-in)
      // Dev window keys (F8/F9) enabled by default in debug builds only
      #[cfg(debug_assertions)]
      dev_window_keys: true,
      #[cfg(not(debug_assertions))]
      dev_window_keys: false,
    }
  }
}

/// Release/distribution configuration options.
///
/// Controls build-time settings for distributing the game.
#[derive(Debug, Clone, Default)]
pub struct ReleaseConfig {
  /// Path to custom application icon (PNG), relative to workspace root.
  /// If None, the Ryo2D default logo is used.
  pub logo: Option<PathBuf>,
}

/// System defaults for user configuration (system.json).
///
/// These values from the `[system]` section in Ryo.toml define the
/// developer defaults for user-facing settings. When system.json is created,
/// these values are used. When system.json is loaded, missing fields fall back
/// to these values.
///
/// **Mapped section:** `[system]` only
/// **Not mapped:** `[debug]`, `[project]`, `[paths]`, `[env]` (dev-only)
///
/// Priority chain: system.json (user) > Ryo.toml [system] (dev) > engine defaults
#[derive(Debug, Clone)]
pub struct SystemDefaults {
  /// Default resolution width
  pub resolution_width: u32,
  /// Default resolution height
  pub resolution_height: u32,
  /// Default VSync setting
  pub vsync: bool,
  /// Default display mode (windowed, fullscreen, borderless)
  pub display_mode: super::save::DisplayMode,
  /// Default master volume (0.0 - 1.0)
  pub master_volume: f32,
  /// Default music volume (0.0 - 1.0)
  pub music_volume: f32,
  /// Default SFX volume (0.0 - 1.0)
  pub sfx_volume: f32,
  /// Default show FPS setting (from [system] section)
  pub show_fps: bool,
  /// Whether to enable the developer console (toggled with backtick key).
  /// Unlike debug settings, this is available in both debug and release builds.
  pub dev_console: bool,
}

impl Default for SystemDefaults {
  fn default() -> Self {
    SystemDefaults {
      resolution_width: 1280,
      resolution_height: 720,
      vsync: true,
      display_mode: super::save::DisplayMode::Windowed,
      master_volume: 0.8,
      music_volume: 1.0,
      sfx_volume: 1.0,
      show_fps: false,
      dev_console: false, // Disabled by default, enable in Ryo.toml [system] section
    }
  }
}

/// Raw TOML structure for deserialization
#[derive(Debug, Deserialize, Default)]
struct RyoToml {
  project: Option<ProjectSection>,
  paths: Option<PathsSection>,
  system: Option<SystemSection>,
  debug: Option<DebugSection>,
  release: Option<ReleaseSection>,
  /// Game-specific environment variables (arbitrary key-value pairs)
  #[serde(default)]
  env: std::collections::HashMap<String, toml::Value>,
}

#[derive(Debug, Deserialize, Default)]
struct ProjectSection {
  name: Option<String>,
  version: Option<String>,
  /// Window title (defaults to name if not specified)
  title: Option<String>,
  /// Project description (optional, for tooling)
  description: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PathsSection {
  entry_point: Option<String>,
  save_data: Option<String>,
  assets: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct DebugSection {
  show_colliders: Option<bool>,
  show_velocities: Option<bool>,
  show_origins: Option<bool>,
  show_frame_timing: Option<bool>,
  disable_debug_keys: Option<bool>,
  enable_profiler: Option<bool>,
  logger_window: Option<bool>,
  dev_window_keys: Option<bool>,
}

/// Raw TOML [release] section for build/distribution settings
#[derive(Debug, Deserialize, Default)]
struct ReleaseSection {
  /// Path to custom application icon (PNG). If not specified, uses Ryo2D default.
  logo: Option<String>,
}

/// Raw TOML [system] section for user-facing defaults
#[derive(Debug, Deserialize, Default)]
struct SystemSection {
  resolution_width: Option<u32>,
  resolution_height: Option<u32>,
  vsync: Option<bool>,
  /// New display mode field (windowed, fullscreen, borderless_fullscreen)
  display_mode: Option<String>,
  /// Legacy fullscreen field for backward compatibility
  fullscreen: Option<bool>,
  /// Borderless window mode (when true and fullscreen=false, creates borderless window)
  borderless: Option<bool>,
  master_volume: Option<f32>,
  music_volume: Option<f32>,
  sfx_volume: Option<f32>,
  /// Whether to display FPS counter (user preference)
  show_fps: Option<bool>,
  /// Whether to enable the developer console (toggled with backtick key)
  console: Option<bool>,
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
    let cwd = std::env::current_dir().map_err(|e| EngineError::WorkspaceError(e.to_string()))?;

    Self::discover_from(&cwd)
  }

  /// Discover workspace starting from a specific directory.
  ///
  /// Searches the given directory and up to 3 parent directories
  /// for `Ryo.toml` (case-insensitive).
  pub fn discover_from(start: &Path) -> Result<Self, EngineError> {
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

    let raw: RyoToml = toml::from_str(&contents).map_err(|e| {
      EngineError::WorkspaceError(format!("Failed to parse {}: {}", config_name, e))
    })?;

    let config = Self::build_config(root, raw)?;
    Ok(Workspace {
      root: root.to_path_buf(),
      config,
    })
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
    let debug_section = raw.debug.unwrap_or_default();
    let release_section = raw.release.unwrap_or_default();

    // Project name: from config, or directory name
    let name = project.name.unwrap_or_else(|| {
      root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unnamed")
        .to_string()
    });

    // Window title: from [project].title, fall back to name
    let title = project.title.unwrap_or_else(|| name.clone());

    // Project description (optional)
    let description = project.description;

    // Entry point: from config, or inferred
    let entry_point = match paths.entry_point {
      Some(ep) => PathBuf::from(ep),
      None => Self::infer_entry_point(root)?,
    };

    // Save data: from config, or default
    let save_data = PathBuf::from(paths.save_data.unwrap_or_else(|| "data".to_string()));

    // Assets: from config, or default
    let assets = PathBuf::from(paths.assets.unwrap_or_else(|| "assets".to_string()));

    // Debug config
    // Dev window keys default to true in debug builds, false in release
    #[cfg(debug_assertions)]
    let dev_window_keys_default = true;
    #[cfg(not(debug_assertions))]
    let dev_window_keys_default = false;

    // System defaults (for system.json generation)
    // Combines [system] + [info] sections (user-facing settings only)
    let system_section = raw.system.unwrap_or_default();

    let debug = DebugConfig {
      show_colliders: debug_section.show_colliders.unwrap_or(false),
      show_velocities: debug_section.show_velocities.unwrap_or(false),
      show_origins: debug_section.show_origins.unwrap_or(false),
      show_frame_timing: debug_section.show_frame_timing.unwrap_or(false),
      show_stats: false,                                  // Always off by default
      show_fps: system_section.show_fps.unwrap_or(false), // From [system] section
      disable_debug_keys: debug_section.disable_debug_keys.unwrap_or(false),
      enable_profiler: debug_section.enable_profiler.unwrap_or(false),
      logger_window: debug_section.logger_window.unwrap_or(false), // Default false (opt-in)
      dev_window_keys: debug_section
        .dev_window_keys
        .unwrap_or(dev_window_keys_default),
    };

    let engine_defaults = SystemDefaults::default();

    // Parse display_mode from string, with fallback to legacy fullscreen/borderless fields
    let display_mode = if let Some(mode_str) = &system_section.display_mode {
      match mode_str.to_lowercase().as_str() {
        "windowed" => super::save::DisplayMode::Windowed,
        "fullscreen" => super::save::DisplayMode::Fullscreen,
        "borderless" | "borderless_fullscreen" => super::save::DisplayMode::BorderlessFullscreen,
        _ => engine_defaults.display_mode,
      }
    } else if system_section.fullscreen == Some(true) {
      // Legacy: fullscreen=true maps to BorderlessFullscreen for better UX
      super::save::DisplayMode::BorderlessFullscreen
    } else if system_section.borderless == Some(true) {
      // borderless=true (with fullscreen=false) creates borderless fullscreen window
      super::save::DisplayMode::BorderlessFullscreen
    } else {
      engine_defaults.display_mode
    };

    let system = SystemDefaults {
      resolution_width: system_section
        .resolution_width
        .unwrap_or(engine_defaults.resolution_width),
      resolution_height: system_section
        .resolution_height
        .unwrap_or(engine_defaults.resolution_height),
      vsync: system_section.vsync.unwrap_or(engine_defaults.vsync),
      display_mode,
      master_volume: system_section
        .master_volume
        .unwrap_or(engine_defaults.master_volume),
      music_volume: system_section
        .music_volume
        .unwrap_or(engine_defaults.music_volume),
      sfx_volume: system_section
        .sfx_volume
        .unwrap_or(engine_defaults.sfx_volume),
      // show_fps is a user preference, stored in [system] section
      show_fps: system_section.show_fps.unwrap_or(engine_defaults.show_fps),
      // console is in [system] section so it works in both debug and release builds
      dev_console: system_section
        .console
        .unwrap_or(engine_defaults.dev_console),
    };

    // Game-specific env (pass through as-is)
    let env = raw.env;

    // Release config
    let release = ReleaseConfig {
      logo: release_section.logo.map(PathBuf::from),
    };

    Ok(WorkspaceConfig {
      name,
      title,
      description,
      version: project.version,
      entry_point,
      save_data,
      assets,
      debug,
      system,
      release,
      env,
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

  /// Check if a `Ryo.toml` config file exists in the workspace (case-insensitive).
  pub fn has_config_file(&self) -> bool {
    Self::find_config_file(&self.root).is_some()
  }

  /// Change the save data directory at runtime.
  ///
  /// This updates the workspace configuration in memory. The change is not
  /// persisted to `ryo.toml` but affects all subsequent save operations.
  ///
  /// # Arguments
  ///
  /// * `path` - New path for save data. If relative, it's relative to workspace root.
  pub fn set_save_data_path(&mut self, path: impl Into<PathBuf>) {
    self.config.save_data = path.into();
  }

  /// Create a `SaveManager` configured for this workspace.
  ///
  /// The save manager will use the workspace's save data directory.
  /// The directory will be created if it doesn't exist.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// let workspace = Workspace::discover()?;
  /// let save_manager = workspace.create_save_manager()?;
  /// save_manager.save_game("slot1", &my_data)?;
  /// ```
  pub fn create_save_manager(&self) -> Result<super::save::SaveManager, EngineError> {
    let save_dir = self.save_data_dir()?;
    super::save::SaveManager::new(save_dir).map_err(EngineError::Save)
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
      system: SystemDefaults::default(),
      release: ReleaseConfig::default(),
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

  /// Check if an environment variable exists.
  pub fn has_env(&self, key: &str) -> bool {
    self.env.contains_key(key)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::config::save::DisplayMode;
  use std::env;

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

  // ==========================================================================
  // SystemDefaults tests
  // ==========================================================================

  #[test]
  fn system_defaults_engine_values() {
    let defaults = SystemDefaults::default();

    // Engine defaults: 1280x720
    assert_eq!(defaults.resolution_width, 1280);
    assert_eq!(defaults.resolution_height, 720);
    // VSync on, windowed
    assert!(defaults.vsync);
    assert_eq!(defaults.display_mode, DisplayMode::Windowed);
    // Audio: master=0.8, music=1.0, sfx=1.0
    assert_eq!(defaults.master_volume, 0.8);
    assert_eq!(defaults.music_volume, 1.0);
    assert_eq!(defaults.sfx_volume, 1.0);
    // FPS off by default
    assert!(!defaults.show_fps);
  }

  #[test]
  fn system_defaults_clone() {
    let defaults = SystemDefaults::default();
    let cloned = defaults.clone();

    assert_eq!(cloned.resolution_width, defaults.resolution_width);
    assert_eq!(cloned.resolution_height, defaults.resolution_height);
    assert_eq!(cloned.vsync, defaults.vsync);
    assert_eq!(cloned.display_mode, defaults.display_mode);
    assert_eq!(cloned.master_volume, defaults.master_volume);
    assert_eq!(cloned.music_volume, defaults.music_volume);
    assert_eq!(cloned.sfx_volume, defaults.sfx_volume);
    assert_eq!(cloned.show_fps, defaults.show_fps);
  }

  #[test]
  fn debug_config_defaults() {
    let config = DebugConfig::default();
    assert!(!config.show_colliders);
    assert!(!config.show_velocities);
    assert!(!config.show_origins);
    assert!(!config.show_frame_timing);
    assert!(!config.disable_debug_keys);
    // enable_profiler defaults to false (opt-in)
    assert!(!config.enable_profiler);
  }

  #[test]
  fn workspace_config_system_defaults_included() {
    let config = WorkspaceConfig::default();
    // System defaults should be engine defaults
    assert_eq!(config.system.resolution_width, 1280);
    assert_eq!(config.system.resolution_height, 720);
    assert_eq!(config.system.master_volume, 0.8);
    assert!(!config.system.show_fps);
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
    // show_fps is now in [system] section, not [info]
    let config = WorkspaceConfig::default();
    assert!(!config.system.show_fps);
    assert!(!config.debug.show_fps);
  }

  #[test]
  fn workspace_config_debug_disable_keys_default() {
    let config = WorkspaceConfig::default();
    assert!(!config.debug.disable_debug_keys);
  }

  #[test]
  fn debug_section_parses_disable_debug_keys_true() {
    let toml_str = r#"
      disable_debug_keys = true
    "#;
    let section: DebugSection = toml::from_str(toml_str).unwrap();
    assert_eq!(section.disable_debug_keys, Some(true));
  }

  #[test]
  fn debug_section_parses_disable_debug_keys_false() {
    let toml_str = r#"
      disable_debug_keys = false
    "#;
    let section: DebugSection = toml::from_str(toml_str).unwrap();
    assert_eq!(section.disable_debug_keys, Some(false));
  }

  #[test]
  fn debug_section_defaults_disable_debug_keys_to_none() {
    let toml_str = r#"
      show_colliders = true
    "#;
    let section: DebugSection = toml::from_str(toml_str).unwrap();
    assert_eq!(section.disable_debug_keys, None);
  }

  #[test]
  fn debug_section_parses_enable_profiler() {
    let toml_str = r#"
      enable_profiler = true
    "#;
    let section: DebugSection = toml::from_str(toml_str).unwrap();
    assert_eq!(section.enable_profiler, Some(true));
  }

  #[test]
  fn workspace_config_enable_profiler_default_false() {
    let config = WorkspaceConfig::default();
    // enable_profiler should default to false (opt-in)
    assert!(!config.debug.enable_profiler);
  }
}
