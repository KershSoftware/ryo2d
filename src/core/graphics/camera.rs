use glam::Vec2;

use super::super::ecs::{Component, Ease};

/// Camera shake for impact feedback, on a camera entity.
///
/// Uses a trauma-based system where trauma accumulates from impacts
/// and decays over time (in real time, so it keeps going through pause and
/// slow motion). Shake intensity is controlled by an easing curve (default:
/// InQuad for punchy feel).
#[derive(Component, Debug, Clone)]
pub struct CameraShake {
  /// Current trauma level (0.0 to 1.0, clamped)
  trauma: f32,
  /// Maximum pixel offset at full trauma (default: 10.0)
  max_offset: f32,
  /// Trauma decay rate per second (default: 1.0)
  decay_rate: f32,
  /// Easing curve for trauma-to-intensity mapping (default: InQuad)
  intensity_curve: Ease,
  /// Time accumulator for smooth noise sampling
  noise_time: f32,
  /// Current shake offset in X (pixels)
  offset_x: f32,
  /// Current shake offset in Y (pixels)
  offset_y: f32,
}

impl Default for CameraShake {
  fn default() -> Self {
    Self {
      trauma: 0.0,
      max_offset: 10.0,
      decay_rate: 1.0,
      intensity_curve: Ease::InQuad,
      noise_time: 0.0,
      offset_x: 0.0,
      offset_y: 0.0,
    }
  }
}

impl CameraShake {
  /// Create shake with custom parameters.
  pub fn new(max_offset: f32, decay_rate: f32) -> Self {
    Self {
      max_offset,
      decay_rate,
      ..Default::default()
    }
  }

  /// Set the intensity curve for trauma-to-shake mapping.
  ///
  /// The curve controls how trauma translates to shake intensity:
  /// - `InQuad` (default): Punchy feel, strong at high trauma
  /// - `Linear`: Direct mapping, shake proportional to trauma
  /// - `InCubic`/`InQuart`: Even punchier, mostly calm until high trauma
  /// - `OutQuad`: More shake at low trauma levels
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Very punchy shake - mostly calm, intense at high trauma
  /// camera.set_shake_curve(Ease::InCubic);
  ///
  /// // Linear shake - proportional to trauma
  /// camera.set_shake_curve(Ease::Linear);
  /// ```
  pub fn with_intensity_curve(mut self, curve: Ease) -> Self {
    self.intensity_curve = curve;
    self
  }

  /// Smooth noise function using layered sine waves.
  /// Returns value in range [-1.0, 1.0].
  fn noise(t: f32, seed: f32) -> f32 {
    // Layer multiple frequencies for more organic feel
    let n1 = (t * 7.0 + seed).sin();
    let n2 = (t * 13.0 + seed * 2.3).sin() * 0.5;
    let n3 = (t * 23.0 + seed * 5.7).sin() * 0.25;
    (n1 + n2 + n3) / 1.75 // Normalize to [-1, 1]
  }

  /// Add trauma (accumulates, clamped to 0.0-1.0): 0.1-0.3 for minor
  /// impacts, 0.5-0.8 for explosions.
  pub fn add_trauma(&mut self, amount: f32) {
    self.trauma = (self.trauma + amount).clamp(0.0, 1.0);
  }

  /// The offset at full trauma, in screen units, and how fast trauma decays
  /// per second (defaults 10.0 and 1.0).
  pub fn configure(&mut self, max_offset: f32, decay_rate: f32) {
    self.max_offset = max_offset;
    self.decay_rate = decay_rate;
  }

  /// The current trauma (0.0 to 1.0).
  pub fn trauma(&self) -> f32 {
    self.trauma
  }

  /// Clear the trauma and the offset at once.
  pub fn reset(&mut self) {
    self.trauma = 0.0;
    self.offset_x = 0.0;
    self.offset_y = 0.0;
  }

  /// The current shake offset, in screen units.
  pub fn offset(&self) -> Vec2 {
    Vec2::new(self.offset_x, self.offset_y)
  }

  /// Update shake state and calculate new offsets.
  pub(crate) fn update(&mut self, dt: f32) {
    if self.trauma <= 0.0 {
      self.offset_x = 0.0;
      self.offset_y = 0.0;
      return;
    }

    // Advance noise time
    self.noise_time += dt * 10.0;

    // Calculate intensity using the configured curve
    let intensity = self.intensity_curve.apply(self.trauma);

    // Calculate offsets using smooth noise
    self.offset_x = Self::noise(self.noise_time, 0.0) * self.max_offset * intensity;
    self.offset_y = Self::noise(self.noise_time, 100.0) * self.max_offset * intensity;

    // Decay trauma
    self.trauma = (self.trauma - self.decay_rate * dt).max(0.0);
  }

  /// Set how trauma maps to intensity.
  pub fn set_curve(&mut self, curve: Ease) {
    self.intensity_curve = curve;
  }

  /// How trauma maps to intensity.
  pub fn curve(&self) -> Ease {
    self.intensity_curve
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn shake_starts_at_zero() {
    let shake = CameraShake::default();
    assert_eq!(shake.trauma(), 0.0);
    assert_eq!(shake.offset(), Vec2::ZERO);
  }

  #[test]
  fn add_trauma_accumulates_and_clamps_to_one() {
    let mut shake = CameraShake::default();
    shake.add_trauma(0.3);
    assert!((shake.trauma() - 0.3).abs() < 0.001);
    shake.add_trauma(0.4);
    assert!((shake.trauma() - 0.7).abs() < 0.001);
    shake.add_trauma(0.5);
    assert_eq!(shake.trauma(), 1.0);
  }

  #[test]
  fn shake_decays_over_time() {
    let mut shake = CameraShake::default();
    shake.add_trauma(1.0);
    shake.update(0.5);
    assert!((shake.trauma() - 0.5).abs() < 0.001);
    shake.update(0.5);
    assert_eq!(shake.trauma(), 0.0);
  }

  #[test]
  fn shake_produces_an_offset_only_while_active() {
    let mut shake = CameraShake::default();
    shake.update(0.1);
    assert_eq!(shake.offset(), Vec2::ZERO);
    shake.add_trauma(1.0);
    shake.update(0.1);
    assert!(shake.offset() != Vec2::ZERO);
  }

  #[test]
  fn configure_changes_the_decay() {
    let mut shake = CameraShake::default();
    shake.configure(20.0, 2.0);
    shake.add_trauma(1.0);
    shake.update(0.5);
    assert_eq!(shake.trauma(), 0.0);
  }

  #[test]
  fn the_curve_defaults_to_in_quad_and_can_change() {
    let mut shake = CameraShake::default();
    assert_eq!(shake.curve(), Ease::InQuad);
    shake.set_curve(Ease::Linear);
    assert_eq!(shake.curve(), Ease::Linear);
    let built = CameraShake::new(15.0, 2.0).with_intensity_curve(Ease::InCubic);
    assert_eq!(built.curve(), Ease::InCubic);
  }

  #[test]
  fn reset_clears_trauma_and_offset() {
    let mut shake = CameraShake::default();
    shake.add_trauma(0.8);
    shake.update(0.1);
    shake.reset();
    assert_eq!(shake.trauma(), 0.0);
    assert_eq!(shake.offset(), Vec2::ZERO);
  }
}
