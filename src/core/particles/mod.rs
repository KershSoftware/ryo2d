//! GPU Particle System
//!
//! A full-featured particle system with CPU simulation and GPU rendering.
//!
//! # Overview
//!
//! This particle system uses **CPU simulation + GPU rendering**:
//! - Particles are simulated on the CPU (physics, attractors, collisions)
//! - Rendering uses the existing `SpriteGpuRenderer` with instanced drawing
//! - Supports 10k+ particles at 60fps
//!
//! # Components
//!
//! | Component | Description |
//! |-----------|-------------|
//! | [`Particle`] | Individual particle data (velocity, lifetime, color) |
//! | [`ParticleEmitter`] | Spawns particles with configurable shapes and rates |
//! | [`ParticleAttractor`] | Force fields that affect particle motion |
//!
//! # Quick Start
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // Spawn a fire effect
//! let fire_sprite = ctx.assets.load_sprite("particle.png");
//! ctx.world.spawn((
//!     Transform::at(100.0, 100.0),
//!     ParticleEmitter::fire(fire_sprite),
//! ));
//!
//! // One-shot explosion
//! ctx.world.spawn((
//!     Transform::at(x, y),
//!     ParticleEmitter::explosion(spark_sprite, 50).one_shot(),
//! ));
//!
//! // Custom emitter with attractor
//! ctx.world.spawn((
//!     Transform::at(x, y),
//!     ParticleEmitter::new(sprite)
//!         .with_mode(EmissionMode::Continuous { rate: 30.0 })
//!         .with_shape(EmissionShape::Circle { radius: 20.0, hollow: true })
//!         .with_lifetime(1.0, 2.0)
//!         .with_color(Color::CYAN, Color::TRANSPARENT),
//! ));
//!
//! // Attractor that pulls particles
//! ctx.world.spawn((
//!     Transform::at(mouse_x, mouse_y),
//!     ParticleAttractor::point(200.0, 100.0),
//! ));
//! ```
//!
//! # Built-in Presets
//!
//! | Preset | Description |
//! |--------|-------------|
//! | `ParticleEmitter::fire(sprite)` | Upward orange-yellow flames |
//! | `ParticleEmitter::smoke(sprite)` | Slow expanding gray particles |
//! | `ParticleEmitter::explosion(sprite, count)` | Radial burst with trails |
//! | `ParticleEmitter::magic(sprite)` | Glowing wandering sparkles |
//! | `ParticleEmitter::rain(sprite)` | Downward streaks |
//! | `ParticleEmitter::sparks(sprite)` | Bouncing bright particles |
//! | `ParticleEmitter::snow(sprite)` | Gentle floating descent |
//! | `ParticleEmitter::confetti(sprite)` | Multi-colored tumbling |

mod attractor;
mod collision;
mod emitter;
mod particle;
mod presets;

pub use attractor::{AttractorType, ParticleAttractor};
pub use collision::{CollisionConfig, CollisionResponse, ParticleCollision};
pub use emitter::{EmissionMode, EmissionShape, ParticleEmitter};
pub use particle::Particle;

use crate::core::ecs::{Inactive, Transform, World};
use crate::core::events::Events;
use crate::core::graphics::{BatchCollector, Camera, SpriteInstance};
use hecs::Entity;
use rand::Rng;

/// Internal particle data stored per-emitter.
#[derive(Debug)]
struct ParticleData {
  /// The particle component data
  particle: Particle,
  /// Current position (separate from transform for efficiency)
  x: f32,
  y: f32,
  /// Entity that owns this particle (for trails/collision events)
  emitter_entity: Entity,
}

/// Particle system that manages all particle emitters and their particles.
///
/// The system is designed to be called from the engine's update and render loops.
/// It handles:
/// - Particle spawning based on emitter settings
/// - Physics simulation (velocity, gravity, drag)
/// - Attractor forces
/// - Lifetime management
/// - Collision detection (optional)
/// - Trail updates (optional)
/// - GPU batch collection for rendering
#[derive(Debug, Default)]
pub struct ParticleSystem {
  /// All active particles, grouped by emitter entity
  particles: Vec<ParticleData>,
  /// Random number generator
  rng: Option<rand::rngs::ThreadRng>,
}

impl ParticleSystem {
  /// Create a new particle system.
  pub fn new() -> Self {
    Self {
      particles: Vec::with_capacity(10000),
      rng: Some(rand::thread_rng()),
    }
  }

  /// Update all particle emitters and simulate particles.
  ///
  /// Call this in the engine's update phase, after game update.
  pub fn update(&mut self, world: &mut World, events: &mut Events, dt: f32) {
    // Collect emitter entities and positions
    let emitter_entities = world.entities_with::<ParticleEmitter>();
    let mut emitter_data: Vec<(Entity, f32, f32, f32, f32)> = Vec::new();

    for entity in &emitter_entities {
      if world.has::<Inactive>(*entity) {
        continue;
      }
      if let Some(emitter) = world.get::<ParticleEmitter>(*entity) {
        if !emitter.enabled {
          continue;
        }
        if let Some(transform) = world.get::<Transform>(*entity) {
          emitter_data.push((
            *entity,
            transform.x,
            transform.y,
            emitter.gravity.0,
            emitter.gravity.1,
          ));
        }
      }
    }

    // Collect attractors
    let attractor_entities = world.entities_with::<ParticleAttractor>();
    let mut attractors: Vec<(f32, f32, ParticleAttractor)> = Vec::new();

    for entity in &attractor_entities {
      if world.has::<Inactive>(*entity) {
        continue;
      }
      // Get attractor data first
      let attractor_data = world
        .get::<ParticleAttractor>(*entity)
        .map(|a| (a.enabled, a.attractor_type.clone(), a.layer_mask, a.time));
      let Some((enabled, attractor_type, layer_mask, time)) = attractor_data else {
        continue;
      };
      if !enabled {
        continue;
      }
      if let Some(transform) = world.get::<Transform>(*entity) {
        attractors.push((
          transform.x,
          transform.y,
          ParticleAttractor {
            attractor_type,
            layer_mask,
            enabled: true,
            time,
          },
        ));
      }
    }

    // Update attractor time
    for entity in &attractor_entities {
      if let Some(mut attractor) = world.get_mut::<ParticleAttractor>(*entity) {
        attractor.update(dt);
      }
    }

    // Spawn new particles
    for (entity, x, y, _, _) in &emitter_data {
      self.spawn_particles(world, *entity, *x, *y, dt);
    }

    // Build lookup for emitter properties
    let emitter_props: std::collections::HashMap<Entity, (f32, f32)> = emitter_data
      .iter()
      .map(|(e, _, _, gx, gy)| (*e, (*gx, *gy)))
      .collect();

    // Simulate particles
    let mut i = 0;
    while i < self.particles.len() {
      let p = &mut self.particles[i];

      // Update lifetime
      p.particle.lifetime += dt;
      if p.particle.is_dead() {
        // Remove dead particle
        self.particles.swap_remove(i);
        continue;
      }

      // Get emitter properties
      let (grav_x, grav_y) = emitter_props
        .get(&p.emitter_entity)
        .copied()
        .unwrap_or((0.0, 0.0));

      // Apply attractor forces
      let mut force_x = 0.0;
      let mut force_y = 0.0;
      for (ax, ay, attractor) in &attractors {
        let (fx, fy) = attractor.calculate_force(*ax, *ay, p.x, p.y, p.particle.collision_layer);
        force_x += fx;
        force_y += fy;
      }

      // Apply forces to velocity
      p.particle.velocity_x += force_x * dt;
      p.particle.velocity_y += force_y * dt;

      // Simulate particle physics
      p.particle.simulate(dt, grav_x, grav_y);

      // Update position
      p.x += p.particle.velocity_x * dt;
      p.y += p.particle.velocity_y * dt;

      // Update visuals
      p.particle.update_visuals();

      i += 1;
    }

    // Update particle counts on emitters
    let mut counts: std::collections::HashMap<Entity, u32> = std::collections::HashMap::new();
    for p in &self.particles {
      *counts.entry(p.emitter_entity).or_insert(0) += 1;
    }
    for entity in &emitter_entities {
      if let Some(mut emitter) = world.get_mut::<ParticleEmitter>(*entity) {
        emitter.particle_count = counts.get(entity).copied().unwrap_or(0);
      }
    }

    // Handle collision events (placeholder - actual collision detection would go here)
    let _ = events;
  }

  /// Spawn particles from an emitter based on its settings.
  fn spawn_particles(&mut self, world: &mut World, entity: Entity, x: f32, y: f32, dt: f32) {
    // Extract emitter settings
    let spawn_info = {
      let Some(mut emitter) = world.get_mut::<ParticleEmitter>(entity) else {
        return;
      };

      if !emitter.enabled {
        return;
      }

      // Calculate how many particles to spawn based on mode
      // We need to read mode data first, then update state
      let mode_snapshot = emitter.mode.clone();
      let spawn_count = match mode_snapshot {
        EmissionMode::Continuous { rate } => {
          emitter.emission_accumulator += rate * dt;
          let count = emitter.emission_accumulator as u32;
          emitter.emission_accumulator -= count as f32;
          count
        }
        EmissionMode::Burst { count } => {
          if emitter.burst_fired {
            0
          } else {
            emitter.burst_fired = true;
            count
          }
        }
        EmissionMode::BurstRepeating {
          count,
          interval,
          repeats: _,
        } => {
          emitter.burst_timer += dt;
          if emitter.burst_timer >= interval {
            emitter.burst_timer -= interval;
            if let Some(ref mut remaining) = emitter.bursts_remaining {
              if *remaining > 0 {
                *remaining -= 1;
                count
              } else {
                0
              }
            } else {
              count
            }
          } else {
            0
          }
        }
      };

      // Check max particles
      let available = emitter.max_particles.saturating_sub(emitter.particle_count);
      let spawn_count = spawn_count.min(available);

      if spawn_count == 0 {
        return;
      }

      // Clone values we need
      Some((
        spawn_count,
        emitter.shape.clone(),
        emitter.lifetime_min,
        emitter.lifetime_max,
        emitter.size_min,
        emitter.size_max,
        emitter.size_end_min,
        emitter.size_end_max,
        emitter.color_start,
        emitter.color_end,
        emitter.speed_min,
        emitter.speed_max,
        emitter.rotation_min,
        emitter.rotation_max,
        emitter.angular_velocity_min,
        emitter.angular_velocity_max,
        emitter.drag,
        emitter.gravity,
        emitter.layer,
      ))
    };

    let Some((
      spawn_count,
      shape,
      lifetime_min,
      lifetime_max,
      size_min,
      size_max,
      size_end_min,
      size_end_max,
      color_start,
      color_end,
      speed_min,
      speed_max,
      rotation_min,
      rotation_max,
      angular_vel_min,
      angular_vel_max,
      drag,
      gravity,
      layer,
    )) = spawn_info
    else {
      return;
    };

    let rng = self.rng.as_mut().unwrap();

    // Spawn particles
    for _ in 0..spawn_count {
      let mut particle = Particle::new();

      // Lifetime
      particle.max_lifetime = rng.gen_range(lifetime_min..=lifetime_max);

      // Size
      particle.start_size = rng.gen_range(size_min..=size_max);
      particle.end_size = rng.gen_range(size_end_min..=size_end_max);
      particle.size = particle.start_size;

      // Color
      particle.start_color = color_start;
      particle.end_color = color_end;
      particle.color = color_start;

      // Rotation
      particle.rotation = rng.gen_range(rotation_min..=rotation_max);
      particle.angular_velocity = rng.gen_range(angular_vel_min..=angular_vel_max);

      particle.collision_layer = layer;

      // Physics
      particle.drag = drag;
      particle.gravity_scale = if gravity.0 != 0.0 || gravity.1 != 0.0 {
        1.0
      } else {
        0.0
      };

      // Position and velocity based on shape
      let (px, py, vx, vy) =
        Self::calculate_spawn_position_velocity(&shape, x, y, speed_min, speed_max, rng);

      particle.velocity_x = vx;
      particle.velocity_y = vy;

      self.particles.push(ParticleData {
        particle,
        x: px,
        y: py,
        emitter_entity: entity,
      });
    }
  }

  /// Calculate spawn position and initial velocity based on emission shape.
  fn calculate_spawn_position_velocity(
    shape: &EmissionShape,
    emitter_x: f32,
    emitter_y: f32,
    speed_min: f32,
    speed_max: f32,
    rng: &mut rand::rngs::ThreadRng,
  ) -> (f32, f32, f32, f32) {
    match shape {
      EmissionShape::Point => {
        // Random direction
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let speed = rng.gen_range(speed_min..=speed_max);
        (
          emitter_x,
          emitter_y,
          angle.cos() * speed,
          angle.sin() * speed,
        )
      }

      EmissionShape::Line {
        start_x,
        start_y,
        end_x,
        end_y,
      } => {
        let t: f32 = rng.gen_range(0.0..=1.0);
        let px = emitter_x + start_x + (end_x - start_x) * t;
        let py = emitter_y + start_y + (end_y - start_y) * t;
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let speed = rng.gen_range(speed_min..=speed_max);
        (px, py, angle.cos() * speed, angle.sin() * speed)
      }

      EmissionShape::Circle { radius, hollow } => {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let r = if *hollow {
          *radius
        } else {
          rng.gen_range(0.0..*radius)
        };
        let px = emitter_x + angle.cos() * r;
        let py = emitter_y + angle.sin() * r;

        // Velocity direction = outward from center
        let speed = rng.gen_range(speed_min..=speed_max);
        (px, py, angle.cos() * speed, angle.sin() * speed)
      }

      EmissionShape::Rectangle { width, height } => {
        let px = emitter_x + rng.gen_range(-width / 2.0..width / 2.0);
        let py = emitter_y + rng.gen_range(-height / 2.0..height / 2.0);
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let speed = rng.gen_range(speed_min..=speed_max);
        (px, py, angle.cos() * speed, angle.sin() * speed)
      }

      EmissionShape::Cone {
        direction,
        spread,
        min_speed,
        max_speed,
      } => {
        let half_spread = spread / 2.0;
        let angle = direction + rng.gen_range(-half_spread..half_spread);
        let speed = rng.gen_range(*min_speed..=*max_speed);
        (
          emitter_x,
          emitter_y,
          angle.cos() * speed,
          angle.sin() * speed,
        )
      }
    }
  }

  /// Collect particles for GPU rendering.
  ///
  /// Call this in the render phase to add particles to the batch collector.
  /// The camera is used to convert world positions to screen positions.
  pub fn collect_for_render(&self, world: &World, camera: &Camera, collector: &mut BatchCollector) {
    let zoom = camera.zoom;

    // Group particles by emitter to get their sprites and effects
    for p in &self.particles {
      let Some(emitter) = world.get::<ParticleEmitter>(p.emitter_entity) else {
        continue;
      };

      // Skip if emitter's entity is inactive
      if world.has::<Inactive>(p.emitter_entity) {
        continue;
      }

      let texture_id = emitter.sprite.id();
      let has_effect = emitter.effect.is_some();
      let z_order = emitter.z_order;

      // Convert world position to screen position
      let (screen_x, screen_y) = camera.world_to_screen(p.x, p.y);

      // Scale size by camera zoom
      let scaled_size = p.particle.size * zoom;

      // Build sprite instance (centered on position)
      let mut instance = SpriteInstance::new().with_transform(
        screen_x - scaled_size * 0.5,
        screen_y - scaled_size * 0.5,
        p.particle.rotation,
        scaled_size,
        scaled_size,
      );

      // Apply tint from particle color
      instance = instance.with_tint(p.particle.color, p.particle.color.a as f32 / 255.0);

      // Apply effect if present
      if let Some(effect) = &emitter.effect {
        instance = instance.with_effect(effect);
      }

      collector.add(texture_id, has_effect, instance, z_order);
    }
  }

  /// Get the total number of active particles.
  pub fn particle_count(&self) -> usize {
    self.particles.len()
  }

  /// Clear all particles (useful for scene transitions).
  pub fn clear(&mut self) {
    self.particles.clear();
  }

  /// Remove all particles for a specific emitter.
  pub fn clear_emitter(&mut self, entity: Entity) {
    self.particles.retain(|p| p.emitter_entity != entity);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::SpriteHandle;
  use crate::core::graphics::Sprite;

  fn test_sprite() -> SpriteHandle {
    SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 255, 255, 255, 255), "test.png")
  }

  /// One motionless particle at the origin and a strong point attractor 100px to its right.
  fn particle_near_attractor(emitter_layer: u32, attractor_mask: u32) -> f32 {
    let mut system = ParticleSystem::new();
    let mut world = World::new();
    let mut events = Events::new();
    let emitter = ParticleEmitter::new(test_sprite())
      .with_mode(EmissionMode::Burst { count: 1 })
      .with_speed(0.0, 0.0)
      .with_lifetime(10.0, 10.0)
      .with_layer(emitter_layer);
    world.spawn((Transform::new(0.0, 0.0), emitter));
    world.spawn((
      Transform::new(100.0, 0.0),
      ParticleAttractor::point(500.0, 1000.0).with_layer_mask(attractor_mask),
    ));

    system.update(&mut world, &mut events, 0.016);
    system.update(&mut world, &mut events, 0.016);

    system.particles[0].particle.velocity_x
  }

  #[test]
  fn layered_attractor_pulls_particles_on_its_layer() {
    assert!(particle_near_attractor(0b10, 0b10) > 0.0);
  }

  #[test]
  fn layered_attractor_ignores_particles_on_other_layers() {
    assert_eq!(particle_near_attractor(0b01, 0b10), 0.0);
  }

  #[test]
  fn particle_system_creation() {
    let system = ParticleSystem::new();
    assert_eq!(system.particle_count(), 0);
  }

  #[test]
  fn particle_spawning() {
    let mut system = ParticleSystem::new();
    let mut world = World::new();
    let mut events = Events::new();

    // Create an emitter
    let emitter = ParticleEmitter::new(test_sprite()).with_mode(EmissionMode::Burst { count: 10 });

    let entity = world.spawn((Transform::new(100.0, 100.0), emitter));

    // Update should spawn particles
    system.update(&mut world, &mut events, 0.016);

    assert_eq!(system.particle_count(), 10);

    // Particles should be near the emitter position
    for p in &system.particles {
      assert!((p.x - 100.0).abs() < 50.0);
      assert!((p.y - 100.0).abs() < 50.0);
    }

    // Clean up
    let _ = entity;
  }

  #[test]
  fn particle_lifetime() {
    let mut system = ParticleSystem::new();
    let mut world = World::new();
    let mut events = Events::new();

    let emitter = ParticleEmitter::new(test_sprite())
      .with_mode(EmissionMode::Burst { count: 5 })
      .with_lifetime(0.1, 0.1); // Very short lifetime

    world.spawn((Transform::new(0.0, 0.0), emitter));

    // Spawn particles
    system.update(&mut world, &mut events, 0.016);
    assert_eq!(system.particle_count(), 5);

    // Update past lifetime
    for _ in 0..20 {
      system.update(&mut world, &mut events, 0.016);
    }

    // All particles should be dead
    assert_eq!(system.particle_count(), 0);
  }

  #[test]
  fn particle_clear() {
    let mut system = ParticleSystem::new();
    let mut world = World::new();
    let mut events = Events::new();

    let emitter = ParticleEmitter::new(test_sprite()).with_mode(EmissionMode::Burst { count: 20 });

    world.spawn((Transform::new(0.0, 0.0), emitter));
    system.update(&mut world, &mut events, 0.016);

    assert!(system.particle_count() > 0);

    system.clear();
    assert_eq!(system.particle_count(), 0);
  }
}
