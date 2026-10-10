//! Particle collision configuration
//!
//! Defines how particles interact with world colliders: entities with a `Transform` and a
//! non-trigger `Collider` whose layer shares a bit with [`CollisionConfig::collision_mask`].
//! Particles ignore the collider on their own emitter's entity.

/// How a particle responds to collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CollisionResponse {
  /// Particle is destroyed on collision.
  #[default]
  Die,
  /// Particle bounces off the surface.
  Bounce {
    /// Velocity retention (0.0 = stop, 1.0 = perfect bounce)
    restitution_percent: u8,
  },
  /// Particle stops where it hit and stays there until its lifetime ends.
  Stick,
  /// Particle passes through and emits a [`ParticleCollision`] when it goes from touching no
  /// collider to touching one. Moving straight from one collider into an overlapping or adjacent
  /// one does not notify again.
  Event,
}

impl CollisionResponse {
  /// Create a bounce response with the given restitution.
  pub fn bounce(restitution: f32) -> Self {
    CollisionResponse::Bounce {
      restitution_percent: (restitution.clamp(0.0, 1.0) * 100.0) as u8,
    }
  }

  /// Get the restitution value for bounce responses.
  pub fn restitution(&self) -> f32 {
    match self {
      CollisionResponse::Bounce {
        restitution_percent,
      } => *restitution_percent as f32 / 100.0,
      _ => 0.0,
    }
  }
}

/// Minimum impact speed along the surface normal, in pixels/second, for a bounce to emit a
/// [`ParticleCollision`] event. Filters out tiny hops; a particle resting on a surface emits
/// nothing anyway, because bounce events fire only when a particle goes from touching no
/// collider to touching one. Also exported from `ryo2d::prelude`.
pub const MIN_BOUNCE_EVENT_SPEED: f32 = 20.0;

/// Configuration for particle collisions.
#[derive(Debug, Clone)]
pub struct CollisionConfig {
  /// Collision radius for the particle
  pub radius: f32,
  /// Bitmask for which collision layers to interact with
  pub collision_mask: u32,
  /// How to respond to collisions
  pub response: CollisionResponse,
  /// Emit a [`ParticleCollision`] on impact for any response (see [`CollisionConfig::with_events`])
  pub send_events: bool,
}

impl Default for CollisionConfig {
  fn default() -> Self {
    Self {
      radius: 4.0,
      collision_mask: u32::MAX,
      response: CollisionResponse::Die,
      send_events: false,
    }
  }
}

impl CollisionConfig {
  /// Create a new collision config.
  pub fn new(radius: f32) -> Self {
    Self {
      radius,
      ..Default::default()
    }
  }

  /// Set the collision mask.
  pub fn with_mask(mut self, mask: u32) -> Self {
    self.collision_mask = mask;
    self
  }

  /// Set the collision response.
  pub fn with_response(mut self, response: CollisionResponse) -> Self {
    self.response = response;
    self
  }

  /// Emit a [`ParticleCollision`] event on impact, for any response.
  ///
  /// `Die` and `Stick` emit once per particle. `Bounce` emits when the particle hits a collider
  /// it was not already touching last update, at an impact speed along the surface normal of at
  /// least [`MIN_BOUNCE_EVENT_SPEED`]. `CollisionResponse::Event` always emits when a particle
  /// goes from touching no collider to touching one, with or without this.
  pub fn with_events(mut self) -> Self {
    self.send_events = true;
    self
  }

  /// Create a config that makes particles die on collision.
  pub fn die_on_hit(radius: f32) -> Self {
    Self {
      radius,
      collision_mask: u32::MAX,
      response: CollisionResponse::Die,
      send_events: false,
    }
  }

  /// Create a config that makes particles bounce.
  pub fn bouncy(radius: f32, restitution: f32) -> Self {
    Self {
      radius,
      collision_mask: u32::MAX,
      response: CollisionResponse::bounce(restitution),
      send_events: false,
    }
  }

  /// Create a config that makes particles stick on collision.
  pub fn sticky(radius: f32) -> Self {
    Self {
      radius,
      collision_mask: u32::MAX,
      response: CollisionResponse::Stick,
      send_events: false,
    }
  }
}

/// Event emitted when a particle hits a collider.
///
/// [`CollisionResponse::Event`] always emits it when a particle goes from touching no collider to
/// touching one. Other responses emit it only when the config opts in with
/// [`CollisionConfig::with_events`].
#[derive(Debug, Clone)]
pub struct ParticleCollision {
  /// Entity that owns the emitter
  pub emitter_entity: hecs::Entity,
  /// The entity whose collider was hit
  pub collider_entity: hecs::Entity,
  /// Particle position when the contact was detected
  pub position_x: f32,
  pub position_y: f32,
  /// Unit normal of the surface hit, pointing towards the particle
  pub normal_x: f32,
  pub normal_y: f32,
  /// Particle velocity before the collision response
  pub velocity_x: f32,
  pub velocity_y: f32,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn collision_response_bounce() {
    let response = CollisionResponse::bounce(0.8);
    assert!((response.restitution() - 0.8).abs() < 0.02);
  }

  #[test]
  fn collision_config_default() {
    let config = CollisionConfig::default();
    assert!(config.radius > 0.0);
    assert_eq!(config.collision_mask, u32::MAX);
  }

  #[test]
  fn collision_config_bouncy() {
    let config = CollisionConfig::bouncy(5.0, 0.5);
    assert!((config.radius - 5.0).abs() < 0.001);
    assert!(matches!(config.response, CollisionResponse::Bounce { .. }));
  }
}
