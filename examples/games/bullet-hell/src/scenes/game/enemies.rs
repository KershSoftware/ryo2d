//! Enemy update systems (drones, nebulas, rockets, lasers)

use crate::components::*;
use crate::*;
use rand::Rng;
use ryo2d::prelude::*;

use super::{GameScene, NebulaOrbMarker};

impl GameScene {
  pub(super) fn update_drones(&mut self, ctx: &mut SceneContext, dt: f32) {
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
  pub(super) fn update_laser_tracers(&mut self, ctx: &mut SceneContext, dt: f32) {
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
  pub(super) fn extend_line_to_screen_edge(
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

  pub(super) fn spawn_nebula(&mut self, ctx: &mut SceneContext) {
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

  pub(super) fn update_nebulas(&mut self, ctx: &mut SceneContext, dt: f32) {
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

  pub(super) fn update_nebula_orb_positions(&self, ctx: &mut SceneContext, nebula_entity: Entity) {
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

  pub(super) fn update_nebula_orb_shaders(
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

  pub(super) fn update_nebula_orb_charge_effect(
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

  pub(super) fn spawn_rocket(&mut self, ctx: &mut SceneContext, pos: Vec2, rocket_index: u32) {
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

  pub(super) fn update_rockets(&mut self, ctx: &mut SceneContext, dt: f32) {
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
}
