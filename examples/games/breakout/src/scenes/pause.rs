//! Pause Scene
//!
//! Overlay shown when the game is paused.

use std::time::Duration;

use ryo2d::prelude::*;

use super::MenuScene;
use crate::look::spawn_ui_file;
use crate::save::save_high_score;

const PAUSE: &str = "assets/ui/pause.ui.ron";
const PAUSE_EMBEDDED: &str = include_str!("../../assets/ui/pause.ui.ron");

/// Pause menu overlay
pub struct PauseScene {
  score: u32,
  high_score: u32,
  /// System state for volume controls
  settings: SystemState,
  /// Track if we need to save settings
  settings_changed: bool,
  /// The menu's tree, when its file loaded
  root: Option<Entity>,
}

impl PauseScene {
  /// Create a new pause scene
  pub fn new(score: u32, high_score: u32) -> Self {
    // Try to load existing settings, or use defaults
    let settings = if let Ok(workspace) = Workspace::discover() {
      if let Ok(save_manager) = workspace.create_save_manager() {
        save_manager.load_system().unwrap_or_default()
      } else {
        SystemState::new()
      }
    } else {
      SystemState::new()
    };

    Self {
      score,
      high_score,
      settings,
      settings_changed: false,
      root: None,
    }
  }

  /// Save the volumes if they changed, and go back to the game
  fn resume(&mut self) -> SceneAction {
    if self.settings_changed {
      if let Ok(workspace) = Workspace::discover()
        && let Ok(save_manager) = workspace.create_save_manager()
      {
        let _ = save_manager.save_system(&self.settings);
      }
      self.settings_changed = false;
    }
    SceneAction::Pop
  }

  /// Save the high score and go back to the main menu
  fn quit_to_menu(&self) -> SceneAction {
    save_high_score(self.high_score);
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
    // The sliders run 0-100; the volumes are applied as they change
    let s = &self.settings;
    ctx.ui_set(root, "master", s.master_volume * 100.0);
    ctx.ui_set(root, "music", s.music_volume * 100.0);
    ctx.ui_set(root, "sfx", s.sfx_volume * 100.0);
    ctx.ui_set(root, "score", self.score);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Start, which paused the game on a pad, resumes it
    if ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::Start)
    {
      return self.resume();
    }
    // Q is a keyboard shortcut back to the menu
    if ctx.input.key_pressed_and_consume(KeyCode::KeyQ) {
      return self.quit_to_menu();
    }
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
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
          match name {
            "master" => {
              self.settings.master_volume = value;
              ctx.audio.set_master_volume(value);
            }
            "music" => {
              self.settings.music_volume = value;
              ctx.audio.set_music_volume(value);
            }
            _ => {
              self.settings.sfx_volume = value;
              ctx.audio.set_sfx_volume(value);
            }
          }
          self.settings_changed = true;
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
      Some(Choice::Resume) => self.resume(),
      Some(Choice::Menu) => self.quit_to_menu(),
      Some(Choice::Desktop) => {
        save_high_score(self.high_score);
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
