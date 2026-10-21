//! UI scenes shared by the UI rendering tests (`ui_renderer`, `ui_retain`)

use std::sync::{Arc, Mutex, MutexGuard};

use ryo2d::prelude::*;

/// `shared`'s value, also after a failed test poisoned it
pub fn lock<T>(shared: &Mutex<T>) -> MutexGuard<'_, T> {
  shared
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Commands that draw nothing: off the UI image, outside their clip, and
/// under an empty clip
pub struct Culled;

impl Scene for Culled {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_rect(Rect::new(-500.0, -500.0, 40.0, 20.0), Color::WHITE);
    ui.draw_text("Far away", 900.0, 900.0, Color::WHITE);
    ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
    ui.draw_rect(Rect::new(100.0, 100.0, 40.0, 20.0), Color::WHITE);
    ui.pop_clip();
    ui.push_clip(Rect::new(10.0, 10.0, 0.0, 50.0));
    ui.draw_text("Clipped away", 12.0, 12.0, Color::WHITE);
    ui.pop_clip();
    SceneAction::Continue
  }
}

/// A panel and a label while the shared flag is set, nothing otherwise
pub struct Toggle(pub Arc<Mutex<bool>>);

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
