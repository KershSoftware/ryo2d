//! Game Over Scene - Displays final score and allows restart

use ryo2d::prelude::*;
use std::time::Duration;

use super::MenuScene;
use crate::constants::*;

pub struct GameOverScene {
  score: u32,
  high_score: u32,
  is_new_high_score: bool,
}

impl GameOverScene {
  pub fn new(score: u32, high_score: u32, is_new_high_score: bool) -> Self {
    Self {
      score,
      high_score,
      is_new_high_score,
    }
  }
}

impl Scene for GameOverScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .camera
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Space or Enter to restart
    if ctx.input.key_pressed(KeyCode::Space) || ctx.input.key_pressed(KeyCode::Enter) {
      return SceneAction::SwitchWith(
        Box::new(super::GameScene::new(self.high_score)),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    // Escape to return to menu
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::SwitchWith(
        Box::new(MenuScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let (screen_w, screen_h) = ui.screen_size();

    // Semi-transparent overlay
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::new(0, 0, 0, 180),
    );

    // Game Over title
    ui.draw_text_anchored("GAME OVER", Anchor::TopCenter, 0.0, 150.0, Color::WHITE);

    // Score display
    let score_text = format!("Score: {}", self.score);
    ui.draw_text_anchored(&score_text, Anchor::TopCenter, 0.0, 220.0, Color::WHITE);

    // High score
    if self.is_new_high_score {
      ui.draw_text_anchored(
        "NEW HIGH SCORE!",
        Anchor::TopCenter,
        0.0,
        270.0,
        Color::rgb(255, 215, 0), // Gold
      );
    } else {
      let high_text = format!("High Score: {}", self.high_score);
      ui.draw_text_anchored(
        &high_text,
        Anchor::TopCenter,
        0.0,
        270.0,
        Color::rgb(200, 200, 200),
      );
    }

    // Restart button
    let button_w = 200.0;
    let button_h = 50.0;
    let button_x = screen_w as f32 / 2.0 - button_w / 2.0;

    if ui.button("RESTART", Rect::new(button_x, 350.0, button_w, button_h)) {
      return SceneAction::SwitchWith(
        Box::new(super::GameScene::new(self.high_score)),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    // Menu button
    if ui.button("MENU", Rect::new(button_x, 420.0, button_w, button_h)) {
      return SceneAction::SwitchWith(
        Box::new(MenuScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    // Keyboard hints
    ui.draw_text_anchored(
      "SPACE to restart  |  ESC for menu",
      Anchor::TopCenter,
      0.0,
      500.0,
      Color::rgb(150, 150, 150),
    );
    SceneAction::Continue
  }
}
