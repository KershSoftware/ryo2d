//! Dropdown demo page - demonstrates the dropdown widget

use demo_kit::prelude::*;

pub struct DropdownDemo {
  /// Difficulty dropdown
  difficulty: DropdownState,
  /// Resolution dropdown
  resolution: DropdownState,
  /// Audio device dropdown
  audio_device: DropdownState,
  /// Language dropdown
  language: DropdownState,
}

impl DropdownDemo {
  pub fn new() -> Self {
    Self {
      difficulty: DropdownState::with_id_and_selected(1, 1), // ID 1, Medium by default
      resolution: DropdownState::with_id_and_selected(2, 2), // ID 2, 1920x1080 by default
      audio_device: DropdownState::with_id(3),               // ID 3
      language: DropdownState::with_id(4),                   // ID 4
    }
  }
}

impl DemoPage for DropdownDemo {
  fn name(&self) -> &str {
    "Dropdowns"
  }

  fn description(&self) -> &str {
    "Pick from a list of options with mouse or keyboard."
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Tab, "Next dropdown"),
      Control::mouse("Click a dropdown to open it"),
    ]
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    // Everything below was laid out for a panel at y = 65
    let oy = content_area(ui).y - 65.0;

    // --- Settings Panel ---
    let panel_rect = Rect::new(20.0, 65.0 + oy, 400.0, 300.0);
    ui.draw_rect(panel_rect, Color::rgb(35, 35, 45));
    ui.draw_rect_outline(panel_rect, Color::rgb(60, 60, 80), 1.0);

    ui.draw_text("Game Settings", 35.0, 80.0 + oy, Color::rgb(100, 180, 255));

    // Difficulty dropdown
    ui.draw_text("Difficulty:", 35.0, 115.0 + oy, Color::LIGHT_GRAY);
    let difficulty_options = ["Easy", "Medium", "Hard", "Nightmare"];
    let difficulty_rect = Rect::new(140.0, 110.0 + oy, 200.0, 28.0);
    if ui.dropdown(&mut self.difficulty, difficulty_rect, &difficulty_options) {
      // Selection changed - could trigger game logic here
    }

    // Resolution dropdown
    ui.draw_text("Resolution:", 35.0, 160.0 + oy, Color::LIGHT_GRAY);
    let resolution_options = [
      "1280x720",
      "1600x900",
      "1920x1080",
      "2560x1440",
      "3840x2160",
    ];
    let resolution_rect = Rect::new(140.0, 155.0 + oy, 200.0, 28.0);
    ui.dropdown(&mut self.resolution, resolution_rect, &resolution_options);

    // Audio device dropdown
    ui.draw_text("Audio:", 35.0, 205.0 + oy, Color::LIGHT_GRAY);
    let audio_options = [
      "Default Device",
      "Speakers",
      "Headphones",
      "USB Audio",
      "HDMI Audio",
    ];
    let audio_rect = Rect::new(140.0, 200.0 + oy, 200.0, 28.0);
    ui.dropdown(&mut self.audio_device, audio_rect, &audio_options);

    // Language dropdown
    ui.draw_text("Language:", 35.0, 250.0 + oy, Color::LIGHT_GRAY);
    let language_options = [
      "English",
      "Español",
      "Français",
      "Deutsch",
      "Ελληνικά",
      "Русский",
      "Čeština",
    ];
    let language_rect = Rect::new(140.0, 245.0 + oy, 200.0, 28.0);
    ui.dropdown(&mut self.language, language_rect, &language_options);

    // --- Current Selections Display ---
    let info_x = 450.0;
    ui.draw_text(
      "Current Selections:",
      info_x,
      80.0 + oy,
      Color::rgb(100, 180, 255),
    );

    ui.draw_text(
      &format!(
        "Difficulty: {}",
        difficulty_options[self.difficulty.selected()]
      ),
      info_x + 10.0,
      110.0 + oy,
      Color::WHITE,
    );
    ui.draw_text(
      &format!(
        "Resolution: {}",
        resolution_options[self.resolution.selected()]
      ),
      info_x + 10.0,
      130.0 + oy,
      Color::WHITE,
    );
    ui.draw_text(
      &format!("Audio: {}", audio_options[self.audio_device.selected()]),
      info_x + 10.0,
      150.0 + oy,
      Color::WHITE,
    );
    ui.draw_text(
      &format!("Language: {}", language_options[self.language.selected()]),
      info_x + 10.0,
      170.0 + oy,
      Color::WHITE,
    );

    // --- Usage Notes ---
    let notes_y = 390.0 + oy;
    ui.draw_text("Features:", 20.0, notes_y, Color::rgb(100, 180, 255));
    ui.draw_text(
      "- Click trigger to open/close dropdown",
      30.0,
      notes_y + 20.0,
      Color::LIGHT_GRAY,
    );
    ui.draw_text(
      "- Arrow Up/Down to navigate options",
      30.0,
      notes_y + 38.0,
      Color::LIGHT_GRAY,
    );
    ui.draw_text(
      "- Enter to select, Escape to cancel",
      30.0,
      notes_y + 56.0,
      Color::LIGHT_GRAY,
    );
    ui.draw_text(
      "- Tab/Shift+Tab to navigate between dropdowns",
      30.0,
      notes_y + 74.0,
      Color::LIGHT_GRAY,
    );
    ui.draw_text(
      "- Space or Enter to open focused dropdown",
      30.0,
      notes_y + 92.0,
      Color::LIGHT_GRAY,
    );

    // Handle tab navigation between dropdowns
    ui.handle_dropdown_tab_navigation(&mut [
      &mut self.difficulty,
      &mut self.resolution,
      &mut self.audio_device,
      &mut self.language,
    ]);
  }
}
