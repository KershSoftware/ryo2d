//! Main game scene with all gameplay logic

use crate::components::*;
use crate::save::save_best_time;
use crate::shared;
use crate::*;
use ryo2d::prelude::*;

use super::{GameOverScene, PauseScene};

mod enemies;
mod player;
mod spawning;

// Fixed design resolution - game logic always uses these values
// Display settings only affect window scaling, not gameplay
const DESIGN_WIDTH: f32 = 1920.0;
const DESIGN_HEIGHT: f32 = 1080.0;

/// Bundle type for pooled bullets
pub(super) type BulletBundle = (
  Transform,
  Renderable,
  RigidBody,
  Collider,
  Velocity,
  Bullet,
  Trail2D,
);

/// Marker component for nebula orb child entities
#[derive(Debug, Clone)]
pub(super) struct NebulaOrbMarker {
  pub(super) parent: Entity,
  pub(super) index: usize,
}

pub struct GameScene {
  pub(super) player: Option<Entity>,
  pub(super) turrets: Vec<Entity>,
  pub(super) drones: Vec<Entity>,
  pub(super) nebulas: Vec<Entity>,
  pub(super) rockets: Vec<Entity>,
  pub(super) lasers: Vec<Entity>,
  pub(super) fading_lasers: Vec<Entity>,
  pub(super) bullets: Vec<Entity>,
  pub(super) particles: Vec<Entity>,
  pub(super) platforms: Vec<Entity>,
  pub(super) barriers: Vec<Entity>,
  pub(super) survival_time: f32,
  #[allow(dead_code)]
  pub(super) best_time: f32,
  pub(super) player_lives: u32,
  pub(super) player_invincible: bool,
  pub(super) player_sprite: SpriteHandle,
  pub(super) turret_sprite: SpriteHandle,
  pub(super) drone_sprite: SpriteHandle,
  pub(super) nebula_orb_sprite: SpriteHandle,
  pub(super) rocket_sprite: SpriteHandle,
  pub(super) bullet_sprite: SpriteHandle,
  pub(super) particle_sprite: SpriteHandle,
  pub(super) platform_sprite: SpriteHandle,
  pub(super) barrier_sprite: SpriteHandle,
  // GPU-accelerated parallax background
  pub(super) background: GpuParallaxBackground,
  // Progression tracking
  pub(super) difficulty_level: u32,
  pub(super) time_since_last_progression: f32,
  pub(super) current_turret_speed: f32,
  pub(super) current_burst_size: u32,
  // Drone spawning
  pub(super) next_drone_spawn_time: f32,
  // Nebula spawning
  pub(super) next_nebula_spawn_time: f32,
  // Death effect
  pub(super) death_effect_timer: Option<f32>,
  // Screen dimensions (obtained from engine at runtime)
  pub(super) screen_width: f32,
  pub(super) screen_height: f32,
  // Object pool for bullets (high-frequency spawning)
  pub(super) bullet_pool: Option<Pool<BulletBundle>>,
}

impl GameScene {
  pub fn new() -> Self {
    // Load sprites
    // Heroic gold/orange player color with black outline
    let player_sprite =
      Sprite::solid_rect(PLAYER_SIZE as u32, PLAYER_SIZE as u32, 255, 180, 50, 255);
    let turret_sprite = Sprite::load("assets/turret.png").expect("Failed to load turret sprite");
    // Create a small round bullet programmatically (bright orange-red)
    let bullet_sprite = Sprite::solid_circle(BULLET_SIZE as u32, 255, 100, 50, 255);
    let particle_sprite =
      Sprite::load("assets/particle.png").expect("Failed to load particle sprite");
    let platform_sprite =
      Sprite::load("assets/platform.png").expect("Failed to load platform sprite");
    // Create barrier sprite programmatically (dark gray rectangle)
    let barrier_sprite =
      Sprite::solid_rect(BARRIER_WIDTH as u32, BARRIER_HEIGHT as u32, 60, 60, 70, 255);
    // Create drone sprite programmatically (purple color)
    let drone_sprite = Sprite::solid_rect(DRONE_SIZE as u32, DRONE_SIZE as u32, 150, 80, 200, 255);
    // Create nebula orb sprite (forest green)
    let nebula_orb_sprite = Sprite::solid_circle(NEBULA_ORB_SIZE as u32, 45, 120, 50, 255);
    // Create rocket sprite (orange/red)
    let rocket_sprite = Sprite::solid_rect(
      ROCKET_SIZE as u32,
      (ROCKET_SIZE * 0.6) as u32,
      255,
      120,
      50,
      255,
    );

    // Load parallax background layers as sprite handles (for GPU upload)
    let sky_sprite = SpriteHandle::from_sprite(
      Sprite::load("assets/desert_sky.png").expect("Failed to load sky sprite"),
      "desert_sky",
    );
    let moon_sprite = SpriteHandle::from_sprite(
      Sprite::load("assets/desert_moon.png").expect("Failed to load moon sprite"),
      "desert_moon",
    );
    let cloud_sprite = SpriteHandle::from_sprite(
      Sprite::load("assets/desert_cloud.png").expect("Failed to load cloud sprite"),
      "desert_cloud",
    );
    let mountain_sprite = SpriteHandle::from_sprite(
      Sprite::load("assets/desert_mountain.png").expect("Failed to load mountain sprite"),
      "desert_mountain",
    );
    let dune_mid_sprite = SpriteHandle::from_sprite(
      Sprite::load("assets/desert_dunemid.png").expect("Failed to load mid dune sprite"),
      "desert_dunemid",
    );
    let dune_front_sprite = SpriteHandle::from_sprite(
      Sprite::load("assets/desert_dunefrontt.png").expect("Failed to load front dune sprite"),
      "desert_dunefront",
    );

    // Scale factor to fit 1000px sprites to 1080px screen height
    let scale = 1080.0 / 1000.0;

    // Build GPU-accelerated parallax background with desert layers
    // Speeds must match menu.rs for seamless scene transition
    let background = GpuParallaxBuilder::new()
      .static_layer(&sky_sprite, -100)
      .with_scale(scale)
      .layer(&moon_sprite, 0.0, -90)
      .with_scale(scale)
      .with_auto_scroll(10.0)
      .with_tile_mode(TileMode::InfiniteScroll)
      .layer(&cloud_sprite, 0.0, -80)
      .with_scale(scale)
      .with_auto_scroll(30.0)
      .with_tile_mode(TileMode::InfiniteScroll)
      .layer(&mountain_sprite, 0.0, -60)
      .with_scale(scale)
      .with_auto_scroll(60.0)
      .with_tile_mode(TileMode::InfiniteScroll)
      .layer(&dune_mid_sprite, 0.0, -40)
      .with_scale(scale)
      .with_auto_scroll(100.0)
      .with_tile_mode(TileMode::InfiniteScroll)
      .layer(&dune_front_sprite, 0.0, -20)
      .with_scale(scale)
      .with_auto_scroll(160.0)
      .with_tile_mode(TileMode::InfiniteScroll)
      .build();

    Self {
      player: None,
      turrets: Vec::new(),
      drones: Vec::new(),
      nebulas: Vec::new(),
      rockets: Vec::new(),
      lasers: Vec::new(),
      fading_lasers: Vec::new(),
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
      drone_sprite: SpriteHandle::from_sprite(drone_sprite, "drone"),
      nebula_orb_sprite: SpriteHandle::from_sprite(nebula_orb_sprite, "nebula_orb"),
      rocket_sprite: SpriteHandle::from_sprite(rocket_sprite, "rocket"),
      bullet_sprite: SpriteHandle::from_sprite(bullet_sprite, "bullet.png"),
      particle_sprite: SpriteHandle::from_sprite(particle_sprite, "particle.png"),
      platform_sprite: SpriteHandle::from_sprite(platform_sprite, "platform.png"),
      barrier_sprite: SpriteHandle::from_sprite(barrier_sprite, "barrier"),
      background,
      // Progression starts at level 0
      difficulty_level: 0,
      time_since_last_progression: 0.0,
      current_turret_speed: TURRET_MOVE_SPEED,
      current_burst_size: TURRET_BURST_SIZE,
      // Drone spawning - first drone at 30 seconds
      next_drone_spawn_time: DRONE_FIRST_SPAWN_TIME,
      // Nebula spawning - spawn immediately for testing (normally NEBULA_FIRST_SPAWN_TIME)
      next_nebula_spawn_time: 1.0,
      // Death effect not active
      death_effect_timer: None,
      // Screen dimensions - fixed design resolution for consistent gameplay
      screen_width: DESIGN_WIDTH,
      screen_height: DESIGN_HEIGHT,
      // Bullet pool initialized in on_enter
      bullet_pool: None,
    }
  }

  fn update_barriers(&mut self, ctx: &mut SceneContext, dt: f32) {
    profile_scope!("barriers");
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
    profile_scope!("turrets");
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

      // Update spawn timer and apply dissolve effect while spawning
      let is_spawning = turret.is_spawning();
      if is_spawning {
        let just_finished = turret.update_spawn(dt);
        let spawn_progress = turret.spawn_progress();
        let turret_side = turret.side;

        drop(transform);
        drop(turret);

        // Apply dissolve shader for spawn effect (reverse progress for spawn-in)
        if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(turret_entity) {
          if just_finished {
            // Finished spawning - restore normal outline shader and play sound with panning
            renderable.shader = Some(SpriteShader::outline(Color::rgb(255, 140, 140), 2));
            let pan = match turret_side {
              TurretSide::Left => -0.8,
              TurretSide::Right => 0.8,
            };
            if let Some(audio) = &mut ctx.audio {
              let _ =
                audio.play_sound_with("enemy-spawn", SoundOptions::new().volume(0.15).panning(pan));
            }
          } else {
            // Still spawning - dissolve from invisible to visible with cyan edge glow
            let dissolve_progress = 1.0 - spawn_progress;
            renderable.shader = Some(SpriteShader::dissolve_with_edge(
              dissolve_progress,
              Color::rgb(100, 200, 255),
            ));
          }
        }
        continue; // Skip movement and shooting while spawning
      }

      // Move up and down using current (progression-scaled) speed
      transform.y += turret.move_direction * self.current_turret_speed * dt;

      // Bounce off screen top and floor platform
      let top_margin = TURRET_SIZE;
      let floor_top = self.screen_height - 30.0; // Floor platform top edge
      let bottom_limit = floor_top - TURRET_SIZE; // Turret can't go below this

      if transform.y < top_margin || transform.y > bottom_limit {
        turret.reverse_direction();
        transform.y = transform.y.clamp(top_margin, bottom_limit);
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

      // Shooting logic (only when fully spawned)
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
          drop(turret);

          // Play burst sound with panning based on turret side
          let pan = match turret_side {
            TurretSide::Left => -0.8,
            TurretSide::Right => 0.8,
          };
          if let Some(audio) = &mut ctx.audio {
            let _ = audio.play_sound_with("bullet", SoundOptions::new().volume(0.1).panning(pan));
          }
        }
      }
    }
  }

  fn update_bullets(&mut self, ctx: &mut SceneContext, _dt: f32) {
    profile_scope!("bullets");
    // Collect (index, entity) pairs to remove/release
    let mut bullets_to_release: Vec<(usize, Entity)> = Vec::new();
    let bullets_to_process: Vec<_> = self.bullets.iter().copied().enumerate().collect();

    for (i, bullet_entity) in bullets_to_process {
      // With Rapier integration, bullet position is updated by physics engine
      let Some(transform) = ctx.world.get::<Transform>(bullet_entity) else {
        bullets_to_release.push((i, bullet_entity));
        continue;
      };
      if !ctx.world.has::<Bullet>(bullet_entity) {
        bullets_to_release.push((i, bullet_entity));
        continue;
      }

      let bx = transform.x;
      let by = transform.y;

      drop(transform);

      // Trail is automatically updated by the engine's trail system

      // Check if out of bounds
      if bx < -50.0 || bx > self.screen_width + 50.0 || by < -50.0 || by > self.screen_height + 50.0
      {
        bullets_to_release.push((i, bullet_entity));
        continue;
      }

      // Check collision with player
      if self.check_bullet_player_collision(ctx, bx, by) {
        self.create_explosion(ctx, bx, by);
        bullets_to_release.push((i, bullet_entity));
        continue;
      }

      // Check collision with platforms
      if self.check_bullet_platform_collision(ctx, bx, by) {
        self.create_explosion(ctx, bx, by);
        bullets_to_release.push((i, bullet_entity));
        continue;
      }

      // Check collision with barriers
      if self.check_bullet_barrier_collision(ctx, bx, by) {
        self.create_explosion(ctx, bx, by);
        bullets_to_release.push((i, bullet_entity));
      }
    }

    // Release bullets back to pool (or despawn if not pooled)
    // Sort by index descending to safely swap_remove
    bullets_to_release.sort_by(|a, b| b.0.cmp(&a.0));
    for (i, entity) in bullets_to_release {
      self.bullets.swap_remove(i);
      // Release to pool if available, otherwise despawn
      if let Some(ref mut pool) = self.bullet_pool {
        pool.release(ctx.world, entity);
      } else {
        ctx.world.despawn(entity).ok();
      }
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
      self.apply_player_damage(ctx);
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

  fn update_particles(&mut self, ctx: &mut SceneContext, dt: f32) {
    profile_scope!("particles");
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
    if let Some(player_entity) = self.player
      && let Some(player) = ctx.world.get::<Player>(player_entity)
    {
      return !player.is_alive();
    }
    false
  }

  fn update_progression(&mut self, ctx: &mut SceneContext, dt: f32) {
    profile_scope!("progression");
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

    // Spawn drones based on survival time (independent of difficulty progression)
    // First drone at 30s, then every 20s up to max 3
    if (self.drones.len() as u32) < DRONE_MAX_COUNT
      && self.survival_time >= self.next_drone_spawn_time
    {
      self.spawn_drone(ctx);
      self.next_drone_spawn_time = self.survival_time + DRONE_SPAWN_INTERVAL;
    }

    // Spawn nebulas based on survival time
    if (self.nebulas.len() as u32) < NEBULA_MAX_COUNT
      && self.survival_time >= self.next_nebula_spawn_time
    {
      self.spawn_nebula(ctx);
      self.next_nebula_spawn_time = self.survival_time + NEBULA_SPAWN_INTERVAL;
    }
  }

}

impl Scene for GameScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Use fixed design resolution for consistent gameplay
    // Display settings only affect window scaling, not game logic
    self.screen_width = DESIGN_WIDTH;
    self.screen_height = DESIGN_HEIGHT;

    // Register handler for game:reset command (declared in MenuScene)
    // Handler is auto-cleaned when this scene exits
    ctx.register_console_command("game:reset", |_args, cmd_ctx| {
      cmd_ctx.console.print_system("Resetting game...");
      shared::request_reset();
      CommandResult::Ok
    });

    // Load sound effects
    if let Some(audio) = &mut ctx.audio {
      let _ = audio.load_sound("enemy-spawn", "assets/enemy-spawn.wav");
      let _ = audio.load_sound("laser", "assets/laser.mp3");
      let _ = audio.load_sound("hurt", "assets/hurt.mp3");
      let _ = audio.load_sound("bullet", "assets/bullet.wav");
    }

    // Clear world
    ctx.world.clear();

    // Sync parallax background with shared time (persists from menu)
    self
      .background
      .set_elapsed_time(shared::get_parallax_time());

    // Reset state
    self.survival_time = 0.0;
    self.player = None;
    self.turrets.clear();
    self.drones.clear();
    self.lasers.clear();
    self.fading_lasers.clear();
    self.bullets.clear();
    self.particles.clear();
    self.platforms.clear();
    self.barriers.clear();

    // Configure camera shake for impactful hits (20px max offset, fast decay)
    ctx.camera.configure_shake(20.0, 1.5);

    // Spawn game entities
    self.spawn_platforms(ctx);
    self.spawn_barriers(ctx);
    self.spawn_turrets(ctx);
    // Drones additionally spawn progressively every 30 seconds
    self.spawn_drone(ctx);
    self.spawn_player(ctx);

    // Initialize bullet pool (pre-allocate 200 bullets)
    let bullet_sprite = self.bullet_sprite.clone();
    self.bullet_pool = Some(Pool::new(
      ctx.world,
      PoolConfig::default()
        .with_capacity(200)
        .with_max_capacity(500)
        .with_expand_batch_size(50)
        .with_exhausted_policy(PoolExhaustedPolicy::RecycleOldest),
      move || {
        (
          Transform::default(),
          Renderable::sprite(bullet_sprite.clone())
            .with_z_order(Z_BULLET)
            .interpolated(),
          // CCD for fast-moving bullets, no gravity
          RigidBody::new().with_ccd().with_gravity_scale(0.0),
          Collider::circle_collider(BULLET_SIZE / 2.0).as_trigger(),
          Velocity::zero(),
          Bullet::new(0.0, 0.0),
          Trail2D::new(Color::new(255, 140, 30, 230), 3.5)
            .with_length(25)
            .with_taper(0.7)
            .with_fade(true)
            .with_min_distance(2.0),
        )
      },
    ));

    // Center camera
    ctx
      .camera
      .set_position(self.screen_width / 2.0, self.screen_height / 2.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Check for reset request from console command
    if shared::take_reset_request() {
      return SceneAction::Switch(Box::new(GameScene::new()));
    }

    // Pause: push overlay scene
    if ctx.input.key_pressed_and_consume(KeyCode::Escape)
      || ctx
        .input
        .gamepad_button_pressed_and_consume(GamepadButton::Start)
    {
      return SceneAction::Push(Box::new(PauseScene::new()));
    }

    // Update parallax background
    // Update shared parallax time (persists between menu and game)
    let parallax_time = shared::add_parallax_time(dt);
    self.background.set_elapsed_time(parallax_time);

    self.survival_time += dt;

    // Update progression
    self.update_progression(ctx, dt);

    // Update player state cache for UI
    if let Some(player_entity) = self.player
      && let Some(player) = ctx.world.get::<Player>(player_entity)
    {
      self.player_lives = player.lives;
      self.player_invincible = player.is_invincible();
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

    // Update barriers FIRST so player can ride them at correct position
    self.update_barriers(ctx, dt);
    self.update_player(ctx, dt);
    self.update_turrets(ctx, dt);
    self.update_drones(ctx, dt);
    self.update_nebulas(ctx, dt);
    self.update_rockets(ctx, dt);
    self.update_laser_tracers(ctx, dt);
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

  fn render_gpu(&mut self, ctx: &mut GpuRenderContext) {
    profile_scope!("sprites");
    self.background.render(ctx);
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    profile_scope!("ui");

    // Use cached player state
    let lives = self.player_lives;
    let invincible = self.player_invincible;

    // Lives - draw 3 hearts (red = alive, grey = lost)
    let heart_size = 44.0;
    let heart_spacing = 16.0;
    let hearts_x = 40.0;
    let hearts_y = 40.0;

    for i in 0..PLAYER_MAX_LIVES {
      let x = hearts_x + i as f32 * (heart_size + heart_spacing);
      let has_life = i < lives;
      self.draw_heart(ui, x, hearts_y, heart_size, has_life, invincible);
    }

    // Survival time - positioned below hearts
    let time_text = format!("Time: {:.1}s", self.survival_time);
    ui.label(
      &time_text,
      40.0,
      hearts_y + heart_size + 20.0,
      Color::WHITE,
      Some(Color::new(0, 0, 0, 150)),
    );

    // Hit indicator
    if invincible {
      ui.draw_text_anchored(
        "HIT!",
        Anchor::TopCenter,
        0.0,
        200.0,
        Color::rgb(255, 50, 50),
      );
    }

    SceneAction::Continue
  }
}

// Helper methods for UI rendering (not part of Scene trait)
impl GameScene {
  /// Draw a pixelated heart icon
  fn draw_heart(
    &self,
    ui: &mut UiContext,
    x: f32,
    y: f32,
    size: f32,
    filled: bool,
    flashing: bool,
  ) {
    // Colors - apply flash effect when hit
    let fill_color = if filled {
      if flashing {
        // Pulsing flash effect using survival_time for animation
        let pulse = (self.survival_time * 10.0).sin().abs();
        let base_r = 220u8;
        let base_g = 50u8;
        let base_b = 50u8;
        // Flash towards white
        let flash_r = base_r.saturating_add((pulse * 35.0) as u8);
        let flash_g = base_g.saturating_add((pulse * 200.0) as u8);
        let flash_b = base_b.saturating_add((pulse * 200.0) as u8);
        Color::rgb(flash_r, flash_g, flash_b)
      } else {
        Color::rgb(220, 50, 50) // Red for alive
      }
    } else {
      Color::rgb(140, 130, 120) // Beige/grey for lost
    };
    let border_color = Color::rgb(60, 50, 50); // Dark border

    // Scale factor (heart pattern is 9x8 base)
    let scale = size / 9.0;
    let px = scale; // Pixel size

    // Classic pixel heart pattern (9 wide, 8 tall)
    // 1 = filled, 0 = empty
    #[rustfmt::skip]
    let pattern: [[u8; 9]; 8] = [
      [0, 1, 1, 0, 0, 0, 1, 1, 0], // Row 0: two bumps
      [1, 1, 1, 1, 1, 1, 1, 1, 1], // Row 1
      [1, 1, 1, 1, 1, 1, 1, 1, 1], // Row 2
      [1, 1, 1, 1, 1, 1, 1, 1, 1], // Row 3
      [0, 1, 1, 1, 1, 1, 1, 1, 0], // Row 4
      [0, 0, 1, 1, 1, 1, 1, 0, 0], // Row 5
      [0, 0, 0, 1, 1, 1, 0, 0, 0], // Row 6
      [0, 0, 0, 0, 1, 0, 0, 0, 0], // Row 7
    ];

    // Draw border first (slightly larger)
    let border_offset = 1.0;
    for (row_idx, row) in pattern.iter().enumerate() {
      for (col_idx, &pixel) in row.iter().enumerate() {
        if pixel == 1 {
          let px_x = x + col_idx as f32 * px;
          let px_y = y + row_idx as f32 * px;
          // Draw border by checking if any neighbor is empty
          let has_empty_neighbor = Self::has_empty_neighbor(&pattern, row_idx, col_idx);
          if has_empty_neighbor {
            ui.draw_rect(
              Rect::new(
                px_x - border_offset,
                px_y - border_offset,
                px + border_offset * 2.0,
                px + border_offset * 2.0,
              ),
              border_color,
            );
          }
        }
      }
    }

    // Draw fill
    for (row_idx, row) in pattern.iter().enumerate() {
      for (col_idx, &pixel) in row.iter().enumerate() {
        if pixel == 1 {
          let px_x = x + col_idx as f32 * px;
          let px_y = y + row_idx as f32 * px;
          ui.draw_rect(Rect::new(px_x, px_y, px, px), fill_color);
        }
      }
    }
  }

  /// Apply damage to the player: flash, camera shake, and hurt sound.
  /// Returns true if damage was actually applied (player was not invincible).
  fn apply_player_damage(&mut self, ctx: &mut SceneContext) -> bool {
    let Some(player_e) = self.player else {
      return false;
    };
    let Some(mut player) = ctx.world.get_mut::<Player>(player_e) else {
      return false;
    };
    let was_invincible = player.is_invincible();
    player.take_damage();
    self.player_lives = player.lives;
    self.player_invincible = player.is_invincible();
    drop(player);

    if !was_invincible {
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(player_e) {
        renderable.shader = Some(SpriteShader::flash(Color::rgb(255, 100, 100), 1.0));
      }
      ctx.camera.add_trauma(0.5);
      if let Some(audio) = &mut ctx.audio {
        let _ = audio.play_sound("hurt");
      }
    }
    !was_invincible
  }

  /// Check if a pixel in the heart pattern has an empty neighbor
  fn has_empty_neighbor(pattern: &[[u8; 9]; 8], row: usize, col: usize) -> bool {
    let directions: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    for (dr, dc) in directions {
      let nr = row as i32 + dr;
      let nc = col as i32 + dc;
      if !(0..8).contains(&nr) || !(0..9).contains(&nc) {
        return true; // Edge of pattern counts as empty
      }
      if pattern[nr as usize][nc as usize] == 0 {
        return true;
      }
    }
    false
  }
}
