//! Text and UI frame costs: a HUD of 100 texts and 50 panels.

use criterion::{Criterion, criterion_group, criterion_main};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// A HUD: 100 lines of text over 50 translucent panels
struct Hud;

impl Scene for Hud {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    for i in 0..50 {
      let x = (i % 10) as f32 * 64.0;
      let y = (i / 10) as f32 * 64.0;
      ui.draw_rect(Rect::new(x, y, 60.0, 60.0), Color::new(20, 20, 30, 200));
    }
    for i in 0..100 {
      let x = (i % 5) as f32 * 128.0 + 4.0;
      let y = (i / 5) as f32 * 16.0 + 4.0;
      ui.draw_text(&format!("Score {i}: {}", i * 37), x, y, Color::WHITE);
    }
    SceneAction::Continue
  }
}

fn hud_frame(c: &mut Criterion) {
  let builder = EngineBuilder::new().resolution(640, 360).start_scene(Hud);
  // Needs a GPU; without one this benchmark is skipped
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(2).expect("warm-up frames");
  c.bench_function("headless frame, HUD of 100 texts and 50 panels", |b| {
    b.iter(|| engine.step().expect("frame"))
  });
}

criterion_group!(benches, hud_frame);
criterion_main!(benches);
