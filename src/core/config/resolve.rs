//! Layered resolution of startup settings.
//!
//! Every setting more than one source can set resolves in one order, each layer
//! overriding only what it actually sets:
//!
//! 1. Engine defaults
//! 2. Project values: `Ryo.toml` in dev, or the values `ryo2d build` embedded
//!    from it in release builds
//! 3. `EngineBuilder` calls
//! 4. Saved player preferences in `system.json` - only keys present in the file
//!    and in the `with_user_preferences` filter

use std::collections::HashMap;
use std::path::PathBuf;

use super::debug::DebugConfig;
use super::save::{
  DisplayMode, SystemState, UserPreference, UserPreferenceFilter, parse_resolution,
};
use super::workspace::Workspace;

/// Engine default resolution.
pub(crate) const DEFAULT_RESOLUTION: (u32, u32) = (1280, 720);
/// Engine default master volume.
pub(crate) const DEFAULT_MASTER_VOLUME: f32 = 0.8;
/// Engine default music volume.
pub(crate) const DEFAULT_MUSIC_VOLUME: f32 = 1.0;
/// Engine default sound effects volume.
pub(crate) const DEFAULT_SFX_VOLUME: f32 = 1.0;
const DEFAULT_TITLE: &str = "Ryo2D";

/// Environment variables `ryo2d build` embeds. `RYO_EMBED_RESOLUTION` is always
/// set by `ryo2d build` and marks a binary built that way.
pub(crate) const EMBEDDED_KEYS: &[&str] = &[
  "RYO_EMBED_RESOLUTION",
  "RYO_EMBED_VSYNC",
  "RYO_EMBED_DISPLAY_MODE",
  "RYO_EMBED_MASTER_VOLUME",
  "RYO_EMBED_MUSIC_VOLUME",
  "RYO_EMBED_SFX_VOLUME",
  "RYO_EMBED_CONSOLE",
  "RYO_EMBED_TITLE",
  "RYO_EMBED_SHOW_FPS",
  "RYO_EMBED_SHOW_COLLIDERS",
  "RYO_EMBED_SHOW_VELOCITIES",
  "RYO_EMBED_SHOW_ORIGINS",
  "RYO_EMBED_SHOW_FRAME_TIMING",
  "RYO_EMBED_DISABLE_DEBUG_KEYS",
  "RYO_EMBED_ENABLE_PROFILER",
  "RYO_EMBED_DEV_WINDOW_KEYS",
  "RYO_EMBED_LOGGER_WINDOW",
  "RYO_EMBED_BACKGROUND_FPS",
  "RYO_EMBED_BACKGROUND_RENDERING",
];

/// Settings set through `EngineBuilder` calls; `None` = not set.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct BuilderOverrides {
  pub title: Option<String>,
  pub resolution: Option<(u32, u32)>,
  pub display_mode: Option<DisplayMode>,
  pub vsync: Option<bool>,
  pub show_fps: Option<bool>,
  pub show_colliders: Option<bool>,
  pub show_velocities: Option<bool>,
  pub show_origins: Option<bool>,
  pub background_fps: Option<u32>,
  pub background_rendering: Option<bool>,
  pub icon: Option<PathBuf>,
}

/// Settings from the project: `Ryo.toml` in dev, embedded values in release.
/// `None` = not provided.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ProjectLayer {
  pub title: Option<String>,
  pub resolution: Option<(u32, u32)>,
  pub display_mode: Option<DisplayMode>,
  pub vsync: Option<bool>,
  pub master_volume: Option<f32>,
  pub music_volume: Option<f32>,
  pub sfx_volume: Option<f32>,
  pub show_fps: Option<bool>,
  pub show_colliders: Option<bool>,
  pub show_velocities: Option<bool>,
  pub show_origins: Option<bool>,
  pub show_frame_timing: Option<bool>,
  pub enable_profiler: Option<bool>,
  pub disable_debug_keys: Option<bool>,
  pub logger_window: Option<bool>,
  pub dev_window_keys: Option<bool>,
  pub dev_console: Option<bool>,
  pub background_fps: Option<u32>,
  pub background_rendering: Option<bool>,
  pub icon: Option<PathBuf>,
}

/// Player preferences present in `system.json`; `None` = key absent or malformed.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct StoredPreferences {
  pub resolution: Option<(u32, u32)>,
  pub display_mode: Option<DisplayMode>,
  pub vsync: Option<bool>,
  pub master_volume: Option<f32>,
  pub music_volume: Option<f32>,
  pub sfx_volume: Option<f32>,
  pub show_fps: Option<bool>,
}

/// Final value of every layered setting.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedSettings {
  pub title: String,
  pub resolution: (u32, u32),
  pub display_mode: DisplayMode,
  pub vsync: bool,
  pub master_volume: f32,
  pub music_volume: f32,
  pub sfx_volume: f32,
  pub show_fps: bool,
  pub show_colliders: bool,
  pub show_velocities: bool,
  pub show_origins: bool,
  pub show_frame_timing: bool,
  pub enable_profiler: bool,
  pub disable_debug_keys: bool,
  pub logger_window: bool,
  pub dev_window_keys: bool,
  pub dev_console: bool,
  pub background_fps: u32,
  pub background_rendering: bool,
  pub icon: Option<PathBuf>,
}

impl ResolvedSettings {
  fn defaults() -> Self {
    let debug = DebugConfig::default();
    ResolvedSettings {
      title: DEFAULT_TITLE.to_string(),
      resolution: DEFAULT_RESOLUTION,
      display_mode: DisplayMode::Windowed,
      vsync: true,
      master_volume: DEFAULT_MASTER_VOLUME,
      music_volume: DEFAULT_MUSIC_VOLUME,
      sfx_volume: DEFAULT_SFX_VOLUME,
      show_fps: debug.show_fps,
      show_colliders: debug.show_colliders,
      show_velocities: debug.show_velocities,
      show_origins: debug.show_origins,
      show_frame_timing: debug.show_frame_timing,
      enable_profiler: debug.enable_profiler,
      disable_debug_keys: false,
      logger_window: debug.logger_window,
      dev_window_keys: debug.dev_window_keys,
      dev_console: debug.dev_console,
      background_fps: 0,
      background_rendering: true,
      icon: None,
    }
  }

  /// The `system.json` contents a new file is seeded with.
  pub(crate) fn to_system_state(&self, env: HashMap<String, serde_json::Value>) -> SystemState {
    SystemState {
      master_volume: self.master_volume,
      music_volume: self.music_volume,
      sfx_volume: self.sfx_volume,
      width: self.resolution.0,
      height: self.resolution.1,
      display_mode: self.display_mode,
      vsync: Some(self.vsync),
      show_fps: Some(self.show_fps),
      env,
    }
  }
}

/// Layer project values, builder calls and saved preferences over the engine defaults.
pub(crate) fn resolve_settings(
  project: &ProjectLayer,
  builder: &BuilderOverrides,
  prefs: Option<(&StoredPreferences, &UserPreferenceFilter)>,
) -> ResolvedSettings {
  let mut s = ResolvedSettings::defaults();

  set(&mut s.title, project.title.clone());
  set(&mut s.resolution, project.resolution);
  set(&mut s.display_mode, project.display_mode);
  set(&mut s.vsync, project.vsync);
  set(&mut s.master_volume, project.master_volume);
  set(&mut s.music_volume, project.music_volume);
  set(&mut s.sfx_volume, project.sfx_volume);
  set(&mut s.show_fps, project.show_fps);
  set(&mut s.show_colliders, project.show_colliders);
  set(&mut s.show_velocities, project.show_velocities);
  set(&mut s.show_origins, project.show_origins);
  set(&mut s.show_frame_timing, project.show_frame_timing);
  set(&mut s.enable_profiler, project.enable_profiler);
  set(&mut s.disable_debug_keys, project.disable_debug_keys);
  set(&mut s.logger_window, project.logger_window);
  set(&mut s.dev_window_keys, project.dev_window_keys);
  set(&mut s.dev_console, project.dev_console);
  set(&mut s.background_fps, project.background_fps);
  set(&mut s.background_rendering, project.background_rendering);
  if project.icon.is_some() {
    s.icon = project.icon.clone();
  }

  set(&mut s.title, builder.title.clone());
  set(&mut s.resolution, builder.resolution);
  set(&mut s.display_mode, builder.display_mode);
  set(&mut s.vsync, builder.vsync);
  set(&mut s.show_fps, builder.show_fps);
  set(&mut s.show_colliders, builder.show_colliders);
  set(&mut s.show_velocities, builder.show_velocities);
  set(&mut s.show_origins, builder.show_origins);
  set(&mut s.background_fps, builder.background_fps);
  set(&mut s.background_rendering, builder.background_rendering);
  if builder.icon.is_some() {
    s.icon = builder.icon.clone();
  }

  if let Some((prefs, filter)) = prefs {
    if filter.contains(UserPreference::Resolution) {
      set(&mut s.resolution, prefs.resolution);
    }
    if filter.contains(UserPreference::DisplayMode) {
      set(&mut s.display_mode, prefs.display_mode);
    }
    if filter.contains(UserPreference::Vsync) {
      set(&mut s.vsync, prefs.vsync);
    }
    if filter.contains(UserPreference::MasterVolume) {
      set(&mut s.master_volume, prefs.master_volume);
    }
    if filter.contains(UserPreference::MusicVolume) {
      set(&mut s.music_volume, prefs.music_volume);
    }
    if filter.contains(UserPreference::SfxVolume) {
      set(&mut s.sfx_volume, prefs.sfx_volume);
    }
    if filter.contains(UserPreference::ShowFps) {
      set(&mut s.show_fps, prefs.show_fps);
    }
  }

  s
}

/// Overwrite `target` when a layer provides a value.
fn set<T>(target: &mut T, value: Option<T>) {
  if let Some(value) = value {
    *target = value;
  }
}

impl ProjectLayer {
  /// Values from a workspace's `Ryo.toml` (dev builds).
  pub(crate) fn from_workspace(workspace: &Workspace) -> Self {
    let config = workspace.config();
    let system = &config.system;
    let debug = &config.debug;
    ProjectLayer {
      title: Some(config.title.clone()),
      resolution: system.resolution,
      display_mode: Some(system.display_mode),
      vsync: Some(system.vsync),
      master_volume: Some(system.master_volume),
      music_volume: Some(system.music_volume),
      sfx_volume: Some(system.sfx_volume),
      show_fps: Some(system.show_fps),
      show_colliders: Some(debug.show_colliders),
      show_velocities: Some(debug.show_velocities),
      show_origins: Some(debug.show_origins),
      show_frame_timing: Some(debug.show_frame_timing),
      enable_profiler: Some(debug.enable_profiler),
      disable_debug_keys: Some(debug.disable_debug_keys),
      logger_window: Some(debug.logger_window),
      dev_window_keys: Some(debug.dev_window_keys),
      dev_console: Some(system.dev_console),
      background_fps: Some(system.target_background_fps),
      background_rendering: Some(system.background_rendering),
      icon: config
        .release
        .logo
        .as_ref()
        .map(|logo| workspace.root().join(logo)),
    }
  }

  /// Values `ryo2d build` embedded (release builds); `None` when the binary
  /// wasn't built by `ryo2d build`. Malformed values are skipped with a warning.
  pub(crate) fn from_embedded<'a>(lookup: impl Fn(&str) -> Option<&'a str>) -> Option<Self> {
    let resolution = lookup("RYO_EMBED_RESOLUTION")?;
    Some(ProjectLayer {
      title: lookup("RYO_EMBED_TITLE").map(str::to_string),
      resolution: parse_resolution(resolution).or_else(|| {
        warn_malformed("RYO_EMBED_RESOLUTION", resolution);
        None
      }),
      display_mode: lookup("RYO_EMBED_DISPLAY_MODE").map(parse_display_mode),
      vsync: parse_embedded(&lookup, "RYO_EMBED_VSYNC"),
      master_volume: parse_embedded(&lookup, "RYO_EMBED_MASTER_VOLUME"),
      music_volume: parse_embedded(&lookup, "RYO_EMBED_MUSIC_VOLUME"),
      sfx_volume: parse_embedded(&lookup, "RYO_EMBED_SFX_VOLUME"),
      show_fps: parse_embedded(&lookup, "RYO_EMBED_SHOW_FPS"),
      show_colliders: parse_embedded(&lookup, "RYO_EMBED_SHOW_COLLIDERS"),
      show_velocities: parse_embedded(&lookup, "RYO_EMBED_SHOW_VELOCITIES"),
      show_origins: parse_embedded(&lookup, "RYO_EMBED_SHOW_ORIGINS"),
      show_frame_timing: parse_embedded(&lookup, "RYO_EMBED_SHOW_FRAME_TIMING"),
      enable_profiler: parse_embedded(&lookup, "RYO_EMBED_ENABLE_PROFILER"),
      disable_debug_keys: parse_embedded(&lookup, "RYO_EMBED_DISABLE_DEBUG_KEYS"),
      logger_window: parse_embedded(&lookup, "RYO_EMBED_LOGGER_WINDOW"),
      dev_window_keys: parse_embedded(&lookup, "RYO_EMBED_DEV_WINDOW_KEYS"),
      dev_console: parse_embedded(&lookup, "RYO_EMBED_CONSOLE"),
      background_fps: parse_embedded(&lookup, "RYO_EMBED_BACKGROUND_FPS"),
      background_rendering: parse_embedded(&lookup, "RYO_EMBED_BACKGROUND_RENDERING"),
      icon: None,
    })
  }
}

/// Values embedded into this binary at compile time, in [`EMBEDDED_KEYS`] order.
const COMPILED_EMBEDDED: &[(&str, Option<&str>)] = &[
  ("RYO_EMBED_RESOLUTION", option_env!("RYO_EMBED_RESOLUTION")),
  ("RYO_EMBED_VSYNC", option_env!("RYO_EMBED_VSYNC")),
  (
    "RYO_EMBED_DISPLAY_MODE",
    option_env!("RYO_EMBED_DISPLAY_MODE"),
  ),
  (
    "RYO_EMBED_MASTER_VOLUME",
    option_env!("RYO_EMBED_MASTER_VOLUME"),
  ),
  (
    "RYO_EMBED_MUSIC_VOLUME",
    option_env!("RYO_EMBED_MUSIC_VOLUME"),
  ),
  ("RYO_EMBED_SFX_VOLUME", option_env!("RYO_EMBED_SFX_VOLUME")),
  ("RYO_EMBED_CONSOLE", option_env!("RYO_EMBED_CONSOLE")),
  ("RYO_EMBED_TITLE", option_env!("RYO_EMBED_TITLE")),
  ("RYO_EMBED_SHOW_FPS", option_env!("RYO_EMBED_SHOW_FPS")),
  (
    "RYO_EMBED_SHOW_COLLIDERS",
    option_env!("RYO_EMBED_SHOW_COLLIDERS"),
  ),
  (
    "RYO_EMBED_SHOW_VELOCITIES",
    option_env!("RYO_EMBED_SHOW_VELOCITIES"),
  ),
  (
    "RYO_EMBED_SHOW_ORIGINS",
    option_env!("RYO_EMBED_SHOW_ORIGINS"),
  ),
  (
    "RYO_EMBED_SHOW_FRAME_TIMING",
    option_env!("RYO_EMBED_SHOW_FRAME_TIMING"),
  ),
  (
    "RYO_EMBED_DISABLE_DEBUG_KEYS",
    option_env!("RYO_EMBED_DISABLE_DEBUG_KEYS"),
  ),
  (
    "RYO_EMBED_ENABLE_PROFILER",
    option_env!("RYO_EMBED_ENABLE_PROFILER"),
  ),
  (
    "RYO_EMBED_DEV_WINDOW_KEYS",
    option_env!("RYO_EMBED_DEV_WINDOW_KEYS"),
  ),
  (
    "RYO_EMBED_LOGGER_WINDOW",
    option_env!("RYO_EMBED_LOGGER_WINDOW"),
  ),
  (
    "RYO_EMBED_BACKGROUND_FPS",
    option_env!("RYO_EMBED_BACKGROUND_FPS"),
  ),
  (
    "RYO_EMBED_BACKGROUND_RENDERING",
    option_env!("RYO_EMBED_BACKGROUND_RENDERING"),
  ),
];

/// A value embedded into this binary by `ryo2d build`.
pub(crate) fn compiled_embedded_value(name: &str) -> Option<&'static str> {
  COMPILED_EMBEDDED
    .iter()
    .find(|(key, _)| *key == name)
    .and_then(|(_, value)| *value)
}

fn parse_embedded<'a, T: std::str::FromStr>(
  lookup: &impl Fn(&str) -> Option<&'a str>,
  name: &str,
) -> Option<T> {
  let raw = lookup(name)?;
  raw.parse().ok().or_else(|| {
    warn_malformed(name, raw);
    None
  })
}

fn parse_display_mode(mode: &str) -> DisplayMode {
  match mode.to_lowercase().as_str() {
    "exclusive_fullscreen" => DisplayMode::ExclusiveFullscreen,
    "borderless_fullscreen" => DisplayMode::BorderlessFullscreen,
    _ => DisplayMode::Windowed,
  }
}

fn warn_malformed(name: &str, value: &str) {
  eprintln!("[ryo2d] Warning: Malformed embedded value {name}='{value}', using the default");
}

#[cfg(test)]
mod tests {
  use super::*;

  fn builder_with_everything() -> BuilderOverrides {
    BuilderOverrides {
      title: Some("Builder".to_string()),
      resolution: Some((640, 480)),
      display_mode: Some(DisplayMode::ExclusiveFullscreen),
      vsync: Some(false),
      show_fps: Some(true),
      show_colliders: Some(true),
      show_velocities: Some(true),
      show_origins: Some(true),
      background_fps: Some(30),
      background_rendering: Some(false),
      icon: Some(PathBuf::from("builder.png")),
    }
  }

  fn project_with_everything() -> ProjectLayer {
    ProjectLayer {
      title: Some("Project".to_string()),
      resolution: Some((800, 600)),
      display_mode: Some(DisplayMode::BorderlessFullscreen),
      vsync: Some(true),
      master_volume: Some(0.3),
      music_volume: Some(0.4),
      sfx_volume: Some(0.5),
      show_fps: Some(false),
      show_colliders: Some(false),
      show_velocities: Some(false),
      show_origins: Some(false),
      show_frame_timing: Some(true),
      enable_profiler: Some(true),
      disable_debug_keys: Some(true),
      logger_window: Some(true),
      dev_window_keys: Some(false),
      dev_console: Some(true),
      background_fps: Some(15),
      background_rendering: Some(true),
      icon: Some(PathBuf::from("project.png")),
    }
  }

  fn prefs_with_everything() -> StoredPreferences {
    StoredPreferences {
      resolution: Some((1920, 1080)),
      display_mode: Some(DisplayMode::Windowed),
      vsync: Some(true),
      master_volume: Some(0.1),
      music_volume: Some(0.2),
      sfx_volume: Some(0.9),
      show_fps: Some(false),
    }
  }

  #[test]
  fn defaults_apply_when_no_layer_sets_anything() {
    let s = resolve_settings(&ProjectLayer::default(), &BuilderOverrides::default(), None);
    assert_eq!(s.title, "Ryo2D");
    assert_eq!(s.resolution, (1280, 720));
    assert_eq!(s.display_mode, DisplayMode::Windowed);
    assert!(s.vsync);
    assert_eq!(
      (s.master_volume, s.music_volume, s.sfx_volume),
      (0.8, 1.0, 1.0)
    );
    assert!(!s.disable_debug_keys);
    assert_eq!((s.background_fps, s.background_rendering), (0, true));
    assert_eq!(s.icon, None);
  }

  #[test]
  fn project_overrides_defaults() {
    let s = resolve_settings(
      &project_with_everything(),
      &BuilderOverrides::default(),
      None,
    );
    assert_eq!(s.title, "Project");
    assert_eq!(s.resolution, (800, 600));
    assert_eq!(s.display_mode, DisplayMode::BorderlessFullscreen);
    assert_eq!(
      (s.master_volume, s.music_volume, s.sfx_volume),
      (0.3, 0.4, 0.5)
    );
    assert!(s.show_frame_timing && s.enable_profiler && s.disable_debug_keys);
    assert!(s.logger_window && !s.dev_window_keys && s.dev_console);
    assert_eq!((s.background_fps, s.background_rendering), (15, true));
    assert_eq!(s.icon, Some(PathBuf::from("project.png")));
  }

  #[test]
  fn builder_overrides_project() {
    let s = resolve_settings(&project_with_everything(), &builder_with_everything(), None);
    assert_eq!(s.title, "Builder");
    assert_eq!(s.resolution, (640, 480));
    assert_eq!(s.display_mode, DisplayMode::ExclusiveFullscreen);
    assert!(!s.vsync);
    assert!(s.show_fps && s.show_colliders && s.show_velocities && s.show_origins);
    assert_eq!((s.background_fps, s.background_rendering), (30, false));
    assert_eq!(s.icon, Some(PathBuf::from("builder.png")));
    // The builder has no volume setters, so the project's volumes stay
    assert_eq!(s.master_volume, 0.3);
  }

  #[test]
  fn preferences_override_builder() {
    let prefs = prefs_with_everything();
    let filter = UserPreferenceFilter::all();
    let s = resolve_settings(
      &project_with_everything(),
      &builder_with_everything(),
      Some((&prefs, &filter)),
    );
    assert_eq!(s.resolution, (1920, 1080));
    assert_eq!(s.display_mode, DisplayMode::Windowed);
    assert!(s.vsync);
    assert_eq!(
      (s.master_volume, s.music_volume, s.sfx_volume),
      (0.1, 0.2, 0.9)
    );
    assert!(!s.show_fps);
  }

  #[test]
  fn preferences_outside_the_filter_are_ignored() {
    let prefs = prefs_with_everything();
    let filter = UserPreferenceFilter::only([UserPreference::MasterVolume]);
    let s = resolve_settings(
      &project_with_everything(),
      &builder_with_everything(),
      Some((&prefs, &filter)),
    );
    assert_eq!(s.master_volume, 0.1);
    assert_eq!(s.resolution, (640, 480));
    assert!(!s.vsync);
    assert_eq!(s.music_volume, 0.4);
  }

  #[test]
  fn absent_preferences_fall_through_to_builder_and_project() {
    let prefs = StoredPreferences::default();
    let filter = UserPreferenceFilter::all();
    let s = resolve_settings(
      &project_with_everything(),
      &builder_with_everything(),
      Some((&prefs, &filter)),
    );
    assert_eq!(s.resolution, (640, 480));
    assert!(!s.vsync);
    assert_eq!(s.master_volume, 0.3);
    assert!(s.show_fps);
  }

  #[test]
  fn from_workspace_maps_every_field() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
      dir.path().join("Ryo.toml"),
      r#"
[project]
title = "Layer Test"

[system]
resolution = "800x600"
vsync = false
display_mode = "borderless_fullscreen"
master_volume = 0.3
music_volume = 0.4
sfx_volume = 0.5
show_fps = true
console = true
target_background_fps = 15
background_rendering = false

[debug]
show_colliders = true
show_velocities = true
show_origins = true
show_frame_timing = true
disable_debug_keys = true
enable_profiler = true
logger_window = true
dev_window_keys = false

[release]
logo = "art/logo.png"
"#,
    )
    .unwrap();
    let ws = Workspace::from_path(dir.path()).unwrap();

    let layer = ProjectLayer::from_workspace(&ws);

    assert_eq!(layer.title.as_deref(), Some("Layer Test"));
    assert_eq!(layer.resolution, Some((800, 600)));
    assert_eq!(layer.vsync, Some(false));
    assert_eq!(layer.display_mode, Some(DisplayMode::BorderlessFullscreen));
    assert_eq!(layer.master_volume, Some(0.3));
    assert_eq!(layer.music_volume, Some(0.4));
    assert_eq!(layer.sfx_volume, Some(0.5));
    assert_eq!(layer.show_fps, Some(true));
    assert_eq!(layer.dev_console, Some(true));
    assert_eq!(layer.background_fps, Some(15));
    assert_eq!(layer.background_rendering, Some(false));
    assert_eq!(layer.show_colliders, Some(true));
    assert_eq!(layer.show_velocities, Some(true));
    assert_eq!(layer.show_origins, Some(true));
    assert_eq!(layer.show_frame_timing, Some(true));
    assert_eq!(layer.disable_debug_keys, Some(true));
    assert_eq!(layer.enable_profiler, Some(true));
    assert_eq!(layer.logger_window, Some(true));
    assert_eq!(layer.dev_window_keys, Some(false));
    assert_eq!(layer.icon, Some(dir.path().join("art/logo.png")));
  }

  fn embedded(name: &str) -> Option<&'static str> {
    match name {
      "RYO_EMBED_RESOLUTION" => Some("800x600"),
      "RYO_EMBED_VSYNC" => Some("false"),
      "RYO_EMBED_DISPLAY_MODE" => Some("exclusive_fullscreen"),
      "RYO_EMBED_MASTER_VOLUME" => Some("0.3"),
      "RYO_EMBED_MUSIC_VOLUME" => Some("0.4"),
      "RYO_EMBED_SFX_VOLUME" => Some("0.5"),
      "RYO_EMBED_CONSOLE" => Some("true"),
      "RYO_EMBED_TITLE" => Some("Shipped"),
      "RYO_EMBED_SHOW_FPS" => Some("true"),
      "RYO_EMBED_SHOW_COLLIDERS" => Some("true"),
      "RYO_EMBED_SHOW_VELOCITIES" => Some("true"),
      "RYO_EMBED_SHOW_ORIGINS" => Some("true"),
      "RYO_EMBED_SHOW_FRAME_TIMING" => Some("true"),
      "RYO_EMBED_DISABLE_DEBUG_KEYS" => Some("true"),
      "RYO_EMBED_ENABLE_PROFILER" => Some("true"),
      "RYO_EMBED_DEV_WINDOW_KEYS" => Some("true"),
      "RYO_EMBED_LOGGER_WINDOW" => Some("true"),
      "RYO_EMBED_BACKGROUND_FPS" => Some("15"),
      "RYO_EMBED_BACKGROUND_RENDERING" => Some("false"),
      _ => None,
    }
  }

  #[test]
  fn from_embedded_reads_every_key() {
    let layer = ProjectLayer::from_embedded(embedded).unwrap();
    assert_eq!(layer.title.as_deref(), Some("Shipped"));
    assert_eq!(layer.resolution, Some((800, 600)));
    assert_eq!(layer.vsync, Some(false));
    assert_eq!(layer.display_mode, Some(DisplayMode::ExclusiveFullscreen));
    assert_eq!(layer.master_volume, Some(0.3));
    assert_eq!(layer.music_volume, Some(0.4));
    assert_eq!(layer.sfx_volume, Some(0.5));
    assert_eq!(layer.dev_console, Some(true));
    assert_eq!(layer.show_fps, Some(true));
    assert_eq!(layer.show_colliders, Some(true));
    assert_eq!(layer.show_velocities, Some(true));
    assert_eq!(layer.show_origins, Some(true));
    assert_eq!(layer.show_frame_timing, Some(true));
    assert_eq!(layer.disable_debug_keys, Some(true));
    assert_eq!(layer.enable_profiler, Some(true));
    assert_eq!(layer.dev_window_keys, Some(true));
    assert_eq!(layer.logger_window, Some(true));
    assert_eq!(layer.background_fps, Some(15));
    assert_eq!(layer.background_rendering, Some(false));
    assert_eq!(layer.icon, None);
  }

  #[test]
  fn from_embedded_is_none_without_a_ryo2d_build() {
    assert_eq!(ProjectLayer::from_embedded(|_| None), None);
  }

  #[test]
  fn malformed_embedded_value_only_drops_that_setting() {
    let layer = ProjectLayer::from_embedded(|name| match name {
      "RYO_EMBED_MASTER_VOLUME" => Some("loud"),
      "RYO_EMBED_RESOLUTION" => Some("huge"),
      other => embedded(other),
    })
    .unwrap();
    assert_eq!(layer.master_volume, None);
    assert_eq!(layer.resolution, None);
    assert_eq!(layer.music_volume, Some(0.4));
    assert_eq!(layer.disable_debug_keys, Some(true));
  }

  #[test]
  fn compiled_table_lists_exactly_the_embedded_keys() {
    let compiled: Vec<&str> = COMPILED_EMBEDDED.iter().map(|(key, _)| *key).collect();
    assert_eq!(compiled, EMBEDDED_KEYS);
    assert_eq!(compiled_embedded_value("RYO_EMBED_NOT_A_KEY"), None);
  }

  #[test]
  fn seed_state_carries_resolved_values() {
    let s = resolve_settings(&project_with_everything(), &builder_with_everything(), None);
    let mut env = HashMap::new();
    env.insert("difficulty".to_string(), serde_json::json!("hard"));

    let state = s.to_system_state(env);

    assert_eq!((state.width, state.height), (640, 480));
    assert_eq!(state.display_mode, DisplayMode::ExclusiveFullscreen);
    assert_eq!(state.vsync, Some(false));
    assert_eq!(state.show_fps, Some(true));
    assert_eq!(
      (state.master_volume, state.music_volume, state.sfx_volume),
      (0.3, 0.4, 0.5)
    );
    assert_eq!(state.get_env_str("difficulty"), Some("hard"));
  }
}
