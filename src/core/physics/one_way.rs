//! One-way colliders: the direction kept in Rapier's collider user data, the
//! contact hook dynamic bodies meet them through, and the rule the character
//! sweep applies.

use rapier2d::prelude::{
  ActiveHooks, Collider as RapierCollider, ContactModificationContext, PhysicsHooks, Pose, Shape,
  Vector,
};

use super::convert::to_rapier;
use crate::core::ecs::Vec2;

/// How far a contact normal may turn from a one-way collider's direction and
/// still push (45°).
pub(crate) const ONE_WAY_ANGLE: f32 = std::f32::consts::FRAC_PI_4;

/// Marks user data that holds a direction.
const FLAG: u128 = 1 << 64;

/// A one-way direction as Rapier user data: the flag bit, then the unit
/// direction's `y` and `x` bits. A missing or zero direction is 0.
pub(crate) fn encode(direction: Option<Vec2>) -> u128 {
  match direction.and_then(Vec2::try_normalize) {
    Some(d) => FLAG | (u128::from(d.y.to_bits()) << 32) | u128::from(d.x.to_bits()),
    None => 0,
  }
}

pub(crate) fn decode(data: u128) -> Option<Vec2> {
  (data & FLAG != 0).then(|| {
    Vec2::new(
      f32::from_bits(data as u32),
      f32::from_bits((data >> 32) as u32),
    )
  })
}

/// The hooks a collider needs for its one-way setting.
pub(crate) fn hooks(direction: Option<Vec2>) -> ActiveHooks {
  if encode(direction) != 0 {
    ActiveHooks::MODIFY_SOLVER_CONTACTS
  } else {
    ActiveHooks::empty()
  }
}

/// The step's contact hook: a contact with a one-way collider pushes only
/// while its normal points along the collider's direction. Rapier tracks
/// each crossing in the manifold, so a body part-way through is not snapped
/// back.
pub(crate) struct OneWayHooks;

impl PhysicsHooks for OneWayHooks {
  fn modify_solver_contacts(&self, context: &mut ContactModificationContext) {
    let (Some(c1), Some(c2)) = (
      context.colliders.get(context.collider1),
      context.colliders.get(context.collider2),
    ) else {
      return;
    };
    // `update_as_oneway_platform` compares the normal leaving collider 1, in
    // collider 1's own space
    let allowed = if let Some(local) = decode(c1.user_data) {
      to_rapier(local)
    } else if let Some(local) = decode(c2.user_data) {
      let world = c2.position().rotation.transform_vector(to_rapier(local));
      c1.position().rotation.inverse().transform_vector(-world)
    } else {
      return;
    };
    context.update_as_oneway_platform(allowed, ONE_WAY_ANGLE);
  }
}

/// How far a character may sit inside a one-way collider's top and still
/// count as above it, in px.
const ABOVE_TOLERANCE: f32 = 1.0;

/// The lowest and highest of `p · d` over `shape` at `pose`: its support
/// points along `-d` and `d`. A shape without a support map (a polyline) is
/// measured by its own box, turned with it, which can reach past the shape.
fn extent(shape: &dyn Shape, pose: &Pose, d: Vector) -> (f32, f32) {
  if let Some(support) = shape.as_support_map() {
    return (
      support.support_point(pose, -d).dot(d),
      support.support_point(pose, d).dot(d),
    );
  }
  let own = shape.compute_local_aabb();
  [
    own.mins,
    Vector::new(own.maxs.x, own.mins.y),
    own.maxs,
    Vector::new(own.mins.x, own.maxs.y),
  ]
  .iter()
  .map(|corner| pose.transform_point(*corner).dot(d))
  .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| {
    (lo.min(x), hi.max(x))
  })
}

/// A one-way collider's world direction.
fn direction(collider: &RapierCollider, local: Vec2) -> Vector {
  collider
    .position()
    .rotation
    .transform_vector(to_rapier(local))
}

/// Whether a character's `shape` at `pose` is entirely on a one-way
/// collider's solid side (above it), within `ABOVE_TOLERANCE`.
pub(crate) fn character_above(
  collider: &RapierCollider,
  local: Vec2,
  shape: &dyn Shape,
  pose: &Pose,
) -> bool {
  let d = direction(collider, local);
  let (_, top) = extent(collider.shape(), collider.position(), d);
  let (bottom, _) = extent(shape, pose, d);
  bottom >= top - ABOVE_TOLERANCE
}

/// Whether a character's `shape` at `pose` is entirely past a one-way
/// collider, below its lowest point.
pub(crate) fn character_below(
  collider: &RapierCollider,
  local: Vec2,
  shape: &dyn Shape,
  pose: &Pose,
) -> bool {
  let d = direction(collider, local);
  let (low, _) = extent(collider.shape(), collider.position(), d);
  let (_, high) = extent(shape, pose, d);
  high < low
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Transform, Velocity};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{Collider, Physics, RigidBody};

  #[test]
  fn a_direction_round_trips_through_user_data() {
    let d = Vec2::new(0.6, -0.8);
    assert_eq!(decode(encode(Some(d))), Some(d));
    assert_eq!(decode(encode(None)), None);
    assert_eq!(decode(encode(Some(Vec2::ZERO))), None, "zero is two-sided");
    assert_eq!(decode(0), None);
  }

  fn platform(world: &mut crate::core::ecs::World, collider: Collider) {
    world.spawn((Transform::IDENTITY, collider));
  }

  #[test]
  fn a_crate_tossed_up_through_a_one_way_platform_lands_on_top() {
    let (mut world, mut s) = world(60);
    platform(&mut world, Collider::rect(200.0, 10.0).one_way());
    let crate_ = world
      .spawn((
        Transform::from_xy(0.0, 60.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(20.0, 20.0),
        Velocity::new(0.0, -700.0),
      ))
      .id();
    run(&mut world, &mut s, 180);
    let y = at(&world, crate_).translation.y;
    assert!((y + 15.0).abs() < 1.0, "resting on top at -15: {y}");
  }

  #[test]
  fn a_body_moving_sideways_passes_through_a_one_way_platform() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    platform(&mut world, Collider::rect(40.0, 40.0).one_way());
    let crate_ = world
      .spawn((
        Transform::from_xy(-60.0, 0.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(10.0, 10.0),
        Velocity::new(300.0, 0.0),
      ))
      .id();
    run(&mut world, &mut s, 30);
    assert!(at(&world, crate_).translation.x > 60.0);
  }

  #[test]
  fn a_turned_one_way_collider_is_solid_along_its_own_up() {
    // Turned a quarter: its up, (0, -1), points along +X, so it stops what
    // comes from the right and lets through what comes from the left
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    world.spawn((
      Transform::IDENTITY.with_rotation(std::f32::consts::FRAC_PI_2),
      Collider::rect(80.0, 10.0).one_way(),
    ));
    let from_right = world
      .spawn((
        Transform::from_xy(60.0, 0.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(10.0, 10.0),
        Velocity::new(-300.0, 0.0),
      ))
      .id();
    let from_left = world
      .spawn((
        Transform::from_xy(-60.0, 30.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(10.0, 10.0),
        Velocity::new(300.0, 0.0),
      ))
      .id();
    run(&mut world, &mut s, 30);
    assert!(
      at(&world, from_right).translation.x > 5.0,
      "stopped on the solid side"
    );
    assert!(at(&world, from_left).translation.x > 60.0, "passed through");
  }

  #[test]
  fn making_a_collider_one_way_applies_in_place() {
    let (mut world, mut s) = world(60);
    let slab = world
      .spawn((Transform::IDENTITY, Collider::rect(200.0, 10.0)))
      .id();
    run(&mut world, &mut s, 1);
    world.get_mut::<Collider>(slab).unwrap().one_way = Some(Vec2::NEG_Y);
    let crate_ = world
      .spawn((
        Transform::from_xy(0.0, 60.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(20.0, 20.0),
        Velocity::new(0.0, -700.0),
      ))
      .id();
    run(&mut world, &mut s, 180);
    assert!(at(&world, crate_).translation.y < -5.0);
  }

  #[test]
  fn a_zero_one_way_direction_is_solid_and_warned_once() {
    let (mut world, mut s) = world(60);
    platform(
      &mut world,
      Collider::rect(200.0, 10.0).one_way_towards(Vec2::ZERO),
    );
    let crate_ = world
      .spawn((
        Transform::from_xy(0.0, 60.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(20.0, 20.0),
        Velocity::new(0.0, -700.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    assert!(at(&world, crate_).translation.y > 5.0, "blocked from below");
    assert_eq!(world.resource::<Physics>().warnings, 1);
  }
}
