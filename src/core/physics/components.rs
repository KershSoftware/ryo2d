//! Physics components for Rapier2D integration
//!
//! This module provides component types used by the Rapier2D physics backend.
//! The actual physics simulation is handled by [`super::rapier::RapierPhysicsWorld`].
//!
//! ## Components
//!
//! - [`RigidBody`] - Controls how physics affects an entity (dynamic vs kinematic)
//! - [`PhysicsConfig`] - Global physics settings like gravity
//!
//! ## How Physics Works
//!
//! 1. Add `RigidBody` + `Collider` + `Transform` to an entity
//! 2. The engine auto-registers it with Rapier
//! 3. Rapier handles gravity, collision response, and velocity integration
//! 4. Positions sync back to `Transform` after each physics step
//!
//! For collision events, see [`super::rapier::PhysicsCollisionEvent`].

use crate::core::ecs::{Component, Resource};

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

/// Physics world configuration
#[derive(Resource, Debug, Clone)]
pub struct PhysicsConfig {
  /// Gravity acceleration in pixels per second squared (positive Y = down in screen coords)
  pub gravity: (f32, f32),
}

impl PhysicsConfig {
  /// Create physics config with Earth-like gravity (scaled for pixel units)
  /// Default: 980 pixels/s² downward (similar to 9.8 m/s² at 100 pixels per meter)
  pub fn new() -> Self {
    PhysicsConfig {
      gravity: (0.0, 980.0),
    }
  }

  /// Create physics config with custom gravity
  pub fn with_gravity(gx: f32, gy: f32) -> Self {
    PhysicsConfig { gravity: (gx, gy) }
  }

  /// No gravity (space, top-down games)
  pub fn zero_gravity() -> Self {
    PhysicsConfig {
      gravity: (0.0, 0.0),
    }
  }
}

impl Default for PhysicsConfig {
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

  #[test]
  fn physics_config_defaults() {
    let config = PhysicsConfig::new();
    assert_eq!(config.gravity.0, 0.0);
    assert_eq!(config.gravity.1, 980.0);
  }

  #[test]
  fn physics_config_zero_gravity() {
    let config = PhysicsConfig::zero_gravity();
    assert_eq!(config.gravity, (0.0, 0.0));
  }
}
