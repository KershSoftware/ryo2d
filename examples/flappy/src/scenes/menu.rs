//! Menu Scene - Starting screen for the game

use ryo2d::prelude::*;
use std::time::Duration;

use super::{DebugScene, GameScene};
use crate::constants::*;
use crate::save::load_high_score;

pub struct MenuScene {
  high_score: u32,
  start_clicked: bool,
  debug_clicked: bool,
  debug_enabled: bool,
}

impl MenuScene {
  pub fn new() -> Self {
    // Check if debug mode is enabled in Ryo.toml
    let debug_enabled = Workspace::discover()
      .ok()
      .and_then(|ws| ws.config().env_bool("debug_scene"))
      .unwrap_or(false);

    Self {
      high_score: load_high_score(),
      start_clicked: false,
      debug_clicked: false,
      debug_enabled,
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
    // Start game on Space, Enter, or button click
    if ctx.input.key_pressed(KeyCode::Space)
      || ctx.input.key_pressed(KeyCode::Enter)
      || self.start_clicked
    {
      self.start_clicked = false;
      return SceneAction::SwitchWith(
        Box::new(GameScene::new(self.high_score)),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    // Debug mode on D key or debug button click (only if enabled)
    if self.debug_enabled && (ctx.input.key_pressed(KeyCode::KeyD) || self.debug_clicked) {
      self.debug_clicked = false;
      return SceneAction::SwitchWith(
        Box::new(DebugScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::rgb(135, 206, 235), // Sky blue
    );

    // Title
    ui.draw_text(
      "FLAPPY BIRD",
      screen_w as f32 / 2.0 - 100.0,
      100.0,
      Color::WHITE,
    );

    // High score
    if self.high_score > 0 {
      ui.draw_text(
        &format!("High Score: {}", self.high_score),
        screen_w as f32 / 2.0 - 80.0,
        200.0,
        Color::rgb(100, 100, 100),
      );
    }

    // Start button
    let button_w = 200.0;
    let button_h = 60.0;
    let button_x = screen_w as f32 / 2.0 - button_w / 2.0;
    let button_y = screen_h as f32 / 2.0;

    if ui.button("START", Rect::new(button_x, button_y, button_w, button_h)) {
      self.start_clicked = true;
    }

    // Debug button (smaller, below start) - only if debug mode enabled
    if self.debug_enabled {
      let debug_button_w = 120.0;
      let debug_button_h = 40.0;
      let debug_button_x = screen_w as f32 / 2.0 - debug_button_w / 2.0;
      let debug_button_y = button_y + button_h + 20.0;

      if ui.button(
        "DEBUG",
        Rect::new(
          debug_button_x,
          debug_button_y,
          debug_button_w,
          debug_button_h,
        ),
      ) {
        self.debug_clicked = true;
      }
    }

    // Instructions
    let instructions = if self.debug_enabled {
      "Press SPACE or CLICK to flap | D for debug"
    } else {
      "Press SPACE or CLICK to flap"
    };
    let (text_w, _) = ui.text_size(instructions);
    ui.draw_text(
      instructions,
      screen_w as f32 / 2.0 - text_w as f32 / 2.0,
      screen_h as f32 - 100.0,
      Color::rgb(100, 100, 100),
    );
  }
}
