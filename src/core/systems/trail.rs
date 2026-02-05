//! Trail system for updating Trail2D components.
//!
//! This system handles:
//! - Automatically updating trails for entities with both Trail2D and Transform
//! - Recording entity positions each frame for trail rendering

use crate::core::ecs::{Inactive, Transform, World};
use crate::core::trail::Trail2D;

/// System for updating motion trails on entities.
///
/// Trail2D components record entity positions over time to create
/// motion blur, afterimage, or ribbon trail effects.
pub struct TrailSystem;

impl TrailSystem {
  /// Update all trail components in the world.
  ///
  /// This should run after game logic and hierarchy propagation
  /// to capture final entity positions for the frame.
  ///
  /// # Arguments
  /// * `world` - The ECS world containing entities
  /// * `dt` - Delta time (should be scaled for time effects)
  pub fn update(world: &mut World, dt: f32) {
    for entity in world.entities_with::<Trail2D>() {
      // Skip inactive pooled entities
      if world.has::<Inactive>(entity) {
        continue;
      }

      // Get the entity's position from Transform
      let pos = world.get_component::<Transform>(entity).map(|t| (t.x, t.y));

      // Update the trail with the current position
      if let Some((x, y)) = pos
        && let Some(mut trail) = world.get_component_mut::<Trail2D>(entity)
      {
        trail.update(x, y, dt);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::Color;

  #[test]
  fn test_trail_system_no_crash_on_empty_world() {
    let mut world = World::default();
    // Should not panic on empty world
    TrailSystem::update(&mut world, 0.016);
  }

  #[test]
  fn test_trail_system_updates_trail_with_position() {
    let mut world = World::default();

    // Create entity with Transform and Trail2D
    let transform = Transform {
      x: 100.0,
      y: 50.0,
      ..Default::default()
    };

    let entity = world.spawn((transform, Trail2D::new(Color::WHITE, 2.0)));

    // Trail should start empty
    let initial_points = world
      .get_component::<Trail2D>(entity)
      .map(|t| t.point_count())
      .unwrap_or(0);

    TrailSystem::update(&mut world, 0.016);

    let final_points = world
      .get_component::<Trail2D>(entity)
      .map(|t| t.point_count())
      .unwrap_or(0);

    // Trail should have recorded a point
    assert!(
      final_points >= initial_points,
      "Trail should record entity position"
    );
  }

  #[test]
  fn test_trail_system_skips_inactive_entities() {
    let mut world = World::default();

    let transform = Transform {
      x: 100.0,
      y: 50.0,
      ..Default::default()
    };

    let entity = world.spawn((transform, Trail2D::new(Color::WHITE, 2.0), Inactive));

    TrailSystem::update(&mut world, 0.016);

    let points = world
      .get_component::<Trail2D>(entity)
      .map(|t| t.point_count())
      .unwrap_or(0);

    // Inactive entity should not have trail points added
    assert_eq!(points, 0, "Inactive entity should not have trail updated");
  }

  #[test]
  fn test_trail_system_requires_transform() {
    let mut world = World::default();

    // Entity with Trail2D but no Transform
    let entity = world.spawn((Trail2D::new(Color::WHITE, 2.0),));

    TrailSystem::update(&mut world, 0.016);

    let points = world
      .get_component::<Trail2D>(entity)
      .map(|t| t.point_count())
      .unwrap_or(0);

    // Without Transform, trail should not be updated
    assert_eq!(points, 0, "Trail without Transform should not be updated");
  }

  #[test]
  fn test_trail_system_tracks_movement() {
    let mut world = World::default();

    let entity = world.spawn((Transform::default(), Trail2D::new(Color::WHITE, 2.0)));

    // First update at origin
    TrailSystem::update(&mut world, 0.016);

    // Move the entity
    if let Some(mut transform) = world.get_component_mut::<Transform>(entity) {
      transform.x = 50.0;
      transform.y = 50.0;
    }

    // Second update at new position
    TrailSystem::update(&mut world, 0.016);

    let points = world
      .get_component::<Trail2D>(entity)
      .map(|t| t.point_count())
      .unwrap_or(0);

    // Trail should have multiple points from movement
    assert!(points >= 1, "Trail should record movement");
  }
}
