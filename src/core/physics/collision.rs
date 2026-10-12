//! Collider shapes for physics bodies.

use crate::core::ecs::{Component, Vec2};

/// Collider shapes that can be attached to entities
#[derive(Debug, Clone)]
pub enum ColliderShape {
  /// Axis-aligned box, centred on the entity's origin plus the offset
  Box { width: f32, height: f32 },
  /// Circle, centred on the entity's origin plus the offset
  Circle { radius: f32 },
}

/// The collision shape of a physics body.
///
/// The shape is centred on the entity's origin, which is the sprite's centre
/// with the default `Anchor::Center`; `with_offset` moves it from there.
/// Sizes are world units: the entity's scale does not resize the collider.
#[derive(Component, Debug, Clone)]
pub struct Collider {
  /// The shape of this collider
  pub shape: ColliderShape,
  /// Offset of the shape's centre from the entity's origin
  pub offset: Vec2,
  /// Whether this collider is a trigger (no physics response, just collision messages)
  pub is_trigger: bool,
  /// Collision layer (for filtering)
  pub layer: u32,
  /// Collision mask (which layers to collide with)
  pub mask: u32,
  /// Restitution (bounciness): 0.0 = no bounce, 1.0 = perfect bounce
  pub restitution: f32,
  /// Friction: 0.0 = frictionless, higher = more drag
  pub friction: f32,
}

impl Collider {
  fn new(shape: ColliderShape) -> Self {
    Collider {
      shape,
      offset: Vec2::ZERO,
      is_trigger: false,
      layer: 1,
      mask: u32::MAX,   // Collide with everything by default
      restitution: 0.5, // Moderate bounce
      friction: 0.3,    // Moderate friction
    }
  }

  /// Create a box collider
  pub fn box_collider(width: f32, height: f32) -> Self {
    Self::new(ColliderShape::Box { width, height })
  }

  /// Create a circle collider
  pub fn circle_collider(radius: f32) -> Self {
    Self::new(ColliderShape::Circle { radius })
  }

  /// Move the shape's centre away from the entity's origin, for example a
  /// hitbox to one side of a character.
  pub fn with_offset(mut self, x: f32, y: f32) -> Self {
    self.offset = Vec2::new(x, y);
    self
  }

  /// Make this a trigger collider
  pub fn as_trigger(mut self) -> Self {
    self.is_trigger = true;
    self
  }

  /// Set collision layer
  pub fn with_layer(mut self, layer: u32) -> Self {
    self.layer = layer;
    self
  }

  /// Set collision mask
  pub fn with_mask(mut self, mask: u32) -> Self {
    self.mask = mask;
    self
  }

  /// Set restitution (bounciness)
  /// - 0.0 = no bounce (inelastic collision)
  /// - 1.0 = perfect bounce (elastic collision)
  /// - >1.0 = adds energy (not physically accurate)
  pub fn with_restitution(mut self, restitution: f32) -> Self {
    self.restitution = restitution;
    self
  }

  /// Set friction coefficient
  /// - 0.0 = frictionless (no tangential drag)
  /// - Higher values = more resistance to sliding
  pub fn with_friction(mut self, friction: f32) -> Self {
    self.friction = friction;
    self
  }

  /// Check if this collider should interact with another based on layers
  pub fn should_collide_with(&self, other: &Collider) -> bool {
    (self.mask & other.layer) != 0 && (other.mask & self.layer) != 0
  }

  /// The shape's centre when the entity's origin is at `position`.
  pub fn world_center(&self, position: Vec2) -> Vec2 {
    position + self.offset
  }

  /// The shape's axis-aligned bounds `(min, max)` when the entity's origin is
  /// at `position`.
  pub fn world_bounds(&self, position: Vec2) -> (Vec2, Vec2) {
    let center = self.world_center(position);
    let half = match self.shape {
      ColliderShape::Box { width, height } => Vec2::new(width, height) / 2.0,
      ColliderShape::Circle { radius } => Vec2::splat(radius),
    };
    (center - half, center + half)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn collider_builders() {
    let collider = Collider::box_collider(32.0, 32.0)
      .with_offset(5.0, -5.0)
      .with_restitution(0.8)
      .with_friction(0.6)
      .as_trigger();
    assert_eq!(collider.offset, Vec2::new(5.0, -5.0));
    assert_eq!(collider.restitution, 0.8);
    assert_eq!(collider.friction, 0.6);
    assert!(collider.is_trigger);
  }

  #[test]
  fn collider_layer_filtering() {
    let a = Collider::box_collider(10.0, 10.0)
      .with_layer(1)
      .with_mask(2);
    let b = Collider::box_collider(10.0, 10.0)
      .with_layer(2)
      .with_mask(1);
    let c = Collider::box_collider(10.0, 10.0)
      .with_layer(4)
      .with_mask(4);

    assert!(a.should_collide_with(&b));
    assert!(b.should_collide_with(&a));
    assert!(!a.should_collide_with(&c));
  }

  #[test]
  fn world_bounds_are_centred_on_the_origin_plus_offset() {
    let boxed = Collider::box_collider(20.0, 10.0);
    assert_eq!(
      boxed.world_bounds(Vec2::new(100.0, 50.0)),
      (Vec2::new(90.0, 45.0), Vec2::new(110.0, 55.0))
    );
    let circle = Collider::circle_collider(8.0).with_offset(0.0, -10.0);
    assert_eq!(
      circle.world_bounds(Vec2::ZERO),
      (Vec2::new(-8.0, -18.0), Vec2::new(8.0, -2.0))
    );
  }
}
