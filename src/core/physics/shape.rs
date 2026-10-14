//! Rapier shapes from `ColliderShape`s, under the linear part of the
//! collider's world transform taken into the frame Rapier places it in
//! (scale, mirroring and, for turned shapes under non-uniform scale, skew).

use std::f32::consts::{PI, TAU};

use glam::Mat2;
use rapier2d::prelude::SharedShape;

use super::collider::ColliderShape;
#[cfg(test)]
use super::convert::from_rapier;
use super::convert::to_rapier;
use crate::core::ecs::Vec2;

/// Points on a full circle approximating a stretched round shape.
const ARC_POINTS: usize = 24;
/// Relative tolerance for "this map has no off-diagonal part" and "these
/// two scales are equal".
const MAP_TOLERANCE: f32 = 1e-4;

fn positive(v: f32) -> bool {
  v.is_finite() && v > 0.0
}

/// The axis scales when `map` is diagonal (mirroring allowed).
fn diagonal(map: Mat2) -> Option<Vec2> {
  let scale = Vec2::new(map.x_axis.x, map.y_axis.y);
  let tolerance = MAP_TOLERANCE * scale.abs().max_element().max(1.0);
  (map.x_axis.y.abs() < tolerance && map.y_axis.x.abs() < tolerance).then(|| scale.abs())
}

/// The scale when `map` is diagonal with equal axis scales.
fn uniform(map: Mat2) -> Option<f32> {
  let scale = diagonal(map)?;
  ((scale.x - scale.y).abs() < MAP_TOLERANCE * scale.x.max(1.0)).then_some(scale.x)
}

/// `count` points on an arc of `radius` around `centre`, from `from` to `to` radians.
fn arc(centre: Vec2, radius: f32, from: f32, to: f32, count: usize) -> impl Iterator<Item = Vec2> {
  (0..count).map(move |i| {
    let t = from + (to - from) * i as f32 / count as f32;
    centre + Vec2::from_angle(t) * radius
  })
}

/// The largest coordinate a hull point may have: parry's dot products and
/// squared lengths stay finite below it.
const HULL_MAX_COORD: f32 = 1e18;
/// The smallest width or height a hull may span: parry's squared lengths
/// underflow to zero for points closer than about 1e-22.
const HULL_MIN_EXTENT: f32 = 1e-6;

/// The convex hull of the mapped points. parry panics instead of refusing
/// on coincident points, clusters closer than its squared lengths can tell
/// apart and coordinates near `f32::MAX`, so those are refused here.
fn hull(points: impl IntoIterator<Item = Vec2>, map: Mat2) -> Option<SharedShape> {
  let mapped: Vec<Vec2> = points.into_iter().map(|p| map * p).collect();
  let first = *mapped.first()?;
  let mut min = first;
  let mut max = first;
  for p in &mapped {
    if !p.is_finite() || p.abs().max_element() > HULL_MAX_COORD {
      return None;
    }
    min = min.min(*p);
    max = max.max(*p);
  }
  if (max - min).max_element() <= HULL_MIN_EXTENT {
    return None;
  }
  let mapped: Vec<_> = mapped.into_iter().map(to_rapier).collect();
  SharedShape::convex_hull(&mapped)
}

/// The Rapier shape for `shape` with `map` applied to its points, or `None`
/// when the result is degenerate (a zero, negative or non-finite size, a
/// polygon without three corners, a zero-length segment, a polyline without
/// an edge, or a map that flattens everything).
pub(crate) fn build(shape: &ColliderShape, map: Mat2) -> Option<SharedShape> {
  if !map.is_finite() || map.determinant().abs() <= f32::EPSILON {
    return None;
  }
  match shape {
    ColliderShape::Rect { size } => {
      if !(positive(size.x) && positive(size.y)) {
        return None;
      }
      let h = *size / 2.0;
      match diagonal(map) {
        Some(scale) => Some(SharedShape::cuboid(h.x * scale.x, h.y * scale.y)),
        None => hull(
          [(-h.x, -h.y), (h.x, -h.y), (h.x, h.y), (-h.x, h.y)].map(|(x, y)| Vec2::new(x, y)),
          map,
        ),
      }
    }
    ColliderShape::Circle { radius } => {
      if !positive(*radius) {
        return None;
      }
      match uniform(map) {
        Some(scale) => Some(SharedShape::ball(radius * scale)),
        None => hull(arc(Vec2::ZERO, *radius, 0.0, TAU, ARC_POINTS), map),
      }
    }
    ColliderShape::Capsule { height, radius } => {
      if !(positive(*height) && positive(*radius)) {
        return None;
      }
      let half = (height / 2.0 - radius).max(0.0);
      match uniform(map) {
        Some(scale) if half == 0.0 => Some(SharedShape::ball(radius * scale)),
        Some(scale) => Some(SharedShape::capsule_y(half * scale, radius * scale)),
        None if half == 0.0 => hull(arc(Vec2::ZERO, *radius, 0.0, TAU, ARC_POINTS), map),
        None => hull(
          arc(Vec2::new(0.0, half), *radius, 0.0, PI, ARC_POINTS / 2)
            .chain(arc(Vec2::new(0.0, -half), *radius, PI, TAU, ARC_POINTS / 2))
            .chain([Vec2::new(-radius, half), Vec2::new(*radius, -half)]),
          map,
        ),
      }
    }
    ColliderShape::Polygon { points } => {
      if points.len() < 3 {
        return None;
      }
      hull(points.iter().copied(), map)
    }
    ColliderShape::Segment { a, b } => {
      let (a, b) = (map * *a, map * *b);
      (a.is_finite() && b.is_finite() && a.distance(b) > f32::EPSILON)
        .then(|| SharedShape::segment(to_rapier(a), to_rapier(b)))
    }
    ColliderShape::Polyline { points } => {
      let mut mapped: Vec<Vec2> = Vec::with_capacity(points.len());
      for p in points {
        let p = map * *p;
        if !p.is_finite() {
          return None;
        }
        if mapped
          .last()
          .is_none_or(|last| last.distance(p) > f32::EPSILON)
        {
          mapped.push(p);
        }
      }
      (mapped.len() >= 2)
        .then(|| SharedShape::polyline(mapped.into_iter().map(to_rapier).collect(), None))
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use rapier2d::prelude::{Pose, ShapeType};

  fn local_aabb(shape: &SharedShape) -> (Vec2, Vec2) {
    let aabb = shape.compute_aabb(&Pose::IDENTITY);
    (from_rapier(aabb.mins), from_rapier(aabb.maxs))
  }

  #[test]
  fn every_shape_builds_under_the_identity() {
    let tri = vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)];
    for shape in [
      ColliderShape::Rect {
        size: Vec2::new(10.0, 4.0),
      },
      ColliderShape::Circle { radius: 3.0 },
      ColliderShape::Capsule {
        height: 20.0,
        radius: 4.0,
      },
      ColliderShape::Polygon {
        points: tri.clone(),
      },
      ColliderShape::Segment {
        a: Vec2::ZERO,
        b: Vec2::X,
      },
      ColliderShape::Polyline { points: tri },
    ] {
      assert!(build(&shape, Mat2::IDENTITY).is_some(), "{shape:?}");
    }
  }

  #[test]
  fn degenerate_shapes_are_rejected() {
    let line = vec![Vec2::ZERO, Vec2::X, Vec2::new(2.0, 0.0)];
    for shape in [
      ColliderShape::Rect {
        size: Vec2::new(0.0, 4.0),
      },
      ColliderShape::Rect {
        size: Vec2::new(f32::NAN, 4.0),
      },
      ColliderShape::Circle { radius: -1.0 },
      ColliderShape::Capsule {
        height: 0.0,
        radius: 4.0,
      },
      ColliderShape::Polygon {
        points: vec![Vec2::ZERO, Vec2::X],
      },
      ColliderShape::Polygon { points: line },
      // Inputs parry's hull panics on instead of refusing
      ColliderShape::Polygon {
        points: vec![Vec2::splat(5.0); 3],
      },
      ColliderShape::Polygon {
        points: vec![Vec2::ZERO, Vec2::new(1e-24, 0.0), Vec2::new(0.0, 1e-24)],
      },
      ColliderShape::Polygon {
        points: vec![
          Vec2::new(-f32::MAX, 0.0),
          Vec2::new(-f32::MAX, 1.0),
          Vec2::new(-f32::MAX, 2.0),
        ],
      },
      ColliderShape::Segment {
        a: Vec2::X,
        b: Vec2::X,
      },
      ColliderShape::Polyline {
        points: vec![Vec2::X],
      },
      ColliderShape::Polyline {
        points: vec![Vec2::X, Vec2::X],
      },
    ] {
      assert!(build(&shape, Mat2::IDENTITY).is_none(), "{shape:?}");
    }
    let rect = ColliderShape::Rect { size: Vec2::ONE };
    assert!(build(&rect, Mat2::ZERO).is_none(), "zero scale");
  }

  #[test]
  fn a_capsule_shorter_than_its_width_is_a_circle() {
    let shape = build(
      &ColliderShape::Capsule {
        height: 6.0,
        radius: 4.0,
      },
      Mat2::IDENTITY,
    )
    .unwrap();
    assert_eq!(shape.shape_type(), ShapeType::Ball);
  }

  #[test]
  fn a_diagonal_map_scales_a_rect_into_a_cuboid() {
    let map = Mat2::from_diagonal(Vec2::new(2.0, -3.0));
    let shape = build(
      &ColliderShape::Rect {
        size: Vec2::new(10.0, 4.0),
      },
      map,
    )
    .unwrap();
    assert_eq!(shape.shape_type(), ShapeType::Cuboid);
    assert_eq!(
      local_aabb(&shape),
      (Vec2::new(-10.0, -6.0), Vec2::new(10.0, 6.0))
    );
  }

  #[test]
  fn a_skewing_map_turns_a_rect_into_a_polygon() {
    let map = Mat2::from_cols(Vec2::new(1.0, 0.0), Vec2::new(0.5, 1.0));
    let shape = build(
      &ColliderShape::Rect {
        size: Vec2::new(10.0, 10.0),
      },
      map,
    )
    .unwrap();
    assert_eq!(shape.shape_type(), ShapeType::ConvexPolygon);
    let (min, max) = local_aabb(&shape);
    assert!(
      (max.x - 7.5).abs() < 1e-4 && (min.x + 7.5).abs() < 1e-4,
      "{min} {max}"
    );
  }

  #[test]
  fn circles_scale_uniformly_and_stretch_into_polygons() {
    let circle = ColliderShape::Circle { radius: 5.0 };
    let uniform = build(&circle, Mat2::from_diagonal(Vec2::splat(2.0))).unwrap();
    assert_eq!(uniform.shape_type(), ShapeType::Ball);
    assert_eq!(
      local_aabb(&uniform),
      (Vec2::splat(-10.0), Vec2::splat(10.0))
    );
    let stretched = build(&circle, Mat2::from_diagonal(Vec2::new(2.0, 1.0))).unwrap();
    assert_eq!(stretched.shape_type(), ShapeType::ConvexPolygon);
    let (min, max) = local_aabb(&stretched);
    assert!(
      (max.x - 10.0).abs() < 0.01 && (max.y - 5.0).abs() < 0.01,
      "{min} {max}"
    );
  }

  #[test]
  fn a_mirrored_polygon_is_still_valid() {
    let tri = ColliderShape::Polygon {
      points: vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)],
    };
    let shape = build(&tri, Mat2::from_diagonal(Vec2::new(-1.0, 1.0))).unwrap();
    assert_eq!(
      local_aabb(&shape),
      (Vec2::new(-10.0, 0.0), Vec2::new(0.0, 10.0))
    );
  }
}
