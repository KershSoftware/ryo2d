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
      println!("UI Scale: 1x (forced)");
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      self.ui_scale = Some(2);
      ctx.request_ui_scale(Some(2));
      println!("UI Scale: 2x (forced)");
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      self.ui_scale = Some(3);
      ctx.request_ui_scale(Some(3));
      println!("UI Scale: 3x (forced)");
    }
    if ctx.input.key_pressed(KeyCode::KeyA) {
      self.ui_scale = None;
      ctx.request_ui_scale(None);
      println!("UI Scale: Auto-detect");
    }

    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let (width, height) = ui.screen_size();
    let current_scale = ui.ui_scale();

    // Dark background
    ui.draw_rect(
      Rect::new(0.0, 0.0, width as f32, height as f32),
      Color::rgb(20, 25, 35),
    );

    // Title panel
    let title_rect = Rect::new(20.0, 20.0, width as f32 - 40.0, 60.0);
    ui.draw_rect(title_rect, Color::rgb(40, 50, 70));
    ui.draw_rect_outline(title_rect, Color::rgb(80, 100, 140), 2.0);
    ui.draw_text_centered("DPI Scaling Demo", title_rect, Color::WHITE);

    // Info panel
    let info_y = 100.0;
    let info_rect = Rect::new(20.0, info_y, width as f32 - 40.0, 120.0);
    ui.draw_rect(info_rect, Color::rgb(30, 35, 45));
    ui.draw_rect_outline(info_rect, Color::rgb(60, 70, 90), 1.0);

    let line_height = (8 * current_scale) as f32 + 4.0;
    ui.draw_text(
      &format!("Window DPI Scale Factor: {:.2}", self.scale_factor),
      40.0,
      info_y + 15.0,
      Color::rgb(150, 200, 255),
    );
    ui.draw_text(
      &format!("Current UI Scale: {}x", current_scale),
      40.0,
      info_y + 15.0 + line_height,
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
      40.0,
      info_y + 15.0 + line_height * 2.0,
      Color::rgb(255, 200, 150),
    );

    // Controls panel
    let controls_y = 240.0;
    let controls_rect = Rect::new(20.0, controls_y, width as f32 - 40.0, 100.0);
    ui.draw_rect(controls_rect, Color::rgb(30, 35, 45));
    ui.draw_rect_outline(controls_rect, Color::rgb(60, 70, 90), 1.0);

    ui.draw_text("Controls:", 40.0, controls_y + 10.0, Color::YELLOW);
    ui.draw_text(
      "[1] 1x Scale  [2] 2x Scale  [3] 3x Scale",
      40.0,
      controls_y + 10.0 + line_height,
      Color::LIGHT_GRAY,
    );
    ui.draw_text(
      "[A] Auto-detect  [Esc] Quit",
      40.0,
      controls_y + 10.0 + line_height * 2.0,
      Color::LIGHT_GRAY,
    );

    // Sample text panel - shows how text looks at current scale
    let sample_y = 360.0;
    let sample_rect = Rect::new(20.0, sample_y, width as f32 - 40.0, 140.0);
    ui.draw_rect(sample_rect, Color::rgb(30, 35, 45));
    ui.draw_rect_outline(sample_rect, Color::rgb(60, 70, 90), 1.0);

    ui.draw_text("Sample Text:", 40.0, sample_y + 10.0, Color::YELLOW);
    ui.draw_text(
      "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
      40.0,
      sample_y + 10.0 + line_height,
      Color::WHITE,
    );
    ui.draw_text(
      "abcdefghijklmnopqrstuvwxyz",
      40.0,
      sample_y + 10.0 + line_height * 2.0,
      Color::WHITE,
    );
    ui.draw_text(
      "0123456789 !@#$%^&*()_+-=",
      40.0,
      sample_y + 10.0 + line_height * 3.0,
      Color::WHITE,
    );
    ui.draw_text(
      "The quick brown fox jumps.",
      40.0,
      sample_y + 10.0 + line_height * 4.0,
      Color::rgb(200, 200, 200),
    );

    // Visual scale comparison - fixed size boxes with labels
    let compare_y = 520.0;
    ui.draw_text("Scale Comparison:", 40.0, compare_y, Color::YELLOW);

    // Draw three boxes at different visual sizes to show scaling effect
    let box_y = compare_y + line_height + 10.0;

    // Small reference box (always 16x16 visual pixels)
    ui.draw_rect(
      Rect::new(40.0, box_y, 16.0, 16.0),
      Color::rgb(255, 100, 100),
    );
    ui.draw_text("16px", 60.0, box_y + 4.0, Color::LIGHT_GRAY);

    // Medium reference box (always 32x32 visual pixels)
    ui.draw_rect(
      Rect::new(120.0, box_y, 32.0, 32.0),
      Color::rgb(100, 255, 100),
    );
    ui.draw_text("32px", 156.0, box_y + 12.0, Color::LIGHT_GRAY);

    // Large reference box (always 64x64 visual pixels)
    ui.draw_rect(
      Rect::new(220.0, box_y, 64.0, 64.0),
      Color::rgb(100, 100, 255),
    );
    ui.draw_text("64px", 290.0, box_y + 28.0, Color::LIGHT_GRAY);

    // Footer with hint
    let footer_y = height as f32 - 30.0;
    ui.draw_text(
      "Note: Text scales with ui_scale, shapes remain at game resolution",
      40.0,
      footer_y,
      Color::GRAY,
    );
    SceneAction::Continue
  }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  EngineBuilder::new()
    .title("Ryo2D DPI Scaling Demo")
    .resolution(800, 600)
    .window_mode(WindowMode::ResizableAspectRatio)
    .ui_scale(None) // Auto-detect from DPI
    .with_scene(DpiScalingScene::new())
    .build()
    .run()?;

  Ok(())
}
