//! Text input demo page - demonstrates the text input widget
//!
//! Four fields editing the page's strings, in one focus scope: a click or
//! Tab / Shift+Tab moves between them, Escape leaves one.

use demo_kit::prelude::*;

pub struct TextInputDemo {
  /// Username text
  username: String,
  /// Password text (shown masked)
  password: String,
  /// Bio/description text
  bio: String,
  /// Search text
  search: String,
  /// Track if form was "submitted"
  submitted: bool,
  /// Last submitted username
  last_submitted_username: String,
}

impl TextInputDemo {
  pub fn new() -> Self {
    Self {
      username: String::new(),
      password: String::new(),
      bio: "Type your bio here...".to_string(),
      search: String::new(),
      submitted: false,
      last_submitted_username: String::new(),
    }
  }
}

impl DemoPage for TextInputDemo {
  fn name(&self) -> &str {
    "Text Input"
  }

  fn description(&self) -> &str {
    "Editable text fields with a cursor and selection."
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Enter, "Submit the username"),
      Control::key(KeyCode::Tab, "Next field"),
    ]
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    // Get screen dimensions
    let (sw, _sh) = ui.screen_size();
    let top = content_area(ui).y;

    // Form layout
    let form_x = 20.0;
    let mut y = top + 10.0;
    let label_color = Color::WHITE;
    let field_width = 300.0;
    let field_height = 28.0;

    // Each field's label above it
    let mut field_at = |ui: &mut UiContext, label: &str, width: f32| {
      ui.draw_text(label, form_x, y, label_color);
      let rect = Rect::new(form_x, y + 20.0, width, field_height);
      y += 20.0 + field_height + 15.0;
      rect
    };
    let username_rect = field_at(ui, "Username:", field_width);
    let password_rect = field_at(ui, "Password:", field_width);
    // Bio field (wider)
    let bio_rect = field_at(ui, "Bio:", field_width + 100.0);
    let search_rect = field_at(ui, "Search:", 200.0);
    y += 10.0;

    // One focus scope, so Tab walks the fields; each keeps its rect
    let mut focused = "None";
    let mut submitted = false;
    ui.column().focus_scope().show(|ui| {
      let username = ui
        .text_input(&mut self.username)
        .at(username_rect)
        .placeholder("Enter username...")
        .default_focus()
        .show();
      submitted = username.submitted();
      let password = ui
        .text_input(&mut self.password)
        .at(password_rect)
        .placeholder("Enter password...")
        .password()
        .show();
      let bio = ui
        .text_input(&mut self.bio)
        .at(bio_rect)
        .placeholder("Tell us about yourself...")
        .show();
      let search = ui
        .text_input(&mut self.search)
        .at(search_rect)
        .placeholder("Search...")
        .show();
      for (name, field) in [
        ("Username", &username),
        ("Password", &password),
        ("Bio", &bio),
        ("Search", &search),
      ] {
        if field.focused() {
          focused = name;
        }
      }
    });

    // Enter in the username "submits" the form
    if submitted {
      self.submitted = true;
      self.last_submitted_username = self.username.clone();
    }

    // Instructions
    ui.draw_text("Instructions:", form_x, y, Color::rgb(100, 180, 255));
    y += 20.0;
    for line in [
      "- Click a field to focus it",
      "- Tab / Shift+Tab to navigate",
      "- Type to enter text",
      "- Arrow keys to move cursor",
      "- Shift+Arrow to select text",
      "- Ctrl+C/X/V for clipboard",
      "- Ctrl+A to select all",
      "- Escape to leave a field",
      "- Enter in username to 'submit'",
    ] {
      ui.draw_text(line, form_x + 10.0, y, Color::LIGHT_GRAY);
      y += 16.0;
    }
    y += 9.0;

    // Show submitted message if applicable
    if self.submitted {
      ui.draw_text(
        &format!("Submitted! Username: {}", self.last_submitted_username),
        form_x,
        y,
        Color::rgb(80, 200, 80),
      );
    }

    // Debug panel on the right showing current state
    let debug_x = sw as f32 - 280.0;
    let mut debug_y = top + 10.0;

    ui.draw_text("Debug Info:", debug_x, debug_y, Color::rgb(100, 180, 255));
    debug_y += 25.0;

    // Show which field is focused
    ui.draw_text(
      &format!("Focused: {}", focused),
      debug_x,
      debug_y,
      Color::WHITE,
    );
    debug_y += 20.0;

    // Show username text length
    ui.draw_text(
      &format!("Username length: {}", self.username.chars().count()),
      debug_x,
      debug_y,
      Color::WHITE,
    );
    debug_y += 20.0;

    // The password's length: the field shows only bullets
    ui.draw_text(
      &format!("Password length: {}", self.password.chars().count()),
      debug_x,
      debug_y,
      Color::WHITE,
    );
    debug_y += 20.0;

    // Show current text content
    let text_preview = if self.username.chars().count() > 20 {
      let cut: String = self.username.chars().take(20).collect();
      format!("{}...", cut)
    } else {
      self.username.clone()
    };
    ui.draw_text(
      &format!("Text: \"{}\"", text_preview),
      debug_x,
      debug_y,
      Color::WHITE,
    );
  }
}
