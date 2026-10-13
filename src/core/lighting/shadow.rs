//! Shadow geometry generation for 2D lighting
//!
//! Generates shadow volume geometry from light occluders. For each light,
//! occluder edges facing away from the light are projected outward to create
//! shadow quads that block the light.
//!
//! # Algorithm
//!
//! For each shadow-casting light:
//! 1. Find occluders within the light's radius
//! 2. Transform occluder vertices to world space
//! 3. For each edge facing away from the light (backfacing):
//!    - Project edge vertices outward from light to light's radius
//!    - Create a quad: [edge_start, edge_end, projected_end, projected_start]
//! 4. Return all shadow quads for rendering

use crate::core::ecs::{GlobalTransform, Vec2};
use crate::core::lighting::{Light2D, LightOccluder2D};
use crate::core::ui::Color;

/// Maximum number of shadow vertices per frame (4 vertices per quad)
pub const MAX_SHADOW_VERTICES: usize = 8192;

/// A shadow vertex for GPU rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShadowVertex {
  /// Position in world space
  pub position: [f32; 2],
  /// Light position (for per-light shadows)
  pub light_pos: [f32; 2],
  /// Light radius (for falloff calculation)
  pub light_radius: f32,
  /// Shadow softness (0.0 = hard, 1.0 = very soft)
  pub softness: f32,
  /// Shadow color (packed RGBA as u32, unpacked in shader)
  pub shadow_color: u32,
  /// Padding for alignment
  pub _padding: u32,
}

impl ShadowVertex {
  pub fn new(x: f32, y: f32, light_x: f32, light_y: f32, light_radius: f32, softness: f32) -> Self {
    Self::with_color(x, y, light_x, light_y, light_radius, softness, 0x000000FF)
  }

  pub fn with_color(
    x: f32,
    y: f32,
    light_x: f32,
    light_y: f32,
    light_radius: f32,
    softness: f32,
    shadow_color: u32,
  ) -> Self {
    Self {
      position: [x, y],
      light_pos: [light_x, light_y],
      light_radius,
      softness,
      shadow_color,
      _padding: 0,
    }
  }

  /// Pack RGBA color components into a u32
  pub fn pack_color(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | ((a as u32) << 24)
  }
}

/// `color` decoded from sRGB to linear light, packed as RGBA bytes.
fn pack_linear(color: crate::core::ui::Color) -> u32 {
  let [r, g, b, a] = color.to_linear();
  let byte = |c: f32| (c * 255.0).round() as u8;
  ShadowVertex::pack_color(byte(r), byte(g), byte(b), byte(a))
}

/// Collected shadow geometry for a frame
#[derive(Default)]
pub struct ShadowGeometry {
  /// Shadow vertices (triangles, 3 vertices each)
  pub vertices: Vec<ShadowVertex>,
}

impl ShadowGeometry {
  pub fn new() -> Self {
    Self {
      vertices: Vec::with_capacity(MAX_SHADOW_VERTICES),
    }
  }

  pub fn clear(&mut self) {
    self.vertices.clear();
  }

  /// Check if there's any shadow geometry to render
  pub fn is_empty(&self) -> bool {
    self.vertices.is_empty()
  }

  /// Number of vertices (for draw call)
  pub fn vertex_count(&self) -> u32 {
    self.vertices.len() as u32
  }
}

/// Generate shadow geometry for all shadow-casting lights and occluders.
///
/// # Arguments
/// * `lights` - Iterator of (world position, light) pairs
/// * `occluders` - (global transform, occluder) pairs
///
/// # Returns
/// Shadow geometry ready for GPU upload
pub fn generate_shadow_geometry<'a>(
  lights: impl Iterator<Item = (&'a Vec2, &'a Light2D)>,
  occluders: &[(&GlobalTransform, &LightOccluder2D)],
) -> ShadowGeometry {
  let mut geometry = ShadowGeometry::new();

  // Collect shadow-casting lights
  let shadow_lights: Vec<_> = lights
    .filter(|(_, light)| light.casts_shadows())
    .map(|(t, l)| {
      let (radius, is_spot, angle, direction, softness, color) = match l {
        Light2D::Point {
          radius,
          shadow_softness,
          shadow_color,
          ..
        } => (*radius, false, 360.0, 0.0, *shadow_softness, *shadow_color),
        Light2D::Spot {
          radius,
          angle,
          direction,
          shadow_softness,
          shadow_color,
          ..
        } => (
          *radius,
          true,
          *angle,
          *direction,
          *shadow_softness,
          *shadow_color,
        ),
      };
      (
        Vec2::new(t.x, t.y),
        radius,
        is_spot,
        angle,
        direction,
        softness,
        color,
      )
    })
    .collect();

  // For each shadow-casting light
  for (
    light_pos,
    light_radius,
    is_spot,
    spot_angle,
    spot_direction,
    shadow_softness,
    shadow_color,
  ) in shadow_lights
  {
    // For each occluder
    for (occluder_transform, occluder) in occluders {
      if !occluder.is_enabled() {
        continue;
      }

      // Transform occluder vertices to world space
      let occluder_center = occluder_transform.translation();
      let world_vertices = occluder.transform_vertices(
        occluder_center.x,
        occluder_center.y,
        occluder_transform.rotation(),
      );

      // Quick distance check - skip if occluder center is too far
      let dist_to_light = ((occluder_center.x - light_pos.x).powi(2)
        + (occluder_center.y - light_pos.y).powi(2))
      .sqrt();

      // Rough bounds check (occluder could still cast shadows even if center is outside)
      if dist_to_light > light_radius * 2.0 {
        continue;
      }

      // Generate shadow quads for backfacing edges
      generate_shadow_quads_for_occluder(
        &mut geometry,
        &world_vertices,
        light_pos,
        light_radius,
        is_spot,
        spot_angle,
        spot_direction,
        shadow_softness,
        shadow_color,
      );
    }
  }

  geometry
}

/// Generate shadow quads for a single occluder relative to a light.
#[allow(clippy::too_many_arguments)]
fn generate_shadow_quads_for_occluder(
  geometry: &mut ShadowGeometry,
  vertices: &[Vec2],
  light_pos: Vec2,
  light_radius: f32,
  is_spot: bool,
  spot_angle: f32,
  spot_direction: f32,
  shadow_softness: f32,
  shadow_color: Color,
) {
  if vertices.len() < 3 {
    return;
  }

  // Process each edge of the polygon
  for i in 0..vertices.len() {
    let v0 = vertices[i];
    let v1 = vertices[(i + 1) % vertices.len()];

    // Edge midpoint for facing check
    let edge_mid = Vec2::new((v0.x + v1.x) / 2.0, (v0.y + v1.y) / 2.0);

    // Vector from light to edge midpoint
    let to_edge = Vec2::new(edge_mid.x - light_pos.x, edge_mid.y - light_pos.y);

    // Edge normal (perpendicular to edge, pointing outward for CCW winding)
    let edge_dir = Vec2::new(v1.x - v0.x, v1.y - v0.y);
    let edge_normal = Vec2::new(-edge_dir.y, edge_dir.x); // 90 degree rotation

    // Dot product: positive means edge faces away from light (backfacing)
    let facing = to_edge.x * edge_normal.x + to_edge.y * edge_normal.y;

    if facing <= 0.0 {
      // Edge faces toward light, no shadow from this edge
      continue;
    }

    // For spotlights, check if edge is within the cone
    if is_spot {
      let angle_rad = spot_direction.to_radians();
      let half_angle_rad = (spot_angle / 2.0).to_radians();
      let light_dir = Vec2::new(angle_rad.cos(), angle_rad.sin());

      // Check if edge midpoint is within spotlight cone
      let to_edge_len = (to_edge.x * to_edge.x + to_edge.y * to_edge.y).sqrt();
      if to_edge_len > 0.001 {
        let to_edge_normalized = Vec2::new(to_edge.x / to_edge_len, to_edge.y / to_edge_len);
        let dot = light_dir.x * to_edge_normalized.x + light_dir.y * to_edge_normalized.y;
        let angle_to_edge = dot.clamp(-1.0, 1.0).acos();

        // Skip edges outside the spotlight cone (with some margin for smooth shadows)
        if angle_to_edge > half_angle_rad + 0.2 {
          continue;
        }
      }
    }

    // Project vertices away from light
    let proj0 = project_vertex(v0, light_pos, light_radius);
    let proj1 = project_vertex(v1, light_pos, light_radius);

    // Create shadow quad as two triangles
    // Quad: v0 -> v1 -> proj1 -> proj0
    // Triangle 1: v0, v1, proj1
    // Triangle 2: v0, proj1, proj0

    if geometry.vertices.len() + 6 > MAX_SHADOW_VERTICES {
      // Out of space, stop generating
      return;
    }

    let lx = light_pos.x;
    let ly = light_pos.y;
    let lr = light_radius;
    let ss = shadow_softness;
    let sc = pack_linear(shadow_color);

    // Triangle 1
    geometry
      .vertices
      .push(ShadowVertex::with_color(v0.x, v0.y, lx, ly, lr, ss, sc));
    geometry
      .vertices
      .push(ShadowVertex::with_color(v1.x, v1.y, lx, ly, lr, ss, sc));
    geometry.vertices.push(ShadowVertex::with_color(
      proj1.x, proj1.y, lx, ly, lr, ss, sc,
    ));

    // Triangle 2
    geometry
      .vertices
      .push(ShadowVertex::with_color(v0.x, v0.y, lx, ly, lr, ss, sc));
    geometry.vertices.push(ShadowVertex::with_color(
      proj1.x, proj1.y, lx, ly, lr, ss, sc,
    ));
    geometry.vertices.push(ShadowVertex::with_color(
      proj0.x, proj0.y, lx, ly, lr, ss, sc,
    ));
  }
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
  #[test]
  fn shadow_colour_packs_decoded() {
    use crate::core::ui::Color;
    let packed = super::pack_linear(Color::new(128, 255, 0, 128));
    // 128 decodes to 0.2159 (55 of 255); alpha stays straight
    assert_eq!(packed, 55 | (255 << 8) | (128 << 24));
  }

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
  fn empty_geometry_by_default() {
    let geometry = ShadowGeometry::new();
    assert!(geometry.is_empty());
    assert_eq!(geometry.vertex_count(), 0);
  }

  #[test]
  fn shadow_vertex_size() {
    // Ensure proper GPU alignment
    assert_eq!(std::mem::size_of::<ShadowVertex>(), 32); // 8 floats * 4 bytes
  }
}
