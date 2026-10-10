# Config Precedence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every startup setting resolves in one order — engine defaults → embedded values (release) or `Ryo.toml` (dev) → builder calls → saved user preferences present in `system.json` — in dev and in shipped builds.

**Architecture:** A new pure module `src/core/config/resolve.rs` layers four inputs (`ProjectLayer`, `BuilderOverrides`, `StoredPreferences` + filter) into `ResolvedSettings`. `EngineBuilder` stores its explicit values as `Option`s and calls the resolver from `resolve_in(workspace, headless)`. `save.rs` learns to report which preference keys a `system.json` actually contains, seeds new files from resolved values, drops the resolution migration, and persists changed keys missing from the file. `ryo2d build` embeds the remaining Ryo.toml values so release matches dev.

**Tech Stack:** Rust 2024, serde/serde_json, toml, tempfile (dev-dependency).

**Spec:** `docs/plans/2026-10-10-config-precedence-design.md`

## Global Constraints

- Rust 2024 edition, 2-space indentation, no `unwrap`/`expect` in library code (tests may use them).
- Format only files you change, with `rustfmt --edition 2024 <file>`. Never run rustfmt on `src/lib.rs`, `src/core/mod.rs` or `src/core/config/mod.rs` (it recurses into submodules) — use `rustfmt --edition 2024 --check <file>` on those and fix only lines you wrote.
- CRLF files must stay CRLF on every line: `src/core/scene.rs` and the skill files under `src/cli/skills/` and `.claude/skills/`. Edit them byte-preservingly and check with `file <path>`.
- Volume defaults are unified at master 0.8, music 1.0, sfx 1.0.
- Warnings printed during resolution use the existing style: `eprintln!("[ryo2d] Warning: ...")`.
- Headless builds (`resolve_in(_, true)`) never read or write `system.json`.
- Commit on `main`, one commit per task, with the message given in the task. No co-author lines. Do not push.
- Docs: no new mdbook sections (mdbook is being retired); only the minimal corrections in Task 5.
- Done means: `cargo test --workspace` 0 failed and `cargo clippy --workspace --all-targets` with no new warnings in touched files.

## Review Focus

1. A full `system.json` written by the previous engine version (every key present) must load unchanged and re-save without losing or gaining keys — Task 2 `full_file_from_previous_version_round_trips`.
2. A malformed or wrong-typed `system.json` (`not json`, `{"vsync": "yes"}`) must be ignored with a warning, never overwritten at startup, and the game must still start with builder/project values — Task 2 `malformed_file_is_an_error_and_left_alone`, Task 3 `unreadable_system_json_is_ignored_and_left_alone`.
3. A save directory that can't be created must not panic; startup falls back to builder/project values — Task 3 `unusable_save_dir_falls_back_to_project_settings`.
4. Repeated builder calls keep the last value, and repeated `with_user_preferences` calls combine their filters — Task 3 `repeated_builder_calls_and_filters`.
5. One malformed embedded value (e.g. `RYO_EMBED_MASTER_VOLUME=loud`) must only lose that setting; every other embedded value still applies — Task 1 `malformed_embedded_value_only_drops_that_setting`.

---

### Task 1: Pure layered resolver (`config/resolve.rs`)

**Files:**
- Create: `src/core/config/resolve.rs`
- Modify: `src/core/config/mod.rs` (add `pub(crate) mod resolve;` after `pub mod save;`)

**Interfaces:**
- Consumes: `super::debug::DebugConfig` (runtime debug flags, `Default`), `super::save::{DisplayMode, SystemState, UserPreference, UserPreferenceFilter, parse_resolution}`, `super::workspace::Workspace`.
- Produces (all `pub(crate)`):
  - consts `DEFAULT_RESOLUTION: (u32, u32) = (1280, 720)`, `DEFAULT_MASTER_VOLUME: f32 = 0.8`, `DEFAULT_MUSIC_VOLUME: f32 = 1.0`, `DEFAULT_SFX_VOLUME: f32 = 1.0`, `EMBEDDED_KEYS: &[&str]`
  - `struct BuilderOverrides` (fields below, `Default`)
  - `struct ProjectLayer` with `fn from_workspace(&Workspace) -> Self` and `fn from_embedded<'a>(lookup: impl Fn(&str) -> Option<&'a str>) -> Option<Self>`
  - `fn compiled_embedded_value(name: &str) -> Option<&'static str>`
  - `struct StoredPreferences` (fields below, `Default`)
  - `struct ResolvedSettings` with `fn to_system_state(&self, env: HashMap<String, serde_json::Value>) -> SystemState`
  - `fn resolve_settings(project: &ProjectLayer, builder: &BuilderOverrides, prefs: Option<(&StoredPreferences, &UserPreferenceFilter)>) -> ResolvedSettings`

Dead-code warnings for these items are expected until Task 3 wires them in. Do not add `allow` attributes.

- [ ] **Step 1: Create the module with types and a stub resolver**

Create `src/core/config/resolve.rs`:

```rust
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
use super::save::{DisplayMode, SystemState, UserPreference, UserPreferenceFilter, parse_resolution};
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
}

/// Layer project values, builder calls and saved preferences over the engine defaults.
pub(crate) fn resolve_settings(
  project: &ProjectLayer,
  builder: &BuilderOverrides,
  prefs: Option<(&StoredPreferences, &UserPreferenceFilter)>,
) -> ResolvedSettings {
  let _ = (project, builder, prefs);
  ResolvedSettings::defaults()
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
    assert_eq!((s.master_volume, s.music_volume, s.sfx_volume), (0.8, 1.0, 1.0));
    assert!(!s.disable_debug_keys);
    assert_eq!((s.background_fps, s.background_rendering), (0, true));
    assert_eq!(s.icon, None);
  }

  #[test]
  fn project_overrides_defaults() {
    let s = resolve_settings(&project_with_everything(), &BuilderOverrides::default(), None);
    assert_eq!(s.title, "Project");
    assert_eq!(s.resolution, (800, 600));
    assert_eq!(s.display_mode, DisplayMode::BorderlessFullscreen);
    assert_eq!((s.master_volume, s.music_volume, s.sfx_volume), (0.3, 0.4, 0.5));
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
    assert_eq!((s.master_volume, s.music_volume, s.sfx_volume), (0.1, 0.2, 0.9));
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
}
```

In `src/core/config/mod.rs`, after `pub mod save;` add:

```rust
pub(crate) mod resolve;
```

- [ ] **Step 2: Run the tests to watch them fail**

Run: `cargo test --lib config::resolve 2>&1 | tail -20`
Expected: `defaults_apply_when_no_layer_sets_anything` passes (the stub returns defaults); `project_overrides_defaults`, `builder_overrides_project`, `preferences_override_builder`, `preferences_outside_the_filter_are_ignored` and `absent_preferences_fall_through_to_builder_and_project` FAIL on their first assertion.

- [ ] **Step 3: Implement `resolve_settings`**

Replace the stub body with:

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib config::resolve 2>&1 | tail -5`
Expected: 6 passed, 0 failed.

- [ ] **Step 5: Write failing tests for the project-layer constructors and seeding**

Add these tests to the `tests` module:

```rust
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
    assert_eq!((state.master_volume, state.music_volume, state.sfx_volume), (0.3, 0.4, 0.5));
    assert_eq!(state.get_env_str("difficulty"), Some("hard"));
  }
```

- [ ] **Step 6: Run them to watch them fail**

Run: `cargo test --lib config::resolve 2>&1 | tail -20`
Expected: compile error — `from_workspace`, `from_embedded`, `COMPILED_EMBEDDED`, `compiled_embedded_value` and `to_system_state` don't exist.

- [ ] **Step 7: Implement the constructors, the compiled lookup and seeding**

Add to `resolve.rs` (after `ResolvedSettings::defaults`, inside the same `impl ResolvedSettings` block for `to_system_state`):

```rust
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
```

And a new `impl ProjectLayer` block plus helpers:

```rust
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
  ("RYO_EMBED_DISPLAY_MODE", option_env!("RYO_EMBED_DISPLAY_MODE")),
  ("RYO_EMBED_MASTER_VOLUME", option_env!("RYO_EMBED_MASTER_VOLUME")),
  ("RYO_EMBED_MUSIC_VOLUME", option_env!("RYO_EMBED_MUSIC_VOLUME")),
  ("RYO_EMBED_SFX_VOLUME", option_env!("RYO_EMBED_SFX_VOLUME")),
  ("RYO_EMBED_CONSOLE", option_env!("RYO_EMBED_CONSOLE")),
  ("RYO_EMBED_TITLE", option_env!("RYO_EMBED_TITLE")),
  ("RYO_EMBED_SHOW_FPS", option_env!("RYO_EMBED_SHOW_FPS")),
  ("RYO_EMBED_SHOW_COLLIDERS", option_env!("RYO_EMBED_SHOW_COLLIDERS")),
  ("RYO_EMBED_SHOW_VELOCITIES", option_env!("RYO_EMBED_SHOW_VELOCITIES")),
  ("RYO_EMBED_SHOW_ORIGINS", option_env!("RYO_EMBED_SHOW_ORIGINS")),
  ("RYO_EMBED_SHOW_FRAME_TIMING", option_env!("RYO_EMBED_SHOW_FRAME_TIMING")),
  ("RYO_EMBED_DISABLE_DEBUG_KEYS", option_env!("RYO_EMBED_DISABLE_DEBUG_KEYS")),
  ("RYO_EMBED_ENABLE_PROFILER", option_env!("RYO_EMBED_ENABLE_PROFILER")),
  ("RYO_EMBED_DEV_WINDOW_KEYS", option_env!("RYO_EMBED_DEV_WINDOW_KEYS")),
  ("RYO_EMBED_LOGGER_WINDOW", option_env!("RYO_EMBED_LOGGER_WINDOW")),
  ("RYO_EMBED_BACKGROUND_FPS", option_env!("RYO_EMBED_BACKGROUND_FPS")),
  ("RYO_EMBED_BACKGROUND_RENDERING", option_env!("RYO_EMBED_BACKGROUND_RENDERING")),
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
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test --lib config::resolve 2>&1 | tail -5`
Expected: 12 passed, 0 failed.

- [ ] **Step 9: Format, lint, commit**

Run: `rustfmt --edition 2024 src/core/config/resolve.rs && rustfmt --edition 2024 --check src/core/config/mod.rs && cargo clippy --lib --tests 2>&1 | grep -A3 "config/resolve.rs" | grep -v "never used\|never constructed\|never read" | head`
Expected: rustfmt check clean; clippy shows nothing for `resolve.rs` except dead-code warnings.

```bash
git add src/core/config/resolve.rs src/core/config/mod.rs
git commit -m "feat(config): add layered settings resolver"
```

---

### Task 2: `system.json` reports present keys, seeds, and saves missing keys

**Files:**
- Modify: `src/core/config/save.rs` (SystemState deserialize ~441-539, Default ~541-555, `merge_defaults` ~581-615, `save_system` ~842-898, `load_or_create_system_filtered` ~961-1023, tests module)
- Modify: `src/core/config/workspace.rs` (`impl Default for SystemDefaults` volumes ~220-236)

**Interfaces:**
- Consumes (Task 1): `super::resolve::{StoredPreferences, DEFAULT_RESOLUTION, DEFAULT_MASTER_VOLUME, DEFAULT_MUSIC_VOLUME, DEFAULT_SFX_VOLUME}`.
- Produces:
  - `SaveManager::load_stored_preferences(&self) -> SaveResult<Option<StoredPreferences>>` (`pub(crate)`): `Ok(None)` when no `system.json`; `Err` when the file can't be parsed.
  - `SaveManager::load_preferences_or_seed(&self, seed: &SystemState, filter: &UserPreferenceFilter, defaults: &super::workspace::SystemDefaults) -> SaveResult<Option<StoredPreferences>>` (`pub(crate)`): no file → writes `seed`'s filtered keys (plus `env` when `seed.env` is non-empty) and returns `Ok(None)`; file present → merges new `defaults.env` keys into it and returns `Ok(Some(prefs))`; unparsable file → `Err` and the file is left untouched.
  - Public behaviour changes: `merge_defaults` no longer migrates resolution; `save_system` writes a key missing from the file when the state's value differs from what the file loads as; `load_or_create_system_filtered` returns the state as it loads from disk after creating a file.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module of `save.rs` (it already imports `tempdir` and `HashMap`):

```rust
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
    write_system_json(dir.path(), r#"{"master_volume": 0.5, "resolution": "1600x900"}"#);

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
    let filter = UserPreferenceFilter::only([UserPreference::Resolution, UserPreference::MasterVolume]);

    let prefs = manager
      .load_preferences_or_seed(&seed, &filter, &super::super::workspace::SystemDefaults::default())
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

    manager.save_system(&manager.load_system().unwrap()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(previous).unwrap();
    assert_eq!(read_system_json(dir.path()), expected);
  }
```

Invert the two migration tests. Replace `system_state_merge_defaults_migrates_resolution` (save.rs ~1622) with:

```rust
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
```

Replace `load_or_create_system_merges_missing_fields` (save.rs ~1730) with:

```rust
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
```

- [ ] **Step 2: Run the tests to watch them fail**

Run: `cargo test --lib config::save 2>&1 | tail -30`
Expected: compile error — `load_stored_preferences` and `load_preferences_or_seed` don't exist. (Temporarily commenting the new tests out would show `merge_defaults_keeps_a_saved_720p_resolution`, `load_or_create_system_keeps_a_saved_720p_resolution` and `save_system_writes_a_changed_key_missing_from_the_file` failing on assertions; that is the RED for those three.)

- [ ] **Step 3: Replace the `SystemState` deserializer with a raw file model**

In `save.rs`, add near the top imports:

```rust
use super::resolve::{
  DEFAULT_MASTER_VOLUME, DEFAULT_MUSIC_VOLUME, DEFAULT_RESOLUTION, DEFAULT_SFX_VOLUME,
  StoredPreferences,
};
```

Replace the whole `impl<'de> Deserialize<'de> for SystemState { ... }` block with:

```rust
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
        eprintln!("[ryo2d] Warning: Malformed resolution '{}' in system.json, ignoring it", res);
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
```

In `impl Default for SystemState`, use the constants:

```rust
      master_volume: DEFAULT_MASTER_VOLUME,
      music_volume: DEFAULT_MUSIC_VOLUME,
      sfx_volume: DEFAULT_SFX_VOLUME,
      width: DEFAULT_RESOLUTION.0,
      height: DEFAULT_RESOLUTION.1,
```

In `workspace.rs` `impl Default for SystemDefaults`, use `super::resolve::DEFAULT_MASTER_VOLUME`, `DEFAULT_MUSIC_VOLUME`, `DEFAULT_SFX_VOLUME` for the three volumes.

- [ ] **Step 4: Remove the resolution migration**

Replace `merge_defaults` (doc and body) with:

```rust
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
```

- [ ] **Step 5: Make `save_system` persist changed keys missing from the file**

Replace `save_system` with:

```rust
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
          Err(_) => serde_json::Map::new(),
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
```

Add these private helpers: `system_path` and `write_system_map` inside `impl SaveManager`, and `system_state_map` as a free function next to `clean_f32`:

```rust
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
```

```rust
/// A `SystemState` as the JSON object `system.json` stores.
fn system_state_map(state: &SystemState) -> SaveResult<serde_json::Map<String, serde_json::Value>> {
  match serde_json::to_value(state).map_err(|e| SaveError::Serialization(e.to_string()))? {
    serde_json::Value::Object(map) => Ok(map),
    _ => Err(SaveError::Serialization(
      "system state did not serialize to a JSON object".to_string(),
    )),
  }
}
```

Use `self.system_path()` in place of the repeated `self.save_dir.join(format!("system.{}", SYSTEM_EXTENSION))` in the system-state methods you touch.

- [ ] **Step 6: Add the preference loader and seeding; make filtered creation return what's on disk**

Add to `impl SaveManager` (system-state section):

```rust
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
    let path = self.system_path();
    if !path.exists() {
      self.write_filtered_system(seed, filter, !seed.env.is_empty())?;
      return Ok(None);
    }

    let raw: RawSystemFile = self.read_json_file(&path)?;
    let prefs = raw.stored_preferences();
    let mut state = raw.into_state();
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
```

Replace `load_or_create_system_filtered` (doc and body) with:

```rust
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
```

- [ ] **Step 7: Run the save tests**

Run: `cargo test --lib config::save 2>&1 | tail -8`
Expected: all pass, including the 10 new tests, the two inverted tests, `save_system_preserves_filtered_structure`, `filtered_creation_writes_only_requested_keys` and `filtered_file_loads_back_with_defaults_for_missing_fields`.

- [ ] **Step 8: Run the integration tests that use `SystemState`**

Run: `cargo test --test integration config_resolution 2>&1 | tail -5`
Expected: all pass (deserialization behaviour for `SystemState` is unchanged).

- [ ] **Step 9: Format, lint, commit**

Run: `rustfmt --edition 2024 src/core/config/save.rs src/core/config/workspace.rs && cargo clippy --lib --tests 2>&1 | grep -B2 -A8 "config/save.rs\|config/workspace.rs" | head -40`
Expected: no clippy findings in `save.rs`/`workspace.rs` (dead-code warnings for the new `pub(crate)` methods are expected until Task 3).

```bash
git add src/core/config/save.rs src/core/config/workspace.rs
git commit -m "fix(config): treat missing system.json keys as no preference

- Report which preference keys system.json holds; seed new files from given values
- Stop resetting a saved 1280x720 resolution to Ryo.toml's value on every load
- save_system adds a changed key the file doesn't hold yet"
```

---

### Task 3: Wire `EngineBuilder` to the resolver

**Files:**
- Modify: `src/core/engine.rs` (`EngineBuilder` struct/new/setters ~3432-3740, `with_user_preferences` + `apply_user_preferences` ~3743-3853, `resolve` ~3958-4172, `ResolvedBuild` ~4174-4188, tests ~4196-4560)

**Interfaces:**
- Consumes (Task 1): `config::resolve::{BuilderOverrides, ProjectLayer, ResolvedSettings, StoredPreferences, compiled_embedded_value, resolve_settings}`. (Task 2): `SaveManager::load_preferences_or_seed(&SystemState, &UserPreferenceFilter, &SystemDefaults) -> SaveResult<Option<StoredPreferences>>`.
- Produces: `EngineBuilder::resolve_in(self, workspace: Option<&Workspace>, headless: bool) -> ResolvedBuild` (private); `EngineBuilder` fields `config` (builder-only settings) and `overrides: BuilderOverrides`. `Engine::new`'s signature is unchanged.

- [ ] **Step 1: Write the failing tests**

In `engine.rs`'s `tests` module, add a helper and these tests:

```rust
  fn temp_workspace(
    ryo_toml: &str,
    system_json: Option<&str>,
  ) -> (tempfile::TempDir, crate::core::config::Workspace) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ryo.toml"), ryo_toml).unwrap();
    if let Some(json) = system_json {
      std::fs::create_dir_all(dir.path().join("data")).unwrap();
      std::fs::write(dir.path().join("data/system.json"), json).unwrap();
    }
    let workspace = crate::core::config::Workspace::from_path(dir.path()).unwrap();
    (dir, workspace)
  }

  #[test]
  fn missing_preference_keys_fall_through_to_builder_and_project() {
    let (_dir, ws) = temp_workspace(
      "[system]\nmaster_volume = 0.3\nvsync = true\n",
      Some(r#"{"show_fps": true}"#),
    );
    let r = EngineBuilder::new()
      .resolution(800, 600)
      .display_mode(DisplayMode::BorderlessFullscreen)
      .vsync(false)
      .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false);
    assert_eq!((r.config.width, r.config.height), (800, 600));
    assert_eq!(r.config.display_mode, DisplayMode::BorderlessFullscreen);
    assert_eq!(r.vsync, Some(false));
    assert_eq!(r.master_volume, 0.3);
    assert!(r.config.debug.show_fps);
  }

  #[test]
  fn saved_preference_beats_builder() {
    let (_dir, ws) = temp_workspace("", Some(r#"{"resolution": "1600x900"}"#));
    let r = EngineBuilder::new()
      .resolution(800, 600)
      .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false);
    assert_eq!((r.config.width, r.config.height), (1600, 900));
  }

  #[test]
  fn saved_720p_survives_a_different_ryo_toml_resolution() {
    let (dir, ws) = temp_workspace(
      "[system]\nresolution = \"1920x1080\"\n",
      Some(r#"{"resolution": "1280x720"}"#),
    );
    let r = EngineBuilder::new()
      .with_user_preferences([crate::core::config::UserPreference::Resolution])
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false);
    assert_eq!((r.config.width, r.config.height), (1280, 720));
    let raw = std::fs::read_to_string(dir.path().join("data/system.json")).unwrap();
    assert!(raw.contains("1280x720"));
  }

  #[test]
  fn first_run_seeds_system_json_from_builder_and_project() {
    let (dir, ws) = temp_workspace("[system]\nmaster_volume = 0.3\n", None);
    let r = EngineBuilder::new()
      .resolution(800, 600)
      .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false);
    assert_eq!((r.config.width, r.config.height), (800, 600));
    let raw: serde_json::Value = serde_json::from_str(
      &std::fs::read_to_string(dir.path().join("data/system.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(raw["resolution"], "800x600");
    assert_eq!(raw["master_volume"], 0.3);
  }

  #[test]
  fn builder_debug_and_background_settings_survive_ryo_toml() {
    let (_dir, ws) = temp_workspace(
      "[debug]\nshow_colliders = false\nshow_velocities = false\nshow_origins = false\n\
       [system]\ntarget_background_fps = 5\nbackground_rendering = true\n",
      None,
    );
    let r = EngineBuilder::new()
      .show_colliders(true)
      .show_velocities(true)
      .show_origins(true)
      .background_fps(30)
      .background_rendering(false)
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false);
    assert!(r.config.debug.show_colliders);
    assert!(r.config.debug.show_velocities);
    assert!(r.config.debug.show_origins);
    assert_eq!(r.config.target_background_fps, 30);
    assert!(!r.config.background_rendering);
  }

  #[test]
  fn ryo_toml_settings_apply_without_builder_calls() {
    let (_dir, ws) = temp_workspace(
      "[project]\ntitle = \"From Toml\"\n[debug]\nshow_colliders = true\ndisable_debug_keys = true\n\
       [system]\nshow_fps = true\ntarget_background_fps = 5\n",
      None,
    );
    let r = EngineBuilder::new()
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false);
    assert_eq!(r.config.title, "From Toml");
    assert!(r.config.debug.show_colliders);
    assert!(r.config.debug.show_fps);
    assert!(r.disable_debug_keys);
    assert_eq!(r.config.target_background_fps, 5);
  }

  #[test]
  fn builder_title_beats_ryo_toml_even_when_it_is_the_engine_default() {
    let (_dir, ws) = temp_workspace("[project]\ntitle = \"From Toml\"\n", None);
    let r = EngineBuilder::new()
      .title("Ryo2D")
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false);
    assert_eq!(r.config.title, "Ryo2D");
  }

  #[test]
  fn headless_resolve_never_reads_or_creates_system_json() {
    let (dir, ws) = temp_workspace("", None);
    EngineBuilder::new()
      .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), true);
    assert!(!dir.path().join("data/system.json").exists());

    let (_dir, ws) = temp_workspace("", Some(r#"{"resolution": "1920x1080"}"#));
    let r = EngineBuilder::new()
      .resolution(320, 180)
      .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), true);
    assert_eq!((r.config.width, r.config.height), (320, 180));
  }

  #[test]
  fn unreadable_system_json_is_ignored_and_left_alone() {
    let (dir, ws) = temp_workspace("", Some("not json"));
    let r = EngineBuilder::new()
      .resolution(800, 600)
      .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false);
    assert_eq!((r.config.width, r.config.height), (800, 600));
    assert_eq!(
      std::fs::read_to_string(dir.path().join("data/system.json")).unwrap(),
      "not json"
    );
  }

  #[test]
  fn unusable_save_dir_falls_back_to_project_settings() {
    // The save directory path is an existing file, so it can't be created
    let (_dir, ws) = temp_workspace(
      "[paths]\nsave_data = \"Ryo.toml\"\n[system]\nmaster_volume = 0.3\n",
      None,
    );
    let r = EngineBuilder::new()
      .resolution(800, 600)
      .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false);
    assert_eq!((r.config.width, r.config.height), (800, 600));
    assert_eq!(r.master_volume, 0.3);
  }

  #[test]
  fn repeated_builder_calls_and_filters() {
    use crate::core::config::UserPreference;
    let (_dir, ws) = temp_workspace(
      "",
      Some(r#"{"master_volume": 0.2, "resolution": "1600x900", "vsync": false}"#),
    );
    let r = EngineBuilder::new()
      .vsync(false)
      .vsync(true)
      .with_user_preferences([UserPreference::MasterVolume])
      .with_user_preferences([UserPreference::Resolution])
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false);
    assert_eq!(r.master_volume, 0.2);
    assert_eq!((r.config.width, r.config.height), (1600, 900));
    // Vsync isn't in either filter: the last builder call wins
    assert_eq!(r.vsync, Some(true));
  }
```

Delete these tests (they poke removed fields; the resolver tests cover their behaviour): `workspace_show_fps_applies_without_builder_call`, `engine_builder_vsync_user_pref_blocks_builder`, `engine_builder_resolution_user_pref_blocks_builder`, `engine_builder_volume_user_pref_blocks_ryo_toml`.

Rewrite `builder_show_fps_survives_workspace_config` to not depend on the repo's own Ryo.toml:

```rust
  #[test]
  fn builder_show_fps_survives_workspace_config() {
    let (_dir, ws) = temp_workspace("[system]\nshow_fps = false\n", None);
    let resolved = EngineBuilder::new()
      .show_fps(true)
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false);
    assert!(resolved.config.debug.show_fps);
  }
```

Update the builder field tests to read `overrides`:

```rust
  #[test]
  fn user_preferences_are_deferred_until_build() {
    let builder =
      EngineBuilder::new().with_user_preferences(crate::core::config::UserPreferenceFilter::all());
    assert_eq!(builder.user_preferences.len(), 1);
    assert_eq!(builder.overrides, Default::default());
  }

  #[test]
  fn engine_builder_sets_title() {
    let builder = EngineBuilder::new().title("Test Game");
    assert_eq!(builder.overrides.title.as_deref(), Some("Test Game"));
  }

  #[test]
  fn engine_builder_sets_resolution() {
    let builder = EngineBuilder::new().resolution(1920, 1080);
    assert_eq!(builder.overrides.resolution, Some((1920, 1080)));
  }

  #[test]
  fn engine_builder_chains() {
    let builder = EngineBuilder::new()
      .title("Chained")
      .resolution(640, 480)
      .target_fps(30)
      .physics_hz(50);

    assert_eq!(builder.overrides.title.as_deref(), Some("Chained"));
    assert_eq!(builder.overrides.resolution, Some((640, 480)));
    assert_eq!(builder.config.target_fps, 30);
    assert_eq!(builder.config.physics_hz, 50);
  }

  #[test]
  fn engine_builder_sets_background_fps() {
    let builder = EngineBuilder::new().background_fps(10);
    assert_eq!(builder.overrides.background_fps, Some(10));
  }

  #[test]
  fn engine_builder_sets_background_fps_uncapped() {
    let builder = EngineBuilder::new().background_fps(0);
    assert_eq!(builder.overrides.background_fps, Some(0));
  }

  #[test]
  fn engine_builder_sets_background_rendering() {
    let builder = EngineBuilder::new().background_rendering(true);
    assert_eq!(builder.overrides.background_rendering, Some(true));
  }

  #[test]
  fn engine_builder_debug_keys_enabled_by_default() {
    let resolved = EngineBuilder::new()
      .start_scene(EmptyScene)
      .resolve_in(None, false);
    assert!(!resolved.disable_debug_keys);
  }

  #[test]
  fn engine_builder_vsync_defaults_to_none() {
    // Not set by the builder; resolve() applies the engine default (true)
    let builder = EngineBuilder::new();
    assert_eq!(builder.overrides.vsync, None);
  }

  #[test]
  fn engine_builder_sets_vsync() {
    let builder = EngineBuilder::new().vsync(false);
    assert_eq!(builder.overrides.vsync, Some(false));

    let builder = EngineBuilder::new().vsync(true);
    assert_eq!(builder.overrides.vsync, Some(true));
  }
```

- [ ] **Step 2: Run the tests to watch them fail**

Run: `cargo test --lib engine::tests 2>&1 | tail -20`
Expected: compile errors — no `resolve_in`, no `overrides` field. That is the RED for this task (the behaviour bugs these tests pin were reproduced during design: builder debug flags and background settings clobbered by Ryo.toml, missing keys loading as hardcoded defaults, the 720p migration).

- [ ] **Step 3: Replace the builder's flags with `BuilderOverrides`**

Change the struct and constructor:

```rust
/// Builder pattern for Engine configuration
pub struct EngineBuilder {
  /// Builder-only settings (target FPS, physics, window mode, ...). Settings
  /// other sources can also set live in `overrides`.
  config: EngineConfig,
  /// Settings set by builder calls, layered over project values in `resolve`
  overrides: super::config::resolve::BuilderOverrides,
  initial_scene: Option<Box<dyn Scene>>,
  /// Console command configuration (declarations and removals)
  console_builder: Option<ConsoleCommandBuilder>,
  resources: super::resources::Resources,
  /// Filters passed to `with_user_preferences`, combined when the engine is built
  user_preferences: Vec<super::config::UserPreferenceFilter>,
}

impl EngineBuilder {
  pub fn new() -> Self {
    EngineBuilder {
      config: EngineConfig::default(),
      overrides: Default::default(),
      initial_scene: None,
      console_builder: None,
      resources: super::resources::Resources::new(),
      user_preferences: Vec::new(),
    }
  }
```

Change these setters' bodies (keep their rustdoc, updated in Task 5):

```rust
  pub fn title(mut self, title: impl Into<String>) -> Self {
    self.overrides.title = Some(title.into());
    self
  }

  pub fn resolution(mut self, width: u32, height: u32) -> Self {
    self.overrides.resolution = Some((width, height));
    self
  }

  pub fn background_fps(mut self, fps: u32) -> Self {
    self.overrides.background_fps = Some(fps);
    self
  }

  pub fn background_rendering(mut self, enabled: bool) -> Self {
    self.overrides.background_rendering = Some(enabled);
    self
  }

  pub fn vsync(mut self, enabled: bool) -> Self {
    self.overrides.vsync = Some(enabled);
    self
  }

  pub fn display_mode(mut self, mode: DisplayMode) -> Self {
    self.overrides.display_mode = Some(mode);
    self
  }

  pub fn icon(mut self, path: impl Into<std::path::PathBuf>) -> Self {
    self.overrides.icon = Some(path.into());
    self
  }

  pub fn show_colliders(mut self, enabled: bool) -> Self {
    self.overrides.show_colliders = Some(enabled);
    self
  }

  pub fn show_fps(mut self, enabled: bool) -> Self {
    self.overrides.show_fps = Some(enabled);
    self
  }

  pub fn show_velocities(mut self, enabled: bool) -> Self {
    self.overrides.show_velocities = Some(enabled);
    self
  }

  pub fn show_origins(mut self, enabled: bool) -> Self {
    self.overrides.show_origins = Some(enabled);
    self
  }
```

Delete `apply_user_preferences` entirely.

- [ ] **Step 4: Replace `resolve` with the layered version**

Replace the whole `fn resolve(mut self, headless: bool) -> ResolvedBuild { ... }` with:

```rust
  /// Apply every configuration source in priority order, without creating the engine.
  fn resolve(self, headless: bool) -> ResolvedBuild {
    let workspace = super::config::Workspace::discover().ok();
    self.resolve_in(workspace.as_ref(), headless)
  }

  /// [`resolve`](Self::resolve) against an explicit workspace (`None` = no project files).
  ///
  /// Order: engine defaults → embedded values (release) or Ryo.toml (dev) →
  /// builder calls → saved preferences present in system.json and in the
  /// `with_user_preferences` filters. Headless builds never touch system.json.
  fn resolve_in(
    self,
    workspace: Option<&super::config::Workspace>,
    headless: bool,
  ) -> ResolvedBuild {
    use super::config::resolve::{ProjectLayer, compiled_embedded_value, resolve_settings};

    let initial_scene = self
      .initial_scene
      .expect("EngineBuilder requires a scene - call .start_scene() before .build()");

    let (project, game_name, game_version) =
      match (ProjectLayer::from_embedded(compiled_embedded_value), workspace) {
        (Some(embedded), _) => (embedded, None, None),
        (None, Some(ws)) => (
          ProjectLayer::from_workspace(ws),
          Some(ws.config().name.clone()),
          ws.config().version.clone(),
        ),
        (None, None) => (ProjectLayer::default(), None, None),
      };

    let mut settings = resolve_settings(&project, &self.overrides, None);
    if !headless
      && !self.user_preferences.is_empty()
      && let Some(ws) = workspace
    {
      let filter = combined_filter(&self.user_preferences);
      if let Some(prefs) = load_user_preferences(ws, &filter, &settings) {
        settings = resolve_settings(&project, &self.overrides, Some((&prefs, &filter)));
      }
    }

    let mut config = self.config;
    config.title = settings.title;
    config.width = settings.resolution.0;
    config.height = settings.resolution.1;
    config.display_mode = settings.display_mode;
    config.target_background_fps = settings.background_fps;
    config.background_rendering = settings.background_rendering;
    config.icon_path = settings.icon;
    config.debug.show_fps = settings.show_fps;
    config.debug.show_colliders = settings.show_colliders;
    config.debug.show_velocities = settings.show_velocities;
    config.debug.show_origins = settings.show_origins;
    config.debug.show_frame_timing = settings.show_frame_timing;
    config.debug.enable_profiler = settings.enable_profiler;
    config.debug.logger_window = settings.logger_window;
    config.debug.dev_window_keys = settings.dev_window_keys;
    config.debug.dev_console = settings.dev_console;

    // Runtime env var override for profiler (from `ryo2d dev --profiler`)
    // When -p flag is used, ensure enable_profiler is true so puffin scopes are collected
    if std::env::var("RYO_PROFILER_AUTO_OPEN").is_ok() {
      config.debug.enable_profiler = true;
    }

    // For release builds: check for icon.png next to executable (created by `ryo2d build`)
    // If not found, load_window_icon() will use the embedded default Ryo2D logo
    if config.icon_path.is_none()
      && let Ok(exe_path) = std::env::current_exe()
      && let Some(exe_dir) = exe_path.parent()
    {
      let icon_path = exe_dir.join("icon.png");
      if icon_path.exists() {
        config.icon_path = Some(icon_path);
      }
    }

    ResolvedBuild {
      config,
      initial_scene,
      vsync: Some(settings.vsync),
      master_volume: settings.master_volume,
      music_volume: settings.music_volume,
      sfx_volume: settings.sfx_volume,
      disable_debug_keys: settings.disable_debug_keys,
      game_name,
      game_version,
      console_builder: self.console_builder,
      resources: self.resources,
    }
  }
```

Add these private free functions after `impl Default for EngineBuilder`:

```rust
/// Every preference any `with_user_preferences` call asked for.
fn combined_filter(
  filters: &[super::config::UserPreferenceFilter],
) -> super::config::UserPreferenceFilter {
  use super::config::{UserPreference, UserPreferenceFilter};
  UserPreferenceFilter::only(
    UserPreference::all()
      .iter()
      .copied()
      .filter(|pref| filters.iter().any(|filter| filter.contains(*pref))),
  )
}

/// Saved preferences, seeding `system.json` from `seed` when it doesn't exist.
/// Errors are reported and treated as "no saved preferences".
fn load_user_preferences(
  workspace: &super::config::Workspace,
  filter: &super::config::UserPreferenceFilter,
  seed: &super::config::resolve::ResolvedSettings,
) -> Option<super::config::resolve::StoredPreferences> {
  let defaults = &workspace.config().system;
  let result = workspace
    .create_save_manager()
    .map_err(|e| e.to_string())
    .and_then(|manager| {
      manager
        .load_preferences_or_seed(&seed.to_system_state(defaults.env.clone()), filter, defaults)
        .map_err(|e| e.to_string())
    });
  match result {
    Ok(prefs) => prefs,
    Err(e) => {
      eprintln!("[ryo2d] Warning: Couldn't load user preferences ({e}), using project settings");
      None
    }
  }
}
```

- [ ] **Step 5: Run the engine tests**

Run: `cargo test --lib engine::tests 2>&1 | tail -8`
Expected: all pass.

- [ ] **Step 6: Run the full suite**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | head -20`
Expected: every `test result: ok`; 0 failed. In particular `tests/e2e/headless_engine.rs::headless_ignores_user_preferences_and_fps_overlay` still passes.

- [ ] **Step 7: Format, lint, commit**

Run: `rustfmt --edition 2024 src/core/engine.rs && cargo clippy --workspace --all-targets 2>&1 | grep -E "^\s+--> src/core/(engine|config/)" | head`
Expected: no findings in `engine.rs` or `src/core/config/` (Task 1/2 dead-code warnings are gone now).

```bash
git add src/core/engine.rs
git commit -m "fix(engine): resolve startup settings in one layered order

- Builder calls are stored as overrides and beat Ryo.toml and embedded values for every setting, including debug overlays and background FPS
- Saved preferences only override keys present in system.json and in the filter
- A new system.json is seeded from the resolved settings
- Repeated with_user_preferences calls combine their filters"
```

---

### Task 4: Release builds embed every project setting

**Files:**
- Modify: `src/cli/commands/build.rs` (the `Command::new("cargo")` env chain ~42-85; add a tests module)
- Modify: `src/build.rs` (whole file)

**Interfaces:**
- Consumes (Task 1): `crate::core::config::resolve::{EMBEDDED_KEYS, ProjectLayer}` (`ProjectLayer::from_embedded<'a>(impl Fn(&str) -> Option<&'a str>) -> Option<ProjectLayer>`).
- Produces: `fn embedded_config_env(config: &WorkspaceConfig) -> Vec<(&'static str, String)>` (private to the CLI build command).

- [ ] **Step 1: Write the failing tests**

Append to `src/cli/commands/build.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::config::WorkspaceConfig;
  use crate::core::config::resolve::{EMBEDDED_KEYS, ProjectLayer};

  #[test]
  fn embedded_env_covers_every_key_the_engine_reads() {
    let mut names: Vec<&str> = embedded_config_env(&WorkspaceConfig::default())
      .into_iter()
      .map(|(name, _)| name)
      .collect();
    let mut expected = EMBEDDED_KEYS.to_vec();
    names.sort_unstable();
    expected.sort_unstable();
    assert_eq!(names, expected);
  }

  #[test]
  fn embedded_debug_and_background_settings_reach_the_engine() {
    let mut config = WorkspaceConfig::default();
    config.debug.disable_debug_keys = true;
    config.debug.enable_profiler = true;
    config.debug.dev_window_keys = true;
    config.debug.logger_window = true;
    config.system.target_background_fps = 15;
    config.system.background_rendering = false;

    let env = embedded_config_env(&config);
    let layer = ProjectLayer::from_embedded(|name| {
      env
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.as_str())
    })
    .unwrap();

    assert_eq!(layer.disable_debug_keys, Some(true));
    assert_eq!(layer.enable_profiler, Some(true));
    assert_eq!(layer.dev_window_keys, Some(true));
    assert_eq!(layer.logger_window, Some(true));
    assert_eq!(layer.background_fps, Some(15));
    assert_eq!(layer.background_rendering, Some(false));
  }

  #[test]
  fn build_script_tracks_every_embedded_key() {
    let script = include_str!("../../build.rs");
    for key in EMBEDDED_KEYS {
      assert!(script.contains(key), "src/build.rs doesn't track {key}");
    }
  }
}
```

- [ ] **Step 2: Run them to watch them fail**

Run: `cargo test --lib cli::commands::build 2>&1 | tail -15`
Expected: compile error — `embedded_config_env` doesn't exist.

- [ ] **Step 3: Extract and extend the embedded env list**

Add above `create_release_bundle`:

```rust
/// Ryo.toml values passed to the release build; the engine reads them with
/// `option_env!` (see `core::config::resolve::EMBEDDED_KEYS`).
fn embedded_config_env(config: &crate::core::config::WorkspaceConfig) -> Vec<(&'static str, String)> {
  let system = &config.system;
  let debug = &config.debug;
  let display_mode = match system.display_mode {
    crate::core::config::DisplayMode::Windowed => "windowed",
    crate::core::config::DisplayMode::ExclusiveFullscreen => "exclusive_fullscreen",
    crate::core::config::DisplayMode::BorderlessFullscreen => "borderless_fullscreen",
  };
  vec![
    (
      "RYO_EMBED_RESOLUTION",
      system
        .resolution
        .map(|(w, h)| format!("{}x{}", w, h))
        .unwrap_or_else(|| "1280x720".to_string()),
    ),
    ("RYO_EMBED_VSYNC", system.vsync.to_string()),
    ("RYO_EMBED_DISPLAY_MODE", display_mode.to_string()),
    ("RYO_EMBED_MASTER_VOLUME", system.master_volume.to_string()),
    ("RYO_EMBED_MUSIC_VOLUME", system.music_volume.to_string()),
    ("RYO_EMBED_SFX_VOLUME", system.sfx_volume.to_string()),
    ("RYO_EMBED_CONSOLE", system.dev_console.to_string()),
    ("RYO_EMBED_TITLE", config.title.clone()),
    ("RYO_EMBED_SHOW_FPS", system.show_fps.to_string()),
    ("RYO_EMBED_SHOW_COLLIDERS", debug.show_colliders.to_string()),
    ("RYO_EMBED_SHOW_VELOCITIES", debug.show_velocities.to_string()),
    ("RYO_EMBED_SHOW_ORIGINS", debug.show_origins.to_string()),
    ("RYO_EMBED_SHOW_FRAME_TIMING", debug.show_frame_timing.to_string()),
    ("RYO_EMBED_DISABLE_DEBUG_KEYS", debug.disable_debug_keys.to_string()),
    ("RYO_EMBED_ENABLE_PROFILER", debug.enable_profiler.to_string()),
    ("RYO_EMBED_DEV_WINDOW_KEYS", debug.dev_window_keys.to_string()),
    ("RYO_EMBED_LOGGER_WINDOW", debug.logger_window.to_string()),
    ("RYO_EMBED_BACKGROUND_FPS", system.target_background_fps.to_string()),
    ("RYO_EMBED_BACKGROUND_RENDERING", system.background_rendering.to_string()),
  ]
}
```

In `execute`, delete the `display_mode_str` match and the `system` binding if now unused, and replace every `.env("RYO_EMBED_…", …)` call from `RYO_EMBED_RESOLUTION` through `RYO_EMBED_SHOW_FRAME_TIMING` with one call:

```rust
    .envs(embedded_config_env(config))
```

Keep `.env("CARGO_TARGET_DIR", &target_dir)` and `.env("RYO_EMBED_ICON", &icon_path)` (the icon is for the game's own build script, not the engine).

- [ ] **Step 4: Update the engine build script**

Replace `src/build.rs` with:

```rust
//! Build script that tracks the Ryo.toml values `ryo2d build` embeds.
//!
//! `ryo2d build` passes RYO_EMBED_* environment variables to cargo; the engine
//! reads them with `option_env!` (see `core::config::resolve::EMBEDDED_KEYS`).
//! This keeps the list here in sync so cargo rebuilds when a value changes.

const EMBEDDED_KEYS: &[&str] = &[
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

fn main() {
  for key in EMBEDDED_KEYS {
    println!("cargo:rerun-if-env-changed={key}");
    if let Ok(value) = std::env::var(key) {
      println!("cargo:rustc-env={key}={value}");
    }
  }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --lib cli::commands::build 2>&1 | tail -5`
Expected: 3 passed.

- [ ] **Step 6: Verify embedded values reach the engine end to end**

`engine_builder_debug_keys_enabled_by_default` resolves with no workspace, so when the engine is compiled with embedded values it must pick them up. Run, from the repo root:

```bash
RYO_EMBED_RESOLUTION=640x360 RYO_EMBED_DISABLE_DEBUG_KEYS=true \
  cargo test --lib engine::tests::engine_builder_debug_keys_enabled_by_default 2>&1 | grep -E "test result|panicked"
cargo test --lib engine::tests::engine_builder_debug_keys_enabled_by_default 2>&1 | grep "test result"
```

Expected: the first run FAILS (`assertion failed: !resolved.disable_debug_keys`) — the embedded `RYO_EMBED_DISABLE_DEBUG_KEYS=true` flowed through `option_env!` into the resolver. The second run (env vars unset) rebuilds and PASSES, proving cargo tracks the embedded variables.

- [ ] **Step 7: Format, lint, commit**

Run: `rustfmt --edition 2024 src/cli/commands/build.rs src/build.rs && cargo clippy --workspace --all-targets 2>&1 | grep -E "^\s+--> src/(cli/commands/build|build)\.rs" | head`
Expected: no findings.

```bash
git add src/cli/commands/build.rs src/build.rs
git commit -m "fix(cli): embed every Ryo.toml setting the engine reads in release builds

- Release builds now honour disable_debug_keys, dev_window_keys, logger_window, enable_profiler and the background settings
- The engine build script tracks the current embedded variable names"
```

---

### Task 5: Documentation matches the rule

**Files:**
- Modify: `src/core/engine.rs` (rustdoc on `with_user_preferences`, `vsync`, `show_colliders`, `show_velocities`, `show_origins`, `background_fps`, `background_rendering`, `display_mode`, `resolution`)
- Modify: `src/core/config/save.rs` (module doc lines ~9-19, `UserPreference` doc ~96-102, `UserPreferenceFilter` doc examples ~157-160 and `none()` doc ~179, `SystemState` doc ~270-283, `load_or_create_system` doc ~915-936)
- Modify: `src/core/config/workspace.rs` (module doc ~18, `SystemDefaults` doc ~181-191)
- Modify: `src/core/scene.rs` (~225, CRLF)
- Modify: `src/core/config/AGENTS.md`
- Modify: `src/cli/skills/ryo-config-save.md`, `.claude/skills/ryo-config-save/SKILL.md`, `src/cli/skills/ryo-game-dev.md`, `.claude/skills/ryo-game-dev/SKILL.md`, `src/cli/skills/ryo-api.md`, `.claude/skills/ryo-api/SKILL.md` (all CRLF)
- Modify: `docs/src/concepts/system-configuration.md`, `docs/src/getting-started/configuration.md`

**Interfaces:**
- Consumes: the behaviour from Tasks 1-4.
- Produces: nothing new.

**The rule, worded once (reuse it verbatim where a full statement is needed):**

> Startup settings resolve in one order, each layer overriding only what it sets: engine defaults → `Ryo.toml` (dev) or the values `ryo2d build` embedded from it (release) → `EngineBuilder` calls → saved player preferences in `system.json` (only keys present in the file and in the `with_user_preferences` filter). Without `with_user_preferences`, `system.json` is never read. A new `system.json` is seeded from the resolved settings.

- [ ] **Step 1: Record the stale statements before editing**

Run:

```bash
grep -rn "system.json > Ryo.toml\|system.json (user) > Ryo.toml\|Runtime code is always king\|always taking priority\|use only Ryo.toml defaults\|Fields not in the filter will use Ryo.toml\|Fields not in the filter use Ryo.toml\|created from Ryo.toml defaults\|missing fields fall back to these values\|Missing fields fall back through the chain\|Merges missing fields when loading\|merges from Ryo.toml \[system\]\|with_system_state_overrides\|SystemFieldFilter\|SystemField\b\|EngineBuilder::build()\|everything uses Ryo.toml/engine defaults\|display.vsync as fallback" \
  src docs .claude/skills > /tmp/stale-before.txt; wc -l < /tmp/stale-before.txt
```

Expected: a non-zero count (the investigation found about 30). Keep the list as your checklist.

- [ ] **Step 2: Fix rustdoc and module docs**

Make these edits:
- `engine.rs` `with_user_preferences` rustdoc: replace the "Priority Chain" list and "If system.json doesn't exist…" paragraph with the rule above, and the "Field Filtering" sentence with: "Use a [`UserPreferenceFilter`](super::config::UserPreferenceFilter) to choose which saved preferences may override the game's settings; keys outside the filter, or missing from the file, keep their builder / Ryo.toml / default values." Change the example comment `// Don't load any user preferences (use only Ryo.toml defaults)` to `// Don't apply any saved preferences (system.json is still created)`.
- `engine.rs` `vsync` rustdoc: "Default: `true`. Overrides Ryo.toml; a saved `vsync` in system.json overrides this when the [`with_user_preferences`](Self::with_user_preferences) filter includes `UserPreference::Vsync`."
- `engine.rs` `resolution`, `display_mode`, `show_colliders`, `show_velocities`, `show_origins`, `background_fps`, `background_rendering`: add one line each: "Overrides Ryo.toml." (and for `resolution`/`display_mode` add: "A saved preference overrides this when the `with_user_preferences` filter includes it.").
- `save.rs` module doc "Configuration Hierarchy": replace items 1-3 and the two bullets under it with the rule and: "Use [`SaveManager::load_or_create_system`] to read or create the file directly from a game; the engine resolves startup settings itself."
- `save.rs` `UserPreference` doc: replace "Fields not in the filter will use Ryo.toml `[system]` defaults." with "Preferences not in the filter, or missing from system.json, keep their builder / Ryo.toml / default values."
- `save.rs` `UserPreferenceFilter` example comment `// Load nothing from system.json (use only Ryo.toml defaults)` → `// Apply no saved preferences`; `none()` doc → "No fields - no saved preference overrides the game's settings."
- `save.rs` `SystemState` doc: replace the hierarchy block and the "When `system.json` doesn't exist…" paragraph with: "The engine reads it at startup (see [`EngineBuilder::with_user_preferences`](crate::core::engine::EngineBuilder::with_user_preferences)): saved preferences override builder calls and Ryo.toml, and a new file is seeded from the resolved settings. Keys missing from the file load as engine defaults here but are not treated as preferences at startup."
- `save.rs` `load_or_create_system` doc: replace the 1-3 list and "Behavior" bullets with: "Creates system.json from `defaults` (Ryo.toml `[system]`) when it doesn't exist; otherwise loads it and adds new `env` keys from `defaults`. Saved values are never replaced."
- `workspace.rs` module doc line ~18 and `SystemDefaults` doc: replace "system.json > Ryo.toml [system] > engine defaults" with "builder calls and saved system.json preferences override these (see `EngineBuilder::with_user_preferences`)", and replace "When system.json is loaded, missing fields fall back to these values." with "They seed a new system.json in dev builds."
- `scene.rs` ~225 (CRLF — edit byte-preservingly): "(from system.json > Ryo.toml > engine defaults)" → "(resolved at startup: saved preference > builder > Ryo.toml > engine default)".

- [ ] **Step 3: Fix the config AGENTS.md**

Replace the "Configuration Precedence" section of `src/core/config/AGENTS.md` with:

```markdown
## Configuration Precedence

`EngineBuilder::resolve_in` in `engine.rs` calls `resolve::resolve_settings` (pure, unit-tested). Lowest to highest:
1. Engine defaults (`ResolvedSettings::defaults`)
2. Project values: Ryo.toml (dev) or `RYO_EMBED_*` values from `ryo2d build` (release) - `ProjectLayer`
3. Builder calls - `BuilderOverrides`
4. `system.json` preferences - only with `.with_user_preferences(filter)`, only keys present in the file and in the (combined) filter - `StoredPreferences`

- A missing system.json key is "no preference", not a default
- A new system.json is seeded from levels 1-3; headless builds never read or write it
- Embedded keys live in `resolve::EMBEDDED_KEYS`, `cli/commands/build.rs::embedded_config_env` and `src/build.rs` (tests keep them in sync)
```

In "Gotchas", replace "plus the field copy in `EngineBuilder::build()`" with "plus fields in `resolve.rs` (`ProjectLayer`, `ResolvedSettings`, and `BuilderOverrides` if it has a setter), the copy in `EngineBuilder::resolve_in`, and an `RYO_EMBED_*` key".

- [ ] **Step 4: Fix the shipped skills (CRLF, both copies of each)**

For each pair, make the same edit in `src/cli/skills/<name>.md` and `.claude/skills/<name>/SKILL.md`, preserving CRLF on every line:
- `ryo-config-save` line ~21: replace "**Priority chain:** `system.json` > `Ryo.toml [system]` > engine defaults" with "**Priority chain:** saved `system.json` preferences (only keys present, only with `with_user_preferences`) > `EngineBuilder` calls > `Ryo.toml` (dev) / embedded values (release) > engine defaults".
- `ryo-config-save` line ~67: replace "Created automatically from `[system]` section when missing (title from `[info]`)" with "Created on first run from the game's resolved settings (builder > Ryo.toml > defaults), holding only the keys in the `with_user_preferences` filter".
- `ryo-game-dev` (~320 / ~326) and `ryo-api` (~104 / ~92): after the sentence about builder methods overriding the config file, add " A saved player preference in system.json overrides a builder call when `with_user_preferences` includes it."

Check: `for f in src/cli/skills/ryo-config-save.md .claude/skills/ryo-config-save/SKILL.md src/cli/skills/ryo-game-dev.md .claude/skills/ryo-game-dev/SKILL.md src/cli/skills/ryo-api.md .claude/skills/ryo-api/SKILL.md src/core/scene.rs; do file $f; done`
Expected: every line reports `with CRLF line terminators`, and `git diff --stat` shows only small changes in each (no whole-file rewrite).

- [ ] **Step 5: Minimal mdbook corrections**

In `docs/src/concepts/system-configuration.md` and `docs/src/getting-started/configuration.md` only:
- Replace "Runtime code is always king" / "runtime code always taking priority" sentences and the "Code (highest) > system.json (filtered) > …" chains with the rule above.
- Replace `with_system_state_overrides` with `with_user_preferences`, `SystemFieldFilter` with `UserPreferenceFilter`, and `SystemField` with `UserPreference` (check each code sample still matches the real API: `UserPreferenceFilter::all()`, `::none()`, `::only([...])`, or an array of `UserPreference`).
- `system-configuration.md` "Missing fields fall back through the chain." → "Keys missing from system.json are not preferences; the builder, Ryo.toml and defaults apply."
- `system-configuration.md` vsync "Not set → Mailbox → Immediate" → "Not set → on (`true`)".
- `configuration.md` "Resolution comes from system.json > Ryo.toml > engine defaults" → the rule's order.
Add no new sections.

- [ ] **Step 6: Verify nothing stale remains and docs build**

Run:

```bash
grep -rn "system.json > Ryo.toml\|system.json (user) > Ryo.toml\|Runtime code is always king\|always taking priority\|use only Ryo.toml defaults\|Fields not in the filter will use Ryo.toml\|Fields not in the filter use Ryo.toml\|created from Ryo.toml defaults\|missing fields fall back to these values\|Missing fields fall back through the chain\|Merges missing fields when loading\|merges from Ryo.toml \[system\]\|with_system_state_overrides\|SystemFieldFilter\|SystemField\b\|EngineBuilder::build()\|everything uses Ryo.toml/engine defaults\|display.vsync as fallback" src docs .claude/skills
cargo doc --no-deps 2>&1 | grep -E "^warning|^error" | head
cargo test --doc 2>&1 | grep "test result"
```

Expected: the grep prints nothing; `cargo doc` prints no new warnings for the touched files; doc tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/core/engine.rs src/core/config/save.rs src/core/config/workspace.rs src/core/scene.rs src/core/config/AGENTS.md src/cli/skills/ryo-config-save.md .claude/skills/ryo-config-save/SKILL.md src/cli/skills/ryo-game-dev.md .claude/skills/ryo-game-dev/SKILL.md src/cli/skills/ryo-api.md .claude/skills/ryo-api/SKILL.md docs/src/concepts/system-configuration.md docs/src/getting-started/configuration.md
git commit -m "docs: describe the single config precedence order"
```
