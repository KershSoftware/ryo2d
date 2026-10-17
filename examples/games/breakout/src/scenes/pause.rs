//! Pause Scene
//!
//! Overlay shown when the game is paused.

use std::time::Duration;

use ryo2d::prelude::*;

use super::MenuScene;
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

/// A row of a label, a volume slider and its percentage; whether the slider
/// changed the volume
fn volume_row(ui: &mut UiContext, label: &str, volume: &mut f32) -> bool {
  let mut changed = false;
  ui.row().gap(8.0).align(Align::Center).show(|ui| {
    ui.column().width(64.0).show(|ui| {
      ui.text(label).color(Color::WHITE);
    });
    changed = ui
      .slider(volume, 0.0..=1.0)
      .step(0.05)
      .fill_width()
      .show()
      .changed();
    ui.column().width(40.0).show(|ui| {
      ui.text(format!("{:.0}%", *volume * 100.0))
        .color(Color::WHITE)
        .align(TextAlign::End);
    });
  });
  changed
}

impl Scene for PauseScene {
  fn on_enter(&mut self, _ctx: &mut SceneContext) {
    // Pause overlay - no world changes needed
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
    let (screen_w, screen_h) = ui.screen_size();

    // Dim overlay
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::new(0, 0, 0, 180),
    );

    let mut action = None;
    let settings = &mut self.settings;
    let (mut master, mut music, mut sfx) = (false, false, false);
    ui.panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(340.0)
      .padding(16.0)
      .gap(12.0)
      .show(|ui| {
        ui.text("PAUSED")
          .color(Color::WHITE)
          .align(TextAlign::Center);
        ui.text(format!("Score: {}", self.score))
          .color(Color::YELLOW)
          .align(TextAlign::Center);

        // Volumes, applied as they change for real-time feedback
        master = volume_row(ui, "Master", &mut settings.master_volume);
        music = volume_row(ui, "Music", &mut settings.music_volume);
        sfx = volume_row(ui, "SFX", &mut settings.sfx_volume);

        if ui
          .button("RESUME")
          .fill_width()
          .height(40.0)
          .default_focus()
          .clicked()
        {
          action = Some(Choice::Resume);
        }
        if ui
          .button("QUIT TO MENU")
          .fill_width()
          .height(40.0)
          .clicked()
        {
          action = Some(Choice::Menu);
        }
        if ui
          .button("QUIT TO DESKTOP")
          .fill_width()
          .height(40.0)
          .clicked()
        {
          action = Some(Choice::Desktop);
        }
      });

    if master {
      ctx.audio.set_master_volume(self.settings.master_volume);
    }
    if music {
      ctx.audio.set_music_volume(self.settings.music_volume);
    }
    if sfx {
      ctx.audio.set_sfx_volume(self.settings.sfx_volume);
    }
    if master || music || sfx {
      self.settings_changed = true;
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
