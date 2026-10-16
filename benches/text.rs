//! Text and UI frame costs: a HUD of 100 texts and 50 panels, 1,000 cached
//! UI texts and 200 world labels.

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

/// 1,000 unchanging UI texts: every frame is layout-cache hits
struct StaticTexts;

impl Scene for StaticTexts {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    for i in 0..1000 {
      let x = (i % 20) as f32 * 32.0;
      let y = (i / 20) as f32 * 7.0;
      ui.draw_text_styled("label", x, y, &TextStyle::new().size(6.0));
    }
    SceneAction::Continue
  }
}

/// 200 world labels on entities
struct Labels;

impl Scene for Labels {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    for i in 0..200 {
      let (x, y) = (
        (i % 20) as f32 * 30.0 - 300.0,
        (i / 20) as f32 * 30.0 - 150.0,
      );
      ctx
        .world
        .spawn((Transform::from_xy(x, y), Text2d::new(format!("#{i}"))));
    }
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
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

fn static_texts_frame(c: &mut Criterion) {
  let builder = EngineBuilder::new()
    .resolution(640, 360)
    .start_scene(StaticTexts);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(2).expect("warm-up frames");
  c.bench_function("headless frame, 1000 cached UI texts", |b| {
    b.iter(|| engine.step().expect("frame"))
  });
}

fn world_labels_frame(c: &mut Criterion) {
  let builder = EngineBuilder::new()
    .resolution(640, 360)
    .start_scene(Labels);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(2).expect("warm-up frames");
  c.bench_function("headless frame, 200 world text labels", |b| {
    b.iter(|| engine.step().expect("frame"))
  });
}

criterion_group!(benches, hud_frame, static_texts_frame, world_labels_frame);
criterion_main!(benches);
