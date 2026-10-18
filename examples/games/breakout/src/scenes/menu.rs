//! Menu Scene
//!
//! The main menu displayed when the game starts.

use std::time::Duration;

use ryo2d::prelude::*;

use super::{GameScene, QuitConfirmScene};
use crate::constants::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::look::{spawn_ui_file, use_game_look};
use crate::save::load_high_score;

const MENU: &str = "assets/ui/menu.ui.ron";
const MENU_EMBEDDED: &str = include_str!("../../assets/ui/menu.ui.ron");

/// Main menu scene
pub struct MenuScene {
  high_score: u32,
  /// Background music
  music: Option<MusicHandle>,
  /// The game is starting: the menu still shows while it fades out, and a
  /// second press then must not play the intro again
  starting: bool,
  /// The title screen's tree, when its file loaded
  root: Option<Entity>,
}

impl MenuScene {
  /// Create a new menu scene
  pub fn new() -> Self {
    Self {
      high_score: load_high_score(),
      music: None,
      starting: false,
      root: None,
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

  /// The title screen, from its file
  fn spawn_nodes(&mut self, ctx: &mut SceneContext) {
    self.root = spawn_ui_file(ctx, MENU, MENU_EMBEDDED);
    if let Some(root) = self.root {
      ctx.ui_set(root, "high", self.high_score);
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
