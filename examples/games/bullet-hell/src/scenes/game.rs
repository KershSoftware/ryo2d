//! Main game scene with all gameplay logic

use crate::components::*;
use crate::save::save_best_time;
use crate::*;
use rand::Rng;
use ryo2d::prelude::*;

use super::{GameOverScene, MenuScene};

// Fixed design resolution - game logic always uses these values
// Display settings only affect window scaling, not gameplay
const DESIGN_WIDTH: f32 = 1920.0;
const DESIGN_HEIGHT: f32 = 1080.0;

pub struct GameScene {
  player: Option<Entity>,
  turrets: Vec<Entity>,
  bullets: Vec<Entity>,
  particles: Vec<Entity>,
  platforms: Vec<Entity>,
  barriers: Vec<Entity>,
  survival_time: f32,
  #[allow(dead_code)]
  best_time: f32,
  player_lives: u32,
  player_invincible: bool,
  player_sprite: SpriteHandle,
  turret_sprite: SpriteHandle,
  bullet_sprite: SpriteHandle,
  particle_sprite: SpriteHandle,
  platform_sprite: SpriteHandle,
  barrier_sprite: SpriteHandle,
  bg_sprite: SpriteHandle,
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
  // Pause menu state
  is_paused: bool,
  settings_open: bool,
  selected_display_mode_idx: usize,
  quit_to_menu: bool,
  quit_to_desktop: bool,
  // Flag to apply settings (set in render_ui, processed in update)
  apply_settings: bool,
}

impl Default for GameScene {
  fn default() -> Self {
    Self::new()
  }
}

impl GameScene {
  pub fn new() -> Self {
    // Load sprites
    let player_sprite = Sprite::load("assets/player.png").expect("Failed to load player sprite");
    let turret_sprite = Sprite::load("assets/turret.png").expect("Failed to load turret sprite");
    // Create a small round bullet programmatically (dark red/maroon)
    let bullet_sprite = Sprite::solid_circle(BULLET_SIZE as u32, 80, 20, 20, 255);
    let particle_sprite =
      Sprite::load("assets/particle.png").expect("Failed to load particle sprite");
    let platform_sprite =
      Sprite::load("assets/platform.png").expect("Failed to load platform sprite");
    // Create barrier sprite programmatically (dark gray rectangle)
    let barrier_sprite =
      Sprite::solid_rect(BARRIER_WIDTH as u32, BARRIER_HEIGHT as u32, 60, 60, 70, 255);
    let bg_sprite = Sprite::load("assets/bg.png").expect("Failed to load background sprite");

    // Load current display mode setting
    let display_mode_idx = Self::load_current_display_mode_index();

    Self {
      player: None,
      turrets: Vec::new(),
      bullets: Vec::new(),
      particles: Vec::new(),
      platforms: Vec::new(),
      barriers: Vec::new(),
      survival_time: 0.0,
      best_time: save::load_best_time(),
      player_lives: PLAYER_MAX_LIVES,
      player_invincible: false,
      player_sprite: SpriteHandle::from_sprite(player_sprite, "player.png"),
      turret_sprite: SpriteHandle::from_sprite(turret_sprite, "turret.png"),
      bullet_sprite: SpriteHandle::from_sprite(bullet_sprite, "bullet.png"),
      particle_sprite: SpriteHandle::from_sprite(particle_sprite, "particle.png"),
      platform_sprite: SpriteHandle::from_sprite(platform_sprite, "platform.png"),
      barrier_sprite: SpriteHandle::from_sprite(barrier_sprite, "barrier"),
      bg_sprite: SpriteHandle::from_sprite(bg_sprite, "bg.png"),
      // Progression starts at level 0
      difficulty_level: 0,
      time_since_last_progression: 0.0,
      current_turret_speed: TURRET_MOVE_SPEED,
      current_burst_size: TURRET_BURST_SIZE,
      // Death effect not active
      death_effect_timer: None,
      // Screen dimensions - fixed design resolution for consistent gameplay
      screen_width: DESIGN_WIDTH,
      screen_height: DESIGN_HEIGHT,
      // Pause menu state
      is_paused: false,
      settings_open: false,
      selected_display_mode_idx: display_mode_idx,
      quit_to_menu: false,
      quit_to_desktop: false,
      apply_settings: false,
    }
  }

  /// Load current display mode from system.json
  fn load_current_display_mode_index() -> usize {
    let Ok(workspace) = Workspace::discover() else {
      return 0; // Fallback: Windowed
    };

    let Ok(save_manager) = workspace.create_save_manager() else {
      return 0;
    };

    let system_state = save_manager
      .load_or_create_system(&workspace.config().system)
      .unwrap_or_else(|_| SystemState::from_defaults(&workspace.config().system));

    // Find display mode index
    let modes = DisplayMode::all();
    modes
      .iter()
      .position(|m| *m == system_state.display.display_mode)
      .unwrap_or(0) // Default to Windowed if not found
  }

  fn spawn_player(&mut self, ctx: &mut SceneContext) {
    let sprite = self.player_sprite.clone();
    // Spawn player at center-bottom of screen
    let spawn_y = self.screen_height - 240.0; // Near bottom
    let player_entity = ctx.world.spawn((
      Transform::new(self.screen_width / 2.0, spawn_y),
      Renderable::sprite(sprite)
        .with_z_order(Z_PLAYER)
        .with_shader(SpriteShader::outline(Color::rgb(40, 40, 40), 2)),
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

    // Left side turrets - flush against left wall
    for _ in 0..TURRETS_PER_SIDE {
      let y = rng.gen_range(100.0..(sh - 100.0));
      let turret = ctx.world.spawn((
        Transform::new(0.0, y),
        Renderable::sprite(sprite.clone())
          .with_z_order(Z_TURRET)
          .with_shader(SpriteShader::outline(Color::rgb(40, 40, 40), 2)),
        Collider::box_collider(TURRET_SIZE, TURRET_SIZE),
        Turret::new(TurretSide::Left),
      ));
      self.turrets.push(turret);
    }

    // Right side turrets - flush against right wall
    for _ in 0..TURRETS_PER_SIDE {
      let y = rng.gen_range(100.0..(sh - 100.0));
      let turret = ctx.world.spawn((
        Transform::new(sw - TURRET_SIZE, y),
        Renderable::sprite(sprite.clone())
          .with_z_order(Z_TURRET)
          .with_shader(SpriteShader::outline(Color::rgb(40, 40, 40), 2)),
        Collider::box_collider(TURRET_SIZE, TURRET_SIZE),
        Turret::new(TurretSide::Right),
      ));
      self.turrets.push(turret);
    }
  }

  fn spawn_barriers(&mut self, ctx: &mut SceneContext) {
    let sprite = self.barrier_sprite.clone();
    let center_x = self.screen_width / 2.0;

    // Vertical movement bounds (avoid ground and top)
    let min_y = 150.0;
    let max_y = self.screen_height - BARRIER_HEIGHT - 100.0;

    // Left barrier - left of center, moving down initially
    let left_barrier = ctx.world.spawn((
      Transform::new(center_x - 250.0, min_y + 100.0),
      Renderable::sprite(sprite.clone())
        .with_z_order(Z_BARRIER)
        .with_shader(SpriteShader::outline(Color::rgb(80, 80, 90), 2)),
      Barrier::new(BARRIER_SPEED, min_y, max_y),
    ));
    self.barriers.push(left_barrier);

    // Right barrier - right of center, moving up initially (mirrored)
    let right_barrier = ctx.world.spawn((
      Transform::new(center_x + 250.0 - BARRIER_WIDTH, max_y - 100.0),
      Renderable::sprite(sprite.clone())
        .with_z_order(Z_BARRIER)
        .with_shader(SpriteShader::outline(Color::rgb(80, 80, 90), 2)),
      Barrier::new(-BARRIER_SPEED, min_y, max_y),
    ));
    self.barriers.push(right_barrier);
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

    // Screen bounds (sprite draws from top-left, so clamp to keep fully on screen)
    transform.x = transform.x.clamp(0.0, self.screen_width - PLAYER_SIZE);

    let _tx = transform.x;
    let ty = transform.y;
    drop(velocity);
    drop(transform);

    // Ground collision (sprite draws from top-left, so bottom edge is at y + PLAYER_SIZE)
    if ty >= self.screen_height - PLAYER_SIZE - 30.0 {
      // -30 accounts for ground platform thickness
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
        transform.y = self.screen_height - PLAYER_SIZE - 30.0;
      }
      if let Some(mut velocity) = ctx.world.get_mut::<PlayerVelocity>(player_entity) {
        velocity.y = 0.0;
      }
      if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
        player.is_grounded = true;
      }
    } else {
      // Check platform and barrier collisions
      self.check_player_platform_collision(ctx, player_entity);
      self.check_player_barrier_collision(ctx, player_entity);
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

  fn check_player_barrier_collision(&mut self, ctx: &mut SceneContext, player_entity: Entity) {
    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return;
    };
    let Some(player_velocity) = ctx.world.get::<PlayerVelocity>(player_entity) else {
      return;
    };

    let player_left = player_transform.x;
    let player_right = player_transform.x + PLAYER_SIZE;
    let player_top = player_transform.y;
    let player_bottom = player_transform.y + PLAYER_SIZE;
    let vel_x = player_velocity.x;
    let vel_y = player_velocity.y;

    drop(player_transform);
    drop(player_velocity);

    for &barrier_entity in &self.barriers {
      let Some(barrier_transform) = ctx.world.get::<Transform>(barrier_entity) else {
        continue;
      };

      let barrier_left = barrier_transform.x;
      let barrier_right = barrier_transform.x + BARRIER_WIDTH;
      let barrier_top = barrier_transform.y;
      let barrier_bottom = barrier_transform.y + BARRIER_HEIGHT;

      // Check AABB collision
      if player_right > barrier_left
        && player_left < barrier_right
        && player_bottom > barrier_top
        && player_top < barrier_bottom
      {
        // Push player out of barrier based on movement direction
        let overlap_left = player_right - barrier_left;
        let overlap_right = barrier_right - player_left;
        let overlap_top = player_bottom - barrier_top;
        let overlap_bottom = barrier_bottom - player_top;

        // Find smallest overlap to determine push direction
        let min_overlap = overlap_left
          .min(overlap_right)
          .min(overlap_top)
          .min(overlap_bottom);

        if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
          if min_overlap == overlap_left && vel_x > 0.0 {
            // Push left
            transform.x = barrier_left - PLAYER_SIZE;
          } else if min_overlap == overlap_right && vel_x < 0.0 {
            // Push right
            transform.x = barrier_right;
          } else if min_overlap == overlap_top && vel_y > 0.0 {
            // Push up (landing on top)
            transform.y = barrier_top - PLAYER_SIZE;
            drop(transform);
            if let Some(mut velocity) = ctx.world.get_mut::<PlayerVelocity>(player_entity) {
              velocity.y = 0.0;
            }
            if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
              player.is_grounded = true;
            }
          } else if min_overlap == overlap_bottom && vel_y < 0.0 {
            // Push down (hitting from below)
            transform.y = barrier_bottom;
            drop(transform);
            if let Some(mut velocity) = ctx.world.get_mut::<PlayerVelocity>(player_entity) {
              velocity.y = 0.0;
            }
          }
        }
        break;
      }
    }
  }

  fn update_barriers(&mut self, ctx: &mut SceneContext, dt: f32) {
    for &barrier_entity in &self.barriers {
      let Some(mut transform) = ctx.world.get_mut::<Transform>(barrier_entity) else {
        continue;
      };
      let Some(mut barrier) = ctx.world.get_mut::<Barrier>(barrier_entity) else {
        continue;
      };

      // Move barrier
      transform.y += barrier.velocity_y * dt;

      // Bounce at bounds
      if transform.y <= barrier.min_y {
        transform.y = barrier.min_y;
        barrier.velocity_y = barrier.velocity_y.abs();
      } else if transform.y >= barrier.max_y {
        transform.y = barrier.max_y;
        barrier.velocity_y = -barrier.velocity_y.abs();
      }
    }
  }

  fn update_turrets(&mut self, ctx: &mut SceneContext, dt: f32) {
    let turrets_to_update: Vec<_> = self.turrets.to_vec();

    // First pass: collect turret positions and sides for collision detection
    let mut turret_data: Vec<(Entity, f32, TurretSide, f32)> = Vec::new(); // entity, y, side, direction
    for &turret_entity in &turrets_to_update {
      if let (Some(transform), Some(turret)) = (
        ctx.world.get::<Transform>(turret_entity),
        ctx.world.get::<Turret>(turret_entity),
      ) {
        turret_data.push((
          turret_entity,
          transform.y,
          turret.side,
          turret.move_direction,
        ));
      }
    }

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

      // Check collision with other turrets on the same side
      let my_y = transform.y;
      let my_side = turret.side;
      let my_dir = turret.move_direction;

      for &(other_entity, other_y, other_side, _) in &turret_data {
        if other_entity == turret_entity || other_side != my_side {
          continue;
        }

        // Check if turrets overlap (within TURRET_SIZE of each other)
        let dist = (my_y - other_y).abs();
        if dist < TURRET_SIZE {
          // Only bounce if moving towards the other turret
          let moving_towards = (my_y < other_y && my_dir > 0.0) || (my_y > other_y && my_dir < 0.0);

          if moving_towards {
            turret.reverse_direction();
            // Push apart slightly to prevent sticking
            let push = (TURRET_SIZE - dist) / 2.0 + 1.0;
            if my_y < other_y {
              transform.y -= push;
            } else {
              transform.y += push;
            }
            break;
          }
        }
      }

      let turret_x = transform.x + TURRET_SIZE / 2.0; // Shoot from center of turret
      let turret_y = transform.y + TURRET_SIZE / 2.0;
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
      // Center the bullet sprite using anchor
      Transform::new(x, y).with_anchor(Anchor::Center),
      Renderable::sprite(sprite)
        .with_z_order(Z_BULLET)
        .with_shader(SpriteShader::outline(Color::rgb(255, 220, 50), 1)),
      Collider::circle_collider(BULLET_SIZE / 2.0).as_trigger(),
      Bullet::new(velocity_x, velocity_y),
      // Trail effect for bullets - dark red trail that tapers to nothing
      Trail2D::bullet(Color::rgb(150, 50, 50))
        .with_length(10)
        .with_taper(0.0)
        .with_fade(true),
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

      // Update trail with new position
      if let Some(mut trail) = ctx.world.get_mut::<Trail2D>(bullet_entity) {
        trail.update(bx, by, dt);
      }

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
        continue;
      }

      // Check collision with barriers
      if self.check_bullet_barrier_collision(ctx, bx, by) {
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

  fn check_bullet_barrier_collision(&self, ctx: &mut SceneContext, bx: f32, by: f32) -> bool {
    let bullet_half = BULLET_SIZE / 2.0;

    for &barrier_entity in &self.barriers {
      let Some(barrier_transform) = ctx.world.get::<Transform>(barrier_entity) else {
        continue;
      };

      // Barrier drawn from top-left
      let barrier_left = barrier_transform.x;
      let barrier_right = barrier_transform.x + BARRIER_WIDTH;
      let barrier_top = barrier_transform.y;
      let barrier_bottom = barrier_transform.y + BARRIER_HEIGHT;

      if bx + bullet_half > barrier_left
        && bx - bullet_half < barrier_right
        && by + bullet_half > barrier_top
        && by - bullet_half < barrier_bottom
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
      // Left side turret - flush against left wall
      let y = rng.gen_range(100.0..(sh - 100.0));
      let turret = ctx.world.spawn((
        Transform::new(0.0, y),
        Renderable::sprite(sprite.clone())
          .with_z_order(Z_TURRET)
          .with_shader(SpriteShader::outline(Color::rgb(40, 40, 40), 2)),
        Collider::box_collider(TURRET_SIZE, TURRET_SIZE),
        Turret::new(TurretSide::Left),
      ));
      self.turrets.push(turret);

      // Right side turret - flush against right wall
      let y = rng.gen_range(100.0..(sh - 100.0));
      let turret = ctx.world.spawn((
        Transform::new(sw - TURRET_SIZE, y),
        Renderable::sprite(sprite.clone())
          .with_z_order(Z_TURRET)
          .with_shader(SpriteShader::outline(Color::rgb(40, 40, 40), 2)),
        Collider::box_collider(TURRET_SIZE, TURRET_SIZE),
        Turret::new(TurretSide::Right),
      ));
      self.turrets.push(turret);
    }
  }
}

impl Scene for GameScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Use fixed design resolution for consistent gameplay
    // Display settings only affect window scaling, not game logic
    self.screen_width = DESIGN_WIDTH;
    self.screen_height = DESIGN_HEIGHT;

    // Clear world
    ctx.world.clear();

    // Reset state
    self.survival_time = 0.0;
    self.player = None;
    self.turrets.clear();
    self.bullets.clear();
    self.particles.clear();
    self.platforms.clear();
    self.barriers.clear();

    // Spawn background (scaled to fill screen)
    let bg = self.bg_sprite.clone();
    let (bg_width, bg_height) = {
      let sprite = bg.sprite();
      (sprite.width() as f32, sprite.height() as f32)
    };
    let scale_x = self.screen_width / bg_width;
    let scale_y = self.screen_height / bg_height;
    ctx.world.spawn((
      Transform::new(0.0, 0.0).with_scale(scale_x, scale_y),
      Renderable::sprite(bg).with_z_order(Z_BACKGROUND),
    ));

    // Spawn game entities
    self.spawn_platforms(ctx);
    self.spawn_barriers(ctx);
    self.spawn_turrets(ctx);
    self.spawn_player(ctx);

    // Center camera
    ctx
      .camera
      .set_position(self.screen_width / 2.0, self.screen_height / 2.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Handle pause toggle
    if ctx.input.key_pressed(KeyCode::Escape) {
      if self.settings_open {
        self.settings_open = false;
      } else {
        self.is_paused = !self.is_paused;
      }
    }

    // Check if user wants to quit to menu
    if self.quit_to_menu {
      self.quit_to_menu = false;
      return SceneAction::Switch(Box::new(MenuScene::new()));
    }

    // Check if user wants to quit to desktop
    if self.quit_to_desktop {
      return SceneAction::Quit;
    }

    // Apply display mode if requested
    if self.apply_settings {
      self.apply_settings = false;
      let modes = DisplayMode::all();
      let display_mode = modes[self.selected_display_mode_idx];
      ctx.request_display_mode(display_mode);
    }

    // If paused, skip game logic
    if self.is_paused {
      return SceneAction::Continue;
    }

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
    self.update_barriers(ctx, dt);
    self.update_bullets(ctx, dt);
    self.update_particles(ctx, dt);

    // Check game over - start death effect instead of immediate transition
    if self.check_game_over(ctx) {
      const DEATH_EFFECT_DURATION: f32 = 0.6;
      self.death_effect_timer = Some(DEATH_EFFECT_DURATION);
      return SceneAction::Continue;
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Use cached player state
    let lives = self.player_lives;
    let invincible = self.player_invincible;

    // Lives - white text with semi-transparent background for visibility
    let lives_text = format!("Lives: {}", lives);
    ui.label(
      &lives_text,
      20.0,
      20.0,
      Color::WHITE,
      Some(Color::new(0, 0, 0, 150)),
    );

    // Survival time - white text with background for visibility
    let time_text = format!("Time: {:.1}s", self.survival_time);
    ui.label(
      &time_text,
      20.0,
      50.0,
      Color::WHITE,
      Some(Color::new(0, 0, 0, 150)),
    );

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

    // Pause menu
    if self.is_paused {
      self.render_pause_menu(ui, screen_w as f32, screen_h as f32);
    }
  }
}

// Helper methods for pause menu (not part of Scene trait)
impl GameScene {
  fn render_pause_menu(&mut self, ui: &mut UiContext, screen_w: f32, screen_h: f32) {
    // Darken background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w, screen_h),
      Color::new(0, 0, 0, 180),
    );

    let center_x = screen_w / 2.0;
    let center_y = screen_h / 2.0;

    if self.settings_open {
      // Settings modal - simplified to just display mode
      // Game uses fixed 1920x1080 design resolution, window can be freely resized
      let modal_w = 400.0;
      let modal_h = 180.0;
      let modal_x = center_x - modal_w / 2.0;
      let modal_y = center_y - modal_h / 2.0;

      // Modal background
      ui.draw_rect(
        Rect::new(modal_x, modal_y, modal_w, modal_h),
        Color::rgb(40, 40, 50),
      );
      ui.draw_rect_outline(
        Rect::new(modal_x, modal_y, modal_w, modal_h),
        Color::rgb(100, 100, 120),
        2.0,
      );

      // Settings title
      let title = "SETTINGS";
      let (title_w, _) = ui.text_size(title);
      ui.draw_text(
        title,
        center_x - title_w as f32 / 2.0,
        modal_y + 20.0,
        Color::WHITE,
      );

      // Display mode options
      let mode_y = modal_y + 55.0;
      ui.draw_text(
        "Display Mode:",
        modal_x + 20.0,
        mode_y,
        Color::rgb(180, 180, 180),
      );

      let modes = DisplayMode::all();
      for (i, mode) in modes.iter().enumerate() {
        let btn_x = modal_x + 20.0 + i as f32 * 120.0;
        let btn_y = mode_y + 25.0;
        let btn_rect = Rect::new(btn_x, btn_y, 110.0, 28.0);

        let is_selected = i == self.selected_display_mode_idx;
        let (bg, hover, pressed) = if is_selected {
          (
            Color::rgb(60, 100, 60),
            Color::rgb(70, 110, 70),
            Color::rgb(50, 90, 50),
          )
        } else {
          (
            Color::rgb(50, 50, 60),
            Color::rgb(60, 60, 70),
            Color::rgb(40, 40, 50),
          )
        };

        if ui.button_styled(
          mode.display_name(),
          btn_rect,
          bg,
          hover,
          pressed,
          Color::WHITE,
        ) {
          self.selected_display_mode_idx = i;
        }
      }

      // Apply and Back buttons
      let btn_y = modal_y + modal_h - 50.0;
      let apply_rect = Rect::new(center_x - 110.0, btn_y, 100.0, 35.0);
      let back_rect = Rect::new(center_x + 10.0, btn_y, 100.0, 35.0);

      if ui.button_styled(
        "Apply",
        apply_rect,
        Color::rgb(50, 80, 50),
        Color::rgb(60, 100, 60),
        Color::rgb(40, 70, 40),
        Color::WHITE,
      ) {
        // Save display mode to system.json for next launch
        let display_mode = modes[self.selected_display_mode_idx];
        crate::save::save_display_mode(display_mode);
        // Flag to apply changes at runtime (processed in update)
        self.apply_settings = true;
        self.settings_open = false;
      }

      if ui.button_styled(
        "Back",
        back_rect,
        Color::rgb(70, 50, 50),
        Color::rgb(90, 60, 60),
        Color::rgb(60, 40, 40),
        Color::WHITE,
      ) {
        self.settings_open = false;
      }
    } else {
      // Main pause menu
      let title = "PAUSED";
      let (title_w, _) = ui.text_size(title);
      ui.draw_text(
        title,
        center_x - title_w as f32 / 2.0,
        center_y - 100.0,
        Color::WHITE,
      );

      let btn_w = 200.0;
      let btn_h = 40.0;
      let btn_x = center_x - btn_w / 2.0;

      // Resume button
      let resume_rect = Rect::new(btn_x, center_y - 50.0, btn_w, btn_h);
      if ui.button_styled(
        "Resume",
        resume_rect,
        Color::rgb(50, 70, 50),
        Color::rgb(60, 90, 60),
        Color::rgb(40, 60, 40),
        Color::WHITE,
      ) {
        self.is_paused = false;
      }

      // Settings button
      let settings_rect = Rect::new(btn_x, center_y, btn_w, btn_h);
      if ui.button_styled(
        "Settings",
        settings_rect,
        Color::rgb(50, 50, 70),
        Color::rgb(60, 60, 90),
        Color::rgb(40, 40, 60),
        Color::WHITE,
      ) {
        self.settings_open = true;
      }

      // Quit to Menu button
      let quit_menu_rect = Rect::new(btn_x, center_y + 50.0, btn_w, btn_h);
      if ui.button_styled(
        "Quit to Menu",
        quit_menu_rect,
        Color::rgb(70, 60, 50),
        Color::rgb(90, 80, 60),
        Color::rgb(60, 50, 40),
        Color::WHITE,
      ) {
        self.is_paused = false;
        self.quit_to_menu = true;
      }

      // Quit to Desktop button
      let quit_desktop_rect = Rect::new(btn_x, center_y + 100.0, btn_w, btn_h);
      if ui.button_styled(
        "Quit to Desktop",
        quit_desktop_rect,
        Color::rgb(70, 50, 50),
        Color::rgb(90, 60, 60),
        Color::rgb(60, 40, 40),
        Color::WHITE,
      ) {
        self.quit_to_desktop = true;
      }

      // Instructions
      let hint = "Press ESC to resume";
      let (hint_w, _) = ui.text_size(hint);
      ui.draw_text(
        hint,
        center_x - hint_w as f32 / 2.0,
        center_y + 170.0,
        Color::rgb(150, 150, 150),
      );
    }
  }
}
