//! SquashStretch system for updating SquashStretch components.
//!
//! This system handles:
//! - Updating squash/stretch progress and scale
//! - Capturing base scale from Transform
//! - Applying squash/stretch scale to Transform
//! - Emitting SquashStretchStarted/SquashStretchFinished events

use crate::core::ecs::{Inactive, SquashStretch, Transform, World};
use crate::core::events::{Events, SquashStretchFinished, SquashStretchStarted};

/// System for updating squash and stretch effects on entities.
///
/// SquashStretch is used for juice effects like landing impacts, jump anticipation,
/// and hit reactions. It temporarily modifies an entity's scale with easing.
pub struct SquashStretchSystem;

impl SquashStretchSystem {
  /// Update all squash/stretch components in the world.
  ///
  /// # Arguments
  /// * `world` - The ECS world containing entities
  /// * `events` - Event bus for emitting squash/stretch events
  /// * `dt` - Delta time (should be scaled for time effects)
  pub fn update(world: &mut World, events: &mut Events, dt: f32) {
    for entity in world.entities_with::<SquashStretch>() {
      // Skip inactive pooled entities
      if world.has::<Inactive>(entity) {
        continue;
      }

      // Check if squash/stretch is active before borrowing mutably
      let is_active = world
        .get::<SquashStretch>(entity)
        .is_some_and(|ss| ss.active);

      if !is_active {
        continue;
      }

      // Capture base scale from Transform if not yet captured
      if let (Some(transform), Some(mut ss)) = (
        world.get::<Transform>(entity),
        world.get_mut::<SquashStretch>(entity),
      ) {
        ss.capture_base_scale(transform.scale_x, transform.scale_y);
      }

      // Update the squash/stretch component and get scale
      let (just_started, just_finished, scale) =
        if let Some(mut ss) = world.get_mut::<SquashStretch>(entity) {
          let was_just_started = ss.just_started;
          ss.update(dt);
          let scale = ss.current_scale();
          (was_just_started, ss.just_finished, scale)
        } else {
          continue;
        };

      // Fire events
      if just_started {
        events.emit(SquashStretchStarted { entity });
      }
      if just_finished {
        events.emit(SquashStretchFinished { entity });
      }

      // Apply scale to Transform
      if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.scale_x = scale.0;
        transform.scale_y = scale.1;
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_squash_stretch_system_no_crash_on_empty_world() {
    let mut world = World::default();
    let mut events = Events::default();
    // Should not panic on empty world
    SquashStretchSystem::update(&mut world, &mut events, 0.016);
  }

  #[test]
  fn test_squash_stretch_modifies_scale() {
    let mut world = World::default();
    let mut events = Events::default();

    // Create entity with Transform and active SquashStretch
    // SquashStretch::new() creates an already active effect
    let entity = world.spawn((
      Transform::default(),    // scale_x = 1.0, scale_y = 1.0
      SquashStretch::new(0.3), // 30% squash intensity, starts active
    ));

    let initial_scale_y = world.get::<Transform>(entity).unwrap().scale_y;

    SquashStretchSystem::update(&mut world, &mut events, 0.016);

    let final_scale_y = world.get::<Transform>(entity).unwrap().scale_y;

    // Scale should have changed (squashed on Y axis means scale_y < 1.0)
    assert_ne!(
      initial_scale_y, final_scale_y,
      "Scale should be modified by squash/stretch"
    );
  }

  #[test]
  fn test_squash_stretch_emits_started_event() {
    let mut world = World::default();
    let mut events = Events::default();

    // SquashStretch::new() creates an effect with just_started = true
    world.spawn((Transform::default(), SquashStretch::new(0.3)));

    SquashStretchSystem::update(&mut world, &mut events, 0.016);

    let started_events: Vec<_> = events.read::<SquashStretchStarted>().collect();
    assert_eq!(
      started_events.len(),
      1,
      "Should emit SquashStretchStarted event"
    );
  }

  #[test]
  fn test_squash_stretch_skips_inactive_entities() {
    let mut world = World::default();
    let mut events = Events::default();

    let entity = world.spawn((
      Transform::default(),
      SquashStretch::new(0.3),
      Inactive, // Inactive marker component
    ));

    let initial_scale = world.get::<Transform>(entity).unwrap().scale_y;

    SquashStretchSystem::update(&mut world, &mut events, 0.016);

    let final_scale = world.get::<Transform>(entity).unwrap().scale_y;

    assert_eq!(
      initial_scale, final_scale,
      "Inactive entity should not be affected"
    );
  }

  #[test]
  fn test_squash_stretch_captures_base_scale() {
    let mut world = World::default();
    let mut events = Events::default();

    // Create entity with non-default scale
    let transform = Transform {
      scale_x: 2.0,
      scale_y: 2.0,
      ..Default::default()
    };

    let entity = world.spawn((transform, SquashStretch::new(0.3)));

    // Run system multiple times until effect completes
    for _ in 0..100 {
      SquashStretchSystem::update(&mut world, &mut events, 0.016);
    }

    // After effect completes, scale should return to base (2.0)
    let final_scale = world.get::<Transform>(entity).unwrap().scale_y;
    assert!(
      (final_scale - 2.0).abs() < 0.1,
      "Scale should return to base after effect completes"
    );
  }
}
