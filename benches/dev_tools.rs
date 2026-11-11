//! Today's log and console, measured: what a log call costs and what an open
//! console costs a frame. Recorded, with no targets; the figures are the
//! "before" of `docs/backlog/dev-tools.md`.
//!
//! - `log_call_quiet`: one `log::debug!("value {}", n)` (below the default
//!   level) with the log's stderr output off
//!   (`testing::set_log_stderr(false)`), nanoseconds a call over 1,000,000
//!   calls;
//! - `log_call_warn`: one `log::warn!("value {}", n)`, nanoseconds a call
//!   over 100,000 calls. The log's stderr output is switched OFF here too
//!   (not redirected to the null device), because the bench prints its own
//!   results on the terminal, so this figure holds the formatting and the
//!   outputs (the ring, in a build with tools), and not the terminal's write;
//! - `console_open_idle`: a `HeadlessEngine` (1920 x 1080, with a GPU) with
//!   the console open and 1,000 lines of 80 characters printed, against the
//!   same engine with the console closed, stepped in turn (an engine stepping
//!   evicts the others' data) with the GPU drained outside the measure;
//!   the median and 95th percentile of each open frame over the closed
//!   frames' median.

mod guards;

use std::process::Command;
use std::time::{Duration, Instant};

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, set_log_stderr, shared_gpu};

const RESOLUTION: (u32, u32) = (1920, 1080);
const QUIET_CALLS: u32 = 1_000_000;
const WARN_CALLS: u32 = 100_000;
const LINES: usize = 1_000;
/// Frames stepped before measuring, and measured frames of each engine
const WARM: usize = 60;
const ROUNDS: usize = 1000;

struct Empty;

impl Scene for Empty {}

fn ns_per_call(calls: u32, mut call: impl FnMut(u32)) -> f64 {
  let start = Instant::now();
  for n in 0..calls {
    call(n);
  }
  start.elapsed().as_secs_f64() * 1e9 / calls as f64
}

fn log_calls() {
  // The logger installs with the first engine or log line; installed here,
  // its set-up is not in the first call's time
  ryo2d::logging::ensure();
  set_log_stderr(false);
  if guards::runs(&["log_call_quiet"]) {
    let ns = ns_per_call(QUIET_CALLS, |n| log::debug!("value {}", n));
    println!("log_call_quiet: {ns:.1} ns a call over {QUIET_CALLS} calls (terminal output off)");
  }
  if guards::runs(&["log_call_warn"]) {
    let ns = ns_per_call(WARN_CALLS, |n| log::warn!("value {}", n));
    println!(
      "log_call_warn: {ns:.1} ns a call over {WARN_CALLS} calls (terminal output off, not \
       redirected)"
    );
  }
}

fn engine(open: bool) -> Option<HeadlessEngine> {
  let builder = EngineBuilder::new()
    .resolution(RESOLUTION.0, RESOLUTION.1)
    .start_scene(Empty);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  if open {
    engine.console_fill(LINES);
    engine.console_open(true);
  }
  for _ in 0..WARM {
    engine.step().expect("a warm-up frame");
  }
  Some(engine)
}

fn settle_gpu() {
  if let Ok((device, _)) = shared_gpu() {
    device
      .poll(wgpu::PollType::wait_indefinitely())
      .expect("the GPU finishes its work");
  }
}

fn us(time: Duration) -> f64 {
  time.as_secs_f64() * 1e6
}

fn console_open_idle() {
  let (Some(mut closed), Some(mut open)) = (engine(false), engine(true)) else {
    return;
  };
  let (mut closed_steps, mut open_steps) = (Vec::new(), Vec::new());
  for round in 0..ROUNDS {
    // Every other round the open engine steps first
    for open_first in [round % 2 == 1, round % 2 == 0] {
      let (engine, steps) = if open_first {
        (&mut open, &mut open_steps)
      } else {
        (&mut closed, &mut closed_steps)
      };
      settle_gpu();
      let start = Instant::now();
      engine.step().expect("a frame");
      steps.push(start.elapsed());
    }
  }
  closed_steps.sort_unstable();
  let base = closed_steps[closed_steps.len() / 2];
  let mut over: Vec<Duration> = open_steps.iter().map(|t| t.saturating_sub(base)).collect();
  over.sort_unstable();
  let p95 = over[(over.len() * 95 / 100).min(over.len() - 1)];
  println!(
    "console_open_idle: {LINES} lines open at {}x{}, a closed frame's median {:.1} us; the open \
     frame over it: median {:.1} us, 95th percentile {:.1} us ({ROUNDS} interleaved frames)",
    RESOLUTION.0,
    RESOLUTION.1,
    us(base),
    us(over[over.len() / 2]),
    us(p95),
  );
}

fn main() {
  let load = Command::new("uptime")
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_else(|_| "uptime unavailable".into());
  println!("machine: {load}");
  println!("dev tools bench: a log call, a warning, an open console's frame (no targets)");
  log_calls();
  if guards::runs(&["console_open_idle"]) {
    console_open_idle();
  }
}
