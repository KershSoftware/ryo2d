//! Cellular automata procedural generation
//!
//! Creates organic cave-like structures using neighbor-counting rules.
//! Similar to Conway's Game of Life but tuned for cave generation.

use super::config::{GridRect, SeededRng, TileType};
use super::grid::{GenerationGrid, RegionType};
use std::collections::{HashSet, VecDeque};

/// Configuration for cellular automata generation
#[derive(Debug, Clone)]
pub struct CellularConfig {
  /// Initial probability of a cell being a wall (0.0 to 1.0)
  pub fill_probability: f32,
  /// Number of simulation iterations
  pub iterations: u32,
  /// Number of wall neighbors required for a cell to become a wall
  pub birth_threshold: u32,
  /// Number of wall neighbors required for a wall to stay a wall
  pub survival_threshold: u32,
  /// Keep only the largest connected region
  pub keep_largest_only: bool,
  /// Border padding (cells that are always walls)
  pub border_padding: u32,
  /// The tile type for floors
  pub floor_tile: TileType,
  /// The tile type for walls
  pub wall_tile: TileType,
}

impl Default for CellularConfig {
  fn default() -> Self {
    CellularConfig {
      fill_probability: 0.45,
      iterations: 5,
      birth_threshold: 5,
      survival_threshold: 4,
      keep_largest_only: true,
      border_padding: 1,
      floor_tile: TileType::Floor,
      wall_tile: TileType::Wall,
    }
  }
}

impl CellularConfig {
  pub fn new() -> Self {
    Self::default()
  }

  /// Set the initial fill probability
  pub fn with_fill_probability(mut self, probability: f32) -> Self {
    self.fill_probability = probability.clamp(0.0, 1.0);
    self
  }

  /// Set the number of iterations
  pub fn with_iterations(mut self, iterations: u32) -> Self {
    self.iterations = iterations;
    self
  }

  /// Set the birth threshold
  pub fn with_birth_threshold(mut self, threshold: u32) -> Self {
    self.birth_threshold = threshold.min(8);
    self
  }

  /// Set the survival threshold
  pub fn with_survival_threshold(mut self, threshold: u32) -> Self {
    self.survival_threshold = threshold.min(8);
    self
  }

  /// Set whether to keep only the largest connected region
  pub fn with_keep_largest_only(mut self, keep: bool) -> Self {
    self.keep_largest_only = keep;
    self
  }

  /// Set the border padding
  pub fn with_border_padding(mut self, padding: u32) -> Self {
    self.border_padding = padding;
    self
  }

  /// Use a cave-like preset (open caves with some pillars)
  pub fn cave_preset() -> Self {
    CellularConfig {
      fill_probability: 0.40,
      iterations: 4,
      birth_threshold: 5,
      survival_threshold: 4,
      keep_largest_only: true,
      border_padding: 1,
      floor_tile: TileType::Floor,
      wall_tile: TileType::Wall,
    }
  }

  /// Use a maze-like preset (more walls, tighter passages)
  pub fn maze_preset() -> Self {
    CellularConfig {
      fill_probability: 0.50,
      iterations: 3,
      birth_threshold: 4,
      survival_threshold: 3,
      keep_largest_only: true,
      border_padding: 1,
      floor_tile: TileType::Floor,
      wall_tile: TileType::Wall,
    }
  }

  /// Use a smooth cave preset (fewer isolated walls)
  pub fn smooth_cave_preset() -> Self {
    CellularConfig {
      fill_probability: 0.45,
      iterations: 7,
      birth_threshold: 5,
      survival_threshold: 4,
      keep_largest_only: true,
      border_padding: 1,
      floor_tile: TileType::Floor,
      wall_tile: TileType::Wall,
    }
  }
}

/// Generate a cave using cellular automata
pub fn generate(
  width: u32,
  height: u32,
  config: &CellularConfig,
  rng: &mut SeededRng,
) -> GenerationGrid {
  let mut grid = GenerationGrid::new(width, height);

  // Initialize with random walls
  initialize_random(&mut grid, config, rng);

  // Apply cellular automata rules
  for _ in 0..config.iterations {
    step(&mut grid, config);
  }

  // Keep only the largest connected region
  if config.keep_largest_only {
    keep_largest_region(&mut grid, config);
  }

  // Add regions from connected floor areas
  add_regions(&mut grid, config);

  grid
}

/// Initialize the grid with random walls
fn initialize_random(grid: &mut GenerationGrid, config: &CellularConfig, rng: &mut SeededRng) {
  let (width, height) = grid.dimensions();
  let padding = config.border_padding as i32;

  for y in 0..height as i32 {
    for x in 0..width as i32 {
      // Border is always wall, interior tiles randomly filled
      let is_border =
        x < padding || y < padding || x >= width as i32 - padding || y >= height as i32 - padding;

      if is_border || rng.random::<f32>() < config.fill_probability {
        grid.set(x, y, config.wall_tile);
      } else {
        grid.set(x, y, config.floor_tile);
      }
    }
  }
}

/// Perform one step of the cellular automata simulation
fn step(grid: &mut GenerationGrid, config: &CellularConfig) {
  let (width, height) = grid.dimensions();
  let padding = config.border_padding as i32;

  // Create a copy to read from while writing to the original
  let old_grid = grid.clone();

  for y in padding..(height as i32 - padding) {
    for x in padding..(width as i32 - padding) {
      let wall_count = count_wall_neighbors(&old_grid, x, y, config.wall_tile);
      let is_wall = old_grid.get(x, y) == config.wall_tile;

      let new_tile = if is_wall {
        // Wall survives if enough neighbors
        if wall_count >= config.survival_threshold {
          config.wall_tile
        } else {
          config.floor_tile
        }
      } else {
        // Floor becomes wall if enough neighbors (birth)
        if wall_count >= config.birth_threshold {
          config.wall_tile
        } else {
          config.floor_tile
        }
      };

      grid.set(x, y, new_tile);
    }
  }
}

/// Count wall neighbors (8-directional)
fn count_wall_neighbors(grid: &GenerationGrid, x: i32, y: i32, wall_tile: TileType) -> u32 {
  grid
    .neighbors_8(x, y)
    .filter(|(_, _, t)| *t == wall_tile)
    .count() as u32
}

/// Keep only the largest connected floor region
fn keep_largest_region(grid: &mut GenerationGrid, config: &CellularConfig) {
  let (width, height) = grid.dimensions();
  let mut visited = vec![false; (width * height) as usize];
  let mut regions: Vec<Vec<(i32, i32)>> = Vec::new();

  // Find all connected floor regions using flood fill
  for y in 0..height as i32 {
    for x in 0..width as i32 {
      let idx = (y * width as i32 + x) as usize;
      if !visited[idx] && grid.get(x, y) == config.floor_tile {
        let region = flood_fill(grid, x, y, config.floor_tile, &mut visited, width);
        regions.push(region);
      }
    }
  }

  // Find the largest region
  if regions.is_empty() {
    return;
  }

  let largest_idx = regions
    .iter()
    .enumerate()
    .max_by_key(|(_, r)| r.len())
    .map(|(i, _)| i)
    .unwrap();

  // Convert all non-largest regions to walls
  for (i, region) in regions.iter().enumerate() {
    if i != largest_idx {
      for &(x, y) in region {
        grid.set(x, y, config.wall_tile);
      }
    }
  }
}

/// Flood fill to find a connected region
fn flood_fill(
  grid: &GenerationGrid,
  start_x: i32,
  start_y: i32,
  target_tile: TileType,
  visited: &mut [bool],
  width: u32,
) -> Vec<(i32, i32)> {
  let mut region = Vec::new();
  let mut queue = VecDeque::new();
  queue.push_back((start_x, start_y));

  while let Some((x, y)) = queue.pop_front() {
    let idx = (y * width as i32 + x) as usize;

    if visited[idx] {
      continue;
    }

    if grid.get(x, y) != target_tile {
      continue;
    }

    visited[idx] = true;
    region.push((x, y));

    // Add 4-directional neighbors
    for (nx, ny, _) in grid.neighbors_4(x, y) {
      let nidx = (ny * width as i32 + nx) as usize;
      if !visited[nidx] && grid.get(nx, ny) == target_tile {
        queue.push_back((nx, ny));
      }
    }
  }

  region
}

/// Add regions to the grid based on connected floor areas
fn add_regions(grid: &mut GenerationGrid, config: &CellularConfig) {
  let (width, height) = grid.dimensions();
  let mut visited = HashSet::new();

  for y in 0..height as i32 {
    for x in 0..width as i32 {
      if !visited.contains(&(x, y)) && grid.get(x, y) == config.floor_tile {
        // Flood fill to find region
        let mut region_tiles = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back((x, y));

        while let Some((cx, cy)) = queue.pop_front() {
          if visited.contains(&(cx, cy)) {
            continue;
          }
          if grid.get(cx, cy) != config.floor_tile {
            continue;
          }

          visited.insert((cx, cy));
          region_tiles.push((cx, cy));

          for (nx, ny, _) in grid.neighbors_4(cx, cy) {
            if !visited.contains(&(nx, ny)) {
              queue.push_back((nx, ny));
            }
          }
        }

        // Calculate bounds
        if !region_tiles.is_empty() {
          let min_x = region_tiles.iter().map(|&(x, _)| x).min().unwrap();
          let max_x = region_tiles.iter().map(|&(x, _)| x).max().unwrap();
          let min_y = region_tiles.iter().map(|&(_, y)| y).min().unwrap();
          let max_y = region_tiles.iter().map(|&(_, y)| y).max().unwrap();

          let bounds = GridRect::new(
            min_x,
            min_y,
            (max_x - min_x + 1) as u32,
            (max_y - min_y + 1) as u32,
          );

          // Calculate center of mass
          let cx: i32 =
            region_tiles.iter().map(|&(x, _)| x).sum::<i32>() / region_tiles.len() as i32;
          let cy: i32 =
            region_tiles.iter().map(|&(_, y)| y).sum::<i32>() / region_tiles.len() as i32;

          grid.add_region_with_center(bounds, (cx, cy), RegionType::Room);
        }
      }
    }
  }
}

/// Apply cellular automata rules to an existing grid
pub fn apply_to_grid(grid: &mut GenerationGrid, config: &CellularConfig, _rng: &mut SeededRng) {
  for _ in 0..config.iterations {
    step(grid, config);
  }

  if config.keep_largest_only {
    keep_largest_region(grid, config);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn basic_generation() {
    let mut rng = SeededRng::new(12345);
    let config = CellularConfig::default();
    let grid = generate(32, 32, &config, &mut rng);

    // Should have both floor and wall tiles
    let floor_count = grid.count_type(TileType::Floor);
    let wall_count = grid.count_type(TileType::Wall);

    assert!(floor_count > 0);
    assert!(wall_count > 0);
  }

  #[test]
  fn reproducibility() {
    let config = CellularConfig::default();

    let mut rng1 = SeededRng::new(12345);
    let grid1 = generate(32, 32, &config, &mut rng1);

    let mut rng2 = SeededRng::new(12345);
    let grid2 = generate(32, 32, &config, &mut rng2);

    for y in 0..32 {
      for x in 0..32 {
        assert_eq!(grid1.get(x, y), grid2.get(x, y));
      }
    }
  }

  #[test]
  fn border_is_wall() {
    let mut rng = SeededRng::new(12345);
    let config = CellularConfig::default().with_border_padding(2);
    let grid = generate(32, 32, &config, &mut rng);

    // Check borders
    for x in 0..32 {
      assert_eq!(grid.get(x, 0), TileType::Wall);
      assert_eq!(grid.get(x, 1), TileType::Wall);
    }
    for y in 0..32 {
      assert_eq!(grid.get(0, y), TileType::Wall);
      assert_eq!(grid.get(1, y), TileType::Wall);
    }
  }

  #[test]
  fn keep_largest_region_works() {
    let mut rng = SeededRng::new(42);
    let config = CellularConfig::default().with_keep_largest_only(true);
    let grid = generate(32, 32, &config, &mut rng);

    // Should have only one region
    assert_eq!(grid.regions().len(), 1);
  }

  #[test]
  fn presets_work() {
    let mut rng = SeededRng::new(12345);

    let cave = generate(32, 32, &CellularConfig::cave_preset(), &mut rng);
    assert!(cave.count_type(TileType::Floor) > 0);

    rng.reset();
    let maze = generate(32, 32, &CellularConfig::maze_preset(), &mut rng);
    assert!(maze.count_type(TileType::Floor) > 0);

    rng.reset();
    let smooth = generate(32, 32, &CellularConfig::smooth_cave_preset(), &mut rng);
    assert!(smooth.count_type(TileType::Floor) > 0);
  }

  #[test]
  fn fill_probability_affects_density() {
    let mut rng1 = SeededRng::new(12345);
    let sparse_config = CellularConfig::default()
      .with_fill_probability(0.3)
      .with_iterations(0);
    let sparse_grid = generate(32, 32, &sparse_config, &mut rng1);

    let mut rng2 = SeededRng::new(12345);
    let dense_config = CellularConfig::default()
      .with_fill_probability(0.7)
      .with_iterations(0);
    let dense_grid = generate(32, 32, &dense_config, &mut rng2);

    // Lower fill probability = fewer walls = more floor
    let sparse_floor = sparse_grid.count_type(TileType::Floor);
    let dense_floor = dense_grid.count_type(TileType::Floor);

    assert!(sparse_floor > dense_floor);
  }
}
