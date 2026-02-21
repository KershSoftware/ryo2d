//! Integration tests for resolution config format and priority chain.
//!
//! Tests the "WxH" string resolution format across Ryo.toml parsing,
//! system.json save/load, and the config priority chain.

use ryo2d::prelude::*;

// ==========================================================================
// Resolution parsing integration
// ==========================================================================

#[test]
fn parse_resolution_standard_formats() {
  assert_eq!(parse_resolution("1280x720"), Some((1280, 720)));
  assert_eq!(parse_resolution("800x600"), Some((800, 600)));
  assert_eq!(parse_resolution("1920x1080"), Some((1920, 1080)));
  assert_eq!(parse_resolution("3840x2160"), Some((3840, 2160)));
}

#[test]
fn parse_resolution_rejects_invalid() {
  assert_eq!(parse_resolution(""), None);
  assert_eq!(parse_resolution("1280"), None);
  assert_eq!(parse_resolution("axb"), None);
  assert_eq!(parse_resolution("1280x720x60"), None);
}

// ==========================================================================
// SystemState serialization format (flat, with "WxH" resolution string)
// ==========================================================================

#[test]
fn system_state_serializes_resolution_as_string() {
  let state = SystemState {
    width: 1920,
    height: 1080,
    display_mode: DisplayMode::Windowed,
    ..SystemState::default()
  };

  let json = serde_json::to_value(&state).unwrap();
  assert_eq!(json["resolution"], "1920x1080");
  assert!(json.get("width").is_none());
  assert!(json.get("height").is_none());
}

#[test]
fn system_state_deserializes_resolution_string() {
  let json = serde_json::json!({
    "master_volume": 1.0,
    "music_volume": 1.0,
    "sfx_volume": 1.0,
    "resolution": "2560x1440",
    "display_mode": "windowed",
    "vsync": true
  });

  let state: SystemState = serde_json::from_value(json).unwrap();
  assert_eq!(state.width, 2560);
  assert_eq!(state.height, 1440);
}

#[test]
fn system_state_backward_compat_with_legacy_nested_display() {
  // Old system.json had a nested "display" object
  let json = serde_json::json!({
    "master_volume": 1.0,
    "music_volume": 1.0,
    "sfx_volume": 1.0,
    "display": {
      "width": 1600,
      "height": 900,
      "display_mode": "windowed",
      "vsync": true
    }
  });

  let state: SystemState = serde_json::from_value(json).unwrap();
  assert_eq!(state.width, 1600);
  assert_eq!(state.height, 900);
  assert_eq!(state.display_mode, DisplayMode::Windowed);
  assert_eq!(state.vsync, Some(true));
}

#[test]
fn system_state_flat_resolution_string_wins_over_legacy_fields() {
  let json = serde_json::json!({
    "master_volume": 1.0,
    "music_volume": 1.0,
    "sfx_volume": 1.0,
    "resolution": "1920x1080",
    "width": 800,
    "height": 600,
    "display_mode": "windowed",
    "vsync": true
  });

  let state: SystemState = serde_json::from_value(json).unwrap();
  assert_eq!(state.width, 1920);
  assert_eq!(state.height, 1080);
}

#[test]
fn system_state_malformed_resolution_uses_engine_defaults() {
  let json = serde_json::json!({
    "master_volume": 1.0,
    "music_volume": 1.0,
    "sfx_volume": 1.0,
    "resolution": "invalid",
    "display_mode": "windowed",
    "vsync": true
  });

  let state: SystemState = serde_json::from_value(json).unwrap();
  assert_eq!(state.width, 1280);
  assert_eq!(state.height, 720);
}

// ==========================================================================
// SystemDefaults resolution field
// ==========================================================================

#[test]
fn system_defaults_resolution_is_none_by_default() {
  let defaults = SystemDefaults::default();
  assert_eq!(defaults.resolution, None);
}

#[test]
fn system_state_from_defaults_with_resolution() {
  let defaults = SystemDefaults {
    resolution: Some((800, 600)),
    ..SystemDefaults::default()
  };

  let state = SystemState::from_defaults(&defaults);
  assert_eq!(state.width, 800);
  assert_eq!(state.height, 600);
}

#[test]
fn system_state_from_defaults_without_resolution_uses_engine_default() {
  let defaults = SystemDefaults::default();
  let state = SystemState::from_defaults(&defaults);

  // Engine default is 1280x720
  assert_eq!(state.width, 1280);
  assert_eq!(state.height, 720);
}

// ==========================================================================
// Full save/load roundtrip with new format
// ==========================================================================

#[test]
fn system_json_roundtrip_preserves_resolution() {
  let dir = tempfile::tempdir().unwrap();
  let manager = SaveManager::new(dir.path()).unwrap();

  let defaults = SystemDefaults {
    resolution: Some((800, 600)),
    ..SystemDefaults::default()
  };

  // Create system.json from defaults
  let state = manager.load_or_create_system(&defaults).unwrap();
  assert_eq!(state.width, 800);
  assert_eq!(state.height, 600);

  // Reload and verify
  let reloaded = manager.load_system().unwrap();
  assert_eq!(reloaded.width, 800);
  assert_eq!(reloaded.height, 600);
}

#[test]
fn legacy_system_json_loads_and_resaves_as_new_format() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("system.json");

  // Write legacy format directly (nested display object)
  std::fs::write(
    &path,
    r#"{
      "master_volume": 0.8,
      "music_volume": 1.0,
      "sfx_volume": 1.0,
      "display": {
        "width": 1920,
        "height": 1080,
        "display_mode": "borderless_fullscreen",
        "vsync": true
      },
      "show_fps": true,
      "env": {}
    }"#,
  )
  .unwrap();

  let manager = SaveManager::new(dir.path()).unwrap();

  // Load should parse legacy format
  let state = manager.load_system().unwrap();
  assert_eq!(state.width, 1920);
  assert_eq!(state.height, 1080);
  assert_eq!(state.display_mode, DisplayMode::BorderlessFullscreen);

  // Re-save should write new flat format
  manager.save_system(&state).unwrap();
  let content = std::fs::read_to_string(&path).unwrap();
  let raw: serde_json::Value = serde_json::from_str(&content).unwrap();
  // Should have flat keys, not nested display object
  assert_eq!(raw["resolution"], "1920x1080");
  assert_eq!(raw["display_mode"], "borderless_fullscreen");
  assert!(raw.get("display").is_none());
}
