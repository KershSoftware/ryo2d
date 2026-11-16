//! A spring constraint's motion: the model a skeleton's step advances for
//! each of its springs, and what the result does to the spring's bone. It
//! knows nothing of a pose's walk or a step: the constraint pass
//! (`constrain.rs`) calls it. The module notes give the rules.

use glam::{Mat2, Vec2};

use super::apply::shortest;
use super::component::invertible;
use super::constraint::{SpringSetup, SpringValues};
use super::pose::BonePose;

/// The most sub-steps a spring takes in one step. Past what they keep
/// stable, the strength saturates: it acts as the strongest pull this many
/// keep stable, never more, so a huge strength costs this many and no more
pub(crate) const MOST_SUB_STEPS: u32 = 64;

/// A sub-step's length times the square root of the strength is kept below
/// this, which keeps the simulation stable however stiff the spring
const STABLE: f32 = 0.5;

/// One point of a spring's bone (its origin or its tip): how far it is from
/// where animation puts it, and how fast that offset changes, both in world
/// units
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct SpringPoint {
  pub(crate) offset: Vec2,
  pub(crate) speed: Vec2,
}

/// How many equal sub-steps a step of `h` takes at `strength`, and the
/// strength they apply: as many as keep `h / count × √strength` below 0.5,
/// at most [`MOST_SUB_STEPS`]; past that the strength is the most those
/// keep stable (to a thousandth), so its effect saturates
pub(crate) fn sub_steps(h: f32, strength: f32) -> (u32, f32) {
  let reach = h * strength.sqrt();
  let most = MOST_SUB_STEPS as f32;
  if reach.is_finite() && reach / STABLE < most {
    // Truncation is the point: one more than the whole halves in the reach
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = (reach / STABLE).floor() as u32 + 1;
    return (count.clamp(1, MOST_SUB_STEPS), strength);
  }
  let saturated = most * STABLE * 0.999 / h;
  (MOST_SUB_STEPS, saturated * saturated)
}

/// Advances `point` by one step of `h` seconds, in which animation moved it
/// by `moved` in the world:
/// 1. the movement is clamped to `limit × h` in length;
/// 2. inertia leaves the point behind: `offset -= moved × inertia`;
/// 3. to 5., in equal sub-steps ([`sub_steps`]): the speed changes by the
///    pull back and the forces, `speed += (-strength × offset + (wind,
///    gravity)) × h`, damping takes it down, `speed *= e^(-damping × h)`,
///    and the offset moves by it, `offset += speed × h`.
///
/// A step that is not above 0, or not finite, changes nothing; movement
/// that is not finite counts as none; and a point that comes out not finite
/// is put back at rest
pub(crate) fn advance(
  point: &mut SpringPoint,
  moved: Vec2,
  h: f32,
  setup: &SpringSetup,
  values: &SpringValues,
) {
  if !(h > 0.0 && h.is_finite()) {
    return;
  }
  let moved = if moved.is_finite() {
    moved.clamp_length_max((setup.limit * h).max(0.0))
  } else {
    Vec2::ZERO
  };
  point.offset -= moved * setup.inertia;
  let (count, strength) = sub_steps(h, setup.strength);
  let step = h / count as f32;
  let force = Vec2::new(values.wind, values.gravity);
  let keep = (-setup.damping * step).exp();
  for _ in 0..count {
    point.speed += (force - point.offset * strength) * step;
    point.speed *= keep;
    point.offset += point.speed * step;
  }
  if !(point.offset.is_finite() && point.speed.is_finite()) {
    *point = SpringPoint::default();
  }
}

/// What a spring does to its bone, in the world: how far its place moves,
/// how far it turns (radians, the positive way clockwise on screen) and what
/// its scale along its length is multiplied by
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Sprung {
  pub(crate) moved: Vec2,
  pub(crate) turn: f32,
  pub(crate) stretch: f32,
}

/// What the offsets of a bone's `origin` and `tip` (in the world, as
/// animation put them) do to it, at `mix`. The sprung points are the
/// animated ones plus their offsets × `mix`. The place moves by the
/// origin's, times `x` along the world's x and `y` along its y. The sprung
/// bone runs from that place to the sprung tip: the bone turns by the
/// angle from the animated bone (origin to tip) to it, times `rotate`, and
/// its scale along its length is multiplied by `1 + (sprung length /
/// animated length - 1) × scale_x`. So a bone whose place stays (`x` and
/// `y` 0) pivots on its origin toward its lagging tip, and one whose place
/// follows its origin's lag whole turns only as far as its two points lag
/// differently. A bone of no length neither turns nor stretches
pub(crate) fn sprung(
  origin: Vec2,
  tip: Vec2,
  offsets: [SpringPoint; 2],
  setup: &SpringSetup,
  mix: f32,
) -> Sprung {
  let [from, to] = offsets.map(|point| point.offset * mix);
  let moved = Vec2::new(from.x * setup.x, from.y * setup.y);
  let animated = tip - origin;
  let bent = (tip + to) - (origin + moved);
  let (long, sprung_long) = (animated.length(), bent.length());
  let mut change = Sprung {
    moved,
    turn: 0.0,
    stretch: 1.0,
  };
  if long > LENGTHLESS && long.is_finite() {
    if sprung_long > LENGTHLESS && sprung_long.is_finite() {
      change.turn = animated.angle_to(bent) * setup.rotate;
    }
    change.stretch = 1.0 + (sprung_long / long - 1.0) * setup.scale_x;
  }
  if !(change.moved.is_finite() && change.turn.is_finite() && change.stretch.is_finite()) {
    return Sprung {
      moved: Vec2::ZERO,
      turn: 0.0,
      stretch: 1.0,
    };
  }
  change
}

/// Below this a bone, or its sprung line, has no length to turn by
const LENGTHLESS: f32 = 1e-6;

/// Writes `change` into a bone's own values, as a world-space override is
/// written: its move back through `parent` (its parent's transform in the
/// world, which its place is in under every inherit mode), and its turn
/// back through `frame` (what carries the bone's own direction to the
/// world), from `axis`, the bone's x axis in the world as animation left
/// it. Its stretch multiplies its x scale. False, with nothing written,
/// when `parent` is flat (a zero scale above the bone); a turn through a
/// flat `frame` is left out
pub(crate) fn onto_bone(
  own: &mut BonePose,
  parent: Mat2,
  frame: Mat2,
  axis: Vec2,
  change: Sprung,
) -> bool {
  if !invertible(parent) {
    return false;
  }
  let moved = parent.inverse() * change.moved;
  own.x += moved.x;
  own.y += moved.y;
  if change.turn != 0.0 && invertible(frame) && axis.length_squared() > 0.0 {
    // The bone's x axis turned in the world, brought back to its own
    // direction; under a negative x scale the axis points its back
    let along = if own.scale_x < 0.0 { -1.0 } else { 1.0 };
    let turned = Vec2::from_angle(change.turn).rotate(axis.normalize());
    let direction = frame.inverse() * (turned * along);
    let rotation = direction.to_angle() - own.shear_x;
    own.rotation += shortest(rotation - own.rotation);
  }
  own.scale_x *= change.stretch;
  true
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use super::*;

  fn close(actual: f32, expected: f32) {
    assert!(
      (actual - expected).abs() < 1e-4 * expected.abs().max(1.0),
      "expected {expected}, got {actual}"
    );
  }

  fn close2(actual: Vec2, expected: Vec2) {
    close(actual.x, expected.x);
    close(actual.y, expected.y);
  }

  /// A bone's own values at `x` along its parent, turned `rotation`
  fn bone(x: f32, rotation: f32) -> BonePose {
    BonePose {
      x,
      y: 0.0,
      rotation,
      scale_x: 1.0,
      scale_y: 1.0,
      shear_x: 0.0,
      shear_y: 0.0,
    }
  }

  fn still() -> SpringValues {
    SpringValues {
      mix: 1.0,
      wind: 0.0,
      gravity: 0.0,
    }
  }

  #[test]
  fn each_step_of_the_spring_model_matches_the_hand_worked_numbers() {
    // h = 0.01 at strength 100: 0.1 of a reach, one sub-step
    let setup = SpringSetup {
      rotate: 1.0,
      inertia: 0.5,
      strength: 100.0,
      damping: 0.0,
      limit: 500.0,
      ..SpringSetup::default()
    };
    // The limit: 10 moved in 0.01 is clamped to 500 × 0.01 = 5; inertia
    // leaves half of it behind, -2.5; the pull back gives a speed of
    // 100 × 2.5 × 0.01 = 2.5, and the offset moves 0.025 by it
    let mut point = SpringPoint::default();
    advance(&mut point, Vec2::new(10.0, 0.0), 0.01, &setup, &still());
    close2(point.offset, Vec2::new(-2.475, 0.0));
    close2(point.speed, Vec2::new(2.5, 0.0));
    // Within the limit, the movement is taken whole
    let mut point = SpringPoint::default();
    advance(&mut point, Vec2::new(0.0, 4.0), 0.01, &setup, &still());
    close2(point.offset, Vec2::new(0.0, -1.98));
    close2(point.speed, Vec2::new(0.0, 2.0));
    // Inertia 0 leaves nothing behind
    let mut point = SpringPoint::default();
    let lightless = SpringSetup {
      inertia: 0.0,
      ..setup
    };
    advance(&mut point, Vec2::new(4.0, 0.0), 0.01, &lightless, &still());
    assert_eq!(point, SpringPoint::default());
    // Damping 10 takes the speed down by e^(-0.1)
    let mut point = SpringPoint::default();
    let damped = SpringSetup {
      damping: 10.0,
      ..setup
    };
    advance(&mut point, Vec2::new(10.0, 0.0), 0.01, &damped, &still());
    close2(point.speed, Vec2::new(2.262_093_5, 0.0));
    close2(point.offset, Vec2::new(-2.477_379, 0.0));
    // Wind along the world's x and gravity along its y, down positive
    let mut point = SpringPoint::default();
    let blown = SpringValues {
      mix: 1.0,
      wind: 3.0,
      gravity: 4.0,
    };
    advance(&mut point, Vec2::ZERO, 0.01, &setup, &blown);
    close2(point.speed, Vec2::new(0.03, 0.04));
    close2(point.offset, Vec2::new(0.0003, 0.0004));
    // Sub-steps: at strength 10,000 a step of 0.01 is a reach of 1, so
    // three of them, each of a third of the step
    assert_eq!(sub_steps(0.01, 10_000.0), (3, 10_000.0));
    assert_eq!(sub_steps(0.01, 100.0), (1, 100.0));
    assert_eq!(sub_steps(1.0 / 60.0, 1e6).0, 34);
    assert_eq!(sub_steps(1.0 / 240.0, 1e6).0, 9);
    let stiff = SpringSetup {
      strength: 10_000.0,
      ..setup
    };
    let mut point = SpringPoint {
      offset: Vec2::new(1.0, 0.0),
      speed: Vec2::ZERO,
    };
    advance(&mut point, Vec2::ZERO, 0.01, &stiff, &still());
    close2(point.offset, Vec2::new(0.393_69, 0.0));
    close2(point.speed, Vec2::new(-85.596_71, 0.0));

    // Onto the bone, with mix: the origin 2 right and 4 down, the tip 10
    // down, at mix 0.5, put the sprung origin at (1, 2) and the sprung tip
    // 5 down from the animated tip at (10, 0). The place follows the
    // origin by x 1 and y 0.5, to (1, 1), and the sprung bone runs from
    // there to (10, 5), that is (9, 4)
    let amounts = SpringSetup {
      x: 1.0,
      y: 0.5,
      rotate: 0.5,
      scale_x: 0.5,
      ..SpringSetup::default()
    };
    let offsets = [
      SpringPoint {
        offset: Vec2::new(2.0, 4.0),
        speed: Vec2::ZERO,
      },
      SpringPoint {
        offset: Vec2::new(0.0, 10.0),
        speed: Vec2::ZERO,
      },
    ];
    let change = sprung(Vec2::ZERO, Vec2::new(10.0, 0.0), offsets, &amounts, 0.5);
    // The place by the origin's offset × mix × (x, y)
    close2(change.moved, Vec2::new(1.0, 1.0));
    // atan(4 / 9) × rotate 0.5
    close(change.turn, 0.209_112_16);
    // √97 / 10 = 0.9849, so 1 + (0.9849 - 1) × 0.5
    close(change.stretch, 0.992_442_9);
    // Each amount alone
    let only = |set: fn(&mut SpringSetup)| {
      let mut setup = SpringSetup::default();
      set(&mut setup);
      sprung(Vec2::ZERO, Vec2::new(10.0, 0.0), offsets, &setup, 0.5)
    };
    let x = only(|setup| setup.x = 1.0);
    assert_eq!(
      (x.moved, x.turn, x.stretch),
      (Vec2::new(1.0, 0.0), 0.0, 1.0)
    );
    let y = only(|setup| setup.y = 1.0);
    assert_eq!(
      (y.moved, y.turn, y.stretch),
      (Vec2::new(0.0, 2.0), 0.0, 1.0)
    );
    // Its place staying, the bone pivots toward the sprung tip, (10, 5)
    let turned = only(|setup| setup.rotate = 1.0);
    close(turned.turn, 0.463_647_6);
    assert_eq!((turned.moved, turned.stretch), (Vec2::ZERO, 1.0));
    let stretched = only(|setup| setup.scale_x = 1.0);
    close(stretched.stretch, 1.118_034);
    assert_eq!((stretched.moved, stretched.turn), (Vec2::ZERO, 0.0));
    // At mix 0 nothing moves
    let none = sprung(Vec2::ZERO, Vec2::new(10.0, 0.0), offsets, &amounts, 0.0);
    assert_eq!(
      (none.moved, none.turn, none.stretch),
      (Vec2::ZERO, 0.0, 1.0)
    );

    // Written into the bone's own values: under a parent turned a quarter
    // turn and doubled, a world move of (0, 2) is 1 along the parent's x,
    // and a world turn of 0.3 is 0.3 of its own
    let parent = Mat2::from_scale_angle(Vec2::splat(2.0), std::f32::consts::FRAC_PI_2);
    let mut own = bone(5.0, 0.2);
    let axis = parent * Vec2::from_angle(0.2);
    let change = Sprung {
      moved: Vec2::new(0.0, 2.0),
      turn: 0.3,
      stretch: 1.5,
    };
    assert!(onto_bone(&mut own, parent, parent, axis, change));
    close(own.x, 6.0);
    close(own.y, 0.0);
    close(own.rotation, 0.5);
    close(own.scale_x, 1.5);
    // Under a flat parent nothing is written
    let mut flat = bone(0.0, 0.0);
    assert!(!onto_bone(&mut flat, Mat2::ZERO, Mat2::ZERO, axis, change));
    assert_eq!(flat, bone(0.0, 0.0));
  }

  /// Steps a point kicked by `kick` in its first step for `seconds` at
  /// `hz`, the largest offset seen, and where it ends
  fn kicked(setup: &SpringSetup, hz: f32, seconds: f32, kick: Vec2) -> (f32, SpringPoint) {
    let mut point = SpringPoint::default();
    let h = 1.0 / hz;
    let mut most: f32 = 0.0;
    let mut moved = kick;
    // Truncation is fine: a count of steps
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    for _ in 0..(seconds * hz) as u32 {
      advance(&mut point, moved, h, setup, &still());
      moved = Vec2::ZERO;
      assert!(point.offset.is_finite() && point.speed.is_finite());
      most = most.max(point.offset.length());
    }
    (most, point)
  }

  #[test]
  fn a_stiff_spring_is_stable_at_any_fixed_rate() {
    // Strength 10^6 at 60 and 240 Hz: a kick of 10 leaves 5 behind, swings
    // out hardly past it (the sub-steps' own rounding, a few hundredths at
    // most), and is gone within 4 seconds
    let stiff = SpringSetup {
      rotate: 1.0,
      strength: 1e6,
      ..SpringSetup::default()
    };
    for hz in [60.0, 240.0] {
      let (most, end) = kicked(&stiff, hz, 4.0, Vec2::new(10.0, 0.0));
      assert!(most < 5.5, "{hz} Hz: swung out to {most}");
      assert!(end.offset.length() < 1e-3, "{hz} Hz: {end:?}");
    }
    // Strength 10^30 takes the most sub-steps and no more: bounded, in
    // bounded time, and never a NaN
    let absurd = SpringSetup {
      strength: 1e30,
      ..stiff
    };
    assert_eq!(sub_steps(1.0 / 60.0, 1e30).0, MOST_SUB_STEPS);
    assert_eq!(sub_steps(1.0 / 60.0, f32::MAX).0, MOST_SUB_STEPS);
    let started = Instant::now();
    for hz in [60.0, 240.0] {
      let (most, end) = kicked(&absurd, hz, 4.0, Vec2::new(10.0, 0.0));
      assert!(most < 5.5, "{hz} Hz: swung out to {most}");
      assert!(end.offset.length() < 1e-3, "{hz} Hz: {end:?}");
    }
    assert!(started.elapsed() < Duration::from_secs(1));
    // Saturated, it acts as the strongest pull the cap keeps stable: a
    // step's sub-steps each stay below 0.5 of a reach
    let (count, strength) = sub_steps(1.0 / 60.0, 1e30);
    assert!(1.0 / 60.0 / count as f32 * strength.sqrt() < 0.5);
  }

  #[test]
  fn the_same_steps_give_the_same_spring() {
    let setup = SpringSetup {
      rotate: 1.0,
      ..SpringSetup::default()
    };
    let gusty = SpringValues {
      mix: 1.0,
      wind: 40.0,
      gravity: 98.0,
    };
    let run = || {
      let mut point = SpringPoint::default();
      for step in 0..240 {
        let moved = Vec2::new((step as f32 * 0.1).sin() * 6.0, 1.5);
        advance(&mut point, moved, 1.0 / 60.0, &setup, &gusty);
      }
      point
    };
    let (a, b) = (run(), run());
    let bits =
      |point: SpringPoint| [point.offset, point.speed].map(|v| [v.x.to_bits(), v.y.to_bits()]);
    assert_eq!(bits(a), bits(b));
  }
}
