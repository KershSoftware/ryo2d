//! Lines and trails as triangles: a strip along the points, a wedge on the
//! outside of every corner and a cap at each open end.
//!
//! Every vertex carries its distances to the line's edges, in the line's
//! units, and how far a fringe may push it outward; the builder does not know
//! the camera.
//!
//! - **Distances are true over every triangle.** Each triangle belongs to
//!   one straight piece and measures across from its centre line (`dist`,
//!   with `half` the half width there), a second edge where it lies against
//!   two (`second`: the other segment's outer edge in a miter's point, the
//!   bevel's edge in a bevel's halves, a butt or square cap's edge in the
//!   capped half of its segment), or, in a round piece, its offset from the
//!   corner point (`round`). Every one is an affine function of position over
//!   its triangle, so it is exact wherever it is interpolated
//!   (`every_attribute_is_the_true_distance_over_its_triangle`). A vertex is
//!   shared only between triangles that measure the same; elsewhere a piece
//!   has its own copy at the same place, pushed the same way: a miter's point,
//!   a bevel's halves and a round join's arc each have their own vertices, a
//!   round cap its own copies of the strip's end vertices.
//! - **A push of one unit grows every distance by one unit**
//!   (`a_push_of_one_unit_grows_every_distance_by_one`): `dist` away from the
//!   centre line (down where `FLAG_GROWS_DOWN` says), `second` one less, and
//!   `round` by the push itself. A vertex on two edges pushes along their
//!   miter: a miter's point and the strip's outer vertices beside it along
//!   the miter, a bevel's elbows and middle along the miter of the bevel and
//!   their segment's edge, a cap's corners out across the cap and the side.
//! - **Pushed, the mesh stays watertight and covers nothing twice**
//!   (`a_translucent_line_covers_no_pixel_twice_at_any_join`). Where two
//!   copies at one place push apart, a fringe triangle with no area until
//!   pushed fills what opens: between a bevel's halves (bounded by the
//!   bevel's edge alone, `half` = `NO_EDGE`) and between a capped half and
//!   the rest of its segment, which is cut at its middle for it. An unshared
//!   inner vertex pushes opposite its outer one, so their end keeps passing
//!   through the corner point the wedge fans from. A sharp corner's inner
//!   side still overlaps, as it does unpushed.
//! - **A pushed quad never folds** (`a_pushed_inner_vertex_never_folds_its_quad`).
//!   Every vertex of a segment's quad pushes at most the quad's limit, the
//!   least its corners allow: a shared inner vertex until it would pass the
//!   shorter segment's middle, and a push that runs along the segment (a
//!   miter's or a bevel's) until it has run half the quad's length. A capped
//!   half moves whole and needs none. Vertices never pushed have a limit of
//!   zero.
//! - **Limit:** on a tapered line a push that runs along the segment keeps
//!   the vertex's `half`, which the taper would change by its slope times
//!   that run.

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
  /// The atlas slot a textured line repeats (UV min and max), set when it
  /// is placed; zero for untextured ones
  pub(crate) bounds: [f32; 4],
  /// Signed distance across from the centre line of its triangle's straight
  /// piece, in the line's units (positive on the left, by the normal)
  pub(crate) dist: f32,
  /// The half width there (`NO_EDGE` where only the second edge bounds it)
  pub(crate) half: f32,
  /// The distance inside a second edge (the other outer edge near a miter
  /// tip, the bevel's edge, a cap's edge), or `NO_EDGE`
  pub(crate) second: f32,
  /// The most the fringe may push it, in the line's units
  pub(crate) push_limit: f32,
  /// A round piece's offset from its corner point (zero elsewhere)
  pub(crate) round: [f32; 2],
  /// The way the fringe pushes it, scaled so a push of one unit grows each
  /// of its distances by one unit
  pub(crate) push: [f32; 2],
  /// `FLAG_ROUND`, `FLAG_SECOND` and `FLAG_GROWS_DOWN`
  pub(crate) flags: u32,
}

/// A vertex of a round piece: coverage by the length of its `round` offset
pub(crate) const FLAG_ROUND: u32 = 1;
/// A vertex whose `second` edge distance counts
pub(crate) const FLAG_SECOND: u32 = 2;
/// A vertex whose `dist` falls as it is pushed (on the right of its centre
/// line); otherwise it rises
pub(crate) const FLAG_GROWS_DOWN: u32 = 4;
/// No edge: the `second` of a vertex without one, the `half` of one that only
/// a second edge bounds, and the `push_limit` of one pushed without limit
pub(crate) const NO_EDGE: f32 = 1.0e6;

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
  /// The most any vertex of its quad may push: the least its corners allow
  limit: f32,
}

impl Segment {
  fn between(a: Vec2, b: Vec2) -> Self {
    let len = a.distance(b);
    let dir = (b - a) / len;
    Self {
      dir,
      normal: dir.perp(),
      len,
      limit: NO_EDGE,
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

/// What a vertex measures, in the line's units, and how the fringe pushes
/// it.
#[derive(Clone, Copy)]
struct Measure {
  dist: f32,
  half: f32,
  second: Option<f32>,
  round: Option<Vec2>,
  push: Vec2,
  limit: f32,
  /// The way `dist` goes as the vertex is pushed: 1 up, -1 down
  grows: f32,
}

impl Measure {
  /// On the edge of a strip of half width `half`, on the `side` (1 left, -1
  /// right) of its centre line, pushed along `push` (one unit across per
  /// unit) by at most `limit`.
  fn edge(side: f32, half: f32, push: Vec2, limit: f32) -> Self {
    Self {
      dist: side * half,
      half,
      second: None,
      round: None,
      push,
      limit,
      grows: side,
    }
  }

  /// A corner point, never pushed.
  fn centre(half: f32) -> Self {
    Self::edge(0.0, half, Vec2::ZERO, 0.0)
  }

  /// A vertex of a round piece, `offset` from its corner point.
  fn round(half: f32, offset: Vec2, push: Vec2, limit: f32) -> Self {
    Self {
      round: Some(offset),
      ..Self::edge(0.0, half, push, limit)
    }
  }

  /// A vertex of a piece no strip bounds, only its second edge.
  fn second_only(second: f32, push: Vec2, limit: f32, grows: f32) -> Self {
    Self {
      dist: 0.0,
      half: NO_EDGE,
      second: Some(second),
      round: None,
      push,
      limit,
      grows,
    }
  }

  fn with_second(self, second: f32) -> Self {
    Self {
      second: Some(second),
      ..self
    }
  }
}

/// `v` measuring a second edge `second` inside it.
fn set_second(v: &mut Vertex2D, second: f32) {
  v.second = second;
  v.flags |= FLAG_SECOND;
}

/// `v` measuring no second edge.
fn clear_second(v: &mut Vertex2D) {
  v.second = NO_EDGE;
  v.flags &= !FLAG_SECOND;
}

/// `v` as a vertex of a round piece, `offset` from its corner point.
fn set_round(v: &mut Vertex2D, offset: Vec2) {
  v.dist = 0.0;
  v.second = NO_EDGE;
  v.round = offset.into();
  v.flags = FLAG_ROUND;
}

/// The push that moves a point out across two edges of outward normals `a`
/// and `b` one unit each per unit (`a` and `b` at most a right angle apart).
fn miter_of(a: Vec2, b: Vec2) -> Vec2 {
  (a + b) / (1.0 + a.dot(b))
}

/// How a corner turns: what its vertices and their pushes follow.
#[derive(Clone, Copy)]
struct Turn {
  /// The outside of the turn, as a multiple of the normals
  side: f32,
  cos: f32,
  /// Where the offset edges meet, as a multiple of the half width
  miter: Vec2,
  bisector: Vec2,
  /// Whether the segments share their inner vertex
  shares_inside: bool,
  /// Whether a miter join keeps its point (else it is bevelled)
  mitered: bool,
  /// The bevel's outward normal
  bevel: Vec2,
  /// How the outer vertices push, at the end of `before` and the start of
  /// `after`
  push: (Vec2, Vec2),
  /// How far the shared inner vertex may push
  inner_limit: f32,
}

impl Turn {
  /// Where `before` meets `after` at a point of half width `hw`; `None`
  /// where the line runs straight on.
  fn at(before: Segment, after: Segment, hw: f32, join: LineJoin) -> Option<Self> {
    let (n0, n1) = (before.normal, after.normal);
    let turn = before.dir.perp_dot(after.dir);
    let cos = n0.dot(n1);
    if turn.abs() < 1e-6 && cos > 0.0 {
      return None;
    }
    let side = if turn > 0.0 { -1.0 } else { 1.0 };
    // The outer edges' outward normals
    let (m0, m1) = (side * n0, side * n1);
    let miter = if 1.0 + cos > 1e-6 {
      (n0 + n1) / (1.0 + cos)
    } else {
      Vec2::splat(f32::INFINITY)
    };
    let reach = miter.length() * hw;
    let shortest = before.len.min(after.len);
    // Shared only while it stays in the near half of both segments: the
    // corner at a segment's other end may pull its inner edge from that end
    // too, and past the middle the two would cross and twist the segment
    let shares_inside = reach.is_finite() && reach <= shortest / 2.0;
    let mitered = join == LineJoin::Miter && miter.length() <= MITER_LIMIT;
    // Forward at a U-turn, where the bevel ends the line
    let bevel = (m0 + m1).normalize_or(before.dir);
    // A miter's outer vertices push along the miter, which takes both outer
    // edges out by one unit per unit, as its point measures the other
    // segment's edge; a bevel's along the miter of their segment's edge and
    // the bevel's; a round's straight out
    let push = match join {
      LineJoin::Round => (m0, m1),
      _ if mitered => (side * miter, side * miter),
      _ => (miter_of(m0, bevel), miter_of(m1, bevel)),
    };
    // The shared inner vertex pushes along the inner miter until it would
    // pass the shorter segment's middle
    let inner_limit = if shares_inside {
      (shortest / (2.0 * miter.length()) - hw).max(0.0)
    } else {
      NO_EDGE
    };
    Some(Self {
      side,
      cos,
      miter,
      bisector: (n0 + n1).normalize_or(n0),
      shares_inside,
      mitered,
      bevel,
      push,
      inner_limit,
    })
  }

  /// The most a vertex of the quad beside it, `length` long along
  /// `segment`, may push for this corner: until a push along the miter or a
  /// bevel's, which also runs along the segment, has run half the quad's
  /// length, and no further than the shared inner vertex may.
  fn limit_on(&self, push: Vec2, segment: Segment, length: f32) -> f32 {
    let along = push.dot(segment.dir).abs();
    let runs = if along > 1e-6 {
      (length / (2.0 * along)).min(NO_EDGE)
    } else {
      NO_EDGE
    };
    runs.min(self.inner_limit)
  }
}

struct Builder<'a> {
  out: &'a mut Triangles,
  pts: &'a [PolyPoint],
  style: &'a PolyStyle,
}

impl Builder<'_> {
  fn build(&mut self, closed: bool) {
    let n = self.pts.len();
    let mut segments: Vec<Segment> = (0..if closed { n } else { n - 1 })
      .map(|i| Segment::between(self.pts[i].pos, self.pts[(i + 1) % n].pos))
      .collect();
    let cut = !closed && matches!(self.style.cap, LineCap::Butt | LineCap::Square);
    let last = segments.len() - 1;
    // Every vertex of a segment's quad pushes as far as the least its
    // corners allow, so the quad grows evenly and never folds
    let corners = if closed { 0..n } else { 1..n - 1 };
    for i in corners {
      let (b, a) = (
        (i + segments.len() - 1) % segments.len(),
        i % segments.len(),
      );
      let Some(turn) = Turn::at(
        segments[b],
        segments[a],
        self.pts[i].half_width,
        self.style.join,
      ) else {
        continue;
      };
      // A cut segment's quad beside a corner is its uncapped half
      let length = |s: usize| {
        let len = segments[s].len;
        if cut && (s == 0 || s == last) {
          len / 2.0
        } else {
          len
        }
      };
      let before = turn.limit_on(turn.push.0, segments[b], length(b));
      let after = turn.limit_on(turn.push.1, segments[a], length(a));
      segments[b].limit = segments[b].limit.min(before);
      segments[a].limit = segments[a].limit.min(after);
    }
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
    for (i, segment) in segments.iter().enumerate() {
      let (from, to) = (joints[i].start, joints[i + 1].end);
      self.strip(from, to, *segment, cut && i == 0, cut && i == last);
    }
  }

  /// `u` per world unit along the segment from point `a` to point `b`.
  fn u_scale(&self, a: usize, b: usize) -> f32 {
    let (pa, pb) = (self.pts[a], self.pts[b]);
    let len = pa.pos.distance(pb.pos);
    if len > 0.0 { (pb.u - pa.u) / len } else { 0.0 }
  }

  fn vertex(&mut self, at: Vec2, point: &PolyPoint, uv: [f32; 2], across: Vec2, m: Measure) -> u32 {
    let index = self.out.vertices.len() as u32;
    let mut flags = 0;
    if m.round.is_some() {
      flags |= FLAG_ROUND;
    }
    if m.second.is_some() {
      flags |= FLAG_SECOND;
    }
    if m.grows < 0.0 {
      flags |= FLAG_GROWS_DOWN;
    }
    self.out.vertices.push(Vertex2D {
      position: at.into(),
      uv,
      color: point.color,
      params: self.style.params,
      across: across.into(),
      bounds: [0.0; 4],
      dist: m.dist,
      half: m.half,
      second: m.second.unwrap_or(NO_EDGE),
      push_limit: m.limit,
      round: m.round.unwrap_or(Vec2::ZERO).into(),
      push: m.push.into(),
      flags,
    });
    index
  }

  /// A copy of vertex `index`, at its place and pushed as it is, measuring
  /// what `edit` makes it measure.
  fn copy(&mut self, index: u32, edit: impl FnOnce(&mut Vertex2D)) -> u32 {
    let mut vertex = self.out.vertices[index as usize];
    edit(&mut vertex);
    self.out.vertices.push(vertex);
    self.out.vertices.len() as u32 - 1
  }

  /// The strip of `segment` from edge pair `from` to edge pair `to`.
  ///
  /// Beside a butt or square cap the strip measures the cap's edge as its
  /// second, and its vertices push out across it too, so a capped strip is
  /// cut at its middle: each capped half measures its own cap and moves with
  /// it, and a fringe quad between the halves, with no area until they push
  /// apart, keeps the strip watertight.
  fn strip(
    &mut self,
    from: (u32, u32),
    to: (u32, u32),
    segment: Segment,
    cap_start: bool,
    cap_end: bool,
  ) {
    let (n, dir) = (segment.normal, segment.dir);
    // A capped half moves whole, so it needs no limit; the uncapped one
    // pushes as far as the segment's quad may
    let middle = |b: &mut Self, push: (Vec2, Vec2), cap: Option<((u32, u32), Vec2)>, limit| {
      let left = cap.map(|(at, inward)| (at.0, inward));
      let right = cap.map(|(at, inward)| (at.1, inward));
      (
        b.middle(from.0, to.0, 1.0, push.0, left, limit),
        b.middle(from.1, to.1, -1.0, push.1, right, limit),
      )
    };
    let near = cap_start.then(|| middle(self, (n - dir, -n - dir), Some((from, dir)), NO_EDGE));
    let far = cap_end.then(|| middle(self, (n + dir, -n + dir), Some((to, -dir)), NO_EDGE));
    let loose = |b: &mut Self, (left, right): (u32, u32)| {
      (b.copy(left, clear_second), b.copy(right, clear_second))
    };
    match (near, far) {
      (None, None) => self.quad(from, to),
      (Some(near), Some(far)) => {
        self.quad(from, near);
        let gap = (loose(self, near), loose(self, far));
        self.quad(gap.0, gap.1);
        self.quad(far, to);
      }
      (Some(near), None) => {
        let rest = middle(self, (n, -n), None, segment.limit);
        self.quad(from, near);
        let gap = loose(self, near);
        self.quad(gap, rest);
        self.quad(rest, to);
      }
      (None, Some(far)) => {
        let rest = middle(self, (n, -n), None, segment.limit);
        self.quad(from, rest);
        let gap = loose(self, far);
        self.quad(rest, gap);
        self.quad(far, to);
      }
    }
  }

  /// A vertex halfway along a strip's edge from vertex `a` to vertex `b`, on
  /// the `side` (1 left, -1 right) of its centre line, pushed along `push` by
  /// at most `limit`; `cap` gives the cap vertex and inward direction a
  /// capped half measures its cap's edge from.
  fn middle(
    &mut self,
    a: u32,
    b: u32,
    side: f32,
    push: Vec2,
    cap: Option<(u32, Vec2)>,
    limit: f32,
  ) -> u32 {
    let (va, vb) = (self.out.vertices[a as usize], self.out.vertices[b as usize]);
    let mean = |x: f32, y: f32| (x + y) / 2.0;
    let at = (Vec2::from(va.position) + Vec2::from(vb.position)) / 2.0;
    let mut vertex = va;
    vertex.position = at.into();
    vertex.uv = [mean(va.uv[0], vb.uv[0]), mean(va.uv[1], vb.uv[1])];
    vertex.color = [0, 1, 2, 3].map(|k| mean(va.color[k], vb.color[k]));
    vertex.dist = mean(va.dist, vb.dist);
    vertex.half = mean(va.half, vb.half);
    vertex.push = push.into();
    vertex.push_limit = limit;
    vertex.round = [0.0; 2];
    vertex.second = NO_EDGE;
    vertex.flags = if side < 0.0 { FLAG_GROWS_DOWN } else { 0 };
    if let Some((cap, inward)) = cap {
      let edge = self.out.vertices[cap as usize];
      let second = edge.second + (at - Vec2::from(edge.position)).dot(inward);
      set_second(&mut vertex, second);
    }
    self.out.vertices.push(vertex);
    self.out.vertices.len() as u32 - 1
  }

  /// The edge vertices at an open end, with its cap.
  fn end_pair(&mut self, i: usize, segment: Segment, first: bool) -> Joint {
    let point = self.pts[i];
    let (p, hw, n, u) = (point.pos, point.half_width, segment.normal, point.u);
    // Outward from the line, and how far `u` runs per world unit
    let (out, scale) = if first {
      (-segment.dir, -self.u_scale(0, 1))
    } else {
      (segment.dir, self.u_scale(i - 1, i))
    };
    let (left, right) = match self.style.cap {
      // The strip's end vertices are the cap's corners, pushed out across
      // both of their edges
      LineCap::Butt => (
        self.vertex(
          p + n * hw,
          &point,
          [u, 0.0],
          n,
          Measure::edge(1.0, hw, n + out, NO_EDGE).with_second(0.0),
        ),
        self.vertex(
          p - n * hw,
          &point,
          [u, 1.0],
          n,
          Measure::edge(-1.0, hw, -n + out, NO_EDGE).with_second(0.0),
        ),
      ),
      LineCap::Square => {
        let edge = |side: f32, second: f32| {
          Measure::edge(side, hw, side * n + out, NO_EDGE).with_second(second)
        };
        let left = self.vertex(p + n * hw, &point, [u, 0.0], n, edge(1.0, hw));
        let right = self.vertex(p - n * hw, &point, [u, 1.0], n, edge(-1.0, hw));
        let far_u = u + hw * scale;
        let far_left = self.vertex(
          p + n * hw + out * hw,
          &point,
          [far_u, 0.0],
          n,
          edge(1.0, 0.0),
        );
        let far_right = self.vertex(
          p - n * hw + out * hw,
          &point,
          [far_u, 1.0],
          n,
          edge(-1.0, 0.0),
        );
        // Along the line's direction, like the strip
        if first {
          self.quad((far_left, far_right), (left, right));
        } else {
          self.quad((left, right), (far_left, far_right));
        }
        (left, right)
      }
      LineCap::Round => {
        let (left, right) = self.straight_pair(&point, n, u, segment.limit);
        // The fan measures its distance from the end point, so it has its
        // own copies of the strip's end vertices, at their places and pushed
        // as they are
        let centre = self.vertex(
          p,
          &point,
          [u, 0.5],
          Vec2::ZERO,
          Measure::round(hw, Vec2::ZERO, Vec2::ZERO, 0.0),
        );
        // From the left edge round the outside to the right edge
        let turn = if first { 1.0 } else { -1.0 };
        let steps = 16;
        let mut previous = self.copy(left, |v| set_round(v, n * hw));
        for k in 1..steps {
          let angle = turn * std::f32::consts::PI * k as f32 / steps as f32;
          let outward = Vec2::from_angle(angle).rotate(n);
          let offset = outward * hw;
          let along = offset.dot(out);
          let uv = [u + along * scale, 0.5 - 0.5 * outward.dot(n)];
          let arc = self.vertex(
            p + offset,
            &point,
            uv,
            n,
            Measure::round(hw, offset, outward, NO_EDGE),
          );
          // The fan turns the other way at the end: wind it like the strip
          self.wedge(-turn, centre, previous, arc);
          previous = arc;
        }
        let last = self.copy(right, |v| set_round(v, -n * hw));
        self.wedge(-turn, centre, previous, last);
        (left, right)
      }
    };
    Joint {
      end: (left, right),
      start: (left, right),
    }
  }

  fn corner(&mut self, i: usize, before: Segment, after: Segment) -> Joint {
    self.corner_with_u(i, before, after, None)
  }

  /// A left and right vertex at `point`, `n` (unit) and `-n` times its half
  /// width from it, pushed straight out by at most `limit`.
  fn straight_pair(&mut self, point: &PolyPoint, n: Vec2, u: f32, limit: f32) -> (u32, u32) {
    let (p, hw) = (point.pos, point.half_width);
    (
      self.vertex(
        p + n * hw,
        point,
        [u, 0.0],
        n,
        Measure::edge(1.0, hw, n, limit),
      ),
      self.vertex(
        p - n * hw,
        point,
        [u, 1.0],
        n,
        Measure::edge(-1.0, hw, -n, limit),
      ),
    )
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
    let u_end = end_u.unwrap_or(u);
    // Each segment's vertices push as far as its quad's limit; a vertex both
    // quads share, as far as both allow
    let (limit0, limit1) = (before.limit, after.limit);
    let both = limit0.min(limit1);

    // Straight on: one pair, shared
    let Some(turn) = Turn::at(before, after, hw, self.style.join) else {
      let n = (n0 + n1).normalize_or(n0);
      let start = self.straight_pair(&point, n, u, both);
      let end = match end_u {
        None => start,
        Some(u) => self.straight_pair(&point, n, u, both),
      };
      return Joint { end, start };
    };
    let Turn {
      side,
      cos,
      miter,
      bisector,
      shares_inside,
      mitered,
      bevel,
      push: (push0, push1),
      ..
    } = turn;
    // The edge (`v`) the outside of the turn is on, and its outward normals
    let (v_out, v_in) = if side > 0.0 { (0.0, 1.0) } else { (1.0, 0.0) };
    let (m0, m1) = (side * n0, side * n1);

    let outer_end = self.vertex(
      p + m0 * hw,
      &point,
      [u_end, v_out],
      n0,
      Measure::edge(side, hw, push0, limit0),
    );
    let outer_start = self.vertex(
      p + m1 * hw,
      &point,
      [u, v_out],
      n1,
      Measure::edge(side, hw, push1, limit1),
    );
    let (inner_end, inner_start) = if shares_inside {
      // The corner point: the segments' slanted ends fan from it, so `v`
      // keeps measuring the distance from the centre line across the join
      let centre = self.vertex(p, &point, [u, 0.5], Vec2::ZERO, Measure::centre(hw));
      let at = p - side * miter * hw;
      let measure = Measure::edge(-side, hw, -side * miter, both);
      let inner = self.vertex(at, &point, [u, v_in], bisector, measure);
      let inner_end = match end_u {
        None => inner,
        Some(u) => self.vertex(at, &point, [u, v_in], bisector, measure),
      };
      self.wedge(side, centre, inner_end, outer_end);
      self.wedge(side, centre, outer_start, inner);
      (inner_end, inner)
    } else {
      // Too sharp to share: each segment keeps its own inner edge, pushed
      // opposite its outer vertex, so the end they share keeps passing
      // through the corner point the wedge fans from
      let inner_end = self.vertex(
        p - m0 * hw,
        &point,
        [u_end, v_in],
        n0,
        Measure::edge(-side, hw, -push0, limit0),
      );
      let inner_start = self.vertex(
        p - m1 * hw,
        &point,
        [u, v_in],
        n1,
        Measure::edge(-side, hw, -push1, limit1),
      );
      (inner_end, inner_start)
    };

    // The outside: the miter's point, a bevel or the round's arc, each with
    // vertices of its own at the strip's places, pushed as the strip's are
    match self.style.join {
      LineJoin::Miter if mitered => {
        // Each half of the point measures its segment across and the other
        // segment's outer edge as its second
        let centre = self.vertex(
          p,
          &point,
          [u, 0.5],
          Vec2::ZERO,
          Measure::centre(hw).with_second(hw),
        );
        let other_edge = hw * (1.0 - cos);
        let end = self.copy(outer_end, |v| set_second(v, other_edge));
        let tip = self.vertex(
          p + side * miter * hw,
          &point,
          [u, v_out],
          bisector,
          Measure::edge(side, hw, side * miter, NO_EDGE).with_second(0.0),
        );
        let start = self.copy(outer_start, |v| set_second(v, other_edge));
        self.wedge(side, centre, end, tip);
        self.wedge(side, centre, tip, start);
      }
      LineJoin::Miter | LineJoin::Bevel => {
        // Each half of the bevel measures its segment across and the bevel's
        // edge as its second; between them a fringe triangle bounded by the
        // bevel's edge alone, with no area until the halves push apart
        let inside = hw * m0.dot(bevel);
        let middle = p + (m0 + m1) * (hw / 2.0);
        let dist = side * hw * (1.0 + cos) / 2.0;
        let half_measure = |push: Vec2| Measure {
          dist,
          half: hw,
          second: Some(0.0),
          round: None,
          push,
          limit: NO_EDGE,
          grows: side,
        };
        let centre = self.vertex(
          p,
          &point,
          [u, 0.5],
          Vec2::ZERO,
          Measure::centre(hw).with_second(inside),
        );
        let end = self.copy(outer_end, |v| set_second(v, 0.0));
        let half_end = self.vertex(middle, &point, [u, v_out], bisector, half_measure(push0));
        let half_start = self.vertex(middle, &point, [u, v_out], bisector, half_measure(push1));
        let start = self.copy(outer_start, |v| set_second(v, 0.0));
        let gap_centre = self.vertex(
          p,
          &point,
          [u, 0.5],
          Vec2::ZERO,
          Measure::second_only(inside, Vec2::ZERO, 0.0, 1.0),
        );
        let gap_end = self.vertex(
          middle,
          &point,
          [u, v_out],
          bisector,
          Measure::second_only(0.0, push0, NO_EDGE, side),
        );
        let gap_start = self.vertex(
          middle,
          &point,
          [u, v_out],
          bisector,
          Measure::second_only(0.0, push1, NO_EDGE, side),
        );
        self.wedge(side, centre, end, half_end);
        self.wedge(side, gap_centre, gap_end, gap_start);
        self.wedge(side, centre, half_start, start);
      }
      LineJoin::Round => {
        let centre = self.vertex(
          p,
          &point,
          [u, 0.5],
          Vec2::ZERO,
          Measure::round(hw, Vec2::ZERO, Vec2::ZERO, 0.0),
        );
        let angle = m0.angle_to(m1);
        let steps = ((angle.abs() / (std::f32::consts::PI / 16.0)).ceil() as u32).max(2);
        let mut previous = self.copy(outer_end, |v| set_round(v, m0 * hw));
        for k in 1..steps {
          let t = k as f32 / steps as f32;
          let outward = Vec2::from_angle(angle * t).rotate(m0);
          let across = Vec2::from_angle(angle * t).rotate(n0);
          let arc = self.vertex(
            p + outward * hw,
            &point,
            [u, v_out],
            across,
            Measure::round(hw, outward * hw, outward, NO_EDGE),
          );
          self.wedge(side, centre, previous, arc);
          previous = arc;
        }
        let last = self.copy(outer_start, |v| set_round(v, m1 * hw));
        self.wedge(side, centre, previous, last);
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

  /// A triangle of a corner's wedge (or a cap's fan), given as for a left
  /// turn: wound like the strip whichever way it turns (`side` is the
  /// outside).
  fn wedge(&mut self, side: f32, a: u32, b: u32, c: u32) {
    if side < 0.0 {
      self.triangle(a, b, c);
    } else {
      self.triangle(a, c, b);
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
    let halves = vec![width / 2.0; points.len()];
    build_tapered(points, &halves, join, cap, closed)
  }

  fn build_tapered(
    points: &[(f32, f32)],
    halves: &[f32],
    join: LineJoin,
    cap: LineCap,
    closed: bool,
  ) -> Triangles {
    let pts: Vec<PolyPoint> = points
      .iter()
      .zip(halves)
      .enumerate()
      .map(|(i, (&(x, y), &half_width))| PolyPoint {
        pos: Vec2::new(x, y),
        half_width,
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
    // Six triangles: butt caps cut the segment into two capped halves and
    // the fringe quad between them
    assert_eq!(t.polyline(&points, &style), 0..18);
  }

  #[test]
  fn no_triangle_lies_along_one_edge() {
    // Edge anti-aliasing reads `v` as distance from the centre: a triangle
    // whose corners all sit on one edge would fade away
    for join in [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round] {
      for cap in [LineCap::Butt, LineCap::Square, LineCap::Round] {
        let t = build(&L, 2.0, join, cap, false);
        for tri in t.indices.chunks(3) {
          let v: Vec<f32> = tri.iter().map(|&k| t.vertices[k as usize].uv[1]).collect();
          let one_edge = v.iter().all(|&v| v == 0.0) || v.iter().all(|&v| v == 1.0);
          assert!(!one_edge, "{join:?} {cap:?}: {v:?}");
        }
      }
    }
  }

  /// Each triangle's signed area (zero-area ones left out: the fringe
  /// triangles between copies at one place have none until pushed).
  fn windings(t: &Triangles) -> Vec<f32> {
    t.indices
      .chunks(3)
      .map(|tri| {
        let [a, b, c] = [0, 1, 2].map(|k| Vec2::from(t.vertices[tri[k] as usize].position));
        (b - a).perp_dot(c - a) / 2.0
      })
      .filter(|area| area.abs() > 1e-6)
      .collect()
  }

  #[test]
  fn every_triangle_winds_the_same_way() {
    // A segment whose inner edge ran backwards would be a twisted quad,
    // wound the other way
    let tight_u = [(0.0, 0.0), (1.5, 0.0), (1.5, 1.5), (0.0, 1.5)];
    let zigzag = [(0.0, 0.0), (1.0, 0.7), (2.0, 0.0), (3.0, 0.7), (4.0, 0.0)];
    let long_zigzag = [(0.0, 0.0), (10.0, 7.0), (20.0, 0.0), (30.0, 7.0)];
    let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    for join in [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round] {
      for cap in [LineCap::Butt, LineCap::Square, LineCap::Round] {
        let shapes: [(&[(f32, f32)], bool); 5] = [
          (&L, false),
          (&tight_u, false),
          (&zigzag, false),
          (&long_zigzag, false),
          (&square, true),
        ];
        for (points, closed) in shapes {
          let t = build(points, 2.0, join, cap, closed);
          let areas = windings(&t);
          assert!(
            areas.iter().all(|a| *a > 0.0) || areas.iter().all(|a| *a < 0.0),
            "{join:?} {cap:?} {points:?}: {areas:?}"
          );
        }
      }
    }
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

  #[test]
  fn a_vertex_packs_its_edges_into_108_bytes() {
    // Four distances and limits in one lane, the round offset and the push in
    // another, and the flags
    assert_eq!(std::mem::size_of::<Vertex2D>(), 108);
  }

  const JOINS: [LineJoin; 3] = [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round];
  const CAPS: [LineCap; 3] = [LineCap::Butt, LineCap::Square, LineCap::Round];

  /// A polyline to test: its points, their half widths, and whether it
  /// closes.
  struct Shape {
    points: Vec<(f32, f32)>,
    halves: Vec<f32>,
    closed: bool,
  }

  impl Shape {
    fn even(points: &[(f32, f32)], half: f32, closed: bool) -> Self {
      Self {
        points: points.to_vec(),
        halves: vec![half; points.len()],
        closed,
      }
    }

    fn tapered(points: &[(f32, f32)], halves: &[f32]) -> Self {
      Self {
        points: points.to_vec(),
        halves: halves.to_vec(),
        closed: false,
      }
    }

    fn build(&self, join: LineJoin, cap: LineCap) -> Triangles {
      build_tapered(&self.points, &self.halves, join, cap, self.closed)
    }

    fn at(&self, i: usize) -> Vec2 {
      let (x, y) = self.points[i % self.points.len()];
      Vec2::new(x, y)
    }

    fn half(&self, i: usize) -> f32 {
      self.halves[i % self.halves.len()]
    }

    fn even_width(&self) -> bool {
      self.halves.iter().all(|&half| half == self.halves[0])
    }

    /// Each segment's start point index
    fn segments(&self) -> Vec<usize> {
      let n = self.points.len();
      (0..if self.closed { n } else { n - 1 }).collect()
    }

    /// Each segment's centre line: a point on it and its normal
    fn centre_lines(&self) -> Vec<(Vec2, Vec2)> {
      self
        .segments()
        .into_iter()
        .map(|i| {
          let (a, b) = (self.at(i), self.at(i + 1));
          (a, (b - a).normalize().perp())
        })
        .collect()
    }

    /// Every line a second edge may lie on: a point on it and its outward
    /// normal. The segments' edges at each end's half width, the caps' and
    /// each corner's bevel, both ways.
    fn edge_lines(&self, cap: LineCap) -> Vec<(Vec2, Vec2)> {
      let mut lines = Vec::new();
      for i in self.segments() {
        let (a, b) = (self.at(i), self.at(i + 1));
        let n = (b - a).normalize().perp();
        for (p, hw) in [(a, self.half(i)), (b, self.half(i + 1))] {
          lines.push((p + n * hw, n));
          lines.push((p - n * hw, -n));
        }
      }
      let n = self.points.len();
      if !self.closed {
        let ends = [
          (
            self.at(0),
            self.half(0),
            (self.at(0) - self.at(1)).normalize(),
          ),
          (
            self.at(n - 1),
            self.half(n - 1),
            (self.at(n - 1) - self.at(n - 2)).normalize(),
          ),
        ];
        for (p, hw, out) in ends {
          let reach = if cap == LineCap::Square { hw } else { 0.0 };
          lines.push((p + out * reach, out));
        }
      }
      let corners = if self.closed { 0..n } else { 1..n - 1 };
      for i in corners {
        let p = self.at(i);
        let before = (p - self.at(i + n - 1)).normalize();
        let after = (self.at(i + 1) - p).normalize();
        for side in [1.0, -1.0] {
          let (m0, m1) = (side * before.perp(), side * after.perp());
          let normal = (m0 + m1).normalize_or(before);
          lines.push((p + m0 * self.half(i), normal));
        }
      }
      lines
    }
  }

  /// The shapes the attribute tests build: corners both ways, obtuse and
  /// sharp (an inner side that cannot share), a U-turn, closed outlines,
  /// single segments and tapered lines.
  fn shapes() -> Vec<Shape> {
    vec![
      Shape::even(&L, 1.0, false),
      Shape::even(&[(0.0, 0.0), (10.0, 0.0), (10.0, -10.0)], 1.0, false),
      Shape::even(
        &[(0.0, 0.0), (10.0, 3.0), (20.0, 0.0), (30.0, 3.0)],
        1.0,
        false,
      ),
      Shape::even(&[(0.0, 0.0), (10.0, 0.0), (2.0, 3.0)], 1.0, false),
      Shape::even(&[(0.0, 0.0), (10.0, 0.0), (0.0, 0.1)], 1.0, false),
      Shape::even(
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
        1.0,
        true,
      ),
      Shape::even(&[(0.0, 0.0), (10.0, 0.0), (5.0, 8.0)], 0.5, true),
      Shape::even(&[(1.0, 2.0), (7.0, 9.0)], 1.5, false),
      Shape::tapered(
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 12.0)],
        &[1.5, 1.0, 0.75, 0.25],
      ),
      Shape::tapered(&[(0.0, 0.0), (10.0, 0.0)], &[1.0, 0.0]),
    ]
  }

  /// A vertex's attributes, or their interpolation at a point of a triangle.
  #[derive(Clone, Copy, Debug)]
  struct Sample {
    pos: Vec2,
    dist: f32,
    half: f32,
    second: f32,
    round: Vec2,
  }

  impl Sample {
    fn of(v: &Vertex2D) -> Self {
      Self {
        pos: Vec2::from(v.position),
        dist: v.dist,
        half: v.half,
        second: v.second,
        round: Vec2::from(v.round),
      }
    }

    /// At a triangle's corners and its centroid
    fn over(tri: &[Vertex2D; 3]) -> [Self; 4] {
      let [a, b, c] = tri.map(|v| Self::of(&v));
      let centroid = Self {
        pos: (a.pos + b.pos + c.pos) / 3.0,
        dist: (a.dist + b.dist + c.dist) / 3.0,
        half: (a.half + b.half + c.half) / 3.0,
        second: (a.second + b.second + c.second) / 3.0,
        round: (a.round + b.round + c.round) / 3.0,
      };
      [a, b, c, centroid]
    }
  }

  fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-3 * a.abs().max(b.abs()).max(1.0)
  }

  /// What a triangle measures, found among its shape's lines and points:
  /// the centre line its `dist` is from, the line its `second` is inside
  /// and the point its `round` is from.
  #[derive(Debug, Default)]
  struct Fields {
    centre: Option<(Vec2, Vec2)>,
    second: Option<(Vec2, Vec2)>,
    corner: Option<Vec2>,
  }

  /// `pushed`: the fields must also give each pushed vertex's attributes
  /// one unit of push on (where a fringe triangle has no area, its corners
  /// alone fit more than one line).
  fn fields(
    shape: &Shape,
    cap: LineCap,
    tri: &[Vertex2D; 3],
    pushed: bool,
  ) -> Result<Fields, String> {
    let flags = tri[0].flags & !FLAG_GROWS_DOWN;
    if tri.iter().any(|v| v.flags & !FLAG_GROWS_DOWN != flags) {
      return Err("its vertices' flags differ".into());
    }
    let samples = Sample::over(tri);
    // Each pushed vertex one unit on, as the fringe would leave it
    let moved: Vec<Sample> = tri
      .iter()
      .filter(|v| pushed && v.push_limit > 0.0)
      .map(|v| {
        let grows = if v.flags & FLAG_GROWS_DOWN != 0 {
          -1.0
        } else {
          1.0
        };
        let s = Sample::of(v);
        Sample {
          pos: s.pos + Vec2::from(v.push),
          dist: s.dist + grows,
          second: s.second - 1.0,
          round: s.round + Vec2::from(v.push),
          ..s
        }
      })
      .collect();
    let mut found = Fields::default();
    if flags & FLAG_ROUND != 0 {
      let corner = (0..shape.points.len()).find(|&i| {
        let from = |s: &Sample| (s.pos - shape.at(i) - s.round).length() < 1e-3;
        samples
          .iter()
          .all(|s| from(s) && near(s.half, shape.half(i)))
          && moved.iter().all(from)
      });
      found.corner = Some(shape.at(corner.ok_or("round from no point")?));
      for v in tri.iter().filter(|v| pushed && v.push_limit > 0.0) {
        let round = Vec2::from(v.round);
        if !near((round + Vec2::from(v.push)).length(), round.length() + 1.0) {
          return Err("a round piece's push is not straight out".into());
        }
      }
    } else if samples.iter().all(|s| s.half == NO_EDGE) {
      if samples.iter().any(|s| s.dist != 0.0) || flags & FLAG_SECOND == 0 {
        return Err("bounded by nothing".into());
      }
    } else {
      found.centre = shape.centre_lines().into_iter().find(|&(a, n)| {
        samples
          .iter()
          .all(|s| near(s.dist, (s.pos - a).dot(n)) && s.dist.abs() <= s.half + 1e-3)
          && moved.iter().all(|s| near(s.dist, (s.pos - a).dot(n)))
      });
      found
        .centre
        .ok_or("dist from no centre line, or outside its half width")?;
      if shape.even_width() && samples.iter().any(|s| !near(s.half, shape.half(0))) {
        return Err("half is not the half width".into());
      }
    }
    if flags & FLAG_SECOND != 0 {
      found.second = shape.edge_lines(cap).into_iter().find(|&(q, m)| {
        samples
          .iter()
          .chain(&moved)
          .all(|s| near(s.second, (q - s.pos).dot(m)))
      });
      found.second.ok_or("second inside no edge")?;
    } else if samples.iter().any(|s| s.second != NO_EDGE) {
      return Err("a second edge without its flag".into());
    }
    Ok(found)
  }

  fn triangles_of(t: &Triangles) -> impl Iterator<Item = [Vertex2D; 3]> + '_ {
    t.indices
      .chunks(3)
      .map(|tri| [0, 1, 2].map(|k| t.vertices[tri[k] as usize]))
  }

  /// Every triangle of every shape, joined and capped every way, has fields
  /// `check` accepts.
  fn every_triangle(pushed: bool) {
    for (index, shape) in shapes().iter().enumerate() {
      for join in JOINS {
        for cap in CAPS {
          let t = shape.build(join, cap);
          assert!(!t.indices.is_empty());
          for tri in triangles_of(&t) {
            if let Err(why) = fields(shape, cap, &tri, pushed) {
              panic!("shape {index} {join:?} {cap:?}: {why}: {tri:#?}");
            }
            for v in tri.iter().filter(|v| v.push_limit == 0.0) {
              assert_eq!(Vec2::from(v.push), Vec2::ZERO, "shape {index}: {v:#?}");
            }
          }
        }
      }
    }
  }

  #[test]
  fn every_attribute_is_the_true_distance_over_its_triangle() {
    // Sampled at each triangle's corners and centroid, so each attribute is
    // the true distance wherever it is interpolated
    every_triangle(false);
  }

  #[test]
  fn a_push_of_one_unit_grows_every_distance_by_one() {
    // The same fields give each pushed vertex's distances one unit on: across
    // grown by one, inside its second edge one less, its round offset one
    // longer
    every_triangle(true);
  }

  /// Every vertex's position pushed by `p` units, as far as its limit lets
  /// it.
  fn pushed(t: &Triangles, p: f32) -> Vec<Vec2> {
    t.vertices
      .iter()
      .map(|v| Vec2::from(v.position) + Vec2::from(v.push) * p.min(v.push_limit))
      .collect()
  }

  /// Each triangle's corners at `positions`, left out where it has no area.
  fn solid_triangles(t: &Triangles, positions: &[Vec2]) -> Vec<[Vec2; 3]> {
    t.indices
      .chunks(3)
      .map(|tri| [0, 1, 2].map(|k| positions[tri[k] as usize]))
      .filter(|[a, b, c]| (b - a).perp_dot(c - a).abs() > 1e-6)
      .collect()
  }

  /// Whether `point` lies strictly inside the triangle, either winding
  fn strictly_inside([a, b, c]: &[Vec2; 3], point: Vec2) -> bool {
    let sides = [
      (b - a).perp_dot(point - a),
      (c - b).perp_dot(point - b),
      (a - c).perp_dot(point - c),
    ];
    sides.iter().all(|&s| s > 0.0) || sides.iter().all(|&s| s < 0.0)
  }

  fn distance_to_triangle(tri: &[Vec2; 3], point: Vec2) -> f32 {
    if strictly_inside(tri, point) {
      return 0.0;
    }
    (0..3)
      .map(|k| {
        let (a, b) = (tri[k], tri[(k + 1) % 3]);
        let t = ((point - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
        point.distance(a + (b - a) * t)
      })
      .fold(f32::INFINITY, f32::min)
  }

  /// A grid of sample points over a box, off any edge's exact place.
  struct Grid {
    origin: Vec2,
    step: f32,
    size: (usize, usize),
  }

  impl Grid {
    fn over(min: Vec2, max: Vec2, step: f32) -> Self {
      let origin = min - Vec2::splat(1.0) + Vec2::new(0.0123, 0.0371);
      let extent = (max - min + Vec2::splat(2.0)) / step;
      Self {
        origin,
        step,
        size: (extent.x.ceil() as usize, extent.y.ceil() as usize),
      }
    }

    fn point(&self, x: usize, y: usize) -> Vec2 {
      self.origin + Vec2::new(x as f32, y as f32) * self.step
    }

    /// The sample indices inside a box grown by `grow`
    fn cells(&self, tri: &[Vec2; 3], grow: f32) -> impl Iterator<Item = (usize, usize)> + use<> {
      let min = tri[0].min(tri[1]).min(tri[2]) - Vec2::splat(grow);
      let max = tri[0].max(tri[1]).max(tri[2]) + Vec2::splat(grow);
      let from = ((min - self.origin) / self.step).floor().max(Vec2::ZERO);
      let to = ((max - self.origin) / self.step).ceil();
      let to = (
        (to.x.max(0.0) as usize).min(self.size.0),
        (to.y.max(0.0) as usize).min(self.size.1),
      );
      let (fx, fy) = (from.x as usize, from.y as usize);
      (fy..to.1).flat_map(move |y| (fx..to.0).map(move |x| (x, y)))
    }
  }

  #[test]
  fn a_translucent_line_covers_no_pixel_twice_at_any_join() {
    // Pushed by `p`, the mesh covers every point within 0.9 p of the line as
    // drawn unpushed (no gap), and no point twice. Shapes whose inner sides
    // all share (a sharper corner overlaps on its inside, pushed or not)
    let shapes = [
      Shape::even(&L, 1.0, false),
      Shape::even(&[(0.0, 0.0), (10.0, 0.0), (10.0, -10.0)], 1.0, false),
      Shape::even(
        &[(0.0, 0.0), (10.0, 3.0), (20.0, 0.0), (30.0, 3.0)],
        1.0,
        false,
      ),
      Shape::even(
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
        1.0,
        true,
      ),
      Shape::even(&[(0.0, 0.0), (20.0, 0.0), (10.0, 16.0)], 1.0, true),
      Shape::even(&[(1.0, 2.0), (9.0, 7.0)], 1.5, false),
      Shape::tapered(
        &[(0.0, 0.0), (12.0, 0.0), (12.0, 12.0), (24.0, 14.0)],
        &[1.5, 1.2, 0.9, 0.6],
      ),
    ];
    for (index, shape) in shapes.iter().enumerate() {
      for join in JOINS {
        for cap in CAPS {
          let t = shape.build(join, cap);
          let rest: Vec<Vec2> = t.vertices.iter().map(|v| Vec2::from(v.position)).collect();
          let drawn = solid_triangles(&t, &rest);
          for p in [0.25, 0.5, 1.0, 2.0] {
            let moved = solid_triangles(&t, &pushed(&t, p));
            let (mut min, mut max) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
            for corner in moved.iter().flatten() {
              min = min.min(*corner);
              max = max.max(*corner);
            }
            let grid = Grid::over(min, max, 0.05);
            let mut covered = vec![0u8; grid.size.0 * grid.size.1];
            for tri in &moved {
              for (x, y) in grid.cells(tri, 0.0) {
                if strictly_inside(tri, grid.point(x, y)) {
                  covered[y * grid.size.0 + x] += 1;
                }
              }
            }
            let mut close = vec![false; covered.len()];
            for tri in &drawn {
              for (x, y) in grid.cells(tri, p) {
                if distance_to_triangle(tri, grid.point(x, y)) < 0.9 * p {
                  close[y * grid.size.0 + x] = true;
                }
              }
            }
            for y in 0..grid.size.1 {
              for x in 0..grid.size.0 {
                let (count, near_line) = (covered[y * grid.size.0 + x], close[y * grid.size.0 + x]);
                let what = format!(
                  "shape {index} {join:?} {cap:?} p {p} at {}",
                  grid.point(x, y)
                );
                assert!(count <= 1, "covered {count} times: {what}");
                assert!(count == 1 || !near_line, "a gap: {what}");
              }
            }
          }
        }
      }
    }
  }

  #[test]
  fn a_pushed_inner_vertex_never_folds_its_quad() {
    // A 2-unit line with 4-unit segments, at every sharpness, pushed as far
    // as zooming out a hundredfold would push it: no triangle turns over
    let mut cases: Vec<(Vec<(f32, f32)>, bool)> = Vec::new();
    for degrees in [
      10.0f32, 30.0, 45.0, 60.0, 90.0, 120.0, 135.0, 150.0, 165.0, 175.0,
    ] {
      for back in [false, true] {
        let turn = degrees.to_radians();
        let d1 = Vec2::from_angle(turn);
        let d2 = Vec2::from_angle(if back { 0.0 } else { 2.0 * turn });
        let p1 = Vec2::X * 4.0;
        let p2 = p1 + d1 * 4.0;
        let p3 = p2 + d2 * 4.0;
        let points = [Vec2::ZERO, p1, p2, p3].map(|p| (p.x, p.y)).to_vec();
        cases.push((points, false));
      }
    }
    for sides in [3, 4, 5] {
      let turn = std::f32::consts::TAU / sides as f32;
      let mut at = Vec2::ZERO;
      let points = (0..sides)
        .map(|k| {
          let point = (at.x, at.y);
          at += Vec2::from_angle(turn * k as f32) * 4.0;
          point
        })
        .collect();
      cases.push((points, true));
    }
    for (points, closed) in &cases {
      for join in JOINS {
        for cap in CAPS {
          let t = build(points, 2.0, join, cap, *closed);
          let rest: Vec<Vec2> = t.vertices.iter().map(|v| Vec2::from(v.position)).collect();
          let wound = |positions: &[Vec2]| -> Vec<f32> {
            t.indices
              .chunks(3)
              .map(|tri| [0, 1, 2].map(|k| positions[tri[k] as usize]))
              .filter_map(|[a, b, c]| {
                let area = (b - a).perp_dot(c - a);
                let longest = (b - a).length().max((c - b).length()).max((a - c).length());
                (area.abs() > 1e-5 * longest * longest).then_some(area)
              })
              .collect()
          };
          let sign = wound(&rest)[0].signum();
          let area_at_rest: f32 = wound(&rest).iter().map(|area| area.abs()).sum();
          for p in [0.5, 1.0, 2.0, 4.0, 16.0, 100.0] {
            let positions = pushed(&t, p);
            assert!(positions.iter().all(|at| at.is_finite()));
            let areas = wound(&positions);
            assert!(
              areas.iter().all(|area| area.signum() == sign),
              "{join:?} {cap:?} {points:?} p {p}: {areas:?}"
            );
            // And the fringe does grow it
            let area: f32 = areas.iter().map(|area| area.abs()).sum();
            assert!(area > area_at_rest, "{join:?} {cap:?} {points:?} p {p}");
          }
        }
      }
    }
  }
}
