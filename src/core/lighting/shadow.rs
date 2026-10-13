//! Shadow volumes: the quads each occluder edge facing away from a light
//! casts, as triangles in the batch's triangle stream

use std::ops::Range;

use glam::Vec2;

use super::{Light2D, LightOccluder2D};
use crate::core::ecs::GlobalTransform;
use crate::core::render::batch::{Triangles, Vertex2D};

/// Push the shadow volumes `light` at `position` casts from `occluders`
/// into `triangles`; returns their index range (empty when it casts none).
///
/// Each vertex carries what `shadow.wgsl` needs: the light's position (in
/// `uv`), its radius and shadow softness (in `params`), and its shadow
/// colour in linear light, whose alpha is the shadow's strength.
pub(crate) fn push_shadow_volumes(
  position: Vec2,
  light: &Light2D,
  occluders: &[(GlobalTransform, &LightOccluder2D)],
  triangles: &mut Triangles,
) -> Range<u32> {
  let start = triangles.indices.len() as u32;
  if !light.casts_shadows() {
    return start..start;
  }
  let radius = light.radius();
  let spot = match light {
    Light2D::Point { .. } => None,
    Light2D::Spot {
      angle, direction, ..
    } => Some((*angle, *direction)),
  };
  // A softness that is not finite draws hard shadows
  let softness = light.shadow_softness();
  let softness = if softness.is_finite() { softness } else { 0.0 };
  let vertex = |at: Vec2| Vertex2D {
    position: at.into(),
    uv: position.into(),
    color: light.shadow_color().to_linear(),
    params: [radius, softness, 0.0, 0.0],
    across: [0.0; 2],
    bounds: [0.0; 4],
  };
  for (transform, occluder) in occluders {
    if !occluder.is_enabled() {
      continue;
    }
    let center = transform.translation();
    // Too far for its shadow to reach the light's radius
    if center.distance(position) > radius * 2.0 {
      continue;
    }
    let corners = occluder.transform_vertices(center.x, center.y, transform.rotation());
    if corners.len() < 3 || !corners.iter().all(|corner| corner.is_finite()) {
      continue;
    }
    for i in 0..corners.len() {
      let (v0, v1) = (corners[i], corners[(i + 1) % corners.len()]);
      if !casts(v0, v1, position, spot) {
        continue;
      }
      let (p0, p1) = (
        project_vertex(v0, position, radius),
        project_vertex(v1, position, radius),
      );
      let first = triangles.vertices.len() as u32;
      triangles
        .vertices
        .extend([vertex(v0), vertex(v1), vertex(p1), vertex(p0)]);
      triangles
        .indices
        .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }
  }
  start..triangles.indices.len() as u32
}

/// Whether the edge `v0`-`v1` faces away from a light at `light` (and, for
/// a spot of `(angle, direction)` in degrees, lies near its cone).
fn casts(v0: Vec2, v1: Vec2, light: Vec2, spot: Option<(f32, f32)>) -> bool {
  let to_edge = (v0 + v1) * 0.5 - light;
  let edge = v1 - v0;
  // Edge normal for counter-clockwise winding: positive means it faces away
  if to_edge.dot(Vec2::new(-edge.y, edge.x)) <= 0.0 {
    return false;
  }
  let Some((angle, direction)) = spot else {
    return true;
  };
  let length = to_edge.length();
  if length <= 0.001 {
    return true;
  }
  let facing = Vec2::from_angle(direction.to_radians());
  let off_axis = facing.dot(to_edge / length).clamp(-1.0, 1.0).acos();
  // A margin past the cone keeps soft edges shadowed
  off_axis <= (angle / 2.0).to_radians() + 0.2
}

/// Project a vertex away from a light source to the light's radius.
fn project_vertex(vertex: Vec2, light_pos: Vec2, light_radius: f32) -> Vec2 {
  let dx = vertex.x - light_pos.x;
  let dy = vertex.y - light_pos.y;
  let dist = (dx * dx + dy * dy).sqrt();

  if dist < 0.001 {
    // Vertex is at light position, project in arbitrary direction
    return Vec2::new(light_pos.x + light_radius, light_pos.y);
  }

  // Normalize and extend to light radius (plus margin for full coverage)
  let extend_dist = light_radius * 1.5; // Extend beyond light radius for full shadow coverage
  let scale = extend_dist / dist;

  Vec2::new(light_pos.x + dx * scale, light_pos.y + dy * scale)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn project_vertex_extends_to_radius() {
    let light = Vec2::new(0.0, 0.0);
    let vertex = Vec2::new(10.0, 0.0);
    let projected = project_vertex(vertex, light, 100.0);

    // Should be extended along the same direction
    assert!(projected.x > vertex.x);
    assert!((projected.y - 0.0).abs() < 0.001);
  }

  #[test]
  fn an_occluder_casts_one_quad_per_back_edge() {
    let light = Light2D::point(crate::core::ui::Color::WHITE, 100.0).with_shadows();
    let occluder = LightOccluder2D::rect(4.0, 16.0);
    let at = GlobalTransform::from_transform(&crate::core::ecs::Transform::from_xy(40.0, 0.0));
    let mut triangles = Triangles::default();
    let range = push_shadow_volumes(Vec2::ZERO, &light, &[(at, &occluder)], &mut triangles);
    assert!(
      !range.is_empty() && range.len().is_multiple_of(6),
      "{range:?}"
    );
    assert_eq!(
      triangles.vertices.len() * 6,
      range.len() * 4,
      "four corners per quad"
    );
    // Every vertex knows its light and is fully strong (black shadow)
    assert!(
      triangles
        .vertices
        .iter()
        .all(|v| v.uv == [0.0, 0.0] && v.color[3] == 1.0)
    );
  }

  #[test]
  fn a_light_without_shadows_casts_none() {
    let light = Light2D::point(crate::core::ui::Color::WHITE, 100.0);
    let occluder = LightOccluder2D::rect(4.0, 16.0);
    let at = GlobalTransform::from_transform(&crate::core::ecs::Transform::from_xy(40.0, 0.0));
    let mut triangles = Triangles::default();
    assert!(push_shadow_volumes(Vec2::ZERO, &light, &[(at, &occluder)], &mut triangles).is_empty());
  }
}
