//! Menu Scene
//!
//! The main menu displayed when the game starts.

use std::time::Duration;

use ryo2d::prelude::*;

use super::{GameScene, QuitConfirmScene};
use crate::constants::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::look::{spawn_ui_file, use_game_look};
use crate::save::load_high_score;

const MENU: &str = "ui/menu.ui.ron";
const MENU_EMBEDDED: &str = include_str!("../../assets/ui/menu.ui.ron");
const MUSIC: &str = "audio/bgm-main-menu.music.ron";
const INTRO: &str = "audio/intro-sfx.sound.ron";

/// Main menu scene
pub struct MenuScene {
  high_score: u32,
  /// The intro played as the game starts
  intro: Option<Handle<Sound>>,
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
      high_score: 0,
      intro: None,
      starting: false,
      root: None,
    }
  }

  /// Play the intro and switch to the game, whose music takes over
  fn start_game(&mut self, ctx: &mut SceneContext) -> SceneAction {
    if self.starting {
      return SceneAction::Continue;
    }
    self.starting = true;
    if let Some(intro) = &self.intro {
      ctx.audio().play(intro);
    }
    SceneAction::SwitchWith(
      Box::new(GameScene::new(self.high_score)),
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
    self.high_score = load_high_score(ctx);

    ctx
      .camera()
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

    // The menu's music, taking over from the game's
    let music = ctx.assets().load::<Music>(MUSIC);
    ctx.audio().play_music(&music);
    self.intro = Some(ctx.assets().load(INTRO));

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
