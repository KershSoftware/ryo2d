//! The project layer: the settings a project's Ryo.toml gives the engine, or
//! the values `ryo2d build` embedded from it in a release build.
//!
//! Each setting is declared once, in `project_table.rs`. This module's
//! `project_settings!` turns the table into [`ProjectLayer`] with its Ryo.toml
//! reader and its embedded values, the list of embed keys and the known
//! Ryo.toml keys; `src/build.rs` includes the same table with a macro of its
//! own that keeps only the embed keys.

use std::path::PathBuf;

use super::preferences::{DisplayMode, parse_resolution};
use super::workspace::Workspace;
use crate::core::lighting::ShadowQuality;
use crate::core::ui::file::vocab::edit_distance;

/// The key `ryo2d build` always embeds, which marks a binary it built
const RELEASE_MARKER: &str = "RYO_EMBED_RESOLUTION";

/// How far (in edits) a misspelt key may be from the known key it suggests
const SUGGESTION_DISTANCE: usize = 3;

macro_rules! project_settings {
  (
    settings {
      $(
        $field:ident : $ty:ty = $section:ident . $key:ident
        $(, embed $embed:literal)? , sample $sample:expr ;
      )*
    }
    workspace_keys {
      $( $ws_section:ident . $ws_key:ident $(embed $ws_embed:literal)? ),* $(,)?
    }
    env embed $env_embed:literal ;
  ) => {
    /// Settings from the project: `Ryo.toml` in dev, embedded values in
    /// release. `None` = not provided.
    #[derive(Debug, Clone, Default, PartialEq)]
    pub(crate) struct ProjectLayer {
      $( pub $field: Option<$ty>, )*
    }

    impl ProjectLayer {
      /// The settings a Ryo.toml sets. A value of the wrong type or text is
      /// left unset, and `warn` is told.
      pub(crate) fn from_toml(table: &toml::Table, warn: &mut dyn FnMut(String)) -> Self {
        ProjectLayer {
          $( $field: toml_value(table, stringify!($section), stringify!($key), warn), )*
        }
      }

      /// Values `ryo2d build` embedded (release builds); `None` when the
      /// binary wasn't built by `ryo2d build`. Malformed values are skipped
      /// with a warning.
      pub(crate) fn from_embedded<'a>(lookup: impl Fn(&str) -> Option<&'a str>) -> Option<Self> {
        lookup(RELEASE_MARKER)?;
        Some(ProjectLayer {
          $( $field: embedded_value(&lookup, &[$($embed)?]), )*
        })
      }

      /// The embedded values of the settings this layer sets.
      #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
      pub(crate) fn to_embedded_env(&self) -> Vec<(&'static str, String)> {
        let mut env = Vec::new();
        $($(
          if let Some(value) = &self.$field {
            env.push(($embed, value.to_embedded()));
          }
        )?)*
        env
      }

      /// The settings this layer leaves unset
      #[cfg(test)]
      pub(crate) fn unset_fields(&self) -> Vec<&'static str> {
        let mut unset = Vec::new();
        $(
          if self.$field.is_none() {
            unset.push(stringify!($field));
          }
        )*
        unset
      }
    }

    /// Every key `ryo2d build` embeds
    pub const EMBED_KEYS: &[&str] = &[
      $($($embed,)?)*
      $($($ws_embed,)?)*
      $env_embed,
    ];

    /// The values `ryo2d build` compiled into this binary, in `EMBED_KEYS` order
    const COMPILED_VALUES: &[Option<&str>] = &[
      $($(option_env!($embed),)?)*
      $($(option_env!($ws_embed),)?)*
      option_env!($env_embed),
    ];

    /// Every key of the known Ryo.toml sections, as (section, key)
    pub(crate) const KNOWN_KEYS: &[(&str, &str)] = &[
      $((stringify!($section), stringify!($key)),)*
      $((stringify!($ws_section), stringify!($ws_key)),)*
    ];

    /// One layer per entry, setting only that entry, to its sample
    #[cfg(test)]
    pub(crate) fn sample_layers() -> Vec<(&'static str, ProjectLayer)> {
      vec![$(
        (
          stringify!($field),
          ProjectLayer {
            $field: Some(sample(toml::Value::from($sample))),
            ..ProjectLayer::default()
          },
        ),
      )*]
    }

    /// Every entry set to its sample
    #[cfg(test)]
    pub(crate) fn full_sample_layer() -> ProjectLayer {
      ProjectLayer {
        $( $field: Some(sample(toml::Value::from($sample))), )*
      }
    }

    /// A Ryo.toml table that sets every entry to its sample
    #[cfg(test)]
    pub(crate) fn sample_table() -> toml::Table {
      let mut table = toml::Table::new();
      $(
        insert_sample(
          &mut table,
          stringify!($section),
          stringify!($key),
          toml::Value::from($sample),
        );
      )*
      table
    }
  };
}

include!("project_table.rs");

impl ProjectLayer {
  /// A workspace's project values (dev builds): what its Ryo.toml sets, with
  /// the title it falls back to (`[project] name`, then the folder's name),
  /// and the logo as a path from the workspace's root.
  pub(crate) fn from_workspace(workspace: &Workspace) -> Self {
    let config = workspace.config();
    ProjectLayer {
      title: Some(config.title.clone()),
      icon: config
        .release
        .logo
        .as_ref()
        .map(|logo| workspace.root().join(logo)),
      ..workspace.project().clone()
    }
  }
}

impl ProjectLayer {
  /// A workspace's project values as a release build has them (what
  /// `ryo2d build` embeds): the resolution is always set, and the icon is
  /// not a value (a release finds icon.png beside the game). The `[debug]`
  /// values are only those Ryo.toml sets.
  pub(crate) fn for_release(workspace: &Workspace) -> Self {
    let mut layer = Self::from_workspace(workspace);
    layer.icon = None;
    layer
      .resolution
      .get_or_insert(super::resolve::DEFAULT_RESOLUTION);
    layer
  }
}

/// Ryo.toml values passed to the release build; the engine reads them with
/// `option_env!`. Each setting the project sets ships as its table entry's
/// embed key (a `[debug]` value only when the project sets it: a build with
/// the tools then takes the engine's own defaults, as in dev, and one
/// without ignores them), the title as the workspace resolves it, and also:
/// - `RYO_EMBED_RESOLUTION` always (the engine's default when unset), as it
///   marks a binary `ryo2d build` made;
/// - `RYO_EMBED_PORTABLE`, `RYO_EMBED_SAVE_FOLDER` (the folder name, from
///   [`release_save_folder`](super::save_location::release_save_folder)) and
///   `RYO_EMBED_ENV` (`[env]` as JSON).
///
/// The writer whose reader is [`ProjectLayer::from_embedded`].
pub fn embedded_config_env(workspace: &Workspace) -> Vec<(&'static str, String)> {
  let config = workspace.config();
  let mut env = ProjectLayer::for_release(workspace).to_embedded_env();
  env.push(("RYO_EMBED_PORTABLE", config.release.portable.to_string()));
  env.push((
    "RYO_EMBED_SAVE_FOLDER",
    super::save_location::release_save_folder(config),
  ));
  env.push((
    "RYO_EMBED_ENV",
    serde_json::to_string(&config.env_json()).unwrap_or_else(|_| "{}".to_string()),
  ));
  env
}

/// A type a table entry can have: read from Ryo.toml, and embedded as text.
pub(crate) trait TableValue: Sized {
  /// The value a Ryo.toml gives; `None` for the wrong type or text
  fn from_toml(value: &toml::Value) -> Option<Self>;
  /// An embedded value; `None` when malformed
  fn from_embedded(text: &str) -> Option<Self>;
  /// The text `ryo2d build` embeds
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  fn to_embedded(&self) -> String;
}

impl TableValue for bool {
  fn from_toml(value: &toml::Value) -> Option<Self> {
    value.as_bool()
  }

  fn from_embedded(text: &str) -> Option<Self> {
    text.parse().ok()
  }

  fn to_embedded(&self) -> String {
    self.to_string()
  }
}

impl TableValue for f32 {
  /// A whole number is a value too (`master_volume = 1`)
  fn from_toml(value: &toml::Value) -> Option<Self> {
    match value {
      toml::Value::Float(value) => Some(*value as f32),
      toml::Value::Integer(value) => Some(*value as f32),
      _ => None,
    }
  }

  fn from_embedded(text: &str) -> Option<Self> {
    text.parse().ok()
  }

  fn to_embedded(&self) -> String {
    self.to_string()
  }
}

impl TableValue for u32 {
  fn from_toml(value: &toml::Value) -> Option<Self> {
    value
      .as_integer()
      .and_then(|value| u32::try_from(value).ok())
  }

  fn from_embedded(text: &str) -> Option<Self> {
    text.parse().ok()
  }

  fn to_embedded(&self) -> String {
    self.to_string()
  }
}

impl TableValue for String {
  fn from_toml(value: &toml::Value) -> Option<Self> {
    value.as_str().map(str::to_string)
  }

  fn from_embedded(text: &str) -> Option<Self> {
    Some(text.to_string())
  }

  fn to_embedded(&self) -> String {
    self.clone()
  }
}

impl TableValue for PathBuf {
  fn from_toml(value: &toml::Value) -> Option<Self> {
    value.as_str().map(PathBuf::from)
  }

  fn from_embedded(text: &str) -> Option<Self> {
    Some(PathBuf::from(text))
  }

  fn to_embedded(&self) -> String {
    self.to_string_lossy().into_owned()
  }
}

impl TableValue for DisplayMode {
  fn from_toml(value: &toml::Value) -> Option<Self> {
    value.as_str().and_then(DisplayMode::from_config_name)
  }

  fn from_embedded(text: &str) -> Option<Self> {
    DisplayMode::from_config_name(text)
  }

  fn to_embedded(&self) -> String {
    self.config_name().to_string()
  }
}

impl TableValue for ShadowQuality {
  fn from_toml(value: &toml::Value) -> Option<Self> {
    value.as_str().and_then(ShadowQuality::from_config_name)
  }

  fn from_embedded(text: &str) -> Option<Self> {
    ShadowQuality::from_config_name(text)
  }

  fn to_embedded(&self) -> String {
    self.config_name().to_string()
  }
}

/// A resolution, written `"WxH"`
impl TableValue for (u32, u32) {
  fn from_toml(value: &toml::Value) -> Option<Self> {
    value.as_str().and_then(parse_resolution)
  }

  fn from_embedded(text: &str) -> Option<Self> {
    parse_resolution(text)
  }

  fn to_embedded(&self) -> String {
    format!("{}x{}", self.0, self.1)
  }
}

/// An entry's value in a Ryo.toml, `None` when absent. A value of the wrong
/// type or text warns and is left unset.
pub(crate) fn toml_value<T: TableValue>(
  table: &toml::Table,
  section: &str,
  key: &str,
  warn: &mut dyn FnMut(String),
) -> Option<T> {
  let value = table.get(section)?.get(key)?;
  let read = T::from_toml(value);
  if read.is_none() {
    warn(format!(
      "Ryo.toml: [{section}] {key} = {value} is not valid, using the default"
    ));
  }
  read
}

/// An entry's embedded value, for an entry with an embed key (`embed` holds
/// it, or is empty). A malformed value warns and is left unset.
fn embedded_value<'a, T: TableValue>(
  lookup: &impl Fn(&str) -> Option<&'a str>,
  embed: &[&str],
) -> Option<T> {
  let key = embed.first()?;
  let raw = lookup(key)?;
  T::from_embedded(raw).or_else(|| {
    log::warn!("Malformed embedded value {key}='{raw}', using the default");
    None
  })
}

/// A value embedded into this binary by `ryo2d build`.
pub(crate) fn compiled_embedded_value(name: &str) -> Option<&'static str> {
  let index = EMBED_KEYS.iter().position(|key| *key == name)?;
  COMPILED_VALUES.get(index).copied().flatten()
}

/// Warn of each key in a known section (one the table lists keys of) that
/// the table doesn't list, suggesting the key of that name in another
/// section (`console` under `[system]` is `debug.console`), else the nearest known
/// key of that section. `[env]` and the game's own sections are free-form.
pub(crate) fn warn_unknown_keys(table: &toml::Table, warn: &mut dyn FnMut(String)) {
  for (section, keys) in table {
    let known = KNOWN_KEYS
      .iter()
      .filter(|(known, _)| known == section)
      .map(|(_, key)| *key);
    if known.clone().next().is_none() {
      continue;
    }
    let Some(keys) = keys.as_table() else {
      warn(format!(
        "Ryo.toml: `{section}` is not a section, ignoring it"
      ));
      continue;
    };
    for key in keys.keys() {
      if known.clone().any(|known| known == key) {
        continue;
      }
      let moved = KNOWN_KEYS
        .iter()
        .find(|(other, name)| other != section && name == key)
        .map(|(other, name)| format!("{other}.{name}"));
      let guess = moved
        .or_else(|| nearest_key(key, known.clone()).map(str::to_string))
        .map(|nearest| format!(" (did you mean `{nearest}`?)"))
        .unwrap_or_default();
      warn(format!(
        "Ryo.toml: unknown key `{key}` in [{section}]{guess}"
      ));
    }
  }
}

/// The known key nearest to `key`, if it is within `SUGGESTION_DISTANCE`
/// edits (the first wins a tie)
fn nearest_key<'a>(key: &str, known: impl Iterator<Item = &'a str>) -> Option<&'a str> {
  let key: Vec<char> = key.chars().collect();
  known
    .map(|candidate| {
      let candidate_chars: Vec<char> = candidate.chars().collect();
      (edit_distance(&key, &candidate_chars), candidate)
    })
    .filter(|(distance, _)| *distance <= SUGGESTION_DISTANCE)
    .min_by_key(|(distance, _)| *distance)
    .map(|(_, candidate)| candidate)
}

/// A sample as its entry's type
#[cfg(test)]
fn sample<T: TableValue>(value: toml::Value) -> T {
  T::from_toml(&value)
    .unwrap_or_else(|| panic!("the sample {value} doesn't read as its entry's type"))
}

#[cfg(test)]
fn insert_sample(table: &mut toml::Table, section: &str, key: &str, value: toml::Value) {
  let section = table
    .entry(section)
    .or_insert_with(|| toml::Value::Table(toml::Table::new()));
  if let toml::Value::Table(section) = section {
    section.insert(key.to_string(), value);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn lookup_in<'a>(env: &'a [(&'static str, String)]) -> impl Fn(&str) -> Option<&'a str> + 'a {
    move |name| {
      env
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.as_str())
    }
  }

  #[test]
  fn every_embed_key_round_trips() {
    let every = full_sample_layer();
    let env = every.to_embedded_env();
    let back = ProjectLayer::from_embedded(lookup_in(&env));
    // The release logo is never embedded: a shipped game finds icon.png beside it
    assert_eq!(
      back,
      Some(ProjectLayer {
        icon: None,
        ..every
      })
    );
  }

  #[test]
  fn a_ryo_toml_with_every_key_sets_every_field() {
    let text = toml::to_string(&sample_table()).unwrap();
    let table: toml::Table = text.parse().unwrap();
    let mut warnings = Vec::new();

    let layer = ProjectLayer::from_toml(&table, &mut |warning| warnings.push(warning));

    assert_eq!(layer.unset_fields(), Vec::<&str>::new());
    assert_eq!(layer, full_sample_layer());
    assert_eq!(warnings, Vec::<String>::new());
  }

  #[test]
  fn a_value_of_the_wrong_type_or_text_warns_and_is_left_unset() {
    let table: toml::Table = "[system]\nvsync = \"yes\"\nresolution = \"huge\"\n\
                              display_mode = \"sideways\"\nmaster_volume = 1\n"
      .parse()
      .unwrap();
    let mut warnings = Vec::new();

    let layer = ProjectLayer::from_toml(&table, &mut |warning| warnings.push(warning));

    assert_eq!(layer.vsync, None);
    assert_eq!(layer.resolution, None);
    assert_eq!(layer.display_mode, None);
    assert_eq!(layer.master_volume, Some(1.0), "a whole volume is a volume");
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(
      warnings
        .iter()
        .any(|w| w == "Ryo.toml: [system] resolution = \"huge\" is not valid, using the default"),
      "{warnings:?}"
    );
  }

  fn embedded(name: &str) -> Option<&'static str> {
    match name {
      "RYO_EMBED_RESOLUTION" => Some("800x600"),
      "RYO_EMBED_PIXEL_ART" => Some("true"),
      "RYO_EMBED_VSYNC" => Some("false"),
      "RYO_EMBED_DISPLAY_MODE" => Some("exclusive_fullscreen"),
      "RYO_EMBED_MASTER_VOLUME" => Some("0.3"),
      "RYO_EMBED_MUSIC_VOLUME" => Some("0.4"),
      "RYO_EMBED_SFX_VOLUME" => Some("0.5"),
      "RYO_EMBED_UI_VOLUME" => Some("0.6"),
      "RYO_EMBED_VOICE_VOLUME" => Some("0.7"),
      "RYO_EMBED_AMBIENCE_VOLUME" => Some("0.8"),
      "RYO_EMBED_AUDIO_BUFFER_FRAMES" => Some("256"),
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
      "RYO_EMBED_TILED_PROJECT" => Some("maps/game.tiled-project"),
      _ => None,
    }
  }

  #[test]
  fn from_embedded_reads_every_key() {
    let layer = ProjectLayer::from_embedded(embedded).unwrap();
    assert_eq!(layer.title.as_deref(), Some("Shipped"));
    assert_eq!(layer.resolution, Some((800, 600)));
    assert_eq!(layer.pixel_art, Some(true));
    assert_eq!(layer.vsync, Some(false));
    assert_eq!(layer.display_mode, Some(DisplayMode::ExclusiveFullscreen));
    assert_eq!(layer.master_volume, Some(0.3));
    assert_eq!(layer.music_volume, Some(0.4));
    assert_eq!(layer.sfx_volume, Some(0.5));
    assert_eq!(layer.ui_volume, Some(0.6));
    assert_eq!(layer.voice_volume, Some(0.7));
    assert_eq!(layer.ambience_volume, Some(0.8));
    assert_eq!(layer.audio_buffer_frames, Some(256));
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
    assert_eq!(
      layer.tiled_project.as_deref(),
      Some("maps/game.tiled-project")
    );
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
      "RYO_EMBED_DISPLAY_MODE" => Some("sideways"),
      other => embedded(other),
    })
    .unwrap();
    assert_eq!(layer.master_volume, None);
    assert_eq!(layer.resolution, None);
    assert_eq!(layer.display_mode, None);
    assert_eq!(layer.music_volume, Some(0.4));
    assert_eq!(layer.disable_debug_keys, Some(true));
  }

  #[test]
  fn a_dev_workspace_fills_in_the_title_and_the_logo_path() {
    let ws = Workspace::from_toml_str(
      "[project]\nname = \"Hopper\"\n\n[release]\nlogo = \"art/logo.png\"\n",
      PathBuf::from("game"),
    )
    .unwrap();

    let layer = ProjectLayer::from_workspace(&ws);

    assert_eq!(layer.title.as_deref(), Some("Hopper"));
    assert_eq!(layer.icon, Some(PathBuf::from("game/art/logo.png")));
    assert_eq!(layer.dev_console, None, "the build's own default");
    assert_eq!(layer.dev_window_keys, None, "the build's own default");
    assert_eq!(layer.vsync, None);
  }

  #[test]
  fn the_embed_keys_are_unique() {
    let unique: std::collections::HashSet<&str> = EMBED_KEYS.iter().copied().collect();
    assert_eq!(EMBED_KEYS.len(), 29);
    assert_eq!(unique.len(), EMBED_KEYS.len());
    assert!(EMBED_KEYS.iter().all(|key| key.starts_with("RYO_EMBED_")));
    assert_eq!(compiled_embedded_value("RYO_EMBED_NOT_A_KEY"), None);
  }

  #[test]
  fn the_known_keys_are_unique() {
    let unique: std::collections::HashSet<&(&str, &str)> = KNOWN_KEYS.iter().collect();
    assert_eq!(unique.len(), KNOWN_KEYS.len());
  }

  /// A workspace from Ryo.toml text, with no folder on disk
  fn workspace(toml: &str) -> Workspace {
    Workspace::from_toml_str(toml, PathBuf::from("game")).unwrap()
  }

  fn env_value(env: &[(&'static str, String)], key: &str) -> Option<String> {
    env
      .iter()
      .find(|(name, _)| *name == key)
      .map(|(_, value)| value.clone())
  }

  /// Resolve a Ryo.toml as the web does (the compiled-in file) and as a release
  /// of it does (the values `ryo2d build` embeds)
  fn web_and_release_resolve(text: &'static str) {
    use crate::core::config::ProjectFile;
    use crate::core::config::resolve::{BuilderOverrides, resolve_settings};

    let env = embedded_config_env(&workspace(text));
    let release = ProjectLayer::from_embedded(lookup_in(&env)).unwrap();
    let web = ProjectFile::from_toml(text).read().unwrap().layer;
    assert_eq!(
      resolve_settings(&web, &BuilderOverrides::default()),
      resolve_settings(&release, &BuilderOverrides::default()),
    );
  }

  #[test]
  fn a_web_style_resolve_matches_the_release_resolve_of_the_same_ryo_toml() {
    web_and_release_resolve(include_str!("../../../tests/fixtures/project/Ryo.toml"));
    // Nothing set but the name: the unset values resolve alike too
    web_and_release_resolve("[project]\nname = \"Bare\"\n");
    web_and_release_resolve(
      "[project]\nname = \"Titled\"\ntitle = \"The Title\"\n[debug]\nconsole = true\ndev_window_keys = true\n",
    );
  }

  #[test]
  fn save_folder_defaults_to_the_project_name() {
    let env = embedded_config_env(&workspace("[project]\nname = \"Star Hopper\"\n"));
    assert_eq!(
      env_value(&env, "RYO_EMBED_SAVE_FOLDER").as_deref(),
      Some("Star Hopper")
    );
    assert_eq!(
      env_value(&env, "RYO_EMBED_PORTABLE").as_deref(),
      Some("false")
    );
  }

  #[test]
  fn save_folder_override_and_portable_are_embedded() {
    let env = embedded_config_env(&workspace(
      "[release]\nsave_folder = \"Studio/Hopper\"\nportable = true\n",
    ));
    assert_eq!(
      env_value(&env, "RYO_EMBED_SAVE_FOLDER").as_deref(),
      Some("Studio/Hopper")
    );
    assert_eq!(
      env_value(&env, "RYO_EMBED_PORTABLE").as_deref(),
      Some("true")
    );
  }

  #[test]
  fn embedded_env_covers_every_key_the_engine_reads() {
    let every = toml::to_string(&sample_table()).unwrap();
    let mut names: Vec<&str> = embedded_config_env(&workspace(&every))
      .into_iter()
      .map(|(name, _)| name)
      .collect();
    let mut expected: Vec<&str> = EMBED_KEYS.to_vec();
    names.sort_unstable();
    expected.sort_unstable();
    assert_eq!(names, expected);
  }

  #[test]
  fn a_project_that_sets_nothing_still_ships_the_release_marker_and_the_fixed_keys() {
    let env = embedded_config_env(&workspace(""));
    let value = |key| env_value(&env, key);
    assert_eq!(value("RYO_EMBED_RESOLUTION").as_deref(), Some("1280x720"));
    assert_eq!(value("RYO_EMBED_TITLE").as_deref(), Some("game"));
    assert_eq!(value("RYO_EMBED_ENV").as_deref(), Some("{}"));
    assert_eq!(value("RYO_EMBED_SAVE_FOLDER").as_deref(), Some("game"));
  }

  #[test]
  fn embedded_display_title_and_volumes_round_trip() {
    let ws = workspace(
      "[project]\ntitle = \"Hopper\"\n\n[system]\ndisplay_mode = \"borderless_fullscreen\"\n\
       resolution = \"800x600\"\nmaster_volume = 0.3\nmusic_volume = 0.4\nsfx_volume = 0.5\n\
       ui_volume = 0.6\nvoice_volume = 0.7\nambience_volume = 0.8\naudio_buffer_frames = 256\n\n\
       [tilemap]\ntiled_project = \"maps/game.tiled-project\"\n",
    );

    let env = embedded_config_env(&ws);
    let layer = ProjectLayer::from_embedded(lookup_in(&env)).unwrap();

    assert_eq!(layer.display_mode, Some(DisplayMode::BorderlessFullscreen));
    assert_eq!(layer.resolution, Some((800, 600)));
    assert_eq!(layer.title.as_deref(), Some("Hopper"));
    assert_eq!(layer.master_volume, Some(0.3));
    assert_eq!(layer.music_volume, Some(0.4));
    assert_eq!(layer.sfx_volume, Some(0.5));
    assert_eq!(layer.ui_volume, Some(0.6));
    assert_eq!(layer.voice_volume, Some(0.7));
    assert_eq!(layer.ambience_volume, Some(0.8));
    assert_eq!(layer.audio_buffer_frames, Some(256));
    assert_eq!(
      layer.tiled_project.as_deref(),
      Some("maps/game.tiled-project")
    );
    // Unset, the device's own buffer and the default Tiled project ship as nothing
    let env = embedded_config_env(&workspace("[project]\ntitle = \"Hopper\"\n"));
    let workspace = Workspace::from_embedded(std::path::Path::new("."), lookup_in(&env));
    assert_eq!(
      ProjectLayer::from_embedded(lookup_in(&env)).map(|layer| layer.audio_buffer_frames),
      Some(None)
    );
    assert_eq!(
      workspace.map(|ws| ws.config().tiled_project.clone()),
      Some(None)
    );
    assert_eq!(
      ProjectLayer::from_embedded(lookup_in(&env)).map(|layer| layer.tiled_project),
      Some(None)
    );
  }

  #[test]
  fn embedded_debug_and_background_settings_reach_the_engine() {
    let ws = workspace(
      "[debug]\ndisable_debug_keys = true\nenable_profiler = true\ndev_window_keys = true\n\
       logger_window = true\n\n[system]\ntarget_background_fps = 15\nbackground_rendering = false\n",
    );

    let env = embedded_config_env(&ws);
    let layer = ProjectLayer::from_embedded(lookup_in(&env)).unwrap();

    assert_eq!(layer.disable_debug_keys, Some(true));
    assert_eq!(layer.enable_profiler, Some(true));
    assert_eq!(layer.dev_window_keys, Some(true));
    assert_eq!(layer.logger_window, Some(true));
    assert_eq!(layer.background_fps, Some(15));
    assert_eq!(layer.background_rendering, Some(false));
  }

  #[test]
  fn a_release_build_embeds_only_what_the_project_set() {
    let env = embedded_config_env(&workspace("[project]\nname = \"g\"\n"));
    for key in [
      "RYO_EMBED_CONSOLE",
      "RYO_EMBED_DEV_WINDOW_KEYS",
      "RYO_EMBED_LOGGER_WINDOW",
      "RYO_EMBED_ENABLE_PROFILER",
      "RYO_EMBED_SHOW_COLLIDERS",
    ] {
      assert_eq!(env_value(&env, key), None, "{key} was not set");
    }
    let env = embedded_config_env(&workspace(
      "[project]\nname = \"g\"\n\n[debug]\nconsole = false\nlogger_window = true\n",
    ));
    let value = |key| env_value(&env, key);
    assert_eq!(value("RYO_EMBED_CONSOLE").as_deref(), Some("false"));
    assert_eq!(value("RYO_EMBED_LOGGER_WINDOW").as_deref(), Some("true"));
    assert_eq!(value("RYO_EMBED_DEV_WINDOW_KEYS"), None);
  }

  #[test]
  fn env_section_is_embedded_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
      dir.path().join("Ryo.toml"),
      "[env]\ndifficulty = \"hard\"\nlives = 3\nhard_mode = true\nspeed = 1.5\n",
    )
    .unwrap();
    let workspace = Workspace::from_path(dir.path()).unwrap();
    let env = embedded_config_env(&workspace);

    let shipped = Workspace::from_embedded(dir.path(), lookup_in(&env)).unwrap();

    let config = shipped.config();
    assert_eq!(config.env_str("difficulty"), Some("hard"));
    assert_eq!(config.env_int("lives"), Some(3));
    assert_eq!(config.env_bool("hard_mode"), Some(true));
    assert_eq!(config.env_float("speed"), Some(1.5));
  }

  #[test]
  fn the_build_script_reads_the_project_table() {
    let script = include_str!("../../build.rs");
    assert!(script.contains("include!(\"core/config/project_table.rs\")"));
  }
}
