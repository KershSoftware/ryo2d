//! Lines and trails as triangles: a strip along the points, a wedge on the
//! outside of every corner and a cap at each open end.
#![cfg_attr(
  not(test),
  expect(
    dead_code,
    reason = "trails and lines draw through it once they join the batch"
  )
)]

use std::ops::Range;

use glam::Vec2;

use crate::core::trail::{LineCap, LineJoin};

/// One vertex of the triangle stream.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Vertex2D {
  /// World position
  pub(crate) position: [f32; 2],
  /// Along the line (`u`) and across it (`v`, 0 on one edge, 1 on the other)
  pub(crate) uv: [f32; 2],
  /// Linear colour, straight alpha
  pub(crate) color: [f32; 4],
  /// Style and effect parameters
  pub(crate) params: [f32; 4],
  /// The unit perpendicular displacement effects move the vertex along
  pub(crate) across: [f32; 2],
}

/// A point of a polyline.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PolyPoint {
  pub(crate) pos: Vec2,
  pub(crate) half_width: f32,
  pub(crate) color: [f32; 4],
  /// Texture coordinate along the line: 0..1 when stretched, distance over
  /// tile length when tiled
  pub(crate) u: f32,
}

/// How a polyline is drawn.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PolyStyle {
  pub(crate) join: LineJoin,
  pub(crate) cap: LineCap,
  /// The last point joins the first
  pub(crate) closed: bool,
  /// Copied into every vertex (effect and style bits in `[0]`)
  pub(crate) params: [f32; 4],
}

/// A miter longer than this many times the line's width becomes a bevel.
pub(crate) const MITER_LIMIT: f32 = 4.0;

/// Triangles built on the CPU for the batch's triangle stream.
#[derive(Default)]
pub(crate) struct Triangles {
  pub(crate) vertices: Vec<Vertex2D>,
  pub(crate) indices: Vec<u32>,
}

impl Triangles {
  /// Forget every triangle, keeping the allocations.
  pub(crate) fn clear(&mut self) {
    self.vertices.clear();
    self.indices.clear();
  }

  /// Add the triangles of a polyline through `points`; the index range they
  /// take (empty when nothing is drawn).
  ///
  /// Non-finite points are skipped and points closer than `1e-4` to the one
  /// before collapse. Each segment is a quad sharing its edge vertices with
  /// its neighbours; each corner adds one wedge on its outside, so no area
  /// is covered twice (a corner too sharp to share its inner vertex
  /// overlaps on the inside instead of spiking).
  pub(crate) fn polyline(&mut self, points: &[PolyPoint], style: &PolyStyle) -> Range<u32> {
    let start = self.indices.len() as u32;
    let mut pts: Vec<PolyPoint> = Vec::with_capacity(points.len());
    for point in points {
      if !point.pos.is_finite() || !point.half_width.is_finite() {
        continue;
      }
      if pts
        .last()
        .is_some_and(|last| last.pos.distance(point.pos) < 1e-4)
      {
        continue;
      }
      pts.push(*point);
    }
    let mut closed = style.closed;
    if closed && pts.len() > 2 && pts[0].pos.distance(pts[pts.len() - 1].pos) < 1e-4 {
      pts.pop();
    }
    if pts.len() < 3 {
      closed = false;
    }
    if pts.len() < 2 || pts.iter().all(|point| point.half_width <= 0.0) {
      return start..start;
    }
    let mut line = Builder {
      out: self,
      pts: &pts,
      style,
    };
    line.build(closed);
    start..self.indices.len() as u32
  }
}

/// A segment's direction and left-hand normal.
#[derive(Clone, Copy)]
struct Segment {
  dir: Vec2,
  normal: Vec2,
  len: f32,
}

impl Segment {
  fn between(a: Vec2, b: Vec2) -> Self {
    let len = a.distance(b);
    let dir = (b - a) / len;
    Self {
      dir,
      normal: dir.perp(),
      len,
    }
  }
}

/// The vertices a segment ends on at a point (`end`) and the next one
/// starts from (`start`): (left, right) by the normal.
#[derive(Clone, Copy)]
struct Joint {
  end: (u32, u32),
  start: (u32, u32),
}

struct Builder<'a> {
  out: &'a mut Triangles,
  pts: &'a [PolyPoint],
  style: &'a PolyStyle,
}

impl Builder<'_> {
  fn build(&mut self, closed: bool) {
    let n = self.pts.len();
    let segments: Vec<Segment> = (0..if closed { n } else { n - 1 })
      .map(|i| Segment::between(self.pts[i].pos, self.pts[(i + 1) % n].pos))
      .collect();
    let mut joints = Vec::with_capacity(n + 1);
    for i in 0..n {
      let joint = match (closed, i) {
        (false, 0) => self.end_pair(0, segments[0], true),
        (false, _) if i == n - 1 => self.end_pair(i, segments[i - 1], false),
        // The seam: the closing segment ends on the first corner, with `u`
        // running on past the last point
        (true, 0) => {
          let last = segments[n - 1];
          let u = self.pts[n - 1].u + last.len * self.u_scale(n - 2, n - 1);
          self.corner_with_u(0, last, segments[0], Some(u))
        }
        _ => self.corner(i, segments[i - 1], segments[i % segments.len()]),
      };
      joints.push(joint);
    }
    if closed {
      joints.push(joints[0]);
    }
    for i in 0..segments.len() {
      let (from, to) = (joints[i].start, joints[i + 1].end);
      self.quad(from, to);
    }
  }

  /// `u` per world unit along the segment from point `a` to point `b`.
  fn u_scale(&self, a: usize, b: usize) -> f32 {
    let (pa, pb) = (self.pts[a], self.pts[b]);
    let len = pa.pos.distance(pb.pos);
    if len > 0.0 { (pb.u - pa.u) / len } else { 0.0 }
  }

  fn vertex(&mut self, at: Vec2, point: &PolyPoint, uv: [f32; 2], across: Vec2) -> u32 {
    let index = self.out.vertices.len() as u32;
    self.out.vertices.push(Vertex2D {
      position: at.into(),
      uv,
      color: point.color,
      params: self.style.params,
      across: across.into(),
    });
    index
  }

  /// The edge vertices at an open end, with its cap.
  fn end_pair(&mut self, i: usize, segment: Segment, first: bool) -> Joint {
    let point = self.pts[i];
    let (p, hw, n) = (point.pos, point.half_width, segment.normal);
    let left = self.vertex(p + n * hw, &point, [point.u, 0.0], n);
    let right = self.vertex(p - n * hw, &point, [point.u, 1.0], n);
    // Outward from the line, and how far `u` runs per world unit
    let (out, scale) = if first {
      (-segment.dir, -self.u_scale(0, 1))
    } else {
      (segment.dir, self.u_scale(i - 1, i))
    };
    match self.style.cap {
      LineCap::Butt => {}
      LineCap::Square => {
        let u = point.u + hw * scale;
        let far_left = self.vertex(p + n * hw + out * hw, &point, [u, 0.0], n);
        let far_right = self.vertex(p - n * hw + out * hw, &point, [u, 1.0], n);
        self.quad((left, right), (far_left, far_right));
      }
      LineCap::Round => {
        let centre = self.vertex(p, &point, [point.u, 0.5], Vec2::ZERO);
        // From the left edge round the outside to the right edge
        let from = n;
        let turn = if first { 1.0 } else { -1.0 };
        let steps = 16;
        let mut previous = left;
        for k in 1..steps {
          let angle = turn * std::f32::consts::PI * k as f32 / steps as f32;
          let offset = Vec2::from_angle(angle).rotate(from) * hw;
          let along = offset.dot(out);
          let uv = [point.u + along * scale, 0.5 - 0.5 * offset.dot(n) / hw];
          let arc = self.vertex(p + offset, &point, uv, n);
          self.triangle(centre, previous, arc);
          previous = arc;
        }
        self.triangle(centre, previous, right);
      }
    }
    Joint {
      end: (left, right),
      start: (left, right),
    }
  }

  fn corner(&mut self, i: usize, before: Segment, after: Segment) -> Joint {
    self.corner_with_u(i, before, after, None)
  }

  /// The vertices where `before` meets `after` at point `i`, with the join's
  /// wedge. `end_u` gives the ending side a `u` of its own (the closed
  /// seam), so nothing is shared across it.
  fn corner_with_u(
    &mut self,
    i: usize,
    before: Segment,
    after: Segment,
    end_u: Option<f32>,
  ) -> Joint {
    let point = self.pts[i];
    let (p, hw, u) = (point.pos, point.half_width, point.u);
    let (n0, n1) = (before.normal, after.normal);
    let turn = before.dir.perp_dot(after.dir);
    let cos = n0.dot(n1);
    let u_end = end_u.unwrap_or(u);

    // Straight on: one pair, shared
    if turn.abs() < 1e-6 && cos > 0.0 {
      let n = (n0 + n1).normalize_or(n0);
      let start = (
        self.vertex(p + n * hw, &point, [u, 0.0], n),
        self.vertex(p - n * hw, &point, [u, 1.0], n),
      );
      let end = match end_u {
        None => start,
        Some(u) => (
          self.vertex(p + n * hw, &point, [u, 0.0], n),
          self.vertex(p - n * hw, &point, [u, 1.0], n),
        ),
      };
      return Joint { end, start };
    }

    // The outside of the turn, as a multiple of the normals, and the edge
    // (`v`) it is on
    let side = if turn > 0.0 { -1.0 } else { 1.0 };
    let (v_out, v_in) = if side > 0.0 { (0.0, 1.0) } else { (1.0, 0.0) };
    // Where the offset edges meet, as a multiple of the half-width
    let miter = if 1.0 + cos > 1e-6 {
      (n0 + n1) / (1.0 + cos)
    } else {
      Vec2::splat(f32::INFINITY)
    };
    let bisector = (n0 + n1).normalize_or(n0);
    let reach = miter.length() * hw;
    let shares_inside = reach.is_finite() && reach <= before.len.min(after.len);

    let outer_end = self.vertex(p + side * n0 * hw, &point, [u_end, v_out], n0);
    let outer_start = self.vertex(p + side * n1 * hw, &point, [u, v_out], n1);
    let (inner_end, inner_start, pivot) = if shares_inside {
      let at = p - side * miter * hw;
      let inner = self.vertex(at, &point, [u, v_in], bisector);
      let inner_end = match end_u {
        None => inner,
        Some(u) => self.vertex(at, &point, [u, v_in], bisector),
      };
      (inner_end, inner, inner)
    } else {
      // Too sharp to share: each segment keeps its own inner edge, and the
      // wedge turns about the point
      let inner_end = self.vertex(p - side * n0 * hw, &point, [u_end, v_in], n0);
      let inner_start = self.vertex(p - side * n1 * hw, &point, [u, v_in], n1);
      let centre = self.vertex(p, &point, [u, 0.5], Vec2::ZERO);
      (inner_end, inner_start, centre)
    };

    // The wedge: a bevel, then the miter's point or the round's arc
    self.triangle(pivot, outer_end, outer_start);
    match self.style.join {
      LineJoin::Bevel => {}
      LineJoin::Miter => {
        if miter.length() <= MITER_LIMIT {
          let tip = self.vertex(p + side * miter * hw, &point, [u, v_out], bisector);
          self.triangle(outer_end, tip, outer_start);
        }
      }
      LineJoin::Round => {
        let from = side * n0;
        let angle = from.angle_to(side * n1);
        let steps = ((angle.abs() / (std::f32::consts::PI / 16.0)).ceil() as u32).max(2);
        let mut previous = None;
        for k in 1..steps {
          let t = k as f32 / steps as f32;
          let direction = Vec2::from_angle(angle * t).rotate(from);
          let across = Vec2::from_angle(angle * t).rotate(n0);
          let arc = self.vertex(p + direction * hw, &point, [u, v_out], across);
          if let Some(previous) = previous {
            self.triangle(outer_end, previous, arc);
          }
          previous = Some(arc);
        }
        if let Some(previous) = previous {
          self.triangle(outer_end, previous, outer_start);
        }
      }
    }

    // As (left, right) by the normal
    let pair = |outer, inner| {
      if side > 0.0 {
        (outer, inner)
      } else {
        (inner, outer)
      }
    };
    Joint {
      end: pair(outer_end, inner_end),
      start: pair(outer_start, inner_start),
    }
  }

  /// The quad from edge pair `from` to edge pair `to`.
  fn quad(&mut self, from: (u32, u32), to: (u32, u32)) {
    self.triangle(from.0, from.1, to.0);
    self.triangle(from.1, to.1, to.0);
  }

  fn triangle(&mut self, a: u32, b: u32, c: u32) {
    self.out.indices.extend_from_slice(&[a, b, c]);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::trail::{LineCap, LineJoin};
  use glam::Vec2;

  fn build(
    points: &[(f32, f32)],
    width: f32,
    join: LineJoin,
    cap: LineCap,
    closed: bool,
  ) -> Triangles {
    let pts: Vec<PolyPoint> = points
      .iter()
      .enumerate()
      .map(|(i, &(x, y))| PolyPoint {
        pos: Vec2::new(x, y),
        half_width: width / 2.0,
        color: [1.0; 4],
        u: i as f32,
      })
      .collect();
    let mut t = Triangles::default();
    t.polyline(
      &pts,
      &PolyStyle {
        join,
        cap,
        closed,
        params: [0.0; 4],
      },
    );
    t
  }

  fn area(t: &Triangles) -> f32 {
    t.indices
      .chunks(3)
      .map(|tri| {
        let [a, b, c] = [0, 1, 2].map(|k| Vec2::from(t.vertices[tri[k] as usize].position));
        (b - a).perp_dot(c - a).abs() / 2.0
      })
      .sum()
  }

  const L: [(f32, f32); 3] = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];

  #[test]
  fn a_straight_segment_covers_its_rectangle() {
    let t = build(
      &[(0.0, 0.0), (10.0, 0.0)],
      2.0,
      LineJoin::Miter,
      LineCap::Butt,
      false,
    );
    assert!((area(&t) - 20.0).abs() < 1e-3, "{}", area(&t));
  }

  #[test]
  fn a_mitered_corner_covers_its_outline_once() {
    // Outline (0,-1) (11,-1) (11,10) (9,10) (9,1) (0,1): area 40
    let t = build(&L, 2.0, LineJoin::Miter, LineCap::Butt, false);
    assert!((area(&t) - 40.0).abs() < 1e-3, "{}", area(&t));
  }

  #[test]
  fn a_bevelled_corner_cuts_the_miter_triangle() {
    let t = build(&L, 2.0, LineJoin::Bevel, LineCap::Butt, false);
    assert!((area(&t) - 39.5).abs() < 1e-3, "{}", area(&t));
  }

  #[test]
  fn a_round_corner_approaches_a_quarter_circle() {
    let t = build(&L, 2.0, LineJoin::Round, LineCap::Butt, false);
    let expected = 39.0 + std::f32::consts::FRAC_PI_4;
    assert!(
      (area(&t) - expected).abs() < 0.05,
      "{} vs {expected}",
      area(&t)
    );
  }

  #[test]
  fn caps_extend_the_ends() {
    let square = build(
      &[(0.0, 0.0), (10.0, 0.0)],
      2.0,
      LineJoin::Miter,
      LineCap::Square,
      false,
    );
    assert!((area(&square) - 24.0).abs() < 1e-3, "{}", area(&square));
    let round = build(
      &[(0.0, 0.0), (10.0, 0.0)],
      2.0,
      LineJoin::Miter,
      LineCap::Round,
      false,
    );
    assert!(
      (area(&round) - (20.0 + std::f32::consts::PI)).abs() < 0.1,
      "{}",
      area(&round)
    );
  }

  #[test]
  fn a_closed_square_joins_at_the_seam() {
    let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let t = build(&square, 2.0, LineJoin::Miter, LineCap::Butt, true);
    assert!(
      (area(&t) - 80.0).abs() < 1e-3,
      "outer 144 minus inner 64: {}",
      area(&t)
    );
  }

  #[test]
  fn a_u_turn_falls_back_to_a_bevel() {
    let t = build(
      &[(0.0, 0.0), (10.0, 0.0), (0.0, 0.1)],
      2.0,
      LineJoin::Miter,
      LineCap::Butt,
      false,
    );
    for v in &t.vertices {
      let p = Vec2::from(v.position);
      assert!(p.is_finite(), "{p}");
      assert!(
        p.distance(Vec2::new(10.0, 0.0)) < 10.0 + MITER_LIMIT * 2.0,
        "spike at {p}"
      );
    }
    assert!(!t.indices.is_empty());
  }

  #[test]
  fn degenerate_input_draws_nothing() {
    for points in [
      vec![],
      vec![(1.0, 1.0)],
      vec![(1.0, 1.0), (1.0, 1.0)],
      vec![(f32::NAN, 0.0), (1.0, 1.0)],
    ] {
      let t = build(&points, 2.0, LineJoin::Miter, LineCap::Round, false);
      assert!(t.indices.is_empty(), "{points:?}");
    }
    let zero = build(
      &[(0.0, 0.0), (10.0, 0.0)],
      0.0,
      LineJoin::Miter,
      LineCap::Butt,
      false,
    );
    assert!(zero.indices.is_empty());
  }

  #[test]
  fn ranges_count_from_the_last_clear() {
    let mut t = build(&L, 2.0, LineJoin::Miter, LineCap::Butt, false);
    let style = PolyStyle {
      join: LineJoin::Miter,
      cap: LineCap::Butt,
      closed: false,
      params: [0.0; 4],
    };
    let points = [(0.0, 0.0), (5.0, 0.0)].map(|(x, y)| PolyPoint {
      pos: Vec2::new(x, y),
      half_width: 1.0,
      color: [1.0; 4],
      u: x / 5.0,
    });
    let second = t.polyline(&points, &style);
    assert_eq!(second.start as usize, t.indices.len() - second.len());
    t.clear();
    assert!(t.vertices.is_empty() && t.indices.is_empty());
    assert_eq!(t.polyline(&points, &style), 0..6);
  }

  #[test]
  fn uv_runs_along_and_across() {
    let t = build(
      &[(0.0, 0.0), (10.0, 0.0)],
      2.0,
      LineJoin::Miter,
      LineCap::Butt,
      false,
    );
    for v in &t.vertices {
      assert!(v.uv[1] == 0.0 || v.uv[1] == 1.0, "{:?}", v.uv);
      assert!((0.0..=1.0).contains(&v.uv[0]), "{:?}", v.uv);
    }
  }
}
