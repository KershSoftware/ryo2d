//! What a scene controls about lighting; the renderer reads it each frame.

use crate::core::ecs::Resource;

use super::{AmbientLight, Atmosphere, VolumetricSettings};

/// The scene-controlled lighting state: ambient light, atmosphere and
/// volumetric settings. The engine copies it into the lighting renderer
/// before each lit frame.
#[derive(Resource, Debug, Clone, Default)]
pub struct LightingSettings {
  ambient: AmbientLight,
  atmosphere: Atmosphere,
  volumetric: VolumetricSettings,
}

impl LightingSettings {
  pub fn ambient(&self) -> &AmbientLight {
    &self.ambient
  }

  pub fn set_ambient(&mut self, ambient: AmbientLight) {
    self.ambient = ambient;
  }

  pub fn atmosphere(&self) -> &Atmosphere {
    &self.atmosphere
  }

  pub fn set_atmosphere(&mut self, atmosphere: Atmosphere) {
    self.atmosphere = atmosphere;
  }

  pub fn volumetric(&self) -> &VolumetricSettings {
    &self.volumetric
  }

  pub fn set_volumetric(&mut self, volumetric: VolumetricSettings) {
    self.volumetric = volumetric;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn settings_default_to_the_parts_defaults_and_keep_what_is_set() {
    let mut settings = LightingSettings::default();
    assert_eq!(settings.ambient(), &AmbientLight::default());
    let ambient = AmbientLight::with_intensity(0.25);
    settings.set_ambient(ambient);
    assert_eq!(settings.ambient(), &ambient);
    settings.set_atmosphere(Atmosphere::default());
    settings.set_volumetric(VolumetricSettings::default());
    assert_eq!(settings.volumetric(), &VolumetricSettings::default());
  }
}
