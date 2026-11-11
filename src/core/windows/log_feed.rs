//! The logger window's reading of the log, with no window: it follows the
//! ring by sequence number in one log scope, keeps its own bounded list, and
//! filters it by level and origin.

use std::collections::VecDeque;
use std::fmt::Write as _;

use crate::core::logging::{LogRecord, Origin};

/// The rows the feed keeps: as many as the ring holds, so a feed that has
/// fallen behind can take everything the ring still has, and no more (a few
/// megabytes at most).
pub(crate) const FEED_ROWS: usize = 10_000;

/// A filter button: a level, or an origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Toggle {
  Level(log::Level),
  Engine,
  Game,
  Library,
}

impl Toggle {
  /// The filter buttons, in the window's order.
  pub(crate) const ALL: [Toggle; 8] = [
    Toggle::Level(log::Level::Trace),
    Toggle::Level(log::Level::Debug),
    Toggle::Level(log::Level::Info),
    Toggle::Level(log::Level::Warn),
    Toggle::Level(log::Level::Error),
    Toggle::Engine,
    Toggle::Game,
    Toggle::Library,
  ];

  /// The button's label.
  pub(crate) fn label(self) -> &'static str {
    match self {
      Toggle::Level(level) => level_letter(level),
      Toggle::Engine => "ENG",
      Toggle::Game => "GAME",
      Toggle::Library => "LIB",
    }
  }

  fn index(self) -> usize {
    match self {
      // Trace is 5 and Error 1: the buttons run Trace to Error
      Toggle::Level(level) => 5 - level as usize,
      Toggle::Engine => 5,
      Toggle::Game => 6,
      Toggle::Library => 7,
    }
  }
}

/// A level's letter: `E`, `W`, `I`, `D`, `T`.
pub(crate) fn level_letter(level: log::Level) -> &'static str {
  match level {
    log::Level::Error => "E",
    log::Level::Warn => "W",
    log::Level::Info => "I",
    log::Level::Debug => "D",
    log::Level::Trace => "T",
  }
}

/// An origin's letter: `E`ngine, `G`ame, `L`ibrary.
pub(crate) fn origin_letter(origin: Origin) -> &'static str {
  match origin {
    Origin::Engine => "E",
    Origin::Game => "G",
    Origin::Library => "L",
  }
}

/// A row's text: `HH:MM:SS.mmm frame L O target: message`, the frame blank
/// for a record without one.
pub(crate) fn row_text(record: &LogRecord) -> String {
  let mut text = String::with_capacity(32 + record.target.len() + record.message.len());
  let _ = write!(text, "{} ", crate::core::logging::clock(record.time));
  match record.frame {
    Some(frame) => {
      let _ = write!(text, "{frame:>7} ");
    }
    None => text.push_str("        "),
  }
  let _ = write!(
    text,
    "{} {} {}: {}",
    level_letter(record.level),
    origin_letter(record.origin),
    record.target,
    record.message
  );
  text
}

/// The log as the logger window shows it: the records of one scope, read
/// after the last sequence number taken, never by index.
pub(crate) struct LogFeed {
  scope: u64,
  /// The last sequence number read: the next poll asks after it
  seq: u64,
  /// The ring's warning mark after the last read
  mark: u64,
  rows: VecDeque<LogRecord>,
  /// The poll's buffer, kept between polls
  incoming: Vec<LogRecord>,
  /// On or off, by [`Toggle::index`]
  on: [bool; 8],
  /// The warnings and errors the ring dropped before the feed read them
  missed: u64,
}

impl LogFeed {
  /// A feed of `scope`'s records, from the oldest the ring still holds.
  pub(crate) fn new(scope: u64) -> Self {
    let (seq, mark) = crate::core::logging::mirror_start(scope);
    Self {
      scope,
      seq,
      mark,
      rows: VecDeque::new(),
      incoming: Vec::new(),
      // Trace and the libraries off; the rest on
      on: [false, true, true, true, true, true, true, false],
      missed: 0,
    }
  }

  /// Takes the records logged since the last poll, and returns how many of
  /// them the filter shows (a scrolled view keeps still by them). Costs one
  /// atomic read when there are none.
  pub(crate) fn poll(&mut self) -> usize {
    use crate::core::logging::{mirror_records, records_may_follow};
    if !records_may_follow(self.seq) {
      return 0;
    }
    let mut incoming = std::mem::take(&mut self.incoming);
    let found = mirror_records(
      self.seq,
      self.mark,
      self.scope,
      log::LevelFilter::Trace,
      FEED_ROWS,
      &mut incoming,
    );
    self.seq = found.newest;
    self.mark = found.mark;
    self.missed += found.hidden;
    let mut shown = 0;
    for record in incoming.drain(..) {
      shown += usize::from(self.shows(&record));
      if self.rows.len() >= FEED_ROWS {
        self.rows.pop_front();
      }
      self.rows.push_back(record);
    }
    self.incoming = incoming;
    shown
  }

  /// Empties the feed's list (not the ring's); it carries on after the
  /// newest record logged so far.
  pub(crate) fn clear(&mut self) {
    self.rows.clear();
    self.missed = 0;
    let found = crate::core::logging::mirror_records(
      self.seq,
      self.mark,
      self.scope,
      log::LevelFilter::Off,
      0,
      &mut self.incoming,
    );
    self.seq = found.newest;
    self.mark = found.mark;
  }

  /// Every row kept, oldest first, shown or not.
  pub(crate) fn rows(&self) -> &VecDeque<LogRecord> {
    &self.rows
  }

  /// The rows the filter shows, oldest first.
  pub(crate) fn visible(&self) -> impl DoubleEndedIterator<Item = &LogRecord> + '_ {
    self.rows.iter().filter(|record| self.shows(record))
  }

  /// Whether the filter shows `record`: its level's button and its
  /// origin's are on.
  pub(crate) fn shows(&self, record: &LogRecord) -> bool {
    let origin = match record.origin {
      Origin::Engine => Toggle::Engine,
      Origin::Game => Toggle::Game,
      Origin::Library => Toggle::Library,
    };
    self.is_on(Toggle::Level(record.level)) && self.is_on(origin)
  }

  pub(crate) fn is_on(&self, toggle: Toggle) -> bool {
    self.on[toggle.index()]
  }

  pub(crate) fn toggle(&mut self, toggle: Toggle) {
    let on = &mut self.on[toggle.index()];
    *on = !*on;
  }

  /// The last sequence number read.
  #[cfg(test)]
  pub(crate) fn seq(&self) -> u64 {
    self.seq
  }

  /// The warnings and errors the ring dropped before the feed could read
  /// them.
  pub(crate) fn missed(&self) -> u64 {
    self.missed
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::logging::{capture_log, capture_log_bounded, ring_turn};

  fn messages<'a>(rows: impl Iterator<Item = &'a LogRecord>) -> Vec<&'a str> {
    rows.map(|r| r.message.as_str()).collect()
  }

  #[test]
  fn the_feed_shows_record_10_001_and_after() {
    // A flood from another test could push these out between reads
    let _turn = ring_turn();
    let capture = capture_log_bounded(16);
    let mut feed = LogFeed::new(capture.scope());
    for i in 1..=10_500 {
      log::info!(target: "game::feed", "r{i}");
      // The window reads a frame's worth at a time
      if i % 100 == 0 {
        feed.poll();
      }
    }
    feed.poll();
    let newest = feed.rows().back().expect("rows");
    assert_eq!(newest.message, "r10500");
    // Other tests log meanwhile: the feed reads past them, never short
    assert!(
      feed.seq() >= newest.seq,
      "the feed read up to its newest row"
    );
    assert_eq!(feed.rows().len(), FEED_ROWS, "its own list is bounded");
    assert_eq!(feed.rows()[0].message, "r501", "the oldest go first");
    assert!(
      feed
        .rows()
        .iter()
        .zip(feed.rows().iter().skip(1))
        .all(|(a, b)| a.seq < b.seq)
    );

    log::warn!(target: "game::feed", "one more");
    feed.poll();
    assert_eq!(
      feed.rows().back().map(|r| r.message.as_str()),
      Some("one more")
    );
  }

  #[test]
  fn the_feed_reads_only_its_scope() {
    let capture = capture_log();
    let mut feed = LogFeed::new(capture.scope());
    log::info!(target: "game::feed", "mine");
    let theirs = std::thread::spawn(|| {
      let capture = capture_log();
      log::info!(target: "game::feed", "theirs");
      capture.scope()
    })
    .join()
    .expect("a thread");
    log::info!(target: "game::feed", "mine too");
    feed.poll();
    assert_eq!(messages(feed.visible()), ["mine", "mine too"]);

    let mut other = LogFeed::new(theirs);
    other.poll();
    assert_eq!(messages(other.visible()), ["theirs"]);
  }

  #[test]
  fn library_rows_show_only_with_lib_on() {
    let capture = capture_log();
    let mut feed = LogFeed::new(capture.scope());
    log::info!(target: "ryo2d::core::feed", "engine");
    log::info!(target: "game::feed", "game");
    log::warn!(target: "wgpu_core", "library");
    feed.poll();
    assert_eq!(feed.rows().len(), 3, "the feed holds every origin");
    assert!(!feed.is_on(Toggle::Library), "LIB is off by default");
    assert!(feed.is_on(Toggle::Engine) && feed.is_on(Toggle::Game));
    assert_eq!(messages(feed.visible()), ["engine", "game"]);

    feed.toggle(Toggle::Library);
    assert_eq!(messages(feed.visible()), ["engine", "game", "library"]);
    feed.toggle(Toggle::Engine);
    feed.toggle(Toggle::Level(log::Level::Info));
    assert_eq!(messages(feed.visible()), ["library"]);
  }

  #[test]
  fn clr_empties_the_list_and_the_feed_carries_on() {
    let capture = capture_log();
    let mut feed = LogFeed::new(capture.scope());
    for i in 0..3 {
      log::info!(target: "game::feed", "before {i}");
    }
    feed.poll();
    assert_eq!(feed.rows().len(), 3);
    // Logged but not yet read when CLR is pressed: cleared too
    log::info!(target: "game::feed", "unread");
    feed.clear();
    assert!(feed.rows().is_empty());
    feed.poll();
    assert!(feed.rows().is_empty(), "the feed does not start again");

    log::info!(target: "game::feed", "after 0");
    log::info!(target: "game::feed", "after 1");
    feed.poll();
    assert_eq!(messages(feed.visible()), ["after 0", "after 1"]);
  }

  #[test]
  fn a_row_shows_time_frame_level_origin_and_target() {
    let record = |frame| LogRecord {
      seq: 1,
      level: log::Level::Warn,
      target: "game::menu".into(),
      origin: crate::core::logging::Origin::Game,
      message: "no save".into(),
      time: std::time::Duration::from_millis(3_723_045),
      frame,
      scope: 0,
    };
    assert_eq!(
      row_text(&record(Some(42))),
      "01:02:03.045      42 W G game::menu: no save"
    );
    assert_eq!(
      row_text(&record(None)),
      "01:02:03.045         W G game::menu: no save"
    );
  }
}
