//! A save store held in memory, which can also inject faults for tests.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::replace::pause;
use super::storage::FileOps;

/// A save store held in memory: what is saved to it never touches the disk.
///
/// Clones share one store, so a test can keep a clone and plant or read files
/// through it by their names (`slot1.ryo-save`, `system.json`).
#[derive(Clone, Debug, Default)]
pub struct MemorySaves {
  state: Arc<Mutex<State>>,
}

#[derive(Debug, Default)]
struct State {
  files: BTreeMap<String, Vec<u8>>,
  faults: Vec<Fault>,
  denied_renames: usize,
  per_op: Duration,
  ops: usize,
  writes: usize,
}

/// A file operation a fault can be set on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FaultOp {
  Write,
  Rename,
  Remove,
  SyncDir,
}

#[derive(Debug)]
struct Fault {
  op: FaultOp,
  /// Calls of `op` since the fault was set
  seen: usize,
  nth: usize,
  kind: FaultKind,
}

#[derive(Debug, Clone, Copy)]
enum FaultKind {
  /// The nth call fails
  Once,
  /// Every call from the nth on fails
  From,
  /// The nth write stores only its first `keep` bytes, then fails
  Torn { keep: usize },
}

/// What the faults do to one call
enum Trip {
  Fail,
  Torn { keep: usize },
}

impl State {
  fn trip(&mut self, op: FaultOp) -> Option<Trip> {
    let mut trip = None;
    for fault in self.faults.iter_mut().filter(|fault| fault.op == op) {
      let call = fault.seen;
      fault.seen += 1;
      match fault.kind {
        FaultKind::From if call >= fault.nth => trip = trip.or(Some(Trip::Fail)),
        FaultKind::Once if call == fault.nth => trip = trip.or(Some(Trip::Fail)),
        // A torn write leaves more to see than a plain failure, so it wins
        FaultKind::Torn { keep } if call == fault.nth => trip = Some(Trip::Torn { keep }),
        _ => {}
      }
    }
    self
      .faults
      .retain(|fault| matches!(fault.kind, FaultKind::From) || fault.seen <= fault.nth);
    trip
  }

  fn add(&mut self, op: FaultOp, nth: usize, kind: FaultKind) {
    self.faults.push(Fault {
      op,
      seen: 0,
      nth,
      kind,
    });
  }
}

fn injected(what: &str) -> io::Error {
  io::Error::other(format!("injected {what} fault"))
}

impl MemorySaves {
  /// An empty store
  pub fn new() -> MemorySaves {
    MemorySaves::default()
  }

  /// Puts a file in the store, replacing any of that name
  pub fn put(&self, name: &str, bytes: impl Into<Vec<u8>>) {
    self.lock().files.insert(name.to_string(), bytes.into());
  }

  /// The file's bytes, if the store has it
  pub fn get(&self, name: &str) -> Option<Vec<u8>> {
    self.lock().files.get(name).cloned()
  }

  /// Every file name in the store, sorted
  pub fn names(&self) -> Vec<String> {
    self.lock().files.keys().cloned().collect()
  }

  /// Removes a file from the store, if it is there
  pub fn remove(&self, name: &str) {
    self.lock().files.remove(name);
  }

  /// The `nth` call of `op` (0-based, counted from now) fails
  pub(crate) fn fail(&self, op: FaultOp, nth: usize) {
    self.lock().add(op, nth, FaultKind::Once);
  }

  /// Every call of `op` from the `nth` on (0-based, counted from now) fails
  pub(crate) fn fail_from(&self, op: FaultOp, nth: usize) {
    self.lock().add(op, nth, FaultKind::From);
  }

  /// The `nth` write (0-based, counted from now) stores only its first `keep`
  /// bytes, then fails, as a crash part way through it would leave the file
  pub(crate) fn crash_write(&self, nth: usize, keep: usize) {
    self
      .lock()
      .add(FaultOp::Write, nth, FaultKind::Torn { keep });
  }

  /// The next `times` renames fail with `PermissionDenied`, as when another
  /// process holds the file
  pub(crate) fn deny_rename(&self, times: usize) {
    self.lock().denied_renames = times;
  }

  /// Every file operation takes at least `per_op`
  pub(crate) fn slow(&self, per_op: Duration) {
    self.lock().per_op = per_op;
  }

  /// File operations run so far (direct access through `put`, `get`, `names`
  /// and `remove` is not counted)
  pub(crate) fn ops(&self) -> usize {
    self.lock().ops
  }

  /// Writes tried so far, failed ones included
  pub(crate) fn writes(&self) -> usize {
    self.lock().writes
  }

  fn lock(&self) -> MutexGuard<'_, State> {
    self.state.lock().unwrap_or_else(PoisonError::into_inner)
  }

  /// Counts an operation and waits out the store's slowness, outside the lock
  fn begin(&self) -> MutexGuard<'_, State> {
    let per_op = {
      let mut state = self.lock();
      state.ops += 1;
      state.per_op
    };
    if !per_op.is_zero() {
      pause(per_op);
    }
    self.lock()
  }
}

impl FileOps for MemorySaves {
  fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
    Ok(self.begin().files.get(name).cloned())
  }

  fn write_synced(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
    let mut state = self.begin();
    state.writes += 1;
    match state.trip(FaultOp::Write) {
      Some(Trip::Fail) => Err(injected("write")),
      Some(Trip::Torn { keep }) => {
        let torn = bytes[..keep.min(bytes.len())].to_vec();
        state.files.insert(name.to_string(), torn);
        Err(injected("torn write"))
      }
      None => {
        state.files.insert(name.to_string(), bytes.to_vec());
        Ok(())
      }
    }
  }

  fn rename(&self, from: &str, to: &str) -> io::Result<()> {
    let mut state = self.begin();
    if state.trip(FaultOp::Rename).is_some() {
      return Err(injected("rename"));
    }
    if state.denied_renames > 0 {
      state.denied_renames -= 1;
      return Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("\"{from}\" is held by another process"),
      ));
    }
    let bytes = state
      .files
      .remove(from)
      .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no file \"{from}\"")))?;
    state.files.insert(to.to_string(), bytes);
    Ok(())
  }

  fn remove(&self, name: &str) -> io::Result<()> {
    let mut state = self.begin();
    if state.trip(FaultOp::Remove).is_some() {
      return Err(injected("remove"));
    }
    state.files.remove(name);
    Ok(())
  }

  fn names(&self) -> io::Result<Vec<String>> {
    Ok(self.begin().files.keys().cloned().collect())
  }

  fn sync_dir(&self) -> io::Result<()> {
    let mut state = self.begin();
    match state.trip(FaultOp::SyncDir) {
      Some(_) => Err(injected("folder sync")),
      None => Ok(()),
    }
  }
}

#[cfg(test)]
mod tests {
  use std::time::Instant;

  use super::*;

  #[test]
  fn the_store_keeps_what_is_put_in_it() {
    let store = MemorySaves::new();
    let shared = store.clone();
    store.put("b", b"2");
    store.put("a", vec![1u8]);
    assert_eq!(shared.get("b").as_deref(), Some(&b"2"[..]));
    assert_eq!(shared.names(), ["a", "b"].map(String::from));
    shared.remove("a");
    assert_eq!(store.get("a"), None);
    assert_eq!(store.ops(), 0, "direct access is not a storage op");
  }

  #[test]
  fn a_fault_fires_at_the_nth_call_from_when_it_is_set() {
    let store = MemorySaves::new();
    FileOps::write_synced(&store, "before", b"x").unwrap();
    store.fail(FaultOp::Write, 1);
    assert!(FileOps::write_synced(&store, "a", b"x").is_ok());
    assert!(FileOps::write_synced(&store, "b", b"x").is_err());
    assert!(FileOps::write_synced(&store, "c", b"x").is_ok());
    assert_eq!(store.get("b"), None);
    assert_eq!(store.writes(), 4);
    assert_eq!(store.ops(), 4);
  }

  #[test]
  fn a_fault_from_the_nth_call_keeps_failing() {
    let store = MemorySaves::new();
    store.put("a", b"x");
    store.fail_from(FaultOp::Rename, 1);
    assert!(FileOps::rename(&store, "a", "b").is_ok());
    for _ in 0..3 {
      assert!(FileOps::rename(&store, "b", "c").is_err());
    }
    assert_eq!(store.get("b").as_deref(), Some(&b"x"[..]));

    store.fail_from(FaultOp::Remove, 0);
    assert!(FileOps::remove(&store, "b").is_err());
    store.fail(FaultOp::SyncDir, 0);
    assert!(FileOps::sync_dir(&store).is_err());
    assert!(FileOps::sync_dir(&store).is_ok());
  }

  #[test]
  fn a_crashed_write_keeps_only_its_first_bytes() {
    let store = MemorySaves::new();
    store.crash_write(0, 2);
    assert!(FileOps::write_synced(&store, "a", b"abcdef").is_err());
    assert_eq!(store.get("a").as_deref(), Some(&b"ab"[..]));
  }

  #[test]
  fn a_denied_rename_is_permission_denied_until_its_count_runs_out() {
    let store = MemorySaves::new();
    store.put("a", b"x");
    store.deny_rename(2);
    for _ in 0..2 {
      let error = FileOps::rename(&store, "a", "b").unwrap_err();
      assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
    FileOps::rename(&store, "a", "b").unwrap();
    assert_eq!(
      FileOps::rename(&store, "a", "b").unwrap_err().kind(),
      io::ErrorKind::NotFound
    );
  }

  #[test]
  fn a_slow_store_takes_its_time_for_each_op() {
    let store = MemorySaves::new();
    store.slow(Duration::from_millis(5));
    let start = Instant::now();
    FileOps::read(&store, "a").unwrap();
    FileOps::names(&store).unwrap();
    assert!(start.elapsed() >= Duration::from_millis(10));
  }
}
