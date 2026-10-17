//! Pause menu scene (overlay)
//!
//! Pushed on top of GameScene when the player presses Escape or Start.
//! Handles resume, settings, quit-to-menu, and quit-to-desktop.

use ryo2d::prelude::*;

use super::MenuScene;

#[derive(Default)]
pub struct PauseScene {
  settings_open: bool,
  selected_display_mode_idx: usize,
}

impl PauseScene {
  pub fn new() -> Self {
    Self {
      settings_open: false,
      selected_display_mode_idx: 0,
    }
  }
}

impl Scene for PauseScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let modes = DisplayMode::all();
    self.selected_display_mode_idx = modes
      .iter()
      .position(|m| *m == ctx.display_mode())
      .unwrap_or(0);
    ctx.ui_theme().modify(|theme| {
      theme.variants.insert(
        "pause-item".to_string(),
        pause_variant(
          Color::rgb(50, 50, 60),
          Color::rgb(60, 60, 70),
          Color::rgb(40, 40, 50),
        ),
      );
      theme.variants.insert(
        "pause-selected".to_string(),
        pause_variant(
          Color::rgb(60, 100, 60),
          Color::rgb(70, 110, 70),
          Color::rgb(50, 90, 50),
        ),
      );
    });
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Start, the pause key on a pad, closes settings or resumes
    if ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::Start)
    {
      if self.settings_open {
        self.settings_open = false;
      } else {
        return SceneAction::Pop;
      }
    }
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let (screen_w, screen_h) = ui.screen_size();
    let (screen_w, screen_h) = (screen_w as f32, screen_h as f32);

    // Darken background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w, screen_h),
      Color::new(0, 0, 0, 180),
    );

    if let Some(action) = self.main_menu(ui) {
      return action;
    }

    // Instructions: the menu's, or the settings dialog's inside it
    if self.settings_open {
      self.settings(ui, ctx);
    } else {
      ui.draw_text_anchored(
        "D-Pad/Arrows: Navigate | A/Enter: Select | B/ESC: Resume",
        Anchor::TopCenter,
        0.0,
        screen_h / 2.0 + 170.0,
        HINT,
      );
    }

    // Back (Escape, B) closes settings, else resumes
    if ui.back() {
      if self.settings_open {
        self.settings_open = false;
      } else {
        return SceneAction::Pop;
      }
    }
    SceneAction::Continue
  }

  fn is_transparent(&self) -> bool {
    true
  }

  fn is_physics_paused(&self) -> bool {
    true
  }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

impl PauseScene {
  /// The pause menu, centred; the scene action a button asked for
  fn main_menu(&mut self, ui: &mut UiContext) -> Option<SceneAction> {
    let mut action = None;
    let settings_open = &mut self.settings_open;
    ui.panel()
      .focus_scope()
      .wrap_focus()
      .anchor(Anchor::Center)
      .padding(16.0)
      .gap(10.0)
      .show(|ui| {
        ui.text("PAUSED")
          .color(Color::WHITE)
          .align(TextAlign::Center);
        if ui
          .button("Resume")
          .width(200.0)
          .height(40.0)
          .variant("pause-item")
          .default_focus()
          .clicked()
        {
          action = Some(SceneAction::Pop);
        }
        if ui
          .button("Settings")
          .width(200.0)
          .height(40.0)
          .variant("pause-item")
          .clicked()
        {
          *settings_open = true;
        }
        if ui
          .button("Quit to Menu")
          .width(200.0)
          .height(40.0)
          .variant("pause-item")
          .clicked()
        {
          action = Some(SceneAction::Reset(Box::new(MenuScene::new())));
        }
        if ui
          .button("Quit to Desktop")
          .width(200.0)
          .height(40.0)
          .variant("danger")
          .clicked()
        {
          action = Some(SceneAction::Quit);
        }
      });
    action
  }

  /// The settings dialog over the pause menu
  fn settings(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let modes = DisplayMode::all();
    let mut apply = false;
    let mut back = false;
    let selected = &mut self.selected_display_mode_idx;
    ui.modal().padding(20.0).gap(12.0).show(|ui| {
      ui.text("SETTINGS")
        .color(Color::WHITE)
        .align(TextAlign::Center);
      ui.text("Display Mode").color(Color::rgb(180, 180, 180));
      // Focus starts on the mode in use
      let current = *selected;
      for (i, mode) in modes.iter().enumerate() {
        let radio = ui.radio(&mut *selected, i, mode.display_name());
        if i == current {
          radio.default_focus().show();
        } else {
          radio.show();
        }
      }
      ui.row().gap(10.0).justify(Justify::Center).show(|ui| {
        apply = ui
          .button("Apply")
          .width(100.0)
          .height(35.0)
          .variant("pause-selected")
          .clicked();
        back = ui
          .button("Back")
          .width(100.0)
          .height(35.0)
          .variant("danger")
          .clicked();
      });
      ui.text("D-Pad: Navigate | A: Select | B: Back")
        .color(HINT)
        .align(TextAlign::Center);
    });

    if apply {
      let display_mode = modes[self.selected_display_mode_idx];
      crate::save::save_display_mode(display_mode);
      ctx.request_display_mode(display_mode);
      self.settings_open = false;
    }
    if back {
      self.settings_open = false;
    }
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The colour of the control hints
const HINT: Color = Color::rgb(150, 150, 150);

/// A button variant with the given fills for its normal, hovered and
/// pressed states
fn pause_variant(normal: Color, hovered: Color, pressed: Color) -> Variant {
  Variant::of(WidgetKind::Button)
    .normal(LookPatch::default().fill(normal))
    .hovered(LookPatch::default().fill(hovered))
    .pressed(LookPatch::default().fill(pressed))
}
