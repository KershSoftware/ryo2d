//! Layered resolution of startup settings.
//!
//! Every setting more than one source can set resolves in one order, each layer
//! overriding only what it actually sets:
//!
//! 1. Engine defaults
//! 2. Project values: `Ryo.toml` in dev, or the values `ryo2d build` embedded
//!    from it in release builds
//! 3. `EngineBuilder` calls
//!
//! The result is the game's defaults. The player's settings file applies over
//! the settings the game keeps, in `Settings` (`crate::core::settings`).

use std::path::PathBuf;

use crate::core::diagnostics::DebugConfig;

use super::preferences::DisplayMode;
use super::project::ProjectLayer;
use crate::core::graphics::screen::Presentation;
use crate::core::lighting::ShadowQuality;

/// Engine default resolution.
pub(crate) const DEFAULT_RESOLUTION: (u32, u32) = (1280, 720);
/// Engine default master volume: 1, the mix as designed (0 dB on the
/// volume curve, amplitude = v²).
pub(crate) const DEFAULT_MASTER_VOLUME: f32 = 1.0;
/// Engine default music volume.
pub(crate) const DEFAULT_MUSIC_VOLUME: f32 = 1.0;
/// Engine default sound effects volume.
pub(crate) const DEFAULT_SFX_VOLUME: f32 = 1.0;
/// Engine default interface sounds volume.
pub(crate) const DEFAULT_UI_VOLUME: f32 = 1.0;
/// Engine default dialogue volume.
pub(crate) const DEFAULT_VOICE_VOLUME: f32 = 1.0;
/// Engine default ambience volume.
pub(crate) const DEFAULT_AMBIENCE_VOLUME: f32 = 1.0;
const DEFAULT_TITLE: &str = "Ryo2D";

/// Settings set through `EngineBuilder` calls; `None` = not set.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct BuilderOverrides {
  pub title: Option<String>,
  pub resolution: Option<(u32, u32)>,
  pub pixel_art: Option<bool>,
  pub window_size: Option<(u32, u32)>,
  pub display_mode: Option<DisplayMode>,
  pub vsync: Option<bool>,
  pub shadow_quality: Option<ShadowQuality>,
  pub show_fps: Option<bool>,
  pub show_colliders: Option<bool>,
  pub show_velocities: Option<bool>,
  pub show_origins: Option<bool>,
  pub background_fps: Option<u32>,
  pub background_rendering: Option<bool>,
  pub icon: Option<PathBuf>,
  pub tiled_project: Option<String>,
}

/// Final value of every layered setting.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedSettings {
  pub title: String,
  /// The fit or pixel-art resolution, or (when no layer set one) the main
  /// camera's design area
  pub resolution: (u32, u32),
  /// Whether a layer set `resolution`: the game then fits it to the window
  pub resolution_set: bool,
  /// Whether `resolution` is a pixel-art target
  pub pixel_art: bool,
  /// The window's logical size, when a layer picked one
  pub window_size: Option<(u32, u32)>,
  pub display_mode: DisplayMode,
  pub vsync: bool,
  pub shadow_quality: ShadowQuality,
  pub master_volume: f32,
  pub music_volume: f32,
  pub sfx_volume: f32,
  pub ui_volume: f32,
  pub voice_volume: f32,
  pub ambience_volume: f32,
  /// The audio device's buffer in frames; `None`: the device's default
  pub audio_buffer_frames: Option<u32>,
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
  /// The Tiled project's asset path; `None`: `project.tiled-project` at the
  /// asset root
  pub tiled_project: Option<String>,
}

impl ResolvedSettings {
  fn defaults() -> Self {
    let debug = DebugConfig::default();
    ResolvedSettings {
      title: DEFAULT_TITLE.to_string(),
      resolution: DEFAULT_RESOLUTION,
      resolution_set: false,
      pixel_art: false,
      window_size: None,
      display_mode: DisplayMode::Windowed,
      vsync: true,
      shadow_quality: ShadowQuality::default(),
      master_volume: DEFAULT_MASTER_VOLUME,
      music_volume: DEFAULT_MUSIC_VOLUME,
      sfx_volume: DEFAULT_SFX_VOLUME,
      ui_volume: DEFAULT_UI_VOLUME,
      voice_volume: DEFAULT_VOICE_VOLUME,
      ambience_volume: DEFAULT_AMBIENCE_VOLUME,
      audio_buffer_frames: None,
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
      tiled_project: None,
    }
  }

  /// How the world reaches the window: pixel art, a fit of the resolution a
  /// layer set, or resolution independent.
  pub(crate) fn presentation(&self) -> Presentation {
    let (width, height) = self.resolution;
    if self.pixel_art {
      Presentation::PixelArt { width, height }
    } else if self.resolution_set {
      Presentation::Fit { width, height }
    } else {
      Presentation::ResolutionIndependent
    }
  }
}

/// Layer project values and builder calls over the engine defaults.
pub(crate) fn resolve_settings(
  project: &ProjectLayer,
  builder: &BuilderOverrides,
) -> ResolvedSettings {
  let mut s = ResolvedSettings::defaults();

  set(&mut s.title, project.title.clone());
  set(&mut s.resolution, project.resolution);
  set(&mut s.pixel_art, project.pixel_art);
  set(&mut s.display_mode, project.display_mode);
  set(&mut s.vsync, project.vsync);
  set(&mut s.shadow_quality, project.shadow_quality);
  set(&mut s.master_volume, project.master_volume);
  set(&mut s.music_volume, project.music_volume);
  set(&mut s.sfx_volume, project.sfx_volume);
  set(&mut s.ui_volume, project.ui_volume);
  set(&mut s.voice_volume, project.voice_volume);
  set(&mut s.ambience_volume, project.ambience_volume);
  // 0 frames is no fixed buffer
  s.audio_buffer_frames = project.audio_buffer_frames.filter(|&frames| frames > 0);
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
  s.tiled_project = project
    .tiled_project
    .clone()
    .filter(|path| !path.is_empty());

  set(&mut s.title, builder.title.clone());
  set(&mut s.resolution, builder.resolution);
  s.resolution_set = project.resolution.is_some() || builder.resolution.is_some();
  set(&mut s.pixel_art, builder.pixel_art);
  if builder.window_size.is_some() {
    s.window_size = builder.window_size;
  }
  set(&mut s.display_mode, builder.display_mode);
  set(&mut s.vsync, builder.vsync);
  set(&mut s.shadow_quality, builder.shadow_quality);
  set(&mut s.show_fps, builder.show_fps);
  set(&mut s.show_colliders, builder.show_colliders);
  set(&mut s.show_velocities, builder.show_velocities);
  set(&mut s.show_origins, builder.show_origins);
  set(&mut s.background_fps, builder.background_fps);
  set(&mut s.background_rendering, builder.background_rendering);
  if builder.icon.is_some() {
    s.icon = builder.icon.clone();
  }
  if builder.tiled_project.is_some() {
    s.tiled_project = builder.tiled_project.clone();
  }

  s.master_volume = sanitize_volume(s.master_volume, DEFAULT_MASTER_VOLUME);
  s.music_volume = sanitize_volume(s.music_volume, DEFAULT_MUSIC_VOLUME);
  s.sfx_volume = sanitize_volume(s.sfx_volume, DEFAULT_SFX_VOLUME);
  s.ui_volume = sanitize_volume(s.ui_volume, DEFAULT_UI_VOLUME);
  s.voice_volume = sanitize_volume(s.voice_volume, DEFAULT_VOICE_VOLUME);
  s.ambience_volume = sanitize_volume(s.ambience_volume, DEFAULT_AMBIENCE_VOLUME);

  s
}

/// Clamp a volume to `0.0..=1.0`; NaN and infinities fall back to `default`.
fn sanitize_volume(volume: f32, default: f32) -> f32 {
  if volume.is_finite() {
    volume.clamp(0.0, 1.0)
  } else {
    default
  }
}

/// Overwrite `target` when a layer provides a value.
pub(super) fn set<T>(target: &mut T, value: Option<T>) {
  if let Some(value) = value {
    *target = value;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::config::project::sample_layers;
  use crate::core::config::workspace::Workspace;

  #[test]
  fn out_of_range_volumes_are_clamped_and_non_finite_ones_default() {
    let project = ProjectLayer {
      master_volume: Some(f32::NAN),
      music_volume: Some(3.0),
      sfx_volume: Some(-1.0),
      ui_volume: Some(f32::INFINITY),
      voice_volume: Some(f32::NAN),
      ambience_volume: Some(0.25),
      ..ProjectLayer::default()
    };

    let s = resolve_settings(&project, &BuilderOverrides::default());

    assert_eq!(s.master_volume, DEFAULT_MASTER_VOLUME);
    assert_eq!(s.music_volume, 1.0);
    assert_eq!(s.sfx_volume, 0.0);
    assert_eq!(s.ui_volume, DEFAULT_UI_VOLUME);
    assert_eq!(s.voice_volume, DEFAULT_VOICE_VOLUME);
    assert_eq!(s.ambience_volume, 0.25);
  }

  fn builder_with_everything() -> BuilderOverrides {
    BuilderOverrides {
      title: Some("Builder".to_string()),
      resolution: Some((640, 480)),
      pixel_art: Some(false),
      window_size: None,
      display_mode: Some(DisplayMode::ExclusiveFullscreen),
      vsync: Some(false),
      shadow_quality: Some(ShadowQuality::Low),
      show_fps: Some(true),
      show_colliders: Some(true),
      show_velocities: Some(true),
      show_origins: Some(true),
      background_fps: Some(30),
      background_rendering: Some(false),
      icon: Some(PathBuf::from("builder.png")),
      tiled_project: Some("builder.tiled-project".to_string()),
    }
  }

  fn project_with_everything() -> ProjectLayer {
    ProjectLayer {
      title: Some("Project".to_string()),
      resolution: Some((800, 600)),
      pixel_art: Some(true),
      display_mode: Some(DisplayMode::BorderlessFullscreen),
      vsync: Some(true),
      shadow_quality: Some(ShadowQuality::Medium),
      master_volume: Some(0.3),
      music_volume: Some(0.4),
      sfx_volume: Some(0.5),
      ui_volume: Some(0.6),
      voice_volume: Some(0.7),
      ambience_volume: Some(0.8),
      audio_buffer_frames: Some(256),
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
      tiled_project: Some("project.tiled-project".to_string()),
    }
  }

  #[test]
  fn defaults_apply_when_no_layer_sets_anything() {
    let s = resolve_settings(&ProjectLayer::default(), &BuilderOverrides::default());
    assert_eq!(s.title, "Ryo2D");
    assert_eq!(s.resolution, (1280, 720));
    assert_eq!(s.display_mode, DisplayMode::Windowed);
    assert!(s.vsync);
    assert_eq!(
      (s.master_volume, s.music_volume, s.sfx_volume),
      (1.0, 1.0, 1.0)
    );
    assert!(!s.disable_debug_keys);
    assert_eq!((s.background_fps, s.background_rendering), (0, true));
    assert_eq!(s.icon, None);
    assert_eq!(s.tiled_project, None);
  }

  #[test]
  #[cfg(dev_tools)]
  fn the_console_and_dev_keys_default_on_with_tools() {
    let s = resolve_settings(&ProjectLayer::default(), &BuilderOverrides::default());
    assert!(s.dev_console, "the console is on by default");
    assert!(s.dev_window_keys, "the dev window keys are on by default");
    // Ryo.toml's `[debug]` turns each off
    let ws = Workspace::from_toml_str(
      "[debug]\nconsole = false\ndev_window_keys = false\n",
      PathBuf::from("game"),
    )
    .unwrap();
    let s = resolve_settings(
      &ProjectLayer::from_workspace(&ws),
      &BuilderOverrides::default(),
    );
    assert!(!s.dev_console && !s.dev_window_keys);
    // A project that sets neither keeps them on
    let ws = Workspace::from_toml_str("[project]\nname = \"g\"\n", PathBuf::from("game")).unwrap();
    let s = resolve_settings(
      &ProjectLayer::from_workspace(&ws),
      &BuilderOverrides::default(),
    );
    assert!(s.dev_console && s.dev_window_keys);
  }

  #[test]
  fn project_overrides_defaults() {
    let s = resolve_settings(&project_with_everything(), &BuilderOverrides::default());
    assert_eq!(s.title, "Project");
    assert_eq!(s.resolution, (800, 600));
    assert_eq!(s.display_mode, DisplayMode::BorderlessFullscreen);
    assert_eq!(
      (s.master_volume, s.music_volume, s.sfx_volume),
      (0.3, 0.4, 0.5)
    );
    assert_eq!(
      (s.ui_volume, s.voice_volume, s.ambience_volume),
      (0.6, 0.7, 0.8)
    );
    assert_eq!(s.audio_buffer_frames, Some(256));
    assert!(s.show_frame_timing && s.enable_profiler && s.disable_debug_keys);
    assert!(s.logger_window && !s.dev_window_keys && s.dev_console);
    assert_eq!((s.background_fps, s.background_rendering), (15, true));
    assert_eq!(s.icon, Some(PathBuf::from("project.png")));
    assert_eq!(s.tiled_project.as_deref(), Some("project.tiled-project"));
  }

  #[test]
  fn builder_overrides_project() {
    let s = resolve_settings(&project_with_everything(), &builder_with_everything());
    assert_eq!(s.title, "Builder");
    assert_eq!(s.resolution, (640, 480));
    assert_eq!(s.display_mode, DisplayMode::ExclusiveFullscreen);
    assert!(!s.vsync);
    assert!(s.show_fps && s.show_colliders && s.show_velocities && s.show_origins);
    assert_eq!((s.background_fps, s.background_rendering), (30, false));
    assert_eq!(s.icon, Some(PathBuf::from("builder.png")));
    assert_eq!(s.tiled_project.as_deref(), Some("builder.tiled-project"));
    // The builder has no volume setters, so the project's volumes stay
    assert_eq!(s.master_volume, 0.3);
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
ui_volume = 0.6
voice_volume = 0.7
ambience_volume = 0.8
audio_buffer_frames = 512
show_fps = true
target_background_fps = 15
background_rendering = false

[debug]
console = true
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

[tilemap]
tiled_project = "maps/game.tiled-project"
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
    assert_eq!(layer.ui_volume, Some(0.6));
    assert_eq!(layer.voice_volume, Some(0.7));
    assert_eq!(layer.ambience_volume, Some(0.8));
    assert_eq!(layer.audio_buffer_frames, Some(512));
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
    assert_eq!(
      layer.tiled_project.as_deref(),
      Some("maps/game.tiled-project")
    );
  }

  #[test]
  fn every_table_entry_reaches_the_resolved_settings() {
    let defaults = resolve_settings(&ProjectLayer::default(), &BuilderOverrides::default());
    for (name, layer) in sample_layers() {
      let resolved = resolve_settings(&layer, &BuilderOverrides::default());
      assert_ne!(
        resolved, defaults,
        "`{name}` doesn't reach the resolved settings"
      );
    }
  }

  #[test]
  fn every_builder_override_reaches_the_resolved_settings() {
    // Listed field by field, so a new override can't be left out of the test
    let BuilderOverrides {
      title,
      resolution,
      pixel_art,
      window_size,
      display_mode,
      vsync,
      shadow_quality,
      show_fps,
      show_colliders,
      show_velocities,
      show_origins,
      background_fps,
      background_rendering,
      icon,
      tiled_project,
    } = BuilderOverrides {
      title: Some("Builder".to_string()),
      resolution: Some((640, 480)),
      pixel_art: Some(true),
      window_size: Some((1024, 768)),
      display_mode: Some(DisplayMode::ExclusiveFullscreen),
      vsync: Some(false),
      shadow_quality: Some(ShadowQuality::Low),
      show_fps: Some(true),
      show_colliders: Some(true),
      show_velocities: Some(true),
      show_origins: Some(true),
      background_fps: Some(30),
      background_rendering: Some(false),
      icon: Some(PathBuf::from("builder.png")),
      tiled_project: Some("builder.tiled-project".to_string()),
    };
    let none = BuilderOverrides::default;
    let overrides = [
      ("title", BuilderOverrides { title, ..none() }),
      (
        "resolution",
        BuilderOverrides {
          resolution,
          ..none()
        },
      ),
      (
        "pixel_art",
        BuilderOverrides {
          pixel_art,
          ..none()
        },
      ),
      (
        "window_size",
        BuilderOverrides {
          window_size,
          ..none()
        },
      ),
      (
        "display_mode",
        BuilderOverrides {
          display_mode,
          ..none()
        },
      ),
      ("vsync", BuilderOverrides { vsync, ..none() }),
      (
        "shadow_quality",
        BuilderOverrides {
          shadow_quality,
          ..none()
        },
      ),
      ("show_fps", BuilderOverrides { show_fps, ..none() }),
      (
        "show_colliders",
        BuilderOverrides {
          show_colliders,
          ..none()
        },
      ),
      (
        "show_velocities",
        BuilderOverrides {
          show_velocities,
          ..none()
        },
      ),
      (
        "show_origins",
        BuilderOverrides {
          show_origins,
          ..none()
        },
      ),
      (
        "background_fps",
        BuilderOverrides {
          background_fps,
          ..none()
        },
      ),
      (
        "background_rendering",
        BuilderOverrides {
          background_rendering,
          ..none()
        },
      ),
      ("icon", BuilderOverrides { icon, ..none() }),
      (
        "tiled_project",
        BuilderOverrides {
          tiled_project,
          ..none()
        },
      ),
    ];

    let defaults = resolve_settings(&ProjectLayer::default(), &none());
    for (name, builder) in overrides {
      let resolved = resolve_settings(&ProjectLayer::default(), &builder);
      assert_ne!(
        resolved, defaults,
        "`{name}` doesn't reach the resolved settings"
      );
    }
  }
}
