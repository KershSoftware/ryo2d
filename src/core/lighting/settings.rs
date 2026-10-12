//! What a scene controls about lighting; the renderer reads it each frame.

use crate::core::ecs::Resource;

use super::{AmbientLight, Atmosphere};

/// The scene-controlled lighting state: whether lighting runs, ambient light
/// and atmosphere. The engine copies it into the lighting renderer before each
/// lit frame. Volumetric lighting is per light (`Light2D::set_volumetric`).
#[derive(Resource, Debug, Clone, Default)]
pub struct LightingSettings {
  enabled: Option<bool>,
  /// A `Light2D` has existed since these settings were last reset (scene
  /// cleanup resets them), which keeps automatic lighting on
  lights_seen: bool,
  ambient: AmbientLight,
  atmosphere: Atmosphere,
}

impl LightingSettings {
  /// Force lighting on (a dark scene before any light exists) or off.
  /// Without this, lighting turns on once any `Light2D` exists and stays on
  /// until the scene's lighting is reset (a scene change, by default).
  pub fn set_enabled(&mut self, enabled: bool) {
    self.enabled = Some(enabled);
  }

  /// The forced state, if any (see [`set_enabled`](Self::set_enabled)).
  pub fn enabled(&self) -> Option<bool> {
    self.enabled
  }

  /// Whether lighting runs this frame: the forced state, otherwise on once
  /// any light has existed since the settings were reset. `lights_present`
  /// is whether a `Light2D` exists now.
  pub(crate) fn resolve(&mut self, lights_present: bool) -> bool {
    if lights_present && !self.lights_seen {
      self.lights_seen = true;
    }
    self.enabled.unwrap_or(self.lights_seen)
  }

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
    assert_eq!(settings.atmosphere(), &Atmosphere::default());
  }

  #[test]
  fn automatic_lighting_latches_on_until_reset() {
    let mut settings = LightingSettings::default();
    assert!(!settings.resolve(false));
    assert!(settings.resolve(true));
    assert!(settings.resolve(false), "stays on when the lights go");
    settings = LightingSettings::default();
    assert!(!settings.resolve(false), "a reset turns it off");
    settings.set_enabled(false);
    assert!(!settings.resolve(true), "forced off wins");
  }

  #[test]
  fn lighting_is_automatic_until_forced() {
    let mut settings = LightingSettings::default();
    assert_eq!(settings.enabled(), None);
    settings.set_enabled(true);
    assert_eq!(settings.enabled(), Some(true));
    settings.set_enabled(false);
    assert_eq!(settings.enabled(), Some(false));
  }
}
