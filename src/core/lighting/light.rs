//! 2D Light components
//!
//! Light sources that can be attached to entities to illuminate the scene.
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Create a point light
//! let torch = world.spawn((
//!     Transform::at(100.0, 100.0),
//!     Light2D::point(Color::ORANGE, 150.0),
//! ));
//!
//! // Create a spotlight
//! let flashlight = world.spawn((
//!     Transform::at(200.0, 200.0),
//!     Light2D::spot(Color::WHITE, 200.0, 45.0, 0.0), // 45 degree cone, pointing right
//! ));
//! ```

use crate::core::ui::Color;

/// A 2D light source component.
///
/// Attach this to any entity with a `Transform` to add dynamic lighting.
/// Lights are rendered additively to a light accumulation buffer, which
/// is then multiplied with the scene.
#[derive(Debug, Clone, PartialEq)]
pub enum Light2D {
  /// A point light that emits light in all directions
  Point {
    /// Light color
    color: Color,
    /// Maximum radius of the light in pixels
    radius: f32,
    /// Light intensity (0.0 - 1.0+, can be > 1.0 for HDR-like effects)
    intensity: f32,
    /// Falloff curve (1.0 = linear, 2.0 = quadratic, 0.5 = sqrt)
    falloff: f32,
    /// Whether this light casts shadows
    cast_shadows: bool,
  },

  /// A spotlight that emits light in a cone
  Spot {
    /// Light color
    color: Color,
    /// Maximum radius of the light in pixels
    radius: f32,
    /// Light intensity (0.0 - 1.0+)
    intensity: f32,
    /// Falloff curve
    falloff: f32,
    /// Cone angle in degrees (full angle, not half-angle)
    angle: f32,
    /// Direction the spotlight is pointing (in degrees, 0 = right, 90 = down)
    direction: f32,
    /// Softness of the cone edge (0.0 = hard, 1.0 = very soft)
    softness: f32,
    /// Whether this light casts shadows
    cast_shadows: bool,
  },
}

impl Light2D {
  /// Create a point light with default settings
  ///
  /// # Arguments
  /// * `color` - The light color
  /// * `radius` - The maximum radius in pixels
  pub fn point(color: Color, radius: f32) -> Self {
    Self::Point {
      color,
      radius: radius.max(1.0),
      intensity: 1.0,
      falloff: 2.0, // Quadratic falloff is most natural
      cast_shadows: false,
    }
  }

  /// Create a point light with custom intensity
  pub fn point_with_intensity(color: Color, radius: f32, intensity: f32) -> Self {
    Self::Point {
      color,
      radius: radius.max(1.0),
      intensity: intensity.max(0.0),
      falloff: 2.0,
      cast_shadows: false,
    }
  }

  /// Create a point light with all parameters
  pub fn point_full(
    color: Color,
    radius: f32,
    intensity: f32,
    falloff: f32,
    cast_shadows: bool,
  ) -> Self {
    Self::Point {
      color,
      radius: radius.max(1.0),
      intensity: intensity.max(0.0),
      falloff: falloff.clamp(0.1, 10.0),
      cast_shadows,
    }
  }

  /// Create a spotlight with default settings
  ///
  /// # Arguments
  /// * `color` - The light color
  /// * `radius` - The maximum radius in pixels
  /// * `angle` - The cone angle in degrees (full cone width)
  /// * `direction` - The direction in degrees (0 = right, 90 = down)
  pub fn spot(color: Color, radius: f32, angle: f32, direction: f32) -> Self {
    Self::Spot {
      color,
      radius: radius.max(1.0),
      intensity: 1.0,
      falloff: 2.0,
      angle: angle.clamp(1.0, 360.0),
      direction,
      softness: 0.1,
      cast_shadows: false,
    }
  }

  /// Create a spotlight with custom intensity
  pub fn spot_with_intensity(
    color: Color,
    radius: f32,
    angle: f32,
    direction: f32,
    intensity: f32,
  ) -> Self {
    Self::Spot {
      color,
      radius: radius.max(1.0),
      intensity: intensity.max(0.0),
      falloff: 2.0,
      angle: angle.clamp(1.0, 360.0),
      direction,
      softness: 0.1,
      cast_shadows: false,
    }
  }

  /// Create a spotlight with all parameters
  #[allow(clippy::too_many_arguments)]
  pub fn spot_full(
    color: Color,
    radius: f32,
    intensity: f32,
    falloff: f32,
    angle: f32,
    direction: f32,
    softness: f32,
    cast_shadows: bool,
  ) -> Self {
    Self::Spot {
      color,
      radius: radius.max(1.0),
      intensity: intensity.max(0.0),
      falloff: falloff.clamp(0.1, 10.0),
      angle: angle.clamp(1.0, 360.0),
      direction,
      softness: softness.clamp(0.0, 1.0),
      cast_shadows,
    }
  }

  /// Get the color of this light
  pub fn color(&self) -> Color {
    match self {
      Self::Point { color, .. } | Self::Spot { color, .. } => *color,
    }
  }

  /// Set the color of this light
  pub fn set_color(&mut self, new_color: Color) {
    match self {
      Self::Point { color, .. } | Self::Spot { color, .. } => *color = new_color,
    }
  }

  /// Get the radius of this light
  pub fn radius(&self) -> f32 {
    match self {
      Self::Point { radius, .. } | Self::Spot { radius, .. } => *radius,
    }
  }

  /// Set the radius of this light
  pub fn set_radius(&mut self, new_radius: f32) {
    let r = new_radius.max(1.0);
    match self {
      Self::Point { radius, .. } | Self::Spot { radius, .. } => *radius = r,
    }
  }

  /// Get the intensity of this light
  pub fn intensity(&self) -> f32 {
    match self {
      Self::Point { intensity, .. } | Self::Spot { intensity, .. } => *intensity,
    }
  }

  /// Set the intensity of this light
  pub fn set_intensity(&mut self, new_intensity: f32) {
    let i = new_intensity.max(0.0);
    match self {
      Self::Point { intensity, .. } | Self::Spot { intensity, .. } => *intensity = i,
    }
  }

  /// Check if this light casts shadows
  pub fn casts_shadows(&self) -> bool {
    match self {
      Self::Point { cast_shadows, .. } | Self::Spot { cast_shadows, .. } => *cast_shadows,
    }
  }

  /// Enable or disable shadow casting
  pub fn set_cast_shadows(&mut self, cast: bool) {
    match self {
      Self::Point { cast_shadows, .. } | Self::Spot { cast_shadows, .. } => *cast_shadows = cast,
    }
  }

  /// Check if this is a point light
  pub fn is_point(&self) -> bool {
    matches!(self, Self::Point { .. })
  }

  /// Check if this is a spotlight
  pub fn is_spot(&self) -> bool {
    matches!(self, Self::Spot { .. })
  }

  /// Get the direction (only meaningful for spotlights, returns 0.0 for point lights)
  pub fn direction(&self) -> f32 {
    match self {
      Self::Point { .. } => 0.0,
      Self::Spot { direction, .. } => *direction,
    }
  }

  /// Set the direction (only affects spotlights)
  pub fn set_direction(&mut self, new_direction: f32) {
    if let Self::Spot { direction, .. } = self {
      *direction = new_direction;
    }
  }

  /// Get the cone angle (only meaningful for spotlights, returns 360.0 for point lights)
  pub fn angle(&self) -> f32 {
    match self {
      Self::Point { .. } => 360.0,
      Self::Spot { angle, .. } => *angle,
    }
  }

  /// Builder method to enable shadow casting
  pub fn with_shadows(mut self) -> Self {
    self.set_cast_shadows(true);
    self
  }

  /// Builder method to set intensity
  pub fn with_intensity(mut self, intensity: f32) -> Self {
    self.set_intensity(intensity);
    self
  }

  /// Builder method to set falloff
  pub fn with_falloff(mut self, falloff: f32) -> Self {
    let f = falloff.clamp(0.1, 10.0);
    match &mut self {
      Self::Point {
        falloff: ref mut fo,
        ..
      }
      | Self::Spot {
        falloff: ref mut fo,
        ..
      } => *fo = f,
    }
    self
  }
}

/// Convenient light presets
impl Light2D {
  /// A warm torch/candle light
  pub fn torch(radius: f32) -> Self {
    Self::point(Color::new(255, 200, 100, 255), radius).with_intensity(0.8)
  }

  /// A cool moonlight
  pub fn moonlight(radius: f32) -> Self {
    Self::point(Color::new(200, 220, 255, 255), radius).with_intensity(0.4)
  }

  /// A bright white light (like a lamp)
  pub fn lamp(radius: f32) -> Self {
    Self::point(Color::WHITE, radius).with_intensity(1.0)
  }

  /// A flashlight spotlight
  pub fn flashlight(radius: f32, direction: f32) -> Self {
    Self::spot(Color::WHITE, radius, 30.0, direction).with_intensity(1.2)
  }

  /// A red warning/danger light
  pub fn warning(radius: f32) -> Self {
    Self::point(Color::RED, radius).with_intensity(0.9)
  }

  /// A magical/mystical light
  pub fn magic(radius: f32, color: Color) -> Self {
    Self::point(color, radius)
      .with_intensity(1.2)
      .with_falloff(1.5)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn point_light_creation() {
    let light = Light2D::point(Color::WHITE, 100.0);
    assert!(light.is_point());
    assert!(!light.is_spot());
    assert_eq!(light.radius(), 100.0);
    assert_eq!(light.intensity(), 1.0);
    assert!(!light.casts_shadows());
  }

  #[test]
  fn spot_light_creation() {
    let light = Light2D::spot(Color::YELLOW, 150.0, 45.0, 90.0);
    assert!(!light.is_point());
    assert!(light.is_spot());
    assert_eq!(light.radius(), 150.0);
    assert_eq!(light.angle(), 45.0);
    assert_eq!(light.direction(), 90.0);
  }

  #[test]
  fn radius_clamped_to_minimum() {
    let light = Light2D::point(Color::WHITE, -10.0);
    assert_eq!(light.radius(), 1.0);
  }

  #[test]
  fn intensity_clamped_to_minimum() {
    let light = Light2D::point_with_intensity(Color::WHITE, 100.0, -5.0);
    assert_eq!(light.intensity(), 0.0);
  }

  #[test]
  fn builder_methods() {
    let light = Light2D::point(Color::WHITE, 100.0)
      .with_shadows()
      .with_intensity(0.5)
      .with_falloff(1.5);

    assert!(light.casts_shadows());
    assert_eq!(light.intensity(), 0.5);
  }

  #[test]
  fn presets() {
    let torch = Light2D::torch(50.0);
    assert!(torch.is_point());

    let flashlight = Light2D::flashlight(100.0, 45.0);
    assert!(flashlight.is_spot());
    assert_eq!(flashlight.direction(), 45.0);
  }

  #[test]
  fn setters() {
    let mut light = Light2D::point(Color::WHITE, 100.0);
    light.set_color(Color::RED);
    light.set_radius(200.0);
    light.set_intensity(0.5);
    light.set_cast_shadows(true);

    assert_eq!(light.color(), Color::RED);
    assert_eq!(light.radius(), 200.0);
    assert_eq!(light.intensity(), 0.5);
    assert!(light.casts_shadows());
  }
}
