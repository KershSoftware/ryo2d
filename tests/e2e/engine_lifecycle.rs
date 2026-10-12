//! Engine lifecycle e2e tests, run headless.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

struct Solid([u8; 3]);

impl Scene for Solid {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let [r, g, b] = self.0;
    ui.draw_rect(Rect::new(0.0, 0.0, 64.0, 36.0), Color::rgb(r, g, b));
    SceneAction::Continue
  }
}

/// Pushes `Solid(blue)` on its second update; the pushed scene pops on its second update.
struct Pusher {
  updates: u32,
}

struct PopsOnSecond {
  updates: u32,
}

impl Scene for Pusher {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.updates += 1;
    if self.updates == 2 {
      SceneAction::Push(Box::new(PopsOnSecond { updates: 0 }))
    } else {
      SceneAction::Continue
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_rect(Rect::new(0.0, 0.0, 64.0, 36.0), Color::rgb(255, 0, 0));
    SceneAction::Continue
  }
}

impl Scene for PopsOnSecond {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.updates += 1;
    if self.updates == 2 {
      SceneAction::Pop
    } else {
      SceneAction::Continue
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_rect(Rect::new(0.0, 0.0, 64.0, 36.0), Color::rgb(0, 0, 255));
    SceneAction::Continue
  }
}

#[test]
fn engine_builds_with_defaults() {
  let Some(mut engine) =
    HeadlessEngine::new_or_skip(EngineBuilder::new().start_scene(Solid([0, 255, 0])))
  else {
    return;
  };
  engine.step().unwrap();
  let frame = engine.frame().unwrap();
  assert_eq!(frame.get_pixel(10, 10).0, [0, 255, 0, 255]);
}

#[test]
fn engine_can_be_dropped_and_rebuilt() {
  for _ in 0..3 {
    let Some(mut engine) = HeadlessEngine::new_or_skip(
      EngineBuilder::new()
        .resolution(64, 36)
        .start_scene(Solid([0, 0, 255])),
    ) else {
      return;
    };
    engine.step_frames(2).unwrap();
  }
}

#[test]
fn scene_push_and_pop_render_the_top_scene() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(64, 36)
      .start_scene(Pusher { updates: 0 }),
  ) else {
    return;
  };
  let colour = |engine: &HeadlessEngine| engine.frame().unwrap().get_pixel(10, 10).0;

  engine.step_frames(3).unwrap();
  assert_eq!(colour(&engine), [0, 0, 255, 255], "pushed scene on top");
  engine.step_frames(3).unwrap();
  assert_eq!(
    colour(&engine),
    [255, 0, 0, 255],
    "popped back to the first scene"
  );
}

/// Counts its fixed updates.
struct CountsUpdates(std::sync::Arc<std::sync::atomic::AtomicU32>);

impl Scene for CountsUpdates {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    SceneAction::Continue
  }
}

#[test]
fn each_headless_step_runs_one_fixed_update_at_any_rate() {
  // A headless step advances the clock by exactly one fixed timestep
  for hz in [30, 60, 120] {
    let updates = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let builder = EngineBuilder::new()
      .resolution(32, 32)
      .fixed_hz(hz)
      .start_scene(CountsUpdates(updates.clone()));
    let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
      return;
    };
    engine.step_frames(5).unwrap();
    assert_eq!(
      updates.load(std::sync::atomic::Ordering::SeqCst),
      5,
      "{hz} Hz"
    );
    assert_eq!(
      engine.world().resource::<Time>().fixed.step_count(),
      5,
      "{hz} Hz"
    );
  }
}
