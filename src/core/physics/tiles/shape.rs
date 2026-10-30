//! Authored cell shapes. Each polygon is cleaned (finite points, no repeats
//! or straight-through points, wound as cells are: clockwise on screen),
//! then a solid shape's are split into convex parts, as Rapier's polygons
//! are convex: ear clipping into triangles, merged back across their
//! diagonals while the merge stays convex (Hertel and Mehlhorn), so a
//! polygon's straight edges are never cut. A one-way shape keeps its
//! polygons' top edges.

use glam::DVec2;

use super::CellShape;
use crate::core::ecs::Vec2;

/// Points closer than this (tile units) are one point; a corner turning
/// less than this (the sine of its turn) is straight.
const EPSILON: f64 = 1e-6;

/// A top edge faces within 45° of up (inclusive): its normal's y is at most
/// -cos 45°.
const TOP: f64 = -std::f64::consts::FRAC_1_SQRT_2 + EPSILON;

/// The most corners a concave polygon is split with (splitting takes time
/// cubic in them); a larger one collides as its hull.
const MOST_SPLIT: usize = 256;

/// What a shape builds, in tile units, worked out once as it is added.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ShapeBuild {
  /// Convex parts, each a solid polygon collider (a solid shape)
  pub(crate) parts: Vec<Vec<Vec2>>,
  /// Top edges and the way each faces, each a one-way segment (a one-way shape)
  pub(crate) tops: Vec<(Vec2, Vec2, Vec2)>,
  /// A solid shape covering the cell's centre: navigation blocks the cell
  pub(crate) blocks: bool,
}

impl ShapeBuild {
  pub(crate) fn of(shape: &CellShape) -> Self {
    let mut build = Self::default();
    let (mut tangled, mut large) = (false, false);
    for polygon in &shape.polygons {
      let Some(ring) = clean(polygon) else {
        continue;
      };
      let n = ring.len();
      if shape.one_way {
        for i in 0..n {
          let (a, b) = (ring[i], ring[(i + 1) % n]);
          let along = b - a;
          // Outward, as the ring is clockwise on screen
          let facing = DVec2::new(along.y, -along.x) / along.length();
          if facing.y <= TOP {
            build
              .tops
              .push((a.as_vec2(), b.as_vec2(), facing.as_vec2()));
          }
        }
        continue;
      }
      build.blocks |= covers(&ring, DVec2::splat(0.5));
      let parts = if convex(&ring, &(0..n).collect::<Vec<_>>()) {
        vec![(0..n).collect()]
      } else if n > MOST_SPLIT {
        large = true;
        vec![(0..n).collect()]
      } else if let Some(triangles) = triangulate(&ring) {
        merge_convex(&ring, triangles)
      } else {
        // Not simple: it collides as its hull (polygons are hulls)
        tangled = true;
        vec![(0..n).collect()]
      };
      build.parts.extend(
        parts
          .into_iter()
          .map(|part| part.into_iter().map(|i| ring[i].as_vec2()).collect()),
      );
    }
    if tangled {
      crate::engine_warn!(
        module: "Physics",
        "a CellShape polygon crosses itself; it collides as its convex hull"
      );
    }
    if large {
      crate::engine_warn!(
        module: "Physics",
        "a concave CellShape polygon has over {MOST_SPLIT} corners; it collides as its convex hull"
      );
    }
    build
  }
}

/// Twice the signed area: positive when clockwise on screen (y down).
fn doubled_area(ring: &[DVec2]) -> f64 {
  let n = ring.len();
  (0..n).map(|i| ring[i].perp_dot(ring[(i + 1) % n])).sum()
}

/// Whether the corner from `a` through `b` to `c` turns clockwise on screen
/// (convex in a ring wound that way), by more than `EPSILON` of a turn when
/// `strict`, or does not turn the other way by more than that.
fn turns(a: DVec2, b: DVec2, c: DVec2, strict: bool) -> bool {
  let (u, v) = (b - a, c - b);
  let sine = u.perp_dot(v);
  let scale = EPSILON * u.length() * v.length();
  if strict { sine > scale } else { sine >= -scale }
}

/// `points` cleaned, wound clockwise on screen; None when a point is not
/// finite or fewer than three corners or no area are left.
fn clean(points: &[Vec2]) -> Option<Vec<DVec2>> {
  if points.iter().any(|p| !p.is_finite()) {
    return None;
  }
  let mut ring: Vec<DVec2> = Vec::with_capacity(points.len());
  for p in points.iter().map(|p| p.as_dvec2()) {
    if ring
      .last()
      .is_none_or(|last| last.distance_squared(p) > EPSILON * EPSILON)
    {
      ring.push(p);
    }
  }
  while ring.len() > 1 && ring[0].distance_squared(ring[ring.len() - 1]) <= EPSILON * EPSILON {
    ring.pop();
  }
  // Straight-through points (and spikes back on themselves) go, until a
  // whole round finds none
  let (mut i, mut unchanged) = (0, 0);
  while ring.len() >= 3 && unchanged < ring.len() {
    let n = ring.len();
    let (a, b, c) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
    let (u, v) = (b - a, c - b);
    if u.perp_dot(v).abs() <= EPSILON * u.length() * v.length() {
      ring.remove(i);
      unchanged = 0;
      i = (i + ring.len() - 1) % ring.len().max(1);
    } else {
      i = (i + 1) % n;
      unchanged += 1;
    }
  }
  if ring.len() < 3 {
    return None;
  }
  let area = doubled_area(&ring);
  if area.abs() <= 2.0 * EPSILON {
    return None;
  }
  if area < 0.0 {
    ring.reverse();
  }
  Some(ring)
}

/// Whether the polygon of `ring`'s points at `part` is convex.
fn convex(ring: &[DVec2], part: &[usize]) -> bool {
  let n = part.len();
  (0..n).all(|i| {
    turns(
      ring[part[i]],
      ring[part[(i + 1) % n]],
      ring[part[(i + 2) % n]],
      false,
    )
  })
}

/// Whether `p` is inside the triangle `a`, `b`, `c` (clockwise on screen)
/// or on its edges, but not at one of its corners.
fn in_triangle(p: DVec2, a: DVec2, b: DVec2, c: DVec2) -> bool {
  if [a, b, c]
    .iter()
    .any(|corner| corner.distance_squared(p) <= EPSILON * EPSILON)
  {
    return false;
  }
  [(a, b), (b, c), (c, a)]
    .iter()
    .all(|&(from, to)| (to - from).perp_dot(p - from) >= -EPSILON)
}

/// Triangles covering `ring` (simple, clockwise on screen), as indices,
/// cut off one ear at a time; None when no ear is left (it crosses itself).
fn triangulate(ring: &[DVec2]) -> Option<Vec<Vec<usize>>> {
  let mut left: Vec<usize> = (0..ring.len()).collect();
  let mut triangles = Vec::with_capacity(ring.len().saturating_sub(2));
  while left.len() > 3 {
    let n = left.len();
    let corner = |k: usize| (left[(k + n - 1) % n], left[k], left[(k + 1) % n]);
    let ear = (0..n).find(|&k| {
      let (a, b, c) = corner(k);
      turns(ring[a], ring[b], ring[c], true)
        && !left
          .iter()
          .any(|&j| j != a && j != b && j != c && in_triangle(ring[j], ring[a], ring[b], ring[c]))
    })?;
    let (a, b, c) = corner(ear);
    triangles.push(vec![a, b, c]);
    left.remove(ear);
  }
  triangles.push(left);
  Some(triangles)
}

/// Parts merged across the edges they share while the merge stays convex.
fn merge_convex(ring: &[DVec2], mut parts: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
  'merging: loop {
    for a in 0..parts.len() {
      for b in a + 1..parts.len() {
        if let Some(joined) = join(ring, &parts[a], &parts[b]) {
          parts[a] = joined;
          parts.swap_remove(b);
          continue 'merging;
        }
      }
    }
    return parts;
  }
}

/// `a` and `b` joined across an edge they share (one runs it each way),
/// when that makes a convex part.
fn join(ring: &[DVec2], a: &[usize], b: &[usize]) -> Option<Vec<usize>> {
  let (na, nb) = (a.len(), b.len());
  for i in 0..na {
    let (u, v) = (a[i], a[(i + 1) % na]);
    let Some(j) = (0..nb).find(|&j| b[j] == v && b[(j + 1) % nb] == u) else {
      continue;
    };
    // Round `a` from `v` to `u`, then round `b` from after `u` to before `v`
    let mut joined: Vec<usize> = (1..=na).map(|k| a[(i + k) % na]).collect();
    joined.extend((2..nb).map(|k| b[(j + k) % nb]));
    return convex(ring, &joined).then_some(joined);
  }
  None
}

/// Whether `p` is inside `ring` or on its edges.
fn covers(ring: &[DVec2], p: DVec2) -> bool {
  let n = ring.len();
  let mut inside = false;
  for i in 0..n {
    let (a, b) = (ring[i], ring[(i + 1) % n]);
    let along = b - a;
    let t = ((p - a).dot(along) / along.length_squared()).clamp(0.0, 1.0);
    if (a + along * t).distance_squared(p) <= EPSILON * EPSILON {
      return true;
    }
    if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * along.x / along.y {
      inside = !inside;
    }
  }
  inside
}

#[cfg(test)]
mod tests {
  use super::*;

  fn shape(points: &[(f32, f32)], one_way: bool) -> CellShape {
    CellShape {
      polygons: vec![points.iter().map(|&(x, y)| Vec2::new(x, y)).collect()],
      one_way,
    }
  }

  fn signed_area(points: &[Vec2]) -> f32 {
    let n = points.len();
    (0..n)
      .map(|i| points[i].perp_dot(points[(i + 1) % n]))
      .sum::<f32>()
      / 2.0
  }

  fn convex(points: &[Vec2]) -> bool {
    let n = points.len();
    let sign = signed_area(points).signum();
    n >= 3
      && (0..n).all(|i| {
        let (a, b, c) = (points[i], points[(i + 1) % n], points[(i + 2) % n]);
        (b - a).perp_dot(c - b) * sign >= -1e-6
      })
  }

  #[test]
  fn concave_polygons_split_into_convex_parts_of_the_same_area() {
    let l = [
      (0.0, 0.0),
      (0.5, 0.0),
      (0.5, 0.5),
      (1.0, 0.5),
      (1.0, 1.0),
      (0.0, 1.0),
    ];
    let u = [
      (0.0, 0.0),
      (0.25, 0.0),
      (0.25, 0.75),
      (0.75, 0.75),
      (0.75, 0.0),
      (1.0, 0.0),
      (1.0, 1.0),
      (0.0, 1.0),
    ];
    let comb = [
      (0.0, 1.0),
      (0.0, 0.0),
      (0.2, 0.0),
      (0.2, 0.6),
      (0.4, 0.6),
      (0.4, 0.0),
      (0.6, 0.0),
      (0.6, 0.6),
      (0.8, 0.6),
      (0.8, 0.0),
      (1.0, 0.0),
      (1.0, 1.0),
    ];
    for (name, points, most) in [("L", &l[..], 2), ("U", &u[..], 3), ("comb", &comb[..], 5)] {
      for reversed in [false, true] {
        let mut points = points.to_vec();
        if reversed {
          points.reverse();
        }
        let built = ShapeBuild::of(&shape(&points, false));
        let whole = signed_area(&shape(&points, false).polygons[0]).abs();
        let total: f32 = built.parts.iter().map(|p| signed_area(p).abs()).sum();
        assert!(
          (total - whole).abs() < 1e-5,
          "{name} (reversed {reversed}): parts cover {total} of {whole}"
        );
        assert!(
          built.parts.iter().all(|p| convex(p)),
          "{name}: {:?}",
          built.parts
        );
        assert!(
          built.parts.len() <= most,
          "{name}: {} parts",
          built.parts.len()
        );
        assert!(built.tops.is_empty());
      }
    }
    // A convex polygon stays whole, its repeated and straight-through points dropped
    let square = [
      (0.0, 0.0),
      (0.5, 0.0),
      (1.0, 0.0),
      (1.0, 1.0),
      (1.0, 1.0),
      (0.0, 1.0),
      (0.0, 0.0),
    ];
    let built = ShapeBuild::of(&shape(&square, false));
    assert_eq!(built.parts.len(), 1);
    assert_eq!(built.parts[0].len(), 4, "{:?}", built.parts[0]);
  }

  #[test]
  fn a_concave_polygon_of_too_many_corners_collides_as_its_hull() {
    // A comb of 75 teeth: 302 corners
    let mut points = vec![(0.0, 1.0)];
    for tooth in 0..75 {
      let x = tooth as f32 / 75.0;
      let w = 1.0 / 150.0;
      points.extend([(x, 0.0), (x + w, 0.0), (x + w, 0.5), (x + 2.0 * w, 0.5)]);
    }
    points.push((1.0, 1.0));
    let built = ShapeBuild::of(&shape(&points, false));
    assert_eq!(built.parts.len(), 1);
    // A comb under the limit is split
    let built = ShapeBuild::of(&shape(&points[..50], false));
    assert!(built.parts.len() > 1);
  }

  #[test]
  fn degenerate_polygons_build_nothing() {
    let polygons: [&[(f32, f32)]; 4] = [
      &[],
      &[(0.0, 0.0), (1.0, 1.0)],
      &[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)],
      &[(0.0, 0.0), (f32::NAN, 0.0), (1.0, 1.0), (0.0, 1.0)],
    ];
    for points in polygons {
      for one_way in [false, true] {
        let built = ShapeBuild::of(&shape(points, one_way));
        assert_eq!(built, ShapeBuild::default(), "{points:?}");
      }
    }
  }

  #[test]
  fn one_way_shapes_keep_their_top_edges_facing_up() {
    // A flat top, a gentle slope down to the right, then a wall, a floor and a wall
    let points = [(0.0, 0.25), (0.5, 0.25), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)];
    for reversed in [false, true] {
      let mut points = points.to_vec();
      if reversed {
        points.reverse();
      }
      let built = ShapeBuild::of(&shape(&points, true));
      assert!(built.parts.is_empty() && !built.blocks);
      let mut tops: Vec<_> = built
        .tops
        .iter()
        .map(|&(a, b, up)| {
          let (a, b) = if a.x <= b.x { (a, b) } else { (b, a) };
          (a, b, up)
        })
        .collect();
      tops.sort_by(|p, q| p.0.x.total_cmp(&q.0.x));
      assert_eq!(tops.len(), 2, "{tops:?}");
      assert_eq!(
        (tops[0].0, tops[0].1),
        (Vec2::new(0.0, 0.25), Vec2::new(0.5, 0.25))
      );
      assert_eq!(tops[0].2, Vec2::NEG_Y);
      assert_eq!(
        (tops[1].0, tops[1].1),
        (Vec2::new(0.5, 0.25), Vec2::new(1.0, 0.5))
      );
      let slope = Vec2::new(0.25, -0.5).normalize();
      assert!(tops[1].2.distance(slope) < 1e-6, "{:?}", tops[1].2);
    }
  }

  #[test]
  fn a_solid_shape_blocks_when_it_covers_the_cell_centre() {
    let square = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let corner = [(0.0, 0.0), (0.3, 0.0), (0.3, 0.3), (0.0, 0.3)];
    // Its inner corner on the centre: on the edge counts
    let l = [
      (0.0, 0.0),
      (0.5, 0.0),
      (0.5, 0.5),
      (1.0, 0.5),
      (1.0, 1.0),
      (0.0, 1.0),
    ];
    assert!(ShapeBuild::of(&shape(&square, false)).blocks);
    assert!(!ShapeBuild::of(&shape(&corner, false)).blocks);
    assert!(ShapeBuild::of(&shape(&l, false)).blocks);
    assert!(
      !ShapeBuild::of(&shape(&square, true)).blocks,
      "one-way shapes stay walkable, as one-way cells do"
    );
  }
}
