//! Object Pooling System
//!
//! Pre-allocates and recycles entities to avoid allocation overhead during gameplay.
//! Ideal for high-frequency spawning scenarios like bullets, particles, and enemies.
//!
//! # Overview
//!
//! Object pools work by:
//! 1. Pre-spawning entities with `Inactive` marker at initialization
//! 2. Moving entities from available → active when acquired (swapping `Inactive` for `Active`)
//! 3. Moving entities from active → available when released (swapping `Active` for `Inactive`)
//!
//! Inactive entities are automatically skipped by rendering and physics systems.
//!
//! # Example
//!
//! ```rust,ignore
//! // In scene struct
//! struct GameScene {
//!     bullet_pool: Pool<(Transform, Renderable, Collider, Bullet)>,
//! }
//!
//! // Initialize in on_enter
//! fn on_enter(&mut self, ctx: &mut SceneContext) {
//!     let sprite = ctx.assets.load_sprite("bullet.png");
//!     self.bullet_pool = Pool::new(
//!         &mut ctx.world,
//!         PoolConfig::default().with_capacity(200),
//!         move || (
//!             Transform::default(),
//!             Renderable::sprite(sprite.clone()),
//!             Collider::circle(4.0).as_trigger(),
//!             Bullet::default(),
//!         )
//!     );
//! }
//!
//! // Spawn bullet (no allocation!)
//! fn fire(&mut self, ctx: &mut SceneContext, x: f32, y: f32, vx: f32, vy: f32) {
//!     if let Some(entity) = self.bullet_pool.acquire(&mut ctx.world) {
//!         if let Some(mut t) = ctx.world.get_mut::<Transform>(entity) {
//!             t.x = x;
//!             t.y = y;
//!         }
//!     }
//! }
//!
//! // Return to pool (no deallocation!)
//! fn despawn_bullet(&mut self, ctx: &mut SceneContext, entity: Entity) {
//!     self.bullet_pool.release(&mut ctx.world, entity);
//! }
//! ```

use super::active::{Active, Inactive};
use super::world::World;
use hecs::{DynamicBundle, Entity};

/// Configuration for pool behavior.
///
/// Controls initial capacity, growth limits, and expansion behavior.
#[derive(Debug, Clone)]
pub struct PoolConfig {
  /// Number of entities to pre-spawn at pool creation (default: 32)
  pub initial_capacity: usize,
  /// Maximum entities the pool can hold. 0 = unlimited (default: 0)
  pub max_capacity: usize,
  /// Number of entities to spawn when pool is exhausted (default: 16)
  pub expand_batch_size: usize,
  /// What to do when pool is empty and max capacity reached
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

impl PoolConfig {
  /// Create a new pool config with default values.
  pub fn new() -> Self {
    Self::default()
  }

  /// Set initial capacity (entities pre-spawned at creation).
  pub fn with_capacity(mut self, capacity: usize) -> Self {
    self.initial_capacity = capacity;
    self
  }

  /// Set maximum capacity (0 = unlimited).
  pub fn with_max_capacity(mut self, max: usize) -> Self {
    self.max_capacity = max;
    self
  }

  /// Set batch size for expansion when pool runs out.
  pub fn with_expand_batch_size(mut self, batch_size: usize) -> Self {
    self.expand_batch_size = batch_size;
    self
  }

  /// Set policy for when pool is exhausted.
  pub fn with_exhausted_policy(mut self, policy: PoolExhaustedPolicy) -> Self {
    self.exhausted_policy = policy;
    self
  }
}

/// What to do when the pool has no available entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PoolExhaustedPolicy {
  /// Spawn more entities up to max_capacity (default).
  #[default]
  Expand,
  /// Return None from acquire() - caller handles shortage.
  ReturnNone,
  /// Recycle the oldest active entity (force release).
  RecycleOldest,
}

/// Statistics for debugging and profiling pool behavior.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
  /// Total entities ever spawned by this pool
  pub total_spawned: usize,
  /// Entities currently available for acquisition
  pub current_available: usize,
  /// Entities currently in active use
  pub current_active: usize,
  /// Peak number of active entities at any point
  pub peak_active: usize,
  /// Total acquire() calls
  pub acquires: usize,
  /// Total release() calls
  pub releases: usize,
  /// Total expand() operations
  pub expansions: usize,
}

/// Generic object pool for entity reuse.
///
/// Maintains two lists of entities:
/// - **available**: Inactive entities ready for reuse
/// - **active**: Currently in-use entities
///
/// The pool uses a factory closure to create component bundles for new entities.
/// Entities are spawned with `Inactive` and swapped to `Active` on acquire.
///
/// # World Generation Safety
///
/// The pool tracks `world.generation()` to detect when `world.clear()` has been
/// called (e.g., during scene transitions). When detected, the pool automatically
/// clears its entity lists to prevent dangling references.
pub struct Pool<B: DynamicBundle + Clone> {
  /// Entities ready for reuse (have Inactive component)
  available: Vec<Entity>,
  /// Entities currently in use (have Active component)
  active: Vec<Entity>,
  /// Pool configuration
  config: PoolConfig,
  /// Factory for creating component bundles
  factory: Box<dyn Fn() -> B + Send + Sync>,
  /// World generation when pool was last valid
  world_generation: u64,
  /// Usage statistics
  stats: PoolStats,
}

impl<B: DynamicBundle + Clone + Send + Sync + 'static> Pool<B> {
  /// Create a new object pool and pre-warm it with entities.
  ///
  /// The factory closure creates the component bundle for each entity.
  /// Entities are spawned with `Inactive` marker and stored in the available list.
  ///
  /// # Arguments
  ///
  /// * `world` - The ECS world to spawn entities in
  /// * `config` - Pool configuration (capacity, limits, policies)
  /// * `factory` - Closure that creates component bundles for new entities
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// let bullet_sprite = ctx.assets.load_sprite("bullet.png");
  /// let pool = Pool::new(
  ///     &mut ctx.world,
  ///     PoolConfig::default().with_capacity(100),
  ///     move || (
  ///         Transform::default(),
  ///         Renderable::sprite(bullet_sprite.clone()),
  ///         Bullet::default(),
  ///     )
  /// );
  /// ```
  pub fn new<F>(world: &mut World, config: PoolConfig, factory: F) -> Self
  where
    F: Fn() -> B + Send + Sync + 'static,
  {
    let mut pool = Pool {
      available: Vec::with_capacity(config.initial_capacity),
      active: Vec::new(),
      config,
      factory: Box::new(factory),
      world_generation: world.generation(),
      stats: PoolStats::default(),
    };

    // Pre-warm the pool
    pool.expand(world, pool.config.initial_capacity);

    pool
  }

  /// Acquire an entity from the pool.
  ///
  /// Returns an entity that has been activated (Inactive → Active swap).
  /// The entity's components are in their default/factory state - caller should
  /// update them (position, velocity, etc.) after acquisition.
  ///
  /// Returns `None` if:
  /// - Pool is exhausted and policy is `ReturnNone`
  /// - Pool is at max capacity and can't expand
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// if let Some(bullet) = pool.acquire(&mut ctx.world) {
  ///     if let Some(mut t) = ctx.world.get_mut::<Transform>(bullet) {
  ///         t.x = spawn_x;
  ///         t.y = spawn_y;
  ///     }
  /// }
  /// ```
  pub fn acquire(&mut self, world: &mut World) -> Option<Entity> {
    // Check for world generation change (scene cleared)
    self.check_generation(world);

    self.stats.acquires += 1;

    // Try to get from available pool
    if let Some(entity) = self.available.pop() {
      self.activate_entity(world, entity);
      self.active.push(entity);
      self.update_peak_stats();
      self.stats.current_available = self.available.len();
      self.stats.current_active = self.active.len();
      return Some(entity);
    }

    // Pool empty - handle based on policy
    match self.config.exhausted_policy {
      PoolExhaustedPolicy::Expand => {
        // Check max capacity
        if self.config.max_capacity > 0 {
          let total = self.stats.total_spawned;
          if total >= self.config.max_capacity {
            return None;
          }
          // Limit batch to not exceed max
          let remaining = self.config.max_capacity - total;
          let batch = self.config.expand_batch_size.min(remaining);
          self.expand(world, batch);
        } else {
          self.expand(world, self.config.expand_batch_size);
        }

        // Try again after expansion
        if let Some(entity) = self.available.pop() {
          self.activate_entity(world, entity);
          self.active.push(entity);
          self.update_peak_stats();
          self.stats.current_available = self.available.len();
          self.stats.current_active = self.active.len();
          return Some(entity);
        }
        None
      }
      PoolExhaustedPolicy::ReturnNone => None,
      PoolExhaustedPolicy::RecycleOldest => {
        // Force-release the oldest active entity and reuse it
        if let Some(entity) = self.active.first().copied() {
          self.release(world, entity);
          // Now it's in available, acquire it
          if let Some(entity) = self.available.pop() {
            self.activate_entity(world, entity);
            self.active.push(entity);
            self.stats.current_available = self.available.len();
            self.stats.current_active = self.active.len();
            return Some(entity);
          }
        }
        None
      }
    }
  }

  /// Release an entity back to the pool.
  ///
  /// The entity is deactivated (Active → Inactive swap) and returned to
  /// the available list for reuse. The entity's components remain in their
  /// current state - they will be overwritten on next acquisition.
  ///
  /// Returns `true` if the entity was found and released, `false` if it
  /// wasn't in the active list (already released or not from this pool).
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// // Bullet went off screen, return to pool
  /// pool.release(&mut ctx.world, bullet_entity);
  /// ```
  pub fn release(&mut self, world: &mut World, entity: Entity) -> bool {
    // Check for world generation change (scene cleared)
    self.check_generation(world);

    // Find and remove from active list
    if let Some(idx) = self.active.iter().position(|&e| e == entity) {
      self.active.swap_remove(idx);
      self.deactivate_entity(world, entity);
      self.available.push(entity);
      self.stats.releases += 1;
      self.stats.current_available = self.available.len();
      self.stats.current_active = self.active.len();
      return true;
    }

    false
  }

  /// Release all active entities back to the pool.
  ///
  /// Useful for resetting gameplay state (e.g., clearing all bullets on level end).
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// // Level complete, return all bullets to pool
  /// pool.release_all(&mut ctx.world);
  /// ```
  pub fn release_all(&mut self, world: &mut World) {
    // Check for world generation change (scene cleared)
    self.check_generation(world);

    // Collect entities to avoid borrow issues
    let entities: Vec<Entity> = self.active.drain(..).collect();

    // Deactivate and move to available
    for entity in entities {
      // Inline deactivate to avoid borrow issues
      let _ = world.remove_one::<Active>(entity);
      let _ = world.insert_one(entity, Inactive);
      self.available.push(entity);
      self.stats.releases += 1;
    }
    self.stats.current_available = self.available.len();
    self.stats.current_active = 0;
  }

  /// Iterate over all currently active entities.
  ///
  /// Useful for batch updates (e.g., updating all bullet positions).
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// for &bullet in pool.active_iter() {
  ///     if let Some(mut t) = ctx.world.get_mut::<Transform>(bullet) {
  ///         t.y -= bullet_speed * dt;
  ///     }
  /// }
  /// ```
  pub fn active_iter(&self) -> impl Iterator<Item = &Entity> {
    self.active.iter()
  }

  /// Get the number of currently active entities.
  pub fn active_count(&self) -> usize {
    self.active.len()
  }

  /// Get the number of available (inactive) entities.
  pub fn available_count(&self) -> usize {
    self.available.len()
  }

  /// Get pool usage statistics.
  pub fn stats(&self) -> &PoolStats {
    &self.stats
  }

  /// Check if an entity belongs to this pool (either active or available).
  pub fn contains(&self, entity: Entity) -> bool {
    self.active.contains(&entity) || self.available.contains(&entity)
  }

  /// Check if an entity is currently active in this pool.
  pub fn is_active(&self, entity: Entity) -> bool {
    self.active.contains(&entity)
  }

  // =========================================================================
  // Internal Methods
  // =========================================================================

  /// Expand the pool by spawning new entities.
  fn expand(&mut self, world: &mut World, count: usize) {
    self.stats.expansions += 1;

    for _ in 0..count {
      let bundle = (self.factory)();
      // Spawn with the user's bundle first, then add Inactive separately
      // This is necessary because we can't flatten arbitrary tuple bundles
      let entity = world.spawn(bundle);
      let _ = world.insert_one(entity, Inactive);
      self.available.push(entity);
      self.stats.total_spawned += 1;
    }
    self.stats.current_available = self.available.len();
  }

  /// Activate an entity (remove Inactive, add Active).
  fn activate_entity(&self, world: &mut World, entity: Entity) {
    let _ = world.remove_one::<Inactive>(entity);
    let _ = world.insert_one(entity, Active);
  }

  /// Deactivate an entity (remove Active, add Inactive).
  fn deactivate_entity(&self, world: &mut World, entity: Entity) {
    let _ = world.remove_one::<Active>(entity);
    let _ = world.insert_one(entity, Inactive);
  }

  /// Check if world generation has changed (scene cleared) and reset pool if so.
  fn check_generation(&mut self, world: &World) {
    let current_gen = world.generation();
    if current_gen != self.world_generation {
      // World was cleared - all our entities are gone
      self.available.clear();
      self.active.clear();
      self.stats.current_available = 0;
      self.stats.current_active = 0;
      self.world_generation = current_gen;
    }
  }

  /// Update peak active count statistic.
  fn update_peak_stats(&mut self) {
    if self.active.len() > self.stats.peak_active {
      self.stats.peak_active = self.active.len();
    }
  }
}

// Manual Default impl since we can't derive with Box<dyn Fn>
impl<B: DynamicBundle + Clone + Default + Send + Sync + 'static> Default for Pool<B> {
  fn default() -> Self {
    // Create a placeholder pool - must be initialized properly with new()
    Pool {
      available: Vec::new(),
      active: Vec::new(),
      config: PoolConfig::default(),
      factory: Box::new(B::default),
      world_generation: 0,
      stats: PoolStats::default(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::Transform;

  #[derive(Debug, Clone, Default)]
  struct TestComponent {
    value: i32,
  }

  #[test]
  fn pool_creation_prewarms() {
    let mut world = World::new();
    let pool = Pool::new(&mut world, PoolConfig::default().with_capacity(10), || {
      (Transform::default(), TestComponent::default())
    });

    assert_eq!(pool.available_count(), 10);
    assert_eq!(pool.active_count(), 0);
    assert_eq!(pool.stats().total_spawned, 10);
  }

  #[test]
  fn acquire_activates_entity() {
    let mut world = World::new();
    let mut pool = Pool::new(&mut world, PoolConfig::default().with_capacity(5), || {
      (Transform::default(), TestComponent::default())
    });

    let entity = pool.acquire(&mut world).unwrap();

    assert!(world.has::<Active>(entity));
    assert!(!world.has::<Inactive>(entity));
    assert_eq!(pool.available_count(), 4);
    assert_eq!(pool.active_count(), 1);
  }

  #[test]
  fn release_deactivates_entity() {
    let mut world = World::new();
    let mut pool = Pool::new(&mut world, PoolConfig::default().with_capacity(5), || {
      (Transform::default(), TestComponent::default())
    });

    let entity = pool.acquire(&mut world).unwrap();
    assert!(pool.release(&mut world, entity));

    assert!(!world.has::<Active>(entity));
    assert!(world.has::<Inactive>(entity));
    assert_eq!(pool.available_count(), 5);
    assert_eq!(pool.active_count(), 0);
  }

  #[test]
  fn acquire_release_cycle() {
    let mut world = World::new();
    let mut pool = Pool::new(&mut world, PoolConfig::default().with_capacity(3), || {
      (Transform::default(), TestComponent::default())
    });

    // Acquire all
    let e1 = pool.acquire(&mut world).unwrap();
    let e2 = pool.acquire(&mut world).unwrap();
    let e3 = pool.acquire(&mut world).unwrap();
    assert_eq!(pool.available_count(), 0);
    assert_eq!(pool.active_count(), 3);

    // Release some
    pool.release(&mut world, e2);
    assert_eq!(pool.available_count(), 1);
    assert_eq!(pool.active_count(), 2);

    // Acquire again - should get e2 back (LIFO)
    let e4 = pool.acquire(&mut world).unwrap();
    assert_eq!(e4, e2);

    // Cleanup
    pool.release(&mut world, e1);
    pool.release(&mut world, e3);
    pool.release(&mut world, e4);
    assert_eq!(pool.available_count(), 3);
    assert_eq!(pool.active_count(), 0);
  }

  #[test]
  fn expand_policy_grows_pool() {
    let mut world = World::new();
    let mut pool = Pool::new(
      &mut world,
      PoolConfig::default()
        .with_capacity(2)
        .with_expand_batch_size(3)
        .with_exhausted_policy(PoolExhaustedPolicy::Expand),
      || (Transform::default(), TestComponent::default()),
    );

    // Use initial capacity
    pool.acquire(&mut world);
    pool.acquire(&mut world);
    assert_eq!(pool.available_count(), 0);

    // Next acquire triggers expansion
    let entity = pool.acquire(&mut world);
    assert!(entity.is_some());
    assert_eq!(pool.stats().total_spawned, 5); // 2 initial + 3 expanded
    assert_eq!(pool.stats().expansions, 2); // initial + one expand
  }

  #[test]
  fn return_none_policy() {
    let mut world = World::new();
    let mut pool = Pool::new(
      &mut world,
      PoolConfig::default()
        .with_capacity(2)
        .with_exhausted_policy(PoolExhaustedPolicy::ReturnNone),
      || (Transform::default(), TestComponent::default()),
    );

    pool.acquire(&mut world);
    pool.acquire(&mut world);
    assert!(pool.acquire(&mut world).is_none());
  }

  #[test]
  fn recycle_oldest_policy() {
    let mut world = World::new();
    let mut pool = Pool::new(
      &mut world,
      PoolConfig::default()
        .with_capacity(2)
        .with_exhausted_policy(PoolExhaustedPolicy::RecycleOldest),
      || (Transform::default(), TestComponent { value: 0 }),
    );

    let e1 = pool.acquire(&mut world).unwrap();
    let _e2 = pool.acquire(&mut world).unwrap();

    // Mark e1 so we can identify it
    if let Some(mut c) = world.get_mut::<TestComponent>(e1) {
      c.value = 42;
    }

    // Pool is full, next acquire recycles oldest (e1)
    let e3 = pool.acquire(&mut world).unwrap();
    assert_eq!(e3, e1);

    // The recycled entity still has its components
    let c = world.get::<TestComponent>(e3).unwrap();
    assert_eq!(c.value, 42);
  }

  #[test]
  fn max_capacity_limits_growth() {
    let mut world = World::new();
    let mut pool = Pool::new(
      &mut world,
      PoolConfig::default()
        .with_capacity(2)
        .with_max_capacity(4)
        .with_expand_batch_size(10)
        .with_exhausted_policy(PoolExhaustedPolicy::Expand),
      || (Transform::default(), TestComponent::default()),
    );

    // Use all initial
    pool.acquire(&mut world);
    pool.acquire(&mut world);

    // Expand, but limited to max_capacity
    pool.acquire(&mut world);
    assert_eq!(pool.stats().total_spawned, 4); // Not 12 (2 + 10)

    pool.acquire(&mut world);

    // At max capacity, no more
    assert!(pool.acquire(&mut world).is_none());
  }

  #[test]
  fn release_all_returns_everything() {
    let mut world = World::new();
    let mut pool = Pool::new(&mut world, PoolConfig::default().with_capacity(5), || {
      (Transform::default(), TestComponent::default())
    });

    pool.acquire(&mut world);
    pool.acquire(&mut world);
    pool.acquire(&mut world);
    assert_eq!(pool.active_count(), 3);
    assert_eq!(pool.available_count(), 2);

    pool.release_all(&mut world);
    assert_eq!(pool.active_count(), 0);
    assert_eq!(pool.available_count(), 5);
  }

  #[test]
  fn world_clear_invalidates_pool() {
    let mut world = World::new();
    let mut pool = Pool::new(&mut world, PoolConfig::default().with_capacity(5), || {
      (Transform::default(), TestComponent::default())
    });

    pool.acquire(&mut world);
    pool.acquire(&mut world);
    assert_eq!(pool.active_count(), 2);
    assert_eq!(pool.available_count(), 3);

    // Simulate scene transition
    world.clear();

    // Next operation detects generation change
    let entity = pool.acquire(&mut world);
    // Pool should have detected the clear and reset
    // Then expanded to get new entities
    assert!(entity.is_some());
    assert_eq!(pool.stats().total_spawned, 5 + 16); // original + expand batch
  }

  #[test]
  fn stats_tracking() {
    let mut world = World::new();
    let mut pool = Pool::new(&mut world, PoolConfig::default().with_capacity(3), || {
      (Transform::default(), TestComponent::default())
    });

    let e1 = pool.acquire(&mut world).unwrap();
    let e2 = pool.acquire(&mut world).unwrap();
    pool.release(&mut world, e1);
    pool.acquire(&mut world);

    let stats = pool.stats();
    assert_eq!(stats.total_spawned, 3);
    assert_eq!(stats.acquires, 3);
    assert_eq!(stats.releases, 1);
    assert_eq!(stats.peak_active, 2);
    assert_eq!(stats.current_active, 2);
    assert_eq!(stats.current_available, 1);

    // Cleanup
    pool.release(&mut world, e2);
  }

  #[test]
  fn active_iter_returns_active_entities() {
    let mut world = World::new();
    let mut pool = Pool::new(&mut world, PoolConfig::default().with_capacity(5), || {
      (Transform::default(), TestComponent::default())
    });

    let e1 = pool.acquire(&mut world).unwrap();
    let e2 = pool.acquire(&mut world).unwrap();
    let e3 = pool.acquire(&mut world).unwrap();

    let active: Vec<_> = pool.active_iter().copied().collect();
    assert_eq!(active.len(), 3);
    assert!(active.contains(&e1));
    assert!(active.contains(&e2));
    assert!(active.contains(&e3));
  }

  #[test]
  fn contains_and_is_active() {
    let mut world = World::new();
    let mut pool = Pool::new(&mut world, PoolConfig::default().with_capacity(3), || {
      (Transform::default(), TestComponent::default())
    });

    let e1 = pool.acquire(&mut world).unwrap();
    pool.release(&mut world, e1);

    // e1 is in pool but not active
    assert!(pool.contains(e1));
    assert!(!pool.is_active(e1));

    let e2 = pool.acquire(&mut world).unwrap();
    assert!(pool.contains(e2));
    assert!(pool.is_active(e2));

    // Random entity not in pool
    let random = world.spawn((Transform::default(),));
    assert!(!pool.contains(random));
  }
}
