//! Squash and stretch: deform an entity's scale and let it recover.

use crate::core::ecs::{Entity, MessageWriter, Query, Res, SquashStretch, Transform, Vec2};
use crate::core::messages::{SquashStretchFinished, SquashStretchStarted};
use crate::core::time::Time;

/// Advance every active `SquashStretch` by one fixed step and write the
/// deformed scale to the entity's `Transform`.
pub fn update_squash_stretch(
  time: Res<Time>,
  mut query: Query<(Entity, &mut SquashStretch, Option<&mut Transform>)>,
  mut started: MessageWriter<SquashStretchStarted>,
  mut finished: MessageWriter<SquashStretchFinished>,
) {
  let dt = time.fixed.timestep();
  for (entity, mut effect, transform) in &mut query {
    if !effect.active {
      continue;
    }
    if let Some(transform) = &transform {
      effect.capture_base_scale(transform.scale.x, transform.scale.y);
    }
    let just_started = effect.just_started;
    effect.update(dt);
    let (x, y) = effect.current_scale();
    if just_started {
      started.write(SquashStretchStarted { entity });
    }
    if effect.just_finished {
      finished.write(SquashStretchFinished { entity });
    }
    if let Some(mut transform) = transform {
      transform.scale = Vec2::new(x, y);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Disabled, SquashAxis, SquashStretch, Transform, Vec2, World};
  use crate::core::messages::{SquashStretchFinished, SquashStretchStarted};
  use crate::core::systems::test_util::{messages, run, world};

  fn settle(world: &mut World) {
    for _ in 0..100 {
      run(world, update_squash_stretch);
    }
  }

  #[test]
  fn runs_on_an_empty_world() {
    let mut world = world(60);
    run(&mut world, update_squash_stretch);
  }

  #[test]
  fn squash_deforms_the_scale_and_says_it_started() {
    let mut world = world(60);
    let entity = world
      .spawn((Transform::IDENTITY, SquashStretch::new(0.3)))
      .id();
    run(&mut world, update_squash_stretch);
    assert!(world.get::<Transform>(entity).unwrap().scale.y < 1.0);
    assert_eq!(
      messages::<SquashStretchStarted>(&world),
      [SquashStretchStarted { entity }]
    );
  }

  #[test]
  fn squash_returns_to_the_scale_it_started_from() {
    let mut world = world(60);
    let entity = world
      .spawn((
        Transform::IDENTITY.with_uniform_scale(2.0),
        SquashStretch::new(0.3),
      ))
      .id();
    settle(&mut world);
    assert_eq!(
      world.get::<Transform>(entity).unwrap().scale,
      Vec2::splat(2.0)
    );
    assert_eq!(
      messages::<SquashStretchFinished>(&world),
      [SquashStretchFinished { entity }]
    );
  }

  #[test]
  fn a_later_squash_starts_from_the_scale_the_entity_has_now() {
    let mut world = world(60);
    let entity = world
      .spawn((Transform::IDENTITY, SquashStretch::new(0.3)))
      .id();
    settle(&mut world);
    // A tween (or the game) grows the entity between squashes
    world.get_mut::<Transform>(entity).unwrap().scale = Vec2::splat(3.0);
    SquashStretch::apply(&mut world, entity, 0.3);
    settle(&mut world);
    assert_eq!(
      world.get::<Transform>(entity).unwrap().scale,
      Vec2::splat(3.0)
    );
  }

  #[test]
  fn a_disabled_entity_is_untouched() {
    let mut world = world(60);
    let entity = world
      .spawn((Transform::IDENTITY, SquashStretch::new(0.3), Disabled))
      .id();
    run(&mut world, update_squash_stretch);
    assert_eq!(world.get::<Transform>(entity).unwrap().scale, Vec2::ONE);
  }

  #[test]
  fn the_world_helpers_add_or_restart_the_effect() {
    let mut world = world(60);
    let entity = world.spawn(Transform::IDENTITY).id();
    SquashStretch::apply_x(&mut world, entity, 0.4);
    run(&mut world, update_squash_stretch);
    let scale = world.get::<Transform>(entity).unwrap().scale;
    assert!(scale.x < 1.0 && scale.y > 1.0, "squashed on X: {scale}");
    SquashStretch::apply_on_axis(&mut world, entity, 0.2, SquashAxis::Y);
    SquashStretch::apply_with_decay(&mut world, entity, 0.5, 12.0);
    let ss = world.get::<SquashStretch>(entity).unwrap();
    assert_eq!(ss.intensity(), 0.5);
    assert_eq!(ss.decay_rate(), 12.0);
  }
}
