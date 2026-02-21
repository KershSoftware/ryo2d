//! Knockback system for updating Knockback components.
//!
//! This system handles:
//! - Updating knockback progress and velocity
//! - Applying knockback velocity to Transform position
//! - Updating Velocity component for physics systems
//! - Emitting KnockbackStarted/KnockbackFinished events

use crate::core::ecs::{Inactive, Knockback, Transform, Velocity, World};
use crate::core::events::{Events, KnockbackFinished, KnockbackStarted};

/// System for updating knockback effects on entities.
///
/// Knockback is used for hit reactions, recoil effects, and force-based movement.
/// It applies velocity to entities over a duration with easing.
pub struct KnockbackSystem;

impl KnockbackSystem {
  /// Update all knockback components in the world.
  ///
  /// # Arguments
  /// * `world` - The ECS world containing entities
  /// * `events` - Event bus for emitting knockback events
  /// * `dt` - Delta time (should be scaled for time effects)
  pub fn update(world: &mut World, events: &mut Events, dt: f32) {
    for entity in world.entities_with::<Knockback>() {
      // Skip inactive pooled entities
      if world.has::<Inactive>(entity) {
        continue;
      }

      // Check if knockback is active before borrowing mutably
      let is_active = world.get::<Knockback>(entity).is_some_and(|kb| kb.active);

      if !is_active {
        continue;
      }

      // Update the knockback component
      let (just_started, just_finished, velocity) =
        if let Some(mut kb) = world.get_mut::<Knockback>(entity) {
          let was_just_started = kb.just_started;
          kb.update(dt);
          let vel = kb.current_velocity();
          (was_just_started, kb.just_finished, vel)
        } else {
          continue;
        };

      // Fire events
      if just_started {
        events.emit(KnockbackStarted { entity });
      }
      if just_finished {
        events.emit(KnockbackFinished { entity });
      }

      // Apply velocity to entity
      // For entities with Velocity component, update the velocity
      // For all entities with Transform, apply position change
      if let Some(mut vel_component) = world.get_mut::<Velocity>(entity) {
        // Set velocity to knockback velocity (for physics systems that read Velocity)
        vel_component.dx = velocity.0;
        vel_component.dy = velocity.1;
      }

      // Always apply knockback directly to Transform position
      // (Velocity component is for physics systems, but knockback should move entity directly)
      if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.x += velocity.0 * dt;
        transform.y += velocity.1 * dt;
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use glam::Vec2;

  #[test]
  fn test_knockback_system_no_crash_on_empty_world() {
    let mut world = World::default();
    let mut events = Events::default();
    // Should not panic on empty world
    KnockbackSystem::update(&mut world, &mut events, 0.016);
  }

  #[test]
  fn test_knockback_moves_entity_position() {
    let mut world = World::default();
    let mut events = Events::default();

    // Create entity with Transform and active Knockback pushing right
    let entity = world.spawn((
      Transform::default(),
      Knockback::new(Vec2::new(1.0, 0.0), 100.0), // Push right at 100 px/s
    ));

    let initial_x = world.get::<Transform>(entity).unwrap().x;

    // Update the system
    KnockbackSystem::update(&mut world, &mut events, 0.1); // 100ms

    let final_x = world.get::<Transform>(entity).unwrap().x;

    // Entity should have moved right (position increased)
    assert!(
      final_x > initial_x,
      "Entity should move in knockback direction"
    );
  }

  #[test]
  fn test_knockback_emits_started_event() {
    let mut world = World::default();
    let mut events = Events::default();

    // Create entity with active knockback (just_started = true by default in new())
    world.spawn((
      Transform::default(),
      Knockback::new(Vec2::new(1.0, 0.0), 100.0),
    ));

    KnockbackSystem::update(&mut world, &mut events, 0.016);

    // Check that KnockbackStarted event was emitted
    let started_events: Vec<_> = events.read::<KnockbackStarted>().collect();
    assert_eq!(
      started_events.len(),
      1,
      "Should emit KnockbackStarted event"
    );
  }

  #[test]
  fn test_knockback_skips_inactive_entities() {
    let mut world = World::default();
    let mut events = Events::default();

    // Create inactive entity with knockback
    let entity = world.spawn((
      Transform::default(),
      Knockback::new(Vec2::new(1.0, 0.0), 100.0),
      Inactive,
    ));

    let initial_x = world.get::<Transform>(entity).unwrap().x;

    KnockbackSystem::update(&mut world, &mut events, 0.1);

    let final_x = world.get::<Transform>(entity).unwrap().x;

    // Entity should not have moved (inactive)
    assert_eq!(initial_x, final_x, "Inactive entity should not be affected");
  }

  #[test]
  fn test_knockback_updates_velocity_component() {
    let mut world = World::default();
    let mut events = Events::default();

    // Create entity with Transform, Velocity, and Knockback
    let entity = world.spawn((
      Transform::default(),
      Velocity {
        dx: 0.0,
        dy: 0.0,
        dz: 0.0,
      },
      Knockback::new(Vec2::new(1.0, 0.0), 100.0),
    ));

    KnockbackSystem::update(&mut world, &mut events, 0.016);

    let velocity = world.get::<Velocity>(entity).unwrap();

    // Velocity should be set to knockback velocity
    assert!(velocity.dx > 0.0, "Velocity should be updated by knockback");
  }
}
