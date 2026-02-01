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
  bird_flap_up: Sprite,
  bird_flap_down: Sprite,
  anim_timer: f32,
  anim_frame: bool,
}

impl MenuScene {
  pub fn new() -> Self {
    // Check if debug mode is enabled in Ryo.toml
    let debug_enabled = Workspace::discover()
      .ok()
      .and_then(|ws| ws.config().env_bool("debug_scene"))
      .unwrap_or(false);

    // Load bird sprites for animation
    let bird_flap_up =
      Sprite::load_svg_fit("assets/bird-flap-up.svg", 64, 64).expect("Failed to load bird sprite");
    let bird_flap_down = Sprite::load_svg_fit("assets/bird-flap-down.svg", 64, 64)
      .expect("Failed to load bird sprite");

    Self {
      high_score: load_high_score(),
      start_clicked: false,
      debug_clicked: false,
      debug_enabled,
      bird_flap_up,
      bird_flap_down,
      anim_timer: 0.0,
      anim_frame: false,
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

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Animate bird
    self.anim_timer += dt;
    if self.anim_timer >= 0.15 {
      self.anim_timer = 0.0;
      self.anim_frame = !self.anim_frame;
    }

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
    let title = "FLAPPY";
    let (title_w, _) = ui.text_size(title);
    ui.draw_text(
      title,
      screen_w as f32 / 2.0 - title_w as f32 / 2.0,
      100.0,
      Color::WHITE,
    );

    // High score (upper right)
    if self.high_score > 0 {
      let high_score_text = format!("Best: {}", self.high_score);
      let (text_w, _) = ui.text_size(&high_score_text);
      ui.draw_text(
        &high_score_text,
        screen_w as f32 - text_w as f32 - 16.0,
        16.0,
        Color::rgb(80, 80, 80),
      );
    }

    // Bird preview (centered below title, animated)
    let bird_sprite = if self.anim_frame {
      &self.bird_flap_up
    } else {
      &self.bird_flap_down
    };
    let bird_w = bird_sprite.width() as f32;
    let bird_x = screen_w as f32 / 2.0 - bird_w / 2.0;
    let bird_y = 180.0;
    ui.draw_sprite(bird_sprite, bird_x, bird_y);

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
