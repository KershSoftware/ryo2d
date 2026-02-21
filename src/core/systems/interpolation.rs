//! Interpolation system for smooth rendering between fixed timestep updates.
//!
//! This system handles:
//! - Snapshotting Transform → PreviousTransform before each update
//! - Auto-adding PreviousTransform to entities with interpolation enabled
//! - Resetting the skip flag after it's been used

use crate::core::ecs::{Inactive, PreviousTransform, Renderable, Transform, World};

/// System for managing render interpolation state.
///
/// Call `snapshot()` once per frame BEFORE running fixed timestep updates.
/// This captures the current Transform into PreviousTransform so the renderer
/// can interpolate between frames.
pub struct InterpolationSystem;

impl InterpolationSystem {
  /// Snapshot current transforms for interpolation.
  ///
  /// This should be called once per frame, BEFORE any fixed timestep updates.
  /// It copies Transform → PreviousTransform for all entities with interpolation
  /// enabled.
  ///
  /// # Arguments
  /// * `world` - The ECS world
  pub fn snapshot(world: &mut World) {
    // Collect entities that need PreviousTransform added
    let needs_previous: Vec<_> = world
      .entities_with::<Renderable>()
      .into_iter()
      .filter(|&e| {
        !world.has::<Inactive>(e)
          && world.get::<Renderable>(e).is_some_and(|r| r.interpolate)
          && !world.has::<PreviousTransform>(e)
          && world.get::<Transform>(e).is_some()
      })
      .collect();

    // Collect transform data for entities that need PreviousTransform added
    let to_add: Vec<_> = needs_previous
      .into_iter()
      .filter_map(|entity| {
        let t = world.get::<Transform>(entity)?;
        Some((entity, t.x, t.y, t.z, t.rotation, t.scale_x, t.scale_y))
      })
      .collect();

    // Add PreviousTransform to entities that need it
    for (entity, x, y, z, rotation, scale_x, scale_y) in to_add {
      let prev = PreviousTransform {
        x,
        y,
        z,
        rotation,
        scale_x,
        scale_y,
        skip: true, // Skip interpolation on first frame
      };
      let _ = world.insert_one(entity, prev);
    }

    // Update existing PreviousTransform components
    // First collect the data we need
    let updates: Vec<_> = world
      .entities_with::<PreviousTransform>()
      .into_iter()
      .filter(|&e| !world.has::<Inactive>(e))
      .filter_map(|e| {
        let transform = world.get::<Transform>(e)?;
        Some((
          e,
          transform.x,
          transform.y,
          transform.z,
          transform.rotation,
          transform.scale_x,
          transform.scale_y,
        ))
      })
      .collect();

    // Now apply the updates
    for (entity, x, y, z, rotation, scale_x, scale_y) in updates {
      if let Some(mut prev) = world.get_mut::<PreviousTransform>(entity) {
        // If skip was set, just copy current position (no interpolation this frame)
        // Then clear the skip flag
        if prev.skip {
          prev.x = x;
          prev.y = y;
          prev.z = z;
          prev.rotation = rotation;
          prev.scale_x = scale_x;
          prev.scale_y = scale_y;
          prev.skip = false;
        } else {
          // Normal case: copy current transform to previous
          prev.x = x;
          prev.y = y;
          prev.z = z;
          prev.rotation = rotation;
          prev.scale_x = scale_x;
          prev.scale_y = scale_y;
        }
      }
    }
  }

  /// Mark an entity to skip interpolation on the next frame.
  ///
  /// Use this after teleporting an entity to prevent it from visually
  /// sliding from the old position to the new one.
  ///
  /// # Arguments
  /// * `world` - The ECS world
  /// * `entity` - The entity to skip interpolation for
  pub fn skip_interpolation(world: &mut World, entity: crate::core::ecs::Entity) {
    if let Some(mut prev) = world.get_mut::<PreviousTransform>(entity) {
      prev.skip = true;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::SpriteHandle;
  use crate::core::graphics::Sprite;

  fn test_sprite_handle() -> SpriteHandle {
    SpriteHandle::from_sprite(Sprite::solid_rect(1, 1, 255, 0, 0, 255), "test.png")
  }

  #[test]
  fn test_interpolation_system_no_crash_on_empty_world() {
    let mut world = World::default();
    InterpolationSystem::snapshot(&mut world);
  }

  #[test]
  fn test_interpolation_system_adds_previous_transform() {
    let mut world = World::default();

    // Create entity with interpolation enabled
    let entity = world.spawn((
      Transform::new(100.0, 200.0),
      Renderable::sprite(test_sprite_handle()).interpolated(),
    ));

    // Should not have PreviousTransform yet
    assert!(!world.has::<PreviousTransform>(entity));

    // Run snapshot
    InterpolationSystem::snapshot(&mut world);

    // Should now have PreviousTransform
    assert!(world.has::<PreviousTransform>(entity));

    // PreviousTransform should match Transform
    let prev = world.get::<PreviousTransform>(entity).unwrap();
    assert_eq!(prev.x, 100.0);
    assert_eq!(prev.y, 200.0);
  }

  #[test]
  fn test_interpolation_system_skips_non_interpolated() {
    let mut world = World::default();

    // Create entity WITHOUT interpolation
    let entity = world.spawn((
      Transform::new(100.0, 200.0),
      Renderable::sprite(test_sprite_handle()), // interpolate = false by default
    ));

    InterpolationSystem::snapshot(&mut world);

    // Should NOT have PreviousTransform
    assert!(!world.has::<PreviousTransform>(entity));
  }

  #[test]
  fn test_interpolation_system_updates_previous_transform() {
    let mut world = World::default();

    let entity = world.spawn((
      Transform::new(100.0, 200.0),
      Renderable::sprite(test_sprite_handle()).interpolated(),
    ));

    // First snapshot adds PreviousTransform
    InterpolationSystem::snapshot(&mut world);

    // Move the entity
    if let Some(mut t) = world.get_mut::<Transform>(entity) {
      t.x = 150.0;
      t.y = 250.0;
    }

    // Second snapshot updates PreviousTransform
    InterpolationSystem::snapshot(&mut world);

    let prev = world.get::<PreviousTransform>(entity).unwrap();
    assert_eq!(prev.x, 150.0);
    assert_eq!(prev.y, 250.0);
  }

  #[test]
  fn test_interpolation_system_skip_flag() {
    let mut world = World::default();

    let entity = world.spawn((
      Transform::new(100.0, 200.0),
      Renderable::sprite(test_sprite_handle()).interpolated(),
    ));

    InterpolationSystem::snapshot(&mut world);

    // Set skip flag (simulating teleport)
    InterpolationSystem::skip_interpolation(&mut world, entity);

    // Move entity
    if let Some(mut t) = world.get_mut::<Transform>(entity) {
      t.x = 500.0;
      t.y = 600.0;
    }

    // Snapshot should copy position and clear skip flag
    InterpolationSystem::snapshot(&mut world);

    let prev = world.get::<PreviousTransform>(entity).unwrap();
    assert_eq!(prev.x, 500.0);
    assert_eq!(prev.y, 600.0);
    assert!(!prev.skip);
  }

  #[test]
  fn test_interpolation_system_skips_inactive_entities() {
    let mut world = World::default();

    let entity = world.spawn((
      Transform::new(100.0, 200.0),
      Renderable::sprite(test_sprite_handle()).interpolated(),
      Inactive,
    ));

    InterpolationSystem::snapshot(&mut world);

    // Should NOT have PreviousTransform (entity is inactive)
    assert!(!world.has::<PreviousTransform>(entity));
  }

  #[test]
  fn test_previous_transform_lerp() {
    let prev = PreviousTransform {
      x: 0.0,
      y: 0.0,
      z: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      skip: false,
    };

    let target = Transform::new(100.0, 200.0);

    let (x, y, _, _, _) = prev.lerp(&target, 0.0);
    assert_eq!(x, 0.0);
    assert_eq!(y, 0.0);

    let (x, y, _, _, _) = prev.lerp(&target, 0.5);
    assert_eq!(x, 50.0);
    assert_eq!(y, 100.0);

    let (x, y, _, _, _) = prev.lerp(&target, 1.0);
    assert_eq!(x, 100.0);
    assert_eq!(y, 200.0);
  }
}
