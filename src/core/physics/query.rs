//! The public side of `Physics`: forces and impulses and body reads (scene
//! queries and contact state join them later).

use super::world::{Physics, Push};
use crate::core::ecs::{Entity, Vec2};

impl Physics {
  /// An instant change of momentum (mass times px/s), applied before the
  /// next step. Ignored for bodies that are not dynamic.
  pub fn apply_impulse(&mut self, body: Entity, impulse: Vec2) {
    self.queue(body, Push::Impulse(impulse));
  }

  /// An impulse at a world point: off-centre it also spins the body.
  pub fn apply_impulse_at(&mut self, body: Entity, impulse: Vec2, point: Vec2) {
    self.queue(body, Push::ImpulseAt(impulse, point));
  }

  /// An instant change of angular momentum.
  pub fn apply_angular_impulse(&mut self, body: Entity, impulse: f32) {
    self.queue(body, Push::AngularImpulse(impulse));
  }

  /// A force (mass times px/s²) for the next step only: call it every step
  /// for a steady push.
  pub fn apply_force(&mut self, body: Entity, force: Vec2) {
    self.queue(body, Push::Force(force));
  }

  /// A force at a world point for the next step only.
  pub fn apply_force_at(&mut self, body: Entity, force: Vec2, point: Vec2) {
    self.queue(body, Push::ForceAt(force, point));
  }

  /// A torque for the next step only.
  pub fn apply_torque(&mut self, body: Entity, torque: f32) {
    self.queue(body, Push::Torque(torque));
  }

  /// The body's mass: the sum of its colliders' (a 100×100 px box at
  /// density 1 weighs 1). `None` for an entity without a simulated body.
  pub fn mass(&self, body: Entity) -> Option<f32> {
    let entry = self.body_entries.get(&body)?;
    Some(self.bodies.get(entry.handle)?.mass())
  }
}

#[cfg(test)]
mod tests {
  use crate::core::ecs::{Transform, Vec2, Velocity};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{AngularVelocity, Collider, Physics, RigidBody};

  fn crate_100(world: &mut crate::core::ecs::World, body: RigidBody) -> crate::core::ecs::Entity {
    world
      .spawn((
        Transform::IDENTITY,
        body,
        Collider::box_collider(100.0, 100.0),
      ))
      .id()
  }

  #[test]
  fn mass_comes_from_collider_area() {
    let (mut world, mut s) = world(60);
    let one = crate_100(&mut world, RigidBody::dynamic());
    let two = world
      .spawn((
        Transform::from_xy(500.0, 0.0),
        RigidBody::dynamic(),
        Collider::box_collider(200.0, 100.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert!((physics.mass(one).unwrap() - 1.0).abs() < 1e-3);
    assert!((physics.mass(two).unwrap() - 2.0).abs() < 1e-3);
  }

  #[test]
  fn an_impulse_changes_velocity_by_impulse_over_mass() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = crate_100(&mut world, RigidBody::dynamic().lock_rotation());
    // Queued before the body is registered: applied once it is
    world
      .resource_mut::<Physics>()
      .apply_impulse(e, Vec2::new(100.0, 0.0));
    run(&mut world, &mut s, 1);
    let dx = world.get::<Velocity>(e).unwrap().dx;
    assert!((dx - 100.0).abs() < 0.5, "{dx}");
  }

  #[test]
  fn a_force_acts_for_one_step() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = crate_100(&mut world, RigidBody::dynamic().lock_rotation());
    run(&mut world, &mut s, 1);
    world
      .resource_mut::<Physics>()
      .apply_force(e, Vec2::new(600.0, 0.0));
    run(&mut world, &mut s, 1);
    let after_one = world.get::<Velocity>(e).unwrap().dx;
    assert!((after_one - 10.0).abs() < 0.1, "600 / 1 / 60: {after_one}");
    run(&mut world, &mut s, 1);
    assert_eq!(
      world.get::<Velocity>(e).unwrap().dx,
      after_one,
      "gone after its step"
    );
  }

  #[test]
  fn torques_and_off_centre_impulses_spin_a_body() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let a = crate_100(&mut world, RigidBody::dynamic());
    let b = world
      .spawn((
        Transform::from_xy(500.0, 0.0),
        RigidBody::dynamic(),
        Collider::box_collider(100.0, 100.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    {
      let mut physics = world.resource_mut::<Physics>();
      physics.apply_torque(a, 5000.0);
      physics.apply_impulse_at(b, Vec2::new(0.0, 100.0), Vec2::new(550.0, 0.0));
      physics.apply_angular_impulse(a, 0.0);
    }
    run(&mut world, &mut s, 1);
    assert!(world.get::<AngularVelocity>(a).unwrap().0 > 0.0);
    assert!(world.get::<AngularVelocity>(b).unwrap().0 > 0.0);
    assert!(world.get::<Velocity>(b).unwrap().dy > 0.0);
  }

  #[test]
  fn pushes_on_non_dynamic_or_unknown_entities_are_ignored() {
    let (mut world, mut s) = world(60);
    let wall = crate_100(&mut world, RigidBody::fixed());
    let nobody = world.spawn(Transform::IDENTITY).id();
    {
      let mut physics = world.resource_mut::<Physics>();
      physics.apply_impulse(wall, Vec2::new(1e6, 0.0));
      physics.apply_force(nobody, Vec2::new(1e6, 0.0));
      physics.apply_impulse(wall, Vec2::new(f32::NAN, 0.0));
    }
    run(&mut world, &mut s, 10);
    assert_eq!(at(&world, wall).translation, Vec2::ZERO);
    assert_eq!(world.resource::<Physics>().mass(nobody), None);
  }
}
