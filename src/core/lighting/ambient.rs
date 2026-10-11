//! Ambient lighting configuration
//!
//! Ambient light provides a base level of illumination to the entire scene,
//! preventing areas without lights from being completely black.
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // In your game setup
//! ctx.lighting.set_ambient(AmbientLight::new(Color::new(30, 30, 50, 255), 0.2));
//!
//! // For a completely dark scene (only light sources visible)
//! ctx.lighting.set_ambient(AmbientLight::dark());
//!
//! // For a moonlit night
//! ctx.lighting.set_ambient(AmbientLight::moonlit());
//! ```

use crate::core::ui::Color;

/// Global ambient light settings.
///
/// Ambient light is applied uniformly to the entire scene before
/// dynamic lights are added. This prevents unlit areas from being
/// completely black.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmbientLight {
  /// The ambient light color
  pub color: Color,
  /// The ambient light intensity (0.0 = no ambient, 1.0 = full brightness)
  pub intensity: f32,
}

impl AmbientLight {
  /// Create a new ambient light with a specific color and intensity.
  pub fn new(color: Color, intensity: f32) -> Self {
    Self {
      color,
      intensity: intensity.clamp(0.0, 1.0),
    }
  }

  /// Create an ambient light with just intensity (white color).
  pub fn with_intensity(intensity: f32) -> Self {
    Self::new(Color::WHITE, intensity)
  }

  /// A completely dark ambient (no base illumination).
  ///
  /// Use this for horror games or scenes where only light sources
  /// should be visible.
  pub fn dark() -> Self {
    Self::new(Color::BLACK, 0.0)
  }

  /// A dim ambient suitable for night scenes.
  pub fn night() -> Self {
    Self::new(Color::new(20, 20, 40, 255), 0.1)
  }

  /// A moonlit ambient (slightly blue-tinted).
  pub fn moonlit() -> Self {
    Self::new(Color::new(100, 110, 140, 255), 0.15)
  }

  /// A dim indoor ambient.
  pub fn indoor() -> Self {
    Self::new(Color::new(60, 55, 50, 255), 0.2)
  }

  /// A daylight ambient (nearly full brightness).
  pub fn daylight() -> Self {
    Self::new(Color::WHITE, 0.8)
  }

  /// A sunset ambient (warm orange tint).
  pub fn sunset() -> Self {
    Self::new(Color::new(255, 180, 120, 255), 0.5)
  }

  /// Get the effective color with intensity applied.
  ///
  /// Returns normalized RGB values (0.0 - 1.0) with intensity multiplied in.
  pub fn effective_color(&self) -> [f32; 3] {
    [
      (self.color.r as f32 / 255.0) * self.intensity,
      (self.color.g as f32 / 255.0) * self.intensity,
      (self.color.b as f32 / 255.0) * self.intensity,
    ]
  }

  /// Set the intensity.
  pub fn set_intensity(&mut self, intensity: f32) {
    self.intensity = intensity.clamp(0.0, 1.0);
  }

  /// Set the color.
  pub fn set_color(&mut self, color: Color) {
    self.color = color;
  }

  /// Builder method to set intensity.
  pub fn with_custom_intensity(mut self, intensity: f32) -> Self {
    self.set_intensity(intensity);
    self
  }
}

impl Default for AmbientLight {
  /// Default ambient is a dim gray, suitable for most games.
  fn default() -> Self {
    Self::new(Color::new(50, 50, 55, 255), 0.15)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_ambient() {
    let ambient = AmbientLight::default();
    assert!(ambient.intensity > 0.0);
  }

  #[test]
  fn dark_ambient() {
    let ambient = AmbientLight::dark();
    assert_eq!(ambient.intensity, 0.0);
  }

  #[test]
  fn intensity_clamped() {
    let high = AmbientLight::new(Color::WHITE, 2.0);
    assert_eq!(high.intensity, 1.0);

    let low = AmbientLight::new(Color::WHITE, -0.5);
    assert_eq!(low.intensity, 0.0);
  }

  #[test]
  fn effective_color() {
    let ambient = AmbientLight::new(Color::WHITE, 0.5);
    let eff = ambient.effective_color();
    assert!((eff[0] - 0.5).abs() < 0.001);
    assert!((eff[1] - 0.5).abs() < 0.001);
    assert!((eff[2] - 0.5).abs() < 0.001);
  }

  #[test]
  fn presets() {
    let _ = AmbientLight::night();
    let _ = AmbientLight::moonlit();
    let _ = AmbientLight::indoor();
    let _ = AmbientLight::daylight();
    let _ = AmbientLight::sunset();
  }
}
