//! Time Control Demo - Ryo2D Example
//!
//! This example demonstrates the game clock:
//! - Speed: slow-mo and fast-forward (`ctx.time.game.set_speed`)
//! - Pause and resume (`ctx.time.game.pause` / `resume`)
//! - Hitstop: a freeze frame for impact emphasis (`ctx.time.game.hitstop`)
//!
//! Physics, tweens and the page's `update` all run on the fixed clock, which
//! is fed by game time: slow-mo runs fewer fixed steps, pause and hitstop run
//! none. The time controls themselves live in the per-frame UI pass so they
//! keep working while game time is slowed or stopped.
//!
//! Runs on the demo-kit shell: the time scale is a setting, hitstops are keys.

use demo_kit::prelude::*;
use std::f32::consts::PI;

const BALL_RADIUS: u32 = 12;
const WALL_THICKNESS: f32 = 40.0;

/// Time scale choices, as labels and values (`None` pauses)
const TIME_SCALE_LABELS: [&str; 5] = ["1.0x", "0.3x", "0.1x", "2.0x", "Paused"];
const TIME_SCALES: [Option<f32>; 5] = [Some(1.0), Some(0.3), Some(0.1), Some(2.0), None];

/// Main demo page
struct TimeControlPage {
  time_scale: usize,
  screen_w: f32,
  screen_h: f32,
}

impl TimeControlPage {
  fn new() -> Self {
    Self {
      time_scale: 0,
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

  /// Invisible walls just outside the screen edges keep the balls in view
  fn spawn_walls(&self, ctx: &mut SceneContext) {
    let (w, h, t) = (self.screen_w, self.screen_h, WALL_THICKNESS);
    let walls = [
      (w / 2.0, -t / 2.0, w + 2.0 * t, t),    // top
      (w / 2.0, h + t / 2.0, w + 2.0 * t, t), // bottom
      (-t / 2.0, h / 2.0, t, h),              // left
      (w + t / 2.0, h / 2.0, t, h),           // right
    ];
    for (x, y, width, height) in walls {
      ctx.world.spawn((
        Transform::from_xy(x, y),
        RigidBody::fixed(),
        Collider::box_collider(width, height)
          .with_restitution(1.0)
          .with_friction(0.0),
      ));
    }
  }

  /// Spawn bouncing balls: Rapier bodies under gravity, bouncing off the walls
  /// and each other
  fn spawn_balls(&self, ctx: &mut SceneContext) {
    let colors = [
      (255, 100, 100), // Red
      (100, 255, 100), // Green
      (100, 100, 255), // Blue
      (255, 255, 100), // Yellow
      (255, 100, 255), // Magenta
      (100, 255, 255), // Cyan
    ];

    for (i, (r, g, b)) in colors.iter().enumerate() {
      let sprite = Self::create_circle(BALL_RADIUS, *r, *g, *b);

      // Spread balls across the screen with different velocities
      let x = 100.0 + (i as f32 * 100.0);
      let y = 150.0 + (i as f32 * 30.0);

      // Varied speeds and directions
      let angle = (i as f32 * 0.5) + 0.3;
      let speed = 150.0 + (i as f32 * 20.0);

      ctx.world.spawn((
        Transform::from_xy(x, y),
        Renderable::sprite(sprite).with_z_order(10),
        Velocity::new(angle.cos() * speed, angle.sin() * speed),
        RigidBody::dynamic().with_gravity_scale(0.5).lock_rotation(),
        Collider::circle_collider(BALL_RADIUS as f32)
          .with_restitution(1.0)
          .with_friction(0.0),
      ));
    }
  }

  /// Spawn a rotating square (uses Tween)
  fn spawn_rotating_square(&self, ctx: &mut SceneContext) {
    let sprite = Self::create_square(40, 255, 200, 100);

    ctx.world.spawn((
      Transform::from_xy(150.0, self.screen_h - 100.0),
      Renderable::sprite(sprite).with_z_order(5),
      Tween::to_rotation(PI * 2.0, 2.0)
        .with_easing(Ease::Linear)
        .with_loop(TweenLoop::Loop),
    ));
  }

  /// Spawn a pulsing circle (uses Tween for scale)
  fn spawn_pulsing_circle(&self, ctx: &mut SceneContext) {
    let sprite = Self::create_circle(20, 100, 200, 255);

    ctx.world.spawn((
      Transform::from_xy(self.screen_w - 150.0, self.screen_h - 100.0),
      Renderable::sprite(sprite).with_z_order(5),
      Tween::to_scale(1.5, 1.5, 0.5)
        .with_easing(Ease::InOutSine)
        .with_loop(TweenLoop::PingPong),
    ));
  }

  /// Apply the time scale setting to the game clock
  fn apply_time_scale(&self, time: &mut Time) {
    match TIME_SCALES[self.time_scale.min(TIME_SCALES.len() - 1)] {
      Some(speed) => {
        time.game.resume();
        time.game.set_speed(speed);
      }
      None => time.game.pause(),
    }
  }
}

impl DemoPage for TimeControlPage {
  fn name(&self) -> &str {
    "Time"
  }

  fn description(&self) -> &str {
    "Watch physics & tweens respond to time!\n\
     The game clock's speed gives slow-mo and fast-forward, pause stops it and \
     hitstop freezes it briefly for impact. Rapier physics, tweens and update \
     all run on the fixed clock, which follows game time."
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

    self.spawn_walls(ctx);
    self.spawn_balls(ctx);
    self.spawn_rotating_square(ctx);
    self.spawn_pulsing_circle(ctx);
  }

  /// The time controls run once a frame on real time: `update` runs on the
  /// fixed clock, so it would run rarely in slow-mo and never while paused
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.apply_time_scale(ctx.time);

    if ui.input().key_pressed(KeyCode::Space) {
      ui.consume_key(KeyCode::Space);
      ctx.time.game.hitstop(0.1);
    }
    if ui.input().key_pressed(KeyCode::KeyJ) {
      ui.consume_key(KeyCode::KeyJ);
      ctx.time.game.hitstop(0.05);
    }

    let text_accent = Color::rgb(130, 180, 255);
    let text_light = Color::rgb(220, 220, 220);
    let text_warn = Color::rgb(255, 200, 80);

    // Clock readout, top left of the content area
    let area = content_area(ui);
    let (x, top) = (area.x + 6.0, area.y + 4.0);
    let game = &ctx.time.game;
    let state = if game.is_hitstop() {
      "HITSTOP"
    } else if game.is_paused() {
      "PAUSED"
    } else {
      "running"
    };
    ui.draw_text(
      &format!("Game clock: speed {:.1}x, {}", game.speed(), state),
      x,
      top,
      if state == "running" {
        text_light
      } else {
        text_warn
      },
    );
    ui.draw_text(
      &format!(
        "Game time {:.1} s   real time {:.1} s",
        game.elapsed(),
        ctx.time.real.elapsed()
      ),
      x,
      top + 20.0,
      text_light,
    );
    ui.draw_text(
      &format!("Fixed steps: {}", ctx.time.fixed.step_count()),
      x,
      top + 40.0,
      text_light,
    );

    // Bottom labels, above the shell's legend
    let bottom_y = area.y + area.height - 20.0;
    ui.draw_text("Rotating (Tween)", 100.0, bottom_y, text_accent);
    ui.draw_text("Pulsing (Tween)", 580.0, bottom_y, text_accent);
    ui.draw_text("Balls: Rapier physics", 300.0, bottom_y, text_accent);
  }
}

fn main() {
  Demo::new("Time Control")
    .resolution(800, 600)
    .configure(|b| b.target_fps(60).fixed_hz(60))
    .page(TimeControlPage::new)
    .run();
}
