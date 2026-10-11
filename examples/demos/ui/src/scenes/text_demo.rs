//! Text demo page - demonstrates text wrapping

use demo_kit::prelude::*;

pub struct TextDemo {
  /// Width of the wrapping area
  wrap_width: i32,
  /// Sample texts to demonstrate wrapping
  sample_index: usize,
}

impl TextDemo {
  pub fn new() -> Self {
    Self {
      wrap_width: 200,
      sample_index: 0,
    }
  }

  fn sample_texts() -> &'static [(&'static str, &'static str)] {
    &[
      ("Short text", "Hello, world!"),
      (
        "Long sentence",
        "The quick brown fox jumps over the lazy dog near the riverbank.",
      ),
      (
        "Multiple paragraphs",
        "First paragraph with some text.\n\nSecond paragraph after a blank line.\n\nThird paragraph to show spacing.",
      ),
      (
        "Long word",
        "This has a supercalifragilisticexpialidocious word that must break.",
      ),
      (
        "Mixed content",
        "Short line.\nA much longer line that should wrap around nicely.\nAnother short one.",
      ),
    ]
  }
}

impl Default for TextDemo {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for TextDemo {
  fn name(&self) -> &str {
    "Text Wrapping"
  }

  fn description(&self) -> &str {
    "Word wrap and text layout."
  }

  fn settings(&mut self, s: &mut Settings) {
    let names: Vec<&str> = Self::sample_texts()
      .iter()
      .map(|(title, _)| *title)
      .collect();
    s.choice("Sample", &mut self.sample_index, &names);
    s.int("Wrap width", &mut self.wrap_width, 50..=600);
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();
    let top = content_area(ui).y;
    let wrap_width = self.wrap_width as f32;

    // Background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::rgb(25, 25, 35),
    );

    let samples = Self::sample_texts();
    let (_, text) = samples[self.sample_index.min(samples.len() - 1)];

    // Wrapping demonstration area
    let demo_x = 50.0;
    let demo_y = top + 30.0;
    let max_width = self.wrap_width as u32;

    // Draw boundary box to show wrap width
    let (_, wrapped_h) = ui.wrapped_text_size(text, max_width);
    let boundary = Rect::new(
      demo_x - 5.0,
      demo_y - 5.0,
      wrap_width + 10.0,
      wrapped_h as f32 + 10.0,
    );
    ui.draw_panel(
      boundary,
      Color::rgb(35, 35, 50),
      Some(Color::rgb(80, 80, 100)),
    );

    // Draw the wrapped text
    ui.draw_text_wrapped(text, demo_x, demo_y, max_width, Color::WHITE);

    // Show the wrapped lines for debugging
    let lines = ui.wrap_text(text, max_width);
    let debug_y = demo_y + wrapped_h as f32 + 40.0;
    ui.draw_text("Wrapped lines:", demo_x, debug_y, Color::GRAY);
    for (i, line) in lines.iter().enumerate() {
      let line_y = debug_y + 20.0 + (i as f32 * 15.0);
      let display = if line.is_empty() { "(empty)" } else { line };
      ui.draw_text(
        &format!("  {}: \"{}\"", i + 1, display),
        demo_x,
        line_y,
        Color::rgb(150, 150, 150),
      );
    }

    // Centered text demo on the right side
    let center_x = screen_w as f32 / 2.0 + 100.0;
    let center_demo_y = demo_y;
    let center_rect = Rect::new(
      center_x - wrap_width / 2.0 - 5.0,
      center_demo_y - 5.0,
      wrap_width + 10.0,
      wrapped_h as f32 + 10.0,
    );

    ui.draw_text(
      "Centered:",
      center_x - wrap_width / 2.0,
      center_demo_y - 25.0,
      Color::CYAN,
    );
    ui.draw_panel(
      center_rect,
      Color::rgb(35, 35, 50),
      Some(Color::rgb(80, 80, 100)),
    );

    // Create rect for centered wrapped text
    let text_rect = Rect::new(
      center_x - wrap_width / 2.0,
      center_demo_y,
      wrap_width,
      wrapped_h as f32,
    );
    ui.draw_text_wrapped_centered(text, text_rect, Color::WHITE);
  }
}
