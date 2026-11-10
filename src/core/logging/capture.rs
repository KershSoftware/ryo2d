//! Test capture: a guard that collects every record logged in its own scope.

use std::collections::VecDeque;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use super::record::LogRecord;
use super::scope::{new_scope, replace_scope};

/// A capture's buffer, which the outputs push into.
pub(super) struct Sink {
  scope: u64,
  limit: usize,
  seen: AtomicU64,
  records: Mutex<VecDeque<LogRecord>>,
}

impl Sink {
  pub(super) fn push(&self, record: LogRecord) {
    self.seen.fetch_add(1, Ordering::Relaxed);
    let mut records = self.records.lock().unwrap_or_else(PoisonError::into_inner);
    if records.len() >= self.limit {
      records.pop_front();
    }
    records.push_back(record);
  }
}

/// The open captures, and how many there are (so a game, which has none,
/// never takes the lock).
static OPEN: RwLock<Vec<Arc<Sink>>> = RwLock::new(Vec::new());
static OPEN_COUNT: AtomicUsize = AtomicUsize::new(0);

/// The capture of `scope`, if one is open.
pub(super) fn sink_for(scope: u64) -> Option<Arc<Sink>> {
  if scope == 0 || OPEN_COUNT.load(Ordering::Acquire) == 0 {
    return None;
  }
  let open = OPEN.read().unwrap_or_else(PoisonError::into_inner);
  open.iter().find(|sink| sink.scope == scope).cloned()
}

/// Collects the records logged in a fresh scope: the test thread's own, and
/// those of every thread given its scope (an engine's workers) while it is
/// open. Its records skip stderr.
///
/// It sets the scope of the thread that opens it, until it drops, so it stays
/// on that thread.
///
/// ```
/// use ryo2d::testing::capture_log;
///
/// let log = capture_log();
/// ryo2d::warn!("low on coins");
/// assert_eq!(log.warnings(), ["low on coins"]);
/// ```
pub struct LogCapture {
  sink: Arc<Sink>,
  previous: u64,
  _on_its_thread: PhantomData<*const ()>,
}

/// Starts collecting the records of a fresh scope on this thread; see
/// [`LogCapture`]. It installs the engine's logger if nothing has yet.
pub fn capture_log() -> LogCapture {
  open(usize::MAX)
}

/// [`capture_log`], keeping only the newest `limit` records, for a test that
/// floods the log. [`LogCapture::seen`] still counts every one.
pub fn capture_log_bounded(limit: usize) -> LogCapture {
  open(limit.max(1))
}

fn open(limit: usize) -> LogCapture {
  super::ensure();
  let scope = new_scope();
  let sink = Arc::new(Sink {
    scope,
    limit,
    seen: AtomicU64::new(0),
    records: Mutex::new(VecDeque::new()),
  });
  OPEN
    .write()
    .unwrap_or_else(PoisonError::into_inner)
    .push(Arc::clone(&sink));
  OPEN_COUNT.fetch_add(1, Ordering::Release);
  LogCapture {
    sink,
    previous: replace_scope(scope),
    _on_its_thread: PhantomData,
  }
}

impl LogCapture {
  /// The records kept, oldest first.
  pub fn records(&self) -> Vec<LogRecord> {
    let records = self
      .sink
      .records
      .lock()
      .unwrap_or_else(PoisonError::into_inner);
    records.iter().cloned().collect()
  }

  /// How many records at exactly `level` were kept.
  pub fn count(&self, level: log::Level) -> usize {
    self.filtered(|r| r.level == level).len()
  }

  /// The messages of the warnings kept (`Level::Warn` only).
  pub fn warnings(&self) -> Vec<String> {
    self
      .filtered(|r| r.level == log::Level::Warn)
      .into_iter()
      .map(|r| r.message)
      .collect()
  }

  /// How many records kept have `needle` in their message.
  pub fn containing(&self, needle: &str) -> usize {
    self.filtered(|r| r.message.contains(needle)).len()
  }

  /// The records kept whose target starts with `prefix`.
  pub fn from_target(&self, prefix: &str) -> Vec<LogRecord> {
    self.filtered(|r| r.target.starts_with(prefix))
  }

  /// The scope it collects, which every record it keeps carries.
  pub fn scope(&self) -> u64 {
    self.sink.scope
  }

  /// Every record it has collected, the ones a bounded capture dropped too.
  pub fn seen(&self) -> u64 {
    self.sink.seen.load(Ordering::Relaxed)
  }

  fn filtered(&self, keep: impl Fn(&LogRecord) -> bool) -> Vec<LogRecord> {
    let records = self
      .sink
      .records
      .lock()
      .unwrap_or_else(PoisonError::into_inner);
    records.iter().filter(|r| keep(r)).cloned().collect()
  }
}

impl Drop for LogCapture {
  fn drop(&mut self) {
    replace_scope(self.previous);
    let mut open = OPEN.write().unwrap_or_else(PoisonError::into_inner);
    open.retain(|sink| !Arc::ptr_eq(sink, &self.sink));
    OPEN_COUNT.fetch_sub(1, Ordering::Release);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::logging::scope::{current_scope, in_scope};

  #[test]
  fn capture_collects_its_own_scope_only() {
    let threads: Vec<_> = (0..2)
      .map(|t| {
        std::thread::spawn(move || {
          let capture = capture_log();
          for i in 0..100 {
            log::warn!(target: "ryo2d::core::logging::capture_test", "thread {t} line {i}");
          }
          (
            capture.scope(),
            capture.records(),
            capture.count(log::Level::Warn),
            capture.warnings().len(),
            capture.from_target("ryo2d::core::logging").len(),
          )
        })
      })
      .collect();

    let mut scopes = Vec::new();
    for (t, thread) in threads.into_iter().enumerate() {
      let (scope, records, warns, warnings, from_target) = thread.join().expect("a capture thread");
      assert_eq!(records.len(), 100);
      assert_eq!((warns, warnings, from_target), (100, 100, 100));
      let prefix = format!("thread {t} ");
      assert!(
        records
          .iter()
          .all(|r| r.scope == scope && r.message.starts_with(&prefix))
      );
      scopes.push(scope);
    }
    assert_ne!(scopes[0], scopes[1]);
  }

  #[test]
  fn a_thread_given_a_scope_logs_into_it() {
    let capture = capture_log();
    let scope = capture.scope();
    let after = std::thread::spawn(move || {
      in_scope(
        scope,
        || log::warn!(target: "game::worker", "from a worker"),
      );
      current_scope()
    })
    .join()
    .expect("a worker thread");
    assert_eq!(after, 0, "in_scope gives back the thread's scope");
    assert_eq!(capture.containing("from a worker"), 1);
    assert_eq!(capture.records()[0].scope, scope);
  }

  #[test]
  fn a_capture_gives_back_the_scope_before_it() {
    let before = current_scope();
    {
      let outer = capture_log();
      assert_eq!(current_scope(), outer.scope());
      {
        let inner = capture_log();
        assert_eq!(current_scope(), inner.scope());
      }
      assert_eq!(current_scope(), outer.scope());
    }
    assert_eq!(current_scope(), before);
  }

  #[test]
  fn a_bounded_capture_keeps_its_newest() {
    let capture = capture_log_bounded(2);
    for i in 0..5 {
      log::warn!(target: "game::bounded", "line {i}");
    }
    let messages: Vec<_> = capture.records().into_iter().map(|r| r.message).collect();
    assert_eq!(messages, ["line 3", "line 4"]);
    assert_eq!(capture.seen(), 5);
  }
}
