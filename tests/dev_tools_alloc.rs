//! Allocations of the developer tools while they have nothing to do,
//! counted by a global allocator in this test binary: with the console
//! closed and no dev window, and with the console open on 1,000 lines and
//! nothing new printed or logged, 100 frames make no allocation in the
//! console's or the dev windows' code.
//!
//! An allocation is the tools' when its stack, read as in
//! `tests/particles_alloc.rs`, passes through an engine function of
//! `ryo2d::core::console` or `ryo2d::core::windows`, or one of the engine's
//! console functions (`console_tick` and the like, in `core::engine`): what
//! the tools' code asks of the rest of the engine (wrapping or measuring
//! text, reading the ring) counts as theirs. One kind is not: the UI's
//! recording of a draw the open console makes (`UiContext::text`, which
//! copies the row's text into its leaf as for any label, and `push_clip`
//! on the frame's clip stack), which an open console makes every frame as
//! any immediate-mode window does; those are printed, not asserted. The
//! other allocations of a frame (the GPU API's among them) are not the
//! tools' and are not asserted.
//!
//! Only allocation tests live here: the counting allocator would slow any
//! other test down. Each thread counts its own allocations, so the two
//! tests never count each other's.
#![cfg(dev_tools)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::backtrace::Backtrace;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

thread_local! {
  static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
  /// Whether each allocation's stack is kept, and whether one is being
  /// taken (its own allocations are not)
  static TRACING: Cell<bool> = const { Cell::new(false) };
  static IN_TRACE: Cell<bool> = const { Cell::new(false) };
  static TRACES: RefCell<Vec<Backtrace>> = const { RefCell::new(Vec::new()) };
}

/// Counts an allocation, and keeps its stack while tracing (taking a stack
/// allocates, and those are not counted)
fn note() {
  if IN_TRACE.with(Cell::get) {
    return;
  }
  ALLOCATIONS.with(|c| c.set(c.get() + 1));
  if TRACING.with(Cell::get) {
    IN_TRACE.with(|c| c.set(true));
    let trace = Backtrace::force_capture();
    TRACES.with(|t| t.borrow_mut().push(trace));
    IN_TRACE.with(|c| c.set(false));
  }
}

/// Counts the allocations each thread makes
struct Counting;

// SAFETY: every call goes to `System`. The counters are const-initialised
// thread locals without destructors, so touching them never allocates; a
// stack is only taken while tracing, and its own allocations (and the
// list's) happen with `IN_TRACE` set, so they are neither counted nor
// traced
unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    note();
    unsafe { System.alloc(layout) }
  }
  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) }
  }
  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
    note();
    unsafe { System.realloc(ptr, layout, new) }
  }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// The allocations this thread has made
fn allocations() -> usize {
  ALLOCATIONS.with(Cell::get)
}

/// Frames before the measure, and measured
const WARM: usize = 20;
const MEASURED: usize = 100;
/// The lines the open console holds
const LINES: usize = 1_000;

/// A scene that does nothing, and logs nothing
struct Idle;

impl Scene for Idle {}

/// A stack's function names, leaf first
fn functions(text: &str) -> Vec<&str> {
  text
    .lines()
    .map(str::trim)
    .filter(|line| !line.starts_with("at "))
    .filter_map(|line| line.split_once(": ").map(|(_, name)| name))
    .collect()
}

/// The tools' function a stack passes through, nearest the leaf: one of
/// the console's or the dev windows' modules, or an engine function of the
/// console's
fn tools_function<'a>(frames: &[&'a str]) -> Option<&'a str> {
  frames.iter().copied().find(|frame| {
    frame.contains("ryo2d::core::console")
      || frame.contains("ryo2d::core::windows")
      || (frame.contains("ryo2d::") && frame.contains("console"))
  })
}

/// The UI's drawing calls: what they allocate is the UI recording a draw
const UI_DRAWS: [&str; 3] = ["UiContext>::text", "UiText", "UiContext>::push_clip"];

/// A step's allocations: the tools' own and the UI's recording of the
/// tools' draws, each by site (the tools' function and the one it called),
/// and how many it made altogether
#[derive(Default)]
struct Frame {
  tools: BTreeMap<String, usize>,
  draws: BTreeMap<String, usize>,
  count: usize,
}

/// A step of `engine`, its allocations traced and told apart
fn traced(engine: &mut HeadlessEngine) -> Frame {
  TRACES.with(|t| t.borrow_mut().clear());
  let before = allocations();
  TRACING.with(|c| c.set(true));
  engine.step().expect("a frame");
  TRACING.with(|c| c.set(false));
  let mut frame = Frame {
    count: allocations() - before,
    ..Frame::default()
  };
  let traces = TRACES.with(|t| std::mem::take(&mut *t.borrow_mut()));
  for trace in traces {
    let text = trace.to_string();
    let frames = functions(&text);
    let Some(tool) = tools_function(&frames) else {
      continue;
    };
    let at = frames.iter().position(|f| *f == tool).unwrap_or(0);
    let called = frames[at.saturating_sub(1)];
    let site = format!("{called} <- {tool}");
    let kind = if UI_DRAWS.iter().any(|draw| called.contains(draw)) {
      &mut frame.draws
    } else {
      &mut frame.tools
    };
    *kind.entry(site).or_insert(0) += 1;
  }
  frame
}

/// `MEASURED` traced frames of `engine` after `WARM` untraced ones: the
/// tools' allocations by site, over them all
fn measure(engine: &mut HeadlessEngine) -> BTreeMap<String, usize> {
  for _ in 0..WARM {
    engine.step().expect("a warm-up frame");
  }
  let mut tools: BTreeMap<String, usize> = BTreeMap::new();
  let mut draws: BTreeMap<String, usize> = BTreeMap::new();
  let mut count = 0;
  for _ in 0..MEASURED {
    let frame = traced(engine);
    count += frame.count;
    for (site, n) in frame.tools {
      *tools.entry(site).or_insert(0) += n;
    }
    for (site, n) in frame.draws {
      *draws.entry(site).or_insert(0) += n;
    }
  }
  println!(
    "{MEASURED} frames made {count} allocations; the UI's recording of the tools' draws \
     (printed, not asserted): {draws:#?}"
  );
  println!("the tools' own: {tools:#?}");
  tools
}

fn engine() -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(320, 180).start_scene(Idle))
}

#[test]
fn tools_closed_cost_nothing_they_can_avoid() {
  let Some(mut engine) = engine() else {
    return;
  };
  // A headless engine has no dev window; the console starts closed
  let tools = measure(&mut engine);
  assert!(
    tools.is_empty(),
    "the closed console allocated in {MEASURED} frames: {tools:#?}"
  );
}

#[test]
fn an_open_idle_console_allocates_nothing_of_its_own() {
  let Some(mut engine) = engine() else {
    return;
  };
  engine.console_fill(LINES);
  engine.console_open(true);
  let tools = measure(&mut engine);
  assert!(
    tools.is_empty(),
    "the open console, on {LINES} lines with nothing new, allocated in {MEASURED} frames: \
     {tools:#?}"
  );
}
