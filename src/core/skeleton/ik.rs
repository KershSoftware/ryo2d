//! Inverse kinematics: the turns that bring one bone to point at a target,
//! or the tip of two bones or of a chain to reach it.
//!
//! The functions here are pure. They take bones' own values (each relative
//! to its parent) and a target point in the space of the first bone's
//! parent, and change those bones' rotation and scale. They know nothing of
//! a pose or a skeleton: bringing the target into that space, and placing
//! the bones again afterwards, is the caller's.

use std::f32::consts::PI;

use glam::{Affine2, Vec2};

use super::apply::shortest;
use super::constraint::{IkBend, IkSetup};
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

/// How near one line a chain's joints count as on it, as a share of the
/// chain's length: a pose that near a line has no bend of its own. A
/// chain's tip comes far nearer than this to a target it can meet, and the
/// tests hold it to this
pub(crate) const CHAIN_NEAR: f32 = 0.001;

/// How many entries of scratch one solve of a chain of `bones` bones fills,
/// so that a caller can make room for its longest chain before any is
/// solved. A chain has a joint for each bone and one for its tip, and a
/// part from each joint to the next: the solve keeps every joint, every
/// part as it stood in the pose passed in, and every part's length beside
/// an angle, which is three entries a bone and one more
pub(crate) const fn chain_room(bones: usize) -> usize {
  3 * bones + 1
}

/// The most a chain's swing, how far apart its parts point, changes in one
/// step of a walk along its family of shapes, in radians. A closing walk's
/// steps are shorter wherever the tip's distance could turn back sooner, so
/// that no turn lies inside a step (`fall`); an opening walk takes the
/// steps whole
const CHAIN_STEP: f32 = PI / 8.0;

/// How many times shorter than its most a closing walk's step may grow
/// before the walk stops there: the tip's distance turns back within that
/// of the stop, or has all but stopped falling
const CHAIN_SETTLED: f32 = 4096.0;

/// The most steps of a walk
const CHAIN_WALK: usize = 96;

/// How many times the step that holds the shape wanted is halved
const CHAIN_HALVINGS: usize = 24;

/// The values of an IK constraint that a solve takes: those its setup gives
/// and animations can key. `uniform` is the setup's alone
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IkValues {
  /// 0 (the animated pose) to 1 (the solved pose)
  pub mix: f32,
  /// Two bones: a distance from full reach within which the tip slows as
  /// the bones straighten
  pub softness: f32,
  /// Which way it bends: for two bones, the joint the positive or the
  /// negative way; for a chain, kept to one of those sides, or closing the
  /// way its pose bends
  pub bend: IkBend,
  /// One bone: scale it down when the target is nearer than its length
  pub compress: bool,
  /// One or two bones: scale the first along its length when the target
  /// is beyond reach
  pub stretch: bool,
  /// One or two bones: the most stretch multiplies the scale by before the
  /// mix, as `distance / reach` is taken no higher. Not keyed: an
  /// animation's key leaves it as the setup's. `None` is no limit
  pub max_stretch: Option<f32>,
}

impl IkValues {
  /// The values a constraint starts each step with: its setup's. A setup
  /// that leaves `bend` out gives [`IkBend::Positive`]; the built data's
  /// setups hold the default for their bone count
  pub fn of(setup: &IkSetup) -> Self {
    Self {
      mix: setup.mix,
      softness: setup.softness,
      bend: setup.bend.unwrap_or(IkBend::Positive),
      compress: setup.compress,
      stretch: setup.stretch,
      max_stretch: setup.max_stretch,
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
/// a mix of 1 puts its tip on the target (`distance / reach` is taken no
/// higher than `values.max_stretch`, so a farther target is pointed at and
/// not reached); `uniform` multiplies `scale_y` by
/// the same. A bone with no reach (no `length`, or no x scale) only turns.
///
/// Only the bone's rotation and scale are written. Nothing is, when the mix
/// is 0 or less, when the target is on the bone's origin, or when any value
/// given, or any that would be written, is not finite
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
    scale_factor(distance, reach, mix, values.max_stretch)
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
/// is turned the positive way from part one with `values.bend`
/// [`IkBend::Positive`], the other with [`IkBend::Negative`]. Part one is
/// to point at the joint and part two from the joint to the target:
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
/// full reach and does not arrive.
///
/// With `values.stretch`, no softness and an even `first`, a target beyond
/// reach multiplies `first`'s `scale_x` (and `scale_y` with `uniform`) by
/// `1 + (distance / reach - 1) × mix` (with `distance / reach` taken no
/// higher than `values.max_stretch`), and both parts are laid along the
/// line to the target on `first` as stretched. The tip is then on the
/// target when `second` lies on `first`'s axis or with `uniform`, and
/// otherwise on the line, a little short of the target.
///
/// Exact when `first` scales every direction alike. When its own scale is
/// uneven or it is sheared, part two's length changes as `second` turns.
/// The triangle is still solved once, with part two as long as it is in
/// the pose passed in, so `second` is pointed the right way and comes out
/// another length: the tip misses along part two, by at most the spread
/// between the longest and the shortest that `first` makes it.
///
/// Only the bones' rotations and `first`'s scale are written. Nothing is,
/// when the mix is 0 or less, when the target is on `first`'s origin, when
/// `first` is flat or part two has no length, or when any value given, or
/// any that would be written, is not finite. When `second` sits on
/// `first`'s origin, `first` is left as it is and `second` points at the
/// target
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
  let axis = Vec2::from_angle(second.rotation + second.shear_x) * second.scale_x;
  let part_one = unturned * Vec2::new(second.x, second.y);
  let part_two = unturned * (axis * length);
  let (l1, l2) = (part_one.length(), part_two.length());
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
  // Out of reach, or inside the fold, there is no triangle: `off` is 0 and
  // both parts lie on the line to the target
  let along = (l1 * l1 - l2 * l2 + d * d) / (2.0 * d);
  let off = (l1 * l1 - along * along).max(0.0).sqrt();
  let toward = to_target / distance;
  let negative = values.bend == IkBend::Negative;
  let side = toward.perp() * if negative { -off } else { off };
  let one = toward * along - side;
  let two = toward * (d - along) + side;

  let stretches = values.stretch && softness <= 0.0 && distance > reach && even(first);
  let factor = if stretches {
    scale_factor(distance, reach, mix, values.max_stretch)
  } else {
    1.0
  };
  let scale_x = first.scale_x * factor;
  let scale_y = if uniform {
    first.scale_y * factor
  } else {
    first.scale_y
  };
  // The rotations that lay part one along `one` and point the second bone's
  // axis along `two`, the first bone holding part one as `held` and carrying
  // the second bone's axis as `carrier` does
  let rotations = |carrier: &BonePose, held: Vec2| {
    let solved = one.to_angle() - held.to_angle();
    let aim = pointing(second, carrier.linear_turned(solved).inverse() * two);
    (solved, aim)
  };
  let (solved, aim) = if stretches {
    // Stretched along x alone, the first bone holds a part one off its axis
    // at another angle, and no longer scales every direction alike: both
    // parts are laid along the line on the bone as it will be
    let stretched = BonePose {
      scale_x,
      scale_y,
      ..*first
    };
    let held = stretched.linear_turned(0.0) * Vec2::new(second.x, second.y);
    rotations(&stretched, held)
  } else {
    rotations(first, part_one)
  };
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

/// Turns a chain of bones, each the child of the one before, so that the
/// tip of the last (`length` along its x axis) reaches for `target`, a
/// point in the space of the first bone's parent, by `mix` the short way
/// round. A mix above 1 is taken as 1. With a `side` (1 the positive, -1
/// the negative) the chain keeps to it; without one it closes the way its
/// pose bends.
///
/// The chain is its joints: each bone's origin and then the tip, where the
/// bones' own matrices put them in the pose passed in. A part runs from one
/// joint to the next, and is rigid at the length it has there. The pose's
/// shape is the turn at each joint, from one part to the next.
///
/// The chain is given one shape of a family that runs through that pose,
/// the one whose tip is as far from the first joint as the target is, and
/// is then turned about its first joint onto the target. So the shape
/// follows the target's distance alone and its direction only turns the
/// chain, and a target that moves does not make the chain jump. What the
/// pose decides is not the target's: as the pose is animated the chain can
/// flip sides (below, and the module notes' limits):
///
/// - A target farther than the pose's tip opens the pose: every turn is
///   scaled by one factor, from 1 (the pose) toward 0 (straight).
/// - A nearer one closes it: one even turn is added to every joint, the way
///   that brings the tip nearer (for a pose bent one way, the way it
///   bends; for an S, whichever way that is, so a chain swaying in an S
///   flips sides as that way changes). The chain curls further as an arc,
///   until its tip's distance stops falling or a joint has folded half a
///   turn. A target nearer than that is not reached: the chain takes that
///   nearest shape, pointed at the target. It curls, and does not fold.
/// - A target at least the chain's length away lays every part along the
///   line to it, which is where opening ends.
/// - A pose with no bend of its own, every joint within `CHAIN_NEAR ×` its
///   length of one line, has no way of its own to bend, and is closed the
///   positive way: an even arc.
///
/// A chain kept to a side is given a shape of another family, which does
/// not run through the pose: the chain laid straight and curled as an even
/// arc, every joint turned as much as the others the side's way, further
/// the nearer the target, and laid straight toward a target at least its
/// length away. Its shape is its parts' lengths' and the target's distance
/// alone, so it never changes sides as its pose is animated; the pose
/// comes back only by the mix. It is walked as the closing family is, and
/// stops where a straight pose's arc would.
///
/// The shape is found by walking the family from the pose in steps, for the
/// first step in which the tip's distance crosses the target's, and halving
/// that step. A closing walk's steps are sized from the parts' lengths so
/// that the tip's distance cannot turn back inside one: it stops at the
/// first turn (or short of it, for a pose whose steps stay small past
/// `CHAIN_WALK` of them), and the shape follows a target coming in without
/// a jump out to there. The walk goes the way the tip's distance changes
/// at the pose: a pose wound past half a turn, so far that bending it
/// further takes its tip out, is bent further for a farther target, as far
/// as that helps, and only then opened, which can make it jump.
///
/// Each part has then turned from where it stood, and turning a bone turns
/// its part and every part after it. So a bone is turned by what its part
/// turned more than the part before it, and the first bone by what its
/// part turned: the other way round under bones before it that mirror
/// between them by their own scales. A part of no length is passed over:
/// its joint stays on the one before, and its bone is not turned.
///
/// Exact when every bone but the last scales every direction alike. Under
/// a bone with an uneven scale or a shear, the parts after it change
/// length as they turn, and the tip may miss.
///
/// Only the bones' rotations are written. Nothing is, when there are fewer
/// than two bones, when the mix is 0 or less, when the target is on the
/// first joint, when the chain has no length, or when any value given, or
/// any that would be written, is not finite.
///
/// `joints` is scratch the caller keeps. It is cleared and filled here with
/// `chain_room` entries for as many bones, so a run takes no more room than
/// a run with as many bones took before, and none at all in a scratch made
/// with that room
pub(crate) fn chain(
  bones: &mut [BonePose],
  length: f32,
  target: Vec2,
  mix: f32,
  side: Option<f32>,
  joints: &mut Vec<Vec2>,
) {
  let count = bones.len();
  let given = length.is_finite() && target.is_finite() && mix.is_finite();
  if count < 2 || !given || mix <= 0.0 || !bones.iter().all(finite) {
    return;
  }
  let mix = mix.min(1.0);

  // The scratch: the joints (each bone's origin, then the tip), each part
  // (one joint to the next) as it stands in the pose passed in, and each
  // part's length (`x`) beside an angle (`y`): its swing from the first
  // part while the shape is found, then its bone's rotation
  joints.clear();
  joints.resize(chain_room(count), Vec2::ZERO);
  let (joints, parts) = joints.split_at_mut(count + 1);
  let (stood, sizes) = parts.split_at_mut(count);

  let mut carried = Affine2::IDENTITY;
  for (joint, bone) in joints.iter_mut().zip(bones.iter()) {
    *joint = carried.transform_point2(Vec2::new(bone.x, bone.y));
    carried *= bone.affine();
  }
  joints[count] = carried.transform_point2(Vec2::new(length, 0.0));
  let mut total = 0.0;
  for index in 0..count {
    let part = joints[index + 1] - joints[index];
    let long = part.length();
    stood[index] = part;
    sizes[index].x = if long > NO_LENGTH { long } else { 0.0 };
    total += sizes[index].x;
  }
  let placed = total.is_finite() && joints.iter().all(|joint| joint.is_finite());
  let to_target = target - joints[0];
  let distance = to_target.length();
  if !placed || total <= NO_LENGTH || distance <= NO_LENGTH {
    return;
  }

  // The pose's shape: each part's swing from the first part with a length.
  // With it, which way the first part points, how many parts have a
  // length, the turns that go farthest each way, and the swings that do
  let (mut before, mut heading, mut swung, mut long) = (None::<Vec2>, 0.0, 0.0, 0.0);
  let (mut turns, mut swings) = ((0.0_f32, 0.0_f32), (0.0_f32, 0.0_f32));
  for index in 0..count {
    if sizes[index].x > 0.0 {
      match before {
        Some(before) => {
          let by = turn(before, stood[index]);
          swung += by;
          turns = (turns.0.min(by), turns.1.max(by));
          swings = (swings.0.min(swung), swings.1.max(swung));
        }
        None => heading = stood[index].to_angle(),
      }
      before = Some(stood[index]);
      long += 1.0;
    }
    sizes[index].y = swung;
  }
  let sizes_now = &*sizes;
  let reach = |shape: Shape| reached(sizes_now, shape);

  let shape = if distance >= total || long < 2.0 {
    Shape::STRAIGHT
  } else if let Some(side) = side {
    // Kept to a side: an even arc that way, from straight, whatever the
    // pose
    let way = if side < 0.0 { -1.0 } else { 1.0 };
    let step = CHAIN_STEP / (long - 1.0);
    Shape {
      factor: 0.0,
      added: fall(sizes_now, Shape::STRAIGHT, way, step, PI, distance) * way,
    }
  } else if on_one_line(joints, CHAIN_NEAR * total) {
    // No bend of its own: an even arc the positive way, from straight
    let step = CHAIN_STEP / (long - 1.0);
    Shape {
      factor: 0.0,
      added: fall(sizes_now, Shape::STRAIGHT, 1.0, step, PI, distance),
    }
  } else {
    let (_, pose, by_factor, by_added) = reach(Shape::POSE);
    if distance > pose {
      // Opened: every turn scaled toward straight, where the tip is the
      // chain's length away. A pose wound so far that its tip goes out as
      // it bends is first bent further, as far as that brings the tip out
      let scaled = |factor: f32| {
        let (_, far, by_factor, _) = reach(Shape { factor, added: 0.0 });
        (far, by_factor)
      };
      let step = CHAIN_STEP / (swings.1 - swings.0).max(f32::MIN_POSITIVE);
      let folded = PI / turns.1.max(-turns.0);
      let further = if by_factor > 0.0 {
        rise(&scaled, 1.0, pose, step, folded, distance, true)
      } else {
        None
      };
      let opened = || rise(&scaled, 1.0, pose, -step.min(0.125), 0.0, distance, false);
      Shape {
        factor: further.or_else(opened).unwrap_or(0.0),
        added: 0.0,
      }
    } else {
      // Closed: an even turn added to every joint, the way the tip comes
      // nearer, until a joint has folded half a turn
      let way = if by_added > 0.0 { -1.0 } else { 1.0 };
      let folded = if way > 0.0 {
        PI - turns.1
      } else {
        PI + turns.0
      };
      let step = CHAIN_STEP / (long - 1.0);
      let curl = fall(sizes_now, Shape::POSE, way, step, folded.max(0.0), distance);
      Shape {
        factor: 1.0,
        added: curl * way,
      }
    }
  };

  // The shape, turned about the first joint so its tip is on the line to
  // the target
  let (tip, far, ..) = reach(shape);
  if far > NO_LENGTH {
    heading = to_target.to_angle() - tip.to_angle();
  }
  let mut joints_before = 0.0;
  for index in 0..count {
    let swing = sizes[index].y * shape.factor + shape.added * joints_before;
    joints[index + 1] = joints[index] + Vec2::from_angle(heading + swing) * sizes[index].x;
    if sizes[index].x > 0.0 {
      joints_before += 1.0;
    }
  }

  // Each bone's rotation, kept beside its part until every one is known to
  // be finite
  let (mut before, mut mirrored) = (0.0, false);
  for index in 0..count {
    let swung = if sizes[index].x > 0.0 {
      turn(stood[index], joints[index + 1] - joints[index])
    } else {
      before
    };
    let more = if mirrored {
      before - swung
    } else {
      swung - before
    };
    sizes[index].y = bones[index].rotation + shortest(more) * mix;
    before = swung;
    mirrored ^= mirrors(&bones[index]);
  }
  if sizes.iter().all(|size| size.y.is_finite()) {
    for (bone, size) in bones.iter_mut().zip(sizes.iter()) {
      bone.rotation = size.y;
    }
  }
}

/// One shape of a chain's family: every part's swing from the first part is
/// the pose's times `factor`, and `added` more for each joint before the
/// part
#[derive(Clone, Copy)]
struct Shape {
  factor: f32,
  added: f32,
}

impl Shape {
  /// The chain laid straight
  const STRAIGHT: Self = Self {
    factor: 0.0,
    added: 0.0,
  };

  /// The pose as it was passed in
  const POSE: Self = Self {
    factor: 1.0,
    added: 0.0,
  };
}

/// Where a shape puts the tip, from the first joint with the first part
/// along x; how far that is; and how fast that distance changes as the
/// shape's factor grows, and as its added turn grows. `sizes` holds each
/// part's length (`x`) and its swing from the first part in the pose (`y`)
fn reached(sizes: &[Vec2], shape: Shape) -> (Vec2, f32, f32, f32) {
  let (mut tip, mut by_factor, mut by_added) = (Vec2::ZERO, Vec2::ZERO, Vec2::ZERO);
  let mut joints_before = 0.0;
  for size in sizes.iter().filter(|size| size.x > 0.0) {
    let along = Vec2::from_angle(size.y * shape.factor + shape.added * joints_before);
    tip += along * size.x;
    by_factor += along.perp() * (size.x * size.y);
    by_added += along.perp() * (size.x * joints_before);
    joints_before += 1.0;
  }
  let far = tip.length();
  if far > NO_LENGTH {
    (tip, far, tip.dot(by_factor) / far, tip.dot(by_added) / far)
  } else {
    (tip, far, 0.0, 0.0)
  }
}

/// Walks a chain's closing family from `from` (the pose, or straight),
/// curling it by an even turn at every joint, `way` round, as far as `end`,
/// for the curl that puts the tip `distance` from the first joint. The tip's
/// distance is to fall along the way. Where it stops falling first, or the
/// family ends, with the tip still farther than `distance`, that nearest
/// curl is given: a later crossing would make the chain jump as its target
/// passed the turn.
///
/// No turn of the tip's distance can hide inside a step. Its square is a sum
/// of cosines of the curl, so how fast its slope can change is bounded by
/// the parts' lengths alone (`turning_bound`). From the slope and the
/// slope's rate at a place, each step goes only as far as the slope is sure
/// to stay below 0 (`falling_for`), and never more than `step`: across every
/// step the distance falls, so a crossing there is the only one. The sure
/// stretch shrinks as the walk nears a turn, and the walk stops once it is
/// less than `step / CHAIN_SETTLED`: at the first turn, or where the
/// distance has all but stopped falling; or after `CHAIN_WALK` steps, short
/// of the turn (rare: 11 of 150,000 random poses of sixteen bones, the
/// worst losing 0.026 of the length of its reach inward). The places walked are the pose's
/// alone, whatever the target, so as the target comes in the curl follows it
/// without a jump, out to that stop
fn fall(sizes: &[Vec2], from: Shape, way: f32, step: f32, end: f32, distance: f32) -> f32 {
  let bound = turning_bound(sizes);
  let at = |curl: f32| curled(sizes, from, way, curl);
  let near = |curl: f32| {
    let (far, slope, _) = at(curl);
    (far, slope)
  };
  let settled = step / CHAIN_SETTLED;
  let (_, mut slope, mut bend) = at(0.0);
  let mut held = 0.0;
  for _ in 0..CHAIN_WALK {
    let sure = falling_for(slope, bend, bound);
    if sure < settled {
      return held;
    }
    let next = (held + sure.min(step)).min(end);
    let (far, next_slope, next_bend) = at(next);
    if far <= distance {
      return halved(&near, held, next, distance);
    }
    if next >= end {
      return end;
    }
    (held, slope, bend) = (next, next_slope, next_bend);
  }
  held
}

/// Where `shape` curled by `curl` more at every joint, `way` round, puts the
/// tip: how far from the first joint, how fast the square of that distance
/// changes as the curl grows, and how fast that changes. `sizes` is as for
/// `reached`
fn curled(sizes: &[Vec2], shape: Shape, way: f32, curl: f32) -> (f32, f32, f32) {
  let (mut tip, mut by_curl, mut by_curl_twice) = (Vec2::ZERO, Vec2::ZERO, Vec2::ZERO);
  let added = shape.added + curl * way;
  let mut joints_before = 0.0;
  for size in sizes.iter().filter(|size| size.x > 0.0) {
    let along = Vec2::from_angle(size.y * shape.factor + added * joints_before);
    tip += along * size.x;
    by_curl += along.perp() * (size.x * joints_before * way);
    by_curl_twice -= along * (size.x * joints_before * joints_before);
    joints_before += 1.0;
  }
  let slope = 2.0 * tip.dot(by_curl);
  let bend = 2.0 * (by_curl.length_squared() + tip.dot(by_curl_twice));
  (tip.length(), slope, bend)
}

/// The most the third derivative of the square of the tip's distance can be
/// along a closing family. That square is the sum over every two parts of
/// `lᵢ lⱼ cos(aᵢ - aⱼ)`, and the curl turns `aᵢ - aⱼ` by `kᵢ - kⱼ` times
/// itself, `k` counting the joints before a part: so its third derivative is
/// at most the sum of `lᵢ lⱼ |kᵢ - kⱼ|³`
fn turning_bound(sizes: &[Vec2]) -> f64 {
  let parts = || sizes.iter().filter(|size| size.x > 0.0).map(|size| size.x);
  let mut bound = 0.0;
  for (one, first) in parts().enumerate() {
    for (other, second) in parts().enumerate().skip(one + 1) {
      let apart = (other - one) as f64;
      bound += 2.0 * f64::from(first) * f64::from(second) * apart * apart * apart;
    }
  }
  bound
}

/// How far past a place the square of the tip's distance is sure to go on
/// falling, from its `slope` and that slope's rate (`bend`) there, its third
/// derivative being at most `bound`: the slope further on is at most
/// `slope + bend × t + bound × t² / 2`, which is below 0 up to its first
/// root. None, where it is not falling there
fn falling_for(slope: f32, bend: f32, bound: f64) -> f32 {
  let (slope, bend) = (f64::from(slope), f64::from(bend));
  if !(slope < 0.0 || (slope == 0.0 && bend < 0.0)) {
    return 0.0;
  }
  if bound <= 0.0 {
    return f32::MAX;
  }
  let root = (bend * bend - 2.0 * bound * slope).sqrt();
  // Each form where it does not take one near number from another
  let sure = if bend <= 0.0 {
    (root - bend) / bound
  } else {
    -2.0 * slope / (bend + root)
  };
  sure as f32
}

/// Walks a chain's family from the place `from`, where the tip is `here`
/// from the first joint, in steps of `step` as far as `end`, for the first
/// place where the tip is at least `distance` away. `None` when the family
/// ends first, or, with `stop`, when the tip's distance turns back first
fn rise(
  at: &impl Fn(f32) -> (f32, f32),
  from: f32,
  here: f32,
  step: f32,
  end: f32,
  distance: f32,
  stop: bool,
) -> Option<f32> {
  let (mut held, mut at_held) = (from, here);
  for _ in 0..CHAIN_WALK {
    let next = if step > 0.0 {
      (held + step).min(end)
    } else {
      (held + step).max(end)
    };
    let (at_next, _) = at(next);
    if at_next >= distance {
      return Some(halved(at, held, next, distance));
    }
    if (stop && at_next < at_held) || next == end {
      return None;
    }
    held = next;
    at_held = at_next;
  }
  None
}

/// The place between `held` and `crossed` where the tip is `distance` from
/// the first joint: at `held` it is on one side of that, and at `crossed`
/// on the other
fn halved(at: &impl Fn(f32) -> (f32, f32), mut held: f32, mut crossed: f32, distance: f32) -> f32 {
  let short = at(held).0 < distance;
  for _ in 0..CHAIN_HALVINGS {
    let middle = 0.5 * (held + crossed);
    if (at(middle).0 < distance) == short {
      held = middle;
    } else {
      crossed = middle;
    }
  }
  0.5 * (held + crossed)
}

/// Whether every joint is within `near` of one line through the first
/// joint: the line to whichever of them is farthest from it
fn on_one_line(joints: &[Vec2], near: f32) -> bool {
  let first = joints[0];
  let points = || joints[1..].iter().map(move |joint| *joint - first);
  let farthest = points().fold(Vec2::ZERO, |farthest, point| {
    if point.length_squared() > farthest.length_squared() {
      point
    } else {
      farthest
    }
  });
  let along = farthest.normalize_or_zero();
  points().all(|point| along.perp_dot(point).abs() <= near)
}

/// The turn from one direction to another, the short way round
fn turn(from: Vec2, to: Vec2) -> f32 {
  from.perp_dot(to).atan2(from.dot(to))
}

/// Whether a bone mirrors what it carries: the determinant of its linear
/// part, `scale_x × scale_y × cos(shear_x - shear_y)`, is negative
fn mirrors(bone: &BonePose) -> bool {
  (bone.scale_x < 0.0) ^ (bone.scale_y < 0.0) ^ ((bone.shear_x - bone.shear_y).cos() < 0.0)
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
/// `distance`, by `mix`, with `distance / reach` taken no higher than
/// `max` when there is one. Compress is below 1 and a limit is at least 1,
/// so it never limits compress
fn scale_factor(distance: f32, reach: f32, mix: f32, max: Option<f32>) -> f32 {
  let ratio = distance / reach;
  1.0 + (max.map_or(ratio, |max| ratio.min(max)) - 1.0) * mix
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
  length.is_finite()
    && target.is_finite()
    && values.mix.is_finite()
    && values.softness.is_finite()
    && values.max_stretch.is_none_or(f32::is_finite)
}

#[cfg(test)]
mod tests {
  use std::f32::consts::{FRAC_PI_2, FRAC_PI_3, FRAC_PI_6, PI, TAU};

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

  /// The way two bones bend: the positive way or the negative
  fn bent(positive: bool) -> IkBend {
    if positive {
      IkBend::Positive
    } else {
      IkBend::Negative
    }
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
      bend: Some(IkBend::Negative),
      compress: true,
      stretch: false,
      max_stretch: Some(1.5),
      uniform: true,
    };
    assert_eq!(
      IkValues::of(&setup),
      IkValues {
        mix: 0.25,
        softness: 3.0,
        bend: IkBend::Negative,
        compress: true,
        stretch: false,
        max_stretch: Some(1.5),
      }
    );
    let setup = IkSetup {
      compress: false,
      stretch: true,
      ..setup
    };
    let values = IkValues::of(&setup);
    assert!(values.stretch && !values.compress, "{values:?}");
    assert_eq!(values.max_stretch, Some(1.5));
    assert_eq!(
      IkValues::of(&IkSetup::default()),
      IkValues {
        mix: 1.0,
        softness: 0.0,
        bend: IkBend::Positive,
        compress: false,
        stretch: false,
        max_stretch: None,
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
        for is_positive in [true, false] {
          let what = format!("{rig}, target {target}, bend positive: {is_positive}");
          let values = IkValues {
            bend: bent(is_positive),
            ..values()
          };
          let (a, b) = solved((first, second), length, ORIGIN + target, values, false);
          assert_near(tip(&a, &b, length), ORIGIN + target, &what);
          let (at, cross) = if is_positive {
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
    for (what, target, positive, at, cross) in [
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
        bend: bent(positive),
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
        bend: bent(!positive),
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
      for positive in [true, false] {
        let what = format!("{distance} away, bend positive: {positive}");
        let values = IkValues {
          bend: bent(positive),
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
      for positive in [true, false] {
        let what = format!("{what}, bend positive: {positive}");
        // Compress is one bone's: two bones fold, and their scales are left
        let values = IkValues {
          bend: bent(positive),
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
    for positive in [true, false] {
      let what = format!("off the axis, scaled, bend positive: {positive}");
      let values = IkValues {
        bend: bent(positive),
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
  fn stretch_stops_at_its_limit_and_the_limb_points_on() {
    let capped = IkValues {
      stretch: true,
      max_stretch: Some(1.5),
      ..values()
    };
    // One bone of length 30 scaled (2, 3): reach 60. A target 180 below is
    // 3 times that; the limit holds scale_x to 2 x 1.5 = 3 (and uniform
    // scale_y to 4.5), so the tip is 90 below, on the line to the target
    let start = BonePose {
      scale_x: 2.0,
      scale_y: 3.0,
      ..bone(ORIGIN.x, ORIGIN.y, 0.0)
    };
    let far = ORIGIN + Vec2::new(0.0, 180.0);
    let after = pointed(start, 30.0, far, capped, true);
    assert_close(after.scale_x, 3.0, "one bone");
    assert_close(after.scale_y, 4.5, "one bone uniform");
    assert_near(end(&after, 30.0), ORIGIN + Vec2::new(0.0, 90.0), "one bone");
    // Mix 0.5: 1 + (1.5 - 1) x 0.5 = 1.25, not 1 + (3 - 1) x 0.5
    let half = IkValues { mix: 0.5, ..capped };
    let after = pointed(start, 30.0, far, half, true);
    assert_close(after.scale_x, 2.5, "one bone, mix 0.5");
    assert_close(after.scale_y, 3.75, "one bone, mix 0.5");
    // A target within the limit is reached: 1.2 times the reach is 72
    let near = ORIGIN + Vec2::new(0.0, 72.0);
    let after = pointed(start, 30.0, near, capped, false);
    assert_near(end(&after, 30.0), near, "one bone within the limit");
    assert_close(after.scale_x, 2.4, "one bone within the limit");
    // No limit stretches all the way
    let after = pointed(
      start,
      30.0,
      far,
      IkValues {
        max_stretch: None,
        ..capped
      },
      false,
    );
    assert_close(after.scale_x, 6.0, "one bone, no limit");

    // Two bones, reach 70, a target 210 straight below (3 times): scale_x
    // 1.5, the joint 60 below and the tip 105 below, on the line
    let (first, second) = limb();
    let target = ORIGIN + Vec2::new(0.0, 210.0);
    let (a, b) = solved((first, second), 30.0, target, capped, true);
    assert_close(a.scale_x, 1.5, "two bones");
    assert_close(a.scale_y, 1.5, "two bones uniform");
    assert_near(joint(&a, &b), ORIGIN + Vec2::new(0.0, 60.0), "two bones");
    assert_near(
      tip(&a, &b, 30.0),
      ORIGIN + Vec2::new(0.0, 105.0),
      "two bones",
    );
    assert!(bend(&a, &b, 30.0).abs() < 0.01, "straight");
    // And off to a side: still pointing at the target
    let toward = Vec2::new(0.6, 0.8);
    let (a, b) = solved(
      (first, second),
      30.0,
      ORIGIN + toward * 300.0,
      capped,
      false,
    );
    assert_near(
      tip(&a, &b, 30.0),
      ORIGIN + toward * 105.0,
      "two bones aslant",
    );
    // Mix 0.5 uses the capped factor too
    let (a, _) = solved((first, second), 30.0, target, half, true);
    assert_close(a.scale_x, 1.25, "two bones, mix 0.5");
    assert_close(a.scale_y, 1.25, "two bones, mix 0.5");
    // Within the limit: 105 away is reached
    let (a, b) = solved(
      (first, second),
      30.0,
      ORIGIN + Vec2::new(0.0, 105.0),
      capped,
      false,
    );
    assert_near(tip(&a, &b, 30.0), ORIGIN + Vec2::new(0.0, 105.0), "within");
    assert_close(a.scale_x, 1.5, "within");
    // A limit that is not finite changes nothing
    let bad = IkValues {
      max_stretch: Some(f32::NAN),
      ..capped
    };
    let (a, b) = solved((first, second), 30.0, target, bad, true);
    assert_eq!(bits(&a), bits(&first));
    assert_eq!(bits(&b), bits(&second));
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
        for positive in [true, false] {
          for softness in [0.0, 8.0] {
            let values = IkValues {
              bend: bent(positive),
              softness,
              ..values()
            };
            let side = if positive { 1.0 } else { -1.0 };
            let mut last: Option<(Vec2, Vec2)> = None;
            for &from_origin in path {
              let what = format!(
                "{rig}, {walk}, bend positive: {positive}, softness {softness}, target {from_origin}"
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
      for (target, positive, at, cross) in [
        (Vec2::new(58.0, 12.0), true, Vec2::new(40.0, -12.0), 1176.0),
        (Vec2::new(36.0, 22.0), false, Vec2::new(12.0, 40.0), -1176.0),
      ] {
        let what = format!("scale ({scale_x}, {scale_y}), bend positive: {positive}");
        let values = IkValues {
          bend: bent(positive),
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
        bend: IkBend::Negative,
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

  /// A first bone at `ORIGIN` with this scale and x shear
  fn uneven(scale_x: f32, scale_y: f32, shear_x: f32) -> BonePose {
    BonePose {
      scale_x,
      scale_y,
      shear_x,
      ..bone(ORIGIN.x, ORIGIN.y, 0.3)
    }
  }

  /// The second bone's places these tests try: on the first's axis and off
  /// it
  const PLACES: [Vec2; 2] = [Vec2::new(40.0, 0.0), Vec2::new(40.0, 12.0)];

  /// The spread between the longest and the shortest that `first` makes
  /// part two as the second bone turns through a full turn
  fn spread(first: &BonePose, second: &BonePose, length: f32) -> f32 {
    let lengths = (0..720).map(|half_degree| {
      let turned = BonePose {
        rotation: (half_degree as f32 * 0.5).to_radians(),
        ..*second
      };
      tip(first, &turned, length).distance(joint(first, &turned))
    });
    let (shortest, longest) = lengths.fold((f32::MAX, 0.0_f32), |(low, high), length| {
      (low.min(length), high.max(length))
    });
    longest - shortest
  }

  /// The worst miss of a limb of length 30 under `first`, as a share of its
  /// straight reach: over a second bone at each of `PLACES`, started at
  /// every twelfth of a turn; and both bend sides, toward targets in four
  /// directions at every twentieth of the way from the folded reach to the
  /// straight reach, both reaches among them. No miss may be more than the
  /// spread of part two's length. Every pose solved, there and beyond the
  /// straight reach, must be finite, bent to the side asked for, and
  /// changed in its rotations alone
  fn worst_share(what: &str, first: BonePose) -> f32 {
    let mut worst = 0.0_f32;
    for place in PLACES {
      for twelfth in 0..12 {
        let second = bone(place.x, place.y, twelfth as f32 * FRAC_PI_6);
        let (folded, straight) = reaches(&first, &second, 30.0);
        let spread = spread(&first, &second, 30.0);
        for twentieth in 0..=26 {
          let fraction = twentieth as f32 * 0.05;
          let distance = folded + (straight - folded) * fraction;
          for quarter in 0..4 {
            let target = ORIGIN + Vec2::from_angle(0.4 + quarter as f32 * FRAC_PI_2) * distance;
            for positive in [true, false] {
              let what = format!(
                "{what}, second bone at {place} turned {twelfth} twelfths, {fraction} of the way, target {target}, bend positive: {positive}"
              );
              let values = IkValues {
                bend: bent(positive),
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
              let turn = bend(&a, &b, 30.0) * if positive { 1.0 } else { -1.0 };
              assert!(turn > -1.0, "{what}: a cross of {turn}");
              assert_only_turned(&first, &a, &what);
              assert_only_turned(&second, &b, &what);
              if twentieth <= 20 {
                let miss = tip(&a, &b, 30.0).distance(target);
                assert!(
                  miss < spread + 0.01,
                  "{what}: a miss of {miss}, above the spread of {spread}"
                );
                worst = worst.max(miss / straight);
              }
            }
          }
        }
      }
    }
    worst
  }

  /// The farthest the tip moves in one step as a target is walked straight
  /// out in steps of `step`, from 1 away from the first bone's origin to
  /// 1.2 times the limb's straight reach, so through its folded reach and
  /// its straight one: over three lines, both bend sides, a second bone at
  /// each of `PLACES`, started at every sixth of a turn
  fn largest_tip_step(first: BonePose, step: f32) -> f32 {
    let mut largest = 0.0_f32;
    for place in PLACES {
      for sixth in 0..6 {
        let second = bone(place.x, place.y, sixth as f32 * FRAC_PI_3);
        let (_, straight) = reaches(&first, &second, 30.0);
        let steps = ((straight * 1.2 - 1.0) / step) as usize;
        for line in [0.4_f32, 2.0, -1.2] {
          for positive in [true, false] {
            let values = IkValues {
              bend: bent(positive),
              ..values()
            };
            let mut last: Option<Vec2> = None;
            for index in 0..=steps {
              let target = ORIGIN + Vec2::from_angle(line) * (1.0 + index as f32 * step);
              let (a, b) = solved((first, second), 30.0, target, values, false);
              let tip = tip(&a, &b, 30.0);
              if let Some(last) = last {
                largest = largest.max(tip.distance(last));
              }
              last = Some(tip);
            }
          }
        }
      }
    }
    largest
  }

  #[test]
  fn an_uneven_first_bone_misses_by_a_bounded_amount() {
    // The recorded limit. Under a first bone whose own scale is uneven, or
    // whose axes are sheared out of square, part two's length changes as
    // the second bone turns. The triangle is solved once, with part two as
    // long as it is in the pose passed in, so the second bone is pointed
    // the right way and comes out another length: the tip is off its
    // target, along part two, by the difference.
    //
    // That difference is at most the spread between the longest and the
    // shortest the first bone makes part two, which `worst_share` holds
    // every miss to. Under a scale of (a, 1), with part two 30 long
    // unscaled, the spread is 30 (a - 1); and with the second bone 40
    // along the first's axis the straight reach is 70 a. So the miss is at
    // most 3 (a - 1) / 7 a of the reach: 7.1% at (1.2, 1) and 21.4% at
    // (2, 1), and the worst miss found comes close to it.
    //
    // The lower bounds are deliberate: a solve that removes the limit fails
    // here on purpose, and must restate the limit in the module notes and
    // the design
    let pinned = [
      ("a scale of (1.2, 1)", uneven(1.2, 1.0, 0.0), 0.05, 0.1),
      ("a scale of (2, 1)", uneven(2.0, 1.0, 0.0), 0.15, 0.3),
    ];
    for (what, first, least, most) in pinned {
      let share = worst_share(what, first);
      assert!(
        share > least && share < most,
        "{what}: the worst miss is {share} of the reach, outside {least} to {most}"
      );
    }
    // Every other bone tried is as finite, and held to its spread: 15 at
    // (1.5, 1) and at (1, 1.5), 60 at (3, 1), 8.97 under an x shear of 0.3
    // and 17.7 under one of 0.6
    for (what, first) in [
      ("a scale of (1.5, 1)", uneven(1.5, 1.0, 0.0)),
      ("a scale of (3, 1)", uneven(3.0, 1.0, 0.0)),
      ("a scale of (1, 1.5)", uneven(1.0, 1.5, 0.0)),
      ("an x shear of 0.3", uneven(1.0, 1.0, 0.3)),
      ("an x shear of 0.6", uneven(1.0, 1.0, 0.6)),
    ] {
      let share = worst_share(what, first);
      assert!(
        share < 0.35,
        "{what}: the worst miss is {share} of the reach"
      );
    }

    // Through its reaches the tip follows a target steeply, and without a
    // jump. Under a scale of (1.5, 1), a target walked out through both
    // reaches in steps of 0.1 never moves the tip more than 2 in a step,
    // and in steps of 0.01 never more than 0.7. A smaller step moves the
    // tip less, though not in proportion: where the solve passes from bent
    // to straight or to folded, the joint's place goes as a square root of
    // the target's, as it does under an even bone, and here the tip shows
    // it
    let first = uneven(1.5, 1.0, 0.0);
    for (step, most) in [(0.1, 2.0), (0.01, 0.7)] {
      let largest = largest_tip_step(first, step);
      assert!(
        largest < most,
        "a scale of (1.5, 1): the tip moved {largest} for a step of {step}, above {most}"
      );
    }
  }

  /// A chain's joints in the space of its first bone's parent, each bone's
  /// origin and then the tip: the bones' own transforms composed, as placing
  /// a pose composes them
  fn chain_joints(bones: &[BonePose], length: f32) -> Vec<Vec2> {
    let mut joints = Vec::with_capacity(bones.len() + 1);
    chain_joints_into(bones, length, &mut joints);
    joints
  }

  /// `chain_joints` into a list a walk keeps
  fn chain_joints_into(bones: &[BonePose], length: f32, joints: &mut Vec<Vec2>) {
    let mut carried = Affine2::IDENTITY;
    joints.clear();
    for bone in bones {
      joints.push(carried.transform_point2(Vec2::new(bone.x, bone.y)));
      carried *= bone.affine();
    }
    joints.push(carried.transform_point2(Vec2::new(length, 0.0)));
  }

  /// A chain's tip: the last of its joints
  fn chain_tip(bones: &[BonePose], length: f32) -> Vec2 {
    chain_joints(bones, length)[bones.len()]
  }

  /// A chain's length: its parts', joint to joint
  fn chain_length(bones: &[BonePose], length: f32) -> f32 {
    chain_joints(bones, length)
      .windows(2)
      .map(|pair| pair[0].distance(pair[1]))
      .sum()
  }

  /// The turn at each joint after the first, from the part before it to the
  /// part after: positive the positive way
  fn chain_turns(bones: &[BonePose], length: f32) -> Vec<f32> {
    chain_joints(bones, length)
      .windows(3)
      .map(|three| {
        let (before, after) = (three[1] - three[0], three[2] - three[1]);
        before.perp_dot(after).atan2(before.dot(after))
      })
      .collect()
  }

  /// A chain of unit bones with the tip's length: the first at `ORIGIN`
  /// turned `first`, each later one at the end of the one before and turned
  /// from it by the next of `each`, taken round and round. Its parts are
  /// `lengths` long
  fn links_each(lengths: &[f32], first: f32, each: &[f32]) -> (Vec<BonePose>, f32) {
    let mut bones = vec![bone(ORIGIN.x, ORIGIN.y, first)];
    let mut last = 0.0;
    for (index, length) in lengths.iter().enumerate() {
      if index > 0 {
        bones.push(bone(last, 0.0, each[(index - 1) % each.len()]));
      }
      last = *length;
    }
    (bones, last)
  }

  /// `links_each` with one turn for every joint
  fn links(lengths: &[f32], first: f32, each: f32) -> (Vec<BonePose>, f32) {
    links_each(lengths, first, &[each])
  }

  /// Four bones, their parts 30, 25, 25 and 20 long (100 together), bent
  /// the positive way by 0.5 rad at each joint
  fn four() -> (Vec<BonePose>, f32) {
    links(&[30.0, 25.0, 25.0, 20.0], 0.3, 0.5)
  }

  /// Sixteen bones, their parts 10 long (160 together), bent the positive
  /// way by 0.1 rad at each joint
  fn sixteen() -> (Vec<BonePose>, f32) {
    links(&[10.0; 16], -0.4, 0.1)
  }

  /// `bones` solved toward `target`, closing the way the pose bends
  fn reached(bones: &[BonePose], length: f32, target: Vec2, mix: f32) -> Vec<BonePose> {
    reached_on(bones, length, target, mix, None)
  }

  /// `bones` solved toward `target`, kept to `side` (1 the positive side, -1
  /// the negative), or closing the way the pose bends (`None`)
  fn reached_on(
    bones: &[BonePose],
    length: f32,
    target: Vec2,
    mix: f32,
    side: Option<f32>,
  ) -> Vec<BonePose> {
    let mut bones = bones.to_vec();
    chain(&mut bones, length, target, mix, side, &mut Vec::new());
    bones
  }

  /// How near its first joint a pose's family of shapes brings its tip: the
  /// tip's distance from it when the target is a thousandth of the chain's
  /// length away
  fn chain_nearest(bones: &[BonePose], length: f32) -> f32 {
    let beside = Vec2::new(0.6, 0.8) * (0.001 * chain_length(bones, length));
    chain_tip(&reached(bones, length, ORIGIN + beside, 1.0), length).distance(ORIGIN)
  }

  /// What composing a chain's bones adds to where its solve put the joints,
  /// in rounding, ten times over: 0.00006 of a unit is the most these tests'
  /// chains show
  const COMPOSED: f32 = 0.001;

  /// The tip is within `CHAIN_NEAR ×` the chain's length of its target
  #[track_caller]
  fn assert_on_target(bones: &[BonePose], length: f32, target: Vec2, what: &str) {
    let near = CHAIN_NEAR * chain_length(bones, length);
    let miss = chain_tip(bones, length).distance(target);
    assert!(
      miss <= near + COMPOSED,
      "{what}: the tip is {miss} from its target, and {near} is near"
    );
  }

  /// Nothing of any bone but its rotation changed, to the bit
  #[track_caller]
  fn assert_chain_only_turned(before: &[BonePose], after: &[BonePose], what: &str) {
    assert_eq!(before.len(), after.len(), "{what}");
    for (before, after) in before.iter().zip(after) {
      assert_only_turned(before, after, what);
    }
  }

  fn chain_bits(bones: &[BonePose]) -> Vec<[u32; 7]> {
    bones.iter().map(bits).collect()
  }

  /// Fourteen chains of 3 to 16 bones: even and uneven parts, bent one way
  /// hard and gently, curled up, and bent in an S
  fn walked_rigs() -> Vec<(&'static str, Vec<BonePose>, f32)> {
    let mut rigs = Vec::new();
    let mut add = |name, (bones, length)| rigs.push((name, bones, length));
    add("3 of 20, 0.4 a joint", links(&[20.0; 3], 0.0, 0.4));
    add(
      "30, 20, 10, 0.4 a joint",
      links(&[30.0, 20.0, 10.0], 0.2, 0.4),
    );
    add("3 of 20, 0.1 a joint", links(&[20.0; 3], 0.0, 0.1));
    add("30, 25, 25, 20, 0.5 a joint", four());
    add("4 of 25, 0.15 a joint", links(&[25.0; 4], 0.0, 0.15));
    add(
      "4 of 25 in an S",
      links_each(&[25.0; 4], 0.0, &[0.4, -0.5, 0.4]),
    );
    add(
      "10, 30, 15, 25, 20, -0.6 a joint",
      links(&[10.0, 30.0, 15.0, 25.0, 20.0], 0.0, -0.6),
    );
    add("8 of 12, 0.25 a joint", links(&[12.0; 8], 0.0, 0.25));
    add("8 of 12, 0.05 a joint", links(&[12.0; 8], 0.0, 0.05));
    add(
      "8 of 12 in an S",
      links_each(&[12.0; 8], 0.0, &[0.3, 0.3, -0.3, -0.3]),
    );
    add("16 of 10, 0.1 a joint", sixteen());
    add("16 of 10, 0.03 a joint", links(&[10.0; 16], 0.0, 0.03));
    add("16 of 10, 0.3 a joint", links(&[10.0; 16], 0.0, 0.3));
    add(
      "16 of 10 in an S",
      links_each(&[10.0; 16], 0.0, &[0.2, 0.2, 0.2, -0.2, -0.2, -0.2]),
    );
    rigs
  }

  /// A path a target is walked along, in lengths of the chain from its
  /// first joint
  #[derive(Clone, Copy)]
  enum Stroke {
    /// From one point to another
    Line(Vec2, Vec2),
    /// Once round a centre, at a radius
    Circle(Vec2, f32),
  }

  impl Stroke {
    fn length(self) -> f32 {
      match self {
        Stroke::Line(from, to) => from.distance(to),
        Stroke::Circle(_, radius) => TAU * radius,
      }
    }

    /// The target `along` the stroke (0 to 1), for a chain `total` long
    fn at(self, along: f32, total: f32) -> Vec2 {
      let point = match self {
        Stroke::Line(from, to) => from.lerp(to, along),
        Stroke::Circle(centre, radius) => centre + Vec2::from_angle(along * TAU) * radius,
      };
      ORIGIN + point * total
    }
  }

  /// Twelve strokes: out along four lines from the first joint, round it at
  /// four distances, straight across missing it as a pointer does, and
  /// round two points away from it
  const STROKES: [Stroke; 12] = [
    Stroke::Line(Vec2::new(0.3, 0.0), Vec2::new(1.0, 0.0)),
    Stroke::Line(Vec2::new(0.0, 0.3), Vec2::new(0.0, 1.0)),
    Stroke::Line(Vec2::new(-0.212, 0.212), Vec2::new(-0.707, 0.707)),
    Stroke::Line(Vec2::new(0.18, -0.24), Vec2::new(0.6, -0.8)),
    Stroke::Circle(Vec2::ZERO, 0.3),
    Stroke::Circle(Vec2::ZERO, 0.6),
    Stroke::Circle(Vec2::ZERO, 0.9),
    Stroke::Circle(Vec2::ZERO, 0.97),
    Stroke::Line(Vec2::new(-1.1, 0.25), Vec2::new(1.1, 0.25)),
    Stroke::Line(Vec2::new(0.4, -1.1), Vec2::new(0.4, 1.1)),
    Stroke::Circle(Vec2::new(0.5, 0.2), 0.25),
    Stroke::Circle(Vec2::new(-0.3, 0.5), 0.3),
  ];

  /// Walks a target along a stroke in steps of `step` of the chain's
  /// length, solving the same pose at each. A step that moves a joint more
  /// than three steps and a thousandth of the length is halved ten times,
  /// toward the half the joints move more in, and what a joint still moves
  /// across the last 1/1024 of the step is a jump: a chain that only
  /// follows its target steeply moves less the smaller the step, and one
  /// that pops does not. Gives the solves made, the largest jump as a share
  /// of the length, and how many steps left the tip off a target the pose
  /// can meet: one inside the chain's length and outside its nearest shape
  fn walked(bones: &[BonePose], length: f32, stroke: Stroke, step: f32) -> (usize, f32, usize) {
    let total = chain_length(bones, length);
    let nearest = chain_nearest(bones, length) + 0.002 * total;
    let steps = (stroke.length() / step).ceil() as usize;
    let (mut work, mut scratch, mut solves, mut missed) = (bones.to_vec(), Vec::new(), 0, 0);
    let solve = |along: f32, joints: &mut Vec<Vec2>| {
      let target = stroke.at(along, total);
      work.copy_from_slice(bones);
      chain(&mut work, length, target, 1.0, None, &mut scratch);
      chain_joints_into(&work, length, joints);
      solves += 1;
      let met = joints[bones.len()].distance(target) <= CHAIN_NEAR * total + COMPOSED;
      if !met && (nearest..total).contains(&target.distance(ORIGIN)) {
        missed += 1;
      }
    };
    let jump = largest_jump(steps, step, total, solve);
    (solves, jump, missed)
  }

  /// The jump rule of `walked`: `solve` gives the joints at a place along
  /// a walk (0 to 1) made in `steps` steps, each moving the target `step`
  /// of the chain's length `total`. Gives the largest jump, as a share of
  /// the length
  fn largest_jump(
    steps: usize,
    step: f32,
    total: f32,
    mut solve: impl FnMut(f32, &mut Vec<Vec2>),
  ) -> f32 {
    let moved = |from: &[Vec2], to: &[Vec2]| {
      from
        .iter()
        .zip(to)
        .map(|(from, to)| from.distance(*to))
        .fold(0.0, f32::max)
        / total
    };
    let (mut before, mut here) = (Vec::new(), Vec::new());
    let (mut from, mut to, mut middle) = (Vec::new(), Vec::new(), Vec::new());
    let mut jump = 0.0_f32;
    solve(0.0, &mut before);
    for index in 1..=steps {
      let (start, end) = (
        (index - 1) as f32 / steps as f32,
        index as f32 / steps as f32,
      );
      solve(end, &mut here);
      if moved(&before, &here) > 3.0 * step + 0.001 {
        let (mut start, mut end) = (start, end);
        from.clone_from(&before);
        to.clone_from(&here);
        for _ in 0..10 {
          let half = 0.5 * (start + end);
          solve(half, &mut middle);
          if moved(&from, &middle) >= moved(&middle, &to) {
            end = half;
            std::mem::swap(&mut to, &mut middle);
          } else {
            start = half;
            std::mem::swap(&mut from, &mut middle);
          }
        }
        jump = jump.max(moved(&from, &to));
      }
      std::mem::swap(&mut before, &mut here);
    }
    jump
  }

  #[test]
  fn a_chain_does_not_pop_as_its_target_moves() {
    // Every one of the fourteen chains, its target walked along every one
    // of the twelve strokes in steps of a thousandth of its length: 168
    // strokes and 400,000 solves, each from the same pose. A chain's shape
    // follows its target's distance alone, along one family of shapes, and
    // the target's direction only turns it: nowhere does a joint jump by
    // 1% of the chain's length. Reaching for the target from the pose
    // afresh at each step, in sweeps back and out along the joints, jumped
    // so on 53 of these strokes, 91 times, by up to 0.95 of the length
    let (mut solves, mut worst) = (0, 0.0_f32);
    for (rig, bones, length) in walked_rigs() {
      for (index, stroke) in STROKES.into_iter().enumerate() {
        let (made, jump, missed) = walked(&bones, length, stroke, 0.001);
        assert!(
          jump < 0.01,
          "{rig}, stroke {index}: a joint jumps {jump} of the length"
        );
        // And the tip is on its target all the way, wherever the pose can
        // put it there
        assert_eq!(missed, 0, "{rig}, stroke {index}: steps off the target");
        solves += made;
        worst = worst.max(jump);
      }
    }
    assert!((393_000..420_000).contains(&solves), "{solves} solves");
    // What is left across the last 1/1024 of the steepest step is the
    // chain following its target through its full reach, where a joint
    // moves as the square root of the target's step: 0.0007 of the length
    assert!(worst < 0.002, "the largest jump is {worst} of the length");
  }

  /// The chain solve as it stood before a chain could keep a side, kept to
  /// hold the pose's family to it, bit for bit
  fn chain_as_it_stood(
    bones: &mut [BonePose],
    length: f32,
    target: Vec2,
    mix: f32,
    joints: &mut Vec<Vec2>,
  ) {
    let count = bones.len();
    let given = length.is_finite() && target.is_finite() && mix.is_finite();
    if count < 2 || !given || mix <= 0.0 || !bones.iter().all(finite) {
      return;
    }
    let mix = mix.min(1.0);

    joints.clear();
    joints.resize(chain_room(count), Vec2::ZERO);
    let (joints, parts) = joints.split_at_mut(count + 1);
    let (stood, sizes) = parts.split_at_mut(count);

    let mut carried = Affine2::IDENTITY;
    for (joint, bone) in joints.iter_mut().zip(bones.iter()) {
      *joint = carried.transform_point2(Vec2::new(bone.x, bone.y));
      carried *= bone.affine();
    }
    joints[count] = carried.transform_point2(Vec2::new(length, 0.0));
    let mut total = 0.0;
    for index in 0..count {
      let part = joints[index + 1] - joints[index];
      let long = part.length();
      stood[index] = part;
      sizes[index].x = if long > NO_LENGTH { long } else { 0.0 };
      total += sizes[index].x;
    }
    let placed = total.is_finite() && joints.iter().all(|joint| joint.is_finite());
    let to_target = target - joints[0];
    let distance = to_target.length();
    if !placed || total <= NO_LENGTH || distance <= NO_LENGTH {
      return;
    }

    let (mut before, mut heading, mut swung, mut long) = (None::<Vec2>, 0.0, 0.0, 0.0);
    let (mut turns, mut swings) = ((0.0_f32, 0.0_f32), (0.0_f32, 0.0_f32));
    for index in 0..count {
      if sizes[index].x > 0.0 {
        match before {
          Some(before) => {
            let by = turn(before, stood[index]);
            swung += by;
            turns = (turns.0.min(by), turns.1.max(by));
            swings = (swings.0.min(swung), swings.1.max(swung));
          }
          None => heading = stood[index].to_angle(),
        }
        before = Some(stood[index]);
        long += 1.0;
      }
      sizes[index].y = swung;
    }
    let sizes_now = &*sizes;
    let reach = |shape: Shape| super::reached(sizes_now, shape);

    let shape = if distance >= total || long < 2.0 {
      Shape::STRAIGHT
    } else if on_one_line(joints, CHAIN_NEAR * total) {
      let step = CHAIN_STEP / (long - 1.0);
      Shape {
        factor: 0.0,
        added: fall(sizes_now, Shape::STRAIGHT, 1.0, step, PI, distance),
      }
    } else {
      let (_, pose, by_factor, by_added) = reach(Shape::POSE);
      if distance > pose {
        let scaled = |factor: f32| {
          let (_, far, by_factor, _) = reach(Shape { factor, added: 0.0 });
          (far, by_factor)
        };
        let step = CHAIN_STEP / (swings.1 - swings.0).max(f32::MIN_POSITIVE);
        let folded = PI / turns.1.max(-turns.0);
        let further = if by_factor > 0.0 {
          rise(&scaled, 1.0, pose, step, folded, distance, true)
        } else {
          None
        };
        let opened = || rise(&scaled, 1.0, pose, -step.min(0.125), 0.0, distance, false);
        Shape {
          factor: further.or_else(opened).unwrap_or(0.0),
          added: 0.0,
        }
      } else {
        let way = if by_added > 0.0 { -1.0 } else { 1.0 };
        let folded = if way > 0.0 {
          PI - turns.1
        } else {
          PI + turns.0
        };
        let step = CHAIN_STEP / (long - 1.0);
        let curl = fall(sizes_now, Shape::POSE, way, step, folded.max(0.0), distance);
        Shape {
          factor: 1.0,
          added: curl * way,
        }
      }
    };

    let (tip, far, ..) = reach(shape);
    if far > NO_LENGTH {
      heading = to_target.to_angle() - tip.to_angle();
    }
    let mut joints_before = 0.0;
    for index in 0..count {
      let swing = sizes[index].y * shape.factor + shape.added * joints_before;
      joints[index + 1] = joints[index] + Vec2::from_angle(heading + swing) * sizes[index].x;
      if sizes[index].x > 0.0 {
        joints_before += 1.0;
      }
    }

    let (mut before, mut mirrored) = (0.0, false);
    for index in 0..count {
      let swung = if sizes[index].x > 0.0 {
        turn(stood[index], joints[index + 1] - joints[index])
      } else {
        before
      };
      let more = if mirrored {
        before - swung
      } else {
        swung - before
      };
      sizes[index].y = bones[index].rotation + shortest(more) * mix;
      before = swung;
      mirrored ^= mirrors(&bones[index]);
    }
    if sizes.iter().all(|size| size.y.is_finite()) {
      for (bone, size) in bones.iter_mut().zip(sizes.iter()) {
        bone.rotation = size.y;
      }
    }
  }

  #[test]
  fn a_pose_chain_is_unchanged() {
    // A chain that closes the way its pose bends is solved as it was before
    // a chain could keep a side: over the not-pop test's 168 strokes, in
    // steps of a thousandth of the length, at a mix of 1 and of a half by
    // turns, every bone comes out the same to the bit as the solve above
    // gives it
    let (mut now, mut then, mut scratch) = (Vec::new(), Vec::new(), Vec::new());
    let mut solves = 0;
    for (rig, bones, length) in walked_rigs() {
      let total = chain_length(&bones, length);
      for (index, stroke) in STROKES.into_iter().enumerate() {
        let steps = (stroke.length() / 0.001).ceil() as usize;
        for at in 0..=steps {
          let target = stroke.at(at as f32 / steps as f32, total);
          let mix = if at % 2 == 0 { 1.0 } else { 0.5 };
          now.clone_from(&bones);
          then.clone_from(&bones);
          chain(&mut now, length, target, mix, None, &mut scratch);
          chain_as_it_stood(&mut then, length, target, mix, &mut scratch);
          assert_eq!(
            chain_bits(&now),
            chain_bits(&then),
            "{rig}, stroke {index}, step {at}"
          );
          solves += 1;
        }
      }
    }
    assert!((393_000..420_000).contains(&solves), "{solves} solves");
  }

  /// The joints a chain's turns put on `side` of the line from its first
  /// joint to `target`, but for the first joint and the tip: none, when
  /// every one of them is on that side
  fn off_side(joints: &[Vec2], target: Vec2, side: f32) -> usize {
    let line = target - joints[0];
    let middle = &joints[1..joints.len() - 1];
    // A joint of an arc turned the positive way lies the negative way of
    // the line through its ends, as a perp product counts it
    middle
      .iter()
      .filter(|joint| side * line.perp_dot(**joint - joints[0]) >= 0.0)
      .count()
  }

  #[test]
  fn a_fixed_side_chain_never_changes_sides_as_its_pose_sways() {
    // The two sways that flip a chain closed the way its pose bends: every
    // joint turned alike, swinging through straight, and an S-shaped wave
    // travelling down the chain. Each is swayed through two cycles while
    // its target walks from 0.3 of the length out to 0.995 and back,
    // crossing the animated tip's distance, so the pose passed in changes
    // at every step. Kept to a side, the chain never leaves it: every
    // middle joint stays on that side of the line from the first joint to
    // the target, the tip stays on the target, and the not-pop test's rule
    // finds no jump
    for (rig, lengths, amplitude) in [
      ("4 of 25", vec![25.0; 4], 0.4),
      ("30, 25, 25, 20", vec![30.0, 25.0, 25.0, 20.0], 0.4),
      ("8 of 12", vec![12.0; 8], 0.3),
      ("16 of 10", vec![10.0; 16], 0.2),
    ] {
      let count = lengths.len();
      for sway in ["through straight", "in an S"] {
        let pose = |along: f32| {
          // At its most bent where the target is farthest
          let phase = TAU * (2.0 * along + 0.25);
          let turns: Vec<f32> = (1..count)
            .map(|joint| match sway {
              "through straight" => amplitude * phase.sin(),
              _ => amplitude * (TAU * joint as f32 / count as f32 - phase).sin(),
            })
            .collect();
          links_each(&lengths, 0.4, &turns).0
        };
        let (_, length) = links(&lengths, 0.0, 0.0);
        let total = chain_length(&pose(0.0), length);
        let toward = Vec2::from_angle(0.7);
        let place = |along: f32| {
          let out = 1.0 - (2.0 * along - 1.0).abs();
          ORIGIN + toward * (total * (0.3 + 0.695 * out))
        };
        for side in [1.0, -1.0] {
          let what = format!("{rig}, swayed {sway}, side {side}");
          let (mut work, mut scratch) = (Vec::new(), Vec::new());
          let (mut off, mut missed, mut nearer, mut farther) = (0, 0, false, false);
          let solve = |along: f32, joints: &mut Vec<Vec2>| {
            let (bones, target) = (pose(along), place(along));
            let tip = chain_tip(&bones, length).distance(ORIGIN);
            let distance = target.distance(ORIGIN);
            nearer |= distance < tip;
            farther |= distance > tip;
            work.clone_from(&bones);
            chain(&mut work, length, target, 1.0, Some(side), &mut scratch);
            chain_joints_into(&work, length, joints);
            off += off_side(joints, target, side);
            if joints[count].distance(target) > CHAIN_NEAR * total + COMPOSED {
              missed += 1;
            }
          };
          let jump = largest_jump(1390, 0.001, total, solve);
          assert!(
            nearer && farther,
            "{what}: the target never crosses the tip"
          );
          assert_eq!(off, 0, "{what}: joints off their side");
          assert_eq!(missed, 0, "{what}: steps off the target");
          assert!(jump < 0.01, "{what}: a joint jumps {jump} of the length");
        }
      }
    }
  }

  #[test]
  fn a_fixed_side_chain_curls_as_an_even_arc_and_lies_straight_beyond_its_length() {
    // Kept to a side, a chain is solved from straight, not from its pose:
    // poses straight, bent either way and in an S, sent one target, come to
    // the same joints, an even arc on that side, every joint turned as much
    // as the others, curling further as the target comes nearer
    let line = Vec2::from_angle(0.3);
    let aside = Vec2::from_angle(-1.9);
    for lengths in [vec![25.0; 4], vec![30.0, 25.0, 25.0, 20.0], vec![10.0; 16]] {
      let poses = [
        links(&lengths, 0.3, 0.0),
        links(&lengths, -1.0, 0.5),
        links(&lengths, 0.8, -0.5),
        links_each(&lengths, 0.0, &[0.4, -0.5, 0.4]),
      ];
      let length = poses[0].1;
      let total = chain_length(&poses[0].0, length);
      for side in [1.0, -1.0] {
        for toward in [line, aside] {
          let mut curl = f32::MAX;
          for share in [0.2, 0.4, 0.6, 0.8, 0.95, 0.995] {
            let what = format!(
              "{} parts, side {side}, {share} of the length toward {toward}",
              lengths.len()
            );
            let target = ORIGIN + toward * (share * total);
            let mut first = None;
            for (bones, _) in &poses {
              let after = reached_on(bones, length, target, 1.0, Some(side));
              assert_on_target(&after, length, target, &what);
              assert_chain_only_turned(bones, &after, &what);
              let turns = chain_turns(&after, length);
              assert!(
                turns.iter().all(|turn| turn * side > 0.0),
                "{what}: turns of {turns:?}"
              );
              for turn in &turns {
                assert!(
                  (turn - turns[0]).abs() < 1.0e-3,
                  "{what}: turns of {turns:?}"
                );
              }
              let joints = chain_joints(&after, length);
              match &first {
                None => first = Some((joints, turns[0].abs())),
                Some((first, _)) => {
                  for (joint, same) in joints.iter().zip(first) {
                    assert_near(*joint, *same, &format!("{what}: another pose"));
                  }
                }
              }
            }
            // A nearer target curls it further
            let turned = first.map_or(0.0, |(_, turned)| turned);
            assert!(turned < curl, "{what}: turned {turned}, and {curl} nearer");
            curl = turned;
          }

          // At and beyond its length it lies straight along the line to
          // the target, whatever its pose
          for share in [1.001, 1.5, 100.0] {
            let what = format!(
              "{} parts, side {side}, {share} of the length",
              lengths.len()
            );
            let target = ORIGIN + toward * (share * total);
            for (bones, _) in &poses {
              let after = reached_on(bones, length, target, 1.0, Some(side));
              let mut along = 0.0;
              let joints = chain_joints(&after, length);
              for (index, joint) in joints.iter().enumerate() {
                assert_near(*joint, ORIGIN + toward * along, &what);
                if index < lengths.len() {
                  along += lengths[index];
                }
              }
            }
          }
        }
      }
    }

    // Four parts of 25 to a target 75 along: each turned 0.656 rad, the
    // side's way
    let (bones, length) = links(&[25.0; 4], -0.6, 0.2);
    for side in [1.0, -1.0] {
      let after = reached_on(&bones, length, ORIGIN + line * 75.0, 1.0, Some(side));
      for turn in chain_turns(&after, length) {
        assert!(
          (turn - 0.656 * side).abs() < 0.002,
          "side {side}: a turn of {turn}"
        );
      }
    }

    // The animated pose comes back by the mix alone: at a half mix each
    // bone is turned half the short way from its pose to where a mix of 1
    // puts it
    let (bones, length) = links(&[30.0, 25.0, 25.0, 20.0], 0.8, -0.5);
    let target = ORIGIN + line * 50.0;
    let full = reached_on(&bones, length, target, 1.0, Some(1.0));
    let half = reached_on(&bones, length, target, 0.5, Some(1.0));
    for ((pose, full), half) in bones.iter().zip(&full).zip(&half) {
      let expected = pose.rotation + 0.5 * shortest(full.rotation - pose.rotation);
      assert_angle(half.rotation, expected, "at a half mix");
    }
  }

  #[test]
  fn a_chain_does_not_jump_where_its_tip_s_distance_turns_inside_one_step() {
    // Two poses whose tip, as they close, comes in, goes out a hair and
    // comes in again, all inside one of the walk's largest steps. Parts of
    // 9.90, 21.64 and 18.84 turned -0.906 and -0.587: the tip's distance
    // dips to 0.17064 of the length at an added turn of about 2.0 rad a
    // joint, and is back up at 0.17078 by 2.1. Parts of 13.19, 22.63, 6.34
    // and 6.57 turned 0.412, 0.697 and 0.633 turn back at 0.36986. The walk
    // cannot step over a turn, so it stops at the first, and a target
    // walked in through it in steps of a ten thousandth of the length moves
    // no joint by a jump. A walk that looked for a turn only at each step's
    // end stepped over these, on to a shape that comes nearer: a joint
    // jumped 0.085 and 0.020 of the length as the target passed the turn
    let toward = Vec2::from_angle(0.7);
    for (what, (bones, length), nearest, stroke) in [
      (
        "9.90, 21.64 and 18.84",
        links_each(
          &[9.900633, 21.638046, 18.837463],
          0.0,
          &[-0.90585124, -0.58698565],
        ),
        0.17064,
        Stroke::Line(Vec2::new(0.25, 0.0), Vec2::new(0.12, 0.0)),
      ),
      (
        "13.19, 22.63, 6.34 and 6.57",
        links_each(
          &[13.188526, 22.632181, 6.343409, 6.571412],
          2.1846757,
          &[0.4121013, 0.6971329, 0.63312465],
        ),
        0.36986,
        Stroke::Line(toward * 0.42, toward * 0.30),
      ),
    ] {
      let (_, jump, missed) = walked(&bones, length, stroke, 0.0001);
      assert!(jump < 0.01, "{what}: a joint jumps {jump} of the length");
      assert_eq!(missed, 0, "{what}: steps off the target");
      let comes = chain_nearest(&bones, length) / chain_length(&bones, length);
      assert!(
        (comes - nearest).abs() < 0.0002,
        "{what}: as near as {comes}"
      );
    }
  }

  /// A number from 0 to 1, the next from `seed`: splitmix64, so a test's
  /// random poses are the same every run
  fn random(seed: &mut u64) -> f32 {
    *seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut mixed = *seed;
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^= mixed >> 31;
    (mixed >> 40) as f32 / (1_u64 << 24) as f32
  }

  #[test]
  fn a_chain_walked_in_from_random_poses_does_not_jump() {
    // Seeded poses, each with its target walked in along a line through
    // the first joint. A turn of the tip's distance inside one step that
    // made a jump of 1% of the length is rare in poses drawn at random,
    // about one in 1,700 of three to five bones walked in from the tip, so
    // sixty of these are drawn about the two poses that have one, each
    // part's length and each turn moved by up to 1%, and walked in through
    // where they turn in steps of 0.00025 of the length. Forty more are
    // drawn at random, three to five bones with parts of 5 to 30 and turns
    // up to 1 rad a joint, and walked in from 95% of the length to 15% in
    // steps of 0.001. No joint jumps. A walk that looked for a turn only at
    // each step's end jumped on 14 of the sixty, by up to 0.09 of the length
    let mut seed = 2026;
    let wiggling: [(&[f32], &[f32], f32); 2] = [
      (
        &[9.900633, 21.638046, 18.837463],
        &[-0.90585124, -0.58698565],
        0.0,
      ),
      (
        &[13.188526, 22.632181, 6.343409, 6.571412],
        &[0.4121013, 0.6971329, 0.63312465],
        2.1846757,
      ),
    ];
    let mut poses = Vec::new();
    for index in 0..60 {
      let (lengths, turns, first) = wiggling[index % 2];
      let mut moved = |value: f32| value * (1.0 + 0.01 * (2.0 * random(&mut seed) - 1.0));
      let lengths: Vec<f32> = lengths.iter().map(|length| moved(*length)).collect();
      let turns: Vec<f32> = turns
        .iter()
        .map(|turn| turn + 0.01 * (2.0 * random(&mut seed) - 1.0))
        .collect();
      let toward = Vec2::from_angle(TAU * random(&mut seed));
      let stroke = Stroke::Line(toward * 0.45, toward * 0.12);
      poses.push((links_each(&lengths, first, &turns), stroke, 0.00025));
    }
    for index in 0..40 {
      let count = 3 + index % 3;
      let lengths: Vec<f32> = (0..count).map(|_| 5.0 + 25.0 * random(&mut seed)).collect();
      let turns: Vec<f32> = (1..count).map(|_| 2.0 * random(&mut seed) - 1.0).collect();
      let first = TAU * random(&mut seed);
      let toward = Vec2::from_angle(TAU * random(&mut seed));
      let stroke = Stroke::Line(toward * 0.95, toward * 0.15);
      poses.push((links_each(&lengths, first, &turns), stroke, 0.001));
    }
    let mut jumped = Vec::new();
    for (index, ((bones, length), stroke, step)) in poses.into_iter().enumerate() {
      let (_, jump, missed) = walked(&bones, length, stroke, step);
      if jump >= 0.01 {
        jumped.push((index, jump));
      }
      assert_eq!(missed, 0, "pose {index}: steps off the target");
    }
    assert!(jumped.is_empty(), "joints jump: {jumped:?}");
  }

  #[test]
  fn a_chain_settles_steeply_into_its_nearest_shape() {
    // The pose that settled most steeply of 4,000 drawn at random (three
    // bones, turns up to 1 rad a joint), walked in from its tip: parts of
    // 11.90, 15.63 and 24.58 turned 0.782 and -0.976, whose nearest shape
    // is 0.00316 of the length from the first joint. Near there the shape
    // goes as the square root of how far the target is outside it, so a
    // joint moves steeply over the target's last steps: walked in along
    // sixteen lines through the first joint to 0.002 of the length, the
    // most a joint moves across the last 1/1024 of a step is 0.0116 of the
    // length in steps of 0.001 and 0.0015 in steps of 0.0001. Steep, and
    // less the smaller the step: not a jump
    let (bones, length) = links_each(
      &[11.898093, 15.626931, 24.577766],
      0.0,
      &[0.78162336, -0.9757828],
    );
    let total = chain_length(&bones, length);
    let nearest = chain_nearest(&bones, length) / total;
    assert!(
      (nearest - 0.00316).abs() < 0.00002,
      "the nearest shape is {nearest} of the length away"
    );
    let tip = chain_tip(&bones, length).distance(ORIGIN) / total;
    let mut most = [0.0_f32; 2];
    for index in 0..16 {
      let toward = Vec2::from_angle(TAU * index as f32 / 16.0);
      let stroke = Stroke::Line(toward * tip, toward * 0.002);
      for (most, step) in most.iter_mut().zip([0.001, 0.0001]) {
        let (_, moved, missed) = walked(&bones, length, stroke, step);
        assert!(
          moved < 0.015,
          "line {index}, steps of {step}: a joint moves {moved} of the length"
        );
        assert_eq!(
          missed, 0,
          "line {index}, steps of {step}: steps off the target"
        );
        *most = most.max(moved);
      }
    }
    // The case is the steep one, and a smaller step moves a joint less
    assert!(
      most[0] > 0.009 && most[1] < most[0] / 4.0,
      "a joint moves at most {} of the length in steps of 0.001, {} in steps of 0.0001",
      most[0],
      most[1]
    );
  }

  #[test]
  fn a_chain_whose_closing_walk_runs_out_of_steps_stops_short_of_its_first_turn() {
    // Of 150,000 poses of sixteen bones drawn at random (parts 5 to 30
    // long, turns up to 1 rad a joint), the one that lost the most reach:
    // its closing walk's steps stay small for all `CHAIN_WALK` of them, so
    // it stops before the tip's distance first turns. Along the closing
    // family (every joint curled by the same added turn) the tip comes
    // nearest the first joint 0.0058 of the length away; the chain's
    // nearest shape is 0.0323 away, so 0.026 of the length of its reach
    // inward is lost
    let (bones, length) = links_each(
      &[
        11.849832, 24.498531, 9.307253, 24.300673, 14.914842, 13.321269, 25.84975, 22.07027,
        29.127676, 21.63734, 8.36898, 20.327698, 12.015852, 17.040762, 10.327988, 17.161476,
      ],
      2.1118746,
      &[
        -0.7299214,
        -0.15719052,
        -0.54538554,
        0.6643152,
        0.42509753,
        0.4988583,
        -0.6412369,
        -0.08497129,
        0.08515261,
        -0.8755955,
        -0.19222638,
        -0.676992,
        0.21935646,
        -0.56450045,
        -0.68637544,
      ],
    );
    // Each part's length and angle, in f64, and the tip's distance from
    // the first joint with every joint curled `curl` more
    let parts: Vec<(f64, f64)> = chain_joints(&bones, length)
      .windows(2)
      .map(|pair| {
        let part = pair[1] - pair[0];
        let (x, y) = (f64::from(part.x), f64::from(part.y));
        (x.hypot(y), y.atan2(x))
      })
      .collect();
    let total: f64 = parts.iter().map(|&(part, _)| part).sum();
    let reach = |curl: f64| {
      let (mut x, mut y) = (0.0, 0.0);
      for (joints_before, &(part, angle)) in parts.iter().enumerate() {
        let angle = angle + curl * joints_before as f64;
        x += part * angle.cos();
        y += part * angle.sin();
      }
      f64::hypot(x, y)
    };
    // The family closes the way the distance falls; walked in steps of
    // 1e-4 rad to the first turn of the distance
    let way = if reach(1e-6) < reach(-1e-6) {
      1.0
    } else {
      -1.0
    };
    let (mut curl, mut nearest) = (0.0, reach(0.0));
    while curl < std::f64::consts::PI {
      let next = reach((curl + 1e-4) * way);
      if next > nearest {
        break;
      }
      (curl, nearest) = (curl + 1e-4, next);
    }
    let first_turn = nearest / total;
    let stopped = f64::from(chain_nearest(&bones, length)) / total;
    assert!(
      (first_turn - 0.0058).abs() < 0.0005,
      "the first turn is {first_turn} of the length away"
    );
    let lost = stopped - first_turn;
    assert!(
      (lost - 0.026).abs() < 0.002,
      "the nearest shape is {stopped} of the length away: {lost} lost"
    );
  }

  #[test]
  fn a_chain_meets_every_target_from_its_nearest_shape_out() {
    // The fourteen chains again, and one whose joints are where the bones'
    // own matrices put them: its second and fourth bones sit off their
    // parents' axes, its first is scaled (2, 2) and its third (0.5, 0.5),
    // its second is turned by equal shears, and its last has a scale and a
    // shear of its own, which only its tip is carried by. Its parts are
    // 2 sqrt(241) = 31.05, 2 sqrt(661) = 51.42, sqrt(1664) = 40.79 and
    // 10 x 1.5 = 15 long
    let off_the_axes = vec![
      BonePose {
        scale_x: 2.0,
        scale_y: 2.0,
        ..bone(ORIGIN.x, ORIGIN.y, 0.3)
      },
      BonePose {
        shear_x: 0.25,
        shear_y: 0.25,
        ..bone(15.0, 4.0, 0.5)
      },
      BonePose {
        scale_x: 0.5,
        scale_y: 0.5,
        ..bone(25.0, -6.0, -0.4)
      },
      BonePose {
        scale_x: 1.5,
        scale_y: 0.7,
        shear_x: 0.2,
        shear_y: -0.3,
        ..bone(40.0, 8.0, 0.6)
      },
    ];
    let total = chain_length(&off_the_axes, 10.0);
    assert!((total - 138.26).abs() < 0.01, "{total}");
    let mut rigs = walked_rigs();
    rigs.push(("off the axes", off_the_axes, 10.0));

    // Eleven distances from a thousandth of the length outside each pose's
    // nearest shape to 99.9% of the length, each in eight directions an
    // eighth of a turn apart, 0.1 rad off the axes
    let mut worst = 0.0_f32;
    for (rig, bones, length) in rigs {
      let total = chain_length(&bones, length);
      let nearest = chain_nearest(&bones, length) + 0.001 * total;
      for tenth in 0..=10 {
        let distance = nearest + (0.999 * total - nearest) * tenth as f32 / 10.0;
        for eighth in 0..8 {
          let what = format!("{rig}, {distance} away, direction {eighth}");
          let target = ORIGIN + Vec2::from_angle(eighth as f32 * TAU / 8.0 + 0.1) * distance;
          let after = reached(&bones, length, target, 1.0);
          assert_on_target(&after, length, target, &what);
          assert_chain_only_turned(&bones, &after, &what);
          worst = worst.max(chain_tip(&after, length).distance(target) / total);
        }
      }
    }
    // The tip is far nearer than `CHAIN_NEAR ×` the length asks: the shape
    // is found to a rounding, and 0.0000005 of the length is the most here
    assert!(
      worst < 0.1 * CHAIN_NEAR,
      "the worst is {worst} of the length"
    );

    // Bones before the last in one line and the tip bent from them, with
    // the target on that line: parts of 20, turned 0 and 0.5 rad, toward
    // targets 57 and 50 along. The first two parts have no bend to scale;
    // an even turn at both joints brings the tip in
    let bent_tip = [
      bone(ORIGIN.x, ORIGIN.y, 0.0),
      bone(20.0, 0.0, 0.0),
      bone(20.0, 0.0, 0.5),
    ];
    for along in [57.0, 50.0] {
      let target = ORIGIN + Vec2::new(along, 0.0);
      let after = reached(&bent_tip, 20.0, target, 1.0);
      assert_on_target(&after, 20.0, target, &format!("a bent tip, {along} along"));
    }
  }

  #[test]
  fn a_chain_out_of_reach_lies_straight_toward_its_target() {
    // Parts of 30, 25, 25 and 20. Toward a target farther than their 100
    // along (0.6, 0.8) the joints are 30, 55, 80 and 100 along the line to
    // it; and parts of 10, toward (-0.8, 0.6), ten apart along that
    let (bones, length) = four();
    let toward = Vec2::new(0.6, 0.8);
    for distance in [100.0, 100.01, 101.0, 150.0, 1.0e3, 1.0e4] {
      let what = format!("four bones, {distance} away");
      let after = reached(&bones, length, ORIGIN + toward * distance, 1.0);
      let joints = chain_joints(&after, length);
      for (joint, along) in joints.iter().zip([0.0, 30.0, 55.0, 80.0, 100.0]) {
        assert_near(*joint, ORIGIN + toward * along, &what);
      }
      assert_chain_only_turned(&bones, &after, &what);
    }
    let (bones, length) = sixteen();
    let toward = Vec2::new(-0.8, 0.6);
    for distance in [160.02, 165.0, 400.0, 1.0e4] {
      let what = format!("sixteen bones, {distance} away");
      let after = reached(&bones, length, ORIGIN + toward * distance, 1.0);
      let joints = chain_joints(&after, length);
      for (index, joint) in joints.iter().enumerate() {
        assert_near(*joint, ORIGIN + toward * (index as f32 * 10.0), &what);
      }
      assert_chain_only_turned(&bones, &after, &what);
    }
    // A chain with no bend of its own is turned toward the target as it is,
    // and not bent
    let (bones, length) = links(&[25.0; 4], 0.3, 0.0);
    let toward = Vec2::new(0.0, -1.0);
    let after = reached(&bones, length, ORIGIN + toward * 130.0, 1.0);
    for (index, joint) in chain_joints(&after, length).iter().enumerate() {
      assert_near(
        *joint,
        ORIGIN + toward * (index as f32 * 25.0),
        "a straight chain",
      );
    }
  }

  #[test]
  fn a_chain_gives_the_same_result_every_run() {
    // One of each way through the solve: opened toward a target farther
    // than the pose's tip; closed onto a nearer one; given its nearest
    // shape; laid straight out of reach; and a straight pose bent into an
    // arc
    let straight = links(&[10.0; 16], 0.3, 0.0);
    let uneven = links(&[30.0, 20.0, 10.0], 0.2, 0.4);
    for (what, (bones, length), target) in [
      ("opened", four(), ORIGIN + Vec2::new(-57.0, 76.0)),
      ("closed", four(), ORIGIN + Vec2::new(40.0, 30.0)),
      ("its nearest shape", uneven, ORIGIN + Vec2::new(3.0, -4.0)),
      ("out of reach", four(), ORIGIN + Vec2::new(-90.0, 130.0)),
      ("an arc", straight, ORIGIN + Vec2::from_angle(1.0) * 100.0),
    ] {
      let mut first = bones.clone();
      let mut scratch = Vec::new();
      chain(&mut first, length, target, 1.0, None, &mut scratch);
      assert_ne!(chain_bits(&first), chain_bits(&bones), "{what}");
      // Again in the scratch the first run left
      let mut second = bones.clone();
      chain(&mut second, length, target, 1.0, None, &mut scratch);
      assert_eq!(chain_bits(&second), chain_bits(&first), "{what}");
      // And in one another chain's solve left
      let (mut other, other_length) = links(&[7.0, 9.0, 11.0, 13.0, 15.0], 2.0, -0.3);
      let mut used = Vec::new();
      chain(
        &mut other,
        other_length,
        Vec2::new(-20.0, 5.0),
        1.0,
        None,
        &mut used,
      );
      let mut third = bones.clone();
      chain(&mut third, length, target, 1.0, None, &mut used);
      assert_eq!(chain_bits(&third), chain_bits(&first), "{what}");
    }
  }

  #[test]
  fn a_chain_starts_from_the_animated_pose() {
    // Parts of 30, 25, 25 and 20, turned 0.5, -0.3 and 0.8 rad at the
    // joints: bent the positive way on the whole, its tip 93.6 from its
    // first joint
    let (bones, length) = links_each(&[30.0, 25.0, 25.0, 20.0], 0.3, &[0.5, -0.3, 0.8]);
    let pose = chain_turns(&bones, length);
    let tip = chain_tip(&bones, length).distance(ORIGIN);
    assert!((tip - 93.6).abs() < 0.1, "{tip}");
    let toward = Vec2::new(-0.6, 0.8);

    // A farther target opens the pose: every turn is the pose's times one
    // factor, less the farther the target, down to 0 at the chain's length
    let mut last = 1.0;
    for distance in [94.5, 96.0, 98.0, 99.5, 99.9] {
      let what = format!("{distance} away");
      let target = ORIGIN + toward * distance;
      let after = reached(&bones, length, target, 1.0);
      assert_on_target(&after, length, target, &what);
      let turns = chain_turns(&after, length);
      let factor = turns[0] / pose[0];
      assert!(
        factor > 0.0 && factor < last,
        "{what}: a factor of {factor}"
      );
      for (turn, pose) in turns.iter().zip(&pose) {
        assert!((turn - pose * factor).abs() < 1.0e-3, "{what}: {turns:?}");
      }
      last = factor;
    }
    // A target as far as the pose's tip leaves the shape as it is, and
    // turns the chain onto it
    let after = reached(&bones, length, ORIGIN + toward * tip, 1.0);
    for (turn, pose) in chain_turns(&after, length).iter().zip(&pose) {
      assert!((turn - pose).abs() < 1.0e-3, "as far as the tip");
    }
    assert_on_target(&after, length, ORIGIN + toward * tip, "as far as the tip");

    // A nearer target closes it: one turn is added to every joint, the
    // positive way as the pose bends, and more the nearer the target. The
    // joint bent the other way is carried with the rest
    let mut last = 0.0;
    for distance in [92.0, 80.0, 60.0, 40.0, 20.0] {
      let what = format!("{distance} away");
      let target = ORIGIN + toward * distance;
      let after = reached(&bones, length, target, 1.0);
      assert_on_target(&after, length, target, &what);
      let turns = chain_turns(&after, length);
      let added = turns[0] - pose[0];
      assert!(added > last, "{what}: {added} added");
      for (turn, pose) in turns.iter().zip(&pose) {
        assert!((turn - pose - added).abs() < 1.0e-3, "{what}: {turns:?}");
      }
      last = added;
    }

    // Four parts of 25 at -0.75, -0.25, 0.25 and 0.75 rad: bent the
    // positive way, out to one side of the x axis through the first joint
    // and back onto it, the middle joints 17.04, 23.23 and 17.04 to its
    // negative side and the tip on it 50 (cos 0.75 + cos 0.25) = 85.03
    // along. Its mirror in that axis is bent the other way, the same joints
    // to the positive side. Both are sent to one target on the axis
    let (clockwise, length) = links(&[25.0; 4], -0.75, 0.5);
    let (mirror, _) = links(&[25.0; 4], 0.75, -0.5);
    let side = |bones: &[BonePose]| {
      chain_joints(bones, length)[1..4]
        .iter()
        .map(|joint| joint.y - ORIGIN.y)
        .collect::<Vec<_>>()
    };
    assert!(
      side(&clockwise).iter().all(|off| *off < -17.0),
      "{:?}",
      side(&clockwise)
    );
    assert!(
      side(&mirror).iter().all(|off| *off > 17.0),
      "{:?}",
      side(&mirror)
    );
    assert_near(
      chain_tip(&clockwise, length),
      ORIGIN + Vec2::new(85.03, 0.0),
      "the tip",
    );
    for along in [10.0, 30.0, 50.0, 70.0, 95.0, 99.5] {
      let target = ORIGIN + Vec2::new(along, 0.0);
      let what = format!("{along} along");
      let (a, b) = (
        reached(&clockwise, length, target, 1.0),
        reached(&mirror, length, target, 1.0),
      );
      assert_on_target(&a, length, target, &what);
      assert_on_target(&b, length, target, &what);
      // Each keeps its bend to its own side, well off the axis
      let (ours, theirs) = (side(&a), side(&b));
      assert!(ours.iter().all(|off| *off < -1.0), "{what}: {ours:?}");
      assert!(theirs.iter().all(|off| *off > 1.0), "{what}: {theirs:?}");
      // And is the other's mirror image
      for (joint, twin) in chain_joints(&a, length)
        .iter()
        .zip(chain_joints(&b, length))
      {
        let mirrored = Vec2::new(twin.x, 2.0 * ORIGIN.y - twin.y);
        assert_near(*joint, mirrored, &what);
      }
    }
  }

  #[test]
  fn a_chain_at_half_mix_turns_each_bone_half_way() {
    let (bones, length) = four();
    // Behind the chain, so it turns a long way round to reach it
    let target = ORIGIN + Vec2::new(-30.0, -60.0);
    let full = reached(&bones, length, target, 1.0);
    assert_on_target(&full, length, target, "mix 1");

    // How far each part turned, and so how far each bone was turned: by
    // what its part turned more than the part before it. As plain numbers
    // one of those differences is more than half a turn, and the bone was
    // turned the short way round to the same place
    let turned: Vec<f32> = chain_joints(&bones, length)
      .windows(2)
      .zip(chain_joints(&full, length).windows(2))
      .map(|(before, after)| {
        let (before, after) = (before[1] - before[0], after[1] - after[0]);
        before.perp_dot(after).atan2(before.dot(after))
      })
      .collect();
    let mut plain = vec![turned[0]];
    plain.extend(turned.windows(2).map(|pair| pair[1] - pair[0]));
    assert!(
      plain.iter().any(|by| by.abs() > PI),
      "no bone's turn passes half a turn: {plain:?}"
    );
    for ((before, after), plain) in bones.iter().zip(&full).zip(&plain) {
      let by = after.rotation - before.rotation;
      assert!(by.abs() <= PI + 1.0e-4, "turned {by}");
      assert!(
        (by - shortest(*plain)).abs() < 1.0e-3,
        "turned {by}, not {plain} the short way"
      );
    }

    // Half way is half of each bone's turn
    let half = reached(&bones, length, target, 0.5);
    for ((before, half), full) in bones.iter().zip(&half).zip(&full) {
      let expected = before.rotation + (full.rotation - before.rotation) * 0.5;
      assert!(
        (half.rotation - expected).abs() < 1.0e-4,
        "at half mix a bone is at {}, not {expected}",
        half.rotation
      );
    }
    assert_chain_only_turned(&bones, &half, "mix 0.5");
    // And a quarter a quarter
    let quarter = reached(&bones, length, target, 0.25);
    for ((before, quarter), full) in bones.iter().zip(&quarter).zip(&full) {
      let expected = before.rotation + (full.rotation - before.rotation) * 0.25;
      assert!((quarter.rotation - expected).abs() < 1.0e-4, "mix 0.25");
    }

    // No mix, or less than none, leaves every bone exactly as it was
    for mix in [0.0, -0.0, -1.0] {
      let after = reached(&bones, length, target, mix);
      assert_eq!(chain_bits(&after), chain_bits(&bones), "mix {mix}");
    }
    // And a mix above 1 is taken as 1
    for mix in [1.5, 4.0, 1.0e30, f32::MAX] {
      let after = reached(&bones, length, target, mix);
      assert_eq!(chain_bits(&after), chain_bits(&full), "mix {mix}");
    }
  }

  #[test]
  fn a_chain_changes_only_rotations() {
    // A chain with every value a bone has set to something: bones off
    // their parents' axes, scaled evenly, mirrored and turned by equal
    // shears
    let bones = vec![
      BonePose {
        scale_x: 1.5,
        scale_y: 1.5,
        ..bone(ORIGIN.x, ORIGIN.y, 0.3)
      },
      BonePose {
        shear_x: -0.2,
        shear_y: -0.2,
        ..bone(20.0, 5.0, 0.6)
      },
      BonePose {
        scale_x: 0.8,
        scale_y: -0.8,
        ..bone(18.0, -4.0, -0.5)
      },
      BonePose {
        scale_x: 1.25,
        scale_y: 1.25,
        ..bone(22.0, 3.0, 0.4)
      },
      bone(15.0, -2.0, 0.7),
    ];
    let length = 12.0;
    let total = chain_length(&bones, length);
    let straight = links(&[25.0; 4], 0.3, 0.0);
    for (what, bones, length, target, mix) in [
      (
        "closed",
        &bones,
        length,
        ORIGIN + Vec2::new(30.0, -50.0),
        1.0,
      ),
      (
        "opened, nearly taut",
        &bones,
        length,
        ORIGIN + Vec2::new(0.6, 0.8) * (0.995 * total),
        1.0,
      ),
      (
        "out of reach",
        &bones,
        length,
        ORIGIN + Vec2::new(-300.0, 40.0),
        1.0,
      ),
      (
        "at a part mix",
        &bones,
        length,
        ORIGIN + Vec2::new(-40.0, 20.0),
        0.3,
      ),
      (
        "a straight chain",
        &straight.0,
        straight.1,
        ORIGIN + Vec2::from_angle(0.3) * 40.0,
        1.0,
      ),
    ] {
      let after = reached(bones, length, target, mix);
      assert_chain_only_turned(bones, &after, what);
      // And it did turn them
      let turned = bones
        .iter()
        .zip(&after)
        .filter(|(before, after)| before.rotation != after.rotation)
        .count();
      assert!(turned >= 2, "{what}: {turned} bones turned");
      if mix == 1.0 && what != "out of reach" {
        assert_on_target(&after, length, target, what);
      }
    }

    // A target on the pose's own tip asks for the pose: no bone turns by
    // more than a rounding
    let after = reached(&bones, length, chain_tip(&bones, length), 1.0);
    assert_chain_only_turned(&bones, &after, "on the tip");
    for (before, after) in bones.iter().zip(&after) {
      let by = after.rotation - before.rotation;
      assert!(by.abs() < 1.0e-5, "on the tip: turned {by}");
    }
  }

  #[test]
  fn a_chain_takes_no_more_room_on_a_second_run() {
    let (bones, length) = sixteen();
    // One target that closes the chain and one that opens it
    let targets = [
      ORIGIN + Vec2::new(60.0, 70.0),
      ORIGIN + Vec2::new(-90.0, 130.0),
    ];
    let mut scratch = Vec::new();
    chain(
      &mut bones.clone(),
      length,
      targets[0],
      1.0,
      None,
      &mut scratch,
    );
    let room = (scratch.as_ptr(), scratch.capacity());
    // Its room is what `chain_room` says a solve of as many bones fills:
    // three entries a bone and one more
    assert_eq!(chain_room(bones.len()), 49);
    assert_eq!(scratch.len(), chain_room(bones.len()));
    assert!(scratch.capacity() >= chain_room(bones.len()));
    for target in targets {
      let mut again = bones.clone();
      chain(&mut again, length, target, 1.0, None, &mut scratch);
      assert_ne!(chain_bits(&again), chain_bits(&bones));
      assert_eq!((scratch.as_ptr(), scratch.capacity()), room, "{target}");
    }
    // A shorter chain is solved in the same room, which is not given back
    let (mut short, short_length) = four();
    chain(
      &mut short,
      short_length,
      targets[0],
      1.0,
      None,
      &mut scratch,
    );
    assert_eq!((scratch.as_ptr(), scratch.capacity()), room, "four bones");
    chain(
      &mut bones.clone(),
      length,
      targets[1],
      1.0,
      None,
      &mut scratch,
    );
    assert_eq!(
      (scratch.as_ptr(), scratch.capacity()),
      room,
      "sixteen again"
    );
    // Nor is a solve that changes nothing a reason to take or give any
    chain(
      &mut bones.clone(),
      length,
      Vec2::NAN,
      1.0,
      None,
      &mut scratch,
    );
    chain(
      &mut bones.clone(),
      length,
      targets[0],
      0.0,
      None,
      &mut scratch,
    );
    assert_eq!(
      (scratch.as_ptr(), scratch.capacity()),
      room,
      "nothing to do"
    );
    // And a scratch made with that room, as a skeleton makes one for its
    // longest chain, does not grow on its first solve
    let mut sized = Vec::with_capacity(chain_room(bones.len()));
    let room = (sized.as_ptr(), sized.capacity());
    chain(
      &mut bones.clone(),
      length,
      targets[0],
      1.0,
      None,
      &mut sized,
    );
    assert_eq!((sized.as_ptr(), sized.capacity()), room, "sized before");
  }

  /// One input of a chain's solve, by number: each bone's seven values in
  /// turn, then the target's two, the length and the mix
  fn chain_input<'a>(
    bones: &'a mut [BonePose],
    rest: &'a mut (Vec2, f32, f32),
    index: usize,
  ) -> &'a mut f32 {
    let count = bones.len() * 7;
    if index < count {
      let bone = &mut bones[index / 7];
      match index % 7 {
        0 => &mut bone.x,
        1 => &mut bone.y,
        2 => &mut bone.rotation,
        3 => &mut bone.scale_x,
        4 => &mut bone.scale_y,
        5 => &mut bone.shear_x,
        _ => &mut bone.shear_y,
      }
    } else {
      match index - count {
        0 => &mut rest.0.x,
        1 => &mut rest.0.y,
        2 => &mut rest.1,
        _ => &mut rest.2,
      }
    }
  }

  #[test]
  fn a_chain_with_a_part_of_no_length_or_a_value_not_finite_makes_no_nan() {
    let (bones, length) = four();
    let target = ORIGIN + Vec2::new(40.0, 30.0);
    let unchanged = |what: &str, bones: &[BonePose], length: f32, target: Vec2, mix: f32| {
      let after = reached(bones, length, target, mix);
      assert_eq!(chain_bits(&after), chain_bits(bones), "{what}");
    };

    // A bone on its parent's origin makes a part of no length. The chain
    // reaches with its other parts, and that parent, whose part points
    // nowhere, keeps its rotation
    let mut on_parent = bones.clone();
    on_parent[2].x = 0.0;
    let after = reached(&on_parent, length, target, 1.0);
    assert_on_target(&after, length, target, "a bone on its parent's origin");
    assert_chain_only_turned(&on_parent, &after, "a bone on its parent's origin");
    assert_eq!(
      after[1].rotation, on_parent[1].rotation,
      "the parent turned"
    );
    assert_ne!(
      after[2].rotation, on_parent[2].rotation,
      "the bone on it did not"
    );
    // Within a millionth of a unit of it counts as on it: a chain of parts
    // 1 long at the origin, where so small a step is not lost in rounding
    let beside = vec![
      bone(0.0, 0.0, 0.3),
      bone(1.0, 0.0, 0.5),
      bone(5.0e-7, 0.0, 0.5),
      bone(1.0, 0.0, 0.5),
    ];
    let near_by = Vec2::new(1.5, 1.0);
    let part = chain_joints(&beside, 1.0);
    assert!(part[1].distance(part[2]) > 1.0e-7, "the step was lost");
    let after = reached(&beside, 1.0, near_by, 1.0);
    assert_on_target(&after, 1.0, near_by, "a bone beside its parent's origin");
    assert_eq!(after[1].rotation, beside[1].rotation, "the parent turned");
    // So does a first bone whose child is on it, and a last bone with no x
    // scale, whose tip is on its own origin
    let mut on_first = bones.clone();
    on_first[1].x = 0.0;
    let after = reached(&on_first, length, target, 1.0);
    assert_on_target(&after, length, target, "a bone on the first bone's origin");
    assert_eq!(
      after[0].rotation, on_first[0].rotation,
      "the first bone turned"
    );
    let mut no_tip = bones.clone();
    no_tip[3].scale_x = 0.0;
    let after = reached(&no_tip, length, target, 1.0);
    assert_on_target(&after, length, target, "a tip on the last bone's origin");
    assert_eq!(
      after[3].rotation, no_tip[3].rotation,
      "the last bone turned"
    );
    assert_on_target(
      &reached(&bones, 0.0, target, 1.0),
      0.0,
      target,
      "a tip with no length",
    );

    // Nothing to reach with, or for: a chain with no length at all, and
    // fewer than two bones
    let heap = vec![
      bone(ORIGIN.x, ORIGIN.y, 0.3),
      bone(0.0, 0.0, 0.5),
      bone(0.0, 0.0, 0.5),
    ];
    unchanged("every joint on one spot", &heap, 0.0, target, 1.0);
    unchanged("one bone", &bones[..1], 30.0, target, 1.0);
    unchanged("no bones", &[], 30.0, target, 1.0);

    // A value that is not finite, anywhere, changes nothing
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
      for index in 0..bones.len() * 7 + 4 {
        let (mut bones, mut rest) = (bones.clone(), (target, length, 1.0));
        *chain_input(&mut bones, &mut rest, index) = bad;
        unchanged(
          &format!("input {index} = {bad}"),
          &bones,
          rest.1,
          rest.0,
          rest.2,
        );
      }
    }

    // Bones the chain is not exact for give finite values, in their
    // rotations alone: uneven, sheared and flat ones
    for (what, scale_x, scale_y, shear_x) in [
      ("an uneven bone", 1.6, 0.7, 0.0),
      ("a sheared bone", 1.0, 1.0, 0.5),
      ("a bone flat along x", 0.0, 1.0, 0.0),
      ("a bone flat along y", 1.0, 0.0, 0.0),
      ("a bone scaled to nothing", 0.0, 0.0, 0.0),
    ] {
      for index in 0..bones.len() {
        let mut odd = bones.clone();
        odd[index] = BonePose {
          scale_x,
          scale_y,
          shear_x,
          ..odd[index]
        };
        for target in [
          target,
          ORIGIN + Vec2::new(-20.0, 90.0),
          ORIGIN + Vec2::new(0.0, 300.0),
        ] {
          let after = reached(&odd, length, target, 1.0);
          assert!(after.iter().all(finite), "{what} at {index}: {after:?}");
          assert_chain_only_turned(&odd, &after, what);
        }
      }
    }

    // Whatever goes in, alone or in pairs: nothing comes out that is not
    // finite, and an input that is not finite changes nothing. Three bones,
    // so every pair of their values is tried
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
    let (bones, length) = links(&[30.0, 20.0, 25.0], 0.3, 0.6);
    let inputs = bones.len() * 7 + 4;
    for one in 0..inputs {
      for one_value in extremes {
        for other in one..inputs {
          for other_value in extremes {
            let (mut bones, mut rest) = (bones.clone(), (target, length, 1.0));
            *chain_input(&mut bones, &mut rest, one) = one_value;
            *chain_input(&mut bones, &mut rest, other) = other_value;
            let given = bones.iter().all(finite)
              && rest.0.is_finite()
              && rest.1.is_finite()
              && rest.2.is_finite();
            let after = reached(&bones, rest.1, rest.0, rest.2);
            for (was, is) in chain_bits(&bones).into_iter().zip(chain_bits(&after)) {
              for (was, is) in was.into_iter().zip(is) {
                assert!(
                  was == is || (given && f32::from_bits(is).is_finite()),
                  "input {one} = {one_value:e}, input {other} = {other_value:e}: {bones:?} became {after:?}"
                );
              }
            }
          }
        }
      }
    }
  }

  #[test]
  fn a_chain_does_not_jump_as_its_target_crosses_its_full_reach() {
    // A target walked straight out through each chain's full length, twenty
    // steps short of it to twenty past. Inside it the pose opens as the
    // target nears the limit, and at the limit the chain is straight, as it
    // is beyond
    for (rig, (bones, length)) in [("four bones", four()), ("sixteen bones", sixteen())] {
      let total = chain_length(&bones, length);
      for toward in [
        Vec2::new(0.0, 1.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(-0.6, 0.8),
      ] {
        for share in [1.0e-2, 1.0e-3, 1.0e-4] {
          let what = format!("{rig}, toward {toward}, in steps of {share} of the length");
          let step = share * total;
          let poses: Vec<Vec<Vec2>> = (-20..=20)
            .map(|steps| {
              let target = ORIGIN + toward * (total + steps as f32 * step);
              let after = reached(&bones, length, target, 1.0);
              // The tip is on its target up to the limit, and at the limit
              // from there on
              if steps < 0 {
                assert_on_target(&after, length, target, &what);
              } else {
                assert_near(chain_tip(&after, length), ORIGIN + toward * total, &what);
              }
              chain_joints(&after, length)
            })
            .collect();
          let moved = |joint: usize| {
            poses
              .windows(2)
              .map(|pair| pair[0][joint].distance(pair[1][joint]))
              .fold(0.0, f32::max)
          };
          // The tip moves as far as its target does, and no farther
          let tip = moved(bones.len());
          assert!(tip < 1.05 * step, "{what}: the tip moved {tip}");
          // A joint goes sideways as the square root of how far inside the
          // limit the target is, as the joint of two bones does: 0.63 of
          // the square root of the step times the length is the most here,
          // so a smaller step moves it less
          let joint = (0..bones.len()).map(moved).fold(0.0, f32::max);
          let most = 0.8 * (step * total).sqrt();
          assert!(joint < most, "{what}: a joint moved {joint}, above {most}");
        }
      }
    }
  }

  #[test]
  fn a_straight_chain_bends_into_an_even_arc_the_positive_way() {
    // Chains laid along one line at 0.3 rad have no bend of their own to
    // open or to close. Each is bent into an even arc the positive way:
    // toward targets on its line, nearer than its length and behind its
    // first joint, and toward targets off it, every joint turns the
    // positive way and by as much as the others
    let line = Vec2::from_angle(0.3);
    let aside = Vec2::from_angle(1.3);
    for lengths in [vec![25.0; 4], vec![10.0; 16]] {
      let (bones, length) = links(&lengths, 0.3, 0.0);
      let total = chain_length(&bones, length);
      for (toward, share) in [
        (line, 0.05),
        (line, 0.25),
        (line, 0.5),
        (line, 0.75),
        (line, 0.9),
        (line, 0.97),
        (line, 0.995),
        (line, -0.5),
        (aside, 0.25),
        (aside, 0.75),
      ] {
        let what = format!(
          "{} parts, {share} of the length toward {toward}",
          lengths.len()
        );
        let target = ORIGIN + toward * (share * total);
        let after = reached(&bones, length, target, 1.0);
        assert_on_target(&after, length, target, &what);
        assert_chain_only_turned(&bones, &after, &what);
        let turns = chain_turns(&after, length);
        assert!(
          turns.iter().all(|turn| (0.02..2.0).contains(turn)),
          "{what}: turns of {turns:?}"
        );
        for turn in &turns {
          assert!(
            (turn - turns[0]).abs() < 1.0e-3,
            "{what}: turns of {turns:?}"
          );
        }
      }
    }

    // Four parts of 25 to a target 75 along: with each part turned `t` from
    // the one before, the tip is 25 sin(2t) / sin(t / 2) from the first
    // joint, which is 75 at a turn of 0.656 rad
    let (bones, length) = links(&[25.0; 4], 0.3, 0.0);
    let after = reached(&bones, length, ORIGIN + line * 75.0, 1.0);
    for turn in chain_turns(&after, length) {
      assert!((turn - 0.656).abs() < 0.002, "a turn of {turn}");
    }

    // A chain within `CHAIN_NEAR ×` its length of a line counts as on it,
    // whichever way it leans: here by a ten thousandth of a radian a joint,
    // the negative way
    let (leaning, length) = links(&[25.0; 4], 0.3, -1.0e-4);
    let target = ORIGIN + line * 50.0;
    let after = reached(&leaning, length, target, 1.0);
    assert_on_target(&after, length, target, "leaning a little");
    let turns = chain_turns(&after, length);
    assert!(turns.iter().all(|turn| *turn > 0.02), "{turns:?}");
    // One bent by more than that has a bend of its own, and is closed its
    // own way: an even turn more at every joint, the negative way, and no
    // joint folded
    let (bent, length) = links(&[25.0; 4], 0.3, -0.01);
    let toward = (chain_tip(&bent, length) - ORIGIN).normalize();
    let target = ORIGIN + toward * 50.0;
    let after = reached(&bent, length, target, 1.0);
    assert_on_target(&after, length, target, "bent a little");
    let turns = chain_turns(&after, length);
    assert!(
      turns.iter().all(|turn| (-1.5..-0.5).contains(turn)),
      "{turns:?}"
    );
    for turn in &turns {
      assert!((turn - turns[0]).abs() < 1.0e-3, "{turns:?}");
    }

    // A pose is straight or not by its own joints, wherever its target is:
    // a target that leaves the chain's line by a little, either side of a
    // thousandth of the chain's length from it, moves the chain a little
    let (bones, length) = links(&[25.0; 4], 0.0, 0.0);
    let pose = |off: f32| {
      let target = ORIGIN + Vec2::new(60.0, off);
      let after = reached(&bones, length, target, 1.0);
      assert_on_target(&after, length, target, &format!("{off} off the line"));
      chain_joints(&after, length)
    };
    for (one, other) in [(0.09, 0.11), (-0.09, -0.11), (0.0, 0.09)] {
      for (joint, moved) in pose(one).iter().zip(pose(other)) {
        assert!(
          joint.distance(moved) < 0.2,
          "{one} and {other} off the line: {joint} and {moved}"
        );
      }
    }
  }

  /// `bones` with bone `index` mirrored by a `scale_y` of -1 and every bone
  /// after it written for a parent mirrored so: its `y` and its rotation
  /// the other way round. Its joints are where `bones`' are
  fn mirrored_from(bones: &[BonePose], index: usize) -> Vec<BonePose> {
    let mut twin = bones.to_vec();
    twin[index].scale_y = -twin[index].scale_y;
    for bone in &mut twin[index + 1..] {
      bone.y = -bone.y;
      bone.rotation = -bone.rotation;
    }
    twin
  }

  #[test]
  fn a_mirrored_bone_in_a_chain_passes_its_turn_on_reversed() {
    // A chain with bones off their parents' axes, and its twins: the same
    // joints, with a bone in it mirrored by its own scale. Under a mirrored
    // bone the bones after it turn the other way for the same turn of
    // their parts, so each twin's joints come to where the plain chain's do
    let plain = vec![
      bone(ORIGIN.x, ORIGIN.y, 0.3),
      bone(30.0, 4.0, 0.5),
      bone(25.0, -3.0, 0.4),
      bone(25.0, 5.0, -0.6),
      bone(20.0, 0.0, 0.7),
    ];
    let length = 15.0;
    let second = mirrored_from(&plain, 1);
    // Mirrored by its x scale instead, which is the same bone turned half
    // round
    let mut by_x = second.clone();
    by_x[1] = BonePose {
      scale_x: -1.0,
      scale_y: 1.0,
      rotation: second[1].rotation + PI,
      ..second[1]
    };
    let twins = [
      ("the second bone mirrored", second),
      ("mirrored by its x scale", by_x),
      ("the first bone mirrored", mirrored_from(&plain, 0)),
      // Two mirrors: the bones after both turn the plain way again
      (
        "two bones mirrored",
        mirrored_from(&mirrored_from(&plain, 1), 3),
      ),
    ];
    let total = chain_length(&plain, length);
    for (what, twin) in &twins {
      for (joint, same) in chain_joints(&plain, length)
        .iter()
        .zip(chain_joints(twin, length))
      {
        assert_near(same, *joint, what);
      }
    }
    for target in [
      // Closed onto; opened toward; out of reach
      ORIGIN + Vec2::new(40.0, 50.0),
      ORIGIN + Vec2::new(-0.6, 0.8) * (0.99 * total),
      ORIGIN + Vec2::new(0.0, -400.0),
    ] {
      let solved = reached(&plain, length, target, 1.0);
      let joints = chain_joints(&solved, length);
      let moved = joints
        .iter()
        .zip(chain_joints(&plain, length))
        .any(|(joint, stood)| joint.distance(stood) > 10.0);
      assert!(moved, "target {target}: nothing moved");
      for (what, twin) in &twins {
        let what = format!("{what}, target {target}");
        let after = reached(twin, length, target, 1.0);
        assert_chain_only_turned(twin, &after, &what);
        for (joint, same) in joints.iter().zip(chain_joints(&after, length)) {
          assert_near(same, *joint, &what);
        }
      }
    }
  }

  #[test]
  fn a_chain_curls_and_does_not_fold_onto_a_near_target() {
    // The limit on how near a chain reaches. Closing a pose adds an even
    // turn at every joint, so the chain curls as an arc, and stops where
    // curling further would take its tip away again. Parts of 30, 20 and 10
    // curl until their tip is 16.3 from their first joint, 0.27 of their
    // length; folded, they would reach it. A target nearer than that is
    // not reached: the chain takes that nearest shape, its tip on the line
    // to the target
    let (bones, length) = links(&[30.0, 20.0, 10.0], 0.2, 0.4);
    let nearest = chain_nearest(&bones, length);
    assert!((nearest - 16.3).abs() < 0.1, "the nearest is {nearest}");
    let shape = chain_turns(
      &reached(&bones, length, ORIGIN + Vec2::new(0.0, 1.0), 1.0),
      length,
    );
    for (toward, distance) in [
      (Vec2::new(0.6, 0.8), 1.0),
      (Vec2::new(-1.0, 0.0), 8.0),
      (Vec2::new(0.0, -1.0), 14.0),
      (Vec2::new(0.8, -0.6), 16.0),
    ] {
      let what = format!("{distance} away toward {toward}");
      let after = reached(&bones, length, ORIGIN + toward * distance, 1.0);
      assert!(after.iter().all(finite), "{what}: {after:?}");
      assert_chain_only_turned(&bones, &after, &what);
      assert_near(chain_tip(&after, length), ORIGIN + toward * nearest, &what);
      for (turn, same) in chain_turns(&after, length).iter().zip(&shape) {
        assert!((turn - same).abs() < 1.0e-3, "{what}: another shape");
      }
    }
    // From there out the tip is on its target
    let target = ORIGIN + Vec2::new(0.6, 0.8) * (nearest + 0.1);
    assert_on_target(
      &reached(&bones, length, target, 1.0),
      length,
      target,
      "just outside",
    );

    // Parts of 15, 10, 5 and 5 turned 0.5, -0.9 and 0.6. As an even turn is
    // added their tip comes in to 8.26 from the first joint, goes out a
    // little, and comes in again, to 5.1 where a joint has folded. The
    // chain stops at the first of those: the later shape would meet a
    // target 8 away, and taking it would jump the chain as its target came
    // in past 8.26
    let (bones, length) = links_each(&[15.0, 10.0, 5.0, 5.0], 0.0, &[0.5, -0.9, 0.6]);
    let nearest = chain_nearest(&bones, length);
    assert!((nearest - 8.26).abs() < 0.01, "the nearest is {nearest}");
    let toward = Vec2::new(0.6, 0.8);
    for distance in [8.2, 8.0, 6.0, 2.0] {
      let after = reached(&bones, length, ORIGIN + toward * distance, 1.0);
      assert_near(
        chain_tip(&after, length),
        ORIGIN + toward * nearest,
        &format!("{distance} away"),
      );
    }

    // Parts of 25, 25 and 10 turned -1.1 and -1.7 close the negative way.
    // Their tip is still coming in when the second joint has folded half a
    // turn, 1.44 rad on, and there they stop, the tip 15.2 from the first
    // joint: no joint is turned past flat, where the chain would pass
    // through itself
    let (bones, length) = links_each(&[25.0, 25.0, 10.0], 0.0, &[-1.1, -1.7]);
    let nearest = chain_nearest(&bones, length);
    assert!((nearest - 15.2).abs() < 0.01, "the nearest is {nearest}");
    let folded = chain_turns(&reached(&bones, length, ORIGIN + toward * 5.0, 1.0), length);
    assert!((folded[0] + 2.542).abs() < 1.0e-3, "{folded:?}");
    assert!((folded[1].abs() - PI).abs() < 1.0e-3, "{folded:?}");

    // How near a pose comes is its parts' and its bends'. Equal parts bent
    // one way curl onto their first joint; unequal ones stop short of it;
    // and an S of equal parts, its middle joint bent against the others,
    // closes onto it too, where scaling its turns would fold it
    // (0.001 of the length is the nearest this asks a chain to come)
    for (what, (bones, length), share) in [
      ("3 of 20", links(&[20.0; 3], 0.0, 0.4), 0.001),
      ("16 of 10", sixteen(), 0.001),
      (
        "an S of 4 of 25",
        links_each(&[25.0; 4], 0.0, &[0.4, -0.5, 0.4]),
        0.001,
      ),
      ("30, 25, 25 and 20", four(), 0.070),
      (
        "10, 30, 15, 25 and 20",
        links(&[10.0, 30.0, 15.0, 25.0, 20.0], 0.0, -0.6),
        0.006,
      ),
    ] {
      let comes = chain_nearest(&bones, length) / chain_length(&bones, length);
      assert!((comes - share).abs() < 0.001, "{what}: as near as {comes}");
    }
  }

  #[test]
  fn a_chain_changes_sides_when_its_pose_swings_through_straight() {
    // The limit that is the pose's, not the target's. A chain closes the
    // way its pose bends, so two poses a hair apart, bent 0.01 rad a joint
    // one way and 0.01 the other, close to opposite sides of the line to a
    // target at half their length: mirror images, their middle joints 0.35
    // of the length off that line either way, and so 0.70 apart. An
    // animation that swings a chain through straight while its target is
    // nearer than its tip flips it so. A chain that is to reach inward is
    // kept bent one way throughout its animation, as a limb is given a
    // bend direction
    for lengths in [vec![25.0; 4], vec![12.0; 8], vec![10.0; 16]] {
      let what = format!("{} parts", lengths.len());
      let (one, length) = links(&lengths, 0.0, 0.01);
      let (other, _) = links(&lengths, 0.0, -0.01);
      let total = chain_length(&one, length);
      let target = ORIGIN + Vec2::new(0.5 * total, 0.0);
      let (one, other) = (
        reached(&one, length, target, 1.0),
        reached(&other, length, target, 1.0),
      );
      assert_on_target(&one, length, target, &what);
      assert_on_target(&other, length, target, &what);
      let middle = 1..lengths.len();
      let (ours, theirs) = (chain_joints(&one, length), chain_joints(&other, length));
      assert!(
        ours[middle.clone()].iter().all(|joint| joint.y < ORIGIN.y),
        "{what}: {ours:?}"
      );
      assert!(
        theirs[middle].iter().all(|joint| joint.y > ORIGIN.y),
        "{what}: {theirs:?}"
      );
      let apart = ours
        .iter()
        .zip(&theirs)
        .map(|(joint, twin)| joint.distance(*twin))
        .fold(0.0, f32::max)
        / total;
      assert!((0.65..0.75).contains(&apart), "{what}: {apart} apart");
    }
    // The nearer the target is to the chain's length the less there is to
    // flip: 0.38 of the length at 90% of it, and 0.09 at 99.5%
    let (one, length) = links(&[12.0; 8], 0.0, 0.01);
    let (other, _) = links(&[12.0; 8], 0.0, -0.01);
    for (share, flips) in [(0.9, 0.38), (0.995, 0.09)] {
      let target = ORIGIN + Vec2::new(share * 96.0, 0.0);
      let apart = chain_joints(&reached(&one, length, target, 1.0), length)
        .iter()
        .zip(chain_joints(&reached(&other, length, target, 1.0), length))
        .map(|(joint, twin)| joint.distance(twin))
        .fold(0.0, f32::max)
        / 96.0;
      assert!((apart - flips).abs() < 0.02, "at {share}: {apart} apart");
    }
  }

  #[test]
  fn an_s_shaped_sway_changes_sides_when_its_target_is_nearer_than_its_tip() {
    // The chain's main limit, as a tail's usual sway shows it: a wave that
    // travels down the chain, each joint turned `amplitude × sin(2π joint /
    // wave - phase)`. The pose is an S, bent both ways at once, and never
    // passes through straight; but which way it closes changes as the wave
    // travels, twice a cycle, and with its target held nearer than its tip
    // the chain flips to the other side each time
    for (what, lengths, wave, amplitude, share, size) in [
      ("4 of 25", vec![25.0; 4], 4.0, 0.4, 0.5, 0.72),
      ("4 of 25", vec![25.0; 4], 4.0, 0.4, 0.9, 0.35),
      ("16 of 10", vec![10.0; 16], 16.0, 0.2, 0.5, 0.66),
      ("16 of 10", vec![10.0; 16], 16.0, 0.2, 0.9, 0.23),
    ] {
      let what = format!("{what}, target at {share} of the length");
      let pose = |phase: f32| {
        let turns: Vec<f32> = (1..lengths.len())
          .map(|joint| amplitude * (TAU * joint as f32 / wave - phase).sin())
          .collect();
        links_each(&lengths, 0.0, &turns)
      };
      let (bones, length) = pose(0.0);
      let total = chain_length(&bones, length);
      let target = ORIGIN + Vec2::new(share * total, 0.0);
      // A cycle in 2,000 frames, each half a frame off the phases where the
      // pose is exactly symmetric
      let frames = 2000;
      let (mut least_bend, mut joints) = (f32::MAX, Vec::new());
      for frame in 0..frames {
        let (bones, length) = pose(TAU * (frame as f32 + 0.5) / frames as f32);
        let bend = chain_turns(&bones, length)
          .iter()
          .fold(0.0_f32, |most, turn| most.max(turn.abs()));
        least_bend = least_bend.min(bend);
        let after = reached(&bones, length, target, 1.0);
        assert_on_target(&after, length, target, &what);
        joints.push(chain_joints(&after, length));
      }
      let pops: Vec<f32> = (0..frames)
        .filter_map(|frame| {
          let (from, to) = (&joints[frame], &joints[(frame + 1) % frames]);
          let moved = from
            .iter()
            .zip(to)
            .map(|(from, to)| from.distance(*to))
            .fold(0.0, f32::max)
            / total;
          (moved > 0.01).then_some(moved)
        })
        .collect();
      assert!(least_bend > 0.19, "{what}: bent only {least_bend} at most");
      assert_eq!(pops.len(), 2, "{what}: {pops:?}");
      for pop in pops {
        assert!((pop - size).abs() < 0.03, "{what}: a joint jumps {pop}");
      }
    }
  }

  #[test]
  fn a_wound_up_pose_may_jump_as_its_target_moves_out() {
    // The limit for a pose wound up on itself. Sixteen parts of 10 turned
    // 0.45 rad at each joint are wound 6.75 rad, more than a full turn,
    // their tip 19.8 from the first joint. A pose opens toward straight to
    // reach farther, but this one's tip comes in as it opens, onto the
    // first joint, before it goes out. So the walk bends it further while
    // that takes the tip out, as far as 35.2, and only for a target farther
    // than that opens it: the chain goes from wound tighter than its pose
    // to unwound past it, and a joint jumps half the chain's length. No
    // pose whose parts point half a turn or less apart can do this
    let (bones, length) = links(&[10.0; 16], 0.0, 0.45);
    let total = chain_length(&bones, length);
    let tip = chain_tip(&bones, length).distance(ORIGIN);
    assert!((tip - 19.8).abs() < 0.1, "{tip}");
    let toward = Vec2::new(0.6, 0.8);
    let pose = |distance: f32| {
      let target = ORIGIN + toward * distance;
      let after = reached(&bones, length, target, 1.0);
      assert_on_target(&after, length, target, &format!("{distance} away"));
      (chain_turns(&after, length)[0], chain_joints(&after, length))
    };
    // Out to 35.2 the pose is bent further, a little at a time
    let mut last = pose(20.0);
    for distance in [24.0, 28.0, 32.0, 35.0] {
      let here = pose(distance);
      assert!(
        here.0 > last.0 && here.0 < 0.6,
        "{distance} away: {}",
        here.0
      );
      last = here;
    }
    // And past it the chain is opened: its turns less than the pose's
    let (inside, outside) = (pose(35.0), pose(35.4));
    assert!(outside.0 < 0.4, "a turn of {}", outside.0);
    let jump = inside
      .1
      .iter()
      .zip(&outside.1)
      .map(|(joint, moved)| joint.distance(*moved))
      .fold(0.0, f32::max)
      / total;
    assert!(
      (0.4..0.6).contains(&jump),
      "a joint jumps {jump} of the length"
    );
  }

  #[test]
  fn a_target_passing_over_a_chain_s_first_joint_turns_it_half_round() {
    // The limit every solve here has: a target on the first joint gives no
    // way to turn, and changes nothing
    let (bones, length) = four();
    for target in [ORIGIN, ORIGIN + Vec2::new(5.0e-7, -3.0e-7)] {
      let after = reached(&bones, length, target, 1.0);
      assert_eq!(chain_bits(&after), chain_bits(&bones), "{target}");
    }
    // Either side of it the chain has one shape, the nearest its pose
    // comes, pointed at the target: so pointed opposite ways, every joint
    // through the first joint from where it was
    for toward in [Vec2::new(1.0, 0.0), Vec2::new(-0.6, 0.8)] {
      let (one, other) = (
        chain_joints(
          &reached(&bones, length, ORIGIN + toward * 0.01, 1.0),
          length,
        ),
        chain_joints(
          &reached(&bones, length, ORIGIN - toward * 0.01, 1.0),
          length,
        ),
      );
      assert!(one[2].distance(ORIGIN) > 20.0, "{one:?}");
      for (joint, across) in one.iter().zip(&other) {
        assert_near(*across, ORIGIN * 2.0 - *joint, "across the first joint");
      }
    }
    // A target that misses the first joint by a thousandth of the chain's
    // length turns the chain round it quickly, and without a jump
    let stroke = Stroke::Line(Vec2::new(-0.3, 0.001), Vec2::new(0.3, 0.001));
    let (_, jump, _) = walked(&bones, length, stroke, 0.001);
    assert!(jump < 0.01, "a joint jumps {jump} of the length");
  }
}
