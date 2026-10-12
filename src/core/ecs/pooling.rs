//! Object pooling on the entity-disabling filter: inactive pooled entities
//! carry `Disabled` and vanish from every ordinary query.

use std::collections::{HashMap, VecDeque};

use super::kernel::{Bundle, Disabled, Entity, World};
use super::world_ext::WorldExt;

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
///
/// `acquire`, `release` and `is_active` cost O(1). Entities despawned behind
/// the pool's back (a scene cleanup) are dropped when the pool meets them or
/// runs dry.
pub struct Pool<B: Bundle> {
  factory: Box<dyn Fn() -> B + Send + Sync>,
  config: PoolConfig,
  available: Vec<Entity>,
  /// Active entities with the acquisition each entry stands for, oldest
  /// first; an entry whose number no longer matches `active_since` is stale
  /// (released since) and skipped.
  active: VecDeque<(u64, Entity)>,
  active_since: HashMap<Entity, u64>,
  acquisitions: u64,
  spawned: bool,
}

impl<B: Bundle> Pool<B> {
  pub fn new(factory: impl Fn() -> B + Send + Sync + 'static, config: PoolConfig) -> Self {
    Self {
      factory: Box::new(factory),
      config,
      available: Vec::new(),
      active: VecDeque::new(),
      active_since: HashMap::new(),
      acquisitions: 0,
      spawned: false,
    }
  }

  fn total(&self) -> usize {
    self.available.len() + self.active_since.len()
  }

  fn is_current(&self, (number, entity): (u64, Entity)) -> bool {
    self.active_since.get(&entity) == Some(&number)
  }

  /// Drop entities the world no longer has (a scene cleanup despawned them)
  /// and stale active entries.
  fn purge_dead(&mut self, world: &World) {
    self.available.retain(|&e| world.get_entity(e).is_ok());
    self
      .active_since
      .retain(|&e, _| world.get_entity(e).is_ok());
    let active = std::mem::take(&mut self.active);
    self.active = active
      .into_iter()
      .filter(|&entry| self.is_current(entry))
      .collect();
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

  /// The most recently freed entity the world still has.
  fn pop_available(&mut self, world: &World) -> Option<Entity> {
    while let Some(entity) = self.available.pop() {
      if world.get_entity(entity).is_ok() {
        return Some(entity);
      }
    }
    None
  }

  /// The pool has nothing free: purge, then refill as the policy says.
  fn refill(&mut self, world: &mut World) {
    self.purge_dead(world);
    if self.total() == 0 {
      self.expand(world, self.config.initial_capacity.max(1));
      return;
    }
    match self.config.exhausted_policy {
      PoolExhaustedPolicy::Expand => self.expand(world, self.config.expand_batch_size.max(1)),
      PoolExhaustedPolicy::ReturnNone => {}
      PoolExhaustedPolicy::RecycleOldest => {
        if let Some((_, oldest)) = self.active.pop_front() {
          self.active_since.remove(&oldest);
          self.available.push(oldest);
        }
      }
    }
  }

  fn mark_active(&mut self, entity: Entity) {
    self.acquisitions += 1;
    self.active.push_back((self.acquisitions, entity));
    self.active_since.insert(entity, self.acquisitions);
    // Keep stale entries from outgrowing the live ones
    if self.active.len() > 2 * self.active_since.len() + 16 {
      let active = std::mem::take(&mut self.active);
      self.active = active
        .into_iter()
        .filter(|&entry| self.is_current(entry))
        .collect();
    }
  }

  /// Take an entity out of the pool (enabled). Components keep whatever
  /// values they had; set every field you rely on. An interpolated entity
  /// draws where you put it this step instead of sliding from where it was
  /// released.
  pub fn acquire(&mut self, world: &mut World) -> Option<Entity> {
    if !self.spawned {
      self.spawned = true;
      self.expand(world, self.config.initial_capacity.max(1));
    }
    let entity = match self.pop_available(world) {
      Some(entity) => entity,
      None => {
        self.refill(world);
        self.pop_available(world)?
      }
    };
    world.entity_mut(entity).remove::<Disabled>();
    world.skip_interpolation(entity);
    self.mark_active(entity);
    Some(entity)
  }

  /// Return an entity to the pool (disabled). False if it was not active.
  pub fn release(&mut self, world: &mut World, entity: Entity) -> bool {
    if self.active_since.remove(&entity).is_none() {
      return false;
    }
    if let Ok(mut e) = world.get_entity_mut(entity) {
      e.insert(Disabled);
      self.available.push(entity);
    }
    true
  }

  /// Return every active entity to the pool.
  pub fn release_all(&mut self, world: &mut World) {
    let active: Vec<Entity> = self.active().collect();
    self.active.clear();
    self.active_since.clear();
    for entity in active {
      if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(Disabled);
        self.available.push(entity);
      }
    }
  }

  /// Active entities, oldest first.
  pub fn active(&self) -> impl Iterator<Item = Entity> + '_ {
    self
      .active
      .iter()
      .filter(|&&entry| self.is_current(entry))
      .map(|&(_, entity)| entity)
  }

  pub fn is_active(&self, entity: Entity) -> bool {
    self.active_since.contains_key(&entity)
  }

  pub fn stats(&self) -> PoolStats {
    PoolStats {
      total_spawned: self.total(),
      active: self.active_since.len(),
      available: self.available.len(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{Allow, IsResource, Without};
  use crate::core::ecs::transform::{GlobalTransform, PreviousGlobalTransform, Transform};

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
  fn an_acquired_entity_does_not_slide_from_where_it_was_released() {
    let mut world = World::new();
    let mut p = Pool::new(
      || {
        (
          Transform::IDENTITY,
          PreviousGlobalTransform {
            global: GlobalTransform::IDENTITY,
            skip: false,
          },
        )
      },
      PoolConfig::default(),
    );
    let e = p.acquire(&mut world).unwrap();
    assert!(world.get::<PreviousGlobalTransform>(e).unwrap().skip);
    world.get_mut::<PreviousGlobalTransform>(e).unwrap().skip = false;
    p.release(&mut world, e);
    let again = p.acquire(&mut world).unwrap();
    assert_eq!(again, e);
    assert!(world.get::<PreviousGlobalTransform>(e).unwrap().skip);
  }

  #[test]
  fn releasing_from_the_middle_keeps_the_oldest_first_order() {
    let mut world = World::new();
    let mut p = pool(PoolConfig {
      initial_capacity: 3,
      max_capacity: 3,
      expand_batch_size: 1,
      exhausted_policy: PoolExhaustedPolicy::RecycleOldest,
    });
    let a = p.acquire(&mut world).unwrap();
    let b = p.acquire(&mut world).unwrap();
    let c = p.acquire(&mut world).unwrap();
    assert!(p.release(&mut world, a));
    assert!(!p.release(&mut world, a), "already released");
    assert_eq!(p.active().collect::<Vec<_>>(), [b, c]);
    assert_eq!(p.acquire(&mut world), Some(a), "a free entity first");
    assert_eq!(p.acquire(&mut world), Some(b), "then the oldest active one");
    assert_eq!(p.active().collect::<Vec<_>>(), [c, a, b]);
    assert!(p.is_active(b) && !p.is_active(world.spawn_empty().id()));
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
    assert_eq!(p.active().count(), 2);
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
