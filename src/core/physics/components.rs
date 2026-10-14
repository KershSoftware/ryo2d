//! Physics components for Rapier2D integration
//!
//! This module provides component types used by the Rapier2D physics backend.
//! The actual physics simulation is handled by [`super::Physics`].
//!
//! ## Components
//!
//! - [`RigidBody`] - Controls how physics affects an entity (dynamic vs kinematic)
//!
//! ## How Physics Works
//!
//! 1. Add `RigidBody` + `Collider` + `Transform` to an entity
//! 2. The engine auto-registers it with Rapier
//! 3. Rapier handles gravity, collision response, and velocity integration
//! 4. Positions sync back to `Transform` after each physics step
//!
//! For collision events, see [`super::PhysicsCollisionEvent`].

use crate::core::ecs::Component;

/// RigidBody component - controls how physics affects an entity
#[derive(Component, Debug, Clone)]
pub struct RigidBody {
  /// Multiplier for gravity (0 = no gravity, 1 = normal, 2 = double, etc.)
  pub gravity_scale: f32,
  /// Linear drag (velocity decay per second, 0-1 range typical)
  /// Applied as: velocity *= (1 - drag * dt)
  pub drag: f32,
  /// Whether this body is affected by physics simulation
  pub is_kinematic: bool,
  /// Enable Continuous Collision Detection to prevent tunneling through thin colliders.
  /// Recommended for fast-moving objects like players, projectiles, or falling objects.
  /// Has a small performance cost, so only enable where needed.
  pub ccd_enabled: bool,
}

impl RigidBody {
  /// Create a new dynamic rigid body with default physics properties
  pub fn new() -> Self {
    RigidBody {
      gravity_scale: 1.0,
      drag: 0.0,
      is_kinematic: false,
      ccd_enabled: false,
    }
  }

  /// Create a kinematic body (not affected by physics, moved manually)
  pub fn kinematic() -> Self {
    RigidBody {
      gravity_scale: 0.0,
      drag: 0.0,
      is_kinematic: true,
      ccd_enabled: false,
    }
  }

  /// Builder method to set gravity scale
  pub fn with_gravity_scale(mut self, scale: f32) -> Self {
    self.gravity_scale = scale;
    self
  }

  /// Builder method to set drag
  pub fn with_drag(mut self, drag: f32) -> Self {
    self.drag = drag;
    self
  }

  /// Enable Continuous Collision Detection (CCD) to prevent tunneling.
  ///
  /// Use this for fast-moving objects that might pass through thin colliders:
  /// - Players falling from heights
  /// - Bullets and projectiles
  /// - Fast-moving enemies
  ///
  /// CCD has a small performance cost, so only enable where needed.
  pub fn with_ccd(mut self) -> Self {
    self.ccd_enabled = true;
    self
  }
}

impl Default for RigidBody {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rigid_body_defaults() {
    let rb = RigidBody::new();
    assert_eq!(rb.gravity_scale, 1.0);
    assert_eq!(rb.drag, 0.0);
    assert!(!rb.is_kinematic);
    assert!(!rb.ccd_enabled);
  }

  #[test]
  fn rigid_body_kinematic() {
    let rb = RigidBody::kinematic();
    assert!(rb.is_kinematic);
    assert_eq!(rb.gravity_scale, 0.0);
    assert!(!rb.ccd_enabled);
  }

  #[test]
  fn rigid_body_builder() {
    let rb = RigidBody::new().with_gravity_scale(0.5).with_drag(0.1);

    assert_eq!(rb.gravity_scale, 0.5);
    assert_eq!(rb.drag, 0.1);
  }

  #[test]
  fn rigid_body_with_ccd() {
    let rb = RigidBody::new().with_ccd();
    assert!(rb.ccd_enabled);

    // CCD should work with other options
    let rb2 = RigidBody::new().with_ccd().with_drag(0.5);
    assert!(rb2.ccd_enabled);
    assert_eq!(rb2.drag, 0.5);
  }
}
