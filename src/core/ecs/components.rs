//! Built-in components for the game engine

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
///
/// ## Pivot Point
///
/// The `pivot_x` and `pivot_y` fields define the rotation/scale origin as a normalized
/// value (0.0 to 1.0) relative to the sprite dimensions:
/// - `(0.5, 0.5)` - Center (default) - rotates around the center
/// - `(0.0, 0.0)` - Top-left corner
/// - `(0.5, 0.0)` - Top-center - useful for hanging objects like pendulums
/// - `(0.0, 0.5)` - Left-center - useful for door hinges
/// - `(1.0, 1.0)` - Bottom-right corner
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
  /// Pivot X as normalized value (0.0 = left, 0.5 = center, 1.0 = right)
  pub pivot_x: f32,
  /// Pivot Y as normalized value (0.0 = top, 0.5 = center, 1.0 = bottom)
  pub pivot_y: f32,
}

impl Transform {
  /// Create a new 2D transform (z defaults to 0.0, pivot defaults to center)
  pub fn new(x: f32, y: f32) -> Self {
    Transform {
      x,
      y,
      z: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      pivot_x: 0.5,
      pivot_y: 0.5,
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
      pivot_x: 0.5,
      pivot_y: 0.5,
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

  /// Builder method to set rotation
  pub fn with_rotation(mut self, rotation: f32) -> Self {
    self.rotation = rotation;
    self
  }

  /// Builder method to set scale
  pub fn with_scale(mut self, scale_x: f32, scale_y: f32) -> Self {
    self.scale_x = scale_x;
    self.scale_y = scale_y;
    self
  }

  /// Builder method to set uniform scale
  pub fn with_uniform_scale(mut self, scale: f32) -> Self {
    self.scale_x = scale;
    self.scale_y = scale;
    self
  }

  /// Builder method to set pivot point (normalized 0.0-1.0)
  ///
  /// The pivot determines the center of rotation and scaling:
  /// - `(0.5, 0.5)` - Center (default)
  /// - `(0.5, 0.0)` - Top-center (pendulums, hanging objects)
  /// - `(0.0, 0.5)` - Left-center (door hinges)
  /// - `(0.0, 0.0)` - Top-left corner
  pub fn with_pivot(mut self, pivot_x: f32, pivot_y: f32) -> Self {
    self.pivot_x = pivot_x;
    self.pivot_y = pivot_y;
    self
  }

  /// Builder method to set pivot at top-center (useful for hanging objects)
  pub fn with_pivot_top_center(self) -> Self {
    self.with_pivot(0.5, 0.0)
  }

  /// Builder method to set pivot at bottom-center (useful for standing objects)
  pub fn with_pivot_bottom_center(self) -> Self {
    self.with_pivot(0.5, 1.0)
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
    assert_eq!(t.pivot_x, 0.5);
    assert_eq!(t.pivot_y, 0.5);
  }

  #[test]
  fn transform_3d() {
    let t = Transform::new_3d(1.0, 2.0, 3.0);
    assert_eq!(t.x, 1.0);
    assert_eq!(t.y, 2.0);
    assert_eq!(t.z, 3.0);
    assert_eq!(t.pivot_x, 0.5);
    assert_eq!(t.pivot_y, 0.5);
  }

  #[test]
  fn transform_pivot_builder() {
    let t = Transform::new(100.0, 100.0).with_pivot(0.5, 0.0);
    assert_eq!(t.pivot_x, 0.5);
    assert_eq!(t.pivot_y, 0.0);

    let t2 = Transform::new(100.0, 100.0).with_pivot_top_center();
    assert_eq!(t2.pivot_x, 0.5);
    assert_eq!(t2.pivot_y, 0.0);

    let t3 = Transform::new(100.0, 100.0).with_pivot_bottom_center();
    assert_eq!(t3.pivot_x, 0.5);
    assert_eq!(t3.pivot_y, 1.0);
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
