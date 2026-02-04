//! Volumetric lighting settings
//!
//! Per-light volumetric settings for visible light beams and god rays.
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Create a flashlight with visible beam
//! let flashlight = Light2D::flashlight(200.0, 0.0)
//!     .with_volumetric();
//!
//! // Create a light that contributes to god rays
//! let sun = Light2D::point(Color::WHITE, 500.0)
//!     .with_god_rays();
//!
//! // Full control over volumetric settings
//! let light = Light2D::spot(Color::WHITE, 300.0, 45.0, 90.0)
//!     .with_volumetric_settings(VolumetricSettings {
//!         enabled: true,
//!         intensity: 0.5,
//!         density: 0.3,
//!         scattering: 0.2,
//!         god_rays: true,
//!     });
//! ```

/// Per-light volumetric settings for atmospheric effects.
///
/// Controls how this light interacts with volumetric rendering:
/// - Visible light cones/beams in dusty air
/// - Contribution to screen-space god rays
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumetricSettings {
  /// Whether volumetric effects are enabled for this light
  pub enabled: bool,
  /// Volumetric intensity (0.0 - 1.0)
  /// Controls how visible the light cone/beam is
  pub intensity: f32,
  /// Fog density within the light cone (0.0 - 1.0)
  /// Higher values = more visible beam
  pub density: f32,
  /// Light scattering coefficient (0.0 - 1.0)
  /// Controls how much light spreads in the atmosphere
  pub scattering: f32,
  /// Whether this light contributes to god rays post-effect
  pub god_rays: bool,
}

impl Default for VolumetricSettings {
  fn default() -> Self {
    Self {
      enabled: false,
      intensity: 0.5,
      density: 0.3,
      scattering: 0.2,
      god_rays: false,
    }
  }
}

impl VolumetricSettings {
  /// Create new volumetric settings with custom parameters
  pub fn new(intensity: f32, density: f32, scattering: f32) -> Self {
    Self {
      enabled: true,
      intensity: intensity.clamp(0.0, 1.0),
      density: density.clamp(0.0, 1.0),
      scattering: scattering.clamp(0.0, 1.0),
      god_rays: false,
    }
  }

  /// Create default volumetric settings (enabled)
  pub fn enabled() -> Self {
    Self {
      enabled: true,
      ..Default::default()
    }
  }

  /// Create settings optimized for god rays only (no visible beam)
  pub fn god_rays_only() -> Self {
    Self {
      enabled: true,
      intensity: 0.0,
      density: 0.0,
      scattering: 0.0,
      god_rays: true,
    }
  }

  /// Create settings for a subtle visible beam
  pub fn subtle_beam() -> Self {
    Self {
      enabled: true,
      intensity: 0.4,
      density: 0.3,
      scattering: 0.2,
      god_rays: false,
    }
  }

  /// Create settings for a strong visible beam (dusty room effect)
  pub fn strong_beam() -> Self {
    Self {
      enabled: true,
      intensity: 1.0,
      density: 0.8,
      scattering: 0.5,
      god_rays: false,
    }
  }

  /// Create settings for a flashlight beam
  pub fn flashlight() -> Self {
    Self {
      enabled: true,
      intensity: 0.8,
      density: 0.6,
      scattering: 0.4,
      god_rays: false,
    }
  }

  /// Enable god rays contribution
  pub fn with_god_rays(mut self) -> Self {
    self.god_rays = true;
    self
  }

  /// Set intensity
  pub fn with_intensity(mut self, intensity: f32) -> Self {
    self.intensity = intensity.clamp(0.0, 1.0);
    self
  }

  /// Set density
  pub fn with_density(mut self, density: f32) -> Self {
    self.density = density.clamp(0.0, 1.0);
    self
  }

  /// Set scattering
  pub fn with_scattering(mut self, scattering: f32) -> Self {
    self.scattering = scattering.clamp(0.0, 1.0);
    self
  }

  /// Pack volumetric settings into a vec4 for GPU
  ///
  /// Format: [intensity, density, scattering, god_rays (0.0 or 1.0)]
  pub fn to_gpu_vec4(&self) -> [f32; 4] {
    [
      if self.enabled { self.intensity } else { 0.0 },
      if self.enabled { self.density } else { 0.0 },
      if self.enabled { self.scattering } else { 0.0 },
      if self.god_rays { 1.0 } else { 0.0 },
    ]
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_is_disabled() {
    let settings = VolumetricSettings::default();
    assert!(!settings.enabled);
    assert!(!settings.god_rays);
  }

  #[test]
  fn enabled_preset() {
    let settings = VolumetricSettings::enabled();
    assert!(settings.enabled);
    assert!(!settings.god_rays);
  }

  #[test]
  fn god_rays_only_preset() {
    let settings = VolumetricSettings::god_rays_only();
    assert!(settings.enabled);
    assert!(settings.god_rays);
    assert_eq!(settings.intensity, 0.0);
  }

  #[test]
  fn builder_methods() {
    let settings = VolumetricSettings::enabled()
      .with_intensity(0.8)
      .with_density(0.6)
      .with_god_rays();

    assert!(settings.enabled);
    assert!((settings.intensity - 0.8).abs() < 0.001);
    assert!((settings.density - 0.6).abs() < 0.001);
    assert!(settings.god_rays);
  }

  #[test]
  fn gpu_vec4_packing() {
    let settings = VolumetricSettings::new(0.5, 0.3, 0.2).with_god_rays();
    let packed = settings.to_gpu_vec4();

    assert!((packed[0] - 0.5).abs() < 0.001);
    assert!((packed[1] - 0.3).abs() < 0.001);
    assert!((packed[2] - 0.2).abs() < 0.001);
    assert!((packed[3] - 1.0).abs() < 0.001);
  }

  #[test]
  fn disabled_packs_to_zero() {
    let settings = VolumetricSettings::default();
    let packed = settings.to_gpu_vec4();

    assert_eq!(packed[0], 0.0);
    assert_eq!(packed[1], 0.0);
    assert_eq!(packed[2], 0.0);
    assert_eq!(packed[3], 0.0);
  }
}
