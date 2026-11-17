//! Lines and trails as triangles: a strip along the points, a wedge on the
//! outside of every corner and a cap at each open end.
//!
//! Every vertex carries its distances to the line's edges, in the line's
//! units, and how far a fringe may push it outward; the builder does not know
//! the camera. What a line's vertices share (its style and effect
//! parameters, the atlas slot a textured line repeats, its emission, a
//! two-point capped line's half length) is its entry in the line table
//! (`LineInfo`), which each vertex names.
//!
//! - **A vertex is 48 bytes** (`a_vertex_packs_its_edges_into_48_bytes`):
//!   place and uv, an sRGB colour (decoded by the vertex stage, which
//!   multiplies it by the line's emission), its three distances (a round
//!   piece's offset sharing two of them), its push, the push's limit and the
//!   rates its distances move at in halves, its angle across, and its line,
//!   flags and `k` in one word.
//! - **Distances are true over every triangle.** Each triangle belongs to
//!   one straight piece and measures across from its centre line (`dist`,
//!   with `half` the half width there), a second edge where it lies against
//!   two (`second`: the other segment's outer edge in a miter's point, the
//!   bevel's edge in a bevel's fans, a butt or square cap's edge in its
//!   segment's quad, or, on a two-point capped line, its distance along from
//!   the middle between its caps, a strip of the line's `along_half`), or,
//!   in a round piece, its offset from the corner point. Every one is an
//!   affine function of position over its triangle, so it is exact wherever
//!   it is interpolated (`every_attribute_is_the_true_distance_over_its_triangle`).
//!   A vertex is shared only between triangles that measure the same;
//!   elsewhere a piece has its own copy at the same place, pushed the same
//!   way: a miter's point, a bevel's fans, a round join's arc and a capped
//!   segment's far end each have their own vertices, a round cap its own
//!   copies of the strip's end vertices.
//! - **A pushed vertex measures where it lands**
//!   (`a_pushed_vertex_measures_where_it_lands`): pushed `a` units, `dist`
//!   rises by `dist_rate × a`, `second` falls by `second_rate × a` and a
//!   round offset grows by the push, each rate worked out from the push as
//!   stored, so the pushed triangle's distances stay true. A vertex on two
//!   edges pushes along their miter (a miter's point and the strip's outer
//!   vertices beside it, a bevel's elbows, a cap's corners), so both rise by
//!   one unit per unit; a bevel's middle pushes along the bevel's normal,
//!   its distance across rising by `m · bevel`.
//! - **Two edges blend by their angle.** Each vertex of a triangle with a
//!   second edge carries `k`, the dot product of the two edges' outward
//!   normals (`cos` of the turn in a miter's point, `m · bevel` in a bevel's
//!   fans, zero beside a cap), for the coverage model's blend
//!   (`coverage.rs`). Pushed one pixel, the mesh gives every pixel the
//!   model's coverage within 0.12 of the exact area of the line drawn
//!   unpushed, at every join, cap and closed corner, at half widths of 0.75
//!   to 6 pixels, turns of 5 to 165 degrees, 8 directions and 4 sub-pixel
//!   places (`the_pushed_mesh_gives_every_pixel_its_coverage`; worst 0.105
//!   at round joins, 0.089 at bevels, 0.086 at miters, 0.072 at caps).
//! - **A bevel is a fan each side.** From the corner point through copies of
//!   the shared inner vertex (when the segments share it) and of the strip's
//!   elbow, to the bevel's middle, which both fans share: every triangle
//!   touching the elbow measures its segment across and the bevel's edge.
//!   The strip's elbow pushes along the miter of its edge and the bevel, so
//!   pushed, the strip quad's corner reaches past the bevel's edge by the
//!   push, where only its strip is measured: that is in the bevels' 0.089
//!   (at 90 degrees; a slide along the bevel's edge would keep it inside,
//!   0.055 there, for two more vertices a bevel and a limit that runs short
//!   at shallow bevels).
//! - **A capped segment measures its cap over its whole quad**, through
//!   copies of the pair it ends on at its far end; a two-point capped line
//!   measures both caps as one strip along it. Nothing is cut.
//! - **Pushed, the mesh stays watertight and covers nothing twice**
//!   (`a_translucent_line_covers_no_pixel_twice_at_any_join`). Copies at one
//!   place push alike. An unshared inner vertex pushes opposite its outer
//!   one, so their end keeps passing through the corner point the wedge fans
//!   from. A sharp corner's inner side still overlaps, as it does unpushed.
//! - **A pushed quad never folds** (`a_pushed_inner_vertex_never_folds_its_quad`).
//!   Every vertex of a segment's quad, a cap's corners included, pushes at
//!   most the quad's limit, the least its corners allow: a shared inner
//!   vertex until it would pass the shorter segment's middle, and a push
//!   that runs along the segment (a miter's or a bevel's) until it has run
//!   half the segment's length. Limits are stored 2⁻¹⁰ short of themselves
//!   and rounded to halves, so the halves' rounding of the pushes never
//!   takes two ends past each other. Vertices never pushed have a limit of
//!   zero.
//! - **Limit: the inner corner.** Within a pixel of a shared inner vertex,
//!   or near it within a pixel of both inner edges, a pixel lies across both
//!   segments' strips, and one triangle's distances cannot give their union:
//!   the coverage there is off by up to 0.16 at a 60 degree turn, 0.26 at
//!   90, 0.34 at 120, 0.39 at 150 and 0.45 at 165, whatever the join
//!   (measured by the coverage test, which holds it to 0.45).
//! - **Limit:** a push rounds to halves, off by up to 2⁻¹¹ of its length: a
//!   two thousandths of a unit per unit at the miter limit's tip; the shared inner
//!   vertex of a sharp corner pushes further, inside the inner corner.
//! - **Limit:** on a tapered line a push that runs along the segment keeps
//!   the vertex's `half`, which the taper would change by its slope times
//!   that run.

use std::ops::Range;

use glam::Vec2;
use half::f16;

use crate::core::trail::{LineCap, LineJoin};
use crate::core::ui::Color;

/// One vertex of the triangle stream: 48 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Vertex2D {
  /// World position
  pub(crate) position: [f32; 2],
  /// Along the line (`u`) and across it (`v`, 0 on one edge, 1 on the other)
  pub(crate) uv: [f32; 2],
  /// sRGB colour, straight alpha; the vertex stage decodes it to linear
  /// light and multiplies it by its line's emission
  pub(crate) color: [u8; 4],
  /// Signed distance across from the centre line of its triangle's straight
  /// piece, in the line's units (positive on the left, by the normal); in a
  /// round piece, the x of its offset from its corner point
  pub(crate) dist: f32,
  /// The distance inside a second edge (the other outer edge in a miter's
  /// point, the bevel's edge, a cap's edge), or a two-point capped line's
  /// distance along from the middle between its caps, or `NO_EDGE`; in a
  /// round piece, the y of its offset from its corner point
  pub(crate) second: f32,
  /// The half width there (`NO_EDGE` where only the second edge bounds it)
  pub(crate) half: f32,
  /// The way the fringe pushes it, in the line's units per unit of push
  pub(crate) push: [f16; 2],
  /// The most the fringe may push it (rounded down)
  pub(crate) limit: f16,
  /// How much `dist` rises per unit of push
  pub(crate) dist_rate: f16,
  /// How much `second` falls per unit of push
  pub(crate) second_rate: f16,
  /// The unit perpendicular displacement effects move the vertex along, as
  /// a sixteen-bit angle (`NO_ACROSS`: none)
  pub(crate) across: u16,
  /// Its line's entry in the line table (the low 20 bits), its flags (4) and
  /// `k`, the dot product of its two edges' outward normals (8, signed, in
  /// 127ths)
  pub(crate) packed: u32,
}

/// A vertex of a round piece: coverage by the length of its offset
pub(crate) const FLAG_ROUND: u32 = 1;
/// A vertex whose `second` edge distance counts
pub(crate) const FLAG_SECOND: u32 = 2;
/// A vertex whose `second` is a distance along a two-point capped line from
/// the middle between its caps: a strip of its line's `along_half`
pub(crate) const FLAG_ALONG: u32 = 4;
/// No edge: the `second` of a vertex without one and the `half` of one that
/// only a second edge bounds
pub(crate) const NO_EDGE: f32 = 1.0e6;
/// The line table holds this many lines a frame; the rest draw nothing
pub(crate) const MAX_LINES: usize = 1 << LINE_BITS;
const LINE_BITS: u32 = 20;
const LINE_MASK: u32 = (1 << LINE_BITS) - 1;
const FLAG_SHIFT: u32 = 20;
const K_SHIFT: u32 = 24;
/// The `across` of a vertex effects leave where it is
const NO_ACROSS: u16 = u16::MAX;

impl Vertex2D {
  /// Its line's entry in the line table
  pub(crate) fn line(&self) -> usize {
    (self.packed & LINE_MASK) as usize
  }

  /// `FLAG_ROUND`, `FLAG_SECOND` and `FLAG_ALONG`
  pub(crate) fn flags(&self) -> u32 {
    (self.packed >> FLAG_SHIFT) & 0xF
  }

  /// The dot product of its two edges' outward normals
  #[cfg(test)]
  pub(crate) fn k(&self) -> f32 {
    f32::from((self.packed >> K_SHIFT) as u8 as i8) / 127.0
  }

  pub(crate) fn push(&self) -> Vec2 {
    Vec2::new(self.push[0].to_f32(), self.push[1].to_f32())
  }
}

/// A vertex's `across`: a unit direction as a sixteen-bit angle, worked out
/// once per segment or corner and turned for the vertices of an arc.
#[derive(Clone, Copy)]
struct Across(u16);

impl Across {
  const NONE: Self = Self(NO_ACROSS);

  fn of(direction: Vec2) -> Self {
    if direction == Vec2::ZERO || !direction.is_finite() {
      return Self::NONE;
    }
    Self::from_angle(direction.y.atan2(direction.x))
  }

  /// This direction turned by `angle` radians
  fn turned(self, angle: f32) -> Self {
    if self.0 == NO_ACROSS {
      return self;
    }
    Self::from_angle(f32::from(self.0) / ACROSS_STEPS * std::f32::consts::TAU + angle)
  }

  /// The direction `angle` radians from the x axis
  fn from_angle(angle: f32) -> Self {
    let steps = (angle * (ACROSS_STEPS / std::f32::consts::TAU)).round() as i32;
    Self(steps.rem_euclid(ACROSS_STEPS as i32) as u16)
  }
}

/// The steps of a whole turn an `across` angle counts
const ACROSS_STEPS: f32 = 65535.0;

/// `limit` in half precision, never above it, so a push never passes it:
/// rounding to the nearest half moves a value by at most 2⁻¹¹ of itself, so
/// a value first made 2⁻¹⁰ smaller rounds to at most what it was
fn limit_of(limit: f32) -> f16 {
  if limit.is_nan() || limit <= 0.0 {
    return f16::ZERO;
  }
  f16::from_f32((limit * (1.0 - 1.0 / 1024.0)).min(f16::MAX.to_f32()))
}

/// `k` in 127ths
fn k_code(k: f32) -> u32 {
  u32::from((k.clamp(-1.0, 1.0) * 127.0).round() as i8 as u8)
}

/// What every vertex of one line shares: an entry of the line table, which
/// the vertex stage reads by the line's number in each vertex.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LineInfo {
  /// Style and effect parameters (effect and style bits in `[0]`)
  pub(crate) params: [f32; 4],
  /// The atlas slot a textured line repeats (UV min and max), set when it
  /// is placed; zero for untextured ones
  pub(crate) bounds: [f32; 4],
  /// What its colours' light is multiplied by
  pub(crate) emission: f32,
  /// A two-point capped line's half length between its caps (zero
  /// elsewhere): the half width of the strip its `FLAG_ALONG` vertices
  /// measure
  pub(crate) along_half: f32,
  pub(crate) _pad: [f32; 2],
}

/// A point of a polyline.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PolyPoint {
  pub(crate) pos: Vec2,
  pub(crate) half_width: f32,
  /// sRGB, straight alpha
  pub(crate) color: Color,
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
  /// Its line table entry's (effect and style bits in `[0]`)
  pub(crate) params: [f32; 4],
  /// What its colours' light is multiplied by
  pub(crate) emission: f32,
}

/// A miter longer than this many times the line's width becomes a bevel.
pub(crate) const MITER_LIMIT: f32 = 4.0;

/// Triangles built on the CPU for the batch's triangle stream, and the line
/// table their vertices name.
#[derive(Default)]
pub(crate) struct Triangles {
  pub(crate) vertices: Vec<Vertex2D>,
  pub(crate) indices: Vec<u32>,
  pub(crate) lines: Vec<LineInfo>,
  /// Room the builder keeps from line to line
  scratch: Scratch,
  /// Whether the log has been told the line table is full
  warned: bool,
}

#[derive(Default)]
struct Scratch {
  points: Vec<PolyPoint>,
  segments: Vec<Segment>,
  turns: Vec<Option<Turn>>,
  joints: Vec<Joint>,
}

impl Triangles {
  /// Forget every triangle and line, keeping the allocations.
  pub(crate) fn clear(&mut self) {
    self.vertices.clear();
    self.indices.clear();
    self.lines.clear();
  }

  /// Add the triangles of a polyline through `points`, and its line table
  /// entry; the index range they take (empty when nothing is drawn).
  ///
  /// Non-finite points are skipped and points closer than `1e-4` to the one
  /// before collapse. Each segment is a quad sharing its edge vertices with
  /// its neighbours; each corner adds one wedge on its outside, so no area
  /// is covered twice (a corner too sharp to share its inner vertex
  /// overlaps on the inside instead of spiking).
  pub(crate) fn polyline(&mut self, points: &[PolyPoint], style: &PolyStyle) -> Range<u32> {
    let start = self.indices.len() as u32;
    let mut scratch = std::mem::take(&mut self.scratch);
    let drawn = self.build(points, style, &mut scratch);
    self.scratch = scratch;
    if drawn {
      start..self.indices.len() as u32
    } else {
      start..start
    }
  }

  fn build(&mut self, points: &[PolyPoint], style: &PolyStyle, scratch: &mut Scratch) -> bool {
    let Scratch {
      points: pts,
      segments,
      turns,
      joints,
    } = scratch;
    pts.clear();
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
    if closed
      && pts.len() > 2
      && let (Some(first), Some(last)) = (pts.first(), pts.last())
      && first.pos.distance(last.pos) < 1e-4
    {
      pts.pop();
    }
    if pts.len() < 3 {
      closed = false;
    }
    if pts.len() < 2 || pts.iter().all(|point| point.half_width <= 0.0) {
      return false;
    }
    if self.lines.len() >= MAX_LINES {
      if !self.warned {
        self.warned = true;
        log::warn!(
          "more than {MAX_LINES} lines and trails in one frame: the line table is full, so the rest draw nothing"
        );
      }
      return false;
    }
    let line = self.lines.len() as u32;
    self.lines.push(LineInfo {
      params: style.params,
      bounds: [0.0; 4],
      emission: style.emission,
      along_half: 0.0,
      _pad: [0.0; 2],
    });
    let mut builder = Builder {
      out: self,
      pts,
      style,
      line,
    };
    builder.build(closed, segments, turns, joints);
    true
  }

  /// Vertex `index` as a corner of a UI line's triangle, with its line's
  /// parameters.
  pub(crate) fn corner(&self, index: u32) -> Option<(Corner, [f32; 4])> {
    let vertex = self.vertices.get(index as usize)?;
    let line = self.lines.get(vertex.line())?;
    let [r, g, b, a] = vertex.color;
    let [r, g, b, a] = Color::new(r, g, b, a).to_linear();
    let e = line.emission;
    let corner = (
      Vec2::from(vertex.position),
      Vec2::from(vertex.uv),
      [r * e, g * e, b * e, a],
    );
    Some((corner, line.params))
  }
}

/// A corner of a UI line's triangle: its place, its uv and its colour in
/// linear light.
pub(crate) type Corner = (Vec2, Vec2, [f32; 4]);

/// A segment's direction and left-hand normal.
#[derive(Clone, Copy)]
struct Segment {
  dir: Vec2,
  normal: Vec2,
  /// The normal as its vertices' `across`
  across: Across,
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
      across: Across::of(dir.perp()),
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

/// A second edge a vertex measures.
#[derive(Clone, Copy)]
enum Bound {
  /// The distance inside an edge through `point` with outward normal `out`
  Edge { point: Vec2, out: Vec2 },
  /// The signed distance along `dir` from `mid`, the middle between a
  /// two-point line's caps
  Along { mid: Vec2, dir: Vec2 },
}

#[derive(Clone, Copy)]
struct Second {
  bound: Bound,
  /// The dot product of the outward normals of this edge and the strip's
  /// edge it meets
  k: f32,
}

impl Second {
  fn edge(point: Vec2, out: Vec2, k: f32) -> Self {
    Self {
      bound: Bound::Edge { point, out },
      k,
    }
  }

  /// What it measures at `at`
  fn at(&self, at: Vec2) -> f32 {
    match self.bound {
      Bound::Edge { point, out } => (point - at).dot(out),
      Bound::Along { mid, dir } => (at - mid).dot(dir),
    }
  }

  /// How much what it measures falls per unit of `push`
  fn rate(&self, push: Vec2) -> f32 {
    match self.bound {
      Bound::Edge { out, .. } => push.dot(out),
      Bound::Along { dir, .. } => -push.dot(dir),
    }
  }

  fn flags(&self) -> u32 {
    match self.bound {
      Bound::Edge { .. } => FLAG_SECOND,
      Bound::Along { .. } => FLAG_SECOND | FLAG_ALONG,
    }
  }
}

/// What a vertex measures, in the line's units, and how the fringe pushes
/// it.
#[derive(Clone, Copy)]
struct Measure {
  dist: f32,
  half: f32,
  /// The normal of the centre line `dist` is measured from, which says how
  /// it rises as the vertex is pushed
  normal: Vec2,
  second: Option<Second>,
  round: Option<Vec2>,
  push: Vec2,
  limit: f32,
}

impl Measure {
  /// On the edge of a strip of half width `half`, on the `side` (1 left, -1
  /// right) of its centre line of normal `normal`, pushed along `push` by at
  /// most `limit`.
  fn edge(side: f32, half: f32, normal: Vec2, push: Vec2, limit: f32) -> Self {
    Self {
      dist: side * half,
      half,
      normal,
      second: None,
      round: None,
      push,
      limit,
    }
  }

  /// A corner point, never pushed.
  fn centre(half: f32) -> Self {
    Self::edge(0.0, half, Vec2::ZERO, Vec2::ZERO, 0.0)
  }

  /// A vertex of a round piece, `offset` from its corner point.
  fn round(half: f32, offset: Vec2, push: Vec2, limit: f32) -> Self {
    Self {
      round: Some(offset),
      ..Self::edge(0.0, half, Vec2::ZERO, push, limit)
    }
  }

  fn with(self, second: Option<Second>) -> Self {
    Self { second, ..self }
  }
}

/// `v` measuring `second` where it is, as it is pushed.
fn measure_second(v: &mut Vertex2D, second: Second) {
  v.second = second.at(Vec2::from(v.position));
  v.second_rate = f16::from_f32(second.rate(v.push()));
  let flags = (v.flags() & !FLAG_ALONG) | second.flags();
  v.packed = (v.packed & LINE_MASK) | flags << FLAG_SHIFT | k_code(second.k) << K_SHIFT;
}

/// `v` as a vertex of a round piece, `offset` from its corner point.
fn set_round(v: &mut Vertex2D, offset: Vec2) {
  v.dist = offset.x;
  v.second = offset.y;
  v.dist_rate = f16::ZERO;
  v.second_rate = f16::ZERO;
  v.packed = (v.packed & LINE_MASK) | FLAG_ROUND << FLAG_SHIFT;
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
  /// The bisector of the normals, as the `across` of the vertices on it
  bisector: Across,
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
    // the bevel's, which the bevel's fans measure; a round's straight out
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
      bisector: Across::of((n0 + n1).normalize_or(n0)),
      shares_inside,
      mitered,
      bevel,
      push,
      inner_limit,
    })
  }

  /// The most a vertex of the quad beside it, along `segment`, may push for
  /// this corner: until a push along the miter or a bevel's, which also runs
  /// along the segment, has run half the segment's length, and no further
  /// than the shared inner vertex may.
  fn limit_on(&self, push: Vec2, segment: Segment) -> f32 {
    let along = push.dot(segment.dir).abs();
    let runs = if along > 1e-6 {
      (segment.len / (2.0 * along)).min(NO_EDGE)
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
  /// Its line's entry in the line table
  line: u32,
}

impl Builder<'_> {
  fn build(
    &mut self,
    closed: bool,
    segments: &mut Vec<Segment>,
    turns: &mut Vec<Option<Turn>>,
    joints: &mut Vec<Joint>,
  ) {
    let pts = self.pts;
    let n = pts.len();
    let count = if closed { n } else { n - 1 };
    segments.clear();
    segments.extend((0..count).map(|i| {
      let (a, b) = (pts[i].pos, pts[(i + 1) % n].pos);
      Segment::between(a, b)
    }));
    // Each corner's turn, worked out once; the segments either side of
    // point `i` are `before(i)` and `i % count`
    let before = |i: usize| (i + count - 1) % count;
    turns.clear();
    for (i, point) in pts.iter().enumerate() {
      let corner = closed || (i > 0 && i < n - 1);
      let turn = match (corner, segments.get(before(i)), segments.get(i % count)) {
        (true, Some(&b), Some(&a)) => Turn::at(b, a, point.half_width, self.style.join),
        _ => None,
      };
      turns.push(turn);
    }
    // Every vertex of a segment's quad pushes as far as the least its
    // corners allow, so the quad grows evenly and never folds
    for (i, turn) in turns.iter().enumerate() {
      let Some(turn) = turn else {
        continue;
      };
      let (b, a) = (before(i), i % count);
      let (Some(&seg_b), Some(&seg_a)) = (segments.get(b), segments.get(a)) else {
        continue;
      };
      let (limit_b, limit_a) = (
        turn.limit_on(turn.push.0, seg_b),
        turn.limit_on(turn.push.1, seg_a),
      );
      if let Some(segment) = segments.get_mut(b) {
        segment.limit = segment.limit.min(limit_b);
      }
      if let Some(segment) = segments.get_mut(a) {
        segment.limit = segment.limit.min(limit_a);
      }
    }
    // A butt or square cap's edge, which the quad of its segment measures
    // over its whole length; a two-point line measures both as one strip
    // along it
    let capped = !closed && matches!(self.style.cap, LineCap::Butt | LineCap::Square);
    let (Some(&first), Some(&last)) = (segments.first(), segments.last()) else {
      return;
    };
    let reach = |point: &PolyPoint| {
      if self.style.cap == LineCap::Square {
        point.half_width
      } else {
        0.0
      }
    };
    let (start_cap, end_cap) = match (capped, pts.first(), pts.last()) {
      (true, Some(p0), Some(p1)) if count == 1 => {
        let (r0, r1) = (reach(p0), reach(p1));
        let mid = (p0.pos - first.dir * r0 + p1.pos + first.dir * r1) / 2.0;
        if let Some(info) = self.out.lines.get_mut(self.line as usize) {
          info.along_half = (first.len + r0 + r1) / 2.0;
        }
        let along = Second {
          bound: Bound::Along {
            mid,
            dir: first.dir,
          },
          k: 0.0,
        };
        (Some(along), Some(along))
      }
      (true, Some(p0), Some(p1)) => (
        Some(Second::edge(
          p0.pos - first.dir * reach(p0),
          -first.dir,
          0.0,
        )),
        Some(Second::edge(p1.pos + last.dir * reach(p1), last.dir, 0.0)),
      ),
      _ => (None, None),
    };
    joints.clear();
    for (i, point) in pts.iter().enumerate() {
      let joint = match (closed, i) {
        (false, 0) => self.end_pair(point, first, start_cap, None),
        (false, _) if i == n - 1 => {
          let back = pts.get(n - 2).map(|p| self.u_scale(p, point));
          self.end_pair(point, last, end_cap, back)
        }
        _ => {
          let (Some(&b), Some(&a), Some(turn)) = (
            segments.get(before(i)),
            segments.get(i % count),
            turns.get(i).copied(),
          ) else {
            continue;
          };
          // The seam: the closing segment ends on the first corner, with
          // `u` running on past the last point
          let end_u = match (closed, i, pts.get(n - 2), pts.last()) {
            (true, 0, Some(a), Some(z)) => Some(z.u + b.len * self.u_scale(a, z)),
            _ => None,
          };
          self.corner(point, b, a, turn, end_u)
        }
      };
      joints.push(joint);
    }
    if closed && let Some(&first) = joints.first() {
      joints.push(first);
    }
    for i in 0..count {
      let (Some(from), Some(to)) = (joints.get(i), joints.get(i + 1)) else {
        continue;
      };
      let (mut from, mut to) = (from.start, to.end);
      // A capped segment's far end measures its cap too, with copies of the
      // pair it ends on there, at their places and pushed as they are
      if count > 1 {
        if let (0, Some(cap)) = (i, start_cap) {
          to = self.measured_pair(to, cap);
        }
        if let Some(cap) = end_cap
          && i == count - 1
        {
          from = self.measured_pair(from, cap);
        }
      }
      self.quad(from, to);
    }
  }

  /// `u` per world unit along the segment from `a` to `b`.
  fn u_scale(&self, a: &PolyPoint, b: &PolyPoint) -> f32 {
    let len = a.pos.distance(b.pos);
    if len > 0.0 { (b.u - a.u) / len } else { 0.0 }
  }

  fn vertex(
    &mut self,
    at: Vec2,
    point: &PolyPoint,
    uv: [f32; 2],
    across: Across,
    m: Measure,
  ) -> u32 {
    let index = self.out.vertices.len() as u32;
    let push = [f16::from_f32(m.push.x), f16::from_f32(m.push.y)];
    let Color { r, g, b, a } = point.color;
    let mut vertex = Vertex2D {
      position: at.into(),
      uv,
      color: [r, g, b, a],
      dist: m.dist,
      second: NO_EDGE,
      half: m.half,
      push,
      limit: limit_of(m.limit),
      dist_rate: f16::ZERO,
      second_rate: f16::ZERO,
      across: across.0,
      packed: self.line,
    };
    vertex.dist_rate = f16::from_f32(m.push.dot(m.normal));
    if let Some(offset) = m.round {
      set_round(&mut vertex, offset);
    } else if let Some(second) = m.second {
      measure_second(&mut vertex, second);
    }
    self.out.vertices.push(vertex);
    index
  }

  /// A copy of vertex `index`, at its place and pushed as it is, measuring
  /// what `edit` makes it measure.
  fn copy(&mut self, index: u32, edit: impl FnOnce(&mut Vertex2D)) -> u32 {
    let Some(mut vertex) = self.out.vertices.get(index as usize).copied() else {
      return index;
    };
    edit(&mut vertex);
    self.out.vertices.push(vertex);
    self.out.vertices.len() as u32 - 1
  }

  /// Copies of an edge pair measuring `second` too.
  fn measured_pair(&mut self, (left, right): (u32, u32), second: Second) -> (u32, u32) {
    (
      self.copy(left, |v| measure_second(v, second)),
      self.copy(right, |v| measure_second(v, second)),
    )
  }

  /// The edge vertices at an open end, with its cap: `cap` is the second
  /// edge a butt or square cap's vertices measure, and `back` how far `u`
  /// runs per world unit into the last point (`None` at the first).
  fn end_pair(
    &mut self,
    point: &PolyPoint,
    segment: Segment,
    cap: Option<Second>,
    back: Option<f32>,
  ) -> Joint {
    let (p, hw, n, u) = (point.pos, point.half_width, segment.normal, point.u);
    let across = segment.across;
    let first = back.is_none();
    // Outward from the line, and how far `u` runs per world unit outward
    let (out, scale) = match back {
      None => {
        let next = self
          .pts
          .get(1)
          .map_or(0.0, |next| self.u_scale(point, next));
        (-segment.dir, -next)
      }
      Some(back) => (segment.dir, back),
    };
    // A cap's corners push out across both of their edges, as far as the
    // rest of their segment's quad may, so it never folds
    let corner = |side: f32| Measure::edge(side, hw, n, side * n + out, segment.limit).with(cap);
    let (left, right) = match self.style.cap {
      LineCap::Butt => (
        self.vertex(p + n * hw, point, [u, 0.0], across, corner(1.0)),
        self.vertex(p - n * hw, point, [u, 1.0], across, corner(-1.0)),
      ),
      LineCap::Square => {
        let left = self.vertex(p + n * hw, point, [u, 0.0], across, corner(1.0));
        let right = self.vertex(p - n * hw, point, [u, 1.0], across, corner(-1.0));
        let far_u = u + hw * scale;
        let far_left = self.vertex(
          p + n * hw + out * hw,
          point,
          [far_u, 0.0],
          across,
          corner(1.0),
        );
        let far_right = self.vertex(
          p - n * hw + out * hw,
          point,
          [far_u, 1.0],
          across,
          corner(-1.0),
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
        let (left, right) = self.straight_pair(point, (n, across), u, segment.limit);
        // The fan measures its distance from the end point, so it has its
        // own copies of the strip's end vertices, at their places and pushed
        // as they are
        let centre = self.vertex(
          p,
          point,
          [u, 0.5],
          Across::NONE,
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
            point,
            uv,
            across,
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

  /// A left and right vertex at `point`, `n` (unit, with its `across`) and
  /// `-n` times its half width from it, pushed straight out by at most
  /// `limit`.
  fn straight_pair(
    &mut self,
    point: &PolyPoint,
    (n, across): (Vec2, Across),
    u: f32,
    limit: f32,
  ) -> (u32, u32) {
    let (p, hw) = (point.pos, point.half_width);
    (
      self.vertex(
        p + n * hw,
        point,
        [u, 0.0],
        across,
        Measure::edge(1.0, hw, n, n, limit),
      ),
      self.vertex(
        p - n * hw,
        point,
        [u, 1.0],
        across,
        Measure::edge(-1.0, hw, n, -n, limit),
      ),
    )
  }

  /// The vertices where `before` meets `after` at `point`, with the join's
  /// wedge. `end_u` gives the ending side a `u` of its own (the closed
  /// seam), so nothing is shared across it.
  fn corner(
    &mut self,
    point: &PolyPoint,
    before: Segment,
    after: Segment,
    turn: Option<Turn>,
    end_u: Option<f32>,
  ) -> Joint {
    let (p, hw, u) = (point.pos, point.half_width, point.u);
    let (n0, n1) = (before.normal, after.normal);
    let u_end = end_u.unwrap_or(u);
    // Each segment's vertices push as far as its quad's limit; a vertex both
    // quads share, as far as both allow
    let (limit0, limit1) = (before.limit, after.limit);
    let both = limit0.min(limit1);

    // Straight on: one pair, shared
    let Some(turn) = turn else {
      let n = (n0 + n1).normalize_or(n0);
      let normal = (n, Across::of(n));
      let start = self.straight_pair(point, normal, u, both);
      let end = match end_u {
        None => start,
        Some(u) => self.straight_pair(point, normal, u, both),
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
      push: (push0, push1),
      ..
    } = turn;
    // The edge (`v`) the outside of the turn is on, and its outward normals
    let (v_out, v_in) = if side > 0.0 { (0.0, 1.0) } else { (1.0, 0.0) };
    let (m0, m1) = (side * n0, side * n1);

    let outer_end = self.vertex(
      p + m0 * hw,
      point,
      [u_end, v_out],
      before.across,
      Measure::edge(side, hw, n0, push0, limit0),
    );
    let outer_start = self.vertex(
      p + m1 * hw,
      point,
      [u, v_out],
      after.across,
      Measure::edge(side, hw, n1, push1, limit1),
    );
    let (inner_end, inner_start) = if shares_inside {
      // The segments' slanted ends fan from the corner point, so `v` keeps
      // measuring the distance from the centre line across the join
      let at = p - side * miter * hw;
      let measure = Measure::edge(-side, hw, n0, -side * miter, both);
      let inner = self.vertex(at, point, [u, v_in], bisector, measure);
      let inner_end = match end_u {
        None => inner,
        Some(u) => self.vertex(at, point, [u, v_in], bisector, measure),
      };
      (inner_end, inner)
    } else {
      // Too sharp to share: each segment keeps its own inner edge, pushed
      // opposite its outer vertex, so the end they share keeps passing
      // through the corner point the wedge fans from
      let inner_end = self.vertex(
        p - m0 * hw,
        point,
        [u_end, v_in],
        before.across,
        Measure::edge(-side, hw, n0, -push0, limit0),
      );
      let inner_start = self.vertex(
        p - m1 * hw,
        point,
        [u, v_in],
        after.across,
        Measure::edge(-side, hw, n1, -push1, limit1),
      );
      (inner_end, inner_start)
    };

    let bevelled = !mitered && self.style.join != LineJoin::Round;
    if shares_inside && !bevelled {
      // The slanted ends measure their segment's strip alone
      let centre = self.vertex(p, point, [u, 0.5], Across::NONE, Measure::centre(hw));
      self.wedge(side, centre, inner_end, outer_end);
      self.wedge(side, centre, outer_start, inner_start);
    }

    // The outside: the miter's point, the bevel or the round's arc, each
    // with vertices of its own at the strip's places
    if bevelled {
      self.bevel(
        point,
        &turn,
        (before, after),
        (outer_end, outer_start),
        (inner_end, inner_start),
      );
    } else if mitered {
      // Each half of the point measures its segment across and the other
      // segment's outer edge as its second, the two edges `cos` apart
      let other0 = Second::edge(p + m1 * hw, m1, cos);
      let other1 = Second::edge(p + m0 * hw, m0, cos);
      let centre = self.vertex(
        p,
        point,
        [u, 0.5],
        Across::NONE,
        Measure::centre(hw).with(Some(other0)),
      );
      let end = self.copy(outer_end, |v| measure_second(v, other0));
      let tip = self.vertex(
        p + side * miter * hw,
        point,
        [u, v_out],
        bisector,
        Measure::edge(side, hw, n0, side * miter, NO_EDGE).with(Some(other0)),
      );
      let start = self.copy(outer_start, |v| measure_second(v, other1));
      self.wedge(side, centre, end, tip);
      self.wedge(side, centre, tip, start);
    } else {
      let centre = self.vertex(
        p,
        point,
        [u, 0.5],
        Across::NONE,
        Measure::round(hw, Vec2::ZERO, Vec2::ZERO, 0.0),
      );
      let angle = m0.angle_to(m1);
      let steps = ((angle.abs() / (std::f32::consts::PI / 16.0)).ceil() as u32).max(2);
      let mut previous = self.copy(outer_end, |v| set_round(v, m0 * hw));
      for k in 1..steps {
        let t = k as f32 / steps as f32;
        let outward = Vec2::from_angle(angle * t).rotate(m0);
        let arc = self.vertex(
          p + outward * hw,
          point,
          [u, v_out],
          before.across.turned(angle * t),
          Measure::round(hw, outward * hw, outward, NO_EDGE),
        );
        self.wedge(side, centre, previous, arc);
        previous = arc;
      }
      let last = self.copy(outer_start, |v| set_round(v, m1 * hw));
      self.wedge(side, centre, previous, last);
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

  /// A bevel at `point`: a fan from the corner point on each side, through
  /// copies of the inner vertex (when the segments share it) and of the
  /// strip's elbow, to the bevel's middle. Every triangle of it measures its
  /// segment across and the bevel's edge as its second, the two edges
  /// `m · bevel` apart.
  fn bevel(
    &mut self,
    point: &PolyPoint,
    turn: &Turn,
    (before, after): (Segment, Segment),
    (outer_end, outer_start): (u32, u32),
    (inner_end, inner_start): (u32, u32),
  ) {
    let (p, hw, u) = (point.pos, point.half_width, point.u);
    let Turn {
      side,
      cos,
      bisector,
      shares_inside,
      bevel,
      ..
    } = *turn;
    let v_out = if side > 0.0 { 0.0 } else { 1.0 };
    let (n0, n1) = (before.normal, after.normal);
    let (m0, m1) = (side * n0, side * n1);
    let edge = Some(Second::edge(p + m0 * hw, bevel, m0.dot(bevel)));
    let centre = self.vertex(
      p,
      point,
      [u, 0.5],
      Across::NONE,
      Measure::centre(hw).with(edge),
    );
    let measured = |b: &mut Self, index: u32| match edge {
      Some(edge) => b.copy(index, |v| measure_second(v, edge)),
      None => index,
    };
    let inner = shares_inside.then(|| {
      let end = measured(self, inner_end);
      let start = if inner_start == inner_end {
        end
      } else {
        measured(self, inner_start)
      };
      (end, start)
    });
    let elbow_end = measured(self, outer_end);
    let elbow_start = measured(self, outer_start);
    // Pushed along the bevel's normal, its distance from either centre line
    // rises by `m · bevel` per unit
    let middle = self.vertex(
      p + (m0 + m1) * (hw / 2.0),
      point,
      [u, v_out],
      bisector,
      Measure {
        dist: side * hw * (1.0 + cos) / 2.0,
        half: hw,
        normal: n0,
        second: edge,
        round: None,
        push: bevel,
        limit: NO_EDGE,
      },
    );
    if let Some((end, _)) = inner {
      self.wedge(side, centre, end, elbow_end);
    }
    self.wedge(side, centre, elbow_end, middle);
    self.wedge(side, centre, middle, elbow_start);
    if let Some((_, start)) = inner {
      self.wedge(side, centre, elbow_start, start);
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
        color: Color::WHITE,
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
        emission: 1.0,
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
      emission: 1.0,
    };
    let points = [(0.0, 0.0), (5.0, 0.0)].map(|(x, y)| PolyPoint {
      pos: Vec2::new(x, y),
      half_width: 1.0,
      color: Color::WHITE,
      u: x / 5.0,
    });
    let second = t.polyline(&points, &style);
    assert_eq!(second.start as usize, t.indices.len() - second.len());
    t.clear();
    assert!(t.vertices.is_empty() && t.indices.is_empty());
    // One quad, which measures both butt caps as a strip along it
    assert_eq!(t.polyline(&points, &style), 0..6);
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
  fn a_vertex_packs_its_edges_into_48_bytes() {
    // Place and uv, an sRGB colour, three distances (or a round offset and
    // the half width), the push, its limit and rates in halves, the angle
    // across, and the line, flags and angle between edges in one word
    assert_eq!(std::mem::size_of::<Vertex2D>(), 48);
    assert_eq!(std::mem::size_of::<LineInfo>(), 48);
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

    /// The middle between a two-point line's caps and its direction, which
    /// its `FLAG_ALONG` vertices measure from
    fn along(&self, cap: LineCap) -> Option<(Vec2, Vec2)> {
      if self.closed || self.points.len() != 2 || cap == LineCap::Round {
        return None;
      }
      let (a, b) = (self.at(0), self.at(1));
      let dir = (b - a).normalize();
      let reach = |i: usize| {
        if cap == LineCap::Square {
          self.half(i)
        } else {
          0.0
        }
      };
      Some(((a - dir * reach(0) + b + dir * reach(1)) / 2.0, dir))
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
      let round = v.flags() & FLAG_ROUND != 0;
      Self {
        pos: Vec2::from(v.position),
        dist: if round { 0.0 } else { v.dist },
        half: v.half,
        second: if round { NO_EDGE } else { v.second },
        round: if round {
          Vec2::new(v.dist, v.second)
        } else {
          Vec2::ZERO
        },
      }
    }

    /// One unit of push on, as the fringe would leave it
    fn pushed(v: &Vertex2D) -> Self {
      let s = Self::of(v);
      Self {
        pos: s.pos + v.push(),
        dist: s.dist + v.dist_rate.to_f32(),
        second: s.second - v.second_rate.to_f32(),
        round: if v.flags() & FLAG_ROUND != 0 {
          s.round + v.push()
        } else {
          Vec2::ZERO
        },
        ..s
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

  /// Near enough for a pushed vertex, whose push and rates are halves
  fn near_pushed(a: f32, b: f32) -> bool {
    (a - b).abs() <= 3e-3 * a.abs().max(b.abs()).max(1.0)
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
    let (flags, k) = (tri[0].flags(), tri[0].k());
    if tri.iter().any(|v| v.flags() != flags || v.k() != k) {
      return Err("its vertices' flags or angles differ".into());
    }
    let samples = Sample::over(tri);
    let moved: Vec<Sample> = tri
      .iter()
      .filter(|v| pushed && v.limit.to_f32() > 0.0)
      .map(Sample::pushed)
      .collect();
    let mut found = Fields::default();
    if flags & FLAG_ROUND != 0 {
      let corner = (0..shape.points.len()).find(|&i| {
        let from =
          |tolerance: f32| move |s: &Sample| (s.pos - shape.at(i) - s.round).length() < tolerance;
        samples
          .iter()
          .all(|s| from(1e-3)(s) && near(s.half, shape.half(i)))
          && moved.iter().all(from(1e-2))
      });
      found.corner = Some(shape.at(corner.ok_or("round from no point")?));
      for v in tri.iter().filter(|v| pushed && v.limit.to_f32() > 0.0) {
        let round = Sample::of(v).round;
        if !near_pushed((round + v.push()).length(), round.length() + 1.0) {
          return Err("a round piece's push is not straight out".into());
        }
      }
    } else {
      found.centre = shape.centre_lines().into_iter().find(|&(a, n)| {
        samples
          .iter()
          .all(|s| near(s.dist, (s.pos - a).dot(n)) && s.dist.abs() <= s.half + 1e-3)
          && moved
            .iter()
            .all(|s| near_pushed(s.dist, (s.pos - a).dot(n)))
      });
      found
        .centre
        .ok_or("dist from no centre line, or outside its half width")?;
      if shape.even_width() && samples.iter().any(|s| !near(s.half, shape.half(0))) {
        return Err("half is not the half width".into());
      }
    }
    if flags & FLAG_ALONG != 0 {
      let (mid, dir) = shape.along(cap).ok_or("along a line with no two caps")?;
      let fits = samples
        .iter()
        .all(|s| near(s.second, (s.pos - mid).dot(dir)))
        && moved
          .iter()
          .all(|s| near_pushed(s.second, (s.pos - mid).dot(dir)));
      if !fits {
        return Err("second is not the distance along from between the caps".into());
      }
      found.second = Some((mid, dir));
    } else if flags & FLAG_SECOND != 0 {
      found.second = shape.edge_lines(cap).into_iter().find(|&(q, m)| {
        samples.iter().all(|s| near(s.second, (q - s.pos).dot(m)))
          && moved
            .iter()
            .all(|s| near_pushed(s.second, (q - s.pos).dot(m)))
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
            for v in tri.iter().filter(|v| v.limit.to_f32() == 0.0) {
              assert_eq!(v.push(), Vec2::ZERO, "shape {index}: {v:#?}");
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
  fn a_pushed_vertex_measures_where_it_lands() {
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
      .map(|v| Vec2::from(v.position) + v.push() * p.min(v.limit.to_f32()))
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

  /// A vertex as the vertex stage leaves it, pushed `p` units (as far as its
  /// limit lets it): where it lands and what it measures there.
  #[derive(Clone, Copy, Debug)]
  struct Landed {
    pos: Vec2,
    dist: f32,
    half: f32,
    second: f32,
    round: Vec2,
  }

  fn landed(v: &Vertex2D, p: f32) -> Landed {
    let a = p.min(v.limit.to_f32());
    let s = Sample::of(v);
    Landed {
      pos: s.pos + v.push() * a,
      dist: s.dist + v.dist_rate.to_f32() * a,
      half: s.half,
      second: s.second - v.second_rate.to_f32() * a,
      round: s.round + v.push() * a,
    }
  }

  /// The model's coverage at `at`, inside the pushed triangle `tri` whose
  /// corners landed at `land`: its attributes interpolated there.
  fn modelled(tri: &[Vertex2D; 3], land: &[Landed; 3], at: Vec2, along_half: f32) -> f32 {
    use super::super::coverage::{cover_corner, cover_edge, cover_strip};
    let [a, b, c] = land.map(|l| l.pos);
    let area = (b - a).perp_dot(c - a);
    let wb = (at - a).perp_dot(c - a) / area;
    let wc = (b - a).perp_dot(at - a) / area;
    let wa = 1.0 - wb - wc;
    let mix = |f: fn(&Landed) -> f32| wa * f(&land[0]) + wb * f(&land[1]) + wc * f(&land[2]);
    let flags = tri[0].flags();
    if flags & FLAG_ROUND != 0 {
      let round = Vec2::new(mix(|l| l.round.x), mix(|l| l.round.y));
      return cover_edge(mix(|l| l.half) - round.length());
    }
    let half = mix(|l| l.half);
    let across = if half >= NO_EDGE * 0.5 {
      1.0
    } else {
      cover_strip(mix(|l| l.dist), half)
    };
    if flags & FLAG_SECOND == 0 {
      return across;
    }
    let second = if flags & FLAG_ALONG != 0 {
      cover_strip(mix(|l| l.second), along_half)
    } else {
      cover_edge(mix(|l| l.second))
    };
    cover_corner(across, second, tri[0].k())
  }

  /// Whether `at` lies in the triangle (its edges included)
  fn within([a, b, c]: [Vec2; 3], at: Vec2) -> bool {
    let sides = [
      (b - a).perp_dot(at - a),
      (c - b).perp_dot(at - b),
      (a - c).perp_dot(at - c),
    ];
    sides.iter().all(|&s| s >= -1e-6) || sides.iter().all(|&s| s <= 1e-6)
  }

  /// A corner whose segments share their inner vertex: where it is, and
  /// the two inner edges' lines (a point and a normal each).
  #[derive(Clone, Copy, Debug)]
  struct Inner {
    at: Vec2,
    edges: [(Vec2, Vec2); 2],
  }

  impl Inner {
    /// Whether a pixel centred at `at` is at the inner corner: within a
    /// pixel of its vertex, or near it within a pixel of both inner edges,
    /// so the pixel lies across both segments' strips where they overlap
    fn holds(&self, at: Vec2) -> bool {
      let distance = at.distance(self.at);
      let both = self
        .edges
        .iter()
        .all(|&(q, n)| (at - q).dot(n).abs() <= 1.0);
      distance <= 1.0 || (distance <= INNER_REACH && both)
    }
  }

  /// How far along the inner edges from the inner vertex the inner corner
  /// reaches, where they are within a pixel of each other
  const INNER_REACH: f32 = 4.0;

  /// The worst coverage error over pixels whose centres lie within `reach`
  /// of one of `around`, against the exact area of the unpushed mesh, and
  /// the worst at one of the `inner` corners; and where the worst lies.
  fn worst_coverage(
    t: &Triangles,
    around: &[Vec2],
    reach: f32,
    inner: &[Inner],
  ) -> ((f32, Vec2), (f32, Vec2)) {
    use super::super::coverage::area_in_pixel;
    let rest: Vec<[Vec2; 3]> = triangles_of(t)
      .map(|tri| tri.map(|v| Vec2::from(v.position)))
      .collect();
    let pushed: Vec<([Vertex2D; 3], [Landed; 3])> = triangles_of(t)
      .map(|tri| (tri, tri.map(|v| landed(&v, 1.0))))
      .filter(|(_, land)| {
        let [a, b, c] = land.map(|l| l.pos);
        (b - a).perp_dot(c - a).abs() > 1e-7
      })
      .collect();
    let boxed = |tri: &[Vec2; 3]| {
      (
        tri[0].min(tri[1]).min(tri[2]),
        tri[0].max(tri[1]).max(tri[2]),
      )
    };
    let rest_boxes: Vec<_> = rest.iter().map(boxed).collect();
    let pushed_boxes: Vec<_> = pushed
      .iter()
      .map(|(_, land)| boxed(&land.map(|l| l.pos)))
      .collect();
    let (mut outer, mut near_inner) = ((0.0f32, Vec2::ZERO), (0.0f32, Vec2::ZERO));
    for &centre in around {
      let (from, to) = ((centre - reach).floor(), (centre + reach).ceil());
      for y in from.y as i32..to.y as i32 {
        for x in from.x as i32..to.x as i32 {
          let at = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
          if at.distance(centre) > reach {
            continue;
          }
          let meets = |(min, max): &(Vec2, Vec2), grow: f32| {
            at.x + grow >= min.x
              && at.x - grow <= max.x
              && at.y + grow >= min.y
              && at.y - grow <= max.y
          };
          let truth: f32 = rest
            .iter()
            .zip(&rest_boxes)
            .filter(|(_, b)| meets(b, 0.5))
            .map(|(tri, _)| area_in_pixel(*tri, at))
            .sum::<f32>()
            .min(1.0);
          let model = pushed
            .iter()
            .zip(&pushed_boxes)
            .filter(|(_, b)| meets(b, 0.0))
            .find(|((_, land), _)| within(land.map(|l| l.pos), at))
            .map_or(0.0, |((tri, land), _)| {
              let along = t
                .lines
                .get(tri[0].line())
                .map_or(0.0, |line| line.along_half);
              modelled(tri, land, at, along)
            });
          let error = (model - truth).abs();
          let worst = if inner.iter().any(|corner| corner.holds(at)) {
            &mut near_inner
          } else {
            &mut outer
          };
          if error > worst.0 {
            *worst = (error, at);
          }
        }
      }
    }
    (outer, near_inner)
  }

  /// Where the inner sides of a corner at `p` meet, between segments along
  /// `d0` then `d1` of half width `hw`, if they share it.
  fn shared_inner(p: Vec2, d0: Vec2, d1: Vec2, hw: f32, shortest: f32) -> Option<Inner> {
    let (n0, n1) = (d0.perp(), d1.perp());
    let side = if d0.perp_dot(d1) > 0.0 { -1.0 } else { 1.0 };
    let miter = (n0 + n1) / (1.0 + n0.dot(n1));
    (miter.length() * hw <= shortest / 2.0).then(|| Inner {
      at: p - side * miter * hw,
      edges: [(p - side * n0 * hw, n0), (p - side * n1 * hw, n1)],
    })
  }

  #[test]
  fn the_pushed_mesh_gives_every_pixel_its_coverage() {
    // Pushed one pixel, every pixel's modelled coverage is within 0.12 of
    // the exact area of the line drawn unpushed, at every join, cap and
    // closed corner; at a shared inner vertex (within a pixel of it, or
    // near it within a pixel of both inner edges), within the inner
    // corner's recorded figure
    const TOLERANCE: f32 = 0.12;
    const INNER: f32 = 0.45;
    let offsets = [
      Vec2::ZERO,
      Vec2::new(0.25, 0.1),
      Vec2::new(0.5, 0.6),
      Vec2::new(0.8, 0.35),
    ];
    let mut failures = Vec::new();
    let mut inner_worst = (0.0f32, String::new());
    let mut outer_worst = (0.0f32, String::new());
    let mut check = |what: String, t: &Triangles, around: &[Vec2], reach: f32, inner: &[Inner]| {
      let ((error, at), (near, near_at)) = worst_coverage(t, around, reach, inner);
      if error > TOLERANCE {
        failures.push(format!("{what}: {error:.3} at {at}"));
      }
      if error > outer_worst.0 {
        outer_worst = (error, format!("{what} at {at}"));
      }
      if near > inner_worst.0 {
        inner_worst = (near, format!("{what} at {near_at}"));
      }
    };
    for hw in [0.75f32, 1.5, 3.0, 6.0] {
      let length = 20.0 * hw + 10.0;
      for rotation in 0..8 {
        let d0 = Vec2::from_angle(rotation as f32 * std::f32::consts::TAU / 8.0 + 0.1);
        for &offset in &offsets {
          let p = Vec2::new(100.0, 100.0) + offset;
          // Joins, both ways
          for degrees in [5.0f32, 15.0, 30.0, 45.0, 60.0, 90.0, 120.0, 150.0, 165.0] {
            for turn in [1.0f32, -1.0] {
              let d1 = Vec2::from_angle(turn * degrees.to_radians()).rotate(d0);
              let points = [p - d0 * length, p, p + d1 * length].map(|q| (q.x, q.y));
              let inner: Vec<Inner> = shared_inner(p, d0, d1, hw, length).into_iter().collect();
              for join in JOINS {
                let t = build(&points, 2.0 * hw, join, LineCap::Butt, false);
                let what = format!("{join:?} {degrees}° {turn} hw {hw} rot {rotation} {offset}");
                check(what, &t, &[p], 5.0 * hw + 3.0, &inner);
              }
            }
          }
          // Caps on one segment and on two
          for cap in CAPS {
            let two = [p - d0 * length, p];
            let bent = [p - d0 * length * 2.0, p - d0 * length, p];
            let d1 = Vec2::from_angle(0.5).rotate(d0);
            let bent = [bent[0], bent[1], bent[1] + d1 * length];
            for points in [&two[..], &bent[..]] {
              let ends = [points[0], points[points.len() - 1]];
              let pts: Vec<(f32, f32)> = points.iter().map(|q| (q.x, q.y)).collect();
              let t = build(&pts, 2.0 * hw, LineJoin::Miter, cap, false);
              let what = format!(
                "{cap:?} {} points hw {hw} rot {rotation} {offset}",
                pts.len()
              );
              check(what, &t, &ends, 2.0 * hw + 3.0, &[]);
            }
          }
        }
      }
      // Closed corners: a square and a triangle
      let square = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
      let triangle = [(0.0, 0.0), (1.0, 0.0), (0.5, 0.8)];
      for shape in [&square[..], &triangle[..]] {
        let scale = 25.0 * hw + 10.0;
        let corners: Vec<Vec2> = shape
          .iter()
          .map(|&(x, y)| Vec2::new(50.0, 50.0) + Vec2::new(x, y) * scale + offsets[1])
          .collect();
        let n = corners.len();
        let inner: Vec<Inner> = (0..n)
          .filter_map(|i| {
            let (a, p, b) = (corners[(i + n - 1) % n], corners[i], corners[(i + 1) % n]);
            let shortest = p.distance(a).min(p.distance(b));
            shared_inner(p, (p - a).normalize(), (b - p).normalize(), hw, shortest)
          })
          .collect();
        let pts: Vec<(f32, f32)> = corners.iter().map(|q| (q.x, q.y)).collect();
        for join in JOINS {
          let t = build(&pts, 2.0 * hw, join, LineCap::Butt, true);
          let what = format!("closed {n} {join:?} hw {hw}");
          check(what, &t, &corners, 5.0 * hw + 3.0, &inner);
        }
      }
    }
    eprintln!("worst away from inner corners: {outer_worst:?}");
    eprintln!("worst at an inner corner: {inner_worst:?}");
    assert!(
      failures.is_empty(),
      "{} pixels off:\n{}",
      failures.len(),
      failures.join("\n")
    );
    assert!(
      inner_worst.0 <= INNER,
      "an inner corner is off by {inner_worst:?}"
    );
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
