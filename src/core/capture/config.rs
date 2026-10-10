//! `RYO_CAPTURE_*` environment configuration.

use std::path::PathBuf;

/// Default output directory for captures.
pub const DEFAULT_CAPTURE_DIR: &str = "captures";

/// Auto-capture settings read from `RYO_CAPTURE_*` environment variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoCaptureConfig {
  /// Presented-frame numbers to capture (1-based, sorted, no duplicates)
  pub frames: Vec<u64>,
  /// Output directory for default-named captures
  pub dir: PathBuf,
  /// Quit once the last scheduled capture is written
  pub exit_after: bool,
}

impl Default for AutoCaptureConfig {
  fn default() -> Self {
    Self {
      frames: Vec::new(),
      dir: PathBuf::from(DEFAULT_CAPTURE_DIR),
      exit_after: false,
    }
  }
}

impl AutoCaptureConfig {
  /// Read `RYO_CAPTURE_FRAMES`, `RYO_CAPTURE_DIR` and `RYO_CAPTURE_EXIT`.
  ///
  /// Returns the config and a warning for each invalid value.
  pub fn from_env() -> (Self, Vec<String>) {
    Self::parse(
      std::env::var("RYO_CAPTURE_FRAMES").ok().as_deref(),
      std::env::var("RYO_CAPTURE_DIR").ok().as_deref(),
      std::env::var("RYO_CAPTURE_EXIT").ok().as_deref(),
    )
  }

  /// Parse raw variable values (`None` means unset).
  pub fn parse(frames: Option<&str>, dir: Option<&str>, exit: Option<&str>) -> (Self, Vec<String>) {
    let mut config = Self::default();
    let mut warnings = Vec::new();

    if let Some(dir) = dir.map(str::trim).filter(|d| !d.is_empty()) {
      config.dir = PathBuf::from(dir);
    }

    if let Some(raw) = frames {
      match parse_frames(raw) {
        Some(list) => config.frames = list,
        None => warnings.push(format!(
          "RYO_CAPTURE_FRAMES={raw:?} is invalid (expected positive frame numbers like \"60,120\"); auto-capture disabled"
        )),
      }
    }

    if let Some(raw) = exit {
      match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => config.exit_after = true,
        "0" | "false" | "" => {}
        _ => warnings.push(format!(
          "RYO_CAPTURE_EXIT={raw:?} is invalid (expected 1/true or 0/false); ignoring"
        )),
      }
    }

    // Exiting only makes sense once scheduled captures exist
    if config.frames.is_empty() {
      config.exit_after = false;
    }

    (config, warnings)
  }
}

fn parse_frames(raw: &str) -> Option<Vec<u64>> {
  let mut frames = Vec::new();
  for part in raw.split(',') {
    let frame: u64 = part.trim().parse().ok()?;
    if frame == 0 {
      return None;
    }
    frames.push(frame);
  }
  frames.sort_unstable();
  frames.dedup();
  Some(frames)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn unset_is_disabled() {
    let (config, warnings) = AutoCaptureConfig::parse(None, None, None);
    assert!(config.frames.is_empty());
    assert_eq!(config.dir, PathBuf::from("captures"));
    assert!(!config.exit_after);
    assert!(warnings.is_empty());
  }

  #[test]
  fn frames_are_sorted_and_deduplicated() {
    let (config, warnings) = AutoCaptureConfig::parse(Some(" 120, 60,60 "), None, None);
    assert_eq!(config.frames, vec![60, 120]);
    assert!(warnings.is_empty());
  }

  #[test]
  fn invalid_frames_disable_auto_capture() {
    for raw in ["", "abc", "60,x", "0", "-5", "60,,120"] {
      let (config, warnings) = AutoCaptureConfig::parse(Some(raw), None, Some("1"));
      assert!(config.frames.is_empty(), "{raw:?} should be rejected");
      assert!(!config.exit_after, "{raw:?}: exit needs frames");
      assert_eq!(warnings.len(), 1, "{raw:?} should warn once");
    }
  }

  #[test]
  fn exit_accepts_one_and_true() {
    for raw in ["1", "true", "TRUE", " True "] {
      let (config, warnings) = AutoCaptureConfig::parse(Some("5"), None, Some(raw));
      assert!(config.exit_after, "{raw:?}");
      assert!(warnings.is_empty());
    }
  }

  #[test]
  fn exit_off_values_and_invalid_values() {
    for raw in ["0", "false", ""] {
      let (config, warnings) = AutoCaptureConfig::parse(Some("5"), None, Some(raw));
      assert!(!config.exit_after, "{raw:?}");
      assert!(warnings.is_empty(), "{raw:?}");
    }
    let (config, warnings) = AutoCaptureConfig::parse(Some("5"), None, Some("yes"));
    assert!(!config.exit_after);
    assert_eq!(warnings.len(), 1);
  }

  #[test]
  fn exit_without_frames_has_no_effect() {
    let (config, warnings) = AutoCaptureConfig::parse(None, None, Some("1"));
    assert!(!config.exit_after);
    assert!(warnings.is_empty());
  }

  #[test]
  fn custom_dir_applies_without_frames() {
    let (config, _) = AutoCaptureConfig::parse(None, Some(" shots "), None);
    assert_eq!(config.dir, PathBuf::from("shots"));
  }
}
