//! Combat systems (turrets, bullets, particles, barriers, progression)

use crate::components::*;
use crate::*;
use ryo2d::prelude::*;

use super::GameScene;

impl GameScene {
  pub(super) fn update_barriers(&mut self, ctx: &mut SceneContext, dt: f32) {
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

  pub(super) fn update_turrets(&mut self, ctx: &mut SceneContext, dt: f32) {
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

  pub(super) fn update_bullets(&mut self, ctx: &mut SceneContext, _dt: f32) {
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
    bullets_to_release.sort_by_key(|&(i, _)| std::cmp::Reverse(i));
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

  pub(super) fn check_bullet_player_collision(
    &mut self,
    ctx: &mut SceneContext,
    bx: f32,
    by: f32,
  ) -> bool {
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

  pub(super) fn check_bullet_platform_collision(
    &self,
    ctx: &mut SceneContext,
    bx: f32,
    by: f32,
  ) -> bool {
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

  pub(super) fn check_bullet_barrier_collision(
    &self,
    ctx: &mut SceneContext,
    bx: f32,
    by: f32,
  ) -> bool {
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

  pub(super) fn update_particles(&mut self, ctx: &mut SceneContext, dt: f32) {
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

  pub(super) fn check_game_over(&self, ctx: &mut SceneContext) -> bool {
    if let Some(player_entity) = self.player
      && let Some(player) = ctx.world.get::<Player>(player_entity)
    {
      return !player.is_alive();
    }
    false
  }

  pub(super) fn update_progression(&mut self, ctx: &mut SceneContext, dt: f32) {
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
