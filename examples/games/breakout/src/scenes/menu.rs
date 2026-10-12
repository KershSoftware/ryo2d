//! Menu Scene
//!
//! The main menu displayed when the game starts.

use std::time::Duration;

use ryo2d::prelude::*;

use super::{GameScene, QuitConfirmScene};
use crate::constants::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::save::load_high_score;

/// Main menu scene
pub struct MenuScene {
  high_score: u32,
  /// Pending action from button click (set in ui, consumed in update)
  start_clicked: bool,
  /// Wait for escape to be released before accepting input
  escape_was_released: bool,
  /// Background music
  music: Option<MusicHandle>,
}

impl MenuScene {
  /// Create a new menu scene
  pub fn new() -> Self {
    Self {
      high_score: load_high_score(),
      start_clicked: false,
      escape_was_released: false,
      music: None,
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
    ctx
      .camera
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

    // Load and start menu background music
    let audio = &mut *ctx.audio;
    let _ = audio.load_sound("intro_sfx", "assets/audio/intro-sfx.wav");

    // If we have music from previous scene (game), crossfade to menu music
    if let Some(mut prev_music) = self.music.take() {
      if let Ok(music) = audio.crossfade_music(
        &mut prev_music,
        "assets/audio/bgm-main-menu.mp3",
        Duration::from_millis(1000),
      ) {
        self.music = Some(music);
      }
    } else {
      // No previous music, just start menu music with auto-fade on scene exit
      if let Ok(music) = audio.play_music_with(
        "assets/audio/bgm-main-menu.mp3",
        MusicOptions::new().fade_out_on_drop(Duration::from_millis(500)),
      ) {
        self.music = Some(music);
      }
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Wait for Escape/B to be released before accepting new presses
    if !self.escape_was_released
      && !ctx.input.key_down(KeyCode::Escape)
      && !ctx.input.gamepad_button_down(GamepadButton::East)
      && !ctx.input.gamepad_button_down(GamepadButton::Select)
    {
      self.escape_was_released = true;
    }

    // Start game on Enter, Space, button click, or gamepad A/Start
    // Use consume to prevent input from cascading to the next scene
    if ctx.input.key_pressed_and_consume(KeyCode::Enter)
      || ctx.input.key_pressed_and_consume(KeyCode::Space)
      || ctx.input.gamepad_button_pressed_and_consume(GamepadButton::South) // A button
      || ctx.input.gamepad_button_pressed_and_consume(GamepadButton::Start)
      || self.start_clicked
    {
      self.start_clicked = false;

      // Play intro sound effect
      let _ = ctx
        .audio
        .play_sound_with("intro_sfx", SoundOptions::new().volume(0.5));

      // Take music handle to pass to game for crossfading
      let music = self.music.take();
      return SceneAction::SwitchWith(
        Box::new(GameScene::new(self.high_score, music)),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    // ESC or gamepad B/Select to open quit confirmation (only after released once)
    // Use consume to prevent input from cascading to the overlay
    if self.escape_was_released
      && (ctx.input.key_pressed_and_consume(KeyCode::Escape)
        || ctx.input.gamepad_button_pressed_and_consume(GamepadButton::East) // B button
        || ctx.input.gamepad_button_pressed_and_consume(GamepadButton::Select))
    {
      self.escape_was_released = false; // Reset for when we return from overlay
      return SceneAction::Push(Box::new(QuitConfirmScene::new()));
    }

    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
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
      "Press ENTER, SPACE, or A to Start",
      Anchor::TopCenter,
      0.0,
      450.0,
      Color::LIGHT_GRAY,
    );

    ui.draw_text_anchored(
      "ESC or B to Quit",
      Anchor::BottomCenter,
      0.0,
      -30.0,
      Color::DARK_GRAY,
    );
    SceneAction::Continue
  }

  fn name(&self) -> &str {
    "MenuScene"
  }
}
