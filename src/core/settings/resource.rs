//! The [`Settings`] resource.

use std::any::type_name;
use std::fmt;

use super::EngineValues;
use super::file::{FORMAT, FileValues, SettingsFile, two_decimals};
use super::sections::SectionRegistry;
use crate::core::audio::VolumeKind;
use crate::core::config::{DisplayMode, UserPreference, UserPreferenceFilter};
use crate::core::ecs::Resource;

/// The preference that keeps each volume, in [`VolumeKind::ALL`] order
const VOLUME_PREFERENCES: [UserPreference; 6] = [
  UserPreference::MasterVolume,
  UserPreference::MusicVolume,
  UserPreference::SfxVolume,
  UserPreference::UiVolume,
  UserPreference::VoiceVolume,
  UserPreference::AmbienceVolume,
];

/// The player's settings: the engine's (volumes, window size, display mode,
/// vsync, the FPS counter) and the game's own sections.
///
/// Scenes reach it with `ctx.settings()`, systems with `Res<Settings>` and
/// `ResMut<Settings>`. A change applies at once and is saved shortly after.
/// The settings file keeps the settings the game keeps (its
/// `with_user_preferences` filter) whose values differ from the game's
/// defaults, and the game sections' changed fields; everything else lasts
/// for the session.
///
/// A game section is a type the game registers with
/// `EngineBuilder::game_settings` under a key, read with [`game`](Self::game)
/// and changed with [`game_mut`](Self::game_mut).
#[derive(Resource)]
pub struct Settings {
  values: EngineValues,
  defaults: EngineValues,
  kept: UserPreferenceFilter,
  sections: SectionRegistry,
}

impl Settings {
  /// A volume setting, from 0 (silent) to 1 (the mix as designed)
  pub fn volume(&self, kind: VolumeKind) -> f32 {
    self.values.volumes[kind.index()]
  }

  /// Sets a volume, clamped to 0..=1. NaN is the game's default.
  pub fn set_volume(&mut self, kind: VolumeKind, value: f32) {
    let index = kind.index();
    self.values.volumes[index] = if value.is_nan() {
      self.defaults.volumes[index]
    } else {
      kind.sanitize(value)
    };
  }

  /// The window's size in logical pixels, while windowed
  pub fn window_size(&self) -> (u32, u32) {
    self.values.window_size
  }

  /// Sets the window's size in logical pixels, each side at least 1
  pub fn set_window_size(&mut self, width: u32, height: u32) {
    self.values.window_size = at_least_one((width, height));
  }

  /// Windowed, exclusive fullscreen or borderless fullscreen
  pub fn display_mode(&self) -> DisplayMode {
    self.values.display_mode
  }

  /// Sets the display mode
  pub fn set_display_mode(&mut self, mode: DisplayMode) {
    self.values.display_mode = mode;
  }

  /// Whether presenting waits for the display (no tearing)
  pub fn vsync(&self) -> bool {
    self.values.vsync
  }

  /// Sets whether presenting waits for the display
  pub fn set_vsync(&mut self, vsync: bool) {
    self.values.vsync = vsync;
  }

  /// Whether the FPS counter shows
  pub fn show_fps(&self) -> bool {
    self.values.show_fps
  }

  /// Shows or hides the FPS counter
  pub fn set_show_fps(&mut self, show: bool) {
    self.values.show_fps = show;
  }

  /// Puts one setting back to the game's default. Bindings live with the
  /// actions: `ctx.actions().reset_defaults()` resets them, and
  /// `reset(UserPreference::Bindings)` does nothing.
  pub fn reset(&mut self, preference: UserPreference) {
    let defaults = self.defaults;
    match preference {
      UserPreference::Resolution => self.values.window_size = defaults.window_size,
      UserPreference::DisplayMode => self.values.display_mode = defaults.display_mode,
      UserPreference::Vsync => self.values.vsync = defaults.vsync,
      UserPreference::ShowFps => self.values.show_fps = defaults.show_fps,
      UserPreference::Bindings => {}
      volume @ (UserPreference::MasterVolume
      | UserPreference::MusicVolume
      | UserPreference::SfxVolume
      | UserPreference::UiVolume
      | UserPreference::VoiceVolume
      | UserPreference::AmbienceVolume) => {
        if let Some(index) = VOLUME_PREFERENCES.iter().position(|&kept| kept == volume) {
          self.values.volumes[index] = defaults.volumes[index];
        }
      }
    }
  }

  /// Puts every setting back to the game's defaults, the game sections to
  /// their types' defaults. Bindings are reset with
  /// `ctx.actions().reset_defaults()`.
  pub fn reset_all(&mut self) {
    self.values = self.defaults;
    self.sections.reset_all();
  }

  /// Whether the game keeps `preference` in the settings file (its
  /// `with_user_preferences` filter). A setting not kept lasts for the
  /// session.
  pub fn is_kept(&self, preference: UserPreference) -> bool {
    self.kept.contains(preference)
  }

  /// The game section of type `T`.
  ///
  /// # Panics
  ///
  /// When `T` is not registered with `EngineBuilder::game_settings`.
  pub fn game<T: 'static>(&self) -> &T {
    match self.sections.get::<T>() {
      Some(section) => section,
      None => unregistered::<T>(),
    }
  }

  /// The game section of type `T`, to change.
  ///
  /// # Panics
  ///
  /// When `T` is not registered with `EngineBuilder::game_settings`.
  pub fn game_mut<T: 'static>(&mut self) -> &mut T {
    match self.sections.get_mut::<T>() {
      Some(section) => section,
      None => unregistered::<T>(),
    }
  }
}

/// What the engine uses: making the settings, and reading and writing the
/// settings file
impl Settings {
  /// Settings at `defaults`, keeping the `kept` ones and the `sections` in
  /// the file
  pub(crate) fn new(
    defaults: EngineValues,
    kept: UserPreferenceFilter,
    sections: SectionRegistry,
  ) -> Settings {
    let mut defaults = defaults;
    for kind in VolumeKind::ALL {
      let volume = &mut defaults.volumes[kind.index()];
      *volume = kind.sanitize(*volume);
    }
    defaults.window_size = at_least_one(defaults.window_size);
    Settings {
      values: defaults,
      defaults,
      kept,
      sections,
    }
  }

  /// The engine's settings as they are now
  pub(crate) fn values(&self) -> &EngineValues {
    &self.values
  }

  /// Applies the kept values and the sections `file` holds. A section that
  /// does not read keeps its value, with a warning.
  pub(crate) fn apply_file(&mut self, file: &SettingsFile, warnings: &mut Vec<String>) {
    let values = &file.values;
    for kind in VolumeKind::ALL {
      if let Some(volume) = values.volumes[kind.index()]
        && self.is_kept(VOLUME_PREFERENCES[kind.index()])
      {
        self.set_volume(kind, volume);
      }
    }
    if let Some((width, height)) = values.window_size
      && self.is_kept(UserPreference::Resolution)
    {
      self.set_window_size(width, height);
    }
    if let Some(mode) = values.display_mode
      && self.is_kept(UserPreference::DisplayMode)
    {
      self.values.display_mode = mode;
    }
    if let Some(vsync) = values.vsync
      && self.is_kept(UserPreference::Vsync)
    {
      self.values.vsync = vsync;
    }
    if let Some(show) = values.show_fps
      && self.is_kept(UserPreference::ShowFps)
    {
      self.values.show_fps = show;
    }
    self.sections.apply(&file.sections, warnings);
  }

  /// The file to write: the kept values that differ from the defaults (a
  /// volume compared as the file keeps it, to two decimals), the kept
  /// bindings and the game sections, with `base`'s unknown keys and
  /// sections carried over
  #[cfg_attr(not(test), expect(dead_code, reason = "the settings writer's"))]
  pub(crate) fn to_file(&self, base: &SettingsFile) -> SettingsFile {
    let (now, defaults) = (&self.values, &self.defaults);
    let kept = |preference, differs: bool| differs && self.is_kept(preference);
    let mut values = FileValues::default();
    for kind in VolumeKind::ALL {
      let index = kind.index();
      let differs = two_decimals(now.volumes[index]) != two_decimals(defaults.volumes[index]);
      if kept(VOLUME_PREFERENCES[index], differs) {
        values.volumes[index] = Some(now.volumes[index]);
      }
    }
    let differs = now.window_size != defaults.window_size;
    values.window_size = kept(UserPreference::Resolution, differs).then_some(now.window_size);
    let differs = now.display_mode != defaults.display_mode;
    values.display_mode = kept(UserPreference::DisplayMode, differs).then_some(now.display_mode);
    values.vsync = kept(UserPreference::Vsync, now.vsync != defaults.vsync).then_some(now.vsync);
    let differs = now.show_fps != defaults.show_fps;
    values.show_fps = kept(UserPreference::ShowFps, differs).then_some(now.show_fps);

    SettingsFile {
      format: FORMAT,
      values,
      bindings: base
        .bindings
        .clone()
        .filter(|_| self.is_kept(UserPreference::Bindings)),
      sections: self.sections.store(&base.sections),
      unknown: base.unknown.clone(),
    }
  }
}

fn at_least_one((width, height): (u32, u32)) -> (u32, u32) {
  (width.max(1), height.max(1))
}

fn unregistered<T>() -> ! {
  panic!(
    "Settings::game::<{}>: register it with EngineBuilder::game_settings",
    type_name::<T>()
  )
}

impl fmt::Debug for Settings {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Settings")
      .field("values", &self.values)
      .field("defaults", &self.defaults)
      .field("kept", &self.kept)
      .field("sections", &self.sections)
      .finish()
  }
}

#[cfg(test)]
mod tests {
  use serde::{Deserialize, Serialize};
  use serde_json::{Map, Value, json};

  use super::*;
  use crate::core::saves::SaveVersions;
  use crate::core::settings::file::{self, Parsed, RawSection, fixtures};

  fn settings(defaults: EngineValues) -> Settings {
    Settings::new(
      defaults,
      UserPreferenceFilter::all(),
      SectionRegistry::default(),
    )
  }

  fn read(bytes: &[u8], project_env: &Map<String, Value>) -> SettingsFile {
    match file::parse(bytes, project_env) {
      Parsed::Read { file, warnings } => {
        assert!(warnings.is_empty(), "{warnings:?}");
        file
      }
      other => panic!("expected Read, got {other:?}"),
    }
  }

  /// `settings` written over `base`, as the JSON the file holds
  fn written(settings: &Settings, base: &SettingsFile) -> Value {
    serde_json::from_slice(&file::render(&settings.to_file(base))).unwrap()
  }

  fn env(value: Value) -> Map<String, Value> {
    match value {
      Value::Object(map) => map,
      other => panic!("not an object: {other}"),
    }
  }

  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  struct Video {
    subtitles: bool,
    scale: u32,
    language: String,
  }

  impl Default for Video {
    fn default() -> Self {
      Video {
        subtitles: true,
        scale: 1,
        language: "en".into(),
      }
    }
  }

  fn with_video(defaults: EngineValues) -> Settings {
    let mut sections = SectionRegistry::default();
    sections.register::<Video>("video", None);
    Settings::new(defaults, UserPreferenceFilter::all(), sections)
  }

  /// Registers `T` as the section `s`, changes it, writes the file and loads
  /// it into fresh settings: the file's `game.s` entry, and the settings
  fn section_round_trip<T>(change: impl FnOnce(&mut T)) -> (Value, Settings)
  where
    T: Default + Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
  {
    let make = || {
      let mut sections = SectionRegistry::default();
      sections.register::<T>("s", None);
      Settings::new(
        EngineValues::default(),
        UserPreferenceFilter::all(),
        sections,
      )
    };
    let mut settings = make();
    change(settings.game_mut::<T>());
    let bytes = file::render(&settings.to_file(&SettingsFile::default()));
    let mut loaded = make();
    let mut warnings = Vec::new();
    loaded.apply_file(&read(&bytes, &Map::new()), &mut warnings);
    assert!(warnings.is_empty(), "{warnings:?}");
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    (json["game"]["s"].clone(), loaded)
  }

  #[test]
  fn a_setting_changed_from_its_default_is_the_only_engine_key_written() {
    let mut settings = settings(EngineValues::default());
    assert_eq!(
      written(&settings, &SettingsFile::default()),
      json!({"format_version": 3})
    );

    settings.set_volume(VolumeKind::Music, 0.3);
    assert_eq!(
      written(&settings, &SettingsFile::default()),
      json!({"format_version": 3, "music_volume": 0.3})
    );

    settings.set_window_size(800, 600);
    settings.set_display_mode(DisplayMode::BorderlessFullscreen);
    settings.set_vsync(false);
    settings.set_show_fps(true);
    assert_eq!(
      written(&settings, &SettingsFile::default()),
      json!({
        "format_version": 3,
        "music_volume": 0.3,
        "resolution": "800x600",
        "display_mode": "borderless_fullscreen",
        "vsync": false,
        "show_fps": true,
      })
    );
  }

  #[test]
  fn a_setting_set_back_to_its_default_leaves_the_file() {
    let base = read(
      br#"{"format_version": 3, "music_volume": 0.3, "vsync": false}"#,
      &Map::new(),
    );
    let mut settings = settings(EngineValues::default());
    settings.apply_file(&base, &mut Vec::new());
    assert_eq!(settings.volume(VolumeKind::Music), 0.3);
    assert!(!settings.vsync());

    settings.set_volume(VolumeKind::Music, 1.0);
    assert_eq!(
      written(&settings, &base),
      json!({"format_version": 3, "vsync": false})
    );
    settings.reset(UserPreference::Vsync);
    assert_eq!(written(&settings, &base), json!({"format_version": 3}));
  }

  #[test]
  fn a_default_changed_in_the_project_reaches_a_setting_the_player_never_set() {
    let mut first = settings(EngineValues::default());
    first.set_volume(VolumeKind::Music, 0.4);
    let bytes = file::render(&first.to_file(&SettingsFile::default()));

    let mut changed = EngineValues::default();
    changed.volumes[VolumeKind::Master.index()] = 0.6;
    changed.window_size = (1920, 1080);
    let mut later = settings(changed);
    later.apply_file(&read(&bytes, &Map::new()), &mut Vec::new());
    assert_eq!(later.volume(VolumeKind::Master), 0.6);
    assert_eq!(later.window_size(), (1920, 1080));
    assert_eq!(later.volume(VolumeKind::Music), 0.4);
  }

  #[test]
  fn volumes_are_clamped_and_nan_means_the_default() {
    let mut defaults = EngineValues::default();
    defaults.volumes[VolumeKind::Sfx.index()] = 0.7;
    let mut settings = settings(defaults);

    settings.set_volume(VolumeKind::Music, 1.5);
    assert_eq!(settings.volume(VolumeKind::Music), 1.0);
    settings.set_volume(VolumeKind::Music, -0.5);
    assert_eq!(settings.volume(VolumeKind::Music), 0.0);
    settings.set_volume(VolumeKind::Music, f32::INFINITY);
    assert_eq!(settings.volume(VolumeKind::Music), 1.0);
    settings.set_volume(VolumeKind::Sfx, 0.2);
    settings.set_volume(VolumeKind::Sfx, f32::NAN);
    assert_eq!(settings.volume(VolumeKind::Sfx), 0.7);
    settings.set_volume(VolumeKind::Ui, 0.25);
    assert_eq!(settings.volume(VolumeKind::Ui), 0.25);

    settings.set_window_size(0, 0);
    assert_eq!(settings.window_size(), (1, 1));
  }

  #[test]
  fn the_format_1_fixture_converts_its_volumes_and_keeps_env_changes() {
    let project_env = env(json!({"difficulty": "normal"}));
    let base = read(fixtures::FORMAT_1, &project_env);
    assert_eq!(base.format, 1);
    assert_eq!(base.values.volumes[VolumeKind::Master.index()], Some(0.5));
    assert_eq!(base.values.volumes[VolumeKind::Music.index()], Some(1.0));
    assert_eq!(base.values.window_size, Some((1600, 900)));
    assert_eq!(
      base.sections.get("env"),
      Some(&RawSection {
        version: 1,
        whole: false,
        value: json!({"difficulty": "hard"}),
      })
    );

    let mut settings = settings(EngineValues::default());
    settings.apply_file(&base, &mut Vec::new());
    assert_eq!(settings.volume(VolumeKind::Master), 0.5);
    assert_eq!(
      written(&settings, &base),
      json!({
        "format_version": 3,
        "master_volume": 0.5,
        "resolution": "1600x900",
        "game": {"env": {"version": 1, "value": {"difficulty": "hard"}}},
      })
    );

    // An env value the project still has is not the player's
    let same = read(fixtures::FORMAT_1, &env(json!({"difficulty": "hard"})));
    assert!(same.sections.is_empty());
  }

  #[test]
  fn the_format_2_fixture_keeps_only_values_that_differ_from_the_defaults() {
    let base = read(fixtures::FORMAT_2, &Map::new());
    assert_eq!(base.format, 2);
    assert_eq!(base.values.vsync, None, "null is unset");

    let mut settings = settings(EngineValues::default());
    settings.apply_file(&base, &mut Vec::new());
    assert_eq!(settings.volume(VolumeKind::Master), 0.8);
    assert_eq!(
      written(&settings, &base),
      json!({
        "format_version": 3,
        "master_volume": 0.8,
        "game": {"env": {"version": 1, "value": {"difficulty": "hard"}}},
      })
    );
  }

  #[test]
  fn unknown_keys_survive_a_write() {
    let base = read(
      br#"{
        "format_version": 3,
        "later": {"hdr": true},
        "master_volume": 0.5,
        "bindings": {"jump": ["Key:Space"]},
        "game": {"other": {"version": 2, "value": [1, 2]}}
      }"#,
      &Map::new(),
    );
    let mut settings = settings(EngineValues::default());
    settings.apply_file(&base, &mut Vec::new());
    settings.set_volume(VolumeKind::Ui, 0.2);

    let bytes = file::render(&settings.to_file(&base));
    let text = std::str::from_utf8(&bytes).unwrap();
    assert_eq!(
      serde_json::from_str::<Value>(text).unwrap(),
      json!({
        "format_version": 3,
        "master_volume": 0.5,
        "ui_volume": 0.2,
        "bindings": {"jump": ["Key:Space"]},
        "game": {"other": {"version": 2, "value": [1, 2]}},
        "later": {"hdr": true},
      })
    );
    let at = |key: &str| text.find(&format!("\"{key}\"")).unwrap();
    let order = [
      "format_version",
      "master_volume",
      "ui_volume",
      "bindings",
      "game",
      "later",
    ];
    assert!(
      order.windows(2).all(|pair| at(pair[0]) < at(pair[1])),
      "{text}"
    );
  }

  #[test]
  fn a_bad_value_falls_back_with_a_warning_and_the_rest_applies() {
    let text = br#"{
      "format_version": 3,
      "master_volume": "loud",
      "music_volume": 1.5,
      "sfx_volume": 0.4,
      "resolution": "huge",
      "display_mode": "tiny",
      "vsync": "yes",
      "show_fps": true
    }"#;
    let Parsed::Read { file, warnings } = file::parse(text, &Map::new()) else {
      panic!("expected Read");
    };
    assert_eq!(warnings.len(), 5, "{warnings:?}");
    assert_eq!(
      warnings[0],
      "system.json: `master_volume` has a bad value (\"loud\" is not a number from 0 to 1); using the default"
    );
    assert!(warnings[1].starts_with("system.json: `music_volume` has a bad value (1.5"));

    let mut settings = settings(EngineValues::default());
    settings.apply_file(&file, &mut Vec::new());
    assert_eq!(settings.volume(VolumeKind::Master), 1.0);
    assert_eq!(settings.volume(VolumeKind::Music), 1.0);
    assert_eq!(settings.volume(VolumeKind::Sfx), 0.4);
    assert_eq!(settings.window_size(), EngineValues::default().window_size);
    assert_eq!(settings.display_mode(), DisplayMode::Windowed);
    assert!(settings.vsync());
    assert!(settings.show_fps());
  }

  #[test]
  fn a_setting_the_game_does_not_keep_is_never_read_or_written() {
    let only_music = UserPreferenceFilter::only([UserPreference::MusicVolume]);
    let mut settings = Settings::new(
      EngineValues::default(),
      only_music,
      SectionRegistry::default(),
    );
    let base = read(
      br#"{"format_version": 3, "master_volume": 0.5, "music_volume": 0.5, "bindings": {"jump": ["Key:W"]}}"#,
      &Map::new(),
    );
    settings.apply_file(&base, &mut Vec::new());
    assert!(settings.is_kept(UserPreference::MusicVolume));
    assert!(!settings.is_kept(UserPreference::MasterVolume));
    assert_eq!(settings.volume(VolumeKind::Master), 1.0);
    assert_eq!(settings.volume(VolumeKind::Music), 0.5);

    settings.set_vsync(false);
    assert_eq!(
      written(&settings, &base),
      json!({"format_version": 3, "music_volume": 0.5})
    );
  }

  #[test]
  fn a_game_section_round_trips_only_changed_fields() {
    let mut settings = with_video(EngineValues::default());
    assert_eq!(settings.game::<Video>(), &Video::default());
    assert_eq!(
      written(&settings, &SettingsFile::default()),
      json!({"format_version": 3})
    );

    settings.game_mut::<Video>().scale = 2;
    let bytes = file::render(&settings.to_file(&SettingsFile::default()));
    assert_eq!(
      serde_json::from_slice::<Value>(&bytes).unwrap(),
      json!({"format_version": 3, "game": {"video": {"version": 1, "value": {"scale": 2}}}})
    );

    let mut loaded = with_video(EngineValues::default());
    let mut warnings = Vec::new();
    loaded.apply_file(&read(&bytes, &Map::new()), &mut warnings);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
      loaded.game::<Video>(),
      &Video {
        scale: 2,
        ..Video::default()
      }
    );
  }

  #[test]
  fn a_game_section_upgrades_with_its_steps() {
    #[derive(Default, Serialize, Deserialize)]
    #[serde(default)]
    struct TextV1 {
      subtitles: bool,
      size: String,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Text {
      subtitles: bool,
      points: u32,
    }

    impl Default for Text {
      fn default() -> Self {
        Text {
          subtitles: false,
          points: 12,
        }
      }
    }

    let mut sections = SectionRegistry::default();
    sections.register(
      "text",
      Some(SaveVersions::<Text>::new(2).step(1, |old: TextV1| Text {
        subtitles: old.subtitles,
        points: if old.size == "large" { 18 } else { 12 },
      })),
    );
    let mut settings = Settings::new(
      EngineValues::default(),
      UserPreferenceFilter::all(),
      sections,
    );
    // Version 1 kept only the fields that differed from its own default
    let base = read(
      br#"{"format_version": 3, "game": {"text": {"version": 1, "value": {"size": "large"}}}}"#,
      &Map::new(),
    );
    let mut warnings = Vec::new();
    settings.apply_file(&base, &mut warnings);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
      settings.game::<Text>(),
      &Text {
        subtitles: false,
        points: 18
      }
    );
    assert_eq!(
      written(&settings, &base),
      json!({"format_version": 3, "game": {"text": {"version": 2, "value": {"points": 18}}}})
    );
  }

  #[test]
  fn a_section_that_does_not_read_is_kept_until_it_changes() {
    let mut settings = with_video(EngineValues::default());
    let base = read(
      br#"{"format_version": 3, "game": {"video": {"version": 2, "value": {"scale": 3}}}}"#,
      &Map::new(),
    );
    let mut warnings = Vec::new();
    settings.apply_file(&base, &mut warnings);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].starts_with("system.json: `game.video` has a bad value"));
    assert_eq!(settings.game::<Video>(), &Video::default());
    assert_eq!(
      written(&settings, &base),
      json!({"format_version": 3, "game": {"video": {"version": 2, "value": {"scale": 3}}}})
    );

    settings.game_mut::<Video>().subtitles = false;
    assert_eq!(
      written(&settings, &base),
      json!({"format_version": 3, "game": {"video": {"version": 1, "value": {"subtitles": false}}}})
    );
  }

  #[test]
  fn a_section_field_skipped_when_none_keeps_the_player_s_none() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Hud {
      #[serde(skip_serializing_if = "Option::is_none")]
      limit: Option<u32>,
      scale: u32,
    }

    impl Default for Hud {
      fn default() -> Self {
        Hud {
          limit: Some(5),
          scale: 1,
        }
      }
    }

    let (entry, loaded) = section_round_trip::<Hud>(|hud| hud.limit = None);
    assert_eq!(
      loaded.game::<Hud>(),
      &Hud {
        limit: None,
        scale: 1
      }
    );
    assert_eq!(
      entry,
      json!({"version": 1, "whole": true, "value": {"scale": 1}})
    );
  }

  #[test]
  fn a_map_section_keeps_a_removed_default_key_removed() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(transparent)]
    struct Names(std::collections::BTreeMap<String, u32>);

    impl Default for Names {
      fn default() -> Self {
        Names([("a".to_string(), 1), ("b".to_string(), 2)].into())
      }
    }

    let (entry, loaded) = section_round_trip::<Names>(|names| {
      names.0.remove("a");
      names.0.insert("c".into(), 3);
    });
    assert_eq!(
      loaded.game::<Names>(),
      &Names([("b".to_string(), 2), ("c".to_string(), 3)].into())
    );
    assert_eq!(
      entry,
      json!({"version": 1, "whole": true, "value": {"b": 2, "c": 3}})
    );
  }

  #[test]
  fn an_enum_section_keeps_the_player_s_variant() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum Control {
      Mouse { sensitivity: u32 },
      Pad { dead_zone: u32 },
    }

    impl Default for Control {
      fn default() -> Self {
        Control::Mouse { sensitivity: 5 }
      }
    }

    let (entry, loaded) =
      section_round_trip::<Control>(|control| *control = Control::Pad { dead_zone: 2 });
    assert_eq!(loaded.game::<Control>(), &Control::Pad { dead_zone: 2 });
    assert_eq!(
      entry,
      json!({"version": 1, "whole": true, "value": {"Pad": {"dead_zone": 2}}})
    );
  }

  #[test]
  #[should_panic(expected = "register it with EngineBuilder::game_settings")]
  fn an_unregistered_section_panics_with_a_hint() {
    let settings = with_video(EngineValues::default());
    let _ = settings.game::<String>();
  }

  #[test]
  fn reset_all_returns_every_setting_to_the_game_s_defaults() {
    let defaults = EngineValues {
      volumes: [0.9, 0.8, 0.7, 0.6, 0.5, 0.4],
      window_size: (800, 600),
      display_mode: DisplayMode::BorderlessFullscreen,
      vsync: false,
      show_fps: true,
    };
    let mut settings = with_video(defaults);
    for kind in VolumeKind::ALL {
      settings.set_volume(kind, 0.1);
    }
    settings.set_window_size(1024, 768);
    settings.set_display_mode(DisplayMode::Windowed);
    settings.set_vsync(true);
    settings.set_show_fps(false);
    settings.game_mut::<Video>().language = "fr".into();

    settings.reset(UserPreference::MusicVolume);
    assert_eq!(settings.volume(VolumeKind::Music), 0.8);
    assert_eq!(settings.volume(VolumeKind::Master), 0.1);
    settings.reset(UserPreference::Bindings);

    settings.reset_all();
    for kind in VolumeKind::ALL {
      assert_eq!(settings.volume(kind), defaults.volumes[kind.index()]);
    }
    assert_eq!(settings.window_size(), (800, 600));
    assert_eq!(settings.display_mode(), DisplayMode::BorderlessFullscreen);
    assert!(!settings.vsync());
    assert!(settings.show_fps());
    assert_eq!(settings.game::<Video>(), &Video::default());
    assert_eq!(
      written(&settings, &SettingsFile::default()),
      json!({"format_version": 3})
    );
  }
}
