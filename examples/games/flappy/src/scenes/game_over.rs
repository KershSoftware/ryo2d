//! Game Over Scene - Displays final score and allows restart

use ryo2d::prelude::*;
use std::time::Duration;

use super::MenuScene;
use crate::constants::*;

pub struct GameOverScene {
  score: u32,
  high_score: u32,
  is_new_high_score: bool,
  restart_clicked: bool,
  menu_clicked: bool,
}

impl GameOverScene {
  pub fn new(score: u32, high_score: u32, is_new_high_score: bool) -> Self {
    Self {
      score,
      high_score,
      is_new_high_score,
      restart_clicked: false,
      menu_clicked: false,
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
    if self.restart_clicked
      || ctx.input.key_pressed(KeyCode::Space)
      || ctx.input.key_pressed(KeyCode::Enter)
    {
      self.restart_clicked = false;
      return SceneAction::SwitchWith(
        Box::new(super::GameScene::new(self.high_score)),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    // Escape to return to menu
    if self.menu_clicked || ctx.input.key_pressed(KeyCode::Escape) {
      self.menu_clicked = false;
      return SceneAction::SwitchWith(
        Box::new(MenuScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Semi-transparent overlay
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::new(0, 0, 0, 180),
    );

    // Game Over title
    let title = "GAME OVER";
    let (title_w, _) = ui.text_size(title);
    ui.draw_text(
      title,
      screen_w as f32 / 2.0 - title_w as f32 / 2.0,
      150.0,
      Color::WHITE,
    );

    // Score display
    let score_text = format!("Score: {}", self.score);
    let (score_w, _) = ui.text_size(&score_text);
    ui.draw_text(
      &score_text,
      screen_w as f32 / 2.0 - score_w as f32 / 2.0,
      220.0,
      Color::WHITE,
    );

    // High score
    if self.is_new_high_score {
      let high_text = "NEW HIGH SCORE!";
      let (high_w, _) = ui.text_size(high_text);
      ui.draw_text(
        high_text,
        screen_w as f32 / 2.0 - high_w as f32 / 2.0,
        270.0,
        Color::rgb(255, 215, 0), // Gold
      );
    } else {
      let high_text = format!("High Score: {}", self.high_score);
      let (high_w, _) = ui.text_size(&high_text);
      ui.draw_text(
        &high_text,
        screen_w as f32 / 2.0 - high_w as f32 / 2.0,
        270.0,
        Color::rgb(200, 200, 200),
      );
    }

    // Restart button
    let button_w = 200.0;
    let button_h = 50.0;
    let button_x = screen_w as f32 / 2.0 - button_w / 2.0;

    if ui.button("RESTART", Rect::new(button_x, 350.0, button_w, button_h)) {
      self.restart_clicked = true;
    }

    // Menu button
    if ui.button("MENU", Rect::new(button_x, 420.0, button_w, button_h)) {
      self.menu_clicked = true;
    }

    // Keyboard hints
    let hint = "SPACE to restart  |  ESC for menu";
    let (hint_w, _) = ui.text_size(hint);
    ui.draw_text(
      hint,
      screen_w as f32 / 2.0 - hint_w as f32 / 2.0,
      500.0,
      Color::rgb(150, 150, 150),
    );
  }
}
