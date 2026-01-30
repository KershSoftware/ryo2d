//! Built-in components for the game engine

use crate::core::asset::SpriteHandle;

// Re-export glam types for convenience
pub use glam::{Mat4, Vec2, Vec3, Vec4};

/// Common render layers for organizing draw order
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderLayer {
  Background = -1000,
  Default = 0,
  Foreground = 1000,
  UI = 2000,
}

impl RenderLayer {
  pub fn as_z_order(self) -> i32 {
    self as i32
  }
}

/// Position and rotation in 2D/3D space
///
/// For 2D games, the `z` field represents depth (used for parallax, layering).
/// For 3D games, `z` becomes the third spatial dimension.
#[derive(Debug, Clone)]
pub struct Transform {
  pub x: f32,
  pub y: f32,
  /// Depth in 2D (parallax/layering) or Z-axis position in 3D.
  /// Defaults to 0.0 for standard 2D games.
  pub z: f32,
  pub rotation: f32,
  pub scale_x: f32,
  pub scale_y: f32,
}

impl Transform {
  /// Create a new 2D transform (z defaults to 0.0)
  pub fn new(x: f32, y: f32) -> Self {
    Transform {
      x,
      y,
      z: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
    }
  }

  /// Create a new 3D transform with explicit z position
  pub fn new_3d(x: f32, y: f32, z: f32) -> Self {
    Transform {
      x,
      y,
      z,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
    }
  }

  /// Create a transform from a Vec2 (z defaults to 0.0)
  pub fn from_vec2(pos: Vec2) -> Self {
    Self::new(pos.x, pos.y)
  }

  /// Create a transform from a Vec3
  pub fn from_vec3(pos: Vec3) -> Self {
    Self::new_3d(pos.x, pos.y, pos.z)
  }

  /// Get position as Vec2 (discards z)
  pub fn as_vec2(&self) -> Vec2 {
    Vec2::new(self.x, self.y)
  }

  /// Get position as Vec3
  pub fn as_vec3(&self) -> Vec3 {
    Vec3::new(self.x, self.y, self.z)
  }

  pub fn origin() -> Self {
    Self::new(0.0, 0.0)
  }
}

impl Default for Transform {
  fn default() -> Self {
    Self::origin()
  }
}

/// Movement per second in 2D/3D space
#[derive(Debug, Clone, Default)]
pub struct Velocity {
  pub dx: f32,
  pub dy: f32,
  /// Z-axis velocity (for 3D or depth-based movement)
  pub dz: f32,
}

impl Velocity {
  /// Create a new 2D velocity (dz defaults to 0.0)
  pub fn new(dx: f32, dy: f32) -> Self {
    Velocity { dx, dy, dz: 0.0 }
  }

  /// Create a new 3D velocity
  pub fn new_3d(dx: f32, dy: f32, dz: f32) -> Self {
    Velocity { dx, dy, dz }
  }

  /// Create velocity from Vec2 (dz defaults to 0.0)
  pub fn from_vec2(vel: Vec2) -> Self {
    Self::new(vel.x, vel.y)
  }

  /// Create velocity from Vec3
  pub fn from_vec3(vel: Vec3) -> Self {
    Self::new_3d(vel.x, vel.y, vel.z)
  }

  /// Get velocity as Vec2 (discards dz)
  pub fn as_vec2(&self) -> Vec2 {
    Vec2::new(self.dx, self.dy)
  }

  /// Get velocity as Vec3
  pub fn as_vec3(&self) -> Vec3 {
    Vec3::new(self.dx, self.dy, self.dz)
  }

  pub fn zero() -> Self {
    Self::new(0.0, 0.0)
  }
}

/// Visual representation for rendering
///
/// Uses `SpriteHandle` for efficient shared ownership and hot reload support.
#[derive(Clone)]
pub struct SpriteRenderer {
  pub sprite: SpriteHandle,
  pub visible: bool,
  pub z_order: i32,
}

impl SpriteRenderer {
  pub fn new(sprite: SpriteHandle) -> Self {
    SpriteRenderer {
      sprite,
      visible: true,
      z_order: 0,
    }
  }

  pub fn with_z_order(mut self, z: i32) -> Self {
    self.z_order = z;
    self
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn transform_defaults() {
    let t = Transform::default();
    assert_eq!(t.x, 0.0);
    assert_eq!(t.y, 0.0);
    assert_eq!(t.z, 0.0);
    assert_eq!(t.rotation, 0.0);
    assert_eq!(t.scale_x, 1.0);
    assert_eq!(t.scale_y, 1.0);
  }

  #[test]
  fn transform_3d() {
    let t = Transform::new_3d(1.0, 2.0, 3.0);
    assert_eq!(t.x, 1.0);
    assert_eq!(t.y, 2.0);
    assert_eq!(t.z, 3.0);
  }

  #[test]
  fn transform_vec_conversion() {
    let t = Transform::from_vec3(Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(t.as_vec3(), Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(t.as_vec2(), Vec2::new(1.0, 2.0));
  }

  #[test]
  fn velocity_defaults() {
    let v = Velocity::default();
    assert_eq!(v.dx, 0.0);
    assert_eq!(v.dy, 0.0);
    assert_eq!(v.dz, 0.0);
  }

  #[test]
  fn velocity_3d() {
    let v = Velocity::new_3d(1.0, 2.0, 3.0);
    assert_eq!(v.dx, 1.0);
    assert_eq!(v.dy, 2.0);
    assert_eq!(v.dz, 3.0);
  }

  #[test]
  fn velocity_vec_conversion() {
    let v = Velocity::from_vec3(Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(v.as_vec3(), Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(v.as_vec2(), Vec2::new(1.0, 2.0));
  }
}
