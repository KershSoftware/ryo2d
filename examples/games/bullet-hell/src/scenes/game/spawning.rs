//! Entity spawning methods for GameScene

use crate::components::*;
use crate::*;
use rand::Rng;
use ryo2d::prelude::*;

use super::GameScene;

impl GameScene {
  pub(super) fn spawn_player(&mut self, ctx: &mut SceneContext) {
    let sprite = self.player_sprite.clone();
    // Spawn player at center-bottom of screen (positions are sprite centres)
    let spawn_y = self.screen_height - 240.0; // Near bottom
    let player_entity = ctx
      .world
      .spawn((
        Transform::from_xy(self.screen_width / 2.0, spawn_y),
        Renderable::sprite(sprite)
          .with_z_order(Z_PLAYER)
          .with_shader(SpriteShader::outline(Color::rgb(0, 0, 0), 2)),
        Interpolated,
        // Kinematic body - we handle movement manually for wall slides/custom gravity
        // Rapier still provides collision detection for events
        RigidBody::kinematic(),
        Collider::box_collider(PLAYER_SIZE, PLAYER_SIZE),
        PlayerVelocity::default(),
        Player::new(PLAYER_MAX_LIVES),
      ))
      .id();
    self.player = Some(player_entity);
  }

  pub(super) fn spawn_platforms(&mut self, ctx: &mut SceneContext) {
    let sprite = self.platform_sprite.clone();
    let sw = self.screen_width;
    let sh = self.screen_height;

    // Ground platform - full width, its top edge 30px above the bottom
    let ground_transform = Transform::from_xy(sw / 2.0, sh - 30.0 + PLATFORM_THICKNESS / 2.0)
      .with_scale(Vec2::new(sw / 800.0, 1.0)); // Scale from 800px base
    let ground = ctx
      .world
      .spawn((
        ground_transform,
        Renderable::sprite(sprite.clone())
          .with_z_order(Z_PLATFORM)
          .with_shader(SpriteShader::platform_outline(2)),
        RigidBody::fixed(),
        // Colliders are not scaled with the sprite: give the final size
        Collider::box_collider(sw, PLATFORM_THICKNESS),
        Platform,
      ))
      .id();
    self.platforms.push(ground);

    // Floating platforms - scaled proportionally to screen size
    // Base design was for 2560x1440, scale positions accordingly
    let scale_x = sw / 2560.0;
    let scale_y = sh / 1440.0;

    // Platform configs: (center_x, y, width) in 2560x1440 base coordinates
    // Reduced platform count for more dynamic gameplay - staggered pattern
    let platform_configs = vec![
      // Layer 1: Bottom (fewer platforms to start)
      (640.0, 1270.0, 350.0),  // Left-center
      (1920.0, 1270.0, 350.0), // Right-center
      // Layer 2: y=1100 - single side platforms
      (320.0, 1100.0, 300.0),  // Far left
      (2240.0, 1100.0, 300.0), // Far right
      // Layer 3: y=920 - center only
      (1280.0, 920.0, 450.0), // Center (wider)
      // Layer 4: y=740 - sides
      (500.0, 740.0, 350.0),  // Left
      (2060.0, 740.0, 350.0), // Right
      // Layer 5: y=560 - center only
      (1280.0, 560.0, 400.0), // Center
      // Layer 6: y=380 - sides
      (700.0, 380.0, 300.0),  // Left-center
      (1860.0, 380.0, 300.0), // Right-center
      // Layer 7: y=200 - top center
      (1280.0, 200.0, 350.0), // Top center
    ];

    for (base_x, base_y, base_width) in platform_configs {
      // Scale positions and widths to current screen size (y is the top edge)
      let center_x = base_x * scale_x;
      let y = base_y * scale_y;
      let width = base_width * scale_x;

      let transform = Transform::from_xy(center_x, y + PLATFORM_THICKNESS / 2.0)
        .with_scale(Vec2::new(width / 800.0, 1.0)); // Scale from 800px base sprite
      let platform = ctx
        .world
        .spawn((
          transform,
          Renderable::sprite(sprite.clone())
            .with_z_order(Z_PLATFORM)
            .with_shader(SpriteShader::platform_outline(2)),
          RigidBody::fixed(),
          // Colliders are not scaled with the sprite: give the final size
          Collider::box_collider(width, PLATFORM_THICKNESS),
          Platform,
        ))
        .id();
      self.platforms.push(platform);
    }
  }

  pub(super) fn spawn_turrets(&mut self, ctx: &mut SceneContext) {
    let sprite = self.turret_sprite.clone();
    let mut rng = rand::thread_rng();
    let sh = self.screen_height;
    let sw = self.screen_width;

    // Stagger offset between sides so sounds play back-to-back
    let stagger_delay = 1.3;

    // Randomize which side spawns first
    let left_first = rng.gen_bool(0.5);

    // Left side turrets - flush against left wall
    for _ in 0..TURRETS_PER_SIDE {
      let y = rng.gen_range(100.0..(sh - 100.0)) + TURRET_SIZE / 2.0;
      let turret = ctx
        .world
        .spawn((
          Transform::from_xy(TURRET_SIZE / 2.0, y),
          Renderable::sprite(sprite.clone())
            .with_z_order(Z_TURRET)
            .with_shader(SpriteShader::outline(Color::rgb(255, 140, 140), 2)),
          RigidBody::kinematic(),
          Collider::box_collider(TURRET_SIZE, TURRET_SIZE),
          Turret::new(TurretSide::Left),
        ))
        .id();
      // Add stagger delay if right side spawns first
      if !left_first && let Some(mut turret_comp) = ctx.world.get_mut::<Turret>(turret) {
        turret_comp.spawn_timer += stagger_delay;
      }
      self.turrets.push(turret);
    }

    // Right side turrets - flush against right wall
    for _ in 0..TURRETS_PER_SIDE {
      let y = rng.gen_range(100.0..(sh - 100.0)) + TURRET_SIZE / 2.0;
      let turret_entity = ctx
        .world
        .spawn((
          Transform::from_xy(sw - TURRET_SIZE / 2.0, y),
          Renderable::sprite(sprite.clone())
            .with_z_order(Z_TURRET)
            .with_shader(SpriteShader::outline(Color::rgb(255, 140, 140), 2)),
          RigidBody::kinematic(),
          Collider::box_collider(TURRET_SIZE, TURRET_SIZE),
          Turret::new(TurretSide::Right),
        ))
        .id();
      // Add stagger delay if left side spawns first
      if left_first && let Some(mut turret) = ctx.world.get_mut::<Turret>(turret_entity) {
        turret.spawn_timer += stagger_delay;
      }
      self.turrets.push(turret_entity);
    }
  }

  pub(super) fn spawn_barriers(&mut self, ctx: &mut SceneContext) {
    let sprite = self.barrier_sprite.clone();
    let center_x = self.screen_width / 2.0;

    // Vertical movement bounds of the barrier's centre (top edge at 150 or
    // lower, bottom edge at least 100 above the bottom of the screen)
    let min_y = 150.0 + BARRIER_HEIGHT / 2.0;
    let max_y = self.screen_height - 100.0 - BARRIER_HEIGHT / 2.0;

    // Left barrier - left of center, moving down initially
    let left_barrier = ctx
      .world
      .spawn((
        Transform::from_xy(center_x - 250.0 + BARRIER_WIDTH / 2.0, min_y + 100.0),
        Renderable::sprite(sprite.clone())
          .with_z_order(Z_BARRIER)
          .with_shader(SpriteShader::white_outline(2)),
        RigidBody::kinematic(),
        Collider::box_collider(BARRIER_WIDTH, BARRIER_HEIGHT),
        Barrier::new(BARRIER_SPEED, min_y, max_y),
      ))
      .id();
    self.barriers.push(left_barrier);

    // Right barrier - right of center, moving up initially (mirrored)
    let right_barrier = ctx
      .world
      .spawn((
        Transform::from_xy(center_x + 250.0 - BARRIER_WIDTH / 2.0, max_y - 100.0),
        Renderable::sprite(sprite.clone())
          .with_z_order(Z_BARRIER)
          .with_shader(SpriteShader::white_outline(2)),
        RigidBody::kinematic(),
        Collider::box_collider(BARRIER_WIDTH, BARRIER_HEIGHT),
        Barrier::new(-BARRIER_SPEED, min_y, max_y),
      ))
      .id();
    self.barriers.push(right_barrier);
  }

  pub(super) fn spawn_drone(&mut self, ctx: &mut SceneContext) {
    use crate::constants::{DRONE_SIZE, Z_DRONE};

    let sprite = self.drone_sprite.clone();

    // Spawn drone in center-upper area of screen
    let spawn_x = self.screen_width / 2.0;
    let spawn_y = 200.0;

    let drone_entity = ctx
      .world
      .spawn((
        Transform::from_xy(spawn_x, spawn_y),
        Renderable::sprite(sprite)
          .with_z_order(Z_DRONE)
          // Start with dissolve shader for spawn effect (black edge)
          .with_shader(SpriteShader::dissolve_with_edge(1.0, Color::rgb(0, 0, 0))),
        Interpolated,
        RigidBody::kinematic(),
        Collider::box_collider(DRONE_SIZE, DRONE_SIZE),
        Drone::new(spawn_x, spawn_y),
      ))
      .id();
    self.drones.push(drone_entity);
  }

  pub(super) fn shoot_bullet(&mut self, ctx: &mut SceneContext, x: f32, y: f32, side: TurretSide) {
    let Some(player_entity) = self.player else {
      return;
    };

    let (target_x, target_y) = {
      let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
        return;
      };

      // Calculate direction to player with randomness
      let mut rng = rand::thread_rng();
      (
        player_transform.translation.x + rng.gen_range(-50.0..50.0),
        player_transform.translation.y + rng.gen_range(-50.0..50.0),
      )
    };

    let dx = target_x - x;
    let dy = target_y - y;
    let length = (dx * dx + dy * dy).sqrt();

    let (velocity_x, velocity_y) = if length > 0.0 {
      (dx / length * BULLET_SPEED, dy / length * BULLET_SPEED)
    } else {
      let direction = match side {
        TurretSide::Left => 1.0,
        TurretSide::Right => -1.0,
      };
      (direction * BULLET_SPEED, 0.0)
    };

    // Use bullet pool for zero-allocation spawning
    if let Some(ref mut pool) = self.bullet_pool
      && let Some(bullet) = pool.acquire(ctx.world)
    {
      // Configure the pooled bullet entity (acquire stops it sliding from
      // where it was last used)
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(bullet) {
        transform.translation = Vec2::new(x, y);
      }
      // Set velocity for Rapier physics
      if let Some(mut vel) = ctx.world.get_mut::<Velocity>(bullet) {
        vel.dx = velocity_x;
        vel.dy = velocity_y;
      }
      if let Some(mut b) = ctx.world.get_mut::<Bullet>(bullet) {
        b.velocity_x = velocity_x;
        b.velocity_y = velocity_y;
      }
      // Clear trail from previous use
      if let Some(mut trail) = ctx.world.get_mut::<Trail2D>(bullet) {
        trail.clear();
      }
      self.bullets.push(bullet);
      return;
    }

    // Fallback: direct spawning if pool unavailable
    let sprite = self.bullet_sprite.clone();
    let bullet = ctx
      .world
      .spawn((
        Transform::from_xy(x, y),
        Renderable::sprite(sprite).with_z_order(Z_BULLET),
        Interpolated,
        // CCD for fast-moving bullets, no gravity
        RigidBody::dynamic()
          .with_ccd()
          .with_gravity_scale(0.0)
          .lock_rotation(),
        Collider::circle_collider(BULLET_SIZE / 2.0).as_trigger(),
        Velocity::new(velocity_x, velocity_y),
        Bullet::new(velocity_x, velocity_y),
        Trail2D::new(Color::new(255, 140, 30, 230), 3.5)
          .with_z_order(Z_BULLET)
          .with_length(25)
          .with_taper(0.7)
          .with_fade(true)
          .with_min_distance(2.0),
      ))
      .id();

    self.bullets.push(bullet);
  }

  pub(super) fn create_explosion(&mut self, ctx: &mut SceneContext, x: f32, y: f32) {
    let mut rng = rand::thread_rng();
    let sprite = self.particle_sprite.clone();

    for _ in 0..PARTICLES_PER_EXPLOSION {
      let angle = rng.gen_range(0.0..std::f32::consts::TAU);
      let speed = rng.gen_range(PARTICLE_SPEED_MIN..PARTICLE_SPEED_MAX);
      let velocity_x = angle.cos() * speed;
      let velocity_y = angle.sin() * speed;

      let particle_entity = ctx
        .world
        .spawn((
          Transform::from_xy(x, y),
          Renderable::sprite(sprite.clone()).with_z_order(Z_PARTICLE),
          Collider::circle_collider(PARTICLE_SIZE / 2.0).as_trigger(),
          Particle::new(velocity_x, velocity_y, PARTICLE_LIFETIME),
        ))
        .id();
      self.particles.push(particle_entity);
    }
  }

  pub(super) fn spawn_additional_turrets(&mut self, ctx: &mut SceneContext, count: u32) {
    let sprite = self.turret_sprite.clone();
    let mut rng = rand::thread_rng();
    let sh = self.screen_height;
    let sw = self.screen_width;

    // Stagger offset between sides so sounds play back-to-back
    let stagger_delay = 1.3;

    for _ in 0..count {
      // Randomize which side spawns first for each pair
      let left_first = rng.gen_bool(0.5);

      // Left side turret - flush against left wall
      let y = rng.gen_range(100.0..(sh - 100.0)) + TURRET_SIZE / 2.0;
      let left_turret = ctx
        .world
        .spawn((
          Transform::from_xy(TURRET_SIZE / 2.0, y),
          Renderable::sprite(sprite.clone())
            .with_z_order(Z_TURRET)
            .with_shader(SpriteShader::outline(Color::rgb(255, 140, 140), 2)),
          RigidBody::kinematic(),
          Collider::box_collider(TURRET_SIZE, TURRET_SIZE),
          Turret::new(TurretSide::Left),
        ))
        .id();
      // Add stagger delay if right side spawns first
      if !left_first && let Some(mut turret) = ctx.world.get_mut::<Turret>(left_turret) {
        turret.spawn_timer += stagger_delay;
      }
      self.turrets.push(left_turret);

      // Right side turret - flush against right wall
      let y = rng.gen_range(100.0..(sh - 100.0)) + TURRET_SIZE / 2.0;
      let right_turret = ctx
        .world
        .spawn((
          Transform::from_xy(sw - TURRET_SIZE / 2.0, y),
          Renderable::sprite(sprite.clone())
            .with_z_order(Z_TURRET)
            .with_shader(SpriteShader::outline(Color::rgb(255, 140, 140), 2)),
          RigidBody::kinematic(),
          Collider::box_collider(TURRET_SIZE, TURRET_SIZE),
          Turret::new(TurretSide::Right),
        ))
        .id();
      // Add stagger delay if left side spawns first
      if left_first && let Some(mut turret) = ctx.world.get_mut::<Turret>(right_turret) {
        turret.spawn_timer += stagger_delay;
      }
      self.turrets.push(right_turret);
    }
  }
}
