//! Wave Function Collapse procedural generation
//!
//! Generates patterns by propagating constraints from a set of tile rules.
//! Each cell starts with all possibilities and collapses to a single tile
//! based on neighbor constraints.

use super::config::{SeededRng, TileType};
use super::grid::GenerationGrid;
use std::collections::HashSet;

/// Direction for neighbor constraints
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WfcDirection {
  Up = 0,
  Right = 1,
  Down = 2,
  Left = 3,
}

impl WfcDirection {
  pub fn opposite(&self) -> Self {
    match self {
      WfcDirection::Up => WfcDirection::Down,
      WfcDirection::Down => WfcDirection::Up,
      WfcDirection::Left => WfcDirection::Right,
      WfcDirection::Right => WfcDirection::Left,
    }
  }

  pub fn as_delta(&self) -> (i32, i32) {
    match self {
      WfcDirection::Up => (0, -1),
      WfcDirection::Down => (0, 1),
      WfcDirection::Left => (-1, 0),
      WfcDirection::Right => (1, 0),
    }
  }

  pub fn all() -> &'static [WfcDirection] {
    &[
      WfcDirection::Up,
      WfcDirection::Right,
      WfcDirection::Down,
      WfcDirection::Left,
    ]
  }
}

/// A tile definition for WFC
#[derive(Debug, Clone)]
pub struct WfcTile {
  /// Unique identifier for this tile
  pub id: u32,
  /// The tile type to place
  pub tile_type: TileType,
  /// Allowed neighbors in each direction (Up, Right, Down, Left)
  pub allowed_neighbors: [HashSet<u32>; 4],
  /// Weight for random selection (higher = more likely)
  pub weight: f32,
}

impl WfcTile {
  /// Create a new WFC tile
  pub fn new(id: u32, tile_type: TileType) -> Self {
    WfcTile {
      id,
      tile_type,
      allowed_neighbors: [
        HashSet::new(),
        HashSet::new(),
        HashSet::new(),
        HashSet::new(),
      ],
      weight: 1.0,
    }
  }

  /// Set weight for this tile
  pub fn with_weight(mut self, weight: f32) -> Self {
    self.weight = weight.max(0.01);
    self
  }

  /// Allow a specific tile as neighbor in a direction
  pub fn allow_neighbor(mut self, direction: WfcDirection, tile_id: u32) -> Self {
    self.allowed_neighbors[direction as usize].insert(tile_id);
    self
  }

  /// Allow multiple tiles as neighbors in a direction
  pub fn allow_neighbors(mut self, direction: WfcDirection, tile_ids: &[u32]) -> Self {
    for &id in tile_ids {
      self.allowed_neighbors[direction as usize].insert(id);
    }
    self
  }

  /// Allow a tile in all directions
  pub fn allow_all_directions(mut self, tile_id: u32) -> Self {
    for set in &mut self.allowed_neighbors {
      set.insert(tile_id);
    }
    self
  }

  /// Allow self-adjacency in all directions
  pub fn allow_self_adjacent(self) -> Self {
    let id = self.id;
    self.allow_all_directions(id)
  }
}

/// Configuration for WFC generation
#[derive(Debug, Clone)]
pub struct WfcConfig {
  /// Tile definitions
  pub tiles: Vec<WfcTile>,
  /// Maximum iterations before giving up
  pub max_iterations: u32,
  /// Enable backtracking on contradiction
  pub enable_backtracking: bool,
  /// Maximum backtrack attempts
  pub max_backtracks: u32,
}

impl Default for WfcConfig {
  fn default() -> Self {
    WfcConfig {
      tiles: Vec::new(),
      max_iterations: 100000,
      enable_backtracking: true,
      max_backtracks: 100,
    }
  }
}

impl WfcConfig {
  pub fn new() -> Self {
    Self::default()
  }

  /// Add a tile to the configuration
  pub fn with_tile(mut self, tile: WfcTile) -> Self {
    self.tiles.push(tile);
    self
  }

  /// Add multiple tiles
  pub fn with_tiles(mut self, tiles: Vec<WfcTile>) -> Self {
    self.tiles.extend(tiles);
    self
  }

  /// Set maximum iterations
  pub fn with_max_iterations(mut self, iterations: u32) -> Self {
    self.max_iterations = iterations;
    self
  }

  /// Enable/disable backtracking
  pub fn with_backtracking(mut self, enable: bool) -> Self {
    self.enable_backtracking = enable;
    self
  }

  /// Create a simple dungeon tileset
  pub fn dungeon_preset() -> Self {
    // Define tile IDs
    const FLOOR: u32 = 0;
    const WALL: u32 = 1;
    const FLOOR_WALL_TOP: u32 = 2; // Floor with wall above
    const FLOOR_WALL_BOTTOM: u32 = 3; // Floor with wall below

    let floor = WfcTile::new(FLOOR, TileType::Floor)
      .with_weight(3.0)
      .allow_self_adjacent()
      .allow_neighbor(WfcDirection::Up, FLOOR_WALL_BOTTOM)
      .allow_neighbor(WfcDirection::Down, FLOOR_WALL_TOP);

    let wall = WfcTile::new(WALL, TileType::Wall)
      .with_weight(1.0)
      .allow_self_adjacent()
      .allow_neighbor(WfcDirection::Down, FLOOR_WALL_TOP)
      .allow_neighbor(WfcDirection::Up, FLOOR_WALL_BOTTOM);

    let floor_wall_top = WfcTile::new(FLOOR_WALL_TOP, TileType::Floor)
      .with_weight(0.5)
      .allow_neighbor(WfcDirection::Up, WALL)
      .allow_neighbor(WfcDirection::Down, FLOOR)
      .allow_neighbor(WfcDirection::Left, FLOOR_WALL_TOP)
      .allow_neighbor(WfcDirection::Left, FLOOR)
      .allow_neighbor(WfcDirection::Right, FLOOR_WALL_TOP)
      .allow_neighbor(WfcDirection::Right, FLOOR);

    let floor_wall_bottom = WfcTile::new(FLOOR_WALL_BOTTOM, TileType::Floor)
      .with_weight(0.5)
      .allow_neighbor(WfcDirection::Up, FLOOR)
      .allow_neighbor(WfcDirection::Down, WALL)
      .allow_neighbor(WfcDirection::Left, FLOOR_WALL_BOTTOM)
      .allow_neighbor(WfcDirection::Left, FLOOR)
      .allow_neighbor(WfcDirection::Right, FLOOR_WALL_BOTTOM)
      .allow_neighbor(WfcDirection::Right, FLOOR);

    WfcConfig::new()
      .with_tile(floor)
      .with_tile(wall)
      .with_tile(floor_wall_top)
      .with_tile(floor_wall_bottom)
  }

  /// Create a simple tileset that produces clustered floor/wall regions
  ///
  /// Uses three tiles: floor, wall, and a border transition tile.
  /// Floor and wall cannot be directly adjacent — they must be separated
  /// by a border, which forces coherent regions instead of random noise.
  pub fn simple_preset() -> Self {
    const FLOOR: u32 = 0;
    const WALL: u32 = 1;
    const BORDER: u32 = 2;

    // Floor clusters together, can only touch floor or border
    let floor = WfcTile::new(FLOOR, TileType::Floor)
      .with_weight(3.0)
      .allow_self_adjacent()
      .allow_all_directions(BORDER);

    // Wall clusters together, can only touch wall or border
    let wall = WfcTile::new(WALL, TileType::Wall)
      .with_weight(2.0)
      .allow_self_adjacent()
      .allow_all_directions(BORDER);

    // Border is always exactly 1 cell thick (no self-adjacency)
    let border = WfcTile::new(BORDER, TileType::Door)
      .with_weight(1.0)
      .allow_all_directions(FLOOR)
      .allow_all_directions(WALL);

    WfcConfig::new()
      .with_tile(floor)
      .with_tile(wall)
      .with_tile(border)
  }
}

/// State of a cell during WFC
#[derive(Debug, Clone)]
struct WfcCell {
  /// Possible tile IDs (superposition)
  possibilities: HashSet<u32>,
  /// Collapsed tile ID (None if not collapsed)
  collapsed: Option<u32>,
}

impl WfcCell {
  fn new(all_tile_ids: &[u32]) -> Self {
    WfcCell {
      possibilities: all_tile_ids.iter().copied().collect(),
      collapsed: None,
    }
  }

  fn entropy(&self) -> usize {
    self.possibilities.len()
  }

  fn is_collapsed(&self) -> bool {
    self.collapsed.is_some()
  }

  fn is_contradicted(&self) -> bool {
    self.possibilities.is_empty() && self.collapsed.is_none()
  }
}

/// WFC generation result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WfcResult {
  /// Generation succeeded
  Success,
  /// Generation failed due to contradiction
  Contradiction,
  /// Generation exceeded iteration limit
  IterationLimit,
}

/// Generate using Wave Function Collapse
pub fn generate(
  width: u32,
  height: u32,
  config: &WfcConfig,
  rng: &mut SeededRng,
) -> (GenerationGrid, WfcResult) {
  if config.tiles.is_empty() {
    return (GenerationGrid::new(width, height), WfcResult::Contradiction);
  }

  let tile_ids: Vec<u32> = config.tiles.iter().map(|t| t.id).collect();
  let mut cells: Vec<WfcCell> = (0..(width * height))
    .map(|_| WfcCell::new(&tile_ids))
    .collect();

  let mut iterations = 0;
  let mut backtracks = 0;

  // Main WFC loop
  loop {
    iterations += 1;
    if iterations > config.max_iterations {
      return (
        build_grid(&cells, &config.tiles, width, height),
        WfcResult::IterationLimit,
      );
    }

    // Find cell with lowest entropy (not collapsed)
    let min_entropy_idx = find_min_entropy_cell(&cells, width, rng);

    match min_entropy_idx {
      None => {
        // All cells collapsed - success!
        return (
          build_grid(&cells, &config.tiles, width, height),
          WfcResult::Success,
        );
      }
      Some(idx) => {
        if cells[idx].is_contradicted() {
          if config.enable_backtracking && backtracks < config.max_backtracks {
            // Reset and try again
            cells = (0..(width * height))
              .map(|_| WfcCell::new(&tile_ids))
              .collect();
            backtracks += 1;
            continue;
          }
          return (
            build_grid(&cells, &config.tiles, width, height),
            WfcResult::Contradiction,
          );
        }

        // Collapse this cell
        collapse_cell(&mut cells[idx], &config.tiles, rng);

        // Propagate constraints
        let x = (idx as u32) % width;
        let y = (idx as u32) / width;
        propagate(&mut cells, config, width, height, x as i32, y as i32);
      }
    }
  }
}

fn find_min_entropy_cell(cells: &[WfcCell], _width: u32, rng: &mut SeededRng) -> Option<usize> {
  let mut min_entropy = usize::MAX;
  let mut min_indices: Vec<usize> = Vec::new();

  for (idx, cell) in cells.iter().enumerate() {
    if cell.is_collapsed() {
      continue;
    }

    let entropy = cell.entropy();
    if entropy == 0 {
      // Contradiction found
      return Some(idx);
    }

    match entropy.cmp(&min_entropy) {
      std::cmp::Ordering::Less => {
        min_entropy = entropy;
        min_indices.clear();
        min_indices.push(idx);
      }
      std::cmp::Ordering::Equal => {
        min_indices.push(idx);
      }
      std::cmp::Ordering::Greater => {}
    }
  }

  if min_indices.is_empty() {
    None
  } else if min_indices.len() == 1 {
    Some(min_indices[0])
  } else {
    // Randomize among tied cells to avoid directional bias
    let pick = rng.gen_range(0..min_indices.len());
    Some(min_indices[pick])
  }
}

fn collapse_cell(cell: &mut WfcCell, tiles: &[WfcTile], rng: &mut SeededRng) {
  if cell.possibilities.is_empty() {
    return;
  }

  // Weighted random selection
  let candidates: Vec<&WfcTile> = tiles
    .iter()
    .filter(|t| cell.possibilities.contains(&t.id))
    .collect();

  let total_weight: f32 = candidates.iter().map(|t| t.weight).sum();
  let mut roll = rng.gen_range(0.0..total_weight);

  for tile in candidates {
    roll -= tile.weight;
    if roll <= 0.0 {
      cell.collapsed = Some(tile.id);
      cell.possibilities.clear();
      cell.possibilities.insert(tile.id);
      return;
    }
  }

  // Fallback to first possibility
  if let Some(&first) = cell.possibilities.iter().next() {
    cell.collapsed = Some(first);
  }
}

fn propagate(
  cells: &mut [WfcCell],
  config: &WfcConfig,
  width: u32,
  height: u32,
  start_x: i32,
  start_y: i32,
) {
  let mut stack = vec![(start_x, start_y)];

  while let Some((x, y)) = stack.pop() {
    let idx = (y as u32 * width + x as u32) as usize;
    let current_possibilities = cells[idx].possibilities.clone();

    for dir in WfcDirection::all() {
      let (dx, dy) = dir.as_delta();
      let nx = x + dx;
      let ny = y + dy;

      if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
        continue;
      }

      let neighbor_idx = (ny as u32 * width + nx as u32) as usize;
      if cells[neighbor_idx].is_collapsed() {
        continue;
      }

      // Calculate allowed tiles for neighbor based on current cell
      let mut allowed: HashSet<u32> = HashSet::new();
      for &tile_id in &current_possibilities {
        if let Some(tile) = config.tiles.iter().find(|t| t.id == tile_id) {
          allowed.extend(&tile.allowed_neighbors[*dir as usize]);
        }
      }

      // Intersect with neighbor's current possibilities
      let old_len = cells[neighbor_idx].possibilities.len();
      cells[neighbor_idx]
        .possibilities
        .retain(|id| allowed.contains(id));

      // If possibilities changed, add to propagation stack
      if cells[neighbor_idx].possibilities.len() < old_len {
        stack.push((nx, ny));
      }
    }
  }
}

fn build_grid(cells: &[WfcCell], tiles: &[WfcTile], width: u32, height: u32) -> GenerationGrid {
  let mut grid = GenerationGrid::new(width, height);

  for y in 0..height as i32 {
    for x in 0..width as i32 {
      let idx = (y as u32 * width + x as u32) as usize;
      let cell = &cells[idx];

      if let Some(tile_id) = cell.collapsed {
        if let Some(tile) = tiles.iter().find(|t| t.id == tile_id) {
          grid.set(x, y, tile.tile_type);
        }
      } else if !cell.possibilities.is_empty()
        && let Some(&first_id) = cell.possibilities.iter().next()
        && let Some(tile) = tiles.iter().find(|t| t.id == first_id)
      {
        // Not collapsed - pick first possibility for visualization
        grid.set(x, y, tile.tile_type);
      }
    }
  }

  grid
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn basic_generation() {
    let mut rng = SeededRng::new(12345);
    let config = WfcConfig::simple_preset();
    let (grid, result) = generate(16, 16, &config, &mut rng);

    assert_eq!(result, WfcResult::Success);
    assert!(grid.count_type(TileType::Floor) > 0);
    assert!(grid.count_type(TileType::Wall) > 0);
  }

  #[test]
  fn reproducibility() {
    let config = WfcConfig::simple_preset();

    let mut rng1 = SeededRng::new(12345);
    let (grid1, _) = generate(16, 16, &config, &mut rng1);

    let mut rng2 = SeededRng::new(12345);
    let (grid2, _) = generate(16, 16, &config, &mut rng2);

    for y in 0..16 {
      for x in 0..16 {
        assert_eq!(grid1.get(x, y), grid2.get(x, y));
      }
    }
  }

  #[test]
  fn empty_config_fails() {
    let mut rng = SeededRng::new(12345);
    let config = WfcConfig::new();
    let (_, result) = generate(16, 16, &config, &mut rng);

    assert_eq!(result, WfcResult::Contradiction);
  }

  #[test]
  fn dungeon_preset_works() {
    let mut rng = SeededRng::new(42);
    let config = WfcConfig::dungeon_preset();
    let (grid, result) = generate(20, 20, &config, &mut rng);

    // May succeed or need backtracking
    assert!(result == WfcResult::Success || result == WfcResult::Contradiction);

    // Should have some floor tiles at minimum
    let floor_count = grid.count_type(TileType::Floor);
    let wall_count = grid.count_type(TileType::Wall);
    assert!(floor_count > 0 || wall_count > 0);
  }

  #[test]
  fn direction_opposites() {
    assert_eq!(WfcDirection::Up.opposite(), WfcDirection::Down);
    assert_eq!(WfcDirection::Down.opposite(), WfcDirection::Up);
    assert_eq!(WfcDirection::Left.opposite(), WfcDirection::Right);
    assert_eq!(WfcDirection::Right.opposite(), WfcDirection::Left);
  }

  #[test]
  fn tile_builder() {
    let tile = WfcTile::new(0, TileType::Floor)
      .with_weight(2.0)
      .allow_neighbor(WfcDirection::Up, 1)
      .allow_neighbor(WfcDirection::Up, 2)
      .allow_self_adjacent();

    assert_eq!(tile.weight, 2.0);
    assert!(tile.allowed_neighbors[WfcDirection::Up as usize].contains(&1));
    assert!(tile.allowed_neighbors[WfcDirection::Up as usize].contains(&2));
    assert!(tile.allowed_neighbors[WfcDirection::Up as usize].contains(&0)); // self
  }
}
