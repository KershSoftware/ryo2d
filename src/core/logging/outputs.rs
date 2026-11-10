//! The outputs: each takes the record in turn, after its one format.
//!
//! No output holds a lock while another writes: the ring and each capture
//! have their own mutex, held only for their push, the file has its own, held
//! only for its write and flush, and stderr's is std's own.

use std::sync::atomic::AtomicU64;
#[cfg(not(dev_tools))]
use std::sync::atomic::Ordering;

use super::capture::sink_for;
use super::record::{LogRecord, Origin};
use crate::core::dev_log::{self, LogContext, LogLevel};

/// The next sequence number. With the ring, it is taken under the ring's lock.
pub(super) static NEXT_SEQ: AtomicU64 = AtomicU64::new(1);

/// Writes `record` (its `seq` not yet set) to every output.
pub(super) fn emit(mut record: LogRecord) {
  #[cfg(dev_tools)]
  super::ring::push(&mut record);
  #[cfg(not(dev_tools))]
  {
    record.seq = NEXT_SEQ.fetch_add(1, Ordering::Relaxed);
  }

  let sink = sink_for(record.scope);

  // A captured record is the test's to read, not the terminal's
  #[cfg(all(dev_tools, not(target_arch = "wasm32")))]
  if sink.is_none() {
    eprintln!("{} {}: {}", record.level, record.target, record.message);
  }

  #[cfg(target_arch = "wasm32")]
  browser_console(&record);

  // The file takes every record, a captured one too
  #[cfg(not(target_arch = "wasm32"))]
  super::file::write(&record);

  match sink {
    Some(sink) => sink.push(record),
    // A captured record skips the old buffer too: no one reads it there, and
    // a test's flood would push other tests' lines out of it
    None => bridge(record),
  }
}

#[cfg(target_arch = "wasm32")]
fn browser_console(record: &LogRecord) {
  use log::Level;
  let line = format!("{} {}: {}", record.level, record.target, record.message).into();
  match record.level {
    Level::Error => web_sys::console::error_1(&line),
    Level::Warn => web_sys::console::warn_1(&line),
    Level::Info => web_sys::console::info_1(&line),
    Level::Debug | Level::Trace => web_sys::console::debug_1(&line),
  }
}

/// Into the old log's buffer, which the logger window still reads.
fn bridge(record: LogRecord) {
  let level = match record.level {
    log::Level::Error => LogLevel::Error,
    log::Level::Warn => LogLevel::Warn,
    log::Level::Info => LogLevel::Info,
    log::Level::Debug => LogLevel::Debug,
    log::Level::Trace => LogLevel::Trace,
  };
  let context = match record.origin {
    Origin::Engine => LogContext::Engine,
    Origin::Game => LogContext::Game,
    Origin::Library => LogContext::External,
  };
  dev_log::push_quiet(level, context, old_module(&record.target), record.message);
}

/// The old log's module label: `ryo2d::core::particles::step` is `Particles`.
fn old_module(target: &str) -> Option<String> {
  let segment = target.strip_prefix("ryo2d::core::")?.split("::").next()?;
  let mut chars = segment.chars();
  let first = chars.next()?;
  Some(first.to_uppercase().chain(chars).collect())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_old_module_label_is_the_core_segment() {
    assert_eq!(
      old_module("ryo2d::core::particles::step").as_deref(),
      Some("Particles")
    );
    assert_eq!(old_module("ryo2d::core::audio").as_deref(), Some("Audio"));
    assert_eq!(old_module("ryo2d::cli"), None);
    assert_eq!(old_module("game::menu"), None);
    assert_eq!(old_module("ryo2d::core::"), None);
  }
}
