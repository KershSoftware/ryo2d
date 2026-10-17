//! Menu Scene
//!
//! The main menu displayed when the game starts.

use std::time::Duration;

use ryo2d::prelude::*;

use super::{GameScene, QuitConfirmScene};
use crate::constants::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::look::{text_style, use_game_look};
use crate::save::load_high_score;

/// Main menu scene
pub struct MenuScene {
  high_score: u32,
  /// Background music
  music: Option<MusicHandle>,
  /// The game is starting: the menu still shows while it fades out, and a
  /// second press then must not play the intro again
  starting: bool,
}

impl MenuScene {
  /// Create a new menu scene
  pub fn new() -> Self {
    Self {
      high_score: load_high_score(),
      music: None,
      starting: false,
    }
  }

  /// Play the intro and switch to the game, handing it the music to
  /// crossfade
  fn start_game(&mut self, ctx: &mut SceneContext) -> SceneAction {
    if self.starting {
      return SceneAction::Continue;
    }
    self.starting = true;
    let _ = ctx
      .audio
      .play_sound_with("intro_sfx", SoundOptions::new().volume(0.5));
    let music = self.music.take();
    SceneAction::SwitchWith(
      Box::new(GameScene::new(self.high_score, music)),
      Transition::fade(Duration::from_millis(300)),
    )
  }
}

impl Default for MenuScene {
  fn default() -> Self {
    Self::new()
  }
}

impl Scene for MenuScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    use_game_look(ctx);

    ctx
      .camera()
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

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let (screen_w, screen_h) = ui.screen_size();

    // Background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::rgb(20, 20, 35),
    );

    // Title
    ui.draw_text_anchored_styled(
      "BREAKOUT",
      Anchor::TopCenter,
      0.0,
      100.0,
      &text_style(Color::WHITE),
    );

    // Subtitle
    ui.draw_text_anchored_styled(
      "A Ryo2D Engine Demo",
      Anchor::TopCenter,
      0.0,
      130.0,
      &text_style(Color::GRAY),
    );

    // High score
    let high_score_text = format!("High Score: {}", self.high_score);
    ui.draw_text_anchored_styled(
      &high_score_text,
      Anchor::TopCenter,
      0.0,
      180.0,
      &text_style(Color::YELLOW),
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

    // Start button: Confirm (Enter, Space, A) on it starts too
    let mut start = false;
    ui.column()
      .focus_scope()
      .anchor(Anchor::TopCenter)
      .offset(0.0, 350.0)
      .show(|ui| {
        start = ui
          .button("START GAME")
          .width(200.0)
          .height(50.0)
          .default_focus()
          .clicked();
      });
    if start {
      return self.start_game(ctx);
    }

    // Instructions
    ui.draw_text_anchored_styled(
      "Press ENTER, SPACE, or A to Start",
      Anchor::TopCenter,
      0.0,
      450.0,
      &text_style(Color::LIGHT_GRAY),
    );

    ui.draw_text_anchored_styled(
      "ESC or B to Quit",
      Anchor::BottomCenter,
      0.0,
      -30.0,
      &text_style(Color::DARK_GRAY),
    );

    // Back (Escape, B) asks before quitting
    if ui.back() {
      return SceneAction::Push(Box::new(QuitConfirmScene::new()));
    }
    SceneAction::Continue
  }

  fn name(&self) -> &str {
    "MenuScene"
  }
}
