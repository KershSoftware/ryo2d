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

/// A record's time as `HH:MM:SS.mmm` (the log file's and the logger
/// window's, both native).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn clock(time: Duration) -> String {
  let secs = time.as_secs();
  format!(
    "{:02}:{:02}:{:02}.{:03}",
    secs / 3600,
    secs / 60 % 60,
    secs % 60,
    time.subsec_millis()
  )
}

/// One line of the log.
#[derive(Debug, Clone)]
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
}
