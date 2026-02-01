//! Text input demo scene - demonstrates the text input widget

use ryo2d::prelude::*;

pub struct TextInputDemo {
  /// Username text input state
  username: TextInputState,
  /// Password text input state
  password: TextInputState,
  /// Bio/description text input (multiline-style display)
  bio: TextInputState,
  /// Search field
  search: TextInputState,
  /// Track if form was "submitted"
  submitted: bool,
  /// Last submitted username
  last_submitted_username: String,
  /// Delta time from last frame (for cursor blinking)
  last_dt: f32,
}

impl TextInputDemo {
  pub fn new() -> Self {
    Self {
      username: TextInputState::new(),
      password: TextInputState::new(),
      bio: TextInputState::with_text("Type your bio here..."),
      search: TextInputState::new(),
      submitted: false,
      last_submitted_username: String::new(),
      last_dt: 0.016, // Default to ~60fps
    }
  }

  /// Cycle focus between input fields.
  /// If `reverse` is true (Shift+Tab), go backwards.
  fn cycle_focus(&mut self, reverse: bool) {
    // Field order: username -> password -> bio -> search -> (wrap)
    let current = if self.username.is_focused() {
      0
    } else if self.password.is_focused() {
      1
    } else if self.bio.is_focused() {
      2
    } else if self.search.is_focused() {
      3
    } else {
      // None focused, start at first (or last if reverse)
      if reverse { 4 } else { usize::MAX }
    };

    // Clear all focus first
    self.username.set_focused(false);
    self.password.set_focused(false);
    self.bio.set_focused(false);
    self.search.set_focused(false);

    // Calculate next field
    let next = if reverse {
      if current == 0 { 3 } else { current.saturating_sub(1) }
    } else if current >= 3 {
      0
    } else {
      current.wrapping_add(1)
    };

    // Set focus on next field
    match next {
      0 => self.username.set_focused(true),
      1 => self.password.set_focused(true),
      2 => self.bio.set_focused(true),
      3 => self.search.set_focused(true),
      _ => self.username.set_focused(true),
    }
  }
}

impl Scene for TextInputDemo {
  fn name(&self) -> &str {
    "Text Input Demo"
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Store dt for use in render_ui
    self.last_dt = dt;

    // Press Escape to go back to main menu
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Pop;
    }

    // Handle Tab to cycle between fields
    if ctx.input.key_pressed(KeyCode::Tab) {
      let shift = ctx.input.key_down(KeyCode::ShiftLeft) || ctx.input.key_down(KeyCode::ShiftRight);
      self.cycle_focus(shift);
    }

    // Check for Enter key to "submit" the form when username is focused
    if self.username.is_focused() && ctx.input.key_pressed(KeyCode::Enter) {
      self.submitted = true;
      self.last_submitted_username = self.username.text().to_string();
      self.username.set_focused(false);
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let dt = self.last_dt;
    
    // Get screen dimensions
    let (sw, _sh) = ui.screen_size();

    // Title
    ui.draw_text("Text Input Demo", 20.0, 20.0, Color::WHITE);
    ui.draw_text("Press ESC to return to menu", 20.0, 40.0, Color::GRAY);

    // Form layout
    let form_x = 20.0;
    let mut y = 80.0;
    let label_color = Color::WHITE;
    let field_width = 300.0;
    let field_height = 28.0;

    // Username field
    ui.draw_text("Username:", form_x, y, label_color);
    y += 20.0;
    let username_rect = Rect::new(form_x, y, field_width, field_height);
    if ui.text_input(&mut self.username, username_rect, "Enter username...", dt) {
      // Text changed - could validate here
    }
    y += field_height + 15.0;

    // Password field (note: we can't mask characters yet, this is for demo)
    ui.draw_text("Password:", form_x, y, label_color);
    y += 20.0;
    let password_rect = Rect::new(form_x, y, field_width, field_height);
    ui.text_input(&mut self.password, password_rect, "Enter password...", dt);
    y += field_height + 15.0;

    // Bio field (wider)
    ui.draw_text("Bio:", form_x, y, label_color);
    y += 20.0;
    let bio_rect = Rect::new(form_x, y, field_width + 100.0, field_height);
    ui.text_input(&mut self.bio, bio_rect, "Tell us about yourself...", dt);
    y += field_height + 15.0;

    // Search field with icon-style label
    ui.draw_text("Search:", form_x, y, label_color);
    y += 20.0;
    let search_rect = Rect::new(form_x, y, 200.0, field_height);
    ui.text_input(&mut self.search, search_rect, "Search...", dt);
    y += field_height + 25.0;

    // Instructions
    ui.draw_text("Instructions:", form_x, y, Color::rgb(100, 180, 255));
    y += 20.0;
    ui.draw_text("- Click a field to focus it", form_x + 10.0, y, Color::LIGHT_GRAY);
    y += 16.0;
    ui.draw_text("- Type to enter text", form_x + 10.0, y, Color::LIGHT_GRAY);
    y += 16.0;
    ui.draw_text("- Arrow keys to move cursor", form_x + 10.0, y, Color::LIGHT_GRAY);
    y += 16.0;
    ui.draw_text("- Shift+Arrow to select text", form_x + 10.0, y, Color::LIGHT_GRAY);
    y += 16.0;
    ui.draw_text("- Backspace/Delete to remove", form_x + 10.0, y, Color::LIGHT_GRAY);
    y += 16.0;
    ui.draw_text("- Ctrl+A to select all", form_x + 10.0, y, Color::LIGHT_GRAY);
    y += 16.0;
    ui.draw_text("- Home/End to jump", form_x + 10.0, y, Color::LIGHT_GRAY);
    y += 16.0;
    ui.draw_text("- Enter in username to 'submit'", form_x + 10.0, y, Color::LIGHT_GRAY);
    y += 25.0;

    // Show submitted message if applicable
    if self.submitted {
      ui.draw_text(
        &format!("Submitted! Username: {}", self.last_submitted_username),
        form_x,
        y,
        Color::rgb(80, 200, 80),
      );
      // Silence unused assignment warning
      let _ = y;
    }

    // Debug panel on the right showing current state
    let debug_x = sw as f32 - 280.0;
    let mut debug_y = 80.0;

    ui.draw_text("Debug Info:", debug_x, debug_y, Color::rgb(100, 180, 255));
    debug_y += 25.0;

    // Show which field is focused
    let focused_name = if self.username.is_focused() {
      "Username"
    } else if self.password.is_focused() {
      "Password"
    } else if self.bio.is_focused() {
      "Bio"
    } else if self.search.is_focused() {
      "Search"
    } else {
      "None"
    };
    ui.draw_text(&format!("Focused: {}", focused_name), debug_x, debug_y, Color::WHITE);
    debug_y += 20.0;

    // Show cursor position for username
    ui.draw_text(
      &format!("Username cursor: {}", self.username.cursor()),
      debug_x,
      debug_y,
      Color::WHITE,
    );
    debug_y += 20.0;

    // Show selection info
    if let Some((start, end)) = self.username.selection_range() {
      ui.draw_text(
        &format!("Selection: {}..{}", start, end),
        debug_x,
        debug_y,
        Color::WHITE,
      );
    } else {
      ui.draw_text("Selection: None", debug_x, debug_y, Color::WHITE);
    }
    debug_y += 20.0;

    // Show username text length
    ui.draw_text(
      &format!("Text length: {}", self.username.text().len()),
      debug_x,
      debug_y,
      Color::WHITE,
    );
    debug_y += 20.0;

    // Show current text content
    let text_preview = if self.username.text().len() > 20 {
      format!("{}...", &self.username.text()[..20])
    } else {
      self.username.text().to_string()
    };
    ui.draw_text(&format!("Text: \"{}\"", text_preview), debug_x, debug_y, Color::WHITE);
  }
}
