//! Lines and trails as triangles, built piece by piece: a segment between
//! every two points, a join at every corner and a cap at each open end.
//! This builder is the reference the GPU's line pieces
//! (`shaders/include/line_pieces.wgsl`) are tested against, so it builds
//! what they build, in the same order, in f32: the GPU's vertices equal its
//! own triangle for triangle (`the_gpu_builds_the_reference_s_vertices`, in
//! `line_gpu_tests.rs`). World lines and the UI's are built on the GPU, by
//! the same piece functions; the builder itself is compiled for tests
//! alone.
//!
//! Every vertex carries its distances to the line's edges, in the line's
//! units, and how far a fringe may push it outward; the builder does not know
//! the camera. What a line's vertices share (its style and effect
//! parameters, the atlas slot a textured line repeats, its emission, a
//! two-point capped line's half length) is its entry in the line table
//! (`LineInfo`), which each vertex names.
//!
//! - **Points are filtered as they are copied**, before any piece is built
//!   (`the_filter_copies_points_before_any_piece`): non-finite points go,
//!   and so do points within 1e-4 of the one kept before and a closed line's
//!   repeated end. A closed line left with fewer than three points is open;
//!   a line left with fewer than two, or with no width, draws nothing. Each
//!   point keeps its `u`.
//! - **The piece list depends only on where a point sits in its line**
//!   (`piece_list`, `the_piece_list_depends_only_on_where_a_point_sits`):
//!   an open line of `n` points is its start cap, then segment 0, join 1,
//!   segment 1, ..., segment `n − 2`, then its end cap; a closed line is
//!   segment `i` then the join at `i + 1` for every point, the last join at
//!   point 0. Butt caps are left out (they have no triangles). Each piece
//!   draws a fixed number of triangles, in a fixed order
//!   (`piece_triangles`): a segment 4, a miter or bevel join 4, a round join
//!   18, a square cap 2, a round cap 16. What a corner's geometry leaves
//!   unused collapses to a triangle of no area at the corner point (a join
//!   where the line runs straight on draws only those).
//! - **A piece is self-contained.** It has its own vertices, shares none
//!   with its neighbours, and reads only its window of the line's points
//!   (`a_piece_reads_only_its_window`): a segment from point `i` reads
//!   points `i − 2` to `i + 3`, a join or a cap at point `i` reads `i − 2` to
//!   `i + 2`; neighbours wrap on a closed line and are absent past an open
//!   one's ends.
//! - **One corner function decides every corner** (`Joint::at`), for every
//!   piece that touches it, from the same points in the same order, so the
//!   copies of a place agree bit for bit: miter or bevel (the miter limit),
//!   the inner side shared or split (shared while it reaches at most half the
//!   shorter segment), straight on (the join draws nothing) and a U-turn (a
//!   flag, never an infinity). It also gives the corner's pushes and the
//!   limits it sets the segments beside it (`copies_at_one_place_push_alike`).
//!   A round piece's angle is the `acos` of its edges' normals, signed by
//!   their turn (`the_gpu_builds_the_reference_s_vertices`: with `atan2`,
//!   Metal gave the opposite sign at exact right angles).
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
//! - **A pushed vertex measures where it lands**
//!   (`a_pushed_vertex_measures_where_it_lands`): pushed `a` units, `dist`
//!   rises by `dist_rate × a`, `second` falls by `second_rate × a` and a
//!   round offset grows by the push, so the pushed triangle's distances stay
//!   true. A vertex on two edges pushes along their miter (a miter's point
//!   and the strip's outer vertices beside it, a bevel's elbows, a cap's
//!   corners), so both rise by one unit per unit; a bevel's middle pushes
//!   along the bevel's normal, its distance across rising by `m · bevel`.
//! - **Round arcs are circumscribed**
//!   (`round_pieces_are_contained_and_true_at_a_half_width_of_150`): a round
//!   join's 16 triangles fan over its angle in 15 steps, a round cap's over
//!   half a turn; the arc's ends are the strip's outer vertices and the
//!   corners between them sit at the half-step angles, `hw / cos(step / 2)`
//!   from the corner point, pushed by `1 / cos(step / 2)` units per unit, so
//!   the polygon holds the disc, pushed or not, at any width. Coverage takes
//!   the offset's length, so the radius is exact whatever the steps.
//! - **Two edges blend by their angle.** Each vertex of a triangle with a
//!   second edge carries `k`, the dot product of the two edges' outward
//!   normals (`cos` of the turn in a miter's point, `m · bevel` in a bevel's
//!   fans, zero beside a cap), for the coverage model's blend
//!   (`coverage.rs`). Pushed one pixel, the mesh gives every pixel the
//!   model's coverage within 0.12 of the exact area of the line drawn
//!   unpushed, at every join, cap and closed corner, at half widths of 0.75
//!   to 6 pixels, turns of 5 to 165 degrees, 8 directions and 4 sub-pixel
//!   places (`the_pushed_mesh_gives_every_pixel_its_coverage`).
//! - **A split corner's ends push straight out**
//!   (`the_pushed_mesh_gives_every_pixel_its_coverage`, its split corners).
//!   Where a corner's inner side splits, a segment's outer and inner
//!   vertices at it push along its normal, so its end edge keeps to its line
//!   through the corner point, and the join's two slanted-end slots hold a
//!   fringe triangle each (corner point, the strip's elbow pushed straight
//!   out, the fan's elbow pushed along the corner), measuring what the fan
//!   measures. These elbows are the one place copies push two ways, and the
//!   fringe triangle joins them (`copies_at_one_place_push_alike`).
//! - **Effects move every copy alike**
//!   (`copies_at_one_place_move_alike_under_effects`). Each vertex carries
//!   the `u` displacement effects move it by (`effect_u`): its texture `u`,
//!   but at a corner point, and at a vertex both segments at a corner share
//!   (the shared inner vertex, a straight-on pair), the point's own; and the
//!   `across` it moves along at a corner point and a shared inner vertex is
//!   the corner's bisector (a round cap's centre: its segment's normal), for
//!   every piece. So the copies of a place agree bit for bit in both, at a
//!   closed line's seam too, where the arriving segment's texture `u` runs on
//!   past the last point while point 0's starts again, and no crack or notch
//!   opens under an effect.
//! - **Limit: a displaced piece is sheared.** Its ends move along its
//!   normal by different amounts, so its edges stay parallel and its
//!   distances, measured at the vertices, are unchanged: across the line
//!   along the normal (a column across a level line) it covers what it
//!   covers undisplaced, the fringe moving with it (each column's summed
//!   alpha within 0.6% of the undisplaced line's,
//!   `a_displaced_line_keeps_its_coverage_across_it`), but across its slant
//!   it draws narrower, and its edge's ramp shorter, by `L ÷ √(L² + Δ²)`
//!   for a piece of length `L` whose ends move `Δ` apart: 98% on average
//!   and 91% at the steepest piece of a moderate Lightning or Ribbon line,
//!   62% on average and 34% at the steepest of a Ribbon as steep as the
//!   trails demo's. Widening a piece by its slant would need the
//!   neighbouring points' displacement at every vertex and would part a
//!   corner's copies (a join borders two slants), so it is left.
//! - **A bevel is a fan each side.** From the corner point through copies of
//!   the shared inner vertex (when the segments share it) and of the strip's
//!   elbow, to the bevel's middle: every triangle touching the elbow
//!   measures its segment across and the bevel's edge. The strip's elbow
//!   pushes along the miter of its edge and the bevel, so pushed, the strip
//!   quad's corner reaches past the bevel's edge by the push, where only its
//!   strip is measured (a slide along the bevel's edge would keep it inside,
//!   for two more vertices a bevel and a limit that runs short at shallow
//!   bevels).
//! - **A capped segment measures its cap over its whole quad**; a two-point
//!   capped line measures both caps as one strip along it. Nothing is cut.
//! - **Pushed, the mesh stays watertight and covers nothing twice**
//!   (`a_translucent_line_covers_no_pixel_twice_at_any_join`). Copies at one
//!   place push alike, bit for bit (`copies_at_one_place_push_alike`). An
//!   unshared inner vertex pushes opposite its outer
//!   one, so their end keeps passing through the corner point the join fans
//!   from. A sharp corner's inner side still overlaps, as it does unpushed.
//! - **No vertex lies inside another triangle's edge**
//!   (`no_vertex_lies_inside_an_edge`). Where a corner's inner side splits,
//!   or a round cap ends the line, the segment's end runs through the point
//!   the join or the cap fans from, so the segment has a vertex there and
//!   fans its quad from it (its two further triangles have no area where
//!   neither end does).
//! - **A pushed quad never folds** (`a_pushed_inner_vertex_never_folds_its_quad`),
//!   on lines of even width (but see the taper's limit below).
//!   Every vertex of a segment's quad, a cap's corners included, pushes at
//!   most the quad's limit, the least its corners allow: a shared inner
//!   vertex until it would pass the shorter segment's middle, and a push
//!   that runs along the segment (a miter's or a bevel's) until it has run
//!   half the segment's length. A shared inner vertex pushes at most the
//!   lesser of its two quads' limits. Vertices never pushed have a limit of
//!   zero.
//! - **Limit: the inner corner.** Within a pixel of a shared inner vertex,
//!   or near it within a pixel of both inner edges, a pixel lies across both
//!   segments' strips, and one triangle's distances cannot give their union:
//!   the coverage there is off by up to 0.16 at a 60 degree turn, 0.26 at
//!   90, 0.34 at 120, 0.39 at 150 and 0.45 at 165, whatever the join
//!   (measured by the coverage test, which holds it to 0.45).
//! - **Limit: a split corner.** Near its corner point, where the segments'
//!   ends and the fan meet and the bevel (or the arc) cuts close past them,
//!   a pixel lies against edges its triangle does not measure: off by up to
//!   0.09 at a 60 degree turn, 0.15 at 90, 0.22 at 120, 0.32 at 150 and 0.37
//!   at 165 (0.13 at 90 and 0.21 at 120 while its segments are at least
//!   `miter · hw` long; measured with segments of 0.5, 1.0 and 1.9 times
//!   that; 0.98 before its ends pushed straight out), and within a pixel of
//!   the point, or near it within a pixel of both ends, by the inner
//!   corner's figure (`the_pushed_mesh_gives_every_pixel_its_coverage`).
//! - **Limit:** on a tapered line a push that runs along the segment keeps
//!   the vertex's `half`, which the taper would change by its slope times
//!   that run.
//! - **Limit: tapered lines can fold when pushed** at extreme tapers: 250 of
//!   5,247 random open tapered lines turning 120 to 170 degrees folded (for
//!   example a 0.83-unit segment at a half width of 6.1 beside one of 2.0).
//!   The quad limits take no account of the half width's change along a
//!   segment.

#[cfg(test)]
use std::ops::Range;

use glam::Vec2;

use crate::core::trail::{LineCap, LineJoin};
use crate::core::ui::Color;

/// One vertex of the triangle stream: 76 bytes, every number an f32.
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
  /// The unit perpendicular displacement effects move the vertex along
  /// (zero: none)
  pub(crate) across: [f32; 2],
  /// Its line's entry in the line table
  pub(crate) line: u32,
  /// `FLAG_ROUND`, `FLAG_SECOND` and `FLAG_ALONG`
  pub(crate) flags: u32,
  /// The dot product of its two edges' outward normals
  pub(crate) k: f32,
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
  pub(crate) push: [f32; 2],
  /// The most the fringe may push it
  pub(crate) limit: f32,
  /// How much `dist` rises per unit of push
  pub(crate) dist_rate: f32,
  /// How much `second` falls per unit of push
  pub(crate) second_rate: f32,
  /// The `u` displacement effects move it by: its `uv`'s, but at a corner
  /// point and at a vertex both segments share the point's own, so every
  /// piece's copy there moves alike (a closed line's seam runs a segment's
  /// texture `u` on past its last point)
  pub(crate) effect_u: f32,
}

/// A vertex of a round piece: coverage by the length of its offset
#[cfg(test)]
pub(crate) const FLAG_ROUND: u32 = 1;
/// A vertex whose `second` edge distance counts
#[cfg(test)]
pub(crate) const FLAG_SECOND: u32 = 2;
/// A vertex whose `second` is a distance along a two-point capped line from
/// the middle between its caps: a strip of its line's `along_half`
#[cfg(test)]
pub(crate) const FLAG_ALONG: u32 = 4;
/// No edge: the `second` of a vertex without one, the `half` of one that
/// only a second edge bounds, and no limit
#[cfg(test)]
pub(crate) const NO_EDGE: f32 = 1.0e6;
/// The line table holds this many lines a frame; the rest draw nothing
pub(crate) const MAX_LINES: usize = 1 << 20;

#[cfg(test)]
impl Vertex2D {
  pub(crate) fn push(&self) -> Vec2 {
    Vec2::from(self.push)
  }
}

/// What every piece of one line shares: an entry of the line table, which
/// the vertex stage reads by the line's number (`LineInfo` in
/// `line_pieces.wgsl`, 80 bytes).
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
  /// Its first point in the point buffer, and how many it has (filtered)
  pub(crate) first: u32,
  pub(crate) count: u32,
  /// `JOIN_MITER`, `JOIN_BEVEL` or `JOIN_ROUND`
  pub(crate) join: u32,
  /// `CAP_BUTT`, `CAP_SQUARE` or `CAP_ROUND`
  pub(crate) cap: u32,
  /// 1 when its last point joins its first
  pub(crate) closed: u32,
  /// How its edges draw: 0 smooth
  pub(crate) edge: u32,
  /// A miter longer than this many half widths is bevelled
  pub(crate) miter_limit: f32,
  pub(crate) _pad: [u32; 3],
}

/// A line's join and cap as the line table numbers them
pub(crate) const JOIN_MITER: u32 = 0;
pub(crate) const JOIN_BEVEL: u32 = 1;
pub(crate) const JOIN_ROUND: u32 = 2;
pub(crate) const CAP_BUTT: u32 = 0;
pub(crate) const CAP_SQUARE: u32 = 1;
pub(crate) const CAP_ROUND: u32 = 2;

impl LineInfo {
  /// The entry of a line drawn in `style`, its `count` points from `first`
  pub(crate) fn of(
    style: &PolyStyle,
    closed: bool,
    along_half: f32,
    first: u32,
    count: u32,
  ) -> Self {
    Self {
      params: style.params,
      bounds: [0.0; 4],
      emission: style.emission,
      along_half,
      first,
      count,
      join: match style.join {
        LineJoin::Miter => JOIN_MITER,
        LineJoin::Bevel => JOIN_BEVEL,
        LineJoin::Round => JOIN_ROUND,
      },
      cap: match style.cap {
        LineCap::Butt => CAP_BUTT,
        LineCap::Square => CAP_SQUARE,
        LineCap::Round => CAP_ROUND,
      },
      closed: u32::from(closed),
      edge: 0,
      miter_limit: MITER_LIMIT,
      _pad: [0; 3],
    }
  }
}

/// A two-point butt or square capped line measures both caps as one strip
/// along it, this long each way from its middle (zero for any other line)
pub(crate) fn along_half(pts: &[PolyPoint], closed: bool, cap: LineCap) -> f32 {
  match (closed, pts, cap) {
    (false, [a, b], LineCap::Butt | LineCap::Square) => {
      let reach = |p: &PolyPoint| cap_reach(p, cap);
      (a.pos.distance(b.pos) + reach(a) + reach(b)) / 2.0
    }
    _ => 0.0,
  }
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

/// What a piece of a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PieceKind {
  /// The quad from its point to the next
  Segment,
  /// The corner at its point
  Join,
  /// The cap before the first point
  StartCap,
  /// The cap after the last point
  EndCap,
}

/// One piece of a line: what it is and the point it is built at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Piece {
  pub(crate) kind: PieceKind,
  pub(crate) point: u32,
}

/// The triangles a segment draws: its quad, fanned from the corner point of
/// an end that splits or is round capped
pub(crate) const SEGMENT_TRIANGLES: u32 = 4;
/// The triangles a miter or bevel join draws
pub(crate) const JOIN_TRIANGLES: u32 = 4;
/// The triangles a round join draws: its two slanted ends and its arc
pub(crate) const ROUND_JOIN_TRIANGLES: u32 = 2 + ARC_TRIANGLES;
/// The triangles a square cap draws
pub(crate) const SQUARE_CAP_TRIANGLES: u32 = 2;
/// The triangles a round cap draws
pub(crate) const ROUND_CAP_TRIANGLES: u32 = ARC_TRIANGLES;
/// The triangles of a round piece's arc
const ARC_TRIANGLES: u32 = 16;

/// The triangles a piece of `kind` draws, whatever its geometry.
pub(crate) fn piece_triangles(kind: PieceKind, join: LineJoin, cap: LineCap) -> u32 {
  match (kind, join, cap) {
    (PieceKind::Segment, ..) => SEGMENT_TRIANGLES,
    (PieceKind::Join, LineJoin::Round, _) => ROUND_JOIN_TRIANGLES,
    (PieceKind::Join, ..) => JOIN_TRIANGLES,
    (PieceKind::StartCap | PieceKind::EndCap, _, LineCap::Butt) => 0,
    (PieceKind::StartCap | PieceKind::EndCap, _, LineCap::Square) => SQUARE_CAP_TRIANGLES,
    (PieceKind::StartCap | PieceKind::EndCap, _, LineCap::Round) => ROUND_CAP_TRIANGLES,
  }
}

/// The pieces of a line of `n` filtered points, in the order they draw
/// (into `out`, cleared first). Where each point sits in its line decides
/// them, never where it is.
pub(crate) fn piece_list(n: usize, closed: bool, cap: LineCap, out: &mut Vec<Piece>) {
  out.clear();
  let piece = |kind, point: usize| Piece {
    kind,
    point: point as u32,
  };
  if closed {
    for i in 0..n {
      out.push(piece(PieceKind::Segment, i));
      out.push(piece(PieceKind::Join, (i + 1) % n));
    }
    return;
  }
  if n < 2 {
    return;
  }
  let capped = cap != LineCap::Butt;
  if capped {
    out.push(piece(PieceKind::StartCap, 0));
  }
  for i in 0..n - 1 {
    out.push(piece(PieceKind::Segment, i));
    if i + 1 < n - 1 {
      out.push(piece(PieceKind::Join, i + 1));
    }
  }
  if capped {
    out.push(piece(PieceKind::EndCap, n - 1));
  }
}

/// `points` filtered into `out` as a line is copied (cleared first), and
/// whether the line is still closed; `None` when it draws nothing.
pub(crate) fn filter_points(
  points: &[PolyPoint],
  closed: bool,
  out: &mut Vec<PolyPoint>,
) -> Option<bool> {
  out.clear();
  for point in points {
    if !point.pos.is_finite() || !point.half_width.is_finite() {
      continue;
    }
    if out
      .last()
      .is_some_and(|last| last.pos.distance(point.pos) < 1e-4)
    {
      continue;
    }
    out.push(*point);
  }
  let mut closed = closed;
  if closed
    && out.len() > 2
    && let (Some(first), Some(last)) = (out.first(), out.last())
    && first.pos.distance(last.pos) < 1e-4
  {
    out.pop();
  }
  if out.len() < 3 {
    closed = false;
  }
  if out.len() < 2 || out.iter().all(|point| point.half_width <= 0.0) {
    return None;
  }
  Some(closed)
}

/// Triangles built on the CPU for the batch's triangle stream, and the line
/// table their vertices name.
#[cfg(test)]
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

#[cfg(test)]
#[derive(Default)]
struct Scratch {
  points: Vec<PolyPoint>,
  pieces: Vec<Piece>,
}

#[cfg(test)]
impl Triangles {
  /// Forget every triangle and line, keeping the allocations.
  pub(crate) fn clear(&mut self) {
    self.vertices.clear();
    self.indices.clear();
    self.lines.clear();
  }

  /// Add the triangles of a polyline through `points`, and its line table
  /// entry; the index range they take (empty when nothing is drawn).
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
    let Some(closed) = filter_points(points, style.closed, &mut scratch.points) else {
      return false;
    };
    if self.lines.len() >= MAX_LINES {
      if !self.warned {
        self.warned = true;
        log::warn!(
          "more than {MAX_LINES} lines and trails in one frame: the line table is full, so the rest draw nothing"
        );
      }
      return false;
    }
    let pts = &scratch.points;
    let line = self.lines.len() as u32;
    let along = along_half(pts, closed, style.cap);
    self
      .lines
      .push(LineInfo::of(style, closed, along, 0, pts.len() as u32));
    piece_list(pts.len(), closed, style.cap, &mut scratch.pieces);
    for &piece in &scratch.pieces {
      self.piece(pts, closed, style, line, piece);
    }
    true
  }

  /// Add one piece of the line through the filtered points `pts`.
  fn piece(&mut self, pts: &[PolyPoint], closed: bool, style: &PolyStyle, line: u32, piece: Piece) {
    let reach = if piece.kind == PieceKind::Segment {
      3
    } else {
      2
    };
    let mut window = Window::of(pts, piece.point as usize, closed, reach);
    window.cap = style.cap;
    let start = self.indices.len();
    let mut builder = Builder {
      out: self,
      window,
      style,
      line,
      first: None,
    };
    match piece.kind {
      PieceKind::Segment => builder.segment(),
      PieceKind::Join => builder.join(),
      PieceKind::StartCap => builder.cap(true),
      PieceKind::EndCap => builder.cap(false),
    }
    debug_assert_eq!(
      (self.indices.len() - start) as u32,
      3 * piece_triangles(piece.kind, style.join, style.cap),
      "{piece:?}"
    );
  }
}

/// How far a butt or square cap reaches past its end point.
fn cap_reach(point: &PolyPoint, cap: LineCap) -> f32 {
  if cap == LineCap::Square {
    point.half_width
  } else {
    0.0
  }
}

/// The points a piece at point `i` reads: `i − 2` to `i + reach`, wrapped
/// on a closed line and absent past an open one's ends.
#[cfg(test)]
#[derive(Clone, Copy)]
struct Window {
  points: [Option<PolyPoint>; 6],
  /// Which of them is a closed line's first point, where its seam is
  seam: [bool; 6],
  cap: LineCap,
}

#[cfg(test)]
impl Window {
  fn of(pts: &[PolyPoint], i: usize, closed: bool, reach: i32) -> Self {
    let n = pts.len() as i32;
    let mut window = Self {
      points: [None; 6],
      seam: [false; 6],
      cap: LineCap::Butt,
    };
    for j in -2..=reach {
      let at = i as i32 + j;
      let at = if closed { at.rem_euclid(n) } else { at };
      let slot = (j + 2) as usize;
      if (0..n).contains(&at) {
        window.points[slot] = pts.get(at as usize).copied();
        window.seam[slot] = closed && at == 0;
      }
    }
    window
  }

  /// Point `j` from the piece's own
  fn get(&self, j: i32) -> Option<&PolyPoint> {
    self.points.get(usize::try_from(j + 2).ok()?)?.as_ref()
  }

  /// The segment from point `j` to the next
  fn segment(&self, j: i32) -> Option<Segment> {
    Some(Segment::between(self.get(j)?.pos, self.get(j + 1)?.pos))
  }

  /// `u` per world unit along the segment from point `j`
  fn u_scale(&self, j: i32) -> f32 {
    match (self.get(j), self.get(j + 1)) {
      (Some(a), Some(b)) => {
        let len = a.pos.distance(b.pos);
        if len > 0.0 { (b.u - a.u) / len } else { 0.0 }
      }
      _ => 0.0,
    }
  }

  /// The `u` the segment ending at point `j` ends with: at a closed line's
  /// seam it runs on past the last point
  fn end_u(&self, j: i32) -> f32 {
    let Some(point) = self.get(j) else {
      return 0.0;
    };
    let seam = usize::try_from(j + 2)
      .ok()
      .and_then(|slot| self.seam.get(slot))
      .copied()
      .unwrap_or(false);
    match (seam, self.get(j - 1)) {
      (true, Some(last)) => last.u + last.pos.distance(point.pos) * self.u_scale(j - 2),
      _ => point.u,
    }
  }

  /// The most a vertex of the quad from point `j` may push: the least its
  /// corners allow
  fn quad_limit(&self, j: i32, join: LineJoin) -> f32 {
    Joint::at(self, j, join)
      .after
      .min(Joint::at(self, j + 1, join).before)
  }

  /// The most a vertex both quads at point `j` share may push
  fn both(&self, j: i32, join: LineJoin) -> f32 {
    let before = self
      .get(j - 1)
      .map_or(NO_EDGE, |_| self.quad_limit(j - 1, join));
    let after = self
      .get(j + 1)
      .map_or(NO_EDGE, |_| self.quad_limit(j, join));
    before.min(after)
  }

  /// The second edge the segment from point `j` measures: its line's cap
  /// edge when it ends the line, or both caps as one strip along a
  /// two-point line
  fn segment_second(&self, j: i32) -> Option<Second> {
    let (a, b) = (self.get(j)?, self.get(j + 1)?);
    let first = self.get(j - 1).is_none();
    let last = self.get(j + 2).is_none();
    end_second(a, b, self.cap, first, last)
  }
}

/// The second edge a line's end measures, on the segment from `a` to `b`:
/// the start cap's (`first`), the end cap's (`last`), or both as one strip
/// along a two-point line; none for round caps.
#[cfg(test)]
fn end_second(
  a: &PolyPoint,
  b: &PolyPoint,
  cap: LineCap,
  first: bool,
  last: bool,
) -> Option<Second> {
  if cap == LineCap::Round {
    return None;
  }
  let dir = (b.pos - a.pos).normalize_or_zero();
  let (ra, rb) = (cap_reach(a, cap), cap_reach(b, cap));
  match (first, last) {
    (true, true) => Some(Second {
      bound: Bound::Along {
        mid: (a.pos - dir * ra + b.pos + dir * rb) / 2.0,
        dir,
      },
      k: 0.0,
    }),
    (true, false) => Some(Second::edge(a.pos - dir * ra, -dir, 0.0)),
    (false, true) => Some(Second::edge(b.pos + dir * rb, dir, 0.0)),
    (false, false) => None,
  }
}

/// A segment's direction and left-hand normal.
#[cfg(test)]
#[derive(Clone, Copy)]
struct Segment {
  dir: Vec2,
  normal: Vec2,
  len: f32,
}

#[cfg(test)]
impl Segment {
  fn between(a: Vec2, b: Vec2) -> Self {
    let len = a.distance(b);
    let dir = (b - a).normalize_or(Vec2::X);
    Self {
      dir,
      normal: dir.perp(),
      len,
    }
  }
}

/// What the corner function decides at a point.
#[cfg(test)]
#[derive(Clone, Copy)]
enum Shape {
  /// An open line's end
  End,
  /// The line runs straight on: one pair of edge vertices on `normal`
  Straight {
    normal: Vec2,
  },
  Turn(Turn),
}

/// A corner: its shape and the most it lets the quads before and after it
/// push.
#[cfg(test)]
#[derive(Clone, Copy)]
struct Joint {
  shape: Shape,
  before: f32,
  after: f32,
}

#[cfg(test)]
impl Joint {
  /// The corner at point `j` of `window`: the one function every piece
  /// touching it asks.
  fn at(window: &Window, j: i32, join: LineJoin) -> Self {
    let (Some(point), Some(before), Some(after)) =
      (window.get(j), window.segment(j - 1), window.segment(j))
    else {
      return Self {
        shape: Shape::End,
        before: NO_EDGE,
        after: NO_EDGE,
      };
    };
    match Turn::at(before, after, point.half_width, join) {
      None => Self {
        shape: Shape::Straight {
          normal: (before.normal + after.normal).normalize_or(before.normal),
        },
        before: NO_EDGE,
        after: NO_EDGE,
      },
      Some(turn) => Self {
        shape: Shape::Turn(turn),
        before: turn.limit_on(turn.push.0, before),
        after: turn.limit_on(turn.push.1, after),
      },
    }
  }
}

/// A second edge a vertex measures.
#[cfg(test)]
#[derive(Clone, Copy)]
enum Bound {
  /// The distance inside an edge through `point` with outward normal `out`
  Edge { point: Vec2, out: Vec2 },
  /// The signed distance along `dir` from `mid`, the middle between a
  /// two-point line's caps
  Along { mid: Vec2, dir: Vec2 },
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct Second {
  bound: Bound,
  /// The dot product of the outward normals of this edge and the strip's
  /// edge it meets
  k: f32,
}

#[cfg(test)]
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
#[cfg(test)]
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
  /// Effects move it by its point's own `u`: a vertex both segments at a
  /// corner share, whatever `u` its texture takes
  point_u: bool,
}

#[cfg(test)]
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
      point_u: false,
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

  /// Moved by effects as its point is: a vertex both segments share
  fn shared(self) -> Self {
    Self {
      point_u: true,
      ..self
    }
  }
}

/// `v` measuring `second` where it is, as it is pushed.
#[cfg(test)]
fn measure_second(v: &mut Vertex2D, second: Second) {
  v.second = second.at(Vec2::from(v.position));
  v.second_rate = second.rate(v.push());
  v.flags = (v.flags & !FLAG_ALONG) | second.flags();
  v.k = second.k;
}

/// `v` as a vertex of a round piece, `offset` from its corner point.
#[cfg(test)]
fn set_round(v: &mut Vertex2D, offset: Vec2) {
  v.dist = offset.x;
  v.second = offset.y;
  v.dist_rate = 0.0;
  v.second_rate = 0.0;
  v.flags = FLAG_ROUND;
  v.k = 0.0;
}

/// The push that moves a point out across two edges of outward normals `a`
/// and `b` one unit each per unit (`a` and `b` at most a right angle apart).
#[cfg(test)]
fn miter_of(a: Vec2, b: Vec2) -> Vec2 {
  (a + b) / (1.0 + a.dot(b))
}

/// How a corner turns: what its vertices and their pushes follow.
#[cfg(test)]
#[derive(Clone, Copy)]
struct Turn {
  /// The outside of the turn, as a multiple of the normals
  side: f32,
  cos: f32,
  /// Where the offset edges meet, as a multiple of the half width (zero at
  /// a U-turn, where they never meet)
  miter: Vec2,
  /// The bisector of the normals, the `across` of the vertices on it
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
  /// The round arc's angle, from the end's outward normal to the start's
  angle: f32,
}

#[cfg(test)]
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
    let u_turn = 1.0 + cos <= 1e-6;
    let miter = if u_turn {
      Vec2::ZERO
    } else {
      (n0 + n1) / (1.0 + cos)
    };
    let miter_length = miter.length();
    let shortest = before.len.min(after.len);
    // Shared only while it stays in the near half of both segments: the
    // corner at a segment's other end may pull its inner edge from that end
    // too, and past the middle the two would cross and twist the segment
    let shares_inside = !u_turn && miter_length * hw <= shortest / 2.0;
    let mitered = join == LineJoin::Miter && !u_turn && miter_length <= MITER_LIMIT;
    // Forward at a U-turn, where the bevel ends the line
    let bevel = if u_turn {
      before.dir
    } else {
      (m0 + m1).normalize_or(before.dir)
    };
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
      (shortest / (2.0 * miter_length) - hw).max(0.0)
    } else {
      NO_EDGE
    };
    // Round the outside, through the line's direction at a U-turn
    let angle = if u_turn {
      if m0.perp_dot(before.dir) >= 0.0 {
        std::f32::consts::PI
      } else {
        -std::f32::consts::PI
      }
    } else {
      let angle = m0.dot(m1).clamp(-1.0, 1.0).acos();
      if m0.perp_dot(m1) >= 0.0 {
        angle
      } else {
        -angle
      }
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
      angle,
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

/// The vertices at one end of a segment's quad: (left, right) by the
/// normal, and the corner point where the inner side splits.
#[cfg(test)]
struct End {
  left: u32,
  right: u32,
  split: Option<u32>,
}

#[cfg(test)]
struct Builder<'a> {
  out: &'a mut Triangles,
  window: Window,
  style: &'a PolyStyle,
  /// Its line's entry in the line table
  line: u32,
  /// The piece's corner point vertex, which unused triangles collapse to
  first: Option<u32>,
}

#[cfg(test)]
impl Builder<'_> {
  fn join_kind(&self) -> LineJoin {
    self.style.join
  }

  fn vertex(&mut self, at: Vec2, point: &PolyPoint, uv: [f32; 2], across: Vec2, m: Measure) -> u32 {
    let index = self.out.vertices.len() as u32;
    let Color { r, g, b, a } = point.color;
    let limit = if m.limit > 0.0 {
      m.limit.min(NO_EDGE)
    } else {
      0.0
    };
    let mut vertex = Vertex2D {
      position: at.into(),
      uv,
      color: [r, g, b, a],
      across: across.into(),
      line: self.line,
      flags: 0,
      k: 0.0,
      dist: m.dist,
      second: NO_EDGE,
      half: m.half,
      push: m.push.into(),
      limit,
      dist_rate: m.push.dot(m.normal),
      second_rate: 0.0,
      effect_u: if m.point_u || at == point.pos {
        point.u
      } else {
        uv[0]
      },
    };
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

  /// A vertex at the corner point of piece point `j`, never pushed, which
  /// the triangles a piece leaves unused collapse to.
  fn collapse_point(&mut self, j: i32) -> u32 {
    if let Some(first) = self.first {
      return first;
    }
    let Some(point) = self.window.get(j).copied() else {
      return 0;
    };
    let index = self.vertex(
      point.pos,
      &point,
      [point.u, 0.5],
      Vec2::ZERO,
      Measure::centre(point.half_width),
    );
    self.first = Some(index);
    index
  }

  /// `count` triangles of no area, in the slots a piece leaves unused.
  fn collapsed(&mut self, j: i32, count: u32) {
    let at = self.collapse_point(j);
    for _ in 0..count {
      self.triangle(at, at, at);
    }
  }

  /// The quad from point 0 to point 1, fanned from a split end's corner
  /// point: four triangles.
  fn segment(&mut self) {
    let w = self.window;
    let join = self.join_kind();
    let (Some(_), Some(_), Some(segment)) = (w.get(0), w.get(1), w.segment(0)) else {
      self.collapsed(0, SEGMENT_TRIANGLES);
      return;
    };
    let limit = w.quad_limit(0, join);
    let second = w.segment_second(0);
    let from = self.end(0, segment, limit, second, true);
    let to = self.end(1, segment, limit, second, false);
    // The quad's outline, fanned from `to`'s corner point (or its left
    // vertex): left, (corner point), right at the start, right, (corner
    // point), left at the end. Where an end has no corner point its
    // triangles have no area, and where neither has, the quad is cut from
    // its start's right to its end's left
    let fan = to.split.unwrap_or(to.left);
    let start_split = from.split.unwrap_or(from.left);
    self.triangle(fan, to.left, from.left);
    self.triangle(fan, from.left, start_split);
    self.triangle(fan, start_split, from.right);
    self.triangle(fan, from.right, to.right);
  }

  /// The vertices at point `j` (0 or 1) of the segment from point 0, as its
  /// quad's start (`start`) or end: an open end's (with its cap), a
  /// straight-on pair, or a corner's.
  fn end(
    &mut self,
    j: i32,
    segment: Segment,
    limit: f32,
    second: Option<Second>,
    start: bool,
  ) -> End {
    let w = self.window;
    let join = self.join_kind();
    let Some(point) = w.get(j).copied() else {
      return End {
        left: 0,
        right: 0,
        split: None,
      };
    };
    let (p, hw, n) = (point.pos, point.half_width, segment.normal);
    let u = if start { point.u } else { w.end_u(j) };
    match Joint::at(&w, j, join).shape {
      Shape::End => {
        let (left, right) = self.open_pair(&point, segment, limit, second, start);
        // A round cap fans from the end point, which the quad's end runs
        // through: the quad has a vertex there too, moved by effects as its
        // end is
        let split = (self.style.cap == LineCap::Round)
          .then(|| self.vertex(p, &point, [u, 0.5], n, Measure::centre(hw)));
        End { left, right, split }
      }
      Shape::Straight { normal } => {
        let both = w.both(j, join);
        let pair = |side: f32| {
          Measure::edge(side, hw, normal, side * normal, both)
            .with(second)
            .shared()
        };
        End {
          left: self.vertex(p + normal * hw, &point, [u, 0.0], normal, pair(1.0)),
          right: self.vertex(p - normal * hw, &point, [u, 1.0], normal, pair(-1.0)),
          split: None,
        }
      }
      Shape::Turn(turn) => {
        let side = turn.side;
        let (v_out, v_in) = if side > 0.0 { (0.0, 1.0) } else { (1.0, 0.0) };
        let m = side * n;
        // Where the inner side splits, the end's edge pushes straight out,
        // keeping to its line through the corner point; the join's fringe
        // triangles take it on to the corner's own push
        let push = match (turn.shares_inside, start) {
          (false, _) => m,
          (true, true) => turn.push.1,
          (true, false) => turn.push.0,
        };
        let outer = self.vertex(
          p + m * hw,
          &point,
          [u, v_out],
          n,
          Measure::edge(side, hw, n, push, limit).with(second),
        );
        let (inner, split) = if turn.shares_inside {
          let both = w.both(j, join);
          let measure = Measure::edge(-side, hw, n, -side * turn.miter, both)
            .with(second)
            .shared();
          let at = p - side * turn.miter * hw;
          (
            self.vertex(at, &point, [u, v_in], turn.bisector, measure),
            None,
          )
        } else {
          // Too sharp to share: the inner vertex pushes opposite the outer,
          // so the end keeps passing through the corner point, where the
          // join fans from and the quad has a vertex of its own
          let inner = self.vertex(
            p - m * hw,
            &point,
            [u, v_in],
            n,
            Measure::edge(-side, hw, n, -push, limit).with(second),
          );
          // Moved by effects along the corner's bisector, as every piece's
          // vertex at the corner point is
          let centre = self.vertex(
            p,
            &point,
            [u, 0.5],
            turn.bisector,
            Measure::edge(0.0, hw, n, Vec2::ZERO, 0.0).with(second),
          );
          (inner, Some(centre))
        };
        let (left, right) = if side > 0.0 {
          (outer, inner)
        } else {
          (inner, outer)
        };
        End { left, right, split }
      }
    }
  }

  /// The edge vertices at an open end of `segment` (the line's first point
  /// when `start`), measuring the cap's `second` edge: a butt or square
  /// cap's corners push out across both of their edges, a round cap's
  /// straight out, as far as the rest of their quad may.
  fn open_pair(
    &mut self,
    point: &PolyPoint,
    segment: Segment,
    limit: f32,
    second: Option<Second>,
    start: bool,
  ) -> (u32, u32) {
    let (p, hw, n, u) = (point.pos, point.half_width, segment.normal, point.u);
    let out = if start { -segment.dir } else { segment.dir };
    let measure = |side: f32| match self.style.cap {
      LineCap::Round => Measure::edge(side, hw, n, side * n, limit),
      _ => Measure::edge(side, hw, n, side * n + out, limit).with(second),
    };
    let (left, right) = (measure(1.0), measure(-1.0));
    (
      self.vertex(p + n * hw, point, [u, 0.0], n, left),
      self.vertex(p - n * hw, point, [u, 1.0], n, right),
    )
  }

  /// The corner at point 0: the slanted ends of its segments where their
  /// inner side is shared, and its outside (a miter's point, a bevel or a
  /// round's arc), each with vertices of its own at the strip's places,
  /// pushed as the strip's are.
  fn join(&mut self) {
    let w = self.window;
    let join = self.join_kind();
    let count = piece_triangles(PieceKind::Join, join, self.style.cap);
    let (Some(point), Some(before), Some(after), Shape::Turn(turn)) = (
      w.get(0).copied(),
      w.segment(-1),
      w.segment(0),
      Joint::at(&w, 0, join).shape,
    ) else {
      self.collapsed(0, count);
      return;
    };
    let (p, hw, u) = (point.pos, point.half_width, point.u);
    let u_end = w.end_u(0);
    let (n0, n1) = (before.normal, after.normal);
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
    let (limit0, limit1) = (w.quad_limit(-1, join), w.quad_limit(0, join));
    let both = w.both(0, join);
    let (v_out, v_in) = if side > 0.0 { (0.0, 1.0) } else { (1.0, 0.0) };
    let (m0, m1) = (side * n0, side * n1);
    let centre = self.vertex(p, &point, [u, 0.5], bisector, Measure::centre(hw));
    self.first = Some(centre);
    let outer_end = self.vertex(
      p + m0 * hw,
      &point,
      [u_end, v_out],
      n0,
      Measure::edge(side, hw, n0, push0, limit0),
    );
    let outer_start = self.vertex(
      p + m1 * hw,
      &point,
      [u, v_out],
      n1,
      Measure::edge(side, hw, n1, push1, limit1),
    );
    // The shared inner vertex, as each segment has it
    let inner = shares_inside.then(|| {
      let at = p - side * miter * hw;
      let end = self.vertex(
        at,
        &point,
        [u_end, v_in],
        bisector,
        Measure::edge(-side, hw, n0, -side * miter, both).shared(),
      );
      let start = self.vertex(
        at,
        &point,
        [u, v_in],
        bisector,
        Measure::edge(-side, hw, n1, -side * miter, both).shared(),
      );
      (end, start)
    });
    let bevelled = !mitered && join != LineJoin::Round;
    // Where the inner side splits, the strip's elbow as the segment pushes
    // it, straight out, measuring what the corner's fan measures: its
    // fringe triangle joins the segment's end to the fan
    let strip = |b: &mut Self, end: bool, second: Option<Second>| {
      let (m, n, at_u, limit) = if end {
        (m0, n0, u_end, limit0)
      } else {
        (m1, n1, u, limit1)
      };
      b.vertex(
        p + m * hw,
        &point,
        [at_u, v_out],
        n,
        Measure::edge(side, hw, n, m, limit).with(second),
      )
    };
    if bevelled {
      let edge = Second::edge(p + m0 * hw, turn.bevel, m0.dot(turn.bevel));
      let strips = (!shares_inside).then(|| {
        (
          strip(self, true, Some(edge)),
          strip(self, false, Some(edge)),
        )
      });
      self.bevel(
        &point,
        &turn,
        (n0, n1),
        (outer_end, outer_start),
        inner,
        strips,
      );
      return;
    }
    // The fan's own centre and ends: a miter's point, or a round's arc
    let (fan_centre, fan_end, fan_start, strips) = if mitered {
      // Each half of the point measures its segment across and the other
      // segment's outer edge as its second, the two edges `cos` apart
      let other0 = Second::edge(p + m1 * hw, m1, cos);
      let other1 = Second::edge(p + m0 * hw, m0, cos);
      let point_centre = self.vertex(
        p,
        &point,
        [u, 0.5],
        bisector,
        Measure::centre(hw).with(Some(other0)),
      );
      let end = self.copy(outer_end, |v| measure_second(v, other0));
      let start = self.copy(outer_start, |v| measure_second(v, other1));
      let strips = (!shares_inside).then(|| {
        (
          strip(self, true, Some(other0)),
          strip(self, false, Some(other1)),
        )
      });
      (point_centre, end, start, strips)
    } else {
      let round_centre = self.vertex(
        p,
        &point,
        [u, 0.5],
        bisector,
        Measure::round(hw, Vec2::ZERO, Vec2::ZERO, 0.0),
      );
      // The arc's ends push straight out already, as the strip's elbows do
      let first = self.copy(outer_end, |v| set_round(v, m0 * hw));
      let last = self.copy(outer_start, |v| set_round(v, m1 * hw));
      (
        round_centre,
        first,
        last,
        (!shares_inside).then_some((first, last)),
      )
    };
    // The slanted ends measure their segment's strip alone; where the inner
    // side splits, the fringe triangles take their slots
    match (inner, strips) {
      (Some((end, _)), _) => self.wedge(side, centre, end, outer_end),
      (None, Some((strip_end, _))) => self.wedge(side, fan_centre, strip_end, fan_end),
      (None, None) => self.triangle(centre, centre, centre),
    }
    if mitered {
      let other0 = Second::edge(p + m1 * hw, m1, cos);
      let tip = self.vertex(
        p + side * miter * hw,
        &point,
        [u, v_out],
        bisector,
        Measure::edge(side, hw, n0, side * miter, NO_EDGE).with(Some(other0)),
      );
      self.wedge(side, fan_centre, fan_end, tip);
      self.wedge(side, fan_centre, tip, fan_start);
    } else {
      let arc = |angle: f32| {
        (
          Vec2::from_angle(angle).rotate(m0),
          Vec2::from_angle(angle).rotate(n0),
        )
      };
      self.arc(
        (&point, fan_centre, side),
        (fan_end, fan_start),
        turn.angle,
        [u, v_out],
        arc,
      );
    }
    match (inner, strips) {
      (Some((_, start)), _) => self.wedge(side, centre, outer_start, start),
      (None, Some((_, strip_start))) => self.wedge(side, fan_centre, fan_start, strip_start),
      (None, None) => self.triangle(centre, centre, centre),
    }
  }

  /// A round piece's arc of 16 triangles from `first` to `last` (the
  /// strip's outer vertices, as round vertices), fanned from `centre` over
  /// `angle`, circumscribed: the corners between them at the half-step
  /// angles, `hw / cos(step / 2)` from the corner point, pushed by
  /// `1 / cos(step / 2)` units per unit. `towards(a)` gives the outward
  /// direction and the `across` `a` radians round; `side` winds the fan like
  /// the strip.
  fn arc(
    &mut self,
    (point, centre, side): (&PolyPoint, u32, f32),
    (first, last): (u32, u32),
    angle: f32,
    uv: [f32; 2],
    towards: impl Fn(f32) -> (Vec2, Vec2),
  ) {
    let (p, hw) = (point.pos, point.half_width);
    let steps = ARC_TRIANGLES - 1;
    let step = angle / steps as f32;
    let scale = 1.0 / (step / 2.0).cos();
    let mut previous = first;
    for k in 0..steps {
      let (outward, across) = towards(step * (k as f32 + 0.5));
      let offset = outward * (hw * scale);
      let arc = self.vertex(
        p + offset,
        point,
        uv,
        across,
        Measure::round(hw, offset, outward * scale, NO_EDGE),
      );
      self.wedge(side, centre, previous, arc);
      previous = arc;
    }
    self.wedge(side, centre, previous, last);
  }

  /// A bevel at point 0: a fan from the corner point on each side, through
  /// copies of the inner vertex (when the segments share it) and of the
  /// strip's elbow, to the bevel's middle. Every triangle of it measures its
  /// segment across and the bevel's edge as its second, the two edges
  /// `m · bevel` apart.
  fn bevel(
    &mut self,
    point: &PolyPoint,
    turn: &Turn,
    (n0, n1): (Vec2, Vec2),
    (outer_end, outer_start): (u32, u32),
    inner: Option<(u32, u32)>,
    strips: Option<(u32, u32)>,
  ) {
    let (p, hw, u) = (point.pos, point.half_width, point.u);
    let Turn {
      side,
      cos,
      bisector,
      bevel,
      ..
    } = *turn;
    let v_out = if side > 0.0 { 0.0 } else { 1.0 };
    let (m0, m1) = (side * n0, side * n1);
    let edge = Second::edge(p + m0 * hw, bevel, m0.dot(bevel));
    let centre = self.vertex(
      p,
      point,
      [u, 0.5],
      bisector,
      Measure::centre(hw).with(Some(edge)),
    );
    let inner = inner.map(|(end, start)| {
      (
        self.copy(end, |v| measure_second(v, edge)),
        self.copy(start, |v| measure_second(v, edge)),
      )
    });
    let elbow_end = self.copy(outer_end, |v| measure_second(v, edge));
    let elbow_start = self.copy(outer_start, |v| measure_second(v, edge));
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
        second: Some(edge),
        round: None,
        push: bevel,
        limit: NO_EDGE,
        point_u: false,
      },
    );
    // Where the inner side splits, a fringe triangle each side from the
    // strip's elbow, pushed straight out, to the fan's
    match (inner, strips) {
      (Some((end, _)), _) => self.wedge(side, centre, end, elbow_end),
      (None, Some((strip_end, _))) => self.wedge(side, centre, strip_end, elbow_end),
      (None, None) => self.triangle(centre, centre, centre),
    }
    self.wedge(side, centre, elbow_end, middle);
    self.wedge(side, centre, middle, elbow_start);
    match (inner, strips) {
      (Some((_, start)), _) => self.wedge(side, centre, elbow_start, start),
      (None, Some((_, strip_start))) => self.wedge(side, centre, elbow_start, strip_start),
      (None, None) => self.triangle(centre, centre, centre),
    }
  }

  /// The cap at point 0, the line's first point (`start`) or its last: a
  /// square cap's quad, a round cap's fan, nothing for a butt cap.
  fn cap(&mut self, start: bool) {
    let w = self.window;
    let join = self.join_kind();
    let (Some(point), Some(near)) = (
      w.get(0).copied(),
      w.get(if start { 1 } else { -1 }).copied(),
    ) else {
      self.collapsed(
        0,
        piece_triangles(PieceKind::StartCap, join, self.style.cap),
      );
      return;
    };
    let (segment, limit, second, scale) = if start {
      let other_end = w.get(2).is_none();
      (
        Segment::between(point.pos, near.pos),
        w.quad_limit(0, join),
        end_second(&point, &near, self.style.cap, true, other_end),
        -w.u_scale(0),
      )
    } else {
      let other_end = w.get(-2).is_none();
      (
        Segment::between(near.pos, point.pos),
        w.quad_limit(-1, join),
        end_second(&near, &point, self.style.cap, other_end, true),
        w.u_scale(-1),
      )
    };
    let (p, hw, n, u) = (point.pos, point.half_width, segment.normal, point.u);
    let out = if start { -segment.dir } else { segment.dir };
    let (left, right) = self.open_pair(&point, segment, limit, second, start);
    match self.style.cap {
      LineCap::Butt => {}
      LineCap::Square => {
        let corner = |side: f32| Measure::edge(side, hw, n, side * n + out, limit).with(second);
        let far_u = u + hw * scale;
        let far_left = self.vertex(p + n * hw + out * hw, &point, [far_u, 0.0], n, corner(1.0));
        let far_right = self.vertex(p - n * hw + out * hw, &point, [far_u, 1.0], n, corner(-1.0));
        // Along the line's direction, like the strip
        if start {
          self.quad((far_left, far_right), (left, right));
        } else {
          self.quad((left, right), (far_left, far_right));
        }
      }
      LineCap::Round => {
        // The fan measures its distance from the end point, from the left
        // edge round the outside to the right edge
        // Moved by effects across the segment, as the segment's vertex
        // at the end point is
        let centre = self.vertex(
          p,
          &point,
          [u, 0.5],
          n,
          Measure::round(hw, Vec2::ZERO, Vec2::ZERO, 0.0),
        );
        let first = self.copy(left, |v| set_round(v, n * hw));
        let last = self.copy(right, |v| set_round(v, -n * hw));
        let turn = if start { 1.0 } else { -1.0 };
        let towards = |angle: f32| (Vec2::from_angle(turn * angle).rotate(n), n);
        // `u` runs on past the end with the offset along the line
        let base = self.out.vertices.len();
        self.arc(
          (&point, centre, -turn),
          (first, last),
          std::f32::consts::PI,
          [u, 0.5],
          towards,
        );
        for vertex in self.out.vertices.get_mut(base..).into_iter().flatten() {
          let offset = Vec2::from(vertex.position) - p;
          let outward = offset.normalize_or_zero();
          vertex.uv = [u + offset.dot(out) * scale, 0.5 - 0.5 * outward.dot(n)];
          vertex.effect_u = vertex.uv[0];
        }
      }
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
    // One segment, which measures both butt caps as a strip along it: four
    // triangles, two of them of no area as neither end splits
    assert_eq!(t.polyline(&points, &style), 0..12);
  }

  #[test]
  fn no_triangle_lies_along_one_edge() {
    // Edge anti-aliasing reads `v` as distance from the centre: a triangle
    // whose corners all sit on one edge would fade away
    for join in [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round] {
      for cap in [LineCap::Butt, LineCap::Square, LineCap::Round] {
        let t = build(&L, 2.0, join, cap, false);
        for tri in t.indices.chunks(3) {
          let [a, b, c] = [0, 1, 2].map(|k| Vec2::from(t.vertices[tri[k] as usize].position));
          if (b - a).perp_dot(c - a) == 0.0 {
            // A triangle a piece leaves unused draws nothing
            continue;
          }
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
  fn a_vertex_is_76_bytes_of_plain_numbers() {
    // Place and uv, an sRGB colour, the direction across, the line, flags
    // and angle between edges, three distances (or a round offset and the
    // half width), the push, its limit and rates, and the effects' `u`, all
    // in f32
    assert_eq!(std::mem::size_of::<Vertex2D>(), 76);
    assert_eq!(std::mem::size_of::<LineInfo>(), 80);
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
      let round = v.flags & FLAG_ROUND != 0;
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
        dist: s.dist + v.dist_rate,
        second: s.second - v.second_rate,
        round: if v.flags & FLAG_ROUND != 0 {
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

  /// Near enough for a pushed vertex
  fn near_pushed(a: f32, b: f32) -> bool {
    near(a, b)
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
    let (flags, k) = (tri[0].flags, tri[0].k);
    if tri.iter().any(|v| v.flags != flags || v.k != k) {
      return Err("its vertices' flags or angles differ".into());
    }
    let samples = Sample::over(tri);
    let moved: Vec<Sample> = tri
      .iter()
      .filter(|v| pushed && v.limit > 0.0)
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
      // Straight out, keeping its place on the circumscribed polygon: its
      // offset is the half width times its push, so one unit on it is the
      // half width and one times its push
      for v in tri.iter().filter(|v| pushed && v.limit > 0.0) {
        let (round, push) = (Sample::of(v).round, v.push());
        let out = push * v.half;
        if !(near_pushed(round.x, out.x) && near_pushed(round.y, out.y)) {
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
            for v in tri.iter().filter(|v| v.limit == 0.0) {
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
      .map(|v| Vec2::from(v.position) + v.push() * p.min(v.limit))
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
    // drawn unpushed (no gap), and no point twice but where a corner's
    // inner side splits: its strips overlap on its inside, pushed or not,
    // within a half width and `p` of both its segments (the short segments
    // of such a line come that close to others too, and a pushed cap's
    // corner reaches √2 times that)
    let shapes = [
      Shape::even(
        &[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (4.0, 2.0), (5.0, 0.5)],
        1.0,
        false,
      ),
      Shape::even(&[(0.0, 0.0), (10.0, 0.0), (2.0, 3.0)], 1.0, false),
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
            // Where a corner's inner side splits, its strips overlap: and a
            // line whose segments come within a half width and `p` of each
            // other overlaps itself there, pushed. Not counted: points that
            // close to two of its segments, on a line with a split corner
            let n = shape.points.len();
            let splits = (1..n - 1).any(|i| {
              let (a, c, b) = (shape.at(i - 1), shape.at(i), shape.at(i + 1));
              let shortest = c.distance(a).min(c.distance(b));
              let (n0, n1) = ((c - a).normalize().perp(), (b - c).normalize().perp());
              let miter = (n0 + n1) / (1.0 + n0.dot(n1));
              let shares = miter.length() * shape.half(i) <= shortest / 2.0;
              !shares
            });
            let overlap = |at: Vec2| {
              let near = (0..n - 1)
                .filter(|&i| {
                  let (a, b) = (shape.at(i), shape.at(i + 1));
                  let t = ((at - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
                  // (a cap's corner reaches across both edges)
                  at.distance(a + (b - a) * t) <= (shape.half(i) + p) * std::f32::consts::SQRT_2
                })
                .count();
              splits && near >= 2
            };
            for y in 0..grid.size.1 {
              for x in 0..grid.size.0 {
                let (count, near_line) = (covered[y * grid.size.0 + x], close[y * grid.size.0 + x]);
                let what = format!(
                  "shape {index} {join:?} {cap:?} p {p} at {}",
                  grid.point(x, y)
                );
                assert!(
                  count <= 1 || overlap(grid.point(x, y)),
                  "covered {count} times: {what}"
                );
                // (pushed past a pixel, a short segment's quad stops at its
                // limit, by design)
                let gaps_count = !splits || p <= 1.0;
                assert!(count >= 1 || !near_line || !gaps_count, "a gap: {what}");
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
    let a = p.min(v.limit);
    let s = Sample::of(v);
    Landed {
      pos: s.pos + v.push() * a,
      dist: s.dist + v.dist_rate * a,
      half: s.half,
      second: s.second - v.second_rate * a,
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
    let flags = tri[0].flags;
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
    cover_corner(across, second, tri[0].k)
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

  /// A corner whose inner side splits: its two strips overlap on its
  /// inside, where the coverage is the two strips' and not measured
  #[derive(Clone, Copy, Debug)]
  struct Overlap {
    at: Vec2,
    /// The outside of the turn
    out: Vec2,
    segments: [(Vec2, Vec2); 2],
    hw: f32,
  }

  impl Overlap {
    /// Whether a pixel centred at `at` lies inside the corner, within a
    /// pixel of both strips
    fn holds(&self, at: Vec2) -> bool {
      let near = |(a, b): (Vec2, Vec2)| {
        let t = ((at - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
        at.distance(a + (b - a) * t) <= self.hw + 1.0
      };
      (at - self.at).dot(self.out) < 0.0 && self.segments.iter().all(|&s| near(s))
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
    overlap: &[Overlap],
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
          if at.distance(centre) > reach || overlap.iter().any(|o| o.holds(at)) {
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
                .get(tri[0].line as usize)
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
    // A split corner, near its corner point (its segments' ends and the
    // fan meet there, and the bevel or the arc cuts close past them): the
    // recorded figure, by turn, for segments of 0.5, 1.0 and 1.9 times the
    // miter's reach (`miter · hw`; a corner splits below twice it)
    const SPLIT_LENGTHS: [f32; 3] = [0.5, 1.0, 1.9];
    const SPLIT_TURNS: [f32; 5] = [60.0, 90.0, 120.0, 150.0, 165.0];
    const SPLIT: [[f32; 5]; 3] = [
      [0.12, 0.16, 0.23, 0.33, 0.38],
      [0.12, 0.13, 0.22, 0.33, 0.38],
      [0.12, 0.13, 0.22, 0.33, 0.38],
    ];
    let mut split_worst: [[(f32, String); 5]; 3] = Default::default();
    let mut split_inner = (0.0f32, String::new());
    let offsets = [
      Vec2::ZERO,
      Vec2::new(0.25, 0.1),
      Vec2::new(0.5, 0.6),
      Vec2::new(0.8, 0.35),
    ];
    let mut failures = Vec::new();
    let mut inner_worst = (0.0f32, String::new());
    let mut outer_worst = (0.0f32, String::new());
    let mut check = |what: String,
                     t: &Triangles,
                     around: &[Vec2],
                     reach: f32,
                     inner: &[Inner],
                     overlap: &[Overlap]| {
      let ((error, at), (near, near_at)) = worst_coverage(t, around, reach, inner, overlap);
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
                check(what, &t, &[p], 5.0 * hw + 3.0, &inner, &[]);
              }
            }
          }
          // Corners whose inner side splits: segments shorter than twice
          // the miter's reach, their overlap inside the corner unmeasured
          for (length_index, &ratio) in SPLIT_LENGTHS.iter().enumerate() {
            for (turn_index, &degrees) in SPLIT_TURNS.iter().enumerate() {
              for turn in [1.0f32, -1.0] {
                let d1 = Vec2::from_angle(turn * degrees.to_radians()).rotate(d0);
                let miter = (d0.perp() + d1.perp()) / (1.0 + d0.perp().dot(d1.perp()));
                let short = ratio * miter.length() * hw;
                let (a, b) = (p - d0 * short, p + d1 * short);
                let points = [a, p, b].map(|q| (q.x, q.y));
                let out = if d0.perp_dot(d1) > 0.0 { -miter } else { miter };
                let overlap = [Overlap {
                  at: p,
                  out,
                  segments: [(a, p), (p, b)],
                  hw,
                }];
                // Within a pixel of the corner point, or near it within a
                // pixel of both segments' ends (which meet there), a pixel lies
                // across both strips: the inner corner's zone, as at a shared
                // vertex
                let inner = [Inner {
                  at: p,
                  edges: [(p, d0), (p, d1)],
                }];
                for join in JOINS {
                  let t = build(&points, 2.0 * hw, join, LineCap::Butt, false);
                  let what = format!(
                    "split {ratio} {join:?} {degrees}° {turn} hw {hw} rot {rotation} {offset}"
                  );
                  let ((error, at), (near, near_at)) =
                    worst_coverage(&t, &[p], 3.0 * hw + 3.0, &inner, &overlap);
                  let worst = &mut split_worst[length_index][turn_index];
                  if error > worst.0 {
                    *worst = (error, format!("{what} at {at}"));
                  }
                  if near > split_inner.0 {
                    split_inner = (near, format!("{what} at {near_at}"));
                  }
                }
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
              check(what, &t, &ends, 2.0 * hw + 3.0, &[], &[]);
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
          check(what, &t, &corners, 5.0 * hw + 3.0, &inner, &[]);
        }
      }
    }
    eprintln!("worst away from inner corners: {outer_worst:?}");
    eprintln!("worst at an inner corner: {inner_worst:?}");
    eprintln!("worst at a split corner's point: {split_inner:?}");
    assert!(
      split_inner.0 <= INNER,
      "a split corner's point is off by {split_inner:?}"
    );
    for (length_index, ratio) in SPLIT_LENGTHS.iter().enumerate() {
      for (turn_index, degrees) in SPLIT_TURNS.iter().enumerate() {
        let (error, at) = &split_worst[length_index][turn_index];
        let limit = SPLIT[length_index][turn_index];
        eprintln!("worst at a split {degrees}° corner, segments {ratio} miters: {error:.3} ({at})");
        assert!(
          *error <= limit,
          "a split {degrees}° corner, segments {ratio} miters, is off by {error}: {at}"
        );
      }
    }
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

  fn points_of(points: &[(f32, f32)], half: f32) -> Vec<PolyPoint> {
    points
      .iter()
      .enumerate()
      .map(|(i, &(x, y))| PolyPoint {
        pos: Vec2::new(x, y),
        half_width: half,
        color: Color::WHITE,
        u: i as f32,
      })
      .collect()
  }

  fn style(join: LineJoin, cap: LineCap, closed: bool) -> PolyStyle {
    PolyStyle {
      join,
      cap,
      closed,
      params: [0.0; 4],
      emission: 1.0,
    }
  }

  #[test]
  fn the_filter_copies_points_before_any_piece() {
    let at = |points: &[PolyPoint]| points.iter().map(|p| p.pos).collect::<Vec<_>>();
    let mut out = Vec::new();
    // Non-finite points and points within 1e-4 of the last go; `u` stays
    let mut points = points_of(&[(0.0, 0.0), (5.0, 0.0), (5.00005, 0.0), (5.0, 5.0)], 1.0);
    points.insert(
      1,
      PolyPoint {
        pos: Vec2::new(f32::NAN, 1.0),
        ..points[0]
      },
    );
    assert_eq!(filter_points(&points, false, &mut out), Some(false));
    assert_eq!(
      at(&out),
      [Vec2::ZERO, Vec2::new(5.0, 0.0), Vec2::new(5.0, 5.0)]
    );
    assert_eq!(out.iter().map(|p| p.u).collect::<Vec<_>>(), [0.0, 1.0, 3.0]);
    // A closed line's repeated end goes, and it draws as the same line
    let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let mut repeated = square.to_vec();
    repeated.push((0.0, 0.0));
    assert_eq!(
      filter_points(&points_of(&repeated, 1.0), true, &mut out),
      Some(true)
    );
    assert_eq!(out.len(), 4);
    let a = build(&square, 2.0, LineJoin::Miter, LineCap::Butt, true);
    let b = build(&repeated, 2.0, LineJoin::Miter, LineCap::Butt, true);
    assert_eq!(a.indices, b.indices);
    assert_eq!(a.vertices.len(), b.vertices.len());
    // A closed line under three points is open; under two, or of no width,
    // it draws nothing
    let two = points_of(&[(0.0, 0.0), (10.0, 0.0)], 1.0);
    assert_eq!(filter_points(&two, true, &mut out), Some(false));
    assert_eq!(filter_points(&two[..1], false, &mut out), None);
    assert_eq!(
      filter_points(&points_of(&square, 0.0), true, &mut out),
      None
    );
  }

  #[test]
  fn the_piece_list_depends_only_on_where_a_point_sits() {
    // Straight on, gentle, split and doubling back: the same pieces and the
    // same triangles, in the order the list gives
    let shapes = |n: usize| -> Vec<Vec<(f32, f32)>> {
      let line = (0..n).map(|i| (i as f32 * 10.0, 0.0)).collect();
      let gentle = (0..n)
        .map(|i| (i as f32 * 10.0, (i % 2) as f32 * 2.0))
        .collect();
      let sharp = (0..n)
        .map(|i| (if i % 2 == 0 { 0.0 } else { 3.0 }, i as f32 * 0.5))
        .collect();
      let back = (0..n)
        .map(|i| (if i % 2 == 0 { 0.0 } else { 10.0 }, i as f32 * 0.01))
        .collect();
      vec![line, gentle, sharp, back]
    };
    let mut list = Vec::new();
    for n in 2..=6 {
      for closed in [false, true] {
        for join in JOINS {
          for cap in CAPS {
            for points in shapes(n) {
              let t = build(&points, 2.0, join, cap, closed);
              let mut kept = Vec::new();
              let closed =
                filter_points(&points_of(&points, 1.0), closed, &mut kept).expect("draws");
              assert_eq!(kept.len(), n, "{points:?}");
              piece_list(n, closed, cap, &mut list);
              let triangles: u32 = list
                .iter()
                .map(|piece| piece_triangles(piece.kind, join, cap))
                .sum();
              assert_eq!(
                t.indices.len() as u32,
                3 * triangles,
                "{n} {closed} {join:?} {cap:?} {points:?}"
              );
            }
          }
        }
      }
    }
    piece_list(4, false, LineCap::Round, &mut list);
    let kinds: Vec<(PieceKind, u32)> = list.iter().map(|p| (p.kind, p.point)).collect();
    use PieceKind::*;
    assert_eq!(
      kinds,
      [
        (StartCap, 0),
        (Segment, 0),
        (Join, 1),
        (Segment, 1),
        (Join, 2),
        (Segment, 2),
        (EndCap, 3)
      ]
    );
    piece_list(3, true, LineCap::Round, &mut list);
    let kinds: Vec<(PieceKind, u32)> = list.iter().map(|p| (p.kind, p.point)).collect();
    assert_eq!(
      kinds,
      [
        (Segment, 0),
        (Join, 1),
        (Segment, 1),
        (Join, 2),
        (Segment, 2),
        (Join, 0)
      ]
    );
  }

  /// One piece of the line through `pts`, alone.
  fn piece_alone(pts: &[PolyPoint], closed: bool, style: &PolyStyle, piece: Piece) -> Triangles {
    let mut t = Triangles::default();
    t.piece(pts, closed, style, 0, piece);
    t
  }

  #[test]
  fn a_piece_reads_only_its_window() {
    // A segment from point 4 reads points 2 to 7, a join or a cap at point
    // 4 points 2 to 6: moving the rest changes nothing it builds
    let zigzag: Vec<(f32, f32)> = (0..9)
      .map(|i| {
        (
          i as f32 * 4.0,
          if i % 2 == 0 { 0.0 } else { 3.0 + i as f32 },
        )
      })
      .collect();
    let pts = points_of(&zigzag, 1.0);
    let moved = |outside: &[usize]| {
      let mut moved = pts.clone();
      for &i in outside {
        moved[i].pos += Vec2::new(0.7, -1.3);
      }
      moved
    };
    let cases = [
      (PieceKind::Segment, false, moved(&[0, 1, 8])),
      (PieceKind::Join, false, moved(&[0, 1, 7, 8])),
      (PieceKind::Segment, true, moved(&[0, 1, 8])),
      (PieceKind::Join, true, moved(&[0, 1, 7, 8])),
    ];
    for join in JOINS {
      for cap in CAPS {
        for (kind, closed, other) in &cases {
          let style = style(join, cap, *closed);
          let piece = Piece {
            kind: *kind,
            point: 4,
          };
          let a = piece_alone(&pts, *closed, &style, piece);
          let b = piece_alone(other, *closed, &style, piece);
          assert_eq!(a.vertices, b.vertices, "{kind:?} {closed} {join:?} {cap:?}");
          assert_eq!(a.indices, b.indices);
        }
        // A cap at the line's first point reads points 0 to 2
        let style = style(join, cap, false);
        let piece = Piece {
          kind: PieceKind::StartCap,
          point: 0,
        };
        let a = piece_alone(&pts, false, &style, piece);
        let b = piece_alone(&moved(&[3, 4, 5, 6, 7, 8]), false, &style, piece);
        assert_eq!(a.vertices, b.vertices, "{join:?} {cap:?}");
      }
    }
  }

  #[test]
  fn round_pieces_are_contained_and_true_at_a_half_width_of_150() {
    // Every point of the disc's edge inside a round piece's angle lies in
    // one of its triangles, unpushed and pushed a unit (the polygon is
    // circumscribed), and its attributes are true over every triangle
    const HW: f32 = 150.0;
    let p = Vec2::new(5000.0, 5000.0);
    let mut cases = Vec::new();
    for degrees in [30.0f32, 90.0, 165.0, 179.0] {
      for turn in [1.0f32, -1.0] {
        let d1 = Vec2::from_angle(turn * degrees.to_radians());
        cases.push((
          vec![p - Vec2::X * 2000.0, p, p + d1 * 2000.0],
          LineCap::Butt,
        ));
      }
    }
    let d = Vec2::from_angle(0.3);
    cases.push((vec![p, p + d * 2000.0], LineCap::Round));
    for (points, cap) in cases {
      let pts: Vec<(f32, f32)> = points.iter().map(|q| (q.x, q.y)).collect();
      let shape = Shape::even(&pts, HW, false);
      let t = shape.build(LineJoin::Round, cap);
      for tri in triangles_of(&t) {
        for pushed in [false, true] {
          if let Err(why) = fields(&shape, cap, &tri, pushed) {
            panic!("{pts:?}: {why}");
          }
        }
      }
      let round: Vec<[Vertex2D; 3]> = triangles_of(&t)
        .filter(|tri| tri[0].flags & FLAG_ROUND != 0)
        .collect();
      // Each round piece's corner, and the directions its arc covers
      let mut arcs: Vec<(Vec2, Vec2, f32)> = Vec::new();
      if cap == LineCap::Round {
        let (a, b) = (points[0], points[1]);
        let n = (b - a).normalize().perp();
        arcs.push((a, n, std::f32::consts::PI));
        arcs.push((b, -n, std::f32::consts::PI));
      } else {
        let (d0, d1) = (
          (points[1] - points[0]).normalize(),
          (points[2] - points[1]).normalize(),
        );
        let side = if d0.perp_dot(d1) > 0.0 { -1.0 } else { 1.0 };
        let (m0, m1) = (side * d0.perp(), side * d1.perp());
        arcs.push((points[1], m0, m0.angle_to(m1)));
      }
      for p_push in [0.0f32, 1.0] {
        let placed: Vec<[Vec2; 3]> = round
          .iter()
          .map(|tri| tri.map(|v| Vec2::from(v.position) + v.push() * p_push.min(v.limit)))
          .collect();
        for &(corner, from, angle) in &arcs {
          for k in 1..2000 {
            let direction = Vec2::from_angle(angle * k as f32 / 2000.0).rotate(from);
            let at = corner + direction * (HW + p_push - 1e-3);
            assert!(
              placed.iter().any(|tri| within(*tri, at)),
              "{pts:?} {cap:?} pushed {p_push}: {at} outside the arc"
            );
          }
        }
      }
    }
  }

  /// The shapes the copy tests build: the attribute tests' shapes, short
  /// segments whose corners split, a small closed square, and a closed line
  /// whose seam runs straight on
  fn copy_shapes() -> Vec<Shape> {
    let mut shapes = shapes();
    shapes.push(Shape::even(
      &[(0.0, 0.0), (3.0, 0.0), (3.0, 2.5), (6.0, 2.0), (6.5, 6.0)],
      1.0,
      false,
    ));
    shapes.push(Shape::even(
      &[(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (0.0, 3.0)],
      1.0,
      true,
    ));
    shapes.push(Shape::even(
      &[
        (5.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.0),
      ],
      1.0,
      true,
    ));
    shapes
  }

  type Push = ([u32; 2], u32);

  fn push_of(v: &Vertex2D) -> Push {
    (v.push.map(f32::to_bits), v.limit.to_bits())
  }

  /// `t`'s vertices of some width, by their place, bit for bit
  fn places(t: &Triangles) -> std::collections::HashMap<[u32; 2], Vec<Vertex2D>> {
    let mut seen: std::collections::HashMap<[u32; 2], Vec<Vertex2D>> =
      std::collections::HashMap::new();
    for v in t.vertices.iter().filter(|v| v.half > 0.0) {
      seen
        .entry(v.position.map(f32::to_bits))
        .or_default()
        .push(*v);
    }
    seen
  }

  /// The copies at `at` push alike, or two ways where a fringe triangle has
  /// a copy pushed each way
  fn assert_copies_push_alike(t: &Triangles, at: [u32; 2], copies: &[Vertex2D], what: &str) {
    let mut pushes: Vec<Push> = Vec::new();
    for v in copies {
      if !pushes.contains(&push_of(v)) {
        pushes.push(push_of(v));
      }
    }
    match pushes.as_slice() {
      [_] => {}
      [a, b] => {
        let bridged = triangles_of(t).any(|tri| {
          let here: Vec<Push> = tri
            .iter()
            .filter(|v| v.position.map(f32::to_bits) == at)
            .map(push_of)
            .collect();
          here.contains(a) && here.contains(b)
        });
        assert!(bridged, "{what}: push apart: {pushes:?}");
      }
      _ => panic!("{what}: push apart: {pushes:?}"),
    }
  }

  #[test]
  fn copies_at_one_place_push_alike() {
    // Every piece works a shared place out with the one corner function, so
    // its copies of it push the same way, as far, bit for bit (but at a
    // point of no width, where a line's two edges meet). A split corner's
    // elbow is the one place copies push two ways (the strip's straight
    // out, the fan's along the corner), and a fringe triangle joins them
    for (index, shape) in copy_shapes().iter().enumerate() {
      for join in JOINS {
        for cap in CAPS {
          let t = shape.build(join, cap);
          for (at, copies) in places(&t) {
            let what = format!(
              "shape {index} {join:?} {cap:?}: copies at {:?}",
              at.map(f32::from_bits)
            );
            assert_copies_push_alike(&t, at, &copies, &what);
          }
        }
      }
    }
  }

  #[test]
  fn copies_at_one_place_move_alike_under_effects() {
    // Displacement effects move a vertex along its `across` by its
    // `effect_u`, so every copy of a place carries the same two, bit for
    // bit, as well as the same push and limit (but at a point of no width):
    // at a closed line's seam too, where the segment arriving runs its
    // texture `u` on past the last point while point 0 starts again, so a
    // vertex both segments share takes its point's own `u`
    for (index, shape) in copy_shapes().iter().enumerate() {
      for join in JOINS {
        for cap in CAPS {
          let t = shape.build(join, cap);
          for (at, copies) in places(&t) {
            let what = format!(
              "shape {index} {join:?} {cap:?}: copies at {:?}",
              at.map(f32::from_bits)
            );
            let moved = |v: &Vertex2D| (v.across.map(f32::to_bits), v.effect_u.to_bits());
            let first = moved(&copies[0]);
            for v in &copies {
              assert_eq!(
                moved(v),
                first,
                "{what}: move apart: across {:?} effect_u {} against across {:?} effect_u {}",
                v.across,
                v.effect_u,
                copies[0].across,
                copies[0].effect_u
              );
            }
            assert_copies_push_alike(&t, at, &copies, &what);
          }
        }
      }
    }
  }

  #[test]
  fn no_vertex_lies_inside_an_edge() {
    // No triangle's corner lies inside another's edge, so rasterising the
    // pieces leaves no crack along it: where a corner's inner side splits,
    // the segments' ends meet the join at the corner point
    let mut shapes = shapes();
    shapes.truncate(4);
    shapes.remove(3);
    shapes.extend([
      Shape::even(&[(0.0, 0.0), (10.0, 0.0), (2.0, 3.0)], 1.0, false),
      Shape::even(
        &[(0.0, 0.0), (10.0, 0.0), (2.0, -3.0), (12.0, -3.5)],
        1.0,
        false,
      ),
      Shape::even(&[(0.0, 0.0), (4.0, 0.0), (0.5, 1.5)], 1.0, true),
      Shape::even(
        &[(0.0, 0.0), (3.0, 0.5), (0.2, 1.1), (3.3, 1.7)],
        0.75,
        false,
      ),
      Shape::even(
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
        1.0,
        true,
      ),
    ]);
    for (index, shape) in shapes.iter().enumerate() {
      for join in JOINS {
        for cap in CAPS {
          let t = shape.build(join, cap);
          let rest: Vec<Vec2> = t.vertices.iter().map(|v| Vec2::from(v.position)).collect();
          let solid = solid_triangles(&t, &rest);
          let corners: Vec<Vec2> = solid.iter().flatten().copied().collect();
          for tri in &solid {
            for k in 0..3 {
              let (a, b) = (tri[k], tri[(k + 1) % 3]);
              let length = a.distance(b);
              for &v in &corners {
                if v.distance(a) < 1e-4 || v.distance(b) < 1e-4 {
                  continue;
                }
                let t = (v - a).dot(b - a) / (length * length);
                let off = (v - a).perp_dot(b - a).abs() / length;
                assert!(
                  !(t > 0.0 && t < 1.0 && off < 1e-4),
                  "shape {index} {join:?} {cap:?}: {v} inside the edge {a} to {b}"
                );
              }
            }
          }
        }
      }
    }
  }
}
