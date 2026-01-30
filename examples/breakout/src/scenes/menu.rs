//! Menu Scene
//!
//! The main menu displayed when the game starts.

use std::time::Duration;

use ryo2d::prelude::*;

use super::super::constants::{SCREEN_HEIGHT, SCREEN_WIDTH};
use super::super::save::load_high_score;
use super::{GameScene, QuitConfirmScene};

/// Main menu scene
pub struct MenuScene {
  high_score: u32,
  /// Pending action from button click (set in render_ui, consumed in update)
  start_clicked: bool,
  /// Wait for escape to be released before accepting input
  escape_was_released: bool,
}

impl MenuScene {
  /// Create a new menu scene
  pub fn new() -> Self {
    Self {
      high_score: load_high_score(),
      start_clicked: false,
      escape_was_released: false,
    }
  }
}

impl Default for MenuScene {
  fn default() -> Self {
    Self::new()
  }
}

impl Scene for MenuScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.clear();
    ctx
      .camera
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Wait for Escape to be released before accepting new Escape presses
    if !self.escape_was_released && !ctx.input.key_down(KeyCode::Escape) {
      self.escape_was_released = true;
    }

    // Start game on Enter, Space, or button click
    if ctx.input.key_pressed(KeyCode::Enter)
      || ctx.input.key_pressed(KeyCode::Space)
      || self.start_clicked
    {
      self.start_clicked = false;
      return SceneAction::SwitchWith(
        Box::new(GameScene::new(self.high_score)),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    // ESC to open quit confirmation (only after released once)
    if self.escape_was_released && ctx.input.key_pressed(KeyCode::Escape) {
      self.escape_was_released = false; // Reset for when we return from overlay
      return SceneAction::Push(Box::new(QuitConfirmScene::new()));
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::rgb(20, 20, 35),
    );

    // Title
    ui.draw_text_anchored("BREAKOUT", Anchor::TopCenter, 0.0, 100.0, Color::WHITE);

    // Subtitle
    ui.draw_text_anchored(
      "A Ryo2D Engine Demo",
      Anchor::TopCenter,
      0.0,
      130.0,
      Color::GRAY,
    );

    // High score
    let high_score_text = format!("High Score: {}", self.high_score);
    ui.draw_text_anchored(
      &high_score_text,
      Anchor::TopCenter,
      0.0,
      180.0,
      Color::YELLOW,
    );

    // Decorative blocks
    let colors = [
      Color::hex(0xff4444),
      Color::hex(0xff8800),
      Color::hex(0xffff00),
      Color::hex(0x00cc44),
      Color::hex(0x4488ff),
      Color::hex(0xaa44ff),
    ];

    for (i, color) in colors.iter().enumerate() {
      let x = screen_w as f32 / 2.0 - 180.0 + i as f32 * 65.0;
      ui.draw_rect(Rect::new(x, 250.0, 60.0, 25.0), *color);
    }

    // Start button area
    let button_rect = Rect::new(screen_w as f32 / 2.0 - 100.0, 350.0, 200.0, 50.0);

    if ui.button("START GAME", button_rect) {
      self.start_clicked = true;
    }

    // Instructions
    ui.draw_text_anchored(
      "Press ENTER or SPACE to Start",
      Anchor::TopCenter,
      0.0,
      450.0,
      Color::LIGHT_GRAY,
    );

    ui.draw_text_anchored(
      "ESC to Quit",
      Anchor::BottomCenter,
      0.0,
      -30.0,
      Color::DARK_GRAY,
    );
  }

  fn name(&self) -> &str {
    "MenuScene"
  }
}
