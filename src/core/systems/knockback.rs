//! Knockback: decay each `Knockback` and move its entity.

use crate::core::ecs::{Entity, Knockback, MessageWriter, Query, Res, Transform, Velocity};
use crate::core::messages::{KnockbackFinished, KnockbackStarted};
use crate::core::time::Time;

/// Decay every active `Knockback` by one fixed step. An entity with a
/// `Velocity` gets the knockback velocity there (whatever integrates
/// `Velocity` moves it); one without is moved directly.
pub fn update_knockback(
  time: Res<Time>,
  mut query: Query<(
    Entity,
    &mut Knockback,
    Option<&mut Velocity>,
    Option<&mut Transform>,
  )>,
  mut started: MessageWriter<KnockbackStarted>,
  mut finished: MessageWriter<KnockbackFinished>,
) {
  let dt = time.fixed.timestep();
  for (entity, mut knockback, velocity, transform) in &mut query {
    if !knockback.active {
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
    if let Some(mut velocity) = velocity {
      velocity.dx = dx;
      velocity.dy = dy;
    } else if let Some(mut transform) = transform {
      transform.translation.x += dx * dt;
      transform.translation.y += dy * dt;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Disabled, Knockback, Transform, Vec2, Velocity};
  use crate::core::messages::{KnockbackFinished, KnockbackStarted};
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
  fn knockback_sets_velocity_and_leaves_translation_to_whatever_integrates_it() {
    let mut world = world(60);
    let entity = world
      .spawn((
        Transform::IDENTITY,
        Velocity::zero(),
        Knockback::new(Vec2::X, 100.0),
      ))
      .id();
    run(&mut world, update_knockback);
    assert!(world.get::<Velocity>(entity).unwrap().dx > 0.0);
    assert_eq!(
      world.get::<Transform>(entity).unwrap().translation,
      Vec2::ZERO,
      "moving it here as well would move it twice"
    );
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
