//! Collision shapes and their material.

use crate::core::ecs::{Component, GlobalTransform, Transform, Vec2};

/// The geometry of a collider, in its own space: centred on its origin
/// (the entity's origin plus `Collider::offset`, turned by `rotation`).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ColliderShape {
  /// A box `size` wide and high.
  Rect {
    size: Vec2,
  },
  Circle {
    radius: f32,
  },
  /// An upright capsule `height` tall including its round ends; a capsule
  /// no taller than its width is a circle. Rotate it for a horizontal one.
  Capsule {
    height: f32,
    radius: f32,
  },
  /// The convex hull of the points. Concave shapes are several colliders.
  Polygon {
    points: Vec<Vec2>,
  },
  /// One edge.
  Segment {
    a: Vec2,
    b: Vec2,
  },
  /// An open chain of edges: terrain and level outlines.
  Polyline {
    points: Vec<Vec2>,
  },
}

/// A collision shape and its material. On an entity with a `RigidBody` (or
/// below one) it is part of that body; with no body above it, it is a fixed
/// collider of its own. The shape follows the entity's world scale. Edits
/// apply in place on the next step.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(Transform)]
pub struct Collider {
  pub shape: ColliderShape,
  /// The shape's origin relative to the entity's.
  pub offset: Vec2,
  /// The shape's turn relative to the entity, in radians.
  pub rotation: f32,
  /// Detects overlaps but never pushes.
  pub is_trigger: bool,
  /// The layers it is on (bits). Two colliders interact only if each one's
  /// layer is in the other's mask.
  pub layer: u32,
  /// The layers it interacts with (bits).
  pub mask: u32,
  /// Mass per square metre (`PhysicsConfig::pixels_per_meter`).
  pub density: f32,
  /// This collider's mass, overriding `density`.
  pub mass: Option<f32>,
  /// 0 is frictionless. Two surfaces combine as the geometric mean.
  pub friction: f32,
  /// 0 does not bounce, 1 bounces back fully. Two surfaces take the larger.
  pub restitution: f32,
  /// Send `ContactForce` messages while its contact force exceeds this.
  pub force_threshold: Option<f32>,
}

impl Collider {
  fn new(shape: ColliderShape) -> Self {
    Self {
      shape,
      offset: Vec2::ZERO,
      rotation: 0.0,
      is_trigger: false,
      layer: 1,
      mask: u32::MAX,
      density: 1.0,
      mass: None,
      friction: 0.5,
      restitution: 0.0,
      force_threshold: None,
    }
  }

  pub fn rect(width: f32, height: f32) -> Self {
    Self::new(ColliderShape::Rect {
      size: Vec2::new(width, height),
    })
  }

  pub fn circle(radius: f32) -> Self {
    Self::new(ColliderShape::Circle { radius })
  }

  /// An upright capsule `height` tall, ends included.
  pub fn capsule(height: f32, radius: f32) -> Self {
    Self::new(ColliderShape::Capsule { height, radius })
  }

  /// The convex hull of `points`.
  pub fn polygon(points: impl IntoIterator<Item = Vec2>) -> Self {
    Self::new(ColliderShape::Polygon {
      points: points.into_iter().collect(),
    })
  }

  pub fn segment(a: Vec2, b: Vec2) -> Self {
    Self::new(ColliderShape::Segment { a, b })
  }

  /// An open chain through `points`.
  pub fn polyline(points: impl IntoIterator<Item = Vec2>) -> Self {
    Self::new(ColliderShape::Polyline {
      points: points.into_iter().collect(),
    })
  }

  pub fn with_offset(mut self, x: f32, y: f32) -> Self {
    self.offset = Vec2::new(x, y);
    self
  }

  pub fn with_rotation(mut self, radians: f32) -> Self {
    self.rotation = radians;
    self
  }

  pub fn as_trigger(mut self) -> Self {
    self.is_trigger = true;
    self
  }

  pub fn with_layer(mut self, layer: u32) -> Self {
    self.layer = layer;
    self
  }

  pub fn with_mask(mut self, mask: u32) -> Self {
    self.mask = mask;
    self
  }

  pub fn with_density(mut self, density: f32) -> Self {
    self.density = density;
    self
  }

  pub fn with_mass(mut self, mass: f32) -> Self {
    self.mass = Some(mass);
    self
  }

  pub fn with_friction(mut self, friction: f32) -> Self {
    self.friction = friction;
    self
  }

  pub fn with_restitution(mut self, restitution: f32) -> Self {
    self.restitution = restitution;
    self
  }

  /// Opt into `ContactForce` messages above `threshold` (mass·px/s²).
  pub fn with_force_events(mut self, threshold: f32) -> Self {
    self.force_threshold = Some(threshold);
    self
  }

  /// The world-space bounding box `(min, max)` when the collider's entity
  /// has `global`. Round ends grow by the largest scale.
  pub fn world_bounds(&self, global: &GlobalTransform) -> (Vec2, Vec2) {
    let affine = global.affine();
    let turn = Vec2::from_angle(self.rotation);
    let place = |p: Vec2| affine.transform_point2(self.offset + turn.rotate(p));
    let scale = global.scale().max_element();
    let round = |centre: Vec2, radius: f32| {
      let c = place(centre);
      let r = Vec2::splat(radius * scale);
      (c - r, c + r)
    };
    match &self.shape {
      ColliderShape::Rect { size } => {
        let h = *size / 2.0;
        bounds_of(
          [(-h.x, -h.y), (h.x, -h.y), (h.x, h.y), (-h.x, h.y)].map(|(x, y)| place(Vec2::new(x, y))),
        )
      }
      ColliderShape::Circle { radius } => round(Vec2::ZERO, *radius),
      ColliderShape::Capsule { height, radius } => {
        let h = (height / 2.0 - radius).max(0.0);
        let (a, b) = (
          round(Vec2::new(0.0, -h), *radius),
          round(Vec2::new(0.0, h), *radius),
        );
        (a.0.min(b.0), a.1.max(b.1))
      }
      ColliderShape::Polygon { points } | ColliderShape::Polyline { points } => {
        bounds_of(points.iter().map(|p| place(*p)))
      }
      ColliderShape::Segment { a, b } => bounds_of([place(*a), place(*b)]),
    }
  }
}

fn bounds_of(points: impl IntoIterator<Item = Vec2>) -> (Vec2, Vec2) {
  let mut points = points.into_iter();
  let Some(first) = points.next() else {
    return (Vec2::ZERO, Vec2::ZERO);
  };
  points.fold((first, first), |(min, max), p| (min.min(p), max.max(p)))
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::f32::consts::FRAC_PI_2;

  fn close(a: (Vec2, Vec2), b: (Vec2, Vec2)) -> bool {
    (a.0 - b.0).abs().max_element() < 1e-3 && (a.1 - b.1).abs().max_element() < 1e-3
  }

  #[test]
  fn defaults() {
    let c = Collider::rect(10.0, 20.0);
    assert_eq!(
      c.shape,
      ColliderShape::Rect {
        size: Vec2::new(10.0, 20.0)
      }
    );
    assert_eq!((c.friction, c.restitution, c.density), (0.5, 0.0, 1.0));
    assert_eq!((c.layer, c.mask), (1, u32::MAX));
    assert!(!c.is_trigger && c.mass.is_none() && c.force_threshold.is_none());
  }

  #[test]
  fn builders() {
    let c = Collider::circle(4.0)
      .with_offset(1.0, 2.0)
      .with_rotation(0.5)
      .as_trigger()
      .with_layer(2)
      .with_mask(6)
      .with_density(3.0)
      .with_mass(7.0)
      .with_friction(0.1)
      .with_restitution(0.9)
      .with_force_events(50.0);
    assert_eq!(c.offset, Vec2::new(1.0, 2.0));
    assert_eq!(c.rotation, 0.5);
    assert!(c.is_trigger);
    assert_eq!((c.layer, c.mask), (2, 6));
    assert_eq!((c.density, c.mass), (3.0, Some(7.0)));
    assert_eq!((c.friction, c.restitution), (0.1, 0.9));
    assert_eq!(c.force_threshold, Some(50.0));
  }

  #[test]
  fn shapes() {
    assert_eq!(
      Collider::capsule(40.0, 10.0).shape,
      ColliderShape::Capsule {
        height: 40.0,
        radius: 10.0
      }
    );
    let tri = [Vec2::ZERO, Vec2::X, Vec2::Y];
    assert_eq!(
      Collider::polygon(tri).shape,
      ColliderShape::Polygon {
        points: tri.to_vec()
      }
    );
    assert_eq!(
      Collider::polyline(tri).shape,
      ColliderShape::Polyline {
        points: tri.to_vec()
      }
    );
    assert_eq!(
      Collider::segment(Vec2::ZERO, Vec2::X).shape,
      ColliderShape::Segment {
        a: Vec2::ZERO,
        b: Vec2::X
      }
    );
  }

  #[test]
  fn world_bounds_follow_offset_rotation_and_scale() {
    let at = |t: Transform| GlobalTransform::from_transform(&t);
    let rect = Collider::rect(20.0, 10.0);
    assert!(close(
      rect.world_bounds(&at(Transform::from_xy(100.0, 50.0))),
      (Vec2::new(90.0, 45.0), Vec2::new(110.0, 55.0))
    ));
    assert!(close(
      rect.world_bounds(&at(
        Transform::from_xy(100.0, 50.0).with_rotation(FRAC_PI_2)
      )),
      (Vec2::new(95.0, 40.0), Vec2::new(105.0, 60.0))
    ));
    assert!(close(
      rect.world_bounds(&at(Transform::IDENTITY.with_scale(Vec2::splat(2.0)))),
      (Vec2::new(-20.0, -10.0), Vec2::new(20.0, 10.0))
    ));
    let circle = Collider::circle(8.0).with_offset(0.0, -10.0);
    assert!(close(
      circle.world_bounds(&at(Transform::IDENTITY)),
      (Vec2::new(-8.0, -18.0), Vec2::new(8.0, -2.0))
    ));
    let capsule = Collider::capsule(40.0, 10.0);
    assert!(close(
      capsule.world_bounds(&at(Transform::IDENTITY)),
      (Vec2::new(-10.0, -20.0), Vec2::new(10.0, 20.0))
    ));
  }
}
