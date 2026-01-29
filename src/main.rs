//! Demo game showcasing Ferrum2D engine features

use ferrum2d::prelude::*;

/// Demo game showcasing the engine's features
struct DemoGame {
  player: Option<Entity>,
  balls: Vec<Entity>,
  floor: Option<Entity>,
  physics_world: PhysicsWorld,
}

impl DemoGame {
  fn new() -> Self {
    DemoGame {
      player: None,
      balls: Vec::new(),
      floor: None,
      physics_world: PhysicsWorld::new(),
    }
  }
}

impl Game for DemoGame {
  fn init(&mut self, ctx: &mut GameContext) {
    // Create a player entity with components (kinematic - controlled by input)
    let player_sprite = Sprite::solid_rect(64, 16, 0x40, 0xc0, 0xff, 0xff);
    let player = ctx.world.spawn((
      Transform::new(400.0, 500.0),
      Velocity::zero(),
      RigidBody::kinematic(),
      Collider::box_collider(64.0, 16.0),
      SpriteRenderer::new(player_sprite).with_z_order(10),
    ));

    self.player = Some(player);

    // Create falling balls with physics (many balls to demonstrate spatial partitioning)
    for i in 0..20 {
      let x = 100.0 + (i % 10) as f32 * 110.0;
      let y = 50.0 + (i / 10) as f32 * 50.0 + (i % 5) as f32 * 20.0;

      // Colored ball sprite
      let colors = [
        (0xff, 0x40, 0x40), // Red
        (0x40, 0xff, 0x40), // Green
        (0x40, 0x40, 0xff), // Blue
        (0xff, 0xff, 0x40), // Yellow
        (0xff, 0x40, 0xff), // Magenta
      ];
      let (r, g, b) = colors[i % colors.len()];
      let ball_sprite = Sprite::solid_rect(24, 24, r, g, b, 0xff);

      let ball = ctx.world.spawn((
        Transform::new(x, y),
        Velocity::zero(),
        RigidBody::new().with_gravity_scale(1.0).with_drag(0.01),
        Collider::circle_collider(12.0).with_offset(12.0, 12.0),
        SpriteRenderer::new(ball_sprite).with_z_order(5),
      ));

      self.balls.push(ball);
    }

    // Create a floor (static - no physics, but has collider)
    let floor_sprite = Sprite::solid_rect(1280, 32, 0x60, 0x60, 0x60, 0xff);
    let floor = ctx.world.spawn((
      Transform::new(0.0, 600.0),
      Collider::box_collider(1280.0, 32.0),
      SpriteRenderer::new(floor_sprite).with_z_order(0),
    ));
    self.floor = Some(floor);

    // Create some background markers so zoom is visible
    // Background layer (z_order = -10, drawn first)
    for row in 0..8 {
      for col in 0..14 {
        let x = col as f32 * 100.0;
        let y = row as f32 * 100.0;
        // Alternating colors for a grid pattern
        let color = if (row + col) % 2 == 0 {
          (0x30, 0x30, 0x38) // dark
        } else {
          (0x38, 0x38, 0x40) // slightly lighter
        };
        let marker_sprite = Sprite::solid_rect(16, 16, color.0, color.1, color.2, 0xff);
        ctx.world.spawn((
          Transform::new(x, y),
          SpriteRenderer::new(marker_sprite).with_z_order(-10),
        ));
      }
    }

    // Center camera
    ctx.camera.set_position(640.0, 360.0);
  }

  fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
    // === Player Input System (left/right movement only) ===
    if let Some(player) = self.player
      && let Some(mut velocity) = ctx.world.get_component_mut::<Velocity>(player)
    {
      let speed = 300.0;
      velocity.dx = 0.0;

      if ctx.input.key_down(KeyCode::KeyA) || ctx.input.key_down(KeyCode::ArrowLeft) {
        velocity.dx = -speed;
      }
      if ctx.input.key_down(KeyCode::KeyD) || ctx.input.key_down(KeyCode::ArrowRight) {
        velocity.dx = speed;
      }
    }

    // === Collision Detection using PhysicsWorld (broad + narrow phase) ===
    // Update the spatial hash with current entity positions
    self.physics_world.update(ctx.world);

    // Get all collisions using efficient spatial partitioning
    let collisions = self.physics_world.get_collisions(ctx.world);

    for collision in collisions {
      // Ball vs Floor collision
      if let Some(floor) = self.floor
        && collision.involves(floor)
        && let Some(ball) = collision.other(floor)
        && self.balls.contains(&ball)
      {
        // Get ball velocity to check direction (extract value, release borrow)
        let vel_dy = ctx.world.get_component::<Velocity>(ball).map(|v| v.dy);

        if let Some(vel_dy) = vel_dy
          && vel_dy > 0.0
        {
          // Extract floor position (release borrow before mutating)
          let floor_top = ctx.world.get_component::<Transform>(floor).map(|t| t.y);

          if let Some(floor_top) = floor_top {
            // Now safe to mutate Transform
            if let Some(mut t) = ctx.world.get_component_mut::<Transform>(ball) {
              t.y = floor_top - 24.0;
            }
          }
          // Bounce with energy loss
          if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(ball) {
            vel.dy = -vel.dy.abs() * 0.7;
            if vel.dy.abs() < 30.0 {
              vel.dy = 0.0;
            }
          }
        }
      }

      // Ball vs Player collision
      if let Some(player) = self.player
        && collision.involves(player)
        && let Some(ball) = collision.other(player)
        && self.balls.contains(&ball)
      {
        let vel_dy = ctx.world.get_component::<Velocity>(ball).map(|v| v.dy);

        if let Some(vel_dy) = vel_dy
          && vel_dy > 0.0
        {
          // Extract all values first to release borrows before mutation
          let ball_x = ctx
            .world
            .get_component::<Transform>(ball)
            .map(|t| t.x)
            .unwrap_or(0.0);
          let player_pos = ctx
            .world
            .get_component::<Transform>(player)
            .map(|t| (t.x, t.y));

          if let Some((paddle_x, paddle_y)) = player_pos {
            // Now safe to mutate - no immutable borrows held
            if let Some(mut t) = ctx.world.get_component_mut::<Transform>(ball) {
              t.y = paddle_y - 24.0;
            }
            // Bounce off player with extra force
            if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(ball) {
              vel.dy = -vel.dy.abs() - 100.0;
              // Add horizontal velocity based on hit position
              let ball_center_x = ball_x + 12.0;
              let paddle_center_x = paddle_x + 32.0;
              vel.dx += (ball_center_x - paddle_center_x) * 4.0;
            }
          }
        }
      }
    }

    // === Reset balls with Space key ===
    if ctx.input.key_pressed(KeyCode::Space) {
      for (i, &ball) in self.balls.iter().enumerate() {
        if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(ball) {
          transform.x = 100.0 + (i % 10) as f32 * 110.0;
          transform.y = 50.0 + (i / 10) as f32 * 50.0 + (i % 5) as f32 * 20.0;
        }
        if let Some(mut velocity) = ctx.world.get_component_mut::<Velocity>(ball) {
          velocity.dx = 0.0;
          velocity.dy = 0.0;
        }
      }
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
  let engine = EngineBuilder::new()
    .title("Ferrum2D Demo")
    .resolution(1280, 720)
    .fps(60)
    .with_game(DemoGame::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
