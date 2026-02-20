//! World - The ECS container that manages entities and components
//!
//! This module provides a wrapper around `hecs::World` with a simplified API
//! that maintains compatibility with the existing Ryo2D codebase while
//! gaining the benefits of hecs:
//!
//! - **Generational entity IDs** - Prevents use-after-free bugs
//! - **Archetype-based storage** - Cache-friendly iteration
//! - **Thread-safe queries** - `Send + Sync` by default
//! - **Hierarchy support** - Parent-child relationships with transform propagation

use super::components::Transform;
use super::hierarchy::{Children, Parent, WorldTransform};
use hecs::Entity;

/// The ECS World - container for all entities and components
///
/// This is a thin wrapper around `hecs::World` that provides a simplified API
/// compatible with the existing Ryo2D engine.
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
  /// Generation counter that increments each time clear() is called.
  /// Used by the engine to detect world resets for physics synchronization.
  generation: u64,
  /// Flag set when entities are spawned or despawned.
  /// Used by the engine to invalidate render caches.
  entities_changed: bool,
}

impl World {
  /// Create a new empty world
  pub fn new() -> Self {
    World {
      inner: hecs::World::new(),
      generation: 0,
      entities_changed: false,
    }
  }

  /// Check if entities have been spawned or despawned since last check.
  /// This is used by the engine to invalidate render caches.
  /// Calling this method clears the flag.
  pub fn take_entities_changed(&mut self) -> bool {
    let changed = self.entities_changed;
    self.entities_changed = false;
    changed
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
  ///     Renderable::sprite(sprite),
  /// ));
  /// ```
  pub fn spawn(&mut self, components: impl hecs::DynamicBundle) -> Entity {
    self.entities_changed = true;
    self.inner.spawn(components)
  }

  /// Spawn an empty entity (no components)
  ///
  /// You can add components later with `insert` or `insert_one`.
  /// Prefer `spawn()` with components when possible.
  pub fn spawn_empty(&mut self) -> Entity {
    self.entities_changed = true;
    self.inner.spawn(())
  }

  /// Despawn an entity and all its components
  ///
  /// Returns an error if the entity doesn't exist (already despawned or invalid).
  pub fn despawn(&mut self, entity: Entity) -> Result<(), hecs::NoSuchEntity> {
    self.entities_changed = true;
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
  ///
  /// Note: This increments the world's generation counter, which the engine
  /// uses to detect that physics state needs to be reset.
  pub fn clear(&mut self) {
    self.inner.clear();
    self.generation = self.generation.wrapping_add(1);
    self.entities_changed = true;
  }

  /// Get the current generation counter
  ///
  /// This counter increments each time `clear()` is called. The engine uses
  /// this to detect when the world has been reset and physics needs clearing.
  pub fn generation(&self) -> u64 {
    self.generation
  }

  /// Get the number of entities in the world
  pub fn len(&self) -> u32 {
    self.inner.len()
  }

  /// Check if the world is empty
  pub fn is_empty(&self) -> bool {
    self.inner.is_empty()
  }

  // =========================================================================
  // Hierarchy - Parent/Child Relationships
  // =========================================================================

  /// Spawn an entity as a child of another entity
  ///
  /// The child's `Transform` will be relative to the parent's position.
  /// Use `WorldTransform` to get the computed world-space position.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// let ship = world.spawn((Transform::new(100.0, 200.0),));
  ///
  /// // Spawn turret as child of ship
  /// let turret = world.spawn_child(ship, (
  ///     Transform::new(10.0, 0.0),  // 10 units right of ship center
  ///     Renderable::sprite(turret_sprite),
  /// ));
  /// ```
  pub fn spawn_child(
    &mut self,
    parent: Entity,
    components: impl hecs::DynamicBundle + Send + Sync,
  ) -> Entity {
    // First spawn the child entity with just Parent
    let child = self.inner.spawn((Parent(parent),));
    // Then insert the additional components
    let _ = self.inner.insert(child, components);

    // Update parent's Children list
    if let Ok(mut children) = self.inner.get::<&mut Children>(parent) {
      children.0.push(child);
    } else {
      // Parent doesn't have Children component yet, add it
      let _ = self.inner.insert_one(parent, Children(vec![child]));
    }

    child
  }

  /// Set or change the parent of an entity
  ///
  /// If the entity already has a parent, it will be removed from the old
  /// parent's children list.
  ///
  /// # Errors
  ///
  /// Returns an error if either entity doesn't exist.
  pub fn set_parent(
    &mut self,
    child: Entity,
    new_parent: Entity,
  ) -> Result<(), hecs::NoSuchEntity> {
    // Verify both entities exist
    if !self.inner.contains(child) || !self.inner.contains(new_parent) {
      return Err(hecs::NoSuchEntity);
    }

    // Remove from old parent's children list if it has one
    if let Ok(old_parent) = self.inner.get::<&Parent>(child).map(|p| p.0)
      && let Ok(mut children) = self.inner.get::<&mut Children>(old_parent)
    {
      children.0.retain(|&e| e != child);
    }

    // Set new parent
    let _ = self.inner.insert_one(child, Parent(new_parent));

    // Add to new parent's children list
    if let Ok(mut children) = self.inner.get::<&mut Children>(new_parent) {
      children.0.push(child);
    } else {
      let _ = self.inner.insert_one(new_parent, Children(vec![child]));
    }

    Ok(())
  }

  /// Remove parent from an entity (make it a root entity)
  ///
  /// The entity's local Transform will become its world position.
  pub fn unparent(&mut self, child: Entity) -> Result<(), hecs::NoSuchEntity> {
    if !self.inner.contains(child) {
      return Err(hecs::NoSuchEntity);
    }

    // Get current parent and remove from their children list
    if let Ok(parent) = self.inner.get::<&Parent>(child).map(|p| p.0)
      && let Ok(mut children) = self.inner.get::<&mut Children>(parent)
    {
      children.0.retain(|&e| e != child);
    }

    // Remove Parent component
    let _ = self.inner.remove_one::<Parent>(child);

    Ok(())
  }

  /// Despawn an entity and all its descendants
  ///
  /// This recursively despawns all children, grandchildren, etc.
  pub fn despawn_recursive(&mut self, entity: Entity) -> Result<(), hecs::NoSuchEntity> {
    if !self.inner.contains(entity) {
      return Err(hecs::NoSuchEntity);
    }

    // Collect all descendants first (to avoid borrowing issues)
    let descendants = self.collect_descendants(entity);

    // Remove from parent's children list
    if let Ok(parent) = self.inner.get::<&Parent>(entity).map(|p| p.0)
      && let Ok(mut children) = self.inner.get::<&mut Children>(parent)
    {
      children.0.retain(|&e| e != entity);
    }

    // Despawn all descendants (children first, then entity)
    for descendant in descendants {
      let _ = self.inner.despawn(descendant);
    }

    // Despawn the entity itself
    self.inner.despawn(entity)
  }

  /// Collect all descendants of an entity (children, grandchildren, etc.)
  fn collect_descendants(&self, entity: Entity) -> Vec<Entity> {
    let mut result = Vec::new();
    self.collect_descendants_recursive(entity, &mut result);
    result
  }

  fn collect_descendants_recursive(&self, entity: Entity, result: &mut Vec<Entity>) {
    if let Ok(children) = self.inner.get::<&Children>(entity) {
      for &child in &children.0 {
        result.push(child);
        self.collect_descendants_recursive(child, result);
      }
    }
  }

  /// Get the parent of an entity, if it has one
  pub fn parent(&self, entity: Entity) -> Option<Entity> {
    self.inner.get::<&Parent>(entity).ok().map(|p| p.0)
  }

  /// Get the children of an entity
  pub fn children(&self, entity: Entity) -> Vec<Entity> {
    self
      .inner
      .get::<&Children>(entity)
      .map(|c| c.0.clone())
      .unwrap_or_default()
  }

  /// Check if an entity has a parent
  pub fn has_parent(&self, entity: Entity) -> bool {
    self.inner.get::<&Parent>(entity).is_ok()
  }

  /// Check if an entity has children
  pub fn has_children(&self, entity: Entity) -> bool {
    self
      .inner
      .get::<&Children>(entity)
      .map(|c| !c.0.is_empty())
      .unwrap_or(false)
  }

  // =========================================================================
  // Transform Propagation
  // =========================================================================

  /// Propagate transforms through the hierarchy
  ///
  /// This updates `WorldTransform` for all entities based on their local
  /// `Transform` and parent hierarchy. Call this after updating transforms
  /// and before rendering.
  ///
  /// Entities without a `Transform` component are skipped.
  pub fn propagate_transforms(&mut self) {
    // Phase 1: Update root entities (no Parent component)
    // Collect root entities first to avoid borrow issues
    let roots: Vec<(Entity, Transform)> = {
      let query = self.inner.query::<(Entity, &Transform)>();
      query
        .without::<&Parent>()
        .iter()
        .map(|(e, t)| (e, (*t).clone()))
        .collect()
    };

    for (entity, transform) in roots {
      let world_transform = WorldTransform::from_transform(&transform);
      let _ = self.inner.insert_one(entity, world_transform);

      // Recursively propagate to children
      self.propagate_to_children(entity, world_transform);
    }
  }

  /// Recursively propagate world transforms to children
  fn propagate_to_children(&mut self, parent: Entity, parent_world: WorldTransform) {
    // Get children list (clone to avoid borrow issues)
    let children = self
      .inner
      .get::<&Children>(parent)
      .map(|c| c.0.clone())
      .unwrap_or_default();

    for child in children {
      // Get child's local transform (clone to release borrow)
      let local_transform = self
        .inner
        .get::<&Transform>(child)
        .map(|t| (*t).clone())
        .ok();

      if let Some(local) = local_transform {
        let child_world = WorldTransform::from_parent_and_local(&parent_world, &local);
        let _ = self.inner.insert_one(child, child_world);

        // Recurse to grandchildren
        self.propagate_to_children(child, child_world);
      }
    }
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
  #[deprecated(since = "0.1.0", note = "use `get` instead")]
  #[inline]
  pub fn get_component<T: hecs::Component>(&self, entity: Entity) -> Option<hecs::Ref<'_, T>> {
    self.get::<T>(entity)
  }

  /// Get a mutable reference to a component (compatibility method)
  ///
  /// This is equivalent to `get_mut`. Provided for API compatibility.
  #[deprecated(since = "0.1.0", note = "use `get_mut` instead")]
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

    let pos = world.get::<Position>(entity).unwrap();
    assert_eq!(pos.x, 10.0);
    assert_eq!(pos.y, 20.0);
  }

  #[test]
  fn get_nonexistent_component_returns_none() {
    let mut world = World::new();
    let entity = world.spawn_empty();

    assert!(world.get::<Position>(entity).is_none());
  }

  #[test]
  fn modify_component() {
    let mut world = World::new();
    let entity = world.spawn_empty();

    world.add_component(entity, Position { x: 0.0, y: 0.0 });

    {
      let mut pos = world.get_mut::<Position>(entity).unwrap();
      pos.x = 100.0;
      pos.y = 200.0;
    }

    let pos = world.get::<Position>(entity).unwrap();
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

    let pos = world.get::<Position>(entity).unwrap();
    let health = world.get::<Health>(entity).unwrap();

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

    let pos1 = world.get::<Position>(e1).unwrap();
    let pos2 = world.get::<Position>(e2).unwrap();

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
    assert!(world.get::<Position>(old_entity).is_none());

    // New handle works correctly
    let pos = world.get::<Position>(new_entity).unwrap();
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

    let pos = world.get::<Position>(entity).unwrap();
    let health = world.get::<Health>(entity).unwrap();

    assert_eq!(pos.x, 100.0);
    assert_eq!(health.current, 50);
  }

  #[test]
  fn clear_increments_generation() {
    let mut world = World::new();

    // Initial generation is 0
    assert_eq!(world.generation(), 0);

    // Spawn some entities
    world.spawn((Position { x: 1.0, y: 2.0 },));
    world.spawn((Position { x: 3.0, y: 4.0 },));
    assert_eq!(world.entities().len(), 2);

    // Clear should increment generation
    world.clear();
    assert_eq!(world.generation(), 1);
    assert_eq!(world.entities().len(), 0);

    // Clear again
    world.clear();
    assert_eq!(world.generation(), 2);

    // Spawn after clear works
    let e = world.spawn((Position { x: 10.0, y: 20.0 },));
    assert!(world.contains(e));
    assert_eq!(world.generation(), 2); // Spawn doesn't change generation
  }

  // =========================================================================
  // Hierarchy Tests
  // =========================================================================

  #[test]
  fn spawn_child_creates_parent_relationship() {
    let mut world = World::new();
    let parent = world.spawn((Position { x: 0.0, y: 0.0 },));
    let child = world.spawn_child(parent, (Position { x: 10.0, y: 0.0 },));

    // Child has Parent component
    assert!(world.has_parent(child));
    assert_eq!(world.parent(child), Some(parent));

    // Parent has Children component
    assert!(world.has_children(parent));
    assert_eq!(world.children(parent), vec![child]);
  }

  #[test]
  fn spawn_multiple_children() {
    let mut world = World::new();
    let parent = world.spawn((Position { x: 0.0, y: 0.0 },));
    let c1 = world.spawn_child(parent, (Position { x: 1.0, y: 0.0 },));
    let c2 = world.spawn_child(parent, (Position { x: 2.0, y: 0.0 },));
    let c3 = world.spawn_child(parent, (Position { x: 3.0, y: 0.0 },));

    let children = world.children(parent);
    assert_eq!(children.len(), 3);
    assert!(children.contains(&c1));
    assert!(children.contains(&c2));
    assert!(children.contains(&c3));
  }

  #[test]
  fn set_parent_changes_hierarchy() {
    let mut world = World::new();
    let p1 = world.spawn((Position { x: 0.0, y: 0.0 },));
    let p2 = world.spawn((Position { x: 100.0, y: 0.0 },));
    let child = world.spawn_child(p1, (Position { x: 10.0, y: 0.0 },));

    // Initially child of p1
    assert_eq!(world.parent(child), Some(p1));
    assert_eq!(world.children(p1), vec![child]);

    // Move to p2
    world.set_parent(child, p2).unwrap();
    assert_eq!(world.parent(child), Some(p2));
    assert!(world.children(p1).is_empty());
    assert_eq!(world.children(p2), vec![child]);
  }

  #[test]
  fn unparent_removes_relationship() {
    let mut world = World::new();
    let parent = world.spawn((Position { x: 0.0, y: 0.0 },));
    let child = world.spawn_child(parent, (Position { x: 10.0, y: 0.0 },));

    world.unparent(child).unwrap();

    assert!(!world.has_parent(child));
    assert_eq!(world.parent(child), None);
    assert!(world.children(parent).is_empty());
  }

  #[test]
  fn despawn_recursive_removes_all_descendants() {
    let mut world = World::new();
    let root = world.spawn((Position { x: 0.0, y: 0.0 },));
    let child1 = world.spawn_child(root, (Position { x: 1.0, y: 0.0 },));
    let child2 = world.spawn_child(root, (Position { x: 2.0, y: 0.0 },));
    let grandchild = world.spawn_child(child1, (Position { x: 3.0, y: 0.0 },));

    assert_eq!(world.entities().len(), 4);

    world.despawn_recursive(root).unwrap();

    // All entities should be gone
    assert_eq!(world.entities().len(), 0);
    assert!(!world.contains(root));
    assert!(!world.contains(child1));
    assert!(!world.contains(child2));
    assert!(!world.contains(grandchild));
  }

  #[test]
  fn despawn_recursive_subtree() {
    let mut world = World::new();
    let root = world.spawn((Position { x: 0.0, y: 0.0 },));
    let child1 = world.spawn_child(root, (Position { x: 1.0, y: 0.0 },));
    let child2 = world.spawn_child(root, (Position { x: 2.0, y: 0.0 },));
    let grandchild = world.spawn_child(child1, (Position { x: 3.0, y: 0.0 },));

    // Remove just child1 and its descendants
    world.despawn_recursive(child1).unwrap();

    // Root and child2 should remain
    assert!(world.contains(root));
    assert!(world.contains(child2));
    assert!(!world.contains(child1));
    assert!(!world.contains(grandchild));

    // Root's children should only include child2
    assert_eq!(world.children(root), vec![child2]);
  }

  #[test]
  fn propagate_transforms_root_only() {
    use crate::core::ecs::components::Transform;

    let mut world = World::new();
    let e = world.spawn((Transform {
      x: 10.0,
      y: 20.0,
      z: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      pivot_x: 0.5,
      pivot_y: 0.5,
      offset_x: 0.0,
      offset_y: 0.0,
    },));

    world.propagate_transforms();

    let wt = world.get::<WorldTransform>(e).unwrap();
    assert_eq!(wt.x, 10.0);
    assert_eq!(wt.y, 20.0);
    assert_eq!(wt.rotation, 0.0);
  }

  #[test]
  fn propagate_transforms_with_hierarchy() {
    use crate::core::ecs::components::Transform;

    let mut world = World::new();
    let parent = world.spawn((Transform {
      x: 100.0,
      y: 50.0,
      z: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      pivot_x: 0.5,
      pivot_y: 0.5,
      offset_x: 0.0,
      offset_y: 0.0,
    },));

    let child = world.spawn_child(
      parent,
      (Transform {
        x: 10.0,
        y: 5.0,
        z: 0.0,
        rotation: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        pivot_x: 0.5,
        pivot_y: 0.5,
        offset_x: 0.0,
        offset_y: 0.0,
      },),
    );

    world.propagate_transforms();

    // Parent's world transform equals local transform (no parent)
    let parent_wt = world.get::<WorldTransform>(parent).unwrap();
    assert_eq!(parent_wt.x, 100.0);
    assert_eq!(parent_wt.y, 50.0);

    // Child's world transform is parent + local
    let child_wt = world.get::<WorldTransform>(child).unwrap();
    assert_eq!(child_wt.x, 110.0);
    assert_eq!(child_wt.y, 55.0);
  }

  #[test]
  fn propagate_transforms_with_rotation() {
    use crate::core::ecs::components::Transform;

    let mut world = World::new();
    let parent = world.spawn((Transform {
      x: 100.0,
      y: 100.0,
      z: 0.0,
      rotation: 90.0, // 90 degrees (WorldTransform uses degrees)
      scale_x: 1.0,
      scale_y: 1.0,
      pivot_x: 0.5,
      pivot_y: 0.5,
      offset_x: 0.0,
      offset_y: 0.0,
    },));

    let child = world.spawn_child(
      parent,
      (Transform {
        x: 10.0, // 10 units right in local space
        y: 0.0,  // But parent is rotated 90°, so this should be 10 up in world
        z: 0.0,
        rotation: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        pivot_x: 0.5,
        pivot_y: 0.5,
        offset_x: 0.0,
        offset_y: 0.0,
      },),
    );

    world.propagate_transforms();

    let child_wt = world.get::<WorldTransform>(child).unwrap();
    // After 90° rotation: (10, 0) -> (0, 10)
    // Plus parent offset: (100 + 0, 100 + 10) = (100, 110)
    assert!((child_wt.x - 100.0).abs() < 0.01);
    assert!((child_wt.y - 110.0).abs() < 0.01);
  }
}
