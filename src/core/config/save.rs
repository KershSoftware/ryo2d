//! Save/Load System for Ryo2D
//!
//! Provides a 3-tier save system:
//!
//! - **Tier 1: System State** - User settings (display, audio) \[JSON, uncompressed\]
//! - **Tier 2: World State** - ECS world snapshots (entities and serializable components) \[compressed\]
//! - **Tier 3: Game Data** - Generic user data for game-specific saves \[compressed\]
//!
//! # Configuration Hierarchy
//!
//! System state (Tier 1) follows a priority chain:
//!
//! 1. **system.json** (user settings) - highest priority, player edits
//! 2. **Ryo.toml [system]** (dev defaults) - developer-defined defaults
//! 3. **Engine defaults** - hardcoded fallbacks
//!
//! Use [`SaveManager::load_or_create_system`] to automatically handle this chain:
//! - Creates system.json from Ryo.toml [system] + engine defaults if missing
//! - Merges missing fields when loading existing files
//!
//! # Configuration Files
//!
//! Save files default to `{workspace}/data/` but can be configured:
//!
//! ```toml
//! # Ryo.toml
//! [paths]
//! save_data = "saves"  # Custom save directory
//!
//! [system]  # Defaults for system.json when first created
//! resolution = "1920x1080"
//! vsync = true
//! master_volume = 0.8
//! ```
//!
//! # Tier 1 Example (User Settings)
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Recommended: Use load_or_create_system for proper fallback chain
//! let workspace = Workspace::discover()?;
//! let save_manager = workspace.create_save_manager()?;
//! let settings = save_manager.load_or_create_system(&workspace.config().system)?;
//!
//! // Or manually load/save:
//! let mut settings = save_manager.load_system()?;
//! settings.display.width = 1920; // or set via resolution string
//! settings.master_volume = 0.75;
//! save_manager.save_system(&settings)?;
//! ```
//!
//! # Tier 3 Example (Game Data)
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct PlayerProgress {
//!     level: u32,
//!     score: u64,
//!     unlocked_items: Vec<String>,
//! }
//!
//! // High-level game data
//! let progress = PlayerProgress {
//!     level: 5,
//!     score: 12500,
//!     unlocked_items: vec!["sword".into(), "shield".into()],
//! };
//!
//! // Save game data to slot (compressed)
//! save_manager.save_game("slot1", &progress)?;
//!
//! // Load game data
//! let loaded: PlayerProgress = save_manager.load_game("slot1")?;
//! ```

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::core::error::{SaveError, SaveResult};

// ============================================================================
// System Field Filter
// ============================================================================

/// User preference fields that can be loaded from system.json.
///
/// Used with [`UserPreferenceFilter`] to control which user preferences
/// are loaded from `system.json` when using
/// [`with_user_preferences`](crate::core::engine::EngineBuilder::with_user_preferences).
///
/// Fields not in the filter will use Ryo.toml `[system]` defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserPreference {
  /// Master volume (0.0 - 1.0)
  MasterVolume,
  /// Music volume (0.0 - 1.0)
  MusicVolume,
  /// Sound effects volume (0.0 - 1.0)
  SfxVolume,
  /// Display resolution (width + height together)
  Resolution,
  /// Display mode (windowed, exclusive_fullscreen, borderless_fullscreen)
  DisplayMode,
  /// VSync preference
  Vsync,
  /// Show FPS counter
  ShowFps,
}

impl UserPreference {
  /// Returns all available user preference fields.
  pub fn all() -> &'static [UserPreference] {
    &[
      UserPreference::MasterVolume,
      UserPreference::MusicVolume,
      UserPreference::SfxVolume,
      UserPreference::Resolution,
      UserPreference::DisplayMode,
      UserPreference::Vsync,
      UserPreference::ShowFps,
    ]
  }
}

/// Filter controlling which user preferences are loaded from `system.json`.
///
/// Used with [`with_user_preferences`](crate::core::engine::EngineBuilder::with_user_preferences)
/// to selectively load player settings from `system.json`.
///
/// # Examples
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// // Load only volume settings from system.json
/// EngineBuilder::new()
///     .title("My Game")
///     .with_user_preferences([UserPreference::MasterVolume, UserPreference::MusicVolume, UserPreference::SfxVolume])
///     .with_scene(GameScene::new())
///     .build()
///     .run()?;
///
/// // Load all user preferences (default behavior)
/// EngineBuilder::new()
///     .with_user_preferences(UserPreferenceFilter::all())
///     .build()
///     .run()?;
///
/// // Load nothing from system.json (use only Ryo.toml defaults)
/// EngineBuilder::new()
///     .with_user_preferences(UserPreferenceFilter::none())
///     .build()
///     .run()?;
/// ```
#[derive(Debug, Clone)]
pub struct UserPreferenceFilter {
  fields: HashSet<UserPreference>,
}

impl UserPreferenceFilter {
  /// All fields enabled (default behavior, backward compatible).
  pub fn all() -> Self {
    Self {
      fields: UserPreference::all().iter().copied().collect(),
    }
  }

  /// No fields - everything uses Ryo.toml/engine defaults.
  pub fn none() -> Self {
    Self {
      fields: HashSet::new(),
    }
  }

  /// Only specified fields are loaded from system.json.
  pub fn only(fields: impl IntoIterator<Item = UserPreference>) -> Self {
    Self {
      fields: fields.into_iter().collect(),
    }
  }

  /// Check if a field is enabled.
  pub fn contains(&self, field: UserPreference) -> bool {
    self.fields.contains(&field)
  }

  /// Check if filter is empty (no fields enabled).
  pub fn is_empty(&self) -> bool {
    self.fields.is_empty()
  }

  /// Returns the number of enabled fields.
  pub fn len(&self) -> usize {
    self.fields.len()
  }
}

impl Default for UserPreferenceFilter {
  fn default() -> Self {
    Self::all()
  }
}

/// Allow creating a filter from an array of fields.
impl<const N: usize> From<[UserPreference; N]> for UserPreferenceFilter {
  fn from(fields: [UserPreference; N]) -> Self {
    Self::only(fields)
  }
}

/// Allow creating a filter from a Vec of fields.
impl From<Vec<UserPreference>> for UserPreferenceFilter {
  fn from(fields: Vec<UserPreference>) -> Self {
    Self::only(fields)
  }
}

/// Allow creating a filter from a slice of fields.
impl From<&[UserPreference]> for UserPreferenceFilter {
  fn from(fields: &[UserPreference]) -> Self {
    Self::only(fields.iter().copied())
  }
}

// ============================================================================
// Save File Format
// ============================================================================

/// Magic bytes identifying Ryo2D save files
const MAGIC: &[u8; 4] = b"RYO2";

/// Current save format version
const SAVE_VERSION: u32 = 1;

/// File extension for system saves (JSON)
const SYSTEM_EXTENSION: &str = "json";

/// File extension for world saves
const WORLD_EXTENSION: &str = "ryo-world";

/// File extension for game saves
const GAME_EXTENSION: &str = "ryo-save";

// ============================================================================
// Tier 1: System State
// ============================================================================

/// Serialize f32 as a cleanly-rounded f64 to avoid precision artifacts in JSON.
///
/// Without this, serde_json converts f32 → f64 naively, producing values like
/// `0.4000000059604645` instead of `0.4`.
fn serialize_f32_clean<S: serde::Serializer>(value: &f32, serializer: S) -> Result<S::Ok, S::Error> {
  let rounded = ((*value as f64) * 100.0).round() / 100.0;
  serializer.serialize_f64(rounded)
}

/// User settings and configuration (Tier 1).
///
/// Represents the contents of `system.json`, the player-editable configuration file.
/// This is separate from `Ryo.toml` which contains developer settings.
///
/// # Configuration Hierarchy
///
/// ```text
/// system.json (user) > Ryo.toml [system] (dev) > engine defaults
/// ```
///
/// When `system.json` doesn't exist, it's created from `Ryo.toml`'s `[system]` section
/// combined with engine defaults. Use [`SaveManager::load_or_create_system`] to handle
/// this automatically.
///
/// # Contents
///
/// - Audio volume levels (master, music, sfx)
/// - Display settings (resolution, display mode, vsync)
/// - Optional engine overrides (show_fps)
/// - Custom key-value storage for game-specific env variables
///
/// Saved as human-readable JSON to `{save_dir}/system.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemState {
  /// Master volume (0.0 - 1.0)
  #[serde(serialize_with = "serialize_f32_clean")]
  pub master_volume: f32,
  /// Music volume (0.0 - 1.0)
  #[serde(serialize_with = "serialize_f32_clean")]
  pub music_volume: f32,
  /// Sound effects volume (0.0 - 1.0)
  #[serde(serialize_with = "serialize_f32_clean")]
  pub sfx_volume: f32,

  /// Display settings
  pub display: DisplaySettings,

  /// Show FPS counter (engine override, optional)
  #[serde(default)]
  pub show_fps: Option<bool>,

  /// VSync preference (overrides display.vsync when present)
  /// - None: Use Mailbox with fallback to Immediate (best performance, no tearing)
  /// - Some(true): Use Mailbox with fallback to Fifo (vsync on)
  /// - Some(false): Use Immediate (vsync off, may tear)
  #[serde(default)]
  pub vsync: Option<bool>,

  /// Custom key-value storage for game-specific environment variables.
  ///
  /// Seeded from `Ryo.toml [env]` when creating a new `system.json`.
  /// Values preserve their native types (bool, int, float, string) from TOML/JSON.
  #[serde(default)]
  pub env: HashMap<String, serde_json::Value>,
}

/// Display/window mode for the game.
///
/// Controls how the game window appears on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
  /// Regular resizable window
  #[default]
  Windowed,
  /// Exclusive fullscreen (takes over the display)
  ExclusiveFullscreen,
  /// Borderless fullscreen window (fills screen, easy alt-tab)
  BorderlessFullscreen,
}

impl DisplayMode {
  /// Returns all available display modes for UI selection
  pub fn all() -> &'static [DisplayMode] {
    &[
      DisplayMode::Windowed,
      DisplayMode::ExclusiveFullscreen,
      DisplayMode::BorderlessFullscreen,
    ]
  }

  /// Human-readable name for UI display
  pub fn display_name(&self) -> &'static str {
    match self {
      DisplayMode::Windowed => "Windowed",
      DisplayMode::ExclusiveFullscreen => "Exclusive Fullscreen",
      DisplayMode::BorderlessFullscreen => "Borderless Fullscreen",
    }
  }
}

/// Common screen resolutions for settings UI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolution {
  pub width: u32,
  pub height: u32,
}

/// Static list of common resolutions
static COMMON_RESOLUTIONS: &[Resolution] = &[
  Resolution {
    width: 1280,
    height: 720,
  }, // 720p
  Resolution {
    width: 1366,
    height: 768,
  }, // HD
  Resolution {
    width: 1600,
    height: 900,
  }, // HD+
  Resolution {
    width: 1920,
    height: 1080,
  }, // 1080p
  Resolution {
    width: 2560,
    height: 1440,
  }, // 1440p / 2K
  Resolution {
    width: 3840,
    height: 2160,
  }, // 4K UHD
];

impl Resolution {
  pub const fn new(width: u32, height: u32) -> Self {
    Self { width, height }
  }

  /// Returns common resolutions from 720p to 4K
  pub fn common() -> &'static [Resolution] {
    COMMON_RESOLUTIONS
  }

  /// Format as "WxH" string
  pub fn display_name(&self) -> String {
    format!("{}x{}", self.width, self.height)
  }
}

/// Parse a "WxH" resolution string (e.g., "1280x720"). Returns None if malformed.
pub fn parse_resolution(s: &str) -> Option<(u32, u32)> {
  let parts: Vec<&str> = s.split('x').collect();
  if parts.len() == 2 {
    if let (Ok(w), Ok(h)) = (parts[0].trim().parse(), parts[1].trim().parse()) {
      return Some((w, h));
    }
  }
  None
}

/// Display configuration
///
/// Serializes resolution as a single `"resolution": "WxH"` string.
/// Deserializes from both the new format and legacy `"width"/"height"` fields.
#[derive(Debug, Clone)]
pub struct DisplaySettings {
  /// Window width
  pub width: u32,
  /// Window height
  pub height: u32,
  /// Display mode (windowed, exclusive_fullscreen, borderless_fullscreen)
  pub display_mode: DisplayMode,
  /// VSync enabled
  pub vsync: bool,
}

impl Serialize for DisplaySettings {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeStruct;
    let mut state = serializer.serialize_struct("DisplaySettings", 3)?;
    state.serialize_field("resolution", &format!("{}x{}", self.width, self.height))?;
    state.serialize_field("display_mode", &self.display_mode)?;
    state.serialize_field("vsync", &self.vsync)?;
    state.end()
  }
}

impl<'de> Deserialize<'de> for DisplaySettings {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    #[derive(Deserialize)]
    struct Helper {
      resolution: Option<String>,
      // Legacy fields for backward compat with old system.json
      width: Option<u32>,
      height: Option<u32>,
      #[serde(default)]
      display_mode: DisplayMode,
      #[serde(default = "default_vsync")]
      vsync: bool,
    }

    fn default_vsync() -> bool {
      true
    }

    let helper = Helper::deserialize(deserializer)?;

    let (width, height) = if let Some(ref res) = helper.resolution {
      parse_resolution(res).unwrap_or_else(|| {
        eprintln!("[ryo2d] Warning: Malformed resolution '{}' in system.json, using engine defaults", res);
        (1280, 720)
      })
    } else if let (Some(w), Some(h)) = (helper.width, helper.height) {
      // Legacy format
      (w, h)
    } else {
      (1280, 720)
    };

    Ok(DisplaySettings {
      width,
      height,
      display_mode: helper.display_mode,
      vsync: helper.vsync,
    })
  }
}

impl Default for DisplaySettings {
  fn default() -> Self {
    DisplaySettings {
      width: 1280,
      height: 720,
      display_mode: DisplayMode::Windowed,
      vsync: true,
    }
  }
}

impl Default for SystemState {
  fn default() -> Self {
    SystemState {
      master_volume: 0.8,
      music_volume: 1.0,
      sfx_volume: 1.0,
      display: DisplaySettings::default(),
      show_fps: None,
      vsync: None,
      env: HashMap::new(),
    }
  }
}

impl SystemState {
  /// Create with sensible defaults
  pub fn new() -> Self {
    Self::default()
  }

  /// Create from SystemDefaults (Ryo.toml [system] + [info] sections).
  ///
  /// Used when creating a new system.json file from developer defaults.
  pub fn from_defaults(defaults: &super::workspace::SystemDefaults) -> Self {
    SystemState {
      master_volume: defaults.master_volume,
      music_volume: defaults.music_volume,
      sfx_volume: defaults.sfx_volume,
      display: DisplaySettings {
        width: defaults.resolution.map(|(w, _)| w).unwrap_or(1280),
        height: defaults.resolution.map(|(_, h)| h).unwrap_or(720),
        display_mode: defaults.display_mode,
        vsync: defaults.vsync,
      },
      // show_fps comes from [info] section via SystemDefaults
      show_fps: Some(defaults.show_fps),
      vsync: None,
      env: defaults.env.clone(),
    }
  }

  /// Merge missing fields from defaults.
  ///
  /// This is used when loading an existing system.json that may be missing
  /// fields added in newer versions. Missing fields are filled from the
  /// provided defaults.
  ///
  /// Returns true if any fields were updated.
  pub fn merge_defaults(&mut self, defaults: &super::workspace::SystemDefaults) -> bool {
    // Currently all fields have values, but this method exists for future
    // fields that may be added. The pattern is:
    // - If a field is missing/None and defaults has a value, apply it
    // - Return true if anything changed

    let mut changed = false;

    // If display matches engine defaults but Ryo.toml specifies a different resolution,
    // update to use Ryo.toml values (one-time migration)
    if let Some((w, h)) = defaults.resolution {
      if self.display.width == 1280
        && self.display.height == 720
        && (w != 1280 || h != 720)
      {
        self.display.width = w;
        self.display.height = h;
        changed = true;
      }
    }

    // Add new env keys from defaults without overwriting existing
    for (key, value) in &defaults.env {
      if !self.env.contains_key(key) {
        self.env.insert(key.clone(), value.clone());
        changed = true;
      }
    }

    changed
  }

  /// Set a custom env key-value pair.
  pub fn set_env(&mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) {
    self.env.insert(key.into(), value.into());
  }

  /// Get an env value by key.
  pub fn get_env(&self, key: &str) -> Option<&serde_json::Value> {
    self.env.get(key)
  }

  /// Get an env value as a string.
  pub fn get_env_str(&self, key: &str) -> Option<&str> {
    self.env.get(key).and_then(|v| v.as_str())
  }

  /// Get an env value as a bool.
  pub fn get_env_bool(&self, key: &str) -> Option<bool> {
    self.env.get(key).and_then(|v| v.as_bool())
  }

  /// Get an env value as an i64.
  pub fn get_env_i64(&self, key: &str) -> Option<i64> {
    self.env.get(key).and_then(|v| v.as_i64())
  }

  /// Get an env value as an f64.
  pub fn get_env_f64(&self, key: &str) -> Option<f64> {
    self.env.get(key).and_then(|v| v.as_f64())
  }
}

// ============================================================================
// Tier 2: World State
// ============================================================================

/// Serializable component data for ECS snapshots.
///
/// Components that should be saved must implement `Serialize + DeserializeOwned`.
/// Not all components are serializable (e.g., sprites contain raw pixel data).
///
/// This struct holds serialized component data keyed by type name.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EntitySnapshot {
  /// Entity identifier (for reference purposes)
  pub id: u64,
  /// Component data keyed by component type name
  pub components: HashMap<String, Vec<u8>>,
}

/// World state snapshot for ECS serialization.
///
/// Captures all entities and their serializable components.
/// Non-serializable components (sprites, handles) are excluded.
///
/// World state is saved to `{save_dir}/{slot}.ryo-world`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorldState {
  /// All entity snapshots
  pub entities: Vec<EntitySnapshot>,
  /// Custom world-level metadata
  #[serde(default)]
  pub metadata: HashMap<String, String>,
}

impl WorldState {
  /// Create an empty world state
  pub fn new() -> Self {
    WorldState::default()
  }

  /// Set world metadata
  pub fn set_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
    self.metadata.insert(key.into(), value.into());
  }

  /// Get world metadata
  pub fn get_metadata(&self, key: &str) -> Option<&str> {
    self.metadata.get(key).map(|s| s.as_str())
  }

  /// Add an entity snapshot
  pub fn add_entity(&mut self, snapshot: EntitySnapshot) {
    self.entities.push(snapshot);
  }

  /// Number of entities in the snapshot
  pub fn entity_count(&self) -> usize {
    self.entities.len()
  }
}

// ============================================================================
// Tier 3: Game Data
// ============================================================================

/// Generic game data wrapper with metadata.
///
/// This tier is for game-specific save data that doesn't fit into
/// system settings or ECS snapshots:
/// - Player progress
/// - Inventory
/// - Quest state
/// - Achievements
///
/// Game data is saved to `{save_dir}/{slot}.ryo-save`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSaveData<T> {
  /// The actual game data
  pub data: T,
  /// Save timestamp (Unix epoch seconds)
  pub timestamp: u64,
  /// Optional save name/description
  pub name: Option<String>,
  /// Play time in seconds
  pub play_time: u64,
  /// Additional metadata
  #[serde(default)]
  pub metadata: HashMap<String, String>,
}

impl<T> GameSaveData<T> {
  /// Create a new game save wrapper
  pub fn new(data: T) -> Self {
    GameSaveData {
      data,
      timestamp: std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs(),
      name: None,
      play_time: 0,
      metadata: HashMap::new(),
    }
  }

  /// Set a custom name for this save
  pub fn with_name(mut self, name: impl Into<String>) -> Self {
    self.name = Some(name.into());
    self
  }

  /// Set the play time for this save
  pub fn with_play_time(mut self, seconds: u64) -> Self {
    self.play_time = seconds;
    self
  }

  /// Set metadata for this save
  pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
    self.metadata.insert(key.into(), value.into());
    self
  }
}

// ============================================================================
// Save Manager
// ============================================================================

/// Manages all save/load operations for the engine.
///
/// Provides a unified interface for all three save tiers with
/// automatic compression and file format handling.
///
/// # Save Directory Structure
///
/// ```text
/// {workspace}/data/
/// ├── system.json          # Tier 1: User settings (uncompressed JSON)
/// ├── slot1.ryo-world      # Tier 2: World snapshots (compressed)
/// ├── slot1.ryo-save       # Tier 3: Game data (compressed)
/// ├── slot2.ryo-world
/// ├── slot2.ryo-save
/// └── ...
/// ```
#[derive(Debug)]
pub struct SaveManager {
  /// Base directory for saves
  save_dir: PathBuf,
  /// Compression level (0-9)
  compression_level: u32,
}

impl SaveManager {
  /// Create a new save manager with the given save directory.
  ///
  /// The directory will be created if it doesn't exist.
  pub fn new(save_dir: impl Into<PathBuf>) -> SaveResult<Self> {
    let save_dir = save_dir.into();
    fs::create_dir_all(&save_dir)?;
    Ok(SaveManager {
      save_dir,
      compression_level: 6,
    })
  }

  /// Set the compression level (0-9).
  ///
  /// - 0: No compression
  /// - 1-3: Fast compression
  /// - 4-6: Balanced (default is 6)
  /// - 7-9: Best compression
  pub fn set_compression_level(&mut self, level: u32) {
    self.compression_level = level.min(9);
  }

  /// Get the current save directory.
  pub fn save_dir(&self) -> &Path {
    &self.save_dir
  }

  /// Change the save directory at runtime.
  ///
  /// The new directory will be created if it doesn't exist.
  pub fn set_save_dir(&mut self, path: impl Into<PathBuf>) -> SaveResult<()> {
    let new_dir = path.into();
    fs::create_dir_all(&new_dir)?;
    self.save_dir = new_dir;
    Ok(())
  }

  // ==========================================================================
  // Tier 1: System State (JSON, uncompressed)
  // ==========================================================================

  /// Save system state as human-editable JSON.
  ///
  /// When `system.json` already exists, only keys present in the existing
  /// file are updated — preserving the filtered structure created by
  /// [`load_or_create_system_filtered`](Self::load_or_create_system_filtered).
  /// When no file exists, writes the full state.
  pub fn save_system(&self, state: &SystemState) -> SaveResult<()> {
    let path = self.save_dir.join(format!("system.{}", SYSTEM_EXTENSION));

    if path.exists() {
      // Merge: only update keys already present in the file
      let existing_json = fs::read_to_string(&path)?;
      if let Ok(serde_json::Value::Object(existing_map)) =
        serde_json::from_str::<serde_json::Value>(&existing_json)
      {
        let new_value = serde_json::to_value(state)
          .map_err(|e| SaveError::Serialization(e.to_string()))?;
        if let serde_json::Value::Object(new_map) = new_value {
          let mut merged = serde_json::Map::new();
          for (key, _) in &existing_map {
            if let Some(new_val) = new_map.get(key) {
              merged.insert(key.clone(), new_val.clone());
            }
          }
          let json = serde_json::to_string_pretty(&serde_json::Value::Object(merged))
            .map_err(|e| SaveError::Serialization(e.to_string()))?;
          let mut file = File::create(&path)?;
          file.write_all(json.as_bytes())?;
          file.flush()?;
          return Ok(());
        }
      }
    }

    // No existing file or couldn't parse — write full state
    self.write_json_file(&path, state)
  }

  /// Load system state from JSON file.
  ///
  /// Returns default state if no save file exists.
  pub fn load_system(&self) -> SaveResult<SystemState> {
    let path = self.save_dir.join(format!("system.{}", SYSTEM_EXTENSION));
    if !path.exists() {
      return Ok(SystemState::new());
    }
    self.read_json_file(&path)
  }

  /// Load or create system state with proper fallback chain.
  ///
  /// This is the primary method for loading user configuration with the
  /// correct priority chain:
  ///
  /// 1. **system.json** (user settings) - highest priority
  /// 2. **Ryo.toml [system]** (developer defaults) - middle priority
  /// 3. **Engine defaults** - lowest priority (fallback)
  ///
  /// Behavior:
  /// - If system.json doesn't exist, creates it from Ryo.toml [system] + engine defaults
  /// - If system.json exists but is missing fields, merges from Ryo.toml [system]
  /// - Returns the loaded/created state
  ///
  /// # Arguments
  ///
  /// * `defaults` - Developer defaults from Ryo.toml [system] section
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// let workspace = Workspace::discover()?;
  /// let save_manager = workspace.create_save_manager()?;
  /// let system_state = save_manager.load_or_create_system(&workspace.config().system)?;
  /// ```
  pub fn load_or_create_system(
    &self,
    defaults: &super::workspace::SystemDefaults,
  ) -> SaveResult<SystemState> {
    let path = self.save_dir.join(format!("system.{}", SYSTEM_EXTENSION));

    if !path.exists() {
      // Create new system.json from Ryo.toml [system] defaults
      let state = SystemState::from_defaults(defaults);
      self.save_system(&state)?;
      return Ok(state);
    }

    // Load existing and merge any missing fields
    let mut state: SystemState = self.read_json_file(&path)?;

    if state.merge_defaults(defaults) {
      // Save updated state with merged fields
      self.save_system(&state)?;
    }

    Ok(state)
  }

  /// Load or create system state, writing only filtered fields to a new file.
  ///
  /// Behaves like [`load_or_create_system`](Self::load_or_create_system) when
  /// system.json already exists. When creating a new file, only the JSON keys
  /// corresponding to the given filter are written — so a volume-only filter
  /// produces a system.json with just `master_volume`, `music_volume`, and
  /// `sfx_volume`.
  ///
  /// The returned `SystemState` is always fully populated (from defaults).
  pub fn load_or_create_system_filtered(
    &self,
    defaults: &super::workspace::SystemDefaults,
    filter: &UserPreferenceFilter,
  ) -> SaveResult<SystemState> {
    let path = self.save_dir.join(format!("system.{}", SYSTEM_EXTENSION));

    if path.exists() {
      // Existing file — load as-is and merge defaults (same as unfiltered)
      let mut state: SystemState = self.read_json_file(&path)?;
      if state.merge_defaults(defaults) {
        self.save_system(&state)?;
      }
      return Ok(state);
    }

    // Build full state from defaults
    let state = SystemState::from_defaults(defaults);

    // Serialize to JSON Value, then strip keys not requested by the filter
    let full_value = serde_json::to_value(&state)
      .map_err(|e| SaveError::Serialization(e.to_string()))?;

    if let serde_json::Value::Object(map) = full_value {
      let mut filtered = serde_json::Map::new();

      for (key, value) in &map {
        let dominated_by_filter = match key.as_str() {
          "master_volume" => filter.contains(UserPreference::MasterVolume),
          "music_volume" => filter.contains(UserPreference::MusicVolume),
          "sfx_volume" => filter.contains(UserPreference::SfxVolume),
          "display" => {
            filter.contains(UserPreference::Resolution)
              || filter.contains(UserPreference::DisplayMode)
              || filter.contains(UserPreference::Vsync)
          }
          "vsync" => filter.contains(UserPreference::Vsync),
          "show_fps" => filter.contains(UserPreference::ShowFps),
          "env" => !defaults.env.is_empty(),
          _ => false,
        };

        if dominated_by_filter {
          filtered.insert(key.clone(), value.clone());
        }
      }

      let filtered_json = serde_json::to_string_pretty(&serde_json::Value::Object(filtered))
        .map_err(|e| SaveError::Serialization(e.to_string()))?;
      let mut file = File::create(&path)?;
      file.write_all(filtered_json.as_bytes())?;
      file.flush()?;
    } else {
      // Fallback: write full state (shouldn't happen for a struct)
      self.save_system(&state)?;
    }

    Ok(state)
  }

  /// Check if system state exists.
  pub fn has_system_save(&self) -> bool {
    self
      .save_dir
      .join(format!("system.{}", SYSTEM_EXTENSION))
      .exists()
  }

  // ==========================================================================
  // Tier 2: World State
  // ==========================================================================

  /// Save world state to a named slot.
  pub fn save_world(&self, slot: &str, state: &WorldState) -> SaveResult<()> {
    self.validate_slot_name(slot)?;
    let path = self.save_dir.join(format!("{}.{}", slot, WORLD_EXTENSION));
    self.write_save_file(&path, state)
  }

  /// Load world state from a named slot.
  pub fn load_world(&self, slot: &str) -> SaveResult<WorldState> {
    self.validate_slot_name(slot)?;
    let path = self.save_dir.join(format!("{}.{}", slot, WORLD_EXTENSION));
    if !path.exists() {
      return Err(SaveError::NotFound(path));
    }
    self.read_save_file(&path)
  }

  /// Check if a world save exists.
  pub fn has_world_save(&self, slot: &str) -> bool {
    self
      .save_dir
      .join(format!("{}.{}", slot, WORLD_EXTENSION))
      .exists()
  }

  /// Delete a world save.
  pub fn delete_world(&self, slot: &str) -> SaveResult<()> {
    self.validate_slot_name(slot)?;
    let path = self.save_dir.join(format!("{}.{}", slot, WORLD_EXTENSION));
    if path.exists() {
      fs::remove_file(&path)?;
    }
    Ok(())
  }

  // ==========================================================================
  // Tier 3: Game Data
  // ==========================================================================

  /// Save game data to a named slot.
  ///
  /// The data is wrapped in `GameSaveData` which adds metadata
  /// like timestamp and play time.
  pub fn save_game<T: Serialize>(&self, slot: &str, data: &T) -> SaveResult<()> {
    self.save_game_with_info(slot, GameSaveData::new(data))
  }

  /// Save game data with full metadata control.
  pub fn save_game_with_info<T: Serialize>(
    &self,
    slot: &str,
    save: GameSaveData<&T>,
  ) -> SaveResult<()> {
    self.validate_slot_name(slot)?;
    let path = self.save_dir.join(format!("{}.{}", slot, GAME_EXTENSION));
    self.write_save_file(&path, &save)
  }

  /// Load game data from a named slot.
  pub fn load_game<T: DeserializeOwned>(&self, slot: &str) -> SaveResult<GameSaveData<T>> {
    self.validate_slot_name(slot)?;
    let path = self.save_dir.join(format!("{}.{}", slot, GAME_EXTENSION));
    if !path.exists() {
      return Err(SaveError::NotFound(path));
    }
    self.read_save_file(&path)
  }

  /// Check if a game save exists.
  pub fn has_game_save(&self, slot: &str) -> bool {
    self
      .save_dir
      .join(format!("{}.{}", slot, GAME_EXTENSION))
      .exists()
  }

  /// Delete a game save.
  pub fn delete_game(&self, slot: &str) -> SaveResult<()> {
    self.validate_slot_name(slot)?;
    let path = self.save_dir.join(format!("{}.{}", slot, GAME_EXTENSION));
    if path.exists() {
      fs::remove_file(&path)?;
    }
    Ok(())
  }

  /// List all game save slots.
  pub fn list_game_saves(&self) -> SaveResult<Vec<String>> {
    self.list_saves_with_extension(GAME_EXTENSION)
  }

  /// List all world save slots.
  pub fn list_world_saves(&self) -> SaveResult<Vec<String>> {
    self.list_saves_with_extension(WORLD_EXTENSION)
  }

  // ==========================================================================
  // Raw Data API (for advanced use)
  // ==========================================================================

  /// Save arbitrary serializable data to a custom file.
  ///
  /// Use this for custom save formats not covered by the 3 tiers.
  pub fn save_raw<T: Serialize>(&self, filename: &str, data: &T) -> SaveResult<()> {
    let path = self.save_dir.join(filename);
    self.write_save_file(&path, data)
  }

  /// Load arbitrary data from a custom file.
  pub fn load_raw<T: DeserializeOwned>(&self, filename: &str) -> SaveResult<T> {
    let path = self.save_dir.join(filename);
    if !path.exists() {
      return Err(SaveError::NotFound(path));
    }
    self.read_save_file(&path)
  }

  // ==========================================================================
  // Internal Helpers
  // ==========================================================================

  /// Write data as pretty-printed JSON (for Tier 1: SystemState)
  fn write_json_file<T: Serialize>(&self, path: &Path, data: &T) -> SaveResult<()> {
    let json =
      serde_json::to_string_pretty(data).map_err(|e| SaveError::Serialization(e.to_string()))?;
    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;
    file.flush()?;
    Ok(())
  }

  /// Read data from JSON file (for Tier 1: SystemState)
  fn read_json_file<T: DeserializeOwned>(&self, path: &Path) -> SaveResult<T> {
    let json = fs::read_to_string(path)?;
    let data =
      serde_json::from_str(&json).map_err(|e| SaveError::Deserialization(e.to_string()))?;
    Ok(data)
  }

  /// Write compressed save file with header (for Tiers 2-3)
  fn write_save_file<T: Serialize>(&self, path: &Path, data: &T) -> SaveResult<()> {
    // Serialize to RON format
    let serialized = ron::to_string(data).map_err(|e| SaveError::Serialization(e.to_string()))?;

    // Compress the serialized data
    let compressed = self.compress(serialized.as_bytes())?;

    // Write with header
    let mut file = File::create(path)?;
    file.write_all(MAGIC)?;
    file.write_all(&SAVE_VERSION.to_le_bytes())?;
    file.write_all(&compressed)?;
    file.flush()?;

    Ok(())
  }

  /// Read compressed save file with header verification (for Tiers 2-3)
  fn read_save_file<T: DeserializeOwned>(&self, path: &Path) -> SaveResult<T> {
    let mut file = File::open(path)?;

    // Read and verify magic bytes
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    if &magic != MAGIC {
      return Err(SaveError::Deserialization(
        "Invalid save file format".into(),
      ));
    }

    // Read and verify version
    let mut version_bytes = [0u8; 4];
    file.read_exact(&mut version_bytes)?;
    let version = u32::from_le_bytes(version_bytes);
    if version != SAVE_VERSION {
      return Err(SaveError::VersionMismatch {
        expected: SAVE_VERSION,
        found: version,
      });
    }

    // Read compressed data
    let mut compressed = Vec::new();
    file.read_to_end(&mut compressed)?;

    // Decompress
    let decompressed = self.decompress(&compressed)?;

    // Deserialize from RON
    let data = ron::from_str(&String::from_utf8_lossy(&decompressed))
      .map_err(|e| SaveError::Deserialization(e.to_string()))?;

    Ok(data)
  }

  fn compress(&self, data: &[u8]) -> SaveResult<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(self.compression_level));
    encoder
      .write_all(data)
      .map_err(|e| SaveError::Compression(e.to_string()))?;
    encoder
      .finish()
      .map_err(|e| SaveError::Compression(e.to_string()))
  }

  fn decompress(&self, data: &[u8]) -> SaveResult<Vec<u8>> {
    let mut decoder = DeflateDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder
      .read_to_end(&mut decompressed)
      .map_err(|e| SaveError::Decompression(e.to_string()))?;
    Ok(decompressed)
  }

  fn validate_slot_name(&self, slot: &str) -> SaveResult<()> {
    // Prevent path traversal and invalid characters
    if slot.is_empty()
      || slot.contains('/')
      || slot.contains('\\')
      || slot.contains("..")
      || slot.starts_with('.')
    {
      return Err(SaveError::InvalidSlot(slot.to_string()));
    }
    Ok(())
  }

  fn list_saves_with_extension(&self, ext: &str) -> SaveResult<Vec<String>> {
    let mut saves = Vec::new();
    for entry in fs::read_dir(&self.save_dir)? {
      let entry = entry?;
      let path = entry.path();
      if let Some(file_ext) = path.extension()
        && file_ext == ext
        && let Some(stem) = path.file_stem()
        && let Some(name) = stem.to_str()
      {
        saves.push(name.to_string());
      }
    }
    saves.sort();
    Ok(saves)
  }
}

// ============================================================================
// Convenience Functions
// ============================================================================

/// Compress data using DEFLATE compression.
///
/// Useful for manual compression of game data outside the save system.
pub fn compress_data(data: &[u8]) -> SaveResult<Vec<u8>> {
  let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
  encoder
    .write_all(data)
    .map_err(|e| SaveError::Compression(e.to_string()))?;
  encoder
    .finish()
    .map_err(|e| SaveError::Compression(e.to_string()))
}

/// Decompress DEFLATE compressed data.
pub fn decompress_data(data: &[u8]) -> SaveResult<Vec<u8>> {
  let mut decoder = DeflateDecoder::new(data);
  let mut decompressed = Vec::new();
  decoder
    .read_to_end(&mut decompressed)
    .map_err(|e| SaveError::Decompression(e.to_string()))?;
  Ok(decompressed)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::tempdir;

  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  struct TestGameData {
    level: u32,
    score: u64,
    player_name: String,
  }

  #[test]
  fn save_manager_creation_creates_directory() {
    let dir = tempdir().unwrap();
    let save_path = dir.path().join("saves");
    assert!(!save_path.exists());

    let _manager = SaveManager::new(&save_path).unwrap();
    assert!(save_path.exists());
  }

  #[test]
  fn system_state_save_and_load() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let mut state = SystemState::new();
    state.master_volume = 0.75;
    state.music_volume = 0.5;
    state.display.width = 1920;
    state.display.height = 1080;
    state.set_env("language", serde_json::Value::String("en".into()));

    manager.save_system(&state).unwrap();
    assert!(manager.has_system_save());

    let loaded = manager.load_system().unwrap();
    assert_eq!(loaded.master_volume, 0.75);
    assert_eq!(loaded.music_volume, 0.5);
    assert_eq!(loaded.display.width, 1920);
    assert_eq!(loaded.display.height, 1080);
    assert_eq!(loaded.get_env_str("language"), Some("en"));
  }

  #[test]
  fn system_state_load_returns_default_if_no_file() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let state = manager.load_system().unwrap();
    // Uses engine defaults: master=0.8, music=1.0
    assert_eq!(state.master_volume, 0.8);
    assert_eq!(state.music_volume, 1.0);
  }

  #[test]
  fn system_state_is_json_format() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let mut state = SystemState::new();
    state.master_volume = 0.75;
    state.display.width = 1920;
    state.display.height = 1080;
    state.set_env("language", serde_json::Value::String("en".into()));

    manager.save_system(&state).unwrap();

    // Read raw file and verify it's JSON (not compressed)
    let path = dir.path().join("system.json");
    let content = fs::read_to_string(&path).unwrap();

    // Should be valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["master_volume"], 0.75);
    assert_eq!(parsed["display"]["resolution"], "1920x1080");
    assert_eq!(parsed["env"]["language"], "en");

    // Should be human-readable (contains newlines from pretty-print)
    assert!(content.contains('\n'));
    assert!(content.contains("master_volume"));
  }

  #[test]
  fn world_state_save_and_load() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let mut world = WorldState::new();
    world.set_metadata("scene", "level_1");
    world.add_entity(EntitySnapshot {
      id: 42,
      components: HashMap::new(),
    });

    manager.save_world("slot1", &world).unwrap();
    assert!(manager.has_world_save("slot1"));

    let loaded = manager.load_world("slot1").unwrap();
    assert_eq!(loaded.entity_count(), 1);
    assert_eq!(loaded.get_metadata("scene"), Some("level_1"));
    assert_eq!(loaded.entities[0].id, 42);
  }

  #[test]
  fn game_data_save_and_load() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let data = TestGameData {
      level: 5,
      score: 12500,
      player_name: "Alice".to_string(),
    };

    manager.save_game("save1", &data).unwrap();
    assert!(manager.has_game_save("save1"));

    let loaded: GameSaveData<TestGameData> = manager.load_game("save1").unwrap();
    assert_eq!(loaded.data, data);
    assert!(loaded.timestamp > 0);
  }

  #[test]
  fn game_data_with_metadata() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let data = TestGameData {
      level: 10,
      score: 50000,
      player_name: "Bob".to_string(),
    };

    let save = GameSaveData::new(&data)
      .with_name("My Best Run")
      .with_play_time(3600)
      .with_metadata("difficulty", "hard");

    manager.save_game_with_info("save2", save).unwrap();

    let loaded: GameSaveData<TestGameData> = manager.load_game("save2").unwrap();
    assert_eq!(loaded.name, Some("My Best Run".to_string()));
    assert_eq!(loaded.play_time, 3600);
    assert_eq!(loaded.metadata.get("difficulty"), Some(&"hard".to_string()));
  }

  #[test]
  fn list_saves() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let data = TestGameData {
      level: 1,
      score: 100,
      player_name: "Test".to_string(),
    };

    manager.save_game("slot_a", &data).unwrap();
    manager.save_game("slot_b", &data).unwrap();
    manager.save_game("slot_c", &data).unwrap();

    let saves = manager.list_game_saves().unwrap();
    assert_eq!(saves, vec!["slot_a", "slot_b", "slot_c"]);
  }

  #[test]
  fn delete_saves() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let data = TestGameData {
      level: 1,
      score: 100,
      player_name: "Test".to_string(),
    };

    manager.save_game("to_delete", &data).unwrap();
    assert!(manager.has_game_save("to_delete"));

    manager.delete_game("to_delete").unwrap();
    assert!(!manager.has_game_save("to_delete"));
  }

  #[test]
  fn invalid_slot_names_rejected() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let data = TestGameData {
      level: 1,
      score: 100,
      player_name: "Test".to_string(),
    };

    assert!(manager.save_game("", &data).is_err());
    assert!(manager.save_game("../escape", &data).is_err());
    assert!(manager.save_game("path/traversal", &data).is_err());
    assert!(manager.save_game(".hidden", &data).is_err());
  }

  #[test]
  fn compression_decompression_roundtrip() {
    let original = b"Hello, World! This is some test data for compression.";
    let compressed = compress_data(original).unwrap();
    let decompressed = decompress_data(&compressed).unwrap();
    assert_eq!(decompressed, original);
  }

  #[test]
  fn load_nonexistent_file_returns_not_found() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let result: SaveResult<GameSaveData<TestGameData>> = manager.load_game("nonexistent");
    assert!(matches!(result, Err(SaveError::NotFound(_))));
  }

  #[test]
  fn raw_data_save_and_load() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let data = vec![1u32, 2, 3, 4, 5];
    manager.save_raw("custom.dat", &data).unwrap();

    let loaded: Vec<u32> = manager.load_raw("custom.dat").unwrap();
    assert_eq!(loaded, data);
  }

  #[test]
  fn set_save_dir_changes_location() {
    let dir = tempdir().unwrap();
    let mut manager = SaveManager::new(dir.path().join("original")).unwrap();

    let new_dir = dir.path().join("new_location");
    manager.set_save_dir(&new_dir).unwrap();

    assert_eq!(manager.save_dir(), new_dir);
    assert!(new_dir.exists());
  }

  // ==========================================================================
  // SystemDefaults and from_defaults tests
  // ==========================================================================

  #[test]
  fn system_state_from_defaults_uses_all_fields() {
    let mut env = HashMap::new();
    env.insert("difficulty".to_string(), serde_json::json!("hard"));

    let defaults = super::super::workspace::SystemDefaults {
      resolution: Some((1920, 1080)),
      vsync: false,
      display_mode: DisplayMode::ExclusiveFullscreen,
      master_volume: 0.5,
      music_volume: 0.7,
      sfx_volume: 0.9,
      show_fps: true,
      dev_console: false,
      env,
    };

    let state = SystemState::from_defaults(&defaults);

    assert_eq!(state.display.width, 1920);
    assert_eq!(state.display.height, 1080);
    assert!(!state.display.vsync);
    assert_eq!(state.display.display_mode, DisplayMode::ExclusiveFullscreen);
    assert_eq!(state.master_volume, 0.5);
    assert_eq!(state.music_volume, 0.7);
    assert_eq!(state.sfx_volume, 0.9);
    assert_eq!(state.show_fps, Some(true));
    assert_eq!(state.get_env_str("difficulty"), Some("hard"));
  }

  #[test]
  fn system_state_from_defaults_with_engine_defaults() {
    let defaults = super::super::workspace::SystemDefaults::default();

    let state = SystemState::from_defaults(&defaults);

    // Engine defaults: 1280x720, vsync=true, windowed mode
    assert_eq!(state.display.width, 1280);
    assert_eq!(state.display.height, 720);
    assert!(state.display.vsync);
    assert_eq!(state.display.display_mode, DisplayMode::Windowed);
    // Engine defaults: master=0.8, music=1.0, sfx=1.0
    assert_eq!(state.master_volume, 0.8);
    assert_eq!(state.music_volume, 1.0);
    assert_eq!(state.sfx_volume, 1.0);
    assert_eq!(state.show_fps, Some(false));
  }

  #[test]
  fn system_state_merge_defaults_no_change_when_matching() {
    let defaults = super::super::workspace::SystemDefaults::default();
    let mut state = SystemState::from_defaults(&defaults);

    // No change when state already matches defaults
    let changed = state.merge_defaults(&defaults);
    assert!(!changed);
  }

  #[test]
  fn system_state_merge_defaults_migrates_resolution() {
    let engine_defaults = super::super::workspace::SystemDefaults::default();

    // State with engine default resolution
    let mut state = SystemState::from_defaults(&engine_defaults);
    assert_eq!(state.display.width, 1280);
    assert_eq!(state.display.height, 720);

    // Developer wants different resolution in Ryo.toml
    let dev_defaults = super::super::workspace::SystemDefaults {
      resolution: Some((800, 600)),
      ..engine_defaults
    };

    // Merge should update resolution since it was at engine defaults
    let changed = state.merge_defaults(&dev_defaults);
    assert!(changed);
    assert_eq!(state.display.width, 800);
    assert_eq!(state.display.height, 600);
  }

  #[test]
  fn system_state_merge_defaults_preserves_user_changes() {
    let engine_defaults = super::super::workspace::SystemDefaults::default();

    // User modified their resolution (not at engine defaults)
    let mut state = SystemState::new();
    state.display.width = 1600;
    state.display.height = 900;

    // Developer has different defaults
    let dev_defaults = super::super::workspace::SystemDefaults {
      resolution: Some((800, 600)),
      ..engine_defaults
    };

    // Merge should NOT change user's custom resolution
    let changed = state.merge_defaults(&dev_defaults);
    assert!(!changed);
    assert_eq!(state.display.width, 1600);
    assert_eq!(state.display.height, 900);
  }

  #[test]
  fn display_settings_default_uses_engine_defaults() {
    let settings = DisplaySettings::default();
    assert_eq!(settings.width, 1280);
    assert_eq!(settings.height, 720);
    assert!(settings.vsync);
    assert_eq!(settings.display_mode, DisplayMode::Windowed);
  }

  // ==========================================================================
  // load_or_create_system tests
  // ==========================================================================

  #[test]
  fn load_or_create_system_creates_new_file() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let defaults = super::super::workspace::SystemDefaults {
      resolution: Some((1024, 768)),
      vsync: true,
      display_mode: DisplayMode::Windowed,
      master_volume: 0.9,
      music_volume: 0.8,
      sfx_volume: 0.7,
      show_fps: true,
      dev_console: false,
      env: HashMap::new(),
    };

    assert!(!manager.has_system_save());

    let state = manager.load_or_create_system(&defaults).unwrap();

    assert!(manager.has_system_save());
    assert_eq!(state.display.width, 1024);
    assert_eq!(state.display.height, 768);
    assert_eq!(state.master_volume, 0.9);
    assert_eq!(state.show_fps, Some(true));
  }

  #[test]
  fn load_or_create_system_loads_existing_file() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    // Create initial state with custom values
    let mut initial = SystemState::new();
    initial.master_volume = 0.42;
    initial.display.width = 1600;
    initial.display.height = 900;
    manager.save_system(&initial).unwrap();

    // load_or_create should load existing, not overwrite
    let defaults = super::super::workspace::SystemDefaults::default();
    let state = manager.load_or_create_system(&defaults).unwrap();

    assert_eq!(state.master_volume, 0.42);
    assert_eq!(state.display.width, 1600);
    assert_eq!(state.display.height, 900);
  }

  #[test]
  fn load_or_create_system_merges_missing_fields() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    // Create state with engine default resolution
    let engine_defaults = super::super::workspace::SystemDefaults::default();
    let initial = SystemState::from_defaults(&engine_defaults);
    manager.save_system(&initial).unwrap();

    // Developer later changes resolution in Ryo.toml
    let dev_defaults = super::super::workspace::SystemDefaults {
      resolution: Some((800, 600)),
      ..engine_defaults
    };

    // load_or_create should merge the new defaults
    let state = manager.load_or_create_system(&dev_defaults).unwrap();

    // Resolution should be updated (migrated from engine defaults)
    assert_eq!(state.display.width, 800);
    assert_eq!(state.display.height, 600);
  }

  // ==========================================================================
  // parse_resolution tests
  // ==========================================================================

  #[test]
  fn parse_resolution_valid_strings() {
    assert_eq!(parse_resolution("1280x720"), Some((1280, 720)));
    assert_eq!(parse_resolution("800x600"), Some((800, 600)));
    assert_eq!(parse_resolution("1920x1080"), Some((1920, 1080)));
    assert_eq!(parse_resolution("3840x2160"), Some((3840, 2160)));
    assert_eq!(parse_resolution("320x240"), Some((320, 240)));
  }

  #[test]
  fn parse_resolution_with_whitespace() {
    assert_eq!(parse_resolution(" 800 x 600 "), Some((800, 600)));
    assert_eq!(parse_resolution("1280 x 720"), Some((1280, 720)));
  }

  #[test]
  fn parse_resolution_malformed_returns_none() {
    assert_eq!(parse_resolution(""), None);
    assert_eq!(parse_resolution("1280"), None);
    assert_eq!(parse_resolution("1280x"), None);
    assert_eq!(parse_resolution("x720"), None);
    assert_eq!(parse_resolution("1280x720x60"), None);
    assert_eq!(parse_resolution("widexhigh"), None);
    assert_eq!(parse_resolution("1280*720"), None);
    assert_eq!(parse_resolution("-1x720"), None);
  }

  // ==========================================================================
  // DisplaySettings serde tests
  // ==========================================================================

  #[test]
  fn display_settings_serializes_resolution_string() {
    let settings = DisplaySettings {
      width: 1920,
      height: 1080,
      display_mode: DisplayMode::Windowed,
      vsync: true,
    };

    let json = serde_json::to_string(&settings).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["resolution"], "1920x1080");
    // Should NOT have separate width/height fields
    assert!(parsed.get("width").is_none());
    assert!(parsed.get("height").is_none());
  }

  #[test]
  fn display_settings_deserializes_resolution_string() {
    let json = r#"{"resolution": "800x600", "display_mode": "windowed", "vsync": false}"#;
    let settings: DisplaySettings = serde_json::from_str(json).unwrap();

    assert_eq!(settings.width, 800);
    assert_eq!(settings.height, 600);
    assert!(!settings.vsync);
    assert_eq!(settings.display_mode, DisplayMode::Windowed);
  }

  #[test]
  fn display_settings_roundtrip_serde() {
    let original = DisplaySettings {
      width: 2560,
      height: 1440,
      display_mode: DisplayMode::BorderlessFullscreen,
      vsync: true,
    };

    let json = serde_json::to_string(&original).unwrap();
    let deserialized: DisplaySettings = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.width, original.width);
    assert_eq!(deserialized.height, original.height);
    assert_eq!(deserialized.display_mode, original.display_mode);
    assert_eq!(deserialized.vsync, original.vsync);
  }

  #[test]
  fn display_settings_deserializes_legacy_width_height() {
    // Old system.json format with separate width/height fields
    let json = r#"{"width": 1600, "height": 900, "display_mode": "windowed", "vsync": true}"#;
    let settings: DisplaySettings = serde_json::from_str(json).unwrap();

    assert_eq!(settings.width, 1600);
    assert_eq!(settings.height, 900);
  }

  #[test]
  fn display_settings_resolution_takes_priority_over_legacy() {
    // If both resolution string and legacy width/height exist, resolution wins
    let json = r#"{"resolution": "1920x1080", "width": 800, "height": 600, "display_mode": "windowed", "vsync": true}"#;
    let settings: DisplaySettings = serde_json::from_str(json).unwrap();

    assert_eq!(settings.width, 1920);
    assert_eq!(settings.height, 1080);
  }

  #[test]
  fn display_settings_malformed_resolution_falls_back_to_defaults() {
    let json = r#"{"resolution": "not_valid", "display_mode": "windowed", "vsync": true}"#;
    let settings: DisplaySettings = serde_json::from_str(json).unwrap();

    // Falls back to engine defaults
    assert_eq!(settings.width, 1280);
    assert_eq!(settings.height, 720);
  }

  #[test]
  fn display_settings_missing_resolution_falls_back_to_defaults() {
    let json = r#"{"display_mode": "windowed", "vsync": true}"#;
    let settings: DisplaySettings = serde_json::from_str(json).unwrap();

    assert_eq!(settings.width, 1280);
    assert_eq!(settings.height, 720);
  }

  #[test]
  fn display_settings_vsync_defaults_true() {
    let json = r#"{"resolution": "800x600", "display_mode": "windowed"}"#;
    let settings: DisplaySettings = serde_json::from_str(json).unwrap();

    assert!(settings.vsync);
  }

  // ==========================================================================
  // Integration: system.json save/load roundtrip with new format
  // ==========================================================================

  #[test]
  fn system_state_save_load_roundtrip_new_resolution_format() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let mut state = SystemState::new();
    state.display.width = 1920;
    state.display.height = 1080;
    state.display.display_mode = DisplayMode::BorderlessFullscreen;
    state.master_volume = 0.42;

    manager.save_system(&state).unwrap();

    // Verify JSON on disk uses resolution string
    let path = dir.path().join("system.json");
    let content = std::fs::read_to_string(&path).unwrap();
    let raw: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(raw["display"]["resolution"], "1920x1080");
    assert!(raw["display"].get("width").is_none());

    // Load it back and verify values roundtrip
    let loaded = manager.load_system().unwrap();
    assert_eq!(loaded.display.width, 1920);
    assert_eq!(loaded.display.height, 1080);
    assert_eq!(loaded.display.display_mode, DisplayMode::BorderlessFullscreen);
    assert_eq!(loaded.master_volume, 0.42);
  }

  #[test]
  fn system_state_loads_legacy_system_json() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    // Write a legacy-format system.json directly to disk
    let legacy_json = r#"{
      "master_volume": 0.75,
      "music_volume": 1.0,
      "sfx_volume": 1.0,
      "display": {
        "width": 1600,
        "height": 900,
        "display_mode": "windowed",
        "vsync": true
      },
      "show_fps": false,
      "vsync": null,
      "env": {}
    }"#;
    let path = dir.path().join("system.json");
    std::fs::write(&path, legacy_json).unwrap();

    // Load should parse legacy width/height fields
    let loaded = manager.load_system().unwrap();
    assert_eq!(loaded.display.width, 1600);
    assert_eq!(loaded.display.height, 900);
    assert_eq!(loaded.master_volume, 0.75);
  }

  #[test]
  fn system_state_legacy_resaves_as_new_format() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    // Write legacy format
    let legacy_json = r#"{
      "master_volume": 0.8,
      "music_volume": 1.0,
      "sfx_volume": 1.0,
      "display": {
        "width": 1600,
        "height": 900,
        "display_mode": "windowed",
        "vsync": true
      },
      "show_fps": false,
      "vsync": null,
      "env": {}
    }"#;
    let path = dir.path().join("system.json");
    std::fs::write(&path, legacy_json).unwrap();

    // Load and re-save
    let loaded = manager.load_system().unwrap();
    manager.save_system(&loaded).unwrap();

    // Verify it was re-saved in new format
    let content = std::fs::read_to_string(&path).unwrap();
    let raw: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(raw["display"]["resolution"], "1600x900");
    assert!(raw["display"].get("width").is_none());
    assert!(raw["display"].get("height").is_none());
  }

  // ==========================================================================
  // UserPreference and UserPreferenceFilter tests
  // ==========================================================================

  #[test]
  fn user_preference_filter_all_contains_all_fields() {
    let filter = UserPreferenceFilter::all();

    assert!(filter.contains(UserPreference::MasterVolume));
    assert!(filter.contains(UserPreference::MusicVolume));
    assert!(filter.contains(UserPreference::SfxVolume));
    assert!(filter.contains(UserPreference::Resolution));
    assert!(filter.contains(UserPreference::DisplayMode));
    assert!(filter.contains(UserPreference::Vsync));
    assert!(filter.contains(UserPreference::ShowFps));
    assert_eq!(filter.len(), 7);
    assert!(!filter.is_empty());
  }

  #[test]
  fn user_preference_filter_none_contains_no_fields() {
    let filter = UserPreferenceFilter::none();

    assert!(!filter.contains(UserPreference::MasterVolume));
    assert!(!filter.contains(UserPreference::MusicVolume));
    assert!(!filter.contains(UserPreference::SfxVolume));
    assert!(!filter.contains(UserPreference::Resolution));
    assert!(!filter.contains(UserPreference::DisplayMode));
    assert!(!filter.contains(UserPreference::Vsync));
    assert!(!filter.contains(UserPreference::ShowFps));
    assert_eq!(filter.len(), 0);
    assert!(filter.is_empty());
  }

  #[test]
  fn user_preference_filter_only_contains_specified_fields() {
    let filter = UserPreferenceFilter::only([
      UserPreference::MasterVolume,
      UserPreference::MusicVolume,
      UserPreference::SfxVolume,
    ]);

    assert!(filter.contains(UserPreference::MasterVolume));
    assert!(filter.contains(UserPreference::MusicVolume));
    assert!(filter.contains(UserPreference::SfxVolume));
    assert!(!filter.contains(UserPreference::Resolution));
    assert!(!filter.contains(UserPreference::DisplayMode));
    assert!(!filter.contains(UserPreference::Vsync));
    assert!(!filter.contains(UserPreference::ShowFps));
    assert_eq!(filter.len(), 3);
  }

  #[test]
  fn user_preference_filter_from_array() {
    let filter: UserPreferenceFilter = [UserPreference::Resolution, UserPreference::DisplayMode].into();

    assert!(filter.contains(UserPreference::Resolution));
    assert!(filter.contains(UserPreference::DisplayMode));
    assert!(!filter.contains(UserPreference::MasterVolume));
    assert_eq!(filter.len(), 2);
  }

  #[test]
  fn user_preference_filter_from_vec() {
    let fields = vec![UserPreference::Vsync, UserPreference::ShowFps];
    let filter: UserPreferenceFilter = fields.into();

    assert!(filter.contains(UserPreference::Vsync));
    assert!(filter.contains(UserPreference::ShowFps));
    assert!(!filter.contains(UserPreference::Resolution));
    assert_eq!(filter.len(), 2);
  }

  #[test]
  fn user_preference_filter_default_is_all() {
    let filter = UserPreferenceFilter::default();

    assert_eq!(filter.len(), 7);
    assert!(filter.contains(UserPreference::MasterVolume));
    assert!(filter.contains(UserPreference::Resolution));
  }

  #[test]
  fn user_preference_all_returns_all_variants() {
    let all = UserPreference::all();

    assert_eq!(all.len(), 7);
    assert!(all.contains(&UserPreference::MasterVolume));
    assert!(all.contains(&UserPreference::MusicVolume));
    assert!(all.contains(&UserPreference::SfxVolume));
    assert!(all.contains(&UserPreference::Resolution));
    assert!(all.contains(&UserPreference::DisplayMode));
    assert!(all.contains(&UserPreference::Vsync));
    assert!(all.contains(&UserPreference::ShowFps));
  }

  // ==========================================================================
  // Env (settings → env rename) tests
  // ==========================================================================

  #[test]
  fn env_seeded_from_system_defaults() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let mut env = HashMap::new();
    env.insert("difficulty".to_string(), serde_json::json!("normal"));
    env.insert("debug_mode".to_string(), serde_json::json!(false));

    let defaults = super::super::workspace::SystemDefaults {
      env,
      ..super::super::workspace::SystemDefaults::default()
    };

    let state = manager.load_or_create_system(&defaults).unwrap();
    assert_eq!(state.get_env_str("difficulty"), Some("normal"));
    assert_eq!(state.get_env_bool("debug_mode"), Some(false));

    // Verify it was saved to disk with "env" key
    let path = dir.path().join("system.json");
    let content = std::fs::read_to_string(&path).unwrap();
    let raw: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(raw["env"]["difficulty"], "normal");
    assert_eq!(raw["env"]["debug_mode"], false);
  }

  #[test]
  fn merge_defaults_adds_new_env_keys_preserves_existing() {
    let mut state = SystemState::new();
    state.set_env("language", serde_json::json!("ja"));
    state.set_env("difficulty", serde_json::json!("easy"));

    let mut env = HashMap::new();
    env.insert("difficulty".to_string(), serde_json::json!("normal")); // should NOT overwrite
    env.insert("new_key".to_string(), serde_json::json!(42)); // should be added

    let defaults = super::super::workspace::SystemDefaults {
      env,
      ..super::super::workspace::SystemDefaults::default()
    };

    let changed = state.merge_defaults(&defaults);
    assert!(changed);

    // Existing keys preserved
    assert_eq!(state.get_env_str("language"), Some("ja"));
    assert_eq!(state.get_env_str("difficulty"), Some("easy"));
    // New key added
    assert_eq!(state.get_env_i64("new_key"), Some(42));
  }

  #[test]
  fn env_roundtrip_through_system_json() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let mut state = SystemState::new();
    state.set_env("name", serde_json::json!("test"));
    state.set_env("score", serde_json::json!(100));
    state.set_env("enabled", serde_json::json!(true));
    state.set_env("ratio", serde_json::json!(1.5));

    manager.save_system(&state).unwrap();
    let loaded = manager.load_system().unwrap();

    assert_eq!(loaded.get_env_str("name"), Some("test"));
    assert_eq!(loaded.get_env_i64("score"), Some(100));
    assert_eq!(loaded.get_env_bool("enabled"), Some(true));
    assert_eq!(loaded.get_env_f64("ratio"), Some(1.5));
  }

  #[test]
  fn env_resaves_with_env_key_not_settings() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let mut state = SystemState::new();
    state.set_env("key", serde_json::json!("value"));

    manager.save_system(&state).unwrap();

    let path = dir.path().join("system.json");
    let content = std::fs::read_to_string(&path).unwrap();
    let raw: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Should use "env" key, not "settings"
    assert!(raw.get("env").is_some());
    assert!(raw.get("settings").is_none());
    assert_eq!(raw["env"]["key"], "value");
  }

  // ==========================================================================
  // Filtered creation + merge-on-save tests
  // ==========================================================================

  #[test]
  fn filtered_creation_writes_only_requested_keys() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    let defaults = super::super::workspace::SystemDefaults::default();

    let filter = UserPreferenceFilter::only([
      UserPreference::MasterVolume,
      UserPreference::MusicVolume,
      UserPreference::SfxVolume,
    ]);

    let state = manager
      .load_or_create_system_filtered(&defaults, &filter)
      .unwrap();

    // Returned state is fully populated
    assert_eq!(state.master_volume, 0.8);
    assert_eq!(state.display.width, 1280);

    // But file on disk only has volume keys
    let path = dir.path().join("system.json");
    let raw: serde_json::Value =
      serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(raw.get("master_volume").is_some());
    assert!(raw.get("music_volume").is_some());
    assert!(raw.get("sfx_volume").is_some());
    assert!(raw.get("display").is_none());
    assert!(raw.get("show_fps").is_none());
    assert!(raw.get("vsync").is_none());
    assert!(raw.get("env").is_none());
  }

  #[test]
  fn save_system_preserves_filtered_structure() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    let defaults = super::super::workspace::SystemDefaults::default();

    // Create a volume-only system.json
    let filter = UserPreferenceFilter::only([
      UserPreference::MasterVolume,
      UserPreference::MusicVolume,
      UserPreference::SfxVolume,
    ]);
    let mut state = manager
      .load_or_create_system_filtered(&defaults, &filter)
      .unwrap();

    // Simulate user changing volumes in pause menu
    state.master_volume = 0.5;
    state.music_volume = 0.7;
    manager.save_system(&state).unwrap();

    // File should still only have volume keys (not display, show_fps, etc.)
    let path = dir.path().join("system.json");
    let raw: serde_json::Value =
      serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let master = raw["master_volume"].as_f64().unwrap();
    let music = raw["music_volume"].as_f64().unwrap();
    assert!((master - 0.5).abs() < 0.001);
    assert!((music - 0.7).abs() < 0.001);
    assert!(raw.get("display").is_none());
    assert!(raw.get("show_fps").is_none());
    assert!(raw.get("vsync").is_none());
  }

  #[test]
  fn save_system_full_write_when_no_existing_file() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let state = SystemState::new();
    manager.save_system(&state).unwrap();

    // No existing file → writes all keys
    let path = dir.path().join("system.json");
    let raw: serde_json::Value =
      serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(raw.get("master_volume").is_some());
    assert!(raw.get("display").is_some());
    assert!(raw.get("env").is_some());
  }

  #[test]
  fn filtered_file_loads_back_with_defaults_for_missing_fields() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    let defaults = super::super::workspace::SystemDefaults::default();

    // Create volume-only file
    let filter = UserPreferenceFilter::only([
      UserPreference::MasterVolume,
      UserPreference::MusicVolume,
      UserPreference::SfxVolume,
    ]);
    manager
      .load_or_create_system_filtered(&defaults, &filter)
      .unwrap();

    // Loading the partial file should fill missing fields from serde defaults
    let loaded = manager.load_system().unwrap();
    assert_eq!(loaded.master_volume, 0.8); // from file
    assert_eq!(loaded.display.width, 1280); // from serde default
    assert_eq!(loaded.display.height, 720);
    assert!(loaded.show_fps.is_none()); // from serde default
  }
}
