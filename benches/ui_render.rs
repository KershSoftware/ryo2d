//! The UI's render-side cost with a GPU (`HeadlessEngine`, 1280x720): the
//! 200-widget settings node screen unchanged, with a label changing every
//! frame, and scrolling, with UI render retention on and off, and an empty
//! frame.
//!
//! Before criterion runs, a guard measures the render step of each
//! (`HeadlessEngine::render_time`, the wall time of building, uploading and
//! encoding the frame; not the UI pass) against an empty frame. An engine
//! stepped alone is paced by the GPU: when its CPU work is small (an empty
//! frame most of all) its render step waits on the GPU's backlog, and that
//! wait reads as cost. Stepping the engines in turn does not cure it alone,
//! as the GPU takes longer to finish a frame (about 200 to 300 µs on an
//! Apple GPU) than the CPU takes to step one. So the guard steps its engines
//! in turn, one frame each, and lets the GPU finish all submitted work
//! before each measured step (outside the measure), as a frame at 60 Hz
//! finds it. Each median is taken over the same rounds, so a change in the
//! machine's load moves them all alike. Wall time here equals the render
//! step's thread CPU time to within about 0.5 µs.

use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use ryo2d::prelude::*;
use ryo2d::testing::fixtures::settings_tree;
use ryo2d::testing::{HeadlessEngine, shared_gpu};

/// Rounds each median is taken over: one frame of every engine a round
const FRAMES: usize = 2000;
/// Wheel lines each way: fewer than the list scrolls, so it never rests at
/// an end and every frame moves it
const SWING: u32 = 20;
/// Over a row of the scroll area (the panel centre is not over it)
const OVER_LIST: (f32, f32) = (520.0, 238.0);
/// An idle settings screen's render step over an empty frame's (4g)
const IDLE_TARGET: Duration = Duration::from_micros(5);
/// One changing label's render step over an empty frame's (4h)
const LABEL_TARGET: Duration = Duration::from_micros(40);

struct Empty;

impl Scene for Empty {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// The settings screen; with `label`, a counter over it changes every frame
struct Settings {
  label: bool,
  frame: u32,
}

impl Scene for Settings {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(settings_tree());
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if self.label {
      self.frame += 1;
      ui.draw_text(&format!("Frame {}", self.frame), 8.0, 8.0, Color::WHITE);
    }
    SceneAction::Continue
  }
}

/// What a measured engine shows
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
  Empty,
  /// The settings screen, unchanged
  Idle,
  /// The settings screen and a label changing every frame
  Label,
  /// The settings screen, its list turned by the wheel every frame
  Scrolling,
}

/// An engine on `screen`, warmed up; scrolling, the pointer over the list
fn engine(screen: Screen, retain: bool) -> Option<HeadlessEngine> {
  let builder = EngineBuilder::new().resolution(1280, 720).ui_retain(retain);
  let builder = match screen {
    Screen::Empty => builder.start_scene(Empty),
    Screen::Idle | Screen::Scrolling => builder.start_scene(Settings {
      label: false,
      frame: 0,
    }),
    Screen::Label => builder.start_scene(Settings {
      label: true,
      frame: 0,
    }),
  };
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  if screen == Screen::Scrolling {
    engine.mouse_move(OVER_LIST.0, OVER_LIST.1);
  }
  engine.step_frames(60).ok()?;
  Some(engine)
}

/// The wheel of a scrolling screen: one line a frame, `SWING` down, then
/// `SWING` up
#[derive(Default)]
struct Wheel {
  frame: u32,
}

impl Wheel {
  fn turn(&mut self, engine: &mut HeadlessEngine) {
    self.frame += 1;
    let dy = if (self.frame / SWING).is_multiple_of(2) {
      -1.0
    } else {
      1.0
    };
    engine.wheel(0.0, dy);
  }
}

/// One engine the guard steps, and its render steps so far
struct Probe {
  screen: Screen,
  retain: bool,
  engine: HeadlessEngine,
  wheel: Wheel,
  times: Vec<Duration>,
  /// Frames whose UI image was kept
  kept: usize,
}

impl Probe {
  fn new(screen: Screen, retain: bool) -> Option<Probe> {
    Some(Probe {
      screen,
      retain,
      engine: engine(screen, retain)?,
      wheel: Wheel::default(),
      times: Vec::with_capacity(FRAMES),
      kept: 0,
    })
  }

  fn step(&mut self) {
    if self.screen == Screen::Scrolling {
      self.wheel.turn(&mut self.engine);
    }
    self.engine.step().expect("a guard frame");
    self.times.push(self.engine.render_time());
    self.kept += usize::from(self.engine.ui_render_stats().kept);
  }

  fn median(&mut self) -> Duration {
    self.times.sort_unstable();
    self.times[self.times.len() / 2]
  }
}

/// `time` over `empty`, in µs, signed
fn over(time: Duration, empty: Duration) -> f64 {
  (time.as_secs_f64() - empty.as_secs_f64()) * 1e6
}

/// The UI's render targets over an empty frame: an idle settings screen
/// adds under 5 µs (4g), a changing frame is no slower retained than not
/// (4g), and one changing label adds under 40 µs (4h). Every median is
/// printed, and each target with its verdict; asserted are that one label
/// is no slower retained and that the scrolling list moves every frame.
fn guard() {
  let screens = [
    (Screen::Empty, true),
    (Screen::Idle, true),
    (Screen::Label, true),
    (Screen::Scrolling, true),
    (Screen::Idle, false),
    (Screen::Label, false),
    (Screen::Scrolling, false),
  ];
  let mut probes = Vec::new();
  for (screen, retain) in screens {
    let Some(probe) = Probe::new(screen, retain) else {
      return;
    };
    probes.push(probe);
  }
  // Every headless engine draws on the one shared device
  let Ok((device, _)) = shared_gpu() else {
    return;
  };
  // Round robin, each round started by the next engine, so none always
  // follows the same one
  for round in 0..FRAMES {
    for i in 0..probes.len() {
      device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the GPU finishes its work");
      let at = (round + i) % probes.len();
      probes[at].step();
    }
  }
  let mut median = |screen: Screen, retain: bool| {
    let probe = probes
      .iter_mut()
      .find(|probe| probe.screen == screen && probe.retain == retain)
      .expect("a probe of each screen");
    (probe.median(), probe.kept)
  };
  let (empty, _) = median(Screen::Empty, true);
  let (idle, idle_kept) = median(Screen::Idle, true);
  let (label, _) = median(Screen::Label, true);
  let (scrolling, scrolling_kept) = median(Screen::Scrolling, true);
  let (idle_off, _) = median(Screen::Idle, false);
  let (label_off, _) = median(Screen::Label, false);
  let (scrolling_off, _) = median(Screen::Scrolling, false);
  let us = |time: Duration| over(time, empty);
  let verdict = |met: bool| if met { "met" } else { "missed" };
  let against = |time: Duration, target: Duration| {
    let met = time.saturating_sub(empty) < target;
    format!(
      "{:+.1} µs (target {} µs: {})",
      us(time),
      target.as_micros(),
      verdict(met)
    )
  };
  let off = |time: Duration, retained: Duration| {
    let met = retained <= time;
    format!("{:+.1} µs (retained no slower: {})", us(time), verdict(met))
  };
  eprintln!(
    "ui_render guard, render step over an empty frame ({:.1} µs), medians of {FRAMES} \
     interleaved frames: idle {} ({idle_kept} kept), one label {}, scrolling {:+.1} µs \
     ({scrolling_kept} kept); retention off: idle {:+.1} µs, one label {}, scrolling {}",
    empty.as_secs_f64() * 1e6,
    against(idle, IDLE_TARGET),
    against(label, LABEL_TARGET),
    us(scrolling),
    us(idle_off),
    off(label_off, label),
    off(scrolling_off, scrolling),
  );
  assert_eq!(scrolling_kept, 0, "the scrolling list rested");
  assert!(
    label <= label_off,
    "one label: retained {label:?}, not retained {label_off:?}"
  );
}

/// Where the settings screen's focusable widgets are: the list's move as
/// it scrolls
fn rows(engine: &HeadlessEngine) -> Vec<Rect> {
  engine
    .ui_focusables()
    .into_iter()
    .map(|(_, rect)| rect)
    .collect()
}

fn bench_ui_render(c: &mut Criterion) {
  guard();
  for (name, screen, retain) in [
    ("settings_idle", Screen::Idle, true),
    ("settings_idle_unretained", Screen::Idle, false),
    ("settings_label", Screen::Label, true),
    ("settings_label_unretained", Screen::Label, false),
  ] {
    let Some(mut engine) = engine(screen, retain) else {
      return;
    };
    c.bench_function(name, |b| b.iter(|| engine.step()));
  }
  for (name, retain) in [
    ("settings_scrolling", true),
    ("settings_scrolling_unretained", false),
  ] {
    let Some(mut engine) = engine(Screen::Scrolling, retain) else {
      return;
    };
    let mut wheel = Wheel::default();
    let before = rows(&engine);
    for _ in 0..SWING / 2 {
      wheel.turn(&mut engine);
      engine.step().expect("a scrolling frame");
    }
    assert_ne!(rows(&engine), before, "{name}: the wheel scrolls the list");
    c.bench_function(name, |b| {
      b.iter(|| {
        wheel.turn(&mut engine);
        engine.step()
      })
    });
  }
  let Some(mut engine) = engine(Screen::Empty, true) else {
    return;
  };
  c.bench_function("empty_frame", |b| b.iter(|| engine.step()));
}

criterion_group!(benches, bench_ui_render);
criterion_main!(benches);
