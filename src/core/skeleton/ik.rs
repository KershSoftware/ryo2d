//! Inverse kinematics: the turns that bring one bone to point at a target,
//! or the tip of two bones to reach it.
//!
//! The functions here are pure. They take bones' own values (each relative
//! to its parent) and a target point in the space of the first bone's
//! parent, and change those bones' rotation and scale. They know nothing of
//! a pose or a skeleton: bringing the target into that space, and placing
//! the bones again afterwards, is the caller's.

use std::f32::consts::PI;

use glam::Vec2;

use super::apply::shortest;
use super::constraint::IkSetup;
use super::pose::BonePose;

/// Below this, a length or a distance counts as none: nothing points along
/// it
const NO_LENGTH: f32 = 1e-6;

/// Below this, a linear part's determinant counts as zero: no direction can
/// be brought back through it
const FLAT: f32 = 1e-12;

/// How far apart a bone's two scales, or its two shears, may be while it
/// still counts as even
const EVEN: f32 = 1e-4;

/// How many times two bones' triangle is solved under a first bone that is
/// not even: once from the pose passed in, and then with part two as long
/// as the first bone makes it where the pass before left the second
const UNEVEN_PASSES: usize = 4;

/// The values of an IK constraint that a solve takes: those its setup gives
/// and animations can key. `uniform` is the setup's alone
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IkValues {
  /// 0 (the animated pose) to 1 (the solved pose)
  pub mix: f32,
  /// Two bones: a distance from full reach within which the tip slows as
  /// the bones straighten
  pub softness: f32,
  /// Two bones: which way the joint bends, judged in the space of the
  /// first bone's parent
  pub bend_positive: bool,
  /// One bone: scale it down when the target is nearer than its length
  pub compress: bool,
  /// One or two bones: scale the first along its length when the target
  /// is beyond reach
  pub stretch: bool,
}

impl IkValues {
  /// The values a constraint starts each step with: its setup's
  pub fn of(setup: &IkSetup) -> Self {
    Self {
      mix: setup.mix,
      softness: setup.softness,
      bend_positive: setup.bend_positive,
      compress: setup.compress,
      stretch: setup.stretch,
    }
  }
}

/// Turns `bone` to point its x axis at `target`, a point in the space of
/// the bone's parent, by `values.mix` the short way round. A mix above 1 is
/// taken as 1.
///
/// With `values.stretch` and a target beyond the bone's reach
/// (`length × |scale_x|`), or `values.compress` and one nearer, the bone's
/// `scale_x` is multiplied by `1 + (distance / reach - 1) × mix`, which at
/// a mix of 1 puts its tip on the target; `uniform` multiplies `scale_y` by
/// the same. A bone with no reach (no `length`, or no x scale) only turns.
///
/// Only the bone's rotation and scale are written. Nothing is, when the mix
/// is 0 or less, when the target is on the bone's origin, or when any value
/// given, or any that would be written, is not finite
#[cfg_attr(
  not(test),
  expect(
    dead_code,
    reason = "a skeleton's step applies its IK constraints; until it does, only tests solve one"
  )
)]
pub(crate) fn one_bone(
  bone: &mut BonePose,
  length: f32,
  target: Vec2,
  values: IkValues,
  uniform: bool,
) {
  if !(finite(bone) && given(length, target, values)) || values.mix <= 0.0 {
    return;
  }
  let to_target = target - Vec2::new(bone.x, bone.y);
  let distance = to_target.length();
  if distance <= NO_LENGTH {
    return;
  }
  let mix = values.mix.min(1.0);
  let rotation = turned(bone.rotation, pointing(bone, to_target), mix);
  let reach = length * bone.scale_x.abs();
  let scales = reach > NO_LENGTH
    && ((values.stretch && distance > reach) || (values.compress && distance < reach));
  let factor = if scales {
    scale_factor(distance, reach, mix)
  } else {
    1.0
  };
  let scale_x = bone.scale_x * factor;
  let scale_y = if uniform {
    bone.scale_y * factor
  } else {
    bone.scale_y
  };
  if !(rotation.is_finite() && scale_x.is_finite() && scale_y.is_finite()) {
    return;
  }
  bone.rotation = rotation;
  bone.scale_x = scale_x;
  bone.scale_y = scale_y;
}

/// Turns `first` and its child `second` so that the tip of `second`
/// (`length` along its x axis) reaches for `target`, a point in the space
/// of `first`'s parent, by `values.mix` the short way round. A mix above 1
/// is taken as 1.
///
/// The limb is a triangle of two rigid parts, both measured in that space
/// in the pose passed in. Part one runs from `first`'s origin to
/// `second`'s, wherever `second` sits in `first`; part two from there to
/// the tip. With part one `l1` long, part two `l2` and the target `d`
/// away, the law of cosines puts the joint
/// `along = (l1² - l2² + d²) / 2d` along the line to the target and
/// `off = √(l1² - along²)` to one side of it: the side from which part two
/// is turned the positive way from part one with `values.bend_positive`,
/// the other without. Part one is to point at the joint and part two from
/// the joint to the target:
///
/// - turning `first` turns part one by as much, whatever `first`'s scale
///   and shear, so `first`'s rotation is the angle part one is to have,
///   less the angle part one has under an unturned `first`;
/// - `second`'s x axis reaches `first`'s parent through `first`'s linear
///   part as solved, so its rotation points that axis along part two's
///   direction brought back through that linear part.
///
/// Every direction is found in the parent's space and brought back through
/// the bones' own matrices, so a `first` mirrored by its own scale bends
/// to the same side as one that is not.
///
/// A target beyond `l1 + l2` lays both parts along the line to it, and one
/// nearer than `|l1 - l2|` folds part two back along part one. With a
/// softness `s` (at most the full reach `m = l1 + l2`), a target farther
/// than `m - s` is reached for as if it were
/// `(m - s) + s × (1 - e^(-(d - (m - s)) / s))` away, so the tip nears the
/// full reach and does not arrive. With
/// `values.stretch`, no softness and an even `first`, a target beyond
/// reach multiplies `first`'s `scale_x` (and `scale_y` with `uniform`) by
/// `1 + (distance / reach - 1) × mix`, and both parts are laid along the
/// line to the target on `first` as stretched. The tip is then on the
/// target when `second` lies on `first`'s axis or with `uniform`, and
/// otherwise on the line, a little short of the target or past it.
///
/// One pass is exact when `first` scales every direction alike. When its
/// own scale is uneven or it is sheared, part two's length changes as
/// `second` turns, so the triangle is solved [`UNEVEN_PASSES`] times: each
/// pass after the first takes part two as long as `first` makes it where
/// the pass before left `second`. That brings the tip close to a target the
/// limb reaches well bent, and less close, or no closer, to one near its
/// folded or its straight reach (the module notes have the figures).
///
/// Only the bones' rotations and `first`'s scale are written. Nothing is,
/// when the mix is 0 or less, when the target is on `first`'s origin, when
/// `first` is flat or part two has no length, or when any value given, or
/// any that would be written, is not finite. When `second` sits on
/// `first`'s origin, `first` is left as it is and `second` points at the
/// target
#[cfg_attr(
  not(test),
  expect(
    dead_code,
    reason = "a skeleton's step applies its IK constraints; until it does, only tests solve one"
  )
)]
pub(crate) fn two_bones(
  first: &mut BonePose,
  second: &mut BonePose,
  length: f32,
  target: Vec2,
  values: IkValues,
  uniform: bool,
) {
  if !(finite(first) && finite(second) && given(length, target, values)) || values.mix <= 0.0 {
    return;
  }
  let mix = values.mix.min(1.0);
  let to_target = target - Vec2::new(first.x, first.y);
  let distance = to_target.length();
  // The first bone's linear part less its rotation: turning the bone turns
  // whatever this gives by as much
  let unturned = first.linear_turned(0.0);
  let determinant = unturned.determinant();
  if distance <= NO_LENGTH || !determinant.is_finite() || determinant.abs() <= FLAT {
    return;
  }
  let place = Vec2::new(second.x, second.y);
  let part_one = unturned * place;
  // Part two's length with the second bone turned to `rotation`
  let part_two = |rotation: f32| {
    let axis = Vec2::from_angle(rotation + second.shear_x) * second.scale_x;
    (unturned * (axis * length)).length()
  };
  let (l1, mut l2) = (part_one.length(), part_two(second.rotation));
  if !(l1.is_finite() && l2.is_finite()) || l2 <= NO_LENGTH || length <= 0.0 {
    return;
  }
  if l1 <= NO_LENGTH {
    // The joint is on the first bone's origin, whatever the first bone
    // does: only the second can turn the tip toward the target
    let carried = first.linear_turned(first.rotation);
    let aim = pointing(second, carried.inverse() * to_target);
    let rotation = turned(second.rotation, aim, mix);
    if rotation.is_finite() {
      second.rotation = rotation;
    }
    return;
  }

  // An even first bone keeps part two's length however the second bone
  // turns, so one pass is exact. Under any other, each pass after the first
  // measures part two where the pass before left the second bone
  let even = even(first);
  let passes = if even { 1 } else { UNEVEN_PASSES };
  let toward = to_target / distance;
  let mut shape = Triangle::new(l1, l2, distance, toward, values);
  let (mut solved, mut aim) = shape.rotations(first, second, part_one);
  for _ in 1..passes {
    l2 = part_two(aim);
    if !l2.is_finite() || l2 <= NO_LENGTH {
      return;
    }
    shape = Triangle::new(l1, l2, distance, toward, values);
    (solved, aim) = shape.rotations(first, second, part_one);
  }

  let stretches = even && values.stretch && shape.softness <= 0.0 && distance > shape.reach;
  let factor = if stretches {
    scale_factor(distance, shape.reach, mix)
  } else {
    1.0
  };
  let scale_x = first.scale_x * factor;
  let scale_y = if uniform {
    first.scale_y * factor
  } else {
    first.scale_y
  };
  if stretches {
    // Stretched along x alone, the first bone holds a part one off its axis
    // at another angle, and no longer scales every direction alike: both
    // parts are laid along the line on the bone as it will be
    let stretched = BonePose {
      scale_x,
      scale_y,
      ..*first
    };
    let held = stretched.linear_turned(0.0) * place;
    (solved, aim) = shape.rotations(&stretched, second, held);
  }
  let first_rotation = turned(first.rotation, solved, mix);
  let second_rotation = turned(second.rotation, aim, mix);
  let written = [first_rotation, second_rotation, scale_x, scale_y];
  if !written.iter().all(|value| value.is_finite()) {
    return;
  }
  first.rotation = first_rotation;
  first.scale_x = scale_x;
  first.scale_y = scale_y;
  second.rotation = second_rotation;
}

/// Where one pass has two bones' parts point, in the space of the first
/// bone's parent
struct Triangle {
  /// Part one's direction: from the first bone's origin to the joint
  one: Vec2,
  /// Part two's direction: from the joint to where the tip is to be
  two: Vec2,
  /// The two parts' lengths together
  reach: f32,
  /// The softness taken: the setting, the reach at most
  softness: f32,
}

impl Triangle {
  /// The triangle of parts `l1` and `l2` long, toward a target `distance`
  /// away along `toward`
  fn new(l1: f32, l2: f32, distance: f32, toward: Vec2, values: IkValues) -> Self {
    let reach = l1 + l2;
    let softness = if values.softness > 0.0 {
      values.softness.min(reach)
    } else {
      0.0
    };
    let begins = reach - softness;
    // 1 - e^-x, as exp_m1 gives it without rounding a small x away
    let d = if softness > 0.0 && distance > begins {
      begins - softness * (-(distance - begins) / softness).exp_m1()
    } else {
      distance
    };
    // Out of reach, or inside the fold, there is no triangle: `off` is 0
    // and both parts lie on the line to the target
    let along = (l1 * l1 - l2 * l2 + d * d) / (2.0 * d);
    let off = (l1 * l1 - along * along).max(0.0).sqrt();
    let side = toward.perp() * if values.bend_positive { off } else { -off };
    Self {
      one: toward * along - side,
      two: toward * (d - along) + side,
      reach,
      softness,
    }
  }

  /// The rotations that lay part one along `one` and point `second`'s x
  /// axis along `two`: the first bone's, which holds part one as `held`
  /// while unturned, and `second`'s, whose axis the first bone carries as
  /// `carrier` does
  fn rotations(&self, carrier: &BonePose, second: &BonePose, held: Vec2) -> (f32, f32) {
    let solved = self.one.to_angle() - held.to_angle();
    let aim = pointing(second, carrier.linear_turned(solved).inverse() * self.two);
    (solved, aim)
  }
}

/// The rotation that points `bone`'s x axis along `direction`, a direction
/// in the space of the bone's parent. The axis is drawn at the rotation
/// plus the x shear, and the other way under a negative x scale
fn pointing(bone: &BonePose, direction: Vec2) -> f32 {
  let along = if bone.scale_x < 0.0 { -1.0 } else { 1.0 };
  (direction * along).to_angle() - bone.shear_x
}

/// `from` turned toward `to` by `mix`, the short way round
fn turned(from: f32, to: f32, mix: f32) -> f32 {
  from + shortest(to - from) * mix
}

/// What stretch and compress multiply a scale by so that a `reach` becomes
/// `distance`, by `mix`
fn scale_factor(distance: f32, reach: f32, mix: f32) -> f32 {
  1.0 + (distance / reach - 1.0) * mix
}

/// Whether a bone scales every direction alike: its axes as long as each
/// other and square to each other. Equal shears turn a bone, and shears
/// half a turn apart turn it and flip one axis, as a negative scale does:
/// neither skews it
fn even(bone: &BonePose) -> bool {
  let skew = bone.shear_x - bone.shear_y;
  (bone.scale_x.abs() - bone.scale_y.abs()).abs() <= EVEN
    && (skew - PI * (skew / PI).round()).abs() <= EVEN
}

fn finite(bone: &BonePose) -> bool {
  [
    bone.x,
    bone.y,
    bone.rotation,
    bone.scale_x,
    bone.scale_y,
    bone.shear_x,
    bone.shear_y,
  ]
  .iter()
  .all(|value| value.is_finite())
}

/// Whether everything a solve is given beside its bones is finite
fn given(length: f32, target: Vec2, values: IkValues) -> bool {
  length.is_finite() && target.is_finite() && values.mix.is_finite() && values.softness.is_finite()
}

#[cfg(test)]
mod tests {
  use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

  use super::*;

  /// Where the first bone stands in its parent's space
  const ORIGIN: Vec2 = Vec2::new(10.0, 20.0);

  /// A bone at `(x, y)` turned by `rotation`, at unit scale and unsheared
  fn bone(x: f32, y: f32, rotation: f32) -> BonePose {
    BonePose {
      x,
      y,
      rotation,
      scale_x: 1.0,
      scale_y: 1.0,
      shear_x: 0.0,
      shear_y: 0.0,
    }
  }

  /// A first bone at `ORIGIN` and a second 40 along it whose length is 30:
  /// parts of 40 and 30, turned nowhere a test leaves them
  fn limb() -> (BonePose, BonePose) {
    (bone(ORIGIN.x, ORIGIN.y, 0.3), bone(40.0, 0.0, 0.9))
  }

  /// A first bone at `ORIGIN` scaled (2, 2), and a second at (20, 6) in it
  /// whose length is `SCALED_LENGTH`: parts of sqrt(1744) = 41.7612 and 30,
  /// the second off the first's axis
  fn scaled_limb() -> (BonePose, BonePose) {
    let first = BonePose {
      scale_x: 2.0,
      scale_y: 2.0,
      ..bone(ORIGIN.x, ORIGIN.y, 2.0)
    };
    (first, bone(20.0, 6.0, -1.0))
  }

  const SCALED_LENGTH: f32 = 15.0;

  /// A constraint's default values: a mix of 1, bending the positive way,
  /// and nothing else
  fn values() -> IkValues {
    IkValues::of(&IkSetup::default())
  }

  /// The point `length` along a bone's x axis, in its parent's space
  fn end(bone: &BonePose, length: f32) -> Vec2 {
    bone.affine().transform_point2(Vec2::new(length, 0.0))
  }

  /// The second bone's origin in the first bone's parent's space
  fn joint(first: &BonePose, second: &BonePose) -> Vec2 {
    first
      .affine()
      .transform_point2(Vec2::new(second.x, second.y))
  }

  /// The second bone's tip in the first bone's parent's space: the two
  /// bones' own transforms composed, as placing a pose composes them
  fn tip(first: &BonePose, second: &BonePose, length: f32) -> Vec2 {
    (first.affine() * second.affine()).transform_point2(Vec2::new(length, 0.0))
  }

  /// The cross product of part one (the first bone's origin to the joint)
  /// and part two (the joint to the tip) in the first bone's parent's
  /// space: positive when part two is turned the positive way from part one
  fn bend(first: &BonePose, second: &BonePose, length: f32) -> f32 {
    let joint = joint(first, second);
    (joint - Vec2::new(first.x, first.y)).perp_dot(tip(first, second, length) - joint)
  }

  /// `first` and `second` solved toward `target`
  fn solved(
    (mut first, mut second): (BonePose, BonePose),
    length: f32,
    target: Vec2,
    values: IkValues,
    uniform: bool,
  ) -> (BonePose, BonePose) {
    two_bones(&mut first, &mut second, length, target, values, uniform);
    (first, second)
  }

  /// `bone` solved toward `target`
  fn pointed(
    mut bone: BonePose,
    length: f32,
    target: Vec2,
    values: IkValues,
    uniform: bool,
  ) -> BonePose {
    one_bone(&mut bone, length, target, values, uniform);
    bone
  }

  fn bits(bone: &BonePose) -> [u32; 7] {
    [
      bone.x,
      bone.y,
      bone.rotation,
      bone.scale_x,
      bone.scale_y,
      bone.shear_x,
      bone.shear_y,
    ]
    .map(f32::to_bits)
  }

  /// Positions agree to 0.01 of a unit
  #[track_caller]
  fn assert_near(actual: Vec2, expected: Vec2, what: &str) {
    assert!(
      actual.distance(expected) < 0.01,
      "{what}: expected {expected}, got {actual}"
    );
  }

  /// Angles agree to 0.001 rad, whole turns apart or not
  #[track_caller]
  fn assert_angle(actual: f32, expected: f32, what: &str) {
    assert!(
      shortest(actual - expected).abs() < 0.001,
      "{what}: expected {expected}, got {actual}"
    );
  }

  #[track_caller]
  fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
      (actual - expected).abs() < 1e-4,
      "{what}: expected {expected}, got {actual}"
    );
  }

  #[track_caller]
  fn assert_close_to(actual: f32, expected: f32, within: f32, what: &str) {
    assert!(
      (actual - expected).abs() < within,
      "{what}: expected {expected}, got {actual}"
    );
  }

  /// Nothing of the bone but its rotation changed, to the bit
  #[track_caller]
  fn assert_only_turned(before: &BonePose, after: &BonePose, what: &str) {
    let turned_back = BonePose {
      rotation: before.rotation,
      ..*after
    };
    assert_eq!(bits(&turned_back), bits(before), "{what}");
  }

  #[test]
  fn the_values_a_constraint_starts_with_are_its_setup_s() {
    // Each of the five differs from its default, and from its neighbour
    let setup = IkSetup {
      mix: 0.25,
      softness: 3.0,
      bend_positive: false,
      compress: true,
      stretch: false,
      uniform: true,
    };
    assert_eq!(
      IkValues::of(&setup),
      IkValues {
        mix: 0.25,
        softness: 3.0,
        bend_positive: false,
        compress: true,
        stretch: false,
      }
    );
    let setup = IkSetup {
      compress: false,
      stretch: true,
      ..setup
    };
    let values = IkValues::of(&setup);
    assert!(values.stretch && !values.compress, "{values:?}");
    assert_eq!(
      IkValues::of(&IkSetup::default()),
      IkValues {
        mix: 1.0,
        softness: 0.0,
        bend_positive: true,
        compress: false,
        stretch: false,
      }
    );
  }

  #[test]
  fn one_bone_points_at_its_target_at_each_mix() {
    // A bone of length 30 at (10, 20), turned 135 degrees. Its target is 40
    // left of it and 40 up: at -135 degrees, which is a quarter turn on from
    // 135 the short way (through 180) and three quarters back the long way
    let start = bone(ORIGIN.x, ORIGIN.y, 0.75 * PI);
    let target = ORIGIN + Vec2::new(-40.0, -40.0);
    let at = |mix: f32| pointed(start, 30.0, target, IkValues { mix, ..values() }, false);
    // No mix, or less than none: the bone is left exactly as it was
    for mix in [0.0, -0.0, -1.0] {
      assert_eq!(bits(&at(mix)), bits(&start), "mix {mix}");
    }
    // A full mix: the tip is 30 toward the target, 30 / sqrt(2) = 21.213
    // left and up
    let full = at(1.0);
    assert_near(
      end(&full, 30.0),
      ORIGIN + Vec2::new(-21.213, -21.213),
      "mix 1",
    );
    assert_angle(full.rotation, -0.75 * PI, "mix 1");
    assert_only_turned(&start, &full, "mix 1");
    // Half: half the quarter turn, the short way, is 135 + 45 = 180 degrees,
    // so the bone points left and its tip is at (10 - 30, 20). The long way
    // would leave it at 135 - 135 = 0 degrees, pointing right
    let half = at(0.5);
    assert_near(end(&half, 30.0), ORIGIN + Vec2::new(-30.0, 0.0), "mix 0.5");
    assert_angle(half.rotation, PI, "mix 0.5");
    assert_only_turned(&start, &half, "mix 0.5");
    // Pointing needs no length: a bone of length 0 looks at its target
    let look = pointed(start, 0.0, target, values(), false);
    assert_angle(look.rotation, -0.75 * PI, "no length");
    assert_only_turned(&start, &look, "no length");
  }

  #[test]
  fn one_bone_stretches_and_compresses_to_its_target() {
    // A bone of length 30 scaled (2, 3), unturned: its tip is 60 from its
    // origin. One target is straight below it and 90 away, 1.5 times its
    // reach; the other 30 away, half its reach
    let start = BonePose {
      scale_x: 2.0,
      scale_y: 3.0,
      ..bone(ORIGIN.x, ORIGIN.y, 0.0)
    };
    let far = ORIGIN + Vec2::new(0.0, 90.0);
    let near = ORIGIN + Vec2::new(0.0, 30.0);
    let stretch = IkValues {
      stretch: true,
      ..values()
    };
    let compress = IkValues {
      compress: true,
      ..values()
    };

    // Stretch: scale_x 2 x 1.5 = 3, so the tip is 30 x 3 = 90 below, on the
    // target. The scale across the bone is left
    let after = pointed(start, 30.0, far, stretch, false);
    assert_near(end(&after, 30.0), far, "stretch");
    assert_close(after.scale_x, 3.0, "stretch");
    assert_eq!(after.scale_y, 3.0, "stretch");
    // Uniform: scale_y 3 x 1.5 = 4.5 too
    let after = pointed(start, 30.0, far, stretch, true);
    assert_near(end(&after, 30.0), far, "uniform stretch");
    assert_close(after.scale_x, 3.0, "uniform stretch");
    assert_close(after.scale_y, 4.5, "uniform stretch");

    // Compress: scale_x 2 x 0.5 = 1, so the tip is 30 below, on the target
    let after = pointed(start, 30.0, near, compress, false);
    assert_near(end(&after, 30.0), near, "compress");
    assert_close(after.scale_x, 1.0, "compress");
    assert_eq!(after.scale_y, 3.0, "compress");
    // Uniform: scale_y 3 x 0.5 = 1.5 too
    let after = pointed(start, 30.0, near, compress, true);
    assert_close(after.scale_x, 1.0, "uniform compress");
    assert_close(after.scale_y, 1.5, "uniform compress");

    // Half mix: the factor is 1 + (1.5 - 1) x 0.5 = 1.25, so scale_x is 2.5
    // and scale_y 3.75; and the bone has turned half way from 0 to 90
    // degrees, so its tip is 30 x 2.5 = 75 along 45 degrees: 53.033 right
    // and down
    let half = IkValues {
      mix: 0.5,
      ..stretch
    };
    let after = pointed(start, 30.0, far, half, true);
    assert_near(
      end(&after, 30.0),
      ORIGIN + Vec2::new(53.033, 53.033),
      "half stretch",
    );
    assert_close(after.scale_x, 2.5, "half stretch");
    assert_close(after.scale_y, 3.75, "half stretch");
    // And compress at half mix: 1 + (0.5 - 1) x 0.5 = 0.75, so scale_x 1.5
    let half = IkValues {
      mix: 0.5,
      ..compress
    };
    let after = pointed(start, 30.0, near, half, false);
    assert_close(after.scale_x, 1.5, "half compress");
    assert_eq!(after.scale_y, 3.0, "half compress");

    // Stretch does not shorten, compress does not lengthen, and with
    // neither the bone only turns: its tip stays 60 from its origin
    for (what, target, setting) in [
      ("stretch, near", near, stretch),
      ("compress, far", far, compress),
      ("neither, far", far, values()),
      ("neither, near", near, values()),
    ] {
      let after = pointed(start, 30.0, target, setting, true);
      assert_only_turned(&start, &after, what);
      assert_near(end(&after, 30.0), ORIGIN + Vec2::new(0.0, 60.0), what);
    }
    // With both, each applies on its own side of the reach
    let both = IkValues {
      compress: true,
      ..stretch
    };
    assert_near(
      end(&pointed(start, 30.0, far, both, false), 30.0),
      far,
      "both",
    );
    assert_near(
      end(&pointed(start, 30.0, near, both, false), 30.0),
      near,
      "both",
    );

    // A negative x scale is scaled as it is: -2 x 1.5 = -3, and the tip is
    // still on the target
    let mirrored = BonePose {
      scale_x: -2.0,
      ..start
    };
    let after = pointed(mirrored, 30.0, far, stretch, false);
    assert_near(end(&after, 30.0), far, "negative stretch");
    assert_close(after.scale_x, -3.0, "negative stretch");
  }

  #[test]
  fn one_bone_under_a_negative_scale_or_a_shear_still_points_at_its_target() {
    // The target is straight below the bone. Whatever the bone's scale and
    // shear, its tip, 30 along its x axis as its matrix draws it, comes to
    // 30 below its origin
    let target = ORIGIN + Vec2::new(0.0, 50.0);
    let negative = BonePose {
      scale_x: -1.0,
      ..bone(ORIGIN.x, ORIGIN.y, 0.4)
    };
    let sheared = BonePose {
      shear_x: 0.4,
      ..bone(ORIGIN.x, ORIGIN.y, -1.0)
    };
    let both = BonePose {
      scale_x: -1.0,
      scale_y: 2.0,
      shear_x: -0.7,
      shear_y: 0.3,
      ..bone(ORIGIN.x, ORIGIN.y, 2.0)
    };
    for (what, start, rotation) in [
      // A negative x scale draws the x axis the other way from where the
      // bone's rotation points, so the rotation points up: -90 degrees
      ("a negative x scale", negative, -FRAC_PI_2),
      // The x axis is drawn at rotation + shear_x, so the rotation is the
      // target's 90 degrees less the shear
      ("an x shear", sheared, FRAC_PI_2 - 0.4),
      // Both: up, less the shear: -90 degrees + 0.7. The scale and shear of
      // the y axis are not read
      ("both", both, -FRAC_PI_2 + 0.7),
    ] {
      let after = pointed(start, 30.0, target, values(), false);
      assert_near(end(&after, 30.0), ORIGIN + Vec2::new(0.0, 30.0), what);
      assert_angle(after.rotation, rotation, what);
      assert_only_turned(&start, &after, what);
    }
  }

  #[test]
  fn two_bones_reach_a_target_on_each_bend_side() {
    // Every rig's parts are 40 and 30 long. A target 50 away makes a 3-4-5
    // triangle with its right angle at the joint, so the joint is 32 along
    // the line to the target and 24 off it, and the cross product of the
    // parts is 40 x 30 = 1200 either way round
    let (first, second) = limb();
    let rigs = [
      ("unit bones", first, second, 30.0),
      // 20 x 1.5 = 30, along an x axis its shear turns by 0.2; the scale
      // and shear of its y axis are not read
      (
        "a second bone with a scale and a shear",
        first,
        BonePose {
          scale_x: 1.5,
          scale_y: 0.5,
          shear_x: 0.2,
          shear_y: -0.3,
          ..second
        },
        20.0,
      ),
      (
        "a second bone whose x scale is negative",
        first,
        BonePose {
          scale_x: -1.0,
          ..second
        },
        30.0,
      ),
      // The first bone's scale doubles both parts: 20 x 2 = 40, and
      // 10 x 1.5 x 2 = 30
      (
        "a first bone scaled evenly",
        BonePose {
          scale_x: 2.0,
          scale_y: 2.0,
          ..first
        },
        BonePose {
          x: 20.0,
          scale_x: 1.5,
          ..second
        },
        10.0,
      ),
      // Equal shears turn a bone and do not skew it
      (
        "a first bone turned by its shears",
        BonePose {
          shear_x: 0.25,
          shear_y: 0.25,
          ..first
        },
        second,
        30.0,
      ),
    ];
    // Each target with the joint for a positive bend and for a negative
    // one, all from the first bone's origin
    let cases = [
      // Along (1, 0), a quarter turn on from which is (0, 1). Positive: the
      // joint is 32 along and 24 back from that, (32, -24), and part two is
      // (18, 24): 32 x 24 - (-24) x 18 = 1200. Negative: its mirror image
      (
        Vec2::new(50.0, 0.0),
        Vec2::new(32.0, -24.0),
        Vec2::new(32.0, 24.0),
      ),
      // Along (0.6, 0.8), a quarter turn on from which is (-0.8, 0.6).
      // Positive: 32 (0.6, 0.8) - 24 (-0.8, 0.6) = (38.4, 11.2).
      // Negative: 32 (0.6, 0.8) + 24 (-0.8, 0.6) = (0, 40)
      (
        Vec2::new(30.0, 40.0),
        Vec2::new(38.4, 11.2),
        Vec2::new(0.0, 40.0),
      ),
    ];
    for (rig, first, second, length) in rigs {
      for (target, positive, negative) in cases {
        for bend_positive in [true, false] {
          let what = format!("{rig}, target {target}, bend positive: {bend_positive}");
          let values = IkValues {
            bend_positive,
            ..values()
          };
          let (a, b) = solved((first, second), length, ORIGIN + target, values, false);
          assert_near(tip(&a, &b, length), ORIGIN + target, &what);
          let (at, cross) = if bend_positive {
            (positive, 1200.0)
          } else {
            (negative, -1200.0)
          };
          assert_near(joint(&a, &b), ORIGIN + at, &what);
          let turn = bend(&a, &b, length);
          assert!((turn - cross).abs() < 1.0, "{what}: a cross of {turn}");
          assert_only_turned(&first, &a, &what);
          assert_only_turned(&second, &b, &what);
        }
      }
    }
  }

  #[test]
  fn two_bones_reach_with_the_second_bone_off_the_first_s_axis() {
    // The second bone sits at (40, 12) in the first, so part one is
    // sqrt(40^2 + 12^2) = sqrt(1744) long and 16.7 degrees off the first
    // bone's x axis; part two is 30
    let first = bone(ORIGIN.x, ORIGIN.y, -0.4);
    let second = bone(40.0, 12.0, 0.6);
    for (what, target, bend_positive, at, cross) in [
      // (40, -12) is sqrt(1744) from the origin, and (58, 12) is
      // (18, 24), a length of 30, on from it. Part two is turned the
      // positive way from part one: 40 x 24 - (-12) x 18 = 1176
      (
        "positive",
        Vec2::new(58.0, 12.0),
        true,
        Vec2::new(40.0, -12.0),
        1176.0,
      ),
      // (12, 40) is sqrt(1744) from the origin, and (36, 22) is (24, -18),
      // a length of 30, on from it, turned the negative way:
      // 12 x (-18) - 40 x 24 = -1176
      (
        "negative",
        Vec2::new(36.0, 22.0),
        false,
        Vec2::new(12.0, 40.0),
        -1176.0,
      ),
    ] {
      let values = IkValues {
        bend_positive,
        ..values()
      };
      let (a, b) = solved((first, second), 30.0, ORIGIN + target, values, false);
      assert_near(tip(&a, &b, 30.0), ORIGIN + target, what);
      assert_near(joint(&a, &b), ORIGIN + at, what);
      let turn = bend(&a, &b, 30.0);
      assert!((turn - cross).abs() < 1.0, "{what}: a cross of {turn}");
      assert_only_turned(&first, &a, what);
      assert_only_turned(&second, &b, what);
      // The other bend reaches the same target with the joint on the
      // other side of the line to it
      let other = IkValues {
        bend_positive: !bend_positive,
        ..values
      };
      let (a, b) = solved((first, second), 30.0, ORIGIN + target, other, false);
      assert_near(tip(&a, &b, 30.0), ORIGIN + target, what);
      let turn = bend(&a, &b, 30.0);
      assert!((turn + cross).abs() < 1.0, "{what}: a cross of {turn}");
    }
  }

  #[test]
  fn a_target_out_of_reach_straightens_the_bones_toward_it() {
    // Parts of 40 and 30 reach 70. Toward a target farther off along
    // (0.6, 0.8) the bones lie straight: the joint 40 along, at (24, 32),
    // and the tip 70 along, at (42, 56), whichever way the joint bends
    let toward = Vec2::new(0.6, 0.8);
    for distance in [70.5, 100.0, 200.0, 1.0e4] {
      for bend_positive in [true, false] {
        let what = format!("{distance} away, bend positive: {bend_positive}");
        let values = IkValues {
          bend_positive,
          ..values()
        };
        let (a, b) = solved(limb(), 30.0, ORIGIN + toward * distance, values, false);
        assert_near(joint(&a, &b), ORIGIN + Vec2::new(24.0, 32.0), &what);
        assert_near(tip(&a, &b, 30.0), ORIGIN + Vec2::new(42.0, 56.0), &what);
        assert_only_turned(&limb().0, &a, &what);
        assert_only_turned(&limb().1, &b, &what);
      }
    }
    // With the second bone off the first's axis, at (40, 12), the first
    // part is sqrt(1744) long, and both parts lie along the line
    let (first, second) = (bone(ORIGIN.x, ORIGIN.y, 2.0), bone(40.0, 12.0, -1.0));
    let (a, b) = solved(
      (first, second),
      30.0,
      ORIGIN + toward * 300.0,
      values(),
      false,
    );
    let part = 1744.0_f32.sqrt();
    assert_near(joint(&a, &b), ORIGIN + toward * part, "off the axis");
    assert_near(
      tip(&a, &b, 30.0),
      ORIGIN + toward * (part + 30.0),
      "off the axis",
    );
    // And the same parts under a first bone scaled (2, 2)
    let (a, b) = solved(
      scaled_limb(),
      SCALED_LENGTH,
      ORIGIN + toward * 300.0,
      values(),
      false,
    );
    assert_near(
      joint(&a, &b),
      ORIGIN + toward * part,
      "off the axis, scaled",
    );
    assert_near(
      tip(&a, &b, SCALED_LENGTH),
      ORIGIN + toward * (part + 30.0),
      "off the axis, scaled",
    );
    assert_only_turned(&scaled_limb().0, &a, "off the axis, scaled");
    assert_only_turned(&scaled_limb().1, &b, "off the axis, scaled");
    // No jump as the target passes out of reach: a hair inside, the tip is
    // on the target, and a hair outside it is at full reach, 0.01 on
    let inside = solved(limb(), 30.0, ORIGIN + toward * 69.99, values(), false);
    let outside = solved(limb(), 30.0, ORIGIN + toward * 70.01, values(), false);
    let moved = tip(&inside.0, &inside.1, 30.0).distance(tip(&outside.0, &outside.1, 30.0));
    assert!(moved < 0.03, "the tip moved {moved}");
  }

  #[test]
  fn a_target_too_near_folds_the_bones() {
    // Parts of 40 and 30: folded, the tip is 10 from the origin, and comes
    // no nearer. Toward a target 5 to the right the first part points at
    // it and the second folds back along it: the joint 40 right, the tip 10
    let near = ORIGIN + Vec2::new(5.0, 0.0);
    // Parts of 30 and 40: the second is the longer, so the first points
    // away from the target and the second comes back past the origin: the
    // joint 30 left, the tip 10 right
    let long_second = (bone(ORIGIN.x, ORIGIN.y, 0.3), bone(30.0, 0.0, 0.9));
    for (what, rig, length, at) in [
      ("40 and 30", limb(), 30.0, Vec2::new(40.0, 0.0)),
      ("30 and 40", long_second, 40.0, Vec2::new(-30.0, 0.0)),
    ] {
      for bend_positive in [true, false] {
        let what = format!("{what}, bend positive: {bend_positive}");
        // Compress is one bone's: two bones fold, and their scales are left
        let values = IkValues {
          bend_positive,
          compress: true,
          stretch: true,
          ..values()
        };
        let (a, b) = solved(rig, length, near, values, true);
        assert_near(joint(&a, &b), ORIGIN + at, &what);
        assert_near(tip(&a, &b, length), ORIGIN + Vec2::new(10.0, 0.0), &what);
        assert_only_turned(&rig.0, &a, &what);
        assert_only_turned(&rig.1, &b, &what);
      }
    }
    // Off the axis under a first bone scaled (2, 2), parts of 41.7612 and
    // 30 fold to 11.7612. The first part is the longer, so it points at the
    // target: the joint 41.7612 right, the tip 11.7612
    for bend_positive in [true, false] {
      let what = format!("off the axis, scaled, bend positive: {bend_positive}");
      let values = IkValues {
        bend_positive,
        ..values()
      };
      let (a, b) = solved(scaled_limb(), SCALED_LENGTH, near, values, false);
      assert_near(joint(&a, &b), ORIGIN + Vec2::new(41.7612, 0.0), &what);
      assert_near(
        tip(&a, &b, SCALED_LENGTH),
        ORIGIN + Vec2::new(11.7612, 0.0),
        &what,
      );
      assert_only_turned(&scaled_limb().0, &a, &what);
      assert_only_turned(&scaled_limb().1, &b, &what);
    }
    // No jump as the target comes inside the fold: a hair outside, the tip
    // is on the target, and a hair inside it stays 10 away, 0.01 back
    let outside = solved(
      limb(),
      30.0,
      ORIGIN + Vec2::new(10.01, 0.0),
      values(),
      false,
    );
    let inside = solved(limb(), 30.0, ORIGIN + Vec2::new(9.99, 0.0), values(), false);
    let moved = tip(&inside.0, &inside.1, 30.0).distance(tip(&outside.0, &outside.1, 30.0));
    assert!(moved < 0.03, "the tip moved {moved}");
  }

  #[test]
  fn softness_slows_the_tip_as_the_bones_straighten() {
    // Parts of 40 and 30 reach 70. With a softness of 10 the tip follows
    // its target out to 60, and past that a target d away is reached for
    // as if it were 60 + 10 x (1 - e^(-(d - 60) / 10)) away
    let toward = Vec2::new(0.6, 0.8);
    let soft = IkValues {
      softness: 10.0,
      ..values()
    };
    let reached = |values: IkValues, distance: f32| {
      let (a, b) = solved(limb(), 30.0, ORIGIN + toward * distance, values, false);
      (tip(&a, &b, 30.0), bend(&a, &b, 30.0))
    };
    for (distance, softened) in [
      // Up to where it begins, the target itself
      (20.0, 20.0),
      (40.0, 40.0),
      (50.0, 50.0),
      (59.0, 59.0),
      (59.9, 59.9),
      // Past it: 60 + 10 x (1 - e^-0.01) = 60.0995, and so on with
      // e^-0.1 = 0.904837, e^-0.5 = 0.606531, e^-1 = 0.367879 and
      // e^-3 = 0.049787
      (60.1, 60.0995),
      (61.0, 60.9516),
      (65.0, 63.9347),
      (70.0, 66.3212),
      (90.0, 69.5021),
    ] {
      let (tip, _) = reached(soft, distance);
      let from_origin = tip.distance(ORIGIN);
      assert!(
        (from_origin - softened).abs() < 0.01,
        "{distance} away: the tip is {from_origin} from the origin, not {softened}"
      );
      // On the line to the target, short of the full reach
      assert_near(tip, ORIGIN + toward * softened, &format!("{distance} away"));
      assert!(from_origin < 70.0, "{distance} away: {from_origin}");
    }
    // Far out of reach the tip is still short of 70, 60 + 10 x (1 - e^-9) =
    // 69.9988, and the joint still bent to its side
    let (far, bend) = reached(soft, 150.0);
    let from_origin = far.distance(ORIGIN);
    assert!(
      (69.99..70.0).contains(&from_origin),
      "150 away: {from_origin}"
    );
    assert!(bend > 1.0, "150 away: a cross of {bend}");
    // Farther out than an f32 can tell the softened distance from 70 (20
    // softnesses past where it begins, e^-20 of 10 is 2e-8), the tip is at
    // the full reach
    let from_origin = reached(soft, 260.0).0.distance(ORIGIN);
    assert!(
      (from_origin - 70.0).abs() < 0.001,
      "260 away: {from_origin}"
    );

    // Off the axis under a first bone scaled (2, 2), parts of 41.7612 and
    // 30 reach 71.7612, so a softness of 10 begins at 61.7612. A target 70
    // away is reached for at 61.7612 + 10 x (1 - e^-0.823877) = 67.3740,
    // and one 90 away at 61.7612 + 10 x (1 - e^-2.823877) = 71.1675
    for (distance, softened) in [(50.0, 50.0), (61.0, 61.0), (70.0, 67.3740), (90.0, 71.1675)] {
      let what = format!("off the axis, scaled, {distance} away");
      let (a, b) = solved(
        scaled_limb(),
        SCALED_LENGTH,
        ORIGIN + toward * distance,
        soft,
        false,
      );
      let reached = tip(&a, &b, SCALED_LENGTH);
      assert_near(reached, ORIGIN + toward * softened, &what);
      assert!(reached.distance(ORIGIN) < 71.7612, "{what}");
    }

    // A softness above the reach is taken as the reach: it begins at the
    // origin, and a target d away is reached for as if 70 x (1 - e^(-d / 70))
    // away: 70 x (1 - e^-0.5) = 27.5429 and 70 x (1 - e^-1) = 44.2485
    let softer = IkValues {
      softness: 100.0,
      ..values()
    };
    for (distance, softened) in [(35.0, 27.5429), (70.0, 44.2485)] {
      let from_origin = reached(softer, distance).0.distance(ORIGIN);
      assert!(
        (from_origin - softened).abs() < 0.01,
        "softness 100, {distance} away: the tip is {from_origin} from the origin, not {softened}"
      );
    }
  }

  #[test]
  fn a_half_mix_turns_each_bone_half_way_the_short_way() {
    // Parts of 40 and 30 and a target 50 to the left: the 3-4-5 triangle,
    // bending the positive way. Along (-1, 0), a quarter turn on from which
    // is (0, -1), the joint is 32 along and 24 back: (-32, 24).
    //
    // Solved, the first bone points at the joint: 180 - 36.87 degrees, or
    // 2.49809 rad; and the second is a quarter turn on from the first,
    // 1.5708 rad in its own values.
    let first = bone(ORIGIN.x, ORIGIN.y, -2.9);
    let second = bone(40.0, 0.0, -2.5);
    let target = ORIGIN + Vec2::new(-50.0, 0.0);
    let at = |mix: f32| {
      solved(
        (first, second),
        30.0,
        target,
        IkValues { mix, ..values() },
        false,
      )
    };

    let (a, b) = at(1.0);
    assert_near(joint(&a, &b), ORIGIN + Vec2::new(-32.0, 24.0), "mix 1");
    assert_near(tip(&a, &b, 30.0), target, "mix 1");
    assert_angle(a.rotation, 2.49809, "mix 1");
    assert_angle(b.rotation, FRAC_PI_2, "mix 1");

    // From -2.9 rad the short way to 2.49809 is back through 180 degrees:
    // 2.49809 + 2.9 - 6.28319 = -0.88510, and half of it leaves the bone at
    // -2.9 - 0.44255 = -3.34255. The long way round, half would be
    // -2.9 + 2.69905 = -0.20095.
    // From -2.5 the short way to 1.5708 is 1.5708 + 2.5 - 6.28319 = -2.21239,
    // and half of it leaves the bone at -2.5 - 1.10619 = -3.60619
    let (a, b) = at(0.5);
    assert!(
      (a.rotation + 3.34255).abs() < 0.001,
      "the first bone is at {}",
      a.rotation
    );
    assert!(
      (b.rotation + 3.60619).abs() < 0.001,
      "the second bone is at {}",
      b.rotation
    );
    assert_only_turned(&first, &a, "mix 0.5");
    assert_only_turned(&second, &b, "mix 0.5");

    // No mix, or less than none, leaves both bones exactly as they were
    for mix in [0.0, -0.0, -1.0] {
      let (a, b) = at(mix);
      assert_eq!(
        (bits(&a), bits(&b)),
        (bits(&first), bits(&second)),
        "mix {mix}"
      );
    }
  }

  #[test]
  fn two_bones_stretch_to_a_target_out_of_reach() {
    let stretch = IkValues {
      stretch: true,
      ..values()
    };
    let (first, second) = limb();
    // On the axis: parts of 40 and 30 reach 70, and the target is 105
    // straight below, 1.5 times that. The first bone's scale_x becomes 1.5,
    // so the joint is 60 below and the tip 60 + 45 = 105: on the target
    let target = ORIGIN + Vec2::new(0.0, 105.0);
    let (a, b) = solved((first, second), 30.0, target, stretch, false);
    assert_near(joint(&a, &b), ORIGIN + Vec2::new(0.0, 60.0), "on the axis");
    assert_near(tip(&a, &b, 30.0), target, "on the axis");
    assert_close(a.scale_x, 1.5, "on the axis");
    assert_eq!(a.scale_y, 1.0, "on the axis");
    assert_only_turned(&second, &b, "on the axis");
    // Uniform scales across the first bone too
    let (a, b) = solved((first, second), 30.0, target, stretch, true);
    assert_near(tip(&a, &b, 30.0), target, "uniform on the axis");
    assert_close(a.scale_x, 1.5, "uniform on the axis");
    assert_close(a.scale_y, 1.5, "uniform on the axis");
    // At half mix the factor is 1 + (1.5 - 1) x 0.5 = 1.25
    let half = IkValues {
      mix: 0.5,
      ..stretch
    };
    let (a, _) = solved((first, second), 30.0, target, half, true);
    assert_close(a.scale_x, 1.25, "half mix");
    assert_close(a.scale_y, 1.25, "half mix");
    // A first bone mirrored by its own scale is even, and keeps its sign
    let mirrored = BonePose {
      scale_y: -1.0,
      ..first
    };
    let (a, b) = solved((mirrored, second), 30.0, target, stretch, true);
    assert_near(tip(&a, &b, 30.0), target, "mirrored");
    assert_close(a.scale_x, 1.5, "mirrored");
    assert_close(a.scale_y, -1.5, "mirrored");

    // Off the axis, with the second bone at (40, 12), uniform scales the
    // whole limb about the first bone's origin, so the tip is on the
    // target: the reach is sqrt(1744) + 30 = 71.7612, and a target 120 away
    // scales both axes by 120 / 71.7612 = 1.67221
    let off = bone(40.0, 12.0, 0.9);
    let target = ORIGIN + Vec2::new(72.0, 96.0);
    let (a, b) = solved((first, off), 30.0, target, stretch, true);
    assert_near(tip(&a, &b, 30.0), target, "off the axis");
    assert_close(a.scale_x, 1.67221, "off the axis");
    assert_close(a.scale_y, 1.67221, "off the axis");
    // Without uniform only the first bone's x scale grows. The limb still
    // lies along the line to the target, and falls a little short of it.
    // Straight, the joint in the first bone's own space is the second
    // bone's place, (40, 12), and the tip that times 1 + 30 / sqrt(1744),
    // (68.7348, 20.6204). With each x times 1.67221 they are (66.8885, 12),
    // which is 67.9564 long, and (114.939, 20.6204), which is 116.774 long:
    // 3.226 short of the 120. A first bone mirrored by its own scale, or
    // turned by equal shears, stretches the same
    let toward = Vec2::new(0.6, 0.8);
    for (what, first) in [
      ("off the axis, x alone", first),
      (
        "off the axis, x alone, mirrored",
        BonePose {
          scale_y: -1.0,
          ..first
        },
      ),
      (
        "off the axis, x alone, turned by its shears",
        BonePose {
          shear_x: 0.25,
          shear_y: 0.25,
          ..first
        },
      ),
    ] {
      let (a, b) = solved((first, off), 30.0, target, stretch, false);
      for (part, at, along) in [
        ("joint", joint(&a, &b), 67.9564),
        ("tip", tip(&a, &b, 30.0), 116.774),
      ] {
        let from_origin = at - ORIGIN;
        let aside = toward.perp_dot(from_origin);
        assert!(
          aside.abs() < 0.01,
          "{what}: the {part} is {aside} off the line to the target"
        );
        let reached = toward.dot(from_origin);
        assert!(
          (reached - along).abs() < 0.01,
          "{what}: the {part} is {reached} along the line, not {along}"
        );
      }
      assert_close(a.scale_x, 1.67221, what);
      assert_eq!(a.scale_y, first.scale_y, "{what}");
      assert_only_turned(&off, &b, what);
    }

    // Not applied, so the bones lie straight toward the target and stop
    // at their reach, the first bone's scale as it was:
    let target = ORIGIN + Vec2::new(0.0, 105.0);
    let unstretched = |what: &str, first: BonePose, values: IkValues| {
      let (a, b) = solved((first, second), 30.0, target, values, true);
      assert_only_turned(&first, &a, what);
      assert_only_turned(&second, &b, what);
      tip(&a, &b, 30.0).distance(ORIGIN)
    };
    // without the setting;
    let reach = unstretched("no stretch", first, values());
    assert!((reach - 70.0).abs() < 0.01, "no stretch: {reach}");
    // with any softness, which keeps the tip short of the reach;
    let soft = IkValues {
      softness: 5.0,
      ..stretch
    };
    let reach = unstretched("softness", first, soft);
    assert!(reach < 70.0, "softness: {reach}");
    // with a first bone whose scales differ by more than 1e-4;
    let uneven = BonePose {
      scale_x: 1.2,
      ..first
    };
    unstretched("an uneven first bone", uneven, stretch);
    let nearly = BonePose {
      scale_x: 1.001,
      ..first
    };
    unstretched("a first bone uneven by 0.001", nearly, stretch);
    // or one whose axes its shear takes out of square.
    let sheared = BonePose {
      shear_x: 0.3,
      ..first
    };
    unstretched("a sheared first bone", sheared, stretch);
    // Scales within 1e-4 of each other count as even
    let even = BonePose {
      scale_x: 1.00005,
      ..first
    };
    let (a, _) = solved((even, second), 30.0, target, stretch, false);
    assert!(a.scale_x > 1.4, "even within 1e-4: {}", a.scale_x);
    // And equal shears turn a bone without taking its axes out of square,
    // so it stretches, and its tip is on the target
    let turned = BonePose {
      shear_x: 0.25,
      shear_y: 0.25,
      ..first
    };
    let (a, b) = solved((turned, second), 30.0, target, stretch, false);
    assert_near(tip(&a, &b, 30.0), target, "equal shears");
    assert_close(a.scale_x, 1.5, "equal shears");
    // So do shears half a turn apart: they flip one axis, as a scale of
    // (1, -1) does, and leave the two square
    let flipped = BonePose {
      shear_y: PI,
      ..first
    };
    let (a, b) = solved((flipped, second), 30.0, target, stretch, false);
    assert_near(tip(&a, &b, 30.0), target, "shears half a turn apart");
    assert_close(a.scale_x, 1.5, "shears half a turn apart");
    // A target within reach is reached without stretching
    let within = ORIGIN + Vec2::new(0.0, 50.0);
    let (a, b) = solved((first, second), 30.0, within, stretch, true);
    assert_only_turned(&first, &a, "within reach");
    assert_near(tip(&a, &b, 30.0), within, "within reach");
  }

  #[test]
  fn a_mix_above_one_is_taken_as_one() {
    // One bone of length 30 scaled (2, 3), compressed with uniform toward a
    // target 12 away, a fifth of its reach of 60. At a mix of 1 its scales
    // are a fifth of what they were. Were a mix of 1.5 taken as written,
    // the factor would be 1 + (0.2 - 1) x 1.5 = -0.2, and the bone mirrored
    let start = BonePose {
      scale_x: 2.0,
      scale_y: 3.0,
      ..bone(ORIGIN.x, ORIGIN.y, 0.4)
    };
    let near = ORIGIN + Vec2::new(0.0, 12.0);
    let compress = IkValues {
      compress: true,
      ..values()
    };
    let full = pointed(start, 30.0, near, compress, true);
    assert_close(full.scale_x, 0.4, "one bone, mix 1");
    assert_close(full.scale_y, 0.6, "one bone, mix 1");
    let mixes = [1.5, 4.0, 1.0e30, f32::MAX];
    for mix in mixes {
      let over = pointed(start, 30.0, near, IkValues { mix, ..compress }, true);
      assert_eq!(bits(&over), bits(&full), "one bone, mix {mix}");
    }

    // Two bones, stretched toward a target 105 away, 1.5 times their reach:
    // the rotations and the first bone's scales are those of a mix of 1
    let far = ORIGIN + Vec2::new(63.0, 84.0);
    let stretch = IkValues {
      stretch: true,
      ..values()
    };
    let full = solved(limb(), 30.0, far, stretch, true);
    assert_close(full.0.scale_x, 1.5, "two bones, mix 1");
    for mix in mixes {
      let over = solved(limb(), 30.0, far, IkValues { mix, ..stretch }, true);
      assert_eq!(
        (bits(&over.0), bits(&over.1)),
        (bits(&full.0), bits(&full.1)),
        "two bones, mix {mix}"
      );
    }
    // And where the second bone sits on the first's origin
    let on_origin = (limb().0, bone(0.0, 0.0, 0.9));
    let full = solved(on_origin, 30.0, far, values(), false);
    for mix in mixes {
      let over = solved(on_origin, 30.0, far, IkValues { mix, ..values() }, false);
      assert_eq!(
        bits(&over.1),
        bits(&full.1),
        "joint on the origin, mix {mix}"
      );
    }
  }

  #[test]
  fn the_bend_side_holds_and_nothing_jumps_as_the_target_moves() {
    // Three walks of a target, each given from the first bone's origin:
    // - across the line at 0.3 rad, which the unit limb's first bone lies
    //   along as it starts: 50 along it, from 20 to one side of it to 20 to
    //   the other;
    // - round a full circle 50 away, half a degree a step;
    // - straight out along (0.6, 0.8) from 2 away to 100 away, through the
    //   fold (10 away, or 11.76) and the full reach (70, or 71.76).
    let line = Vec2::from_angle(0.3);
    let across: Vec<Vec2> = (0..=160)
      .map(|step| line * 50.0 + line.perp() * (step as f32 * 0.25 - 20.0))
      .collect();
    let round: Vec<Vec2> = (0..=720)
      .map(|step| Vec2::from_angle((step as f32 * 0.5).to_radians()) * 50.0)
      .collect();
    let out: Vec<Vec2> = (0..=392)
      .map(|step| Vec2::new(0.6, 0.8) * (2.0 + step as f32 * 0.25))
      .collect();
    // On two limbs: the unit limb (parts of 40 and 30), and the limb off
    // the axis under a first bone scaled (2, 2) (parts of 41.76 and 30)
    let rigs = [
      ("the unit limb", limb(), 30.0, 10.0, 70.0),
      (
        "the scaled limb",
        scaled_limb(),
        SCALED_LENGTH,
        11.7612,
        71.7612,
      ),
    ];
    for (rig, bones, length, fold, reach) in rigs {
      for (walk, path) in [("across", &across), ("round", &round), ("out", &out)] {
        for bend_positive in [true, false] {
          for softness in [0.0, 8.0] {
            let values = IkValues {
              bend_positive,
              softness,
              ..values()
            };
            let side = if bend_positive { 1.0 } else { -1.0 };
            let mut last: Option<(Vec2, Vec2)> = None;
            for &from_origin in path {
              let what = format!(
                "{rig}, {walk}, bend positive: {bend_positive}, softness {softness}, target {from_origin}"
              );
              let target = ORIGIN + from_origin;
              let (a, b) = solved(bones, length, target, values, false);
              let tip = tip(&a, &b, length);
              // The joint is never on the other side of the limb; and it is
              // well on its own side wherever the limb is neither folded
              // nor straight
              let turn = bend(&a, &b, length) * side;
              assert!(turn > -0.05, "{what}: a cross of {turn}");
              let distance = from_origin.length();
              if distance > fold + 1.0 && (softness > 0.0 || distance < reach - 1.0) {
                assert!(turn > 1.0, "{what}: a cross of {turn}");
              }
              // The tip moves no farther than its target did
              if let Some((target_before, tip_before)) = last {
                let (moved, step) = (tip.distance(tip_before), target.distance(target_before));
                assert!(
                  moved <= step + 0.001,
                  "{what}: the tip moved {moved} for a step of {step}"
                );
              }
              last = Some((target, tip));
            }
          }
        }
      }
    }
  }

  #[test]
  fn a_part_mix_jumps_when_the_solved_turn_passes_half_a_turn() {
    // The recorded limit. A bone at 0 rad whose target passes behind it:
    // 0.01 rad short of half a turn, the short way to it is the positive
    // way; 0.01 rad past it, the negative way. At a mix of 1 the two poses
    // are 0.02 rad apart. At a part mix the bone goes part of each way, so
    // the two are (2 pi - 0.02) x mix apart: the bone jumps
    let start = bone(ORIGIN.x, ORIGIN.y, 0.0);
    let before = ORIGIN + Vec2::from_angle(PI - 0.01) * 50.0;
    let after = ORIGIN + Vec2::from_angle(0.01 - PI) * 50.0;
    for mix in [0.25, 0.5, 0.75] {
      let values = IkValues { mix, ..values() };
      let short = pointed(start, 30.0, before, values, false).rotation;
      let past = pointed(start, 30.0, after, values, false).rotation;
      assert_close_to(
        short,
        (PI - 0.01) * mix,
        0.001,
        &format!("mix {mix}, short"),
      );
      assert_close_to(past, (0.01 - PI) * mix, 0.001, &format!("mix {mix}, past"));
      assert_close_to(
        short - past,
        (TAU - 0.02) * mix,
        0.002,
        &format!("mix {mix}, the jump"),
      );
    }
    // At a mix of 1 nothing jumps: the two tips are 0.02 rad apart on a
    // circle of 30, 0.6
    let short = pointed(start, 30.0, before, values(), false);
    let past = pointed(start, 30.0, after, values(), false);
    let apart = end(&short, 30.0).distance(end(&past, 30.0));
    assert_close_to(apart, 0.6, 0.01, "mix 1");

    // Two bones the same. The unit limb's first bone starts at 0 rad, and
    // solved toward a target 50 away it lies 0.6435 rad (the 3-4-5
    // triangle's small angle) back from the line to it. So it passes half a
    // turn from where it started as the target passes pi + 0.6435 rad
    let bones = (bone(ORIGIN.x, ORIGIN.y, 0.0), bone(40.0, 0.0, 0.9));
    let before = ORIGIN + Vec2::from_angle(PI + 0.6435 - 0.01) * 50.0;
    let after = ORIGIN + Vec2::from_angle(PI + 0.6435 + 0.01) * 50.0;
    let half = IkValues {
      mix: 0.5,
      ..values()
    };
    let short = solved(bones, 30.0, before, half, false);
    let past = solved(bones, 30.0, after, half, false);
    assert_close_to(
      short.0.rotation,
      (PI - 0.01) * 0.5,
      0.001,
      "two bones, short",
    );
    assert_close_to(past.0.rotation, (0.01 - PI) * 0.5, 0.001, "two bones, past");
    // The joints are 40 from the origin and nearly opposite: nearly 80 apart
    let apart = joint(&short.0, &short.1).distance(joint(&past.0, &past.1));
    assert!(
      apart > 79.0,
      "two bones, mix 0.5: the joints are {apart} apart"
    );
    // At a mix of 1 the joints are 0.02 rad apart on a circle of 40, 0.8
    let short = solved(bones, 30.0, before, values(), false);
    let past = solved(bones, 30.0, after, values(), false);
    let apart = joint(&short.0, &short.1).distance(joint(&past.0, &past.1));
    assert_close_to(apart, 0.8, 0.01, "two bones, mix 1");
  }

  #[test]
  fn a_first_bone_mirrored_by_its_own_scale_keeps_the_bend_side() {
    // The rig with the second bone off the axis, at (40, 12). Unmirrored,
    // a positive bend toward (58, 12) puts the joint at (40, -12) and a
    // negative bend toward (36, 22) puts it at (12, 40), as worked in
    // `two_bones_reach_with_the_second_bone_off_the_first_s_axis`. A first
    // bone mirrored by its own scale, either axis, draws the limb mirrored
    // in itself, but the joint is on the same side of the line to the
    // target in its parent's space
    let second = bone(40.0, 12.0, 0.3);
    for (scale_x, scale_y) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0)] {
      let first = BonePose {
        scale_x,
        scale_y,
        ..bone(ORIGIN.x, ORIGIN.y, 0.7)
      };
      for (target, bend_positive, at, cross) in [
        (Vec2::new(58.0, 12.0), true, Vec2::new(40.0, -12.0), 1176.0),
        (Vec2::new(36.0, 22.0), false, Vec2::new(12.0, 40.0), -1176.0),
      ] {
        let what = format!("scale ({scale_x}, {scale_y}), bend positive: {bend_positive}");
        let values = IkValues {
          bend_positive,
          ..values()
        };
        let (a, b) = solved((first, second), 30.0, ORIGIN + target, values, false);
        assert_near(tip(&a, &b, 30.0), ORIGIN + target, &what);
        assert_near(joint(&a, &b), ORIGIN + at, &what);
        let turn = bend(&a, &b, 30.0);
        assert!((turn - cross).abs() < 1.0, "{what}: a cross of {turn}");
        assert_only_turned(&first, &a, &what);
        assert_only_turned(&second, &b, &what);
      }
    }
  }

  /// One input of a solve, by number: the first bone's seven values, the
  /// second's, the target's two, the length, the mix and the softness
  const INPUTS: usize = 19;

  #[derive(Clone, Copy)]
  struct Inputs {
    first: BonePose,
    second: BonePose,
    target: Vec2,
    length: f32,
    values: IkValues,
  }

  impl Inputs {
    fn input(&mut self, index: usize) -> &mut f32 {
      fn value(bone: &mut BonePose, index: usize) -> &mut f32 {
        match index {
          0 => &mut bone.x,
          1 => &mut bone.y,
          2 => &mut bone.rotation,
          3 => &mut bone.scale_x,
          4 => &mut bone.scale_y,
          5 => &mut bone.shear_x,
          _ => &mut bone.shear_y,
        }
      }
      match index {
        0..=6 => value(&mut self.first, index),
        7..=13 => value(&mut self.second, index - 7),
        14 => &mut self.target.x,
        15 => &mut self.target.y,
        16 => &mut self.length,
        17 => &mut self.values.mix,
        _ => &mut self.values.softness,
      }
    }

    /// Both solves on these inputs. Nothing comes out that is not finite
    /// unless it went in so, and a solve given an input that is not finite
    /// changes nothing (the second bone's values are not one bone's inputs)
    fn check(&self, what: &dyn Fn() -> String) {
      let mut inputs = *self;
      let mut finite = |of: &dyn Fn(usize) -> bool| {
        (0..INPUTS).all(|index| !of(index) || inputs.input(index).is_finite())
      };
      let (two, one) = (
        finite(&|_| true),
        finite(&|index| !(7..=13).contains(&index)),
      );
      let (a, b) = solved(
        (self.first, self.second),
        self.length,
        self.target,
        self.values,
        true,
      );
      let lone = pointed(self.first, self.length, self.target, self.values, true);
      for (solve, finite, before, after) in [
        ("two bones, the first", two, &self.first, &a),
        ("two bones, the second", two, &self.second, &b),
        ("one bone", one, &self.first, &lone),
      ] {
        for (was, is) in bits(before).into_iter().zip(bits(after)) {
          assert!(
            was == is || (finite && f32::from_bits(is).is_finite()),
            "{}: {solve} went from {before:?} to {after:?}",
            what()
          );
        }
      }
    }
  }

  #[test]
  fn a_target_at_the_first_bone_s_origin_changes_nothing_and_makes_no_nan() {
    let (first, second) = limb();
    let every = IkValues {
      stretch: true,
      compress: true,
      ..values()
    };
    let unchanged = |what: &str, first: BonePose, second: BonePose, length: f32, target: Vec2| {
      let (a, b) = solved((first, second), length, target, every, true);
      assert_eq!(
        (bits(&a), bits(&b)),
        (bits(&first), bits(&second)),
        "two bones, {what}"
      );
    };

    // A target on the first bone's origin gives no way to turn
    unchanged("a target on the origin", first, second, 30.0, ORIGIN);
    let one = pointed(first, 30.0, ORIGIN, every, true);
    assert_eq!(bits(&one), bits(&first), "one bone, a target on the origin");
    // Nor does one within a millionth of a unit of it
    let at_zero = bone(0.0, 0.0, 0.3);
    let beside = Vec2::new(5.0e-7, -3.0e-7);
    unchanged("a target beside the origin", at_zero, second, 30.0, beside);
    let one = pointed(at_zero, 30.0, beside, every, true);
    assert_eq!(
      bits(&one),
      bits(&at_zero),
      "one bone, a target beside the origin"
    );

    // A second bone on the first's origin: the joint is on the origin
    // whatever the first bone does, so the first is left exactly as it is
    // and the second points at the target, its tip 30 toward it
    let on_origin = bone(0.0, 0.0, 0.9);
    let target = ORIGIN + Vec2::new(0.0, 50.0);
    let (a, b) = solved((first, on_origin), 30.0, target, every, true);
    assert_eq!(bits(&a), bits(&first), "a second bone on the origin");
    assert_only_turned(&on_origin, &b, "a second bone on the origin");
    assert_near(
      tip(&a, &b, 30.0),
      ORIGIN + Vec2::new(0.0, 30.0),
      "a second bone on the origin",
    );

    // A zero scale that leaves a part no length, or the first bone flat,
    // leaves nothing to solve
    for (what, scale_x, scale_y) in [
      ("a first bone scaled to nothing", 0.0, 0.0),
      ("a first bone flat along x", 0.0, 1.0),
      ("a first bone flat along y", 1.0, 0.0),
      ("a first bone all but flat", 1.0e-13, 1.0),
    ] {
      let flat = BonePose {
        scale_x,
        scale_y,
        ..first
      };
      unchanged(what, flat, second, 30.0, target);
      unchanged(what, flat, bone(40.0, 12.0, 0.9), 30.0, target);
    }
    let no_x = BonePose {
      scale_x: 0.0,
      ..second
    };
    unchanged("a second bone with no x scale", first, no_x, 30.0, target);
    unchanged("a tip with no length", first, second, 0.0, target);
    unchanged("a tip with a negative length", first, second, -30.0, target);
    // One bone with no x scale has no reach to stretch or compress, so its
    // scale is left; it still turns, as pointing needs no length
    let flat = BonePose {
      scale_x: 0.0,
      ..first
    };
    let one = pointed(flat, 30.0, target, every, true);
    assert_only_turned(&flat, &one, "one bone with no x scale");
    assert_angle(one.rotation, FRAC_PI_2, "one bone with no x scale");
    let one = pointed(first, 0.0, target, every, true);
    assert_only_turned(&first, &one, "one bone with no length");

    // Whatever goes in, alone or in pairs: nothing comes out that is not
    // finite, and an input that is not finite changes nothing
    let extremes = [
      0.0,
      -0.0,
      f32::MIN_POSITIVE,
      1.0e-30,
      -1.0e-30,
      1.0e-6,
      1.0,
      -1.0,
      1.0e30,
      -1.0e30,
      f32::MAX,
      f32::NAN,
      f32::INFINITY,
      f32::NEG_INFINITY,
    ];
    for values in [
      // Softness, which keeps stretch out; stretch and compress on their
      // own; the other bend
      IkValues {
        softness: 4.0,
        ..every
      },
      every,
      IkValues {
        bend_positive: false,
        mix: 0.5,
        ..every
      },
    ] {
      let inputs = Inputs {
        first,
        second: bone(40.0, 12.0, 0.9),
        target: ORIGIN + Vec2::new(30.0, 40.0),
        length: 30.0,
        values,
      };
      for one in 0..INPUTS {
        for one_value in extremes {
          for other in one..INPUTS {
            for other_value in extremes {
              let mut inputs = inputs;
              *inputs.input(one) = one_value;
              *inputs.input(other) = other_value;
              inputs.check(&|| {
                format!("input {one} = {one_value:e}, input {other} = {other_value:e}, {values:?}")
              });
            }
          }
        }
      }
    }
  }

  /// How far the tip is from the first bone's origin with the limb folded,
  /// and with it straight: the second bone (unsheared, its scale above 0)
  /// turned back along its own place in the first, and along it
  fn reaches(first: &BonePose, second: &BonePose, length: f32) -> (f32, f32) {
    let along = Vec2::new(second.x, second.y).to_angle();
    let origin = Vec2::new(first.x, first.y);
    let at = |rotation: f32| {
      let turned = BonePose {
        rotation,
        ..*second
      };
      tip(first, &turned, length).distance(origin)
    };
    (at(along + PI), at(along))
  }

  /// The worst miss of a limb of length 30 under a first bone scaled and
  /// sheared as `first` is: over a second bone on the first's axis, at
  /// (40, 0), and off it, at (40, 12); two starting poses; and both bend
  /// sides, toward targets in eight directions, `fraction` of the way from
  /// the folded reach to the straight reach. Every pose solved must be
  /// finite, bent to the side asked for, and changed in its rotations alone
  fn worst_miss(what: &str, first: BonePose, fraction: f32) -> f32 {
    let mut worst = 0.0_f32;
    for place in [Vec2::new(40.0, 0.0), Vec2::new(40.0, 12.0)] {
      for (rotation, second_rotation) in [(0.3, 0.9), (-2.0, -2.2)] {
        let first = BonePose { rotation, ..first };
        let second = bone(place.x, place.y, second_rotation);
        let (folded, straight) = reaches(&first, &second, 30.0);
        let distance = folded + (straight - folded) * fraction;
        for eighth in 0..8 {
          let target = ORIGIN + Vec2::from_angle(eighth as f32 * FRAC_PI_4) * distance;
          for bend_positive in [true, false] {
            let what = format!(
              "{what}, second bone at {place}, from {rotation}, {fraction} of the way, target {target}, bend positive: {bend_positive}"
            );
            let values = IkValues {
              bend_positive,
              ..values()
            };
            let (a, b) = solved((first, second), 30.0, target, values, false);
            assert!(
              bits(&a)
                .into_iter()
                .chain(bits(&b))
                .all(|value| f32::from_bits(value).is_finite()),
              "{what}: {a:?} {b:?}"
            );
            let turn = bend(&a, &b, 30.0) * if bend_positive { 1.0 } else { -1.0 };
            assert!(turn > -1.0, "{what}: a cross of {turn}");
            assert_only_turned(&first, &a, &what);
            assert_only_turned(&second, &b, &what);
            worst = worst.max(tip(&a, &b, 30.0).distance(target));
          }
        }
      }
    }
    worst
  }

  #[test]
  fn an_uneven_first_bone_reaches_its_target_within_a_bound() {
    // Under a first bone whose own scale is uneven, or whose axes are
    // sheared out of square, the second part's length changes as the second
    // bone turns. So the triangle is solved in passes, each with that
    // length measured anew where the pass before left the bone. The passes
    // close on a target the limb reaches well bent. Near the folded and the
    // straight reach they close slowly, or not at all
    let uneven = |scale_x: f32, scale_y: f32, shear_x: f32| BonePose {
      scale_x,
      scale_y,
      shear_x,
      ..bone(ORIGIN.x, ORIGIN.y, 0.0)
    };
    // Each first bone with the miss allowed at three places between the
    // folded reach and the straight reach: half-way, 0.3 and 0.7 of the
    // way, and a tenth of the way in from either end, where one can be
    // promised
    let bones = [
      (
        "a scale of (1.2, 1)",
        uneven(1.2, 1.0, 0.0),
        0.01,
        Some(0.01),
        Some(0.5),
      ),
      (
        "a scale of (1.5, 1)",
        uneven(1.5, 1.0, 0.0),
        0.01,
        Some(0.3),
        None,
      ),
      (
        "a scale of (2, 1)",
        uneven(2.0, 1.0, 0.0),
        0.01,
        Some(5.0),
        None,
      ),
      ("a scale of (3, 1)", uneven(3.0, 1.0, 0.0), 0.01, None, None),
      (
        "a scale of (1, 1.5)",
        uneven(1.0, 1.5, 0.0),
        0.1,
        Some(0.2),
        Some(1.5),
      ),
      (
        "an x shear of 0.3",
        uneven(1.0, 1.0, 0.3),
        0.03,
        Some(0.3),
        None,
      ),
    ];
    for (what, first, half_way, middle, ends) in bones {
      for (fractions, allowed) in [
        (&[0.5][..], Some(half_way)),
        (&[0.3, 0.7], middle),
        (&[0.1, 0.9], ends),
        // At either reach, and beyond the straight one, nothing is
        // promised but a finite pose bent to its side
        (&[0.0, 1.0, 1.3], None),
      ] {
        for &fraction in fractions {
          let miss = worst_miss(what, first, fraction);
          if let Some(allowed) = allowed {
            assert!(
              miss < allowed,
              "{what}: a miss of {miss} {fraction} of the way, above {allowed}"
            );
          }
        }
      }
    }

    // The limit that stands: a tenth of the way in from the straight reach,
    // the passes do not bring a limb under a first bone scaled (1.5, 1) to
    // its target
    let miss = worst_miss("a scale of (1.5, 1)", uneven(1.5, 1.0, 0.0), 0.9);
    assert!(miss > 1.0, "a scale of (1.5, 1), 0.9 of the way: {miss}");

    // Where the passes close, the tip follows its target: half-way along
    // under a first bone scaled (3, 1), a target stepped 0.01 in any of
    // eight directions moves the tip 0.01, and under 0.015
    let first = BonePose {
      rotation: 0.3,
      ..uneven(3.0, 1.0, 0.0)
    };
    let second = bone(40.0, 12.0, 0.9);
    let (folded, straight) = reaches(&first, &second, 30.0);
    let centre = ORIGIN + Vec2::new(0.6, 0.8) * ((folded + straight) * 0.5);
    let reached = |target: Vec2| {
      let (a, b) = solved((first, second), 30.0, target, values(), false);
      tip(&a, &b, 30.0)
    };
    for eighth in 0..8 {
      let step = Vec2::from_angle(eighth as f32 * FRAC_PI_4) * 0.01;
      let moved = reached(centre + step).distance(reached(centre));
      assert!(
        moved < 0.015,
        "a scale of (3, 1): the tip moved {moved} for a step of 0.01"
      );
    }
  }
}
