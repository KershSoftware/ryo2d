//! Pause Scene
//!
//! Overlay shown when the game is paused.

use std::time::Duration;

use ryo2d::prelude::*;

use super::MenuScene;
use crate::look::TEXT_SIZE;
use crate::save::save_high_score;

/// Pause menu overlay
pub struct PauseScene {
  score: u32,
  high_score: u32,
  /// System state for volume controls
  settings: SystemState,
  /// Track if we need to save settings
  settings_changed: bool,
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

/// A row of a label, a volume slider and its percentage
fn volume_row(label: &str, name: &str, value: f32) -> UiNode {
  UiNode::row()
    .gap(8.0)
    .align(Align::Center)
    // Wide enough for "Master" and "100%" in the game's 12 unit font
    .child(
      UiNode::column()
        .width(96.0)
        .child(UiNode::text(label).size(TEXT_SIZE).color(Color::WHITE)),
    )
    .child(
      UiNode::slider(0.0..=1.0, value)
        .step(0.05)
        .fill_width()
        .name(name),
    )
    .child(
      UiNode::column().width(64.0).child(
        UiNode::text(percent(value))
          .size(TEXT_SIZE)
          .color(Color::WHITE)
          .text_align(TextAlign::End)
          .name(format!("{name}-percent")),
      ),
    )
}

fn percent(value: f32) -> String {
  format!("{:.0}%", value * 100.0)
}

fn menu_button(label: &str, action: &str) -> UiNode {
  UiNode::button(label)
    .action(action)
    .fill_width()
    .height(40.0)
}

impl Scene for PauseScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(UiNode::stack().fill().background(Color::new(0, 0, 0, 180)));
    let s = &self.settings;
    ctx.spawn_ui(
      UiNode::panel()
        .focus_scope()
        .anchor(Anchor::Center)
        .width(400.0)
        .padding(16.0)
        .gap(12.0)
        .child(
          UiNode::text("PAUSED")
            .size(TEXT_SIZE)
            .color(Color::WHITE)
            .text_align(TextAlign::Center),
        )
        .child(
          UiNode::text(format!("Score: {}", self.score))
            .size(TEXT_SIZE)
            .color(Color::YELLOW)
            .text_align(TextAlign::Center),
        )
        // Volumes, applied as they change for real-time feedback
        .child(volume_row("Master", "master", s.master_volume))
        .child(volume_row("Music", "music", s.music_volume))
        .child(volume_row("SFX", "sfx", s.sfx_volume))
        .child(menu_button("RESUME", "resume").default_focus())
        .child(menu_button("QUIT TO MENU", "menu"))
        .child(menu_button("QUIT TO DESKTOP", "desktop")),
    );
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
          let Some(value) = ctx.world.get::<UiValue>(event.node).map(|v| v.0) else {
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
          if let Some(label) = ctx.find_ui(&format!("{name}-percent"))
            && let Some(mut label) = ctx.world.get_mut::<UiLabel>(label)
          {
            label.0 = percent(value);
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
