//! Trails and lines as polylines: their points, widths, colours and texture
//! coordinates, and the draw-list item each one becomes.

use glam::Vec2;

use super::polyline::{PolyPoint, PolyStyle, Triangles};
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
      color: line_color(color),
      u,
    });
  }
  Some(PolyStyle {
    join: trail.join(),
    cap: trail.cap(),
    closed: false,
    params: build_effect_params(trail.effect(), trail.style()),
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
      color: line_color(line.color_at(t)),
      u: t,
    });
  }
  Some(PolyStyle {
    join: line.join(),
    cap: line.cap(),
    closed: line.is_closed(),
    params: build_effect_params(None, line.style()),
  })
}

/// Build `points` into `triangles` and add the result to `list` as one
/// item, seen by cameras whose view meets its points' box grown by its
/// widest half width and how far its effect moves it.
pub(crate) fn push_polyline(
  list: &mut DrawList,
  triangles: &mut Triangles,
  points: &[PolyPoint],
  style: &PolyStyle,
  look: LineLook,
) {
  let first_vertex = triangles.vertices.len() as u32;
  let indices = triangles.polyline(points, style);
  if indices.is_empty() {
    return;
  }
  let vertices = first_vertex..triangles.vertices.len() as u32;
  let (mut min, mut max) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
  let mut reach = 0.0f32;
  for point in points.iter().filter(|point| point.pos.is_finite()) {
    min = min.min(point.pos);
    max = max.max(point.pos);
    reach = reach.max(point.half_width);
  }
  let reach = Vec2::splat(reach + effect_reach(&style.params));
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
      vertices,
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

/// A colour decoded to linear light, for a vertex.
pub(crate) fn line_color(color: Color) -> [f32; 4] {
  color.to_linear()
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
    assert_eq!(ends.1.color, [0.0, 0.0, 0.0, 1.0]);
  }

  #[test]
  fn a_tiled_trail_runs_u_by_distance() {
    let texture = crate::core::asset::SpriteHandle::from_sprite(
      crate::core::graphics::Sprite::solid_rect(2, 2, 255, 255, 255, 255),
      "lines-tile",
    );
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
