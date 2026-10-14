//! Knockback: decay each `Knockback` and apply it the way its entity moves.

use crate::core::ecs::{
  Entity, EntityHashSet, Has, Knockback, Local, MessageWriter, Query, Res, Transform, Vec2,
  Velocity,
};
use crate::core::messages::{KnockbackFinished, KnockbackStarted};
use crate::core::physics::{BodyKind, CharacterController, RigidBody};
use crate::core::time::Time;

type Pushed = (
  Entity,
  &'static mut Knockback,
  Option<&'static mut Velocity>,
  Option<&'static RigidBody>,
  Has<CharacterController>,
  Option<&'static mut Transform>,
);

/// `FixedSet::Movement`: decay every active `Knockback` by one fixed step
/// and apply it. A character is swept by it during the physics step (see
/// `Knockback`); a dynamic body gets it as one change in velocity when it
/// starts; a kinematic body without a controller and a fixed body ignore it
/// (logged once); anything else moves through its `Transform`.
pub fn update_knockback(
  time: Res<Time>,
  mut query: Query<Pushed>,
  mut started: MessageWriter<KnockbackStarted>,
  mut finished: MessageWriter<KnockbackFinished>,
  mut warned: Local<EntityHashSet>,
) {
  let dt = time.fixed.timestep();
  for (entity, mut knockback, velocity, body, character, transform) in &mut query {
    if !knockback.active {
      continue;
    }
    let just_started = knockback.just_started;
    let direction = knockback.direction();
    let force = knockback.force();
    knockback.update(dt);
    if just_started {
      started.write(KnockbackStarted { entity });
    }
    if knockback.just_finished {
      finished.write(KnockbackFinished { entity });
    }
    if character {
      continue;
    }
    match body.map(|body| body.kind) {
      Some(BodyKind::Dynamic) => {
        if just_started && let Some(mut velocity) = velocity {
          velocity.dx += direction.x * force;
          velocity.dy += direction.y * force;
        }
      }
      Some(BodyKind::Kinematic | BodyKind::Fixed) => {
        if warned.insert(entity) {
          crate::engine_warn!(
            module: "Gameplay",
            "{entity}: Knockback on a kinematic body without a CharacterController, or on a fixed body, does nothing"
          );
        }
      }
      None => {
        if let Some(mut transform) = transform {
          let (dx, dy) = knockback.current_velocity();
          transform.translation += Vec2::new(dx, dy) * dt;
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Disabled, Knockback, ScheduleLabel, Transform, Vec2, Velocity};
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
        RigidBody::dynamic(),
        Velocity::new(0.0, 300.0),
        Knockback::new(Vec2::X, 100.0),
      ))
      .id()
  }

  #[test]
  fn a_dynamic_body_takes_the_push_once_as_a_change_in_velocity() {
    let mut world = world(60);
    let entity = falling_body(&mut world);
    run(&mut world, update_knockback);
    assert_eq!(
      *world.get::<Velocity>(entity).unwrap(),
      Velocity::new(100.0, 300.0)
    );
    run(&mut world, update_knockback);
    assert_eq!(
      *world.get::<Velocity>(entity).unwrap(),
      Velocity::new(100.0, 300.0),
      "physics slows it from here"
    );
  }

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Probe;

  #[test]
  fn knockback_on_a_kinematic_or_fixed_body_does_nothing_and_warns_once() {
    let mut world = world(60);
    let mut schedule = crate::core::ecs::Schedule::new(Probe);
    schedule.add_systems(update_knockback);
    let kinematic = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::kinematic(),
        Knockback::new(Vec2::X, 100.0),
      ))
      .id();
    let fixed = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::fixed(),
        Knockback::new(Vec2::X, 100.0),
      ))
      .id();
    for _ in 0..3 {
      schedule.run(&mut world);
    }
    for entity in [kinematic, fixed] {
      assert_eq!(
        world.get::<Transform>(entity).unwrap().translation,
        Vec2::ZERO
      );
      assert_eq!(*world.get::<Velocity>(entity).unwrap(), Velocity::zero());
    }
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

#[cfg(test)]
mod with_physics {
  use crate::core::ecs::{Knockback, Transform, Vec2};
  use crate::core::gameplay::test_util::*;
  use crate::core::physics::Collider;

  #[test]
  fn knockback_carries_a_character_in_the_open() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = player(&mut world, 0.0, 0.0);
    world.entity_mut(e).insert(Knockback::new(Vec2::X, 400.0));
    run(&mut world, &mut s, 60);
    let x = at(&world, e).translation.x;
    assert!(x > 20.0, "{x}");
  }

  #[test]
  fn a_character_knocked_into_a_wall_stops_at_it() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    world.spawn((Transform::from_xy(60.0, 0.0), Collider::rect(20.0, 400.0)));
    let e = player(&mut world, 0.0, 0.0);
    world
      .entity_mut(e)
      .insert(Knockback::new(Vec2::X, 2000.0).with_decay_rate(1.0));
    run(&mut world, &mut s, 30);
    assert!(at(&world, e).translation.x + 10.0 <= 50.5);
    let kb = world.get::<Knockback>(e).unwrap();
    assert!(
      kb.is_finished() || kb.direction().x <= 1e-4,
      "clipped: {:?}",
      kb.direction()
    );
  }
}
