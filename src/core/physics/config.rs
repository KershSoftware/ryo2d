//! Global physics settings.

use crate::core::ecs::{Resource, Vec2};

/// Gravity, the pixel scale and solver effort. Edit it live through
/// `ctx.resource_mut::<PhysicsConfig>()`; changes apply on the next step.
#[derive(Resource, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PhysicsConfig {
  /// Acceleration in pixels per second squared (+Y is down).
  pub gravity: Vec2,
  /// How many pixels make a metre: Rapier's tolerances, sleep thresholds
  /// and speed cap are tuned for objects about a metre across, and density
  /// is per square metre.
  pub pixels_per_meter: f32,
  /// Constraint solver iterations per step: more is stiffer stacks and
  /// joints, at a cost.
  pub solver_iterations: usize,
}

impl PhysicsConfig {
  pub const DEFAULT_PIXELS_PER_METER: f32 = 100.0;

  pub fn with_gravity(mut self, gravity: Vec2) -> Self {
    self.gravity = gravity;
    self
  }

  /// No gravity: space and top-down games.
  pub fn zero_gravity(mut self) -> Self {
    self.gravity = Vec2::ZERO;
    self
  }

  pub fn with_pixels_per_meter(mut self, pixels: f32) -> Self {
    self.pixels_per_meter = pixels;
    self
  }

  pub fn with_solver_iterations(mut self, iterations: usize) -> Self {
    self.solver_iterations = iterations;
    self
  }

  /// `pixels_per_meter`, or the default when it is not a positive number.
  pub(crate) fn length_unit(&self) -> f32 {
    if self.pixels_per_meter.is_finite() && self.pixels_per_meter > 0.0 {
      self.pixels_per_meter
    } else {
      Self::DEFAULT_PIXELS_PER_METER
    }
  }

  /// At least one solver iteration.
  pub(crate) fn iterations(&self) -> usize {
    self.solver_iterations.max(1)
  }
}

impl Default for PhysicsConfig {
  fn default() -> Self {
    Self {
      gravity: Vec2::new(0.0, 980.0),
      pixels_per_meter: Self::DEFAULT_PIXELS_PER_METER,
      solver_iterations: 4,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn defaults() {
    let config = PhysicsConfig::default();
    assert_eq!(config.gravity, Vec2::new(0.0, 980.0));
    assert_eq!(config.pixels_per_meter, 100.0);
    assert_eq!(config.solver_iterations, 4);
  }

  #[test]
  fn builders() {
    let config = PhysicsConfig::default()
      .with_gravity(Vec2::new(1.0, 2.0))
      .with_pixels_per_meter(32.0)
      .with_solver_iterations(8);
    assert_eq!(config.gravity, Vec2::new(1.0, 2.0));
    assert_eq!(config.length_unit(), 32.0);
    assert_eq!(config.iterations(), 8);
    assert_eq!(PhysicsConfig::default().zero_gravity().gravity, Vec2::ZERO);
  }

  #[test]
  fn nonsense_values_fall_back() {
    let config = PhysicsConfig::default()
      .with_pixels_per_meter(f32::NAN)
      .with_solver_iterations(0);
    assert_eq!(
      config.length_unit(),
      PhysicsConfig::DEFAULT_PIXELS_PER_METER
    );
    assert_eq!(config.iterations(), 1);
    let negative = PhysicsConfig::default().with_pixels_per_meter(-5.0);
    assert_eq!(
      negative.length_unit(),
      PhysicsConfig::DEFAULT_PIXELS_PER_METER
    );
  }
}
