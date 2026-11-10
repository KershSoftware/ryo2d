//! The log file: `logs/latest.log` in the save folder, one line per record.
//!
//! The output is process-wide, like the logger. It has its own mutex, held
//! only for its own write and flush, so no other output waits on it. Native
//! only: the browser has its console.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use super::record::LogRecord;

/// The most the file takes in one run; after it, one last line and no more.
pub(crate) const CAP: u64 = 8 * 1024 * 1024;
/// A warning or an error flushes at once unless the last flush was this recent.
pub(crate) const FLUSH_GAP: Duration = Duration::from_millis(50);
/// The buffer between the lines and the disk.
const BUFFER: usize = 8 * 1024;
/// The last line of a full file.
const FULL_LINE: &str = "log full: no more is written this run\n";

struct FileOutput {
  writer: BufWriter<File>,
  /// Bytes written (buffered ones too)
  written: u64,
  full: bool,
  unflushed: bool,
  last_flush: Option<Instant>,
  /// Tests share the process, and other tests' records would land in a test's
  /// file: it takes only its own thread's
  #[cfg(test)]
  owner: std::thread::ThreadId,
}

static STATE: Mutex<Option<FileOutput>> = Mutex::new(None);
/// Whether a file is attached, so a record costs one atomic read without one.
static OPEN: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
static FLUSHES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

thread_local! {
  /// Whether this thread holds the file's lock: a panic on it (and its hook)
  /// must not wait for a lock it holds itself.
  static HOLDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The file's lock, noted as this thread's while it is held.
struct Held(MutexGuard<'static, Option<FileOutput>>);

impl Held {
  fn output(&mut self) -> Option<&mut FileOutput> {
    self.0.as_mut()
  }

  /// Detaches the file, after a failure to write it.
  fn detach(mut self, error: &io::Error) {
    *self.0 = None;
    OPEN.store(false, Ordering::Release);
    drop(self);
    super::report(&format!(
      "the log file failed ({error}); logging to it stops"
    ));
  }
}

impl Drop for Held {
  fn drop(&mut self) {
    HOLDING.with(|flag| flag.set(false));
  }
}

/// The lock, or `None` when this thread already holds it (the panic hook, for
/// a panic that came in the middle of a write): it waits for no one else.
fn lock() -> Option<Held> {
  if HOLDING.with(std::cell::Cell::get) {
    return None;
  }
  let guard = STATE.lock().unwrap_or_else(PoisonError::into_inner);
  HOLDING.with(|flag| flag.set(true));
  Some(Held(guard))
}

/// Rotates the last run's file, opens this run's and makes it the file
/// output. Returns the path used: `logs/latest.log`, or `logs/latest-<pid>.log`
/// when that cannot be had (a second copy of the game holds it).
pub(crate) fn open(folder: &Path) -> io::Result<PathBuf> {
  let logs = folder.join("logs");
  fs::create_dir_all(&logs)?;
  let latest = logs.join("latest.log");
  let rotated = match fs::rename(&latest, logs.join("previous.log")) {
    Ok(()) => Ok(()),
    // The first run has nothing to rotate
    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error),
  };
  let (file, path) = match rotated.and_then(|()| File::create(&latest)) {
    Ok(file) => (file, latest),
    Err(_) => {
      let fallback = logs.join(format!("latest-{}.log", std::process::id()));
      (File::create(&fallback)?, fallback)
    }
  };
  let output = FileOutput {
    writer: BufWriter::with_capacity(BUFFER, file),
    written: 0,
    full: false,
    unflushed: false,
    last_flush: None,
    #[cfg(test)]
    owner: std::thread::current().id(),
  };
  *STATE.lock().unwrap_or_else(PoisonError::into_inner) = Some(output);
  OPEN.store(true, Ordering::Release);
  Ok(path)
}

/// Writes the record's line. A failed write detaches the file, says so once
/// on stderr, and the game goes on without it.
pub(super) fn write(record: &LogRecord) {
  if !OPEN.load(Ordering::Acquire) {
    return;
  }
  let Some(mut held) = lock() else { return };
  let Some(output) = held.output() else { return };
  #[cfg(test)]
  if output.owner != std::thread::current().id() {
    return;
  }
  let urgent = record.level <= log::Level::Warn;
  if let Err(error) = output.write_line(record, urgent) {
    held.detach(&error);
  }
}

impl FileOutput {
  fn write_line(&mut self, record: &LogRecord, urgent: bool) -> io::Result<()> {
    if self.full {
      return Ok(());
    }
    let line = format_line(record);
    if self.written + line.len() as u64 > CAP {
      self.full = true;
      self.writer.write_all(FULL_LINE.as_bytes())?;
      self.written += FULL_LINE.len() as u64;
      self.unflushed = true;
      return self.flush_now();
    }
    self.writer.write_all(line.as_bytes())?;
    self.written += line.len() as u64;
    self.unflushed = true;
    if urgent && self.gap_has_passed() {
      self.flush_now()?;
    }
    Ok(())
  }

  fn gap_has_passed(&self) -> bool {
    self
      .last_flush
      .is_none_or(|last| last.elapsed() >= FLUSH_GAP)
  }

  fn flush_now(&mut self) -> io::Result<()> {
    #[cfg(test)]
    FLUSHES.fetch_add(1, Ordering::Relaxed);
    self.last_flush = Some(Instant::now());
    self.unflushed = false;
    self.writer.flush()
  }
}

/// `HH:MM:SS.mmm LEVEL target: message`, the time since the logger started.
fn format_line(record: &LogRecord) -> String {
  let time = record.time;
  let secs = time.as_secs();
  format!(
    "{:02}:{:02}:{:02}.{:03} {} {}: {}\n",
    secs / 3600,
    secs / 60 % 60,
    secs % 60,
    time.subsec_millis(),
    record.level,
    record.target,
    record.message
  )
}

/// Once a frame: flushes what is waiting once the gap since the last flush has
/// passed (a quiet game's last lines reach the disk within a frame of it).
pub(crate) fn tick() {
  if !OPEN.load(Ordering::Acquire) {
    return;
  }
  let Some(mut held) = lock() else { return };
  let Some(output) = held.output() else { return };
  if output.unflushed
    && output.gap_has_passed()
    && let Err(error) = output.flush_now()
  {
    held.detach(&error);
  }
}

/// At exit and from the panic hook: everything waiting goes to disk.
pub(crate) fn flush() {
  if !OPEN.load(Ordering::Acquire) {
    return;
  }
  let Some(mut held) = lock() else { return };
  let Some(output) = held.output() else { return };
  if output.unflushed
    && let Err(error) = output.flush_now()
  {
    held.detach(&error);
  }
}

/// Flushes and detaches the output.
#[cfg(test)]
pub(crate) fn close() {
  let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
  if let Some(output) = state.as_mut() {
    let _ = output.writer.flush();
  }
  *state = None;
  OPEN.store(false, Ordering::Release);
}

#[cfg(test)]
fn is_open() -> bool {
  OPEN.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::logging::LogRecord;
  use crate::core::logging::Origin;
  use std::sync::{Mutex, MutexGuard, PoisonError};
  use std::time::Duration;

  static LOCK: Mutex<()> = Mutex::new(());

  /// The tests share the process-wide file output: one at a time, and each
  /// closes the output before it lets go
  struct Guard(#[allow(dead_code)] MutexGuard<'static, ()>);

  fn exclusive() -> Guard {
    let guard = Guard(LOCK.lock().unwrap_or_else(PoisonError::into_inner));
    close();
    guard
  }

  impl Drop for Guard {
    fn drop(&mut self) {
      close();
    }
  }

  fn record(level: log::Level, message: &str) -> LogRecord {
    LogRecord {
      seq: 1,
      level,
      target: "game::test".into(),
      origin: Origin::Game,
      message: message.into(),
      time: Duration::from_millis(3_723_045),
      frame: None,
      scope: 0,
    }
  }

  fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap()
  }

  #[test]
  fn the_previous_run_is_kept_beside_the_new_one() {
    let _guard = exclusive();
    let dir = tempfile::tempdir().unwrap();

    let first = open(dir.path()).unwrap();
    assert_eq!(first, dir.path().join("logs/latest.log"));
    write(&record(log::Level::Warn, "from the first run"));
    close();

    let second = open(dir.path()).unwrap();
    assert_eq!(second, first);
    write(&record(log::Level::Warn, "from the second run"));
    flush();
    close();

    let previous = read(&dir.path().join("logs/previous.log"));
    let latest = read(&second);
    assert!(previous.contains("from the first run"), "{previous}");
    assert!(!previous.contains("from the second run"), "{previous}");
    assert!(latest.contains("from the second run"), "{latest}");
    assert!(!latest.contains("from the first run"), "{latest}");
    // The record's own time, as HH:MM:SS.mmm (other tests' lines may be
    // among them)
    assert!(
      latest.contains("01:02:03.045 WARN game::test: from the second run\n"),
      "{latest}"
    );
  }

  #[test]
  fn a_warning_is_on_disk_at_once_and_an_info_line_after_a_tick() {
    let _guard = exclusive();
    let dir = tempfile::tempdir().unwrap();
    let path = open(dir.path()).unwrap();

    write(&record(log::Level::Info, "a note"));
    assert!(
      !read(&path).contains("a note"),
      "an info line waits in the buffer"
    );
    std::thread::sleep(FLUSH_GAP * 2);
    tick();
    assert!(read(&path).contains("a note"));

    std::thread::sleep(FLUSH_GAP * 2);
    write(&record(log::Level::Warn, "a problem"));
    assert!(read(&path).contains("a problem"), "on disk at once");
    write(&record(log::Level::Error, "an error right after"));
    // Within the gap of the last flush: left for the tick
    std::thread::sleep(FLUSH_GAP * 2);
    tick();
    assert!(read(&path).contains("an error right after"));
  }

  #[test]
  fn a_flood_flushes_at_most_once_a_gap_and_stops_at_the_cap() {
    let _guard = exclusive();
    let dir = tempfile::tempdir().unwrap();
    let path = open(dir.path()).unwrap();
    let message = "x".repeat(100);
    FLUSHES.store(0, std::sync::atomic::Ordering::Relaxed);

    for _ in 0..100_000 {
      write(&record(log::Level::Warn, &message));
    }

    let flushes = FLUSHES.load(std::sync::atomic::Ordering::Relaxed);
    assert!(flushes < 100_000 / 10, "{flushes} flushes");
    let length = std::fs::metadata(&path).unwrap().len();
    assert!(length <= CAP + 200, "{length} bytes");
    let text = read(&path);
    assert!(
      text.ends_with("log full: no more is written this run\n"),
      "{}",
      &text[text.len().saturating_sub(200)..]
    );
    assert_eq!(text.matches("log full").count(), 1);
  }

  #[test]
  fn a_folder_that_cannot_be_written_gives_no_file_and_one_error() {
    let _guard = exclusive();
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("a-regular-file");
    std::fs::write(&blocker, "").unwrap();

    let result = open(&blocker.join("saves"));
    assert!(result.is_err());
    assert!(!is_open(), "no file output");
    // The game goes on logging
    write(&record(log::Level::Warn, "nowhere to go"));
    flush();
  }

  #[test]
  fn a_held_latest_log_falls_back_to_a_pid_name() {
    let _guard = exclusive();
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    // `latest.log` cannot be moved or opened as a file
    std::fs::create_dir_all(logs.join("latest.log")).unwrap();
    std::fs::create_dir_all(logs.join("previous.log/in-the-way")).unwrap();

    let path = open(dir.path()).unwrap();
    assert_eq!(
      path,
      logs.join(format!("latest-{}.log", std::process::id()))
    );
    write(&record(log::Level::Warn, "in the fallback"));
    flush();
    assert!(read(&path).contains("in the fallback"));
  }

  #[test]
  fn a_panic_is_written_before_the_process_ends() {
    let _guard = exclusive();
    let capture = crate::core::logging::capture_log();
    let dir = tempfile::tempdir().unwrap();
    let path = open(dir.path()).unwrap();
    super::super::panic::install_hook();

    let result = std::panic::catch_unwind(|| panic!("the sky fell"));
    assert!(result.is_err());

    let text = read(&path);
    assert!(text.contains("ERROR ryo2d::panic: panic at "), "{text}");
    assert!(text.contains("file.rs:"), "{text}");
    assert!(text.contains("the sky fell"), "{text}");
    drop(capture);
  }

  #[test]
  fn a_panic_while_the_file_is_locked_neither_panics_nor_hangs() {
    let _guard = exclusive();
    let dir = tempfile::tempdir().unwrap();
    let path = open(dir.path()).unwrap();
    super::super::panic::install_hook();

    // This thread holds the file's lock, as a thread that panicked in a write
    let held = lock();
    let result = std::panic::catch_unwind(|| panic!("while locked"));
    assert!(result.is_err());
    drop(held);
    assert!(!read(&path).contains("while locked"));
  }
}
