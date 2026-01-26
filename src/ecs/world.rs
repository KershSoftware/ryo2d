//! World - The ECS container that manages entities and components

use std::any::TypeId;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;

use super::entity::{Entity, EntityAllocator};

/// Type-erased storage for components of a single type
struct ComponentVec {
  data: Box<dyn std::any::Any>,
}

impl ComponentVec {
  fn new<T: 'static>() -> Self {
    ComponentVec {
      data: Box::new(RefCell::new(HashMap::<Entity, T>::new())),
    }
  }

  fn borrow<T: 'static>(&self) -> Option<Ref<'_, HashMap<Entity, T>>> {
    self
      .data
      .downcast_ref::<RefCell<HashMap<Entity, T>>>()
      .map(|cell| cell.borrow())
  }

  fn borrow_mut<T: 'static>(&self) -> Option<RefMut<'_, HashMap<Entity, T>>> {
    self
      .data
      .downcast_ref::<RefCell<HashMap<Entity, T>>>()
      .map(|cell| cell.borrow_mut())
  }
}

/// The ECS World - container for all entities and components
pub struct World {
  entities: EntityAllocator,
  alive: Vec<Entity>,
  components: HashMap<TypeId, ComponentVec>,
}

impl World {
  pub fn new() -> Self {
    World {
      entities: EntityAllocator::new(),
      alive: Vec::new(),
      components: HashMap::new(),
    }
  }

  /// Spawn a new entity
  pub fn spawn(&mut self) -> Entity {
    let entity = self.entities.allocate();
    self.alive.push(entity);
    entity
  }

  /// Despawn an entity
  pub fn despawn(&mut self, entity: Entity) {
    self.alive.retain(|&e| e != entity);
    // Note: component cleanup is simplified - orphaned data won't be accessed
  }

  /// Add a component to an entity (replaces if exists)
  pub fn add_component<T: 'static + Send + Sync>(&mut self, entity: Entity, component: T) {
    let type_id = TypeId::of::<T>();
    let storage = self
      .components
      .entry(type_id)
      .or_insert_with(ComponentVec::new::<T>);

    if let Some(mut map) = storage.borrow_mut::<T>() {
      map.insert(entity, component);
    }
  }

  /// Remove a component from an entity
  pub fn remove_component<T: 'static + Send + Sync>(&mut self, entity: Entity) -> Option<T> {
    let type_id = TypeId::of::<T>();
    self
      .components
      .get(&type_id)
      .and_then(|storage| storage.borrow_mut::<T>())
      .and_then(|mut map| map.remove(&entity))
  }

  /// Get an immutable reference to a component
  pub fn get_component<T: 'static + Send + Sync>(&self, entity: Entity) -> Option<Ref<'_, T>> {
    let type_id = TypeId::of::<T>();
    self.components.get(&type_id).and_then(|storage| {
      let map = storage.borrow::<T>()?;
      if !map.contains_key(&entity) {
        return None;
      }
      Some(Ref::map(map, |m| m.get(&entity).unwrap()))
    })
  }

  /// Get a mutable reference to a component
  pub fn get_component_mut<T: 'static + Send + Sync>(
    &self,
    entity: Entity,
  ) -> Option<RefMut<'_, T>> {
    let type_id = TypeId::of::<T>();
    self.components.get(&type_id).and_then(|storage| {
      let map = storage.borrow_mut::<T>()?;
      if !map.contains_key(&entity) {
        return None;
      }
      Some(RefMut::map(map, |m| m.get_mut(&entity).unwrap()))
    })
  }

  /// Check if an entity has a specific component
  pub fn has_component<T: 'static + Send + Sync>(&self, entity: Entity) -> bool {
    let type_id = TypeId::of::<T>();
    self
      .components
      .get(&type_id)
      .and_then(|storage| storage.borrow::<T>())
      .is_some_and(|map| map.contains_key(&entity))
  }

  /// Get all living entities
  pub fn entities(&self) -> &[Entity] {
    &self.alive
  }

  /// Get all entities with a specific component type
  pub fn entities_with<T: 'static + Send + Sync>(&self) -> Vec<Entity> {
    let type_id = TypeId::of::<T>();
    self
      .components
      .get(&type_id)
      .and_then(|storage| storage.borrow::<T>())
      .map(|map| map.keys().copied().collect())
      .unwrap_or_default()
  }

  /// Iterate with mutable access to a component type
  pub fn for_each_mut<T: 'static + Send + Sync>(&self, mut f: impl FnMut(Entity, &mut T)) {
    let type_id = TypeId::of::<T>();
    if let Some(storage) = self.components.get(&type_id)
      && let Some(mut map) = storage.borrow_mut::<T>()
    {
      for (&entity, component) in map.iter_mut() {
        f(entity, component);
      }
    }
  }

  /// Get entities that have both component types
  pub fn entities_with_two<A: 'static + Send + Sync, B: 'static + Send + Sync>(
    &self,
  ) -> Vec<Entity> {
    let type_id_a = TypeId::of::<A>();
    let type_id_b = TypeId::of::<B>();

    let entities_a: std::collections::HashSet<Entity> = self
      .components
      .get(&type_id_a)
      .and_then(|s| s.borrow::<A>())
      .map(|map| map.keys().copied().collect())
      .unwrap_or_default();

    let entities_b: std::collections::HashSet<Entity> = self
      .components
      .get(&type_id_b)
      .and_then(|s| s.borrow::<B>())
      .map(|map| map.keys().copied().collect())
      .unwrap_or_default();

    entities_a.intersection(&entities_b).copied().collect()
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
    let e1 = world.spawn();
    let e2 = world.spawn();
    let e3 = world.spawn();

    assert_ne!(e1, e2);
    assert_ne!(e2, e3);
    assert_ne!(e1, e3);
  }

  #[test]
  fn add_and_get_component() {
    let mut world = World::new();
    let entity = world.spawn();

    world.add_component(entity, Position { x: 10.0, y: 20.0 });

    let pos = world.get_component::<Position>(entity).unwrap();
    assert_eq!(pos.x, 10.0);
    assert_eq!(pos.y, 20.0);
  }

  #[test]
  fn get_nonexistent_component_returns_none() {
    let mut world = World::new();
    let entity = world.spawn();

    assert!(world.get_component::<Position>(entity).is_none());
  }

  #[test]
  fn modify_component() {
    let mut world = World::new();
    let entity = world.spawn();

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
    let entity = world.spawn();

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

    let e1 = world.spawn();
    let e2 = world.spawn();
    let e3 = world.spawn();

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

    let e1 = world.spawn();
    let e2 = world.spawn();

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
}
