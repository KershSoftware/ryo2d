//! Combat systems (turrets, bullets, particles, barriers, progression)

use crate::components::*;
use crate::*;
use ryo2d::prelude::*;

use super::GameScene;

impl GameScene {
  pub(super) fn update_barriers(&mut self, ctx: &mut SceneContext, dt: f32) {
    profile_scope!("barriers");
    let mut barriers = ctx.world.query::<(&mut Transform, &mut Barrier)>();
    for (mut transform, mut barrier) in barriers.iter_mut(ctx.world) {
      // Move barrier
      transform.translation.y += barrier.velocity_y * dt;

      // Bounce at bounds
      if transform.translation.y <= barrier.min_y {
        transform.translation.y = barrier.min_y;
        barrier.velocity_y = barrier.velocity_y.abs();
      } else if transform.translation.y >= barrier.max_y {
        transform.translation.y = barrier.max_y;
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
          transform.translation.y,
          turret.side,
          turret.move_direction,
        ));
      }
    }

    for turret_entity in turrets_to_update {
      // Work on copies and write them back below
      let (Some(mut transform), Some(mut turret)) = (
        ctx.world.get::<Transform>(turret_entity).copied(),
        ctx.world.get::<Turret>(turret_entity).copied(),
      ) else {
        continue;
      };

      // Update spawn timer and apply dissolve effect while spawning
      if turret.is_spawning() {
        let just_finished = turret.update_spawn(dt);
        let spawn_progress = turret.spawn_progress();
        let turret_side = turret.side;
        if let Some(mut stored) = ctx.world.get_mut::<Turret>(turret_entity) {
          *stored = turret;
        }

        // Apply dissolve shader for spawn effect (reverse progress for spawn-in)
        if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(turret_entity) {
          if just_finished {
            // Finished spawning - restore normal outline shader and play sound with panning
            renderable.shader = Some(SpriteShader::outline(Color::rgb(255, 140, 140), 2));
            let pan = match turret_side {
              TurretSide::Left => -0.8,
              TurretSide::Right => 0.8,
            };
            let _ = ctx
              .audio
              .play_sound_with("enemy-spawn", SoundOptions::new().volume(0.15).panning(pan));
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
      transform.translation.y += turret.move_direction * self.current_turret_speed * dt;

      // Bounce off screen top and floor platform (limits for the turret's centre)
      let half_size = TURRET_SIZE / 2.0;
      let top_limit = TURRET_SIZE + half_size;
      let floor_top = self.screen_height - GROUND_HEIGHT; // Floor platform top edge
      let bottom_limit = floor_top - half_size; // Turret can't go below this

      if transform.translation.y < top_limit || transform.translation.y > bottom_limit {
        turret.reverse_direction();
        transform.translation.y = transform.translation.y.clamp(top_limit, bottom_limit);
      }

      // Check collision with other turrets on the same side
      let my_y = transform.translation.y;
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
              transform.translation.y -= push;
            } else {
              transform.translation.y += push;
            }
            break;
          }
        }
      }

      // Shoot from the turret's centre
      let turret_x = transform.translation.x;
      let turret_y = transform.translation.y;
      let turret_side = turret.side;
      let max_burst = turret.max_burst_size;

      // Shooting logic (only when fully spawned)
      let mut shoot = false;
      let mut burst_started = false;
      if turret.burst_count > 0 {
        turret.burst_timer -= dt;
        if turret.burst_timer <= 0.0 {
          turret.burst_count -= 1;
          turret.burst_timer = TURRET_BURST_DELAY;
          shoot = true;
        }
      } else {
        turret.shoot_timer -= dt;
        if turret.shoot_timer <= 0.0 {
          // Use the turret's max burst size (updated by progression)
          turret.burst_count = max_burst;
          turret.burst_timer = TURRET_BURST_DELAY;
          turret.shoot_timer = TURRET_SHOOT_INTERVAL;
          burst_started = true;
        }
      }

      if let Some(mut stored) = ctx.world.get_mut::<Transform>(turret_entity) {
        *stored = transform;
      }
      if let Some(mut stored) = ctx.world.get_mut::<Turret>(turret_entity) {
        *stored = turret;
      }

      if shoot {
        self.shoot_bullet(ctx, turret_x, turret_y, turret_side);
      }
      if burst_started {
        // Play burst sound with panning based on turret side
        let pan = match turret_side {
          TurretSide::Left => -0.8,
          TurretSide::Right => 0.8,
        };
        let _ = ctx
          .audio
          .play_sound_with("bullet", SoundOptions::new().volume(0.1).panning(pan));
      }
    }
  }

  pub(super) fn update_bullets(&mut self, ctx: &mut SceneContext, _dt: f32) {
    profile_scope!("bullets");
    // Bullets are dynamic triggers: the last physics step reported each one
    // entering the player, a platform or a barrier (one-way platforms
    // included: their one-way rule only shapes solid contacts)
    let mut spent: Vec<Entity> = Vec::new();
    for hit in ctx.messages::<TriggerEntered>() {
      let (bullet, actor) = (hit.trigger, hit.actor());
      if spent.contains(&bullet) || !self.bullets.contains(&bullet) {
        continue;
      }
      let hit_player = self.player == Some(actor);
      if !hit_player && !self.platforms.contains(&actor) && !self.barriers.contains(&actor) {
        continue;
      }
      if hit_player {
        // The bullet bursts on the player even while it is invincible
        self.apply_player_damage(ctx);
      }
      if let Some(transform) = ctx.world.get::<Transform>(bullet) {
        let at = transform.translation;
        self.create_explosion(ctx, at.x, at.y);
      }
      spent.push(bullet);
    }

    // Bullets that left the screen (or lost their components)
    for &bullet in &self.bullets {
      let out = match ctx.world.get::<Transform>(bullet) {
        Some(transform) if ctx.world.get::<Bullet>(bullet).is_some() => {
          let Vec2 { x, y } = transform.translation;
          x < -50.0 || x > self.screen_width + 50.0 || y < -50.0 || y > self.screen_height + 50.0
        }
        _ => true,
      };
      if out && !spent.contains(&bullet) {
        spent.push(bullet);
      }
    }

    // Release spent bullets back to the pool (or despawn them if not pooled)
    self.bullets.retain(|bullet| !spent.contains(bullet));
    for bullet in spent {
      if let Some(ref mut pool) = self.bullet_pool {
        pool.release(ctx.world, bullet);
      } else {
        ctx.world.despawn(bullet);
      }
    }
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
        particles_to_remove.push(i);
        ctx.world.despawn(particle_entity);
        continue;
      }

      let vx = particle.velocity_x;
      let vy = particle.velocity_y;
      let _alpha = (particle.alpha() * 255.0) as u8;

      // Move particle
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(particle_entity) {
        transform.translation.x += vx * dt;
        transform.translation.y += vy * dt;
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
