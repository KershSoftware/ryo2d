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

  /// The title screen's nodes, each text line in its own root
  fn spawn_nodes(&self, ctx: &mut SceneContext) {
    ctx.spawn_ui(UiNode::stack().fill().background(Color::rgb(20, 20, 35)));

    let line = |text: String, color: Color, y: f32| {
      UiNode::column()
        .anchor(Anchor::TopCenter)
        .offset(0.0, y)
        .child(UiNode::text(text).style(text_style(color)))
    };
    ctx.spawn_ui(line("BREAKOUT".into(), Color::WHITE, 100.0));
    ctx.spawn_ui(line("A Ryo2D Engine Demo".into(), Color::GRAY, 130.0));
    ctx.spawn_ui(line(
      format!("High Score: {}", self.high_score),
      Color::YELLOW,
      180.0,
    ));

    let blocks = [
      Color::hex(0xff4444),
      Color::hex(0xff8800),
      Color::hex(0xffff00),
      Color::hex(0x00cc44),
      Color::hex(0x4488ff),
      Color::hex(0xaa44ff),
    ];
    ctx.spawn_ui(
      UiNode::row()
        .anchor(Anchor::TopCenter)
        .offset(0.0, 250.0)
        .gap(5.0)
        .children(
          blocks
            .iter()
            .map(|color| UiNode::stack().width(60.0).height(25.0).background(*color)),
        ),
    );

    ctx.spawn_ui(
      UiNode::column()
        .focus_scope()
        .anchor(Anchor::TopCenter)
        .offset(0.0, 350.0)
        .child(
          UiNode::button("START GAME")
            .action("start")
            .width(200.0)
            .height(50.0)
            .default_focus(),
        ),
    );

    ctx.spawn_ui(line(
      "Press ENTER, SPACE, or A to Start".into(),
      Color::LIGHT_GRAY,
      450.0,
    ));
    ctx.spawn_ui(
      UiNode::column()
        .anchor(Anchor::BottomCenter)
        .offset(0.0, -30.0)
        .child(UiNode::text("ESC or B to Quit").style(text_style(Color::DARK_GRAY))),
    );
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

    self.spawn_nodes(ctx);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    // Start button: Confirm (Enter, Space, A) on it starts too
    let start = ctx
      .ui_events()
      .iter()
      .any(|e| e.kind == UiEventKind::Clicked && e.action.as_deref() == Some("start"));
    if start {
      return self.start_game(ctx);
    }

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
