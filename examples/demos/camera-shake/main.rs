//! Camera Shake Demo - Ryo2D Example
//!
//! This example demonstrates the trauma-based camera shake system:
//! - Trauma accumulation and decay
//! - Different intensity levels
//! - Configurable shake parameters
//! - Configurable intensity curves
//!
//! ## Controls
//!
//! - 1: Small shake (0.2 trauma) - light impacts
//! - 2: Medium shake (0.5 trauma) - hits, collisions
//! - 3: Large shake (0.8 trauma) - explosions
//! - Space: Maximum shake (1.0 trauma)
//! - Q/W/E: Intensity curve (Linear / InQuad / InCubic)
//! - R: Reset trauma to zero
//! - Up/Down: Adjust max offset (default: 10 pixels)
//! - Left/Right: Adjust decay rate (default: 1.0/sec)
//! - Escape: Exit

use ryo2d::prelude::*;

/// Marker for grid squares (to show shake effect)
#[derive(Debug)]
struct GridSquare;

/// Marker for the "player" entity
#[derive(Debug)]
struct Player;

/// Main demo scene
struct CameraShakeDemoScene {
  max_offset: f32,
  decay_rate: f32,
  intensity_curve: Ease,
}

impl CameraShakeDemoScene {
  fn new() -> Self {
    Self {
      max_offset: 10.0,
      decay_rate: 1.0,
      intensity_curve: Ease::InQuad,
    }
  }

  /// Get a human-readable name for the current curve
  fn curve_name(&self) -> &'static str {
    match self.intensity_curve {
      Ease::Linear => "Linear",
      Ease::InQuad => "InQuad (default)",
      Ease::InCubic => "InCubic",
      Ease::InQuart => "InQuart",
      Ease::OutQuad => "OutQuad",
      _ => "Custom",
    }
  }

  /// Create a colored square sprite
  fn create_square(size: u32, r: u8, g: u8, b: u8) -> SpriteHandle {
    let sprite = Sprite::solid_rect(size, size, r, g, b, 255);
    SpriteHandle::from_sprite(sprite, format!("square_{}_{}_{}_{}", size, r, g, b))
  }

  /// Spawn a grid of reference squares to visualize shake
  fn spawn_grid(&self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    let grid_size = 60.0;
    let square_size = 8;

    // Create a grid of small squares
    let cols = (screen_w / grid_size) as i32 + 1;
    let rows = (screen_h / grid_size) as i32 + 1;

    for row in 0..rows {
      for col in 0..cols {
        let x = col as f32 * grid_size + grid_size / 2.0;
        let y = row as f32 * grid_size + grid_size / 2.0;

        // Alternate colors for visual interest
        let (r, g, b) = if (row + col) % 2 == 0 {
          (80, 80, 100)
        } else {
          (60, 60, 80)
        };

        let sprite = Self::create_square(square_size, r, g, b);
        ctx.world.spawn((
          GridSquare,
          Transform::new(x, y).with_pivot_center(),
          Renderable::sprite(sprite).with_z_order(0),
        ));
      }
    }
  }

  /// Spawn a "player" entity in the center
  fn spawn_player(&self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    let sprite = Self::create_square(32, 100, 200, 255);

    ctx.world.spawn((
      Player,
      Transform::new(screen_w / 2.0, screen_h / 2.0).with_pivot_center(),
      Renderable::sprite(sprite).with_z_order(10),
    ));
  }
}

impl Scene for CameraShakeDemoScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Apply initial shake config
    ctx.camera.configure_shake(self.max_offset, self.decay_rate);
    ctx.camera.set_shake_curve(self.intensity_curve);

    // Spawn visual elements
    self.spawn_grid(ctx);
    self.spawn_player(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Exit on escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Trigger different shake intensities
    if ctx.input.key_pressed(KeyCode::Digit1) {
      ctx.camera.add_trauma(0.2);
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      ctx.camera.add_trauma(0.5);
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      ctx.camera.add_trauma(0.8);
    }
    if ctx.input.key_pressed(KeyCode::Space) {
      ctx.camera.add_trauma(1.0);
    }

    // Change intensity curve
    if ctx.input.key_pressed(KeyCode::KeyQ) {
      self.intensity_curve = Ease::Linear;
      ctx.camera.set_shake_curve(self.intensity_curve);
    }
    if ctx.input.key_pressed(KeyCode::KeyW) {
      self.intensity_curve = Ease::InQuad;
      ctx.camera.set_shake_curve(self.intensity_curve);
    }
    if ctx.input.key_pressed(KeyCode::KeyE) {
      self.intensity_curve = Ease::InCubic;
      ctx.camera.set_shake_curve(self.intensity_curve);
    }

    // Reset trauma
    if ctx.input.key_pressed(KeyCode::KeyR) {
      // Set trauma to 0 by configuring with same params (clears internal state)
      ctx.camera.configure_shake(self.max_offset, self.decay_rate);
    }

    // Adjust max offset
    if ctx.input.key_pressed(KeyCode::ArrowUp) {
      self.max_offset = (self.max_offset + 2.0).min(50.0);
      ctx.camera.configure_shake(self.max_offset, self.decay_rate);
    }
    if ctx.input.key_pressed(KeyCode::ArrowDown) {
      self.max_offset = (self.max_offset - 2.0).max(2.0);
      ctx.camera.configure_shake(self.max_offset, self.decay_rate);
    }

    // Adjust decay rate
    if ctx.input.key_pressed(KeyCode::ArrowRight) {
      self.decay_rate = (self.decay_rate + 0.2).min(5.0);
      ctx.camera.configure_shake(self.max_offset, self.decay_rate);
    }
    if ctx.input.key_pressed(KeyCode::ArrowLeft) {
      self.decay_rate = (self.decay_rate - 0.2).max(0.2);
      ctx.camera.configure_shake(self.max_offset, self.decay_rate);
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let text_light = Color::rgb(220, 220, 220);
    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);
    let text_warning = Color::rgb(255, 200, 100);

    // Title
    ui.draw_text("Camera Shake Demo", 10.0, 10.0, text_light);

    // Current config
    let config_text = format!(
      "Max Offset: {:.0}px | Decay: {:.1}/sec | Curve: {}",
      self.max_offset,
      self.decay_rate,
      self.curve_name()
    );
    ui.draw_text(&config_text, 10.0, 30.0, text_accent);

    // Controls
    ui.draw_text(
      "1-3: Shake levels | Space: Max | R: Reset",
      10.0,
      50.0,
      text_muted,
    );
    ui.draw_text(
      "Q/W/E: Curve | Arrows: Config | ESC: Exit",
      10.0,
      70.0,
      text_muted,
    );

    // Visual indicator area
    let (screen_w, _) = ui.screen_size();
    ui.draw_text(
      "Press keys to trigger shake!",
      screen_w as f32 / 2.0 - 80.0,
      100.0,
      text_warning,
    );
    SceneAction::Continue
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Camera Shake Demo - Ryo2D")
    .resolution(800, 600)
    .window_mode(WindowMode::ResizableAspectRatio)
    .target_fps(60)
    .physics_hz(60)
    .with_scene(CameraShakeDemoScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
