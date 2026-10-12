//! Builder, resolution and offscreen-run tests.

use super::*;
use winit::keyboard::KeyCode;

use crate::core::scene::{SceneAction, SceneContext};

struct EmptyScene;

impl Scene for EmptyScene {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

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
fn an_empty_preference_filter_still_creates_system_json() {
  let (dir, ws) = temp_workspace("", None);
  let _ = EngineBuilder::new()
    .with_user_preferences(crate::core::config::UserPreferenceFilter::none())
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), false);
  assert!(dir.path().join("data/system.json").exists());
}

#[test]
fn builder_only_fields_survive_resolve() {
  let r = EngineBuilder::new()
    .target_fps(144)
    .fixed_hz(120)
    .window_mode(WindowMode::Fixed)
    .max_width(1000)
    .ui_scale(Some(2))
    .start_scene(EmptyScene)
    .resolve_in(None, false);
  assert_eq!(r.config.target_fps, 144);
  assert_eq!(r.config.fixed_hz, 120);
  assert_eq!(r.config.window_mode, WindowMode::Fixed);
  assert_eq!(r.config.max_width, Some(1000));
  assert_eq!(r.config.ui_scale, Some(2));
}

#[test]
fn profiler_flag_turns_the_profiler_on() {
  let mut config = EngineConfig::default();
  apply_launch_overrides(&mut config, true, None);
  assert!(config.debug.enable_profiler);

  let mut config = EngineConfig::default();
  apply_launch_overrides(&mut config, false, None);
  assert!(!config.debug.enable_profiler);
}

#[test]
fn icon_beside_the_game_is_used_when_none_is_set() {
  let dir = tempfile::tempdir().unwrap();
  std::fs::write(dir.path().join("icon.png"), b"png").unwrap();

  let mut config = EngineConfig::default();
  apply_launch_overrides(&mut config, false, Some(dir.path()));
  assert_eq!(config.icon_path, Some(dir.path().join("icon.png")));

  let mut config = EngineConfig {
    icon_path: Some(std::path::PathBuf::from("assets/custom.png")),
    ..EngineConfig::default()
  };
  apply_launch_overrides(&mut config, false, Some(dir.path()));
  assert_eq!(
    config.icon_path,
    Some(std::path::PathBuf::from("assets/custom.png"))
  );

  let empty = tempfile::tempdir().unwrap();
  let mut config = EngineConfig::default();
  apply_launch_overrides(&mut config, false, Some(empty.path()));
  assert_eq!(config.icon_path, None);
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
  assert!(!r.vsync);
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
fn a_headless_build_ignores_saved_preferences() {
  let (_dir, ws) = temp_workspace("", Some(r#"{"resolution": "1600x900"}"#));
  let r = EngineBuilder::new()
    .resolution(800, 600)
    .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), true);
  assert_eq!((r.config.width, r.config.height), (800, 600));

  // Nor does it seed a system.json
  let (dir, ws) = temp_workspace("", None);
  let _ = EngineBuilder::new()
    .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), true);
  assert!(!dir.path().join("data/system.json").exists());
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
  let raw: serde_json::Value =
    serde_json::from_str(&std::fs::read_to_string(dir.path().join("data/system.json")).unwrap())
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
  assert!(r.vsync);
}

#[test]
fn builder_show_fps_survives_workspace_config() {
  let (_dir, ws) = temp_workspace("[system]\nshow_fps = false\n", None);
  let resolved = EngineBuilder::new()
    .show_fps(true)
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), false);
  assert!(resolved.config.debug.show_fps);
}

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
    .fixed_hz(50);

  assert_eq!(builder.overrides.title.as_deref(), Some("Chained"));
  assert_eq!(builder.overrides.resolution, Some((640, 480)));
  assert_eq!(builder.config.target_fps, 30);
  assert_eq!(builder.config.fixed_hz, 50);
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

#[test]
fn repeated_user_preferences_calls_keep_every_filter_in_order() {
  use crate::core::config::{UserPreference, UserPreferenceFilter};

  let builder = EngineBuilder::new()
    .with_user_preferences(UserPreferenceFilter::none())
    .with_user_preferences(UserPreferenceFilter::all());
  assert_eq!(builder.user_preferences.len(), 2);
  assert!(!builder.user_preferences[0].contains(UserPreference::Vsync));
  assert!(builder.user_preferences[1].contains(UserPreference::Vsync));
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Records whether Space reached an update, and quits after `quit_after` updates
struct SpaceWatcher {
  saw_space: Arc<AtomicBool>,
  updates: u32,
  quit_after: u32,
}

impl Scene for SpaceWatcher {
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Space) {
      self.saw_space.store(true, Ordering::SeqCst);
    }
    self.updates += 1;
    if self.updates >= self.quit_after {
      SceneAction::Quit
    } else {
      SceneAction::Continue
    }
  }
}

fn space_watcher(quit_after: u32) -> (SpaceWatcher, Arc<AtomicBool>) {
  let saw_space = Arc::new(AtomicBool::new(false));
  let scene = SpaceWatcher {
    saw_space: Arc::clone(&saw_space),
    updates: 0,
    quit_after,
  };
  (scene, saw_space)
}

#[test]
fn offscreen_run_captures_scripted_frames_and_stops_after_the_last() {
  if crate::core::testing::shared_gpu().is_err() {
    eprintln!("skipping: no GPU available");
    return;
  }
  let dir = tempfile::tempdir().unwrap();
  let (scene, saw_space) = space_watcher(u32::MAX);
  let mut engine = EngineBuilder::new()
    .resolution(64, 48)
    .start_scene(scene)
    .build_headless();
  let capture = AutoCaptureConfig {
    frames: vec![3, 5],
    dir: dir.path().to_path_buf(),
    exit_after: true,
  };
  let script = InputScript::parse("2:Space").unwrap();
  engine.prepare_offscreen_run(capture, script);

  let failed = engine.run_offscreen_frames().unwrap();

  assert!(!failed);
  assert!(saw_space.load(Ordering::SeqCst));
  for frame in ["frame-000003.png", "frame-000005.png"] {
    let image = image::open(dir.path().join(frame)).unwrap();
    assert_eq!((image.width(), image.height()), (64, 48), "{frame}");
  }
}

#[test]
fn offscreen_run_ends_when_the_scene_quits() {
  if crate::core::testing::shared_gpu().is_err() {
    eprintln!("skipping: no GPU available");
    return;
  }
  let (scene, _) = space_watcher(4);
  let mut engine = EngineBuilder::new()
    .resolution(32, 32)
    .start_scene(scene)
    .build_headless();
  engine.prepare_offscreen_run(AutoCaptureConfig::default(), InputScript::default());

  assert!(!engine.run_offscreen_frames().unwrap());
  let presented = engine.capture.presented_frames();
  assert!((1..=4).contains(&presented), "{presented}");
}

#[test]
fn engine_config_has_sensible_defaults() {
  let config = EngineConfig::default();
  assert_eq!(config.title, "Ryo2D");
  assert_eq!(config.width, 1280);
  assert_eq!(config.height, 720);
  assert_eq!(config.target_fps, 60);
  assert_eq!(config.fixed_hz, 60);
}

#[test]
fn engine_builder_sets_fps() {
  let builder = EngineBuilder::new().target_fps(144);
  assert_eq!(builder.config.target_fps, 144);
}

#[test]
fn engine_builder_sets_fixed_hz_at_least_one() {
  let builder = EngineBuilder::new().fixed_hz(120);
  assert_eq!(builder.config.fixed_hz, 120);
  assert_eq!(EngineBuilder::new().fixed_hz(0).config.fixed_hz, 1);
}

#[test]
fn window_mode_defaults_to_resizable_aspect_ratio() {
  let mode = WindowMode::default();
  assert_eq!(mode, WindowMode::ResizableAspectRatio);
}

#[test]
fn engine_config_includes_window_mode() {
  let config = EngineConfig::default();
  assert_eq!(config.window_mode, WindowMode::ResizableAspectRatio);
}

#[test]
fn engine_builder_sets_window_mode() {
  let builder = EngineBuilder::new().window_mode(WindowMode::Fixed);
  assert_eq!(builder.config.window_mode, WindowMode::Fixed);

  let builder = EngineBuilder::new().window_mode(WindowMode::ResizableIntegerScale);
  assert_eq!(
    builder.config.window_mode,
    WindowMode::ResizableIntegerScale
  );

  let builder = EngineBuilder::new().window_mode(WindowMode::ResizableStretch);
  assert_eq!(builder.config.window_mode, WindowMode::ResizableStretch);
}

// ===========================================================================
// Background FPS / Rendering Tests
// ===========================================================================

#[test]
fn engine_config_background_defaults() {
  let config = EngineConfig::default();
  assert_eq!(config.target_background_fps, 0);
  assert!(config.background_rendering);
}

// ===========================================================================
// Debug Keys Tests
// ===========================================================================

#[test]
fn overlay_mode_syncs_to_debug_config_flags() {
  // Test that OverlayMode correctly maps to config flags
  let mode = OverlayMode::Off;
  assert_eq!(mode.to_flags(), (false, false, false));

  let mode = OverlayMode::FpsOnly;
  assert_eq!(mode.to_flags(), (true, false, false));

  // Frame timing includes FPS, so show_fps is false
  let mode = OverlayMode::FrameTiming;
  assert_eq!(mode.to_flags(), (false, true, false));

  let mode = OverlayMode::FullStats;
  assert_eq!(mode.to_flags(), (false, true, true));
}

#[test]
fn overlay_mode_from_debug_config_flags() {
  // Test that config flags correctly map to OverlayMode
  assert_eq!(
    OverlayMode::from_flags(false, false, false),
    OverlayMode::Off
  );
  assert_eq!(
    OverlayMode::from_flags(true, false, false),
    OverlayMode::FpsOnly
  );
  // Frame timing mode (show_fps ignored when frame_timing is true)
  assert_eq!(
    OverlayMode::from_flags(false, true, false),
    OverlayMode::FrameTiming
  );
  assert_eq!(
    OverlayMode::from_flags(false, true, true),
    OverlayMode::FullStats
  );
}

// ===========================================================================
// Console Command Declaration Tests
// ===========================================================================

#[test]
fn engine_builder_console_commands_none_by_default() {
  let builder = EngineBuilder::new();
  assert!(builder.console_builder.is_none());
}

#[test]
fn engine_builder_with_console_commands_declares_simple() {
  let builder = EngineBuilder::new().with_console_commands(|cmd| {
    cmd.declare("game:reset", "Reset the game");
  });
  let cb = builder.console_builder.as_ref().unwrap();
  assert_eq!(cb.declarations.len(), 1);
  assert_eq!(cb.declarations[0].0, "game:reset");
  assert_eq!(cb.declarations[0].1, "Reset the game");
  assert!(cb.declarations[0].2.is_none());
}

#[test]
fn engine_builder_with_console_commands_declares_verbose() {
  let builder = EngineBuilder::new().with_console_commands(|cmd| {
    cmd.declare_verbose(
      "game:spawn",
      "Spawn an entity",
      "Usage: game:spawn <type> [x] [y]",
    );
  });
  let cb = builder.console_builder.as_ref().unwrap();
  assert_eq!(cb.declarations.len(), 1);
  assert_eq!(cb.declarations[0].0, "game:spawn");
  assert_eq!(cb.declarations[0].1, "Spawn an entity");
  assert_eq!(
    cb.declarations[0].2.as_deref(),
    Some("Usage: game:spawn <type> [x] [y]")
  );
}

#[test]
fn engine_builder_with_console_commands_multiple() {
  let builder = EngineBuilder::new().with_console_commands(|cmd| {
    cmd.declare("game:reset", "Reset the game");
    cmd.declare("game:godmode", "Toggle invincibility");
    cmd.declare_verbose("game:spawn", "Spawn an entity", "Usage: game:spawn <type>");
  });
  let cb = builder.console_builder.as_ref().unwrap();
  assert_eq!(cb.declarations.len(), 3);
  assert_eq!(cb.declarations[0].0, "game:reset");
  assert_eq!(cb.declarations[1].0, "game:godmode");
  assert_eq!(cb.declarations[2].0, "game:spawn");
}

#[test]
fn engine_builder_with_console_commands_remove_builtin() {
  let builder = EngineBuilder::new().with_console_commands(|cmd| {
    cmd.remove_builtin("system:ip:wan");
    cmd.remove_builtin("system:version");
  });
  let cb = builder.console_builder.as_ref().unwrap();
  assert_eq!(cb.removals.len(), 2);
  assert_eq!(cb.removals[0], "system:ip:wan");
  assert_eq!(cb.removals[1], "system:version");
}

#[test]
fn engine_builder_with_console_commands_mixed() {
  let builder = EngineBuilder::new().with_console_commands(|cmd| {
    cmd.remove_builtin("system:version");
    cmd.declare("game:reset", "Reset the game");
    cmd.declare_verbose("game:spawn", "Spawn an entity", "Usage: game:spawn <type>");
  });
  let cb = builder.console_builder.as_ref().unwrap();
  assert_eq!(cb.removals.len(), 1);
  assert_eq!(cb.declarations.len(), 2);
}

#[test]
fn engine_builder_with_console_commands_chains_with_other_builder_methods() {
  let builder = EngineBuilder::new()
    .title("Test Game")
    .resolution(800, 600)
    .with_console_commands(|cmd| {
      cmd.declare("game:reset", "Reset");
    })
    .target_fps(60);
  assert_eq!(builder.overrides.title.as_deref(), Some("Test Game"));
  assert_eq!(builder.overrides.resolution, Some((800, 600)));
  assert_eq!(
    builder.console_builder.as_ref().unwrap().declarations.len(),
    1
  );
  assert_eq!(builder.config.target_fps, 60);
}

#[test]
fn headless_build_has_no_audio_gamepads_or_wall_clock_overlays() {
  let engine = EngineBuilder::new()
    .show_fps(true)
    .start_scene(EmptyScene)
    .build_headless();

  assert!(!engine.world.non_send::<SceneHost>().audio.is_available());
  assert!(engine.gilrs.is_none());
  assert!(!engine.config.debug.show_fps);
  assert!(!engine.config.debug.show_frame_timing);
  assert!(!engine.config.debug.show_stats);
  assert!(!engine.config.debug.logger_window);
  assert_eq!(engine.overlay_mode, OverlayMode::Off);
}

#[test]
fn built_engines_hold_the_builder_s_resources_and_messages() {
  #[derive(Resource, Debug, PartialEq)]
  struct Lives(u32);
  #[derive(crate::core::ecs::Message, Clone)]
  struct Scored;

  let engine = EngineBuilder::new()
    .insert_resource(Lives(3))
    .add_message::<Scored>()
    .start_scene(EmptyScene)
    .build_headless();
  assert_eq!(engine.world.resource::<Lives>(), &Lives(3));
  assert!(
    engine
      .world
      .contains_resource::<crate::core::ecs::Messages<Scored>>()
  );
  assert!(engine.world.contains_resource::<Time>());
  assert!(engine.world.contains_resource::<RapierPhysicsWorld>());
}
