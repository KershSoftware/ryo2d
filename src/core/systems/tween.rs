//! Tweens: advance `Tween`, `TweenSequence` and `PropertyTween` components.

use glam::Vec2;

use crate::core::ecs::{
  Entity, Lerp, MessageWriter, PropertyTween, Query, Renderable, Res, Transform, Tween,
  TweenSequence, Without,
};
use crate::core::messages::{TweenCompleted, TweenStarted};
use crate::core::time::Time;
use crate::core::ui::Color;

/// Advance every playing `Tween` by one fixed step, writing its `Transform`
/// and `Renderable` targets. A tween on an entity without a `Transform`
/// still runs its alpha and tint targets.
pub fn update_tweens(
  time: Res<Time>,
  mut placed: Query<(Entity, &mut Tween, &mut Transform, Option<&mut Renderable>)>,
  mut unplaced: Query<(Entity, &mut Tween, Option<&mut Renderable>), Without<Transform>>,
  mut started: MessageWriter<TweenStarted>,
  mut completed: MessageWriter<TweenCompleted>,
) {
  let dt = time.fixed.timestep();
  for (entity, mut tween, mut transform, renderable) in &mut placed {
    if !tween.playing {
      continue;
    }
    // Touch the Transform only when the tween moves it, so change detection
    // (and propagation) does not see every colour tween
    let transform = tween.has_transform_targets().then_some(&mut *transform);
    drive_tween(
      &mut tween,
      dt,
      transform,
      renderable.map(|r| r.into_inner()),
    );
    report(entity, &tween, &mut started, &mut completed);
  }
  for (entity, mut tween, renderable) in &mut unplaced {
    if !tween.playing {
      continue;
    }
    drive_tween(&mut tween, dt, None, renderable.map(|r| r.into_inner()));
    report(entity, &tween, &mut started, &mut completed);
  }
}

/// One step of a tween: capture the colour start, advance, write colour.
fn drive_tween(
  tween: &mut Tween,
  dt: f32,
  transform: Option<&mut Transform>,
  renderable: Option<&mut Renderable>,
) {
  if tween.has_color_targets()
    && !tween.has_started()
    && let Some(renderable) = renderable.as_deref()
  {
    tween.capture_start_renderable(renderable.alpha, renderable.tint);
  }
  match transform {
    Some(transform) => tween.update(dt, transform),
    None => tween.update_without_transform(dt),
  }
  if let Some(renderable) = renderable {
    tween.apply_color(renderable);
  }
}

fn report(
  entity: Entity,
  tween: &Tween,
  started: &mut MessageWriter<TweenStarted>,
  completed: &mut MessageWriter<TweenCompleted>,
) {
  if tween.just_started {
    started.write(TweenStarted { entity });
  }
  if tween.just_finished {
    completed.write(TweenCompleted {
      entity,
      finished: !tween.playing,
    });
  }
}

/// Advance every `TweenSequence` by one fixed step, moving to the next tween
/// when one finishes.
pub fn update_tween_sequences(
  time: Res<Time>,
  mut placed: Query<(
    Entity,
    &mut TweenSequence,
    &mut Transform,
    Option<&mut Renderable>,
  )>,
  mut unplaced: Query<(Entity, &mut TweenSequence, Option<&mut Renderable>), Without<Transform>>,
  mut started: MessageWriter<TweenStarted>,
  mut completed: MessageWriter<TweenCompleted>,
) {
  let dt = time.fixed.timestep();
  for (entity, mut sequence, mut transform, renderable) in &mut placed {
    drive_sequence(
      entity,
      &mut sequence,
      dt,
      Some(&mut *transform),
      renderable.map(|r| r.into_inner()),
      &mut started,
      &mut completed,
    );
  }
  for (entity, mut sequence, renderable) in &mut unplaced {
    drive_sequence(
      entity,
      &mut sequence,
      dt,
      None,
      renderable.map(|r| r.into_inner()),
      &mut started,
      &mut completed,
    );
  }
}

fn drive_sequence(
  entity: Entity,
  sequence: &mut TweenSequence,
  dt: f32,
  transform: Option<&mut Transform>,
  renderable: Option<&mut Renderable>,
  started: &mut MessageWriter<TweenStarted>,
  completed: &mut MessageWriter<TweenCompleted>,
) {
  // `just_finished` lasts one step
  if sequence.just_finished {
    sequence.just_finished = false;
  }
  if !sequence.playing {
    return;
  }
  let speed = sequence.speed;
  let at_start = sequence.current_index() == 0;
  let Some(tween) = sequence.current_mut() else {
    return;
  };
  let transform = transform.filter(|_| tween.has_transform_targets());
  drive_tween(tween, dt * speed, transform, renderable);
  let sequence_started = at_start && tween.just_started;
  let tween_finished = tween.just_finished;
  if tween_finished {
    sequence.advance();
  }
  if sequence_started {
    started.write(TweenStarted { entity });
  }
  if sequence.just_finished {
    completed.write(TweenCompleted {
      entity,
      finished: true,
    });
  }
}

/// Advance every `PropertyTween<f32>`, `PropertyTween<Vec2>` and
/// `PropertyTween<Color>` by one fixed step.
pub fn update_property_tweens(
  time: Res<Time>,
  mut floats: Query<(Entity, &mut PropertyTween<f32>)>,
  mut vectors: Query<(Entity, &mut PropertyTween<Vec2>)>,
  mut colors: Query<(Entity, &mut PropertyTween<Color>)>,
  mut started: MessageWriter<TweenStarted>,
  mut completed: MessageWriter<TweenCompleted>,
) {
  let dt = time.fixed.timestep();
  for (entity, mut tween) in &mut floats {
    drive_property_tween(entity, &mut tween, dt, &mut started, &mut completed);
  }
  for (entity, mut tween) in &mut vectors {
    drive_property_tween(entity, &mut tween, dt, &mut started, &mut completed);
  }
  for (entity, mut tween) in &mut colors {
    drive_property_tween(entity, &mut tween, dt, &mut started, &mut completed);
  }
}

fn drive_property_tween<T: Lerp + Send + Sync + 'static>(
  entity: Entity,
  tween: &mut PropertyTween<T>,
  dt: f32,
  started: &mut MessageWriter<TweenStarted>,
  completed: &mut MessageWriter<TweenCompleted>,
) {
  tween.update(dt);
  if tween.just_started() {
    started.write(TweenStarted { entity });
  }
  if tween.just_finished() {
    completed.write(TweenCompleted {
      entity,
      finished: !tween.playing(),
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::SpriteHandle;
  use crate::core::ecs::{
    Disabled, IntoScheduleConfigs, PropertyTween, Renderable, Transform, Tween, TweenSequence,
    Vec2, World,
  };
  use crate::core::graphics::Sprite;
  use crate::core::messages::{TweenCompleted, TweenStarted};
  use crate::core::systems::test_util::{clear_messages, messages, run, set_hz, world};
  use crate::core::ui::Color;

  fn all(world: &mut World) {
    run(
      world,
      (
        update_tweens,
        update_tween_sequences,
        update_property_tweens,
      )
        .chain(),
    );
  }

  fn sprite() -> Renderable {
    Renderable::sprite(SpriteHandle::from_sprite(
      Sprite::solid_rect(4, 4, 255, 255, 255, 255),
      "white",
    ))
  }

  #[test]
  fn runs_on_an_empty_world() {
    let mut world = world(60);
    all(&mut world);
  }

  #[test]
  fn a_tween_says_when_it_starts() {
    let mut world = world(60);
    let entity = world
      .spawn((Transform::IDENTITY, Tween::to_position(100.0, 100.0, 1.0)))
      .id();
    all(&mut world);
    assert_eq!(messages::<TweenStarted>(&world), [TweenStarted { entity }]);
  }

  #[test]
  fn a_tween_says_when_it_completes() {
    let mut world = world(5);
    let entity = world
      .spawn((Transform::IDENTITY, Tween::to_position(100.0, 100.0, 0.1)))
      .id();
    all(&mut world);
    assert_eq!(
      messages::<TweenCompleted>(&world),
      [TweenCompleted {
        entity,
        finished: true
      }]
    );
    assert_eq!(
      world.get::<Transform>(entity).unwrap().translation,
      Vec2::new(100.0, 100.0)
    );
  }

  #[test]
  fn an_alpha_tween_runs_on_an_entity_without_a_transform() {
    let mut world = world(5);
    let entity = world.spawn((sprite(), Tween::fade_out(0.1))).id();
    all(&mut world);
    assert_eq!(world.get::<Renderable>(entity).unwrap().alpha, 0.0);
    assert_eq!(
      messages::<TweenCompleted>(&world),
      [TweenCompleted {
        entity,
        finished: true
      }]
    );
  }

  #[test]
  fn a_sequence_says_when_it_starts_and_completes() {
    let mut world = world(60);
    let entity = world
      .spawn((
        Transform::IDENTITY,
        TweenSequence::new().then(Tween::to_position(100.0, 0.0, 0.1)),
      ))
      .id();
    all(&mut world);
    assert_eq!(messages::<TweenStarted>(&world), [TweenStarted { entity }]);
    clear_messages(&mut world);
    set_hz(&mut world, 5);
    all(&mut world);
    assert_eq!(
      messages::<TweenCompleted>(&world),
      [TweenCompleted {
        entity,
        finished: true
      }]
    );
  }

  #[test]
  fn sequence_speed_scales_progress() {
    let mut world = world(4);
    let entity = world
      .spawn((
        Transform::IDENTITY,
        TweenSequence::new()
          .then(Tween::to_position(100.0, 0.0, 1.0))
          .with_speed(2.0),
      ))
      .id();
    // At 2x speed, 0.25 s is half the tween
    all(&mut world);
    let x = world.get::<Transform>(entity).unwrap().translation.x;
    assert!(x > 40.0, "at 2x speed, past 40% after 0.25 s, got {x}");
  }

  #[test]
  fn a_looping_sequence_never_completes_but_restarts() {
    let mut world = world(60);
    let entity = world
      .spawn((
        Transform::IDENTITY,
        TweenSequence::new()
          .then(Tween::to_position(100.0, 0.0, 0.1))
          .with_loop(),
      ))
      .id();
    all(&mut world);
    assert_eq!(messages::<TweenStarted>(&world), [TweenStarted { entity }]);
    clear_messages(&mut world);
    set_hz(&mut world, 5);
    all(&mut world);
    assert!(messages::<TweenCompleted>(&world).is_empty());
    clear_messages(&mut world);
    set_hz(&mut world, 60);
    all(&mut world);
    assert_eq!(
      messages::<TweenStarted>(&world),
      [TweenStarted { entity }],
      "the loop restarts"
    );
  }

  #[test]
  fn property_tweens_of_each_type_start_and_complete() {
    let mut world = world(5);
    let a = world.spawn(PropertyTween::new(0.0_f32, 100.0, 0.1)).id();
    let b = world
      .spawn(PropertyTween::new(Vec2::ZERO, Vec2::new(100.0, 50.0), 0.1))
      .id();
    let c = world
      .spawn(PropertyTween::new(Color::RED, Color::BLUE, 0.1))
      .id();
    all(&mut world);
    let started = messages::<TweenStarted>(&world);
    let completed = messages::<TweenCompleted>(&world);
    for entity in [a, b, c] {
      assert!(started.contains(&TweenStarted { entity }));
      assert!(completed.contains(&TweenCompleted {
        entity,
        finished: true
      }));
    }
    assert_eq!(world.get::<PropertyTween<f32>>(a).unwrap().value(), 100.0);
  }

  #[test]
  fn a_disabled_entity_is_untouched() {
    let mut world = world(5);
    let entity = world
      .spawn((
        Transform::IDENTITY,
        Tween::to_position(100.0, 100.0, 0.1),
        Disabled,
      ))
      .id();
    world.spawn((PropertyTween::new(0.0_f32, 1.0, 0.1), Disabled));
    all(&mut world);
    assert_eq!(
      world.get::<Transform>(entity).unwrap().translation,
      Vec2::ZERO
    );
    assert!(messages::<TweenStarted>(&world).is_empty());
  }
}
