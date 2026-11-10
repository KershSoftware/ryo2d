//! The log: `log`'s front, with the engine's logger behind it.
//!
//! Engine code logs with `log::info!` and the rest (the target is the module
//! path); a game logs with the prelude's [`info!`](crate::info) and the rest,
//! which put its records under `game::`. Each target logs at a level: a
//! default per [`Origin`] (`info` for the game and the engine, `warn` for
//! libraries), overrides by target prefix from `RYO_LOG` (read once, when the
//! logger installs) and [`set_level`].
//!
//! The logger installs on first use: the prelude's macros, an engine, or a
//! test's [`capture_log`]. A game that installs its own `log` logger first
//! keeps it, and the engine's outputs stay off.

mod capture;
mod levels;
mod macros;
mod outputs;
mod record;
#[cfg(dev_tools)]
mod ring;
mod scope;

use std::cell::Cell;
use std::sync::{OnceLock, PoisonError, RwLock, RwLockReadGuard};

use log::LevelFilter;
use web_time::Instant;

pub use capture::{LogCapture, capture_log, capture_log_bounded};
use levels::Levels;
pub use record::{LogRecord, Origin};

/// The `log` crate, for the prelude's macros in a game crate.
#[doc(hidden)]
pub use log;

struct Logger {
  start: Instant,
  levels: RwLock<Levels>,
}

impl Logger {
  fn levels(&self) -> RwLockReadGuard<'_, Levels> {
    self.levels.read().unwrap_or_else(PoisonError::into_inner)
  }
}

impl log::Log for Logger {
  fn enabled(&self, metadata: &log::Metadata) -> bool {
    metadata.level() <= self.levels().of(metadata.target())
  }

  fn log(&self, record: &log::Record) {
    if !self.enabled(record.metadata()) {
      return;
    }
    // The one format: every output takes this string
    let message = match record.args().as_str() {
      Some(text) => text.to_owned(),
      None => record.args().to_string(),
    };
    let target = record.target();
    outputs::emit(LogRecord {
      seq: 0,
      level: record.level(),
      target: target.to_owned(),
      origin: Origin::of(target),
      message,
      time: self.start.elapsed(),
      frame: FRAME.with(Cell::get),
      scope: scope::current_scope(),
    });
  }

  fn flush(&self) {}
}

static LOGGER: OnceLock<Logger> = OnceLock::new();
/// Set once, by the first `ensure`: whether the engine's logger is `log`'s.
static INSTALLED: OnceLock<bool> = OnceLock::new();

thread_local! {
  static FRAME: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Installs the engine's logger, once per process; later calls cost one
/// atomic read. Safe from any thread.
pub fn ensure() {
  INSTALLED.get_or_init(install);
}

fn install() -> bool {
  let (levels, problems) = Levels::parse(&std::env::var("RYO_LOG").unwrap_or_default());
  for problem in &problems {
    report(problem);
  }
  let most_verbose = levels.most_verbose();
  let logger = LOGGER.get_or_init(|| Logger {
    start: Instant::now(),
    levels: RwLock::new(levels),
  });
  match outcome(log::set_logger(logger)) {
    Outcome::Installed => {
      log::set_max_level(most_verbose);
      true
    }
    Outcome::Kept(line) => {
      report(line);
      false
    }
  }
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
  Installed,
  /// Another logger was in first: it stays, with this line said once
  Kept(&'static str),
}

fn outcome(result: Result<(), log::SetLoggerError>) -> Outcome {
  match result {
    Ok(()) => Outcome::Installed,
    Err(_) => Outcome::Kept(
      "another `log` logger was installed first; it is kept and the engine's log outputs are off",
    ),
  }
}

/// A line about the log itself, which has nowhere else to go.
fn report(line: &str) {
  #[cfg(not(target_arch = "wasm32"))]
  eprintln!("ryo2d: {line}");
  #[cfg(target_arch = "wasm32")]
  web_sys::console::warn_1(&format!("ryo2d: {line}").into());
}

/// Sets the level of `target` and every target under it (`game::menu` covers
/// `game::menu::options`), at once and without a restart.
pub fn set_level(target: &str, level: LevelFilter) {
  ensure();
  let Some(logger) = LOGGER.get() else { return };
  let mut levels = logger
    .levels
    .write()
    .unwrap_or_else(PoisonError::into_inner);
  levels.set(target, level);
  if installed_by_engine() {
    log::set_max_level(levels.most_verbose());
  }
}

/// The level `target` logs at.
pub fn level(target: &str) -> LevelFilter {
  ensure();
  LOGGER
    .get()
    .map_or(LevelFilter::Off, |logger| logger.levels().of(target))
}

/// Sets this thread's frame number, which its records carry.
#[cfg_attr(
  not(test),
  expect(
    dead_code,
    reason = "the engine sets it each frame once it moves to this log"
  )
)]
pub(crate) fn set_frame(frame: u64) {
  FRAME.with(|cell| cell.set(Some(frame)));
}

/// Whether the engine's logger is the process's (false when another was set
/// first, or before [`ensure`]).
pub(crate) fn installed_by_engine() -> bool {
  INSTALLED.get().copied().unwrap_or(false)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::logging::capture::capture_log;
  use crate::core::logging::record::Origin;
  use crate::core::logging::scope::in_scope;
  use log::{Level, LevelFilter};
  use std::fmt;
  use std::sync::atomic::{AtomicUsize, Ordering};

  #[test]
  // Here rather than beside the macros, where their own definitions would
  // shadow the prelude's
  fn the_prelude_s_macros_log_under_game() {
    use crate::prelude::*;

    let capture = capture_log();
    info!("an info {}", 1);
    warn!("a warning");
    error!("an error");
    debug!("hidden at the default level");
    trace!("hidden at the default level");

    let records = capture.records();
    assert_eq!(records.len(), 3, "{records:?}");
    for record in &records {
      assert!(record.target.starts_with("game::"), "{}", record.target);
      assert_eq!(record.target, concat!("game::", module_path!()));
      assert_eq!(record.origin, Origin::Game);
    }
    assert_eq!(records[0].message, "an info 1");
    assert_eq!(records[1].level, log::Level::Warn);
    assert_eq!(records[2].level, log::Level::Error);
  }

  #[test]
  fn a_library_s_warning_is_kept_and_its_info_is_not() {
    let capture = capture_log();
    log::warn!(target: "wgpu_core", "a library warning");
    log::info!(target: "wgpu_core", "a library note");
    let records = capture.records();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].origin, Origin::Library);
    assert_eq!(records[0].level, Level::Warn);
    assert_eq!(records[0].target, "wgpu_core");
    assert_eq!(records[0].message, "a library warning");
  }

  struct Silent;
  impl log::Log for Silent {
    fn enabled(&self, _: &log::Metadata) -> bool {
      false
    }
    fn log(&self, _: &log::Record) {}
    fn flush(&self) {}
  }
  static SILENT: Silent = Silent;

  #[test]
  fn a_logger_installed_first_is_kept() {
    assert_eq!(outcome(Ok(())), Outcome::Installed);
    // The engine's is in first here, so the other one is refused, as the
    // engine's is when a game installs its own first
    ensure();
    let Outcome::Kept(line) = outcome(log::set_logger(&SILENT)) else {
      panic!("a refused logger is reported");
    };
    assert!(line.contains("another"), "{line}");
    assert!(installed_by_engine());
  }

  /// Counts the times it is formatted
  struct Counted<'a>(&'a AtomicUsize);
  impl fmt::Display for Counted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      self.0.fetch_add(1, Ordering::Relaxed);
      f.write_str("counted")
    }
  }

  #[test]
  fn a_message_below_its_level_is_never_formatted() {
    let capture = capture_log();
    let below = AtomicUsize::new(0);
    log::debug!(target: "game::logging::below", "{}", Counted(&below));
    crate::debug!("{}", Counted(&below));
    log::info!(target: "wgpu_core", "{}", Counted(&below));
    assert_eq!(below.load(Ordering::Relaxed), 0);
    assert!(capture.records().is_empty());

    let passing = AtomicUsize::new(0);
    log::info!(target: "game::logging::below", "{}", Counted(&passing));
    assert_eq!(
      passing.load(Ordering::Relaxed),
      1,
      "formatted once for every output"
    );
    assert_eq!(capture.containing("counted"), 1);
  }

  #[test]
  fn the_frame_is_the_logging_thread_s() {
    let capture = capture_log();
    set_frame(7);
    log::warn!(target: "game::frames", "on the engine's thread");
    let scope = capture.scope();
    std::thread::spawn(move || {
      in_scope(scope, || log::warn!(target: "game::frames", "on a worker"))
    })
    .join()
    .expect("a worker thread");

    let records = capture.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].frame, Some(7));
    assert_eq!(records[1].frame, None);
  }

  #[test]
  fn a_level_can_be_changed_and_changed_back() {
    let capture = capture_log();
    let target = "game::logging::live";
    let before = level(target);
    assert_eq!(before, LevelFilter::Info);
    log::debug!(target: target, "hidden");

    set_level(target, LevelFilter::Debug);
    assert_eq!(level(target), LevelFilter::Debug);
    assert_eq!(level("game::logging::live::deeper"), LevelFilter::Debug);
    assert!(log::max_level() >= LevelFilter::Debug);
    log::debug!(target: target, "shown");

    set_level(target, before);
    assert_eq!(level(target), LevelFilter::Info);
    log::debug!(target: target, "hidden again");

    let messages: Vec<_> = capture.records().into_iter().map(|r| r.message).collect();
    assert_eq!(messages, ["shown"]);
  }
}
