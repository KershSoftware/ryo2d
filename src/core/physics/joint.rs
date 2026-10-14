//! Joints: constraints between two bodies. A `Joint` lives on its own
//! entity (or any entity) and names the two body entities it connects.

use rapier2d::prelude::{
  FixedJointBuilder, GenericJoint, JointAxis, Pose, PrismaticJointBuilder, RevoluteJointBuilder,
  RopeJointBuilder, SpringJointBuilder,
};

use super::convert::{from_rapier, pose, to_rapier};
use crate::core::ecs::{Component, Entity, Vec2};

/// Damping gain of a velocity motor: how hard it pulls toward its target
/// speed each step (Rapier's acceleration-based model).
const VELOCITY_MOTOR_GAIN: f32 = 10.0;

/// What a joint does.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum JointKind {
  /// A hinge or wheel: the bodies turn freely about one shared point.
  Revolute,
  /// A slider: b moves along `axis` (in a's frame) without turning relative to a.
  Prismatic { axis: Vec2 },
  /// A weld: the bodies keep the relative pose they had when it was made.
  Fixed,
  /// A maximum distance between the anchors; slack below it.
  Rope { max_length: f32 },
  /// A damped spring pulling the anchors toward `rest_length` apart.
  Spring {
    rest_length: f32,
    stiffness: f32,
    damping: f32,
  },
}

/// Where a joint attaches to its bodies.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum JointAnchor {
  /// Revolute and prismatic: at b's origin. Rope and spring: between the origins.
  #[default]
  Origins,
  /// A world point (revolute and prismatic), turned into each body's local
  /// anchor when the joint is made.
  World(Vec2),
  /// Anchors in each body's own space.
  Local { a: Vec2, b: Vec2 },
}

/// A motor on a revolute joint's angle or a prismatic joint's position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JointMotor {
  /// Reach `target` speed (rad/s or px/s) with at most `max_force`.
  Velocity { target: f32, max_force: f32 },
  /// Spring toward `target` (radians or px).
  Position {
    target: f32,
    stiffness: f32,
    damping: f32,
  },
}

/// A constraint between bodies `a` and `b`. Edits apply in place on the next
/// step (an unchanged joint never wakes its bodies). A joint whose bodies are
/// not both simulated waits (logged once); despawning a body leaves the joint
/// inert; disabling the joint's entity turns it off until it is re-enabled.
/// Builders that do not apply to a kind are ignored: limits and motors only
/// drive revolute and prismatic joints, `at` only places their pivot.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Joint {
  pub a: Entity,
  pub b: Entity,
  pub kind: JointKind,
  pub anchor: JointAnchor,
  /// Revolute: the angle range in radians; prismatic: the distance range in px.
  pub limits: Option<(f32, f32)>,
  pub motor: Option<JointMotor>,
  /// Whether the two bodies collide with each other (Box2D's collideConnected).
  pub collisions: bool,
  /// Remove the joint (sending `JointBroken`) when it pulls harder than this
  /// force (mass·px/s²).
  pub break_force: Option<f32>,
}

impl Joint {
  fn new(a: Entity, b: Entity, kind: JointKind) -> Self {
    Self {
      a,
      b,
      kind,
      anchor: JointAnchor::Origins,
      limits: None,
      motor: None,
      collisions: false,
      break_force: None,
    }
  }

  /// A hinge, pivoting at b's origin unless placed with `at`.
  pub fn revolute(a: Entity, b: Entity) -> Self {
    Self::new(a, b, JointKind::Revolute)
  }

  /// A slider: b moves along `axis` (in a's frame).
  pub fn prismatic(a: Entity, b: Entity, axis: Vec2) -> Self {
    Self::new(a, b, JointKind::Prismatic { axis })
  }

  /// A weld holding the bodies' relative pose when it is made.
  pub fn fixed(a: Entity, b: Entity) -> Self {
    Self::new(a, b, JointKind::Fixed)
  }

  /// At most `max_length` px between the anchors.
  pub fn rope(a: Entity, b: Entity, max_length: f32) -> Self {
    Self::new(a, b, JointKind::Rope { max_length })
  }

  /// A damped spring pulling the anchors toward `rest_length` px apart.
  pub fn spring(a: Entity, b: Entity, rest_length: f32, stiffness: f32, damping: f32) -> Self {
    Self::new(
      a,
      b,
      JointKind::Spring {
        rest_length,
        stiffness,
        damping,
      },
    )
  }

  /// The pivot at a world point.
  pub fn at(mut self, point: Vec2) -> Self {
    self.anchor = JointAnchor::World(point);
    self
  }

  /// Anchors in each body's own space.
  pub fn with_anchors(mut self, a: Vec2, b: Vec2) -> Self {
    self.anchor = JointAnchor::Local { a, b };
    self
  }

  /// The angle range (revolute, radians) or distance range (prismatic, px).
  pub fn with_limits(mut self, min: f32, max: f32) -> Self {
    self.limits = Some((min, max));
    self
  }

  /// A motor driving toward `target` speed with at most `max_force`.
  pub fn with_motor_velocity(mut self, target: f32, max_force: f32) -> Self {
    self.motor = Some(JointMotor::Velocity { target, max_force });
    self
  }

  /// A motor springing toward the `target` angle or position.
  pub fn with_motor_position(mut self, target: f32, stiffness: f32, damping: f32) -> Self {
    self.motor = Some(JointMotor::Position {
      target,
      stiffness,
      damping,
    });
    self
  }

  /// Let the two bodies collide with each other.
  pub fn with_collisions(mut self) -> Self {
    self.collisions = true;
    self
  }

  /// Break (sending `JointBroken`) when pulled harder than `force`.
  pub fn with_break_force(mut self, force: f32) -> Self {
    self.break_force = Some(force);
    self
  }
}

fn finite(values: &[f32]) -> bool {
  values.iter().all(|v| v.is_finite())
}

/// Rapier's joint for `joint` between bodies at world poses `pose_a` and
/// `pose_b`, or `None` for nonsense (non-finite values, a zero slider axis, a
/// negative rope length).
pub(crate) fn generic_joint(
  joint: &Joint,
  pose_a: (Vec2, f32),
  pose_b: (Vec2, f32),
) -> Option<GenericJoint> {
  let (a, b) = (pose(pose_a.0, pose_a.1), pose(pose_b.0, pose_b.1));
  let pivots = |p: Vec2| {
    (
      from_rapier(a.inverse_transform_point(to_rapier(p))),
      from_rapier(b.inverse_transform_point(to_rapier(p))),
    )
  };
  let hinged = matches!(
    joint.kind,
    JointKind::Revolute | JointKind::Prismatic { .. }
  );
  let (la, lb) = match joint.anchor {
    JointAnchor::Local { a, b } => (a, b),
    JointAnchor::World(p) if hinged => pivots(p),
    _ if hinged => pivots(pose_b.0),
    _ => (Vec2::ZERO, Vec2::ZERO),
  };
  if !(la.is_finite() && lb.is_finite()) {
    return None;
  }
  let (la, lb) = (to_rapier(la), to_rapier(lb));
  let mut generic: GenericJoint = match joint.kind {
    JointKind::Revolute => RevoluteJointBuilder::new()
      .local_anchor1(la)
      .local_anchor2(lb)
      .into(),
    JointKind::Prismatic { axis } => {
      let axis = axis.normalize_or_zero();
      if axis == Vec2::ZERO {
        return None;
      }
      // The same direction seen from b, however the bodies are turned
      let axis2 = Vec2::from_angle(pose_a.1 - pose_b.1).rotate(axis);
      PrismaticJointBuilder::new(to_rapier(axis))
        .local_axis2(to_rapier(axis2))
        .local_anchor1(la)
        .local_anchor2(lb)
        .into()
    }
    JointKind::Fixed => FixedJointBuilder::new()
      .local_frame1(a.inv_mul(&b))
      .local_frame2(Pose::IDENTITY)
      .into(),
    JointKind::Rope { max_length } => {
      if !(max_length.is_finite() && max_length >= 0.0) {
        return None;
      }
      RopeJointBuilder::new(max_length)
        .local_anchor1(la)
        .local_anchor2(lb)
        .into()
    }
    JointKind::Spring {
      rest_length,
      stiffness,
      damping,
    } => {
      if !finite(&[rest_length, stiffness, damping]) {
        return None;
      }
      SpringJointBuilder::new(rest_length.max(0.0), stiffness.max(0.0), damping.max(0.0))
        .local_anchor1(la)
        .local_anchor2(lb)
        .into()
    }
  };
  generic.set_contacts_enabled(joint.collisions);
  let axis = match joint.kind {
    JointKind::Revolute => Some(JointAxis::AngX),
    JointKind::Prismatic { .. } => Some(JointAxis::LinX),
    _ => None,
  };
  if let Some(axis) = axis {
    if let Some((min, max)) = joint.limits
      && finite(&[min, max])
    {
      generic.set_limits(axis, [min.min(max), min.max(max)]);
    }
    match joint.motor {
      Some(JointMotor::Velocity { target, max_force }) if finite(&[target, max_force]) => {
        generic.set_motor_velocity(axis, target, VELOCITY_MOTOR_GAIN);
        generic.set_motor_max_force(axis, max_force.max(0.0));
      }
      Some(JointMotor::Position {
        target,
        stiffness,
        damping,
      }) if finite(&[target, stiffness, damping]) => {
        generic.set_motor_position(axis, target, stiffness.max(0.0), damping.max(0.0));
      }
      _ => {}
    }
  }
  Some(generic)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::Velocity;
  use crate::core::ecs::{Disabled, Messages, Transform, World};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{
    AngularVelocity, Collider, JointBroken, Physics, PhysicsConfig, RigidBody,
  };

  fn anchor(world: &mut World, at: Vec2) -> Entity {
    world
      .spawn((Transform::from_translation(at), RigidBody::fixed()))
      .id()
  }

  fn bob(world: &mut World, at: Vec2) -> Entity {
    world
      .spawn((
        Transform::from_translation(at),
        RigidBody::dynamic(),
        Collider::circle(5.0),
      ))
      .id()
  }

  fn distance(world: &World, a: Entity, b: Entity) -> f32 {
    at(world, a).translation.distance(at(world, b).translation)
  }

  #[test]
  fn a_pendulum_keeps_its_length_and_swings() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = bob(&mut world, Vec2::new(100.0, 0.0));
    world.spawn(Joint::revolute(pivot, weight).at(Vec2::ZERO));
    // A 1 m pendulum released level is near the bottom after half a second
    // and nearly level on the other side after one
    run(&mut world, &mut s, 30);
    assert!((distance(&world, pivot, weight) - 100.0).abs() < 1.0);
    let p = at(&world, weight).translation;
    assert!(p.y > 50.0, "it swung down: {p}");
    run(&mut world, &mut s, 30);
    assert!((distance(&world, pivot, weight) - 100.0).abs() < 1.0);
    let p = at(&world, weight).translation;
    assert!(p.x < -90.0, "and up the other side: {p}");
  }

  #[test]
  fn a_slider_moves_along_its_axis_within_its_limits() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let base = anchor(&mut world, Vec2::ZERO);
    let slider = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Velocity::new(200.0, 200.0),
        Collider::rect(10.0, 10.0),
      ))
      .id();
    world.spawn(Joint::prismatic(base, slider, Vec2::X).with_limits(-50.0, 50.0));
    run(&mut world, &mut s, 60);
    let p = at(&world, slider).translation;
    assert!(p.y.abs() < 0.5 && (p.x - 50.0).abs() < 1.0, "{p}");
  }

  #[test]
  fn a_weld_keeps_the_relative_pose() {
    let (mut world, mut s) = world(60);
    let left = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::rect(20.0, 20.0),
      ))
      .id();
    let right = world
      .spawn((
        Transform::from_xy(30.0, 0.0),
        RigidBody::dynamic(),
        Collider::rect(20.0, 20.0),
      ))
      .id();
    world.spawn(Joint::fixed(left, right));
    run(&mut world, &mut s, 60);
    let offset = at(&world, right).translation - at(&world, left).translation;
    assert!((offset - Vec2::new(30.0, 0.0)).length() < 0.5, "{offset}");
    assert!(at(&world, left).translation.y > 100.0, "they fell together");
  }

  #[test]
  fn a_weld_made_as_a_body_is_moved_keeps_the_moved_pose() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let left = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::rect(20.0, 20.0),
      ))
      .id();
    let right = world
      .spawn((
        Transform::from_xy(100.0, 0.0),
        RigidBody::dynamic(),
        Collider::rect(20.0, 20.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    world.get_mut::<Transform>(right).unwrap().translation = Vec2::new(30.0, 0.0);
    world.spawn(Joint::fixed(left, right));
    run(&mut world, &mut s, 30);
    let offset = at(&world, right).translation - at(&world, left).translation;
    assert!((offset - Vec2::new(30.0, 0.0)).length() < 0.5, "{offset}");
  }

  #[test]
  fn a_rope_stops_at_its_length() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = bob(&mut world, Vec2::new(0.0, 50.0));
    world.spawn(Joint::rope(pivot, weight, 100.0));
    run(&mut world, &mut s, 60);
    let d = distance(&world, pivot, weight);
    assert!((d - 100.0).abs() < 1.0, "{d}");
  }

  #[test]
  fn a_spring_settles_at_its_rest_length() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = bob(&mut world, Vec2::new(200.0, 0.0));
    world.spawn(Joint::spring(pivot, weight, 100.0, 50.0, 5.0));
    run(&mut world, &mut s, 300);
    let d = distance(&world, pivot, weight);
    assert!((d - 100.0).abs() < 5.0, "{d}");
  }

  fn wheel(world: &mut World) -> (Entity, Entity) {
    no_gravity(world);
    let axle = anchor(world, Vec2::ZERO);
    let wheel = bob(world, Vec2::ZERO);
    let joint = world
      .spawn(Joint::revolute(axle, wheel).with_motor_velocity(5.0, 1e6))
      .id();
    (wheel, joint)
  }

  #[test]
  fn a_velocity_motor_turns_a_wheel_and_edits_apply_in_place() {
    let (mut world, mut s) = world(60);
    let (wheel, joint) = wheel(&mut world);
    run(&mut world, &mut s, 60);
    let spin = world.get::<AngularVelocity>(wheel).unwrap().0;
    assert!((spin - 5.0).abs() < 0.5, "{spin}");
    world.get_mut::<Joint>(joint).unwrap().motor = Some(JointMotor::Velocity {
      target: -5.0,
      max_force: 1e6,
    });
    run(&mut world, &mut s, 60);
    let spin = world.get::<AngularVelocity>(wheel).unwrap().0;
    assert!((spin + 5.0).abs() < 0.5, "{spin}");
    assert_eq!(world.resource::<Physics>().joint_count(), 1);
  }

  #[test]
  fn limits_hold_a_hinge() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let arm = world
      .spawn((
        Transform::from_xy(50.0, 0.0),
        RigidBody::dynamic(),
        Collider::rect(100.0, 10.0),
      ))
      .id();
    world.spawn(
      Joint::revolute(pivot, arm)
        .at(Vec2::ZERO)
        .with_limits(-0.5, 0.5),
    );
    run(&mut world, &mut s, 120);
    let r = at(&world, arm).rotation;
    assert!(r.abs() < 0.55, "{r}");
  }

  #[test]
  fn connected_bodies_collide_only_when_asked() {
    let gap = |collide: bool| {
      let (mut world, mut s) = world(60);
      no_gravity(&mut world);
      let a = world
        .spawn((
          Transform::IDENTITY,
          RigidBody::dynamic().lock_rotation(),
          Collider::rect(20.0, 20.0),
        ))
        .id();
      let b = world
        .spawn((
          Transform::from_xy(5.0, 0.0),
          RigidBody::dynamic().lock_rotation(),
          Collider::rect(20.0, 20.0),
        ))
        .id();
      let joint = Joint::rope(a, b, 1000.0);
      world.spawn(if collide {
        joint.with_collisions()
      } else {
        joint
      });
      run(&mut world, &mut s, 30);
      distance(&world, a, b)
    };
    assert!((gap(false) - 5.0).abs() < 0.5);
    assert!(gap(true) > 15.0);
  }

  #[test]
  fn a_joint_breaks_past_its_break_force() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = world
      .spawn((
        Transform::from_xy(0.0, 50.0),
        RigidBody::dynamic(),
        Collider::circle(5.0).with_mass(10.0),
      ))
      .id();
    let joint = world
      .spawn(Joint::rope(pivot, weight, 50.0).with_break_force(1000.0))
      .id();
    run(&mut world, &mut s, 60);
    let messages = world.resource::<Messages<JointBroken>>();
    let broken: Vec<_> = messages.get_cursor().read(messages).copied().collect();
    assert_eq!(
      broken,
      [JointBroken {
        joint,
        a: pivot,
        b: weight
      }]
    );
    assert!(
      world.get::<Joint>(joint).is_none(),
      "the component went with it"
    );
    assert!(
      at(&world, weight).translation.y > 200.0,
      "and the weight fell"
    );
  }

  /// Whether a joint made by `make` breaks within a second holding a 10 kg
  /// weight at rest below its pivot (a pull of 10 × 980 = 9,800).
  fn breaks(make: fn(Entity, Entity) -> Joint, force: f32, iterations: usize) -> bool {
    let (mut world, mut s) = world(60);
    *world.resource_mut::<PhysicsConfig>() =
      PhysicsConfig::default().with_solver_iterations(iterations);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = world
      .spawn((
        Transform::from_xy(0.0, 50.0),
        RigidBody::dynamic(),
        Collider::circle(5.0).with_mass(10.0),
      ))
      .id();
    world.spawn(make(pivot, weight).with_break_force(force));
    run(&mut world, &mut s, 60);
    !world.resource::<Messages<JointBroken>>().is_empty()
  }

  #[test]
  fn a_break_force_is_the_force_the_joint_holds() {
    let rope = |a, b| Joint::rope(a, b, 50.0);
    let hinge = |a, b| Joint::revolute(a, b).at(Vec2::ZERO);
    for make in [rope as fn(Entity, Entity) -> Joint, hinge] {
      for iterations in [1, 4, 8] {
        assert!(!breaks(make, 12_000.0, iterations), "{iterations}");
        assert!(breaks(make, 8_000.0, iterations), "{iterations}");
      }
    }
  }

  #[test]
  fn a_disabled_joint_lets_go_until_re_enabled() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = bob(&mut world, Vec2::new(0.0, 50.0));
    let joint = world.spawn(Joint::rope(pivot, weight, 50.0)).id();
    run(&mut world, &mut s, 10);
    world.entity_mut(joint).insert(Disabled);
    run(&mut world, &mut s, 20);
    assert!(distance(&world, pivot, weight) > 60.0, "it fell");
    world.entity_mut(joint).remove::<Disabled>();
    run(&mut world, &mut s, 30);
    let d = distance(&world, pivot, weight);
    assert!(d < 51.0, "caught again: {d}");
  }

  #[test]
  fn a_disabled_joint_waiting_for_its_bodies_stays_disabled() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let later = world
      .spawn((Transform::from_xy(0.0, 50.0), Collider::circle(5.0)))
      .id();
    let joint = world.spawn(Joint::rope(pivot, later, 50.0)).id();
    run(&mut world, &mut s, 2);
    world.entity_mut(joint).insert(Disabled);
    run(&mut world, &mut s, 1);
    world.entity_mut(later).insert(RigidBody::dynamic());
    run(&mut world, &mut s, 30);
    assert_eq!(world.resource::<Physics>().joint_count(), 1, "made");
    assert!(distance(&world, pivot, later) > 60.0, "but off");
    world.entity_mut(joint).remove::<Disabled>();
    run(&mut world, &mut s, 30);
    let d = distance(&world, pivot, later);
    assert!(d < 51.0, "and on again: {d}");
  }

  #[test]
  fn a_joint_spawned_disabled_is_made_when_enabled() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = bob(&mut world, Vec2::new(0.0, 50.0));
    let joint = world
      .spawn((Joint::rope(pivot, weight, 50.0), Disabled))
      .id();
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().joint_count(), 0);
    world.get_mut::<Joint>(joint).unwrap().kind = JointKind::Rope { max_length: 80.0 };
    run(&mut world, &mut s, 2);
    world.entity_mut(joint).remove::<Disabled>();
    run(&mut world, &mut s, 60);
    assert_eq!(world.resource::<Physics>().joint_count(), 1);
    let d = distance(&world, pivot, weight);
    assert!((d - 80.0).abs() < 1.0, "with the edit made meanwhile: {d}");
  }

  #[test]
  fn a_joint_waits_for_its_bodies() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let later = world
      .spawn((Transform::from_xy(0.0, 50.0), Collider::circle(5.0)))
      .id();
    world.spawn(Joint::rope(pivot, later, 50.0));
    run(&mut world, &mut s, 2);
    assert_eq!(
      world.resource::<Physics>().joint_count(),
      0,
      "inert, no panic"
    );
    world.entity_mut(later).insert(RigidBody::dynamic());
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().joint_count(), 1);
  }

  #[test]
  fn despawning_a_body_leaves_its_joint_inert() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = bob(&mut world, Vec2::new(0.0, 50.0));
    let joint = world.spawn(Joint::rope(pivot, weight, 50.0)).id();
    run(&mut world, &mut s, 2);
    world.despawn(weight);
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().joint_count(), 0);
    world.despawn(joint);
    run(&mut world, &mut s, 1);
  }

  #[test]
  fn builders() {
    let (a, b) = (
      Entity::from_raw_u32(1).unwrap(),
      Entity::from_raw_u32(2).unwrap(),
    );
    let j = Joint::revolute(a, b)
      .at(Vec2::new(1.0, 2.0))
      .with_limits(-1.0, 1.0)
      .with_motor_position(0.5, 10.0, 1.0)
      .with_collisions()
      .with_break_force(100.0);
    assert_eq!(j.anchor, JointAnchor::World(Vec2::new(1.0, 2.0)));
    assert_eq!(j.limits, Some((-1.0, 1.0)));
    assert_eq!(
      j.motor,
      Some(JointMotor::Position {
        target: 0.5,
        stiffness: 10.0,
        damping: 1.0
      })
    );
    assert!(j.collisions);
    assert_eq!(j.break_force, Some(100.0));
    assert_eq!(
      Joint::spring(a, b, 1.0, 2.0, 3.0).kind,
      JointKind::Spring {
        rest_length: 1.0,
        stiffness: 2.0,
        damping: 3.0
      }
    );
    assert_eq!(
      Joint::fixed(a, b).with_anchors(Vec2::X, Vec2::Y).anchor,
      JointAnchor::Local {
        a: Vec2::X,
        b: Vec2::Y
      }
    );
  }
}
