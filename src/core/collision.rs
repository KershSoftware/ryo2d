//! Collision detection components and systems
//!
//! This module provides:
//! - Collider shapes (AABB, Circle)
//! - Collision detection algorithms
//! - Collision results and events

use crate::core::ecs::Entity;

/// Axis-Aligned Bounding Box
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AABB {
  /// Left edge (minimum x)
  pub min_x: f32,
  /// Top edge (minimum y)
  pub min_y: f32,
  /// Right edge (maximum x)
  pub max_x: f32,
  /// Bottom edge (maximum y)
  pub max_y: f32,
}

impl AABB {
  /// Create an AABB from min/max coordinates
  pub fn new(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Self {
    AABB {
      min_x,
      min_y,
      max_x,
      max_y,
    }
  }

  /// Create an AABB from position and size
  pub fn from_position_size(x: f32, y: f32, width: f32, height: f32) -> Self {
    AABB {
      min_x: x,
      min_y: y,
      max_x: x + width,
      max_y: y + height,
    }
  }

  /// Create an AABB centered at a position
  pub fn centered(cx: f32, cy: f32, width: f32, height: f32) -> Self {
    let half_w = width / 2.0;
    let half_h = height / 2.0;
    AABB {
      min_x: cx - half_w,
      min_y: cy - half_h,
      max_x: cx + half_w,
      max_y: cy + half_h,
    }
  }

  /// Get the width of the AABB
  pub fn width(&self) -> f32 {
    self.max_x - self.min_x
  }

  /// Get the height of the AABB
  pub fn height(&self) -> f32 {
    self.max_y - self.min_y
  }

  /// Get the center point
  pub fn center(&self) -> (f32, f32) {
    (
      (self.min_x + self.max_x) / 2.0,
      (self.min_y + self.max_y) / 2.0,
    )
  }

  /// Check if this AABB intersects another
  pub fn intersects(&self, other: &AABB) -> bool {
    self.min_x < other.max_x
      && self.max_x > other.min_x
      && self.min_y < other.max_y
      && self.max_y > other.min_y
  }

  /// Check if this AABB contains a point
  pub fn contains_point(&self, x: f32, y: f32) -> bool {
    x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
  }

  /// Calculate the overlap with another AABB (returns None if no overlap)
  pub fn overlap(&self, other: &AABB) -> Option<(f32, f32)> {
    if !self.intersects(other) {
      return None;
    }

    let overlap_x = (self.max_x.min(other.max_x) - self.min_x.max(other.min_x)).max(0.0);
    let overlap_y = (self.max_y.min(other.max_y) - self.min_y.max(other.min_y)).max(0.0);

    Some((overlap_x, overlap_y))
  }

  /// Translate the AABB by an offset
  pub fn translate(&self, dx: f32, dy: f32) -> AABB {
    AABB {
      min_x: self.min_x + dx,
      min_y: self.min_y + dy,
      max_x: self.max_x + dx,
      max_y: self.max_y + dy,
    }
  }
}

/// Circle shape for collision detection
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
  /// Center X coordinate
  pub x: f32,
  /// Center Y coordinate
  pub y: f32,
  /// Radius
  pub radius: f32,
}

impl Circle {
  /// Create a new circle
  pub fn new(x: f32, y: f32, radius: f32) -> Self {
    Circle { x, y, radius }
  }

  /// Check if this circle intersects another
  pub fn intersects_circle(&self, other: &Circle) -> bool {
    let dx = self.x - other.x;
    let dy = self.y - other.y;
    let distance_squared = dx * dx + dy * dy;
    let radii_sum = self.radius + other.radius;
    distance_squared < radii_sum * radii_sum
  }

  /// Check if this circle intersects an AABB
  pub fn intersects_aabb(&self, aabb: &AABB) -> bool {
    // Find the closest point on the AABB to the circle center
    let closest_x = self.x.clamp(aabb.min_x, aabb.max_x);
    let closest_y = self.y.clamp(aabb.min_y, aabb.max_y);

    // Check if that point is within the circle
    let dx = self.x - closest_x;
    let dy = self.y - closest_y;
    let distance_squared = dx * dx + dy * dy;

    distance_squared < self.radius * self.radius
  }

  /// Check if this circle contains a point
  pub fn contains_point(&self, x: f32, y: f32) -> bool {
    let dx = self.x - x;
    let dy = self.y - y;
    dx * dx + dy * dy <= self.radius * self.radius
  }

  /// Translate the circle by an offset
  pub fn translate(&self, dx: f32, dy: f32) -> Circle {
    Circle {
      x: self.x + dx,
      y: self.y + dy,
      radius: self.radius,
    }
  }

  /// Get the bounding AABB of this circle
  pub fn bounding_aabb(&self) -> AABB {
    AABB {
      min_x: self.x - self.radius,
      min_y: self.y - self.radius,
      max_x: self.x + self.radius,
      max_y: self.y + self.radius,
    }
  }
}

/// Collider shapes that can be attached to entities
#[derive(Debug, Clone)]
pub enum ColliderShape {
  /// Axis-aligned bounding box (offset from entity position)
  Box { width: f32, height: f32 },
  /// Circle (offset from entity position)
  Circle { radius: f32 },
}

/// Collider component - attach to entities for collision detection
#[derive(Debug, Clone)]
pub struct Collider {
  /// The shape of this collider
  pub shape: ColliderShape,
  /// Offset from the entity's Transform position
  pub offset_x: f32,
  pub offset_y: f32,
  /// Whether this collider is a trigger (no physics response, just events)
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
  /// Create a box collider
  pub fn box_collider(width: f32, height: f32) -> Self {
    Collider {
      shape: ColliderShape::Box { width, height },
      offset_x: 0.0,
      offset_y: 0.0,
      is_trigger: false,
      layer: 1,
      mask: u32::MAX,   // Collide with everything by default
      restitution: 0.5, // Moderate bounce
      friction: 0.3,    // Moderate friction
    }
  }

  /// Create a circle collider
  pub fn circle_collider(radius: f32) -> Self {
    Collider {
      shape: ColliderShape::Circle { radius },
      offset_x: 0.0,
      offset_y: 0.0,
      is_trigger: false,
      layer: 1,
      mask: u32::MAX,
      restitution: 0.5, // Moderate bounce
      friction: 0.3,    // Moderate friction
    }
  }

  /// Set the offset from entity position
  pub fn with_offset(mut self, x: f32, y: f32) -> Self {
    self.offset_x = x;
    self.offset_y = y;
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

  /// Get the AABB of this collider at a given position
  pub fn get_aabb(&self, entity_x: f32, entity_y: f32) -> AABB {
    let x = entity_x + self.offset_x;
    let y = entity_y + self.offset_y;

    match self.shape {
      ColliderShape::Box { width, height } => AABB::from_position_size(x, y, width, height),
      ColliderShape::Circle { radius } => Circle::new(x, y, radius).bounding_aabb(),
    }
  }

  /// Get the Circle of this collider (if applicable) at a given position
  pub fn get_circle(&self, entity_x: f32, entity_y: f32) -> Option<Circle> {
    match self.shape {
      ColliderShape::Circle { radius } => Some(Circle::new(
        entity_x + self.offset_x,
        entity_y + self.offset_y,
        radius,
      )),
      _ => None,
    }
  }
}

/// Information about a collision between two entities
#[derive(Debug, Clone, Copy)]
pub struct Collision {
  /// First entity in the collision
  pub entity_a: Entity,
  /// Second entity in the collision
  pub entity_b: Entity,
  /// Penetration depth on X axis (positive = overlap)
  pub penetration_x: f32,
  /// Penetration depth on Y axis (positive = overlap)
  pub penetration_y: f32,
  /// Normal direction X (pointing from A to B)
  pub normal_x: f32,
  /// Normal direction Y (pointing from A to B)
  pub normal_y: f32,
}

impl Collision {
  /// Get the minimum translation vector to separate the entities
  /// Returns (dx, dy) to move entity A to resolve the collision
  pub fn mtv(&self) -> (f32, f32) {
    if self.penetration_x.abs() < self.penetration_y.abs() {
      (-self.normal_x * self.penetration_x, 0.0)
    } else {
      (0.0, -self.normal_y * self.penetration_y)
    }
  }
}

/// Check collision between two colliders at given positions
pub fn check_collision(
  collider_a: &Collider,
  pos_a: (f32, f32),
  collider_b: &Collider,
  pos_b: (f32, f32),
) -> Option<(f32, f32, f32, f32)> {
  // Check layer filtering
  if !collider_a.should_collide_with(collider_b) {
    return None;
  }

  match (&collider_a.shape, &collider_b.shape) {
    // Box vs Box
    (
      ColliderShape::Box {
        width: w1,
        height: h1,
      },
      ColliderShape::Box {
        width: w2,
        height: h2,
      },
    ) => {
      let aabb_a = AABB::from_position_size(
        pos_a.0 + collider_a.offset_x,
        pos_a.1 + collider_a.offset_y,
        *w1,
        *h1,
      );
      let aabb_b = AABB::from_position_size(
        pos_b.0 + collider_b.offset_x,
        pos_b.1 + collider_b.offset_y,
        *w2,
        *h2,
      );

      if let Some((overlap_x, overlap_y)) = aabb_a.overlap(&aabb_b) {
        let (cx_a, cy_a) = aabb_a.center();
        let (cx_b, cy_b) = aabb_b.center();
        let normal_x = if cx_b > cx_a { 1.0 } else { -1.0 };
        let normal_y = if cy_b > cy_a { 1.0 } else { -1.0 };
        Some((overlap_x, overlap_y, normal_x, normal_y))
      } else {
        None
      }
    }

    // Circle vs Circle
    (ColliderShape::Circle { radius: r1 }, ColliderShape::Circle { radius: r2 }) => {
      let c1 = Circle::new(
        pos_a.0 + collider_a.offset_x,
        pos_a.1 + collider_a.offset_y,
        *r1,
      );
      let c2 = Circle::new(
        pos_b.0 + collider_b.offset_x,
        pos_b.1 + collider_b.offset_y,
        *r2,
      );

      if c1.intersects_circle(&c2) {
        let dx = c2.x - c1.x;
        let dy = c2.y - c1.y;
        let distance = (dx * dx + dy * dy).sqrt();
        let penetration = (r1 + r2) - distance;

        if distance > 0.0001 {
          let normal_x = dx / distance;
          let normal_y = dy / distance;
          Some((penetration, penetration, normal_x, normal_y))
        } else {
          // Circles are at same position, arbitrary normal
          Some((penetration, penetration, 1.0, 0.0))
        }
      } else {
        None
      }
    }

    // Box vs Circle (or Circle vs Box)
    (ColliderShape::Box { width, height }, ColliderShape::Circle { radius }) => {
      check_aabb_circle_collision(
        pos_a.0 + collider_a.offset_x,
        pos_a.1 + collider_a.offset_y,
        *width,
        *height,
        pos_b.0 + collider_b.offset_x,
        pos_b.1 + collider_b.offset_y,
        *radius,
        false,
      )
    }
    (ColliderShape::Circle { radius }, ColliderShape::Box { width, height }) => {
      check_aabb_circle_collision(
        pos_b.0 + collider_b.offset_x,
        pos_b.1 + collider_b.offset_y,
        *width,
        *height,
        pos_a.0 + collider_a.offset_x,
        pos_a.1 + collider_a.offset_y,
        *radius,
        true,
      )
    }
  }
}

/// Helper for AABB vs Circle collision
#[allow(clippy::too_many_arguments)]
fn check_aabb_circle_collision(
  box_x: f32,
  box_y: f32,
  box_w: f32,
  box_h: f32,
  circle_x: f32,
  circle_y: f32,
  radius: f32,
  flip_normal: bool,
) -> Option<(f32, f32, f32, f32)> {
  let aabb = AABB::from_position_size(box_x, box_y, box_w, box_h);
  let circle = Circle::new(circle_x, circle_y, radius);

  if circle.intersects_aabb(&aabb) {
    // Find closest point on AABB to circle center
    let closest_x = circle_x.clamp(aabb.min_x, aabb.max_x);
    let closest_y = circle_y.clamp(aabb.min_y, aabb.max_y);

    let dx = circle_x - closest_x;
    let dy = circle_y - closest_y;
    let distance = (dx * dx + dy * dy).sqrt();

    let penetration = radius - distance;

    let (normal_x, normal_y) = if distance > 0.0001 {
      (dx / distance, dy / distance)
    } else {
      // Circle center is inside AABB
      (0.0, -1.0)
    };

    let (nx, ny) = if flip_normal {
      (normal_x, normal_y)
    } else {
      (-normal_x, -normal_y)
    };

    Some((penetration, penetration, nx, ny))
  } else {
    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn aabb_intersects() {
    let a = AABB::new(0.0, 0.0, 10.0, 10.0);
    let b = AABB::new(5.0, 5.0, 15.0, 15.0);
    let c = AABB::new(20.0, 20.0, 30.0, 30.0);

    assert!(a.intersects(&b));
    assert!(b.intersects(&a));
    assert!(!a.intersects(&c));
    assert!(!c.intersects(&a));
  }

  #[test]
  fn aabb_overlap() {
    let a = AABB::new(0.0, 0.0, 10.0, 10.0);
    let b = AABB::new(5.0, 5.0, 15.0, 15.0);

    let overlap = a.overlap(&b).unwrap();
    assert_eq!(overlap, (5.0, 5.0));
  }

  #[test]
  fn aabb_contains_point() {
    let a = AABB::new(0.0, 0.0, 10.0, 10.0);
    assert!(a.contains_point(5.0, 5.0));
    assert!(a.contains_point(0.0, 0.0));
    assert!(!a.contains_point(15.0, 5.0));
  }

  #[test]
  fn circle_intersects_circle() {
    let a = Circle::new(0.0, 0.0, 5.0);
    let b = Circle::new(8.0, 0.0, 5.0);
    let c = Circle::new(20.0, 0.0, 5.0);

    assert!(a.intersects_circle(&b));
    assert!(!a.intersects_circle(&c));
  }

  #[test]
  fn circle_intersects_aabb() {
    let circle = Circle::new(0.0, 0.0, 5.0);
    let aabb_close = AABB::new(3.0, 0.0, 10.0, 10.0);
    let aabb_far = AABB::new(10.0, 10.0, 20.0, 20.0);

    assert!(circle.intersects_aabb(&aabb_close));
    assert!(!circle.intersects_aabb(&aabb_far));
  }

  #[test]
  fn collider_box_creation() {
    let collider = Collider::box_collider(32.0, 32.0).with_offset(5.0, 5.0);
    assert_eq!(collider.offset_x, 5.0);
    assert_eq!(collider.offset_y, 5.0);
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
  fn check_collision_box_box() {
    let a = Collider::box_collider(10.0, 10.0);
    let b = Collider::box_collider(10.0, 10.0);

    // Overlapping
    let result = check_collision(&a, (0.0, 0.0), &b, (5.0, 5.0));
    assert!(result.is_some());

    // Not overlapping
    let result = check_collision(&a, (0.0, 0.0), &b, (20.0, 20.0));
    assert!(result.is_none());
  }

  #[test]
  fn check_collision_circle_circle() {
    let a = Collider::circle_collider(5.0);
    let b = Collider::circle_collider(5.0);

    // Overlapping
    let result = check_collision(&a, (0.0, 0.0), &b, (8.0, 0.0));
    assert!(result.is_some());

    // Not overlapping
    let result = check_collision(&a, (0.0, 0.0), &b, (20.0, 0.0));
    assert!(result.is_none());
  }

  #[test]
  fn aabb_translate() {
    let aabb = AABB::new(0.0, 0.0, 10.0, 10.0);
    let translated = aabb.translate(5.0, 3.0);
    assert_eq!(translated, AABB::new(5.0, 3.0, 15.0, 13.0));
  }

  #[test]
  fn aabb_center() {
    let aabb = AABB::new(0.0, 0.0, 10.0, 10.0);
    assert_eq!(aabb.center(), (5.0, 5.0));
  }

  #[test]
  fn aabb_width_height() {
    let aabb = AABB::new(0.0, 0.0, 10.0, 20.0);
    assert_eq!(aabb.width(), 10.0);
    assert_eq!(aabb.height(), 20.0);
  }

  #[test]
  fn aabb_from_position_size() {
    let aabb = AABB::from_position_size(10.0, 20.0, 5.0, 8.0);
    assert_eq!(aabb.min_x, 10.0);
    assert_eq!(aabb.min_y, 20.0);
    assert_eq!(aabb.max_x, 15.0);
    assert_eq!(aabb.max_y, 28.0);
  }

  #[test]
  fn aabb_centered() {
    let aabb = AABB::centered(10.0, 20.0, 6.0, 8.0);
    assert_eq!(aabb.min_x, 7.0);
    assert_eq!(aabb.min_y, 16.0);
    assert_eq!(aabb.max_x, 13.0);
    assert_eq!(aabb.max_y, 24.0);
  }

  #[test]
  fn circle_translate() {
    let circle = Circle::new(0.0, 0.0, 5.0);
    let translated = circle.translate(10.0, 20.0);
    assert_eq!(translated.x, 10.0);
    assert_eq!(translated.y, 20.0);
    assert_eq!(translated.radius, 5.0);
  }

  #[test]
  fn circle_contains_point() {
    let circle = Circle::new(0.0, 0.0, 5.0);
    assert!(circle.contains_point(0.0, 0.0));
    assert!(circle.contains_point(3.0, 4.0)); // 3-4-5 triangle
    assert!(!circle.contains_point(6.0, 0.0));
  }

  #[test]
  fn circle_bounding_aabb() {
    let circle = Circle::new(10.0, 20.0, 5.0);
    let aabb = circle.bounding_aabb();
    assert_eq!(aabb.min_x, 5.0);
    assert_eq!(aabb.min_y, 15.0);
    assert_eq!(aabb.max_x, 15.0);
    assert_eq!(aabb.max_y, 25.0);
  }

  #[test]
  fn collider_with_restitution() {
    let collider = Collider::box_collider(10.0, 10.0).with_restitution(0.8);
    assert_eq!(collider.restitution, 0.8);
  }

  #[test]
  fn collider_with_friction() {
    let collider = Collider::box_collider(10.0, 10.0).with_friction(0.6);
    assert_eq!(collider.friction, 0.6);
  }

  #[test]
  fn collider_as_trigger() {
    let collider = Collider::box_collider(10.0, 10.0).as_trigger();
    assert!(collider.is_trigger);
  }

  #[test]
  fn collider_get_aabb() {
    let collider = Collider::box_collider(20.0, 30.0).with_offset(5.0, 10.0);
    let aabb = collider.get_aabb(100.0, 200.0);
    assert_eq!(aabb.min_x, 105.0);
    assert_eq!(aabb.min_y, 210.0);
    assert_eq!(aabb.max_x, 125.0);
    assert_eq!(aabb.max_y, 240.0);
  }

  #[test]
  fn collider_get_circle() {
    let box_collider = Collider::box_collider(10.0, 10.0);
    assert!(box_collider.get_circle(0.0, 0.0).is_none());

    let circle_collider = Collider::circle_collider(5.0).with_offset(10.0, 20.0);
    let circle = circle_collider.get_circle(100.0, 200.0).unwrap();
    assert_eq!(circle.x, 110.0);
    assert_eq!(circle.y, 220.0);
    assert_eq!(circle.radius, 5.0);
  }

  #[test]
  fn collision_mtv_prefers_smaller_axis() {
    use crate::core::ecs::World;
    let mut world = World::new();
    let e1 = world.spawn(());
    let e2 = world.spawn(());

    // Mock collision with more penetration on Y
    let collision = Collision {
      entity_a: e1,
      entity_b: e2,
      penetration_x: 2.0,
      penetration_y: 5.0,
      normal_x: 1.0,
      normal_y: 1.0,
    };
    let mtv = collision.mtv();
    assert_eq!(mtv, (-2.0, 0.0)); // Resolves on smaller (X) axis

    // More penetration on X
    let collision2 = Collision {
      entity_a: e1,
      entity_b: e2,
      penetration_x: 10.0,
      penetration_y: 3.0,
      normal_x: 1.0,
      normal_y: -1.0,
    };
    let mtv2 = collision2.mtv();
    assert_eq!(mtv2, (0.0, 3.0)); // Resolves on smaller (Y) axis
  }
}
