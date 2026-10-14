//! Navigation grid for pathfinding
//!
//! Provides a cell-based walkability map for A* pathfinding.

use crate::core::ecs::{Entity, GlobalTransform, Vec2, Without, World};
use crate::core::physics::{Collider, TileCell, TileColliders, TilePart};
use std::collections::HashSet;

/// Cell walkability state
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum CellState {
  /// Cell is passable with normal cost (1.0)
  #[default]
  Walkable,
  /// Cell is completely blocked
  Blocked,
  /// Cell is passable but has increased cost
  Difficult(f32),
}

impl CellState {
  /// Get the traversal cost for this cell
  pub fn cost(&self) -> f32 {
    match self {
      CellState::Walkable => 1.0,
      CellState::Blocked => f32::INFINITY,
      CellState::Difficult(cost) => *cost,
    }
  }

  /// Check if the cell can be traversed
  pub fn is_walkable(&self) -> bool {
    !matches!(self, CellState::Blocked)
  }
}

/// Grid coordinates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridCoord {
  pub x: i32,
  pub y: i32,
}

impl GridCoord {
  pub fn new(x: i32, y: i32) -> Self {
    GridCoord { x, y }
  }
}

/// A grid-based navigation map for pathfinding
///
/// The grid divides the world into cells and tracks walkability.
/// Similar to `SpatialHash` but optimized for pathfinding queries.
#[derive(Debug)]
pub struct NavigationGrid {
  /// Width of the grid in cells
  width: u32,
  /// Height of the grid in cells
  height: u32,
  /// Size of each cell in world units (pixels)
  cell_size: f32,
  /// World origin offset (grid (0,0) corresponds to this world position)
  origin_x: f32,
  origin_y: f32,
  /// Cell states (row-major order: index = y * width + x)
  cells: Vec<CellState>,
  /// Generation counter for path invalidation
  generation: u64,
  /// Entities currently blocking cells
  blocking_entities: HashSet<Entity>,
}

impl NavigationGrid {
  /// Create a new navigation grid
  ///
  /// # Arguments
  /// * `world_width` - Width of the navigable area in world units
  /// * `world_height` - Height of the navigable area in world units
  /// * `cell_size` - Size of each cell in world units (smaller = more precision, higher memory)
  pub fn new(world_width: f32, world_height: f32, cell_size: f32) -> Self {
    Self::with_origin(world_width, world_height, cell_size, 0.0, 0.0)
  }

  /// Create a navigation grid with a custom origin
  pub fn with_origin(
    world_width: f32,
    world_height: f32,
    cell_size: f32,
    origin_x: f32,
    origin_y: f32,
  ) -> Self {
    let cell_size = cell_size.max(1.0);
    let width = ((world_width / cell_size).ceil() as u32).max(1);
    let height = ((world_height / cell_size).ceil() as u32).max(1);

    NavigationGrid {
      width,
      height,
      cell_size,
      origin_x,
      origin_y,
      cells: vec![CellState::Walkable; (width * height) as usize],
      generation: 0,
      blocking_entities: HashSet::new(),
    }
  }

  /// Get the grid dimensions
  pub fn dimensions(&self) -> (u32, u32) {
    (self.width, self.height)
  }

  /// Get the cell size
  pub fn cell_size(&self) -> f32 {
    self.cell_size
  }

  /// Get the current generation (increments when obstacles change)
  pub fn generation(&self) -> u64 {
    self.generation
  }

  /// Convert world coordinates to grid coordinates
  pub fn world_to_grid(&self, world_x: f32, world_y: f32) -> GridCoord {
    GridCoord {
      x: ((world_x - self.origin_x) / self.cell_size).floor() as i32,
      y: ((world_y - self.origin_y) / self.cell_size).floor() as i32,
    }
  }

  /// Convert grid coordinates to world coordinates (center of cell)
  pub fn grid_to_world(&self, coord: GridCoord) -> (f32, f32) {
    (
      self.origin_x + (coord.x as f32 + 0.5) * self.cell_size,
      self.origin_y + (coord.y as f32 + 0.5) * self.cell_size,
    )
  }

  /// Convert grid coordinates to world coordinates (top-left of cell)
  pub fn grid_to_world_corner(&self, coord: GridCoord) -> (f32, f32) {
    (
      self.origin_x + coord.x as f32 * self.cell_size,
      self.origin_y + coord.y as f32 * self.cell_size,
    )
  }

  /// Check if grid coordinates are within bounds
  pub fn in_bounds(&self, coord: GridCoord) -> bool {
    coord.x >= 0 && coord.y >= 0 && (coord.x as u32) < self.width && (coord.y as u32) < self.height
  }

  /// Get the cell index for grid coordinates (returns None if out of bounds)
  fn cell_index(&self, coord: GridCoord) -> Option<usize> {
    if self.in_bounds(coord) {
      Some((coord.y as u32 * self.width + coord.x as u32) as usize)
    } else {
      None
    }
  }

  /// Get the cell state at grid coordinates
  pub fn get(&self, coord: GridCoord) -> CellState {
    self
      .cell_index(coord)
      .map(|i| self.cells[i])
      .unwrap_or(CellState::Blocked)
  }

  /// Set the cell state at grid coordinates
  pub fn set(&mut self, coord: GridCoord, state: CellState) {
    if let Some(i) = self.cell_index(coord)
      && self.cells[i] != state
    {
      self.cells[i] = state;
      self.generation += 1;
    }
  }

  /// Check if a cell is walkable
  pub fn is_walkable(&self, coord: GridCoord) -> bool {
    self.get(coord).is_walkable()
  }

  /// Get the traversal cost for a cell
  pub fn cost(&self, coord: GridCoord) -> f32 {
    self.get(coord).cost()
  }

  /// Get walkable neighbors of a cell (8-directional)
  pub fn neighbors(&self, coord: GridCoord) -> Vec<(GridCoord, f32)> {
    const DIRECTIONS: [(i32, i32, f32); 8] = [
      (-1, 0, 1.0),                       // Left
      (1, 0, 1.0),                        // Right
      (0, -1, 1.0),                       // Up
      (0, 1, 1.0),                        // Down
      (-1, -1, std::f32::consts::SQRT_2), // Top-left
      (1, -1, std::f32::consts::SQRT_2),  // Top-right
      (-1, 1, std::f32::consts::SQRT_2),  // Bottom-left
      (1, 1, std::f32::consts::SQRT_2),   // Bottom-right
    ];

    let mut result = Vec::with_capacity(8);

    for (dx, dy, base_cost) in DIRECTIONS {
      let neighbor = GridCoord::new(coord.x + dx, coord.y + dy);

      if !self.in_bounds(neighbor) {
        continue;
      }

      let cell_state = self.get(neighbor);
      if !cell_state.is_walkable() {
        continue;
      }

      // For diagonal movement, check that both cardinal neighbors are walkable
      // to prevent corner cutting through walls
      if dx != 0 && dy != 0 {
        let cardinal_x = GridCoord::new(coord.x + dx, coord.y);
        let cardinal_y = GridCoord::new(coord.x, coord.y + dy);
        if !self.is_walkable(cardinal_x) || !self.is_walkable(cardinal_y) {
          continue;
        }
      }

      let cost = base_cost * cell_state.cost();
      result.push((neighbor, cost));
    }

    result
  }

  /// Check line of sight between two grid cells using Bresenham's algorithm
  pub fn line_of_sight(&self, from: GridCoord, to: GridCoord) -> bool {
    let mut x = from.x;
    let mut y = from.y;
    let dx = (to.x - from.x).abs();
    let dy = (to.y - from.y).abs();
    let sx = if from.x < to.x { 1 } else { -1 };
    let sy = if from.y < to.y { 1 } else { -1 };
    let mut err = dx - dy;

    loop {
      // Check current cell (skip start cell)
      if (x != from.x || y != from.y) && !self.is_walkable(GridCoord::new(x, y)) {
        return false;
      }

      if x == to.x && y == to.y {
        return true;
      }

      let e2 = 2 * err;
      if e2 > -dy {
        err -= dy;
        x += sx;
      }
      if e2 < dx {
        err += dx;
        y += sy;
      }
    }
  }

  /// Clear all cells to walkable
  pub fn clear(&mut self) {
    self.cells.fill(CellState::Walkable);
    self.blocking_entities.clear();
    self.generation += 1;
  }

  /// Block the cells under a collider: the collider's world bounds, as
  /// physics places it.
  pub fn block_collider(&mut self, global: &GlobalTransform, collider: &Collider, entity: Entity) {
    let (min, max) = collider.world_bounds(global);
    self.block_rect(min.x, min.y, max.x, max.y);
    self.blocking_entities.insert(entity);
  }

  /// Block cells in a rectangular area
  pub fn block_rect(&mut self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) {
    let min_coord = self.world_to_grid(min_x, min_y);
    let max_coord = self.world_to_grid(max_x, max_y);

    for gy in min_coord.y..=max_coord.y {
      for gx in min_coord.x..=max_coord.x {
        self.set(GridCoord::new(gx, gy), CellState::Blocked);
      }
    }
  }

  /// Block the cells under a tile grid's solid and slope cells, its cell
  /// (0, 0)'s top-left at `origin` (its entity's position). One-way cells
  /// stay walkable.
  pub fn block_tiles(&mut self, tiles: &TileColliders, origin: Vec2) {
    let size = tiles.tile_size;
    if !(size.is_finite() && size.min_element() > 0.0) {
      return;
    }
    // The cells a tile covers, worked out in cell units from the grid's
    // origin in f64: the last cell is the one holding the far edge's inside,
    // so a tile never blocks the neighbours it only touches, at any distance
    let (cols, rows) = self.dimensions();
    let cell = f64::from(self.cell_size);
    let span = |from: f64, to: f64, count: u32| {
      let first = (from / cell).floor().max(0.0) as i64;
      let last = (((to / cell).ceil() - 1.0).min(f64::from(count) - 1.0)) as i64;
      first..=last
    };
    for y in 0..tiles.height() {
      for x in 0..tiles.width() {
        if !matches!(tiles.get(x, y), TileCell::Solid | TileCell::Slope(_)) {
          continue;
        }
        let min_x =
          f64::from(origin.x) + f64::from(x) * f64::from(size.x) - f64::from(self.origin_x);
        let min_y =
          f64::from(origin.y) + f64::from(y) * f64::from(size.y) - f64::from(self.origin_y);
        let xs = span(min_x, min_x + f64::from(size.x), cols);
        for gy in span(min_y, min_y + f64::from(size.y), rows) {
          for gx in xs.clone() {
            self.set(GridCoord::new(gx as i32, gy as i32), CellState::Blocked);
          }
        }
      }
    }
  }

  /// Rebuild the grid from entities with Collider components
  ///
  /// Every solid (non-trigger) collider blocks the cells it covers. Tile
  /// grids block exactly their solid and slope cells: their outline
  /// colliders would block their whole bounding boxes, so those are skipped.
  pub fn rebuild_from_world(&mut self, world: &mut World) {
    self.clear();
    let mut colliders =
      world.query_filtered::<(Entity, &GlobalTransform, &Collider), Without<TilePart>>();
    for (entity, global, collider) in colliders.iter(world) {
      if !collider.is_trigger {
        self.block_collider(global, collider, entity);
      }
    }
    let mut maps = world.query::<(&GlobalTransform, &TileColliders)>();
    let maps: Vec<_> = maps
      .iter(world)
      .map(|(global, tiles)| (tiles.clone(), global.translation()))
      .collect();
    for (tiles, origin) in &maps {
      self.block_tiles(tiles, *origin);
    }
  }
}

/// Builder for creating NavigationGrid with a fluent API
pub struct NavigationBuilder {
  world_width: f32,
  world_height: f32,
  cell_size: f32,
  origin_x: f32,
  origin_y: f32,
}

impl NavigationBuilder {
  /// Create a new builder with world dimensions
  pub fn new(world_width: f32, world_height: f32) -> Self {
    NavigationBuilder {
      world_width,
      world_height,
      cell_size: 16.0, // Default cell size
      origin_x: 0.0,
      origin_y: 0.0,
    }
  }

  /// Set the cell size
  pub fn with_cell_size(mut self, cell_size: f32) -> Self {
    self.cell_size = cell_size;
    self
  }

  /// Set the world origin
  pub fn with_origin(mut self, origin_x: f32, origin_y: f32) -> Self {
    self.origin_x = origin_x;
    self.origin_y = origin_y;
    self
  }

  /// Build an empty NavigationGrid
  pub fn build(self) -> NavigationGrid {
    NavigationGrid::with_origin(
      self.world_width,
      self.world_height,
      self.cell_size,
      self.origin_x,
      self.origin_y,
    )
  }

  /// Build a NavigationGrid populated from the world's colliders
  pub fn build_from_world(self, world: &mut World) -> NavigationGrid {
    let mut grid = self.build();
    grid.rebuild_from_world(world);
    grid
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn block_tiles_blocks_exactly_the_solid_cells() {
    use crate::core::physics::{TileCell, TileColliders};
    let cells = vec![
      TileCell::Solid,
      TileCell::Empty,
      TileCell::OneWay,
      TileCell::Solid,
    ];
    let tiles = TileColliders::from_cells(2, 2, Vec2::splat(10.0), cells);
    let mut grid = NavigationGrid::new(40.0, 40.0, 10.0);
    grid.block_tiles(&tiles, Vec2::new(10.0, 10.0));
    assert!(!grid.is_walkable(GridCoord::new(1, 1)));
    assert!(grid.is_walkable(GridCoord::new(2, 1)));
    assert!(
      grid.is_walkable(GridCoord::new(1, 2)),
      "one-way cells are walkable"
    );
    assert!(!grid.is_walkable(GridCoord::new(2, 2)));
    assert!(grid.is_walkable(GridCoord::new(3, 3)));
  }

  #[test]
  fn rebuilding_from_the_world_keeps_the_inside_of_a_tile_u_open() {
    use crate::core::ecs::Transform;
    use crate::core::physics::{TileCell, TileColliders, TilePart};
    let (s, e) = (TileCell::Solid, TileCell::Empty);
    let mut world = World::new();
    let map = world
      .spawn((
        Transform::IDENTITY,
        GlobalTransform::IDENTITY,
        TileColliders::from_cells(3, 2, Vec2::splat(10.0), vec![s, e, s, s, s, s]),
      ))
      .id();
    // As the engine would have built it: an outline collider child
    world.spawn((
      Transform::IDENTITY,
      GlobalTransform::IDENTITY,
      Collider::polyline([
        Vec2::ZERO,
        Vec2::new(30.0, 0.0),
        Vec2::new(30.0, 20.0),
        Vec2::ZERO,
      ]),
      TilePart,
      crate::core::ecs::ChildOf(map),
    ));
    let mut grid = NavigationGrid::new(30.0, 20.0, 10.0);
    grid.rebuild_from_world(&mut world);
    assert!(grid.is_walkable(GridCoord::new(1, 0)), "the U's inside");
    assert!(!grid.is_walkable(GridCoord::new(0, 0)));
    assert!(!grid.is_walkable(GridCoord::new(1, 1)));
  }

  #[test]
  fn a_far_tile_blocks_exactly_one_cell() {
    use crate::core::physics::{TileCell, TileColliders};
    for base in [32768.0_f32, 65536.0] {
      let tiles = TileColliders::from_cells(1, 1, Vec2::splat(16.0), vec![TileCell::Solid]);
      let mut grid = NavigationGrid::with_origin(96.0, 96.0, 16.0, base - 32.0, -32.0);
      grid.block_tiles(&tiles, Vec2::new(base, 0.0));
      let (w, h) = grid.dimensions();
      let blocked: Vec<_> = (0..h as i32)
        .flat_map(|y| (0..w as i32).map(move |x| GridCoord::new(x, y)))
        .filter(|c| !grid.is_walkable(*c))
        .collect();
      assert_eq!(blocked, [GridCoord::new(2, 2)], "at {base}");
    }
  }

  #[test]
  fn grid_creation() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    assert_eq!(grid.dimensions(), (10, 10));
    assert_eq!(grid.cell_size(), 10.0);
  }

  #[test]
  fn a_collider_blocks_the_cells_around_its_origin() {
    use crate::core::ecs::Transform;
    // An 18x18 box at (50, 50) covers 41..59: cells 4 and 5 on each axis
    let mut world = World::new();
    let transform = Transform::from_xy(50.0, 50.0);
    world.spawn((
      transform,
      GlobalTransform::from_transform(&transform),
      Collider::rect(18.0, 18.0),
    ));
    world.spawn((
      Transform::from_xy(5.0, 5.0),
      GlobalTransform::from_transform(&Transform::from_xy(5.0, 5.0)),
      Collider::rect(4.0, 4.0).as_trigger(),
    ));
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    grid.rebuild_from_world(&mut world);
    for (x, y) in [(4, 4), (5, 5), (4, 5), (5, 4)] {
      assert_eq!(
        grid.get(GridCoord::new(x, y)),
        CellState::Blocked,
        "({x}, {y})"
      );
    }
    for (x, y) in [(3, 3), (6, 6), (6, 4), (0, 0)] {
      assert_eq!(
        grid.get(GridCoord::new(x, y)),
        CellState::Walkable,
        "({x}, {y})"
      );
    }
  }

  #[test]
  fn world_to_grid_conversion() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);

    assert_eq!(grid.world_to_grid(0.0, 0.0), GridCoord::new(0, 0));
    assert_eq!(grid.world_to_grid(5.0, 5.0), GridCoord::new(0, 0));
    assert_eq!(grid.world_to_grid(10.0, 10.0), GridCoord::new(1, 1));
    assert_eq!(grid.world_to_grid(95.0, 95.0), GridCoord::new(9, 9));
  }

  #[test]
  fn grid_to_world_conversion() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);

    let (x, y) = grid.grid_to_world(GridCoord::new(0, 0));
    assert_eq!((x, y), (5.0, 5.0)); // Center of cell

    let (x, y) = grid.grid_to_world_corner(GridCoord::new(0, 0));
    assert_eq!((x, y), (0.0, 0.0)); // Corner of cell
  }

  #[test]
  fn cell_state_defaults() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    assert_eq!(grid.get(GridCoord::new(0, 0)), CellState::Walkable);
    assert!(grid.is_walkable(GridCoord::new(0, 0)));
  }

  #[test]
  fn set_and_get_cell() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    let coord = GridCoord::new(5, 5);

    grid.set(coord, CellState::Blocked);
    assert_eq!(grid.get(coord), CellState::Blocked);
    assert!(!grid.is_walkable(coord));

    grid.set(coord, CellState::Difficult(2.0));
    assert!(grid.is_walkable(coord));
    assert_eq!(grid.cost(coord), 2.0);
  }

  #[test]
  fn out_of_bounds_returns_blocked() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    assert_eq!(grid.get(GridCoord::new(-1, 0)), CellState::Blocked);
    assert_eq!(grid.get(GridCoord::new(100, 0)), CellState::Blocked);
  }

  #[test]
  fn neighbors_cardinal() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    let neighbors = grid.neighbors(GridCoord::new(5, 5));

    // Should have 8 neighbors (all directions)
    assert_eq!(neighbors.len(), 8);

    // Check cardinal neighbors have cost 1.0
    let cardinals: Vec<_> = neighbors
      .iter()
      .filter(|(_, cost)| (*cost - 1.0).abs() < f32::EPSILON)
      .collect();
    assert_eq!(cardinals.len(), 4);
  }

  #[test]
  fn neighbors_diagonal_blocked_by_walls() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    let center = GridCoord::new(5, 5);

    // Block cell to the right
    grid.set(GridCoord::new(6, 5), CellState::Blocked);

    let neighbors = grid.neighbors(center);

    // Should not include top-right or bottom-right diagonals
    let has_top_right = neighbors.iter().any(|(c, _)| *c == GridCoord::new(6, 4));
    let has_bottom_right = neighbors.iter().any(|(c, _)| *c == GridCoord::new(6, 6));

    assert!(!has_top_right);
    assert!(!has_bottom_right);
  }

  #[test]
  fn line_of_sight_clear() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    assert!(grid.line_of_sight(GridCoord::new(0, 0), GridCoord::new(9, 9)));
  }

  #[test]
  fn line_of_sight_blocked() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    grid.set(GridCoord::new(5, 5), CellState::Blocked);
    assert!(!grid.line_of_sight(GridCoord::new(0, 0), GridCoord::new(9, 9)));
  }

  #[test]
  fn block_rect() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    grid.block_rect(20.0, 20.0, 40.0, 40.0);

    assert!(!grid.is_walkable(GridCoord::new(2, 2)));
    assert!(!grid.is_walkable(GridCoord::new(3, 3)));
    assert!(!grid.is_walkable(GridCoord::new(4, 4)));
    assert!(grid.is_walkable(GridCoord::new(5, 5)));
  }

  #[test]
  fn generation_increments_on_change() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    let initial_gen = grid.generation();

    grid.set(GridCoord::new(0, 0), CellState::Blocked);
    assert_eq!(grid.generation(), initial_gen + 1);

    // Setting same value shouldn't increment
    grid.set(GridCoord::new(0, 0), CellState::Blocked);
    assert_eq!(grid.generation(), initial_gen + 1);
  }

  #[test]
  fn builder_api() {
    let grid = NavigationBuilder::new(200.0, 150.0)
      .with_cell_size(25.0)
      .with_origin(-50.0, -25.0)
      .build();

    assert_eq!(grid.dimensions(), (8, 6));
    assert_eq!(grid.cell_size(), 25.0);
  }

  #[test]
  fn cell_state_cost() {
    assert_eq!(CellState::Walkable.cost(), 1.0);
    assert_eq!(CellState::Blocked.cost(), f32::INFINITY);
    assert_eq!(CellState::Difficult(2.5).cost(), 2.5);
  }
}
