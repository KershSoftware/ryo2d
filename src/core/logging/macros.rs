//! The game's log macros: `log`'s, under a `game::` target.

/// Logs a trace line from game code, under `game::` and the module path.
/// Off at the default levels; `RYO_LOG=game=trace` turns the game's on.
#[macro_export]
macro_rules! trace {
  ($($arg:tt)+) => {{
    $crate::logging::ensure();
    $crate::logging::log::trace!(target: ::core::concat!("game::", ::core::module_path!()), $($arg)+)
  }};
}

/// Logs a debug line from game code, under `game::` and the module path.
/// Off at the default levels; `RYO_LOG=game=debug` turns the game's on.
#[macro_export]
macro_rules! debug {
  ($($arg:tt)+) => {{
    $crate::logging::ensure();
    $crate::logging::log::debug!(target: ::core::concat!("game::", ::core::module_path!()), $($arg)+)
  }};
}

/// Logs an info line from game code, under `game::` and the module path
/// (`game::my_game::scenes::menu`). It installs the engine's logger if
/// nothing has, so a line logged before the engine is built is kept.
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn main() {
///   info!("starting with {} levels", 3);
/// #   assert_ne!(ryo2d::logging::log::max_level(), ryo2d::logging::log::LevelFilter::Off);
///   ryo2d::info!("by path, without the prelude");
/// }
/// ```
#[macro_export]
macro_rules! info {
  ($($arg:tt)+) => {{
    $crate::logging::ensure();
    $crate::logging::log::info!(target: ::core::concat!("game::", ::core::module_path!()), $($arg)+)
  }};
}

/// Logs a warning from game code, under `game::` and the module path.
#[macro_export]
macro_rules! warn {
  ($($arg:tt)+) => {{
    $crate::logging::ensure();
    $crate::logging::log::warn!(target: ::core::concat!("game::", ::core::module_path!()), $($arg)+)
  }};
}

/// Logs an error from game code, under `game::` and the module path.
#[macro_export]
macro_rules! error {
  ($($arg:tt)+) => {{
    $crate::logging::ensure();
    $crate::logging::log::error!(target: ::core::concat!("game::", ::core::module_path!()), $($arg)+)
  }};
}

/// Logs a warning the first time this call site is reached, and never again:
/// for a warning in code that runs every frame.
#[macro_export]
macro_rules! warn_once {
  ($($arg:tt)+) => {{
    static WARNED: ::core::sync::atomic::AtomicBool = ::core::sync::atomic::AtomicBool::new(false);
    if !WARNED.swap(true, ::core::sync::atomic::Ordering::Relaxed) {
      $crate::warn!($($arg)+);
    }
  }};
}

#[cfg(test)]
mod tests {
  use crate::core::logging::capture::capture_log;

  #[test]
  fn a_line_logged_before_any_engine_is_kept() {
    let capture = capture_log();
    crate::info!("before the engine");
    assert_eq!(capture.records().len(), 1);
    assert_eq!(capture.containing("before the engine"), 1);
  }

  #[test]
  fn warn_once_logs_once_per_call_site() {
    let capture = capture_log();
    for _ in 0..5 {
      crate::warn_once!("one site");
    }
    assert_eq!(capture.containing("one site"), 1);

    for _ in 0..3 {
      crate::warn_once!("two sites");
      crate::warn_once!("two sites");
    }
    assert_eq!(capture.containing("two sites"), 2);
    assert_eq!(capture.count(log::Level::Warn), 3);
  }
}
