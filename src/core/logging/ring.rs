//! The ring: the last [`RING`] records, which the dev tools read by sequence
//! number. A test reads it only in its own capture's scope.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::outputs::NEXT_SEQ;
use super::record::LogRecord;

/// The records the ring keeps.
pub(crate) const RING: usize = 10_000;

/// The kept records, and per scope the warnings and errors pushed and
/// evicted (the console counts those it can no longer show)
struct Ring {
  records: VecDeque<LogRecord>,
  warnings: BTreeMap<u64, WarnCounts>,
}

/// A scope's records at `Warn` or above: how many the ring was given, and
/// how many it has dropped since. It drops in sequence order, so `evicted`
/// is also the number given before its oldest record of the scope.
#[derive(Debug, Clone, Copy, Default)]
struct WarnCounts {
  pushed: u64,
  evicted: u64,
}

static RING_STATE: Mutex<Ring> = Mutex::new(Ring {
  records: VecDeque::new(),
  warnings: BTreeMap::new(),
});

fn ring() -> MutexGuard<'static, Ring> {
  RING_STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn is_warning(record: &LogRecord) -> bool {
  record.level <= log::Level::Warn
}

/// Gives `record` its sequence number and keeps a copy. The number is taken
/// under the ring's lock, so the ring is in sequence order.
pub(super) fn push(record: &mut LogRecord) {
  let mut ring = ring();
  record.seq = NEXT_SEQ.fetch_add(1, Ordering::Relaxed);
  if ring.records.len() >= RING {
    if let Some(oldest) = ring.records.pop_front()
      && is_warning(&oldest)
    {
      ring.warnings.entry(oldest.scope).or_default().evicted += 1;
    }
  } else if ring.records.capacity() == 0 {
    ring.records.reserve_exact(RING);
  }
  if is_warning(record) {
    ring.warnings.entry(record.scope).or_default().pushed += 1;
  }
  ring.records.push_back(record.clone());
}

/// Appends the kept records with a sequence number above `seq` (of `scope`
/// only, when given) to `out`, oldest first, and returns the newest sequence
/// number kept (`seq` when there is none newer).
/// (The ring's own tests' reader; the dev tools read with [`mirror`].)
#[cfg(test)]
pub(crate) fn after(seq: u64, scope: Option<u64>, out: &mut Vec<LogRecord>) -> u64 {
  let ring = ring();
  let records = &ring.records;
  let start = records.partition_point(|r| r.seq <= seq);
  out.extend(
    records
      .range(start..)
      .filter(|r| scope.is_none_or(|scope| r.scope == scope))
      .cloned(),
  );
  records.back().map_or(seq, |r| r.seq.max(seq))
}

/// Where a reader of one scope starts: just before the ring's oldest record
/// (so it reads all the ring holds and nothing counts as lost), as the
/// sequence number and the warning mark [`mirror`] takes.
pub(crate) fn start(scope: u64) -> (u64, u64) {
  let ring = ring();
  let seq = ring.records.front().map_or_else(
    || NEXT_SEQ.load(Ordering::Relaxed).saturating_sub(1),
    |oldest| oldest.seq - 1,
  );
  let mark = ring.warnings.get(&scope).map_or(0, |counts| counts.evicted);
  (seq, mark)
}

/// Whether a record after `seq` may have been given: one atomic load, so a
/// reader with nothing new takes no lock. A record being pushed may be seen
/// a call late, never lost.
pub(crate) fn may_have_after(seq: u64) -> bool {
  NEXT_SEQ.load(Ordering::Relaxed) > seq.saturating_add(1)
}

/// What [`mirror`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mirrored {
  /// The newest sequence number in the ring (the one asked after when
  /// there is none newer): the next read starts after it
  pub(crate) newest: u64,
  /// The scope's warnings and errors given so far: the next read's mark
  pub(crate) mark: u64,
  /// The scope's warnings and errors after the last read that the ring has
  /// dropped, or that matched past `limit`: those the reader will not show
  pub(crate) hidden: u64,
}

/// Appends to `out`, oldest first, the newest `limit` records after `seq`
/// of `scope` at or above `level` (the console's mirror), cloning only those.
/// `mark` is the last read's [`Mirrored::mark`] (or [`start`]'s).
pub(crate) fn mirror(
  seq: u64,
  mark: u64,
  scope: u64,
  level: log::LevelFilter,
  limit: usize,
  out: &mut Vec<LogRecord>,
) -> Mirrored {
  let ring = ring();
  let records = &ring.records;
  let counts = ring.warnings.get(&scope).copied().unwrap_or_default();
  // Everything of the scope up to the last read gone, what it dropped
  // since is past `mark`; while some of it is kept, nothing newer is gone
  let dropped = counts.evicted.saturating_sub(mark);
  let start = records.partition_point(|r| r.seq <= seq);
  let from = out.len();
  let mut beyond = 0;
  for record in records
    .range(start..)
    .rev()
    .filter(|r| r.scope == scope && r.level <= level)
  {
    if out.len() - from < limit {
      out.push(record.clone());
    } else if is_warning(record) {
      beyond += 1;
    }
  }
  out[from..].reverse();
  Mirrored {
    newest: records.back().map_or(seq, |r| r.seq.max(seq)),
    mark: counts.pushed,
    hidden: dropped + beyond,
  }
}

/// The next sequence number to be given (tests: whether anything was logged)
#[cfg(test)]
pub(crate) fn next_seq() -> u64 {
  NEXT_SEQ.load(Ordering::Relaxed)
}

// The tests that fill the ring take turns, so none pushes another's records
// out before it reads them
#[cfg(test)]
static RING_TESTS: Mutex<()> = Mutex::new(());

/// A turn at filling the ring (tests that log past its size hold one)
#[cfg(test)]
pub(crate) fn ring_turn() -> MutexGuard<'static, ()> {
  RING_TESTS.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::logging::capture::{capture_log, capture_log_bounded};

  #[test]
  fn sequence_numbers_never_repeat_and_the_ring_answers_after_n_past_its_size() {
    let _turn = ring_turn();
    let capture = capture_log();
    let target = "ryo2d::core::logging::ring_test";
    for i in 1..=24_990 {
      log::warn!(target: target, "record {i}");
    }

    let mut kept = Vec::new();
    after(0, Some(capture.scope()), &mut kept);
    assert!(!kept.is_empty() && kept.len() <= RING, "{}", kept.len());
    assert_ne!(kept[0].message, "record 1", "the ring drops its oldest");
    assert_eq!(kept[kept.len() - 1].message, "record 24990");
    assert!(kept.windows(2).all(|w| w[0].seq < w[1].seq));

    let mark = kept[kept.len() - 1].seq;
    for i in 24_991..=25_000 {
      log::warn!(target: target, "record {i}");
    }
    let mut last = Vec::new();
    let newest = after(mark, Some(capture.scope()), &mut last);
    let messages: Vec<_> = last.iter().map(|r| r.message.as_str()).collect();
    let expected: Vec<_> = (24_991..=25_000).map(|i| format!("record {i}")).collect();
    assert_eq!(messages, expected);
    assert!(last[0].seq > mark);
    assert!(last.windows(2).all(|w| w[0].seq < w[1].seq));
    assert!(newest >= last[last.len() - 1].seq);

    let all = capture.records();
    assert_eq!(all.len(), 25_000);
    assert!(all.windows(2).all(|w| w[0].seq < w[1].seq));
  }

  #[test]
  fn the_mirror_reads_its_scope_s_newest_at_its_level() {
    let _turn = ring_turn();
    let capture = capture_log();
    let scope = capture.scope();
    let target = "ryo2d::core::logging::mirror_test";
    let (start, mark) = start(scope);
    assert_eq!(mark, 0, "a new scope has dropped nothing");
    log::info!(target: target, "info");
    for i in 0..5 {
      log::warn!(target: target, "warn {i}");
    }
    log::error!(target: target, "error");
    assert!(may_have_after(start));
    let mut out = Vec::new();
    let found = mirror(start, mark, scope, log::LevelFilter::Warn, 3, &mut out);
    let messages: Vec<_> = out.iter().map(|r| r.message.as_str()).collect();
    assert_eq!(
      messages,
      ["warn 3", "warn 4", "error"],
      "the newest, oldest first"
    );
    assert!(found.newest >= out[2].seq);
    assert_eq!(found.hidden, 3, "three warnings past the limit");
    assert_eq!(found.mark, 6);
    out.clear();
    let again = mirror(
      found.newest,
      found.mark,
      scope,
      log::LevelFilter::Warn,
      3,
      &mut out,
    );
    assert!(out.is_empty(), "nothing after the newest");
    assert_eq!(again.hidden, 0);
    mirror(start, mark, scope, log::LevelFilter::Off, 3, &mut out);
    assert!(out.is_empty(), "off takes none");
  }

  #[test]
  fn a_scope_s_dropped_warnings_are_counted_and_no_other_scope_s() {
    let _turn = ring_turn();
    let capture = capture_log_bounded(4);
    let scope = capture.scope();
    let (seq, mark) = start(scope);
    for i in 0..10_050 {
      log::warn!(target: "ryo2d::core::logging::drop_test", "drop {i}");
      log::info!(target: "ryo2d::core::logging::drop_test", "info {i}");
    }
    let mut out = Vec::new();
    let found = mirror(seq, mark, scope, log::LevelFilter::Warn, 10, &mut out);
    assert_eq!(out.len(), 10);
    assert_eq!(out[9].message, "drop 10049");
    // Every warning but the ten shown: dropped by the ring, or past the
    // limit; the infos and other scopes' records count for nothing
    assert_eq!(found.hidden, 10_040);
    // Read again after them: nothing is hidden twice
    out.clear();
    let again = mirror(
      found.newest,
      found.mark,
      scope,
      log::LevelFilter::Warn,
      10,
      &mut out,
    );
    assert_eq!((out.len(), again.hidden), (0, 0));
  }

  #[test]
  fn a_flood_keeps_the_ring_at_its_size() {
    let _turn = ring_turn();
    let capture = capture_log_bounded(16);
    for i in 0..100_000 {
      log::warn!(target: "wgpu_core", "flood {i}");
    }
    assert_eq!(capture.seen(), 100_000);
    assert_eq!(capture.records().len(), 16);

    let mut all = Vec::new();
    after(0, None, &mut all);
    assert_eq!(all.len(), RING);

    let mut mine = Vec::new();
    after(0, Some(capture.scope()), &mut mine);
    assert!(mine.len() <= RING);
    assert_eq!(mine[mine.len() - 1].message, "flood 99999");
  }
}
