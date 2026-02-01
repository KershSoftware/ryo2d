//! Main game scene with all gameplay logic

use crate::components::*;
use crate::save::save_best_time;
use crate::*;
use rand::Rng;
use ryo2d::prelude::*;

use super::GameOverScene;

pub struct GameScene {
  player: Option<Entity>,
  turrets: Vec<Entity>,
  bullets: Vec<Entity>,
  particles: Vec<Entity>,
  platforms: Vec<Entity>,
  survival_time: f32,
  best_time: f32,
  player_lives: u32,
  player_invincible: bool,
  player_sprite: SpriteHandle,
  turret_sprite: SpriteHandle,
  bullet_sprite: SpriteHandle,
  particle_sprite: SpriteHandle,
  platform_sprite: SpriteHandle,
  // Progression tracking
  difficulty_level: u32,
  time_since_last_progression: f32,
  current_turret_speed: f32,
  current_burst_size: u32,
  // Death effect
  death_effect_timer: Option<f32>,
  // Screen dimensions (obtained from engine at runtime)
  screen_width: f32,
  screen_height: f32,
}

impl GameScene {
  pub fn new() -> Self {
    // Load sprites
    let player_sprite = Sprite::load("assets/player.png").expect("Failed to load player sprite");
    let turret_sprite = Sprite::load("assets/turret.png").expect("Failed to load turret sprite");
    let bullet_sprite = Sprite::load("assets/bullet.png").expect("Failed to load bullet sprite");
    let particle_sprite =
      Sprite::load("assets/particle.png").expect("Failed to load particle sprite");
    let platform_sprite =
      Sprite::load("assets/platform.png").expect("Failed to load platform sprite");

    Self {
      player: None,
      turrets: Vec::new(),
      bullets: Vec::new(),
      particles: Vec::new(),
      platforms: Vec::new(),
      survival_time: 0.0,
      best_time: save::load_best_time(),
      player_lives: PLAYER_MAX_LIVES,
      player_invincible: false,
      player_sprite: SpriteHandle::from_sprite(player_sprite, "player.png"),
      turret_sprite: SpriteHandle::from_sprite(turret_sprite, "turret.png"),
      bullet_sprite: SpriteHandle::from_sprite(bullet_sprite, "bullet.png"),
      particle_sprite: SpriteHandle::from_sprite(particle_sprite, "particle.png"),
      platform_sprite: SpriteHandle::from_sprite(platform_sprite, "platform.png"),
      // Progression starts at level 0
      difficulty_level: 0,
      time_since_last_progression: 0.0,
      current_turret_speed: TURRET_MOVE_SPEED,
      current_burst_size: TURRET_BURST_SIZE,
      // Death effect not active
      death_effect_timer: None,
      // Screen dimensions (set in on_enter from engine config)
      screen_width: 1920.0, // placeholder, updated in on_enter
      screen_height: 1080.0,
    }
  }

  fn spawn_player(&mut self, ctx: &mut SceneContext) {
    let sprite = self.player_sprite.clone();
    // Spawn player at center-bottom of screen
    let spawn_y = self.screen_height - 240.0; // Near bottom
    let player_entity = ctx.world.spawn((
      Transform::new(self.screen_width / 2.0, spawn_y),
      Renderable::sprite(sprite).with_z_order(Z_PLAYER),
      Collider::box_collider(PLAYER_SIZE, PLAYER_SIZE),
      PlayerVelocity::zero(),
      Player::new(PLAYER_MAX_LIVES),
    ));
    self.player = Some(player_entity);
  }

  fn spawn_platforms(&mut self, ctx: &mut SceneContext) {
    let sprite = self.platform_sprite.clone();
    let sw = self.screen_width;
    let sh = self.screen_height;

    // Ground platform - full width (x=0 since sprites render from top-left)
    let mut ground_transform = Transform::new(0.0, sh - 30.0);
    ground_transform.scale_x = sw / 800.0; // Scale from 800px base
    let ground = ctx.world.spawn((
      ground_transform,
      Renderable::sprite(sprite.clone()).with_z_order(Z_PLATFORM),
      Collider::box_collider(800.0, PLATFORM_THICKNESS),
      Platform,
    ));
    self.platforms.push(ground);

    // Floating platforms - scaled proportionally to screen size
    // Base design was for 2560x1440, scale positions accordingly
    let scale_x = sw / 2560.0;
    let scale_y = sh / 1440.0;

    // Platform configs: (center_x, y, width) in 2560x1440 base coordinates
    let platform_configs = vec![
      (400.0, 1270.0, 350.0),  // Left side
      (1280.0, 1270.0, 400.0), // Center
      (2160.0, 1270.0, 350.0), // Right side
      // Layer 2: y=1130 (140px up)
      (640.0, 1130.0, 350.0),  // Left-center
      (1920.0, 1130.0, 350.0), // Right-center
      // Layer 3: y=990 (140px up)
      (320.0, 990.0, 300.0),  // Far left
      (1280.0, 990.0, 400.0), // Center
      (2240.0, 990.0, 300.0), // Far right
      // Layer 4: y=850 (140px up)
      (800.0, 850.0, 350.0),  // Left-center
      (1760.0, 850.0, 350.0), // Right-center
      // Layer 5: y=710 (140px up)
      (400.0, 710.0, 350.0),  // Left side
      (1280.0, 710.0, 400.0), // Center
      (2160.0, 710.0, 350.0), // Right side
      // Layer 6: y=570 (140px up)
      (640.0, 570.0, 350.0),  // Left-center
      (1920.0, 570.0, 350.0), // Right-center
      // Layer 7: y=430 (140px up)
      (1280.0, 430.0, 400.0), // Center
      // Layer 8: y=290 (140px up)
      (800.0, 290.0, 350.0),  // Left-center
      (1760.0, 290.0, 350.0), // Right-center
      // Layer 9: y=150 (140px up)
      (1280.0, 150.0, 400.0), // Top center
    ];

    for (base_x, base_y, base_width) in platform_configs {
      // Scale positions and widths to current screen size
      let center_x = base_x * scale_x;
      let y = base_y * scale_y;
      let width = base_width * scale_x;

      // Convert center position to top-left for sprite rendering
      let left_x = center_x - width / 2.0;
      let mut transform = Transform::new(left_x, y);
      transform.scale_x = width / 800.0; // Scale from 800px base sprite
      let platform = ctx.world.spawn((
        transform,
        Renderable::sprite(sprite.clone()).with_z_order(Z_PLATFORM),
        // Collider uses base sprite dimensions; transform.scale_x applies to both
        // sprite rendering and collision detection
        Collider::box_collider(800.0, PLATFORM_THICKNESS),
        Platform,
      ));
      self.platforms.push(platform);
    }
  }

  fn spawn_turrets(&mut self, ctx: &mut SceneContext) {
    let sprite = self.turret_sprite.clone();
    let mut rng = rand::thread_rng();
    let sh = self.screen_height;
    let sw = self.screen_width;

    // Left side turrets
    for _ in 0..TURRETS_PER_SIDE {
      let y = rng.gen_range(100.0..(sh - 100.0));
      let turret = ctx.world.spawn((
        Transform::new(TURRET_SIZE / 2.0 + 20.0, y),
        Renderable::sprite(sprite.clone()).with_z_order(Z_TURRET),
        Collider::box_collider(TURRET_SIZE, TURRET_SIZE),
        Turret::new(TurretSide::Left),
      ));
      self.turrets.push(turret);
    }

    // Right side turrets
    for _ in 0..TURRETS_PER_SIDE {
      let y = rng.gen_range(100.0..(sh - 100.0));
      let turret = ctx.world.spawn((
        Transform::new(sw - TURRET_SIZE / 2.0 - 20.0, y),
        Renderable::sprite(sprite.clone()).with_z_order(Z_TURRET),
        Collider::box_collider(TURRET_SIZE, TURRET_SIZE),
        Turret::new(TurretSide::Right),
      ));
      self.turrets.push(turret);
    }
  }

  fn update_player(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(player_entity) = self.player else {
      return;
    };

    // Get components
    let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) else {
      return;
    };
    let was_invincible = player.is_invincible();
    let was_grounded = player.is_grounded;
    player.update(dt);
    let is_invincible = player.is_invincible();
    drop(player);

    // Animate flash shader during invincibility, clear when it ends
    if is_invincible {
      // Get remaining invincibility time for animation
      let invincible_timer = ctx
        .world
        .get::<Player>(player_entity)
        .map(|p| p.invincible_timer)
        .unwrap_or(0.0);

      // Pulsing flash: oscillate intensity based on time, fading as timer decreases
      let pulse = (invincible_timer * 8.0).sin().abs(); // Fast pulse
      let fade = (invincible_timer / INVINCIBLE_DURATION).min(1.0); // Fade out over time
      let intensity = pulse * fade * 0.8 + 0.2 * fade; // Range from 0.2*fade to 1.0*fade

      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(player_entity) {
        renderable.shader = Some(SpriteShader::flash(Color::rgb(255, 0, 0), intensity));
      }
    } else if was_invincible {
      // Clear shader when invincibility just ended
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(player_entity) {
        renderable.shader = None;
      }
    }

    let Some(mut velocity) = ctx.world.get_mut::<PlayerVelocity>(player_entity) else {
      return;
    };

    // Horizontal movement
    let mut move_x = 0.0;
    if ctx.input.key_down(KeyCode::KeyA) || ctx.input.key_down(KeyCode::ArrowLeft) {
      move_x -= 1.0;
    }
    if ctx.input.key_down(KeyCode::KeyD) || ctx.input.key_down(KeyCode::ArrowRight) {
      move_x += 1.0;
    }
    velocity.x = move_x * PLAYER_SPEED;

    // Jumping
    if (ctx.input.key_pressed(KeyCode::Space) || ctx.input.key_pressed(KeyCode::KeyW))
      && was_grounded
    {
      velocity.y = -PLAYER_JUMP_FORCE;
      drop(velocity);
      if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
        player.is_grounded = false;
      }
    } else {
      drop(velocity);
    }

    // Apply gravity
    if let Some(mut velocity) = ctx.world.get_mut::<PlayerVelocity>(player_entity) {
      velocity.y += GRAVITY * dt;
    }

    // Apply velocity to transform
    let Some(velocity) = ctx.world.get::<PlayerVelocity>(player_entity) else {
      return;
    };
    let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) else {
      return;
    };

    transform.x += velocity.x * dt;
    transform.y += velocity.y * dt;

    // Screen bounds
    let half_size = PLAYER_SIZE / 2.0;
    transform.x = transform.x.clamp(half_size, self.screen_width - half_size);

    let _tx = transform.x;
    let ty = transform.y;
    drop(velocity);
    drop(transform);

    // Ground collision
    if ty >= self.screen_height - half_size {
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
        transform.y = self.screen_height - half_size;
      }
      if let Some(mut velocity) = ctx.world.get_mut::<PlayerVelocity>(player_entity) {
        velocity.y = 0.0;
      }
      if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
        player.is_grounded = true;
      }
    } else {
      // Check platform collisions
      self.check_player_platform_collision(ctx, player_entity);
    }
  }

  fn check_player_platform_collision(&mut self, ctx: &mut SceneContext, player_entity: Entity) {
    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return;
    };
    let Some(player_velocity) = ctx.world.get::<PlayerVelocity>(player_entity) else {
      return;
    };

    // Player sprite is drawn at (x, y) as top-left
    let player_left = player_transform.x;
    let player_right = player_transform.x + PLAYER_SIZE;
    let player_top = player_transform.y;
    let player_bottom = player_transform.y + PLAYER_SIZE;
    let falling = player_velocity.y > 0.0;

    drop(player_transform);
    drop(player_velocity);

    for &platform_entity in &self.platforms {
      let Some(platform_transform) = ctx.world.get::<Transform>(platform_entity) else {
        continue;
      };

      // Platform sprite is 800px base, scaled by transform.scale_x
      // Transform (x, y) is the TOP-LEFT corner of the sprite
      let platform_width = 800.0 * platform_transform.scale_x;

      let platform_left = platform_transform.x;
      let platform_right = platform_transform.x + platform_width;
      let platform_top = platform_transform.y;
      let platform_bottom = platform_transform.y + PLATFORM_THICKNESS;

      // Check AABB collision
      if player_right > platform_left
        && player_left < platform_right
        && player_bottom > platform_top
        && player_top < platform_bottom
      {
        // Landing on top of platform
        if falling && player_bottom <= platform_top + 15.0 {
          if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
            // Place player so their bottom edge sits on platform top
            transform.y = platform_top - PLAYER_SIZE;
          }
          if let Some(mut velocity) = ctx.world.get_mut::<PlayerVelocity>(player_entity) {
            velocity.y = 0.0;
          }
          if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
            player.is_grounded = true;
          }
          break;
        }
      }
    }
  }

  fn update_turrets(&mut self, ctx: &mut SceneContext, dt: f32) {
    let turrets_to_update: Vec<_> = self.turrets.iter().copied().collect();

    for turret_entity in turrets_to_update {
      let Some(mut transform) = ctx.world.get_mut::<Transform>(turret_entity) else {
        continue;
      };
      let Some(mut turret) = ctx.world.get_mut::<Turret>(turret_entity) else {
        continue;
      };

      // Move up and down using current (progression-scaled) speed
      transform.y += turret.move_direction * self.current_turret_speed * dt;

      // Bounce off screen edges
      let margin = TURRET_SIZE;
      if transform.y < margin || transform.y > self.screen_height - margin {
        turret.reverse_direction();
        transform.y = transform.y.clamp(margin, self.screen_height - margin);
      }

      let turret_x = transform.x;
      let turret_y = transform.y;
      let turret_side = turret.side;
      let max_burst = turret.max_burst_size;

      // Shooting logic
      if turret.burst_count > 0 {
        turret.burst_timer -= dt;
        if turret.burst_timer <= 0.0 {
          drop(transform);
          drop(turret);
          self.shoot_bullet(ctx, turret_x, turret_y, turret_side);
          if let Some(mut turret) = ctx.world.get_mut::<Turret>(turret_entity) {
            turret.burst_count -= 1;
            turret.burst_timer = TURRET_BURST_DELAY;
          }
        }
      } else {
        turret.shoot_timer -= dt;
        if turret.shoot_timer <= 0.0 {
          // Use the turret's max burst size (updated by progression)
          turret.burst_count = max_burst;
          turret.burst_timer = TURRET_BURST_DELAY;
          turret.shoot_timer = TURRET_SHOOT_INTERVAL;
        }
      }
    }
  }

  fn shoot_bullet(&mut self, ctx: &mut SceneContext, x: f32, y: f32, side: TurretSide) {
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
        player_transform.x + rng.gen_range(-50.0..50.0),
        player_transform.y + rng.gen_range(-50.0..50.0),
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

    let sprite = self.bullet_sprite.clone();
    let bullet = ctx.world.spawn((
      Transform::new(x, y),
      Renderable::sprite(sprite).with_z_order(Z_BULLET),
      Collider::circle_collider(BULLET_SIZE / 2.0).as_trigger(),
      Bullet::new(velocity_x, velocity_y),
    ));

    self.bullets.push(bullet);
  }

  fn update_bullets(&mut self, ctx: &mut SceneContext, dt: f32) {
    let mut bullets_to_remove = Vec::new();
    let bullets_to_process: Vec<_> = self.bullets.iter().copied().enumerate().collect();

    for (i, bullet_entity) in bullets_to_process {
      let Some(mut transform) = ctx.world.get_mut::<Transform>(bullet_entity) else {
        bullets_to_remove.push(i);
        continue;
      };
      let Some(bullet) = ctx.world.get::<Bullet>(bullet_entity) else {
        bullets_to_remove.push(i);
        continue;
      };

      // Move bullet
      transform.x += bullet.velocity_x * dt;
      transform.y += bullet.velocity_y * dt;

      let bx = transform.x;
      let by = transform.y;

      drop(transform);
      drop(bullet);

      // Check if out of bounds
      if bx < -50.0 || bx > self.screen_width + 50.0 || by < -50.0 || by > self.screen_height + 50.0
      {
        bullets_to_remove.push(i);
        ctx.world.despawn(bullet_entity).ok();
        continue;
      }

      // Check collision with player
      if self.check_bullet_player_collision(ctx, bx, by) {
        self.create_explosion(ctx, bx, by);
        bullets_to_remove.push(i);
        ctx.world.despawn(bullet_entity).ok();
        continue;
      }

      // Check collision with platforms
      if self.check_bullet_platform_collision(ctx, bx, by) {
        self.create_explosion(ctx, bx, by);
        bullets_to_remove.push(i);
        ctx.world.despawn(bullet_entity).ok();
      }
    }

    // Remove bullets in reverse order
    for i in bullets_to_remove.iter().rev() {
      self.bullets.swap_remove(*i);
    }
  }

  fn check_bullet_player_collision(&mut self, ctx: &mut SceneContext, bx: f32, by: f32) -> bool {
    let Some(player_entity) = self.player else {
      return false;
    };

    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return false;
    };

    let dx = bx - player_transform.x;
    let dy = by - player_transform.y;
    let dist_sq = dx * dx + dy * dy;
    let collision_dist = (PLAYER_SIZE + BULLET_SIZE) / 2.0;

    if dist_sq < collision_dist * collision_dist {
      drop(player_transform);
      if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
        player.take_damage();
      }
      // Apply flash shader effect when hit
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(player_entity) {
        renderable.shader = Some(SpriteShader::flash(Color::rgb(255, 0, 0), 0.7));
      }
      return true;
    }

    false
  }

  fn check_bullet_platform_collision(&self, ctx: &mut SceneContext, bx: f32, by: f32) -> bool {
    let bullet_half = BULLET_SIZE / 2.0;

    for &platform_entity in &self.platforms {
      let Some(platform_transform) = ctx.world.get::<Transform>(platform_entity) else {
        continue;
      };

      // Platform sprite is 800px base, scaled by transform.scale_x
      let platform_width = 800.0 * platform_transform.scale_x;
      let platform_half_width = platform_width / 2.0;
      let platform_half_height = PLATFORM_THICKNESS / 2.0;

      let platform_left = platform_transform.x - platform_half_width;
      let platform_right = platform_transform.x + platform_half_width;
      let platform_top = platform_transform.y - platform_half_height;
      let platform_bottom = platform_transform.y + platform_half_height;

      if bx + bullet_half > platform_left
        && bx - bullet_half < platform_right
        && by + bullet_half > platform_top
        && by - bullet_half < platform_bottom
      {
        return true;
      }
    }

    false
  }

  fn create_explosion(&mut self, ctx: &mut SceneContext, x: f32, y: f32) {
    let mut rng = rand::thread_rng();
    let sprite = self.particle_sprite.clone();

    for _ in 0..PARTICLES_PER_EXPLOSION {
      let angle = rng.gen_range(0.0..std::f32::consts::TAU);
      let speed = rng.gen_range(PARTICLE_SPEED_MIN..PARTICLE_SPEED_MAX);
      let velocity_x = angle.cos() * speed;
      let velocity_y = angle.sin() * speed;

      let particle_entity = ctx.world.spawn((
        Transform::new(x, y),
        Renderable::sprite(sprite.clone()).with_z_order(Z_PARTICLE),
        Collider::circle_collider(PARTICLE_SIZE / 2.0).as_trigger(),
        Particle::new(velocity_x, velocity_y, PARTICLE_LIFETIME),
      ));
      self.particles.push(particle_entity);
    }
  }

  fn update_particles(&mut self, ctx: &mut SceneContext, dt: f32) {
    let mut particles_to_remove = Vec::new();

    for (i, &particle_entity) in self.particles.iter().enumerate() {
      let Some(mut particle) = ctx.world.get_mut::<Particle>(particle_entity) else {
        particles_to_remove.push(i);
        continue;
      };

      // Update lifetime
      particle.lifetime -= dt;
      if !particle.is_alive() {
        drop(particle);
        particles_to_remove.push(i);
        ctx.world.despawn(particle_entity).ok();
        continue;
      }

      let vx = particle.velocity_x;
      let vy = particle.velocity_y;
      let _alpha = (particle.alpha() * 255.0) as u8;

      drop(particle);

      // Move particle
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(particle_entity) {
        transform.x += vx * dt;
        transform.y += vy * dt;
      }

      // TODO: Fade out particles based on lifetime (needs sprite tint API)
    }

    // Remove particles in reverse order
    for i in particles_to_remove.iter().rev() {
      self.particles.swap_remove(*i);
    }
  }

  fn check_game_over(&self, ctx: &mut SceneContext) -> bool {
    if let Some(player_entity) = self.player {
      if let Some(player) = ctx.world.get::<Player>(player_entity) {
        return !player.is_alive();
      }
    }
    false
  }

  fn update_progression(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.time_since_last_progression += dt;

    // Check if we should increase difficulty
    if self.difficulty_level < PROGRESSION_STEPS
      && self.time_since_last_progression >= PROGRESSION_INTERVAL
    {
      self.time_since_last_progression = 0.0;
      self.difficulty_level += 1;

      // Calculate progression factor (0.0 to 1.0)
      let progress = self.difficulty_level as f32 / PROGRESSION_STEPS as f32;

      // Increase turret movement speed
      self.current_turret_speed =
        TURRET_MOVE_SPEED + (TURRET_MOVE_SPEED_MAX - TURRET_MOVE_SPEED) * progress;

      // Increase burst size
      self.current_burst_size =
        TURRET_BURST_SIZE + ((TURRET_BURST_SIZE_MAX - TURRET_BURST_SIZE) as f32 * progress) as u32;

      // Add more turrets (one per side every 2 difficulty levels)
      let turrets_per_side =
        TURRETS_PER_SIDE + ((TURRETS_PER_SIDE_MAX - TURRETS_PER_SIDE) as f32 * progress) as u32;
      let current_per_side = self.turrets.len() as u32 / 2;

      if turrets_per_side > current_per_side {
        let new_turrets = turrets_per_side - current_per_side;
        self.spawn_additional_turrets(ctx, new_turrets);
      }

      // Update existing turrets with new burst size
      for &turret_entity in &self.turrets {
        if let Some(mut turret) = ctx.world.get_mut::<Turret>(turret_entity) {
          turret.max_burst_size = self.current_burst_size;
        }
      }
    }
  }

  fn spawn_additional_turrets(&mut self, ctx: &mut SceneContext, count: u32) {
    let sprite = self.turret_sprite.clone();
    let mut rng = rand::thread_rng();
    let sh = self.screen_height;
    let sw = self.screen_width;

    for _ in 0..count {
      // Left side turret
      let y = rng.gen_range(100.0..(sh - 100.0));
      let turret = ctx.world.spawn((
        Transform::new(TURRET_SIZE / 2.0 + 20.0, y),
        Renderable::sprite(sprite.clone()).with_z_order(Z_TURRET),
        Collider::box_collider(TURRET_SIZE, TURRET_SIZE),
        Turret::new(TurretSide::Left),
      ));
      self.turrets.push(turret);

      // Right side turret
      let y = rng.gen_range(100.0..(sh - 100.0));
      let turret = ctx.world.spawn((
        Transform::new(sw - TURRET_SIZE / 2.0 - 20.0, y),
        Renderable::sprite(sprite.clone()).with_z_order(Z_TURRET),
        Collider::box_collider(TURRET_SIZE, TURRET_SIZE),
        Turret::new(TurretSide::Right),
      ));
      self.turrets.push(turret);
    }
  }
}

impl Scene for GameScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Capture screen dimensions from engine
    self.screen_width = ctx.screen_width();
    self.screen_height = ctx.screen_height();

    // Clear world
    ctx.world.clear();

    // Reset state
    self.survival_time = 0.0;
    self.player = None;
    self.turrets.clear();
    self.bullets.clear();
    self.particles.clear();
    self.platforms.clear();

    // Spawn game entities
    self.spawn_platforms(ctx);
    self.spawn_turrets(ctx);
    self.spawn_player(ctx);

    // Center camera
    ctx
      .camera
      .set_position(self.screen_width / 2.0, self.screen_height / 2.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    self.survival_time += dt;

    // Update progression
    self.update_progression(ctx, dt);

    // Update player state cache for UI
    if let Some(player_entity) = self.player {
      if let Some(player) = ctx.world.get::<Player>(player_entity) {
        self.player_lives = player.lives;
        self.player_invincible = player.is_invincible();
      }
    }

    // Handle death effect animation
    if let Some(timer) = self.death_effect_timer {
      let new_timer = timer - dt;

      // Animate chromatic aberration: quick pulse in and out
      const DEATH_EFFECT_DURATION: f32 = 0.6;
      let progress = 1.0 - (new_timer / DEATH_EFFECT_DURATION).clamp(0.0, 1.0);
      // Sine curve for smooth pulse: rises to peak at 0.5, falls back to 0
      let intensity = (progress * std::f32::consts::PI).sin();
      let strength = intensity * 0.015; // Max chromatic aberration strength
      ctx.effects.set_chromatic_aberration(strength);

      if new_timer <= 0.0 {
        // Effect complete, transition to game over
        ctx.effects.clear();
        save_best_time(self.survival_time);
        return SceneAction::Switch(Box::new(GameOverScene::new(self.survival_time)));
      }
      self.death_effect_timer = Some(new_timer);

      // During death effect, still render but don't update gameplay
      return SceneAction::Continue;
    }

    self.update_player(ctx, dt);
    self.update_turrets(ctx, dt);
    self.update_bullets(ctx, dt);
    self.update_particles(ctx, dt);

    // Check game over - start death effect instead of immediate transition
    if self.check_game_over(ctx) {
      const DEATH_EFFECT_DURATION: f32 = 0.6;
      self.death_effect_timer = Some(DEATH_EFFECT_DURATION);
      return SceneAction::Continue;
    }

    // ESC to return to menu
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Switch(Box::new(super::MenuScene::new()));
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, _screen_h) = ui.screen_size();

    // Use cached player state
    let lives = self.player_lives;
    let invincible = self.player_invincible;

    // Lives
    let lives_text = format!("Lives: {}", lives);
    ui.draw_text(&lives_text, 20.0, 20.0, Color::rgb(255, 100, 100));

    // Survival time
    let time_text = format!("Time: {:.1}s", self.survival_time);
    ui.draw_text(&time_text, 20.0, 50.0, Color::rgb(100, 255, 100));

    // Entity counts
    let bullet_count = self.bullets.len();
    let particle_count = self.particles.len();
    let total = bullet_count + particle_count;
    let counts_text = format!(
      "Bullets: {}  Particles: {}  Total: {}",
      bullet_count, particle_count, total
    );
    ui.draw_text(&counts_text, 20.0, 80.0, Color::rgb(150, 150, 200));

    // Hit indicator
    if invincible {
      let hit_text = "HIT!";
      let (hit_w, _) = ui.text_size(hit_text);
      ui.draw_text(
        hit_text,
        screen_w as f32 / 2.0 - hit_w as f32 / 2.0,
        200.0,
        Color::rgb(255, 50, 50),
      );
    }
  }
}
