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
    reason = "the logger window and the console read it when they move to this log"
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::logging::capture::{capture_log, capture_log_bounded};
  use std::sync::{Mutex, MutexGuard, PoisonError};

  // The two tests that fill the ring take turns, so neither pushes the
  // other's records out before it reads them
  static RING_TESTS: Mutex<()> = Mutex::new(());

  fn ring_turn() -> MutexGuard<'static, ()> {
    RING_TESTS.lock().unwrap_or_else(PoisonError::into_inner)
  }

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
