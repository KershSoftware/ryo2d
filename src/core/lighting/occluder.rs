//! Light occluder components for shadow casting
//!
//! Occluders block light and create shadows. They are defined as polygons
//! (series of connected edges) that light cannot pass through.
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Create a rectangular occluder (like a wall)
//! let wall = world.spawn((
//!     Transform::at(100.0, 100.0),
//!     LightOccluder2D::rect(50.0, 100.0), // 50x100 rectangle
//! ));
//!
//! // Create a custom polygon occluder
//! let pillar = world.spawn((
//!     Transform::at(200.0, 200.0),
//!     LightOccluder2D::polygon(vec![
//!         Vec2::new(-10.0, -10.0),
//!         Vec2::new(10.0, -10.0),
//!         Vec2::new(10.0, 10.0),
//!         Vec2::new(-10.0, 10.0),
//!     ]),
//! ));
//! ```

use crate::core::ecs::Vec2;

/// A light occluder component that blocks light and creates shadows.
///
/// Occluders are defined as closed polygons. The vertices should be in
/// counter-clockwise order for correct shadow casting.
#[derive(Debug, Clone, PartialEq)]
pub struct LightOccluder2D {
  /// The vertices of the occluder polygon (local space, relative to entity transform)
  vertices: Vec<Vec2>,
  /// Whether this occluder is currently active
  enabled: bool,
}

impl LightOccluder2D {
  /// Create an occluder from a list of vertices.
  ///
  /// Vertices should be in counter-clockwise order and form a closed polygon.
  /// The polygon is automatically closed (last vertex connects to first).
  ///
  /// # Panics
  /// Panics if fewer than 3 vertices are provided.
  pub fn polygon(vertices: Vec<Vec2>) -> Self {
    assert!(
      vertices.len() >= 3,
      "LightOccluder2D requires at least 3 vertices"
    );
    Self {
      vertices,
      enabled: true,
    }
  }

  /// Create a rectangular occluder centered on the entity.
  ///
  /// # Arguments
  /// * `width` - Width of the rectangle
  /// * `height` - Height of the rectangle
  pub fn rect(width: f32, height: f32) -> Self {
    let hw = width / 2.0;
    let hh = height / 2.0;
    Self::polygon(vec![
      Vec2::new(-hw, -hh), // Top-left
      Vec2::new(hw, -hh),  // Top-right
      Vec2::new(hw, hh),   // Bottom-right
      Vec2::new(-hw, hh),  // Bottom-left
    ])
  }

  /// Create a rectangular occluder with a specific offset from the entity.
  ///
  /// # Arguments
  /// * `x` - X offset from entity position
  /// * `y` - Y offset from entity position
  /// * `width` - Width of the rectangle
  /// * `height` - Height of the rectangle
  pub fn rect_offset(x: f32, y: f32, width: f32, height: f32) -> Self {
    let hw = width / 2.0;
    let hh = height / 2.0;
    Self::polygon(vec![
      Vec2::new(x - hw, y - hh),
      Vec2::new(x + hw, y - hh),
      Vec2::new(x + hw, y + hh),
      Vec2::new(x - hw, y + hh),
    ])
  }

  /// Create a circular occluder approximation.
  ///
  /// # Arguments
  /// * `radius` - Radius of the circle
  /// * `segments` - Number of segments (more = smoother, 8-16 is usually enough)
  pub fn circle(radius: f32, segments: u32) -> Self {
    let segments = segments.max(4);
    let mut vertices = Vec::with_capacity(segments as usize);

    for i in 0..segments {
      let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
      vertices.push(Vec2::new(angle.cos() * radius, angle.sin() * radius));
    }

    Self::polygon(vertices)
  }

  /// Create a single line segment occluder (like a thin wall).
  ///
  /// Note: This creates a very thin rectangle since true line segments
  /// can cause rendering issues.
  ///
  /// # Arguments
  /// * `start` - Start point of the line
  /// * `end` - End point of the line
  /// * `thickness` - Thickness of the line (default ~1.0)
  pub fn line(start: Vec2, end: Vec2, thickness: f32) -> Self {
    let dir = Vec2::new(end.x - start.x, end.y - start.y);
    let len = (dir.x * dir.x + dir.y * dir.y).sqrt();
    if len < 0.001 {
      // Degenerate case: start == end, create a small square
      return Self::rect(thickness, thickness);
    }

    // Normalized perpendicular
    let perp = Vec2::new(-dir.y / len, dir.x / len);
    let half_thick = thickness / 2.0;

    Self::polygon(vec![
      Vec2::new(
        start.x + perp.x * half_thick,
        start.y + perp.y * half_thick,
      ),
      Vec2::new(end.x + perp.x * half_thick, end.y + perp.y * half_thick),
      Vec2::new(end.x - perp.x * half_thick, end.y - perp.y * half_thick),
      Vec2::new(
        start.x - perp.x * half_thick,
        start.y - perp.y * half_thick,
      ),
    ])
  }

  /// Get the vertices of this occluder.
  pub fn vertices(&self) -> &[Vec2] {
    &self.vertices
  }

  /// Get the edges of this occluder as (start, end) pairs.
  pub fn edges(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    self.vertices.windows(2).map(|w| (w[0], w[1])).chain(
      // Close the polygon
      std::iter::once((
        *self.vertices.last().unwrap(),
        *self.vertices.first().unwrap(),
      )),
    )
  }

  /// Check if this occluder is enabled.
  pub fn is_enabled(&self) -> bool {
    self.enabled
  }

  /// Enable or disable this occluder.
  pub fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
  }

  /// Builder method to set enabled state.
  pub fn with_enabled(mut self, enabled: bool) -> Self {
    self.enabled = enabled;
    self
  }

  /// Transform vertices by a position and rotation.
  ///
  /// Returns the vertices in world space.
  pub fn transform_vertices(&self, x: f32, y: f32, rotation: f32) -> Vec<Vec2> {
    let cos_r = rotation.cos();
    let sin_r = rotation.sin();

    self
      .vertices
      .iter()
      .map(|v| {
        // Rotate then translate
        let rx = v.x * cos_r - v.y * sin_r;
        let ry = v.x * sin_r + v.y * cos_r;
        Vec2::new(rx + x, ry + y)
      })
      .collect()
  }
}

impl Default for LightOccluder2D {
  fn default() -> Self {
    Self::rect(32.0, 32.0)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rect_occluder() {
    let occluder = LightOccluder2D::rect(100.0, 50.0);
    assert_eq!(occluder.vertices().len(), 4);
    assert!(occluder.is_enabled());
  }

  #[test]
  fn circle_occluder() {
    let occluder = LightOccluder2D::circle(50.0, 8);
    assert_eq!(occluder.vertices().len(), 8);
  }

  #[test]
  fn edges_iterator() {
    let occluder = LightOccluder2D::rect(100.0, 100.0);
    let edges: Vec<_> = occluder.edges().collect();
    assert_eq!(edges.len(), 4); // Rectangle has 4 edges
  }

  #[test]
  #[should_panic(expected = "at least 3 vertices")]
  fn polygon_requires_min_vertices() {
    LightOccluder2D::polygon(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0)]);
  }

  #[test]
  fn enabled_toggle() {
    let mut occluder = LightOccluder2D::rect(10.0, 10.0);
    assert!(occluder.is_enabled());

    occluder.set_enabled(false);
    assert!(!occluder.is_enabled());
  }

  #[test]
  fn transform_vertices_no_rotation() {
    let occluder = LightOccluder2D::rect(10.0, 10.0);
    let transformed = occluder.transform_vertices(100.0, 50.0, 0.0);

    // Center should be at (100, 50), so corners should be offset by +-5
    assert!((transformed[0].x - 95.0).abs() < 0.001);
    assert!((transformed[0].y - 45.0).abs() < 0.001);
  }
}
