//! Line rendering component
//!
//! Renders arbitrary lines and polylines, useful for laser beams,
//! debug visualization, drawing mechanics, and UI elements.
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Create a simple line
//! let laser = world.spawn((
//!     Line2D::new(
//!         vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)],
//!         Color::RED,
//!         3.0,
//!     ),
//! ));
//!
//! // Create a bezier curve
//! let curve = Line2D::bezier(
//!     Vec2::new(0.0, 0.0),      // start
//!     Vec2::new(50.0, -50.0),   // control 1
//!     Vec2::new(100.0, -50.0),  // control 2
//!     Vec2::new(150.0, 0.0),    // end
//!     Color::BLUE,
//!     2.0,
//!     20,                        // segments
//! );
//! ```

use super::{LineCap, LineJoin, TrailStyle};
use crate::core::ecs::Vec2;
use crate::core::ui::Color;

/// A 2D line or polyline component.
///
/// Unlike trails which automatically record position history,
/// lines have explicitly defined points that you control.
#[derive(Debug, Clone)]
pub struct Line2D {
  /// The points that make up the line (in local/world space)
  points: Vec<Vec2>,
  /// Line color
  color: Color,
  /// Optional end color for gradient
  end_color: Option<Color>,
  /// Line width at start
  width: f32,
  /// Line width at end (for tapering)
  end_width: f32,
  /// Whether the line is visible
  visible: bool,
  /// Whether to close the line (connect last point to first)
  closed: bool,
  /// Z-order for rendering
  z_order: i32,
  /// Rendering style for this line
  style: TrailStyle,
  /// How endpoints are drawn
  cap: LineCap,
  /// How segments connect at corners
  join: LineJoin,
  /// Whether scene lighting applies (false: full brightness over the lit scene)
  lighting: bool,
}

impl Line2D {
  /// Create a new line from a list of points.
  ///
  /// # Arguments
  /// * `points` - The points that define the line
  /// * `color` - The line color
  /// * `width` - The line width in pixels
  pub fn new(points: Vec<Vec2>, color: Color, width: f32) -> Self {
    Self {
      points,
      color,
      end_color: None,
      width: width.max(0.5),
      end_width: width.max(0.5),
      visible: true,
      closed: false,
      z_order: 0,
      style: TrailStyle::Solid,
      cap: LineCap::Butt,
      join: LineJoin::None,
      lighting: true,
    }
  }

  /// Create a simple two-point line segment.
  pub fn segment(start: Vec2, end: Vec2, color: Color, width: f32) -> Self {
    Self::new(vec![start, end], color, width)
  }

  /// Create a horizontal line.
  pub fn horizontal(y: f32, x_start: f32, x_end: f32, color: Color, width: f32) -> Self {
    Self::segment(Vec2::new(x_start, y), Vec2::new(x_end, y), color, width)
  }

  /// Create a vertical line.
  pub fn vertical(x: f32, y_start: f32, y_end: f32, color: Color, width: f32) -> Self {
    Self::segment(Vec2::new(x, y_start), Vec2::new(x, y_end), color, width)
  }

  /// Create a rectangle outline.
  pub fn rect(x: f32, y: f32, width: f32, height: f32, color: Color, line_width: f32) -> Self {
    Self::new(
      vec![
        Vec2::new(x, y),
        Vec2::new(x + width, y),
        Vec2::new(x + width, y + height),
        Vec2::new(x, y + height),
      ],
      color,
      line_width,
    )
    .with_closed(true)
  }

  /// Create a circle outline.
  pub fn circle(center: Vec2, radius: f32, color: Color, width: f32, segments: u32) -> Self {
    let segments = segments.max(4);
    let mut points = Vec::with_capacity(segments as usize);

    for i in 0..segments {
      let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
      points.push(Vec2::new(
        center.x + angle.cos() * radius,
        center.y + angle.sin() * radius,
      ));
    }

    Self::new(points, color, width).with_closed(true)
  }

  /// Create an arc.
  pub fn arc(
    center: Vec2,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    color: Color,
    width: f32,
    segments: u32,
  ) -> Self {
    let segments = segments.max(2);
    let mut points = Vec::with_capacity(segments as usize + 1);
    let angle_range = end_angle - start_angle;

    for i in 0..=segments {
      let t = i as f32 / segments as f32;
      let angle = start_angle + angle_range * t;
      points.push(Vec2::new(
        center.x + angle.cos() * radius,
        center.y + angle.sin() * radius,
      ));
    }

    Self::new(points, color, width)
  }

  /// Create a quadratic bezier curve.
  pub fn quadratic_bezier(
    start: Vec2,
    control: Vec2,
    end: Vec2,
    color: Color,
    width: f32,
    segments: u32,
  ) -> Self {
    let segments = segments.max(2);
    let mut points = Vec::with_capacity(segments as usize + 1);

    for i in 0..=segments {
      let t = i as f32 / segments as f32;
      let t2 = t * t;
      let mt = 1.0 - t;
      let mt2 = mt * mt;

      // B(t) = (1-t)²P0 + 2(1-t)tP1 + t²P2
      let x = mt2 * start.x + 2.0 * mt * t * control.x + t2 * end.x;
      let y = mt2 * start.y + 2.0 * mt * t * control.y + t2 * end.y;
      points.push(Vec2::new(x, y));
    }

    Self::new(points, color, width)
  }

  /// Create a cubic bezier curve.
  pub fn bezier(
    start: Vec2,
    control1: Vec2,
    control2: Vec2,
    end: Vec2,
    color: Color,
    width: f32,
    segments: u32,
  ) -> Self {
    let segments = segments.max(2);
    let mut points = Vec::with_capacity(segments as usize + 1);

    for i in 0..=segments {
      let t = i as f32 / segments as f32;
      let t2 = t * t;
      let t3 = t2 * t;
      let mt = 1.0 - t;
      let mt2 = mt * mt;
      let mt3 = mt2 * mt;

      // B(t) = (1-t)³P0 + 3(1-t)²tP1 + 3(1-t)t²P2 + t³P3
      let x = mt3 * start.x + 3.0 * mt2 * t * control1.x + 3.0 * mt * t2 * control2.x + t3 * end.x;
      let y = mt3 * start.y + 3.0 * mt2 * t * control1.y + 3.0 * mt * t2 * control2.y + t3 * end.y;
      points.push(Vec2::new(x, y));
    }

    Self::new(points, color, width)
  }

  /// Create an arrow pointing from start to end.
  pub fn arrow(start: Vec2, end: Vec2, color: Color, width: f32, head_size: f32) -> Self {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let len = (dx * dx + dy * dy).sqrt();

    if len < 0.001 {
      return Self::new(vec![start], color, width);
    }

    // Normalize direction
    let nx = dx / len;
    let ny = dy / len;

    // Perpendicular
    let px = -ny;
    let py = nx;

    // Arrow head base (a bit back from the tip)
    let base = Vec2::new(end.x - nx * head_size, end.y - ny * head_size);

    // Arrow head wings
    let wing1 = Vec2::new(base.x + px * head_size * 0.5, base.y + py * head_size * 0.5);
    let wing2 = Vec2::new(base.x - px * head_size * 0.5, base.y - py * head_size * 0.5);

    // Build the arrow as: start -> end, then head
    Self::new(vec![start, end, wing1, end, wing2], color, width)
  }

  /// Set the end color for a gradient effect.
  pub fn with_end_color(mut self, color: Color) -> Self {
    self.end_color = Some(color);
    self
  }

  /// Set the end width for tapering.
  pub fn with_end_width(mut self, width: f32) -> Self {
    self.end_width = width.max(0.0);
    self
  }

  /// Set tapering as a ratio (0.0 = point, 1.0 = no taper).
  pub fn with_taper(mut self, ratio: f32) -> Self {
    self.end_width = self.width * ratio.clamp(0.0, 1.0);
    self
  }

  /// Set whether the line should be closed (connect last to first).
  pub fn with_closed(mut self, closed: bool) -> Self {
    self.closed = closed;
    self
  }

  /// Set the draw order (default 0); lines sort with entities, sprites and
  /// trails by z, and draw last at equal z.
  pub fn with_z_order(mut self, z_order: i32) -> Self {
    self.z_order = z_order;
    self
  }

  /// Get the points.
  pub fn points(&self) -> &[Vec2] {
    &self.points
  }

  /// Get a mutable reference to the points.
  pub fn points_mut(&mut self) -> &mut Vec<Vec2> {
    &mut self.points
  }

  /// Set the points.
  pub fn set_points(&mut self, points: Vec<Vec2>) {
    self.points = points;
  }

  /// Add a point to the end of the line.
  pub fn push_point(&mut self, point: Vec2) {
    self.points.push(point);
  }

  /// Remove the last point.
  pub fn pop_point(&mut self) -> Option<Vec2> {
    self.points.pop()
  }

  /// Clear all points.
  pub fn clear(&mut self) {
    self.points.clear();
  }

  /// Get the color.
  pub fn color(&self) -> Color {
    self.color
  }

  /// Set the color.
  pub fn set_color(&mut self, color: Color) {
    self.color = color;
  }

  /// Get the width.
  pub fn width(&self) -> f32 {
    self.width
  }

  /// Set the width.
  pub fn set_width(&mut self, width: f32) {
    self.width = width.max(0.5);
  }

  /// Get the end width.
  pub fn end_width(&self) -> f32 {
    self.end_width
  }

  /// Check if the line is visible.
  pub fn is_visible(&self) -> bool {
    self.visible
  }

  /// Set visibility.
  pub fn set_visible(&mut self, visible: bool) {
    self.visible = visible;
  }

  /// Check if the line is closed.
  pub fn is_closed(&self) -> bool {
    self.closed
  }

  /// Get the z-order.
  pub fn z_order(&self) -> i32 {
    self.z_order
  }

  /// Set the z-order.
  pub fn set_z_order(&mut self, z_order: i32) {
    self.z_order = z_order;
  }

  /// Set the rendering style for this line.
  pub fn with_style(mut self, style: TrailStyle) -> Self {
    self.style = style;
    self
  }

  /// Set the line cap style for endpoints.
  pub fn with_cap(mut self, cap: LineCap) -> Self {
    self.cap = cap;
    self
  }

  /// Set the line join style for corners.
  pub fn with_join(mut self, join: LineJoin) -> Self {
    self.join = join;
    self
  }

  /// Get the rendering style.
  pub fn style(&self) -> TrailStyle {
    self.style
  }

  /// Set the rendering style.
  pub fn set_style(&mut self, style: TrailStyle) {
    self.style = style;
  }

  /// Get the line cap style.
  pub fn cap(&self) -> LineCap {
    self.cap
  }

  /// Set the line cap style.
  pub fn set_cap(&mut self, cap: LineCap) {
    self.cap = cap;
  }

  /// Get the line join style.
  pub fn join(&self) -> LineJoin {
    self.join
  }

  /// Set whether scene lighting applies (default `true`).
  ///
  /// With `false` the line is drawn at full brightness on top of the lit scene
  /// (before post-processing), e.g. for glowing lasers in the dark. Without
  /// lighting the setting has no effect.
  pub fn with_lighting(mut self, enabled: bool) -> Self {
    self.lighting = enabled;
    self
  }

  /// Whether scene lighting applies to this line.
  pub fn lighting(&self) -> bool {
    self.lighting
  }

  /// Set whether scene lighting applies to this line.
  pub fn set_lighting(&mut self, enabled: bool) {
    self.lighting = enabled;
  }

  /// Set the line join style.
  pub fn set_join(&mut self, join: LineJoin) {
    self.join = join;
  }

  /// Get the number of points.
  pub fn point_count(&self) -> usize {
    self.points.len()
  }

  /// Get interpolated color at position t (0.0 = start, 1.0 = end).
  pub fn color_at(&self, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let end = self.end_color.unwrap_or(self.color);

    Color::new(
      lerp(self.color.r as f32, end.r as f32, t) as u8,
      lerp(self.color.g as f32, end.g as f32, t) as u8,
      lerp(self.color.b as f32, end.b as f32, t) as u8,
      lerp(self.color.a as f32, end.a as f32, t) as u8,
    )
  }

  /// Get interpolated width at position t (0.0 = start, 1.0 = end).
  pub fn width_at(&self, t: f32) -> f32 {
    lerp(self.width, self.end_width, t.clamp(0.0, 1.0))
  }

  /// Calculate the total length of the line.
  pub fn length(&self) -> f32 {
    if self.points.len() < 2 {
      return 0.0;
    }

    let mut total = 0.0;
    for i in 1..self.points.len() {
      let dx = self.points[i].x - self.points[i - 1].x;
      let dy = self.points[i].y - self.points[i - 1].y;
      total += (dx * dx + dy * dy).sqrt();
    }

    if self.closed && self.points.len() > 2 {
      let first = self.points.first().unwrap();
      let last = self.points.last().unwrap();
      let dx = first.x - last.x;
      let dy = first.y - last.y;
      total += (dx * dx + dy * dy).sqrt();
    }

    total
  }

  /// Get a point along the line by distance.
  ///
  /// Returns the point at the given distance from the start, or None
  /// if the distance exceeds the line length.
  pub fn point_at_distance(&self, distance: f32) -> Option<Vec2> {
    if self.points.len() < 2 || distance < 0.0 {
      return self.points.first().copied();
    }

    let mut remaining = distance;

    for i in 1..self.points.len() {
      let p0 = self.points[i - 1];
      let p1 = self.points[i];
      let dx = p1.x - p0.x;
      let dy = p1.y - p0.y;
      let seg_len = (dx * dx + dy * dy).sqrt();

      if remaining <= seg_len {
        let t = remaining / seg_len;
        return Some(Vec2::new(p0.x + dx * t, p0.y + dy * t));
      }

      remaining -= seg_len;
    }

    self.points.last().copied()
  }
}

impl Default for Line2D {
  fn default() -> Self {
    Self::new(Vec::new(), Color::WHITE, 2.0)
  }
}

/// Linear interpolation helper
fn lerp(a: f32, b: f32, t: f32) -> f32 {
  a + (b - a) * t
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn line_creation() {
    let line = Line2D::segment(
      Vec2::new(0.0, 0.0),
      Vec2::new(100.0, 0.0),
      Color::WHITE,
      2.0,
    );
    assert_eq!(line.point_count(), 2);
    assert!((line.length() - 100.0).abs() < 0.01);
  }

  #[test]
  fn circle_creation() {
    let circle = Line2D::circle(Vec2::new(0.0, 0.0), 50.0, Color::RED, 2.0, 16);
    assert_eq!(circle.point_count(), 16);
    assert!(circle.is_closed());
  }

  #[test]
  fn bezier_creation() {
    let curve = Line2D::bezier(
      Vec2::new(0.0, 0.0),
      Vec2::new(25.0, -50.0),
      Vec2::new(75.0, -50.0),
      Vec2::new(100.0, 0.0),
      Color::BLUE,
      2.0,
      10,
    );
    assert_eq!(curve.point_count(), 11); // segments + 1
  }

  #[test]
  fn line_manipulation() {
    let mut line = Line2D::new(vec![Vec2::new(0.0, 0.0)], Color::WHITE, 2.0);

    line.push_point(Vec2::new(10.0, 0.0));
    assert_eq!(line.point_count(), 2);

    line.pop_point();
    assert_eq!(line.point_count(), 1);

    line.clear();
    assert_eq!(line.point_count(), 0);
  }

  #[test]
  fn color_interpolation() {
    let line = Line2D::new(vec![], Color::WHITE, 2.0).with_end_color(Color::BLACK);

    let mid = line.color_at(0.5);
    assert!((mid.r as i32 - 127).abs() <= 1);
  }

  #[test]
  fn width_interpolation() {
    let line = Line2D::new(vec![], Color::WHITE, 10.0).with_end_width(2.0);

    assert!((line.width_at(0.0) - 10.0).abs() < 0.01);
    assert!((line.width_at(1.0) - 2.0).abs() < 0.01);
    assert!((line.width_at(0.5) - 6.0).abs() < 0.01);
  }

  #[test]
  fn point_at_distance() {
    let line = Line2D::segment(
      Vec2::new(0.0, 0.0),
      Vec2::new(100.0, 0.0),
      Color::WHITE,
      2.0,
    );

    let p = line.point_at_distance(50.0).unwrap();
    assert!((p.x - 50.0).abs() < 0.01);
    assert!((p.y - 0.0).abs() < 0.01);
  }

  #[test]
  fn rect_creation() {
    let rect = Line2D::rect(0.0, 0.0, 100.0, 50.0, Color::GREEN, 2.0);
    assert_eq!(rect.point_count(), 4);
    assert!(rect.is_closed());
  }

  // ========== Line2D Style/Cap/Join Tests ==========

  #[test]
  fn line_default_style_is_solid() {
    let line = Line2D::default();
    assert_eq!(line.style(), TrailStyle::Solid);
  }

  #[test]
  fn line_with_style() {
    let line = Line2D::default().with_style(TrailStyle::Glow);
    assert_eq!(line.style(), TrailStyle::Glow);
  }

  #[test]
  fn line_set_style() {
    let mut line = Line2D::default();
    line.set_style(TrailStyle::Soft);
    assert_eq!(line.style(), TrailStyle::Soft);
  }

  #[test]
  fn line_default_cap_is_butt() {
    let line = Line2D::default();
    assert_eq!(line.cap(), LineCap::Butt);
  }

  #[test]
  fn line_with_cap() {
    let line = Line2D::default().with_cap(LineCap::Round);
    assert_eq!(line.cap(), LineCap::Round);
  }

  #[test]
  fn line_default_join_is_none() {
    let line = Line2D::default();
    assert_eq!(line.join(), LineJoin::None);
  }

  #[test]
  fn line_with_join() {
    let line = Line2D::default().with_join(LineJoin::Miter);
    assert_eq!(line.join(), LineJoin::Miter);
  }
}
