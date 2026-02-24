//! Tween system for updating Tween and TweenSequence components.
//!
//! This system handles:
//! - Updating tween progress and interpolation
//! - Applying tweened values to Transform (position, scale, rotation)
//! - Applying tweened values to Renderable (alpha, tint)
//! - Managing tween sequences (advancing to next tween when current finishes)

use crate::core::ecs::{Inactive, Renderable, Transform, Tween, TweenSequence, World};
use crate::core::events::{Events, TweenCompleted, TweenStarted};

/// System for updating tweens and tween sequences.
///
/// Handles both single `Tween` components and `TweenSequence` components,
/// applying interpolated values to Transform and Renderable.
pub struct TweenSystem;

impl TweenSystem {
  /// Update all tween components in the world.
  ///
  /// # Arguments
  /// * `world` - The ECS world containing entities
  /// * `dt` - Delta time (should be scaled for time effects)
  pub fn update(world: &mut World, events: &mut Events, dt: f32) {
    Self::update_tweens(world, events, dt);
    Self::update_tween_sequences(world, events, dt);
  }

  /// Update all Tween components and apply to Transform/Renderable.
  fn update_tweens(world: &mut World, events: &mut Events, dt: f32) {
    for entity in world.entities_with::<Tween>() {
      // Skip inactive pooled entities
      if world.has::<Inactive>(entity) {
        continue;
      }

      // Check if tween is playing before borrowing mutably
      let should_update = world.get::<Tween>(entity).is_some_and(|t| t.playing);

      if !should_update {
        continue;
      }

      // Capture color start values from Renderable if tween has color targets and hasn't started
      let needs_color_capture = world
        .get::<Tween>(entity)
        .is_some_and(|t| t.has_color_targets() && !t.has_started());

      if needs_color_capture && let Some(renderable) = world.get::<Renderable>(entity) {
        let alpha = renderable.alpha;
        let tint = renderable.tint;
        if let Some(mut tween) = world.get_mut::<Tween>(entity) {
          tween.capture_start_renderable(alpha, tint);
        }
      }

      // Get transform and tween together for update
      if let (Some(mut transform), Some(mut tween)) = (
        world.get_mut::<Transform>(entity),
        world.get_mut::<Tween>(entity),
      ) {
        tween.update(dt, &mut transform);
      }

      // Apply color/alpha tweening to Renderable
      if let (Some(tween), Some(mut renderable)) = (
        world.get::<Tween>(entity),
        world.get_mut::<Renderable>(entity),
      ) {
        if let Some(alpha) = tween.current_alpha() {
          renderable.alpha = alpha;
        }
        if let Some(tint) = tween.current_tint() {
          renderable.tint = tint;
        }
      }

      // Emit tween events
      if let Some(tween) = world.get::<Tween>(entity) {
        if tween.just_started {
          events.emit(TweenStarted { entity });
        }
        if tween.just_finished {
          events.emit(TweenCompleted {
            entity,
            finished: !tween.playing,
          });
        }
      }
    }
  }

  /// Update all TweenSequence components.
  fn update_tween_sequences(world: &mut World, events: &mut Events, dt: f32) {
    for entity in world.entities_with::<TweenSequence>() {
      // Skip inactive pooled entities
      if world.has::<Inactive>(entity) {
        continue;
      }

      // Clear last-frame state (TweenSequence.just_finished has no auto-reset like Tween does)
      if let Some(mut seq) = world.get_mut::<TweenSequence>(entity) {
        seq.just_finished = false;
      }

      // Check if sequence is playing and has a current tween
      let should_update = world
        .get::<TweenSequence>(entity)
        .is_some_and(|s| s.playing && s.current().is_some());

      if !should_update {
        continue;
      }

      // Capture color start values if current tween has color targets and hasn't started
      let needs_color_capture = world.get::<TweenSequence>(entity).is_some_and(|s| {
        s.current()
          .is_some_and(|t| t.has_color_targets() && !t.has_started())
      });

      if needs_color_capture && let Some(renderable) = world.get::<Renderable>(entity) {
        let alpha = renderable.alpha;
        let tint = renderable.tint;
        if let Some(mut seq) = world.get_mut::<TweenSequence>(entity)
          && let Some(tween) = seq.current_mut()
        {
          tween.capture_start_renderable(alpha, tint);
        }
      }

      // Update the current tween in the sequence
      let tween_finished = if let (Some(mut transform), Some(mut seq)) = (
        world.get_mut::<Transform>(entity),
        world.get_mut::<TweenSequence>(entity),
      ) {
        let speed = seq.speed;
        if let Some(tween) = seq.current_mut() {
          tween.update(dt * speed, &mut transform);
          tween.just_finished
        } else {
          false
        }
      } else {
        false
      };

      // Apply color/alpha tweening from current tween to Renderable
      if let (Some(seq), Some(mut renderable)) = (
        world.get::<TweenSequence>(entity),
        world.get_mut::<Renderable>(entity),
      ) && let Some(tween) = seq.current()
      {
        if let Some(alpha) = tween.current_alpha() {
          renderable.alpha = alpha;
        }
        if let Some(tint) = tween.current_tint() {
          renderable.tint = tint;
        }
      }

      // Capture sequence event state before advance mutates it
      let seq_just_started = if let Some(seq) = world.get::<TweenSequence>(entity) {
        seq.current().is_some_and(|t| t.just_started) && seq.current_index() == 0
      } else {
        false
      };

      // Advance to next tween if current one finished
      if tween_finished && let Some(mut seq) = world.get_mut::<TweenSequence>(entity) {
        seq.advance();
      }

      // Emit sequence events using pre-advance state
      if seq_just_started {
        events.emit(TweenStarted { entity });
      }
      if let Some(seq) = world.get::<TweenSequence>(entity)
        && seq.just_finished
      {
        events.emit(TweenCompleted {
          entity,
          finished: true,
        });
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::events::{Events, TweenCompleted, TweenStarted};

  #[test]
  fn test_tween_system_no_crash_on_empty_world() {
    let mut world = World::default();
    let mut events = Events::default();
    // Should not panic on empty world
    TweenSystem::update(&mut world, &mut events, 0.016);
  }

  #[test]
  fn test_tween_emits_started_event() {
    let mut world = World::default();
    let mut events = Events::default();

    world.spawn((
      Transform::new(0.0, 0.0),
      Tween::to_position(100.0, 100.0, 1.0),
    ));

    TweenSystem::update(&mut world, &mut events, 0.016);

    let started: Vec<_> = events.read::<TweenStarted>().collect();
    assert_eq!(started.len(), 1, "Should emit TweenStarted");
  }

  #[test]
  fn test_tween_emits_completed_event() {
    let mut world = World::default();
    let mut events = Events::default();

    world.spawn((
      Transform::new(0.0, 0.0),
      Tween::to_position(100.0, 100.0, 0.1),
    ));

    // Run past duration
    TweenSystem::update(&mut world, &mut events, 0.2);

    let completed: Vec<_> = events.read::<TweenCompleted>().collect();
    assert_eq!(completed.len(), 1, "Should emit TweenCompleted");
    assert!(completed[0].finished, "Once tween should be finished=true");
  }

  #[test]
  fn test_tween_sequence_emits_started_event() {
    let mut world = World::default();
    let mut events = Events::default();

    world.spawn((
      Transform::new(0.0, 0.0),
      TweenSequence::new().then(Tween::to_position(100.0, 0.0, 1.0)),
    ));

    TweenSystem::update(&mut world, &mut events, 0.016);

    let started: Vec<_> = events.read::<TweenStarted>().collect();
    assert_eq!(started.len(), 1, "Should emit TweenStarted for sequence");
  }

  #[test]
  fn test_tween_sequence_emits_completed_on_finish() {
    let mut world = World::default();
    let mut events = Events::default();

    world.spawn((
      Transform::new(0.0, 0.0),
      TweenSequence::new().then(Tween::to_position(100.0, 0.0, 0.1)),
    ));

    // Run past duration
    TweenSystem::update(&mut world, &mut events, 0.2);

    let completed: Vec<_> = events.read::<TweenCompleted>().collect();
    assert_eq!(
      completed.len(),
      1,
      "Should emit TweenCompleted for sequence"
    );
    assert!(completed[0].finished);
  }

  #[test]
  fn test_tween_sequence_speed_doubles_progress() {
    let mut world = World::default();
    let mut events = Events::default();

    let entity = world.spawn((
      Transform::new(0.0, 0.0),
      TweenSequence::new()
        .then(Tween::to_position(100.0, 0.0, 1.0))
        .with_speed(2.0),
    ));

    // At 2x speed, 0.25s real time = 0.5s tween time
    TweenSystem::update(&mut world, &mut events, 0.25);

    let x = world.get::<Transform>(entity).unwrap().x;
    // Should be approximately at 50% progress (50.0) rather than 25% (25.0)
    assert!(
      x > 40.0,
      "At 2x speed, should be past 40% after 0.25s, got {x}"
    );
  }

  #[test]
  fn test_looping_sequence_does_not_emit_completed() {
    let mut world = World::default();
    let mut events = Events::default();

    world.spawn((
      Transform::new(0.0, 0.0),
      TweenSequence::new()
        .then(Tween::to_position(100.0, 0.0, 0.1))
        .with_loop(),
    ));

    // Run past first iteration
    TweenSystem::update(&mut world, &mut events, 0.2);

    // Looping sequence should NOT emit TweenCompleted (it never "finishes")
    let completed: Vec<_> = events.read::<TweenCompleted>().collect();
    assert_eq!(
      completed.len(),
      0,
      "Looping sequence should not emit TweenCompleted"
    );
  }

  #[test]
  fn test_looping_sequence_emits_started_on_restart() {
    let mut world = World::default();
    let mut events = Events::default();

    world.spawn((
      Transform::new(0.0, 0.0),
      TweenSequence::new()
        .then(Tween::to_position(100.0, 0.0, 0.1))
        .with_loop(),
    ));

    // Frame 1: sequence starts
    TweenSystem::update(&mut world, &mut events, 0.016);
    let started: Vec<_> = events.read::<TweenStarted>().collect();
    assert_eq!(started.len(), 1, "Should emit TweenStarted on first play");

    // Frame 2: run past first iteration, advance() loops back to index 0
    events.clear();
    TweenSystem::update(&mut world, &mut events, 0.2);

    // Frame 3: the restarted tween at index 0 now "starts" again
    events.clear();
    TweenSystem::update(&mut world, &mut events, 0.016);

    let started: Vec<_> = events.read::<TweenStarted>().collect();
    assert_eq!(
      started.len(),
      1,
      "Should emit TweenStarted when loop restarts"
    );
  }
}
