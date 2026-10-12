//! Knockback: decay each `Knockback` and move its entity.

use crate::core::ecs::knockback::retarget;
use crate::core::ecs::{Entity, Knockback, MessageWriter, Query, Res, Transform, Vec2, Velocity};
use crate::core::messages::{KnockbackFinished, KnockbackStarted};
use crate::core::physics::RigidBody;
use crate::core::time::Time;

type Pushed = (
  Entity,
  &'static mut Knockback,
  Option<&'static mut Velocity>,
  Option<&'static RigidBody>,
  Option<&'static mut Transform>,
);

/// Decay every active `Knockback` by one fixed step. A dynamic body's
/// `Velocity` gets the change in knockback velocity added, so gravity and
/// collisions keep acting and the push is taken back as it decays (never
/// more than the body still moves along it); anything else is moved through
/// its `Transform`. A stopped knockback takes back what it added.
pub fn update_knockback(
  time: Res<Time>,
  mut query: Query<Pushed>,
  mut started: MessageWriter<KnockbackStarted>,
  mut finished: MessageWriter<KnockbackFinished>,
) {
  let dt = time.fixed.timestep();
  for (entity, mut knockback, velocity, body, transform) in &mut query {
    if !knockback.active {
      if knockback.applied != Vec2::ZERO {
        if let Some(mut velocity) = velocity {
          retarget(&mut velocity, knockback.applied, Vec2::ZERO);
        }
        knockback.applied = Vec2::ZERO;
      }
      continue;
    }
    let just_started = knockback.just_started;
    knockback.update(dt);
    let (dx, dy) = knockback.current_velocity();
    if just_started {
      started.write(KnockbackStarted { entity });
    }
    if knockback.just_finished {
      finished.write(KnockbackFinished { entity });
    }
    let dynamic = body.is_some_and(|body| !body.is_kinematic);
    match velocity {
      Some(mut velocity) if dynamic => {
        let carried = retarget(&mut velocity, knockback.applied, Vec2::new(dx, dy));
        // Once it finishes, whatever is left was absorbed
        knockback.applied = if knockback.active {
          carried
        } else {
          Vec2::ZERO
        };
      }
      _ => {
        if let Some(mut transform) = transform {
          transform.translation.x += dx * dt;
          transform.translation.y += dy * dt;
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Disabled, Knockback, Transform, Vec2, Velocity};
  use crate::core::messages::{KnockbackFinished, KnockbackStarted};
  use crate::core::physics::RigidBody;
  use crate::core::systems::test_util::{messages, run, world};

  #[test]
  fn runs_on_an_empty_world() {
    let mut world = world(60);
    run(&mut world, update_knockback);
  }

  #[test]
  fn knockback_moves_an_entity_without_velocity() {
    let mut world = world(10);
    let entity = world
      .spawn((Transform::IDENTITY, Knockback::new(Vec2::X, 100.0)))
      .id();
    run(&mut world, update_knockback);
    assert!(world.get::<Transform>(entity).unwrap().translation.x > 0.0);
    assert_eq!(
      messages::<KnockbackStarted>(&world),
      [KnockbackStarted { entity }]
    );
  }

  #[test]
  fn knockback_moves_an_entity_with_velocity_but_no_body() {
    let mut world = world(60);
    let entity = world
      .spawn((
        Transform::IDENTITY,
        Velocity::zero(),
        Knockback::new(Vec2::X, 100.0),
      ))
      .id();
    run(&mut world, update_knockback);
    assert!(
      world.get::<Transform>(entity).unwrap().translation.x > 0.0,
      "nothing integrates Velocity without a body"
    );
  }

  fn falling_body(world: &mut crate::core::ecs::World) -> Entity {
    world
      .spawn((
        Transform::IDENTITY,
        RigidBody::new(),
        Velocity::new(0.0, 300.0),
        Knockback::new(Vec2::X, 100.0),
      ))
      .id()
  }

  #[test]
  fn knockback_adds_to_a_dynamic_body_s_velocity() {
    let mut world = world(60);
    let entity = falling_body(&mut world);
    run(&mut world, update_knockback);
    let velocity = world.get::<Velocity>(entity).unwrap();
    assert!(velocity.dx > 0.0);
    assert_eq!(velocity.dy, 300.0, "the fall goes on under the push");
    assert_eq!(
      world.get::<Transform>(entity).unwrap().translation,
      Vec2::ZERO,
      "physics moves a body"
    );
  }

  #[test]
  fn a_finished_knockback_takes_back_the_velocity_it_added() {
    let mut world = world(60);
    let entity = falling_body(&mut world);
    for _ in 0..60 {
      run(&mut world, update_knockback);
    }
    assert!(!world.get::<Knockback>(entity).unwrap().active);
    let velocity = world.get::<Velocity>(entity).unwrap();
    assert!(velocity.dx.abs() < 1e-3, "{}", velocity.dx);
    assert_eq!(velocity.dy, 300.0);
  }

  fn pushed_body(world: &mut crate::core::ecs::World) -> Entity {
    world
      .spawn((
        Transform::IDENTITY,
        RigidBody::new(),
        Velocity::zero(),
        Knockback::new(Vec2::X, 400.0),
      ))
      .id()
  }

  #[test]
  fn a_push_a_wall_absorbed_is_not_given_back() {
    let mut world = world(60);
    let entity = pushed_body(&mut world);
    run(&mut world, update_knockback);
    // The body hit a wall: physics took all of the push
    world.get_mut::<Velocity>(entity).unwrap().dx = 0.0;
    for _ in 0..60 {
      run(&mut world, update_knockback);
    }
    let dx = world.get::<Velocity>(entity).unwrap().dx;
    assert!(dx > -1e-3, "thrown back at {dx}");
  }

  #[test]
  fn a_stopped_knockback_takes_back_what_it_added() {
    let mut world = world(60);
    let entity = pushed_body(&mut world);
    run(&mut world, update_knockback);
    world.get_mut::<Knockback>(entity).unwrap().stop();
    run(&mut world, update_knockback);
    let dx = world.get::<Velocity>(entity).unwrap().dx;
    assert!(dx.abs() < 1e-3, "{dx}");
  }

  #[test]
  fn removing_a_knockback_takes_back_what_it_added() {
    let mut world = world(60);
    let entity = pushed_body(&mut world);
    run(&mut world, update_knockback);
    world.entity_mut(entity).remove::<Knockback>();
    let dx = world.get::<Velocity>(entity).unwrap().dx;
    assert!(dx.abs() < 1e-3, "{dx}");
  }

  #[test]
  fn knockback_finishes_and_says_so() {
    let mut world = world(1);
    let entity = world
      .spawn((Transform::IDENTITY, Knockback::new(Vec2::X, 100.0)))
      .id();
    run(&mut world, update_knockback);
    assert_eq!(
      messages::<KnockbackFinished>(&world),
      [KnockbackFinished { entity }]
    );
    assert!(!world.get::<Knockback>(entity).unwrap().active);
  }

  #[test]
  fn a_disabled_entity_is_untouched() {
    let mut world = world(10);
    let entity = world
      .spawn((
        Transform::IDENTITY,
        Knockback::new(Vec2::X, 100.0),
        Disabled,
      ))
      .id();
    run(&mut world, update_knockback);
    assert_eq!(
      world.get::<Transform>(entity).unwrap().translation,
      Vec2::ZERO
    );
    assert!(messages::<KnockbackStarted>(&world).is_empty());
  }

  #[test]
  fn apply_adds_or_restarts_knockback() {
    let mut world = world(60);
    let entity = world.spawn(Transform::IDENTITY).id();
    Knockback::apply(&mut world, entity, Vec2::Y, 50.0);
    let kb = world.get::<Knockback>(entity).unwrap();
    assert!(kb.active);
    assert_eq!(kb.direction(), Vec2::Y);
    assert_eq!(kb.decay_rate(), Knockback::DEFAULT_DECAY_RATE);
    // A second hit on an active knockback blends with it (accumulate is on by default)
    Knockback::apply_with_decay(&mut world, entity, Vec2::X, 80.0, 2.0);
    let kb = world.get::<Knockback>(entity).unwrap();
    assert!(kb.direction().x > 0.0 && kb.direction().y > 0.0);
    assert_eq!(kb.decay_rate(), 2.0);
    let gone = world.spawn_empty().id();
    world.despawn(gone);
    Knockback::apply(&mut world, gone, Vec2::X, 10.0);
  }
}
