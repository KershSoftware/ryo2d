//! Spatial partitioning for efficient collision detection
//!
//! This module provides:
//! - `SpatialHash` - A grid-based spatial partitioning structure
//! - `BroadPhase` - Trait for broad-phase collision detection
//! - `PhysicsWorld` - Combines broad + narrow phase collision detection

use super::collision::{Collider, check_collision};
use crate::core::ecs::{Entity, Transform, World};
use std::collections::{HashMap, HashSet};

/// Cell size for the spatial hash grid (in pixels)
/// Larger values = fewer cells but more entities per cell
/// Smaller values = more cells but fewer entities per cell
const DEFAULT_CELL_SIZE: f32 = 64.0;

/// A spatial hash grid for efficient broad-phase collision detection
///
/// Divides the world into a grid of cells. Each entity is placed into
/// one or more cells based on its bounding box. To find collision
/// candidates, we only check entities in the same or adjacent cells.
#[derive(Debug)]
pub struct SpatialHash {
  cell_size: f32,
  /// Maps cell coordinates to entities in that cell
  cells: HashMap<(i32, i32), Vec<Entity>>,
  /// Tracks which cells each entity is in (for efficient removal)
  entity_cells: HashMap<Entity, Vec<(i32, i32)>>,
}

impl Default for SpatialHash {
  fn default() -> Self {
    Self::new(DEFAULT_CELL_SIZE)
  }
}

impl SpatialHash {
  /// Create a new spatial hash with the given cell size
  pub fn new(cell_size: f32) -> Self {
    SpatialHash {
      cell_size: cell_size.max(1.0), // Prevent divide by zero
      cells: HashMap::new(),
      entity_cells: HashMap::new(),
    }
  }

  /// Clear all entities from the spatial hash
  pub fn clear(&mut self) {
    self.cells.clear();
    self.entity_cells.clear();
  }

  /// Get the cell coordinates for a world position
  fn cell_coords(&self, x: f32, y: f32) -> (i32, i32) {
    (
      (x / self.cell_size).floor() as i32,
      (y / self.cell_size).floor() as i32,
    )
  }

  /// Get all cells that an AABB overlaps
  fn cells_for_aabb(&self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Vec<(i32, i32)> {
    let (min_cx, min_cy) = self.cell_coords(min_x, min_y);
    let (max_cx, max_cy) = self.cell_coords(max_x, max_y);

    let mut cells = Vec::new();
    for cx in min_cx..=max_cx {
      for cy in min_cy..=max_cy {
        cells.push((cx, cy));
      }
    }
    cells
  }

  /// Insert an entity into the spatial hash based on its bounding box
  pub fn insert(&mut self, entity: Entity, min_x: f32, min_y: f32, max_x: f32, max_y: f32) {
    // Remove from old cells if already present
    self.remove(entity);

    // Find all cells this entity overlaps
    let cells = self.cells_for_aabb(min_x, min_y, max_x, max_y);

    // Insert into each cell
    for &cell in &cells {
      self.cells.entry(cell).or_default().push(entity);
    }

    // Track which cells this entity is in
    self.entity_cells.insert(entity, cells);
  }

  /// Insert an entity using its collider and position
  pub fn insert_collider(&mut self, entity: Entity, collider: &Collider, x: f32, y: f32) {
    let aabb = collider.get_aabb(x, y);
    self.insert(entity, aabb.min_x, aabb.min_y, aabb.max_x, aabb.max_y);
  }

  /// Remove an entity from the spatial hash
  pub fn remove(&mut self, entity: Entity) {
    if let Some(cells) = self.entity_cells.remove(&entity) {
      for cell in cells {
        if let Some(entities) = self.cells.get_mut(&cell) {
          entities.retain(|&e| e != entity);
          // Clean up empty cells
          if entities.is_empty() {
            self.cells.remove(&cell);
          }
        }
      }
    }
  }

  /// Query for potential collision candidates for an entity
  /// Returns all entities that share cells with the given bounding box
  pub fn query(&self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Vec<Entity> {
    let cells = self.cells_for_aabb(min_x, min_y, max_x, max_y);
    let mut candidates: HashSet<Entity> = HashSet::new();

    for cell in cells {
      if let Some(entities) = self.cells.get(&cell) {
        for &entity in entities {
          candidates.insert(entity);
        }
      }
    }

    candidates.into_iter().collect()
  }

  /// Query for collision candidates using a collider and position
  pub fn query_collider(&self, collider: &Collider, x: f32, y: f32) -> Vec<Entity> {
    let aabb = collider.get_aabb(x, y);
    self.query(aabb.min_x, aabb.min_y, aabb.max_x, aabb.max_y)
  }

  /// Get all unique pairs of entities that might be colliding
  /// This is the main broad-phase output
  pub fn get_collision_pairs(&self) -> Vec<(Entity, Entity)> {
    let mut pairs: HashSet<(Entity, Entity)> = HashSet::new();

    for entities in self.cells.values() {
      // Check all pairs within this cell
      for i in 0..entities.len() {
        for j in (i + 1)..entities.len() {
          let a = entities[i];
          let b = entities[j];
          // Normalize pair ordering to avoid duplicates
          let pair = if a.id() < b.id() { (a, b) } else { (b, a) };
          pairs.insert(pair);
        }
      }
    }

    pairs.into_iter().collect()
  }

  /// Get the number of entities in the spatial hash
  pub fn len(&self) -> usize {
    self.entity_cells.len()
  }

  /// Check if the spatial hash is empty
  pub fn is_empty(&self) -> bool {
    self.entity_cells.is_empty()
  }

  /// Get the number of non-empty cells
  pub fn cell_count(&self) -> usize {
    self.cells.len()
  }
}

/// Collision pair detected during broad phase
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CollisionPair {
  pub entity_a: Entity,
  pub entity_b: Entity,
}

impl CollisionPair {
  /// Create a normalized collision pair (smaller ID first)
  pub fn new(a: Entity, b: Entity) -> Self {
    if a.id() < b.id() {
      CollisionPair {
        entity_a: a,
        entity_b: b,
      }
    } else {
      CollisionPair {
        entity_a: b,
        entity_b: a,
      }
    }
  }
}

/// Physics world that manages spatial partitioning and collision detection
///
/// Call `update()` each frame to rebuild the spatial hash, then use
/// `get_collisions()` to get all collision pairs with narrow-phase results.
pub struct PhysicsWorld {
  spatial_hash: SpatialHash,
}

impl Default for PhysicsWorld {
  fn default() -> Self {
    Self::new()
  }
}

impl PhysicsWorld {
  /// Create a new physics world with default cell size
  pub fn new() -> Self {
    PhysicsWorld {
      spatial_hash: SpatialHash::default(),
    }
  }

  /// Create a physics world with a custom cell size
  pub fn with_cell_size(cell_size: f32) -> Self {
    PhysicsWorld {
      spatial_hash: SpatialHash::new(cell_size),
    }
  }

  /// Rebuild the spatial hash from the current world state
  /// Call this once per frame before querying collisions
  pub fn update(&mut self, world: &World) {
    self.spatial_hash.clear();

    // Get all entities with Collider - collect to Vec to release borrow
    let entities: Vec<Entity> = world.entities_with::<Collider>();

    for entity in entities {
      // Extract values from refs to avoid holding borrows across iterations
      let transform_data = world.get_component::<Transform>(entity).map(|t| (t.x, t.y));
      let collider_data = world
        .get_component::<Collider>(entity)
        .map(|c| (*c).clone());

      if let (Some((x, y)), Some(collider)) = (transform_data, collider_data) {
        self.spatial_hash.insert_collider(entity, &collider, x, y);
      }
    }
  }

  /// Get all collision pairs using broad + narrow phase
  /// Returns collision details for each pair that actually collides
  pub fn get_collisions(&self, world: &World) -> Vec<CollisionResult> {
    let pairs = self.spatial_hash.get_collision_pairs();
    let mut results = Vec::new();

    for (entity_a, entity_b) in pairs {
      // Extract values to avoid holding multiple RefCell borrows simultaneously
      let a_data = {
        let pos = world
          .get_component::<Transform>(entity_a)
          .map(|t| (t.x, t.y));
        let col = world
          .get_component::<Collider>(entity_a)
          .map(|c| (*c).clone());
        pos.zip(col)
      };
      let b_data = {
        let pos = world
          .get_component::<Transform>(entity_b)
          .map(|t| (t.x, t.y));
        let col = world
          .get_component::<Collider>(entity_b)
          .map(|c| (*c).clone());
        pos.zip(col)
      };

      if let (Some((pos_a, col_a)), Some((pos_b, col_b))) = (a_data, b_data) {
        // Narrow phase: actual collision check
        if let Some((pen_x, pen_y, norm_x, norm_y)) = check_collision(&col_a, pos_a, &col_b, pos_b)
        {
          results.push(CollisionResult {
            entity_a,
            entity_b,
            penetration_x: pen_x,
            penetration_y: pen_y,
            normal_x: norm_x,
            normal_y: norm_y,
          });
        }
      }
    }

    results
  }

  /// Query for entities near a point
  pub fn query_point(&self, x: f32, y: f32) -> Vec<Entity> {
    self.spatial_hash.query(x, y, x, y)
  }

  /// Query for entities within a rectangle
  pub fn query_rect(&self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Vec<Entity> {
    self.spatial_hash.query(min_x, min_y, max_x, max_y)
  }

  /// Get the underlying spatial hash for advanced queries
  pub fn spatial_hash(&self) -> &SpatialHash {
    &self.spatial_hash
  }
}

/// Result of a collision between two entities
#[derive(Debug, Clone, Copy)]
pub struct CollisionResult {
  pub entity_a: Entity,
  pub entity_b: Entity,
  pub penetration_x: f32,
  pub penetration_y: f32,
  pub normal_x: f32,
  pub normal_y: f32,
}

impl CollisionResult {
  /// Get the minimum translation vector to separate entity_a from entity_b
  pub fn mtv(&self) -> (f32, f32) {
    if self.penetration_x.abs() < self.penetration_y.abs() {
      (-self.normal_x * self.penetration_x, 0.0)
    } else {
      (0.0, -self.normal_y * self.penetration_y)
    }
  }

  /// Check if a specific entity is involved in this collision
  pub fn involves(&self, entity: Entity) -> bool {
    self.entity_a == entity || self.entity_b == entity
  }

  /// Get the other entity in a collision (given one of the participants)
  pub fn other(&self, entity: Entity) -> Option<Entity> {
    if self.entity_a == entity {
      Some(self.entity_b)
    } else if self.entity_b == entity {
      Some(self.entity_a)
    } else {
      None
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::World;

  // Helper to create entities for testing spatial hash directly
  fn test_entity(id: u32) -> Entity {
    // Create a world just to get valid entities
    // This is a workaround since Entity::from_raw is pub(crate)
    let mut world = World::new();
    for _ in 0..id {
      world.spawn(());
    }
    world.spawn(())
  }

  #[test]
  fn spatial_hash_insert_and_query() {
    let mut hash = SpatialHash::new(64.0);
    let entity = test_entity(0);

    hash.insert(entity, 0.0, 0.0, 32.0, 32.0);

    let candidates = hash.query(16.0, 16.0, 48.0, 48.0);
    assert!(candidates.contains(&entity));
  }

  #[test]
  fn spatial_hash_remove() {
    let mut hash = SpatialHash::new(64.0);
    let entity = test_entity(0);

    hash.insert(entity, 0.0, 0.0, 32.0, 32.0);
    assert_eq!(hash.len(), 1);

    hash.remove(entity);
    assert_eq!(hash.len(), 0);
  }

  #[test]
  fn spatial_hash_collision_pairs() {
    let mut hash = SpatialHash::new(64.0);
    let mut world = World::new();
    let e1 = world.spawn(());
    let e2 = world.spawn(());
    let e3 = world.spawn(());

    // e1 and e2 are in the same cell
    hash.insert(e1, 0.0, 0.0, 32.0, 32.0);
    hash.insert(e2, 16.0, 16.0, 48.0, 48.0);
    // e3 is far away
    hash.insert(e3, 500.0, 500.0, 532.0, 532.0);

    let pairs = hash.get_collision_pairs();
    assert_eq!(pairs.len(), 1);

    let pair = pairs[0];
    assert!(pair == (e1, e2) || pair == (e2, e1));
  }

  #[test]
  fn spatial_hash_entity_spanning_cells() {
    let mut hash = SpatialHash::new(64.0);
    let entity = test_entity(0);

    // Entity spans 4 cells
    hash.insert(entity, 60.0, 60.0, 68.0, 68.0);

    // Should be found in queries to all 4 cells
    assert!(!hash.query(0.0, 0.0, 10.0, 10.0).is_empty());
    assert!(!hash.query(65.0, 0.0, 70.0, 10.0).is_empty());
    assert!(!hash.query(0.0, 65.0, 10.0, 70.0).is_empty());
    assert!(!hash.query(65.0, 65.0, 70.0, 70.0).is_empty());
  }

  #[test]
  fn physics_world_update() {
    let mut world = World::new();
    let entity = world.spawn((
      Transform::new(100.0, 100.0),
      Collider::box_collider(32.0, 32.0),
    ));

    let mut physics = PhysicsWorld::new();
    physics.update(&world);

    assert_eq!(physics.spatial_hash().len(), 1);
    // Use entity to avoid unused warning
    let _ = entity;
  }

  #[test]
  fn physics_world_detects_collision() {
    let mut world = World::new();

    let e1 = world.spawn((
      Transform::new(100.0, 100.0),
      Collider::box_collider(32.0, 32.0),
    ));

    let e2 = world.spawn((
      Transform::new(110.0, 110.0), // Overlapping
      Collider::box_collider(32.0, 32.0),
    ));

    let mut physics = PhysicsWorld::new();
    physics.update(&world);

    let collisions = physics.get_collisions(&world);
    assert_eq!(collisions.len(), 1);
    assert!(collisions[0].involves(e1));
    assert!(collisions[0].involves(e2));
  }

  #[test]
  fn collision_result_mtv() {
    let mut world = World::new();
    let e1 = world.spawn(());
    let e2 = world.spawn(());

    let result = CollisionResult {
      entity_a: e1,
      entity_b: e2,
      penetration_x: 5.0,
      penetration_y: 10.0,
      normal_x: 1.0,
      normal_y: 1.0,
    };

    // Should push along X (smaller penetration)
    let (mtv_x, mtv_y) = result.mtv();
    assert_eq!(mtv_x, -5.0);
    assert_eq!(mtv_y, 0.0);
  }

  #[test]
  fn collision_result_other() {
    let mut world = World::new();
    let e1 = world.spawn(());
    let e2 = world.spawn(());
    let e3 = world.spawn(());

    let result = CollisionResult {
      entity_a: e1,
      entity_b: e2,
      penetration_x: 5.0,
      penetration_y: 5.0,
      normal_x: 1.0,
      normal_y: 0.0,
    };

    assert_eq!(result.other(e1), Some(e2));
    assert_eq!(result.other(e2), Some(e1));
    assert_eq!(result.other(e3), None);
  }

  #[test]
  fn spatial_hash_clear() {
    use crate::core::ecs::World;
    let mut world = World::new();
    let e1 = world.spawn(());
    let e2 = world.spawn(());

    let mut spatial = SpatialHash::new(64.0);
    spatial.insert(e1, 0.0, 0.0, 10.0, 10.0);
    spatial.insert(e2, 50.0, 50.0, 60.0, 60.0);

    spatial.clear();
    assert_eq!(spatial.query(5.0, 5.0, 10.0, 10.0).len(), 0);
    assert_eq!(spatial.query(55.0, 55.0, 10.0, 10.0).len(), 0);
  }

  #[test]
  fn spatial_hash_query_with_bounds() {
    use crate::core::ecs::World;
    let mut world = World::new();
    let e1 = world.spawn(());
    let e2 = world.spawn(());
    let e3 = world.spawn(());

    let mut spatial = SpatialHash::new(64.0);
    spatial.insert(e1, 0.0, 0.0, 10.0, 10.0);
    spatial.insert(e2, 100.0, 100.0, 110.0, 110.0);
    spatial.insert(e3, 5.0, 5.0, 15.0, 15.0);

    // Query near origin should find entities 1 and 3
    let results = spatial.query(0.0, 0.0, 20.0, 20.0);
    assert_eq!(results.len(), 2);
    assert!(results.contains(&e1));
    assert!(results.contains(&e3));

    // Query at 100,100 should only find entity 2
    let results = spatial.query(100.0, 100.0, 120.0, 120.0);
    assert_eq!(results.len(), 1);
    assert!(results.contains(&e2));
  }

  #[test]
  fn spatial_hash_insert_collider() {
    use super::Collider;
    use crate::core::ecs::World;
    let mut world = World::new();
    let entity = world.spawn(());

    let mut spatial = SpatialHash::new(64.0);
    let collider = Collider::box_collider(20.0, 20.0);

    spatial.insert_collider(entity, &collider, 10.0, 10.0);
    let results = spatial.query(15.0, 15.0, 10.0, 10.0);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], entity);
  }

  #[test]
  fn spatial_hash_default_cell_size() {
    let spatial = SpatialHash::default();
    // Just verify it constructs with default cell size
    assert!(spatial.cell_size > 0.0);
  }

  #[test]
  fn spatial_hash_prevents_zero_cell_size() {
    let spatial = SpatialHash::new(0.0);
    // Should clamp to minimum of 1.0
    assert_eq!(spatial.cell_size, 1.0);

    let spatial2 = SpatialHash::new(-5.0);
    assert_eq!(spatial2.cell_size, 1.0);
  }
}
