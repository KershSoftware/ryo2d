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

    let center_x = screen_w as f32 / 2.0;

    // Game Over title
    let title = "GAME OVER";
    let (title_w, _) = ui.text_size(title);
    ui.draw_text(
      title,
      center_x - title_w as f32 / 2.0,
      200.0,
      Color::rgb(255, 50, 50),
    );

    // Survival time
    let time_text = format!("Survived: {:.1}s", self.survival_time);
    let (time_w, _) = ui.text_size(&time_text);
    ui.draw_text(
      &time_text,
      center_x - time_w as f32 / 2.0,
      280.0,
      Color::rgb(255, 255, 255),
    );

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
    let (best_w, _) = ui.text_size(&best_text);
    ui.draw_text(&best_text, center_x - best_w as f32 / 2.0, 320.0, color);

    // Restart prompt
    let restart_text = "Press SPACE or A to Retry";
    let (restart_w, _) = ui.text_size(restart_text);
    ui.draw_text(
      restart_text,
      center_x - restart_w as f32 / 2.0,
      450.0,
      Color::rgb(100, 255, 100),
    );

    // Menu prompt
    let menu_text = "Press ESC or B for Menu";
    let (menu_w, _) = ui.text_size(menu_text);
    ui.draw_text(
      menu_text,
      center_x - menu_w as f32 / 2.0,
      480.0,
      Color::rgb(150, 150, 150),
    );
    SceneAction::Continue
  }
}
