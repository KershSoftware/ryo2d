//! Trails and lines as polylines: their points, widths, colours and texture
//! coordinates, the draw-list item each one becomes, and the stream the GPU
//! builds world lines from.
//!
//! - **The GPU builds world lines from their points** (`LineStream`). A line
//!   is copied as its filtered points (`LinePoint`: place, half width, `u`,
//!   sRGB colour and its line) and its entry in the line table (`LineInfo`:
//!   style, effect, emission, first point, count, join, cap, closed, edge
//!   and miter limit); then an index buffer whose values are vertex codes,
//!   each naming its piece's point, the piece's kind and one of the
//!   piece's fixed slots, three a triangle in the order the pieces draw
//!   (`piece_list`, `piece_template`). Nothing is tessellated on the CPU.
//!   The filter, the piece list and the line table are the CPU's whole
//!   part; their rules are told in `polyline.rs`, the reference builder,
//!   compiled for tests alone. A draw is indexed, over `3 × triangles`
//!   indices, and the vertex stage decodes its `vertex_index`, the code
//!   (`line.wgsl`), so a piece's triangles share its vertices in the GPU's
//!   vertex reuse, draws merge as index ranges did and z-order holds.
//! - **Slots.** Each kind has fixed slots that its triangles name, the same
//!   for every piece of that kind whatever its geometry (`piece_template`;
//!   `line_piece_slot` in `line_pieces.wgsl`, for the UI's unindexed
//!   instances, lays them out alike: `the_slot_layouts_agree`), and what a
//!   slot holds follows the GPU's decision at the corner (miter or bevel,
//!   the inner side shared or split), used alike by every triangle naming
//!   it. Every kind under every decision builds the triangles the reference
//!   builds, each with the same first vertex, which flat attributes are
//!   taken from (`every_slot_builds_its_role_under_every_decision`; a right
//!   turn's triangles wind the other way to the reference's, which draws
//!   the same with no face culled: line pipelines cull none). The GPU's
//!   vertices equal the CPU builder's (`polyline.rs`, the reference),
//!   triangle for triangle (`the_gpu_builds_the_reference_s_vertices`), and
//!   its pushed mesh covers every pixel of a translucent line once
//!   (`a_translucent_line_drawn_on_the_gpu_counts_every_pixel_once`).
//! - **Culling** grows a line's box by `MITER_LIMIT` times its widest half
//!   width (a miter's tip, a square cap's corner) and its effect's reach,
//!   and each camera grows it by one of its pixels for the fringe
//!   (`a_miter_tip_reaching_into_view_is_drawn`, e2e).
//! - **Limit: a miter's tip can reach a little past the box.** Its coverage
//!   reaches `m · (hw + 0.5 px)` from the corner point (`m` the miter's
//!   length, at most `MITER_LIMIT`), while the box grows by `4 hw + 1 px`, so
//!   where the tip of a line just outside the view points into it, a sliver
//!   of coverage under 0.1 can be culled at the view's edge.
//! - **Limit: the stream holds what the device binds.** Its points, their
//!   corner records (64 bytes a point, `corners.rs`) and its line table are
//!   each one storage binding, so each holds as many as the device's
//!   `max_storage_buffer_binding_size` (and `max_buffer_size`) fits, the
//!   points never past the 2^25 a code can name and the table never past
//!   `MAX_LINES`, and its index buffer as many as `max_buffer_size` holds
//!   (`StreamCap`, `LineStream::for_device`: under the `Limits::default()`
//!   the engine asks for, 2,097,152 points, 67,108,864 indices and
//!   1,048,576 lines); a line that would
//!   take any past it draws nothing, and the log is warned once a run
//!   (`a_line_past_the_stream_s_cap_draws_nothing_and_warns_once`). Past it
//!   the bind group would fail validation, which ends the run.

use std::ops::Range;

use glam::Vec2;

use super::corners::CORNER_BYTES;
use super::{BlendMode, Filter};
use crate::core::ecs::Entity;
use crate::core::graphics::{RenderLayers, TextureId};
use crate::core::render::{DrawList, DrawTriangles};
use crate::core::trail::{
  Line2D, LineCap, LineEdge, LineJoin, TextureMode, Trail2D, TrailEffect, TrailStyle,
};
use crate::core::ui::Color;

/// The line table holds this many lines a frame; the rest draw nothing
pub(crate) const MAX_LINES: usize = 1 << 20;

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
  /// How its edges meet the pixels: `EDGE_TARGET`, `EDGE_SMOOTH` or
  /// `EDGE_HARD`
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
/// A line's edge as the line table and the UI's line instances number it:
/// its target's (Hard on a pixel-art target, Smooth elsewhere), or its own
pub(crate) const EDGE_TARGET: u32 = 0;
pub(crate) const EDGE_SMOOTH: u32 = 1;
pub(crate) const EDGE_HARD: u32 = 2;

/// `edge` as the line table numbers it
pub(crate) fn edge_code(edge: Option<LineEdge>) -> u32 {
  match edge {
    None => EDGE_TARGET,
    Some(LineEdge::Smooth) => EDGE_SMOOTH,
    Some(LineEdge::Hard) => EDGE_HARD,
  }
}

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
      edge: edge_code(style.edge),
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
  /// How its edges meet the pixels (`None`: its target's)
  pub(crate) edge: Option<LineEdge>,
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
pub(crate) const ARC_TRIANGLES: u32 = 16;

/// The triangles a piece of `kind` draws, whatever its geometry.
pub(crate) fn piece_triangles(kind: PieceKind, join: LineJoin, cap: LineCap) -> u32 {
  piece_template(kind, join, cap).len() as u32
}

/// A segment's slots: its quad fanned from its end's corner point
const SEGMENT_TEMPLATE: [[u32; 3]; SEGMENT_TRIANGLES as usize] =
  [[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 5]];
/// A miter or bevel join's: its two slanted ends and the two triangles
/// between them, from the corner point
const JOIN_TEMPLATE: [[u32; 3]; JOIN_TRIANGLES as usize] =
  [[0, 2, 3], [1, 4, 5], [1, 5, 6], [0, 7, 8]];
/// A round join's: its slanted ends, then its arc between them
const ROUND_JOIN_TEMPLATE: [[u32; 3]; ROUND_JOIN_TRIANGLES as usize] = {
  let mut template = [[0, 2, 3]; ROUND_JOIN_TRIANGLES as usize];
  let mut k = 0;
  while k < ARC_TRIANGLES {
    template[1 + k as usize] = [1, 6 + k, 7 + k];
    k += 1;
  }
  template[ROUND_JOIN_TRIANGLES as usize - 1] = [0, 4, 5];
  template
};
const SQUARE_CAP_TEMPLATE: [[u32; 3]; SQUARE_CAP_TRIANGLES as usize] = [[0, 1, 2], [1, 3, 2]];
/// A round cap's: its arc fanned from its corner point
const ROUND_CAP_TEMPLATE: [[u32; 3]; ROUND_CAP_TRIANGLES as usize] = {
  let mut template = [[0, 0, 0]; ROUND_CAP_TRIANGLES as usize];
  let mut k = 0;
  while k < ARC_TRIANGLES {
    template[k as usize] = [0, 1 + k, 2 + k];
    k += 1;
  }
  template
};

/// The slots each triangle of a piece of `kind` names, in the order they
/// draw, whatever its geometry (what each slot holds is the GPU's, by the
/// corner: `line_pieces.wgsl`).
pub(crate) fn piece_template(kind: PieceKind, join: LineJoin, cap: LineCap) -> &'static [[u32; 3]] {
  match (kind, join, cap) {
    (PieceKind::Segment, ..) => &SEGMENT_TEMPLATE,
    (PieceKind::Join, LineJoin::Round, _) => &ROUND_JOIN_TEMPLATE,
    (PieceKind::Join, ..) => &JOIN_TEMPLATE,
    (PieceKind::StartCap | PieceKind::EndCap, _, LineCap::Butt) => &[],
    (PieceKind::StartCap | PieceKind::EndCap, _, LineCap::Square) => &SQUARE_CAP_TEMPLATE,
    (PieceKind::StartCap | PieceKind::EndCap, _, LineCap::Round) => &ROUND_CAP_TEMPLATE,
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

/// How far a butt or square cap reaches past its end point.
pub(crate) fn cap_reach(point: &PolyPoint, cap: LineCap) -> f32 {
  if cap == LineCap::Square {
    point.half_width
  } else {
    0.0
  }
}

/// How a trail or line joins the draw list, besides its geometry.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LineLook {
  /// Drawn before lighting; otherwise after it
  pub(crate) lit: bool,
  pub(crate) z: i32,
  /// The texture along it (textured trails)
  pub(crate) texture: Option<TextureId>,
  pub(crate) blend: BlendMode,
  pub(crate) filter: Filter,
  pub(crate) layers: RenderLayers,
  /// Drawn by this camera alone (shapes built for one camera)
  pub(crate) only_camera: Option<Entity>,
}

/// The points of `trail` as a polyline into `out` (cleared first), and how
/// to draw it; `None` when it draws nothing.
///
/// Width follows the trail's taper; colour its fade by distance or position,
/// times its age fade when it fades by neither; `u` runs 0..1 along it, by
/// distance over the tile length when tiled, or by distance over its length
/// when an effect displaces it (so the pattern stays on its points).
pub(crate) fn trail_points(trail: &Trail2D, out: &mut Vec<PolyPoint>) -> Option<PolyStyle> {
  out.clear();
  let points = trail.points();
  let count = points.len();
  if !trail.is_visible() || count < 2 {
    return None;
  }
  let distances = trail.cumulative_distances();
  let distance = |i: usize| distances.get(i).copied().unwrap_or(0.0);
  let total = distances.last().copied().unwrap_or(1.0).max(0.001);
  for (i, point) in points.iter().enumerate() {
    let t = i as f32 / (count - 1).max(1) as f32;
    let mut color = match trail.fade_distance() {
      Some(max) => trail.color_at((distance(i) / max).clamp(0.0, 1.0)),
      None => trail.color_at(t),
    };
    // Age fades only a trail that fades by neither alpha nor distance
    if !trail.fades_alpha() && trail.fade_distance().is_none() {
      let alpha = color.a as f32 * trail.age_alpha(point.age);
      color = Color::new(color.r, color.g, color.b, alpha as u8);
    }
    let u = match (trail.texture(), trail.texture_mode()) {
      (Some(_), TextureMode::Stretch) => t,
      (Some(_), TextureMode::Tile) => distance(i) / trail.tile_length(),
      (None, _) if trail.effect().is_some() => distance(i) / total,
      (None, _) => t,
    };
    out.push(PolyPoint {
      pos: Vec2::new(point.x, point.y),
      half_width: trail.width_at(t) / 2.0,
      color,
      u,
    });
  }
  Some(PolyStyle {
    join: trail.join(),
    cap: trail.cap(),
    closed: false,
    params: build_effect_params(trail.effect(), trail.style()),
    emission: trail.emission(),
    edge: trail.edge(),
  })
}

/// The points of `line`, placed at `at`, as a polyline into `out` (cleared
/// first), and how to draw it; `None` when it draws nothing.
pub(crate) fn line_points(line: &Line2D, at: Vec2, out: &mut Vec<PolyPoint>) -> Option<PolyStyle> {
  out.clear();
  let count = line.point_count();
  if !line.is_visible() || count < 2 {
    return None;
  }
  for (i, point) in line.points().iter().enumerate() {
    let t = i as f32 / (count - 1).max(1) as f32;
    out.push(PolyPoint {
      pos: *point + at,
      half_width: line.width_at(t) / 2.0,
      color: line.color_at(t),
      u: t,
    });
  }
  Some(PolyStyle {
    join: line.join(),
    cap: line.cap(),
    closed: line.is_closed(),
    params: build_effect_params(None, line.style()),
    emission: line.emission(),
    edge: line.edge(),
  })
}

/// A point of a world line as the GPU's line pieces read it (`LinePoint`
/// in `line_pieces.wgsl`, 24 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LinePoint {
  pub(crate) pos: [f32; 2],
  pub(crate) half_width: f32,
  pub(crate) u: f32,
  /// sRGB, straight alpha
  pub(crate) color: [u8; 4],
  /// Its line's entry in the line table
  pub(crate) line: u32,
}

/// A vertex code keeps 25 bits for its point, so the stream never holds
/// more points than this
const CODE_POINTS: usize = 1 << 25;

/// The most the stream holds a frame: as many points (and their corner
/// records) and lines as one storage binding of the device holds, and as
/// many indices as one of its buffers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StreamCap {
  pub(crate) points: usize,
  /// Three a triangle
  pub(crate) indices: usize,
  pub(crate) lines: usize,
}

impl StreamCap {
  /// The cap of a device with `limits`
  pub(crate) fn of(limits: &wgpu::Limits) -> Self {
    let binding = limits
      .max_storage_buffer_binding_size
      .min(limits.max_buffer_size);
    let fits =
      |bytes: u64, element: usize| usize::try_from(bytes / element as u64).unwrap_or(usize::MAX);
    // A point takes a corner record too, the stream's largest element
    let point = std::mem::size_of::<LinePoint>().max(CORNER_BYTES);
    Self {
      points: fits(binding, point).min(CODE_POINTS),
      indices: fits(limits.max_buffer_size, std::mem::size_of::<u32>()),
      lines: fits(binding, std::mem::size_of::<LineInfo>()).min(MAX_LINES),
    }
  }
}

impl Default for StreamCap {
  /// The limits the engine asks its device for
  fn default() -> Self {
    Self::of(&wgpu::Limits::default())
  }
}

/// A vertex's code, a value of the index buffer: its piece's point (in the
/// point buffer), the piece's kind and its slot (`line.wgsl` reads it back).
pub(crate) fn vertex_code(point: u32, kind: PieceKind, slot: u32) -> u32 {
  let kind = match kind {
    PieceKind::Segment => 0,
    PieceKind::Join => 1,
    PieceKind::StartCap => 2,
    PieceKind::EndCap => 3,
  };
  (point << 7) | (kind << 5) | slot
}

/// The frame's world lines as the GPU builds them: their points, the line
/// table and the index buffer of vertex codes, three a triangle.
#[derive(Default)]
pub(crate) struct LineStream {
  pub(crate) points: Vec<LinePoint>,
  pub(crate) lines: Vec<LineInfo>,
  pub(crate) indices: Vec<u32>,
  /// Room kept from line to line
  filtered: Vec<PolyPoint>,
  pieces: Vec<Piece>,
  cap: StreamCap,
  /// Whether the log has been told the stream is full
  warned: bool,
}

impl LineStream {
  /// The stream of a device with `limits`
  pub(crate) fn for_device(limits: &wgpu::Limits) -> Self {
    Self {
      cap: StreamCap::of(limits),
      ..Self::default()
    }
  }

  /// The most it holds a frame
  pub(crate) fn cap(&self) -> StreamCap {
    self.cap
  }

  /// Forget every line, keeping the allocations.
  pub(crate) fn clear(&mut self) {
    self.points.clear();
    self.lines.clear();
    self.indices.clear();
  }

  /// Add the line through `points`: its filtered points, its entry in the
  /// line table and its triangles' vertex codes; the indices they take,
  /// three a triangle (empty when nothing is drawn).
  pub(crate) fn polyline(&mut self, points: &[PolyPoint], style: &PolyStyle) -> Range<u32> {
    let start = self.indices.len() as u32;
    let Some(closed) = filter_points(points, style.closed, &mut self.filtered) else {
      return start..start;
    };
    let count = self.filtered.len();
    piece_list(count, closed, style.cap, &mut self.pieces);
    let triangles: usize = self
      .pieces
      .iter()
      .map(|piece| piece_triangles(piece.kind, style.join, style.cap) as usize)
      .sum();
    let cap = self.cap;
    if self.lines.len() >= cap.lines
      || self.points.len() + count > cap.points
      || self.indices.len() + 3 * triangles > cap.indices
    {
      if !self.warned {
        self.warned = true;
        log::warn!(
          "more than {} lines and trails, {} of their points or {} of their triangles in one frame: the rest draw nothing",
          cap.lines,
          cap.points,
          cap.indices / 3
        );
      }
      return start..start;
    }
    let line = self.lines.len() as u32;
    let first = self.points.len() as u32;
    let along = along_half(&self.filtered, closed, style.cap);
    self
      .lines
      .push(LineInfo::of(style, closed, along, first, count as u32));
    self.points.extend(self.filtered.iter().map(|point| {
      let crate::core::ui::Color { r, g, b, a } = point.color;
      LinePoint {
        pos: point.pos.into(),
        half_width: point.half_width,
        u: point.u,
        color: [r, g, b, a],
        line,
      }
    }));
    for piece in &self.pieces {
      let code = vertex_code(first + piece.point, piece.kind, 0);
      let template = piece_template(piece.kind, style.join, style.cap);
      self
        .indices
        .extend(template.iter().flatten().map(|slot| code | slot));
    }
    start..self.indices.len() as u32
  }
}

/// Add the line through `points` to `lines` and to `list` as one item, seen
/// by the cameras whose view meets its points' box grown by `MITER_LIMIT`
/// times its widest half width and how far its effect moves it (each
/// camera grows it by one of its pixels more, for the fringe).
pub(crate) fn push_polyline(
  list: &mut DrawList,
  lines: &mut LineStream,
  points: &[PolyPoint],
  style: &PolyStyle,
  look: LineLook,
) {
  let line = lines.lines.len() as u32;
  let indices = lines.polyline(points, style);
  if indices.is_empty() {
    return;
  }
  let (mut min, mut max) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
  let mut reach = 0.0f32;
  for point in points.iter().filter(|point| point.pos.is_finite()) {
    min = min.min(point.pos);
    max = max.max(point.pos);
    if point.half_width.is_finite() {
      reach = reach.max(point.half_width);
    }
  }
  let reach = Vec2::splat(MITER_LIMIT * reach + effect_reach(&style.params));
  list.push_triangles(
    look.lit,
    look.z,
    look.only_camera,
    DrawTriangles {
      texture: look.texture,
      blend: look.blend,
      filter: look.filter,
      layers: look.layers,
      min: min - reach,
      max: max + reach,
      indices,
      line,
    },
  );
}

/// How far the effect in `params` can move a vertex off its place.
fn effect_reach(params: &[f32; 4]) -> f32 {
  match params[0] as u32 & 0xF {
    // Lightning: octaves of halving amplitude stay under twice the jitter
    1 => 2.0 * params[1].abs(),
    2 => params[2].abs(),
    4 => params[1].abs(),
    _ => 0.0,
  }
}

/// Effect type in bits 0-3 (0 none, 1 lightning, 2 noise, 3 scroll, 4
/// ribbon) and style in bits 4-5 (0 solid, 1 soft, 2 glow).
fn encode_type_and_style(effect_type: u32, style: u32) -> f32 {
  (effect_type | (style << 4)) as f32
}

fn effect_type_index(effect: Option<&TrailEffect>) -> u32 {
  match effect {
    None => 0,
    Some(TrailEffect::Lightning { .. }) => 1,
    Some(TrailEffect::Noise { .. }) => 2,
    Some(TrailEffect::UvScroll { .. }) => 3,
    Some(TrailEffect::Ribbon { .. }) => 4,
  }
}

fn style_index(style: TrailStyle) -> u32 {
  match style {
    TrailStyle::Solid => 0,
    TrailStyle::Soft => 1,
    TrailStyle::Glow => 2,
  }
}

/// The vertices' parameters: the encoded effect and style, then the
/// effect's own values.
fn build_effect_params(effect: Option<&TrailEffect>, style: TrailStyle) -> [f32; 4] {
  let type_and_style = encode_type_and_style(effect_type_index(effect), style_index(style));
  match effect {
    None => [type_and_style, 0.0, 0.0, 0.0],
    Some(TrailEffect::Lightning {
      jitter,
      speed,
      detail,
    }) => [type_and_style, *jitter, *speed, *detail as f32],
    Some(TrailEffect::Noise {
      frequency,
      amplitude,
      speed,
    }) => [type_and_style, *frequency, *amplitude, *speed],
    Some(TrailEffect::UvScroll { speed }) => [type_and_style, *speed, 0.0, 0.0],
    Some(TrailEffect::Ribbon {
      amplitude,
      frequency,
      speed,
    }) => [type_and_style, *amplitude, *frequency, *speed],
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::trail::{LineEdge, LineJoin};

  #[test]
  fn effect_and_style_share_one_float() {
    assert_eq!(encode_type_and_style(0, 0), 0.0);
    assert_eq!(encode_type_and_style(1, 0), 1.0);
    assert_eq!(encode_type_and_style(4, 2), 36.0);
    let decoded = encode_type_and_style(3, 1) as u32;
    assert_eq!((decoded & 0xF, (decoded >> 4) & 0x3), (3, 1));
  }

  #[test]
  fn effect_params_carry_each_effect() {
    let none = build_effect_params(None, TrailStyle::Glow);
    assert_eq!(none, [32.0, 0.0, 0.0, 0.0]);
    let lightning = TrailEffect::Lightning {
      jitter: 15.0,
      speed: 8.0,
      detail: 3,
    };
    assert_eq!(
      build_effect_params(Some(&lightning), TrailStyle::Solid),
      [1.0, 15.0, 8.0, 3.0]
    );
    let noise = TrailEffect::Noise {
      frequency: 2.0,
      amplitude: 5.0,
      speed: 1.5,
    };
    assert_eq!(
      build_effect_params(Some(&noise), TrailStyle::Soft),
      [18.0, 2.0, 5.0, 1.5]
    );
    let scroll = TrailEffect::UvScroll { speed: 2.5 };
    assert_eq!(
      build_effect_params(Some(&scroll), TrailStyle::Solid),
      [3.0, 2.5, 0.0, 0.0]
    );
    let ribbon = TrailEffect::Ribbon {
      amplitude: 4.0,
      frequency: 3.0,
      speed: 2.0,
    };
    assert_eq!(
      build_effect_params(Some(&ribbon), TrailStyle::Solid),
      [4.0, 4.0, 3.0, 2.0]
    );
  }

  #[test]
  fn an_effect_reaches_past_its_points() {
    let lightning = build_effect_params(
      Some(&TrailEffect::Lightning {
        jitter: 6.0,
        speed: 1.0,
        detail: 4,
      }),
      TrailStyle::Solid,
    );
    assert_eq!(effect_reach(&lightning), 12.0);
    assert_eq!(
      effect_reach(&build_effect_params(None, TrailStyle::Glow)),
      0.0
    );
  }

  #[test]
  fn a_line_past_the_stream_s_cap_draws_nothing_and_warns_once() {
    // The points and line table hold what one storage binding of the
    // device does, a point taking its corner record's 64 bytes, and the
    // index buffer what one buffer does
    assert_eq!(
      StreamCap::default(),
      StreamCap {
        points: 2_097_152,
        indices: 67_108_864,
        lines: 1_048_576,
      }
    );
    let limits = wgpu::Limits {
      max_storage_buffer_binding_size: 1280,
      ..wgpu::Limits::default()
    };
    let bound = StreamCap {
      points: 20,
      indices: 67_108_864,
      lines: 16,
    };
    let mut stream = LineStream::for_device(&limits);
    assert_eq!(stream.cap(), bound);
    // A buffer may be no bigger than the device makes one, either
    let buffer = wgpu::Limits {
      max_buffer_size: 1280,
      ..wgpu::Limits::default()
    };
    assert_eq!(
      StreamCap::of(&buffer),
      StreamCap {
        indices: 320,
        ..bound
      }
    );
    stream.cap = StreamCap {
      points: 16,
      indices: 300,
      lines: 5,
    };
    let line = |count: usize| -> Vec<PolyPoint> {
      (0..count)
        .map(|i| PolyPoint {
          pos: Vec2::new(i as f32 * 10.0, (i % 2) as f32 * 10.0),
          half_width: 1.0,
          color: Color::WHITE,
          u: i as f32,
        })
        .collect()
    };
    let style = |join| PolyStyle {
      join,
      cap: crate::core::trail::LineCap::Butt,
      closed: false,
      params: [0.0; 4],
      emission: 1.0,
      edge: None,
    };
    let (round, miter) = (style(LineJoin::Round), style(LineJoin::Miter));
    // Three lines of three points and 26 triangles each
    for _ in 0..3 {
      assert_eq!(stream.polyline(&line(3), &round).len(), 78);
    }
    assert!(!stream.warned);
    // A fourth would take 104 triangles
    assert!(stream.polyline(&line(3), &round).is_empty());
    assert!(stream.warned, "warned");
    assert!(!stream.polyline(&line(2), &miter).is_empty());
    // Eight more points would take 19
    assert!(stream.polyline(&line(8), &miter).is_empty());
    assert!(!stream.polyline(&line(2), &miter).is_empty());
    // A sixth line
    assert!(stream.polyline(&line(2), &miter).is_empty());
    let cap = stream.cap();
    assert_eq!(
      (
        stream.points.len(),
        stream.indices.len(),
        stream.lines.len()
      ),
      (13, 258, 5)
    );
    assert!(stream.points.len() <= cap.points && stream.indices.len() <= cap.indices);
  }

  #[test]
  fn a_line_tapers_and_fades_along_its_points() {
    let line = Line2D::new(
      vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)],
      Color::WHITE,
      4.0,
    )
    .with_end_width(2.0)
    .with_end_color(Color::new(0, 0, 0, 255));
    let mut points = Vec::new();
    let style = line_points(&line, Vec2::new(1.0, 2.0), &mut points);
    assert!(style.is_some());
    let ends = (points[0], points[2]);
    assert_eq!(
      (ends.0.pos, ends.1.pos),
      (Vec2::new(1.0, 2.0), Vec2::new(21.0, 2.0))
    );
    assert_eq!((ends.0.half_width, ends.1.half_width), (2.0, 1.0));
    assert_eq!((ends.0.u, points[1].u, ends.1.u), (0.0, 0.5, 1.0));
    assert_eq!(ends.1.color, Color::new(0, 0, 0, 255));
  }

  #[test]
  fn a_tiled_trail_runs_u_by_distance() {
    let texture = crate::core::asset::Assets::new().add(crate::core::graphics::Image::solid_rect(
      2, 2, 255, 255, 255, 255,
    ));
    let mut trail = Trail2D::new(Color::WHITE, 2.0)
      .with_texture(texture)
      .with_texture_mode(TextureMode::Tile)
      .with_tile_length(5.0)
      .with_fade(false);
    trail.populate_line(0.0, 0.0, 20.0, 0.0, 5);
    let mut points = Vec::new();
    assert!(trail_points(&trail, &mut points).is_some());
    let last = points.last().map(|point| point.u);
    assert_eq!(last, Some(4.0), "20 units over tiles of 5");
  }

  #[test]
  fn a_line_s_or_trail_s_edge_reaches_its_line_table_entry() {
    let ends = vec![Vec2::ZERO, Vec2::new(10.0, 0.0)];
    let mut points = Vec::new();
    for (edge, code) in [
      (None, EDGE_TARGET),
      (Some(LineEdge::Smooth), EDGE_SMOOTH),
      (Some(LineEdge::Hard), EDGE_HARD),
    ] {
      let mut line = Line2D::new(ends.clone(), Color::WHITE, 2.0);
      line.set_edge(edge);
      let mut trail = Trail2D::new(Color::WHITE, 2.0);
      trail.set_edge(edge);
      trail.populate_line(0.0, 0.0, 20.0, 0.0, 5);
      let styles = [
        line_points(&line, Vec2::ZERO, &mut points),
        trail_points(&trail, &mut points),
      ];
      for style in styles {
        let style = style.expect("drawn");
        assert_eq!(style.edge, edge);
        assert_eq!(
          LineInfo::of(&style, false, 0.0, 0, 2).edge,
          code,
          "{edge:?}"
        );
      }
    }
    let hard = Line2D::new(ends, Color::WHITE, 2.0).with_edge(LineEdge::Hard);
    assert_eq!(hard.edge(), Some(LineEdge::Hard));
    let trail = Trail2D::new(Color::WHITE, 2.0).with_edge(LineEdge::Smooth);
    assert_eq!(trail.edge(), Some(LineEdge::Smooth));
  }
}
