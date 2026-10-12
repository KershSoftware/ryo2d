//! Camera Shake Demo - Ryo2D Example
//!
//! This example demonstrates the trauma-based camera shake system:
//! - Trauma accumulation and decay
//! - Different intensity levels
//! - Configurable shake parameters
//! - Configurable intensity curves
//!
//! Runs on the demo-kit shell: the curve, max offset and decay are settings,
//! trauma is added with the number keys and Space.

use demo_kit::prelude::*;

/// Marker for grid squares (to show shake effect)
#[derive(Component, Debug)]
struct GridSquare;

/// Marker for the "player" entity
#[derive(Component, Debug)]
struct Player;

/// Intensity curves offered by the Curve setting
const CURVES: [(&str, Ease); 3] = [
  ("Linear", Ease::Linear),
  ("InQuad", Ease::InQuad),
  ("InCubic", Ease::InCubic),
];

struct CameraShakePage {
  max_offset: f32,
  decay_rate: f32,
  curve: usize,
  // What was last sent to the camera, to reconfigure only on change
  applied: (f32, f32, usize),
}

impl CameraShakePage {
  fn new() -> Self {
    Self {
      max_offset: 10.0,
      decay_rate: 1.0,
      curve: 1,
      applied: (10.0, 1.0, 1),
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
          Transform::from_xy(x, y),
          Renderable::sprite(sprite).with_z_order(0),
        ));
      }
    }
  }

  /// Send the current settings to the camera
  fn apply_config(&mut self, ctx: &mut SceneContext) {
    ctx.camera.configure_shake(self.max_offset, self.decay_rate);
    ctx
      .camera
      .set_shake_curve(CURVES[self.curve.min(CURVES.len() - 1)].1);
    self.applied = (self.max_offset, self.decay_rate, self.curve);
  }

  /// Spawn a "player" entity in the center
  fn spawn_player(&self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    let sprite = Self::create_square(32, 100, 200, 255);

    ctx.world.spawn((
      Player,
      Transform::from_xy(screen_w / 2.0, screen_h / 2.0),
      Renderable::sprite(sprite).with_z_order(10),
    ));
  }
}

impl DemoPage for CameraShakePage {
  fn name(&self) -> &str {
    "Trauma"
  }

  fn description(&self) -> &str {
    "Trauma-based camera shake: shake grows with trauma^curve and decays over time."
  }

  fn settings(&mut self, s: &mut Settings) {
    let names: Vec<&str> = CURVES.iter().map(|(name, _)| *name).collect();
    s.choice("Curve", &mut self.curve, &names);
    s.float("Max offset", &mut self.max_offset, 2.0..=50.0, 2.0);
    s.float("Decay", &mut self.decay_rate, 0.2..=5.0, 0.2);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Digit1, "Small shake (0.2)"),
      Control::key(KeyCode::Digit2, "Medium shake (0.5)"),
      Control::key(KeyCode::Digit3, "Large shake (0.8)"),
      Control::key(KeyCode::Space, "Maximum shake (1.0)"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.apply_config(ctx);
    self.spawn_grid(ctx);
    self.spawn_player(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if (self.max_offset, self.decay_rate, self.curve) != self.applied {
      self.apply_config(ctx);
    }

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
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.draw_text(
      "Press keys to trigger shake!",
      area.x + 8.0,
      area.y + 8.0,
      Color::rgb(255, 200, 100),
    );
  }
}

fn main() {
  Demo::new("Camera Shake")
    .resolution(800, 600)
    .configure(|b| b.target_fps(60).fixed_hz(60))
    .page(CameraShakePage::new)
    .run();
}
