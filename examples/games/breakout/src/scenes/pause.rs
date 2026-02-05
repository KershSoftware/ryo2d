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
  /// Wait for escape to be released before accepting input
  escape_was_released: bool,
  /// Pending actions from button clicks (set in render_ui, consumed in update)
  resume_clicked: bool,
  quit_clicked: bool,
  quit_desktop_clicked: bool,
  /// System state for volume controls
  settings: SystemState,
  /// Track if we need to save settings
  settings_changed: bool,
  /// Previous volume values for detecting changes
  prev_master_volume: f32,
  prev_music_volume: f32,
  prev_sfx_volume: f32,
  /// Reference to the parent scene's music for real-time volume updates
  parent_music: Option<*mut MusicHandle>,
}

impl PauseScene {
  /// Create a new pause scene
  pub fn new(score: u32, high_score: u32) -> Self {
    Self::with_music(score, high_score, None)
  }

  /// Create a new pause scene with a reference to the parent's music
  pub fn with_music(score: u32, high_score: u32, parent_music: Option<&mut MusicHandle>) -> Self {
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

    let prev_master_volume = settings.master_volume;
    let prev_music_volume = settings.music_volume;
    let prev_sfx_volume = settings.sfx_volume;

    Self {
      score,
      high_score,
      escape_was_released: false,
      resume_clicked: false,
      quit_clicked: false,
      quit_desktop_clicked: false,
      settings,
      settings_changed: false,
      prev_master_volume,
      prev_music_volume,
      prev_sfx_volume,
      parent_music: parent_music.map(|m| m as *mut MusicHandle),
    }
  }
}

impl Scene for PauseScene {
  fn on_enter(&mut self, _ctx: &mut SceneContext) {
    // Pause overlay - no world changes needed
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Check if volumes changed since last frame and apply immediately
    let master_changed = (self.settings.master_volume - self.prev_master_volume).abs() > 0.001;
    let music_changed = (self.settings.music_volume - self.prev_music_volume).abs() > 0.001;
    let sfx_changed = (self.settings.sfx_volume - self.prev_sfx_volume).abs() > 0.001;

    if master_changed || music_changed || sfx_changed {
      if let Some(audio) = ctx.audio.as_mut() {
        audio.set_master_volume(self.settings.master_volume);
        audio.set_music_volume(self.settings.music_volume);
        audio.set_sfx_volume(self.settings.sfx_volume);

        // Update playing music in real-time if we have a reference to it
        if let Some(music_ptr) = self.parent_music {
          unsafe {
            if let Some(music) = music_ptr.as_mut() {
              music.update_category_volume(audio.music_volume());
            }
          }
        }
      }
      // Update previous values
      self.prev_master_volume = self.settings.master_volume;
      self.prev_music_volume = self.settings.music_volume;
      self.prev_sfx_volume = self.settings.sfx_volume;
      self.settings_changed = true;
    }

    // Wait for Escape/Start/B to be released before accepting new presses
    // This prevents the same press that opened the menu from closing it
    if !self.escape_was_released
      && !ctx.input.key_down(KeyCode::Escape)
      && !ctx.input.gamepad_button_down(GamepadButton::Start)
      && !ctx.input.gamepad_button_down(GamepadButton::East)
    {
      self.escape_was_released = true;
    }

    // Check for button clicks from previous render_ui
    if self.resume_clicked {
      self.resume_clicked = false;

      // Save settings if changed
      if self.settings_changed {
        if let Ok(workspace) = Workspace::discover()
          && let Ok(save_manager) = workspace.create_save_manager()
        {
          let _ = save_manager.save_system(&self.settings);
        }
        self.settings_changed = false; // Clear after saving
      }

      return SceneAction::Pop;
    }

    if self.quit_clicked {
      self.quit_clicked = false;
      save_high_score(self.high_score);
      return SceneAction::ResetWith(
        Box::new(MenuScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    if self.quit_desktop_clicked {
      self.quit_desktop_clicked = false;
      save_high_score(self.high_score);
      return SceneAction::Quit;
    }

    // Resume on Enter/A (always) or Escape/Start/B (only after released once)
    // Use consume to prevent input from cascading to underlying scene
    if ctx.input.key_pressed_and_consume(KeyCode::Enter)
      || ctx
        .input
        .gamepad_button_pressed_and_consume(GamepadButton::South)
    {
      return SceneAction::Pop;
    }
    if self.escape_was_released
      && (ctx.input.key_pressed_and_consume(KeyCode::Escape)
        || ctx
          .input
          .gamepad_button_pressed_and_consume(GamepadButton::Start)
        || ctx
          .input
          .gamepad_button_pressed_and_consume(GamepadButton::East))
    {
      return SceneAction::Pop;
    }

    // Return to menu via Q key or Select button
    if ctx.input.key_pressed_and_consume(KeyCode::KeyQ)
      || ctx
        .input
        .gamepad_button_pressed_and_consume(GamepadButton::Select)
    {
      save_high_score(self.high_score);
      return SceneAction::ResetWith(
        Box::new(MenuScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Dim overlay
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::new(0, 0, 0, 180),
    );

    // Pause panel (taller to fit volume sliders)
    let panel_w = 340.0;
    let panel_h = 450.0;
    let panel_x = (screen_w as f32 - panel_w) / 2.0;
    let panel_y = (screen_h as f32 - panel_h) / 2.0;

    ui.draw_panel(
      Rect::new(panel_x, panel_y, panel_w, panel_h),
      Color::rgb(30, 30, 50),
      Some(Color::WHITE),
    );

    // Title
    ui.draw_text_centered(
      "PAUSED",
      Rect::new(panel_x, panel_y + 20.0, panel_w, 30.0),
      Color::WHITE,
    );

    // Score
    let score_text = format!("Score: {}", self.score);
    ui.draw_text_centered(
      &score_text,
      Rect::new(panel_x, panel_y + 60.0, panel_w, 20.0),
      Color::YELLOW,
    );

    // Volume controls section
    let slider_y_start = panel_y + 100.0;
    let slider_spacing = 50.0;
    let slider_x = panel_x + 30.0;
    let slider_w = panel_w - 60.0;

    // Master Volume
    ui.draw_text(
      &format!("Master: {:.0}%", self.settings.master_volume * 100.0),
      slider_x,
      slider_y_start,
      Color::WHITE,
    );
    let master_changed = ui.slider(
      Rect::new(slider_x, slider_y_start + 20.0, slider_w, 8.0),
      &mut self.settings.master_volume,
      0.0,
      1.0,
    );
    if master_changed {
      // Round to 0.05 increments
      self.settings.master_volume = (self.settings.master_volume * 20.0).round() / 20.0;
    }

    // Music Volume
    ui.draw_text(
      &format!("Music: {:.0}%", self.settings.music_volume * 100.0),
      slider_x,
      slider_y_start + slider_spacing,
      Color::WHITE,
    );
    let music_changed = ui.slider(
      Rect::new(
        slider_x,
        slider_y_start + slider_spacing + 20.0,
        slider_w,
        8.0,
      ),
      &mut self.settings.music_volume,
      0.0,
      1.0,
    );
    if music_changed {
      // Round to 0.05 increments
      self.settings.music_volume = (self.settings.music_volume * 20.0).round() / 20.0;
    }

    // SFX Volume
    ui.draw_text(
      &format!("SFX: {:.0}%", self.settings.sfx_volume * 100.0),
      slider_x,
      slider_y_start + slider_spacing * 2.0,
      Color::WHITE,
    );
    let sfx_changed = ui.slider(
      Rect::new(
        slider_x,
        slider_y_start + slider_spacing * 2.0 + 20.0,
        slider_w,
        8.0,
      ),
      &mut self.settings.sfx_volume,
      0.0,
      1.0,
    );
    if sfx_changed {
      // Round to 0.05 increments
      self.settings.sfx_volume = (self.settings.sfx_volume * 20.0).round() / 20.0;
    }

    // Apply volume changes immediately for real-time feedback
    if master_changed || music_changed || sfx_changed {
      self.settings_changed = true;
    }

    // Buttons section
    let buttons_y_start = slider_y_start + slider_spacing * 3.0 + 20.0;

    // Resume button
    let resume_rect = Rect::new(panel_x + 70.0, buttons_y_start, 200.0, 40.0);
    if ui.button("RESUME", resume_rect) {
      self.resume_clicked = true;
    }

    // Quit to menu button
    let quit_rect = Rect::new(panel_x + 70.0, buttons_y_start + 50.0, 200.0, 40.0);
    if ui.button("QUIT TO MENU", quit_rect) {
      self.quit_clicked = true;
    }

    // Quit to desktop button
    let quit_desktop_rect = Rect::new(panel_x + 70.0, buttons_y_start + 100.0, 200.0, 40.0);
    if ui.button("QUIT TO DESKTOP", quit_desktop_rect) {
      self.quit_desktop_clicked = true;
    }
  }

  fn is_transparent(&self) -> bool {
    true // Show game behind pause overlay
  }

  fn name(&self) -> &str {
    "PauseScene"
  }
}
