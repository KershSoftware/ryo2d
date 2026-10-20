//! UI render retention in the engine: with it on and off, two engines driven
//! alike draw the same pixels every frame; an unchanged UI keeps its image,
//! and a changing one rebuilds only its changed texts

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::PixelPair;

/// Ten panels with labels, and a frame counter when `counting`
struct Hud {
  counting: bool,
  frame: u32,
}

impl Scene for Hud {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    for i in 0..10 {
      let y = 8.0 + i as f32 * 20.0;
      ui.draw_rect(
        Rect::new(4.0, y - 2.0, 150.0, 18.0),
        Color::new(20, 20, 30, 200),
      );
      ui.draw_text(&format!("Label {i}"), 8.0, y, Color::WHITE);
    }
    if self.counting {
      self.frame += 1;
      ui.draw_text(&format!("Frame {}", self.frame), 200.0, 8.0, Color::WHITE);
    }
    SceneAction::Continue
  }
}

fn hud(counting: bool) -> impl Fn() -> EngineBuilder {
  move || {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Hud { counting, frame: 0 })
  }
}

fn lock<T>(shared: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  shared
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn an_unchanged_ui_keeps_its_image() {
  let Some(mut pair) = PixelPair::new(hud(false)) else {
    return;
  };
  pair.frames(10, "the HUD opening");
  let before = pair.kept();
  pair.frames(10, "the HUD unchanged");
  assert_eq!(
    pair.kept() - before,
    10,
    "every unchanged frame kept the image"
  );
}

#[test]
fn a_changing_label_redraws_every_frame() {
  let Some(mut pair) = PixelPair::new(hud(true)) else {
    return;
  };
  pair.frames(5, "counting");
  for _ in 0..10 {
    pair.frames(1, "counting");
    assert!(!pair.on.ui_render_stats().kept);
  }
}

#[test]
fn a_resize_redraws_the_ui() {
  let Some(mut pair) = PixelPair::new(hud(false)) else {
    return;
  };
  pair.frames(10, "the HUD");
  assert!(pair.on.ui_render_stats().kept);
  pair.each(|engine| engine.resize(400, 300));
  pair.frames(1, "resized");
  assert!(!pair.on.ui_render_stats().kept, "a new UI image is drawn");
  pair.frames(10, "resized, unchanged");
  assert!(pair.on.ui_render_stats().kept);
}

/// A panel and a label while the shared flag is set, nothing otherwise
struct Toggle(Arc<Mutex<bool>>);

impl Scene for Toggle {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if *lock(&self.0) {
      ui.draw_rect(
        Rect::new(10.0, 10.0, 120.0, 40.0),
        Color::new(40, 60, 90, 255),
      );
      ui.draw_text("Shown", 20.0, 20.0, Color::WHITE);
    }
    SceneAction::Continue
  }
}

#[test]
fn the_ui_hidden_then_shown_comes_back() {
  let shown = Arc::new(Mutex::new(true));
  let make = {
    let shown = shown.clone();
    move || {
      EngineBuilder::new()
        .resolution(320, 240)
        .start_scene(Toggle(shown.clone()))
    }
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(10, "shown");
  *lock(&shown) = false;
  pair.frames(5, "hidden");
  *lock(&shown) = true;
  pair.frames(5, "shown again");
  assert!(pair.on.ui_render_stats().kept);
}

#[test]
fn a_disabled_ui_pass_hides_the_kept_ui() {
  let Some(mut pair) = PixelPair::new(hud(false)) else {
    return;
  };
  pair.frames(10, "the HUD");
  let set_ui_pass = |pair: &mut PixelPair, on: bool| {
    pair.each(|engine| {
      engine
        .world_mut()
        .resource_mut::<RenderPipeline>()
        .set_enabled(pass_ids::UI, on)
        .expect("the ui pass exists");
    });
  };
  set_ui_pass(&mut pair, false);
  pair.frames(5, "the ui pass off");
  set_ui_pass(&mut pair, true);
  pair.frames(5, "the ui pass on again");
}
