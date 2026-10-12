//! Object pooling on the entity-disabling filter: inactive pooled entities
//! carry `Disabled` and vanish from every ordinary query.

use super::kernel::{Bundle, Disabled, Entity, World};

/// What `acquire` does when every pooled entity is in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PoolExhaustedPolicy {
  /// Spawn another batch (up to `max_capacity`, if set)
  #[default]
  Expand,
  /// Hand out nothing
  ReturnNone,
  /// Take back the entity that has been active the longest
  RecycleOldest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolConfig {
  pub initial_capacity: usize,
  /// 0 means unlimited
  pub max_capacity: usize,
  pub expand_batch_size: usize,
  pub exhausted_policy: PoolExhaustedPolicy,
}

impl Default for PoolConfig {
  fn default() -> Self {
    Self {
      initial_capacity: 32,
      max_capacity: 0,
      expand_batch_size: 16,
      exhausted_policy: PoolExhaustedPolicy::Expand,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PoolStats {
  /// Live pooled entities, active or not
  pub total_spawned: usize,
  pub active: usize,
  pub available: usize,
}

/// A pool of entities built from one bundle factory.
pub struct Pool<B: Bundle> {
  factory: Box<dyn Fn() -> B + Send + Sync>,
  config: PoolConfig,
  available: Vec<Entity>,
  /// Oldest first
  active: Vec<Entity>,
  spawned: bool,
}

impl<B: Bundle> Pool<B> {
  pub fn new(factory: impl Fn() -> B + Send + Sync + 'static, config: PoolConfig) -> Self {
    Self {
      factory: Box::new(factory),
      config,
      available: Vec::new(),
      active: Vec::new(),
      spawned: false,
    }
  }

  fn total(&self) -> usize {
    self.available.len() + self.active.len()
  }

  /// Drop entities the world no longer has (a scene cleanup despawned them).
  fn purge_dead(&mut self, world: &World) {
    self.available.retain(|&e| world.get_entity(e).is_ok());
    self.active.retain(|&e| world.get_entity(e).is_ok());
  }

  fn expand(&mut self, world: &mut World, count: usize) {
    for _ in 0..count {
      if self.config.max_capacity != 0 && self.total() >= self.config.max_capacity {
        break;
      }
      let e = world.spawn(((self.factory)(), Disabled)).id();
      self.available.push(e);
    }
  }

  /// Take an entity out of the pool (enabled). Components keep whatever
  /// values they had; set every field you rely on.
  pub fn acquire(&mut self, world: &mut World) -> Option<Entity> {
    self.purge_dead(world);
    if !self.spawned || self.total() == 0 {
      self.spawned = true;
      self.expand(world, self.config.initial_capacity.max(1));
    }
    if self.available.is_empty() {
      match self.config.exhausted_policy {
        PoolExhaustedPolicy::Expand => self.expand(world, self.config.expand_batch_size.max(1)),
        PoolExhaustedPolicy::ReturnNone => {}
        PoolExhaustedPolicy::RecycleOldest => {
          if !self.active.is_empty() {
            let oldest = self.active.remove(0);
            self.available.push(oldest);
          }
        }
      }
    }
    let entity = self.available.pop()?;
    world.entity_mut(entity).remove::<Disabled>();
    self.active.push(entity);
    Some(entity)
  }

  /// Return an entity to the pool (disabled). False if it was not active.
  pub fn release(&mut self, world: &mut World, entity: Entity) -> bool {
    let Some(index) = self.active.iter().position(|&e| e == entity) else {
      return false;
    };
    self.active.remove(index);
    if let Ok(mut e) = world.get_entity_mut(entity) {
      e.insert(Disabled);
      self.available.push(entity);
    }
    true
  }

  /// Return every active entity to the pool.
  pub fn release_all(&mut self, world: &mut World) {
    let active = std::mem::take(&mut self.active);
    for entity in active {
      if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(Disabled);
        self.available.push(entity);
      }
    }
  }

  /// Active entities, oldest first.
  pub fn active(&self) -> &[Entity] {
    &self.active
  }

  pub fn is_active(&self, entity: Entity) -> bool {
    self.active.contains(&entity)
  }

  pub fn stats(&self) -> PoolStats {
    PoolStats {
      total_spawned: self.total(),
      active: self.active.len(),
      available: self.available.len(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{Allow, IsResource, Without};
  use crate::core::ecs::transform::Transform;

  fn pool(config: PoolConfig) -> Pool<Transform> {
    Pool::new(|| Transform::IDENTITY, config)
  }

  #[test]
  fn acquire_enables_and_release_disables() {
    let mut world = World::new();
    let mut p = pool(PoolConfig::default());
    let e = p.acquire(&mut world).unwrap();
    assert!(world.get::<Disabled>(e).is_none());
    assert!(p.is_active(e));
    assert!(p.release(&mut world, e));
    assert!(world.get::<Disabled>(e).is_some());
    let visible = world.query::<&Transform>().iter(&world).count();
    assert_eq!(
      visible, 0,
      "disabled entities are hidden from plain queries"
    );
    let all = world
      .query_filtered::<&Transform, Allow<Disabled>>()
      .iter(&world)
      .count();
    assert_eq!(all, p.stats().total_spawned);
  }

  #[test]
  fn return_none_policy_stops_at_capacity() {
    let mut world = World::new();
    let mut p = pool(PoolConfig {
      initial_capacity: 2,
      max_capacity: 2,
      expand_batch_size: 1,
      exhausted_policy: PoolExhaustedPolicy::ReturnNone,
    });
    assert!(p.acquire(&mut world).is_some());
    assert!(p.acquire(&mut world).is_some());
    assert!(p.acquire(&mut world).is_none());
  }

  #[test]
  fn recycle_oldest_reuses_the_longest_active() {
    let mut world = World::new();
    let mut p = pool(PoolConfig {
      initial_capacity: 2,
      max_capacity: 2,
      expand_batch_size: 1,
      exhausted_policy: PoolExhaustedPolicy::RecycleOldest,
    });
    let first = p.acquire(&mut world).unwrap();
    let _second = p.acquire(&mut world).unwrap();
    let third = p.acquire(&mut world).unwrap();
    assert_eq!(third, first);
    assert_eq!(p.active().len(), 2);
  }

  #[test]
  fn dead_entities_are_purged_after_a_world_cleanup() {
    let mut world = World::new();
    let mut p = pool(PoolConfig {
      initial_capacity: 2,
      max_capacity: 2,
      expand_batch_size: 1,
      exhausted_policy: PoolExhaustedPolicy::ReturnNone,
    });
    let e = p.acquire(&mut world).unwrap();
    // Every game entity, disabled or not; resources live on entities too and
    // must not be swept
    let all: Vec<Entity> = world
      .query_filtered::<Entity, (Allow<Disabled>, Without<IsResource>)>()
      .iter(&world)
      .collect();
    for e in all {
      world.despawn(e);
    }
    let fresh = p
      .acquire(&mut world)
      .expect("the pool refills after its entities died");
    assert_ne!(fresh, e);
    assert!(world.get_entity(fresh).is_ok());
    assert_eq!(
      p.stats().total_spawned,
      2,
      "capacity accounting resets with the purge"
    );
  }
}
