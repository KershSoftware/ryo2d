//! The UI's render-side cost with a GPU (`HeadlessEngine`, 1280x720): the
//! 200-widget settings node screen unchanged, with a label changing every
//! frame, and scrolling, with UI render retention on and off, and an empty
//! frame. Before criterion runs, a guard checks 4g's targets on medians of
//! the render step's CPU time (not the UI pass): it asserts the idle and
//! retained-no-slower targets and reports the one-label target.

use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;
use ryo2d::testing::fixtures::settings_tree;

/// Frames each median is taken over
const FRAMES: usize = 2000;

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

fn settings(label: bool) -> Settings {
  Settings { label, frame: 0 }
}

/// An engine on `scene`, warmed up
fn engine(scene: impl Scene + 'static, retain: bool) -> Option<HeadlessEngine> {
  let builder = EngineBuilder::new()
    .resolution(1280, 720)
    .ui_retain(retain)
    .start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(60).ok()?;
  Some(engine)
}

/// The median render step over `FRAMES` frames
fn median(engine: &mut HeadlessEngine) -> Duration {
  let mut times: Vec<Duration> = (0..FRAMES)
    .map(|_| {
      let _ = engine.step();
      engine.render_time()
    })
    .collect();
  times.sort();
  times[times.len() / 2]
}

/// 4g's targets on render-side work over an empty frame. Asserted: an idle
/// settings screen adds under 5 µs, and a changing frame is no slower
/// retained than not. Reported only: one changing label under 60 µs, which
/// approach B alone cannot reach (the batch's prepare, upload and encode
/// remain; approach C in docs/backlog/ui-retained-nodes.md)
fn guard() {
  let (Some(mut empty), Some(mut idle), Some(mut idle_off), Some(mut label), Some(mut label_off)) = (
    engine(Empty, true),
    engine(settings(false), true),
    engine(settings(false), false),
    engine(settings(true), true),
    engine(settings(true), false),
  ) else {
    return;
  };
  let empty = median(&mut empty);
  let over = |engine: &mut HeadlessEngine| median(engine).saturating_sub(empty);
  let (idle, idle_off, label, label_off) = (
    over(&mut idle),
    over(&mut idle_off),
    over(&mut label),
    over(&mut label_off),
  );
  eprintln!(
    "ui_render guard, render step over an empty frame ({empty:?}): idle {idle:?} (retention off {idle_off:?}), one label {label:?} (off {label_off:?})"
  );
  assert!(
    idle < Duration::from_micros(5),
    "an idle UI costs {idle:?} to render"
  );
  let verdict = if label < Duration::from_micros(60) {
    "met".to_string()
  } else {
    "not met (the batch's prepare, upload and encode remain; approach C in docs/backlog/ui-retained-nodes.md)".to_string()
  };
  eprintln!("ui_render guard: one label {label:?} against the 4g target of 60 µs: {verdict}");
  assert!(
    label <= label_off,
    "retained {label:?}, not retained {label_off:?}"
  );
}

fn bench_ui_render(c: &mut Criterion) {
  guard();
  for (name, label, retain) in [
    ("settings_idle", false, true),
    ("settings_idle_unretained", false, false),
    ("settings_label", true, true),
    ("settings_label_unretained", true, false),
  ] {
    let Some(mut engine) = engine(settings(label), retain) else {
      return;
    };
    c.bench_function(name, |b| b.iter(|| engine.step()));
  }
  for (name, retain) in [
    ("settings_scrolling", true),
    ("settings_scrolling_unretained", false),
  ] {
    let Some(mut engine) = engine(settings(false), retain) else {
      return;
    };
    // Over a row of the scroll area (the panel centre is not over it)
    engine.mouse_move(520.0, 238.0);
    let mut frame = 0u32;
    c.bench_function(name, |b| {
      b.iter(|| {
        frame += 1;
        // Down for a second, up for a second: the list keeps moving
        let dy = if (frame / 120).is_multiple_of(2) {
          -1.0
        } else {
          1.0
        };
        engine.wheel(0.0, dy);
        engine.step()
      })
    });
  }
  let Some(mut engine) = engine(Empty, true) else {
    return;
  };
  c.bench_function("empty_frame", |b| b.iter(|| engine.step()));
}

criterion_group!(benches, bench_ui_render);
criterion_main!(benches);
