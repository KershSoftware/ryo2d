//! Dropdown demo page - demonstrates the dropdown widget

use demo_kit::prelude::*;

const DIFFICULTY: [&str; 4] = ["Easy", "Medium", "Hard", "Nightmare"];
const RESOLUTION: [&str; 5] = [
  "1280x720",
  "1600x900",
  "1920x1080",
  "2560x1440",
  "3840x2160",
];
const AUDIO: [&str; 5] = [
  "Default Device",
  "Speakers",
  "Headphones",
  "USB Audio",
  "HDMI Audio",
];
const LANGUAGE: [&str; 7] = [
  "English",
  "Español",
  "Français",
  "Deutsch",
  "Ελληνικά",
  "Русский",
  "Čeština",
];

pub struct DropdownDemo {
  /// Index into `DIFFICULTY`
  difficulty: usize,
  /// Index into `RESOLUTION`
  resolution: usize,
  /// Index into `AUDIO`
  audio_device: usize,
  /// Index into `LANGUAGE`
  language: usize,
}

impl DropdownDemo {
  pub fn new() -> Self {
    Self {
      difficulty: 1, // Medium
      resolution: 2, // 1920x1080
      audio_device: 0,
      language: 0,
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
    ui.draw_text("Difficulty:", 35.0, 115.0 + oy, Color::LIGHT_GRAY);
    ui.draw_text("Resolution:", 35.0, 160.0 + oy, Color::LIGHT_GRAY);
    ui.draw_text("Audio:", 35.0, 205.0 + oy, Color::LIGHT_GRAY);
    ui.draw_text("Language:", 35.0, 250.0 + oy, Color::LIGHT_GRAY);

    // The dropdowns share one focus scope: the arrows and Tab move between
    // them, Enter or Space opens one, and an open list keeps the focus
    ui.column().focus_scope().show(|ui| {
      ui.dropdown(&mut self.difficulty, &DIFFICULTY)
        .at(Rect::new(140.0, 110.0 + oy, 200.0, 28.0))
        .default_focus()
        .show();
      ui.dropdown(&mut self.resolution, &RESOLUTION)
        .at(Rect::new(140.0, 155.0 + oy, 200.0, 28.0))
        .show();
      ui.dropdown(&mut self.audio_device, &AUDIO)
        .at(Rect::new(140.0, 200.0 + oy, 200.0, 28.0))
        .show();
      ui.dropdown(&mut self.language, &LANGUAGE)
        .at(Rect::new(140.0, 245.0 + oy, 200.0, 28.0))
        .show();
    });

    // --- Current Selections Display ---
    let info_x = 450.0;
    ui.draw_text(
      "Current Selections:",
      info_x,
      80.0 + oy,
      Color::rgb(100, 180, 255),
    );
    let selections = [
      ("Difficulty", DIFFICULTY[self.difficulty]),
      ("Resolution", RESOLUTION[self.resolution]),
      ("Audio", AUDIO[self.audio_device]),
      ("Language", LANGUAGE[self.language]),
    ];
    for (i, (name, value)) in selections.iter().enumerate() {
      ui.draw_text(
        &format!("{name}: {value}"),
        info_x + 10.0,
        110.0 + oy + i as f32 * 20.0,
        Color::WHITE,
      );
    }

    // --- Usage Notes ---
    let notes_y = 390.0 + oy;
    ui.draw_text("Features:", 20.0, notes_y, Color::rgb(100, 180, 255));
    let notes = [
      "- Click a dropdown to open or close its list",
      "- Arrow Up/Down move through an open list",
      "- Enter or Space picks, Escape closes the list",
      "- Tab/Shift+Tab or the arrows move between dropdowns",
      "- Space or Enter opens the focused dropdown",
    ];
    for (i, note) in notes.iter().enumerate() {
      ui.draw_text(
        note,
        30.0,
        notes_y + 20.0 + i as f32 * 18.0,
        Color::LIGHT_GRAY,
      );
    }
  }
}
