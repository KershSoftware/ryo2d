//! World - The ECS container that manages entities and components
//!
//! This module provides a wrapper around `hecs::World` with a simplified API
//! that maintains compatibility with the existing Ferrum2D codebase while
//! gaining the benefits of hecs:
//!
//! - **Generational entity IDs** - Prevents use-after-free bugs
//! - **Archetype-based storage** - Cache-friendly iteration
//! - **Thread-safe queries** - `Send + Sync` by default

use hecs::Entity;

/// The ECS World - container for all entities and components
///
/// This is a thin wrapper around `hecs::World` that provides a simplified API
/// compatible with the existing Ferrum2D engine.
///
/// ## Generational IDs
///
/// Entities now use generational IDs internally. If you despawn an entity and
/// spawn a new one, the old entity handle will not accidentally access the new
/// entity's components - the generation counter prevents this.
///
/// ## Example
///
/// ```rust,ignore
/// let mut world = World::new();
///
/// // Spawn an entity with components
/// let player = world.spawn((Transform::new(100.0, 100.0), Velocity::zero()));
///
/// // Add more components later
/// world.insert_one(player, Health { current: 100, max: 100 });
///
/// // Query for components
/// if let Ok(transform) = world.get::<&Transform>(player) {
///     println!("Player at ({}, {})", transform.x, transform.y);
/// }
/// ```
pub struct World {
  inner: hecs::World,
}

impl World {
  /// Create a new empty world
  pub fn new() -> Self {
    World {
      inner: hecs::World::new(),
    }
  }

  // =========================================================================
  // Entity Lifecycle
  // =========================================================================

  /// Spawn an entity with a bundle of components
  ///
  /// Components are passed as a tuple. Unlike the old API, you should provide
  /// components at spawn time when possible for better performance.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// let entity = world.spawn((
  ///     Transform::new(100.0, 200.0),
  ///     Velocity::new(10.0, 0.0),
  ///     SpriteRenderer::new(sprite),
  /// ));
  /// ```
  pub fn spawn(&mut self, components: impl hecs::DynamicBundle) -> Entity {
    self.inner.spawn(components)
  }

  /// Spawn an empty entity (no components)
  ///
  /// You can add components later with `insert` or `insert_one`.
  /// Prefer `spawn()` with components when possible.
  pub fn spawn_empty(&mut self) -> Entity {
    self.inner.spawn(())
  }

  /// Despawn an entity and all its components
  ///
  /// Returns an error if the entity doesn't exist (already despawned or invalid).
  pub fn despawn(&mut self, entity: Entity) -> Result<(), hecs::NoSuchEntity> {
    self.inner.despawn(entity)
  }

  /// Check if an entity is still alive
  pub fn contains(&self, entity: Entity) -> bool {
    self.inner.contains(entity)
  }

  /// Remove all entities from the world
  ///
  /// This despawns every entity and their components. Useful for scene
  /// transitions where you want to start fresh.
  pub fn clear(&mut self) {
    self.inner.clear();
  }

  // =========================================================================
  // Component Access
  // =========================================================================

  /// Add a single component to an existing entity
  ///
  /// If the entity already has this component type, it will be replaced.
  pub fn insert_one<T: hecs::Component>(
    &mut self,
    entity: Entity,
    component: T,
  ) -> Result<(), hecs::NoSuchEntity> {
    self.inner.insert_one(entity, component)
  }

  /// Add multiple components to an existing entity
  ///
  /// Components are passed as a tuple.
  pub fn insert(
    &mut self,
    entity: Entity,
    components: impl hecs::DynamicBundle,
  ) -> Result<(), hecs::NoSuchEntity> {
    self.inner.insert(entity, components)
  }

  /// Remove a component from an entity
  ///
  /// Returns the removed component, or an error if the entity doesn't exist
  /// or doesn't have the component.
  pub fn remove_one<T: hecs::Component>(
    &mut self,
    entity: Entity,
  ) -> Result<T, hecs::ComponentError> {
    self.inner.remove_one::<T>(entity)
  }

  /// Get a shared reference to a component
  ///
  /// Returns `None` if the entity doesn't exist or doesn't have the component.
  pub fn get<T: hecs::Component>(&self, entity: Entity) -> Option<hecs::Ref<'_, T>> {
    self.inner.get::<&T>(entity).ok()
  }

  /// Get a mutable reference to a component
  ///
  /// Returns `None` if the entity doesn't exist or doesn't have the component.
  pub fn get_mut<T: hecs::Component>(&self, entity: Entity) -> Option<hecs::RefMut<'_, T>> {
    self.inner.get::<&mut T>(entity).ok()
  }

  /// Check if an entity has a specific component
  pub fn has<T: hecs::Component>(&self, entity: Entity) -> bool {
    self.inner.get::<&T>(entity).is_ok()
  }

  // =========================================================================
  // Queries - Efficient iteration over entities
  // =========================================================================

  /// Query for entities with specific components
  ///
  /// Returns an iterator over (Entity, components) tuples.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// // Iterate over all entities with Transform and Velocity
  /// for (entity, (transform, velocity)) in world.query::<(&Transform, &Velocity)>() {
  ///     println!("Entity {:?} at ({}, {})", entity, transform.x, transform.y);
  /// }
  ///
  /// // Mutable access
  /// for (entity, (transform, velocity)) in world.query_mut::<(&mut Transform, &Velocity)>() {
  ///     transform.x += velocity.dx * dt;
  /// }
  /// ```
  pub fn query<Q: hecs::Query>(&self) -> hecs::QueryBorrow<'_, Q> {
    self.inner.query::<Q>()
  }

  /// Query with mutable access to the world
  ///
  /// Slightly more efficient than `query()` when you have `&mut self`.
  pub fn query_mut<Q: hecs::Query>(&mut self) -> hecs::QueryMut<'_, Q> {
    self.inner.query_mut::<Q>()
  }

  // =========================================================================
  // Compatibility Layer - Maps old API to new hecs-backed implementation
  // =========================================================================

  /// Add a component to an entity (compatibility method)
  ///
  /// This is equivalent to `insert_one`. Provided for API compatibility.
  #[inline]
  pub fn add_component<T: hecs::Component>(&mut self, entity: Entity, component: T) {
    let _ = self.inner.insert_one(entity, component);
  }

  /// Get a shared reference to a component (compatibility method)
  ///
  /// This is equivalent to `get`. Provided for API compatibility.
  #[inline]
  pub fn get_component<T: hecs::Component>(&self, entity: Entity) -> Option<hecs::Ref<'_, T>> {
    self.get::<T>(entity)
  }

  /// Get a mutable reference to a component (compatibility method)
  ///
  /// This is equivalent to `get_mut`. Provided for API compatibility.
  #[inline]
  pub fn get_component_mut<T: hecs::Component>(
    &self,
    entity: Entity,
  ) -> Option<hecs::RefMut<'_, T>> {
    self.get_mut::<T>(entity)
  }

  /// Check if an entity has a specific component (compatibility method)
  #[inline]
  pub fn has_component<T: hecs::Component>(&self, entity: Entity) -> bool {
    self.has::<T>(entity)
  }

  /// Get all entities with a specific component type
  ///
  /// Note: This collects into a Vec. For better performance in loops,
  /// prefer using `query()` directly.
  pub fn entities_with<T: hecs::Component>(&self) -> Vec<Entity> {
    self
      .inner
      .query::<(Entity, &T)>()
      .iter()
      .map(|(e, _)| e)
      .collect()
  }

  /// Get entities that have both component types
  ///
  /// Note: This collects into a Vec. For better performance in loops,
  /// prefer using `query::<(&A, &B)>()` directly.
  pub fn entities_with_two<A: hecs::Component, B: hecs::Component>(&self) -> Vec<Entity> {
    self
      .inner
      .query::<(Entity, &A, &B)>()
      .iter()
      .map(|(e, _, _)| e)
      .collect()
  }

  /// Iterate with mutable access to a component type
  ///
  /// For better performance, prefer using `query_mut` directly:
  /// ```rust,ignore
  /// for (entity, component) in world.query_mut::<(Entity, &mut T)>() {
  ///     // modify component
  /// }
  /// ```
  pub fn for_each_mut<T: hecs::Component>(&mut self, mut f: impl FnMut(Entity, &mut T)) {
    for (entity, component) in self.inner.query_mut::<(Entity, &mut T)>() {
      f(entity, component);
    }
  }

  /// Get all living entities
  ///
  /// Note: This collects into a Vec. In hecs, there's no persistent list of
  /// alive entities - we iterate over all archetypes.
  pub fn entities(&self) -> Vec<Entity> {
    self.inner.iter().map(|e| e.entity()).collect()
  }

  // =========================================================================
  // Advanced Access
  // =========================================================================

  /// Get direct access to the underlying hecs::World
  ///
  /// Use this for advanced operations not covered by the wrapper API.
  pub fn inner(&self) -> &hecs::World {
    &self.inner
  }

  /// Get mutable access to the underlying hecs::World
  pub fn inner_mut(&mut self) -> &mut hecs::World {
    &mut self.inner
  }
}

impl Default for World {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Debug, Clone, PartialEq)]
  struct Position {
    x: f32,
    y: f32,
  }

  #[derive(Debug, Clone, PartialEq)]
  struct Health {
    current: i32,
    max: i32,
  }

  #[test]
  fn spawn_creates_unique_entities() {
    let mut world = World::new();
    let e1 = world.spawn_empty();
    let e2 = world.spawn_empty();
    let e3 = world.spawn_empty();

    assert_ne!(e1, e2);
    assert_ne!(e2, e3);
    assert_ne!(e1, e3);
  }

  #[test]
  fn add_and_get_component() {
    let mut world = World::new();
    let entity = world.spawn_empty();

    world.add_component(entity, Position { x: 10.0, y: 20.0 });

    let pos = world.get_component::<Position>(entity).unwrap();
    assert_eq!(pos.x, 10.0);
    assert_eq!(pos.y, 20.0);
  }

  #[test]
  fn get_nonexistent_component_returns_none() {
    let mut world = World::new();
    let entity = world.spawn_empty();

    assert!(world.get_component::<Position>(entity).is_none());
  }

  #[test]
  fn modify_component() {
    let mut world = World::new();
    let entity = world.spawn_empty();

    world.add_component(entity, Position { x: 0.0, y: 0.0 });

    {
      let mut pos = world.get_component_mut::<Position>(entity).unwrap();
      pos.x = 100.0;
      pos.y = 200.0;
    }

    let pos = world.get_component::<Position>(entity).unwrap();
    assert_eq!(pos.x, 100.0);
    assert_eq!(pos.y, 200.0);
  }

  #[test]
  fn multiple_component_types() {
    let mut world = World::new();
    let entity = world.spawn_empty();

    world.add_component(entity, Position { x: 5.0, y: 10.0 });
    world.add_component(
      entity,
      Health {
        current: 100,
        max: 100,
      },
    );

    let pos = world.get_component::<Position>(entity).unwrap();
    let health = world.get_component::<Health>(entity).unwrap();

    assert_eq!(pos.x, 5.0);
    assert_eq!(health.current, 100);
  }

  #[test]
  fn entities_with_filters_correctly() {
    let mut world = World::new();

    let e1 = world.spawn_empty();
    let e2 = world.spawn_empty();
    let e3 = world.spawn_empty();

    world.add_component(e1, Position { x: 0.0, y: 0.0 });
    world.add_component(
      e2,
      Health {
        current: 50,
        max: 100,
      },
    );
    world.add_component(e3, Position { x: 1.0, y: 1.0 });
    world.add_component(
      e3,
      Health {
        current: 75,
        max: 100,
      },
    );

    let with_position = world.entities_with::<Position>();
    let with_health = world.entities_with::<Health>();

    assert_eq!(with_position.len(), 2);
    assert_eq!(with_health.len(), 2);
    assert!(with_position.contains(&e1));
    assert!(with_position.contains(&e3));
    assert!(!with_position.contains(&e2));
  }

  #[test]
  fn for_each_mut_modifies_all() {
    let mut world = World::new();

    let e1 = world.spawn_empty();
    let e2 = world.spawn_empty();

    world.add_component(e1, Position { x: 0.0, y: 0.0 });
    world.add_component(e2, Position { x: 10.0, y: 10.0 });

    world.for_each_mut::<Position>(|_, pos| {
      pos.x += 5.0;
      pos.y += 5.0;
    });

    let pos1 = world.get_component::<Position>(e1).unwrap();
    let pos2 = world.get_component::<Position>(e2).unwrap();

    assert_eq!(pos1.x, 5.0);
    assert_eq!(pos2.x, 15.0);
  }

  #[test]
  fn generational_ids_prevent_stale_access() {
    let mut world = World::new();

    // Spawn and despawn an entity
    let old_entity = world.spawn((Position { x: 1.0, y: 2.0 },));
    world.despawn(old_entity).unwrap();

    // Spawn a new entity (may reuse the same index)
    let new_entity = world.spawn((Position { x: 10.0, y: 20.0 },));

    // Old entity handle should NOT access new entity's data
    assert!(!world.contains(old_entity));
    assert!(world.contains(new_entity));

    // The old handle returns None, not the new entity's position
    assert!(world.get_component::<Position>(old_entity).is_none());

    // New handle works correctly
    let pos = world.get_component::<Position>(new_entity).unwrap();
    assert_eq!(pos.x, 10.0);
  }

  #[test]
  fn spawn_with_bundle() {
    let mut world = World::new();

    let entity = world.spawn((
      Position { x: 100.0, y: 200.0 },
      Health {
        current: 50,
        max: 100,
      },
    ));

    let pos = world.get_component::<Position>(entity).unwrap();
    let health = world.get_component::<Health>(entity).unwrap();

    assert_eq!(pos.x, 100.0);
    assert_eq!(health.current, 50);
  }
}
