//! Trails and lines as polylines: their points, widths, colours and texture
//! coordinates, the draw-list item each one becomes, and the stream the GPU
//! builds world lines from.
//!
//! - **The GPU builds world lines from their points** (`LineStream`). A line
//!   is copied as its filtered points (`LinePoint`: place, half width, `u`,
//!   sRGB colour and its line) and its entry in the line table (`LineInfo`:
//!   style, effect, emission, first point, count, join, cap, closed, edge
//!   and miter limit); then one 32-bit code a triangle names its piece's
//!   point, the piece's kind and the triangle within it, in the order the
//!   pieces draw (`piece_list`, `piece_triangles`). Nothing is tessellated
//!   on the CPU. A draw covers `3 × triangles` vertices of the code stream,
//!   and the vertex stage finds its triangle's code by `vertex_index ÷ 3`
//!   (`line.wgsl`), so draws merge as index ranges did and z-order holds.
//!   The GPU's vertices equal the CPU builder's (`polyline.rs`, the
//!   reference), triangle for triangle
//!   (`the_gpu_builds_the_reference_s_vertices`), and its pushed mesh covers
//!   every pixel of a translucent line once
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
//! - **Limit: the stream holds what the device binds.** Its points, codes
//!   and line table are each one storage binding, so each holds as many as
//!   the device's `max_storage_buffer_binding_size` (and `max_buffer_size`)
//!   fits, the points never past the 2^25 a code can name and the table
//!   never past `MAX_LINES` (`StreamCap`, `LineStream::for_device`: under
//!   the `Limits::default()` the engine asks for, 128 MiB each, so 5,592,405
//!   points, 33,554,432 triangles and 1,048,576 lines); a line that would
//!   take any past it draws nothing, and the log is warned once a run
//!   (`a_line_past_the_stream_s_cap_draws_nothing_and_warns_once`). Past it
//!   the bind group would fail validation, which ends the run.

use std::ops::Range;

use glam::Vec2;

use super::polyline::{
  LineInfo, MAX_LINES, MITER_LIMIT, Piece, PieceKind, PolyPoint, PolyStyle, Triangles, along_half,
  filter_points, piece_list, piece_triangles,
};
use super::{BlendMode, Filter};
use crate::core::ecs::Entity;
use crate::core::graphics::{RenderLayers, TextureId};
use crate::core::render::{DrawList, DrawTriangles};
use crate::core::trail::{Line2D, TextureMode, Trail2D, TrailEffect, TrailStyle};
use crate::core::ui::Color;

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

/// A triangle code keeps 25 bits for its point, so the stream never holds
/// more points than this
const CODE_POINTS: usize = 1 << 25;

/// The most the stream holds a frame, each of its buffers being one storage
/// binding: as many as the device binds and makes in one buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StreamCap {
  pub(crate) points: usize,
  /// Triangles, one code each
  pub(crate) codes: usize,
  pub(crate) lines: usize,
}

impl StreamCap {
  /// The cap of a device with `limits`
  pub(crate) fn of(limits: &wgpu::Limits) -> Self {
    let bytes = limits
      .max_storage_buffer_binding_size
      .min(limits.max_buffer_size);
    let fits = |element: usize| usize::try_from(bytes / element as u64).unwrap_or(usize::MAX);
    Self {
      points: fits(std::mem::size_of::<LinePoint>()).min(CODE_POINTS),
      codes: fits(std::mem::size_of::<u32>()),
      lines: fits(std::mem::size_of::<LineInfo>()).min(MAX_LINES),
    }
  }
}

impl Default for StreamCap {
  /// The limits the engine asks its device for
  fn default() -> Self {
    Self::of(&wgpu::Limits::default())
  }
}

/// A triangle's code: its piece's point (in the point buffer), the piece's
/// kind and the triangle within it (`line.wgsl` reads it back).
pub(crate) fn triangle_code(point: u32, kind: PieceKind, triangle: u32) -> u32 {
  let kind = match kind {
    PieceKind::Segment => 0,
    PieceKind::Join => 1,
    PieceKind::StartCap => 2,
    PieceKind::EndCap => 3,
  };
  (point << 7) | (kind << 5) | triangle
}

/// The frame's world lines as the GPU builds them: their points, the line
/// table and one code a triangle.
#[derive(Default)]
pub(crate) struct LineStream {
  pub(crate) points: Vec<LinePoint>,
  pub(crate) lines: Vec<LineInfo>,
  pub(crate) codes: Vec<u32>,
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
    self.codes.clear();
  }

  /// Add the line through `points`: its filtered points, its entry in the
  /// line table and its triangles' codes; the vertices they draw, three a
  /// triangle (empty when nothing is drawn).
  pub(crate) fn polyline(&mut self, points: &[PolyPoint], style: &PolyStyle) -> Range<u32> {
    let start = 3 * self.codes.len() as u32;
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
      || self.codes.len() + triangles > cap.codes
    {
      if !self.warned {
        self.warned = true;
        log::warn!(
          "more than {} lines and trails, {} of their points or {} of their triangles in one frame: the rest draw nothing",
          cap.lines,
          cap.points,
          cap.codes
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
      let code = triangle_code(first + piece.point, piece.kind, 0);
      let triangles = piece_triangles(piece.kind, style.join, style.cap);
      self
        .codes
        .extend((0..triangles).map(|triangle| code | triangle));
    }
    start..3 * self.codes.len() as u32
  }
}

/// Where a line's triangles go: the GPU's stream (world lines) or the CPU
/// builder (the UI's lines, and the reference).
pub(crate) trait LineSink {
  /// The line table entry the next line takes
  fn next_line(&self) -> u32;
  /// Add the line through `points`; the range its triangles' corners take,
  /// three a triangle (empty when nothing is drawn)
  fn add_line(&mut self, points: &[PolyPoint], style: &PolyStyle) -> Range<u32>;
}

impl LineSink for LineStream {
  fn next_line(&self) -> u32 {
    self.lines.len() as u32
  }

  fn add_line(&mut self, points: &[PolyPoint], style: &PolyStyle) -> Range<u32> {
    self.polyline(points, style)
  }
}

impl LineSink for Triangles {
  fn next_line(&self) -> u32 {
    self.lines.len() as u32
  }

  fn add_line(&mut self, points: &[PolyPoint], style: &PolyStyle) -> Range<u32> {
    self.polyline(points, style)
  }
}

/// Add the line through `points` to `sink` and to `list` as one item, seen
/// by the cameras whose view meets its points' box grown by `MITER_LIMIT`
/// times its widest half width and how far its effect moves it (each
/// camera grows it by one of its pixels more, for the fringe).
pub(crate) fn push_polyline(
  list: &mut DrawList,
  sink: &mut impl LineSink,
  points: &[PolyPoint],
  style: &PolyStyle,
  look: LineLook,
) {
  let line = sink.next_line();
  let indices = sink.add_line(points, style);
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
  use crate::core::trail::LineJoin;

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
    // Each buffer holds what one storage binding of the device does
    assert_eq!(
      StreamCap::default(),
      StreamCap {
        points: 5_592_405,
        codes: 33_554_432,
        lines: 1_048_576,
      }
    );
    let limits = wgpu::Limits {
      max_storage_buffer_binding_size: 400,
      ..wgpu::Limits::default()
    };
    let mut stream = LineStream::for_device(&limits);
    assert_eq!(
      stream.cap(),
      StreamCap {
        points: 16,
        codes: 100,
        lines: 5,
      }
    );
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
      (stream.points.len(), stream.codes.len(), stream.lines.len()),
      (13, 86, 5)
    );
    assert!(stream.points.len() <= cap.points && stream.codes.len() <= cap.codes);
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
}
