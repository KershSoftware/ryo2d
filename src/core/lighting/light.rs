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
//!     Transform::from_xy(100.0, 100.0),
//!     Light2D::point(Color::ORANGE, 150.0),
//! )).id();
//!
//! // Create a spotlight
//! let flashlight = world.spawn((
//!     Transform::from_xy(200.0, 200.0),
//!     Light2D::spot(Color::WHITE, 200.0, 45.0, 0.0), // 45 degree cone, pointing right
//! )).id();
//!
//! // Create a spotlight with volumetric beam
//! let volumetric_flashlight = Light2D::flashlight(200.0, 0.0)
//!     .with_volumetric();  // Visible beam in dusty air
//!
//! // Create a light that contributes to god rays
//! let sun_light = Light2D::point(Color::WHITE, 500.0)
//!     .with_god_rays();
//! ```

use super::VolumetricSettings;
use crate::core::ecs::Component;
use crate::core::ui::Color;

/// A 2D light source component.
///
/// Attach this to any entity with a `Transform` to add dynamic lighting.
/// Lights are rendered additively to a light accumulation buffer, which
/// is then multiplied with the scene. A light with `RenderLayers` lights
/// only the cameras on those layers (by default, layer 0).
#[derive(Component, Debug, Clone, PartialEq)]
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
    /// How much light shadows let through away from the light (0.0 = hard;
    /// see [`Light2D::with_soft_shadows`])
    shadow_softness: f32,
    /// Shadow color (default black, can be tinted for colored shadows)
    shadow_color: Color,
    /// Volumetric lighting settings (visible beams, god rays)
    volumetric: Option<VolumetricSettings>,
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
    /// How much light shadows let through away from the light (0.0 = hard;
    /// see [`Light2D::with_soft_shadows`])
    shadow_softness: f32,
    /// Shadow color (default black, can be tinted for colored shadows)
    shadow_color: Color,
    /// Volumetric lighting settings (visible beams, god rays)
    volumetric: Option<VolumetricSettings>,
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
      shadow_softness: 0.0,
      shadow_color: Color::BLACK,
      volumetric: None,
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
      shadow_softness: 0.0,
      shadow_color: Color::BLACK,
      volumetric: None,
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
      shadow_softness: 0.0,
      shadow_color: Color::BLACK,
      volumetric: None,
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
      shadow_softness: 0.0,
      shadow_color: Color::BLACK,
      volumetric: None,
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
      shadow_softness: 0.0,
      shadow_color: Color::BLACK,
      volumetric: None,
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
      shadow_softness: 0.0,
      shadow_color: Color::BLACK,
      volumetric: None,
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

  /// Builder method to enable shadows that fade with distance from the light
  ///
  /// A shadow starts whole at the light and fades over the first
  /// `softness / 2` of the light's radius, beyond which it lets `softness`
  /// of the light through: 0.5 keeps half the light past a quarter of the
  /// radius. The shadow's outline stays sharp.
  ///
  /// # Arguments
  /// * `softness` - How much light the shadow lets through (0.0 = hard, 1.0 = fades out)
  pub fn with_soft_shadows(mut self, softness: f32) -> Self {
    self.set_cast_shadows(true);
    self.set_shadow_softness(softness);
    self
  }

  /// Get the shadow softness of this light
  pub fn shadow_softness(&self) -> f32 {
    match self {
      Self::Point {
        shadow_softness, ..
      }
      | Self::Spot {
        shadow_softness, ..
      } => *shadow_softness,
    }
  }

  /// Set how much light the light's shadows let through away from the light
  /// (0.0 = hard, 1.0 = fades out; see [`Light2D::with_soft_shadows`])
  pub fn set_shadow_softness(&mut self, softness: f32) {
    let s = softness.clamp(0.0, 1.0);
    match self {
      Self::Point {
        shadow_softness, ..
      }
      | Self::Spot {
        shadow_softness, ..
      } => *shadow_softness = s,
    }
  }

  /// Builder method to set colored shadows
  ///
  /// Colored shadows can create atmospheric effects like stained glass,
  /// sunset lighting, or artistic stylization.
  ///
  /// # Arguments
  /// * `color` - The shadow tint color (alpha controls shadow opacity)
  pub fn with_shadow_color(mut self, color: Color) -> Self {
    self.set_shadow_color(color);
    self
  }

  /// Get the shadow color of this light
  pub fn shadow_color(&self) -> Color {
    match self {
      Self::Point { shadow_color, .. } | Self::Spot { shadow_color, .. } => *shadow_color,
    }
  }

  /// Set the shadow color of this light
  pub fn set_shadow_color(&mut self, color: Color) {
    match self {
      Self::Point {
        shadow_color: sc, ..
      }
      | Self::Spot {
        shadow_color: sc, ..
      } => *sc = color,
    }
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
      Self::Point { falloff: fo, .. } | Self::Spot { falloff: fo, .. } => *fo = f,
    }
    self
  }

  /// Builder method to enable volumetric effects with default settings
  ///
  /// Creates visible light beams in dusty/foggy atmospheres.
  /// For spotlights, this creates a visible cone. For point lights,
  /// this creates a subtle glow effect.
  pub fn with_volumetric(mut self) -> Self {
    let settings = match &self {
      Self::Spot { .. } => VolumetricSettings::flashlight(),
      Self::Point { .. } => VolumetricSettings::subtle_beam(),
    };
    self.set_volumetric(Some(settings));
    self
  }

  /// Builder method to enable god rays contribution
  ///
  /// This light will contribute to the screen-space god rays effect.
  /// Works best with bright lights positioned at the edge of the screen
  /// or partially occluded by objects.
  pub fn with_god_rays(mut self) -> Self {
    let settings = match self.volumetric() {
      Some(existing) => (*existing).with_god_rays(),
      None => VolumetricSettings::god_rays_only(),
    };
    self.set_volumetric(Some(settings));
    self
  }

  /// Builder method to enable both volumetric beam and god rays
  pub fn with_volumetric_and_god_rays(mut self) -> Self {
    let settings = match &self {
      Self::Spot { .. } => VolumetricSettings::flashlight().with_god_rays(),
      Self::Point { .. } => VolumetricSettings::subtle_beam().with_god_rays(),
    };
    self.set_volumetric(Some(settings));
    self
  }

  /// Builder method to set custom volumetric settings
  pub fn with_volumetric_settings(mut self, settings: VolumetricSettings) -> Self {
    self.set_volumetric(Some(settings));
    self
  }

  /// Get the volumetric settings for this light
  pub fn volumetric(&self) -> Option<&VolumetricSettings> {
    match self {
      Self::Point { volumetric, .. } | Self::Spot { volumetric, .. } => volumetric.as_ref(),
    }
  }

  /// Set the volumetric settings for this light
  pub fn set_volumetric(&mut self, settings: Option<VolumetricSettings>) {
    match self {
      Self::Point { volumetric, .. } | Self::Spot { volumetric, .. } => *volumetric = settings,
    }
  }

  /// Check if this light has volumetric effects enabled
  pub fn has_volumetric(&self) -> bool {
    self
      .volumetric()
      .is_some_and(|v| v.enabled && (v.intensity > 0.0 || v.god_rays))
  }

  /// Check if this light contributes to god rays
  pub fn contributes_to_god_rays(&self) -> bool {
    self.volumetric().is_some_and(|v| v.enabled && v.god_rays)
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
    assert!(light.volumetric().is_none());
  }

  #[test]
  fn spot_light_creation() {
    let light = Light2D::spot(Color::YELLOW, 150.0, 45.0, 90.0);
    assert!(!light.is_point());
    assert!(light.is_spot());
    assert_eq!(light.radius(), 150.0);
    assert_eq!(light.angle(), 45.0);
    assert_eq!(light.direction(), 90.0);
    assert!(light.volumetric().is_none());
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

  #[test]
  fn volumetric_builder() {
    let light = Light2D::flashlight(200.0, 0.0).with_volumetric();
    assert!(light.has_volumetric());
    assert!(!light.contributes_to_god_rays());

    let settings = light.volumetric().unwrap();
    assert!(settings.enabled);
    assert!(settings.intensity > 0.0);
  }

  #[test]
  fn god_rays_builder() {
    let light = Light2D::point(Color::WHITE, 500.0).with_god_rays();
    assert!(light.has_volumetric());
    assert!(light.contributes_to_god_rays());
  }

  #[test]
  fn volumetric_and_god_rays_builder() {
    let light = Light2D::flashlight(200.0, 90.0).with_volumetric_and_god_rays();
    assert!(light.has_volumetric());
    assert!(light.contributes_to_god_rays());

    let settings = light.volumetric().unwrap();
    assert!(settings.intensity > 0.0);
    assert!(settings.god_rays);
  }

  #[test]
  fn custom_volumetric_settings() {
    let settings = VolumetricSettings::new(0.8, 0.5, 0.3);
    let light = Light2D::spot(Color::WHITE, 300.0, 45.0, 90.0).with_volumetric_settings(settings);

    let vs = light.volumetric().unwrap();
    assert!((vs.intensity - 0.8).abs() < 0.001);
    assert!((vs.density - 0.5).abs() < 0.001);
  }
}
