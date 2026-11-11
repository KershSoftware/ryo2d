//! `RYO_CAPTURE_*`, `RYO_HEADLESS` and `RYO_HEADLESS_HZ` environment
//! configuration.

use std::path::PathBuf;
use std::time::Duration;

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
      match parse_flag(raw) {
        Some(on) => config.exit_after = on,
        None => warnings.push(format!(
          "RYO_CAPTURE_EXIT={raw:?} is invalid (expected 1/true or 0/false); ignoring"
        )),
      }
    }

    // Exiting only makes sense once scheduled captures exist. Invalid frames
    // already warned above.
    if config.frames.is_empty() {
      if config.exit_after && frames.is_none() {
        warnings.push(
          "RYO_CAPTURE_EXIT is set but RYO_CAPTURE_FRAMES schedules no captures; ignoring"
            .to_string(),
        );
      }
      config.exit_after = false;
    }

    (config, warnings)
  }
}

/// The variables a scripted run reads (`RYO_HEADLESS`, its pace, the
/// captures and the input script)
pub(crate) const SCRIPT_VARS: [&str; 6] = [
  "RYO_HEADLESS",
  "RYO_HEADLESS_HZ",
  "RYO_CAPTURE_FRAMES",
  "RYO_CAPTURE_DIR",
  "RYO_CAPTURE_EXIT",
  "RYO_INPUT",
];

/// A run started with any of [`SCRIPT_VARS`] set (`is_set` says whether a
/// variable is) shows its log on stderr in every build, so `ryo2d run` with
/// `RYO_CAPTURE_FRAMES` says what it wrote and what it could not use.
pub(crate) fn start_log_for_script(is_set: impl Fn(&str) -> bool) {
  if SCRIPT_VARS.into_iter().any(is_set) {
    crate::core::logging::set_stderr(true);
  }
}

/// Read `RYO_HEADLESS`, returning whether to run without a window and a
/// warning for a value that was not used.
pub(crate) fn headless_run_from_env() -> (bool, Option<String>) {
  parse_headless(
    std::env::var("RYO_HEADLESS").ok().as_deref(),
    cfg!(debug_assertions),
  )
}

pub(crate) fn parse_headless(raw: Option<&str>, dev_build: bool) -> (bool, Option<String>) {
  let Some(raw) = raw else {
    return (false, None);
  };
  match parse_flag(raw) {
    Some(false) => (false, None),
    Some(true) if dev_build => (true, None),
    Some(true) => (
      false,
      Some("RYO_HEADLESS is ignored in release builds".to_string()),
    ),
    None => (
      false,
      Some(format!(
        "RYO_HEADLESS={raw:?} is invalid (expected 1/true or 0/false); opening a window"
      )),
    ),
  }
}

/// The fewest steps a second `RYO_HEADLESS_HZ` takes (`0` aside, which is
/// unpaced): slower, each step would sleep for seconds or longer
const MIN_HEADLESS_HZ: f64 = 1.0;

/// The most steps a second `RYO_HEADLESS_HZ` takes
const MAX_HEADLESS_HZ: f64 = 10_000.0;

/// Read `RYO_HEADLESS_HZ`: the wall time each step of a headless run takes
/// at least (`None`: as fast as it can), and a warning for a value that was
/// not used.
pub(crate) fn headless_pace_from_env() -> (Option<Duration>, Option<String>) {
  parse_headless_hz(std::env::var("RYO_HEADLESS_HZ").ok().as_deref())
}

/// Steps a second (a number from 1 to 10,000, `60` or `59.94`), each step
/// then taking at least its share of a second of wall time; unset, empty or
/// `0` is unpaced
pub(crate) fn parse_headless_hz(raw: Option<&str>) -> (Option<Duration>, Option<String>) {
  let Some(raw) = raw else {
    return (None, None);
  };
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return (None, None);
  }
  let pace = match trimmed.parse::<f64>() {
    Ok(0.0) => return (None, None),
    Ok(hz) if (MIN_HEADLESS_HZ..=MAX_HEADLESS_HZ).contains(&hz) => {
      Duration::try_from_secs_f64(1.0 / hz).ok()
    }
    _ => None,
  };
  match pace {
    Some(pace) => (Some(pace), None),
    None => (
      None,
      Some(format!(
        "RYO_HEADLESS_HZ={raw:?} is invalid (expected steps a second from 1 to 10000, like \"60\", or 0 for as fast as it can)"
      )),
    ),
  }
}

/// `1`/`true` or `0`/`false` (any case, surrounding spaces ignored); empty is false
fn parse_flag(raw: &str) -> Option<bool> {
  match raw.trim().to_ascii_lowercase().as_str() {
    "1" | "true" => Some(true),
    "0" | "false" | "" => Some(false),
    _ => None,
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
  fn any_scripting_variable_turns_the_log_s_stderr_on() {
    for name in SCRIPT_VARS {
      // As a build without tools starts
      let _watch = crate::core::logging::watch_stderr(false);
      start_log_for_script(|var| var == name);
      assert!(crate::core::logging::stderr_on(), "{name}");
    }
    let _watch = crate::core::logging::watch_stderr(false);
    start_log_for_script(|var| var == "RYO_LOG");
    assert!(!crate::core::logging::stderr_on(), "a player's run");
  }

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
  fn exit_without_frames_warns_and_has_no_effect() {
    let (config, warnings) = AutoCaptureConfig::parse(None, None, Some("1"));
    assert!(!config.exit_after);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("RYO_CAPTURE_FRAMES"));
    // Exit off needs no frames
    let (_, warnings) = AutoCaptureConfig::parse(None, None, Some("0"));
    assert!(warnings.is_empty());
  }

  #[test]
  fn headless_flag_is_on_only_for_one_or_true_in_dev_builds() {
    assert_eq!(parse_headless(None, true), (false, None));
    for raw in ["1", "true", " TRUE "] {
      assert_eq!(parse_headless(Some(raw), true), (true, None), "{raw:?}");
    }
    for raw in ["0", "false", ""] {
      assert_eq!(parse_headless(Some(raw), true), (false, None), "{raw:?}");
    }
    let (on, warning) = parse_headless(Some("yes"), true);
    assert!(!on);
    assert!(warning.is_some_and(|w| w.contains("\"yes\"")));
    let (on, warning) = parse_headless(Some("1"), false);
    assert!(!on, "release builds always open a window");
    assert!(warning.is_some_and(|w| w.contains("release")));
  }

  #[test]
  fn headless_hz_paces_each_step_to_its_share_of_a_second() {
    assert_eq!(parse_headless_hz(None), (None, None));
    for raw in ["", " ", "0", "0.0"] {
      assert_eq!(parse_headless_hz(Some(raw)), (None, None), "{raw:?}");
    }
    assert_eq!(
      parse_headless_hz(Some(" 60 ")),
      (Some(Duration::from_secs_f64(1.0 / 60.0)), None)
    );
    assert_eq!(
      parse_headless_hz(Some("4")),
      (Some(Duration::from_millis(250)), None)
    );
    assert_eq!(
      parse_headless_hz(Some("10000")),
      (Some(Duration::from_micros(100)), None)
    );
    assert_eq!(
      parse_headless_hz(Some("1")),
      (Some(Duration::from_secs(1)), None)
    );
    // Below one step a second a step would sleep for seconds to days, and the
    // tiniest values overflow a `Duration`
    for raw in [
      "fast", "-60", "10001", "inf", "NaN", "60hz", "0.5", "1e-6", "1e-20", "4.9e-324",
    ] {
      let (pace, warning) = parse_headless_hz(Some(raw));
      assert_eq!(pace, None, "{raw:?}");
      assert!(
        warning.is_some_and(|w| w.contains("RYO_HEADLESS_HZ") && w.contains(raw)),
        "{raw:?} warns, naming itself"
      );
    }
  }

  #[test]
  fn custom_dir_applies_without_frames() {
    let (config, _) = AutoCaptureConfig::parse(None, Some(" shots "), None);
    assert_eq!(config.dir, PathBuf::from("shots"));
  }
}
