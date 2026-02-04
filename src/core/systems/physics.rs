//! Physics system for synchronizing ECS entities with Rapier physics.
//!
//! This system handles:
//! - Auto-registering new physics entities (RigidBody + Collider + Transform)
//! - Auto-unregistering despawned or inactive entities
//! - Syncing ECS transforms to Rapier (kinematic/dynamic bodies)
//! - Stepping the physics simulation
//! - Syncing physics results back to ECS

use crate::core::ecs::{Entity, Inactive, Transform, Velocity, World};
use crate::core::graphics::GroupedCollider;
use crate::core::physics::{Collider, RapierPhysicsWorld, RigidBody};
use crate::profile_scope;

/// System for physics synchronization between ECS and Rapier.
///
/// Handles the full physics lifecycle: registration, sync, step, and sync-back.
pub struct PhysicsSystem;

impl PhysicsSystem {
  /// Synchronize entities between ECS world and physics world.
  ///
  /// This handles:
  /// - Unregistering despawned entities
  /// - Unregistering inactive (pooled) entities
  /// - Auto-registering new entities with RigidBody + Collider + Transform
  /// - Auto-registering entities with GroupedCollider (composite tiles)
  ///
  /// # Arguments
  /// * `physics` - The Rapier physics world
  /// * `world` - The ECS world
  /// * `last_generation` - Previous world generation (for detecting world.clear())
  ///
  /// # Returns
  /// The current world generation (should be stored for next frame)
  pub fn sync_entities(
    physics: &mut RapierPhysicsWorld,
    world: &World,
    last_generation: u64,
  ) -> u64 {
    // Check if world was cleared (generation changed)
    // This means physics must be reset because hecs reuses entity IDs
    let current_generation = world.generation();
    if current_generation != last_generation {
      physics.clear();
      return current_generation;
    }

    // Auto-unregister despawned entities
    let stale_entities: Vec<Entity> = physics
      .registered_entities()
      .filter(|&e| !world.contains(e))
      .collect();

    for entity in stale_entities {
      physics.unregister_entity(entity);
    }

    // Unregister inactive pooled entities (they should not collide)
    let inactive_entities: Vec<Entity> = physics
      .registered_entities()
      .filter(|&e| world.has::<Inactive>(e))
      .collect();

    for entity in inactive_entities {
      physics.unregister_entity(entity);
    }

    // Auto-register new physics entities (RigidBody + Collider + Transform)
    let new_entities: Vec<Entity> = world
      .entities_with::<RigidBody>()
      .into_iter()
      .filter(|&e| {
        !physics.is_registered(e)
          && !world.has::<Inactive>(e)
          && world.get_component::<Transform>(e).is_some()
          && world.get_component::<Collider>(e).is_some()
      })
      .collect();

    for entity in new_entities {
      let transform = world.get_component::<Transform>(entity).unwrap();
      let velocity = world.get_component::<Velocity>(entity);
      let rb = world.get_component::<RigidBody>(entity).unwrap();
      let col = world.get_component::<Collider>(entity).unwrap();
      physics.register_entity(entity, &transform, velocity.as_deref(), &rb, &col);
    }

    // Auto-register entities with GroupedCollider (for composite tiles)
    let grouped_entities: Vec<Entity> = world
      .entities_with::<RigidBody>()
      .into_iter()
      .filter(|&e| {
        !physics.is_registered(e)
          && !world.has::<Inactive>(e)
          && world.get_component::<Transform>(e).is_some()
          && world.get_component::<GroupedCollider>(e).is_some()
      })
      .collect();

    for entity in grouped_entities {
      let transform = world.get_component::<Transform>(entity).unwrap();
      let velocity = world.get_component::<Velocity>(entity);
      let rb = world.get_component::<RigidBody>(entity).unwrap();
      let grouped = world.get_component::<GroupedCollider>(entity).unwrap();
      physics.register_grouped_entity(entity, &transform, velocity.as_deref(), &rb, &grouped);
    }

    current_generation
  }

  /// Sync ECS changes to Rapier and step the physics simulation.
  ///
  /// # Arguments
  /// * `physics` - The Rapier physics world
  /// * `world` - The ECS world (read-only for sync)
  /// * `gravity` - Gravity vector (x, y)
  pub fn step(physics: &mut RapierPhysicsWorld, world: &World, gravity: (f32, f32)) {
    // Sync ECS changes to Rapier before stepping
    // - Kinematic bodies: game code moves them directly via Transform
    // - Dynamic bodies: game code may teleport them (e.g., reset position)
    // - Velocities: game code may apply impulses via Velocity component
    physics.sync_kinematic_from_ecs(world);
    physics.sync_dynamic_from_ecs(world);
    physics.sync_velocities_from_ecs(world);

    // Step the physics simulation
    profile_scope!("cpu:physics");
    physics.step(gravity);
  }

  /// Sync physics results back to ECS (Transform and Velocity).
  ///
  /// # Arguments
  /// * `physics` - The Rapier physics world
  /// * `world` - The ECS world (mutable for writing results)
  pub fn sync_to_ecs(physics: &RapierPhysicsWorld, world: &mut World) {
    physics.sync_to_ecs(world);
  }

  /// Run the full physics update cycle.
  ///
  /// This is a convenience method that runs sync_entities, step, and sync_to_ecs.
  ///
  /// # Arguments
  /// * `physics` - The Rapier physics world
  /// * `world` - The ECS world
  /// * `gravity` - Gravity vector (x, y)
  /// * `last_generation` - Previous world generation
  ///
  /// # Returns
  /// The current world generation
  pub fn update(
    physics: &mut RapierPhysicsWorld,
    world: &mut World,
    gravity: (f32, f32),
    last_generation: u64,
  ) -> u64 {
    let generation = Self::sync_entities(physics, world, last_generation);
    Self::step(physics, world, gravity);
    Self::sync_to_ecs(physics, world);
    generation
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_physics_system_no_crash_on_empty_world() {
    let world = World::default();
    let mut physics = RapierPhysicsWorld::new();

    // Should not panic on empty world
    let generation = PhysicsSystem::sync_entities(&mut physics, &world, 0);
    assert_eq!(generation, world.generation());
  }

  #[test]
  fn test_physics_system_step_no_crash() {
    let world = World::default();
    let mut physics = RapierPhysicsWorld::new();

    // Should not panic
    PhysicsSystem::step(&mut physics, &world, (0.0, -9.81));
  }

  #[test]
  fn test_physics_system_full_update() {
    let mut world = World::default();
    let mut physics = RapierPhysicsWorld::new();

    // Full update cycle should not panic
    let generation = PhysicsSystem::update(&mut physics, &mut world, (0.0, -9.81), 0);
    assert_eq!(generation, world.generation());
  }

  #[test]
  fn test_physics_system_detects_world_clear() {
    let mut world = World::default();
    let mut physics = RapierPhysicsWorld::new();

    let initial_gen = world.generation();

    // Clear world (increments generation)
    world.clear();

    let new_gen = PhysicsSystem::sync_entities(&mut physics, &world, initial_gen);

    // Generation should have changed
    assert_ne!(new_gen, initial_gen);
    assert_eq!(new_gen, world.generation());
  }

  #[test]
  fn test_physics_system_registers_entity() {
    let mut world = World::default();
    let mut physics = RapierPhysicsWorld::new();

    // Create a physics entity
    let entity = world.spawn((
      Transform::default(),
      RigidBody::new(),
      Collider::circle_collider(16.0),
    ));

    // Sync should register it
    PhysicsSystem::sync_entities(&mut physics, &world, world.generation());

    assert!(physics.is_registered(entity));
  }

  #[test]
  fn test_physics_system_skips_inactive_entity() {
    let mut world = World::default();
    let mut physics = RapierPhysicsWorld::new();

    // Create an inactive physics entity
    let entity = world.spawn((
      Transform::default(),
      RigidBody::new(),
      Collider::circle_collider(16.0),
      Inactive,
    ));

    // Sync should NOT register it (it's inactive)
    PhysicsSystem::sync_entities(&mut physics, &world, world.generation());

    assert!(!physics.is_registered(entity));
  }

  #[test]
  fn test_physics_system_unregisters_despawned_entity() {
    let mut world = World::default();
    let mut physics = RapierPhysicsWorld::new();

    // Create and register a physics entity
    let entity = world.spawn((
      Transform::default(),
      RigidBody::new(),
      Collider::circle_collider(16.0),
    ));
    PhysicsSystem::sync_entities(&mut physics, &world, world.generation());
    assert!(physics.is_registered(entity));

    // Despawn the entity
    world.despawn(entity).unwrap();

    // Sync should unregister it
    PhysicsSystem::sync_entities(&mut physics, &world, world.generation());

    assert!(!physics.is_registered(entity));
  }
}
