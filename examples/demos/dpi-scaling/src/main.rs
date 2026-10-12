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

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let (width, height) = ui.screen_size();
    let current_scale = ui.ui_scale();
    let area = content_area(ui);

    // Dark background
    ui.draw_rect(
      Rect::new(0.0, 0.0, width as f32, height as f32),
      Color::rgb(20, 25, 35),
    );

    let line_height = ui.text_size("Ag").1 as f32 + 4.0;
    let char_w = ui.text_size("0").0 as f32;
    let padding = 15.0;
    let margin = area.x;
    let bottom = area.y + area.height;
    // Beside the settings panel at 1x; below it, full width, when larger
    let beside = current_scale == 1;
    let panel_w = if beside {
      area.width * 0.68
    } else {
      area.width
    };
    let mut y = if beside {
      area.y
    } else {
      area.y + settings_panel_height(ui, 1) + 8.0
    };

    // Helper: panel height for N lines of text
    let panel_h = |lines: usize| -> f32 { padding * 2.0 + line_height * lines as f32 };
    let text_x = margin + 20.0;

    // Info panel
    let info = [
      (
        format!("Window DPI Scale Factor: {:.2}", self.scale_factor),
        Color::rgb(150, 200, 255),
      ),
      (
        format!("Current UI Scale: {}x", current_scale),
        Color::rgb(150, 255, 150),
      ),
      (
        format!(
          "Mode: {}",
          if self.scale_choice == AUTO {
            "Auto-detect"
          } else {
            "Manual"
          }
        ),
        Color::rgb(255, 200, 150),
      ),
    ];
    let info_rect = Rect::new(margin, y, panel_w, panel_h(info.len()));
    ui.draw_rect(info_rect, Color::rgb(30, 35, 45));
    ui.draw_rect_outline(info_rect, Color::rgb(60, 70, 90), 1.0);
    for (i, (text, color)) in info.iter().enumerate() {
      ui.draw_text(text, text_x, y + padding + line_height * i as f32, *color);
    }
    y += info_rect.height + 10.0;

    // Sample text panel: as many lines as fit above the scale comparison
    let comparison_h = line_height + 10.0 + 64.0;
    let sample = [
      ("Sample Text:", Color::YELLOW),
      ("ABCDEFGHIJKLMNOPQRSTUVWXYZ", Color::WHITE),
      ("abcdefghijklmnopqrstuvwxyz", Color::WHITE),
      ("0123456789 !@#$%^&*()_+-=", Color::WHITE),
      ("The quick brown fox jumps.", Color::rgb(200, 200, 200)),
    ];
    let room = bottom - y - 10.0 - comparison_h - padding * 2.0;
    let fit = ((room / line_height).floor().max(0.0) as usize).clamp(2, sample.len());
    let sample_rect = Rect::new(margin, y, panel_w, panel_h(fit));
    ui.draw_rect(sample_rect, Color::rgb(30, 35, 45));
    ui.draw_rect_outline(sample_rect, Color::rgb(60, 70, 90), 1.0);
    for (i, (text, color)) in sample.iter().take(fit).enumerate() {
      ui.draw_text(text, text_x, y + padding + line_height * i as f32, *color);
    }
    y += sample_rect.height + 10.0;

    // Visual scale comparison - fixed size boxes with labels, if it fits
    let box_y = y + line_height + 10.0;
    if box_y + 64.0 > bottom {
      return;
    }
    ui.draw_text("Scale Comparison:", text_x, y, Color::YELLOW);
    let label_px = 4.0 * char_w; // "16px" and the others are 4 characters
    let gap = 4.0; // box-to-label gap
    let spacing = 16.0; // label-to-next-box gap
    let mut bx = text_x;
    for (size, color) in [
      (16.0, Color::rgb(255, 100, 100)),
      (32.0, Color::rgb(100, 255, 100)),
      (64.0, Color::rgb(100, 100, 255)),
    ] {
      ui.draw_rect(Rect::new(bx, box_y, size, size), color);
      ui.draw_text(
        &format!("{size}px"),
        bx + size + gap,
        box_y + size / 2.0 - 4.0,
        Color::LIGHT_GRAY,
      );
      bx += size + gap + label_px + spacing;
    }

    // Footer with hint, unless the panels above already reach it
    let footer_y = bottom - line_height * 2.0;
    if footer_y > box_y + 64.0 + 4.0 {
      ui.draw_text(
        "Note: Text scales with ui_scale,",
        text_x,
        footer_y,
        Color::GRAY,
      );
      ui.draw_text(
        "shapes remain at game resolution",
        text_x,
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
