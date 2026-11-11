//! Shadow volumes: the quads each occluder edge facing away from a light
//! casts, as triangles in the batch's triangle stream

use std::ops::Range;

use glam::Vec2;

use super::plan::casts;
use super::{Light2D, LightOccluder2D};
use crate::core::ecs::{Floor, GlobalTransform};
use crate::core::render::batch::{Triangles, Vertex2D};

/// Push the shadow volumes `light` at `position` on `floor` casts from
/// `occluders` (each on its floor) into `triangles`, drawn `lift` up;
/// returns their index range (empty when it casts none).
///
/// Only occluders on a floor that meets the light's block it. The volumes
/// are cast where the light and its occluders stand, then every vertex and
/// the light's position move by `lift`, so they draw where the light does.
/// Each vertex carries what `shadow.wgsl` needs: the light's position (in
/// `uv`), its radius and shadow softness (in `params`), and its shadow
/// colour in linear light, whose alpha is the shadow's strength.
pub(crate) fn push_shadow_volumes(
  position: Vec2,
  lift: Vec2,
  light: &Light2D,
  floor: Floor,
  occluders: &[(GlobalTransform, &LightOccluder2D, Floor)],
  triangles: &mut Triangles,
) -> Range<u32> {
  let start = triangles.indices.len() as u32;
  if !light.casts_shadows() {
    return start..start;
  }
  let radius = light.radius();
  let spot = light.cone();
  // A softness that is not finite draws hard shadows
  let softness = light.shadow_softness();
  let softness = if softness.is_finite() { softness } else { 0.0 };
  let drawn = position + lift;
  let vertex = |at: Vec2| Vertex2D {
    position: (at + lift).into(),
    uv: drawn.into(),
    color: light.shadow_color().to_linear(),
    params: [radius, softness, 0.0, 0.0],
    across: [0.0; 2],
    bounds: [0.0; 4],
  };
  for (transform, occluder, on) in occluders {
    if !occluder.is_enabled() || !floor.meets(*on) {
      continue;
    }
    let center = transform.translation();
    // Too far for its shadow to reach the light's radius: its nearest vertex
    // is at least its extent nearer than its entity
    if center.distance(position) - occluder.extent() > radius * 2.0 {
      continue;
    }
    // Placed as they are walked, so a frame collects nothing
    let corners = occluder.placed(center.x, center.y, transform.rotation());
    let count = corners.len();
    // A polygon's last edge returns to its first vertex; a chain's does not
    let (least, closing) = if occluder.is_closed() {
      (3, corners.clone().next())
    } else {
      (2, None)
    };
    if count < least || !corners.clone().all(|corner| corner.is_finite()) {
      continue;
    }
    let ends = corners.clone().skip(1).chain(closing);
    for (v0, v1) in corners.zip(ends) {
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
    let placed = [(at, &occluder, Floor::GROUND)];
    let range = push_shadow_volumes(
      Vec2::ZERO,
      Vec2::ZERO,
      &light,
      Floor::GROUND,
      &placed,
      &mut triangles,
    );
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
    let placed = [(at, &occluder, Floor::GROUND)];
    let range = push_shadow_volumes(
      Vec2::ZERO,
      Vec2::ZERO,
      &light,
      Floor::GROUND,
      &placed,
      &mut triangles,
    );
    assert!(range.is_empty());
  }

  /// The shadow quads `occluders` (each at its position) cast from a light
  /// at `from`, each as its four corners, sorted, all on the ground floor
  fn quads(from: Vec2, occluders: &[(Vec2, &LightOccluder2D)]) -> Vec<[[u32; 2]; 4]> {
    let grounded: Vec<(Vec2, &LightOccluder2D, Floor)> = occluders
      .iter()
      .map(|&(at, occluder)| (at, occluder, Floor::GROUND))
      .collect();
    floor_quads(Floor::GROUND, from, &grounded)
  }

  /// `quads` of a light on `floor`, each occluder on its own floor
  fn floor_quads(
    floor: Floor,
    from: Vec2,
    occluders: &[(Vec2, &LightOccluder2D, Floor)],
  ) -> Vec<[[u32; 2]; 4]> {
    let light = Light2D::point(crate::core::ui::Color::WHITE, 200.0).with_shadows();
    let placed: Vec<(GlobalTransform, &LightOccluder2D, Floor)> = occluders
      .iter()
      .map(|&(at, occluder, on)| {
        let transform = crate::core::ecs::Transform::from_translation(at);
        (GlobalTransform::from_transform(&transform), occluder, on)
      })
      .collect();
    let mut triangles = Triangles::default();
    push_shadow_volumes(from, Vec2::ZERO, &light, floor, &placed, &mut triangles);
    let bits = |v: &crate::core::render::batch::Vertex2D| v.position.map(f32::to_bits);
    let mut quads: Vec<[[u32; 2]; 4]> = triangles
      .vertices
      .chunks(4)
      .map(|quad| {
        [
          bits(&quad[0]),
          bits(&quad[1]),
          bits(&quad[2]),
          bits(&quad[3]),
        ]
      })
      .collect();
    quads.sort();
    quads
  }

  #[test]
  fn a_chain_occluder_casts_like_the_polygon_it_bounds() {
    // An L, counter-clockwise on screen as polygons are
    let ring = [
      Vec2::new(0.0, 0.0),
      Vec2::new(0.0, 30.0),
      Vec2::new(30.0, 30.0),
      Vec2::new(30.0, 20.0),
      Vec2::new(10.0, 20.0),
      Vec2::new(10.0, 0.0),
    ];
    let polygon = LightOccluder2D::polygon(ring.to_vec());
    let mut closed = ring.to_vec();
    closed.push(ring[0]);
    let whole = LightOccluder2D::chain(closed);
    // The same ring as two chains meeting at their ends
    let first = LightOccluder2D::chain(ring[..4].to_vec());
    let second = LightOccluder2D::chain(vec![ring[3], ring[4], ring[5], ring[0]]);
    let at = Vec2::new(40.0, 10.0);
    for from in [
      Vec2::ZERO,
      Vec2::new(-60.0, 15.0),
      Vec2::new(100.0, 100.0),
      Vec2::new(55.0, -40.0),
      Vec2::new(45.0, 22.0),
    ] {
      let expected = quads(from, &[(at, &polygon)]);
      assert!(!expected.is_empty(), "from {from}");
      assert_eq!(
        quads(from, &[(at, &whole)]),
        expected,
        "closed by its last vertex, from {from}"
      );
      assert_eq!(
        quads(from, &[(at, &first), (at, &second)]),
        expected,
        "in two pieces, from {from}"
      );
    }
    // An open chain casts no edge from its last vertex back to its first:
    // the floor's top lit from above casts nothing, its bottom edge being
    // another chain's
    let top = LightOccluder2D::chain(vec![Vec2::new(10.0, 0.0), Vec2::new(-10.0, 0.0)]);
    assert!(quads(Vec2::new(0.0, -50.0), &[(Vec2::ZERO, &top)]).is_empty());
    assert_eq!(quads(Vec2::new(0.0, 50.0), &[(Vec2::ZERO, &top)]).len(), 1);
    assert_eq!(top.edges().count(), 1);
    assert!(!top.is_closed() && polygon.is_closed());
  }

  #[test]
  fn a_light_on_a_floor_is_blocked_only_by_that_floor_s_occluders() {
    let wall = LightOccluder2D::rect(4.0, 16.0);
    let at = Vec2::new(40.0, 0.0);
    let cast = floor_quads(Floor::GROUND, Vec2::ZERO, &[(at, &wall, Floor::GROUND)]);
    assert!(!cast.is_empty());
    let on_first =
      |wall_floor: Floor| floor_quads(Floor::On(1), Vec2::ZERO, &[(at, &wall, wall_floor)]);
    assert!(on_first(Floor::GROUND).is_empty(), "the ground floor's");
    assert!(on_first(Floor::On(2)).is_empty(), "the floor above's");
    assert_eq!(on_first(Floor::On(1)), cast, "its own floor's");
    assert_eq!(on_first(Floor::Every), cast, "every floor's");
    // Nor does a ground floor light meet floor 1's
    let ground = floor_quads(Floor::GROUND, Vec2::ZERO, &[(at, &wall, Floor::On(1))]);
    assert!(ground.is_empty());

    // Lifted, every vertex and the light it knows move by the lift: the
    // volumes are those cast where it stands
    let light = Light2D::point(crate::core::ui::Color::WHITE, 200.0).with_shadows();
    let placed = [(
      GlobalTransform::from_transform(&crate::core::ecs::Transform::from_translation(at)),
      &wall,
      Floor::On(1),
    )];
    let from = Vec2::new(5.0, 3.0);
    let lift = Vec2::new(0.0, -32.0);
    let (mut standing, mut lifted) = (Triangles::default(), Triangles::default());
    push_shadow_volumes(
      from,
      Vec2::ZERO,
      &light,
      Floor::On(1),
      &placed,
      &mut standing,
    );
    let range = push_shadow_volumes(from, lift, &light, Floor::On(1), &placed, &mut lifted);
    assert!(!range.is_empty());
    assert_eq!(lifted.indices, standing.indices);
    assert_eq!(lifted.vertices.len(), standing.vertices.len());
    for (up, there) in lifted.vertices.iter().zip(&standing.vertices) {
      assert_eq!(Vec2::from(up.position), Vec2::from(there.position) + lift);
      assert_eq!(Vec2::from(up.uv), from + lift);
      assert_eq!((up.color, up.params), (there.color, there.params));
    }
  }

  #[test]
  fn a_light_on_every_floor_is_blocked_by_all() {
    let wall = LightOccluder2D::rect(4.0, 16.0);
    let at = Vec2::new(40.0, 0.0);
    let cast = floor_quads(Floor::GROUND, Vec2::ZERO, &[(at, &wall, Floor::GROUND)]);
    assert!(!cast.is_empty());
    for floor in [Floor::GROUND, Floor::On(1), Floor::On(255), Floor::Every] {
      let quads = floor_quads(Floor::Every, Vec2::ZERO, &[(at, &wall, floor)]);
      assert_eq!(quads, cast, "{floor:?}");
    }
  }
}
