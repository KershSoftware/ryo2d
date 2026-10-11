//! Time Control Demo - Ryo2D Example
//!
//! This example demonstrates time scale and hitstop effects:
//! - Time scale: slow-mo and fast-forward
//! - Hitstop: freeze frame for impact emphasis
//! - Shows how animations/tweens respond to time changes
//!
//! Runs on the demo-kit shell: the time scale is a setting, hitstops are keys.

use demo_kit::prelude::*;
use std::f32::consts::PI;

/// Marker for bouncing ball entities
#[derive(Debug, Clone)]
struct BouncingBall {
  speed_x: f32,
  speed_y: f32,
}

/// Marker for rotating square
#[derive(Debug)]
struct RotatingSquare;

/// Marker for pulsing circle
#[derive(Debug)]
struct PulsingCircle;

/// Time scale choices, as labels and values
const TIME_SCALE_LABELS: [&str; 5] = ["1.0x", "0.3x", "0.1x", "2.0x", "Paused"];
const TIME_SCALES: [f32; 5] = [1.0, 0.3, 0.1, 2.0, 0.0];

/// Main demo page
struct TimeControlPage {
  time_scale: usize,
  balls: Vec<Entity>,
  rotating_square: Option<Entity>,
  pulsing_circle: Option<Entity>,
  screen_w: f32,
  screen_h: f32,
}

impl TimeControlPage {
  fn new() -> Self {
    Self {
      time_scale: 0,
      balls: Vec::new(),
      rotating_square: None,
      pulsing_circle: None,
      screen_w: 0.0,
      screen_h: 0.0,
    }
  }

  /// Create a colored circle sprite
  fn create_circle(radius: u32, r: u8, g: u8, b: u8) -> SpriteHandle {
    let size = radius * 2;
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let center = radius as f32;

    for y in 0..size {
      for x in 0..size {
        let dx = x as f32 - center + 0.5;
        let dy = y as f32 - center + 0.5;
        let dist = (dx * dx + dy * dy).sqrt();

        let idx = ((y * size + x) * 4) as usize;
        if dist <= center {
          pixels[idx] = r;
          pixels[idx + 1] = g;
          pixels[idx + 2] = b;
          pixels[idx + 3] = 255;
        }
      }
    }

    let sprite = Sprite::from_rgba(pixels, size, size);
    SpriteHandle::from_sprite(sprite, format!("circle_{}_{}_{}", r, g, b))
  }

  /// Create a colored square sprite
  fn create_square(size: u32, r: u8, g: u8, b: u8) -> SpriteHandle {
    let sprite = Sprite::solid_rect(size, size, r, g, b, 255);
    SpriteHandle::from_sprite(sprite, format!("square_{}_{}_{}", r, g, b))
  }

  /// Spawn bouncing balls
  fn spawn_balls(&mut self, ctx: &mut SceneContext) {
    let colors = [
      (255, 100, 100), // Red
      (100, 255, 100), // Green
      (100, 100, 255), // Blue
      (255, 255, 100), // Yellow
      (255, 100, 255), // Magenta
      (100, 255, 255), // Cyan
    ];

    for (i, (r, g, b)) in colors.iter().enumerate() {
      let sprite = Self::create_circle(12, *r, *g, *b);

      // Spread balls across the screen with different velocities
      let x = 100.0 + (i as f32 * 100.0);
      let y = 150.0 + (i as f32 * 30.0);

      // Varied speeds and directions
      let angle = (i as f32 * 0.5) + 0.3;
      let speed = 150.0 + (i as f32 * 20.0);
      let speed_x = angle.cos() * speed;
      let speed_y = angle.sin() * speed;

      let entity = ctx.world.spawn((
        BouncingBall { speed_x, speed_y },
        Transform::new(x, y).with_pivot_center(),
        Renderable::sprite(sprite).with_z_order(10),
      ));

      self.balls.push(entity);
    }
  }

  /// Spawn a rotating square (uses Tween)
  fn spawn_rotating_square(&mut self, ctx: &mut SceneContext) {
    let sprite = Self::create_square(40, 255, 200, 100);

    self.rotating_square = Some(
      ctx.world.spawn((
        RotatingSquare,
        Transform::new(150.0, self.screen_h - 100.0).with_pivot_center(),
        Renderable::sprite(sprite).with_z_order(5),
        Tween::to_rotation(PI * 2.0, 2.0)
          .with_easing(Ease::Linear)
          .with_loop(TweenLoop::Loop),
      )),
    );
  }

  /// Spawn a pulsing circle (uses Tween for scale)
  fn spawn_pulsing_circle(&mut self, ctx: &mut SceneContext) {
    let sprite = Self::create_circle(20, 100, 200, 255);

    self.pulsing_circle = Some(
      ctx.world.spawn((
        PulsingCircle,
        Transform::new(self.screen_w - 150.0, self.screen_h - 100.0).with_pivot_center(),
        Renderable::sprite(sprite).with_z_order(5),
        Tween::to_scale(1.5, 1.5, 0.5)
          .with_easing(Ease::InOutSine)
          .with_loop(TweenLoop::PingPong),
      )),
    );
  }

  /// Update bouncing balls manually (affected by dt)
  fn update_balls(&self, ctx: &mut SceneContext, dt: f32) {
    let margin = 12.0; // Ball radius

    for &entity in &self.balls {
      // Get current state
      let (x, y, mut speed_x, mut speed_y) = {
        let transform = ctx.world.get::<Transform>(entity);
        let ball = ctx.world.get::<BouncingBall>(entity);
        match (transform, ball) {
          (Some(t), Some(b)) => (t.x, t.y, b.speed_x, b.speed_y),
          _ => continue,
        }
      };

      // Calculate new position
      let mut new_x = x + speed_x * dt;
      let mut new_y = y + speed_y * dt;

      // Bounce off walls
      if new_x < margin {
        new_x = margin;
        speed_x = -speed_x;
      } else if new_x > self.screen_w - margin {
        new_x = self.screen_w - margin;
        speed_x = -speed_x;
      }

      if new_y < margin {
        new_y = margin;
        speed_y = -speed_y;
      } else if new_y > self.screen_h - margin {
        new_y = self.screen_h - margin;
        speed_y = -speed_y;
      }

      // Update components
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.x = new_x;
        transform.y = new_y;
      }
      if let Some(mut ball) = ctx.world.get_mut::<BouncingBall>(entity) {
        ball.speed_x = speed_x;
        ball.speed_y = speed_y;
      }
    }
  }
}

impl DemoPage for TimeControlPage {
  fn name(&self) -> &str {
    "Time"
  }

  fn description(&self) -> &str {
    "Watch balls & tweens respond to time!\n\
     Time scale gives slow-mo and fast-forward; hitstop freezes briefly for impact"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Time scale", &mut self.time_scale, &TIME_SCALE_LABELS);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Space, "100 ms hitstop"),
      Control::key(KeyCode::KeyJ, "50 ms hitstop"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    self.screen_w = screen_w;
    self.screen_h = screen_h;

    self.spawn_balls(ctx);
    self.spawn_rotating_square(ctx);
    self.spawn_pulsing_circle(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let scale = TIME_SCALES[self.time_scale.min(TIME_SCALES.len() - 1)];
    ctx.set_time_scale(scale);

    // Hitstop controls
    if ctx.input.key_pressed(KeyCode::Space) {
      ctx.hitstop(0.1);
    }
    if ctx.input.key_pressed(KeyCode::KeyJ) {
      ctx.hitstop(0.05);
    }

    // Update bouncing balls with the scaled dt
    // Note: dt is already scaled by time_scale and will be 0 during hitstop
    self.update_balls(ctx, dt);
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let text_accent = Color::rgb(130, 180, 255);

    // Bottom labels, above the shell's legend
    let area = content_area(ui);
    let bottom_y = area.y + area.height - 20.0;

    ui.draw_text("Rotating (Tween)", 100.0, bottom_y, text_accent);
    ui.draw_text("Pulsing (Tween)", 580.0, bottom_y, text_accent);
  }
}

fn main() {
  Demo::new("Time Control")
    .resolution(800, 600)
    .configure(|b| {
      b.window_mode(WindowMode::ResizableAspectRatio)
        .target_fps(60)
        .physics_hz(60)
    })
    .page(TimeControlPage::new)
    .run();
}
