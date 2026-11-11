//! The developer tools through a long, noisy session: a game that logs
//! warnings every frame for 2,000 frames, with the console open for the
//! first half and closed for the second, and a logger window's feed reading
//! all along. Afterwards each tool still shows the newest record, and each
//! holds what its limit says.
//!
//! No other test in this binary reads the ring (the console's mirror or a
//! feed), so the flood, which pushes every other record out of it, takes
//! nothing another test is waiting for.
//!
//! Tests skip when no GPU is available.
#![cfg(dev_tools)]

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, capture_log_bounded, log_ring_len};

/// Frames of the session, and the warnings it logs in each
const FRAMES: u32 = 2_000;
const PER_FRAME: u32 = 15;
const WARNINGS: u32 = FRAMES * PER_FRAME;
/// The ring's records, the console's lines and the feed's rows at most
const RING: usize = 10_000;
const CONSOLE_LINES: usize = 1_000;
const FEED_ROWS: usize = 10_000;

/// Logs `PER_FRAME` warnings a frame, numbered from 1, until it has logged
/// `WARNINGS`
fn noisy(mut logged: Local<u32>) {
  for _ in 0..PER_FRAME {
    if *logged == WARNINGS {
      return;
    }
    *logged += 1;
    warn!("warning {}", *logged);
  }
}

/// A scene that does nothing: the system logs
struct Quiet;

impl Scene for Quiet {}

/// The number of the warning a console line or feed row shows
fn warning_number(line: &str) -> Option<u32> {
  line.rsplit_once(": warning ")?.1.parse().ok()
}

#[test]
fn a_long_session_keeps_every_tool_working() {
  // Captured records skip stderr; the capture keeps only the newest few
  let log = capture_log_bounded(64);
  let builder = EngineBuilder::new()
    .resolution(64, 64)
    .start_scene(Quiet)
    .add_systems(Phase::Update, noisy);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  let mut feed = engine.log_feed();
  engine.console_open(true);
  let mut shown_before_closing = 0;
  for frame in 0..FRAMES {
    if frame == FRAMES / 2 {
      shown_before_closing = engine
        .console_lines()
        .last()
        .and_then(|line| warning_number(line))
        .expect("the open console mirrors the warnings");
      engine.console_open(false);
    }
    engine.step().expect("a frame");
    // The window reads the feed each time it draws
    feed.poll();
  }
  // The console opens on a frame that logs nothing new
  engine.console_open(true);
  engine.step().expect("a frame");
  feed.poll();

  assert!(
    log.seen() >= u64::from(WARNINGS),
    "the game logged {} records",
    log.seen()
  );
  assert_eq!(log_ring_len(), RING, "the ring holds its size");

  // The console: the newest warning last, and the ones it cannot show
  // counted on the line above the newest it can
  let lines = engine.console_lines();
  assert_eq!(lines.len(), CONSOLE_LINES, "the console holds its cap");
  assert_eq!(
    lines.last().and_then(|line| warning_number(line)),
    Some(WARNINGS),
    "the console's last line is the newest warning: {:?}",
    lines.last()
  );
  let kept = (CONSOLE_LINES - 1) as u32;
  assert_eq!(
    lines.first().map(String::as_str),
    Some(
      format!(
        "{} earlier warnings and errors are no longer shown",
        WARNINGS - shown_before_closing - kept
      )
      .as_str()
    ),
  );
  for (at, line) in lines[1..].iter().enumerate() {
    assert_eq!(
      warning_number(line),
      Some(WARNINGS - kept + 1 + at as u32),
      "the console's lines are the newest warnings in order"
    );
  }

  // The feed: the newest warning last, its rows full, none missed
  let rows = feed.rows();
  assert_eq!(feed.len(), FEED_ROWS, "the feed holds its rows");
  assert_eq!(
    rows.last().and_then(|row| warning_number(row)),
    Some(WARNINGS),
    "the feed's last row is the newest warning: {:?}",
    rows.last()
  );
  assert_eq!(
    rows.first().and_then(|row| warning_number(row)),
    Some(WARNINGS - FEED_ROWS as u32 + 1),
    "the feed keeps the newest {FEED_ROWS}"
  );
  assert_eq!(feed.missed(), 0, "a feed read each frame misses nothing");

  // And the console still runs commands
  let answer = engine.console_run("help");
  assert!(!answer.is_empty(), "help answers after the session");
}

#[test]
fn the_tools_time_is_near_zero_closed_and_more_open() {
  let builder = EngineBuilder::new().resolution(64, 64).start_scene(Quiet);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.console_fill(200);
  for _ in 0..30 {
    engine.step().expect("a warm-up frame");
  }
  let median = |engine: &mut HeadlessEngine| {
    let mut times: Vec<_> = (0..101)
      .map(|_| {
        engine.step().expect("a frame");
        engine.tools_time()
      })
      .collect();
    times.sort_unstable();
    times[times.len() / 2]
  };
  let closed = median(&mut engine);
  engine.console_open(true);
  let open = median(&mut engine);
  assert!(
    closed < std::time::Duration::from_micros(20),
    "closed: {closed:?}"
  );
  assert!(open > closed, "open {open:?} over closed {closed:?}");
}
