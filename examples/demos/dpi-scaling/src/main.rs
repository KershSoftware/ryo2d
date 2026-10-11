//! DPI Scaling Demo - Demonstrates HiDPI/scale awareness in Ryo2D
//!
//! This example shows how the engine handles high-DPI displays and allows
//! you to compare different UI scale factors in real-time.
//!
//! The UI scale setting picks 1x, 2x, 3x or Auto (detected from the DPI).
//! Runs on the demo-kit shell, whose own text scales with it.

use demo_kit::prelude::*;

/// Settings choices; the last is Auto
const SCALE_OPTIONS: [&str; 4] = ["1x", "2x", "3x", "Auto"];
const AUTO: usize = 3;

/// Page showing DPI scaling features
struct DpiScalingPage {
  /// Index into `SCALE_OPTIONS`
  scale_choice: usize,
  /// The choice last sent to the engine
  applied: Option<usize>,
  /// Cached scale factor from context
  scale_factor: f64,
}

impl DpiScalingPage {
  fn new() -> Self {
    Self {
      scale_choice: AUTO, // Start with auto-detect
      applied: None,
      scale_factor: 1.0,
    }
  }

  /// Ask the engine for the chosen UI scale when it differs from the last request
  fn apply_scale(&mut self, ctx: &mut SceneContext) {
    if self.applied == Some(self.scale_choice) {
      return;
    }
    let scale = match self.scale_choice {
      AUTO => None,
      choice => Some(choice as u32 + 1),
    };
    ctx.request_ui_scale(scale);
    self.applied = Some(self.scale_choice);
  }
}

impl DemoPage for DpiScalingPage {
  fn name(&self) -> &str {
    "UI Scale"
  }

  fn description(&self) -> &str {
    "Compare UI scale factors: text scales, shapes stay at game resolution"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("UI scale", &mut self.scale_choice, &SCALE_OPTIONS);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // The UI scale lives in the engine, so set it from this page's setting
    self.apply_scale(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    // Cache scale factor for display
    self.scale_factor = ctx.scale_factor();
    self.apply_scale(ctx);
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (width, height) = ui.screen_size();
    let width = width as f32;
    let height = height as f32;
    let current_scale = ui.ui_scale();
    let area = content_area(ui);

    // Dark background
    ui.draw_rect(Rect::new(0.0, 0.0, width, height), Color::rgb(20, 25, 35));

    let line_height = (8 * current_scale) as f32 + 4.0;
    let padding = 15.0;
    let margin = area.x;
    // The settings panel scales with the UI: beside the panels at 1x, above them when larger
    let panel_w = if current_scale == 1 {
      area.width * 0.68
    } else {
      area.width
    };
    let mut y = area.y + 60.0 * (current_scale as f32 - 1.0);

    // Helper: panel height for N lines of text
    let panel_h = |lines: u32| -> f32 { padding * 2.0 + line_height * lines as f32 };

    // Info panel (3 lines)
    let info_h = panel_h(3);
    let info_rect = Rect::new(margin, y, panel_w, info_h);
    ui.draw_rect(info_rect, Color::rgb(30, 35, 45));
    ui.draw_rect_outline(info_rect, Color::rgb(60, 70, 90), 1.0);

    ui.draw_text(
      &format!("Window DPI Scale Factor: {:.2}", self.scale_factor),
      margin + 20.0,
      y + padding,
      Color::rgb(150, 200, 255),
    );
    ui.draw_text(
      &format!("Current UI Scale: {}x", current_scale),
      margin + 20.0,
      y + padding + line_height,
      Color::rgb(150, 255, 150),
    );
    ui.draw_text(
      &format!(
        "Mode: {}",
        if self.scale_choice == AUTO {
          "Auto-detect"
        } else {
          "Manual"
        }
      ),
      margin + 20.0,
      y + padding + line_height * 2.0,
      Color::rgb(255, 200, 150),
    );
    y += info_h + 10.0;

    // Sample text panel (5 lines)
    let sample_h = panel_h(5);
    let sample_rect = Rect::new(margin, y, panel_w, sample_h);
    ui.draw_rect(sample_rect, Color::rgb(30, 35, 45));
    ui.draw_rect_outline(sample_rect, Color::rgb(60, 70, 90), 1.0);

    ui.draw_text("Sample Text:", margin + 20.0, y + padding, Color::YELLOW);
    ui.draw_text(
      "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
      margin + 20.0,
      y + padding + line_height,
      Color::WHITE,
    );
    ui.draw_text(
      "abcdefghijklmnopqrstuvwxyz",
      margin + 20.0,
      y + padding + line_height * 2.0,
      Color::WHITE,
    );
    ui.draw_text(
      "0123456789 !@#$%^&*()_+-=",
      margin + 20.0,
      y + padding + line_height * 3.0,
      Color::WHITE,
    );
    ui.draw_text(
      "The quick brown fox jumps.",
      margin + 20.0,
      y + padding + line_height * 4.0,
      Color::rgb(200, 200, 200),
    );
    y += sample_h + 10.0;

    // Visual scale comparison - fixed size boxes with labels
    ui.draw_text("Scale Comparison:", margin + 20.0, y, Color::YELLOW);
    let box_y = y + line_height + 10.0;

    // "16px" etc. are 4 chars; bitmap font is 8px/char at base scale
    let char_w = 8.0 * current_scale as f32;
    let label_px = 4.0 * char_w;
    let gap = 4.0; // box-to-label gap
    let spacing = 16.0; // label-to-next-box gap
    let mut bx = margin + 20.0;

    // Small reference box (always 16x16 visual pixels)
    ui.draw_rect(Rect::new(bx, box_y, 16.0, 16.0), Color::rgb(255, 100, 100));
    ui.draw_text("16px", bx + 16.0 + gap, box_y + 4.0, Color::LIGHT_GRAY);
    bx += 16.0 + gap + label_px + spacing;

    // Medium reference box (always 32x32 visual pixels)
    ui.draw_rect(Rect::new(bx, box_y, 32.0, 32.0), Color::rgb(100, 255, 100));
    ui.draw_text("32px", bx + 32.0 + gap, box_y + 12.0, Color::LIGHT_GRAY);
    bx += 32.0 + gap + label_px + spacing;

    // Large reference box (always 64x64 visual pixels)
    ui.draw_rect(Rect::new(bx, box_y, 64.0, 64.0), Color::rgb(100, 100, 255));
    ui.draw_text("64px", bx + 64.0 + gap, box_y + 28.0, Color::LIGHT_GRAY);

    // Footer with hint, unless the scaled panels above already reach it
    let footer_y = area.y + area.height - line_height * 2.0;
    if footer_y > box_y + 64.0 + 4.0 {
      ui.draw_text(
        "Note: Text scales with ui_scale,",
        margin + 20.0,
        footer_y,
        Color::GRAY,
      );
      ui.draw_text(
        "shapes remain at game resolution",
        margin + 20.0,
        footer_y + line_height,
        Color::GRAY,
      );
    }
  }
}

fn main() {
  Demo::new("DPI Scaling")
    .resolution(800, 600)
    .configure(|b| {
      b.window_mode(WindowMode::ResizableAspectRatio)
        .ui_scale(None) // Auto-detect from DPI
    })
    .page(DpiScalingPage::new)
    .run();
}
