//! Authored cell shapes. Each polygon is clipped to its cell, snapped to
//! the outline grid (`GRID` points a tile), cleaned (no repeated or
//! straight-through points; nothing left with no area) and wound as cells
//! are, clockwise on screen. A polygon that crosses itself, and a shape
//! whose polygons overlap, collide as their hull. The shape's edges, those
//! two of its polygons share cancelled, join the grid's outlines (a solid
//! shape), or its top edges join its one-way runs (a one-way shape). For
//! `solid_rects` mode a solid shape is also split into convex parts, as
//! Rapier's polygons are convex: ear clipping into triangles, merged back
//! across their diagonals while the merge stays convex (Hertel and
//! Mehlhorn), so no polygon edge is cut.

use glam::DVec2;

use super::{CellShape, GRID, Point, sides};
use crate::core::ecs::Vec2;

/// A corner of a convex part turning less than this (the sine of its turn)
/// is straight; a point this close to a triangle's edge is on it.
const EPSILON: f64 = 1e-9;

/// The most corners a concave polygon is split with (splitting takes time
/// cubic in them); a larger one collides as its hull in `solid_rects` mode.
const MOST_SPLIT: usize = 256;

/// What a shape builds, worked out once as it is added.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ShapeBuild {
  /// Its outline edges in its cell, `GRID` points a side, wound clockwise
  /// on screen (a solid shape)
  pub(crate) edges: Vec<(Point, Point)>,
  /// Its top edges (facing within 45° of up), left to right (a one-way shape)
  pub(crate) tops: Vec<(Point, Point)>,
  /// Convex parts in tile units, for `solid_rects` mode (a solid shape)
  pub(crate) parts: Vec<Vec<Vec2>>,
  /// A solid shape covering the cell's centre: navigation blocks the cell
  pub(crate) blocks: bool,
}

impl ShapeBuild {
  pub(crate) fn of(shape: &CellShape) -> Self {
    let mut tangled = false;
    let mut rings: Vec<Vec<Point>> = shape
      .polygons
      .iter()
      .filter_map(|polygon| {
        let mut ring = snapped(polygon)?;
        if !simple(&ring) {
          tangled = true;
          return hull(&ring);
        }
        if doubled_area(&ring) < 0 {
          ring.reverse();
        }
        Some(ring)
      })
      .collect();
    if overlap(&rings) {
      tangled = true;
      rings = hull(&rings.concat()).into_iter().collect();
    }
    let mut edges = cancelled(&rings);
    // Belt and braces: what reaches the outlines must be a region's
    // boundary inside the cell, or they could meet an edge twice
    if !well_formed(&edges) {
      tangled = true;
      rings = hull(&rings.concat()).into_iter().collect();
      edges = cancelled(&rings);
    }
    if tangled {
      crate::engine_warn!(
        module: "Physics",
        "a CellShape polygon touches or crosses itself or another of its polygons; it collides as their convex hull"
      );
    }
    let mut build = Self::default();
    if shape.one_way {
      // A top edge runs left to right (the ring is clockwise on screen) no
      // steeper than 45°
      build.tops = edges
        .into_iter()
        .filter(|&(a, b)| b.0 - a.0 > 0 && b.0 - a.0 >= (b.1 - a.1).abs())
        .collect();
      build.tops.sort_unstable();
      return build;
    }
    build.edges = edges;
    let centre = (GRID / 2, GRID / 2);
    build.blocks = rings
      .iter()
      .any(|ring| inside(ring, centre) || on_boundary(ring, centre));
    let mut large = false;
    for ring in &rings {
      let ring: Vec<DVec2> = ring
        .iter()
        .map(|&(x, y)| DVec2::new(f64::from(x), f64::from(y)) / f64::from(GRID))
        .collect();
      let n = ring.len();
      let parts = if convex(&ring, &(0..n).collect::<Vec<_>>()) {
        vec![(0..n).collect()]
      } else if n > MOST_SPLIT {
        large = true;
        vec![(0..n).collect()]
      } else if let Some(triangles) = triangulate(&ring) {
        merge_convex(&ring, triangles)
      } else {
        // Not simple after all (it touches itself): its hull
        vec![(0..n).collect()]
      };
      build.parts.extend(
        parts
          .into_iter()
          .map(|part| part.into_iter().map(|i| ring[i].as_vec2()).collect()),
      );
    }
    if large {
      crate::engine_warn!(
        module: "Physics",
        "a concave CellShape polygon has over {MOST_SPLIT} corners; in solid_rects mode it collides as its convex hull"
      );
    }
    build
  }
}

fn sub(a: Point, b: Point) -> (i64, i64) {
  (i64::from(a.0 - b.0), i64::from(a.1 - b.1))
}

fn cross(u: (i64, i64), v: (i64, i64)) -> i64 {
  u.0 * v.1 - u.1 * v.0
}

fn dot(u: (i64, i64), v: (i64, i64)) -> i64 {
  u.0 * v.0 + u.1 * v.1
}

/// Which side of the line from `a` to `b` `p` is on: positive clockwise on
/// screen (the inside of a clockwise ring), zero on the line.
fn side(a: Point, b: Point, p: Point) -> i64 {
  cross(sub(b, a), sub(p, a))
}

/// Twice the signed area: positive when clockwise on screen (y down).
fn doubled_area(ring: &[Point]) -> i64 {
  let n = ring.len();
  (0..n)
    .map(|i| {
      let (a, b) = (ring[i], ring[(i + 1) % n]);
      i64::from(a.0) * i64::from(b.1) - i64::from(b.0) * i64::from(a.1)
    })
    .sum()
}

/// `polygon` clipped to the cell, snapped to the grid and cleaned (either
/// winding, maybe not simple); None when a point is not finite or fewer
/// than three corners are left.
fn snapped(polygon: &[Vec2]) -> Option<Vec<Point>> {
  if polygon.iter().any(|p| !p.is_finite()) {
    return None;
  }
  let mut points: Vec<DVec2> = polygon.iter().map(|p| p.as_dvec2()).collect();
  // Clipped to each side of the cell in turn (Sutherland and Hodgman): kept
  // where `facing · p >= at_least`
  let sides = [
    (DVec2::X, 0.0),
    (DVec2::NEG_X, -1.0),
    (DVec2::Y, 0.0),
    (DVec2::NEG_Y, -1.0),
  ];
  for (facing, at_least) in sides {
    let n = points.len();
    let mut kept = Vec::with_capacity(n + 2);
    for i in 0..n {
      let (a, b) = (points[i], points[(i + 1) % n]);
      let (da, db) = (facing.dot(a) - at_least, facing.dot(b) - at_least);
      if da >= 0.0 {
        kept.push(a);
      }
      if (da >= 0.0) != (db >= 0.0) {
        kept.push(a + (b - a) * (da / (da - db)));
      }
    }
    points = kept;
  }
  let grid = f64::from(GRID);
  let mut ring: Vec<Point> = points
    .iter()
    .map(|p| {
      let at = |v: f64| ((v * grid).round() as i32).clamp(0, GRID);
      (at(p.x), at(p.y))
    })
    .collect();
  clean(&mut ring);
  (ring.len() >= 3).then_some(ring)
}

/// Drop repeated points and points that go straight on or turn straight
/// back, until a whole round finds none.
fn clean(ring: &mut Vec<Point>) {
  ring.dedup();
  while ring.len() > 1 && ring.first() == ring.last() {
    ring.pop();
  }
  let (mut i, mut unchanged) = (0, 0);
  while ring.len() >= 3 && unchanged < ring.len() {
    let n = ring.len();
    let (a, b, c) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
    if a == b || cross(sub(b, a), sub(c, b)) == 0 {
      ring.remove(i);
      unchanged = 0;
      i = (i + ring.len() - 1) % ring.len();
    } else {
      i = (i + 1) % n;
      unchanged += 1;
    }
  }
}

/// Whether segments `a`-`b` and `c`-`d` cross at a point inside both.
fn cross_properly(a: Point, b: Point, c: Point, d: Point) -> bool {
  let (s1, s2) = (side(a, b, c).signum(), side(a, b, d).signum());
  let (s3, s4) = (side(c, d, a).signum(), side(c, d, b).signum());
  s1 * s2 < 0 && s3 * s4 < 0
}

/// Whether two edges run along the same line the same way and share more
/// than a point.
fn overlap_same_way(e: (Point, Point), f: (Point, Point)) -> bool {
  let (u, v) = (sub(e.1, e.0), sub(f.1, f.0));
  if cross(u, v) != 0 || side(e.0, e.1, f.0) != 0 || dot(u, v) <= 0 {
    return false;
  }
  // Their extents along `u` overlap
  let along = |p: Point| dot(u, sub(p, e.0));
  let (lo, hi) = (along(f.0).min(along(f.1)), along(f.0).max(along(f.1)));
  lo < dot(u, u) && hi > 0
}

fn ring_edges(ring: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
  (0..ring.len()).map(move |i| (ring[i], ring[(i + 1) % ring.len()]))
}

/// Whether two closed segments share any point.
fn meet(e: (Point, Point), f: (Point, Point)) -> bool {
  cross_properly(e.0, e.1, f.0, f.1)
    || on_segment(e.0, e.1, f.0)
    || on_segment(e.0, e.1, f.1)
    || on_segment(f.0, f.1, e.0)
    || on_segment(f.0, f.1, e.1)
}

/// Whether a ring is simple: it has area, and no two of its edges meet
/// but neighbours, at their one shared corner. A ring touching itself at a
/// corner, with a corner on another of its edges, running back along
/// itself or through a corner twice is not.
fn simple(ring: &[Point]) -> bool {
  let n = ring.len();
  if n < 3 || doubled_area(ring) == 0 {
    return false;
  }
  let edges: Vec<_> = ring_edges(ring).collect();
  (0..n).all(|i| {
    (i + 1..n).all(|j| {
      let (e, f) = (edges[i], edges[j]);
      if j == i + 1 {
        // e ends where f starts: neither far end on the other edge
        !on_segment(f.0, f.1, e.0) && !on_segment(e.0, e.1, f.1)
      } else if i == 0 && j == n - 1 {
        // f ends where e starts
        !on_segment(f.0, f.1, e.1) && !on_segment(e.0, e.1, f.0)
      } else {
        !meet(e, f)
      }
    })
  })
}

/// Whether edges from `cancelled` bound a region inside the cell: none
/// repeated, and those along a side of the cell run its way round (inside
/// on their right), as `sides::along` reads them.
fn well_formed(edges: &[(Point, Point)]) -> bool {
  let mut sorted = edges.to_vec();
  sorted.sort_unstable();
  let along_a_side = |(a, b): (Point, Point)| {
    let on = [
      a.1 == 0 && b.1 == 0,
      a.0 == GRID && b.0 == GRID,
      a.1 == GRID && b.1 == GRID,
      a.0 == 0 && b.0 == 0,
    ];
    (sides::TOP..=sides::LEFT).all(|side| !on[side] || sides::along((a, b), side).is_some())
  };
  sorted.windows(2).all(|w| w[0] != w[1]) && edges.iter().all(|&edge| along_a_side(edge))
}

/// Whether `p` is strictly inside `ring` (on its boundary is not).
fn inside(ring: &[Point], p: Point) -> bool {
  if on_boundary(ring, p) {
    return false;
  }
  let mut inside = false;
  for (a, b) in ring_edges(ring) {
    if (a.1 > p.1) != (b.1 > p.1) {
      // p is left of the edge where it crosses p's row
      let (num, den) = (
        i64::from(p.0 - a.0) * i64::from(b.1 - a.1),
        i64::from(p.1 - a.1) * i64::from(b.0 - a.0),
      );
      if (b.1 > a.1 && num < den) || (b.1 < a.1 && num > den) {
        inside = !inside;
      }
    }
  }
  inside
}

fn on_segment(a: Point, b: Point, p: Point) -> bool {
  side(a, b, p) == 0
    && p.0 >= a.0.min(b.0)
    && p.0 <= a.0.max(b.0)
    && p.1 >= a.1.min(b.1)
    && p.1 <= a.1.max(b.1)
}

fn on_boundary(ring: &[Point], p: Point) -> bool {
  ring_edges(ring).any(|(a, b)| on_segment(a, b, p))
}

/// Whether any two rings (each simple and clockwise) overlap: their edges
/// cross or run along each other the same way, or a piece of one's edge
/// (split at the other's corners on it) lies strictly inside the other.
/// Rings that only touch do not.
fn overlap(rings: &[Vec<Point>]) -> bool {
  let doubled =
    |ring: &[Point]| -> Vec<Point> { ring.iter().map(|&(x, y)| (2 * x, 2 * y)).collect() };
  (0..rings.len()).any(|i| {
    (0..rings.len()).any(|j| {
      if i == j {
        return false;
      }
      let (a, b) = (&rings[i], &rings[j]);
      let edges_meet = ring_edges(a).any(|e| {
        ring_edges(b).any(|f| cross_properly(e.0, e.1, f.0, f.1) || overlap_same_way(e, f))
      });
      let b2 = doubled(b);
      let within = ring_edges(a).any(|(p, q)| {
        let mut cuts: Vec<Point> = b
          .iter()
          .copied()
          .filter(|&c| on_segment(p, q, c))
          .chain([p, q])
          .collect();
        cuts.sort_by_key(|&c| dot(sub(q, p), sub(c, p)));
        cuts
          .windows(2)
          .any(|w| inside(&b2, (w[0].0 + w[1].0, w[0].1 + w[1].1)))
      });
      edges_meet || within
    })
  })
}

/// The convex hull, clockwise on screen (Andrew's monotone chain); None
/// when the points have no area.
fn hull(points: &[Point]) -> Option<Vec<Point>> {
  let mut sorted = points.to_vec();
  sorted.sort_unstable();
  sorted.dedup();
  if sorted.len() < 3 {
    return None;
  }
  let mut chain: Vec<Point> = Vec::with_capacity(2 * sorted.len());
  for pass in 0..2 {
    let start = chain.len();
    let order: Box<dyn Iterator<Item = &Point>> = if pass == 0 {
      Box::new(sorted.iter())
    } else {
      Box::new(sorted.iter().rev())
    };
    for &p in order {
      while chain.len() >= start + 2 && side(chain[chain.len() - 2], chain[chain.len() - 1], p) <= 0
      {
        chain.pop();
      }
      chain.push(p);
    }
    chain.pop();
  }
  let mut ring = chain;
  clean(&mut ring);
  let area = doubled_area(&ring);
  if ring.len() < 3 || area == 0 {
    return None;
  }
  if area < 0 {
    ring.reverse();
  }
  Some(ring)
}

/// Every ring's edges, split at each corner lying inside another edge, then
/// with each edge run both ways (where two rings touch) cancelled.
fn cancelled(rings: &[Vec<Point>]) -> Vec<(Point, Point)> {
  let mut edges: Vec<(Point, Point)> = rings.iter().flat_map(|ring| ring_edges(ring)).collect();
  let corners: Vec<Point> = rings.concat();
  let mut i = 0;
  while i < edges.len() {
    let (a, b) = edges[i];
    match corners
      .iter()
      .copied()
      .find(|&p| p != a && p != b && on_segment(a, b, p))
    {
      Some(p) => {
        edges[i] = (a, p);
        edges.push((p, b));
      }
      None => i += 1,
    }
  }
  edges.sort_unstable();
  let mut kept: Vec<(Point, Point)> = Vec::with_capacity(edges.len());
  let mut gone = vec![false; edges.len()];
  for i in 0..edges.len() {
    if gone[i] {
      continue;
    }
    let (a, b) = edges[i];
    let reverse = edges.binary_search(&(b, a)).ok().and_then(|j| {
      // The first of equal reverses not already gone
      let first = edges[..=j].partition_point(|&e| e < (b, a));
      (first..edges.len())
        .take_while(|&k| edges[k] == (b, a))
        .find(|&k| !gone[k])
    });
    match reverse {
      Some(j) => {
        gone[i] = true;
        gone[j] = true;
      }
      None => kept.push((a, b)),
    }
  }
  kept
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
  if [a, b, c].contains(&p) {
    return false;
  }
  [(a, b), (b, c), (c, a)]
    .iter()
    .all(|&(from, to)| (to - from).perp_dot(p - from) >= -EPSILON)
}

/// Triangles covering `ring` (simple, clockwise on screen), as indices,
/// cut off one ear at a time; None when no ear is left.
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
    // A comb of `teeth` teeth over a base
    let comb = |teeth: usize| {
      let mut points = vec![(0.0, 1.0)];
      for tooth in 0..teeth {
        let (x, w) = (tooth as f32 / teeth as f32, 0.5 / teeth as f32);
        points.extend([(x, 0.0), (x + w, 0.0), (x + w, 0.5), (x + 2.0 * w, 0.5)]);
      }
      points.push((1.0, 1.0));
      points
    };
    // 75 teeth: 302 corners
    assert_eq!(ShapeBuild::of(&shape(&comb(75), false)).parts.len(), 1);
    assert!(ShapeBuild::of(&shape(&comb(10), false)).parts.len() > 1);
  }

  #[test]
  fn degenerate_polygons_build_nothing() {
    let polygons: [&[(f32, f32)]; 6] = [
      &[],
      &[(0.0, 0.0), (1.0, 1.0)],
      &[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)],
      &[(0.0, 0.0), (f32::NAN, 0.0), (1.0, 1.0), (0.0, 1.0)],
      // Wholly outside the cell, and thinner than the grid
      &[(2.0, 2.0), (3.0, 2.0), (3.0, 3.0)],
      &[(0.0, 0.5), (1.0, 0.5), (1.0, 0.5 + 1e-6), (0.0, 0.5 + 1e-6)],
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
    let g = |v: f32| (v * GRID as f32) as i32;
    for reversed in [false, true] {
      let mut points = points.to_vec();
      if reversed {
        points.reverse();
      }
      let built = ShapeBuild::of(&shape(&points, true));
      assert!(built.parts.is_empty() && built.edges.is_empty() && !built.blocks);
      assert_eq!(
        built.tops,
        [
          ((0, g(0.25)), (g(0.5), g(0.25))),
          ((g(0.5), g(0.25)), (g(1.0), g(0.5)))
        ]
      );
    }
  }

  #[test]
  fn polygons_are_clipped_to_the_cell_and_snapped_to_the_grid() {
    // Sticking out past the right and bottom; a corner a hair off the grid
    let built = ShapeBuild::of(&shape(
      &[(0.5, 0.5), (2.0, 0.5), (2.0, 3.0), (0.5 + 1e-7, 3.0)],
      false,
    ));
    let (h, g) = (GRID / 2, GRID);
    let edges: std::collections::BTreeSet<_> = built.edges.iter().copied().collect();
    let square = [
      ((h, h), (g, h)),
      ((g, h), (g, g)),
      ((g, g), (h, g)),
      ((h, g), (h, h)),
    ];
    assert_eq!(edges, square.into_iter().collect());
    // Two polygons touching along an edge are one outline; overlapping ones
    // their hull
    let touching = CellShape {
      polygons: vec![
        vec![
          Vec2::ZERO,
          Vec2::new(0.5, 0.0),
          Vec2::new(0.5, 1.0),
          Vec2::Y,
        ],
        vec![Vec2::new(0.5, 0.0), Vec2::X, Vec2::ONE, Vec2::new(0.5, 1.0)],
      ],
      one_way: false,
    };
    assert_eq!(
      ShapeBuild::of(&touching).edges.len(),
      6,
      "the shared edge cancels, the sides split"
    );
    let overlapping = CellShape {
      polygons: vec![
        vec![
          Vec2::ZERO,
          Vec2::new(0.75, 0.0),
          Vec2::new(0.75, 0.5),
          Vec2::new(0.0, 0.5),
        ],
        vec![
          Vec2::new(0.25, 0.25),
          Vec2::new(1.0, 0.25),
          Vec2::ONE,
          Vec2::new(0.25, 1.0),
        ],
      ],
      one_way: false,
    };
    assert_eq!(
      ShapeBuild::of(&overlapping).edges.len(),
      6,
      "a hexagon: their hull"
    );
    // A polygon that crosses itself: its hull, a trapezoid
    let crossed = shape(&[(0.0, 0.0), (1.0, 0.0), (0.2, 1.0), (0.8, 1.0)], false);
    assert_eq!(ShapeBuild::of(&crossed).edges.len(), 4);
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
