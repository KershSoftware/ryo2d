//! Transform constraints: a bone copies a target bone's rotation, place,
//! scale and shear, each by its own mix.
//!
//! The functions here are pure. One works on transforms in the skeleton's
//! space (the world modes), the other on bones' own values (the local
//! modes). They know nothing of a pose or a skeleton: reading the target,
//! taking the bones in order and placing them again afterwards is the
//! caller's (`constrain.rs`).

use std::f32::consts::FRAC_PI_2;

use glam::{Affine2, Mat2, Vec2};

use super::apply::shortest;
use super::constraint::{TransformMix, TransformOffset};
use super::pose::BonePose;

/// Below this an axis counts as having no length: its direction, and the
/// angle between it and the other axis, are not known
const NO_LENGTH: f32 = 1e-6;

/// How near a quarter turn a shear set in a world mode may come, in
/// radians: nearer, the two axes would lie along one line, and past it the
/// bone would be mirrored
const SHEAR_ROOM: f32 = 1e-3;

/// A transform's linear part as a world mode reads it
#[derive(Debug, Clone, Copy)]
struct Axes {
  /// The way the x axis points
  rotation: f32,
  /// The x axis's length
  scale_x: f32,
  /// The y axis's length
  scale_y: f32,
  /// How far the y axis is from square to the x axis, on the transform's
  /// own side: positive when the axes are farther apart than a quarter
  /// turn
  shear: f32,
  /// 1, or -1 for a mirrored transform (its determinant is negative)
  side: f32,
}

impl Axes {
  /// `None` for a flat transform: an axis shorter than [`NO_LENGTH`]
  fn of(linear: Mat2) -> Option<Self> {
    let (x, y) = (linear.x_axis, linear.y_axis);
    let (scale_x, scale_y) = (x.length(), y.length());
    if !(scale_x >= NO_LENGTH && scale_y >= NO_LENGTH) {
      return None;
    }
    let side = if x.perp_dot(y) < 0.0 { -1.0 } else { 1.0 };
    Some(Self {
      rotation: x.to_angle(),
      scale_x,
      scale_y,
      shear: side * between(x, y) - FRAC_PI_2,
      side,
    })
  }
}

/// Moves `bone`, a transform in the skeleton's space, toward `target`'s by
/// `mix`, or adds `target`'s to it when `relative`: each of its place,
/// rotation, axes' lengths and shear by its own mix, a mix of 0 leaving
/// that property as it was to the bit.
///
/// - Place: toward the point `(offset.x, offset.y)` in `target`'s space;
///   relative, that point times the mix is added.
/// - Rotation: the bone is turned, both axes together, the short way toward
///   the way `target`'s x axis points plus `offset.rotation`; relative, by
///   that angle times the mix.
/// - Each axis's length moves toward `target`'s plus the offset; relative,
///   it is multiplied by `1 + (target's - 1 + offset) × mix`. A length is
///   never below 0.
/// - Shear (how far the y axis is from square to the x axis, on the
///   transform's own side) moves toward `target`'s plus `offset.shear_y`,
///   turning the y axis alone; relative, that times the mix is added. It
///   is kept within [`SHEAR_ROOM`] of a quarter turn either way.
///
/// Under a mirrored `target` the rotation and shear offsets change sign.
/// The bone's own mirroring is never changed. A flat bone or target (an
/// axis shorter than 1e-6) leaves rotation, scale and shear alone; any
/// input, or result, that is not finite leaves the bone as it was
pub(crate) fn in_the_world(
  bone: &mut Affine2,
  target: &Affine2,
  mix: &TransformMix,
  offset: &TransformOffset,
  relative: bool,
) {
  let finite = bone.is_finite() && target.is_finite() && mix_finite(mix) && offset_finite(offset);
  if !finite {
    return;
  }
  let mut out = *bone;
  let point = target.transform_point2(Vec2::new(offset.x, offset.y));
  let place = |value: f32, goal: f32, mix: f32| {
    if relative {
      value + goal * mix
    } else {
      value + (goal - value) * mix
    }
  };
  if mix.x != 0.0 {
    out.translation.x = place(out.translation.x, point.x, mix.x);
  }
  if mix.y != 0.0 {
    out.translation.y = place(out.translation.y, point.y, mix.y);
  }
  let linear = [mix.rotate, mix.scale_x, mix.scale_y, mix.shear_y];
  if linear.iter().any(|&mix| mix != 0.0)
    && let (Some(own), Some(goal)) = (Axes::of(bone.matrix2), Axes::of(target.matrix2))
  {
    // A mirrored target mirrors the angles added to it
    let rotation = goal.rotation + goal.side * offset.rotation;
    let shear = goal.shear + goal.side * offset.shear_y;
    let mut axes = out.matrix2;
    if mix.rotate != 0.0 {
      let turn = if relative {
        rotation * mix.rotate
      } else {
        shortest(rotation - own.rotation) * mix.rotate
      };
      axes = Mat2::from_angle(turn) * axes;
    }
    if mix.shear_y != 0.0 {
      // The y axis is set at its angle from the x axis, on the bone's own
      // side, and keeps its length
      let sheared = place(own.shear, shear, mix.shear_y);
      let room = FRAC_PI_2 - SHEAR_ROOM;
      let sheared = sheared.clamp(-room, room);
      let along = axes.x_axis.normalize_or_zero();
      let apart = Vec2::from_angle(own.side * (sheared + FRAC_PI_2));
      axes.y_axis = apart.rotate(along) * axes.y_axis.length();
    }
    if mix.scale_x != 0.0 {
      let length = scaled(
        own.scale_x,
        goal.scale_x + offset.scale_x,
        mix.scale_x,
        relative,
      );
      axes.x_axis = axes.x_axis * (length / axes.x_axis.length());
    }
    if mix.scale_y != 0.0 {
      let length = scaled(
        own.scale_y,
        goal.scale_y + offset.scale_y,
        mix.scale_y,
        relative,
      );
      axes.y_axis = axes.y_axis * (length / axes.y_axis.length());
    }
    out.matrix2 = axes;
  }
  if out.is_finite() {
    *bone = out;
  }
}

/// The angle from `x` to `y`, signed. By the arctangent, which is exact
/// at every angle, where an arccosine loses its precision near none and
/// near half a turn
fn between(x: Vec2, y: Vec2) -> f32 {
  x.perp_dot(y).atan2(x.dot(y))
}

/// An axis's length moved toward `goal` by `mix`, or multiplied by
/// `1 + (goal - 1) × mix` when `relative`: never below 0, so an axis is
/// never turned round
fn scaled(length: f32, goal: f32, mix: f32, relative: bool) -> f32 {
  let length = if relative {
    length * (1.0 + (goal - 1.0) * mix)
  } else {
    length + (goal - length) * mix
  };
  length.max(0.0)
}

/// Moves `bone`'s own values toward `target`'s plus `offset` by `mix`:
/// `value += (target's + offset - value) × mix` for its rotation, x, y,
/// scales and y shear. Relative, the rotation, x, y and y shear add
/// `(target's + offset) × mix`, and each scale is multiplied by
/// `1 + (target's - 1 + offset) × mix`. A rotation is a plain number, not
/// taken the short way round: it is what keys set, and may pass a full
/// turn. A mix of 0 leaves its property as it was to the bit; any input,
/// or result, that is not finite leaves the bone as it was
pub(crate) fn in_its_own_values(
  bone: &mut BonePose,
  target: &BonePose,
  mix: &TransformMix,
  offset: &TransformOffset,
  relative: bool,
) {
  let finite = pose_finite(bone) && pose_finite(target) && mix_finite(mix) && offset_finite(offset);
  if !finite {
    return;
  }
  let mut out = *bone;
  let add = |value: &mut f32, goal: f32, mix: f32| {
    if mix != 0.0 {
      *value = if relative {
        *value + goal * mix
      } else {
        *value + (goal - *value) * mix
      };
    }
  };
  let scale = |value: &mut f32, goal: f32, mix: f32| {
    if mix != 0.0 {
      *value = if relative {
        *value * (1.0 + (goal - 1.0) * mix)
      } else {
        *value + (goal - *value) * mix
      };
    }
  };
  add(
    &mut out.rotation,
    target.rotation + offset.rotation,
    mix.rotate,
  );
  add(&mut out.x, target.x + offset.x, mix.x);
  add(&mut out.y, target.y + offset.y, mix.y);
  scale(
    &mut out.scale_x,
    target.scale_x + offset.scale_x,
    mix.scale_x,
  );
  scale(
    &mut out.scale_y,
    target.scale_y + offset.scale_y,
    mix.scale_y,
  );
  add(
    &mut out.shear_y,
    target.shear_y + offset.shear_y,
    mix.shear_y,
  );
  if pose_finite(&out) {
    *bone = out;
  }
}

fn mix_finite(mix: &TransformMix) -> bool {
  let TransformMix {
    rotate,
    x,
    y,
    scale_x,
    scale_y,
    shear_y,
  } = *mix;
  [rotate, x, y, scale_x, scale_y, shear_y]
    .iter()
    .all(|value| value.is_finite())
}

fn offset_finite(offset: &TransformOffset) -> bool {
  let TransformOffset {
    rotation,
    x,
    y,
    scale_x,
    scale_y,
    shear_y,
  } = *offset;
  [rotation, x, y, scale_x, scale_y, shear_y]
    .iter()
    .all(|value| value.is_finite())
}

fn pose_finite(bone: &BonePose) -> bool {
  let BonePose {
    x,
    y,
    rotation,
    scale_x,
    scale_y,
    shear_x,
    shear_y,
  } = *bone;
  [x, y, rotation, scale_x, scale_y, shear_x, shear_y]
    .iter()
    .all(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
  use std::f32::consts::{PI, TAU};

  use super::*;

  /// A transform as a root bone of these own values is placed: its x axis
  /// `scale_x` long at `rotation`, its y axis `scale_y` long at
  /// `rotation + shear` plus a quarter turn (a negative `scale_y` mirrors
  /// it)
  fn placed(x: f32, y: f32, rotation: f32, scale_x: f32, scale_y: f32, shear: f32) -> Affine2 {
    own(x, y, rotation, scale_x, scale_y, shear).affine()
  }

  fn own(x: f32, y: f32, rotation: f32, scale_x: f32, scale_y: f32, shear_y: f32) -> BonePose {
    BonePose {
      x,
      y,
      rotation,
      scale_x,
      scale_y,
      shear_x: 0.0,
      shear_y,
    }
  }

  /// The bone the world tests move: at (10, 20), turned 0.3, its axes 2
  /// and 1.25 long, sheared 0.1
  fn bone() -> Affine2 {
    placed(10.0, 20.0, 0.3, 2.0, 1.25, 0.1)
  }

  /// Its target: at (50, -30), turned 1.2, its axes 1.5 and 0.5 long,
  /// sheared -0.2
  fn target() -> Affine2 {
    placed(50.0, -30.0, 1.2, 1.5, 0.5, -0.2)
  }

  const OFFSET: TransformOffset = TransformOffset {
    rotation: 0.25,
    x: 4.0,
    y: -2.0,
    scale_x: 0.25,
    scale_y: 0.25,
    shear_y: 0.15,
  };

  /// The offset's point in the target's space: (50, -30), plus 4 along its
  /// x axis (1.5 long at 1.2), less 2 along its y axis (0.5 long at
  /// 1.2 - 0.2 + a quarter turn)
  fn offset_point() -> Vec2 {
    let x_axis = 1.5 * Vec2::from_angle(1.2);
    let y_axis = 0.5 * Vec2::new(-(1.0f32).sin(), (1.0f32).cos());
    Vec2::new(50.0, -30.0) + 4.0 * x_axis - 2.0 * y_axis
  }

  /// The six properties, in the order the tests list them: x, y, rotation,
  /// scale x, scale y, shear y
  const NAMES: [&str; 6] = ["x", "y", "rotation", "scale x", "scale y", "shear y"];

  /// Property `property`'s mix alone at `mix`
  fn only(property: usize, mix: f32) -> TransformMix {
    let mut mixes = [0.0; 6];
    mixes[property] = mix;
    every(mixes)
  }

  fn every([x, y, rotate, scale_x, scale_y, shear_y]: [f32; 6]) -> TransformMix {
    TransformMix {
      rotate,
      x,
      y,
      scale_x,
      scale_y,
      shear_y,
    }
  }

  /// A transform's six properties, read from its matrix: its place, the
  /// angle of its x axis, its axes' lengths, and how far its y axis is
  /// from square to its x axis on its own side (the side its determinant's
  /// sign says)
  fn properties(transform: Affine2) -> [f32; 6] {
    let (x_axis, y_axis) = (transform.matrix2.x_axis, transform.matrix2.y_axis);
    let side = x_axis.perp_dot(y_axis).signum();
    [
      transform.translation.x,
      transform.translation.y,
      x_axis.to_angle(),
      x_axis.length(),
      y_axis.length(),
      side * x_axis.perp_dot(y_axis).atan2(x_axis.dot(y_axis)) - FRAC_PI_2,
    ]
  }

  fn own_properties(bone: &BonePose) -> [f32; 6] {
    [
      bone.x,
      bone.y,
      bone.rotation,
      bone.scale_x,
      bone.scale_y,
      bone.shear_y,
    ]
  }

  /// Two values of property `property` agree to `within`; angles the short
  /// way round
  fn agree(property: usize, actual: f32, expected: f32, within: f32, what: &str) {
    let off = if property == 2 {
      shortest(actual - expected)
    } else {
      actual - expected
    };
    assert!(
      off.abs() < within,
      "{what}: {} is {actual}, expected {expected}",
      NAMES[property]
    );
  }

  fn bits(transform: Affine2) -> [u32; 6] {
    transform.to_cols_array().map(f32::to_bits)
  }

  /// Each mix alone at 1 moves its own property of [`bone`] to `expected`
  /// and leaves the other five as they were: the place to the bit, the
  /// parts of the matrix the property does not touch to the bit, and what
  /// is read from an axis that was rescaled or turned to 1e-5
  fn copies_in_the_world(relative: bool, expected: [f32; 6]) {
    let before = properties(bone());
    for (property, name) in NAMES.iter().enumerate() {
      let mut moved = bone();
      in_the_world(
        &mut moved,
        &target(),
        &only(property, 1.0),
        &OFFSET,
        relative,
      );
      let after = properties(moved);
      let what = format!("{name} mix alone");
      for other in 0..6 {
        if other == property {
          agree(other, after[other], expected[other], 1e-4, &what);
        } else if other < 2 {
          assert_eq!(after[other].to_bits(), before[other].to_bits(), "{what}");
        } else {
          agree(other, after[other], before[other], 1e-5, &what);
        }
      }
      let (was, now) = (bone(), moved);
      match property {
        0 | 1 => assert_eq!(
          now.matrix2.to_cols_array().map(f32::to_bits),
          was.matrix2.to_cols_array().map(f32::to_bits),
          "{what}"
        ),
        2 => assert_eq!(now.translation, was.translation, "{what}"),
        3 => assert_eq!(
          (now.translation, now.matrix2.y_axis),
          (was.translation, was.matrix2.y_axis),
          "{what}"
        ),
        _ => assert_eq!(
          (now.translation, now.matrix2.x_axis),
          (was.translation, was.matrix2.x_axis),
          "{what}"
        ),
      }
    }
  }

  #[test]
  fn in_the_world_copies_each_property_by_its_own_mix() {
    // Toward the target's values plus the offsets: the offset's point; a
    // turn of 1.2 + 0.25; lengths of 1.5 + 0.25 and 0.5 + 0.25; a shear of
    // -0.2 + 0.15
    let point = offset_point();
    copies_in_the_world(false, [point.x, point.y, 1.45, 1.75, 0.75, -0.05]);
  }

  #[test]
  fn in_the_world_relative_copies_each_property_by_its_own_mix() {
    // The target's values plus the offsets added: the place moved by the
    // offset's point; turned by 1.45 from 0.3; the lengths multiplied by
    // 1 + (1.5 - 1 + 0.25) and 1 + (0.5 - 1 + 0.25); the shear 0.1 plus
    // -0.05
    let point = offset_point();
    let expected = [
      10.0 + point.x,
      20.0 + point.y,
      1.75,
      2.0 * 1.75,
      1.25 * 0.75,
      0.05,
    ];
    copies_in_the_world(true, expected);
  }

  /// The bone the own-values tests move, and its target: the world tests'
  /// values as own values, the target turned 7.5 (more than a full turn),
  /// each with an x shear that no mix touches
  fn own_bone() -> BonePose {
    BonePose {
      shear_x: 0.05,
      ..own(10.0, 20.0, 0.3, 2.0, 1.25, 0.1)
    }
  }

  fn own_target() -> BonePose {
    BonePose {
      shear_x: 0.4,
      ..own(50.0, -30.0, 7.5, 1.5, 0.5, -0.2)
    }
  }

  /// Each mix alone at 1 moves its own property of [`own_bone`] to
  /// `expected` and leaves the other five, and the x shear, to the bit
  fn copies_its_own_values(relative: bool, expected: [f32; 6]) {
    let before = own_bone();
    for (property, name) in NAMES.iter().enumerate() {
      let mut moved = own_bone();
      in_its_own_values(
        &mut moved,
        &own_target(),
        &only(property, 1.0),
        &OFFSET,
        relative,
      );
      let (was, now) = (own_properties(&before), own_properties(&moved));
      let what = format!("{name} mix alone");
      for other in 0..6 {
        if other == property {
          // A rotation is a plain number here: compared as one
          let off = now[other] - expected[other];
          assert!(
            off.abs() < 1e-4,
            "{what}: {} is {}",
            NAMES[other],
            now[other]
          );
        } else {
          assert_eq!(now[other].to_bits(), was[other].to_bits(), "{what}");
        }
      }
      assert_eq!(moved.shear_x.to_bits(), before.shear_x.to_bits());
    }
  }

  #[test]
  fn in_its_own_values_copies_each_property_by_its_own_mix() {
    // Toward the target's own values plus the offsets: (54, -32); a turn
    // of 7.5 + 0.25, not brought within a turn; 1.75, 0.75 and -0.05
    copies_its_own_values(false, [54.0, -32.0, 7.75, 1.75, 0.75, -0.05]);
  }

  #[test]
  fn in_its_own_values_relative_copies_each_property_by_its_own_mix() {
    // The target's own values plus the offsets added: (10 + 54, 20 - 32);
    // 0.3 + 7.75; scales multiplied by 1 + (1.5 - 1 + 0.25) and
    // 1 + (0.5 - 1 + 0.25); a shear of 0.1 - 0.05
    copies_its_own_values(true, [64.0, -12.0, 8.05, 3.5, 0.9375, 0.05]);
  }

  #[test]
  fn mixes_of_minus_one_a_half_and_two_go_where_the_rule_says() {
    let point = offset_point();
    for m in [-1.0, 0.5, 2.0] {
      let all = every([m; 6]);
      let what = format!("every mix at {m}");
      // Own values, toward: value + (target's + offset - value) × m
      let mut moved = own_bone();
      in_its_own_values(&mut moved, &own_target(), &all, &OFFSET, false);
      let expected = [
        10.0 + (54.0 - 10.0) * m,
        20.0 + (-32.0 - 20.0) * m,
        0.3 + (7.75 - 0.3) * m,
        2.0 + (1.75 - 2.0) * m,
        1.25 + (0.75 - 1.25) * m,
        0.1 + (-0.05 - 0.1) * m,
      ];
      for (property, (now, want)) in own_properties(&moved).iter().zip(expected).enumerate() {
        assert!(
          (now - want).abs() < 1e-4,
          "own, {what}: {}",
          NAMES[property]
        );
      }
      // Own values, relative: value + (target's + offset) × m, and a scale
      // times 1 + (target's - 1 + offset) × m
      let mut moved = own_bone();
      in_its_own_values(&mut moved, &own_target(), &all, &OFFSET, true);
      let expected = [
        10.0 + 54.0 * m,
        20.0 - 32.0 * m,
        0.3 + 7.75 * m,
        2.0 * (1.0 + 0.75 * m),
        1.25 * (1.0 - 0.25 * m),
        0.1 - 0.05 * m,
      ];
      for (property, (now, want)) in own_properties(&moved).iter().zip(expected).enumerate() {
        assert!(
          (now - want).abs() < 1e-4,
          "own relative, {what}: {}",
          NAMES[property]
        );
      }
      // In the world, toward: the short way from 0.3 to 1.45 is 1.15
      let mut moved = bone();
      in_the_world(&mut moved, &target(), &all, &OFFSET, false);
      let expected = [
        10.0 + (point.x - 10.0) * m,
        20.0 + (point.y - 20.0) * m,
        0.3 + 1.15 * m,
        2.0 + (1.75 - 2.0) * m,
        1.25 + (0.75 - 1.25) * m,
        0.1 + (-0.05 - 0.1) * m,
      ];
      for (property, (now, want)) in properties(moved).iter().zip(expected).enumerate() {
        agree(property, *now, want, 1e-4, &format!("world, {what}"));
      }
      // In the world, relative
      let mut moved = bone();
      in_the_world(&mut moved, &target(), &all, &OFFSET, true);
      let expected = [
        10.0 + point.x * m,
        20.0 + point.y * m,
        0.3 + 1.45 * m,
        2.0 * (1.0 + 0.75 * m),
        1.25 * (1.0 - 0.25 * m),
        0.1 - 0.05 * m,
      ];
      for (property, (now, want)) in properties(moved).iter().zip(expected).enumerate() {
        agree(
          property,
          *now,
          want,
          1e-4,
          &format!("world relative, {what}"),
        );
      }
    }
  }

  #[test]
  fn a_mirrored_target_mirrors_the_rotation_and_shear_offsets() {
    // The target with its y axis turned round: x axis still at 1.2, y axis
    // pointing at 1.0 - a quarter turn, so a quarter turn and 0.2 from the
    // x axis on the negative side. On its own side its shear is 0.2
    let mirrored = placed(50.0, -30.0, 1.2, 1.5, -0.5, -0.2);
    assert!(mirrored.matrix2.determinant() < 0.0);
    let turn_and_shear = TransformMix {
      rotate: 1.0,
      shear_y: 1.0,
      ..TransformMix::default()
    };
    // Toward: a turn of 1.2 - 0.25 and a shear of 0.2 - 0.15, the offsets
    // taken off
    let mut moved = bone();
    in_the_world(&mut moved, &mirrored, &turn_and_shear, &OFFSET, false);
    let read = properties(moved);
    agree(2, read[2], 0.95, 1e-4, "toward a mirrored target");
    agree(5, read[5], 0.05, 1e-4, "toward a mirrored target");
    // Relative: 0.3 + 0.95, and 0.1 + 0.05
    let mut moved = bone();
    in_the_world(&mut moved, &mirrored, &turn_and_shear, &OFFSET, true);
    let read = properties(moved);
    agree(2, read[2], 1.25, 1e-4, "relative to a mirrored target");
    agree(5, read[5], 0.15, 1e-4, "relative to a mirrored target");
    // The place offset is a point in the target's space, mirrored with it:
    // 4 along its x axis and 2 along its y axis, which points the other way
    let x_axis = 1.5 * Vec2::from_angle(1.2);
    let y_axis = -0.5 * Vec2::new(-(1.0f32).sin(), (1.0f32).cos());
    let point = Vec2::new(50.0, -30.0) + 4.0 * x_axis - 2.0 * y_axis;
    let place = TransformMix {
      x: 1.0,
      y: 1.0,
      ..TransformMix::default()
    };
    let mut moved = bone();
    in_the_world(&mut moved, &mirrored, &place, &OFFSET, false);
    assert!(
      moved.translation.distance(point) < 1e-4,
      "{}",
      moved.translation
    );
  }

  #[test]
  fn a_mirrored_bone_stays_mirrored_and_an_unmirrored_one_unmirrored() {
    let mirrored = placed(10.0, 20.0, 0.3, 2.0, -1.25, 0.1);
    let mirrored_target = placed(50.0, -30.0, 1.2, 1.5, -0.5, -0.2);
    for relative in [false, true] {
      // A mirrored bone toward an unmirrored target is still mirrored, its
      // lengths and its shear on its own side what the rules give
      let mut moved = mirrored;
      in_the_world(&mut moved, &target(), &TransformMix::ALL, &OFFSET, relative);
      assert!(moved.matrix2.determinant() < 0.0, "relative {relative}");
      let read = properties(moved);
      let expected = if relative {
        // On its own side the mirrored bone's shear is -0.1: its y axis
        // is turned round, so 0.1 nearer its x axis than square
        [1.75, 3.5, 0.9375, -0.15]
      } else {
        [1.45, 1.75, 0.75, -0.05]
      };
      for (property, want) in (2..6).zip(expected) {
        agree(property, read[property], want, 1e-4, "a mirrored bone");
      }
      // An unmirrored bone toward a mirrored target is not mirrored
      let mut moved = bone();
      in_the_world(
        &mut moved,
        &mirrored_target,
        &TransformMix::ALL,
        &OFFSET,
        relative,
      );
      assert!(moved.matrix2.determinant() > 0.0, "relative {relative}");
    }

    // Pushed past what they can be: a length that the rule would make
    // negative is 0 (relative at a mix of 2, 1 + (1.5 - 1 - 3) × 2 is -4),
    // and a shear past a quarter turn (-0.2 + 2) stops 0.001 short of it.
    // Neither turns an axis round
    let far = TransformOffset {
      scale_x: -3.0,
      shear_y: 2.0,
      ..OFFSET
    };
    let shorten = every([0.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
    for start in [bone(), mirrored] {
      let mut moved = start;
      in_the_world(&mut moved, &target(), &shorten, &far, true);
      assert_eq!(moved.matrix2.x_axis, Vec2::ZERO);
      assert_eq!(moved.matrix2.y_axis, start.matrix2.y_axis);
      let mut moved = start;
      let shear = TransformMix {
        shear_y: 1.0,
        ..TransformMix::default()
      };
      in_the_world(&mut moved, &target(), &shear, &far, false);
      let sign = start.matrix2.determinant().signum();
      assert_eq!(moved.matrix2.determinant().signum(), sign);
      agree(5, properties(moved)[5], FRAC_PI_2 - 0.001, 1e-4, "pushed");
    }
  }

  #[test]
  fn an_own_values_relative_half_mix_turns_smoothly_through_its_target_s_full_turn() {
    // The target's own rotation from 0 to a full turn in 64 steps; a gear
    // at half its speed adds half of it to its own 0.3 each step
    let half = TransformMix {
      rotate: 0.5,
      ..TransformMix::default()
    };
    let turned = |step: u32| {
      let mut gear = own_bone();
      let target = BonePose {
        rotation: TAU * step as f32 / 64.0,
        ..own_target()
      };
      in_its_own_values(&mut gear, &target, &half, &TransformOffset::default(), true);
      gear.rotation
    };
    for step in 1..=64 {
      let moved = turned(step) - turned(step - 1);
      assert!(moved.abs() <= PI / 32.0 + 1e-4, "step {step}: {moved}");
    }
    assert!((turned(64) - (0.3 + PI)).abs() < 1e-4);
  }

  #[test]
  fn a_world_half_mix_jumps_when_its_target_passes_half_a_turn() {
    // A world mode turns the short way: from 2.0 toward -2.5 the plain
    // difference is -4.5, and the short way 2π - 4.5 = 1.783 the other way
    // round, half of which takes the bone to 2.892
    let half = TransformMix {
      rotate: 0.5,
      ..TransformMix::default()
    };
    let mut moved = placed(0.0, 0.0, 2.0, 1.0, 1.0, 0.0);
    let target = placed(0.0, 0.0, -2.5, 1.0, 1.0, 0.0);
    in_the_world(
      &mut moved,
      &target,
      &half,
      &TransformOffset::default(),
      false,
    );
    let short_way = 2.0 + (TAU - 4.5) * 0.5;
    agree(
      2,
      moved.matrix2.x_axis.to_angle(),
      short_way,
      1e-4,
      "the short way",
    );

    // Limit: a bone at 0 turned half way toward a target whose x axis
    // goes round a full turn in 64 steps. The short way to the target
    // changes sides at half a turn, and the bone jumps by half a turn
    // there; anywhere else it moves half the target's step
    let turned = |step: u32| {
      let mut moved = placed(0.0, 0.0, 0.0, 1.0, 1.0, 0.0);
      let target = placed(0.0, 0.0, TAU * step as f32 / 64.0, 1.0, 1.0, 0.0);
      in_the_world(
        &mut moved,
        &target,
        &half,
        &TransformOffset::default(),
        false,
      );
      moved.matrix2.x_axis.to_angle()
    };
    let mut jumps = 0;
    for step in 1..=64 {
      let moved = shortest(turned(step) - turned(step - 1)).abs();
      if moved > 3.0 {
        jumps += 1;
      } else {
        assert!(moved <= PI / 64.0 + 1e-4, "step {step}: {moved}");
      }
    }
    assert_eq!(jumps, 1);
  }

  #[test]
  fn a_flat_bone_or_target_changes_nothing_it_cannot_and_makes_no_nan() {
    // A bone flat along x (0, and under 1e-6), or a target flat along y:
    // the place still moves to the offset's point, and the rest of the
    // bone stays as it was, to the bit
    let flat_target = placed(50.0, -30.0, 1.2, 1.5, 0.0, -0.2);
    let flat_point = Vec2::new(50.0, -30.0) + 4.0 * 1.5 * Vec2::from_angle(1.2);
    let cases = [
      (
        placed(10.0, 20.0, 0.3, 0.0, 1.25, 0.1),
        target(),
        offset_point(),
      ),
      (
        placed(10.0, 20.0, 0.3, 1e-7, 1.25, 0.1),
        target(),
        offset_point(),
      ),
      (bone(), flat_target, flat_point),
    ];
    for (start, target, point) in cases {
      let mut moved = start;
      in_the_world(&mut moved, &target, &TransformMix::ALL, &OFFSET, false);
      assert!(moved.translation.distance(point) < 1e-4);
      assert_eq!(
        moved.matrix2.to_cols_array().map(f32::to_bits),
        start.matrix2.to_cols_array().map(f32::to_bits)
      );
      let mut moved = start;
      in_the_world(&mut moved, &target, &TransformMix::ALL, &OFFSET, true);
      assert!(moved.is_finite());
      assert_eq!(moved.matrix2, start.matrix2);
    }

    // Anything that is not finite, given or made, changes nothing
    let nan_target = Affine2 {
      translation: Vec2::new(f32::NAN, 0.0),
      ..target()
    };
    let infinite_bone = Affine2 {
      translation: Vec2::new(f32::INFINITY, 0.0),
      ..bone()
    };
    let nan_offset = TransformOffset {
      rotation: f32::NAN,
      ..OFFSET
    };
    let nan_mix = TransformMix {
      shear_y: f32::NAN,
      ..TransformMix::ALL
    };
    let huge = placed(3e38, 0.0, 0.0, 1.0, 1.0, 0.0);
    let worlds = [
      (bone(), nan_target, TransformMix::ALL, OFFSET, false),
      (infinite_bone, target(), TransformMix::ALL, OFFSET, false),
      (bone(), target(), TransformMix::ALL, nan_offset, false),
      (bone(), target(), nan_mix, OFFSET, false),
      // 3e38 + 3e38 is past the largest number
      (
        huge,
        huge,
        TransformMix::ALL,
        TransformOffset::default(),
        true,
      ),
    ];
    for (start, target, mix, offset, relative) in worlds {
      let mut moved = start;
      in_the_world(&mut moved, &target, &mix, &offset, relative);
      assert_eq!(bits(moved), bits(start));
    }
    let own_bits = |bone: &BonePose| {
      let BonePose {
        x,
        y,
        rotation,
        scale_x,
        scale_y,
        shear_x,
        shear_y,
      } = *bone;
      [x, y, rotation, scale_x, scale_y, shear_x, shear_y].map(f32::to_bits)
    };
    let nan_own = BonePose {
      rotation: f32::NAN,
      ..own_target()
    };
    let huge_own = BonePose {
      x: 3e38,
      ..own_bone()
    };
    let owns = [
      (own_bone(), nan_own, TransformMix::ALL, OFFSET, false),
      (own_bone(), own_target(), nan_mix, OFFSET, false),
      (
        own_bone(),
        own_target(),
        TransformMix::ALL,
        nan_offset,
        true,
      ),
      (
        huge_own,
        huge_own,
        TransformMix::ALL,
        TransformOffset::default(),
        true,
      ),
    ];
    for (start, target, mix, offset, relative) in owns {
      let mut moved = start;
      in_its_own_values(&mut moved, &target, &mix, &offset, relative);
      assert_eq!(own_bits(&moved), own_bits(&start));
    }
  }
}
