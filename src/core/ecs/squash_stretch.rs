//! Squash and Stretch component for impact effects
//!
//! Provides automatic scale deformation that recovers over time,
//! commonly used for landing impacts, hits, and bouncy animations.

use super::kernel::Component;

/// Axis for squash/stretch deformation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SquashAxis {
  /// Squash on Y axis (landing, vertical impacts)
  #[default]
  Y,
  /// Squash on X axis (wall hits, horizontal impacts)
  X,
}

/// Squash and stretch component for impact deformation effects.
///
/// When applied, the entity's scale is deformed based on intensity,
/// then automatically recovers to the base scale over time.
///
/// # Example
///
/// ```ignore
/// // Quick squash on landing
/// ctx.squash(entity, 0.3);
///
/// // Or with full control
/// world.add_component(entity, SquashStretch::new(0.4)
///     .with_axis(SquashAxis::X)
///     .with_decay_rate(6.0)
///     .with_preserve_volume(true));
/// ```
#[derive(Component, Debug, Clone)]
pub struct SquashStretch {
  /// Deformation intensity (0.3 = 30% squash/stretch)
  intensity: f32,
  /// Current strength (1.0 = full effect, decays to 0.0)
  strength: f32,
  /// How fast strength decays (default 8.0 for snappy recovery)
  decay_rate: f32,
  /// Which axis to squash on impact
  axis: SquashAxis,
  /// Whether to preserve volume (squash one axis = stretch other)
  preserve_volume: bool,
  /// Base scale to return to (captured on first apply)
  base_scale_x: f32,
  base_scale_y: f32,
  /// Whether base scale has been captured
  base_captured: bool,
  /// Whether the effect is currently active
  pub active: bool,
  /// Set to true on the frame the effect starts
  pub just_started: bool,
  /// Set to true on the frame the effect finishes
  pub just_finished: bool,
}

impl Default for SquashStretch {
  fn default() -> Self {
    Self {
      intensity: 0.3,
      strength: 0.0,
      decay_rate: 8.0,
      axis: SquashAxis::Y,
      preserve_volume: true,
      base_scale_x: 1.0,
      base_scale_y: 1.0,
      base_captured: false,
      active: false,
      just_started: false,
      just_finished: false,
    }
  }
}

impl SquashStretch {
  /// Create a new squash/stretch effect with given intensity.
  ///
  /// Intensity is the amount of deformation (0.3 = 30%).
  /// The effect starts inactive; call `apply()` or use `ctx.squash()` to trigger.
  pub fn new(intensity: f32) -> Self {
    Self {
      intensity: intensity.abs(),
      strength: 1.0,
      active: true,
      just_started: true,
      ..Default::default()
    }
  }

  /// Set the decay rate (how fast the effect recovers).
  ///
  /// Higher values = faster recovery. Default is 8.0.
  /// - 12.0 = very snappy (~0.08s)
  /// - 8.0 = snappy (~0.125s)
  /// - 4.0 = moderate (~0.25s)
  /// - 2.0 = slow (~0.5s)
  pub fn with_decay_rate(mut self, rate: f32) -> Self {
    self.decay_rate = rate.max(0.1);
    self
  }

  /// Set which axis to squash on.
  ///
  /// - `SquashAxis::Y` (default) - for landing, vertical impacts
  /// - `SquashAxis::X` - for wall hits, horizontal impacts
  pub fn with_axis(mut self, axis: SquashAxis) -> Self {
    self.axis = axis;
    self
  }

  /// Set whether to preserve volume.
  ///
  /// When true (default), squashing one axis stretches the other.
  /// When false, only the primary axis is affected.
  pub fn with_preserve_volume(mut self, preserve: bool) -> Self {
    self.preserve_volume = preserve;
    self
  }

  /// Set the base scale to return to.
  ///
  /// If not set, the base scale is captured from the entity's Transform
  /// on the first update.
  pub fn with_base_scale(mut self, scale_x: f32, scale_y: f32) -> Self {
    self.base_scale_x = scale_x;
    self.base_scale_y = scale_y;
    self.base_captured = true;
    self
  }

  /// Apply a new squash/stretch effect.
  ///
  /// Resets strength to 1.0 and updates intensity.
  pub fn apply(&mut self, intensity: f32) {
    let was_active = self.active;
    self.intensity = intensity.abs();
    self.strength = 1.0;
    self.active = true;
    self.just_started = !was_active;
    self.just_finished = false;
  }

  /// Apply a new squash/stretch effect with custom decay rate.
  ///
  /// Resets strength to 1.0 and updates both intensity and decay_rate.
  pub fn apply_with_decay(&mut self, intensity: f32, decay_rate: f32) {
    let was_active = self.active;
    self.intensity = intensity.abs();
    self.decay_rate = decay_rate.max(0.1);
    self.strength = 1.0;
    self.active = true;
    self.just_started = !was_active;
    self.just_finished = false;
  }

  /// Apply with a specific axis.
  pub fn apply_on_axis(&mut self, intensity: f32, axis: SquashAxis) {
    self.axis = axis;
    self.apply(intensity);
  }

  /// Capture the base scale from current transform values.
  pub fn capture_base_scale(&mut self, scale_x: f32, scale_y: f32) {
    if !self.base_captured {
      self.base_scale_x = scale_x;
      self.base_scale_y = scale_y;
      self.base_captured = true;
    }
  }

  /// Get the current scale values to apply to the transform.
  ///
  /// Returns (scale_x, scale_y) based on current strength and settings.
  pub fn current_scale(&self) -> (f32, f32) {
    if !self.active || self.strength <= 0.0 {
      return (self.base_scale_x, self.base_scale_y);
    }

    // Use squared strength for punchy feel (strong start, smooth end)
    let t = self.strength * self.strength;
    let deform = self.intensity * t;

    match self.axis {
      SquashAxis::Y => {
        // Squash Y = compress vertically
        let scale_y = self.base_scale_y * (1.0 - deform);
        let scale_x = if self.preserve_volume {
          // Stretch X to preserve volume
          self.base_scale_x * (1.0 + deform)
        } else {
          self.base_scale_x
        };
        (scale_x, scale_y)
      }
      SquashAxis::X => {
        // Squash X = compress horizontally
        let scale_x = self.base_scale_x * (1.0 - deform);
        let scale_y = if self.preserve_volume {
          // Stretch Y to preserve volume
          self.base_scale_y * (1.0 + deform)
        } else {
          self.base_scale_y
        };
        (scale_x, scale_y)
      }
    }
  }

  /// Update the effect (called by engine each frame).
  ///
  /// Decays strength and updates state flags.
  pub fn update(&mut self, dt: f32) {
    self.just_started = false;
    self.just_finished = false;

    if !self.active {
      return;
    }

    // Decay strength
    self.strength = (self.strength - self.decay_rate * dt).max(0.0);

    // Check if finished
    if self.strength <= 0.0 {
      self.active = false;
      self.just_finished = true;
    }
  }

  /// Stop the effect immediately, returning to base scale.
  pub fn stop(&mut self) {
    if self.active {
      self.active = false;
      self.strength = 0.0;
      self.just_finished = true;
    }
  }

  /// Check if the effect is currently active.
  pub fn is_active(&self) -> bool {
    self.active
  }

  /// Get the current strength (0.0 to 1.0).
  pub fn strength(&self) -> f32 {
    self.strength
  }

  /// Get the intensity setting.
  pub fn intensity(&self) -> f32 {
    self.intensity
  }

  /// Get the base scale.
  pub fn base_scale(&self) -> (f32, f32) {
    (self.base_scale_x, self.base_scale_y)
  }

  /// Get the decay rate.
  pub fn decay_rate(&self) -> f32 {
    self.decay_rate
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_new_creates_active_effect() {
    let ss = SquashStretch::new(0.3);
    assert!(ss.active);
    assert!(ss.just_started);
    assert!(!ss.just_finished);
    assert_eq!(ss.strength, 1.0);
    assert_eq!(ss.intensity, 0.3);
  }

  #[test]
  fn test_default_is_inactive() {
    let ss = SquashStretch::default();
    assert!(!ss.active);
    assert_eq!(ss.strength, 0.0);
  }

  #[test]
  fn test_decay_reduces_strength() {
    let mut ss = SquashStretch::new(0.3).with_decay_rate(4.0);
    ss.update(0.1); // 4.0 * 0.1 = 0.4 decay
    assert!((ss.strength - 0.6).abs() < 0.001);
    assert!(ss.active);
  }

  #[test]
  fn test_effect_finishes_when_strength_zero() {
    let mut ss = SquashStretch::new(0.3).with_decay_rate(10.0);
    ss.update(0.1); // 10.0 * 0.1 = 1.0 decay, strength = 0
    assert!(!ss.active);
    assert!(ss.just_finished);
  }

  #[test]
  fn test_squash_y_with_volume_preservation() {
    let ss = SquashStretch::new(0.3)
      .with_axis(SquashAxis::Y)
      .with_preserve_volume(true)
      .with_base_scale(1.0, 1.0);

    // At full strength (1.0), intensity 0.3:
    // scale_y = 1.0 * (1.0 - 0.3) = 0.7
    // scale_x = 1.0 * (1.0 + 0.3) = 1.3
    let (sx, sy) = ss.current_scale();
    assert!((sx - 1.3).abs() < 0.001);
    assert!((sy - 0.7).abs() < 0.001);
  }

  #[test]
  fn test_squash_x_with_volume_preservation() {
    let ss = SquashStretch::new(0.3)
      .with_axis(SquashAxis::X)
      .with_preserve_volume(true)
      .with_base_scale(1.0, 1.0);

    let (sx, sy) = ss.current_scale();
    assert!((sx - 0.7).abs() < 0.001);
    assert!((sy - 1.3).abs() < 0.001);
  }

  #[test]
  fn test_no_volume_preservation() {
    let ss = SquashStretch::new(0.3)
      .with_axis(SquashAxis::Y)
      .with_preserve_volume(false)
      .with_base_scale(1.0, 1.0);

    let (sx, sy) = ss.current_scale();
    assert!((sx - 1.0).abs() < 0.001); // X unchanged
    assert!((sy - 0.7).abs() < 0.001);
  }

  #[test]
  fn test_squared_strength_curve() {
    let mut ss = SquashStretch::new(0.4)
      .with_decay_rate(2.0)
      .with_base_scale(1.0, 1.0);

    // At strength 0.5, t = 0.5² = 0.25
    // deform = 0.4 * 0.25 = 0.1
    ss.strength = 0.5;
    let (_, sy) = ss.current_scale();
    let expected = 1.0 * (1.0 - 0.1);
    assert!((sy - expected).abs() < 0.001);
  }

  #[test]
  fn test_apply_resets_strength() {
    let mut ss = SquashStretch::new(0.3);
    ss.update(0.2); // Decay some
    assert!(ss.strength < 1.0);

    ss.apply(0.5);
    assert_eq!(ss.strength, 1.0);
    assert_eq!(ss.intensity, 0.5);
  }

  #[test]
  fn test_stop_immediately_ends_effect() {
    let mut ss = SquashStretch::new(0.3);
    ss.stop();
    assert!(!ss.active);
    assert!(ss.just_finished);
    assert_eq!(ss.strength, 0.0);
  }

  #[test]
  fn test_inactive_returns_base_scale() {
    let mut ss = SquashStretch::new(0.3).with_base_scale(2.0, 1.5);
    ss.stop();

    let (sx, sy) = ss.current_scale();
    assert!((sx - 2.0).abs() < 0.001);
    assert!((sy - 1.5).abs() < 0.001);
  }

  #[test]
  fn test_custom_base_scale() {
    let ss = SquashStretch::new(0.5)
      .with_base_scale(2.0, 2.0)
      .with_axis(SquashAxis::Y);

    // At full strength: deform = 0.5
    // scale_y = 2.0 * (1.0 - 0.5) = 1.0
    // scale_x = 2.0 * (1.0 + 0.5) = 3.0
    let (sx, sy) = ss.current_scale();
    assert!((sx - 3.0).abs() < 0.001);
    assert!((sy - 1.0).abs() < 0.001);
  }

  #[test]
  fn test_intensity_is_absolute() {
    let ss = SquashStretch::new(-0.3);
    assert_eq!(ss.intensity, 0.3);
  }

  #[test]
  fn test_just_started_clears_after_update() {
    let mut ss = SquashStretch::new(0.3);
    assert!(ss.just_started);
    ss.update(0.016);
    assert!(!ss.just_started);
  }

  #[test]
  fn test_apply_on_axis() {
    let mut ss = SquashStretch::new(0.3).with_axis(SquashAxis::Y);
    ss.apply_on_axis(0.4, SquashAxis::X);
    assert_eq!(ss.axis, SquashAxis::X);
    assert_eq!(ss.intensity, 0.4);
  }

  #[test]
  fn test_apply_with_decay_updates_both() {
    let mut ss = SquashStretch::new(0.2).with_decay_rate(4.0);
    assert_eq!(ss.intensity, 0.2);
    assert_eq!(ss.decay_rate, 4.0);

    // Decay some
    ss.update(0.1);
    assert!(ss.strength < 1.0);

    // Apply with new values
    ss.apply_with_decay(0.5, 12.0);
    assert_eq!(ss.intensity, 0.5);
    assert_eq!(ss.decay_rate, 12.0);
    assert_eq!(ss.strength, 1.0); // Reset to full
  }
}
