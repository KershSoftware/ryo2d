//! Pause Scene
//!
//! Overlay shown when the game is paused.

use std::time::Duration;

use ryo2d::prelude::*;

use crate::save::save_high_score;
use super::MenuScene;

/// Pause menu overlay
pub struct PauseScene {
  score: u32,
  high_score: u32,
  /// Wait for escape to be released before accepting input
  escape_was_released: bool,
  /// Pending actions from button clicks (set in render_ui, consumed in update)
  resume_clicked: bool,
  quit_clicked: bool,
  quit_desktop_clicked: bool,
}

impl PauseScene {
  /// Create a new pause scene
  pub fn new(score: u32, high_score: u32) -> Self {
    Self {
      score,
      high_score,
      escape_was_released: false,
      resume_clicked: false,
      quit_clicked: false,
      quit_desktop_clicked: false,
    }
  }
}

impl Scene for PauseScene {
  fn on_enter(&mut self, _ctx: &mut SceneContext) {
    // Pause overlay - no world changes needed
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Wait for Escape to be released before accepting new Escape presses
    // This prevents the same keypress that opened the menu from closing it
    if !self.escape_was_released && !ctx.input.key_down(KeyCode::Escape) {
      self.escape_was_released = true;
    }

    // Check for button clicks from previous render_ui
    if self.resume_clicked {
      self.resume_clicked = false;
      return SceneAction::Pop;
    }

    if self.quit_clicked {
      self.quit_clicked = false;
      save_high_score(self.high_score);
      return SceneAction::ResetWith(
        Box::new(MenuScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    if self.quit_desktop_clicked {
      self.quit_desktop_clicked = false;
      save_high_score(self.high_score);
      return SceneAction::Quit;
    }

    // Resume on Enter (always) or Escape (only after released once)
    if ctx.input.key_pressed(KeyCode::Enter) {
      return SceneAction::Pop;
    }
    if self.escape_was_released && ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Pop;
    }

    // Return to menu via Q key
    if ctx.input.key_pressed(KeyCode::KeyQ) {
      save_high_score(self.high_score);
      return SceneAction::ResetWith(
        Box::new(MenuScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Dim overlay
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::new(0, 0, 0, 180),
    );

    // Pause panel
    let panel_w = 300.0;
    let panel_h = 310.0;
    let panel_x = (screen_w as f32 - panel_w) / 2.0;
    let panel_y = (screen_h as f32 - panel_h) / 2.0;

    ui.draw_panel(
      Rect::new(panel_x, panel_y, panel_w, panel_h),
      Color::rgb(30, 30, 50),
      Some(Color::WHITE),
    );

    // Title
    ui.draw_text_centered(
      "PAUSED",
      Rect::new(panel_x, panel_y + 20.0, panel_w, 30.0),
      Color::WHITE,
    );

    // Score
    let score_text = format!("Score: {}", self.score);
    ui.draw_text_centered(
      &score_text,
      Rect::new(panel_x, panel_y + 60.0, panel_w, 20.0),
      Color::YELLOW,
    );

    // Resume button
    let resume_rect = Rect::new(panel_x + 50.0, panel_y + 110.0, 200.0, 40.0);
    if ui.button("RESUME", resume_rect) {
      self.resume_clicked = true;
    }

    // Quit to menu button
    let quit_rect = Rect::new(panel_x + 50.0, panel_y + 160.0, 200.0, 40.0);
    if ui.button("QUIT TO MENU", quit_rect) {
      self.quit_clicked = true;
    }

    // Quit to desktop button
    let quit_desktop_rect = Rect::new(panel_x + 50.0, panel_y + 210.0, 200.0, 40.0);
    if ui.button("QUIT TO DESKTOP", quit_desktop_rect) {
      self.quit_desktop_clicked = true;
    }
  }

  fn is_transparent(&self) -> bool {
    true // Show game behind pause overlay
  }

  fn name(&self) -> &str {
    "PauseScene"
  }
}
