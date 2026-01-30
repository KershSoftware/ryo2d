//! Debug Scene - Collision testing mode
//!
//! This scene allows manual movement with WASD/arrows to test collision boundaries.
//! Uses physics engine with kinematic bodies (no gravity) to test Rapier collision detection.

use ryo2d::prelude::*;

use crate::constants::*;

/// Debug scene for testing collision boundaries
pub struct DebugScene {
  bird: Option<Entity>,
  pipes: Vec<Entity>,
  ground: Option<Entity>,
  is_colliding: bool,
  move_speed: f32,
  /// Track which entity we're colliding with for debug display
  collision_info: String,
}

impl DebugScene {
  pub fn new() -> Self {
    Self {
      bird: None,
      pipes: Vec::new(),
      ground: None,
      is_colliding: false,
      move_speed: 200.0,
      collision_info: String::new(),
    }
  }

  fn spawn_bird(&mut self, ctx: &mut SceneContext) {
    // Spawn bird as DYNAMIC (not kinematic) so Rapier generates collision events
    // But with zero gravity so it doesn't fall - we move it manually
    let bird = ctx.world.spawn((
      Transform::new(BIRD_START_X, BIRD_START_Y),
      Velocity::new(0.0, 0.0),
      RigidBody::new().with_gravity_scale(0.0), // Dynamic but no gravity
      Collider::circle_collider(BIRD_SIZE / 2.0).with_offset(BIRD_SIZE / 2.0, BIRD_SIZE / 2.0),
      SpriteRenderer::new(Sprite::load("assets/bird.svg").expect("Failed to load bird sprite"))
        .with_z_order(10),
    ));
    self.bird = Some(bird);
  }

  fn spawn_test_pipes(&mut self, ctx: &mut SceneContext) {
    // Spawn a few static pipes at known positions for testing
    // Use kinematic bodies so they participate in physics collision detection
    // Format: (x, gap_center_y) - top_height is calculated from gap_center_y
    let pipe_positions = [
      (200.0, 250.0), // x, gap_center_y (gap at y=175 to y=325)
      (350.0, 300.0), // x, gap_center_y (gap at y=225 to y=375)
    ];

    for (x, gap_center_y) in pipe_positions {
      // Top pipe: from y=0 to (gap_center_y - PIPE_GAP/2)
      let top_height = gap_center_y - PIPE_GAP / 2.0;
      let pipe_sprite = Sprite::load("assets/pipe.png").expect("Failed to load pipe sprite");
      let sprite_w = pipe_sprite.width() as f32;
      let sprite_h = pipe_sprite.height() as f32;
      let scale_y = top_height / sprite_h;
      let mut transform = Transform::new(x, 0.0);
      transform.scale_y = scale_y;
      let top_pipe = ctx.world.spawn((
        transform,
        Velocity::new(0.0, 0.0),
        RigidBody::kinematic(),
        Collider::box_collider(sprite_w, top_height),
        SpriteRenderer::new(pipe_sprite).with_z_order(5),
      ));
      self.pipes.push(top_pipe);

      // Bottom pipe
      let bottom_y = gap_center_y + PIPE_GAP / 2.0;
      let bottom_height = GROUND_Y - bottom_y;
      let pipe_sprite = Sprite::load("assets/pipe.png").expect("Failed to load pipe sprite");
      let sprite_w = pipe_sprite.width() as f32;
      let sprite_h = pipe_sprite.height() as f32;
      let scale_y = bottom_height / sprite_h;
      let mut transform = Transform::new(x, bottom_y);
      transform.scale_y = scale_y;
      let bottom_pipe = ctx.world.spawn((
        transform,
        Velocity::new(0.0, 0.0),
        RigidBody::kinematic(),
        Collider::box_collider(sprite_w, bottom_height),
        SpriteRenderer::new(pipe_sprite).with_z_order(5),
      ));
      self.pipes.push(bottom_pipe);
    }
  }

  fn spawn_ground(&mut self, ctx: &mut SceneContext) {
    let ground = ctx.world.spawn((
      Transform::new(0.0, GROUND_Y),
      Velocity::new(0.0, 0.0),
      RigidBody::kinematic(),
      Collider::box_collider(SCREEN_WIDTH as f32, GROUND_HEIGHT),
      SpriteRenderer::new(Sprite::load("assets/ground.png").expect("Failed to load ground sprite"))
        .with_z_order(15),
    ));
    self.ground = Some(ground);
  }

  fn check_manual_collisions(&mut self, ctx: &mut SceneContext) {
    let Some(bird_entity) = self.bird else {
      return;
    };

    let Some(bird_transform) = ctx.world.get::<Transform>(bird_entity) else {
      return;
    };
    let Some(bird_collider) = ctx.world.get::<Collider>(bird_entity) else {
      return;
    };

    // Get bird's collision circle in world space
    let bird_cx = bird_transform.x + bird_collider.offset_x;
    let bird_cy = bird_transform.y + bird_collider.offset_y;
    let bird_radius = match bird_collider.shape {
      ColliderShape::Circle { radius } => radius,
      _ => BIRD_SIZE / 2.0,
    };

    self.is_colliding = false;

    // Check against all pipes
    for &pipe_entity in &self.pipes {
      let Some(pipe_transform) = ctx.world.get::<Transform>(pipe_entity) else {
        continue;
      };
      let Some(pipe_collider) = ctx.world.get::<Collider>(pipe_entity) else {
        continue;
      };

      if let ColliderShape::Box { width, height } = pipe_collider.shape {
        // Pipe box in world space
        let box_x = pipe_transform.x + pipe_collider.offset_x;
        let box_y = pipe_transform.y + pipe_collider.offset_y;

        // Circle vs AABB collision check
        let closest_x = bird_cx.clamp(box_x, box_x + width);
        let closest_y = bird_cy.clamp(box_y, box_y + height);

        let dx = bird_cx - closest_x;
        let dy = bird_cy - closest_y;
        let dist_sq = dx * dx + dy * dy;

        if dist_sq < bird_radius * bird_radius {
          self.is_colliding = true;
          self.collision_info = format!("Pipe {:?}", pipe_entity);
          break;
        }
      }
    }

    // Check against ground
    if !self.is_colliding {
      if let Some(ground_entity) = self.ground {
        if let Some(ground_transform) = ctx.world.get::<Transform>(ground_entity) {
          if let Some(ground_collider) = ctx.world.get::<Collider>(ground_entity) {
            if let ColliderShape::Box { width, height } = ground_collider.shape {
              let box_x = ground_transform.x + ground_collider.offset_x;
              let box_y = ground_transform.y + ground_collider.offset_y;

              let closest_x = bird_cx.clamp(box_x, box_x + width);
              let closest_y = bird_cy.clamp(box_y, box_y + height);

              let dx = bird_cx - closest_x;
              let dy = bird_cy - closest_y;
              let dist_sq = dx * dx + dy * dy;

              if dist_sq < bird_radius * bird_radius {
                self.is_colliding = true;
                self.collision_info = "Ground".to_string();
              }
            }
          }
        }
      }
    }

    // Check ceiling
    if !self.is_colliding && bird_cy - bird_radius < 0.0 {
      self.is_colliding = true;
      self.collision_info = "Ceiling (manual)".to_string();
    }
  }

  /// Check for collisions using Rapier physics events
  fn check_physics_collisions(&mut self, ctx: &SceneContext) {
    let Some(bird_entity) = self.bird else {
      return;
    };

    // Check if any collision event involves the bird
    for event in ctx.collision_events {
      if event.involves(bird_entity) {
        if event.event_type == CollisionEventType::Started {
          self.is_colliding = true;
          let other = event.other(bird_entity).unwrap();
          // Identify what we hit
          if self.pipes.contains(&other) {
            self.collision_info = format!("PHYSICS: Pipe {:?}", other);
          } else if self.ground == Some(other) {
            self.collision_info = "PHYSICS: Ground".to_string();
          } else {
            self.collision_info = format!("PHYSICS: Unknown {:?}", other);
          }
          return;
        }
      }
    }
  }
}

impl Scene for DebugScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.clear();
    ctx
      .camera
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

    self.spawn_bird(ctx);
    self.spawn_test_pipes(ctx);
    self.spawn_ground(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Manual movement with WASD/Arrows
    let mut dx = 0.0;
    let mut dy = 0.0;

    if ctx.input.key_down(KeyCode::KeyW) || ctx.input.key_down(KeyCode::ArrowUp) {
      dy -= self.move_speed * dt;
    }
    if ctx.input.key_down(KeyCode::KeyS) || ctx.input.key_down(KeyCode::ArrowDown) {
      dy += self.move_speed * dt;
    }
    if ctx.input.key_down(KeyCode::KeyA) || ctx.input.key_down(KeyCode::ArrowLeft) {
      dx -= self.move_speed * dt;
    }
    if ctx.input.key_down(KeyCode::KeyD) || ctx.input.key_down(KeyCode::ArrowRight) {
      dx += self.move_speed * dt;
    }

    // Apply movement to bird transform (kinematic body syncs from ECS)
    if let Some(bird_entity) = self.bird {
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(bird_entity) {
        transform.x += dx;
        transform.y += dy;
      }
    }

    // Reset collision state each frame
    self.is_colliding = false;
    self.collision_info.clear();

    // Check BOTH manual and physics collisions
    self.check_manual_collisions(ctx);
    let manual_result = self.is_colliding;
    let manual_info = self.collision_info.clone();

    // Now check physics
    self.is_colliding = false;
    self.collision_info.clear();
    self.check_physics_collisions(ctx);
    let physics_result = self.is_colliding;
    let physics_info = self.collision_info.clone();

    // Combine results for display
    self.is_colliding = manual_result || physics_result;
    self.collision_info = format!(
      "Manual: {} | Physics: {}",
      if manual_result { &manual_info } else { "NO" },
      if physics_result { &physics_info } else { "NO" }
    );

    // ESC to exit debug mode
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Pop;
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, _screen_h) = ui.screen_size();

    // Title
    ui.draw_text("DEBUG MODE (Physics Enabled)", 10.0, 10.0, Color::WHITE);
    ui.draw_text("WASD/Arrows to move", 10.0, 30.0, Color::WHITE);
    ui.draw_text("ESC to exit", 10.0, 50.0, Color::WHITE);

    // Collision indicator
    let indicator_text = if self.is_colliding {
      "COLLISION: YES"
    } else {
      "COLLISION: NO"
    };
    let indicator_color = if self.is_colliding {
      Color::new(255, 50, 50, 255) // Red
    } else {
      Color::new(50, 255, 50, 255) // Green
    };

    // Draw collision status prominently at top-right
    ui.draw_text(
      indicator_text,
      screen_w as f32 - 120.0,
      10.0,
      indicator_color,
    );

    // Draw detailed collision info
    ui.draw_text(&self.collision_info, 10.0, 70.0, Color::rgb(200, 200, 200));
  }
}
