//! Quit Confirmation Scene
//!
//! Overlay shown when the user presses Escape from the main menu.

use ryo2d::prelude::*;

/// Quit confirmation overlay
pub struct QuitConfirmScene {
  /// Wait for escape to be released before accepting input
  escape_was_released: bool,
  /// Pending actions from button clicks (set in render_ui, consumed in update)
  yes_clicked: bool,
  no_clicked: bool,
}

impl QuitConfirmScene {
  /// Create a new quit confirmation scene
  pub fn new() -> Self {
    Self {
      escape_was_released: false,
      yes_clicked: false,
      no_clicked: false,
    }
  }
}

impl Default for QuitConfirmScene {
  fn default() -> Self {
    Self::new()
  }
}

impl Scene for QuitConfirmScene {
  fn on_enter(&mut self, _ctx: &mut SceneContext) {
    // Overlay - no world changes needed
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Wait for Escape/B to be released before accepting new presses
    if !self.escape_was_released
      && !ctx.input.key_down(KeyCode::Escape)
      && !ctx.input.gamepad_button_down(GamepadButton::East)
    {
      self.escape_was_released = true;
    }

    // Check for button clicks from previous render_ui
    if self.yes_clicked {
      self.yes_clicked = false;
      return SceneAction::Quit;
    }

    if self.no_clicked {
      self.no_clicked = false;
      return SceneAction::Pop;
    }

    // N or B to cancel (B only after released once)
    // Use consume to prevent input from cascading to underlying scene
    if ctx.input.key_pressed_and_consume(KeyCode::KeyN) {
      return SceneAction::Pop;
    }

    // ESC or B to cancel (only after released once)
    if self.escape_was_released
      && (ctx.input.key_pressed_and_consume(KeyCode::Escape)
        || ctx
          .input
          .gamepad_button_pressed_and_consume(GamepadButton::East))
    {
      return SceneAction::Pop;
    }

    // Y or A to confirm quit
    if ctx.input.key_pressed_and_consume(KeyCode::KeyY)
      || ctx
        .input
        .gamepad_button_pressed_and_consume(GamepadButton::South)
    {
      return SceneAction::Quit;
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

    // Confirmation panel
    let panel_w = 350.0;
    let panel_h = 180.0;
    let panel_x = (screen_w as f32 - panel_w) / 2.0;
    let panel_y = (screen_h as f32 - panel_h) / 2.0;

    ui.draw_panel(
      Rect::new(panel_x, panel_y, panel_w, panel_h),
      Color::rgb(30, 30, 50),
      Some(Color::WHITE),
    );

    // Title
    ui.draw_text_centered(
      "Quit to Desktop?",
      Rect::new(panel_x, panel_y + 30.0, panel_w, 30.0),
      Color::WHITE,
    );

    // Yes button
    let yes_rect = Rect::new(panel_x + 30.0, panel_y + 100.0, 140.0, 40.0);
    if ui.button("YES (Y/A)", yes_rect) {
      self.yes_clicked = true;
    }

    // No button
    let no_rect = Rect::new(panel_x + 180.0, panel_y + 100.0, 140.0, 40.0);
    if ui.button("NO (N/B)", no_rect) {
      self.no_clicked = true;
    }
  }

  fn is_transparent(&self) -> bool {
    true // Show menu behind overlay
  }

  fn name(&self) -> &str {
    "QuitConfirmScene"
  }
}
