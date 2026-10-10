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
//! Startup settings resolve in one order, each layer overriding only what it sets: engine
//! defaults → `Ryo.toml` (dev) or the values `ryo2d build` embedded from it (release) →
//! `EngineBuilder` calls → saved player preferences in `system.json` (only keys present in
//! the file and in the `with_user_preferences` filter). Without `with_user_preferences`,
//! `system.json` is never read. A new `system.json` is seeded from the resolved settings.
//!
//! Use [`SaveManager::load_or_create_system`] to read or create the file directly from a
//! game; the engine resolves startup settings itself.
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
//! settings.width = 1920;
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

use super::resolve::{
  DEFAULT_MASTER_VOLUME, DEFAULT_MUSIC_VOLUME, DEFAULT_RESOLUTION, DEFAULT_SFX_VOLUME,
  StoredPreferences,
};
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
/// Preferences not in the filter, or missing from system.json, keep their builder / Ryo.toml /
/// default values.
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
///     .start_scene(GameScene::new())
///     .build()
///     .run()?;
///
/// // Apply all saved preferences
/// EngineBuilder::new()
///     .with_user_preferences(UserPreferenceFilter::all())
///     .build()
///     .run()?;
///
/// // Apply no saved preferences
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
  /// All fields enabled.
  pub fn all() -> Self {
    Self {
      fields: UserPreference::all().iter().copied().collect(),
    }
  }

  /// No fields - no saved preference overrides the game's settings.
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
/// Round an f32 to 2 decimal places for clean JSON output.
fn clean_f32(value: f32) -> f64 {
  ((value as f64) * 100.0).round() / 100.0
}

/// A `SystemState` as the JSON object `system.json` stores.
fn system_state_map(state: &SystemState) -> SaveResult<serde_json::Map<String, serde_json::Value>> {
  match serde_json::to_value(state).map_err(|e| SaveError::Serialization(e.to_string()))? {
    serde_json::Value::Object(map) => Ok(map),
    _ => Err(SaveError::Serialization(
      "system state did not serialize to a JSON object".to_string(),
    )),
  }
}

/// User settings and configuration (Tier 1).
///
/// Represents the contents of `system.json`, the player-editable configuration file.
/// This is separate from `Ryo.toml` which contains developer settings.
///
/// The engine reads it at startup (see
/// [`EngineBuilder::with_user_preferences`](crate::core::engine::EngineBuilder::with_user_preferences)):
/// saved preferences override builder calls and Ryo.toml, and a new file is seeded from the
/// resolved settings. Keys missing from the file load as engine defaults here but are not
/// treated as preferences at startup.
///
/// # Contents
///
/// - Audio volume levels (master, music, sfx)
/// - Display settings (resolution, display mode, vsync)
/// - Optional engine overrides (show_fps)
/// - Custom key-value storage for game-specific env variables
///
/// Saved as human-readable JSON to `{save_dir}/system.json`.
#[derive(Debug, Clone)]
pub struct SystemState {
  /// Master volume (0.0 - 1.0)
  pub master_volume: f32,
  /// Music volume (0.0 - 1.0)
  pub music_volume: f32,
  /// Sound effects volume (0.0 - 1.0)
  pub sfx_volume: f32,

  /// Window width
  pub width: u32,
  /// Window height
  pub height: u32,
  /// Display mode (windowed, exclusive_fullscreen, borderless_fullscreen)
  pub display_mode: DisplayMode,

  /// VSync preference
  /// - None: Use Mailbox with fallback to Immediate (best performance, no tearing)
  /// - Some(true): Use Mailbox with fallback to Fifo (vsync on)
  /// - Some(false): Use Immediate (vsync off, may tear)
  pub vsync: Option<bool>,

  /// Show FPS counter (engine override, optional)
  pub show_fps: Option<bool>,

  /// Custom key-value storage for game-specific environment variables.
  ///
  /// Seeded from `Ryo.toml [env]` when creating a new `system.json`.
  /// Values preserve their native types (bool, int, float, string) from TOML/JSON.
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
  /// Parses a config-file name (case-insensitive); `None` if unrecognized.
  pub(crate) fn from_config_name(name: &str) -> Option<DisplayMode> {
    match name.to_lowercase().as_str() {
      "windowed" => Some(DisplayMode::Windowed),
      "exclusive_fullscreen" => Some(DisplayMode::ExclusiveFullscreen),
      "borderless_fullscreen" => Some(DisplayMode::BorderlessFullscreen),
      _ => None,
    }
  }

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
  if parts.len() == 2
    && let (Ok(w), Ok(h)) = (parts[0].trim().parse(), parts[1].trim().parse())
  {
    return Some((w, h));
  }
  None
}

impl Serialize for SystemState {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    // Count fields: volumes(3) + resolution + display_mode + vsync + show_fps + env
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("master_volume", &clean_f32(self.master_volume))?;
    map.serialize_entry("music_volume", &clean_f32(self.music_volume))?;
    map.serialize_entry("sfx_volume", &clean_f32(self.sfx_volume))?;
    map.serialize_entry("resolution", &format!("{}x{}", self.width, self.height))?;
    map.serialize_entry("display_mode", &self.display_mode)?;
    map.serialize_entry("vsync", &self.vsync)?;
    if self.show_fps.is_some() {
      map.serialize_entry("show_fps", &self.show_fps)?;
    }
    if !self.env.is_empty() {
      map.serialize_entry("env", &self.env)?;
    }
    map.end()
  }
}

/// `system.json` as stored on disk: every key optional, legacy nested `display` supported.
#[derive(Deserialize)]
struct RawSystemFile {
  master_volume: Option<f32>,
  music_volume: Option<f32>,
  sfx_volume: Option<f32>,
  resolution: Option<String>,
  width: Option<u32>,
  height: Option<u32>,
  display_mode: Option<DisplayMode>,
  vsync: Option<bool>,
  show_fps: Option<bool>,
  #[serde(default)]
  env: HashMap<String, serde_json::Value>,
  display: Option<LegacyDisplay>,
}

#[derive(Deserialize)]
struct LegacyDisplay {
  resolution: Option<String>,
  width: Option<u32>,
  height: Option<u32>,
  display_mode: Option<DisplayMode>,
  vsync: Option<bool>,
}

impl RawSystemFile {
  /// The saved resolution: flat `resolution`, then flat `width`/`height`, then legacy `display`.
  fn resolution(&self) -> Option<(u32, u32)> {
    if let Some(res) = &self.resolution {
      let parsed = parse_resolution(res);
      if parsed.is_none() {
        eprintln!(
          "[ryo2d] Warning: Malformed resolution '{}' in system.json, ignoring it",
          res
        );
      }
      return parsed;
    }
    if let (Some(w), Some(h)) = (self.width, self.height) {
      return Some((w, h));
    }
    let display = self.display.as_ref()?;
    match &display.resolution {
      Some(res) => parse_resolution(res),
      None => display.width.zip(display.height),
    }
  }

  fn display_mode(&self) -> Option<DisplayMode> {
    self
      .display_mode
      .or_else(|| self.display.as_ref().and_then(|d| d.display_mode))
  }

  fn vsync(&self) -> Option<bool> {
    self
      .vsync
      .or_else(|| self.display.as_ref().and_then(|d| d.vsync))
  }

  fn stored_preferences(&self) -> StoredPreferences {
    StoredPreferences {
      resolution: self.resolution(),
      display_mode: self.display_mode(),
      vsync: self.vsync(),
      master_volume: self.master_volume,
      music_volume: self.music_volume,
      sfx_volume: self.sfx_volume,
      show_fps: self.show_fps,
    }
  }

  /// Fill absent keys with engine defaults.
  fn into_state(self) -> SystemState {
    let (width, height) = self.resolution().unwrap_or(DEFAULT_RESOLUTION);
    let display_mode = self.display_mode().unwrap_or_default();
    let vsync = self.vsync();
    SystemState {
      master_volume: self.master_volume.unwrap_or(DEFAULT_MASTER_VOLUME),
      music_volume: self.music_volume.unwrap_or(DEFAULT_MUSIC_VOLUME),
      sfx_volume: self.sfx_volume.unwrap_or(DEFAULT_SFX_VOLUME),
      width,
      height,
      display_mode,
      vsync,
      show_fps: self.show_fps,
      env: self.env,
    }
  }
}

impl<'de> Deserialize<'de> for SystemState {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    RawSystemFile::deserialize(deserializer).map(RawSystemFile::into_state)
  }
}

impl Default for SystemState {
  fn default() -> Self {
    SystemState {
      master_volume: DEFAULT_MASTER_VOLUME,
      music_volume: DEFAULT_MUSIC_VOLUME,
      sfx_volume: DEFAULT_SFX_VOLUME,
      width: DEFAULT_RESOLUTION.0,
      height: DEFAULT_RESOLUTION.1,
      display_mode: DisplayMode::Windowed,
      vsync: None,
      show_fps: None,
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
      width: defaults.resolution.map(|(w, _)| w).unwrap_or(1280),
      height: defaults.resolution.map(|(_, h)| h).unwrap_or(720),
      display_mode: defaults.display_mode,
      vsync: Some(defaults.vsync),
      show_fps: Some(defaults.show_fps),
      env: defaults.env.clone(),
    }
  }

  /// Add `env` keys from `defaults` that this state doesn't have yet.
  ///
  /// Existing values, including a saved resolution, are never changed.
  /// Returns true if any key was added.
  pub fn merge_defaults(&mut self, defaults: &super::workspace::SystemDefaults) -> bool {
    let mut changed = false;
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
  /// When `system.json` already exists, keys already in the file are updated,
  /// and a key missing from the file is added only when its value differs from
  /// what the file loads as - so a file written with a narrow
  /// [`UserPreferenceFilter`] keeps its shape until the player changes a
  /// setting it doesn't hold yet. When no file exists, writes the full state.
  pub fn save_system(&self, state: &SystemState) -> SaveResult<()> {
    let path = self.system_path();

    if path.exists() {
      let existing_json = fs::read_to_string(&path)?;
      if let Ok(serde_json::Value::Object(existing_map)) =
        serde_json::from_str::<serde_json::Value>(&existing_json)
      {
        let new_map = system_state_map(state)?;
        // What the file loads as today, with engine defaults for absent keys
        let baseline = match serde_json::from_value::<SystemState>(serde_json::Value::Object(
          existing_map.clone(),
        )) {
          Ok(loaded) => system_state_map(&loaded)?,
          // Unparsable file: treat every key as unchanged so its shape is kept
          Err(_) => new_map.clone(),
        };

        let mut merged = serde_json::Map::new();
        for key in existing_map.keys() {
          if key == "display" {
            // Replace legacy nested display with flat keys
            for flat_key in ["resolution", "display_mode", "vsync"] {
              if let Some(val) = new_map.get(flat_key) {
                merged.insert(flat_key.to_string(), val.clone());
              }
            }
          } else if let Some(new_val) = new_map.get(key) {
            merged.insert(key.clone(), new_val.clone());
          }
        }
        for (key, new_val) in &new_map {
          if !merged.contains_key(key) && baseline.get(key) != Some(new_val) {
            merged.insert(key.clone(), new_val.clone());
          }
        }

        return self.write_system_map(merged);
      }
    }

    // No existing file or couldn't parse — write full state
    self.write_json_file(&path, state)
  }

  /// Load system state from JSON file.
  ///
  /// Returns default state if no save file exists.
  pub fn load_system(&self) -> SaveResult<SystemState> {
    let path = self.system_path();
    if !path.exists() {
      return Ok(SystemState::new());
    }
    self.read_json_file(&path)
  }

  /// Load or create system state.
  ///
  /// Creates system.json from `defaults` (Ryo.toml `[system]`) when it doesn't exist;
  /// otherwise loads it and adds new `env` keys from `defaults`. Saved values are never
  /// replaced.
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
    let path = self.system_path();

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

  /// The player preferences present in `system.json`, or `None` when there's no file.
  pub(crate) fn load_stored_preferences(&self) -> SaveResult<Option<StoredPreferences>> {
    let path = self.system_path();
    if !path.exists() {
      return Ok(None);
    }
    let raw: RawSystemFile = self.read_json_file(&path)?;
    Ok(Some(raw.stored_preferences()))
  }

  /// Read the saved preferences, or seed a new `system.json` from `seed`.
  ///
  /// - No file: writes the keys of `seed` that are in `filter` (plus `env` when
  ///   `seed.env` isn't empty) and returns `Ok(None)` - the seed already is
  ///   what the game resolved, so there is nothing to override.
  /// - File present: adds `env` keys from `defaults` it doesn't have yet and
  ///   returns the preferences it holds.
  /// - Unparsable file: returns the error and leaves the file untouched.
  pub(crate) fn load_preferences_or_seed(
    &self,
    seed: &SystemState,
    filter: &UserPreferenceFilter,
    defaults: &super::workspace::SystemDefaults,
  ) -> SaveResult<Option<StoredPreferences>> {
    let Some(prefs) = self.load_stored_preferences()? else {
      self.write_filtered_system(seed, filter, !seed.env.is_empty())?;
      return Ok(None);
    };

    let mut state = self.load_system()?;
    if state.merge_defaults(defaults) {
      self.save_system(&state)?;
    }
    Ok(Some(prefs))
  }

  /// Write a new `system.json` holding only the keys in `filter` (plus `env` when `include_env`).
  fn write_filtered_system(
    &self,
    state: &SystemState,
    filter: &UserPreferenceFilter,
    include_env: bool,
  ) -> SaveResult<()> {
    let filtered = system_state_map(state)?
      .into_iter()
      .filter(|(key, _)| match key.as_str() {
        "master_volume" => filter.contains(UserPreference::MasterVolume),
        "music_volume" => filter.contains(UserPreference::MusicVolume),
        "sfx_volume" => filter.contains(UserPreference::SfxVolume),
        "resolution" => filter.contains(UserPreference::Resolution),
        "display_mode" => filter.contains(UserPreference::DisplayMode),
        "vsync" => filter.contains(UserPreference::Vsync),
        "show_fps" => filter.contains(UserPreference::ShowFps),
        "env" => include_env,
        _ => false,
      })
      .collect();
    self.write_system_map(filtered)
  }

  fn system_path(&self) -> PathBuf {
    self.save_dir.join(format!("system.{}", SYSTEM_EXTENSION))
  }

  fn write_system_map(&self, map: serde_json::Map<String, serde_json::Value>) -> SaveResult<()> {
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(map))
      .map_err(|e| SaveError::Serialization(e.to_string()))?;
    let mut file = File::create(self.system_path())?;
    file.write_all(json.as_bytes())?;
    file.flush()?;
    Ok(())
  }

  /// Load or create system state, writing only filtered fields to a new file.
  ///
  /// Behaves like [`load_or_create_system`](Self::load_or_create_system) when
  /// system.json already exists. When creating a new file, only the JSON keys
  /// in `filter` are written (plus `env` when `defaults.env` isn't empty) - so a
  /// volume-only filter produces a system.json with just `master_volume`,
  /// `music_volume`, and `sfx_volume`.
  ///
  /// Returns the state as it loads from disk: keys the file doesn't hold get
  /// engine defaults.
  pub fn load_or_create_system_filtered(
    &self,
    defaults: &super::workspace::SystemDefaults,
    filter: &UserPreferenceFilter,
  ) -> SaveResult<SystemState> {
    if self.system_path().exists() {
      let mut state = self.load_system()?;
      if state.merge_defaults(defaults) {
        self.save_system(&state)?;
      }
      return Ok(state);
    }

    self.write_filtered_system(
      &SystemState::from_defaults(defaults),
      filter,
      !defaults.env.is_empty(),
    )?;
    self.load_system()
  }

  /// Check if system state exists.
  pub fn has_system_save(&self) -> bool {
    self.system_path().exists()
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
    state.width = 1920;
    state.height = 1080;
    state.set_env("language", serde_json::Value::String("en".into()));

    manager.save_system(&state).unwrap();
    assert!(manager.has_system_save());

    let loaded = manager.load_system().unwrap();
    assert_eq!(loaded.master_volume, 0.75);
    assert_eq!(loaded.music_volume, 0.5);
    assert_eq!(loaded.width, 1920);
    assert_eq!(loaded.height, 1080);
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
    state.width = 1920;
    state.height = 1080;
    state.set_env("language", serde_json::Value::String("en".into()));

    manager.save_system(&state).unwrap();

    // Read raw file and verify it's JSON (not compressed)
    let path = dir.path().join("system.json");
    let content = fs::read_to_string(&path).unwrap();

    // Should be valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["master_volume"], 0.75);
    assert_eq!(parsed["resolution"], "1920x1080");
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
      target_background_fps: 30,
      background_rendering: false,
      env,
    };

    let state = SystemState::from_defaults(&defaults);

    assert_eq!(state.width, 1920);
    assert_eq!(state.height, 1080);
    assert_eq!(state.vsync, Some(false));
    assert_eq!(state.display_mode, DisplayMode::ExclusiveFullscreen);
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
    assert_eq!(state.width, 1280);
    assert_eq!(state.height, 720);
    assert_eq!(state.vsync, Some(true));
    assert_eq!(state.display_mode, DisplayMode::Windowed);
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
  fn merge_defaults_keeps_a_saved_720p_resolution() {
    let engine_defaults = super::super::workspace::SystemDefaults::default();
    let mut state = SystemState::from_defaults(&engine_defaults);
    let dev_defaults = super::super::workspace::SystemDefaults {
      resolution: Some((800, 600)),
      ..engine_defaults
    };

    let changed = state.merge_defaults(&dev_defaults);

    assert!(!changed);
    assert_eq!((state.width, state.height), (1280, 720));
  }

  #[test]
  fn system_state_merge_defaults_preserves_user_changes() {
    let engine_defaults = super::super::workspace::SystemDefaults::default();

    // User modified their resolution (not at engine defaults)
    let mut state = SystemState::new();
    state.width = 1600;
    state.height = 900;

    // Developer has different defaults
    let dev_defaults = super::super::workspace::SystemDefaults {
      resolution: Some((800, 600)),
      ..engine_defaults
    };

    // Merge should NOT change user's custom resolution
    let changed = state.merge_defaults(&dev_defaults);
    assert!(!changed);
    assert_eq!(state.width, 1600);
    assert_eq!(state.height, 900);
  }

  #[test]
  fn system_state_default_uses_engine_defaults() {
    let state = SystemState::default();
    assert_eq!(state.width, 1280);
    assert_eq!(state.height, 720);
    assert_eq!(state.vsync, None);
    assert_eq!(state.display_mode, DisplayMode::Windowed);
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
      target_background_fps: 30,
      background_rendering: false,
      env: HashMap::new(),
    };

    assert!(!manager.has_system_save());

    let state = manager.load_or_create_system(&defaults).unwrap();

    assert!(manager.has_system_save());
    assert_eq!(state.width, 1024);
    assert_eq!(state.height, 768);
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
    initial.width = 1600;
    initial.height = 900;
    manager.save_system(&initial).unwrap();

    // load_or_create should load existing, not overwrite
    let defaults = super::super::workspace::SystemDefaults::default();
    let state = manager.load_or_create_system(&defaults).unwrap();

    assert_eq!(state.master_volume, 0.42);
    assert_eq!(state.width, 1600);
    assert_eq!(state.height, 900);
  }

  #[test]
  fn load_or_create_system_keeps_a_saved_720p_resolution() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    let engine_defaults = super::super::workspace::SystemDefaults::default();
    manager
      .save_system(&SystemState::from_defaults(&engine_defaults))
      .unwrap();
    let dev_defaults = super::super::workspace::SystemDefaults {
      resolution: Some((800, 600)),
      ..engine_defaults
    };

    let state = manager.load_or_create_system(&dev_defaults).unwrap();

    assert_eq!((state.width, state.height), (1280, 720));
  }

  // ==========================================================================
  // Stored preferences, seeding and missing-key saves
  // ==========================================================================

  fn write_system_json(dir: &std::path::Path, json: &str) {
    std::fs::write(dir.join("system.json"), json).unwrap();
  }

  fn read_system_json(dir: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("system.json")).unwrap()).unwrap()
  }

  #[test]
  fn stored_preferences_report_only_present_keys() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    write_system_json(
      dir.path(),
      r#"{"master_volume": 0.5, "resolution": "1600x900"}"#,
    );

    let prefs = manager.load_stored_preferences().unwrap().unwrap();

    assert_eq!(prefs.master_volume, Some(0.5));
    assert_eq!(prefs.resolution, Some((1600, 900)));
    assert_eq!(prefs.music_volume, None);
    assert_eq!(prefs.sfx_volume, None);
    assert_eq!(prefs.display_mode, None);
    assert_eq!(prefs.vsync, None);
    assert_eq!(prefs.show_fps, None);
  }

  #[test]
  fn stored_preferences_are_none_without_a_file() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    assert_eq!(manager.load_stored_preferences().unwrap(), None);
  }

  #[test]
  fn stored_preferences_read_legacy_nested_display() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    write_system_json(
      dir.path(),
      r#"{"display": {"width": 1920, "height": 1080, "display_mode": "borderless_fullscreen", "vsync": false}}"#,
    );

    let prefs = manager.load_stored_preferences().unwrap().unwrap();

    assert_eq!(prefs.resolution, Some((1920, 1080)));
    assert_eq!(prefs.display_mode, Some(DisplayMode::BorderlessFullscreen));
    assert_eq!(prefs.vsync, Some(false));
  }

  #[test]
  fn stored_preferences_ignore_a_malformed_resolution() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    write_system_json(dir.path(), r#"{"resolution": "big", "master_volume": 0.5}"#);

    let prefs = manager.load_stored_preferences().unwrap().unwrap();

    assert_eq!(prefs.resolution, None);
    assert_eq!(prefs.master_volume, Some(0.5));
  }

  #[test]
  fn seeding_writes_the_filtered_keys_of_the_seed_state() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    let mut seed = SystemState::new();
    seed.width = 800;
    seed.height = 600;
    seed.master_volume = 0.3;
    seed.vsync = Some(false);
    let filter =
      UserPreferenceFilter::only([UserPreference::Resolution, UserPreference::MasterVolume]);

    let prefs = manager
      .load_preferences_or_seed(
        &seed,
        &filter,
        &super::super::workspace::SystemDefaults::default(),
      )
      .unwrap();

    assert_eq!(prefs, None);
    let raw = read_system_json(dir.path());
    assert_eq!(raw["resolution"], "800x600");
    assert_eq!(raw["master_volume"], 0.3);
    assert!(raw.get("vsync").is_none());
    assert!(raw.get("display_mode").is_none());
    assert!(raw.get("env").is_none());
  }

  #[test]
  fn an_existing_file_is_not_reseeded() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    write_system_json(dir.path(), r#"{"master_volume": 0.5}"#);
    let mut seed = SystemState::new();
    seed.master_volume = 0.3;

    let prefs = manager
      .load_preferences_or_seed(
        &seed,
        &UserPreferenceFilter::all(),
        &super::super::workspace::SystemDefaults::default(),
      )
      .unwrap()
      .unwrap();

    assert_eq!(prefs.master_volume, Some(0.5));
    let raw = read_system_json(dir.path());
    assert_eq!(raw["master_volume"], 0.5);
    assert!(raw.get("resolution").is_none());
  }

  #[test]
  fn new_env_keys_are_merged_into_an_existing_file() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    write_system_json(dir.path(), r#"{"master_volume": 0.5}"#);
    let mut env = HashMap::new();
    env.insert("difficulty".to_string(), serde_json::json!("hard"));
    let defaults = super::super::workspace::SystemDefaults {
      env,
      ..super::super::workspace::SystemDefaults::default()
    };

    manager
      .load_preferences_or_seed(&SystemState::new(), &UserPreferenceFilter::all(), &defaults)
      .unwrap();

    let raw = read_system_json(dir.path());
    assert_eq!(raw["env"]["difficulty"], "hard");
    assert_eq!(raw["master_volume"], 0.5);
    assert!(raw.get("resolution").is_none());
  }

  #[test]
  fn malformed_file_is_an_error_and_left_alone() {
    for contents in ["not json", r#"{"vsync": "yes"}"#] {
      let dir = tempdir().unwrap();
      let manager = SaveManager::new(dir.path()).unwrap();
      write_system_json(dir.path(), contents);

      let result = manager.load_preferences_or_seed(
        &SystemState::new(),
        &UserPreferenceFilter::all(),
        &super::super::workspace::SystemDefaults::default(),
      );

      assert!(result.is_err(), "{contents} should not parse");
      assert_eq!(
        std::fs::read_to_string(dir.path().join("system.json")).unwrap(),
        contents
      );
    }
  }

  #[test]
  fn save_system_writes_a_changed_key_missing_from_the_file() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    write_system_json(dir.path(), r#"{"master_volume": 0.5}"#);

    let mut state = manager.load_system().unwrap();
    state.width = 1920;
    state.height = 1080;
    manager.save_system(&state).unwrap();

    let raw = read_system_json(dir.path());
    assert_eq!(raw["resolution"], "1920x1080");
    assert_eq!(raw["master_volume"], 0.5);
    assert!(raw.get("display_mode").is_none());
    assert!(raw.get("vsync").is_none());
  }

  #[test]
  fn save_system_keeps_the_shape_of_an_unparsable_file() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    write_system_json(dir.path(), r#"{"master_volume": 0.5, "vsync": "yes"}"#);

    let mut state = SystemState::new();
    state.width = 1920;
    state.height = 1080;
    manager.save_system(&state).unwrap();

    let raw = read_system_json(dir.path());
    assert!(raw.get("master_volume").is_some());
    assert!(raw.get("vsync").is_some());
    assert!(raw.get("resolution").is_none());
    assert!(raw.get("display_mode").is_none());
  }

  #[test]
  fn filtered_creation_returns_the_state_as_stored() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    let defaults = super::super::workspace::SystemDefaults {
      resolution: Some((800, 600)),
      ..Default::default()
    };
    let filter = UserPreferenceFilter::only([UserPreference::MasterVolume]);

    let state = manager
      .load_or_create_system_filtered(&defaults, &filter)
      .unwrap();

    assert_eq!((state.width, state.height), (1280, 720));
  }

  #[test]
  fn full_file_from_previous_version_round_trips() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    let previous = r#"{
  "master_volume": 0.6,
  "music_volume": 0.7,
  "sfx_volume": 0.9,
  "resolution": "1280x720",
  "display_mode": "windowed",
  "vsync": true,
  "show_fps": false
}"#;
    write_system_json(dir.path(), previous);

    let prefs = manager.load_stored_preferences().unwrap().unwrap();
    assert_eq!(prefs.resolution, Some((1280, 720)));
    assert_eq!(prefs.vsync, Some(true));
    assert_eq!(prefs.show_fps, Some(false));

    manager
      .save_system(&manager.load_system().unwrap())
      .unwrap();
    let expected: serde_json::Value = serde_json::from_str(previous).unwrap();
    assert_eq!(read_system_json(dir.path()), expected);
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
  // SystemState flat display field serde tests
  // ==========================================================================

  #[test]
  fn system_state_serializes_resolution_as_string() {
    let mut state = SystemState::default();
    state.width = 1920;
    state.height = 1080;

    let json = serde_json::to_string(&state).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["resolution"], "1920x1080");
    // No nested display object
    assert!(parsed.get("display").is_none());
    // No separate width/height fields in JSON
    assert!(parsed.get("width").is_none());
    assert!(parsed.get("height").is_none());
  }

  #[test]
  fn system_state_deserializes_flat_format() {
    let json = r#"{"resolution": "800x600", "display_mode": "windowed", "vsync": false}"#;
    let state: SystemState = serde_json::from_str(json).unwrap();

    assert_eq!(state.width, 800);
    assert_eq!(state.height, 600);
    assert_eq!(state.vsync, Some(false));
    assert_eq!(state.display_mode, DisplayMode::Windowed);
  }

  #[test]
  fn system_state_roundtrip_serde() {
    let mut original = SystemState::default();
    original.width = 2560;
    original.height = 1440;
    original.display_mode = DisplayMode::BorderlessFullscreen;
    original.vsync = Some(true);

    let json = serde_json::to_string(&original).unwrap();
    let deserialized: SystemState = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.width, original.width);
    assert_eq!(deserialized.height, original.height);
    assert_eq!(deserialized.display_mode, original.display_mode);
    assert_eq!(deserialized.vsync, original.vsync);
  }

  #[test]
  fn system_state_deserializes_legacy_nested_display() {
    // Old system.json with nested display object
    let json = r#"{
      "master_volume": 0.8,
      "display": {
        "resolution": "1600x900",
        "display_mode": "windowed",
        "vsync": true
      }
    }"#;
    let state: SystemState = serde_json::from_str(json).unwrap();

    assert_eq!(state.width, 1600);
    assert_eq!(state.height, 900);
    assert_eq!(state.display_mode, DisplayMode::Windowed);
    assert_eq!(state.vsync, Some(true));
  }

  #[test]
  fn system_state_flat_fields_take_priority_over_legacy_nested() {
    // If both flat and nested exist, flat wins
    let json = r#"{
      "resolution": "1920x1080",
      "display_mode": "borderless_fullscreen",
      "vsync": false,
      "display": {
        "resolution": "800x600",
        "display_mode": "windowed",
        "vsync": true
      }
    }"#;
    let state: SystemState = serde_json::from_str(json).unwrap();

    assert_eq!(state.width, 1920);
    assert_eq!(state.height, 1080);
    assert_eq!(state.display_mode, DisplayMode::BorderlessFullscreen);
    assert_eq!(state.vsync, Some(false));
  }

  #[test]
  fn system_state_deserializes_legacy_width_height() {
    let json = r#"{"width": 1600, "height": 900, "display_mode": "windowed"}"#;
    let state: SystemState = serde_json::from_str(json).unwrap();

    assert_eq!(state.width, 1600);
    assert_eq!(state.height, 900);
  }

  #[test]
  fn system_state_resolution_takes_priority_over_legacy_width_height() {
    let json =
      r#"{"resolution": "1920x1080", "width": 800, "height": 600, "display_mode": "windowed"}"#;
    let state: SystemState = serde_json::from_str(json).unwrap();

    assert_eq!(state.width, 1920);
    assert_eq!(state.height, 1080);
  }

  #[test]
  fn system_state_malformed_resolution_falls_back_to_defaults() {
    let json = r#"{"resolution": "not_valid", "display_mode": "windowed"}"#;
    let state: SystemState = serde_json::from_str(json).unwrap();

    assert_eq!(state.width, 1280);
    assert_eq!(state.height, 720);
  }

  #[test]
  fn system_state_missing_resolution_falls_back_to_defaults() {
    let json = r#"{"display_mode": "windowed"}"#;
    let state: SystemState = serde_json::from_str(json).unwrap();

    assert_eq!(state.width, 1280);
    assert_eq!(state.height, 720);
  }

  // ==========================================================================
  // Integration: system.json save/load roundtrip with new format
  // ==========================================================================

  #[test]
  fn system_state_save_load_roundtrip_new_resolution_format() {
    let dir = tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();

    let mut state = SystemState::new();
    state.width = 1920;
    state.height = 1080;
    state.display_mode = DisplayMode::BorderlessFullscreen;
    state.master_volume = 0.42;

    manager.save_system(&state).unwrap();

    // Verify JSON on disk uses flat resolution string
    let path = dir.path().join("system.json");
    let content = std::fs::read_to_string(&path).unwrap();
    let raw: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(raw["resolution"], "1920x1080");
    assert!(raw.get("display").is_none());

    // Load it back and verify values roundtrip
    let loaded = manager.load_system().unwrap();
    assert_eq!(loaded.width, 1920);
    assert_eq!(loaded.height, 1080);
    assert_eq!(loaded.display_mode, DisplayMode::BorderlessFullscreen);
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
    assert_eq!(loaded.width, 1600);
    assert_eq!(loaded.height, 900);
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

    // Verify it was re-saved in flat format (no nested display)
    let content = std::fs::read_to_string(&path).unwrap();
    let raw: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(raw["resolution"], "1600x900");
    assert!(raw.get("display").is_none());
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
    let filter: UserPreferenceFilter =
      [UserPreference::Resolution, UserPreference::DisplayMode].into();

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

    // Returned state is what the file loads as (engine defaults for absent keys)
    assert_eq!(state.master_volume, 0.8);
    assert_eq!(state.width, 1280);

    // But file on disk only has volume keys
    let path = dir.path().join("system.json");
    let raw: serde_json::Value =
      serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(raw.get("master_volume").is_some());
    assert!(raw.get("music_volume").is_some());
    assert!(raw.get("sfx_volume").is_some());
    assert!(raw.get("resolution").is_none());
    assert!(raw.get("display_mode").is_none());
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

    // File should still only have volume keys (not display fields, show_fps, etc.)
    let path = dir.path().join("system.json");
    let raw: serde_json::Value =
      serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let master = raw["master_volume"].as_f64().unwrap();
    let music = raw["music_volume"].as_f64().unwrap();
    assert!((master - 0.5).abs() < 0.001);
    assert!((music - 0.7).abs() < 0.001);
    assert!(raw.get("resolution").is_none());
    assert!(raw.get("display_mode").is_none());
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
    assert!(raw.get("resolution").is_some());
    assert!(raw.get("display_mode").is_some());
    assert!(raw.get("vsync").is_some());
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
    assert_eq!(loaded.width, 1280); // from serde default
    assert_eq!(loaded.height, 720);
    assert!(loaded.show_fps.is_none()); // from serde default
  }
}
