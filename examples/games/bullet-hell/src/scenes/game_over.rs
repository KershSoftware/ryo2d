//! Game over scene

use crate::save::load_best_time;
use ryo2d::prelude::*;

use super::GameScene;

pub struct GameOverScene {
  survival_time: f32,
  best_time: f32,
}

impl GameOverScene {
  pub fn new(survival_time: f32) -> Self {
    Self {
      survival_time,
      best_time: load_best_time(),
    }
  }
}

impl Scene for GameOverScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Clear the world to remove all entities (including bullets with trails)
    ctx.world.clear();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Restart game (keyboard or gamepad)
    if ctx.input.key_pressed(KeyCode::Space)
      || ctx.input.key_pressed(KeyCode::Enter)
      || ctx.input.gamepad_button_pressed(GamepadButton::South)
      || ctx.input.gamepad_button_pressed(GamepadButton::Start)
    {
      return SceneAction::Switch(Box::new(GameScene::new()));
    }
    // Return to menu (keyboard or gamepad)
    if ctx.input.key_pressed(KeyCode::Escape)
      || ctx.input.gamepad_button_pressed(GamepadButton::East)
    {
      return SceneAction::Switch(Box::new(super::MenuScene::new()));
    }
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let (screen_w, screen_h) = ui.screen_size();

    // Clear to dark red background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::rgb(40, 20, 20),
    );

    // Game Over title
    ui.draw_text_anchored("GAME OVER", Anchor::TopCenter, 0.0, 200.0, Color::rgb(255, 50, 50));

    // Survival time
    let time_text = format!("Survived: {:.1}s", self.survival_time);
    ui.draw_text_anchored(&time_text, Anchor::TopCenter, 0.0, 280.0, Color::rgb(255, 255, 255));

    // Best time
    let is_new_record = self.survival_time >= self.best_time && self.best_time > 0.0;
    let best_text = if is_new_record {
      "NEW RECORD!".to_string()
    } else {
      format!("Best: {:.1}s", self.best_time)
    };
    let color = if is_new_record {
      Color::rgb(255, 215, 0)
    } else {
      Color::rgb(200, 200, 200)
    };
    ui.draw_text_anchored(&best_text, Anchor::TopCenter, 0.0, 320.0, color);

    // Restart prompt
    ui.draw_text_anchored(
      "Press SPACE or A to Retry",
      Anchor::TopCenter,
      0.0,
      450.0,
      Color::rgb(100, 255, 100),
    );

    // Menu prompt
    ui.draw_text_anchored(
      "Press ESC or B for Menu",
      Anchor::TopCenter,
      0.0,
      480.0,
      Color::rgb(150, 150, 150),
    );
    SceneAction::Continue
  }
}
