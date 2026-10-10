//! E2E tests for ryo2d::testing::HeadlessEngine.

use ryo2d::core::config::UserPreferenceFilter;
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, StepOutcome};

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// Draws a 20x20 red square at (100, 50) and a 30x30 green UI rect at (10, 10).
struct Squares;

impl Scene for Squares {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(20, 20, 255, 0, 0, 255), "red");
    ctx
      .world
      .spawn((Transform::new(100.0, 50.0), Renderable::sprite(sprite)));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    ui.draw_rect(Rect::new(10.0, 10.0, 30.0, 30.0), Color::rgb(0, 255, 0));
    SceneAction::Continue
  }
}

/// A square that moves 2px right per update, for determinism checks.
struct Mover {
  x: f32,
}

impl Scene for Mover {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.x += 2.0;
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    ui.draw_rect(Rect::new(self.x, 40.0, 8.0, 8.0), Color::rgb(255, 255, 0));
    SceneAction::Continue
  }
}

/// Quits on its third update.
struct QuitsOnThird {
  updates: u32,
}

impl Scene for QuitsOnThird {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.updates += 1;
    if self.updates == 3 {
      SceneAction::Quit
    } else {
      SceneAction::Continue
    }
  }
}

fn engine(scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(320, 180).start_scene(scene))
}

#[test]
fn sprite_and_ui_appear_in_frame() {
  let Some(mut engine) = engine(Squares) else {
    return;
  };

  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();

  assert_eq!(frame.dimensions(), (320, 180));
  assert_eq!(frame.get_pixel(110, 60).0, RED);
  assert_eq!(frame.get_pixel(20, 20).0, GREEN);
}

#[test]
fn frame_before_first_step_is_an_error() {
  let Some(engine) = engine(Squares) else {
    return;
  };
  assert!(engine.frame().is_err());
}

#[test]
fn same_steps_give_identical_frames() {
  let Some(mut a) = engine(Mover { x: 0.0 }) else {
    return;
  };
  let Some(mut b) = engine(Mover { x: 0.0 }) else {
    return;
  };

  a.step_frames(20).unwrap();
  b.step_frames(20).unwrap();

  assert_eq!(a.frame().unwrap().as_raw(), b.frame().unwrap().as_raw());
  assert_eq!(a.frame_count(), 20);
}

#[test]
fn two_engines_share_the_gpu() {
  let Some(mut a) = engine(Squares) else { return };
  let Some(mut b) = engine(Squares) else { return };
  a.step().unwrap();
  b.step().unwrap();
  drop(a);
  b.step().unwrap();
  assert_eq!(b.frame_count(), 2);
}

#[test]
fn quit_stops_stepping() {
  let Some(mut engine) = engine(QuitsOnThird { updates: 0 }) else {
    return;
  };

  let outcome = engine.step_frames(10).unwrap();

  assert_eq!(outcome, StepOutcome::Quit);
  assert_eq!(engine.frame_count(), 2);
}

#[test]
fn step_after_quit_stays_quit() {
  let Some(mut engine) = engine(QuitsOnThird { updates: 0 }) else {
    return;
  };
  engine.step_frames(10).unwrap();

  assert_eq!(engine.step().unwrap(), StepOutcome::Quit);
  assert_eq!(engine.frame_count(), 2);
}

#[test]
fn headless_ignores_user_preferences_and_fps_overlay() {
  let prefs = std::path::Path::new("data/system.json");
  let existed = prefs.exists();
  let Some(mut with_prefs) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(320, 180)
      .show_fps(true)
      .with_user_preferences(UserPreferenceFilter::all())
      .start_scene(Squares),
  ) else {
    return;
  };
  let created = !existed && prefs.exists();
  if created {
    let _ = std::fs::remove_dir_all("data");
  }
  assert!(!created, "headless build created data/system.json");

  let Some(mut plain) = engine(Squares) else {
    return;
  };
  with_prefs.step_frames(2).unwrap();
  plain.step_frames(2).unwrap();
  assert_eq!(
    with_prefs.frame().unwrap().as_raw(),
    plain.frame().unwrap().as_raw()
  );
}
