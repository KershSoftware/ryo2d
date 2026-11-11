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
  #[cfg(test)]
  if watch::set(on) {
    return;
  }
  STDERR.store(on, Ordering::Relaxed);
}

/// Whether the stderr output is on.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn stderr_enabled() -> bool {
  #[cfg(test)]
  if let Some(on) = watch::on() {
    return on;
  }
  STDERR.load(Ordering::Relaxed)
}

/// Whether the stderr output is on
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn stderr_on() -> bool {
  stderr_enabled()
}

/// A record's line on stderr: `LEVEL target: message`.
pub(super) fn stderr_line(record: &LogRecord) -> String {
  format!("{} {}: {}", record.level, record.target, record.message)
}

/// Writes `record`'s line to stderr.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn print(record: &LogRecord) {
  #[cfg(test)]
  if watch::printed(record) {
    return;
  }
  if std::thread::panicking() {
    // `eprintln!` panics when stderr is gone, and a panic in a panicking
    // thread (the panic hook's record) aborts before the file is flushed
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "{}", stderr_line(record));
  } else {
    // The test harness captures only the print macros
    eprintln!("{}", stderr_line(record));
  }
}

/// Writes `record` (its `seq` not yet set) to every output, and gives back
/// a record whose buffers no output keeps (the one the ring dropped for it,
/// or the record itself when nothing kept it), for the next record to reuse.
/// `to_stderr` is false for a record stderr must not take (the panic hook's:
/// Rust's own hook prints the panic there right after).
///
/// One record is shared: the file, stderr and the browser console read it,
/// the file's early buffer copies it, and it moves into the last output that
/// keeps it (the ring, else a capture). With a capture and the ring both,
/// the ring takes a copy.
pub(super) fn emit(mut record: LogRecord, to_stderr: bool) -> Option<LogRecord> {
  let sink = sink_for(record.scope);

  // The file takes every record, a captured one too, and first: a line on
  // a terminal that has gone cannot keep it from the file. A captured record
  // is the test's to read, not the terminal's (a failing test's capture
  // prints its records as it drops)
  #[cfg(not(target_arch = "wasm32"))]
  {
    let may_show = to_stderr && sink.is_none();
    let shown = match super::file::write(&record, may_show) {
      // Kept for a file not yet open, which read the switch as it kept it
      Some(shown) => shown,
      None => may_show && stderr_enabled(),
    };
    #[cfg(test)]
    watch::took(Taken::File);
    if shown {
      print(&record);
    }
  }
  #[cfg(target_arch = "wasm32")]
  {
    let _ = to_stderr;
    browser_console(&record);
  }

  // The ring gives the number, under its lock, so it stays in order
  #[cfg(dev_tools)]
  {
    match sink {
      Some(sink) => {
        let (seq, dropped) = super::ring::push(record.clone());
        record.seq = seq;
        sink.push(record);
        dropped
      }
      None => super::ring::push(record).1,
    }
  }
  #[cfg(not(dev_tools))]
  {
    record.seq = NEXT_SEQ.fetch_add(1, Ordering::Relaxed);
    match sink {
      Some(sink) => {
        sink.push(record);
        None
      }
      None => Some(record),
    }
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

/// An output a watched thread's record went to, in [`StderrWatch::outputs`]
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Taken {
  File,
  Stderr,
}

/// For a test: while it is held, its thread has a stderr switch of its own
/// (other tests' engines turn the process's on), and the lines the stderr
/// output would print on the thread are kept here instead of printed, with
/// the order the outputs took them in.
#[cfg(test)]
pub(crate) fn watch_stderr(on: bool) -> StderrWatch {
  watch::WATCH.with(|watch| {
    *watch.borrow_mut() = Some(watch::Watched {
      on,
      lines: Vec::new(),
      taken: Vec::new(),
    });
  });
  StderrWatch
}

/// See [`watch_stderr`]; the thread's watch ends when it drops.
#[cfg(test)]
pub(crate) struct StderrWatch;

#[cfg(test)]
impl StderrWatch {
  /// The lines the stderr output took, in order
  pub(crate) fn lines(&self) -> Vec<String> {
    watch::WATCH.with(|watch| {
      watch
        .borrow()
        .as_ref()
        .map(|watched| watched.lines.clone())
        .unwrap_or_default()
    })
  }

  /// The outputs (the file, stderr) records went to, in order
  pub(crate) fn outputs(&self) -> Vec<Taken> {
    watch::WATCH.with(|watch| {
      watch
        .borrow()
        .as_ref()
        .map(|watched| watched.taken.clone())
        .unwrap_or_default()
    })
  }
}

#[cfg(test)]
impl Drop for StderrWatch {
  fn drop(&mut self) {
    let _ = watch::WATCH.try_with(|watch| watch.borrow_mut().take());
  }
}

#[cfg(test)]
mod watch {
  use std::cell::RefCell;

  use super::{LogRecord, Taken, stderr_line};

  pub(super) struct Watched {
    pub(super) on: bool,
    pub(super) lines: Vec<String>,
    pub(super) taken: Vec<Taken>,
  }

  thread_local! {
    pub(super) static WATCH: RefCell<Option<Watched>> = const { RefCell::new(None) };
  }

  /// Sets the watched thread's switch; false when the thread has no watch
  pub(super) fn set(on: bool) -> bool {
    WATCH
      .try_with(|watch| {
        watch
          .borrow_mut()
          .as_mut()
          .map(|watched| watched.on = on)
          .is_some()
      })
      .unwrap_or(false)
  }

  pub(super) fn on() -> Option<bool> {
    WATCH
      .try_with(|watch| watch.borrow().as_ref().map(|watched| watched.on))
      .ok()
      .flatten()
  }

  /// Keeps the line on a watched thread; false when the thread has none
  pub(super) fn printed(record: &LogRecord) -> bool {
    WATCH
      .try_with(|watch| {
        watch
          .borrow_mut()
          .as_mut()
          .map(|watched| {
            watched.lines.push(stderr_line(record));
            watched.taken.push(Taken::Stderr);
          })
          .is_some()
      })
      .unwrap_or(false)
  }

  pub(super) fn took(output: Taken) {
    let _ = WATCH.try_with(|watch| {
      if let Some(watched) = watch.borrow_mut().as_mut() {
        watched.taken.push(output);
      }
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::logging::Origin;
  use crate::core::logging::scope::{in_scope, new_scope};

  fn record(scope: u64) -> LogRecord {
    LogRecord {
      seq: 0,
      level: log::Level::Warn,
      target: "game::outputs".into(),
      origin: Origin::Game,
      message: "in turn".into(),
      time: std::time::Duration::ZERO,
      frame: None,
      scope,
    }
  }

  #[test]
  fn the_file_takes_a_record_before_stderr() {
    // A scope with no capture, so stderr takes the record
    let scope = new_scope();
    let watch = watch_stderr(true);
    in_scope(scope, || emit(record(scope), true));
    assert_eq!(watch.outputs(), [Taken::File, Taken::Stderr]);
    assert_eq!(watch.lines(), ["WARN game::outputs: in turn"]);
  }

  #[test]
  fn a_captured_record_and_one_kept_from_stderr_skip_it() {
    let capture = crate::core::logging::capture_log();
    let watch = watch_stderr(true);
    emit(record(capture.scope()), true);
    let scope = new_scope();
    in_scope(scope, || emit(record(scope), false));
    assert!(watch.lines().is_empty(), "{:?}", watch.lines());
    assert_eq!(capture.records().len(), 1);
  }
}
