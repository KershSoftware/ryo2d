//! Ferrum2D Demo - Breakout-style physics showcase
//!
//! This demo showcases the engine's Rapier2D physics integration:
//! - Balls fall and bounce with realistic physics (handled by Rapier)
//! - Player controls a paddle with keyboard
//! - Collision events used for game logic (not physics response)

use ferrum2d::prelude::*;

/// Demo game showcasing Rapier2D physics
struct BreakoutDemo {
  paddle: Option<Entity>,
  balls: Vec<Entity>,
  walls: Vec<Entity>,
  score: u32,
}

impl BreakoutDemo {
  fn new() -> Self {
    Self {
      paddle: None,
      balls: Vec::new(),
      walls: Vec::new(),
      score: 0,
    }
  }

  /// Create wall entities (kinematic bodies that don't move)
  fn spawn_walls(&mut self, ctx: &mut GameContext) {
    // Floor
    let floor = ctx.world.spawn((
      Transform::new(0.0, 680.0),
      RigidBody::kinematic(),
      Collider::box_collider(1280.0, 40.0),
      SpriteRenderer::new(Sprite::solid_rect(1280, 40, 0x40, 0x40, 0x40, 0xff)).with_z_order(0),
    ));
    self.walls.push(floor);

    // Left wall
    let left = ctx.world.spawn((
      Transform::new(-40.0, 0.0),
      RigidBody::kinematic(),
      Collider::box_collider(40.0, 720.0),
      SpriteRenderer::new(Sprite::solid_rect(40, 720, 0x40, 0x40, 0x40, 0xff)).with_z_order(0),
    ));
    self.walls.push(left);

    // Right wall
    let right = ctx.world.spawn((
      Transform::new(1280.0, 0.0),
      RigidBody::kinematic(),
      Collider::box_collider(40.0, 720.0),
      SpriteRenderer::new(Sprite::solid_rect(40, 720, 0x40, 0x40, 0x40, 0xff)).with_z_order(0),
    ));
    self.walls.push(right);

    // Ceiling
    let ceiling = ctx.world.spawn((
      Transform::new(0.0, -40.0),
      RigidBody::kinematic(),
      Collider::box_collider(1280.0, 40.0),
      SpriteRenderer::new(Sprite::solid_rect(1280, 40, 0x40, 0x40, 0x40, 0xff)).with_z_order(0),
    ));
    self.walls.push(ceiling);
  }

  /// Create the player paddle (kinematic body - controlled by input)
  fn spawn_paddle(&mut self, ctx: &mut GameContext) {
    let paddle_width = 120.0;
    let paddle_height = 16.0;

    let paddle = ctx.world.spawn((
      Transform::new(580.0, 620.0),
      Velocity::zero(),
      RigidBody::kinematic(),
      Collider::box_collider(paddle_width, paddle_height),
      SpriteRenderer::new(Sprite::solid_rect(
        paddle_width as u32,
        paddle_height as u32,
        0x40,
        0xc0,
        0xff,
        0xff,
      ))
      .with_z_order(10),
    ));
    self.paddle = Some(paddle);
  }

  /// Spawn balls with physics
  fn spawn_balls(&mut self, ctx: &mut GameContext) {
    let colors = [
      (0xff, 0x60, 0x60), // Red
      (0x60, 0xff, 0x60), // Green
      (0x60, 0x60, 0xff), // Blue
      (0xff, 0xff, 0x60), // Yellow
      (0xff, 0x60, 0xff), // Magenta
      (0x60, 0xff, 0xff), // Cyan
    ];

    for i in 0..12 {
      let x = 150.0 + (i % 6) as f32 * 160.0;
      let y = 80.0 + (i / 6) as f32 * 80.0;
      let (r, g, b) = colors[i % colors.len()];

      // Random-ish initial velocity for variety
      let vx = ((i as f32 * 47.0) % 200.0) - 100.0;
      let vy = 50.0 + (i as f32 * 23.0) % 100.0;

      let ball_size = 20;
      let ball_radius = ball_size as f32 / 2.0;

      let ball = ctx.world.spawn((
        Transform::new(x, y),
        Velocity::new(vx, vy),
        // Dynamic body - fully simulated by Rapier
        // Gravity scale controls how much gravity affects this body
        // Restitution (bounciness) is configured in rapier_physics.rs collider defaults
        RigidBody::new().with_gravity_scale(0.5).with_drag(0.01),
        // Circle collider centered on sprite
        Collider::circle_collider(ball_radius).with_offset(ball_radius, ball_radius),
        SpriteRenderer::new(Sprite::solid_rect(ball_size, ball_size, r, g, b, 0xff))
          .with_z_order(5),
      ));

      self.balls.push(ball);
    }
  }

  /// Reset all balls to starting positions
  fn reset_balls(&mut self, ctx: &mut GameContext) {
    for (i, &ball) in self.balls.iter().enumerate() {
      let x = 150.0 + (i % 6) as f32 * 160.0;
      let y = 80.0 + (i / 6) as f32 * 80.0;
      let vx = ((i as f32 * 47.0) % 200.0) - 100.0;
      let vy = 50.0 + (i as f32 * 23.0) % 100.0;

      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(ball) {
        transform.x = x;
        transform.y = y;
      }
      if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(ball) {
        vel.dx = vx;
        vel.dy = vy;
      }
    }
    self.score = 0;
  }

  /// Create background grid for visual reference
  fn spawn_background(&mut self, ctx: &mut GameContext) {
    for row in 0..8 {
      for col in 0..14 {
        let x = col as f32 * 100.0;
        let y = row as f32 * 100.0;
        let color = if (row + col) % 2 == 0 {
          (0x25, 0x25, 0x30)
        } else {
          (0x2a, 0x2a, 0x35)
        };
        let marker = Sprite::solid_rect(16, 16, color.0, color.1, color.2, 0xff);
        ctx.world.spawn((
          Transform::new(x, y),
          SpriteRenderer::new(marker).with_z_order(-10),
        ));
      }
    }
  }
}

impl Game for BreakoutDemo {
  fn init(&mut self, ctx: &mut GameContext) {
    // Spawn game entities
    self.spawn_background(ctx);
    self.spawn_walls(ctx);
    self.spawn_paddle(ctx);
    self.spawn_balls(ctx);

    // Center camera
    ctx.camera.set_position(640.0, 360.0);

    println!("=== Ferrum2D Breakout Demo ===");
    println!("Controls:");
    println!("  A/D or Arrow Keys - Move paddle");
    println!("  Space - Reset balls");
    println!("  Mouse scroll - Zoom camera");
  }

  fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    // === Paddle Movement (kinematic body) ===
    if let Some(paddle) = self.paddle
      && let Some(mut transform) = ctx.world.get_component_mut::<Transform>(paddle)
    {
      let speed = 500.0;

      if ctx.input.key_down(KeyCode::KeyA) || ctx.input.key_down(KeyCode::ArrowLeft) {
        transform.x -= speed * dt;
      }
      if ctx.input.key_down(KeyCode::KeyD) || ctx.input.key_down(KeyCode::ArrowRight) {
        transform.x += speed * dt;
      }

      // Clamp paddle to screen bounds
      transform.x = transform.x.clamp(0.0, 1280.0 - 120.0);
    }

    // === Process Collision Events (game logic only) ===
    // Note: Physics response (bouncing) is handled automatically by Rapier.
    // We only use events for game logic like scoring.
    for event in ctx.collision_events {
      if event.event_type != CollisionEventType::Started {
        continue;
      }

      // Check if a ball hit the paddle
      if let Some(paddle) = self.paddle
        && event.involves(paddle)
        && let Some(ball) = event.other(paddle)
        && self.balls.contains(&ball)
      {
        self.score += 1;
        // Could play a sound here
      }
    }

    // === Reset with Space ===
    if ctx.input.key_pressed(KeyCode::Space) {
      self.reset_balls(ctx);
      println!("Balls reset! Score was: {}", self.score);
    }

    // === Zoom Controls ===
    let scroll = ctx.input.scroll_delta();
    if scroll.1 != 0.0 {
      let new_zoom = ctx.camera.zoom + scroll.1 * 0.1;
      ctx.camera.set_zoom(new_zoom);
    }
  }
}

fn main() {
  // Engine uses default physics: gravity = (0.0, 980.0) pixels/s²
  // Balls have gravity_scale=0.5 for a gentler, bouncier feel
  let engine = EngineBuilder::new()
    .title("Ferrum2D - Breakout Demo")
    .resolution(1280, 720)
    .fps(60)
    .with_game(BreakoutDemo::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
