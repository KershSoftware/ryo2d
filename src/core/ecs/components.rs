//! Built-in components for the game engine

use serde::{Deserialize, Serialize};

// Re-export glam types for convenience
pub use glam::{Mat4, Vec2, Vec3, Vec4};

// Import Anchor from UI module (unified 9-point anchor for both UI and sprite positioning)
use crate::core::ui::Anchor;

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
/// - `(0.0, 0.0)` - Top-left corner (default) - standard 2D sprite positioning
/// - `(0.5, 0.5)` - Center - rotates around the center
/// - `(0.5, 0.0)` - Top-center - useful for hanging objects like pendulums
/// - `(0.0, 0.5)` - Left-center - useful for door hinges
/// - `(1.0, 1.0)` - Bottom-right corner
#[derive(Debug, Clone, Serialize, Deserialize)]
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
  /// Additional X offset in pixels, applied after anchor/pivot calculation
  pub offset_x: f32,
  /// Additional Y offset in pixels, applied after anchor/pivot calculation
  pub offset_y: f32,
}

impl Transform {
  /// Create a new 2D transform (z defaults to 0.0, pivot defaults to top-left)
  pub fn new(x: f32, y: f32) -> Self {
    Transform {
      x,
      y,
      z: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      pivot_x: 0.0,
      pivot_y: 0.0,
      offset_x: 0.0,
      offset_y: 0.0,
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
      pivot_x: 0.0,
      pivot_y: 0.0,
      offset_x: 0.0,
      offset_y: 0.0,
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
  /// - `(0.0, 0.0)` - Top-left corner (default)
  /// - `(0.5, 0.5)` - Center
  /// - `(0.5, 0.0)` - Top-center (pendulums, hanging objects)
  /// - `(0.0, 0.5)` - Left-center (door hinges)
  pub fn with_pivot(mut self, pivot_x: f32, pivot_y: f32) -> Self {
    self.pivot_x = pivot_x;
    self.pivot_y = pivot_y;
    self
  }

  /// Builder method to set pivot using an Anchor enum
  ///
  /// More readable alternative to `with_pivot()`:
  /// ```
  /// use ryo2d::prelude::*;
  /// Transform::new(100.0, 200.0).with_anchor(Anchor::BottomCenter);
  /// ```
  pub fn with_anchor(self, anchor: Anchor) -> Self {
    let (px, py) = anchor.to_pivot();
    self.with_pivot(px, py)
  }

  /// Builder method to set pixel offset from the anchor point
  ///
  /// Offsets are applied after anchor/pivot calculation, allowing fine-tuned
  /// positioning adjustments:
  /// ```
  /// use ryo2d::prelude::*;
  /// // Place bottom-center at (100, 200), then shift 10px right and 5px up
  /// Transform::new(100.0, 200.0)
  ///     .with_anchor(Anchor::BottomCenter)
  ///     .with_offset(10.0, -5.0);
  /// ```
  pub fn with_offset(mut self, offset_x: f32, offset_y: f32) -> Self {
    self.offset_x = offset_x;
    self.offset_y = offset_y;
    self
  }

  /// Builder method to set pivot at center (useful for centered rotation/scaling)
  pub fn with_pivot_center(self) -> Self {
    self.with_pivot(0.5, 0.5)
  }

  /// Builder method to set pivot at top-center (useful for hanging objects)
  pub fn with_pivot_top_center(self) -> Self {
    self.with_pivot(0.5, 0.0)
  }

  /// Builder method to set pivot at bottom-center (useful for standing objects)
  pub fn with_pivot_bottom_center(self) -> Self {
    self.with_pivot(0.5, 1.0)
  }

  /// Builder method to set pivot at top-left (default, but explicit for clarity)
  pub fn with_pivot_top_left(self) -> Self {
    self.with_pivot(0.0, 0.0)
  }

  /// Builder method to set pivot at top-right
  pub fn with_pivot_top_right(self) -> Self {
    self.with_pivot(1.0, 0.0)
  }

  /// Builder method to set pivot at center-left (useful for side-hinged objects)
  pub fn with_pivot_center_left(self) -> Self {
    self.with_pivot(0.0, 0.5)
  }

  /// Builder method to set pivot at center-right
  pub fn with_pivot_center_right(self) -> Self {
    self.with_pivot(1.0, 0.5)
  }

  /// Builder method to set pivot at bottom-left
  pub fn with_pivot_bottom_left(self) -> Self {
    self.with_pivot(0.0, 1.0)
  }

  /// Builder method to set pivot at bottom-right
  pub fn with_pivot_bottom_right(self) -> Self {
    self.with_pivot(1.0, 1.0)
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

/// Previous frame's transform, used for render interpolation.
///
/// When an entity has `Renderable::interpolate = true`, the renderer will
/// lerp between `PreviousTransform` and `Transform` based on the interpolation
/// alpha to achieve smooth motion between fixed timestep updates.
///
/// This component is automatically managed by the engine:
/// - Added when an entity with `interpolate = true` is first rendered
/// - Updated each frame before the fixed timestep updates
///
/// You generally don't need to interact with this component directly.
/// To disable interpolation for a teleport, use `Transform::skip_interpolation()`.
#[derive(Debug, Clone)]
pub struct PreviousTransform {
  pub x: f32,
  pub y: f32,
  pub z: f32,
  pub rotation: f32,
  pub scale_x: f32,
  pub scale_y: f32,
  /// When true, skip interpolation this frame (for teleports, spawns)
  pub skip: bool,
}

impl PreviousTransform {
  /// Create from a Transform, copying all position/rotation/scale values.
  pub fn from_transform(t: &Transform) -> Self {
    PreviousTransform {
      x: t.x,
      y: t.y,
      z: t.z,
      rotation: t.rotation,
      scale_x: t.scale_x,
      scale_y: t.scale_y,
      skip: false,
    }
  }

  /// Copy values from a Transform.
  pub fn copy_from(&mut self, t: &Transform) {
    self.x = t.x;
    self.y = t.y;
    self.z = t.z;
    self.rotation = t.rotation;
    self.scale_x = t.scale_x;
    self.scale_y = t.scale_y;
  }

  /// Mark to skip interpolation this frame (e.g., after teleport).
  pub fn skip_next_frame(&mut self) {
    self.skip = true;
  }

  /// Linearly interpolate between this and a target Transform.
  ///
  /// Returns (x, y, rotation, scale_x, scale_y) at the given alpha.
  /// Alpha of 0.0 = this transform, 1.0 = target transform.
  pub fn lerp(&self, target: &Transform, alpha: f32) -> (f32, f32, f32, f32, f32) {
    let x = self.x + (target.x - self.x) * alpha;
    let y = self.y + (target.y - self.y) * alpha;
    let rotation = self.rotation + (target.rotation - self.rotation) * alpha;
    let scale_x = self.scale_x + (target.scale_x - self.scale_x) * alpha;
    let scale_y = self.scale_y + (target.scale_y - self.scale_y) * alpha;
    (x, y, rotation, scale_x, scale_y)
  }
}

impl Default for PreviousTransform {
  fn default() -> Self {
    PreviousTransform {
      x: 0.0,
      y: 0.0,
      z: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      skip: true, // Skip on first frame (no previous data)
    }
  }
}

/// Movement per second in 2D/3D space
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

// ========== Replicated implementations ==========

use crate::core::network::replication::{ComponentTypeId, Replicated};

impl Replicated for Transform {
  fn component_id() -> ComponentTypeId {
    ComponentTypeId::from_name("ryo2d::Transform")
  }

  fn component_name() -> &'static str {
    "Transform"
  }

  fn replication_priority() -> u8 {
    // Transform is high priority - position updates are time-sensitive
    10
  }
}

impl Replicated for Velocity {
  fn component_id() -> ComponentTypeId {
    ComponentTypeId::from_name("ryo2d::Velocity")
  }

  fn component_name() -> &'static str {
    "Velocity"
  }

  fn replication_priority() -> u8 {
    // Velocity is medium priority
    5
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
    assert_eq!(t.pivot_x, 0.0);
    assert_eq!(t.pivot_y, 0.0);
    assert_eq!(t.offset_x, 0.0);
    assert_eq!(t.offset_y, 0.0);
  }

  #[test]
  fn transform_3d() {
    let t = Transform::new_3d(1.0, 2.0, 3.0);
    assert_eq!(t.x, 1.0);
    assert_eq!(t.y, 2.0);
    assert_eq!(t.z, 3.0);
    assert_eq!(t.pivot_x, 0.0);
    assert_eq!(t.pivot_y, 0.0);
    assert_eq!(t.offset_x, 0.0);
    assert_eq!(t.offset_y, 0.0);
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

    let t4 = Transform::new(100.0, 100.0).with_pivot_center();
    assert_eq!(t4.pivot_x, 0.5);
    assert_eq!(t4.pivot_y, 0.5);
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

  #[test]
  fn anchor_to_pivot() {
    assert_eq!(Anchor::TopLeft.to_pivot(), (0.0, 0.0));
    assert_eq!(Anchor::TopCenter.to_pivot(), (0.5, 0.0));
    assert_eq!(Anchor::TopRight.to_pivot(), (1.0, 0.0));
    assert_eq!(Anchor::CenterLeft.to_pivot(), (0.0, 0.5));
    assert_eq!(Anchor::Center.to_pivot(), (0.5, 0.5));
    assert_eq!(Anchor::CenterRight.to_pivot(), (1.0, 0.5));
    assert_eq!(Anchor::BottomLeft.to_pivot(), (0.0, 1.0));
    assert_eq!(Anchor::BottomCenter.to_pivot(), (0.5, 1.0));
    assert_eq!(Anchor::BottomRight.to_pivot(), (1.0, 1.0));
  }

  #[test]
  fn anchor_default() {
    assert_eq!(Anchor::default(), Anchor::TopLeft);
  }

  #[test]
  fn transform_with_anchor() {
    let t = Transform::new(100.0, 200.0).with_anchor(Anchor::BottomCenter);
    assert_eq!(t.pivot_x, 0.5);
    assert_eq!(t.pivot_y, 1.0);

    let t2 = Transform::new(100.0, 200.0).with_anchor(Anchor::Center);
    assert_eq!(t2.pivot_x, 0.5);
    assert_eq!(t2.pivot_y, 0.5);
  }

  #[test]
  fn transform_with_offset() {
    let t = Transform::new(100.0, 200.0).with_offset(10.0, -5.0);
    assert_eq!(t.offset_x, 10.0);
    assert_eq!(t.offset_y, -5.0);
  }

  #[test]
  fn transform_anchor_and_offset_combined() {
    let t = Transform::new(100.0, 200.0)
      .with_anchor(Anchor::BottomCenter)
      .with_offset(10.0, -5.0);
    assert_eq!(t.x, 100.0);
    assert_eq!(t.y, 200.0);
    assert_eq!(t.pivot_x, 0.5);
    assert_eq!(t.pivot_y, 1.0);
    assert_eq!(t.offset_x, 10.0);
    assert_eq!(t.offset_y, -5.0);
  }

  #[test]
  fn transform_all_pivot_corners() {
    let tl = Transform::new(0.0, 0.0).with_pivot_top_left();
    assert_eq!((tl.pivot_x, tl.pivot_y), (0.0, 0.0));

    let tr = Transform::new(0.0, 0.0).with_pivot_top_right();
    assert_eq!((tr.pivot_x, tr.pivot_y), (1.0, 0.0));

    let bl = Transform::new(0.0, 0.0).with_pivot_bottom_left();
    assert_eq!((bl.pivot_x, bl.pivot_y), (0.0, 1.0));

    let br = Transform::new(0.0, 0.0).with_pivot_bottom_right();
    assert_eq!((br.pivot_x, br.pivot_y), (1.0, 1.0));

    let cl = Transform::new(0.0, 0.0).with_pivot_center_left();
    assert_eq!((cl.pivot_x, cl.pivot_y), (0.0, 0.5));

    let cr = Transform::new(0.0, 0.0).with_pivot_center_right();
    assert_eq!((cr.pivot_x, cr.pivot_y), (1.0, 0.5));
  }
}
