//! Global atmosphere settings for volumetric lighting
//!
//! Controls global fog density, scattering, and god rays parameters.
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! fn on_enter(&mut self, ctx: &mut SceneContext) {
//!     // Use a preset
//!     ctx.lighting().set_atmosphere(Atmosphere::dusty());
//!
//!     // Or configure manually
//!     ctx.lighting().set_atmosphere(Atmosphere {
//!         density: 0.3,
//!         color: Color::new(200, 210, 220, 255),
//!         scattering: 0.2,
//!         god_rays_enabled: true,
//!         god_rays_samples: 16,
//!         god_rays_decay: 0.96,
//!         god_rays_intensity: 0.8,
//!         god_rays_threshold: 1.0,
//!     });
//! }
//! ```

use crate::core::ui::Color;

/// Global atmosphere settings for volumetric effects.
///
/// These settings control:
/// - Global fog/dust density that affects all lights
/// - Light scattering in the atmosphere
/// - God rays post-processing effect parameters
#[derive(Debug, Clone, PartialEq)]
pub struct Atmosphere {
  /// Global fog/dust density (0.0 = clear, 1.0 = very dusty)
  pub density: f32,
  /// Fog tint color
  pub color: Color,
  /// Global light scattering coefficient
  pub scattering: f32,
  /// Whether god rays post-effect is enabled
  pub god_rays_enabled: bool,
  /// Number of samples for god rays (8-32 typical)
  /// More samples = higher quality but more expensive
  pub god_rays_samples: u32,
  /// Ray decay rate (0.9 - 0.99)
  /// Higher values = rays extend further
  pub god_rays_decay: f32,
  /// God rays intensity multiplier
  pub god_rays_intensity: f32,
  /// Brightness threshold for god rays: luminance in linear light, 0 or more; 1.0 is white, so only light brighter than white passes
  /// Only light brighter than this contributes to rays
  pub god_rays_threshold: f32,
}

impl Default for Atmosphere {
  fn default() -> Self {
    Self {
      density: 0.0,
      color: Color::new(200, 200, 210, 255),
      scattering: 0.1,
      god_rays_enabled: false,
      god_rays_samples: 16,
      god_rays_decay: 0.96,
      god_rays_intensity: 0.8,
      god_rays_threshold: 1.0,
    }
  }
}

impl Atmosphere {
  /// Create a new atmosphere with custom settings
  pub fn new(density: f32, color: Color) -> Self {
    Self {
      density: density.clamp(0.0, 1.0),
      color,
      ..Default::default()
    }
  }

  /// Clear atmosphere (no fog, no god rays)
  pub fn clear() -> Self {
    Self::default()
  }

  /// Light dust/haze in the air
  pub fn hazy() -> Self {
    Self {
      density: 0.15,
      color: Color::new(220, 220, 230, 255),
      scattering: 0.15,
      god_rays_enabled: false,
      god_rays_samples: 16,
      god_rays_decay: 0.95,
      god_rays_intensity: 0.5,
      god_rays_threshold: 1.2,
    }
  }

  /// Dusty room atmosphere (visible light beams)
  pub fn dusty() -> Self {
    Self {
      density: 0.35,
      color: Color::new(200, 195, 180, 255),
      scattering: 0.25,
      god_rays_enabled: true,
      god_rays_samples: 16,
      god_rays_decay: 0.96,
      god_rays_intensity: 0.7,
      god_rays_threshold: 0.8,
    }
  }

  /// Heavy fog
  pub fn foggy() -> Self {
    Self {
      density: 0.6,
      color: Color::new(180, 185, 195, 255),
      scattering: 0.4,
      god_rays_enabled: true,
      god_rays_samples: 24,
      god_rays_decay: 0.97,
      god_rays_intensity: 0.9,
      god_rays_threshold: 0.6,
    }
  }

  /// Smoky atmosphere (fire, explosions)
  pub fn smoky() -> Self {
    Self {
      density: 0.5,
      color: Color::new(150, 140, 130, 255),
      scattering: 0.35,
      god_rays_enabled: true,
      god_rays_samples: 20,
      god_rays_decay: 0.95,
      god_rays_intensity: 0.8,
      god_rays_threshold: 0.7,
    }
  }

  /// Underwater atmosphere
  pub fn underwater() -> Self {
    Self {
      density: 0.4,
      color: Color::new(100, 150, 180, 255),
      scattering: 0.5,
      god_rays_enabled: true,
      god_rays_samples: 20,
      god_rays_decay: 0.94,
      god_rays_intensity: 0.6,
      god_rays_threshold: 0.9,
    }
  }

  /// Enable god rays with default settings
  pub fn with_god_rays(mut self) -> Self {
    self.god_rays_enabled = true;
    self
  }

  /// Configure god rays parameters
  pub fn with_god_rays_config(
    mut self,
    intensity: f32,
    samples: u32,
    decay: f32,
    threshold: f32,
  ) -> Self {
    self.god_rays_enabled = true;
    self.god_rays_intensity = intensity.clamp(0.0, 2.0);
    self.god_rays_samples = samples.clamp(4, 64);
    self.god_rays_decay = decay.clamp(0.8, 0.999);
    self.god_rays_threshold = threshold.max(0.0);
    self
  }

  /// Set atmosphere density
  pub fn with_density(mut self, density: f32) -> Self {
    self.density = density.clamp(0.0, 1.0);
    self
  }

  /// Set fog color
  pub fn with_color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }

  /// Set scattering coefficient
  pub fn with_scattering(mut self, scattering: f32) -> Self {
    self.scattering = scattering.clamp(0.0, 1.0);
    self
  }

  /// Get the god rays intensity (0 if disabled)
  pub fn effective_god_rays_intensity(&self) -> f32 {
    if self.god_rays_enabled {
      self.god_rays_intensity
    } else {
      0.0
    }
  }
}

/// Quality presets for god rays rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GodRaysQuality {
  /// Low quality (8 samples) - for mobile/low-end
  Low,
  /// Medium quality (16 samples) - default
  Medium,
  /// High quality (32 samples) - for high-end
  High,
}

impl GodRaysQuality {
  /// Get the number of samples for this quality level
  pub fn samples(&self) -> u32 {
    match self {
      Self::Low => 8,
      Self::Medium => 16,
      Self::High => 32,
    }
  }

  /// Get the decay rate for this quality level
  /// Higher samples can use higher decay (longer rays)
  pub fn decay(&self) -> f32 {
    match self {
      Self::Low => 0.94,
      Self::Medium => 0.96,
      Self::High => 0.97,
    }
  }
}

impl Atmosphere {
  /// Set god rays quality using a preset
  pub fn with_god_rays_quality(mut self, quality: GodRaysQuality) -> Self {
    self.god_rays_enabled = true;
    self.god_rays_samples = quality.samples();
    self.god_rays_decay = quality.decay();
    self
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_is_clear() {
    let atmos = Atmosphere::default();
    assert_eq!(atmos.density, 0.0);
    assert!(!atmos.god_rays_enabled);
  }

  #[test]
  fn dusty_has_god_rays() {
    let atmos = Atmosphere::dusty();
    assert!(atmos.density > 0.0);
    assert!(atmos.god_rays_enabled);
  }

  #[test]
  fn builder_methods() {
    let atmos = Atmosphere::clear()
      .with_density(0.5)
      .with_god_rays()
      .with_god_rays_quality(GodRaysQuality::High);

    assert!((atmos.density - 0.5).abs() < 0.001);
    assert!(atmos.god_rays_enabled);
    assert_eq!(atmos.god_rays_samples, 32);
  }

  #[test]
  fn quality_presets() {
    assert_eq!(GodRaysQuality::Low.samples(), 8);
    assert_eq!(GodRaysQuality::Medium.samples(), 16);
    assert_eq!(GodRaysQuality::High.samples(), 32);
  }

  #[test]
  fn effective_intensity_when_disabled() {
    let atmos = Atmosphere::default();
    assert_eq!(atmos.effective_god_rays_intensity(), 0.0);
  }

  #[test]
  fn preset_god_rays_come_from_light_not_lit_grey() {
    // Mid-grey (sRGB 128 is 0.22 linear) under a light of 2 is about 0.43
    // and stays out of the rays; a sun lit to 1.5 feeds them
    for atmos in [
      Atmosphere::clear(),
      Atmosphere::hazy(),
      Atmosphere::dusty(),
      Atmosphere::foggy(),
      Atmosphere::smoky(),
      Atmosphere::underwater(),
    ] {
      assert!((0.43..1.5).contains(&atmos.god_rays_threshold), "{atmos:?}");
    }
  }

  #[test]
  fn effective_intensity_when_enabled() {
    let atmos = Atmosphere::dusty();
    assert!(atmos.effective_god_rays_intensity() > 0.0);
  }
}
