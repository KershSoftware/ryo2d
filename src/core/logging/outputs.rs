//! The outputs: each takes the record in turn, after its one format.
//!
//! No output holds a lock while another writes: the ring and each capture
//! have their own mutex, held only for their push, the file has its own, held
//! only for its write and flush, and stderr's is std's own.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::capture::sink_for;
use super::record::LogRecord;

/// The next sequence number. With the ring, it is taken under the ring's lock.
pub(super) static NEXT_SEQ: AtomicU64 = AtomicU64::new(1);

/// Whether records go to stderr (native): on in a build with tools, and in a
/// headless run in any build
static STDERR: AtomicBool = AtomicBool::new(cfg!(dev_tools));

/// Turns the stderr output on or off (native; the browser has its console).
pub(crate) fn set_stderr(on: bool) {
  STDERR.store(on, Ordering::Relaxed);
}

/// Whether the stderr output is on
#[cfg(test)]
pub(crate) fn stderr_on() -> bool {
  STDERR.load(Ordering::Relaxed)
}

/// A record's line on stderr: `LEVEL target: message`.
pub(super) fn stderr_line(record: &LogRecord) -> String {
  format!("{} {}: {}", record.level, record.target, record.message)
}

/// Writes `record` (its `seq` not yet set) to every output.
pub(super) fn emit(mut record: LogRecord) {
  #[cfg(dev_tools)]
  super::ring::push(&mut record);
  #[cfg(not(dev_tools))]
  {
    record.seq = NEXT_SEQ.fetch_add(1, Ordering::Relaxed);
  }

  let sink = sink_for(record.scope);

  // A captured record is the test's to read, not the terminal's (a failing
  // test's capture prints its records as it drops)
  #[cfg(not(target_arch = "wasm32"))]
  if sink.is_none() && STDERR.load(Ordering::Relaxed) {
    eprintln!("{}", stderr_line(&record));
  }

  #[cfg(target_arch = "wasm32")]
  browser_console(&record);

  // The file takes every record, a captured one too
  #[cfg(not(target_arch = "wasm32"))]
  super::file::write(&record);

  if let Some(sink) = sink {
    sink.push(record);
  }
}

#[cfg(target_arch = "wasm32")]
fn browser_console(record: &LogRecord) {
  use log::Level;
  let line = stderr_line(record).into();
  match record.level {
    Level::Error => web_sys::console::error_1(&line),
    Level::Warn => web_sys::console::warn_1(&line),
    Level::Info => web_sys::console::info_1(&line),
    Level::Debug | Level::Trace => web_sys::console::debug_1(&line),
  }
}
