//! A record: one line of the log, as every output receives it.

use std::time::Duration;

/// Where a record comes from, read from its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
  /// The engine: `ryo2d`, under it, or one of the `ryo2d_*` crates
  Engine,
  /// The game: a target of `game` or under it (the prelude's macros log there)
  Game,
  /// Anything else: wgpu, naga, a game that calls `log::info!` itself, ...
  Library,
}

impl Origin {
  /// The origin of a record with this target.
  pub fn of(target: &str) -> Origin {
    if under(target, "game") {
      Origin::Game
    } else if under(target, "ryo2d") || target.starts_with("ryo2d_") {
      Origin::Engine
    } else {
      Origin::Library
    }
  }
}

/// Whether `target` is `path` or a module under it (`game::menu` is under
/// `game`; `gamepad` is not).
pub(crate) fn under(target: &str, path: &str) -> bool {
  match target.strip_prefix(path) {
    Some(rest) => rest.is_empty() || rest.starts_with("::"),
    None => false,
  }
}

/// A record's time as `HH:MM:SS.mmm` (the logger window's; the file writes
/// it with [`push_clock`]).
#[cfg(all(dev_tools, not(target_arch = "wasm32")))]
pub(crate) fn clock(time: Duration) -> String {
  let mut text = String::with_capacity(12);
  push_clock(&mut text, time);
  text
}

/// Appends a record's time as `HH:MM:SS.mmm` to `out`, written by hand (the
/// file writes one a line, and the formatter costs several times this).
/// Past 99 hours the hours take as many digits as they need.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn push_clock(out: &mut String, time: Duration) {
  fn digits(out: &mut String, value: u64, width: usize) {
    let mut buffer = [b'0'; 20];
    let mut at = buffer.len();
    let mut rest = value;
    loop {
      at -= 1;
      buffer[at] = b'0' + (rest % 10) as u8;
      rest /= 10;
      if rest == 0 {
        break;
      }
    }
    at = at.min(buffer.len() - width);
    for &digit in &buffer[at..] {
      out.push(char::from(digit));
    }
  }
  let secs = time.as_secs();
  digits(out, secs / 3600, 2);
  out.push(':');
  digits(out, secs / 60 % 60, 2);
  out.push(':');
  digits(out, secs % 60, 2);
  out.push('.');
  digits(out, u64::from(time.subsec_millis()), 3);
}

/// One line of the log, as a test's [`capture_log`](crate::testing::capture_log)
/// gives it. The log makes them: a field may be added later, so outside the
/// engine one is read, never built.
///
/// ```compile_fail
/// let record = ryo2d::logging::LogRecord {
///   seq: 1,
///   level: ryo2d::logging::Level::Warn,
///   target: "game::x".to_owned(),
///   origin: ryo2d::logging::Origin::Game,
///   message: String::new(),
///   time: std::time::Duration::ZERO,
///   frame: None,
///   scope: 0,
/// };
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LogRecord {
  /// Its place in the log: every record's is greater than the one before
  pub seq: u64,
  pub level: log::Level,
  /// The module path it was logged from (`game::` before a game's)
  pub target: String,
  pub origin: Origin,
  pub message: String,
  /// Time since the logger started
  pub time: Duration,
  /// The logging thread's frame number; `None` on a thread without one
  pub frame: Option<u64>,
  /// The logging thread's log scope (0 in a game)
  pub scope: u64,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn targets_name_their_origin() {
    assert_eq!(Origin::of("game"), Origin::Game);
    assert_eq!(Origin::of("game::x"), Origin::Game);
    assert_eq!(Origin::of("ryo2d"), Origin::Engine);
    assert_eq!(Origin::of("ryo2d::core"), Origin::Engine);
    assert_eq!(Origin::of("ryo2d_procgen::noise"), Origin::Engine);
    assert_eq!(Origin::of("wgpu_core"), Origin::Library);
    assert_eq!(Origin::of("gamepad"), Origin::Library);
    assert_eq!(Origin::of("ryo2dx"), Origin::Library);
    assert_eq!(Origin::of(""), Origin::Library);
  }

  #[cfg(not(target_arch = "wasm32"))]
  #[test]
  fn the_clock_reads_as_the_formatter_would_write_it() {
    for millis in [
      0,
      7,
      999,
      1_000,
      59_999,
      3_723_045,
      86_399_999,
      359_999_999,
      360_000_000,
      123_456_789_012,
    ] {
      let time = Duration::from_millis(millis);
      let secs = time.as_secs();
      let expected = format!(
        "{:02}:{:02}:{:02}.{:03}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        time.subsec_millis()
      );
      let mut written = String::new();
      push_clock(&mut written, time);
      assert_eq!(written, expected);
    }
  }
}
