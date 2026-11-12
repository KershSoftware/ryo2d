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

impl Curve {
  /// Eases `t`, a fraction (0 to 1) of the span from one key to the next,
  /// to a fraction of the change in value. The ends are exact: 0 gives 0 and
  /// 1 gives 1, so a key's value is hit exactly. Stepped gives 0 until 1
  pub fn value(&self, t: f32) -> f32 {
    if t.is_nan() || t <= 0.0 {
      return 0.0;
    }
    if t >= 1.0 {
      return 1.0;
    }
    match *self {
      Self::Linear => t,
      Self::Stepped => 0.0,
      Self::Bezier { cx1, cy1, cx2, cy2 } => cubic(cy1, cy2, solve_x(cx1, cx2, t)),
    }
  }
}

/// A coordinate of the curve from 0 through controls `c1` and `c2` to 1, at
/// parameter `s`
fn cubic(c1: f32, c2: f32, s: f32) -> f32 {
  let u = 1.0 - s;
  3.0 * u * u * s * c1 + 3.0 * u * s * s * c2 + s * s * s
}

/// The parameter at which the curve's x is `x`. The x controls lie in
/// 0..=1, so x rises monotonically from 0 to 1 and there is one answer.
/// Newton's method, kept inside a shrinking bracket and falling back to
/// bisection where a step would leave it (or the slope is flat, as at an end
/// with a control on it)
fn solve_x(cx1: f32, cx2: f32, x: f32) -> f32 {
  let (mut low, mut high) = (0.0_f32, 1.0_f32);
  let mut s = x;
  for _ in 0..32 {
    let error = cubic(cx1, cx2, s) - x;
    if error.abs() < 1e-7 {
      break;
    }
    if error > 0.0 {
      high = s;
    } else {
      low = s;
    }
    let u = 1.0 - s;
    let slope = 3.0 * u * u * cx1 + 6.0 * u * s * (cx2 - cx1) + 3.0 * s * s * (1.0 - cx2);
    let next = s - error / slope;
    s = if slope.abs() > 1e-6 && next > low && next < high {
      next
    } else {
      (low + high) * 0.5
    };
  }
  s
}
