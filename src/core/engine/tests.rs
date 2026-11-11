//! Builder, resolution and offscreen-run tests.

use super::*;
use winit::keyboard::KeyCode;

use crate::core::audio::VolumeKind;
use crate::core::scene::{SceneAction, SceneContext};
use crate::core::settings::persist::{DEBOUNCE, SettingsSaveFailed, SettingsWriter};

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
  assert!(!r.settings.vsync());
  assert_eq!(r.settings.volume(VolumeKind::Master), 0.3);
  assert!(r.config.debug.show_fps);
}

#[test]
fn the_player_file_overrides_the_project_for_kept_settings_only() {
  use crate::core::config::{UserPreference, UserPreferenceFilter};
  let store = MemorySaves::new();
  store.put(
    "system.json",
    r#"{"format_version": 3, "master_volume": 0.1, "music_volume": 0.2, "resolution": "1600x900", "display_mode": "windowed", "vsync": true}"#,
  );
  let (_dir, ws) = temp_workspace("[system]\nmusic_volume = 0.4\n", None);
  let resolve = |filter: UserPreferenceFilter| {
    EngineBuilder::new()
      .resolution(800, 600)
      .display_mode(DisplayMode::BorderlessFullscreen)
      .vsync(false)
      .memory_saves(store.clone())
      .with_user_preferences(filter)
      .start_scene(EmptyScene)
      .resolve_in(Some(&ws), false)
  };

  let r = resolve(UserPreferenceFilter::all());
  assert_eq!(r.settings.volume(VolumeKind::Master), 0.1);
  assert_eq!(r.settings.volume(VolumeKind::Music), 0.2);
  assert!(r.settings.vsync());
  assert_eq!(r.config.display_mode, DisplayMode::Windowed);
  // The player's window size sizes the window; the game keeps its resolution
  assert_eq!(r.config.window_size, Some((1600, 900)));
  assert_eq!((r.config.width, r.config.height), (800, 600));

  // Settings outside the filter come from the builder and Ryo.toml
  let r = resolve(UserPreferenceFilter::only([UserPreference::MasterVolume]));
  assert_eq!(r.settings.volume(VolumeKind::Master), 0.1);
  assert_eq!(r.settings.volume(VolumeKind::Music), 0.4);
  assert!(!r.settings.vsync());
  assert_eq!(r.config.display_mode, DisplayMode::BorderlessFullscreen);
  assert_eq!(r.config.window_size, Some((800, 600)));
}

#[test]
fn a_headless_engine_reads_settings_from_its_memory_store_and_never_from_disk() {
  let all = crate::core::config::UserPreferenceFilter::all;
  let on_disk = r#"{"format_version": 3, "music_volume": 0.2, "resolution": "1920x1080"}"#;
  let (dir, ws) = temp_workspace("", Some(on_disk));
  let store = MemorySaves::new();
  store.put(
    "system.json",
    r#"{"format_version": 3, "music_volume": 0.6}"#,
  );
  let r = EngineBuilder::new()
    .resolution(320, 180)
    .memory_saves(store)
    .with_user_preferences(all())
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), true);
  assert_eq!(r.settings.volume(VolumeKind::Music), 0.6, "the store's");
  assert_eq!(r.config.window_size, Some((320, 180)), "not the disk's");

  // Its own store, when it is given none
  let r = EngineBuilder::new()
    .with_user_preferences(all())
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), true);
  assert_eq!(r.settings.volume(VolumeKind::Music), 1.0);
  let files: Vec<_> = std::fs::read_dir(dir.path().join("data"))
    .unwrap()
    .map(|entry| entry.unwrap().file_name())
    .collect();
  assert_eq!(files, ["system.json"], "the save folder is untouched");
  assert_eq!(
    std::fs::read_to_string(dir.path().join("data/system.json")).unwrap(),
    on_disk
  );
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
  let volumes =
    |r: &ResolvedBuild, kinds: [VolumeKind; 3]| kinds.map(|kind| r.settings.volume(kind));
  let r = resolve("[system]\nui_volume = 0.5\n", old);
  assert_eq!(
    volumes(&r, [VolumeKind::Master, VolumeKind::Music, VolumeKind::Sfx]),
    [0.63, 0.77, 0.84],
    "converted to today's curve at the same loudness"
  );
  assert_eq!(
    volumes(
      &r,
      [VolumeKind::Ui, VolumeKind::Voice, VolumeKind::Ambience]
    ),
    [0.5, 1.0, 1.0],
    "Ryo.toml's default, then the engine's with none in Ryo.toml"
  );
  // Saved, they win over Ryo.toml, and a bad one is the default
  let r = resolve(
    "[system]\nui_volume = 0.5\n",
    r#"{"format_version": 2, "ui_volume": 0.2, "voice_volume": 7.0, "ambience_volume": -1.0}"#,
  );
  assert_eq!(
    volumes(
      &r,
      [VolumeKind::Ui, VolumeKind::Voice, VolumeKind::Ambience]
    ),
    [0.2, 1.0, 1.0]
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
  use crate::core::audio::AudioEngine;
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

/// A burst of 8 from an emitter spawned in `engine`, after one fixed step:
/// each particle's velocity, as the store keeps them
fn particles_after_a_step(engine: &mut Engine) -> Vec<(f32, f32)> {
  use crate::core::asset::Assets;
  use crate::core::ecs::Transform;
  use crate::core::graphics::Image;
  use crate::core::particles::{Burst, ParticleEffect, ParticleEmitter, ParticleStore};

  let image = engine
    .world
    .resource_mut::<Assets<Image>>()
    .add(Image::solid_rect(8, 8, 255, 255, 255, 255));
  let effect = ParticleEffect::new(image)
    .with_rate(0.0)
    .with_burst(Burst::at(0.0, 8.0));
  let effect = engine
    .world
    .resource_mut::<Assets<ParticleEffect>>()
    .add(effect);
  let emitter = engine
    .world
    .spawn((Transform::from_xy(10.0, 20.0), ParticleEmitter::new(effect)))
    .id();
  engine.schedules.run(Phase::FixedUpdate, &mut engine.world);
  let slot = engine
    .world
    .get::<ParticleEmitter>(emitter)
    .map_or(u32::MAX, |e| e.slot);
  let store = engine.world.resource::<ParticleStore>();
  store.pool(slot).map_or_else(Vec::new, |pool| {
    let p = &pool.main;
    p.vx.iter().copied().zip(p.vy.iter().copied()).collect()
  })
}

/// The engine steps particles in its fixed step with no scene code, within
/// the builder's budget, and a headless engine seeds them the same way on
/// every run
#[test]
fn engines_step_particles_within_the_builder_s_budget() {
  use crate::core::particles::{DEFAULT_PARTICLE_BUDGET, ParticleStore};

  // The budget of 5 drops spawns, which warns (into this test's capture)
  let _log = crate::core::testing::capture_log();
  let headless = || {
    EngineBuilder::new()
      .start_scene(EmptyScene)
      .build_headless()
  };
  let mut engine = headless();
  assert_eq!(
    engine.world.resource::<ParticleStore>().budget(),
    DEFAULT_PARTICLE_BUDGET
  );
  let first = particles_after_a_step(&mut engine);
  assert_eq!(first.len(), 8, "the burst, in the engine's own step");
  assert_eq!(engine.world.resource::<ParticleStore>().live(), 8);
  assert_eq!(
    particles_after_a_step(&mut headless()),
    first,
    "a second headless run spawns the same"
  );

  let mut engine = EngineBuilder::new()
    .particle_budget(5)
    .start_scene(EmptyScene)
    .build_headless();
  assert_eq!(engine.world.resource::<ParticleStore>().budget(), 5);
  assert_eq!(particles_after_a_step(&mut engine).len(), 5);
}

#[test]
fn first_run_writes_no_settings_file_until_a_setting_changes() {
  let all = crate::core::config::UserPreferenceFilter::all;
  let (dir, ws) = temp_workspace("[system]\nmaster_volume = 0.3\n", None);
  let r = EngineBuilder::new()
    .resolution(800, 600)
    .with_user_preferences(all())
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), false);
  assert_eq!((r.config.width, r.config.height), (800, 600));
  assert_eq!(r.settings.volume(VolumeKind::Master), 0.3);
  assert!(
    !dir.path().join("data/system.json").exists(),
    "the project's defaults are not the player's"
  );

  let store = MemorySaves::new();
  if let Some(mut engine) = crate::core::testing::HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(64, 64)
      .memory_saves(store.clone())
      .with_user_preferences(all())
      .start_scene(EmptyScene),
  ) {
    engine.step_frames(3).unwrap();
    assert_eq!(store.get("system.json"), None, "nothing changed");
  }

  // A kept setting changes: the file is written once the second is up,
  // holding only that key
  let store = MemorySaves::new();
  let mut engine = EngineBuilder::new()
    .resolution(64, 64)
    .memory_saves(store.clone())
    .with_user_preferences(all())
    .start_scene(EmptyScene)
    .build_headless();
  engine.apply_settings();
  engine.save_settings(Instant::now() + DEBOUNCE);
  engine.flush_saves();
  assert_eq!(store.get("system.json"), None, "still nothing changed");
  engine
    .world
    .resource_mut::<Settings>()
    .set_volume(VolumeKind::Master, 0.5);
  engine.apply_settings();
  engine.save_settings(Instant::now());
  engine.flush_saves();
  assert_eq!(
    store.get("system.json"),
    None,
    "not before the second is up"
  );
  engine.save_settings(Instant::now() + DEBOUNCE);
  engine.flush_saves();
  assert_eq!(store.writes(), 1);
  assert_eq!(
    settings_written(&store),
    serde_json::json!({"format_version": 3, "master_volume": 0.5})
  );
}

/// The settings file in `store`, as JSON
fn settings_written(store: &MemorySaves) -> serde_json::Value {
  let bytes = store.get("system.json").expect("system.json is written");
  serde_json::from_slice(&bytes).unwrap()
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
  assert!(r.config.debug.disable_debug_keys);
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
fn a_damaged_settings_file_is_set_aside_and_the_backup_used() {
  let all = crate::core::config::UserPreferenceFilter::all;
  let store = MemorySaves::new();
  store.put("system.json", "not json");
  store.put(
    "system.json.bak",
    r#"{"format_version": 3, "music_volume": 0.4}"#,
  );
  let resolve = || {
    EngineBuilder::new()
      .memory_saves(store.clone())
      .with_user_preferences(all())
      .start_scene(EmptyScene)
      .resolve_in(None, true)
  };
  let r = resolve();
  assert_eq!(r.settings.volume(VolumeKind::Music), 0.4, "the backup's");
  assert_eq!(store.get("system.json.damaged"), Some(b"not json".to_vec()));
  assert_eq!(store.get("system.json"), None);
  assert!(store.get("system.json.bak").is_some(), "the backup stays");

  // Without a good backup: the defaults, the older damaged file replaced
  store.put("system.json", "[1, 2]");
  store.remove("system.json.bak");
  let r = resolve();
  assert_eq!(r.settings.volume(VolumeKind::Music), 1.0);
  assert_eq!(store.get("system.json.damaged"), Some(b"[1, 2]".to_vec()));
  assert_eq!(store.get("system.json"), None);

  // In the save folder on disk
  let (dir, ws) = temp_workspace("", Some("not json"));
  let r = EngineBuilder::new()
    .resolution(800, 600)
    .with_user_preferences(all())
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), false);
  assert_eq!((r.config.width, r.config.height), (800, 600));
  assert_eq!(
    std::fs::read_to_string(dir.path().join("data/system.json.damaged")).unwrap(),
    "not json"
  );
  assert!(!dir.path().join("data/system.json").exists());
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
  assert_eq!(r.settings.volume(VolumeKind::Master), 0.3);
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
  assert_eq!(r.settings.volume(VolumeKind::Master), 0.2);
  assert_eq!(r.config.window_size, Some((1600, 900)));
  // Vsync isn't in either filter: the last builder call wins
  assert!(r.settings.vsync());
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
  assert!(!resolved.config.debug.disable_debug_keys);
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
  use crate::core::diagnostics::FrameCost;
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

const FILE_TOML: &str = "[project]\nname = \"File Game\"\n[system]\nresolution = \"1600x900\"\n";

#[test]
fn a_project_file_gives_the_project_values_where_there_is_no_ryo_toml() {
  let r = EngineBuilder::new()
    .project(crate::core::config::ProjectFile::from_toml(FILE_TOML))
    .start_scene(EmptyScene)
    .resolve_in(None, false);
  assert_eq!((r.config.width, r.config.height), (1600, 900));
  assert_eq!(r.config.title, "File Game");
  assert_eq!(r.store_name, "File Game");
  // The default dev window keys are for dev builds; a project file resolves
  // as a release build does
  assert!(!r.config.debug.dev_window_keys);
}

#[test]
fn a_project_file_without_a_name_names_the_store_after_the_title() {
  let r = EngineBuilder::new()
    .project(crate::core::config::ProjectFile::from_toml(
      "[system]\nvsync = false\n",
    ))
    .title("Untitled Hopper")
    .start_scene(EmptyScene)
    .resolve_in(None, false);
  assert_eq!(r.store_name, "Untitled Hopper");
}

#[test]
fn a_workspace_without_a_ryo_toml_leaves_the_project_file_to_decide() {
  let (_dir, ws) = temp_workspace("", None);
  std::fs::remove_file(_dir.path().join("Ryo.toml")).unwrap();
  let r = EngineBuilder::new()
    .project(crate::core::config::ProjectFile::from_toml(FILE_TOML))
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), false);
  assert_eq!((r.config.width, r.config.height), (1600, 900));
}

#[test]
fn a_dev_build_prefers_the_ryo_toml_on_disk() {
  let (_dir, ws) = temp_workspace(
    "[project]\nname = \"Disk Game\"\n[system]\nresolution = \"800x600\"\n",
    None,
  );
  let r = EngineBuilder::new()
    .project(crate::core::config::ProjectFile::from_toml(FILE_TOML))
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), false);
  assert_eq!((r.config.width, r.config.height), (800, 600));
  assert_eq!(r.config.title, "Disk Game");
}

/// Turns the music down in its update
struct MusicDown;

impl Scene for MusicDown {
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    ctx.settings().set_volume(VolumeKind::Music, 0.3);
    SceneAction::Continue
  }
}

#[test]
fn a_volume_change_reaches_the_mixer_that_frame() {
  use crate::core::audio::AudioEngine;
  use crate::core::testing::HeadlessEngine;

  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(64, 64)
      .start_scene(MusicDown),
  ) else {
    return;
  };
  let music = |engine: &HeadlessEngine| {
    engine
      .world()
      .resource::<AudioEngine>()
      .volume_setting(VolumeKind::Music)
  };
  assert_eq!(music(&engine), 1.0);
  engine.step().unwrap();
  assert_eq!(music(&engine), 0.3);
}

#[test]
fn settings_changes_become_window_and_renderer_requests() {
  use crate::core::audio::AudioEngine;

  let mut engine = EngineBuilder::new()
    .start_scene(EmptyScene)
    .build_headless();
  // A HiDPI window, and the overlay a windowed run shows
  engine.scale_factor = 2.0;
  engine.headless = false;
  {
    let mut settings = engine.world.resource_mut::<Settings>();
    settings.set_volume(VolumeKind::Sfx, 0.25);
    settings.set_window_size(800, 600);
    settings.set_vsync(false);
    settings.set_show_fps(true);
  }
  engine.apply_settings();
  assert_eq!(
    engine
      .world
      .resource::<AudioEngine>()
      .volume_setting(VolumeKind::Sfx),
    0.25
  );
  let request = std::mem::take(&mut *engine.world.resource_mut::<WindowRequest>());
  assert_eq!(
    (request.width, request.height),
    (Some(1600), Some(1200)),
    "the window's physical size"
  );
  assert_eq!(request.display_mode, None);
  assert_eq!(engine.pending_vsync.take(), Some(false));
  assert!(engine.config.debug.show_fps);
  assert_eq!(engine.overlay_mode, OverlayMode::FpsOnly);

  // Nothing changed since: nothing is asked again
  engine.apply_settings();
  assert!(!engine.world.resource::<WindowRequest>().has_changes());
  assert_eq!(engine.pending_vsync, None);

  // Fullscreen: the mode goes at once, the window size waits for a window
  {
    let mut settings = engine.world.resource_mut::<Settings>();
    settings.set_display_mode(DisplayMode::BorderlessFullscreen);
    settings.set_window_size(1024, 768);
  }
  engine.apply_settings();
  let request = std::mem::take(&mut *engine.world.resource_mut::<WindowRequest>());
  assert_eq!(
    request.display_mode,
    Some(DisplayMode::BorderlessFullscreen)
  );
  assert_eq!((request.width, request.height), (None, None));
  engine
    .world
    .resource_mut::<Settings>()
    .set_display_mode(DisplayMode::Windowed);
  engine.apply_settings();
  let request = std::mem::take(&mut *engine.world.resource_mut::<WindowRequest>());
  assert_eq!(request.display_mode, Some(DisplayMode::Windowed));
  assert_eq!((request.width, request.height), (Some(2048), Some(1536)));

  engine.world.resource_mut::<Settings>().set_show_fps(false);
  engine.apply_settings();
  assert!(!engine.config.debug.show_fps);
  assert_eq!(engine.overlay_mode, OverlayMode::Off);
}

#[test]
fn f1_keeps_the_fps_setting_in_step_with_the_overlay() {
  use crate::core::config::UserPreferenceFilter;

  let mut engine = EngineBuilder::new()
    .with_user_preferences(UserPreferenceFilter::all())
    .start_scene(EmptyScene)
    .build_headless();
  engine.headless = false;
  engine.apply_settings();
  for (mode, fps) in [
    (OverlayMode::FpsOnly, true),
    (OverlayMode::FrameTiming, false),
    (OverlayMode::FullStats, false),
    (OverlayMode::Off, false),
    (OverlayMode::FpsOnly, true),
  ] {
    engine.next_overlay();
    assert_eq!(engine.overlay_mode, mode);
    assert_eq!(
      engine.world.resource::<Settings>().show_fps(),
      fps,
      "{mode:?}"
    );
    assert_eq!(engine.applied_settings.values.show_fps, fps, "{mode:?}");
    // Already applied: nothing changes
    engine.apply_settings();
    assert_eq!(engine.overlay_mode, mode);
  }

  // The player's settings screen now turns off the counter F1 showed
  engine.world.resource_mut::<Settings>().set_show_fps(false);
  engine.apply_settings();
  assert_eq!(engine.overlay_mode, OverlayMode::Off);
  assert!(!engine.config.debug.show_fps);
}

#[test]
fn the_vsync_setting_picks_the_present_mode() {
  use crate::core::graphics::renderer::present_mode_for;
  use wgpu::PresentMode::{Fifo, Immediate, Mailbox};

  let every = [Fifo, Mailbox, Immediate];
  assert_eq!(present_mode_for(true, &every), Fifo);
  assert_eq!(present_mode_for(false, &every), Mailbox);
  assert_eq!(present_mode_for(false, &[Fifo, Immediate]), Immediate);
  assert_eq!(present_mode_for(false, &[Fifo]), Fifo);

  // The setting reaches the renderer after the frame
  let mut engine = EngineBuilder::new()
    .start_scene(EmptyScene)
    .build_headless();
  assert!(engine.vsync);
  engine.world.resource_mut::<Settings>().set_vsync(false);
  engine.apply_settings();
  assert_eq!(engine.pending_vsync, Some(false));
  engine.apply_headless_requests();
  assert!(!engine.vsync);
  assert_eq!(engine.pending_vsync, None);
}

#[test]
fn an_os_fullscreen_change_updates_settings_without_echo() {
  let mut engine = EngineBuilder::new()
    .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
    .start_scene(EmptyScene)
    .build_headless();
  engine.apply_settings();

  engine.follow_os_window(DisplayMode::BorderlessFullscreen, None);
  assert_eq!(
    engine.world.resource::<Settings>().display_mode(),
    DisplayMode::BorderlessFullscreen
  );
  assert_eq!(
    engine.world.resource::<WindowInfo>().display_mode,
    DisplayMode::BorderlessFullscreen
  );
  engine.apply_settings();
  assert!(
    !engine.world.resource::<WindowRequest>().has_changes(),
    "the window is already there"
  );

  // Back in a window, which the player drags to a new size
  engine.follow_os_window(DisplayMode::Windowed, Some((900, 500)));
  let settings = engine.world.resource::<Settings>();
  assert_eq!(settings.display_mode(), DisplayMode::Windowed);
  assert_eq!(settings.window_size(), (900, 500));
  engine.apply_settings();
  assert!(!engine.world.resource::<WindowRequest>().has_changes());

  // A size while fullscreen is the screen's, not the player's window
  engine.follow_os_window(DisplayMode::ExclusiveFullscreen, Some((1920, 1080)));
  assert_eq!(
    engine.world.resource::<Settings>().window_size(),
    (900, 500)
  );
  engine.apply_settings();
  assert!(!engine.world.resource::<WindowRequest>().has_changes());
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Video {
  subtitles: bool,
  scale: u32,
}

impl Default for Video {
  fn default() -> Self {
    Video {
      subtitles: true,
      scale: 1,
    }
  }
}

/// Reads the `Video` section as it enters
struct ReadsVideo(Arc<std::sync::Mutex<Option<Video>>>);

impl Scene for ReadsVideo {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    *self.0.lock().unwrap() = Some(ctx.settings().game::<Video>().clone());
  }
}

#[test]
fn a_game_section_registered_on_the_builder_is_readable_in_the_first_scene() {
  let store = MemorySaves::new();
  store.put(
    "system.json",
    r#"{"format_version": 3, "game": {"video": {"version": 1, "value": {"scale": 2}}}}"#,
  );
  let seen = Arc::new(std::sync::Mutex::new(None));
  let Some(_engine) = crate::core::testing::HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(64, 64)
      .memory_saves(store)
      .game_settings::<Video>("video")
      .start_scene(ReadsVideo(Arc::clone(&seen))),
  ) else {
    return;
  };
  assert_eq!(
    *seen.lock().unwrap(),
    Some(Video {
      subtitles: true,
      scale: 2
    })
  );
}

/// A build keeping every setting, its saves and settings in `store`
fn keeping_all(store: &MemorySaves) -> Engine {
  EngineBuilder::new()
    .memory_saves(store.clone())
    .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
    .start_scene(EmptyScene)
    .build_headless()
}

/// Rebinds `jump` (bound to Space) in slot 0
fn rebind_jump(engine: &mut Engine, key: KeyCode) {
  use crate::core::input::Binding;
  let mut input = engine.world.resource_mut::<Input>();
  let actions = input.actions_mut();
  if actions.bindings("jump").is_empty() {
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
  }
  actions.rebind("jump", 0, Binding::Key(key));
}

#[test]
fn bindings_land_in_the_same_file_as_the_settings() {
  let store = MemorySaves::new();
  let mut engine = keeping_all(&store);
  rebind_jump(&mut engine, KeyCode::KeyJ);
  engine
    .world
    .resource_mut::<Settings>()
    .set_volume(VolumeKind::Music, 0.5);
  engine.apply_settings();
  let now = Instant::now();
  engine.save_settings(now);
  engine.save_settings(now + DEBOUNCE);
  engine.flush_saves();
  assert_eq!(store.writes(), 1, "one write for both");
  assert_eq!(
    settings_written(&store),
    serde_json::json!({
      "format_version": 3,
      "music_volume": 0.5,
      "bindings": {"jump": ["Key:KeyJ"]}
    })
  );
}

#[test]
fn a_binding_set_back_to_its_default_leaves_the_file() {
  let store = MemorySaves::new();
  let mut engine = keeping_all(&store);
  rebind_jump(&mut engine, KeyCode::KeyJ);
  let now = Instant::now();
  engine.save_settings(now);
  engine.save_settings(now + DEBOUNCE);
  engine.flush_saves();
  assert_eq!(
    settings_written(&store)["bindings"],
    serde_json::json!({"jump": ["Key:KeyJ"]})
  );

  rebind_jump(&mut engine, KeyCode::Space);
  let now = now + DEBOUNCE * 2;
  engine.save_settings(now);
  engine.save_settings(now + DEBOUNCE);
  engine.flush_saves();
  assert_eq!(store.writes(), 2);
  assert_eq!(
    settings_written(&store),
    serde_json::json!({"format_version": 3})
  );
}

/// Turns the music down in its first update, and quits
struct MusicDownAndQuit;

impl Scene for MusicDownAndQuit {
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    ctx.settings().set_volume(VolumeKind::Music, 0.3);
    SceneAction::Quit
  }
}

#[test]
fn quitting_writes_a_pending_change_at_once() {
  let store = MemorySaves::new();
  let mut engine = keeping_all(&store);
  engine
    .world
    .resource_mut::<Settings>()
    .set_volume(VolumeKind::Sfx, 0.25);
  engine.apply_settings();
  engine.save_settings(Instant::now());
  engine.flush_saves();
  assert_eq!(store.get("system.json"), None, "the second is not up");
  // Closing the window ends the loop with no frame left to apply this
  engine.world.resource_mut::<Settings>().set_vsync(false);
  drop(engine);
  assert_eq!(store.writes(), 1);
  assert_eq!(
    settings_written(&store),
    serde_json::json!({"format_version": 3, "sfx_volume": 0.25, "vsync": false})
  );

  // A run that quits in the frame that changed a setting has written it by
  // the time the run returns
  if crate::core::testing::shared_gpu().is_err() {
    eprintln!("skipping the offscreen run: no GPU available");
    return;
  }
  let store = MemorySaves::new();
  let mut engine = EngineBuilder::new()
    .resolution(32, 32)
    .memory_saves(store.clone())
    .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
    .start_scene(MusicDownAndQuit)
    .build_headless();
  engine.prepare_offscreen_run(AutoCaptureConfig::default(), InputScript::default());
  assert!(!engine.run_offscreen_frames().unwrap());
  assert_eq!(
    settings_written(&store),
    serde_json::json!({"format_version": 3, "music_volume": 0.3})
  );
}

/// How many warnings `log` captured contain `needle`
fn settings_warnings(log: &crate::core::testing::LogCapture, needle: &str) -> usize {
  log
    .warnings()
    .iter()
    .filter(|message| message.contains(needle))
    .count()
}

#[test]
fn a_failed_settings_write_warns_once_and_sends_settings_save_failed() {
  use crate::core::ecs::{MessageCursor, Messages};
  use crate::core::saves::SaveError;
  use crate::core::saves::memory::FaultOp;

  let log = crate::core::testing::capture_log();
  let store = MemorySaves::new();
  store.fail_from(FaultOp::Write, 0);
  let mut engine = keeping_all(&store);
  let mut cursor = MessageCursor::<SettingsSaveFailed>::default();
  for (round, volume) in [0.5, 0.25].into_iter().enumerate() {
    engine
      .world
      .resource_mut::<Settings>()
      .set_volume(VolumeKind::Music, volume);
    engine.apply_settings();
    engine.save_settings(Instant::now() + DEBOUNCE);
    engine.flush_saves();
    engine.schedules.run(Phase::PreUpdate, &mut engine.world);
    let messages = engine.world.resource::<Messages<SettingsSaveFailed>>();
    let failed: Vec<_> = cursor.read(messages).cloned().collect();
    assert_eq!(failed.len(), 1, "round {round}: {failed:?}");
    assert!(
      matches!(&*failed[0].error, SaveError::Io { path, .. } if path == "system.json"),
      "{:?}",
      failed[0].error
    );
  }
  assert_eq!(settings_warnings(&log, "could not save the settings"), 1);
  assert_eq!(store.get("system.json"), None);
  // The game keeps running, at the player's setting
  assert_eq!(
    engine
      .world
      .resource::<Settings>()
      .volume(VolumeKind::Music),
    0.25
  );
}

#[test]
fn a_newer_settings_file_is_never_overwritten() {
  use crate::core::saves::memory::FaultOp;

  let log = crate::core::testing::capture_log();
  let newer = r#"{"format_version": 4, "music_volume": 0.2, "shiny": true}"#;
  let store = MemorySaves::new();
  store.put("system.json", newer);
  let mut engine = keeping_all(&store);
  assert!(engine.world.resource::<SettingsWriter>().read_only);
  assert_eq!(
    engine
      .world
      .resource::<Settings>()
      .volume(VolumeKind::Music),
    0.2,
    "the settings it knows are read"
  );
  engine
    .world
    .resource_mut::<Settings>()
    .set_volume(VolumeKind::Music, 0.7);
  engine.apply_settings();
  engine.save_settings(Instant::now() + DEBOUNCE);
  drop(engine);
  assert_eq!(store.get("system.json"), Some(newer.as_bytes().to_vec()));
  assert_eq!(store.writes(), 0);

  // A damaged file that could not be set aside is not replaced either
  let store = MemorySaves::new();
  store.put("system.json", "not json");
  store.fail(FaultOp::Rename, 0);
  let mut engine = keeping_all(&store);
  assert!(engine.world.resource::<SettingsWriter>().read_only);
  engine.world.resource_mut::<Settings>().set_show_fps(true);
  engine.apply_settings();
  engine.save_settings(Instant::now() + DEBOUNCE);
  drop(engine);
  assert_eq!(store.get("system.json"), Some(b"not json".to_vec()));
  assert_eq!(store.writes(), 0);
  assert_eq!(
    settings_warnings(&log, "so the settings changed this session are not saved"),
    2,
    "once a run"
  );
}

#[test]
fn a_build_that_keeps_nothing_never_writes_settings() {
  let store = MemorySaves::new();
  let mut engine = EngineBuilder::new()
    .memory_saves(store.clone())
    .start_scene(EmptyScene)
    .build_headless();
  assert!(!engine.world.contains_resource::<SettingsWriter>());
  engine
    .world
    .resource_mut::<Settings>()
    .set_volume(VolumeKind::Music, 0.5);
  engine.apply_settings();
  engine.save_settings(Instant::now() + DEBOUNCE);
  drop(engine);
  assert!(store.names().is_empty(), "{:?}", store.names());
}

/// An engine started as the web starts it: no save storage until the
/// browser's arrives
fn started_as_on_the_web(builder: EngineBuilder) -> Engine {
  Engine::new(builder.hot_reload(false).resolve_in(None, false), true)
}

fn keeping_all_on_the_web() -> Engine {
  started_as_on_the_web(
    EngineBuilder::new()
      .with_user_preferences(crate::core::config::UserPreferenceFilter::all())
      .start_scene(EmptyScene),
  )
}

#[test]
fn the_browser_store_arriving_reads_the_settings_and_keeps_the_saves() {
  use crate::core::audio::AudioEngine;
  use crate::core::saves::browser::fake::FakeIdb;
  use crate::core::saves::{SaveError, SaveInfo};

  let file = br#"{"format_version": 3, "music_volume": 0.3, "vsync": false, "bindings": {"jump": ["Key:KeyJ"]}}"#;
  let (browser, storage) = FakeIdb::opened(&[("system.json", file)]);
  let mut engine = keeping_all_on_the_web();
  assert!(matches!(
    engine
      .world
      .resource::<SaveService>()
      .save("web-slot", 1u32, SaveInfo::new()),
    Err(SaveError::Unavailable)
  ));
  assert_eq!(
    engine
      .world
      .resource::<Settings>()
      .volume(VolumeKind::Music),
    1.0
  );

  engine.store_arrived(Ok(Arc::new(storage)));
  let settings = engine.world.resource::<Settings>();
  assert_eq!(settings.volume(VolumeKind::Music), 0.3);
  assert!(!settings.vsync());
  assert_eq!(engine.applied_settings.values, *settings.values());
  assert!(!engine.vsync);
  assert_eq!(
    engine
      .world
      .resource::<AudioEngine>()
      .volume_setting(VolumeKind::Music),
    0.3,
    "before any sound plays"
  );
  assert_eq!(
    engine
      .world
      .resource_mut::<Input>()
      .actions_mut()
      .saved_map()["jump"],
    ["Key:KeyJ"]
  );
  // What was read is not written back
  engine.apply_settings();
  engine.save_settings(Instant::now() + DEBOUNCE);
  assert!(browser.waiting().is_empty());

  // The saves go to the browser's store
  let saves = engine.world.resource::<SaveService>();
  saves.save("web-slot", 7u32, SaveInfo::new()).unwrap();
  browser.wait_for(1);
  browser.release(Ok(()));
  engine.flush_saves();
  assert!(browser.db().contains_key("web-slot.ryo-save"));
}

#[test]
fn a_damaged_settings_file_in_the_browser_is_written_only_once_its_set_aside_lands() {
  use crate::core::saves::browser::IdbFailure;
  use crate::core::saves::browser::fake::FakeIdb;

  let log = crate::core::testing::capture_log();
  let damaged: &[(&str, &[u8])] = &[
    ("system.json", b"not json"),
    (
      "system.json.bak",
      br#"{"format_version": 3, "music_volume": 0.4}"#,
    ),
  ];
  let set_aside = vec![
    crate::core::saves::browser::IdbOp::Put("system.json.damaged".into(), b"not json".to_vec()),
    crate::core::saves::browser::IdbOp::Delete("system.json".into()),
  ];
  let change_music = |engine: &mut Engine, volume: f32| {
    engine
      .world
      .resource_mut::<Settings>()
      .set_volume(VolumeKind::Music, volume);
    engine.apply_settings();
  };
  // Whether the writer queues a write, the second being up
  let write_due = |engine: &mut Engine| {
    let mut queued = false;
    engine.with_settings_writer(|writer, settings, saves| {
      queued = writer.tick(Instant::now() + DEBOUNCE, settings, saves);
    });
    queued
  };

  // The set-aside fails in the browser: the file is never overwritten
  let (browser, storage) = FakeIdb::opened(damaged);
  let mut engine = keeping_all_on_the_web();
  engine.store_arrived(Ok(Arc::new(storage)));
  assert_eq!(
    engine
      .world
      .resource::<Settings>()
      .volume(VolumeKind::Music),
    0.4,
    "the backup's"
  );
  assert_eq!(browser.waiting(), std::slice::from_ref(&set_aside));
  change_music(&mut engine, 0.6);
  assert!(!write_due(&mut engine), "no write while it commits");
  browser.release(Err(IdbFailure::Other("set-aside-refused-in-test".into())));
  assert!(!write_due(&mut engine));
  assert!(engine.world.resource::<SettingsWriter>().read_only);
  drop(engine);
  assert!(browser.waiting().is_empty());
  assert_eq!(browser.db()["system.json"], b"not json");
  assert_eq!(settings_warnings(&log, "set-aside-refused-in-test"), 1);

  // It lands: the change waiting for it is written
  let (browser, storage) = FakeIdb::opened(damaged);
  let mut engine = keeping_all_on_the_web();
  engine.store_arrived(Ok(Arc::new(storage)));
  change_music(&mut engine, 0.6);
  assert!(!write_due(&mut engine));
  assert_eq!(browser.waiting(), [set_aside]);
  browser.release(Ok(()));
  assert!(write_due(&mut engine), "the change kept for it");
  browser.wait_for(1);
  browser.release(Ok(()));
  engine.flush_saves();
  drop(engine);
  let db = browser.db();
  assert_eq!(db["system.json.damaged"], b"not json");
  assert_eq!(
    serde_json::from_slice::<serde_json::Value>(&db["system.json"]).unwrap(),
    serde_json::json!({"format_version": 3, "music_volume": 0.6})
  );
  assert!(db.contains_key("system.json.bak"), "the backup stays");
}

#[test]
fn a_headless_engine_opens_no_log_file() {
  // Built on a project, a headless engine reads no folder for a log
  let (dir, ws) = temp_workspace("", None);
  let resolved = EngineBuilder::new()
    .resolution(64, 64)
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), true);
  let engine = Engine::new(resolved, true);
  assert_eq!(engine.log_folder, None);
  drop(engine);
  assert!(!ws.save_location().join("logs").exists());
  assert!(!dir.path().join("logs").exists());

  // A windowed engine on the same project keeps its log in the save
  // folder, which it opens only when it runs
  let resolved = EngineBuilder::new()
    .resolution(64, 64)
    .start_scene(EmptyScene)
    .resolve_in(Some(&ws), false);
  let engine = Engine::new(resolved, false);
  assert_eq!(engine.log_folder, Some(ws.save_location()));
  drop(engine);
  assert!(!ws.save_location().join("logs").exists());
}

/// Warns each update, so the log has a record from every frame
struct Ticking;

impl Scene for Ticking {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    crate::warn!("t5 tick");
    SceneAction::Continue
  }
}

#[test]
fn two_engines_stamp_their_own_frames() {
  let run = |frames: u64| {
    std::thread::spawn(move || {
      let log = crate::core::testing::capture_log();
      let mut engine = EngineBuilder::new()
        .resolution(32, 32)
        .start_scene(Ticking)
        .build_headless();
      let dt = engine.fixed_timestep();
      for _ in 0..frames {
        engine.frame(Some(dt));
      }
      let ticks: Vec<_> = log
        .records()
        .into_iter()
        .filter(|r| r.message == "t5 tick")
        .collect();
      (
        ticks.len() as u64,
        ticks.last().and_then(|r| r.frame),
        engine.world.resource::<Time>().frame_count(),
      )
    })
  };
  let (three, five) = (run(3), run(5));
  let three = three.join().expect("the first engine's thread");
  let five = five.join().expect("the second engine's thread");
  assert_eq!(three, (3, Some(3), 3));
  assert_eq!(five, (5, Some(5), 5));
}

// ===========================================================================
// The console's commands, with the engine's own state
// ===========================================================================

#[cfg(dev_tools)]
mod console {
  use std::time::Duration;

  use super::*;
  use crate::core::console::{Command, CommandSpec, Console, ConsoleCommands, colors};
  use crate::core::ecs::Entity;
  use crate::core::graphics::{Image, WgpuRenderer};
  use crate::core::particles::{ParticleEffect, ParticleEmitter};

  /// An engine with a GPU and its first scene entered, or `None` (the test
  /// passes) on a machine without one
  fn engine(builder: EngineBuilder) -> Option<Engine> {
    let (device, queue) = crate::core::testing::shared_gpu().ok()?;
    let mut e = builder.resolution(64, 64).build_headless();
    // On before the start scene enters, so what it plays is in the log
    if let Some(mut audio) = e
      .world
      .get_resource_mut::<crate::core::audio::AudioEngine>()
    {
      audio.set_logging(true);
    }
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 64), 1.0);
    Some(e)
  }

  fn step(e: &mut Engine) {
    e.frame(Some(Duration::from_micros(16_667)));
  }

  #[test]
  fn debug_fps_and_f1_agree() {
    let Some(mut e) = engine(EngineBuilder::new().start_scene(EmptyScene)) else {
      return;
    };
    assert_eq!(e.overlay_mode, OverlayMode::Off);
    e.console_run("debug:fps");
    assert!(
      e.world.resource::<Settings>().show_fps(),
      "the player's setting"
    );
    assert!(e.config.debug.show_fps);
    assert_eq!(e.overlay_mode, OverlayMode::FpsOnly);
    step(&mut e);
    e.next_overlay();
    assert_eq!(
      e.overlay_mode,
      OverlayMode::FrameTiming,
      "F1 carries on from the FPS counter, not from the first mode"
    );
    assert_eq!(
      e.world.resource::<Settings>().show_fps(),
      e.config.debug.show_fps,
      "the setting follows F1 too"
    );
  }

  #[test]
  fn debug_stats_and_timing_set_the_overlay_mode() {
    let Some(mut e) = engine(EngineBuilder::new().start_scene(EmptyScene)) else {
      return;
    };
    e.console_run("debug:stats");
    assert_eq!(e.overlay_mode, OverlayMode::FullStats);
    assert!(e.config.debug.show_stats && e.config.debug.show_frame_timing);
    e.next_overlay();
    assert_eq!(e.overlay_mode, OverlayMode::Off, "F1 after the last mode");

    e.console_run("debug:timing");
    assert_eq!(e.overlay_mode, OverlayMode::FrameTiming);
    assert!(e.config.debug.show_frame_timing && !e.config.debug.show_stats);
    e.next_overlay();
    assert_eq!(e.overlay_mode, OverlayMode::FullStats);

    e.console_run("debug:stats");
    assert_eq!(e.overlay_mode, OverlayMode::Off, "a shown overlay goes");
    assert!(!e.config.debug.show_stats && !e.config.debug.show_frame_timing);
  }

  #[test]
  fn a_handler_s_error_prints_in_red() {
    let Some(mut e) = engine(
      EngineBuilder::new()
        .console_command(("game:fail", "Fail"), |_, cmd: &mut Command<'_>| {
          cmd.print("before");
          Err("it broke".to_string())
        })
        .start_scene(EmptyScene),
    ) else {
      return;
    };
    assert_eq!(e.console_run("game:fail"), ["before", "it broke"]);
    let console = e.world.resource::<Console>();
    let tail: Vec<_> = console
      .lines()
      .rev()
      .take(3)
      .map(|line| (line.text.clone(), line.color))
      .collect();
    assert_eq!(
      tail,
      [
        ("it broke".to_string(), colors::ERROR),
        ("before".to_string(), colors::TEXT),
        ("> game:fail".to_string(), colors::COMMAND),
      ],
      "the echo, the printed line, then the error in red"
    );
  }

  #[test]
  fn help_lists_every_command_and_only_those() {
    let Some(mut e) = engine(
      EngineBuilder::new()
        .console_command(
          CommandSpec::new("game:spawn", "Spawn things")
            .usage("game:spawn <kind> [count]")
            .help("Spawns at the player."),
          |_, _| Ok(()),
        )
        .remove_console_command("debug:origins")
        .start_scene(EmptyScene),
    ) else {
      return;
    };
    let names: Vec<String> = e
      .world
      .resource::<ConsoleCommands>()
      .list()
      .into_iter()
      .map(|spec| spec.name)
      .collect();
    assert!(names.len() > 20, "{names:?}");
    assert!(!names.iter().any(|name| name == "debug:origins"));
    let help = e.console_run("help");
    let listed: Vec<&str> = help
      .iter()
      .filter_map(|line| line.strip_prefix("  "))
      .filter_map(|line| line.split_whitespace().next())
      .collect();
    let mut sorted_listed: Vec<&str> = listed.clone();
    sorted_listed.sort_unstable();
    let mut sorted_names: Vec<&str> = names.iter().map(String::as_str).collect();
    sorted_names.sort_unstable();
    assert_eq!(sorted_listed, sorted_names, "{help:#?}");
    assert!(
      help.iter().position(|l| l.starts_with("  clear"))
        < help.iter().position(|l| l.starts_with("  debug:fps")),
      "commands with no prefix first: {help:#?}"
    );

    let one = e.console_run("help game:spawn").join("\n");
    for part in [
      "game:spawn",
      "Spawn things",
      "game:spawn <kind> [count]",
      "Spawns at the player.",
    ] {
      assert!(one.contains(part), "{part} in {one}");
    }
    let unknown = e.console_run("help debug:origins").join("\n");
    assert!(unknown.contains("Unknown command"), "{unknown}");
  }

  /// The emitter `game:burst` played last
  #[derive(Resource, Clone, Copy)]
  struct Burst(Entity, u32);

  #[derive(Default)]
  struct Bursts {
    fx: Option<crate::core::asset::Handle<ParticleEffect>>,
    clip: Option<crate::core::asset::Handle<crate::core::audio::AudioClip>>,
  }

  impl Scene for Bursts {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let image = ctx
        .assets()
        .add(Image::solid_rect(8, 8, 255, 255, 255, 255));
      self.fx = Some(ctx.assets().add(ParticleEffect::new(image)));
      // Four seconds of silence: still playing when its scene is held
      let silence = vec![[0.0; 2]; 4 * 48_000];
      let clip = crate::core::audio::AudioClip::from_frames(48_000, &silence).ok();
      self.clip = clip.map(|clip| ctx.assets().add(clip));
      ctx.console().command(
        ("game:burst", "Play a burst"),
        |scene: &mut Bursts, ctx, _| {
          let fx = scene.fx.clone().ok_or("no effect")?;
          let clip = scene.clip.clone().ok_or("no clip")?;
          let burst = ctx.particles().play_at(&fx, glam::Vec2::ZERO);
          let voice = ctx.audio().play(&clip).id();
          ctx.world.insert_resource(Burst(burst, voice));
          Ok(())
        },
      );
    }
  }

  #[test]
  fn a_command_s_sound_and_particles_belong_to_its_scene() {
    use crate::core::particles::ParticleStore;
    let Some(mut e) = engine(
      EngineBuilder::new()
        .console_command(("test:push", "Push a scene over"), |_, cmd| {
          cmd.act(SceneAction::Push(Box::new(EmptyScene)));
          Ok(())
        })
        .console_command(("test:pop", "Pop"), |_, cmd| {
          cmd.act(SceneAction::Pop);
          Ok(())
        })
        .start_scene(Bursts::default()),
    ) else {
      return;
    };
    step(&mut e);
    let scope = frame::with_host(&mut e.world, |_, host| host.scenes.top_scope());
    assert!(e.console_run("game:burst").is_empty());
    let Burst(burst, voice) = *e.world.resource::<Burst>();
    assert_ne!(voice, 0, "the sound started");
    // The voice's pauses and resumes in the audio log
    let holds = |e: &Engine| -> Vec<crate::core::audio::AudioLogKind> {
      use crate::core::audio::AudioLogKind::{Pause, Resume};
      e.world
        .resource::<crate::core::audio::AudioEngine>()
        .log()
        .iter()
        .filter(|entry| entry.voice == voice && matches!(entry.kind, Pause | Resume))
        .map(|entry| entry.kind)
        .collect()
    };
    let held = |e: &Engine| {
      let slot = e
        .world
        .get::<ParticleEmitter>(burst)
        .map(|emitter| emitter.slot);
      let store = e.world.resource::<ParticleStore>();
      let pool = slot.and_then(|slot| store.pool(slot));
      (
        pool.and_then(|pool| pool.scope),
        pool.is_some_and(|pool| store.holds(pool.scope)),
      )
    };
    assert_eq!(held(&e), (scope, false), "the scene's own emitter");
    e.console_run("test:push");
    step(&mut e);
    assert_eq!(held(&e), (scope, true), "held while its scene is");
    use crate::core::audio::AudioLogKind::{Pause, Resume};
    assert_eq!(holds(&e), [Pause], "its sound is held with its scene");
    e.console_run("test:pop");
    step(&mut e);
    assert_eq!(held(&e), (scope, false));
    assert_eq!(holds(&e), [Pause, Resume], "and let go with it");
  }

  #[test]
  fn a_closed_console_does_no_per_frame_work() {
    let Some(mut e) = engine(EngineBuilder::new().start_scene(EmptyScene)) else {
      return;
    };
    for _ in 0..3 {
      step(&mut e);
    }
    let ticks = |e: &Engine| {
      let ticks = e
        .world
        .get_resource_change_ticks::<Console>()
        .map(|ticks| (ticks.added, ticks.changed));
      (ticks, e.world.archetypes().len())
    };
    let before = ticks(&e);
    for _ in 0..60 {
      step(&mut e);
    }
    assert_eq!(
      ticks(&e),
      before,
      "neither changed nor taken out and put back"
    );
    // Open, it is worked on each frame
    e.world.resource_mut::<Console>().toggle();
    let open = ticks(&e);
    step(&mut e);
    assert_ne!(ticks(&e), open);
  }

  #[test]
  fn a_scene_entry_whose_scene_has_left_says_so_and_goes() {
    use crate::core::console::{Handler, erase};
    let Some(mut e) = engine(EngineBuilder::new().start_scene(EmptyScene)) else {
      return;
    };
    // An entry no scene on the stack owns, as a release that missed it would leave
    e.world.resource_mut::<ConsoleCommands>().add(
      CommandSpec::new("game:orphan", "Left behind"),
      Handler::Scene {
        scope: 9_999,
        call: erase(|_: &mut EmptyScene, _, _| Ok(())),
      },
    );
    assert_eq!(
      e.console_run("game:orphan"),
      ["game:orphan: the scene that registered it has left"]
    );
    assert!(
      e.world
        .resource::<ConsoleCommands>()
        .find("game:orphan")
        .is_none()
    );
    assert_eq!(
      e.console_run("game:orphan"),
      ["Unknown command: game:orphan. Type 'help'."]
    );
  }

  // --- The console's window: typing, completion and the log ---

  use crate::core::input::{Input, InputEvent};
  use crate::core::ui::Color;

  /// A headless engine with no GPU (the console's input and its mirror
  /// need none), built in the caller's log scope
  fn plain() -> Engine {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(EmptyScene)
      .build_headless()
  }

  /// One frame with `events` queued
  fn frame_with(e: &mut Engine, events: Vec<InputEvent>) {
    for event in events {
      e.world.resource_mut::<Input>().queue(event);
    }
    step(e);
  }

  /// A key pressed one frame and released the next
  fn tap(e: &mut Engine, key: KeyCode) {
    frame_with(e, vec![InputEvent::Key { key, pressed: true }]);
    frame_with(
      e,
      vec![InputEvent::Key {
        key,
        pressed: false,
      }],
    );
  }

  /// A key pressed with the text it types, then released
  fn type_key(e: &mut Engine, code: KeyCode, text: &str) {
    frame_with(
      e,
      vec![
        InputEvent::Key {
          key: code,
          pressed: true,
        },
        InputEvent::Text(text.to_string()),
      ],
    );
    frame_with(
      e,
      vec![InputEvent::Key {
        key: code,
        pressed: false,
      }],
    );
  }

  fn typed(e: &Engine) -> String {
    e.world.resource::<Console>().input_line().to_string()
  }

  /// The console's lines that start with `target`, with their colours
  fn shown(e: &Engine, target: &str) -> Vec<(String, Color)> {
    e.world
      .resource::<Console>()
      .lines()
      .filter(|line| line.text.starts_with(target))
      .map(|line| (line.text.clone(), line.color))
      .collect()
  }

  #[test]
  fn a_backquote_can_be_typed_into_a_command() {
    let mut e = plain();
    e.config.debug.dev_console = true;
    step(&mut e);
    // The press that opens the console is swallowed, key and character
    type_key(&mut e, KeyCode::Backquote, "`");
    assert!(e.world.resource::<Console>().visible);
    assert_eq!(typed(&e), "");
    frame_with(&mut e, vec![InputEvent::Text("a".into())]);
    type_key(&mut e, KeyCode::Backquote, "`");
    type_key(&mut e, KeyCode::Backquote, "~");
    assert!(e.world.resource::<Console>().visible, "still open");
    assert_eq!(typed(&e), "a`~");
  }

  #[test]
  fn tab_completes_a_command_name_and_lists_several() {
    let mut e = plain();
    e.world.resource_mut::<Console>().toggle();
    step(&mut e);
    let lines = |e: &Engine| e.world.resource::<Console>().lines().count();

    // One match: the name, and a space for its arguments
    frame_with(&mut e, vec![InputEvent::Text("system:ver".into())]);
    tap(&mut e, KeyCode::Tab);
    assert_eq!(typed(&e), "system:version ");
    // After the first word, Tab does nothing
    let before = lines(&e);
    tap(&mut e, KeyCode::Tab);
    assert_eq!(typed(&e), "system:version ");
    assert_eq!(lines(&e), before);
    tap(&mut e, KeyCode::Enter);
    assert_eq!(typed(&e), "");

    // Several: their common start, then a second Tab lists them on one line
    frame_with(&mut e, vec![InputEvent::Text("sys".into())]);
    tap(&mut e, KeyCode::Tab);
    assert_eq!(typed(&e), "system:");
    let before = lines(&e);
    tap(&mut e, KeyCode::Tab);
    assert_eq!(typed(&e), "system:");
    assert_eq!(lines(&e), before + 1, "one line");
    let listed = e
      .world
      .resource::<Console>()
      .lines()
      .last()
      .map(|line| line.text.clone())
      .unwrap_or_default();
    for name in [
      "system:assets",
      "system:entities",
      "system:scenes",
      "system:timescale",
      "system:version",
    ] {
      assert!(listed.contains(name), "{name} in {listed:?}");
    }

    // A first Tab that adds nothing waits for the second
    for _ in 0.."system:".len() {
      tap(&mut e, KeyCode::Backspace);
    }
    frame_with(&mut e, vec![InputEvent::Text("debug:c".into())]);
    let before = lines(&e);
    tap(&mut e, KeyCode::Tab);
    assert_eq!((typed(&e), lines(&e)), ("debug:c".to_string(), before));
    tap(&mut e, KeyCode::Tab);
    let listed = e
      .world
      .resource::<Console>()
      .lines()
      .last()
      .map(|line| line.text.clone())
      .unwrap_or_default();
    assert_eq!(listed, "debug:capture  debug:colliders");
  }

  #[test]
  fn warnings_and_errors_appear_in_the_console_and_info_does_not() {
    let _log = crate::core::testing::capture_log();
    let mut e = plain();
    e.world.resource_mut::<Console>().toggle();
    step(&mut e);
    log::warn!(target: "game::mirror_test", "a warning");
    log::error!(target: "game::mirror_test", "an error");
    log::info!(target: "game::mirror_test", "some info");
    for _ in 0..10 {
      step(&mut e);
    }
    assert_eq!(
      shown(&e, "game::mirror_test"),
      [
        ("game::mirror_test: a warning".to_string(), colors::WARNING),
        ("game::mirror_test: an error".to_string(), colors::ERROR),
      ],
      "each once, in yellow and red"
    );
  }

  #[test]
  fn log_show_off_stops_the_mirror() {
    let _log = crate::core::testing::capture_log();
    let mut e = plain();
    e.world.resource_mut::<Console>().toggle();
    step(&mut e);
    e.console_run("log:show off");
    log::warn!(target: "game::mirror_off", "while off");
    for _ in 0..3 {
      step(&mut e);
    }
    assert!(shown(&e, "game::mirror_off").is_empty());
    e.console_run("log:show warn");
    log::warn!(target: "game::mirror_off", "back on");
    step(&mut e);
    assert_eq!(
      shown(&e, "game::mirror_off"),
      [("game::mirror_off: back on".to_string(), colors::WARNING)]
    );
  }

  #[test]
  fn two_engines_mirror_only_their_own_warnings() {
    use std::sync::{Arc, Barrier};
    let both_logged = Arc::new(Barrier::new(2));
    let run = |name: &'static str| {
      let both_logged = Arc::clone(&both_logged);
      std::thread::spawn(move || {
        let log = crate::core::testing::capture_log();
        let mut e = plain();
        e.world.resource_mut::<Console>().toggle();
        log::warn!(target: "game::two_engines", "{name}'s warning");
        both_logged.wait();
        step(&mut e);
        assert_eq!(log.warnings(), [format!("{name}'s warning")]);
        shown(&e, "game::two_engines")
          .into_iter()
          .map(|(text, _)| text)
          .collect::<Vec<_>>()
      })
    };
    let (a, b) = (run("a"), run("b"));
    let a = a.join().expect("the first engine's thread");
    let b = b.join().expect("the second engine's thread");
    assert_eq!(a, ["game::two_engines: a's warning"]);
    assert_eq!(b, ["game::two_engines: b's warning"]);
  }

  #[test]
  fn warnings_logged_while_closed_are_there_when_it_opens() {
    // This test fills the ring past its size
    let _turn = crate::core::logging::ring_turn();
    let _log = crate::core::testing::capture_log_bounded(16);
    let mut e = plain();
    step(&mut e);
    log::warn!(target: "game::closed", "while closed");
    for _ in 0..5 {
      step(&mut e);
    }
    assert!(
      shown(&e, "game::closed").is_empty(),
      "a closed console waits"
    );
    e.world.resource_mut::<Console>().toggle();
    step(&mut e);
    assert_eq!(
      shown(&e, "game::closed"),
      [("game::closed: while closed".to_string(), colors::WARNING)]
    );

    // A flood while closed: the newest lines win, and one grey line counts
    // the records the ring no longer holds
    e.world.resource_mut::<Console>().toggle();
    step(&mut e);
    for i in 0..10_050 {
      log::warn!(target: "game::flood", "flood {i}");
    }
    e.world.resource_mut::<Console>().toggle();
    step(&mut e);
    let console = e.world.resource::<Console>();
    let lines: Vec<_> = console.lines().collect();
    assert!(lines.len() <= 1000, "{}", lines.len());
    assert_eq!(
      lines.last().map(|line| line.text.as_str()),
      Some("game::flood: flood 10049")
    );
    let hidden = lines
      .iter()
      .find(|line| {
        line
          .text
          .ends_with("earlier warnings and errors are no longer shown")
      })
      .expect("a line counting the warnings not shown");
    assert_eq!(hidden.color, colors::SYSTEM);
    let count: u64 = hidden
      .text
      .split_whitespace()
      .next()
      .and_then(|n| n.parse().ok())
      .unwrap_or(0);
    // Every flood warning but the 999 shown beside the count: those the ring
    // dropped and those past the cap, of this engine's scope alone
    assert_eq!(count, 10_050 - 999, "{}", hidden.text);
  }

  #[test]
  fn warnings_past_the_cap_are_counted_and_none_when_all_are_shown() {
    // A flood elsewhere could push these out of the ring before they are read
    let _turn = crate::core::logging::ring_turn();
    let _log = crate::core::testing::capture_log();
    let mut e = plain();
    step(&mut e);
    for i in 0..1_500 {
      log::warn!(target: "game::capped", "capped {i}");
    }
    e.world.resource_mut::<Console>().toggle();
    step(&mut e);
    let counts = |e: &Engine| -> Vec<String> {
      e.world
        .resource::<Console>()
        .lines()
        .filter(|line| line.text.ends_with("are no longer shown"))
        .map(|line| line.text.clone())
        .collect()
    };
    assert_eq!(
      counts(&e),
      ["501 earlier warnings and errors are no longer shown"]
    );
    // A few more, all shown: no new count
    for i in 0..3 {
      log::warn!(target: "game::capped", "more {i}");
    }
    step(&mut e);
    let tail: Vec<String> = e
      .world
      .resource::<Console>()
      .lines()
      .rev()
      .take(4)
      .map(|line| line.text.clone())
      .collect();
    assert_eq!(
      tail,
      [
        "game::capped: more 2",
        "game::capped: more 1",
        "game::capped: more 0",
        "game::capped: capped 1499",
      ],
      "no count before them"
    );
  }

  #[test]
  fn a_few_warnings_all_shown_are_not_counted() {
    let _log = crate::core::testing::capture_log();
    let mut e = plain();
    for i in 0..3 {
      log::warn!(target: "game::few", "few {i}");
    }
    e.world.resource_mut::<Console>().toggle();
    step(&mut e);
    let console = e.world.resource::<Console>();
    assert!(
      !console
        .lines()
        .any(|line| line.text.ends_with("are no longer shown"))
    );
    assert_eq!(shown(&e, "game::few").len(), 3);
  }

  #[test]
  fn an_open_console_with_nothing_new_takes_no_lock() {
    let mut e = plain();
    e.world.resource_mut::<Console>().toggle();
    step(&mut e);
    let reads = |e: &Engine| e.world.resource::<Console>().mirror_reads;
    let before = reads(&e);
    // Other tests log meanwhile, so only frames in which nothing was logged
    // anywhere are held to no read
    let mut logged = 0;
    let mut last = crate::core::logging::next_record_seq();
    for _ in 0..60 {
      step(&mut e);
      let now = crate::core::logging::next_record_seq();
      logged += usize::from(now != last);
      last = now;
    }
    let taken = reads(&e) - before;
    assert!(taken <= logged, "{taken} reads, {logged} frames logged");
  }

  #[test]
  fn the_builder_s_warnings_are_in_the_console() {
    let _log = crate::core::testing::capture_log();
    // A Ryo.toml that is not TOML: the builder warns as it resolves
    let resolved = EngineBuilder::new()
      .resolution(64, 64)
      .project(crate::core::config::ProjectFile::from_toml("not [ toml"))
      .start_scene(EmptyScene)
      .resolve_in(None, true);
    let mut e = Engine::new(resolved, true);
    e.world.resource_mut::<Console>().toggle();
    step(&mut e);
    let console = e.world.resource::<Console>();
    assert!(
      console.lines().any(|line| {
        line.text.contains("the project file is ignored") && line.color == colors::WARNING
      }),
      "{:#?}",
      console.lines().map(|line| &line.text).collect::<Vec<_>>()
    );
  }

  #[test]
  fn the_console_key_closes_an_empty_line_and_types_into_a_command() {
    let mut e = plain();
    e.config.debug.dev_console = true;
    step(&mut e);
    let open = |e: &Engine| e.world.resource::<Console>().visible;
    type_key(&mut e, KeyCode::Backquote, "`");
    assert!(open(&e));
    type_key(&mut e, KeyCode::Backquote, "`");
    assert!(!open(&e), "an empty line: the key closes it");
    type_key(&mut e, KeyCode::Backquote, "`");
    frame_with(&mut e, vec![InputEvent::Text("say ".into())]);
    type_key(&mut e, KeyCode::Backquote, "`");
    assert!(open(&e), "a line with text: the key types");
    assert_eq!(typed(&e), "say `");
  }

  #[test]
  fn the_opening_key_types_nothing_until_it_is_released() {
    let mut e = plain();
    e.config.debug.dev_console = true;
    step(&mut e);
    let open = |e: &Engine| e.world.resource::<Console>().visible;
    let press = |text: &str| {
      vec![
        InputEvent::Key {
          key: KeyCode::Backquote,
          pressed: true,
        },
        InputEvent::Text(text.to_string()),
      ]
    };
    // A layout whose key gives another character, then two auto-repeats
    frame_with(&mut e, press("²"));
    assert!(open(&e));
    frame_with(&mut e, press("²"));
    frame_with(&mut e, vec![]);
    frame_with(&mut e, press("²"));
    assert!(open(&e), "a repeat does not close it");
    assert_eq!(typed(&e), "");
    frame_with(
      &mut e,
      vec![InputEvent::Key {
        key: KeyCode::Backquote,
        pressed: false,
      }],
    );
    frame_with(&mut e, vec![InputEvent::Text("a".into())]);
    assert_eq!(typed(&e), "a");
    // Released, the key types into a line with text
    type_key(&mut e, KeyCode::Backquote, "²");
    assert_eq!(typed(&e), "a²");
  }

  #[test]
  fn a_tab_after_an_edit_starts_afresh() {
    let mut e = plain();
    e.world.resource_mut::<Console>().toggle();
    step(&mut e);
    let lines = |e: &Engine| e.world.resource::<Console>().lines().count();
    frame_with(&mut e, vec![InputEvent::Text("sys".into())]);
    tap(&mut e, KeyCode::Tab);
    assert_eq!(typed(&e), "system:");
    // Edited away and typed back: the next Tab is a first Tab
    for _ in 0.."system:".len() {
      tap(&mut e, KeyCode::Backspace);
    }
    frame_with(&mut e, vec![InputEvent::Text("system:".into())]);
    let before = lines(&e);
    tap(&mut e, KeyCode::Tab);
    assert_eq!(lines(&e), before, "no listing on a first Tab");
    tap(&mut e, KeyCode::Tab);
    assert_eq!(lines(&e), before + 1, "the second lists");
  }
}

#[cfg(feature = "profiler")]
#[test]
fn closing_with_the_x_button_clears_the_focus_flag() {
  let resolved = EngineBuilder::new()
    .start_scene(EmptyScene)
    .resolve_in(None, true);
  let mut engine = Engine::new(resolved, true);
  engine.profiler_window_focused = true;
  engine.logger_window_focused = true;

  // The handler's CloseRequested arms call these
  engine.close_profiler_window();
  assert!(!engine.profiler_window_focused);
  assert!(engine.profiler_window.is_none());
  assert!(engine.logger_window_focused);

  engine.close_logger_window();
  assert!(!engine.logger_window_focused);
}
