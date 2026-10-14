//! The engine's glam (0.31) and Rapier's glam (0.33) are different crates:
//! every vector crossing into or out of Rapier goes through here.

use std::f32::consts::{PI, TAU};

use rapier2d::math::{Pose, Vector};

use crate::core::ecs::Vec2;

/// Translation tolerance for "the game moved it", in pixels.
pub(crate) const POSITION_TOLERANCE: f32 = 1e-3;
/// Rotation tolerance for "the game turned it", in radians.
pub(crate) const ROTATION_TOLERANCE: f32 = 1e-4;

pub(crate) fn to_rapier(v: Vec2) -> Vector {
  Vector::new(v.x, v.y)
}

pub(crate) fn from_rapier(v: Vector) -> Vec2 {
  Vec2::new(v.x, v.y)
}

pub(crate) fn pose(translation: Vec2, rotation: f32) -> Pose {
  Pose::new(to_rapier(translation), rotation)
}

/// The shortest signed turn from `from` to `to`, in `[-PI, PI)`.
pub(crate) fn angle_delta(from: f32, to: f32) -> f32 {
  (to - from + PI).rem_euclid(TAU) - PI
}

/// Whether two poses differ by more than the sync tolerance. The rotation
/// tolerance grows with the angles: `Transform.rotation` is continuous, and
/// past a few thousand radians its f32 round-off alone exceeds 1e-4.
pub(crate) fn poses_differ(a: (Vec2, f32), b: (Vec2, f32)) -> bool {
  let round_off = 4.0 * f32::EPSILON * a.1.abs().max(b.1.abs());
  (a.0 - b.0).abs().max_element() > POSITION_TOLERANCE
    || angle_delta(a.1, b.1).abs() > ROTATION_TOLERANCE.max(round_off)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn angle_delta_takes_the_short_way_round() {
    assert!((angle_delta(3.0, -3.0) - (TAU - 6.0)).abs() < 1e-5);
    assert!((angle_delta(0.0, 0.5) - 0.5).abs() < 1e-6);
    assert!((angle_delta(10.0 * TAU, 0.25) - 0.25).abs() < 1e-3);
  }

  #[test]
  fn large_angles_equal_up_to_round_off_do_not_differ() {
    let v = Vec2::ZERO;
    // Rapier's wrapped angle against the continuous `Transform.rotation`
    for big in [2000.0_f32, 3000.0, 3141.5, 4000.0, 5000.0] {
      let wrapped = ((f64::from(big) + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI) as f32;
      assert!(!poses_differ((v, wrapped), (v, big)), "{big}");
    }
    assert!(poses_differ((v, 0.0), (v, 1e-3)));
    assert!(poses_differ((v, 3000.0), (v, 3000.01)));
  }

  #[test]
  fn poses_within_tolerance_do_not_differ() {
    let a = (Vec2::new(10.0, 20.0), 1.0);
    assert!(!poses_differ(a, (Vec2::new(10.0005, 20.0), 1.00005)));
    assert!(poses_differ(a, (Vec2::new(10.01, 20.0), 1.0)));
    assert!(poses_differ(a, (Vec2::new(10.0, 20.0), 1.001)));
    assert!(!poses_differ(a, (Vec2::new(10.0, 20.0), 1.0 + TAU)));
  }
}
