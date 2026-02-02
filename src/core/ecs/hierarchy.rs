//! Entity hierarchy system for parent-child relationships
//!
//! This module provides components and utilities for creating hierarchical
//! entity relationships where child transforms are relative to their parent.
//!
//! # Key Concepts
//!
//! - **Parent/Children**: Relationship components linking entities
//! - **WorldTransform**: Computed world-space transform (includes parent transforms)
//! - **Transform propagation**: System that updates WorldTransform each frame
//!
//! # Example
//!
//! ```rust,ignore
//! // Create a parent entity
//! let ship = world.spawn((
//!     Transform::new(100.0, 200.0).with_rotation(45.0),
//! ));
//!
//! // Create a child (turret on the ship)
//! world.spawn_child(ship, (
//!     Transform::new(10.0, 0.0),  // Offset from ship center
//!     Renderable::sprite(turret_sprite),
//! ));
//!
//! // When ship rotates, turret rotates with it and orbits around ship center
//! ```

use super::components::Transform;
use hecs::Entity;

/// Marks an entity as a child of another entity.
///
/// The child's `Transform` is interpreted as relative to the parent.
/// Use `WorldTransform` to get the computed world-space position.
#[derive(Debug, Clone, Copy)]
pub struct Parent(pub Entity);

impl Parent {
  /// Get the parent entity
  pub fn entity(&self) -> Entity {
    self.0
  }
}

/// Tracks all children of an entity.
///
/// This component is automatically managed by the hierarchy system.
/// Do not modify directly - use `spawn_child()`, `set_parent()`, etc.
#[derive(Debug, Clone, Default)]
pub struct Children(pub Vec<Entity>);

impl Children {
  /// Create an empty children list
  pub fn new() -> Self {
    Self(Vec::new())
  }

  /// Get the number of children
  pub fn len(&self) -> usize {
    self.0.len()
  }

  /// Check if there are no children
  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  /// Iterate over children
  pub fn iter(&self) -> impl Iterator<Item = &Entity> {
    self.0.iter()
  }
}

/// Computed world-space transform.
///
/// For root entities (no parent), this equals the local `Transform`.
/// For child entities, this combines the parent's WorldTransform with
/// the child's local Transform.
///
/// This component is automatically updated by `propagate_transforms()`.
/// Use this for rendering and world-space calculations.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorldTransform {
  /// World X position
  pub x: f32,
  /// World Y position
  pub y: f32,
  /// Depth / Z position (not affected by 2D rotation)
  pub z: f32,
  /// Accumulated rotation in degrees
  pub rotation: f32,
  /// Accumulated X scale
  pub scale_x: f32,
  /// Accumulated Y scale
  pub scale_y: f32,
  /// Pivot X as normalized value (0.0 = left, 0.5 = center, 1.0 = right)
  pub pivot_x: f32,
  /// Pivot Y as normalized value (0.0 = top, 0.5 = center, 1.0 = bottom)
  pub pivot_y: f32,
}

impl WorldTransform {
  /// Create a new world transform at the origin
  pub fn new() -> Self {
    Self {
      x: 0.0,
      y: 0.0,
      z: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      pivot_x: 0.5,
      pivot_y: 0.5,
    }
  }

  /// Create a world transform from a local transform (for root entities)
  pub fn from_transform(t: &Transform) -> Self {
    Self {
      x: t.x,
      y: t.y,
      z: t.z,
      rotation: t.rotation,
      scale_x: t.scale_x,
      scale_y: t.scale_y,
      pivot_x: t.pivot_x,
      pivot_y: t.pivot_y,
    }
  }

  /// Compute child's world transform from parent world transform and child's local transform
  ///
  /// This properly handles:
  /// - Position: Child position is rotated and scaled by parent, then added
  /// - Rotation: Child rotation is added to parent rotation
  /// - Scale: Child scale is multiplied by parent scale
  /// - Pivot: Child's pivot is preserved (not affected by parent)
  pub fn from_parent_and_local(parent: &WorldTransform, local: &Transform) -> Self {
    // Convert parent rotation to radians for trig
    let angle = parent.rotation.to_radians();
    let cos_r = angle.cos();
    let sin_r = angle.sin();

    // Scale the local offset by parent's scale
    let scaled_x = local.x * parent.scale_x;
    let scaled_y = local.y * parent.scale_y;

    // Rotate the scaled offset by parent's rotation
    let rotated_x = scaled_x * cos_r - scaled_y * sin_r;
    let rotated_y = scaled_x * sin_r + scaled_y * cos_r;

    Self {
      // Position: parent position + rotated/scaled local offset
      x: parent.x + rotated_x,
      y: parent.y + rotated_y,
      // Z/depth: simply add (no rotation affects Z in 2D)
      z: parent.z + local.z,
      // Rotation: accumulate
      rotation: parent.rotation + local.rotation,
      // Scale: multiply
      scale_x: parent.scale_x * local.scale_x,
      scale_y: parent.scale_y * local.scale_y,
      // Pivot: use child's pivot (not inherited from parent)
      pivot_x: local.pivot_x,
      pivot_y: local.pivot_y,
    }
  }

  /// Get position as a 2D vector
  pub fn position(&self) -> glam::Vec2 {
    glam::Vec2::new(self.x, self.y)
  }

  /// Get position as a 3D vector
  pub fn position_3d(&self) -> glam::Vec3 {
    glam::Vec3::new(self.x, self.y, self.z)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn world_transform_from_transform() {
    let t = Transform {
      x: 10.0,
      y: 20.0,
      z: 5.0,
      rotation: 45.0,
      scale_x: 2.0,
      scale_y: 0.5,
      pivot_x: 0.5,
      pivot_y: 0.5,
    };

    let wt = WorldTransform::from_transform(&t);

    assert_eq!(wt.x, 10.0);
    assert_eq!(wt.y, 20.0);
    assert_eq!(wt.z, 5.0);
    assert_eq!(wt.rotation, 45.0);
    assert_eq!(wt.scale_x, 2.0);
    assert_eq!(wt.scale_y, 0.5);
    assert_eq!(wt.pivot_x, 0.5);
    assert_eq!(wt.pivot_y, 0.5);
  }

  #[test]
  fn world_transform_no_rotation() {
    let parent = WorldTransform {
      x: 100.0,
      y: 200.0,
      z: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      pivot_x: 0.5,
      pivot_y: 0.5,
    };

    let local = Transform {
      x: 10.0,
      y: 20.0,
      z: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      pivot_x: 0.5,
      pivot_y: 0.5,
    };

    let child = WorldTransform::from_parent_and_local(&parent, &local);

    assert_eq!(child.x, 110.0);
    assert_eq!(child.y, 220.0);
  }

  #[test]
  fn world_transform_with_rotation_90() {
    let parent = WorldTransform {
      x: 100.0,
      y: 100.0,
      z: 0.0,
      rotation: 90.0, // 90 degrees clockwise
      scale_x: 1.0,
      scale_y: 1.0,
      pivot_x: 0.5,
      pivot_y: 0.5,
    };

    let local = Transform {
      x: 10.0, // 10 units to the right locally
      y: 0.0,
      z: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      pivot_x: 0.5,
      pivot_y: 0.5,
    };

    let child = WorldTransform::from_parent_and_local(&parent, &local);

    // After 90 degree rotation, (10, 0) becomes (0, 10)
    assert!((child.x - 100.0).abs() < 0.001);
    assert!((child.y - 110.0).abs() < 0.001);
    assert_eq!(child.rotation, 90.0);
  }

  #[test]
  fn world_transform_with_scale() {
    let parent = WorldTransform {
      x: 0.0,
      y: 0.0,
      z: 0.0,
      rotation: 0.0,
      scale_x: 2.0,
      scale_y: 2.0,
      pivot_x: 0.5,
      pivot_y: 0.5,
    };

    let local = Transform {
      x: 10.0,
      y: 5.0,
      z: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      pivot_x: 0.5,
      pivot_y: 0.5,
    };

    let child = WorldTransform::from_parent_and_local(&parent, &local);

    // Position is scaled by parent
    assert_eq!(child.x, 20.0);
    assert_eq!(child.y, 10.0);
    // Scale is multiplied
    assert_eq!(child.scale_x, 2.0);
    assert_eq!(child.scale_y, 2.0);
  }

  #[test]
  fn world_transform_combined() {
    let parent = WorldTransform {
      x: 50.0,
      y: 50.0,
      z: 1.0,
      rotation: 45.0,
      scale_x: 2.0,
      scale_y: 1.0,
      pivot_x: 0.5,
      pivot_y: 0.5,
    };

    let local = Transform {
      x: 10.0,
      y: 0.0,
      z: 2.0,
      rotation: 15.0,
      scale_x: 0.5,
      scale_y: 2.0,
      pivot_x: 0.25,
      pivot_y: 0.75,
    };

    let child = WorldTransform::from_parent_and_local(&parent, &local);

    // Rotation accumulates
    assert_eq!(child.rotation, 60.0);
    // Scale multiplies
    assert_eq!(child.scale_x, 1.0); // 2.0 * 0.5
    assert_eq!(child.scale_y, 2.0); // 1.0 * 2.0
    // Z adds
    assert_eq!(child.z, 3.0);
    // Pivot is taken from local (child)
    assert_eq!(child.pivot_x, 0.25);
    assert_eq!(child.pivot_y, 0.75);
  }

  #[test]
  fn children_operations() {
    let mut children = Children::new();
    assert!(children.is_empty());
    assert_eq!(children.len(), 0);

    // Simulate adding children (normally done by hierarchy system)
    children.0.push(Entity::DANGLING);
    assert!(!children.is_empty());
    assert_eq!(children.len(), 1);
  }
}
