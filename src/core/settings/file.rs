//! `system.json`, the settings file: format 3, and the conversions from the
//! formats before it.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::core::config::DisplayMode;

/// The format this engine writes
pub(crate) const FORMAT: u32 = 3;
/// The settings file's name in the save storage
pub(crate) const FILE_NAME: &str = "system.json";
/// What a damaged settings file is renamed to, replacing an older one
pub(crate) const DAMAGED_NAME: &str = "system.json.damaged";

/// The storage's check of a copy of the settings file: whole when it is a
/// JSON object, as [`parse`] reads it. No copy is newer than another
pub(crate) fn verify(bytes: &[u8]) -> Option<u64> {
  matches!(serde_json::from_slice::<Value>(bytes), Ok(Value::Object(_))).then_some(0)
}

/// The values a file holds for the engine's settings: `None` for a key it
/// lacks or whose value was bad
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FileValues {
  /// In `VolumeKind::ALL` order
  pub(crate) volumes: [Option<f32>; 6],
  pub(crate) window_size: Option<(u32, u32)>,
  pub(crate) display_mode: Option<DisplayMode>,
  pub(crate) vsync: Option<bool>,
  pub(crate) show_fps: Option<bool>,
}

/// A game section as the file stores it
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawSection {
  pub(crate) version: u32,
  /// Whether `value` is the whole value rather than the fields that differ
  /// from the default's
  pub(crate) whole: bool,
  pub(crate) value: Value,
}

/// A settings file, read or to write
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SettingsFile {
  /// The format it was written in
  pub(crate) format: u32,
  pub(crate) values: FileValues,
  /// The player's changed bindings: action name to binding texts
  pub(crate) bindings: Option<BTreeMap<String, Vec<String>>>,
  /// The game sections, by key
  pub(crate) sections: BTreeMap<String, RawSection>,
  /// Keys this engine does not know, kept as they are
  pub(crate) unknown: Map<String, Value>,
}

impl Default for SettingsFile {
  fn default() -> Self {
    SettingsFile {
      format: FORMAT,
      values: FileValues::default(),
      bindings: None,
      sections: BTreeMap::new(),
      unknown: Map::new(),
    }
  }
}

/// What reading a settings file found
#[derive(Debug)]
pub(crate) enum Parsed {
  /// A file of this format or an older one
  Read {
    file: SettingsFile,
    warnings: Vec<String>,
  },
  /// A file of a newer format: the keys this engine knows are read
  Newer {
    file: SettingsFile,
    warnings: Vec<String>,
  },
  /// Not a JSON object, and why
  Damaged(String),
}

/// Reads a settings file of any format. `project_env` is the project's
/// `[env]`, which an older file's `env` values are compared with.
pub(crate) fn parse(bytes: &[u8], project_env: &Map<String, Value>) -> Parsed {
  let mut object = match serde_json::from_slice::<Value>(bytes) {
    Ok(Value::Object(object)) => object,
    Ok(_) => return Parsed::Damaged("it is not a JSON object".to_string()),
    Err(e) => return Parsed::Damaged(e.to_string()),
  };
  let mut warnings = Vec::new();
  let w = &mut warnings;
  // No format, or one below 2, is format 1
  let format = take(&mut object, "", FORMAT_KEY, w, |value| {
    value
      .as_u64()
      .map(|format| u32::try_from(format).unwrap_or(u32::MAX))
      .ok_or_else(|| format!("{} is not a format number", shown(value)))
  })
  .map_or(1, |format| format.max(1));

  let mut values = FileValues::default();
  for (volume, key) in values.volumes.iter_mut().zip(VOLUME_KEYS) {
    *volume = take(&mut object, "", key, w, |value| read_volume(value, format));
  }
  let mut size = take(&mut object, "", "resolution", w, read_size);
  values.display_mode = take(&mut object, "", "display_mode", w, read_display_mode);
  values.vsync = take(&mut object, "", "vsync", w, read_bool);
  values.show_fps = take(&mut object, "", "show_fps", w, read_bool);
  let bindings = match object.remove("bindings") {
    None | Some(Value::Null) => None,
    Some(value) => read_bindings(value, w),
  };
  let mut sections = match object.remove("game") {
    None | Some(Value::Null) => BTreeMap::new(),
    Some(value) => read_sections(value, w),
  };

  if format < FORMAT {
    let width = take(&mut object, "", "width", w, read_pixels);
    let height = take(&mut object, "", "height", w, read_pixels);
    let mut display = take(&mut object, "", "display", w, read_object).unwrap_or_default();
    let display_size = take(&mut display, "display.", "resolution", w, read_size);
    let display_width = take(&mut display, "display.", "width", w, read_pixels);
    let display_height = take(&mut display, "display.", "height", w, read_pixels);
    size = size
      .or(width.zip(height))
      .or(display_size)
      .or(display_width.zip(display_height));
    let display_mode = take(
      &mut display,
      "display.",
      "display_mode",
      w,
      read_display_mode,
    );
    values.display_mode = values.display_mode.or(display_mode);
    let vsync = take(&mut display, "display.", "vsync", w, read_bool);
    values.vsync = values.vsync.or(vsync);

    // The player's `env` values: those that differ from the project's
    let env = take(&mut object, "", "env", w, read_object).unwrap_or_default();
    let changed: Map<String, Value> = env
      .into_iter()
      .filter(|(key, value)| project_env.get(key) != Some(value))
      .collect();
    if !changed.is_empty() {
      sections.insert(
        ENV_SECTION.to_string(),
        RawSection {
          version: 1,
          whole: false,
          value: Value::Object(changed),
        },
      );
    }
  }
  values.window_size = size;

  let file = SettingsFile {
    format,
    values,
    bindings,
    sections,
    unknown: object,
  };
  if format > FORMAT {
    warnings.insert(
      0,
      format!(
        "system.json is format {format}, newer than this engine's {FORMAT}: the settings it knows are read, and the file is left as it is"
      ),
    );
    Parsed::Newer { file, warnings }
  } else {
    Parsed::Read { file, warnings }
  }
}

/// `file` as format 3: pretty JSON ending in a line feed, with its keys in
/// order: the format, the engine's settings, the bindings, the game sections
/// and the keys this engine does not know
#[cfg_attr(not(test), expect(dead_code, reason = "the settings writer's"))]
pub(crate) fn render(file: &SettingsFile) -> Vec<u8> {
  let values = &file.values;
  let mut entries = vec![(FORMAT_KEY.to_string(), FORMAT.to_string())];
  let mut add = |key: &str, value: Value| entries.push((key.to_string(), pretty(&value, 1)));
  for (volume, key) in values.volumes.iter().zip(VOLUME_KEYS) {
    if let Some(volume) = volume {
      add(key, Value::from(two_decimals(*volume)));
    }
  }
  if let Some((width, height)) = values.window_size {
    add("resolution", Value::from(format!("{width}x{height}")));
  }
  if let Some(mode) = values.display_mode {
    add("display_mode", Value::from(mode.config_name()));
  }
  if let Some(vsync) = values.vsync {
    add("vsync", Value::from(vsync));
  }
  if let Some(show_fps) = values.show_fps {
    add("show_fps", Value::from(show_fps));
  }
  if let Some(bindings) = file
    .bindings
    .as_ref()
    .filter(|bindings| !bindings.is_empty())
  {
    let bindings = bindings
      .iter()
      .map(|(action, texts)| (action.clone(), Value::from(texts.clone())))
      .collect();
    add("bindings", Value::Object(bindings));
  }
  if !file.sections.is_empty() {
    let sections = file
      .sections
      .iter()
      .map(|(key, section)| {
        let mut fields = vec![("version".to_string(), section.version.to_string())];
        if section.whole {
          fields.push(("whole".to_string(), "true".to_string()));
        }
        fields.push(("value".to_string(), pretty(&section.value, 3)));
        (key.clone(), object(fields, 2))
      })
      .collect();
    entries.push(("game".to_string(), object(sections, 1)));
  }
  for (key, value) in &file.unknown {
    entries.push((key.clone(), pretty(value, 1)));
  }
  let mut text = object(entries, 0);
  text.push('\n');
  text.into_bytes()
}

/// The warning for a key whose value is bad, and why
pub(crate) fn bad_value(key: &str, detail: &str) -> String {
  format!("system.json: `{key}` has a bad value ({detail}); using the default")
}

/// `value` rounded to two decimals, as the file keeps volumes
pub(crate) fn two_decimals(value: f32) -> f64 {
  (f64::from(value) * 100.0).round() / 100.0
}

const FORMAT_KEY: &str = "format_version";

/// The volumes' keys, in `VolumeKind::ALL` order
const VOLUME_KEYS: [&str; 6] = [
  "master_volume",
  "music_volume",
  "sfx_volume",
  "ui_volume",
  "voice_volume",
  "ambience_volume",
];

/// The section an older file's `env` values become
const ENV_SECTION: &str = "env";

/// Takes `key` out of `object` and reads it: `None` when it is absent or
/// `null`, and when its value is bad, with a warning naming `{prefix}{key}`
fn take<T>(
  object: &mut Map<String, Value>,
  prefix: &str,
  key: &str,
  warnings: &mut Vec<String>,
  read: impl FnOnce(&Value) -> Result<T, String>,
) -> Option<T> {
  let value = object.remove(key)?;
  if value.is_null() {
    return None;
  }
  read(&value)
    .map_err(|detail| warnings.push(bad_value(&format!("{prefix}{key}"), &detail)))
    .ok()
}

/// A volume from 0 to 1. Format 1 kept amplitudes, heard at `20·log10(v)`
/// dB: each becomes the setting heard at the same dB on today's curve, √v,
/// to two decimals.
fn read_volume(value: &Value, format: u32) -> Result<f32, String> {
  match value.as_f64() {
    Some(volume) if (0.0..=1.0).contains(&volume) => {
      let volume = volume as f32;
      Ok(if format < 2 {
        two_decimals(volume.sqrt()) as f32
      } else {
        volume
      })
    }
    _ => Err(format!("{} is not a number from 0 to 1", shown(value))),
  }
}

fn read_size(value: &Value) -> Result<(u32, u32), String> {
  value
    .as_str()
    .and_then(parse_size)
    .ok_or_else(|| format!("{} is not a size such as \"1280x720\"", shown(value)))
}

/// A `"WxH"` size, each side at least 1
fn parse_size(text: &str) -> Option<(u32, u32)> {
  let (width, height) = text.split_once('x')?;
  let width: u32 = width.trim().parse().ok()?;
  let height: u32 = height.trim().parse().ok()?;
  (width > 0 && height > 0).then_some((width, height))
}

fn read_pixels(value: &Value) -> Result<u32, String> {
  value
    .as_u64()
    .and_then(|pixels| u32::try_from(pixels).ok())
    .filter(|&pixels| pixels > 0)
    .ok_or_else(|| format!("{} is not a size in pixels", shown(value)))
}

fn read_display_mode(value: &Value) -> Result<DisplayMode, String> {
  value
    .as_str()
    .and_then(DisplayMode::from_config_name)
    .ok_or_else(|| {
      format!(
        "{} is not windowed, exclusive_fullscreen or borderless_fullscreen",
        shown(value)
      )
    })
}

fn read_bool(value: &Value) -> Result<bool, String> {
  value
    .as_bool()
    .ok_or_else(|| format!("{} is not true or false", shown(value)))
}

fn read_object(value: &Value) -> Result<Map<String, Value>, String> {
  value
    .as_object()
    .cloned()
    .ok_or_else(|| format!("{} is not an object", shown(value)))
}

/// The bindings: each action's binding texts. An action whose list is bad is
/// left out, with a warning.
fn read_bindings(
  value: Value,
  warnings: &mut Vec<String>,
) -> Option<BTreeMap<String, Vec<String>>> {
  let Value::Object(actions) = value else {
    warnings.push(bad_value(
      "bindings",
      &format!("{} is not an object of actions", shown(&value)),
    ));
    return None;
  };
  let mut bindings = BTreeMap::new();
  for (action, list) in actions {
    let texts: Option<Vec<String>> = list.as_array().and_then(|items| {
      items
        .iter()
        .map(|item| item.as_str().map(str::to_string))
        .collect()
    });
    match texts {
      Some(texts) => {
        bindings.insert(action, texts);
      }
      None => warnings.push(bad_value(
        &format!("bindings.{action}"),
        &format!("{} is not a list of bindings", shown(&list)),
      )),
    }
  }
  Some(bindings)
}

/// The game sections, each `{"version": N, "value": ...}` with an optional
/// `"whole": true`. A section of another shape is left out, with a warning.
fn read_sections(value: Value, warnings: &mut Vec<String>) -> BTreeMap<String, RawSection> {
  let Value::Object(entries) = value else {
    warnings.push(bad_value(
      "game",
      &format!("{} is not an object of sections", shown(&value)),
    ));
    return BTreeMap::new();
  };
  let mut sections = BTreeMap::new();
  for (key, entry) in entries {
    let version = entry.get("version").and_then(Value::as_u64);
    let whole = entry.get("whole").map_or(Some(false), Value::as_bool);
    match (version, whole, entry.get("value")) {
      (Some(version), Some(whole), Some(value)) => {
        let section = RawSection {
          version: u32::try_from(version).unwrap_or(u32::MAX),
          whole,
          value: value.clone(),
        };
        sections.insert(key, section);
      }
      _ => warnings.push(bad_value(
        &format!("game.{key}"),
        &format!(
          "{} is not {{\"version\": N, \"value\": ...}}",
          shown(&entry)
        ),
      )),
    }
  }
  sections
}

/// `value` as compact JSON for a warning, cut short when long
fn shown(value: &Value) -> String {
  const LONGEST: usize = 40;
  let text = value.to_string();
  if text.chars().count() <= LONGEST {
    text
  } else {
    format!("{}...", text.chars().take(LONGEST).collect::<String>())
  }
}

/// A JSON object of `entries` in their order, each value already written for
/// `depth + 1`
fn object(entries: Vec<(String, String)>, depth: usize) -> String {
  if entries.is_empty() {
    return "{}".to_string();
  }
  let indent = "  ".repeat(depth + 1);
  let fields: Vec<String> = entries
    .into_iter()
    .map(|(key, value)| format!("{indent}{}: {value}", Value::String(key)))
    .collect();
  format!("{{\n{}\n{}}}", fields.join(",\n"), "  ".repeat(depth))
}

/// `value` as pretty JSON, its lines after the first indented for `depth`
fn pretty(value: &Value, depth: usize) -> String {
  format!("{value:#}").replace('\n', &format!("\n{}", "  ".repeat(depth)))
}

#[cfg(test)]
pub(crate) mod fixtures {
  /// `system.json` at format 1 and 2, written by the engines before (see
  /// `tests/fixtures/saves/README.md`)
  pub(crate) const FORMAT_1: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/saves/system-format1.json"
  ));
  pub(crate) const FORMAT_2: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/saves/system-format2.json"
  ));
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;

  fn read(text: &str) -> (SettingsFile, Vec<String>) {
    match parse(text.as_bytes(), &Map::new()) {
      Parsed::Read { file, warnings } => (file, warnings),
      other => panic!("expected Read, got {other:?}"),
    }
  }

  #[test]
  fn a_newer_format_is_read_and_reported_newer() {
    let text = r#"{"format_version": 4, "master_volume": 0.5, "hdr": true}"#;
    let Parsed::Newer { file, warnings } = parse(text.as_bytes(), &Map::new()) else {
      panic!("expected Newer");
    };
    assert_eq!(file.format, 4);
    assert_eq!(file.values.volumes[0], Some(0.5));
    assert_eq!(file.unknown.get("hdr"), Some(&json!(true)));
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("format 4"), "{warnings:?}");
  }

  #[test]
  fn a_file_that_is_not_json_is_damaged() {
    for bytes in [&b"{ not json"[..], b"", b"[1, 2]", b"\"text\"", b"\xff\xfe"] {
      match parse(bytes, &Map::new()) {
        Parsed::Damaged(reason) => assert!(!reason.is_empty()),
        other => panic!("{bytes:?} gave {other:?}"),
      }
    }
    let Parsed::Damaged(reason) = parse(b"[1, 2]", &Map::new()) else {
      unreachable!()
    };
    assert_eq!(reason, "it is not a JSON object");
  }

  #[test]
  fn older_files_read_the_legacy_display_and_size_forms() {
    let (file, warnings) = read(
      r#"{"format_version": 2, "display": {"resolution": "800x600", "display_mode": "borderless_fullscreen", "vsync": false}}"#,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(file.values.window_size, Some((800, 600)));
    assert_eq!(
      file.values.display_mode,
      Some(DisplayMode::BorderlessFullscreen)
    );
    assert_eq!(file.values.vsync, Some(false));
    assert!(file.unknown.is_empty(), "{:?}", file.unknown);

    let (file, _) = read(r#"{"width": 1024, "height": 768, "display": {"width": 1, "height": 2}}"#);
    assert_eq!(file.values.window_size, Some((1024, 768)));
    let (file, _) = read(r#"{"display": {"width": 640, "height": 480}}"#);
    assert_eq!(file.values.window_size, Some((640, 480)));
    let (file, _) = read(r#"{"resolution": "1920x1080", "width": 1024, "height": 768}"#);
    assert_eq!(file.values.window_size, Some((1920, 1080)));

    // A format 3 file keeps them as keys it does not know
    let (file, _) = read(r#"{"format_version": 3, "width": 1024, "height": 768}"#);
    assert_eq!(file.values.window_size, None);
    assert_eq!(file.unknown.len(), 2);
  }

  #[test]
  fn a_rendered_file_reads_back_the_same() {
    let mut file = SettingsFile::default();
    file.values.volumes = [Some(0.25), None, Some(0.0), None, None, Some(1.0)];
    file.values.window_size = Some((1600, 900));
    file.values.display_mode = Some(DisplayMode::ExclusiveFullscreen);
    file.values.vsync = Some(false);
    file.values.show_fps = Some(true);
    file.bindings = Some(BTreeMap::from([(
      "jump".to_string(),
      vec!["Key:Space".to_string(), "Pad:South".to_string()],
    )]));
    file.sections.insert(
      "video".into(),
      RawSection {
        version: 2,
        whole: false,
        value: json!({"scale": 2}),
      },
    );
    file.sections.insert(
      "names".into(),
      RawSection {
        version: 1,
        whole: true,
        value: json!({}),
      },
    );
    file.unknown.insert("later".into(), json!({"x": [1, 2]}));

    let bytes = render(&file);
    assert!(bytes.ends_with(b"}\n"));
    let (back, warnings) = read(std::str::from_utf8(&bytes).unwrap());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(back, file);
  }
}
