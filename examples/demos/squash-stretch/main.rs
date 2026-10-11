//! Squash/Stretch Demo - Ryo2D Example
//!
//! This example demonstrates the squash/stretch system for impact deformation:
//! - Different intensities (subtle, normal, exaggerated)
//! - Different decay rates (snappy, normal, slow)
//! - Vertical squash (landing) vs horizontal squash (wall hits)
//! - Events (SquashStretchStarted, SquashStretchFinished)
//!
//! Runs on the demo-kit shell; intensity and recovery are settings.

use demo_kit::prelude::*;

/// Ball entity data
#[derive(Debug)]
struct Ball {
  velocity_y: f32,
  on_ground: bool,
  label: &'static str,
}

/// Intensity levels: (label, squash amount)
const INTENSITIES: [(&str, f32); 3] = [("Subtle", 0.15), ("Normal", 0.3), ("Exaggerated", 0.5)];
const INTENSITY_LABELS: [&str; 3] = [INTENSITIES[0].0, INTENSITIES[1].0, INTENSITIES[2].0];

/// Recovery speeds: (label, decay rate)
const RECOVERIES: [(&str, f32); 3] = [("Snappy", 12.0), ("Normal", 8.0), ("Slow", 4.0)];
const RECOVERY_LABELS: [&str; 3] = [RECOVERIES[0].0, RECOVERIES[1].0, RECOVERIES[2].0];

/// Demo scene
struct SquashStretchDemoScene {
  /// Ball entities organized by row
  balls: Vec<Entity>,
  /// Index into `INTENSITIES`
  intensity_choice: usize,
  /// Index into `RECOVERIES`
  recovery_choice: usize,
  /// Ground Y position
  ground_y: f32,
  /// Gravity
  gravity: f32,
}

impl SquashStretchDemoScene {
  fn new() -> Self {
    Self {
      balls: Vec::new(),
      intensity_choice: 1,
      recovery_choice: 1,
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

  fn intensity(&self) -> f32 {
    INTENSITIES[self.intensity_choice.min(INTENSITIES.len() - 1)].1
  }

  fn decay_rate(&self) -> f32 {
    RECOVERIES[self.recovery_choice.min(RECOVERIES.len() - 1)].1
  }

  /// Apply squash to a specific ball
  fn squash_ball(&self, ctx: &mut SceneContext, index: usize) {
    if let Some(&entity) = self.balls.get(index) {
      ctx.squash_with_decay(entity, self.intensity(), self.decay_rate());
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
          ctx.squash_with_decay(entity, self.intensity(), self.decay_rate());
        }
      }
    }
  }
}

impl DemoPage for SquashStretchDemoScene {
  fn name(&self) -> &str {
    "Bounce"
  }

  fn description(&self) -> &str {
    "Balls deform on impact; intensity sets how much, recovery how fast they spring back"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Intensity", &mut self.intensity_choice, &INTENSITY_LABELS);
    s.choice("Recovery", &mut self.recovery_choice, &RECOVERY_LABELS);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Space, "Drop the balls"),
      Control::keys(
        "1-5",
        &[
          KeyCode::Digit1,
          KeyCode::Digit2,
          KeyCode::Digit3,
          KeyCode::Digit4,
          KeyCode::Digit5,
        ],
        "Squash a ball",
      ),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.spawn_balls(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Drop balls
    if ctx.input.key_pressed(KeyCode::Space) {
      self.drop_balls(ctx);
    }

    // Manual squash on individual balls
    let keys = [
      KeyCode::Digit1,
      KeyCode::Digit2,
      KeyCode::Digit3,
      KeyCode::Digit4,
      KeyCode::Digit5,
    ];
    for (i, key) in keys.into_iter().enumerate() {
      if ctx.input.key_pressed(key) {
        self.squash_ball(ctx, i);
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
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let text_muted = Color::rgb(160, 160, 160);

    // Ground line indicator
    let (screen_w, _) = ui.screen_size();
    let ground_y = self.ground_y;
    ui.draw_text(
      "--- Ground ---",
      screen_w as f32 / 2.0 - 50.0,
      ground_y + 20.0,
      text_muted,
    );
  }
}

fn main() {
  Demo::new("Squash/Stretch")
    .resolution(800, 600)
    .configure(|b| {
      b.window_mode(WindowMode::ResizableAspectRatio)
        .target_fps(60)
        .physics_hz(60)
    })
    .page(SquashStretchDemoScene::new)
    .run();
}
