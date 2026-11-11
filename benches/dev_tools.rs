//! The log and the console, measured and held to their targets. The tools'
//! benches need the tools, and `cargo bench` builds without them: run this
//! with `cargo bench --bench dev_tools --features dev-tools` for every bench,
//! and without the feature for the two log calls alone (a build without the
//! tools has no ring and no console). Both runs are recorded in
//! `docs/backlog/dev-tools.md`.
//!
//! Every log call here is made with an engine target (`ryo2d::bench`), as
//! the engine's own calls are, so the figures compare with the earlier log's,
//! which measured engine records.
//!
//! - `log_call_quiet`: one `log::debug!(target: "ryo2d::bench", "value {}",
//!   n)` at the default levels (`info`), nanoseconds a call, the median of
//!   `REPEATS` runs of 1,000,000 calls; held to `QUIET_TARGET_NS`. The
//!   level check cuts the record before it is formatted or stored: this is
//!   what a game pays for a silenced line. (The earlier log kept its debug
//!   lines, so its figure was a different piece of work.);
//! - `log_call_warn`: one `log::warn!(target: "ryo2d::bench", "value {}",
//!   n)`, the median of `REPEATS` runs of 100,000 calls; held to no slower
//!   than the earlier log's median (`WARN_BEFORE_NS`). The log's stderr
//!   output is switched off (not redirected to the null device: the bench
//!   prints its own results on the terminal), so the figure holds the
//!   formatting and the outputs but not the terminal's write. No log file is
//!   open in a bench (opening one is crate-internal), so the file output
//!   takes its early path: each record is cloned into its 256-record buffer
//!   under a mutex. With `--features dev-tools` the record also goes to the
//!   ring (and its lock); without the tools there is no ring;
//! - `console_open_idle`: a `HeadlessEngine` (1920 x 1080, with a GPU) with
//!   the console open and 1,000 lines of 80 characters printed, against the
//!   same engine with the console closed, stepped in turn (an engine stepping
//!   evicts the others' data) with the GPU drained outside the measure; the
//!   median and 95th percentile of each open frame over the closed frames'
//!   median. The median is held to a quarter of the earlier console's
//!   (`IDLE_BEFORE_US`). Needs the tools;
//! - `tools_closed`: the console's and dev windows' own time in a frame with
//!   everything closed, the median of `TOOLS_FRAMES` frames of an empty
//!   scene, read from the engine's own timer around the console's tick
//!   (`HeadlessEngine::tools_time`); held to `CLOSED_TARGET`. The dev windows
//!   are drawn by the window loop, outside a frame, and a headless engine
//!   opens none, so their closed cost (two option checks) is not in it. The
//!   timer's own two clock reads are. Needs the tools.
//!
//! A missed guard fails the run once every bench has run (`guards::verdict`).

mod guards;

use std::process::Command;
#[cfg(dev_tools)]
use std::time::Duration;
use std::time::Instant;

#[cfg(dev_tools)]
use ryo2d::prelude::*;
use ryo2d::testing::set_log_stderr;
#[cfg(dev_tools)]
use ryo2d::testing::{HeadlessEngine, shared_gpu};

const QUIET_CALLS: u32 = 1_000_000;
const WARN_CALLS: u32 = 100_000;
/// Runs of each log bench, the median of which is the figure
const REPEATS: usize = 5;
/// The target of a silenced log call
const QUIET_TARGET_NS: f64 = 5.0;
/// The earlier log's `log_call_warn` median, nanoseconds a call: the median
/// of the three runs in `docs/backlog/dev-tools.md` ("Before": 98.3, 99.9,
/// 105.0, terminal output off), taken at the commit before the logger was
/// replaced. A warning may cost no more.
const WARN_BEFORE_NS: f64 = 99.9;
/// The earlier console's `console_open_idle` median over the closed frame,
/// microseconds: the median of the three runs in the same record (187.6,
/// 184.1, 188.9). The open idle console must cost under a quarter of it.
#[cfg(dev_tools)]
const IDLE_BEFORE_US: f64 = 187.6;
/// The tools' own time in a closed frame
#[cfg(dev_tools)]
const CLOSED_TARGET: Duration = Duration::from_micros(1);
/// Frames of an empty scene `tools_closed` reads
#[cfg(dev_tools)]
const TOOLS_FRAMES: usize = 2_000;
#[cfg(dev_tools)]
const RESOLUTION: (u32, u32) = (1920, 1080);
#[cfg(dev_tools)]
const LINES: usize = 1_000;
/// Frames stepped before measuring, and measured frames of each engine
#[cfg(dev_tools)]
const WARM: usize = 60;
#[cfg(dev_tools)]
const ROUNDS: usize = 1000;

#[cfg(dev_tools)]
struct Empty;

#[cfg(dev_tools)]
impl Scene for Empty {}

fn ns_per_call(calls: u32, call: fn(u32)) -> f64 {
  let start = Instant::now();
  for n in 0..calls {
    call(n);
  }
  start.elapsed().as_secs_f64() * 1e9 / calls as f64
}

/// The call sites, kept out of line so the compiler cannot hoist the level
/// check out of the timing loop: each call is a real call, as a game's is
#[inline(never)]
fn quiet_call(n: u32) {
  log::debug!(target: "ryo2d::bench", "value {}", n);
}

#[inline(never)]
fn warn_call(n: u32) {
  log::warn!(target: "ryo2d::bench", "value {}", n);
}

/// The median of `REPEATS` runs, nanoseconds a call
fn median_ns(calls: u32, call: fn(u32)) -> f64 {
  let mut runs: Vec<f64> = (0..REPEATS).map(|_| ns_per_call(calls, call)).collect();
  runs.sort_by(f64::total_cmp);
  runs[runs.len() / 2]
}

fn log_calls() {
  // The logger installs with the first engine or log line; installed here,
  // its set-up is not in the first call's time
  ryo2d::logging::ensure();
  set_log_stderr(false);
  if guards::runs(&["log_call_quiet"]) {
    let ns = median_ns(QUIET_CALLS, quiet_call);
    println!(
      "log_call_quiet: {ns:.1} ns a call, median of {REPEATS} runs of {QUIET_CALLS} calls \
       (terminal output off; target {QUIET_TARGET_NS} ns)"
    );
    if ns > QUIET_TARGET_NS {
      guards::miss(format!(
        "log_call_quiet {ns:.1} ns over its {QUIET_TARGET_NS} ns target"
      ));
    }
  }
  if guards::runs(&["log_call_warn"]) {
    let ns = median_ns(WARN_CALLS, warn_call);
    println!(
      "log_call_warn: {ns:.1} ns a call, median of {REPEATS} runs of {WARN_CALLS} calls \
       (terminal output off, not redirected; earlier log {WARN_BEFORE_NS} ns)"
    );
    if ns > WARN_BEFORE_NS {
      guards::miss(format!(
        "log_call_warn {ns:.1} ns over the earlier log's {WARN_BEFORE_NS} ns"
      ));
    }
  }
}

#[cfg(dev_tools)]
fn engine(open: bool) -> Option<HeadlessEngine> {
  let builder = EngineBuilder::new()
    .resolution(RESOLUTION.0, RESOLUTION.1)
    .start_scene(Empty);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  // A headless engine turns the log's stderr output on
  set_log_stderr(false);
  if open {
    engine.console_fill(LINES);
    engine.console_open(true);
  }
  for _ in 0..WARM {
    engine.step().expect("a warm-up frame");
  }
  Some(engine)
}

#[cfg(dev_tools)]
fn settle_gpu() {
  if let Ok((device, _)) = shared_gpu() {
    device
      .poll(wgpu::PollType::wait_indefinitely())
      .expect("the GPU finishes its work");
  }
}

#[cfg(dev_tools)]
fn us(time: Duration) -> f64 {
  time.as_secs_f64() * 1e6
}

#[cfg(dev_tools)]
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
  let budget = IDLE_BEFORE_US / 4.0;
  let median = us(over[over.len() / 2]);
  if median >= budget {
    guards::miss(format!(
      "console_open_idle {median:.1} us, not under {budget:.1} us (a quarter of the earlier \
       console's {IDLE_BEFORE_US} us)"
    ));
  }
  println!(
    "console_open_idle: {LINES} lines open at {}x{}, a closed frame's median {:.1} us; the open \
     frame over it: median {:.1} us, 95th percentile {:.1} us ({ROUNDS} interleaved frames)",
    RESOLUTION.0,
    RESOLUTION.1,
    us(base),
    median,
    us(p95),
  );
  println!("console_open_idle: held under {budget:.1} us");
}

#[cfg(dev_tools)]
fn tools_closed() {
  let Some(mut engine) = engine(false) else {
    return;
  };
  let mut times: Vec<Duration> = (0..TOOLS_FRAMES)
    .map(|_| {
      engine.step().expect("a frame");
      engine.tools_time()
    })
    .collect();
  times.sort_unstable();
  let median = times[times.len() / 2];
  let p95 = times[times.len() * 95 / 100];
  println!(
    "tools_closed: {:.0} ns median, {:.0} ns 95th percentile over {TOOLS_FRAMES} closed frames \
     (target {} ns)",
    median.as_secs_f64() * 1e9,
    p95.as_secs_f64() * 1e9,
    CLOSED_TARGET.as_nanos(),
  );
  if median > CLOSED_TARGET {
    guards::miss(format!(
      "tools_closed {} ns over its {} ns target",
      median.as_nanos(),
      CLOSED_TARGET.as_nanos()
    ));
  }
}

fn main() {
  let load = Command::new("uptime")
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_else(|_| "uptime unavailable".into());
  println!("machine: {load}");
  println!("dev tools bench: a log call, a warning, an open console's frame, closed tools");
  log_calls();
  #[cfg(dev_tools)]
  {
    if guards::runs(&["console_open_idle"]) {
      console_open_idle();
    }
    if guards::runs(&["tools_closed"]) {
      tools_closed();
    }
  }
  guards::verdict(
    "dev_tools",
    "profile a log call or a frame with the profiler's scopes (`ryo2d dev -p`)",
  );
}
