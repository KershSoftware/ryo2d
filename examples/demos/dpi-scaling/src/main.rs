//! DPI Scaling Demo - Demonstrates HiDPI/scale awareness in Ryo2D
//!
//! This example shows how the engine handles high-DPI displays and allows
//! you to compare different UI scale factors in real-time.
//!
//! Controls:
//! - 1: Set UI scale to 1x (normal)
//! - 2: Set UI scale to 2x (HiDPI)
//! - 3: Set UI scale to 3x (very large)
//! - A: Toggle auto-detect mode
//! - Escape: Quit

use ryo2d::prelude::*;

/// Demo scene showing DPI scaling features
struct DpiScalingScene {
  /// Current UI scale setting (None = auto)
  ui_scale: Option<u32>,
  /// Cached scale factor from context
  scale_factor: f64,
}

impl DpiScalingScene {
  fn new() -> Self {
    Self {
      ui_scale: None, // Start with auto-detect
      scale_factor: 1.0,
    }
  }
}

impl Scene for DpiScalingScene {
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Cache scale factor for display
    self.scale_factor = ctx.scale_factor();

    // Handle scale switching with number keys
    if ctx.input.key_pressed(KeyCode::Digit1) {
      self.ui_scale = Some(1);
      ctx.request_ui_scale(Some(1));
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      self.ui_scale = Some(2);
      ctx.request_ui_scale(Some(2));
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      self.ui_scale = Some(3);
      ctx.request_ui_scale(Some(3));
    }
    if ctx.input.key_pressed(KeyCode::KeyA) {
      self.ui_scale = None;
      ctx.request_ui_scale(None);
    }

    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let (width, height) = ui.screen_size();
    let width = width as f32;
    let height = height as f32;
    let current_scale = ui.ui_scale();

    // Dark background
    ui.draw_rect(Rect::new(0.0, 0.0, width, height), Color::rgb(20, 25, 35));

    let line_height = (8 * current_scale) as f32 + 4.0;
    let padding = 15.0;
    let margin = 20.0;
    let panel_w = width - margin * 2.0;
    let mut y = margin;

    // Helper: panel height for N lines of text
    let panel_h = |lines: u32| -> f32 { padding * 2.0 + line_height * lines as f32 };

    // Title panel
    let title_h = panel_h(1);
    let title_rect = Rect::new(margin, y, panel_w, title_h);
    ui.draw_rect(title_rect, Color::rgb(40, 50, 70));
    ui.draw_rect_outline(title_rect, Color::rgb(80, 100, 140), 2.0);
    ui.draw_text_centered("DPI Scaling Demo", title_rect, Color::WHITE);
    y += title_h + 10.0;

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
        if self.ui_scale.is_none() {
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

    // Controls panel (3 lines)
    let controls_h = panel_h(3);
    let controls_rect = Rect::new(margin, y, panel_w, controls_h);
    ui.draw_rect(controls_rect, Color::rgb(30, 35, 45));
    ui.draw_rect_outline(controls_rect, Color::rgb(60, 70, 90), 1.0);

    ui.draw_text("Controls:", margin + 20.0, y + padding, Color::YELLOW);
    ui.draw_text(
      "[1] 1x Scale  [2] 2x Scale  [3] 3x Scale",
      margin + 20.0,
      y + padding + line_height,
      Color::LIGHT_GRAY,
    );
    ui.draw_text(
      "[A] Auto-detect  [Esc] Quit",
      margin + 20.0,
      y + padding + line_height * 2.0,
      Color::LIGHT_GRAY,
    );
    y += controls_h + 10.0;

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
    let mut bx = 40.0;

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

    // Footer with hint
    ui.draw_text(
      "Note: Text scales with ui_scale,",
      margin + 20.0,
      height - 30.0 - line_height,
      Color::GRAY,
    );
    ui.draw_text(
      "shapes remain at game resolution",
      margin + 20.0,
      height - 30.0,
      Color::GRAY,
    );
    SceneAction::Continue
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Ryo2D DPI Scaling Demo")
    .resolution(800, 600)
    .window_mode(WindowMode::ResizableAspectRatio)
    .ui_scale(None) // Auto-detect from DPI
    .start_scene(DpiScalingScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
