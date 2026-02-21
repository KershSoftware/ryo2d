//! Pause menu scene (overlay)
//!
//! Pushed on top of GameScene when the player presses Escape or Start.
//! Handles resume, settings, quit-to-menu, and quit-to-desktop.

use ryo2d::prelude::*;

use super::MenuScene;

pub struct PauseScene {
  settings_open: bool,
  selected_display_mode_idx: usize,
  pause_menu_focus: usize,
  settings_menu_focus: usize,
  /// Flag set in render_ui (mouse click) and processed in update (needs SceneContext)
  apply_settings: bool,
  /// Flag set in render_ui (mouse click on Resume)
  resume_requested: bool,
  /// Flag set in render_ui (mouse click on Quit to Menu)
  quit_to_menu: bool,
  /// Flag set in render_ui (mouse click on Quit to Desktop)
  quit_to_desktop: bool,
}

impl PauseScene {
  pub fn new() -> Self {
    Self {
      settings_open: false,
      selected_display_mode_idx: 0,
      pause_menu_focus: 0,
      settings_menu_focus: 0,
      apply_settings: false,
      resume_requested: false,
      quit_to_menu: false,
      quit_to_desktop: false,
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
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Process flags set by mouse clicks in render_ui
    if self.apply_settings {
      self.apply_settings = false;
      let modes = DisplayMode::all();
      let display_mode = modes[self.selected_display_mode_idx];
      ctx.request_display_mode(display_mode);
    }
    if self.resume_requested {
      return SceneAction::Pop;
    }
    if self.quit_to_menu {
      return SceneAction::Reset(Box::new(MenuScene::new()));
    }
    if self.quit_to_desktop {
      return SceneAction::Quit;
    }

    // Escape / Start: close settings submenu or resume game
    if ctx.input.key_pressed_and_consume(KeyCode::Escape)
      || ctx
        .input
        .gamepad_button_pressed_and_consume(GamepadButton::Start)
    {
      if self.settings_open {
        self.settings_open = false;
      } else {
        return SceneAction::Pop;
      }
    }

    // Handle controller/keyboard navigation
    self.handle_navigation(ctx)
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let (screen_w, screen_h) = ui.screen_size();
    self.render_pause_menu(ui, screen_w as f32, screen_h as f32);
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
// Navigation
// ---------------------------------------------------------------------------

impl PauseScene {
  /// Handle controller/keyboard navigation in the pause menu.
  /// Returns a `SceneAction` so callers can act on resume / quit.
  fn handle_navigation(&mut self, ctx: &mut SceneContext) -> SceneAction {
    if self.settings_open {
      self.handle_settings_navigation(ctx);
    } else {
      return self.handle_main_menu_navigation(ctx);
    }
    SceneAction::Continue
  }

  fn handle_settings_navigation(&mut self, ctx: &mut SceneContext) {
    // Settings layout:
    // Row 1: Display modes (0=Windowed, 1=Borderless, 2=Fullscreen)
    // Row 2: Apply (3), Back (4)

    // Up/Down: Move between rows
    if (ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::DPadDown)
      || ctx.input.key_pressed_and_consume(KeyCode::ArrowDown))
      && self.settings_menu_focus < 3
    {
      self.settings_menu_focus = 3;
    }
    if (ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::DPadUp)
      || ctx.input.key_pressed_and_consume(KeyCode::ArrowUp))
      && self.settings_menu_focus >= 3
    {
      self.settings_menu_focus = 1; // Center (Borderless)
    }

    // Left/Right: Move within rows
    if ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::DPadLeft)
      || ctx.input.key_pressed_and_consume(KeyCode::ArrowLeft)
    {
      if self.settings_menu_focus < 3 && self.settings_menu_focus > 0 {
        self.settings_menu_focus -= 1;
      } else if self.settings_menu_focus == 4 {
        self.settings_menu_focus = 3;
      }
    }
    if ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::DPadRight)
      || ctx.input.key_pressed_and_consume(KeyCode::ArrowRight)
    {
      if self.settings_menu_focus < 2 {
        self.settings_menu_focus += 1;
      } else if self.settings_menu_focus == 3 {
        self.settings_menu_focus = 4;
      }
    }

    // A / Enter to select
    if ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::South)
      || ctx.input.key_pressed_and_consume(KeyCode::Enter)
    {
      match self.settings_menu_focus {
        0..=2 => {
          self.selected_display_mode_idx = self.settings_menu_focus;
        }
        3 => {
          // Apply
          let modes = DisplayMode::all();
          let display_mode = modes[self.selected_display_mode_idx];
          crate::save::save_display_mode(display_mode);
          ctx.request_display_mode(display_mode);
          self.settings_open = false;
        }
        4 => {
          // Back
          self.settings_open = false;
        }
        _ => {}
      }
    }

    // B / East to go back
    if ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::East)
    {
      self.settings_open = false;
    }
  }

  fn handle_main_menu_navigation(&mut self, ctx: &mut SceneContext) -> SceneAction {
    let max_items = 4;

    // D-pad or keyboard navigation
    if ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::DPadDown)
      || ctx.input.key_pressed_and_consume(KeyCode::ArrowDown)
    {
      self.pause_menu_focus = (self.pause_menu_focus + 1) % max_items;
    }
    if ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::DPadUp)
      || ctx.input.key_pressed_and_consume(KeyCode::ArrowUp)
    {
      self.pause_menu_focus = (self.pause_menu_focus + max_items - 1) % max_items;
    }

    // A / Enter to select
    if ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::South)
      || ctx.input.key_pressed_and_consume(KeyCode::Enter)
    {
      match self.pause_menu_focus {
        0 => return SceneAction::Pop, // Resume
        1 => {
          self.settings_open = true;
          self.settings_menu_focus = 0;
        }
        2 => return SceneAction::Switch(Box::new(MenuScene::new())), // Quit to Menu
        3 => return SceneAction::Quit,                               // Quit to Desktop
        _ => {}
      }
    }

    // B / East to resume
    if ctx
      .input
      .gamepad_button_pressed_and_consume(GamepadButton::East)
    {
      return SceneAction::Pop;
    }

    SceneAction::Continue
  }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

impl PauseScene {
  fn render_pause_menu(&mut self, ui: &mut UiContext, screen_w: f32, screen_h: f32) {
    // Darken background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w, screen_h),
      Color::new(0, 0, 0, 180),
    );

    let center_x = screen_w / 2.0;
    let center_y = screen_h / 2.0;

    if self.settings_open {
      self.render_settings(ui, center_x, center_y);
    } else {
      self.render_main_menu(ui, center_x, center_y);
    }
  }

  fn render_settings(&mut self, ui: &mut UiContext, center_x: f32, center_y: f32) {
    let modal_w = 400.0;
    let modal_h = 180.0;
    let modal_x = center_x - modal_w / 2.0;
    let modal_y = center_y - modal_h / 2.0;

    // Modal background
    ui.draw_rect(
      Rect::new(modal_x, modal_y, modal_w, modal_h),
      Color::rgb(40, 40, 50),
    );
    ui.draw_rect_outline(
      Rect::new(modal_x, modal_y, modal_w, modal_h),
      Color::rgb(100, 100, 120),
      2.0,
    );

    // Title
    ui.draw_text_anchored(
      "SETTINGS",
      Anchor::TopCenter,
      0.0,
      modal_y + 20.0,
      Color::WHITE,
    );

    // Display mode options
    let mode_y = modal_y + 55.0;
    ui.draw_text(
      "Display Mode:",
      modal_x + 20.0,
      mode_y,
      Color::rgb(180, 180, 180),
    );

    let modes = DisplayMode::all();
    let settings_focus = self.settings_menu_focus;
    for (i, mode) in modes.iter().enumerate() {
      let btn_x = modal_x + 20.0 + i as f32 * 120.0;
      let btn_y = mode_y + 25.0;
      let btn_rect = Rect::new(btn_x, btn_y, 110.0, 28.0);

      let is_selected = i == self.selected_display_mode_idx;
      let is_focused = settings_focus == i;
      let (bg, hover, pressed) = if is_selected {
        (
          Color::rgb(60, 100, 60),
          Color::rgb(70, 110, 70),
          Color::rgb(50, 90, 50),
        )
      } else {
        (
          Color::rgb(50, 50, 60),
          Color::rgb(60, 60, 70),
          Color::rgb(40, 40, 50),
        )
      };

      if is_focused {
        ui.draw_rect_outline(btn_rect, Color::rgb(255, 220, 100), 2.0);
      }

      if ui.button_styled(
        mode.display_name(),
        btn_rect,
        bg,
        hover,
        pressed,
        Color::WHITE,
      ) {
        self.selected_display_mode_idx = i;
      }
    }

    // Apply and Back buttons
    let btn_y = modal_y + modal_h - 50.0;
    let apply_rect = Rect::new(center_x - 110.0, btn_y, 100.0, 35.0);
    let back_rect = Rect::new(center_x + 10.0, btn_y, 100.0, 35.0);

    if settings_focus == 3 {
      ui.draw_rect_outline(apply_rect, Color::rgb(255, 220, 100), 2.0);
    }

    if ui.button_styled(
      "Apply",
      apply_rect,
      Color::rgb(50, 80, 50),
      Color::rgb(60, 100, 60),
      Color::rgb(40, 70, 40),
      Color::WHITE,
    ) {
      let display_mode = modes[self.selected_display_mode_idx];
      crate::save::save_display_mode(display_mode);
      // Flag for update() to call ctx.request_display_mode (needs SceneContext)
      self.apply_settings = true;
      self.settings_open = false;
    }

    if settings_focus == 4 {
      ui.draw_rect_outline(back_rect, Color::rgb(255, 220, 100), 2.0);
    }

    if ui.button_styled(
      "Back",
      back_rect,
      Color::rgb(70, 50, 50),
      Color::rgb(90, 60, 60),
      Color::rgb(60, 40, 40),
      Color::WHITE,
    ) {
      self.settings_open = false;
    }

    // Navigation hint
    ui.draw_text_anchored(
      "D-Pad: Navigate | A: Select | B: Back",
      Anchor::TopCenter,
      0.0,
      modal_y + modal_h + 10.0,
      Color::rgb(150, 150, 150),
    );
  }

  fn render_main_menu(&mut self, ui: &mut UiContext, center_x: f32, center_y: f32) {
    ui.draw_text_anchored(
      "PAUSED",
      Anchor::TopCenter,
      0.0,
      center_y - 100.0,
      Color::WHITE,
    );

    let btn_w = 200.0;
    let btn_h = 40.0;
    let btn_x = center_x - btn_w / 2.0;
    let focus = self.pause_menu_focus;

    let get_btn_colors = |base_r: u8, base_g: u8, base_b: u8, is_focused: bool| {
      if is_focused {
        (
          Color::rgb(base_r + 60, base_g + 60, base_b + 30),
          Color::rgb(base_r + 80, base_g + 80, base_b + 40),
          Color::rgb(base_r + 40, base_g + 40, base_b + 20),
        )
      } else {
        (
          Color::rgb(base_r + 30, base_g + 30, base_b + 30),
          Color::rgb(base_r + 50, base_g + 50, base_b + 50),
          Color::rgb(base_r + 20, base_g + 20, base_b + 20),
        )
      }
    };

    // Resume
    let resume_rect = Rect::new(btn_x, center_y - 50.0, btn_w, btn_h);
    let (bg, hover, pressed) = get_btn_colors(40, 70, 40, focus == 0);
    if focus == 0 {
      ui.draw_rect_outline(resume_rect, Color::rgb(255, 220, 100), 2.0);
    }
    if ui.button_styled("Resume", resume_rect, bg, hover, pressed, Color::WHITE) {
      self.resume_requested = true;
    }

    // Settings
    let settings_rect = Rect::new(btn_x, center_y, btn_w, btn_h);
    let (bg, hover, pressed) = get_btn_colors(40, 40, 70, focus == 1);
    if focus == 1 {
      ui.draw_rect_outline(settings_rect, Color::rgb(255, 220, 100), 2.0);
    }
    if ui.button_styled("Settings", settings_rect, bg, hover, pressed, Color::WHITE) {
      self.settings_open = true;
      self.settings_menu_focus = 0;
    }

    // Quit to Menu
    let quit_menu_rect = Rect::new(btn_x, center_y + 50.0, btn_w, btn_h);
    let (bg, hover, pressed) = get_btn_colors(70, 50, 30, focus == 2);
    if focus == 2 {
      ui.draw_rect_outline(quit_menu_rect, Color::rgb(255, 220, 100), 2.0);
    }
    if ui.button_styled(
      "Quit to Menu",
      quit_menu_rect,
      bg,
      hover,
      pressed,
      Color::WHITE,
    ) {
      self.quit_to_menu = true;
    }

    // Quit to Desktop
    let quit_desktop_rect = Rect::new(btn_x, center_y + 100.0, btn_w, btn_h);
    let (bg, hover, pressed) = get_btn_colors(70, 40, 40, focus == 3);
    if focus == 3 {
      ui.draw_rect_outline(quit_desktop_rect, Color::rgb(255, 220, 100), 2.0);
    }
    if ui.button_styled(
      "Quit to Desktop",
      quit_desktop_rect,
      bg,
      hover,
      pressed,
      Color::WHITE,
    ) {
      self.quit_to_desktop = true;
    }

    // Instructions
    ui.draw_text_anchored(
      "D-Pad/Arrows: Navigate | A/Enter: Select | B/ESC: Resume",
      Anchor::TopCenter,
      0.0,
      center_y + 170.0,
      Color::rgb(150, 150, 150),
    );
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
