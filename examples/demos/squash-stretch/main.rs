//! Squash/Stretch Demo - Ryo2D Example
//!
//! This example demonstrates the squash/stretch system for impact deformation:
//! - Different intensities (subtle, normal, exaggerated)
//! - Different decay rates (snappy, normal, slow)
//! - Vertical squash (landing) vs horizontal squash (wall hits)
//! - Events (SquashStretchStarted, SquashStretchFinished)
//!
//! ## Controls
//!
//! - 1: Subtle squash (intensity: 0.15)
//! - 2: Normal squash (intensity: 0.3)
//! - 3: Exaggerated squash (intensity: 0.5)
//! - Q: Snappy recovery (decay: 12.0)
//! - W: Normal recovery (decay: 8.0)
//! - E: Slow recovery (decay: 4.0)
//! - Space: Drop all balls (trigger landing squash)
//! - R: Reset all balls
//! - Escape: Exit

use ryo2d::prelude::*;

/// Ball entity data
#[derive(Debug)]
struct Ball {
  start_x: f32,
  start_y: f32,
  velocity_y: f32,
  on_ground: bool,
  label: &'static str,
}

/// Demo scene
struct SquashStretchDemoScene {
  /// Ball entities organized by row
  balls: Vec<Entity>,
  /// Current intensity setting
  intensity: f32,
  /// Current decay rate setting
  decay_rate: f32,
  /// Ground Y position
  ground_y: f32,
  /// Gravity
  gravity: f32,
}

impl SquashStretchDemoScene {
  fn new() -> Self {
    Self {
      balls: Vec::new(),
      intensity: 0.3,
      decay_rate: 8.0,
      ground_y: 0.0,
      gravity: 800.0,
    }
  }

  /// Create a colored circle sprite
  fn create_circle(radius: u32, r: u8, g: u8, b: u8) -> SpriteHandle {
    let sprite = Sprite::solid_circle(radius, r, g, b, 255);
    SpriteHandle::from_sprite(sprite, format!("circle_{}_{}_{}_{}", radius, r, g, b))
  }

  /// Spawn balls in a row
  fn spawn_balls(&mut self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    self.ground_y = screen_h - 100.0;

    let colors = [
      (255, 100, 100, "Red"),
      (100, 255, 100, "Green"),
      (100, 100, 255, "Blue"),
      (255, 255, 100, "Yellow"),
      (255, 100, 255, "Magenta"),
    ];

    let spacing = screen_w / (colors.len() as f32 + 1.0);

    for (i, (r, g, b, label)) in colors.iter().enumerate() {
      let x = spacing * (i as f32 + 1.0);
      let y = 150.0;

      let sprite = Self::create_circle(30, *r, *g, *b);
      let entity = ctx.world.spawn((
        Ball {
          start_x: x,
          start_y: y,
          velocity_y: 0.0,
          on_ground: false,
          label,
        },
        Transform::new(x, y).with_pivot_center(),
        Renderable::sprite(sprite).with_z_order(10),
      ));
      self.balls.push(entity);
    }
  }

  /// Drop all balls
  fn drop_balls(&self, ctx: &mut SceneContext) {
    for &entity in &self.balls {
      if let Some(mut ball) = ctx.world.get_mut::<Ball>(entity) {
        ball.velocity_y = 0.0;
        ball.on_ground = false;
      }
      // Move ball up a bit to give it room to fall
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.y = 150.0;
      }
    }
  }

  /// Reset all balls to starting positions
  fn reset_balls(&self, ctx: &mut SceneContext) {
    for &entity in &self.balls {
      if let Some(ball) = ctx.world.get::<Ball>(entity) {
        let start_x = ball.start_x;
        let start_y = ball.start_y;
        drop(ball);

        if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
          transform.x = start_x;
          transform.y = start_y;
          transform.scale_x = 1.0;
          transform.scale_y = 1.0;
        }
      }
      if let Some(mut ball) = ctx.world.get_mut::<Ball>(entity) {
        ball.velocity_y = 0.0;
        ball.on_ground = false;
      }
      // Stop any active squash effect
      if let Some(mut ss) = ctx.world.get_mut::<SquashStretch>(entity) {
        ss.stop();
      }
    }
  }

  /// Apply squash to a specific ball
  fn squash_ball(&self, ctx: &mut SceneContext, index: usize) {
    if let Some(&entity) = self.balls.get(index) {
      ctx.squash_with_decay(entity, self.intensity, self.decay_rate);
    }
  }

  /// Update ball physics
  fn update_physics(&self, ctx: &mut SceneContext, dt: f32) {
    for &entity in &self.balls {
      let mut should_squash = false;

      // Update velocity and position
      if let Some(mut ball) = ctx.world.get_mut::<Ball>(entity)
        && !ball.on_ground
      {
        ball.velocity_y += self.gravity * dt;
      }

      // Get current position and velocity
      let (_current_y, velocity_y) = {
        let ball = ctx.world.get::<Ball>(entity);
        let transform = ctx.world.get::<Transform>(entity);
        match (ball, transform) {
          (Some(b), Some(t)) => (t.y, b.velocity_y),
          _ => continue,
        }
      };

      // Update position
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.y += velocity_y * dt;

        // Check ground collision
        if transform.y >= self.ground_y {
          transform.y = self.ground_y;
          should_squash = true;
        }
      }

      // Handle landing
      if should_squash {
        let should_apply_squash = {
          if let Some(mut ball) = ctx.world.get_mut::<Ball>(entity) {
            if !ball.on_ground {
              ball.on_ground = true;
              ball.velocity_y = 0.0;
              true
            } else {
              false
            }
          } else {
            false
          }
        };

        if should_apply_squash {
          // Apply squash using current intensity setting
          ctx.squash_with_decay(entity, self.intensity, self.decay_rate);
        }
      }
    }
  }
}

impl Scene for SquashStretchDemoScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    println!("Squash/Stretch Demo initialized!");
    println!("  1/2/3: Intensity (subtle/normal/exaggerated)");
    println!("  Q/W/E: Recovery speed (snappy/normal/slow)");
    println!("  Space: Drop all balls");
    println!("  R: Reset");
    println!("  ESC: Exit");

    self.spawn_balls(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Exit on escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Intensity controls
    if ctx.input.key_pressed(KeyCode::Digit1) {
      self.intensity = 0.15;
      println!("Intensity: Subtle (0.15)");
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      self.intensity = 0.3;
      println!("Intensity: Normal (0.3)");
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      self.intensity = 0.5;
      println!("Intensity: Exaggerated (0.5)");
    }

    // Decay rate controls
    if ctx.input.key_pressed(KeyCode::KeyQ) {
      self.decay_rate = 12.0;
      println!("Recovery: Snappy (12.0)");
    }
    if ctx.input.key_pressed(KeyCode::KeyW) {
      self.decay_rate = 8.0;
      println!("Recovery: Normal (8.0)");
    }
    if ctx.input.key_pressed(KeyCode::KeyE) {
      self.decay_rate = 4.0;
      println!("Recovery: Slow (4.0)");
    }

    // Drop balls
    if ctx.input.key_pressed(KeyCode::Space) {
      self.drop_balls(ctx);
      println!("Dropping balls!");
    }

    // Reset
    if ctx.input.key_pressed(KeyCode::KeyR) {
      self.reset_balls(ctx);
      println!("Reset!");
    }

    // Manual squash on individual balls (number keys + shift)
    if ctx.input.key_down(KeyCode::ShiftLeft) {
      for i in 0..5 {
        let key = match i {
          0 => KeyCode::Digit1,
          1 => KeyCode::Digit2,
          2 => KeyCode::Digit3,
          3 => KeyCode::Digit4,
          4 => KeyCode::Digit5,
          _ => continue,
        };
        if ctx.input.key_pressed(key) {
          self.squash_ball(ctx, i);
        }
      }
    }

    // Update physics
    self.update_physics(ctx, dt);

    // Log events
    for event in ctx.events.read::<SquashStretchStarted>() {
      if let Some(ball) = ctx.world.get::<Ball>(event.entity) {
        println!("{} ball squashed!", ball.label);
      }
    }

    for event in ctx.events.read::<SquashStretchFinished>() {
      if let Some(ball) = ctx.world.get::<Ball>(event.entity) {
        println!("{} ball recovered", ball.label);
      }
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let text_light = Color::rgb(220, 220, 220);
    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);

    // Title
    ui.draw_text("Squash/Stretch Demo", 10.0, 10.0, text_light);

    // Current settings
    let intensity_label = match self.intensity {
      i if i <= 0.2 => "Subtle",
      i if i <= 0.35 => "Normal",
      _ => "Exaggerated",
    };
    let decay_label = match self.decay_rate {
      d if d >= 10.0 => "Snappy",
      d if d >= 6.0 => "Normal",
      _ => "Slow",
    };

    let settings = format!(
      "Intensity: {} ({:.2}) | Recovery: {} ({:.1})",
      intensity_label, self.intensity, decay_label, self.decay_rate
    );
    ui.draw_text(&settings, 10.0, 30.0, text_accent);

    // Controls
    ui.draw_text("1/2/3: Intensity | Q/W/E: Recovery", 10.0, 50.0, text_muted);
    ui.draw_text("Space: Drop | R: Reset | ESC: Exit", 10.0, 70.0, text_muted);

    // Ground line indicator
    let (screen_w, _) = ui.screen_size();
    let ground_y = self.ground_y;
    ui.draw_text(
      "--- Ground ---",
      screen_w as f32 / 2.0 - 50.0,
      ground_y + 20.0,
      text_muted,
    );
    SceneAction::Continue
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Squash/Stretch Demo - Ryo2D")
    .resolution(800, 600)
    .window_mode(WindowMode::ResizableAspectRatio)
    .target_fps(60)
    .physics_hz(60)
    .with_scene(SquashStretchDemoScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
