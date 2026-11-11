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
/// The buffer between the lines and the disk. The flushes bound how long a
/// line waits; the size only sets how often a flood pays for a write call
/// (with 8 KiB, every couple of hundred lines: a third of a line's cost).
const BUFFER: usize = 64 * 1024;
/// The last line of a full file.
const FULL_LINE: &str = "log full: no more is written this run\n";
/// The most the reused line buffer keeps between lines: a longer line's
/// buffer is let go after it is written
const LINE_KEEP: usize = 4 * 1024;

struct FileOutput {
  writer: BufWriter<File>,
  /// Bytes written (buffered ones too)
  written: u64,
  full: bool,
  unflushed: bool,
  /// When the last flush was, by the clock (`tick`'s gap)
  last_flush: Option<Instant>,
  /// When the last flush was, in the log's time (a record's): a warning's
  /// gap is read from its own time, so a line reads no clock of its own
  last_flush_at: Option<Duration>,
  /// The line being written, reused from line to line
  line: String,
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

/// A record kept for a file not yet open.
struct EarlyRecord {
  record: LogRecord,
  /// stderr was to take it but was off: should the run end with no file, it
  /// is printed then
  owed: bool,
}

/// The newest [`EARLY`] records logged before a file opened; `None` once a
/// file has opened or the run has said it will open none.
static EARLY_RECORDS: Mutex<Option<VecDeque<EarlyRecord>>> = Mutex::new(Some(VecDeque::new()));
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
/// when another copy of the game holds that or it cannot be had (said in
/// one info line).
pub(crate) fn open(folder: &Path) -> io::Result<PathBuf> {
  let (file, path, why) = create(folder)?;
  let mut output = FileOutput {
    writer: BufWriter::with_capacity(BUFFER, file),
    written: 0,
    full: false,
    unflushed: false,
    last_flush: None,
    last_flush_at: None,
    line: String::new(),
    #[cfg(test)]
    owner: std::thread::current().id(),
  };
  // The file's lock, then the early buffer's (the documented order). The
  // records kept go first; a record logged meanwhile is kept (the buffer's
  // lock is ours, so it waits and then finds the file open) or finds the
  // file open and waits for the file's lock: `OPEN` is set before the
  // buffer is said to be gone, so no record is dropped between them
  let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
  let mut early = early_records();
  let kept = early.take();
  let written = kept.map_or(Ok(()), |kept| {
    kept
      .iter()
      .try_for_each(|early| output.write_line(&early.record, false))
  });
  *state = Some(output);
  OPEN.store(true, Ordering::Release);
  EARLY_ON.store(false, Ordering::Release);
  drop(early);
  drop(state);
  if let Err(error) = written {
    if let Some(held) = lock() {
      held.detach(&error);
    }
    return Err(error);
  }
  if let Some(why) = why {
    log::info!("{why}; this run writes {}", path.display());
  }
  Ok(path)
}

/// Rotates the last run's file and creates this run's: the file, its path,
/// and why it is not `latest.log` when it is not. A copy of the game that
/// finds `latest.log` held by another (two local copies, as netplay is
/// tested) leaves it and the previous run alone and writes
/// `latest-<pid>.log`; a copy that has `latest.log` removes the pid files no
/// running copy holds.
fn create(folder: &Path) -> io::Result<(File, PathBuf, Option<String>)> {
  let logs = folder.join("logs");
  fs::create_dir_all(&logs)?;
  let latest = logs.join("latest.log");
  let fallback = logs.join(format!("latest-{}.log", std::process::id()));
  if held_by_another(&latest) {
    let file = create_held(&fallback)?;
    let why = "another copy of the game is writing logs/latest.log".to_owned();
    return Ok((file, fallback, Some(why)));
  }
  let rotated = match fs::rename(&latest, logs.join("previous.log")) {
    Ok(()) => Ok(()),
    // The first run has nothing to rotate
    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error),
  };
  match rotated.and_then(|()| create_held(&latest)) {
    Ok(file) => {
      remove_unheld_pid_files(&logs);
      Ok((file, latest, None))
    }
    Err(error) => {
      let file = create_held(&fallback)?;
      let why = format!("logs/latest.log could not be had ({error})");
      Ok((file, fallback, Some(why)))
    }
  }
}

/// Creates (or empties) a log file, held for as long as it is open, so a
/// copy of the game starting meanwhile sees it is in use
/// ([`held_by_another`]). On Unix an advisory lock, which keeps no reader
/// out. On Windows a lock would (its locks are mandatory: the file could
/// not be read while the game runs), so the file is opened without
/// `FILE_SHARE_DELETE` instead: it can be read, but no other process can
/// move or remove it while it is open.
fn create_held(path: &Path) -> io::Result<File> {
  #[cfg(windows)]
  let file = {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;
    fs::OpenOptions::new()
      .write(true)
      .create(true)
      .truncate(true)
      .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
      .open(path)?
  };
  #[cfg(not(windows))]
  let file = File::create(path)?;
  // A file system with no locks still gets the file, held or not
  #[cfg(unix)]
  let _ = file.try_lock();
  Ok(file)
}

/// Whether another process holds the log file at `path` ([`create_held`]).
fn held_by_another(path: &Path) -> bool {
  #[cfg(unix)]
  {
    File::open(path).is_ok_and(|file| matches!(file.try_lock(), Err(fs::TryLockError::WouldBlock)))
  }
  #[cfg(windows)]
  {
    use std::os::windows::fs::OpenOptionsExt;
    // Asking for the right to delete it, as a move does, is refused while
    // its holder shares no deletion
    const DELETE: u32 = 0x0001_0000;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    fs::OpenOptions::new()
      .access_mode(DELETE)
      .open(path)
      .is_err_and(|error| error.raw_os_error() == Some(ERROR_SHARING_VIOLATION))
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = path;
    false
  }
}

/// Removes the `latest-<pid>.log` files in `logs` that no running copy of
/// the game holds: each was a copy's while `latest.log` was another's.
fn remove_unheld_pid_files(logs: &Path) {
  let Ok(entries) = fs::read_dir(logs) else {
    return;
  };
  for entry in entries.flatten() {
    let name = entry.file_name();
    let is_pid_file = name
      .to_str()
      .and_then(|name| name.strip_prefix("latest-")?.strip_suffix(".log"))
      .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()));
    let path = entry.path();
    if is_pid_file && !held_by_another(&path) {
      let _ = fs::remove_file(&path);
    }
  }
}

fn early_records() -> MutexGuard<'static, Option<VecDeque<EarlyRecord>>> {
  EARLY_RECORDS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What [`keep_early`] did with a record.
enum Early {
  /// Kept for the file; `shown` is whether stderr takes it now (else it is
  /// owed to stderr, should the run end with no file)
  Kept { shown: bool },
  /// Not kept: no file will open
  Dropped,
  /// A file opened since the caller looked: the record is the file's
  ForFile,
}

/// Keeps `record` for a file not yet open. `may_show` is whether stderr
/// may take it (it is not captured); whether it does is read here, under
/// the buffer's lock, so a record is never both owed to stderr and printed.
fn keep_early(record: &LogRecord, may_show: bool) -> Early {
  if !EARLY_ON.load(Ordering::Acquire) {
    return if OPEN.load(Ordering::Acquire) {
      Early::ForFile
    } else {
      Early::Dropped
    };
  }
  #[cfg(test)]
  if !KEEPS_EARLY.with(std::cell::Cell::get) {
    return Early::Dropped;
  }
  let copy = record.clone();
  let mut early = early_records();
  if OPEN.load(Ordering::Acquire) {
    return Early::ForFile;
  }
  let Some(kept) = early.as_mut() else {
    return Early::Dropped;
  };
  let shown = may_show && super::outputs::stderr_enabled();
  if kept.len() >= EARLY {
    kept.pop_front();
  }
  kept.push_back(EarlyRecord {
    record: copy,
    owed: may_show && !shown,
  });
  Early::Kept { shown }
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

/// The run ends its start-up with no file of its own (no save folder, or one
/// that cannot be written): stderr is turned on, in every build, and the
/// records kept for a file that stderr did not take go to it first, in
/// order; then the buffer goes and no more are kept.
pub(crate) fn early_to_stderr() {
  #[cfg(test)]
  if !KEEPS_EARLY.with(std::cell::Cell::get) {
    super::outputs::set_stderr(true);
    return;
  }
  // Under the buffer's lock, so a record is either kept (and owed to stderr
  // while it was off) or, after, printed as it is logged
  let kept = {
    let mut early = early_records();
    super::outputs::set_stderr(true);
    EARLY_ON.store(false, Ordering::Release);
    early.take()
  };
  for early in kept.iter().flatten().filter(|early| early.owed) {
    super::outputs::print(&early.record);
  }
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

/// For a test: this thread keeps its records for a file too, adding to
/// those kept, until the guard drops.
#[cfg(test)]
pub(crate) fn keep_early_records_here() -> KeepEarly {
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
/// on stderr, and the game goes on without it. Before a file opens the
/// record is kept for one instead: then it gives back whether stderr takes
/// it (`may_show` is whether it may), read as the record was kept.
pub(super) fn write(record: &LogRecord, may_show: bool) -> Option<bool> {
  if !OPEN.load(Ordering::Acquire) {
    match keep_early(record, may_show) {
      Early::Kept { shown } => return Some(shown),
      Early::Dropped => return None,
      Early::ForFile => {}
    }
  }
  let mut held = lock()?;
  let output = held.output()?;
  #[cfg(test)]
  if !output.takes_this_thread() {
    return None;
  }
  let urgent = record.level <= log::Level::Warn;
  if let Err(error) = output.write_line(record, urgent) {
    held.detach(&error);
  }
  None
}

impl FileOutput {
  /// Tests share the process: a test's file takes its own thread's records,
  /// and those of a thread that keeps records for a file (a file test's)
  #[cfg(test)]
  fn takes_this_thread(&self) -> bool {
    self.owner == std::thread::current().id() || KEEPS_EARLY.with(std::cell::Cell::get)
  }

  fn write_line(&mut self, record: &LogRecord, urgent: bool) -> io::Result<()> {
    if self.full {
      return Ok(());
    }
    let mut line = std::mem::take(&mut self.line);
    format_line(&mut line, record);
    let written = self.write_formatted(&line, record, urgent);
    line.clear();
    if line.capacity() <= LINE_KEEP {
      self.line = line;
    }
    written
  }

  fn write_formatted(&mut self, line: &str, record: &LogRecord, urgent: bool) -> io::Result<()> {
    if self.written + line.len() as u64 > CAP {
      self.full = true;
      self.writer.write_all(FULL_LINE.as_bytes())?;
      self.written += FULL_LINE.len() as u64;
      self.unflushed = true;
      return self.flush_now(record.time);
    }
    self.writer.write_all(line.as_bytes())?;
    self.written += line.len() as u64;
    self.unflushed = true;
    // The gap from the record's own time: no clock is read for a line
    if urgent
      && self
        .last_flush_at
        .is_none_or(|last| record.time.saturating_sub(last) >= FLUSH_GAP)
    {
      self.flush_now(record.time)?;
    }
    Ok(())
  }

  fn gap_has_passed(&self) -> bool {
    self
      .last_flush
      .is_none_or(|last| last.elapsed() >= FLUSH_GAP)
  }

  /// Flushes; `at` is the log's time now (the record's, for a line's flush).
  fn flush_now(&mut self, at: Duration) -> io::Result<()> {
    #[cfg(test)]
    FLUSHES.fetch_add(1, Ordering::Relaxed);
    self.last_flush = Some(Instant::now());
    self.last_flush_at = Some(at);
    self.unflushed = false;
    self.writer.flush()
  }
}

/// Appends `HH:MM:SS.mmm LEVEL target: message` and a newline to `line`, the
/// time since the logger started.
fn format_line(line: &mut String, record: &LogRecord) {
  super::record::push_clock(line, record.time);
  line.push(' ');
  line.push_str(record.level.as_str());
  line.push(' ');
  line.push_str(&record.target);
  line.push_str(": ");
  line.push_str(&record.message);
  line.push('\n');
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
    && let Err(error) = output.flush_now(super::now())
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
    && let Err(error) = output.flush_now(super::now())
  {
    held.detach(&error);
  }
}

/// Flushes and detaches the output (a failed flush is let go: the file is
/// being closed).
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
  fn a_file_line_is_the_clock_and_the_stderr_line() {
    for message in ["", "first line\nsecond line", "café ünïcode ✓ 日本"] {
      let record = record(log::Level::Warn, message);
      let mut line = String::new();
      format_line(&mut line, &record);
      let mut clock = String::new();
      crate::core::logging::record::push_clock(&mut clock, record.time);
      let shared = format!(
        "{clock} {}\n",
        crate::core::logging::outputs::stderr_line(&record)
      );
      assert_eq!(line, shared, "{message:?}");
    }
  }

  #[test]
  fn the_previous_run_is_kept_beside_the_new_one() {
    let _guard = exclusive();
    let dir = tempfile::tempdir().unwrap();

    let first = open(dir.path()).unwrap();
    assert_eq!(first, dir.path().join("logs/latest.log"));
    write(&record(log::Level::Warn, "from the first run"), false);
    close();

    let second = open(dir.path()).unwrap();
    assert_eq!(second, first);
    write(&record(log::Level::Warn, "from the second run"), false);
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

    write(&record(log::Level::Info, "a note"), false);
    assert!(
      !read(&path).contains("a note"),
      "an info line waits in the buffer"
    );
    std::thread::sleep(FLUSH_GAP * 2);
    tick();
    assert!(read(&path).contains("a note"));

    std::thread::sleep(FLUSH_GAP * 2);
    write(&record(log::Level::Warn, "a problem"), false);
    assert!(read(&path).contains("a problem"), "on disk at once");
    write(&record(log::Level::Error, "an error right after"), false);
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
      write(&record(log::Level::Warn, &message), false);
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
  fn a_long_line_is_written_whole_and_its_buffer_is_not_kept() {
    let _guard = exclusive();
    let dir = tempfile::tempdir().unwrap();
    let path = open(dir.path()).unwrap();
    let long = "y".repeat(LINE_KEEP * 4);
    write(&record(log::Level::Info, &long), false);
    write(&record(log::Level::Info, "short"), false);
    let kept = STATE
      .lock()
      .unwrap()
      .as_ref()
      .map(|output| output.line.capacity());
    assert!(kept.is_some_and(|kept| kept <= LINE_KEEP), "{kept:?}");
    flush();
    let text = read(&path);
    assert!(
      text.contains(&format!("01:02:03.045 INFO game::test: {long}\n")),
      "the long line whole"
    );
    assert!(text.contains("01:02:03.045 INFO game::test: short\n"));
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
    write(&record(log::Level::Warn, "nowhere to go"), false);
    flush();
  }

  #[test]
  fn records_logged_before_the_file_opens_are_written_first() {
    let _guard = exclusive();
    let _early = keep_early_records();
    let dir = tempfile::tempdir().unwrap();
    for i in 0..300 {
      write(&record(log::Level::Info, &format!("early {i}")), false);
    }
    let path = open(dir.path()).unwrap();
    write(&record(log::Level::Warn, "after"), false);
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
  fn no_record_is_lost_while_the_file_opens() {
    let _guard = exclusive();
    // Many kept records make the open's own work long
    const KEPT: usize = EARLY - 1;
    for round in 0..50 {
      let dir = tempfile::tempdir().unwrap();
      let ready = std::sync::Arc::new(std::sync::Barrier::new(2));
      // Another thread logs while this one opens the file, until it sees
      // the file open and a few lines after (it keeps records for a file,
      // so a test's file takes its records too)
      let writer = std::thread::spawn({
        let ready = ready.clone();
        move || {
          let _early = keep_early_records();
          ready.wait();
          ready.wait();
          let mut written = 0;
          let mut after = 0;
          while after < 10 && written < 1_000_000 {
            if is_open() {
              after += 1;
            }
            write(
              &record(log::Level::Info, &format!("racing {written}")),
              false,
            );
            written += 1;
          }
          written
        }
      });
      ready.wait();
      let _early = keep_early_records_here();
      for i in 0..KEPT {
        write(&record(log::Level::Info, &format!("kept {i}")), false);
      }
      ready.wait();
      let path = open(dir.path()).unwrap();
      let written = writer.join().unwrap();
      flush();
      close();
      // The early buffer drops its oldest when full, so the racing lines in
      // the file are a run that ends with the last one written: a gap is a
      // lost record
      let text = read(&path);
      let racing: Vec<usize> = text
        .lines()
        .filter_map(|line| line.rsplit_once("racing ")?.1.parse().ok())
        .collect();
      let first = racing.first().copied().unwrap_or(written);
      let expected: Vec<usize> = (first..written).collect();
      assert_eq!(racing, expected, "round {round}: {written} written");
    }
  }

  #[test]
  fn a_second_copy_writes_its_own_file_and_leaves_the_first_s_alone() {
    let _guard = exclusive();
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    // The first copy runs, writing latest.log
    let first = open(dir.path()).unwrap();
    assert_eq!(first, logs.join("latest.log"));
    write(&record(log::Level::Warn, "from the first copy"), false);
    flush();
    std::fs::write(logs.join("previous.log"), "an earlier run\n").unwrap();

    // A second copy starts while it runs
    let (second_file, second, why) = create(dir.path()).unwrap();
    assert!(why.is_some_and(|why| why.contains("another copy")));
    assert_eq!(
      second,
      logs.join(format!("latest-{}.log", std::process::id()))
    );
    assert!(read(&first).contains("from the first copy"));
    assert_eq!(read(&logs.join("previous.log")), "an earlier run\n");

    // Both end; the next start rotates as ever, and the pid file no copy
    // holds goes
    drop(second_file);
    close();
    let third = open(dir.path()).unwrap();
    assert_eq!(third, first);
    assert!(read(&logs.join("previous.log")).contains("from the first copy"));
    assert!(!second.exists(), "a pid file no copy holds is removed");
  }

  #[test]
  fn a_start_removes_the_pid_files_no_running_copy_holds() {
    let _guard = exclusive();
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    // A copy still running that writes a pid file, and one long gone
    let live = logs.join("latest-1.log");
    let _running = create_held(&live).unwrap();
    let stale = logs.join("latest-2.log");
    std::fs::write(&stale, "an old copy's run\n").unwrap();
    let other = logs.join("latest-notes.log");
    std::fs::write(&other, "not a pid file\n").unwrap();

    let (_file, path, why) = create(dir.path()).unwrap();
    assert_eq!(path, logs.join("latest.log"));
    assert!(why.is_none());
    assert!(live.exists(), "a pid file a running copy holds is kept");
    assert!(!stale.exists(), "a pid file no copy holds is removed");
    assert!(other.exists(), "only pid files are removed");
  }

  #[test]
  fn a_held_latest_log_falls_back_to_a_pid_name() {
    let _guard = exclusive();
    let capture = crate::core::logging::capture_log();
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
    write(&record(log::Level::Warn, "in the fallback"), false);
    flush();
    assert!(read(&path).contains("in the fallback"));
    assert_eq!(
      capture.containing_all(&["logs/latest.log could not be had", "this run writes"]),
      1
    );
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
  fn the_panic_hook_s_record_goes_to_the_file_and_not_to_stderr() {
    let _guard = exclusive();
    let dir = tempfile::tempdir().unwrap();
    let path = open(dir.path()).unwrap();
    super::super::panic::install_hook();
    // Rust's own hook prints the panic on stderr right after: the log's
    // record of it goes to the file alone (a scope with no capture, which
    // stderr would take)
    let watch = crate::core::logging::watch_stderr(true);
    let scope = crate::core::logging::scope::new_scope();
    crate::core::logging::in_scope(scope, || {
      let result = std::panic::catch_unwind(|| panic!("a hook's record"));
      assert!(result.is_err());
    });

    let text = read(&path);
    assert!(
      text.contains("ERROR ryo2d::panic: panic at ") && text.contains("a hook's record"),
      "{text}"
    );
    assert!(
      watch
        .lines()
        .iter()
        .all(|line| !line.contains("a hook's record")),
      "{:?}",
      watch.lines()
    );
    assert!(watch.outputs().contains(&crate::core::logging::Taken::File));
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
