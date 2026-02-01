//! Text demo scene - demonstrates text wrapping and (future) text input

use ryo2d::prelude::*;

pub struct TextDemo {
  /// Width of the wrapping area
  wrap_width: f32,
  /// Sample texts to demonstrate wrapping
  sample_index: usize,
}

impl TextDemo {
  pub fn new() -> Self {
    Self {
      wrap_width: 200.0,
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

impl Scene for TextDemo {
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Back to menu
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Pop;
    }

    // Navigate samples with arrow keys
    let samples = Self::sample_texts();
    if ctx.input.key_pressed(KeyCode::ArrowUp) {
      if self.sample_index > 0 {
        self.sample_index -= 1;
      }
    }
    if ctx.input.key_pressed(KeyCode::ArrowDown) {
      if self.sample_index < samples.len() - 1 {
        self.sample_index += 1;
      }
    }

    // Adjust width with left/right
    if ctx.input.key_down(KeyCode::ArrowLeft) {
      self.wrap_width = (self.wrap_width - 2.0).max(50.0);
    }
    if ctx.input.key_down(KeyCode::ArrowRight) {
      self.wrap_width = (self.wrap_width + 2.0).min(600.0);
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::rgb(25, 25, 35),
    );

    // Title
    ui.draw_text_anchored(
      "Text Wrapping Demo",
      Anchor::TopCenter,
      0.0,
      20.0,
      Color::WHITE,
    );

    // Instructions
    ui.draw_text(
      "ESC: Back | Up/Down: Change sample | Left/Right: Adjust width",
      10.0,
      50.0,
      Color::GRAY,
    );

    let samples = Self::sample_texts();
    let (title, text) = samples[self.sample_index];

    // Sample selector
    let selector_y = 80.0;
    ui.draw_text(
      &format!(
        "Sample: {} ({}/{})",
        title,
        self.sample_index + 1,
        samples.len()
      ),
      10.0,
      selector_y,
      Color::CYAN,
    );

    // Width indicator
    ui.draw_text(
      &format!("Wrap width: {}px", self.wrap_width as u32),
      10.0,
      selector_y + 20.0,
      Color::YELLOW,
    );

    // Wrapping demonstration area
    let demo_x = 50.0;
    let demo_y = 140.0;
    let max_width = self.wrap_width as u32;

    // Draw boundary box to show wrap width
    let (_, wrapped_h) = ui.wrapped_text_size(text, max_width);
    let boundary = Rect::new(
      demo_x - 5.0,
      demo_y - 5.0,
      self.wrap_width + 10.0,
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
      center_x - self.wrap_width / 2.0 - 5.0,
      center_demo_y - 5.0,
      self.wrap_width + 10.0,
      wrapped_h as f32 + 10.0,
    );

    ui.draw_text(
      "Centered:",
      center_x - self.wrap_width / 2.0,
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
      center_x - self.wrap_width / 2.0,
      center_demo_y,
      self.wrap_width,
      wrapped_h as f32,
    );
    ui.draw_text_wrapped_centered(text, text_rect, Color::WHITE);
  }
}
