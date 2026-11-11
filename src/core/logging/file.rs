//! The log file: `logs/latest.log` in the save folder, one line per record.
//!
//! The output is process-wide, like the logger. It has its own mutex, held
//! only for its own write and flush, so no other output waits on it. Native
//! only: the browser has its console.
//!
//! Until a file opens it keeps the newest [`EARLY`] records, and writes them
//! first when one does, so a warning from before the engine is built (a bad
//! Ryo.toml, the builder) still reaches the file. A run that will open no
//! file (headless) drops them.

use std::collections::VecDeque;
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

/// The records kept for a file not yet open.
pub(crate) const EARLY: usize = 256;
/// The newest [`EARLY`] records logged before a file opened; `None` once a
/// file has opened or the run has said it will open none.
static EARLY_RECORDS: Mutex<Option<VecDeque<LogRecord>>> = Mutex::new(Some(VecDeque::new()));
/// Whether [`EARLY_RECORDS`] still keeps records, so a run that dropped them
/// costs one atomic read a record.
static EARLY_ON: AtomicBool = AtomicBool::new(true);

#[cfg(test)]
thread_local! {
  /// Tests share the process: only a thread that asked keeps its records
  /// for a file, and only it can drop them
  static KEEPS_EARLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

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
  let (file, path) = create(folder)?;
  let mut output = FileOutput {
    writer: BufWriter::with_capacity(BUFFER, file),
    written: 0,
    full: false,
    unflushed: false,
    last_flush: None,
    #[cfg(test)]
    owner: std::thread::current().id(),
  };
  let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
  // The records kept go first; a record logged meanwhile waits for the
  // file's lock, so none comes between them
  let mut early = early_records();
  let kept = early.take();
  EARLY_ON.store(false, Ordering::Release);
  let written = kept.map_or(Ok(()), |kept| {
    kept
      .iter()
      .try_for_each(|record| output.write_line(record, false))
  });
  *state = Some(output);
  OPEN.store(true, Ordering::Release);
  drop(early);
  drop(state);
  if let Err(error) = written {
    if let Some(held) = lock() {
      held.detach(&error);
    }
    return Err(error);
  }
  Ok(path)
}

/// Rotates the last run's file and creates this run's.
fn create(folder: &Path) -> io::Result<(File, PathBuf)> {
  let logs = folder.join("logs");
  fs::create_dir_all(&logs)?;
  let latest = logs.join("latest.log");
  let rotated = match fs::rename(&latest, logs.join("previous.log")) {
    Ok(()) => Ok(()),
    // The first run has nothing to rotate
    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error),
  };
  match rotated.and_then(|()| File::create(&latest)) {
    Ok(file) => Ok((file, latest)),
    Err(_) => {
      let fallback = logs.join(format!("latest-{}.log", std::process::id()));
      Ok((File::create(&fallback)?, fallback))
    }
  }
}

fn early_records() -> MutexGuard<'static, Option<VecDeque<LogRecord>>> {
  EARLY_RECORDS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Keeps `record` for a file not yet open. False when a file opened since
/// the caller looked, so the record is the file's to write.
fn keep_early(record: &LogRecord) -> bool {
  if !EARLY_ON.load(Ordering::Acquire) {
    return !OPEN.load(Ordering::Acquire);
  }
  #[cfg(test)]
  if !KEEPS_EARLY.with(std::cell::Cell::get) {
    return true;
  }
  let copy = record.clone();
  let mut early = early_records();
  if OPEN.load(Ordering::Acquire) {
    return false;
  }
  if let Some(kept) = early.as_mut() {
    if kept.len() >= EARLY {
      kept.pop_front();
    }
    kept.push_back(copy);
  }
  true
}

/// The run will open no file (a headless engine, or one that could not be
/// had): the records kept for one go, and no more are kept.
pub(crate) fn drop_early() {
  #[cfg(test)]
  if !KEEPS_EARLY.with(std::cell::Cell::get) {
    return;
  }
  let mut early = early_records();
  EARLY_ON.store(false, Ordering::Release);
  *early = None;
}

/// Whether records are still kept for a file not yet open
#[cfg(test)]
pub(crate) fn early_kept() -> bool {
  EARLY_ON.load(Ordering::Acquire)
}

/// For a test: this thread's records are kept for a file again, from none,
/// until the guard drops.
#[cfg(test)]
pub(crate) fn keep_early_records() -> KeepEarly {
  let mut early = early_records();
  *early = Some(VecDeque::new());
  EARLY_ON.store(true, Ordering::Release);
  KEEPS_EARLY.with(|flag| flag.set(true));
  KeepEarly
}

/// Ends [`keep_early_records`] for its thread.
#[cfg(test)]
pub(crate) struct KeepEarly;

#[cfg(test)]
impl Drop for KeepEarly {
  fn drop(&mut self) {
    KEEPS_EARLY.with(|flag| flag.set(false));
  }
}

/// The file tests' turn: they share the process-wide output (and the records
/// kept for it), so they run one at a time
#[cfg(test)]
pub(crate) fn test_turn() -> MutexGuard<'static, ()> {
  static TURN: Mutex<()> = Mutex::new(());
  TURN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Writes the record's line. A failed write detaches the file, says so once
/// on stderr, and the game goes on without it.
pub(super) fn write(record: &LogRecord) {
  if !OPEN.load(Ordering::Acquire) && keep_early(record) {
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
  format!(
    "{} {} {}: {}\n",
    super::record::clock(record.time),
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
  use std::sync::MutexGuard;
  use std::time::Duration;

  /// The tests share the process-wide file output: one at a time, and each
  /// closes the output before it lets go
  struct Guard(#[allow(dead_code)] MutexGuard<'static, ()>);

  fn exclusive() -> Guard {
    let guard = Guard(test_turn());
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
  fn records_logged_before_the_file_opens_are_written_first() {
    let _guard = exclusive();
    let _early = keep_early_records();
    let dir = tempfile::tempdir().unwrap();
    for i in 0..300 {
      write(&record(log::Level::Info, &format!("early {i}")));
    }
    let path = open(dir.path()).unwrap();
    write(&record(log::Level::Warn, "after"));
    flush();

    let text = read(&path);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), EARLY + 1, "the newest {EARLY}, then the rest");
    assert!(
      lines[0].ends_with("INFO game::test: early 44"),
      "{}",
      lines[0]
    );
    for (n, line) in lines[..EARLY].iter().enumerate() {
      assert!(line.ends_with(&format!(": early {}", 44 + n)), "{line}");
    }
    assert!(lines[EARLY].ends_with("WARN game::test: after"));
    assert!(!early_kept(), "an open file keeps no buffer");
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
