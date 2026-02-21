//! Main game scene with all gameplay logic

use crate::components::*;
use crate::save::save_best_time;
use crate::shared;
use crate::*;
use ryo2d::prelude::*;

use super::{GameOverScene, PauseScene};

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

  fn update_drones(&mut self, ctx: &mut SceneContext, dt: f32) {
    profile_scope!("drones");
    use crate::components::DroneState;
    use crate::components::drone::DRONE_AIM_SPEED;
    use crate::constants::{
      DRONE_AIM_DURATION, DRONE_COOLDOWN, DRONE_FIRE_DURATION, DRONE_LASER_WIDTH, DRONE_SPEED,
    };

    // Clean up lasers from previous frame FIRST (so they render for one frame)
    for laser in self.lasers.drain(..) {
      let _ = ctx.world.despawn(laser);
    }

    // Clean up fading lasers that have finished fading (no points left)
    let faded_lasers: Vec<Entity> = self
      .fading_lasers
      .iter()
      .filter(|&&entity| {
        ctx
          .world
          .get::<Trail2D>(entity)
          .map(|trail| trail.point_count() == 0)
          .unwrap_or(true) // Despawn if trail missing
      })
      .copied()
      .collect();
    for entity in &faded_lasers {
      let _ = ctx.world.despawn(*entity);
    }
    self.fading_lasers.retain(|e| !faded_lasers.contains(e));

    let player_entity = self.player;

    // Get player position for aiming (player uses center pivot)
    let player_pos = player_entity.and_then(|e| ctx.world.get::<Transform>(e).map(|t| (t.x, t.y)));

    // Collect drones to update
    let drones_to_update: Vec<_> = self.drones.clone();

    for drone_entity in drones_to_update {
      // Get drone component
      let Some(mut drone) = ctx.world.get_mut::<Drone>(drone_entity) else {
        continue;
      };

      // Handle spawn animation first (like turrets)
      if drone.is_spawning() {
        let just_finished = drone.update_spawn(dt);
        let spawn_progress = drone.spawn_progress();
        drop(drone);

        // Get drone position for panning
        let drone_x = ctx
          .world
          .get::<Transform>(drone_entity)
          .map(|t| t.x)
          .unwrap_or(self.screen_width / 2.0);

        // Apply dissolve shader for spawn effect
        if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(drone_entity) {
          if just_finished {
            // Finished spawning - restore normal outline shader (purple) and play sound with panning
            renderable.shader = Some(SpriteShader::outline(Color::rgb(200, 140, 255), 2));
            // Pan based on screen position (-1 left, 0 center, 1 right)
            let pan = ((drone_x / self.screen_width) * 2.0 - 1.0).clamp(-1.0, 1.0);
            if let Some(audio) = &mut ctx.audio {
              let _ =
                audio.play_sound_with("enemy-spawn", SoundOptions::new().volume(0.15).panning(pan));
            }
          } else {
            // Still spawning - dissolve from invisible to visible
            let dissolve_progress = 1.0 - spawn_progress;
            renderable.shader = Some(SpriteShader::dissolve_with_edge(
              dissolve_progress,
              Color::rgb(0, 0, 0),
            ));
          }
        }
        continue; // Skip behavior while spawning
      }

      let state = drone.state;
      let mut state_timer = drone.state_timer;
      let velocity_x = drone.velocity_x;
      let velocity_y = drone.velocity_y;
      let target_x = drone.target_x;
      let target_y = drone.target_y;
      let aim_x = drone.aim_x;
      let aim_y = drone.aim_y;
      let laser_target_x = drone.laser_target_x;
      let laser_target_y = drone.laser_target_y;
      let laser_active = drone.laser_active;
      drop(drone);

      // Get current position
      let Some(transform) = ctx.world.get::<Transform>(drone_entity) else {
        continue;
      };
      let current_x = transform.x;
      let current_y = transform.y;
      drop(transform);

      state_timer -= dt;

      match state {
        DroneState::Moving => {
          // Move toward target
          if let Some(mut t) = ctx.world.get_mut::<Transform>(drone_entity) {
            t.x += velocity_x * dt;
            t.y += velocity_y * dt;
          }

          // Check if reached target
          let dx = target_x - current_x;
          let dy = target_y - current_y;
          let dist = (dx * dx + dy * dy).sqrt();

          if dist < 15.0 || state_timer <= 0.0 {
            // Reached target, start aiming
            if let Some(mut drone) = ctx.world.get_mut::<Drone>(drone_entity) {
              drone.state = DroneState::Aiming;
              drone.state_timer = DRONE_AIM_DURATION;
              drone.velocity_x = 0.0;
              drone.velocity_y = 0.0;
              // Lock on to player position
              if let Some((px, py)) = player_pos {
                drone.laser_target_x = px;
                drone.laser_target_y = py;
              }
            }
          } else {
            // Update timer
            if let Some(mut drone) = ctx.world.get_mut::<Drone>(drone_entity) {
              drone.state_timer = state_timer;
            }
          }
        }

        DroneState::Aiming => {
          // Slowly pan aim towards player position
          let aim_progress = 1.0 - (state_timer / DRONE_AIM_DURATION).clamp(0.0, 1.0);

          // Calculate new aim position (lerp towards player)
          let mut new_aim_x = aim_x;
          let mut new_aim_y = aim_y;
          if let Some((px, py)) = player_pos {
            let lerp_factor = (DRONE_AIM_SPEED * dt).min(1.0);
            new_aim_x = aim_x + (px - aim_x) * lerp_factor;
            new_aim_y = aim_y + (py - aim_y) * lerp_factor;
          }

          // Update drone aim position
          if let Some(mut drone) = ctx.world.get_mut::<Drone>(drone_entity) {
            drone.aim_x = new_aim_x;
            drone.aim_y = new_aim_y;
            drone.state_timer = state_timer;
          }

          // Calculate extended laser endpoint (extends beyond screen)
          let drone_center_x = current_x;
          let drone_center_y = current_y;
          let (extended_x, extended_y) = Self::extend_line_to_screen_edge(
            drone_center_x,
            drone_center_y,
            new_aim_x,
            new_aim_y,
            self.screen_width,
            self.screen_height,
          );

          // Flash effect - faster flashing as aim progresses, brighter near end
          let flash_speed = 8.0 + aim_progress * 20.0;
          let flash = ((state_timer * flash_speed).sin() * 0.5 + 0.5).clamp(0.0, 1.0);

          // Warning line - thin and semi-transparent, pulses brighter
          // Gets more opaque as aim_progress increases (reaching peak near fire time)
          let base_alpha = 80.0 + aim_progress * 80.0; // 80 -> 160 over aim duration
          let warning_alpha = (base_alpha + flash * 95.0).min(255.0) as u8; // Can reach 255 near end
          let warning_width = 2.0 + flash * 3.0 + aim_progress * 2.0; // Gets thicker too

          let warning_laser = ctx.world.spawn((
            Transform::new(0.0, 0.0),
            Line2D::segment(
              Vec2::new(drone_center_x, drone_center_y),
              Vec2::new(extended_x, extended_y),
              Color::new(255, 100, 100, warning_alpha),
              warning_width,
            ),
          ));
          self.lasers.push(warning_laser);

          // Flash the drone sprite during aiming
          if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(drone_entity) {
            if flash > 0.5 {
              renderable.shader = Some(SpriteShader::flash(Color::new(255, 200, 200, 255), 0.5));
            } else {
              renderable.shader = Some(SpriteShader::outline(Color::rgb(200, 140, 255), 2));
            }
          }

          if state_timer <= 0.0 {
            // Lock the laser target to current aim position and start firing
            if let Some(mut drone) = ctx.world.get_mut::<Drone>(drone_entity) {
              drone.laser_target_x = new_aim_x;
              drone.laser_target_y = new_aim_y;
              drone.state = DroneState::Firing;
              drone.state_timer = DRONE_FIRE_DURATION;
              drone.laser_active = true;
            }
            if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(drone_entity) {
              renderable.shader = Some(SpriteShader::outline(Color::rgb(255, 50, 50), 2));
            }
            if let Some(audio) = &mut ctx.audio {
              let _ = audio.play_sound_with("laser", SoundOptions::new().volume(0.3));
            }
          }
        }

        DroneState::Firing => {
          // Spawn damaging tracer when firing starts (laser_active just turned true)
          // The tracer sweeps from drone to endpoint, damaging player on contact
          if laser_active {
            // Check if we already spawned tracers this firing cycle
            // (laser_active stays true for the whole firing duration)
            let has_active_tracer = self
              .fading_lasers
              .iter()
              .any(|&e| ctx.world.get::<LaserTracer>(e).is_some());

            if !has_active_tracer {
              let drone_center_x = current_x;
              let drone_center_y = current_y;

              // Extend laser beyond screen edge
              let (extended_x, extended_y) = Self::extend_line_to_screen_edge(
                drone_center_x,
                drone_center_y,
                laser_target_x,
                laser_target_y,
                self.screen_width,
                self.screen_height,
              );

              let tracer_speed = 2500.0; // Fast sweep
              let damage_radius = PLAYER_SIZE / 2.0 + DRONE_LASER_WIDTH / 2.0;

              // Outer laser trail (red) - DAMAGING
              let outer_trail = Trail2D::new(Color::new(255, 50, 50, 220), DRONE_LASER_WIDTH)
                .with_length(80)
                .with_fade(true)
                .with_lifetime(0.35)
                .with_min_distance(6.0);

              let outer_tracer = ctx.world.spawn((
                Transform::new(drone_center_x, drone_center_y),
                outer_trail,
                LaserTracer::damaging(extended_x, extended_y, tracer_speed, damage_radius),
              ));
              self.fading_lasers.push(outer_tracer);

              // Inner core trail (bright) - visual only
              let core_trail =
                Trail2D::new(Color::new(255, 200, 150, 255), DRONE_LASER_WIDTH * 0.4)
                  .with_length(80)
                  .with_fade(true)
                  .with_lifetime(0.3)
                  .with_min_distance(6.0);

              let core_tracer = ctx.world.spawn((
                Transform::new(drone_center_x, drone_center_y),
                core_trail,
                LaserTracer::new(extended_x, extended_y, tracer_speed),
              ));
              self.fading_lasers.push(core_tracer);
            }
          }

          // Check if firing duration ended
          let mut finished_firing = false;
          if let Some(mut drone) = ctx.world.get_mut::<Drone>(drone_entity) {
            if state_timer <= 0.0 {
              // Done firing, go to cooldown
              drone.state = DroneState::Cooldown;
              drone.state_timer = DRONE_COOLDOWN;
              drone.laser_active = false;
              finished_firing = true;
            } else {
              drone.state_timer = state_timer;
            }
          }

          // Restore normal shader when firing ends
          if finished_firing
            && let Some(mut renderable) = ctx.world.get_mut::<Renderable>(drone_entity)
          {
            renderable.shader = Some(SpriteShader::outline(Color::rgb(200, 140, 255), 2));
          }
        }

        DroneState::Cooldown => {
          if state_timer <= 0.0 {
            // Pick new target and start moving
            if let Some(mut drone) = ctx.world.get_mut::<Drone>(drone_entity) {
              drone.pick_new_target(current_x, current_y, self.screen_width, self.screen_height);
              drone.calculate_velocity(current_x, current_y, DRONE_SPEED);
              drone.state = DroneState::Moving;
              drone.state_timer = 3.0; // Max time to reach target
            }
          } else if let Some(mut drone) = ctx.world.get_mut::<Drone>(drone_entity) {
            drone.state_timer = state_timer;
          }
        }
      }
    }
  }

  /// Update laser tracer entities (moving trails for sweeping laser effect)
  fn update_laser_tracers(&mut self, ctx: &mut SceneContext, dt: f32) {
    profile_scope!("laser_tracers");
    // Get player position for damage checks
    let player_pos = self
      .player
      .and_then(|e| ctx.world.get::<Transform>(e).map(|t| (t.x, t.y)));

    // Move tracers toward their targets and collect those that reached the end
    let mut reached_target: Vec<Entity> = Vec::new();
    let mut player_hit = false;

    for &entity in &self.fading_lasers {
      // Only process entities with LaserTracer component
      let Some(tracer) = ctx.world.get::<LaserTracer>(entity) else {
        continue;
      };

      let target_x = tracer.target_x;
      let target_y = tracer.target_y;
      let speed = tracer.speed;
      let damages = tracer.damages;
      let damage_radius = tracer.damage_radius;
      drop(tracer);

      // Get current position
      let Some(transform) = ctx.world.get::<Transform>(entity) else {
        continue;
      };
      let current_x = transform.x;
      let current_y = transform.y;
      drop(transform);

      // Check damage against player (only if this tracer damages)
      if damages
        && !player_hit
        && let Some((px, py)) = player_pos
      {
        let dx_p = px - current_x;
        let dy_p = py - current_y;
        let dist_to_player = (dx_p * dx_p + dy_p * dy_p).sqrt();

        if dist_to_player < damage_radius {
          player_hit = true;
        }
      }

      // Calculate direction to target
      let dx = target_x - current_x;
      let dy = target_y - current_y;
      let dist = (dx * dx + dy * dy).sqrt();

      if dist < 10.0 {
        // Reached target - disable trail recording and mark for later cleanup
        if let Some(mut trail) = ctx.world.get_mut::<Trail2D>(entity) {
          trail.set_enabled(false);
        }
        reached_target.push(entity);
      } else {
        // Move toward target
        let dir_x = dx / dist;
        let dir_y = dy / dist;
        let move_dist = (speed * dt).min(dist); // Don't overshoot

        if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
          transform.x += dir_x * move_dist;
          transform.y += dir_y * move_dist;
        }
      }
    }

    // Apply damage to player if hit
    if player_hit {
      self.apply_player_damage(ctx);
    }

    // Remove LaserTracer component from entities that reached target
    // (they become regular fading trails that will be cleaned up when empty)
    for entity in reached_target {
      let _ = ctx.world.remove_one::<LaserTracer>(entity);
    }
  }

  /// Extend a line from (x1, y1) through (x2, y2) to beyond the screen edge
  fn extend_line_to_screen_edge(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    screen_w: f32,
    screen_h: f32,
  ) -> (f32, f32) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = (dx * dx + dy * dy).sqrt();

    if len < 0.001 {
      return (x2, y2); // No direction, just return target
    }

    // Normalize direction and extend far beyond screen
    let dir_x = dx / len;
    let dir_y = dy / len;

    // Extend to 2x the screen diagonal to ensure it goes off-screen
    let extend_dist = (screen_w * screen_w + screen_h * screen_h).sqrt() * 2.0;

    (x1 + dir_x * extend_dist, y1 + dir_y * extend_dist)
  }

  fn spawn_nebula(&mut self, ctx: &mut SceneContext) {
    use crate::constants::{NEBULA_ORB_SIZE, Z_NEBULA};

    let mut rng = rand::thread_rng();

    // Spawn nebula in upper area of screen
    let spawn_x = rng.gen_range(200.0..self.screen_width - 200.0);
    let spawn_y = rng.gen_range(100.0..self.screen_height * 0.4);

    // Create the nebula component to get orb positions
    let nebula = Nebula::new(spawn_x, spawn_y);
    let orb_positions: Vec<Vec2> = nebula
      .orbs
      .iter()
      .map(|orb| orb.position(Vec2::new(spawn_x, spawn_y)))
      .collect();

    // Create center entity for the nebula (invisible, just holds the component)
    let nebula_entity = ctx.world.spawn((Transform::new(spawn_x, spawn_y), nebula));

    // Create the 3 orb child entities
    for (i, orb_pos) in orb_positions.iter().enumerate() {
      let sprite = self.nebula_orb_sprite.clone();

      ctx.world.spawn((
        Transform::new(orb_pos.x, orb_pos.y).with_pivot_center(),
        Renderable::sprite(sprite)
          .with_z_order(Z_NEBULA)
          .with_shader(SpriteShader::dissolve_with_edge(
            1.0,
            Color::rgb(80, 180, 80),
          )),
        RigidBody::kinematic(),
        Collider::circle_collider(NEBULA_ORB_SIZE / 2.0),
        NebulaOrbMarker {
          parent: nebula_entity,
          index: i,
        },
      ));
    }

    self.nebulas.push(nebula_entity);
  }

  fn update_nebulas(&mut self, ctx: &mut SceneContext, dt: f32) {
    use crate::components::NebulaState;
    use crate::components::nebula::NEBULA_ROCKETS_PER_SALVO;
    use crate::constants::{NEBULA_CHARGE_DURATION, NEBULA_COOLDOWN, NEBULA_SPEED};

    // Get player position for rocket targeting (used by spawn_rocket)
    let _player_pos = self
      .player
      .and_then(|e| ctx.world.get::<Transform>(e).map(|t| Vec2::new(t.x, t.y)));

    // Collect all nebula positions for avoidance
    let nebula_positions: Vec<(Entity, Vec2, f32)> = self
      .nebulas
      .iter()
      .filter_map(|&e| {
        let t = ctx.world.get::<Transform>(e)?;
        let n = ctx.world.get::<Nebula>(e)?;
        Some((e, Vec2::new(t.x, t.y), n.radius))
      })
      .collect();

    let nebulas_to_update: Vec<_> = self.nebulas.clone();

    for nebula_entity in nebulas_to_update {
      let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) else {
        continue;
      };

      // Handle spawn animation
      if nebula.is_spawning() {
        let just_finished = nebula.update_spawn(dt);
        let spawn_progress = nebula.spawn_progress();
        drop(nebula);

        // Update orb dissolve shaders
        self.update_nebula_orb_shaders(ctx, nebula_entity, spawn_progress, just_finished);
        continue;
      }

      // Update orb positions
      nebula.update_orbs(dt);

      let state = nebula.state;
      let mut state_timer = nebula.state_timer;
      state_timer -= dt;

      // Get current position
      let current_pos = ctx
        .world
        .get::<Transform>(nebula_entity)
        .map(|t| Vec2::new(t.x, t.y))
        .unwrap_or(Vec2::ZERO);

      drop(nebula);

      match state {
        NebulaState::Moving => {
          // Calculate base velocity toward target
          let nebula = ctx.world.get::<Nebula>(nebula_entity).unwrap();
          let base_vel = nebula.calculate_base_velocity(current_pos, NEBULA_SPEED);
          let reached = nebula.reached_target(current_pos);
          drop(nebula);

          // Apply steering avoidance
          let neighbors: Vec<(Vec2, f32)> = nebula_positions
            .iter()
            .filter(|(e, _, _)| *e != nebula_entity)
            .map(|(_, pos, radius)| (*pos, *radius))
            .collect();

          let avoid = steering::separation(current_pos, 40.0, &neighbors, 60.0);
          let velocity = (base_vel + avoid).clamp_length_max(NEBULA_SPEED);

          // Apply movement
          if let Some(mut t) = ctx.world.get_mut::<Transform>(nebula_entity) {
            t.x += velocity.x * dt;
            t.y += velocity.y * dt;
          }

          // Update orb world positions
          self.update_nebula_orb_positions(ctx, nebula_entity);

          if reached || state_timer <= 0.0 {
            if let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) {
              nebula.state = NebulaState::Charging;
              nebula.state_timer = NEBULA_CHARGE_DURATION;
            }
          } else if let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) {
            nebula.state_timer = state_timer;
          }
        }

        NebulaState::Charging => {
          // Orbs pulse/flash during charge
          let charge_progress = 1.0 - (state_timer / NEBULA_CHARGE_DURATION).clamp(0.0, 1.0);
          self.update_nebula_orb_charge_effect(ctx, nebula_entity, charge_progress);

          // Still update orb positions (they float)
          if let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) {
            nebula.update_orbs(dt);
          }
          self.update_nebula_orb_positions(ctx, nebula_entity);

          if state_timer <= 0.0 {
            if let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) {
              nebula.state = NebulaState::Firing;
              nebula.rockets_remaining = NEBULA_ROCKETS_PER_SALVO;
              nebula.rocket_timer = 0.0;
            }
          } else if let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) {
            nebula.state_timer = state_timer;
          }
        }

        NebulaState::Firing => {
          // Still update orb positions
          if let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) {
            nebula.update_orbs(dt);
          }
          self.update_nebula_orb_positions(ctx, nebula_entity);

          // Fire rockets
          let (rockets_remaining, rocket_timer) = {
            let nebula = ctx.world.get::<Nebula>(nebula_entity).unwrap();
            (nebula.rockets_remaining, nebula.rocket_timer)
          };

          let new_timer = rocket_timer - dt;

          if rockets_remaining > 0 && new_timer <= 0.0 {
            // Fire a rocket
            let nebula = ctx.world.get::<Nebula>(nebula_entity).unwrap();
            let top_pos = nebula.top_orb_position(current_pos);
            drop(nebula);

            self.spawn_rocket(ctx, top_pos, rockets_remaining);

            if let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) {
              nebula.rockets_remaining -= 1;
              nebula.rocket_timer = crate::components::nebula::NEBULA_ROCKET_INTERVAL;
            }
          } else if let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) {
            nebula.rocket_timer = new_timer;
          }

          // Check if done firing
          let rockets_left = ctx
            .world
            .get::<Nebula>(nebula_entity)
            .map(|n| n.rockets_remaining)
            .unwrap_or(0);

          if rockets_left == 0 {
            if let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) {
              nebula.state = NebulaState::Cooldown;
              nebula.state_timer = NEBULA_COOLDOWN;
            }
            // Reset orb shaders to clean outline (not dissolve)
            self.update_nebula_orb_shaders(ctx, nebula_entity, 1.0, true);
          }
        }

        NebulaState::Cooldown => {
          // Still update orb positions
          if let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) {
            nebula.update_orbs(dt);
          }
          self.update_nebula_orb_positions(ctx, nebula_entity);

          if state_timer <= 0.0 {
            if let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) {
              nebula.pick_new_target(current_pos, self.screen_width, self.screen_height);
              nebula.state = NebulaState::Moving;
              nebula.state_timer = 5.0; // Max time to reach target
            }
          } else if let Some(mut nebula) = ctx.world.get_mut::<Nebula>(nebula_entity) {
            nebula.state_timer = state_timer;
          }
        }
      }
    }
  }

  fn update_nebula_orb_positions(&self, ctx: &mut SceneContext, nebula_entity: Entity) {
    let Some(nebula) = ctx.world.get::<Nebula>(nebula_entity) else {
      return;
    };
    let center = ctx
      .world
      .get::<Transform>(nebula_entity)
      .map(|t| Vec2::new(t.x, t.y))
      .unwrap_or(Vec2::ZERO);

    let orb_positions: Vec<Vec2> = nebula.orbs.iter().map(|orb| orb.position(center)).collect();
    drop(nebula);

    // Find and update orb entities
    for (marker, transform) in ctx
      .world
      .query::<(&NebulaOrbMarker, &mut Transform)>()
      .iter()
    {
      if marker.parent == nebula_entity
        && let Some(pos) = orb_positions.get(marker.index)
      {
        transform.x = pos.x;
        transform.y = pos.y;
      }
    }
  }

  fn update_nebula_orb_shaders(
    &self,
    ctx: &mut SceneContext,
    nebula_entity: Entity,
    progress: f32,
    just_finished: bool,
  ) {
    for (marker, renderable) in ctx
      .world
      .query::<(&NebulaOrbMarker, &mut Renderable)>()
      .iter()
    {
      if marker.parent == nebula_entity {
        if just_finished {
          // Glossy glow effect - lighter green glow around forest green orb
          renderable.shader = Some(SpriteShader::outline(Color::rgb(150, 220, 150), 2));
        } else {
          let dissolve = 1.0 - progress;
          renderable.shader = Some(SpriteShader::dissolve_with_edge(
            dissolve,
            Color::rgb(80, 180, 80),
          ));
        }
      }
    }
  }

  fn update_nebula_orb_charge_effect(
    &self,
    ctx: &mut SceneContext,
    nebula_entity: Entity,
    charge_progress: f32,
  ) {
    let flash_speed = 6.0 + charge_progress * 12.0;
    let flash =
      ((charge_progress * flash_speed * std::f32::consts::TAU).sin() * 0.5 + 0.5).clamp(0.0, 1.0);

    for (marker, renderable) in ctx
      .world
      .query::<(&NebulaOrbMarker, &mut Renderable)>()
      .iter()
    {
      if marker.parent == nebula_entity {
        if flash > 0.5 {
          // Bright flash during charge
          renderable.shader = Some(SpriteShader::flash(Color::new(180, 255, 180, 255), 0.7));
        } else {
          // Back to glossy glow
          renderable.shader = Some(SpriteShader::outline(Color::rgb(150, 220, 150), 2));
        }
      }
    }
  }

  fn spawn_rocket(&mut self, ctx: &mut SceneContext, pos: Vec2, rocket_index: u32) {
    use crate::components::rocket::ROCKET_LAUNCH_SPREAD;
    use crate::constants::{ROCKET_SIZE, Z_ROCKET};

    // Offset based on which rocket in the salvo (spread horizontally)
    let offset_x = (rocket_index as f32 - 1.5) * ROCKET_LAUNCH_SPREAD * 0.3;

    let rocket = Rocket::new(offset_x);
    let sprite = self.rocket_sprite.clone();

    // Trail for rocket
    let trail = Trail2D::new(Color::new(255, 150, 50, 180), 4.0)
      .with_length(30)
      .with_fade(true)
      .with_taper(0.8)
      .with_min_distance(3.0);

    let entity = ctx.world.spawn((
      Transform::new(pos.x + offset_x, pos.y).with_pivot_center(),
      Renderable::sprite(sprite)
        .with_z_order(Z_ROCKET)
        .with_shader(SpriteShader::outline(Color::rgb(255, 190, 130), 2))
        .interpolated(),
      RigidBody::kinematic(),
      Collider::circle_collider(ROCKET_SIZE / 2.0),
      rocket,
      trail,
    ));

    self.rockets.push(entity);

    // Play launch sound
    if let Some(audio) = &mut ctx.audio {
      let _ = audio.play_sound_with("bullet", SoundOptions::new().volume(0.2));
    }
  }

  fn update_rockets(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Get player position for calibration
    let player_pos = self
      .player
      .and_then(|e| ctx.world.get::<Transform>(e).map(|t| Vec2::new(t.x, t.y)))
      .unwrap_or(Vec2::new(self.screen_width / 2.0, self.screen_height / 2.0));

    let mut rockets_to_remove: Vec<Entity> = Vec::new();
    let mut player_hit = false;

    for &entity in &self.rockets {
      // Get rocket component and position
      let Some(mut rocket) = ctx.world.get_mut::<Rocket>(entity) else {
        continue;
      };

      let current_pos = ctx
        .world
        .get::<Transform>(entity)
        .map(|t| Vec2::new(t.x, t.y))
        .unwrap_or(Vec2::ZERO);

      // Update rocket
      rocket.update(current_pos, player_pos, dt);
      let velocity = rocket.velocity;
      let rotation = rocket.rotation();
      let is_dangerous = rocket.is_dangerous();
      drop(rocket);

      // Apply movement
      if let Some(mut t) = ctx.world.get_mut::<Transform>(entity) {
        t.x += velocity.x * dt;
        t.y += velocity.y * dt;
        t.rotation = rotation;
      }

      // Trail is automatically updated by the engine's trail system

      // Check bounds - remove if off screen
      let new_pos = ctx
        .world
        .get::<Transform>(entity)
        .map(|t| (t.x, t.y))
        .unwrap_or((0.0, 0.0));
      if new_pos.0 < -50.0
        || new_pos.0 > self.screen_width + 50.0
        || new_pos.1 < -50.0
        || new_pos.1 > self.screen_height + 50.0
      {
        rockets_to_remove.push(entity);
        continue;
      }

      // Check player collision (only when dangerous)
      if is_dangerous && !player_hit && !self.player_invincible {
        let dist = (Vec2::new(new_pos.0, new_pos.1) - player_pos).length();
        if dist < PLAYER_SIZE / 2.0 + ROCKET_SIZE / 2.0 {
          player_hit = true;
          rockets_to_remove.push(entity);
        }
      }
    }

    // Handle player hit
    if player_hit {
      self.apply_player_damage(ctx);

      // Spawn explosion at player
      let player_explosion_pos = self
        .player
        .and_then(|p| ctx.world.get::<Transform>(p).map(|t| (t.x, t.y)));
      if let Some((x, y)) = player_explosion_pos {
        self.create_explosion(ctx, x, y);
      }
    }

    // Collect rocket positions for explosions before removing
    let rocket_positions: Vec<(Entity, f32, f32)> = rockets_to_remove
      .iter()
      .filter_map(|&e| ctx.world.get::<Transform>(e).map(|t| (e, t.x, t.y)))
      .collect();

    // Remove rockets and create explosions
    for (entity, x, y) in rocket_positions {
      self.create_explosion(ctx, x, y);
      let _ = ctx.world.despawn(entity);
      self.rockets.retain(|&e| e != entity);
    }
  }

  fn update_player(&mut self, ctx: &mut SceneContext, dt: f32) {
    profile_scope!("player");
    use crate::components::WallSide;
    use crate::constants::{
      WALL_JUMP_FORCE_X, WALL_JUMP_FORCE_Y, WALL_SLIDE_GRAVITY_SCALE, WALL_SLIDE_MAX_SPEED,
    };

    let Some(player_entity) = self.player else {
      return;
    };

    // Get components
    let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) else {
      return;
    };
    let was_invincible = player.is_invincible();
    let was_grounded = player.is_grounded;
    let was_wall_sliding = player.wall_sliding;
    let can_perform_wall_jump = player.can_perform_wall_jump();
    let wall_jump_side = player.wall_jump_side();
    let is_wall_jump_locked = player.is_wall_jump_locked();
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
      // Restore outline shader when invincibility just ended
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(player_entity) {
        renderable.shader = Some(SpriteShader::outline(Color::rgb(0, 0, 0), 2));
      }
    }

    let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) else {
      return;
    };

    // Horizontal movement (skip if wall jump push is active)
    if !is_wall_jump_locked {
      let mut move_x = 0.0;
      // Keyboard input
      if ctx.input.key_down(KeyCode::KeyA) || ctx.input.key_down(KeyCode::ArrowLeft) {
        move_x -= 1.0;
      }
      if ctx.input.key_down(KeyCode::KeyD) || ctx.input.key_down(KeyCode::ArrowRight) {
        move_x += 1.0;
      }
      // Gamepad input (additive - left stick or D-pad)
      move_x += ctx.input.left_stick_x();
      if ctx.input.gamepad_button_down(GamepadButton::DPadLeft) {
        move_x -= 1.0;
      }
      if ctx.input.gamepad_button_down(GamepadButton::DPadRight) {
        move_x += 1.0;
      }
      // Clamp to -1.0..1.0 range
      move_x = move_x.clamp(-1.0, 1.0);
      velocity.dx = move_x * PLAYER_SPEED;
    }

    let jump_pressed = ctx.input.key_pressed(KeyCode::Space)
      || ctx.input.key_pressed(KeyCode::KeyW)
      || ctx.input.gamepad_button_pressed(GamepadButton::South);

    // Jumping - normal ground jump or wall jump
    if jump_pressed {
      if was_grounded {
        // Normal ground jump
        velocity.dy = -PLAYER_JUMP_FORCE;
        drop(velocity);
        if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
          player.is_grounded = false;
          player.wall_sliding = None;
        }
        // Add elastic squash-and-stretch effect on jump
        // First stretch the sprite, then tween back to normal with elastic bounce
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
          transform.scale_x = 0.7;
          transform.scale_y = 1.4;
        }
        let _ = ctx.world.insert_one(
          player_entity,
          Tween::to_scale(1.0, 1.0, 0.5).with_easing(Ease::OutElastic),
        );
      } else if can_perform_wall_jump && wall_jump_side.is_some() {
        // Wall jump - push away from wall with forced momentum
        // Works both when actively sliding AND during grace period after releasing wall direction
        let wall_side = wall_jump_side.unwrap();
        velocity.dy = -WALL_JUMP_FORCE_Y;
        velocity.dx = match wall_side {
          WallSide::Left => WALL_JUMP_FORCE_X,   // Push right
          WallSide::Right => -WALL_JUMP_FORCE_X, // Push left
        };
        drop(velocity);
        if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
          player.start_wall_jump(); // Sets cooldown to prevent horizontal override
          player.wall_sliding = None;
        }
        // Add elastic squash-and-stretch effect on wall jump
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
          transform.scale_x = 0.7;
          transform.scale_y = 1.4;
        }
        let _ = ctx.world.insert_one(
          player_entity,
          Tween::to_scale(1.0, 1.0, 0.5).with_easing(Ease::OutElastic),
        );
      } else {
        drop(velocity);
      }
    } else {
      drop(velocity);
    }

    // Check if player is standing on a moving barrier and ride it
    let riding_barrier = self.apply_barrier_riding(ctx, player_entity);

    // Apply gravity (skip if riding a barrier - they're supported)
    if !riding_barrier && let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
      // Apply gravity - reduced when wall sliding for smoother slide
      if was_wall_sliding.is_some() && velocity.dy > 0.0 {
        // Wall sliding: reduced gravity and capped max speed
        velocity.dy += GRAVITY * WALL_SLIDE_GRAVITY_SCALE * dt;
        velocity.dy = velocity.dy.min(WALL_SLIDE_MAX_SPEED);
      } else {
        // Normal gravity
        velocity.dy += GRAVITY * dt;
      }
    }

    // Apply velocity to transform (player is kinematic, so we handle movement manually)
    let Some(velocity) = ctx.world.get::<Velocity>(player_entity) else {
      return;
    };
    let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) else {
      return;
    };

    transform.x += velocity.dx * dt;
    // Skip Y velocity when riding barrier - apply_barrier_riding already snapped position
    if !riding_barrier {
      transform.y += velocity.dy * dt;
    }

    let tx = transform.x;
    let ty = transform.y;
    let vel_y = velocity.dy;
    drop(velocity);
    drop(transform);

    // Wall slide detection - check if player is against screen edge walls or barriers and falling
    // Player must be pressing towards the wall to initiate wall slide
    let pressing_left = ctx.input.key_down(KeyCode::KeyA)
      || ctx.input.key_down(KeyCode::ArrowLeft)
      || ctx.input.left_stick_x() < -0.5
      || ctx.input.gamepad_button_down(GamepadButton::DPadLeft);
    let pressing_right = ctx.input.key_down(KeyCode::KeyD)
      || ctx.input.key_down(KeyCode::ArrowRight)
      || ctx.input.left_stick_x() > 0.5
      || ctx.input.gamepad_button_down(GamepadButton::DPadRight);

    let mut new_wall_slide: Option<WallSide> = None;
    if vel_y > 0.0 && !was_grounded {
      // Falling and not grounded (player uses center pivot)
      let half_size = PLAYER_SIZE / 2.0;
      if tx <= half_size && pressing_left {
        // Against left screen wall and pressing into it
        new_wall_slide = Some(WallSide::Left);
      } else if tx >= self.screen_width - half_size && pressing_right {
        // Against right screen wall and pressing into it
        new_wall_slide = Some(WallSide::Right);
      } else {
        // Check if against a barrier (moving vertical platform)
        new_wall_slide = self.check_wall_slide_on_barrier(ctx, player_entity);
      }
    }

    // Update wall slide state
    if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
      if new_wall_slide.is_some() && was_wall_sliding.is_none() {
        // Just started wall sliding - can wall jump
        player.can_wall_jump = true;
        player.wall_slide_grace = 0.0; // Clear any existing grace period
      } else if new_wall_slide.is_none() && was_wall_sliding.is_some() {
        // Just released wall - start grace period for wall jump
        player.start_wall_slide_grace(was_wall_sliding.unwrap());
      }
      player.wall_sliding = new_wall_slide;

      // Reset wall jump ability when grounded
      if player.is_grounded {
        player.wall_sliding = None;
        player.can_wall_jump = false;
        player.wall_slide_grace = 0.0;
        player.last_wall_side = None;
      }
    }

    // Screen bounds (player uses center pivot)
    let half_size = PLAYER_SIZE / 2.0;
    if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
      transform.x = transform.x.clamp(half_size, self.screen_width - half_size);
    }

    // Ground collision (player uses center pivot, bottom edge is at y + half_size)
    if ty >= self.screen_height - half_size - 30.0 {
      // -30 accounts for ground platform thickness
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
        transform.y = self.screen_height - half_size - 30.0;
      }
      if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
        velocity.dy = 0.0;
      }
      if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
        player.is_grounded = true;
        player.wall_sliding = None;
      }
    } else {
      // Check platform, barrier, turret, and drone collisions
      self.check_player_platform_collision(ctx, player_entity);
      self.check_player_barrier_collision(ctx, player_entity);
      self.check_player_turret_collision(ctx, player_entity);
      self.check_player_drone_collision(ctx, player_entity);
    }
  }

  fn check_player_platform_collision(&mut self, ctx: &mut SceneContext, player_entity: Entity) {
    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return;
    };
    let Some(player_velocity) = ctx.world.get::<Velocity>(player_entity) else {
      return;
    };

    // Player uses center pivot
    let half_size = PLAYER_SIZE / 2.0;
    let player_left = player_transform.x - half_size;
    let player_right = player_transform.x + half_size;
    let player_top = player_transform.y - half_size;
    let player_bottom = player_transform.y + half_size;
    let vel_y = player_velocity.dy;
    let falling = vel_y > 0.0;

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

      // Check horizontal overlap (must be within platform X bounds)
      let horizontal_overlap = player_right > platform_left && player_left < platform_right;

      if !horizontal_overlap {
        continue;
      }

      // Check if player is falling and has crossed or is on the platform
      // Use velocity-based tolerance to prevent tunneling at high speeds
      // Tolerance = max(15, velocity * dt) to catch fast-moving players
      let velocity_tolerance = (vel_y.abs() / 60.0).max(15.0); // Assume ~60fps for tolerance calc

      if falling
        && player_bottom >= platform_top
        && player_top < platform_bottom
        && player_bottom <= platform_top + velocity_tolerance
      {
        // Landing on top of platform - snap player to platform top (center pivot)
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
          transform.y = platform_top - half_size;
        }
        if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
          velocity.dy = 0.0;
        }
        if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
          player.is_grounded = true;
        }
        break;
      }
    }
  }

  fn check_player_barrier_collision(&mut self, ctx: &mut SceneContext, player_entity: Entity) {
    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return;
    };
    let Some(player_velocity) = ctx.world.get::<Velocity>(player_entity) else {
      return;
    };

    // Player uses center pivot
    let half_size = PLAYER_SIZE / 2.0;
    let player_left = player_transform.x - half_size;
    let player_right = player_transform.x + half_size;
    let player_top = player_transform.y - half_size;
    let player_bottom = player_transform.y + half_size;
    let vel_x = player_velocity.dx;
    let vel_y = player_velocity.dy;

    drop(player_transform);
    drop(player_velocity);

    for &barrier_entity in &self.barriers {
      let Some(barrier_transform) = ctx.world.get::<Transform>(barrier_entity) else {
        continue;
      };
      let barrier_vel_y = ctx
        .world
        .get::<Barrier>(barrier_entity)
        .map(|b| b.velocity_y)
        .unwrap_or(0.0);

      let barrier_left = barrier_transform.x;
      let barrier_right = barrier_transform.x + BARRIER_WIDTH;
      let barrier_top = barrier_transform.y;
      let barrier_bottom = barrier_transform.y + BARRIER_HEIGHT;

      drop(barrier_transform);

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
            // Push left (center pivot)
            transform.x = barrier_left - half_size;
          } else if min_overlap == overlap_right && vel_x < 0.0 {
            // Push right (center pivot)
            transform.x = barrier_right + half_size;
          } else if min_overlap == overlap_top && vel_y > 0.0 {
            // Push up (landing on top, center pivot)
            transform.y = barrier_top - half_size;
            drop(transform);
            if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
              // Inherit the barrier's vertical velocity so player moves with it
              velocity.dy = barrier_vel_y;
            }
            if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
              player.is_grounded = true;
            }
          } else if min_overlap == overlap_bottom && vel_y < 0.0 {
            // Push down (hitting from below, center pivot)
            transform.y = barrier_bottom + half_size;
            drop(transform);
            if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
              velocity.dy = 0.0;
            }
          }
        }
        break;
      }
    }
  }

  /// Check if player is standing on a barrier and apply its movement
  /// Returns true if player is riding a barrier (gravity should be skipped)
  fn apply_barrier_riding(&self, ctx: &mut SceneContext, player_entity: Entity) -> bool {
    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return false;
    };

    // Player uses center pivot
    let half_size = PLAYER_SIZE / 2.0;
    let player_left = player_transform.x - half_size;
    let player_right = player_transform.x + half_size;
    let player_bottom = player_transform.y + half_size;

    drop(player_transform);

    for &barrier_entity in &self.barriers {
      let Some(barrier_transform) = ctx.world.get::<Transform>(barrier_entity) else {
        continue;
      };
      let Some(barrier) = ctx.world.get::<Barrier>(barrier_entity) else {
        continue;
      };

      let barrier_left = barrier_transform.x;
      let barrier_right = barrier_transform.x + BARRIER_WIDTH;
      let barrier_top = barrier_transform.y;
      let barrier_vel_y = barrier.velocity_y;

      drop(barrier_transform);
      drop(barrier);

      // Check horizontal overlap first
      let horizontal_overlap = player_right > barrier_left && player_left < barrier_right;
      if !horizontal_overlap {
        continue;
      }

      // Use larger tolerance when barrier is moving up to prevent bouncing
      // When barrier moves up, player can briefly separate before next frame
      let tolerance = if barrier_vel_y < 0.0 {
        // Moving up: larger tolerance proportional to speed
        8.0 + (-barrier_vel_y * 0.05).min(10.0)
      } else {
        // Stationary or moving down: standard tolerance
        4.0
      };

      // Check if player is standing on top of this barrier
      // Player's bottom must be near barrier's top
      let distance_to_top = player_bottom - barrier_top;
      let on_top = distance_to_top >= -tolerance && distance_to_top <= tolerance;

      if on_top {
        // Check if player is actively jumping (velocity significantly more negative than barrier)
        // This distinguishes a real jump from just riding an ascending barrier
        let player_vel_y = ctx
          .world
          .get::<Velocity>(player_entity)
          .map(|v| v.dy)
          .unwrap_or(0.0);

        // Player is jumping if their upward velocity exceeds barrier velocity by a threshold
        // (e.g., barrier at -200, player at -400 means player jumped off the barrier)
        let is_jumping = player_vel_y < barrier_vel_y - 50.0;

        // If jumping, don't apply barrier riding - let the jump happen
        if is_jumping {
          return false;
        }

        // Snap player to ride on top of the barrier (prevents bouncing)
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
          transform.y = barrier_top - half_size;
        }

        // Set player velocity to match barrier movement
        if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
          velocity.dy = barrier_vel_y;
        }
        if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
          player.is_grounded = true;
        }
        return true;
      }
    }

    false
  }

  fn check_wall_slide_on_barrier(
    &self,
    ctx: &mut SceneContext,
    player_entity: Entity,
  ) -> Option<WallSide> {
    use crate::components::WallSide;

    let player_transform = ctx.world.get::<Transform>(player_entity)?;

    // Player uses center pivot
    let half_size = PLAYER_SIZE / 2.0;
    let player_left = player_transform.x - half_size;
    let player_right = player_transform.x + half_size;
    let player_top = player_transform.y - half_size;
    let player_bottom = player_transform.y + half_size;

    drop(player_transform);

    // Check input direction (player must be pressing towards the barrier)
    let pressing_right = ctx.input.key_down(KeyCode::KeyD)
      || ctx.input.key_down(KeyCode::ArrowRight)
      || ctx.input.left_stick_x() > 0.5
      || ctx.input.gamepad_button_down(GamepadButton::DPadRight);
    let pressing_left = ctx.input.key_down(KeyCode::KeyA)
      || ctx.input.key_down(KeyCode::ArrowLeft)
      || ctx.input.left_stick_x() < -0.5
      || ctx.input.gamepad_button_down(GamepadButton::DPadLeft);

    // Tolerance for detecting wall contact (larger to account for collision pushback)
    let tolerance = 12.0;

    for &barrier_entity in &self.barriers {
      let Some(barrier_transform) = ctx.world.get::<Transform>(barrier_entity) else {
        continue;
      };

      let barrier_left = barrier_transform.x;
      let barrier_right = barrier_transform.x + BARRIER_WIDTH;
      let barrier_top = barrier_transform.y;
      let barrier_bottom = barrier_transform.y + BARRIER_HEIGHT;

      // Check if player overlaps vertically with barrier
      if player_bottom > barrier_top && player_top < barrier_bottom {
        // Check if player is against left side of barrier (player to the left, pressing right)
        if player_right >= barrier_left - tolerance
          && player_right <= barrier_left + tolerance
          && pressing_right
        {
          return Some(WallSide::Right); // Wall is to player's right, jump will go left
        }

        // Check if player is against right side of barrier (player to the right, pressing left)
        if player_left <= barrier_right + tolerance
          && player_left >= barrier_right - tolerance
          && pressing_left
        {
          return Some(WallSide::Left); // Wall is to player's left, jump will go right
        }
      }
    }

    None
  }

  fn check_player_turret_collision(&mut self, ctx: &mut SceneContext, player_entity: Entity) {
    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return;
    };
    let Some(player_velocity) = ctx.world.get::<Velocity>(player_entity) else {
      return;
    };

    // Player uses center pivot
    let half_size = PLAYER_SIZE / 2.0;
    let player_left = player_transform.x - half_size;
    let player_right = player_transform.x + half_size;
    let player_top = player_transform.y - half_size;
    let player_bottom = player_transform.y + half_size;
    let falling = player_velocity.dy > 0.0;

    drop(player_transform);
    drop(player_velocity);

    for &turret_entity in &self.turrets {
      let Some(turret_transform) = ctx.world.get::<Transform>(turret_entity) else {
        continue;
      };

      let turret_left = turret_transform.x;
      let turret_right = turret_transform.x + TURRET_SIZE;
      let turret_top = turret_transform.y;
      let turret_bottom = turret_transform.y + TURRET_SIZE;

      // Check AABB collision
      if player_right > turret_left
        && player_left < turret_right
        && player_bottom > turret_top
        && player_top < turret_bottom
      {
        // Landing on top of turret (player can use as platform)
        if falling && player_bottom <= turret_top + 15.0 {
          if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
            transform.y = turret_top - half_size;
          }
          if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
            velocity.dy = 0.0;
          }
          if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
            player.is_grounded = true;
            player.wall_sliding = None;
          }
          break;
        }
      }
    }
  }

  fn check_player_drone_collision(&mut self, ctx: &mut SceneContext, player_entity: Entity) {
    use crate::constants::DRONE_SIZE;

    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return;
    };
    let Some(player_velocity) = ctx.world.get::<Velocity>(player_entity) else {
      return;
    };

    // Player uses center pivot
    let half_size = PLAYER_SIZE / 2.0;
    let player_left = player_transform.x - half_size;
    let player_right = player_transform.x + half_size;
    let player_top = player_transform.y - half_size;
    let player_bottom = player_transform.y + half_size;
    let falling = player_velocity.dy > 0.0;

    drop(player_transform);
    drop(player_velocity);

    for &drone_entity in &self.drones {
      let Some(drone_transform) = ctx.world.get::<Transform>(drone_entity) else {
        continue;
      };

      // Drone uses pivot_center, so position is center
      let drone_left = drone_transform.x - DRONE_SIZE / 2.0;
      let drone_right = drone_transform.x + DRONE_SIZE / 2.0;
      let drone_top = drone_transform.y - DRONE_SIZE / 2.0;
      let drone_bottom = drone_transform.y + DRONE_SIZE / 2.0;

      drop(drone_transform);

      // Check AABB collision
      if player_right > drone_left
        && player_left < drone_right
        && player_bottom > drone_top
        && player_top < drone_bottom
      {
        // Landing on top of drone (player can use as platform)
        if falling && player_bottom <= drone_top + 15.0 {
          if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
            transform.y = drone_top - half_size;
          }
          if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
            velocity.dy = 0.0;
          }
          if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
            player.is_grounded = true;
            player.wall_sliding = None;
          }
          break;
        }
      }
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
