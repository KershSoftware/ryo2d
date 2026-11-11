//! The ring: the last [`RING`] records, which the dev tools read by sequence
//! number. Tests never read it (they capture their own scope).

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, PoisonError};

use super::outputs::NEXT_SEQ;
use super::record::LogRecord;

/// The records the ring keeps.
pub(crate) const RING: usize = 10_000;

static RECORDS: Mutex<VecDeque<LogRecord>> = Mutex::new(VecDeque::new());

/// Gives `record` its sequence number and keeps a copy. The number is taken
/// under the ring's lock, so the ring is in sequence order.
pub(super) fn push(record: &mut LogRecord) {
  let mut records = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
  record.seq = NEXT_SEQ.fetch_add(1, Ordering::Relaxed);
  if records.len() >= RING {
    records.pop_front();
  } else if records.capacity() == 0 {
    records.reserve_exact(RING);
  }
  records.push_back(record.clone());
}

/// Appends the kept records with a sequence number above `seq` (of `scope`
/// only, when given) to `out`, oldest first, and returns the newest sequence
/// number kept (`seq` when there is none newer).
#[cfg_attr(
  not(test),
  expect(
    dead_code,
    reason = "the logger window reads it when it moves to this log"
  )
)]
pub(crate) fn after(seq: u64, scope: Option<u64>, out: &mut Vec<LogRecord>) -> u64 {
  let records = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
  let start = records.partition_point(|r| r.seq <= seq);
  out.extend(
    records
      .range(start..)
      .filter(|r| scope.is_none_or(|scope| r.scope == scope))
      .cloned(),
  );
  records.back().map_or(seq, |r| r.seq.max(seq))
}

/// What [`mirror`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mirrored {
  /// The newest sequence number in the ring (the one asked after when
  /// there is none newer): the next read starts after it
  pub(crate) newest: u64,
  /// Records after the one asked after that are no longer in the ring, of
  /// any scope (it keeps the newest `RING`)
  pub(crate) lost: u64,
}

/// Appends to `out`, oldest first, the newest `limit` records after `seq`
/// of `scope` at or above `level` (the console's mirror), cloning only those.
pub(crate) fn mirror(
  seq: u64,
  scope: u64,
  level: log::LevelFilter,
  limit: usize,
  out: &mut Vec<LogRecord>,
) -> Mirrored {
  let records = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
  let lost = records
    .front()
    .map_or(0, |first| first.seq.saturating_sub(seq.saturating_add(1)));
  let start = records.partition_point(|r| r.seq <= seq);
  let from = out.len();
  out.extend(
    records
      .range(start..)
      .rev()
      .filter(|r| r.scope == scope && r.level <= level)
      .take(limit)
      .cloned(),
  );
  out[from..].reverse();
  Mirrored {
    newest: records.back().map_or(seq, |r| r.seq.max(seq)),
    lost,
  }
}

/// The newest sequence number given so far (0 before the first record)
pub(crate) fn newest() -> u64 {
  let records = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
  records.back().map_or(0, |r| r.seq)
}

// The tests that fill the ring take turns, so none pushes another's records
// out before it reads them
#[cfg(test)]
static RING_TESTS: Mutex<()> = Mutex::new(());

/// A turn at filling the ring (tests that log past its size hold one)
#[cfg(test)]
pub(crate) fn ring_turn() -> std::sync::MutexGuard<'static, ()> {
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
    let target = "ryo2d::core::logging::mirror_test";
    let start = newest();
    log::info!(target: target, "info");
    for i in 0..5 {
      log::warn!(target: target, "warn {i}");
    }
    log::error!(target: target, "error");
    let mut out = Vec::new();
    let found = mirror(start, capture.scope(), log::LevelFilter::Warn, 3, &mut out);
    let messages: Vec<_> = out.iter().map(|r| r.message.as_str()).collect();
    assert_eq!(
      messages,
      ["warn 3", "warn 4", "error"],
      "the newest, oldest first"
    );
    assert!(found.newest >= out[2].seq);
    out.clear();
    mirror(
      found.newest,
      capture.scope(),
      log::LevelFilter::Warn,
      3,
      &mut out,
    );
    assert!(out.is_empty(), "nothing after the newest");
    mirror(start, capture.scope(), log::LevelFilter::Off, 3, &mut out);
    assert!(out.is_empty(), "off takes none");
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
