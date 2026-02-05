//! Particle collision configuration
//!
//! Defines how particles interact with world colliders.

/// How a particle responds to collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionResponse {
  /// Particle is destroyed on collision.
  Die,
  /// Particle bounces off the surface.
  Bounce {
    /// Velocity retention (0.0 = stop, 1.0 = perfect bounce)
    restitution_percent: u8,
  },
  /// Particle stops at the collision point.
  Stick,
  /// Particle passes through but emits a ParticleCollision event.
  Event,
}

impl Default for CollisionResponse {
  fn default() -> Self {
    CollisionResponse::Die
  }
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
      CollisionResponse::Bounce { restitution_percent } => *restitution_percent as f32 / 100.0,
      _ => 0.0,
    }
  }
}

/// Configuration for particle collisions.
#[derive(Debug, Clone)]
pub struct CollisionConfig {
  /// Collision radius for the particle
  pub radius: f32,
  /// Bitmask for which collision layers to interact with
  pub collision_mask: u32,
  /// How to respond to collisions
  pub response: CollisionResponse,
}

impl Default for CollisionConfig {
  fn default() -> Self {
    Self {
      radius: 4.0,
      collision_mask: u32::MAX,
      response: CollisionResponse::Die,
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

  /// Create a config that makes particles die on collision.
  pub fn die_on_hit(radius: f32) -> Self {
    Self {
      radius,
      collision_mask: u32::MAX,
      response: CollisionResponse::Die,
    }
  }

  /// Create a config that makes particles bounce.
  pub fn bouncy(radius: f32, restitution: f32) -> Self {
    Self {
      radius,
      collision_mask: u32::MAX,
      response: CollisionResponse::bounce(restitution),
    }
  }

  /// Create a config that makes particles stick on collision.
  pub fn sticky(radius: f32) -> Self {
    Self {
      radius,
      collision_mask: u32::MAX,
      response: CollisionResponse::Stick,
    }
  }
}

/// Event emitted when a particle collides (when response is Event).
#[derive(Debug, Clone)]
pub struct ParticleCollision {
  /// Entity that owns the emitter
  pub emitter_entity: hecs::Entity,
  /// Position of collision
  pub position_x: f32,
  pub position_y: f32,
  /// Normal of the surface hit
  pub normal_x: f32,
  pub normal_y: f32,
  /// Velocity of particle at collision
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
