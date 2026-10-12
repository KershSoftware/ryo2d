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
pub use collision::{
  CollisionConfig, CollisionResponse, MIN_BOUNCE_EVENT_SPEED, ParticleCollision,
};
pub use emitter::{EmissionMode, EmissionShape, ParticleEmitter};
pub use particle::Particle;

use std::collections::HashMap;

use crate::core::ecs::{Entity, GlobalTransform, SpriteEffect, World};
use crate::core::graphics::{BatchCollector, Camera, SpriteInstance};
use crate::core::messages::register_message;
use crate::core::physics::{Collider, ColliderShape};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

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
  /// Stuck to a collider (`CollisionResponse::Stick`): no more motion until it dies
  stuck: bool,
  /// Touched a collider last update (`Event` and `Bounce` events fire on entry only)
  touching: bool,
}

impl ParticleData {
  /// Collision event for this particle, using its state before the response is applied.
  fn collision_event(&self, contact: &Contact) -> ParticleCollision {
    ParticleCollision {
      emitter_entity: self.emitter_entity,
      collider_entity: contact.collider,
      position_x: self.x,
      position_y: self.y,
      normal_x: contact.normal_x,
      normal_y: contact.normal_y,
      velocity_x: self.particle.velocity_x,
      velocity_y: self.particle.velocity_y,
    }
  }
}

/// A uniform sample from `min..=max`; `min` when the range is empty or
/// reversed (no spread, a zero-size shape, or min set above max).
fn range(rng: &mut StdRng, min: f32, max: f32) -> f32 {
  if max > min {
    rng.gen_range(min..=max)
  } else {
    min
  }
}

/// Gap left between a bounced particle and the surface so it starts the next step clear of it.
const CONTACT_SKIN: f32 = 0.01;

/// World-space geometry of a collider, as seen by particles.
#[derive(Debug, Clone, Copy)]
enum ColliderGeometry {
  Box {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
  },
  Circle {
    x: f32,
    y: f32,
    radius: f32,
  },
}

/// A world collider snapshot, gathered once per update.
#[derive(Debug, Clone, Copy)]
struct ParticleCollider {
  entity: Entity,
  geometry: ColliderGeometry,
  /// `Collider::layer`, matched against `CollisionConfig::collision_mask`
  layer: u32,
}

/// Where a particle touches a collider.
#[derive(Debug, Clone, Copy)]
struct Contact {
  collider: Entity,
  /// Unit normal pointing from the surface towards the particle
  normal_x: f32,
  normal_y: f32,
  /// Closest point on the collider surface
  point_x: f32,
  point_y: f32,
}

impl ParticleCollider {
  fn from_component(entity: Entity, global: &GlobalTransform, collider: &Collider) -> Self {
    // Centred on the origin plus offset, unscaled, as physics simulates it
    let position = global.translation();
    let geometry = match collider.shape {
      ColliderShape::Box { .. } => {
        let (min, max) = collider.world_bounds(position);
        ColliderGeometry::Box {
          min_x: min.x,
          min_y: min.y,
          max_x: max.x,
          max_y: max.y,
        }
      }
      ColliderShape::Circle { radius } => {
        let center = collider.world_center(position);
        ColliderGeometry::Circle {
          x: center.x,
          y: center.y,
          radius,
        }
      }
    };
    Self {
      entity,
      geometry,
      layer: collider.layer,
    }
  }

  /// Test a particle circle against this collider.
  fn contact(&self, x: f32, y: f32, radius: f32) -> Option<Contact> {
    let (normal_x, normal_y, point_x, point_y) = match self.geometry {
      ColliderGeometry::Box {
        min_x,
        min_y,
        max_x,
        max_y,
      } => {
        // min/max instead of clamp: clamp panics on NaN bounds
        let closest_x = x.max(min_x).min(max_x);
        let closest_y = y.max(min_y).min(max_y);
        let dx = x - closest_x;
        let dy = y - closest_y;
        let dist_sq = dx * dx + dy * dy;
        if dist_sq > 0.0 {
          if dist_sq >= radius * radius {
            return None;
          }
          let dist = dist_sq.sqrt();
          (dx / dist, dy / dist, closest_x, closest_y)
        } else {
          // Centre inside the box: push out through the nearest edge
          let top = y - min_y;
          let bottom = max_y - y;
          let left = x - min_x;
          let right = max_x - x;
          let nearest = top.min(bottom).min(left).min(right);
          if nearest == top {
            (0.0, -1.0, x, min_y)
          } else if nearest == bottom {
            (0.0, 1.0, x, max_y)
          } else if nearest == left {
            (-1.0, 0.0, min_x, y)
          } else {
            (1.0, 0.0, max_x, y)
          }
        }
      }
      ColliderGeometry::Circle {
        x: cx,
        y: cy,
        radius: circle_radius,
      } => {
        let dx = x - cx;
        let dy = y - cy;
        let reach = circle_radius + radius;
        let dist_sq = dx * dx + dy * dy;
        if dist_sq >= reach * reach {
          return None;
        }
        let dist = dist_sq.sqrt();
        // Exactly at the centre: push out upwards
        let (nx, ny) = if dist > 0.0 {
          (dx / dist, dy / dist)
        } else {
          (0.0, -1.0)
        };
        (nx, ny, cx + nx * circle_radius, cy + ny * circle_radius)
      }
    };
    Some(Contact {
      collider: self.entity,
      normal_x,
      normal_y,
      point_x,
      point_y,
    })
  }
}

/// Colliders a particle can hit, in gather order.
///
/// Skips the collider on the particle's own emitter entity, which particles spawn inside.
fn candidate_colliders<'a>(
  colliders: &'a [ParticleCollider],
  emitter: Entity,
  config: &'a CollisionConfig,
) -> impl Iterator<Item = &'a ParticleCollider> {
  colliders.iter().filter(move |collider| {
    collider.entity != emitter && collider.layer & config.collision_mask != 0
  })
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
#[derive(Debug)]
pub struct ParticleSystem {
  /// All active particles, grouped by emitter entity
  particles: Vec<ParticleData>,
  /// Colliders gathered this update (reused to avoid per-frame allocation)
  colliders: Vec<ParticleCollider>,
  /// Random number generator for spawn variation
  rng: StdRng,
}

impl Default for ParticleSystem {
  fn default() -> Self {
    Self::new()
  }
}

impl ParticleSystem {
  /// Create a new particle system with a random seed.
  pub fn new() -> Self {
    Self::with_rng(StdRng::from_entropy())
  }

  /// Create a particle system whose spawn variation (lifetimes, sizes,
  /// directions, speeds) comes from `seed`.
  ///
  /// The same seed and the same updates give the same particles on a given
  /// engine version - use it for headless visual tests and replays.
  pub fn with_seed(seed: u64) -> Self {
    Self::with_rng(StdRng::seed_from_u64(seed))
  }

  fn with_rng(rng: StdRng) -> Self {
    Self {
      particles: Vec::with_capacity(10000),
      colliders: Vec::new(),
      rng,
    }
  }

  /// Update all particle emitters and simulate particles.
  ///
  /// Call this from the scene's `update`. Emitters and attractors act at
  /// their `GlobalTransform`. Particles from emitters with
  /// [`ParticleEmitter::with_collision`] collide with `Collider` entities and
  /// may write [`ParticleCollision`] messages.
  pub fn update(&mut self, world: &mut World, dt: f32) {
    let emitter_data: Vec<(Entity, f32, f32, f32, f32, Option<CollisionConfig>)> = world
      .query::<(Entity, &GlobalTransform, &ParticleEmitter)>()
      .iter(world)
      .filter(|(_, _, emitter)| emitter.enabled)
      .map(|(entity, global, emitter)| {
        let position = global.translation();
        (
          entity,
          position.x,
          position.y,
          emitter.gravity.0,
          emitter.gravity.1,
          emitter.collision.clone(),
        )
      })
      .collect();

    // Gather colliders only when some emitter collides
    self.colliders.clear();
    if emitter_data
      .iter()
      .any(|(.., collision)| collision.is_some())
    {
      self.colliders.extend(
        world
          .query::<(Entity, &GlobalTransform, &Collider)>()
          .iter(world)
          .filter(|(_, _, collider)| !collider.is_trigger)
          .map(|(entity, global, collider)| {
            ParticleCollider::from_component(entity, global, collider)
          }),
      );
    }

    // Attractors act with this update's state, then advance their time
    let attractors: Vec<(f32, f32, ParticleAttractor)> = world
      .query::<(&GlobalTransform, &ParticleAttractor)>()
      .iter(world)
      .filter(|(_, attractor)| attractor.enabled)
      .map(|(global, attractor)| {
        let position = global.translation();
        (position.x, position.y, attractor.clone())
      })
      .collect();
    for mut attractor in world.query::<&mut ParticleAttractor>().iter_mut(world) {
      attractor.update(dt);
    }

    // Spawn new particles
    for (entity, x, y, ..) in &emitter_data {
      self.spawn_particles(world, *entity, *x, *y, dt);
    }

    // Build lookup for emitter properties
    let emitter_props: HashMap<Entity, (f32, f32, Option<&CollisionConfig>)> = emitter_data
      .iter()
      .map(|(e, _, _, gx, gy, collision)| (*e, (*gx, *gy, collision.as_ref())))
      .collect();

    // Simulate particles
    let mut collisions = Vec::new();
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

      // Stuck particles only age and fade
      if !p.stuck {
        // Get emitter properties
        let (grav_x, grav_y, collision) = emitter_props
          .get(&p.emitter_entity)
          .copied()
          .unwrap_or((0.0, 0.0, None));

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

        if let Some(config) = collision {
          let was_touching = p.touching;
          p.touching = false;
          let mut dead = false;
          // One bounce event per update, even when a corner touches two colliders
          let mut bounce_event_sent = false;
          // Bounce resolves every overlapping collider in turn (a particle resting on a floor
          // must still hit a wall); the other responses stop at the first contact.
          for collider in candidate_colliders(&self.colliders, p.emitter_entity, config) {
            let Some(contact) = collider.contact(p.x, p.y, config.radius) else {
              continue;
            };
            p.touching = true;
            match config.response {
              CollisionResponse::Die => {
                if config.send_events {
                  collisions.push(p.collision_event(&contact));
                }
                dead = true;
                break;
              }
              CollisionResponse::Bounce { .. } => {
                let (nx, ny) = (contact.normal_x, contact.normal_y);
                let speed_along_normal = p.particle.velocity_x * nx + p.particle.velocity_y * ny;
                if speed_along_normal < 0.0 {
                  // Entry only: a particle resting on a surface stays touching and stays quiet
                  if config.send_events
                    && !was_touching
                    && !bounce_event_sent
                    && -speed_along_normal >= MIN_BOUNCE_EVENT_SPEED
                  {
                    collisions.push(p.collision_event(&contact));
                    bounce_event_sent = true;
                  }
                  let impulse = (1.0 + config.response.restitution()) * speed_along_normal;
                  p.particle.velocity_x -= impulse * nx;
                  p.particle.velocity_y -= impulse * ny;
                }
                let push = config.radius + CONTACT_SKIN;
                p.x = contact.point_x + nx * push;
                p.y = contact.point_y + ny * push;
              }
              CollisionResponse::Stick => {
                if config.send_events {
                  collisions.push(p.collision_event(&contact));
                }
                p.particle.velocity_x = 0.0;
                p.particle.velocity_y = 0.0;
                p.stuck = true;
                break;
              }
              CollisionResponse::Event => {
                if !was_touching {
                  collisions.push(p.collision_event(&contact));
                }
                break;
              }
            }
          }
          if dead {
            self.particles.swap_remove(i);
            continue;
          }
        }
      }

      // Update visuals
      p.particle.update_visuals();

      i += 1;
    }

    // Update particle counts on emitters
    let mut counts: HashMap<Entity, u32> = HashMap::new();
    for p in &self.particles {
      *counts.entry(p.emitter_entity).or_insert(0) += 1;
    }
    for (entity, mut emitter) in world
      .query::<(Entity, &mut ParticleEmitter)>()
      .iter_mut(world)
    {
      let count = counts.get(&entity).copied().unwrap_or(0);
      if emitter.particle_count != count {
        emitter.particle_count = count;
      }
    }

    if !collisions.is_empty() {
      register_message::<ParticleCollision>(world);
      world.write_message_batch(collisions);
    }
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

    let rng = &mut self.rng;

    // Spawn particles
    for _ in 0..spawn_count {
      let mut particle = Particle::new();

      // Lifetime
      particle.max_lifetime = range(rng, lifetime_min, lifetime_max);

      // Size
      particle.start_size = range(rng, size_min, size_max);
      particle.end_size = range(rng, size_end_min, size_end_max);
      particle.size = particle.start_size;

      // Color
      particle.start_color = color_start;
      particle.end_color = color_end;
      particle.color = color_start;

      // Rotation
      particle.rotation = range(rng, rotation_min, rotation_max);
      particle.angular_velocity = range(rng, angular_vel_min, angular_vel_max);

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
        stuck: false,
        touching: false,
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
    rng: &mut StdRng,
  ) -> (f32, f32, f32, f32) {
    match shape {
      EmissionShape::Point => {
        // Random direction
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let speed = range(rng, speed_min, speed_max);
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
        let speed = range(rng, speed_min, speed_max);
        (px, py, angle.cos() * speed, angle.sin() * speed)
      }

      EmissionShape::Circle { radius, hollow } => {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let r = if *hollow {
          *radius
        } else {
          range(rng, 0.0, *radius)
        };
        let px = emitter_x + angle.cos() * r;
        let py = emitter_y + angle.sin() * r;

        // Velocity direction = outward from center
        let speed = range(rng, speed_min, speed_max);
        (px, py, angle.cos() * speed, angle.sin() * speed)
      }

      EmissionShape::Rectangle { width, height } => {
        let px = emitter_x + range(rng, -width / 2.0, width / 2.0);
        let py = emitter_y + range(rng, -height / 2.0, height / 2.0);
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let speed = range(rng, speed_min, speed_max);
        (px, py, angle.cos() * speed, angle.sin() * speed)
      }

      EmissionShape::Cone {
        direction,
        spread,
        min_speed,
        max_speed,
      } => {
        let half_spread = spread / 2.0;
        let angle = direction + range(rng, -half_spread, half_spread);
        let speed = range(rng, *min_speed, *max_speed);
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
    if self.particles.is_empty() {
      return;
    }
    // Each emitter's look, read once rather than per particle; disabled
    // emitters are left out, so their particles are not drawn
    let Some(mut emitters) = world.try_query::<(Entity, &ParticleEmitter)>() else {
      return;
    };
    let looks: HashMap<Entity, (String, Option<&SpriteEffect>, i32)> = emitters
      .iter(world)
      .map(|(entity, emitter)| {
        (
          entity,
          (
            emitter.sprite.id(),
            emitter.effect.as_ref(),
            emitter.z_order,
          ),
        )
      })
      .collect();
    let zoom = camera.zoom;

    for p in &self.particles {
      let Some((texture_id, effect, z_order)) = looks.get(&p.emitter_entity) else {
        continue;
      };

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

      if let Some(effect) = effect {
        instance = instance.with_effect(effect);
      }

      collector.add(texture_id, effect.is_some(), instance, *z_order);
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
  use crate::core::ecs::{Disabled, Messages, Transform, propagate_transforms};
  use crate::core::graphics::Sprite;
  use crate::core::physics::Collider;
  use crate::core::systems::test_util;

  /// Propagate transforms, as the engine does every step, then update.
  fn update(system: &mut ParticleSystem, world: &mut World, dt: f32) {
    test_util::run(world, propagate_transforms);
    system.update(world, dt);
  }

  fn test_sprite() -> SpriteHandle {
    SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 255, 255, 255, 255), "test.png")
  }

  /// One motionless particle at the origin and a strong point attractor 100px to its right.
  fn particle_near_attractor(emitter_layer: u32, attractor_mask: u32) -> f32 {
    let mut system = ParticleSystem::new();
    let mut world = World::new();
    let emitter = ParticleEmitter::new(test_sprite())
      .with_mode(EmissionMode::Burst { count: 1 })
      .with_speed(0.0, 0.0)
      .with_lifetime(10.0, 10.0)
      .with_layer(emitter_layer);
    world.spawn((Transform::from_xy(0.0, 0.0), emitter));
    world.spawn((
      Transform::from_xy(100.0, 0.0),
      ParticleAttractor::point(500.0, 1000.0).with_layer_mask(attractor_mask),
    ));

    update(&mut system, &mut world, 0.016);
    update(&mut system, &mut world, 0.016);

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

  fn spawned_motion(seed: u64) -> Vec<(f32, f32, f32)> {
    let mut system = ParticleSystem::with_seed(seed);
    let mut world = World::new();
    world.spawn((
      Transform::from_xy(0.0, 0.0),
      ParticleEmitter::new(test_sprite())
        .with_mode(EmissionMode::Burst { count: 5 })
        .with_speed(10.0, 50.0)
        .with_lifetime(1.0, 3.0),
    ));
    update(&mut system, &mut world, 0.016);
    system
      .particles
      .iter()
      .map(|p| {
        (
          p.particle.velocity_x,
          p.particle.velocity_y,
          p.particle.max_lifetime,
        )
      })
      .collect()
  }

  #[test]
  fn seeded_particle_systems_spawn_identical_particles() {
    assert_eq!(spawned_motion(7), spawned_motion(7));
    assert_ne!(spawned_motion(7), spawned_motion(8));
  }

  #[test]
  fn default_particle_system_spawns_particles() {
    let mut system = ParticleSystem::default();
    let mut world = World::new();
    world.spawn((
      Transform::from_xy(0.0, 0.0),
      ParticleEmitter::new(test_sprite()).with_mode(EmissionMode::Burst { count: 3 }),
    ));

    update(&mut system, &mut world, 0.016);

    assert_eq!(system.particles.len(), 3);
  }

  #[test]
  fn empty_and_reversed_ranges_spawn_without_panicking() {
    // A straight jet (no spread), a zero-size shape and min above max all
    // pick the low end rather than panicking on an empty range
    let mut system = ParticleSystem::with_seed(1);
    let mut world = World::new();
    world.spawn((
      Transform::from_xy(0.0, 0.0),
      ParticleEmitter::new(test_sprite())
        .with_mode(EmissionMode::Burst { count: 4 })
        .with_shape(EmissionShape::Cone {
          direction: 0.0,
          spread: 0.0,
          min_speed: 50.0,
          max_speed: 50.0,
        })
        .with_lifetime(2.0, 1.0),
    ));
    world.spawn((
      Transform::from_xy(0.0, 0.0),
      ParticleEmitter::new(test_sprite())
        .with_mode(EmissionMode::Burst { count: 4 })
        .with_shape(EmissionShape::Rectangle {
          width: 0.0,
          height: 0.0,
        }),
    ));
    world.spawn((
      Transform::from_xy(0.0, 0.0),
      ParticleEmitter::new(test_sprite())
        .with_mode(EmissionMode::Burst { count: 4 })
        .with_shape(EmissionShape::Circle {
          radius: 0.0,
          hollow: false,
        }),
    ));
    update(&mut system, &mut world, 0.016);
    assert_eq!(system.particle_count(), 12);
    let jet: Vec<&ParticleData> = system
      .particles
      .iter()
      .filter(|p| p.particle.max_lifetime == 2.0)
      .collect();
    assert_eq!(jet.len(), 4, "min above max uses min");
    assert!(jet.iter().all(|p| p.particle.velocity_y.abs() < 1e-4));
  }

  #[test]
  fn particles_of_a_disabled_emitter_are_not_drawn() {
    use crate::core::graphics::{BatchCollector, Camera};
    let mut system = ParticleSystem::with_seed(3);
    let mut world = World::new();
    let burst = |count| {
      ParticleEmitter::new(test_sprite())
        .with_mode(EmissionMode::Burst { count })
        .with_speed(0.0, 0.0)
    };
    world.spawn((Transform::from_xy(10.0, 10.0), burst(2)));
    let hidden = world.spawn((Transform::from_xy(20.0, 20.0), burst(3))).id();
    update(&mut system, &mut world, 0.016);
    let camera = Camera::centered_at(0.0, 0.0, 100, 100);
    let drawn = |world: &World| {
      let mut collector = BatchCollector::new();
      system.collect_for_render(world, &camera, &mut collector);
      collector.sprite_count()
    };
    assert_eq!(drawn(&world), 5);
    world.entity_mut(hidden).insert(Disabled);
    assert_eq!(drawn(&world), 2);
  }

  #[test]
  fn a_burst_spawned_this_step_starts_at_its_emitter() {
    // Spawned and updated in the same scene update: no propagation between
    let mut system = ParticleSystem::with_seed(2);
    let mut world = World::new();
    world.spawn((
      Transform::from_xy(300.0, 200.0),
      ParticleEmitter::new(test_sprite())
        .with_mode(EmissionMode::Burst { count: 3 })
        .with_speed(0.0, 0.0),
    ));
    system.update(&mut world, 0.016);
    assert_eq!(system.particle_count(), 3);
    for p in &system.particles {
      assert_eq!((p.x, p.y), (300.0, 200.0));
    }
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

    // Create an emitter
    let emitter = ParticleEmitter::new(test_sprite()).with_mode(EmissionMode::Burst { count: 10 });

    let entity = world
      .spawn((Transform::from_xy(100.0, 100.0), emitter))
      .id();

    // Update should spawn particles
    update(&mut system, &mut world, 0.016);

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

    let emitter = ParticleEmitter::new(test_sprite())
      .with_mode(EmissionMode::Burst { count: 5 })
      .with_lifetime(0.1, 0.1); // Very short lifetime

    world.spawn((Transform::from_xy(0.0, 0.0), emitter));

    // Spawn particles
    update(&mut system, &mut world, 0.016);
    assert_eq!(system.particle_count(), 5);

    // Update past lifetime
    for _ in 0..20 {
      update(&mut system, &mut world, 0.016);
    }

    // All particles should be dead
    assert_eq!(system.particle_count(), 0);
  }

  #[test]
  fn particle_clear() {
    let mut system = ParticleSystem::new();
    let mut world = World::new();

    let emitter = ParticleEmitter::new(test_sprite()).with_mode(EmissionMode::Burst { count: 20 });

    world.spawn((Transform::from_xy(0.0, 0.0), emitter));
    update(&mut system, &mut world, 0.016);

    assert!(system.particle_count() > 0);

    system.clear();
    assert_eq!(system.particle_count(), 0);
  }

  const DT: f32 = 1.0 / 60.0;
  const RADIUS: f32 = 2.0;

  /// One motionless particle at (100, 0) falling under gravity onto a collider below it.
  ///
  /// The collider at (100, 60) is centred under the particle, as colliders are centred on their
  /// entity's origin.
  struct FallScene {
    system: ParticleSystem,
    world: World,
    emitter: Entity,
    collider: Entity,
    /// Top edge of the collider's world bounds
    top: f32,
    /// Bottom edge of the collider's world bounds
    bottom: f32,
    dt: f32,
  }

  impl FallScene {
    fn new(config: CollisionConfig, collider: Collider) -> Self {
      Self::with_motion(config, collider, 600.0, DT)
    }

    fn with_motion(config: CollisionConfig, collider: Collider, gravity_y: f32, dt: f32) -> Self {
      let mut world = World::new();
      let emitter = world
        .spawn((
          Transform::from_xy(100.0, 0.0),
          ParticleEmitter::new(test_sprite())
            .with_mode(EmissionMode::Burst { count: 1 })
            .with_speed(0.0, 0.0)
            .with_lifetime(60.0, 60.0)
            .with_gravity(0.0, gravity_y)
            .with_collision(config),
        ))
        .id();
      let transform = Transform::from_xy(100.0, 60.0);
      let (min, max) = collider.world_bounds(transform.translation);
      let collider = world.spawn((transform, collider)).id();
      Self {
        system: ParticleSystem::new(),
        world,
        emitter,
        collider,
        top: min.y,
        bottom: max.y,
        dt,
      }
    }

    /// A 200x20 floor centred under the particle.
    fn floor(config: CollisionConfig) -> Self {
      Self::new(config, Collider::box_collider(200.0, 20.0))
    }

    fn step(&mut self) {
      update(&mut self.system, &mut self.world, self.dt);
    }

    fn particle(&self) -> Option<&ParticleData> {
      self.system.particles.first()
    }

    /// Every collision written so far: these tests never advance the buffers.
    fn collisions(&self) -> Vec<ParticleCollision> {
      self
        .world
        .get_resource::<Messages<ParticleCollision>>()
        .map_or_else(Vec::new, |buffer| {
          buffer.get_cursor().read(buffer).cloned().collect()
        })
    }
  }

  #[test]
  fn a_corner_hit_sends_one_bounce_event() {
    let config = CollisionConfig::bouncy(RADIUS, 0.5).with_events();
    let mut scene = FallScene::with_motion(config, Collider::box_collider(200.0, 20.0), 0.0, DT);
    // A wall rising from the floor, its left face at x = 150
    scene.world.spawn((
      Transform::from_xy(160.0, 50.0),
      Collider::box_collider(20.0, 200.0),
    ));
    scene.step();
    // Heading into the corner fast enough to touch floor and wall in one update
    let top = scene.top;
    let p = &mut scene.system.particles[0];
    p.x = 147.0;
    p.y = top - 3.0;
    p.particle.velocity_x = 120.0;
    p.particle.velocity_y = 120.0;

    scene.step();

    assert_eq!(scene.collisions().len(), 1);
  }

  #[test]
  fn die_on_hit_removes_particle_at_floor() {
    let mut scene = FallScene::floor(CollisionConfig::die_on_hit(RADIUS));
    scene.step();
    assert_eq!(scene.system.particle_count(), 1);

    let mut last_y = 0.0;
    for _ in 0..60 {
      scene.step();
      if let Some(p) = scene.particle() {
        last_y = p.y;
      }
    }

    assert_eq!(
      scene.system.particle_count(),
      0,
      "particle should die on the floor"
    );
    assert!(
      last_y < scene.top,
      "particle died at y={last_y}, past the floor top {}",
      scene.top
    );
  }

  #[test]
  fn bounce_reflects_off_floor() {
    let mut scene = FallScene::floor(CollisionConfig::bouncy(RADIUS, 0.5));
    let rest_limit = scene.top - RADIUS + 1e-3;

    let mut bounced = false;
    for step in 0..240 {
      scene.step();
      let p = scene.particle().expect("bouncing particle stays alive");
      assert!(
        p.y <= rest_limit,
        "step {step}: y={} sank past {rest_limit}",
        p.y
      );
      bounced |= p.particle.velocity_y < 0.0;
    }

    assert!(bounced, "velocity should reverse off the floor");
  }

  #[test]
  fn stick_freezes_particle() {
    let mut scene = FallScene::floor(CollisionConfig::sticky(RADIUS));
    let contact_y = scene.top - RADIUS;
    for _ in 0..60 {
      scene.step();
      if scene.particle().is_some_and(|p| p.y > contact_y) {
        break;
      }
    }
    let p = scene.particle().expect("particle reached the floor");
    assert!(p.y > contact_y, "particle never reached the floor");
    let (stuck_x, stuck_y) = (p.x, p.y);

    for _ in 0..30 {
      scene.step();
    }

    let p = scene.particle().expect("stuck particle stays alive");
    assert_eq!((p.x, p.y), (stuck_x, stuck_y));
    assert_eq!((p.particle.velocity_x, p.particle.velocity_y), (0.0, 0.0));
  }

  #[test]
  fn event_response_passes_through_and_notifies_once() {
    let config = CollisionConfig::new(RADIUS).with_response(CollisionResponse::Event);
    let mut scene = FallScene::floor(config);
    for _ in 0..120 {
      scene.step();
    }

    let p = scene.particle().expect("pass-through particle stays alive");
    assert!(
      p.y > scene.bottom + RADIUS,
      "particle at y={} should fall through",
      p.y
    );
    let collisions = scene.collisions();
    assert_eq!(collisions.len(), 1, "one event per entry");
    let hit = &collisions[0];
    assert_eq!(hit.collider_entity, scene.collider);
    assert_eq!(hit.emitter_entity, scene.emitter);
    assert_eq!((hit.normal_x, hit.normal_y), (0.0, -1.0));
    assert!(
      hit.velocity_y > 0.0,
      "velocity before the response, moving down"
    );
  }

  #[test]
  fn with_events_reports_die() {
    let mut scene = FallScene::floor(CollisionConfig::die_on_hit(RADIUS).with_events());
    for _ in 0..60 {
      scene.step();
    }
    assert_eq!(scene.system.particle_count(), 0);
    let collisions = scene.collisions();
    assert_eq!(collisions.len(), 1);
    assert_eq!(collisions[0].collider_entity, scene.collider);

    let mut silent = FallScene::floor(CollisionConfig::die_on_hit(RADIUS));
    for _ in 0..60 {
      silent.step();
    }
    assert_eq!(silent.system.particle_count(), 0);
    assert!(silent.collisions().is_empty(), "events are opt-in");
  }

  #[test]
  fn with_events_reports_stick_once() {
    let mut scene = FallScene::floor(CollisionConfig::sticky(RADIUS).with_events());
    for _ in 0..120 {
      scene.step();
    }
    assert_eq!(scene.system.particle_count(), 1);
    assert_eq!(scene.collisions().len(), 1);
  }

  #[test]
  fn collision_mask_filters_colliders() {
    let floor = || Collider::box_collider(200.0, 20.0).with_layer(0b10);

    let mut masked = FallScene::new(CollisionConfig::die_on_hit(RADIUS).with_mask(0b01), floor());
    for _ in 0..120 {
      masked.step();
    }
    let p = masked
      .particle()
      .expect("a masked-out floor does not kill the particle");
    assert!(
      p.y > masked.bottom + RADIUS,
      "particle at y={} should fall through",
      p.y
    );

    let mut matching = FallScene::new(CollisionConfig::die_on_hit(RADIUS).with_mask(0b10), floor());
    for _ in 0..120 {
      matching.step();
    }
    assert_eq!(
      matching.system.particle_count(),
      0,
      "a matching mask still collides"
    );
  }

  #[test]
  fn trigger_colliders_are_ignored() {
    let trigger = Collider::box_collider(200.0, 20.0).as_trigger();
    let mut scene = FallScene::new(CollisionConfig::die_on_hit(RADIUS), trigger);
    for _ in 0..120 {
      scene.step();
    }
    let p = scene
      .particle()
      .expect("a trigger does not kill the particle");
    assert!(
      p.y > scene.bottom + RADIUS,
      "particle at y={} should fall through",
      p.y
    );
  }

  #[test]
  fn bounce_off_circle_collider() {
    // Centred 10px left of the particle, so it lands on the circle's right shoulder
    let circle = Collider::circle_collider(20.0).with_offset(-10.0, 0.0);
    let mut scene = FallScene::new(CollisionConfig::bouncy(RADIUS, 0.5), circle);

    let mut bounce_velocity = None;
    for _ in 0..60 {
      scene.step();
      let p = scene.particle().expect("bouncing particle stays alive");
      if p.particle.velocity_y < 0.0 {
        bounce_velocity = Some((p.particle.velocity_x, p.particle.velocity_y));
        break;
      }
    }

    let (vx, _) = bounce_velocity.expect("velocity should reverse off the circle");
    assert!(
      vx > 0.0,
      "the curved surface deflects the particle away from its centre"
    );
  }

  #[test]
  fn settling_bounce_does_not_flood_events() {
    let mut scene = FallScene::floor(CollisionConfig::bouncy(RADIUS, 0.5).with_events());
    for _ in 0..600 {
      scene.step();
    }
    assert!(
      scene.particle().is_some(),
      "particle should still be resting on the floor"
    );
    let count = scene.collisions().len();
    assert!(
      (1..10).contains(&count),
      "expected a few impact events, got {count}"
    );
  }

  #[test]
  fn disabled_colliders_are_ignored() {
    let mut scene = FallScene::floor(CollisionConfig::die_on_hit(RADIUS));
    scene.world.entity_mut(scene.collider).insert(Disabled);
    for _ in 0..120 {
      scene.step();
    }
    let p = scene
      .particle()
      .expect("a disabled floor does not kill the particle");
    assert!(
      p.y > scene.bottom + RADIUS,
      "particle at y={} should fall through",
      p.y
    );
  }

  #[test]
  fn negatively_scaled_collider_still_collides() {
    let mut scene = FallScene::floor(CollisionConfig::die_on_hit(RADIUS));
    if let Some(mut transform) = scene.world.get_mut::<Transform>(scene.collider) {
      transform.scale.x = -1.0;
    }
    for _ in 0..60 {
      scene.step();
    }
    assert_eq!(
      scene.system.particle_count(),
      0,
      "inverted bounds should still collide"
    );
  }

  #[test]
  fn particles_ignore_their_own_emitters_collider() {
    let mut scene = FallScene::floor(CollisionConfig::die_on_hit(RADIUS));
    // A 20x20 box centred on the emitter, so particles spawn inside it
    let own_box = Collider::box_collider(20.0, 20.0);
    let own_bottom = scene
      .world
      .get::<Transform>(scene.emitter)
      .map(|transform| own_box.world_bounds(transform.translation).1.y)
      .expect("emitter has a transform");
    scene.world.entity_mut(scene.emitter).insert(own_box);

    scene.step();
    assert_eq!(
      scene.system.particle_count(),
      1,
      "own collider must not kill it"
    );

    let mut last_y = 0.0;
    for _ in 0..60 {
      scene.step();
      if let Some(p) = scene.particle() {
        last_y = p.y;
      }
    }
    assert_eq!(scene.system.particle_count(), 0, "the floor still kills it");
    assert!(
      last_y > own_bottom && last_y < scene.top,
      "particle should die at the floor, last seen at y={last_y}"
    );
  }

  #[test]
  fn settling_bounce_does_not_flood_events_under_high_gravity() {
    let config = CollisionConfig::bouncy(RADIUS, 0.5).with_events();
    // ~17px per step: a 200px-thick floor (top still at 50) keeps deep first contacts from
    // resolving through the bottom face
    let floor = Collider::box_collider(200.0, 200.0).with_offset(0.0, 90.0);
    let mut scene = FallScene::with_motion(config, floor, 3000.0, 1.0 / 30.0);
    assert_eq!((scene.top, scene.bottom), (50.0, 250.0));
    for _ in 0..600 {
      scene.step();
    }
    let p = scene.particle().expect("particle should still be alive");
    assert!(
      p.y <= scene.top - RADIUS + 1e-3,
      "particle at y={} should rest on the floor",
      p.y
    );
    let count = scene.collisions().len();
    assert!(
      (1..10).contains(&count),
      "expected a few impact events, got {count}"
    );
  }

  #[test]
  fn sliding_particle_bounces_off_wall_gathered_after_floor() {
    let mut scene = FallScene::floor(CollisionConfig::bouncy(RADIUS, 0.5));
    // Start resting on the floor (x 0..200), then slide right into a wall spawned after it
    let rest_y = scene.top - RADIUS - CONTACT_SKIN;
    if let Some(mut transform) = scene.world.get_mut::<Transform>(scene.emitter) {
      transform.translation.y = rest_y;
    }
    let wall = Collider::box_collider(20.0, 100.0);
    let wall_transform = Transform::from_xy(160.0, scene.top);
    let wall_face = wall.world_bounds(wall_transform.translation).0.x;
    scene.world.spawn((wall_transform, wall));

    scene.step();
    scene.system.particles[0].particle.velocity_x = 120.0;

    let face_limit = wall_face - RADIUS + 1e-3;
    let mut bounced = false;
    for step in 0..120 {
      scene.step();
      let p = scene.particle().expect("bouncing particle stays alive");
      assert!(
        p.x <= face_limit,
        "step {step}: x={} went past the wall face at {wall_face}",
        p.x
      );
      bounced |= p.particle.velocity_x < 0.0;
    }
    assert!(bounced, "velocity_x should flip off the wall");
  }

  #[test]
  fn slow_bounce_emits_no_event() {
    let config = CollisionConfig::bouncy(RADIUS, 0.5).with_events();
    let floor = Collider::box_collider(200.0, 20.0);
    let mut scene = FallScene::with_motion(config, floor, 0.0, DT);
    // 8px above contact, drifting down well under MIN_BOUNCE_EVENT_SPEED
    if let Some(mut transform) = scene.world.get_mut::<Transform>(scene.emitter) {
      transform.translation.y = scene.top - RADIUS - 8.0;
    }
    scene.step();
    scene.system.particles[0].particle.velocity_y = 10.0;

    let mut bounced = false;
    for _ in 0..120 {
      scene.step();
      let p = scene.particle().expect("bouncing particle stays alive");
      bounced |= p.particle.velocity_y < 0.0;
    }
    assert!(bounced, "velocity_y should flip off the floor");
    assert!(
      scene.collisions().is_empty(),
      "a 10 px/s impact is below MIN_BOUNCE_EVENT_SPEED"
    );
  }
}
