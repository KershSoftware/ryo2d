//! Random walk (Drunkard's Walk) procedural generation
//!
//! Creates organic cave-like structures by simulating a "drunk" walker
//! that carves paths through the grid.

use super::config::{GridRect, SeededRng, TileType};
use super::grid::{GenerationGrid, RegionType};

/// Configuration for random walk generation
#[derive(Debug, Clone)]
pub struct RandomWalkConfig {
  /// Starting position (None = center of grid)
  pub start: Option<(i32, i32)>,
  /// Number of steps to take
  pub steps: u32,
  /// Size of the carving brush (1 = single tile)
  pub brush_size: u32,
  /// Allow diagonal movement
  pub allow_diagonal: bool,
  /// Momentum (0.0 to 1.0) - probability of continuing in the same direction
  pub momentum: f32,
  /// Number of walkers (each starts fresh)
  pub walker_count: u32,
  /// The tile type to carve (place as the walker moves)
  pub carve_tile: TileType,
  /// Border padding to keep walkers away from edges
  pub border_padding: u32,
}

impl Default for RandomWalkConfig {
  fn default() -> Self {
    RandomWalkConfig {
      start: None,
      steps: 1000,
      brush_size: 1,
      allow_diagonal: false,
      momentum: 0.3,
      walker_count: 1,
      carve_tile: TileType::Floor,
      border_padding: 1,
    }
  }
}

impl RandomWalkConfig {
  pub fn new() -> Self {
    Self::default()
  }

  /// Set the starting position
  pub fn with_start(mut self, x: i32, y: i32) -> Self {
    self.start = Some((x, y));
    self
  }

  /// Set the number of steps
  pub fn with_steps(mut self, steps: u32) -> Self {
    self.steps = steps;
    self
  }

  /// Set the brush size
  pub fn with_brush_size(mut self, size: u32) -> Self {
    self.brush_size = size.max(1);
    self
  }

  /// Enable/disable diagonal movement
  pub fn with_diagonal(mut self, allow: bool) -> Self {
    self.allow_diagonal = allow;
    self
  }

  /// Set momentum (probability of continuing in the same direction)
  pub fn with_momentum(mut self, momentum: f32) -> Self {
    self.momentum = momentum.clamp(0.0, 1.0);
    self
  }

  /// Set the number of walkers
  pub fn with_walker_count(mut self, count: u32) -> Self {
    self.walker_count = count.max(1);
    self
  }

  /// Set the tile type to carve
  pub fn with_carve_tile(mut self, tile: TileType) -> Self {
    self.carve_tile = tile;
    self
  }

  /// Set the border padding
  pub fn with_border_padding(mut self, padding: u32) -> Self {
    self.border_padding = padding;
    self
  }
}

/// Direction for walker movement
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
  Up,
  Down,
  Left,
  Right,
  UpLeft,
  UpRight,
  DownLeft,
  DownRight,
}

impl Direction {
  fn as_delta(&self) -> (i32, i32) {
    match self {
      Direction::Up => (0, -1),
      Direction::Down => (0, 1),
      Direction::Left => (-1, 0),
      Direction::Right => (1, 0),
      Direction::UpLeft => (-1, -1),
      Direction::UpRight => (1, -1),
      Direction::DownLeft => (-1, 1),
      Direction::DownRight => (1, 1),
    }
  }

  fn cardinal() -> &'static [Direction] {
    &[
      Direction::Up,
      Direction::Down,
      Direction::Left,
      Direction::Right,
    ]
  }

  fn all() -> &'static [Direction] {
    &[
      Direction::Up,
      Direction::Down,
      Direction::Left,
      Direction::Right,
      Direction::UpLeft,
      Direction::UpRight,
      Direction::DownLeft,
      Direction::DownRight,
    ]
  }
}

/// Generate a cave using random walk algorithm
pub fn generate(
  width: u32,
  height: u32,
  config: &RandomWalkConfig,
  rng: &mut SeededRng,
) -> GenerationGrid {
  let mut grid = GenerationGrid::filled(width, height, TileType::Wall);

  for _ in 0..config.walker_count {
    run_walker(&mut grid, config, rng);
  }

  // Find the carved area bounds for region tracking
  let carved_bounds = find_carved_bounds(&grid, config.carve_tile);
  if let Some(bounds) = carved_bounds {
    grid.add_region(bounds, RegionType::Room);
  }

  grid
}

/// Run a single walker on the grid
fn run_walker(grid: &mut GenerationGrid, config: &RandomWalkConfig, rng: &mut SeededRng) {
  let (width, height) = grid.dimensions();

  // Determine starting position
  let (mut x, mut y) = config
    .start
    .unwrap_or((width as i32 / 2, height as i32 / 2));

  // Clamp to valid bounds
  let padding = config.border_padding as i32;
  x = x.clamp(padding, width as i32 - 1 - padding);
  y = y.clamp(padding, height as i32 - 1 - padding);

  let directions = if config.allow_diagonal {
    Direction::all()
  } else {
    Direction::cardinal()
  };

  let mut last_direction: Option<Direction> = None;

  for _ in 0..config.steps {
    // Carve at current position
    carve_brush(grid, x, y, config.brush_size, config.carve_tile);

    // Choose next direction
    let direction = if let Some(last) = last_direction {
      if rng.random::<f32>() < config.momentum {
        // Continue in same direction if valid
        let (dx, dy) = last.as_delta();
        let next_x = x + dx;
        let next_y = y + dy;
        if is_valid_position(next_x, next_y, width, height, padding) {
          last
        } else {
          pick_random_direction(directions, rng)
        }
      } else {
        pick_random_direction(directions, rng)
      }
    } else {
      pick_random_direction(directions, rng)
    };

    // Move
    let (dx, dy) = direction.as_delta();
    let next_x = x + dx;
    let next_y = y + dy;

    if is_valid_position(next_x, next_y, width, height, padding) {
      x = next_x;
      y = next_y;
      last_direction = Some(direction);
    } else {
      // Hit a wall, pick a new direction next time
      last_direction = None;
    }
  }
}

fn pick_random_direction(directions: &[Direction], rng: &mut SeededRng) -> Direction {
  directions[rng.gen_range(0..directions.len())]
}

fn is_valid_position(x: i32, y: i32, width: u32, height: u32, padding: i32) -> bool {
  x >= padding && y >= padding && x < width as i32 - padding && y < height as i32 - padding
}

fn carve_brush(grid: &mut GenerationGrid, x: i32, y: i32, brush_size: u32, tile: TileType) {
  let half = brush_size as i32 / 2;
  for dy in -half..=(brush_size as i32 - half - 1) {
    for dx in -half..=(brush_size as i32 - half - 1) {
      grid.set(x + dx, y + dy, tile);
    }
  }
}

fn find_carved_bounds(grid: &GenerationGrid, tile: TileType) -> Option<GridRect> {
  let mut min_x = i32::MAX;
  let mut min_y = i32::MAX;
  let mut max_x = i32::MIN;
  let mut max_y = i32::MIN;

  for (x, y, t) in grid.iter() {
    if t == tile {
      min_x = min_x.min(x);
      min_y = min_y.min(y);
      max_x = max_x.max(x);
      max_y = max_y.max(y);
    }
  }

  if min_x <= max_x && min_y <= max_y {
    Some(GridRect::new(
      min_x,
      min_y,
      (max_x - min_x + 1) as u32,
      (max_y - min_y + 1) as u32,
    ))
  } else {
    None
  }
}

/// Apply random walk to an existing grid (useful for adding corridors)
pub fn apply_to_grid(grid: &mut GenerationGrid, config: &RandomWalkConfig, rng: &mut SeededRng) {
  for _ in 0..config.walker_count {
    run_walker(grid, config, rng);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn basic_generation() {
    let mut rng = SeededRng::new(12345);
    let config = RandomWalkConfig::default().with_steps(500);
    let grid = generate(32, 32, &config, &mut rng);

    // Should have carved some floor tiles
    let floor_count = grid.count_type(TileType::Floor);
    assert!(floor_count > 0);
  }

  #[test]
  fn reproducibility() {
    let config = RandomWalkConfig::default().with_steps(100);

    let mut rng1 = SeededRng::new(12345);
    let grid1 = generate(32, 32, &config, &mut rng1);

    let mut rng2 = SeededRng::new(12345);
    let grid2 = generate(32, 32, &config, &mut rng2);

    // Same seed should produce same result
    for y in 0..32 {
      for x in 0..32 {
        assert_eq!(grid1.get(x, y), grid2.get(x, y));
      }
    }
  }

  #[test]
  fn brush_size_affects_coverage() {
    let mut rng1 = SeededRng::new(12345);
    let config1 = RandomWalkConfig::default()
      .with_steps(100)
      .with_brush_size(1);
    let grid1 = generate(32, 32, &config1, &mut rng1);

    let mut rng2 = SeededRng::new(12345);
    let config2 = RandomWalkConfig::default()
      .with_steps(100)
      .with_brush_size(3);
    let grid2 = generate(32, 32, &config2, &mut rng2);

    let floor_count1 = grid1.count_type(TileType::Floor);
    let floor_count2 = grid2.count_type(TileType::Floor);

    // Larger brush should carve more tiles (or equal in edge cases)
    assert!(floor_count2 >= floor_count1);
  }

  #[test]
  fn multiple_walkers() {
    let mut rng = SeededRng::new(12345);
    let config = RandomWalkConfig::default()
      .with_steps(50)
      .with_walker_count(5);
    let grid = generate(32, 32, &config, &mut rng);

    // Multiple walkers should carve more area
    let floor_count = grid.count_type(TileType::Floor);
    assert!(floor_count > 0);
  }

  #[test]
  fn region_created() {
    let mut rng = SeededRng::new(12345);
    let config = RandomWalkConfig::default().with_steps(200);
    let grid = generate(32, 32, &config, &mut rng);

    // Should have one region
    assert_eq!(grid.regions().len(), 1);
  }

  #[test]
  fn border_padding_respected() {
    let mut rng = SeededRng::new(12345);
    let config = RandomWalkConfig::default()
      .with_steps(500)
      .with_border_padding(2);
    let grid = generate(32, 32, &config, &mut rng);

    // Check edges are still walls
    for x in 0..32 {
      assert_eq!(grid.get(x, 0), TileType::Wall);
      assert_eq!(grid.get(x, 1), TileType::Wall);
      assert_eq!(grid.get(x, 31), TileType::Wall);
      assert_eq!(grid.get(x, 30), TileType::Wall);
    }
    for y in 0..32 {
      assert_eq!(grid.get(0, y), TileType::Wall);
      assert_eq!(grid.get(1, y), TileType::Wall);
      assert_eq!(grid.get(31, y), TileType::Wall);
      assert_eq!(grid.get(30, y), TileType::Wall);
    }
  }
}
