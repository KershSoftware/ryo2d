//! How a timeline eases from one key to the next.

/// The easing from a key to the next. A bezier's control points are
/// fractions of the span: x of its time, y of its change in value, with the
/// curve running from (0, 0) to (1, 1). Its x controls lie in 0..=1, so time
/// only moves forward
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Curve {
  #[default]
  Linear,
  /// Holds the key's value until the next key
  Stepped,
  Bezier {
    cx1: f32,
    cy1: f32,
    cx2: f32,
    cy2: f32,
  },
}
