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
    .resizable(false)
    .window_size(800, 600)
    .ui_scale(Some(2.0))
    .start_scene(EmptyScene)
    .resolve_in(None, false);
  assert_eq!(r.config.target_fps, 144);
  assert_eq!(r.config.fixed_hz, 120);
  assert!(!r.config.resizable);
  assert_eq!(r.config.window_size, Some((800, 600)));
  assert_eq!(r.config.ui_scale, Some(2.0));
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
  // The saved resolution sizes the window; the game keeps its own
  assert_eq!(r.config.window_size, Some((1600, 900)));
  assert_eq!((r.config.width, r.config.height), (800, 600));
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
  assert_eq!(r.config.window_size, Some((1280, 720)));
  assert_eq!((r.config.width, r.config.height), (1920, 1080));
  let raw = std::fs::read_to_string(dir.path().join("data/system.json")).unwrap();
  assert!(raw.contains("1280x720"));
}

#[test]
fn an_old_system_json_without_the_new_volumes_takes_the_defaults() {
  // Written before the ui, voice and ambience volumes existed, by an engine
  // whose volumes were amplitudes
  let old = r#"{"master_volume": 0.4, "music_volume": 0.6, "sfx_volume": 0.7}"#;
  let resolve = |ryo_toml: &str, system_json: &str| {
    let (_dir, ws) = temp_workspace(ryo_toml, Some(system_json));
    EngineBuilder::new()
      .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false)
  };
  let r = resolve("[system]\nui_volume = 0.5\n", old);
  assert_eq!(
    (r.master_volume, r.music_volume, r.sfx_volume),
    (0.63, 0.77, 0.84),
    "converted to today's curve at the same loudness"
  );
  assert_eq!(r.ui_volume, 0.5, "Ryo.toml's default");
  assert_eq!(
    (r.voice_volume, r.ambience_volume),
    (1.0, 1.0),
    "the engine's, with none in Ryo.toml"
  );
  // Saved, they win over Ryo.toml, and corrupted ones are cleaned
  let r = resolve(
    "[system]\nui_volume = 0.5\n",
    r#"{"format_version": 2, "ui_volume": 0.2, "voice_volume": 7.0, "ambience_volume": -1.0}"#,
  );
  assert_eq!(
    (r.ui_volume, r.voice_volume, r.ambience_volume),
    (0.2, 1.0, 0.0)
  );
}

#[test]
fn the_audio_buffer_setting_resolves_from_ryo_toml() {
  let resolve = |ryo_toml: &str| {
    let (_dir, ws) = temp_workspace(ryo_toml, None);
    EngineBuilder::new()
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), true)
      .audio_buffer_frames
  };
  assert_eq!(resolve("[system]\naudio_buffer_frames = 256\n"), Some(256));
  assert_eq!(resolve(""), None, "unset: cpal's default");
  assert_eq!(resolve("[system]\naudio_buffer_frames = 0\n"), None);
}

#[test]
fn the_tiled_project_path_comes_from_the_builder_then_ryo_toml_then_the_default() {
  let resolve = |ryo_toml: &str, builder: Option<&str>| {
    let (_dir, ws) = temp_workspace(ryo_toml, None);
    let mut b = EngineBuilder::new().start_scene(EmptyScene);
    if let Some(path) = builder {
      b = b.tiled_project(path);
    }
    b.resolve_in(Some(&ws), true).tiled_project
  };
  let toml = "[tilemap]\ntiled_project = \"maps/game.tiled-project\"\n";
  assert_eq!(
    resolve("", None),
    None,
    "unset: project.tiled-project at the asset root"
  );
  assert_eq!(
    resolve(toml, None).as_deref(),
    Some("maps/game.tiled-project")
  );
  assert_eq!(
    resolve(toml, Some("other.tiled-project")).as_deref(),
    Some("other.tiled-project"),
    "the builder wins"
  );
  assert_eq!(resolve("[tilemap]\ntiled_project = \"\"\n", None), None);
}

#[test]
fn the_engine_s_loaders_read_the_configured_tiled_project() {
  use crate::core::asset::{AssetsView, MemorySource};
  let files = MemorySource::new();
  files.insert("kit/game.tiled-project", PROJECT);
  files.insert("m.tmj", MAP_WITH_CRATE);
  let engine = |configured: bool| {
    let mut b = EngineBuilder::new()
      .start_scene(EmptyScene)
      .asset_source(files.clone());
    if configured {
      b = b.tiled_project("kit/game.tiled-project");
    }
    let resolved = b.resolve_in(None, true);
    Engine::new(resolved, true)
  };
  let hp = |mut engine: Engine| {
    let mut assets = AssetsView::new(&mut engine.world);
    let map = assets
      .load_now::<crate::core::tilemap::TileMap>("m.tmj")
      .ok()?;
    let map = assets.get(&map)?;
    let crate::core::tilemap::LayerKind::Objects(objects) = &map.layers.first()?.kind else {
      return None;
    };
    objects.first()?.properties.int("hp")
  };
  assert_eq!(
    hp(engine(true)),
    Some(10),
    "the project's default fills the member"
  );
  assert_eq!(hp(engine(false)), None, "no project: nothing to fill from");
}

const PROJECT: &str = r#"{"propertyTypes":[{"id":1,"name":"Crate","type":"class","members":[{"name":"hp","type":"int","value":10}]}]}"#;

const MAP_WITH_CRATE: &str = r#"{"type":"map","orientation":"orthogonal","width":1,"height":1,"tilewidth":16,"tileheight":16,"tilesets":[],"layers":[{"id":1,"name":"o","type":"objectgroup","objects":[{"id":1,"name":"c","type":"Crate","x":0,"y":0,"width":1,"height":1}]}]}"#;

#[test]
fn the_engine_applies_the_six_volume_settings_at_start() {
  use crate::core::audio::{AudioEngine, VolumeKind};
  let (_dir, ws) = temp_workspace(
    "[system]\nmaster_volume = 0.3\nmusic_volume = 0.4\nsfx_volume = 0.5\nui_volume = 0.6\nvoice_volume = 0.7\nambience_volume = 0.9\n",
    None,
  );
  let resolved = EngineBuilder::new()
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), true);
  let engine = Engine::new(resolved, true);
  let audio = engine.world.resource::<AudioEngine>();
  let settings: Vec<f32> = VolumeKind::ALL
    .iter()
    .map(|&kind| audio.volume_setting(kind))
    .collect();
  assert_eq!(settings, [0.3, 0.4, 0.5, 0.6, 0.7, 0.9]);
}

#[test]
fn the_engine_loads_the_game_s_mixer_when_it_has_one() {
  use std::time::{Duration, Instant};

  use crate::core::asset::MemorySource;
  use crate::core::audio::{AudioEngine, MIXER_PATH, audio_system};

  let files = MemorySource::new();
  files.insert(MIXER_PATH, r#"(buses: { "sfx/steps": (volume: -6.0) })"#);
  let resolved = EngineBuilder::new()
    .asset_source(files)
    .start_scene(EmptyScene)
    .resolve_in(None, true);
  let mut engine = Engine::new(resolved, true);
  let start = Instant::now();
  loop {
    crate::core::asset::pump(&mut engine.world);
    audio_system(&mut engine.world);
    let buses = engine.world.resource::<AudioEngine>().buses();
    if let Some(steps) = buses.iter().find(|bus| bus.name == "sfx/steps") {
      assert_eq!(steps.base_db, -6.0);
      break;
    }
    assert!(start.elapsed() < Duration::from_secs(5), "the mixer lands");
    std::thread::sleep(Duration::from_millis(1));
  }
}

/// A headless engine (`HeadlessEngine`, `RYO_HEADLESS`) picks a sound's
/// clips and draws its gains and pitches the same way on every run, so its
/// captures and tests repeat without the game seeding anything
#[test]
fn headless_engines_pick_the_same_variations() {
  use crate::core::asset::{Assets, Handle};
  use crate::core::audio::{AudioClip, AudioEngine, AudioLogKind, AudioView, Sound, SoundClip};

  let picks = || {
    let resolved = EngineBuilder::new()
      .start_scene(EmptyScene)
      .resolve_in(None, true);
    let mut engine = Engine::new(resolved, true);
    engine.world.resource_mut::<AudioEngine>().set_logging(true);
    let clips: Vec<Handle<AudioClip>> = (0..4)
      .map(|_| {
        let clip = AudioClip::from_frames(48_000, &[[0.1, 0.1]; 480]).unwrap();
        engine.world.resource_mut::<Assets<AudioClip>>().add(clip)
      })
      .collect();
    let sound = Sound {
      clips: clips.iter().cloned().map(SoundClip::new).collect(),
      volume_range_db: 3.0,
      pitch_range: 2.0,
      per_frame: 32,
      ..Default::default()
    };
    let sound = engine.world.resource_mut::<Assets<Sound>>().add(sound);
    for _ in 0..32 {
      AudioView::new(&mut engine.world).play(&sound);
    }
    let engine_audio = engine.world.resource::<AudioEngine>();
    engine_audio
      .log()
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Start)
      .map(|entry| {
        let clip = clips
          .iter()
          .position(|clip| entry.source == format!("{:?}", clip.id()));
        (clip, entry.gain_db, entry.rate)
      })
      .collect::<Vec<_>>()
  };
  let first = picks();
  assert_eq!(first.len(), 32);
  assert!(first.iter().all(|(clip, _, _)| clip.is_some()), "{first:?}");
  assert_eq!(first, picks(), "a second headless run picks the same");
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
    Some(
      r#"{"format_version": 2, "master_volume": 0.2, "resolution": "1600x900", "vsync": false}"#,
    ),
  );
  let r = EngineBuilder::new()
    .vsync(false)
    .vsync(true)
    .with_user_preferences([UserPreference::MasterVolume])
    .with_user_preferences([UserPreference::Resolution])
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), false);
  assert_eq!(r.master_volume, 0.2);
  assert_eq!(r.config.window_size, Some((1600, 900)));
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
fn a_paced_offscreen_run_takes_its_wall_time_and_keeps_its_game_time() {
  if crate::core::testing::shared_gpu().is_err() {
    eprintln!("skipping: no GPU available");
    return;
  }
  // The scene quits after its 8th fixed update, paced or not
  let run = |pace: Option<std::time::Duration>| {
    let (scene, _) = space_watcher(8);
    let mut engine = EngineBuilder::new()
      .resolution(32, 32)
      .start_scene(scene)
      .build_headless();
    engine.prepare_offscreen_run(AutoCaptureConfig::default(), InputScript::default());
    engine.offscreen_pace = pace;
    let started = std::time::Instant::now();
    assert!(!engine.run_offscreen_frames().unwrap());
    let time = engine.world.resource::<Time>();
    (
      started.elapsed(),
      engine.capture.presented_frames(),
      time.fixed.step_count(),
      time.game.elapsed(),
    )
  };
  let (_, frames, steps, game) = run(None);
  let pace = std::time::Duration::from_millis(10);
  let (wall, paced_frames, paced_steps, paced_game) = run(Some(pace));
  assert_eq!(
    (paced_frames, paced_steps, paced_game),
    (frames, steps, game),
    "pacing changes no frame, step or game time"
  );
  assert!(
    wall >= pace * (paced_frames.max(1) as u32 - 1),
    "{paced_frames} paced frames took {wall:?}, at least {pace:?} each"
  );
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
fn the_default_is_resolution_independent_and_resizable() {
  let config = EngineConfig::default();
  assert_eq!(config.presentation, Presentation::ResolutionIndependent);
  assert!(config.resizable);
  assert_eq!(config.window_size, None);
}

#[test]
fn the_banner_says_what_sets_the_frame_rate() {
  assert_eq!(
    frame_rate_summary(true, 144),
    "vsync on: frames follow the display's refresh rate"
  );
  assert_eq!(frame_rate_summary(false, 144), "vsync off: up to 144 fps");
}

#[test]
fn a_resolution_fits_the_window() {
  let r = EngineBuilder::new()
    .resolution(1920, 1080)
    .start_scene(EmptyScene)
    .resolve_in(None, false);
  assert_eq!(
    r.config.presentation,
    Presentation::Fit {
      width: 1920,
      height: 1080
    }
  );
  assert_eq!((r.config.width, r.config.height), (1920, 1080));
}

#[test]
fn without_a_resolution_the_game_is_resolution_independent() {
  let r = EngineBuilder::new()
    .start_scene(EmptyScene)
    .resolve_in(None, false);
  assert_eq!(r.config.presentation, Presentation::ResolutionIndependent);
}

#[test]
fn pixel_art_sets_the_presentation_and_the_resolution() {
  let r = EngineBuilder::new()
    .pixel_art(320, 180)
    .start_scene(EmptyScene)
    .resolve_in(None, false);
  assert_eq!(
    r.config.presentation,
    Presentation::PixelArt {
      width: 320,
      height: 180
    }
  );
  assert_eq!((r.config.width, r.config.height), (320, 180));
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

  assert!(
    engine
      .world
      .resource::<crate::core::audio::AudioEngine>()
      .is_mock(),
    "silent, on kira's mock backend"
  );
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
  assert!(engine.world.contains_resource::<Physics>());
}

/// Spawns `tree` when it enters
struct SpawnUi(Option<crate::core::ui::UiNode>);

impl Scene for SpawnUi {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    if let Some(tree) = self.0.take() {
      ctx.spawn_ui(tree);
    }
  }
}

#[test]
fn the_ui_pass_time_is_measured_and_is_zero_without_ui() {
  use crate::core::testing::HeadlessEngine;
  use crate::core::testing::fixtures::settings_tree;
  use std::time::Duration;

  let Some(mut empty) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(EmptyScene),
  ) else {
    return;
  };
  empty.step_frames(3).unwrap();
  let empty_time = empty.ui_pass_time();

  let builder = EngineBuilder::new()
    .resolution(1280, 720)
    .start_scene(SpawnUi(Some(settings_tree())));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(5).unwrap();
  assert!(
    engine.ui_pass_time() > Duration::ZERO,
    "a UI pass takes time"
  );
  assert!(
    engine.ui_pass_time() > empty_time,
    "a 200-widget screen takes longer than no UI ({:?} against {:?})",
    engine.ui_pass_time(),
    empty_time
  );
}

#[test]
fn hot_reload_is_on_in_debug_builds_unless_headless_or_set() {
  let resolve = |builder: EngineBuilder, headless: bool| {
    builder
      .start_scene(EmptyScene)
      .resolve_in(None, headless)
      .hot_reload
  };
  assert_eq!(resolve(EngineBuilder::new(), false), cfg!(debug_assertions));
  assert!(!resolve(EngineBuilder::new(), true));
  assert!(resolve(EngineBuilder::new().hot_reload(true), true));
  assert!(!resolve(EngineBuilder::new().hot_reload(false), false));
}

/// A text asset for the asset tests: a `.note` file
struct Note(String);
impl crate::core::asset::Asset for Note {}

struct NoteLoader;
impl crate::core::asset::AssetLoader for NoteLoader {
  type Asset = Note;
  type Settings = ();
  fn extensions(&self) -> &[&'static str] {
    &["note"]
  }
  fn load(
    &self,
    bytes: Vec<u8>,
    _: &(),
    cx: &mut crate::core::asset::LoadContext<'_>,
  ) -> Result<Note, crate::core::asset::AssetError> {
    String::from_utf8(bytes)
      .map(Note)
      .map_err(|e| crate::core::asset::AssetError::decode(cx.path(), e))
  }
}

#[test]
fn assets_load_from_the_project_s_asset_folder_unless_given_a_source() {
  use crate::core::asset::{AssetsView, MemorySource};

  let read = |builder: EngineBuilder, ws: &crate::core::config::Workspace| {
    let resolved = builder
      .asset_loader(NoteLoader)
      .start_scene(EmptyScene)
      .resolve_in(Some(ws), true);
    let mut engine = Engine::new(resolved, true);
    let mut assets = AssetsView::new(&mut engine.world);
    let note = assets.load_now::<Note>("hello.note").ok()?;
    assets.get(&note).map(|note| note.0.clone())
  };

  let (dir, ws) = temp_workspace("", None);
  std::fs::create_dir_all(dir.path().join("assets")).unwrap();
  std::fs::write(dir.path().join("assets/hello.note"), "from disk").unwrap();
  assert_eq!(
    read(EngineBuilder::new(), &ws).as_deref(),
    Some("from disk")
  );
  let memory = MemorySource::new();
  memory.insert("hello.note", "from memory");
  assert_eq!(
    read(EngineBuilder::new().asset_source(memory), &ws).as_deref(),
    Some("from memory")
  );
}

#[test]
fn an_engine_with_hot_reload_off_starts_no_watcher() {
  use crate::core::asset::{AssetChanges, AssetError, AssetPath};

  /// A source that records whether it was asked to watch
  #[derive(Clone, Default)]
  struct Asked(Arc<AtomicBool>);
  impl AssetSource for Asked {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
      Err(AssetError::NotFound { path: path.clone() })
    }
    fn watch(&self, _: AssetChanges) -> Result<(), AssetError> {
      self.0.store(true, Ordering::SeqCst);
      Ok(())
    }
  }
  let asked = |builder: EngineBuilder| {
    let source = Asked::default();
    let resolved = builder
      .asset_source(source.clone())
      .start_scene(EmptyScene)
      .resolve_in(None, true);
    let _engine = Engine::new(resolved, true);
    source.0.load(Ordering::SeqCst)
  };
  assert!(!asked(EngineBuilder::new().hot_reload(false)));
  assert!(
    !asked(EngineBuilder::new()),
    "a headless build does not watch unless asked"
  );
  assert!(asked(EngineBuilder::new().hot_reload(true)));
}

#[test]
fn an_engine_with_hot_reload_reloads_a_saved_asset_file() {
  use std::time::{Duration, Instant};

  use crate::core::asset::AssetsView;

  let (dir, ws) = temp_workspace("", None);
  std::fs::create_dir_all(dir.path().join("assets")).unwrap();
  std::fs::write(dir.path().join("assets/hello.note"), "before").unwrap();
  let resolved = EngineBuilder::new()
    .hot_reload(true)
    .asset_loader(NoteLoader)
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), true);
  let mut engine = Engine::new(resolved, true);
  let note = AssetsView::new(&mut engine.world)
    .load_now::<Note>("hello.note")
    .unwrap();
  std::fs::write(dir.path().join("assets/hello.note"), "after").unwrap();
  let start = Instant::now();
  loop {
    crate::core::asset::pump(&mut engine.world);
    let text = AssetsView::new(&mut engine.world)
      .get(&note)
      .map(|note| note.0.clone());
    if text.as_deref() == Some("after") {
      break;
    }
    assert!(
      start.elapsed() < Duration::from_secs(3),
      "timed out: the saved note never reloaded ({text:?})"
    );
    std::thread::sleep(Duration::from_millis(2));
  }
}

#[test]
fn the_engine_publishes_what_its_frames_cost() {
  use crate::core::config::FrameCost;
  use crate::core::testing::HeadlessEngine;

  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(EmptyScene),
  ) else {
    return;
  };
  engine.step_frames(5).unwrap();
  let cost = *engine
    .world()
    .get_resource::<FrameCost>()
    .expect("the engine publishes FrameCost");
  assert_eq!(cost.frames(), 5, "{cost:?}");
  assert!(
    cost.update_ms() > 0.0 && cost.render_ms() > 0.0 && cost.present_ms() > 0.0,
    "every phase takes time: {cost:?}"
  );
  let phases = cost.update_ms() + cost.render_ms() + cost.present_ms();
  assert!(
    cost.total_ms() >= phases - 1e-9,
    "the total holds its phases: {cost:?}"
  );
}
