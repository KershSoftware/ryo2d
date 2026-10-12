//! Generation grid for procedural content
//!
//! Provides the core data structure for storing and manipulating generated maps.

use super::config::{GridRect, TileType};
use ryo2d::core::navigation::NavigationGrid;

/// Type of region generated
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionType {
  /// A room (BSP, cellular cave, etc.)
  Room,
  /// A corridor connecting rooms
  Corridor,
  /// A spawn point or special location
  SpawnPoint,
  /// Custom region type
  Custom(u32),
}

/// Metadata attached to a region
#[derive(Debug, Clone, Default)]
pub struct RegionMetadata {
  /// Custom tags for game logic
  pub tags: Vec<String>,
  /// Arbitrary key-value data
  pub data: std::collections::HashMap<String, String>,
}

impl RegionMetadata {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
    self.tags.push(tag.into());
    self
  }

  pub fn with_data(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
    self.data.insert(key.into(), value.into());
    self
  }
}

/// A region (room, cave, etc.) in the generated map
#[derive(Debug, Clone)]
pub struct Region {
  /// Unique identifier for this region
  pub id: u32,
  /// Bounding rectangle
  pub bounds: GridRect,
  /// Center point
  pub center: (i32, i32),
  /// Type of region
  pub region_type: RegionType,
  /// Additional metadata
  pub metadata: RegionMetadata,
}

impl Region {
  pub fn new(id: u32, bounds: GridRect, region_type: RegionType) -> Self {
    Region {
      id,
      center: bounds.center(),
      bounds,
      region_type,
      metadata: RegionMetadata::new(),
    }
  }

  pub fn with_center(mut self, x: i32, y: i32) -> Self {
    self.center = (x, y);
    self
  }

  pub fn with_metadata(mut self, metadata: RegionMetadata) -> Self {
    self.metadata = metadata;
    self
  }

  /// Calculate distance to another region (center-to-center)
  pub fn distance_to(&self, other: &Region) -> f32 {
    let dx = (self.center.0 - other.center.0) as f32;
    let dy = (self.center.1 - other.center.1) as f32;
    (dx * dx + dy * dy).sqrt()
  }
}

/// A connection between two regions
#[derive(Debug, Clone)]
pub struct Connection {
  /// ID of the first region
  pub region_a: u32,
  /// ID of the second region
  pub region_b: u32,
  /// Path of tiles connecting the regions (if computed)
  pub path: Option<Vec<(i32, i32)>>,
  /// Length of the connection path
  pub length: f32,
}

impl Connection {
  pub fn new(region_a: u32, region_b: u32, length: f32) -> Self {
    Connection {
      region_a,
      region_b,
      path: None,
      length,
    }
  }

  pub fn with_path(mut self, path: Vec<(i32, i32)>) -> Self {
    self.path = Some(path);
    self
  }
}

/// A grid for procedural generation
///
/// Stores tile types and tracks regions/connections for dungeon-style maps.
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// let mut grid = GenerationGrid::new(64, 64);
///
/// // Set tiles
/// grid.set(10, 10, TileType::Floor);
/// grid.fill_rect(5, 5, 10, 10, TileType::Floor);
///
/// // Convert to navigation grid for pathfinding
/// let nav_grid = grid.to_navigation_grid(16.0);
/// ```
#[derive(Debug, Clone)]
pub struct GenerationGrid {
  width: u32,
  height: u32,
  tiles: Vec<TileType>,
  regions: Vec<Region>,
  connections: Vec<Connection>,
  next_region_id: u32,
}

impl GenerationGrid {
  /// Create a new generation grid filled with Empty tiles
  pub fn new(width: u32, height: u32) -> Self {
    GenerationGrid {
      width,
      height,
      tiles: vec![TileType::Empty; (width * height) as usize],
      regions: Vec::new(),
      connections: Vec::new(),
      next_region_id: 0,
    }
  }

  /// Create a grid filled with a specific tile type
  pub fn filled(width: u32, height: u32, tile: TileType) -> Self {
    GenerationGrid {
      width,
      height,
      tiles: vec![tile; (width * height) as usize],
      regions: Vec::new(),
      connections: Vec::new(),
      next_region_id: 0,
    }
  }

  /// Get the grid dimensions
  pub fn dimensions(&self) -> (u32, u32) {
    (self.width, self.height)
  }

  /// Get the width
  pub fn width(&self) -> u32 {
    self.width
  }

  /// Get the height
  pub fn height(&self) -> u32 {
    self.height
  }

  /// Check if coordinates are within bounds
  pub fn in_bounds(&self, x: i32, y: i32) -> bool {
    x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height
  }

  /// Get the tile index for coordinates
  fn tile_index(&self, x: i32, y: i32) -> Option<usize> {
    if self.in_bounds(x, y) {
      Some((y as u32 * self.width + x as u32) as usize)
    } else {
      None
    }
  }

  /// Get the tile at coordinates
  pub fn get(&self, x: i32, y: i32) -> TileType {
    self
      .tile_index(x, y)
      .map(|i| self.tiles[i])
      .unwrap_or(TileType::Empty)
  }

  /// Set the tile at coordinates
  pub fn set(&mut self, x: i32, y: i32, tile: TileType) {
    if let Some(i) = self.tile_index(x, y) {
      self.tiles[i] = tile;
    }
  }

  /// Fill a rectangle with a tile type
  pub fn fill_rect(&mut self, x: i32, y: i32, width: u32, height: u32, tile: TileType) {
    for dy in 0..height as i32 {
      for dx in 0..width as i32 {
        self.set(x + dx, y + dy, tile);
      }
    }
  }

  /// Fill a GridRect with a tile type
  pub fn fill_grid_rect(&mut self, rect: &GridRect, tile: TileType) {
    self.fill_rect(rect.x, rect.y, rect.width, rect.height, tile);
  }

  /// Draw a horizontal line
  pub fn line_horizontal(&mut self, x1: i32, x2: i32, y: i32, tile: TileType) {
    let (start, end) = if x1 <= x2 { (x1, x2) } else { (x2, x1) };
    for x in start..=end {
      self.set(x, y, tile);
    }
  }

  /// Draw a vertical line
  pub fn line_vertical(&mut self, x: i32, y1: i32, y2: i32, tile: TileType) {
    let (start, end) = if y1 <= y2 { (y1, y2) } else { (y2, y1) };
    for y in start..=end {
      self.set(x, y, tile);
    }
  }

  /// Draw an L-shaped corridor between two points
  pub fn corridor_l(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, tile: TileType) {
    // Horizontal first, then vertical
    self.line_horizontal(x1, x2, y1, tile);
    self.line_vertical(x2, y1, y2, tile);
  }

  /// Clear the grid to a specific tile type
  pub fn clear(&mut self, tile: TileType) {
    self.tiles.fill(tile);
    self.regions.clear();
    self.connections.clear();
    self.next_region_id = 0;
  }

  /// Add a region to the grid
  pub fn add_region(&mut self, bounds: GridRect, region_type: RegionType) -> u32 {
    let id = self.next_region_id;
    self.next_region_id += 1;
    self.regions.push(Region::new(id, bounds, region_type));
    id
  }

  /// Add a region with custom center
  pub fn add_region_with_center(
    &mut self,
    bounds: GridRect,
    center: (i32, i32),
    region_type: RegionType,
  ) -> u32 {
    let id = self.next_region_id;
    self.next_region_id += 1;
    self
      .regions
      .push(Region::new(id, bounds, region_type).with_center(center.0, center.1));
    id
  }

  /// Get all regions
  pub fn regions(&self) -> &[Region] {
    &self.regions
  }

  /// Get a mutable reference to regions
  pub fn regions_mut(&mut self) -> &mut Vec<Region> {
    &mut self.regions
  }

  /// Get a region by ID
  pub fn get_region(&self, id: u32) -> Option<&Region> {
    self.regions.iter().find(|r| r.id == id)
  }

  /// Add a connection between two regions
  pub fn add_connection(&mut self, region_a: u32, region_b: u32, length: f32) {
    self
      .connections
      .push(Connection::new(region_a, region_b, length));
  }

  /// Add a connection with a path
  pub fn add_connection_with_path(&mut self, region_a: u32, region_b: u32, path: Vec<(i32, i32)>) {
    let length = path.len() as f32;
    self
      .connections
      .push(Connection::new(region_a, region_b, length).with_path(path));
  }

  /// Get all connections
  pub fn connections(&self) -> &[Connection] {
    &self.connections
  }

  /// Iterate over all tiles with their coordinates
  pub fn iter(&self) -> impl Iterator<Item = (i32, i32, TileType)> + '_ {
    (0..self.height).flat_map(move |y| {
      (0..self.width).map(move |x| (x as i32, y as i32, self.get(x as i32, y as i32)))
    })
  }

  /// Iterate over tiles of a specific type
  pub fn iter_type(&self, tile_type: TileType) -> impl Iterator<Item = (i32, i32)> + '_ {
    self
      .iter()
      .filter(move |(_, _, t)| *t == tile_type)
      .map(|(x, y, _)| (x, y))
  }

  /// Count tiles of a specific type
  pub fn count_type(&self, tile_type: TileType) -> usize {
    self.tiles.iter().filter(|t| **t == tile_type).count()
  }

  /// Get the 4-directional neighbors of a cell
  pub fn neighbors_4(&self, x: i32, y: i32) -> impl Iterator<Item = (i32, i32, TileType)> + '_ {
    const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
    DIRS.into_iter().filter_map(move |(dx, dy)| {
      let nx = x + dx;
      let ny = y + dy;
      if self.in_bounds(nx, ny) {
        Some((nx, ny, self.get(nx, ny)))
      } else {
        None
      }
    })
  }

  /// Get the 8-directional neighbors of a cell
  pub fn neighbors_8(&self, x: i32, y: i32) -> impl Iterator<Item = (i32, i32, TileType)> + '_ {
    const DIRS: [(i32, i32); 8] = [
      (0, -1),
      (1, -1),
      (1, 0),
      (1, 1),
      (0, 1),
      (-1, 1),
      (-1, 0),
      (-1, -1),
    ];
    DIRS.into_iter().filter_map(move |(dx, dy)| {
      let nx = x + dx;
      let ny = y + dy;
      if self.in_bounds(nx, ny) {
        Some((nx, ny, self.get(nx, ny)))
      } else {
        None
      }
    })
  }

  /// Count neighbors of a specific type (8-directional)
  pub fn count_neighbors(&self, x: i32, y: i32, tile_type: TileType) -> usize {
    self
      .neighbors_8(x, y)
      .filter(|(_, _, t)| *t == tile_type)
      .count()
  }

  /// Convert to a NavigationGrid for pathfinding
  ///
  /// # Arguments
  /// * `cell_size` - Size of each cell in world units
  pub fn to_navigation_grid(&self, cell_size: f32) -> NavigationGrid {
    let world_width = self.width as f32 * cell_size;
    let world_height = self.height as f32 * cell_size;
    let mut nav_grid = NavigationGrid::new(world_width, world_height, cell_size);

    for (x, y, tile) in self.iter() {
      let coord = nav_grid.world_to_grid(x as f32 * cell_size, y as f32 * cell_size);
      nav_grid.set(coord, tile.to_cell_state());
    }

    nav_grid
  }

  /// Apply a custom transformation to the grid
  pub fn transform<F>(&mut self, f: F)
  where
    F: Fn(i32, i32, TileType) -> TileType,
  {
    for y in 0..self.height as i32 {
      for x in 0..self.width as i32 {
        let tile = self.get(x, y);
        let new_tile = f(x, y, tile);
        self.set(x, y, new_tile);
      }
    }
  }
}

/// Result of a generation operation
#[derive(Debug, Clone)]
pub struct GenerationResult {
  /// The generated grid
  pub grid: GenerationGrid,
  /// Seed used for generation
  pub seed: u64,
  /// Algorithm that was used
  pub algorithm: String,
  /// Generation time in milliseconds
  pub duration_ms: u64,
  /// Outcome of the last WFC step, or `None` if the pipeline had no WFC step.
  ///
  /// On `Contradiction` or `IterationLimit` the grid is only partly collapsed.
  pub wfc_result: Option<super::wfc::WfcResult>,
}

impl GenerationResult {
  pub fn new(grid: GenerationGrid, seed: u64, algorithm: impl Into<String>) -> Self {
    GenerationResult {
      grid,
      seed,
      algorithm: algorithm.into(),
      duration_ms: 0,
      wfc_result: None,
    }
  }

  pub fn with_duration(mut self, duration_ms: u64) -> Self {
    self.duration_ms = duration_ms;
    self
  }

  /// Outcome of the last WFC step, or `None` if the pipeline had no WFC step.
  pub fn wfc_result(&self) -> Option<super::wfc::WfcResult> {
    self.wfc_result
  }

  /// Get a reference to the grid
  pub fn grid(&self) -> &GenerationGrid {
    &self.grid
  }

  /// Take ownership of the grid
  pub fn into_grid(self) -> GenerationGrid {
    self.grid
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn grid_creation() {
    let grid = GenerationGrid::new(10, 20);
    assert_eq!(grid.dimensions(), (10, 20));
    assert_eq!(grid.get(0, 0), TileType::Empty);
  }

  #[test]
  fn grid_filled() {
    let grid = GenerationGrid::filled(5, 5, TileType::Wall);
    assert_eq!(grid.get(2, 2), TileType::Wall);
  }

  #[test]
  fn set_and_get() {
    let mut grid = GenerationGrid::new(10, 10);
    grid.set(5, 5, TileType::Floor);
    assert_eq!(grid.get(5, 5), TileType::Floor);
    assert_eq!(grid.get(0, 0), TileType::Empty);
  }

  #[test]
  fn out_of_bounds() {
    let grid = GenerationGrid::new(10, 10);
    assert_eq!(grid.get(-1, 0), TileType::Empty);
    assert_eq!(grid.get(100, 0), TileType::Empty);
  }

  #[test]
  fn fill_rect() {
    let mut grid = GenerationGrid::new(10, 10);
    grid.fill_rect(2, 2, 3, 3, TileType::Floor);

    assert_eq!(grid.get(2, 2), TileType::Floor);
    assert_eq!(grid.get(4, 4), TileType::Floor);
    assert_eq!(grid.get(1, 1), TileType::Empty);
    assert_eq!(grid.get(5, 5), TileType::Empty);
  }

  #[test]
  fn count_neighbors() {
    let mut grid = GenerationGrid::new(5, 5);
    grid.set(1, 1, TileType::Wall);
    grid.set(2, 1, TileType::Wall);
    grid.set(3, 1, TileType::Wall);

    assert_eq!(grid.count_neighbors(2, 2, TileType::Wall), 3);
  }

  #[test]
  fn region_management() {
    let mut grid = GenerationGrid::new(64, 64);
    let id1 = grid.add_region(GridRect::new(10, 10, 10, 10), RegionType::Room);
    let id2 = grid.add_region(GridRect::new(30, 30, 10, 10), RegionType::Room);

    assert_eq!(grid.regions().len(), 2);
    assert_eq!(id1, 0);
    assert_eq!(id2, 1);

    let region = grid.get_region(id1).unwrap();
    assert_eq!(region.bounds.x, 10);
  }

  #[test]
  fn connection_management() {
    let mut grid = GenerationGrid::new(64, 64);
    let id1 = grid.add_region(GridRect::new(0, 0, 10, 10), RegionType::Room);
    let id2 = grid.add_region(GridRect::new(20, 20, 10, 10), RegionType::Room);

    grid.add_connection(id1, id2, 28.28);
    assert_eq!(grid.connections().len(), 1);
    assert_eq!(grid.connections()[0].region_a, id1);
    assert_eq!(grid.connections()[0].region_b, id2);
  }

  #[test]
  fn iter_type() {
    let mut grid = GenerationGrid::new(10, 10);
    grid.set(1, 1, TileType::Floor);
    grid.set(2, 2, TileType::Floor);
    grid.set(3, 3, TileType::Wall);

    let floors: Vec<_> = grid.iter_type(TileType::Floor).collect();
    assert_eq!(floors.len(), 2);
    assert!(floors.contains(&(1, 1)));
    assert!(floors.contains(&(2, 2)));
  }

  #[test]
  fn region_distance() {
    let r1 = Region::new(0, GridRect::new(0, 0, 10, 10), RegionType::Room);
    let r2 = Region::new(1, GridRect::new(30, 40, 10, 10), RegionType::Room);

    // Centers: (5, 5) and (35, 45)
    // Distance: sqrt((30)^2 + (40)^2) = 50
    let dist = r1.distance_to(&r2);
    assert!((dist - 50.0).abs() < 0.01);
  }
}
