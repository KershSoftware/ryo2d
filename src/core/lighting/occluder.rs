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
//!     Transform::from_xy(100.0, 100.0),
//!     LightOccluder2D::rect(50.0, 100.0), // 50x100 rectangle
//! )).id();
//!
//! // Create a custom polygon occluder
//! let pillar = world.spawn((
//!     Transform::from_xy(200.0, 200.0),
//!     LightOccluder2D::polygon(vec![
//!         Vec2::new(-10.0, -10.0),
//!         Vec2::new(10.0, -10.0),
//!         Vec2::new(10.0, 10.0),
//!         Vec2::new(-10.0, 10.0),
//!     ]),
//! )).id();
//! ```

use crate::core::ecs::Component;
use crate::core::ecs::GlobalTransform;
use crate::core::ecs::Vec2;

/// A light occluder component that blocks light and creates shadows.
///
/// Occluders are closed polygons, or open chains of edges (`chain`): part
/// of a boundary whose other parts are elsewhere. The vertices should be in
/// counter-clockwise order on screen for correct shadow casting: an edge
/// casts when it faces away from the light, its outside being on its right
/// as it runs on screen (y down).
///
/// An occluder is on its own `Floor`, else its nearest ancestor's, else the
/// ground floor, and blocks only the lights on a floor that meets it.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct LightOccluder2D {
  /// The vertices of the occluder polygon (local space, relative to entity transform)
  vertices: Vec<Vec2>,
  /// Whether the last vertex joins the first (a polygon), or not (a chain)
  closed: bool,
  /// Whether this occluder is currently active
  enabled: bool,
  /// Whether it shadows itself: its faces toward a light cast, not those
  /// away from it, so its own area is in its shadow
  self_shadow: bool,
  /// The farthest any vertex lies from the entity, kept from construction
  /// (the vertices never change)
  extent: f32,
}

/// The farthest any of `vertices` lies from the origin; infinite if one is
/// not finite
fn extent_of(vertices: &[Vec2]) -> f32 {
  vertices.iter().fold(0.0, |far: f32, v| {
    let length = v.length();
    if length.is_finite() {
      far.max(length)
    } else {
      f32::INFINITY
    }
  })
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
      extent: extent_of(&vertices),
      vertices,
      closed: true,
      enabled: true,
      self_shadow: false,
    }
  }

  /// An open chain of edges, each from a vertex to the next and none from
  /// the last back to the first: a stretch of a boundary, such as a tile
  /// map's, whose other stretches are other chains. Each edge casts exactly
  /// as it would as an edge of the polygon the boundary closes, so its
  /// vertices run the way that polygon's would (counter-clockwise on
  /// screen), and chains that together close a ring cast as that polygon
  /// does. A chain whose last vertex repeats its first is closed.
  ///
  /// # Panics
  /// Panics if fewer than 2 vertices are provided.
  pub fn chain(vertices: Vec<Vec2>) -> Self {
    assert!(
      vertices.len() >= 2,
      "a LightOccluder2D chain requires at least 2 vertices"
    );
    Self {
      extent: extent_of(&vertices),
      vertices,
      closed: false,
      enabled: true,
      self_shadow: false,
    }
  }

  /// Whether the last vertex joins the first: true for a polygon, false
  /// for a chain
  pub fn is_closed(&self) -> bool {
    self.closed
  }

  /// Create a rectangular occluder centered on the entity.
  ///
  /// # Arguments
  /// * `width` - Width of the rectangle
  /// * `height` - Height of the rectangle
  pub fn rect(width: f32, height: f32) -> Self {
    let hw = width / 2.0;
    let hh = height / 2.0;
    // CCW winding in screen space (Y-down): top-left → bottom-left → bottom-right → top-right
    Self::polygon(vec![
      Vec2::new(-hw, -hh), // Top-left
      Vec2::new(-hw, hh),  // Bottom-left
      Vec2::new(hw, hh),   // Bottom-right
      Vec2::new(hw, -hh),  // Top-right
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
    // CCW winding in screen space (Y-down)
    Self::polygon(vec![
      Vec2::new(x - hw, y - hh), // Top-left
      Vec2::new(x - hw, y + hh), // Bottom-left
      Vec2::new(x + hw, y + hh), // Bottom-right
      Vec2::new(x + hw, y - hh), // Top-right
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
      Vec2::new(start.x + perp.x * half_thick, start.y + perp.y * half_thick),
      Vec2::new(end.x + perp.x * half_thick, end.y + perp.y * half_thick),
      Vec2::new(end.x - perp.x * half_thick, end.y - perp.y * half_thick),
      Vec2::new(start.x - perp.x * half_thick, start.y - perp.y * half_thick),
    ])
  }

  /// Get the vertices of this occluder.
  pub fn vertices(&self) -> &[Vec2] {
    &self.vertices
  }

  /// The farthest any vertex lies from the entity's origin, which pads an
  /// interpolated occluder's bounds in the occluder index (infinite when a
  /// vertex is not finite)
  pub(crate) fn extent(&self) -> f32 {
    self.extent
  }

  /// Get the edges of this occluder as (start, end) pairs: a polygon's
  /// last edge returns to its first vertex, a chain's does not.
  pub fn edges(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    let closing = match (self.closed, self.vertices.first(), self.vertices.last()) {
      (true, Some(first), Some(last)) => Some((*last, *first)),
      _ => None,
    };
    self
      .vertices
      .windows(2)
      .map(|w| (w[0], w[1]))
      .chain(closing)
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

  /// Whether it shadows itself (`with_self_shadow`)
  pub fn self_shadow(&self) -> bool {
    self.self_shadow
  }

  /// Make it shadow itself, or not (`with_self_shadow`)
  pub fn set_self_shadow(&mut self, on: bool) {
    self.self_shadow = on;
  }

  /// Builder method: whether it shadows itself (off by default). A
  /// self-shadowing occluder casts from its faces toward each light rather
  /// than those away from it, so the shadow starts at its near face and its
  /// own area lies in it: a top-down wall seen from above stays dark
  /// rather than lit on top. Leave it off for a pillar or crate whose top
  /// should catch the light. A chain's edges cast as the self-shadowing
  /// polygon they bound would; a chain has no inside, so a light within the
  /// polygon it bounds is not known to be inside (a closed occluder holding
  /// a light casts nothing for it, either way)
  pub fn with_self_shadow(mut self, on: bool) -> Self {
    self.self_shadow = on;
    self
  }

  /// The vertices in world space, at `(x, y)` turned by `rotation`
  pub(crate) fn placed(
    &self,
    x: f32,
    y: f32,
    rotation: f32,
  ) -> impl ExactSizeIterator<Item = Vec2> + Clone + '_ {
    let cos_r = rotation.cos();
    let sin_r = rotation.sin();
    self.vertices.iter().map(move |v| {
      // Rotate then translate
      let rx = v.x * cos_r - v.y * sin_r;
      let ry = v.x * sin_r + v.y * cos_r;
      Vec2::new(rx + x, ry + y)
    })
  }

  /// The box of the vertices placed by `transform`'s translation and
  /// rotation (as drawn: its scale is not read), as (min, max); `None` when
  /// a placed vertex is not finite
  pub(crate) fn placed_bounds(&self, transform: &GlobalTransform) -> Option<(Vec2, Vec2)> {
    let at = transform.translation();
    let mut min = Vec2::INFINITY;
    let mut max = Vec2::NEG_INFINITY;
    for v in self.placed(at.x, at.y, transform.rotation()) {
      if !v.is_finite() {
        return None;
      }
      min = min.min(v);
      max = max.max(v);
    }
    Some((min, max))
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
  #[should_panic(expected = "at least 2 vertices")]
  fn chain_requires_min_vertices() {
    LightOccluder2D::chain(vec![Vec2::new(0.0, 0.0)]);
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
  fn the_extent_is_the_farthest_vertex() {
    assert_eq!(LightOccluder2D::rect(6.0, 8.0).extent(), 5.0);
    let chain = LightOccluder2D::chain(vec![Vec2::new(-256.0, 0.0), Vec2::new(256.0, 16.0)]);
    assert!((chain.extent() - 256.5).abs() < 0.01, "{}", chain.extent());
    let broken = LightOccluder2D::chain(vec![Vec2::ZERO, Vec2::new(f32::NAN, 0.0)]);
    assert_eq!(broken.extent(), f32::INFINITY);
  }

  #[test]
  fn placed_vertices_without_rotation() {
    let occluder = LightOccluder2D::rect(10.0, 10.0);
    let transformed: Vec<Vec2> = occluder.placed(100.0, 50.0, 0.0).collect();

    // Center should be at (100, 50), so corners should be offset by +-5
    assert!((transformed[0].x - 95.0).abs() < 0.001);
    assert!((transformed[0].y - 45.0).abs() < 0.001);
  }
}
