//! Pause Scene
//!
//! Overlay shown when the game is paused.

use std::time::Duration;

use ryo2d::prelude::*;

use super::MenuScene;
use crate::look::spawn_ui_file;
use crate::save::save_high_score;

const PAUSE: &str = "ui/pause.ui.ron";
const PAUSE_EMBEDDED: &str = include_str!("../../assets/ui/pause.ui.ron");

/// Pause menu overlay
pub struct PauseScene {
  score: u32,
  high_score: u32,
  /// The menu's tree, when its file loaded
  root: Option<Entity>,
}

impl PauseScene {
  /// Create a new pause scene
  pub fn new(score: u32, high_score: u32) -> Self {
    Self {
      score,
      high_score,
      root: None,
    }
  }

  /// Save the high score and go back to the main menu
  fn quit_to_menu(&self, ctx: &mut SceneContext) -> SceneAction {
    save_high_score(ctx, self.high_score);
    SceneAction::ResetWith(
      Box::new(MenuScene::new()),
      Transition::fade(Duration::from_millis(300)),
    )
  }
}

impl Scene for PauseScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.root = spawn_ui_file(ctx, PAUSE, PAUSE_EMBEDDED);
    let Some(root) = self.root else {
      return;
    };
    // The sliders run 0-100; the settings take the volumes as they change
    let (master, music, sfx) = {
      let settings = ctx.settings();
      (
        settings.volume(VolumeKind::Master),
        settings.volume(VolumeKind::Music),
        settings.volume(VolumeKind::Sfx),
      )
    };
    ctx.ui_set(root, "master", master * 100.0);
    ctx.ui_set(root, "music", music * 100.0);
    ctx.ui_set(root, "sfx", sfx * 100.0);
    ctx.ui_set(root, "score", self.score);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Start, which paused the game on a pad, resumes it
    if ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::Start)
    {
      return SceneAction::Pop;
    }
    // Q is a keyboard shortcut back to the menu
    if ctx.input.key_pressed_and_consume(KeyCode::KeyQ) {
      return self.quit_to_menu(ctx);
    }
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    // A saved menu file that gave the root a new kind replaced its entity
    for reloaded in ctx.messages::<UiReloaded>() {
      if self.root.is_some() && self.root == reloaded.replaced {
        self.root = Some(reloaded.root);
      }
    }
    let mut action = None;
    for event in ctx.ui_events() {
      match (event.kind, event.name.as_deref(), event.action.as_deref()) {
        (UiEventKind::Changed, Some(name @ ("master" | "music" | "sfx")), _) => {
          let Some(root) = self.root else {
            continue;
          };
          let Some(value) = ctx.ui_get::<f32>(root, name).map(|v| v / 100.0) else {
            continue;
          };
          let kind = match name {
            "master" => VolumeKind::Master,
            "music" => VolumeKind::Music,
            _ => VolumeKind::Sfx,
          };
          ctx.settings().set_volume(kind, value);
        }
        (UiEventKind::Clicked, _, Some("resume")) => action = Some(Choice::Resume),
        (UiEventKind::Clicked, _, Some("menu")) => action = Some(Choice::Menu),
        (UiEventKind::Clicked, _, Some("desktop")) => action = Some(Choice::Desktop),
        _ => {}
      }
    }

    // Back (Escape, B) resumes
    if action.is_none() && ui.back() {
      action = Some(Choice::Resume);
    }
    match action {
      Some(Choice::Resume) => SceneAction::Pop,
      Some(Choice::Menu) => self.quit_to_menu(ctx),
      Some(Choice::Desktop) => {
        save_high_score(ctx, self.high_score);
        SceneAction::Quit
      }
      None => SceneAction::Continue,
    }
  }

  fn is_transparent(&self) -> bool {
    true // Show game behind pause overlay
  }

  fn name(&self) -> &str {
    "PauseScene"
  }
}

/// What the pause menu was asked to do
enum Choice {
  Resume,
  Menu,
  Desktop,
}
