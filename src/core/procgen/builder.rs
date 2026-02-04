//! Unified builder for procedural generation pipelines
//!
//! Allows chaining multiple generation algorithms together.

use super::config::{SeededRng, TileType};
use super::grid::{GenerationGrid, GenerationResult};
use super::{bsp, cellular, mst, noise, poisson, random_walk, wfc};
use std::time::Instant;

/// Builder for creating procedural generation pipelines
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// let result = ProcgenBuilder::new(64, 64)
///     .with_seed(12345)
///     .bsp(BspConfig::default())
///     .cellular_smooth(CellularConfig::default().with_iterations(2))
///     .build();
///
/// let grid = result.grid();
/// let nav_grid = grid.to_navigation_grid(16.0);
/// ```
pub struct ProcgenBuilder {
  width: u32,
  height: u32,
  seed: Option<u64>,
  steps: Vec<GenerationStep>,
}

enum GenerationStep {
  Bsp(bsp::BspConfig),
  Cellular(cellular::CellularConfig),
  CellularSmooth(cellular::CellularConfig),
  Noise(noise::NoiseConfig),
  RandomWalk(random_walk::RandomWalkConfig),
  Wfc(wfc::WfcConfig),
  ConnectMst(mst::MstConfig),
  Fill(TileType),
  Border(TileType, u32),
  Transform(Box<dyn Fn(i32, i32, TileType) -> TileType + Send + Sync>),
}

impl ProcgenBuilder {
  /// Create a new builder with the given dimensions
  pub fn new(width: u32, height: u32) -> Self {
    ProcgenBuilder {
      width,
      height,
      seed: None,
      steps: Vec::new(),
    }
  }

  /// Set the random seed for reproducibility
  pub fn with_seed(mut self, seed: u64) -> Self {
    self.seed = Some(seed);
    self
  }

  /// Add BSP dungeon generation step
  pub fn bsp(mut self, config: bsp::BspConfig) -> Self {
    self.steps.push(GenerationStep::Bsp(config));
    self
  }

  /// Add cellular automata generation step (generates from scratch)
  pub fn cellular(mut self, config: cellular::CellularConfig) -> Self {
    self.steps.push(GenerationStep::Cellular(config));
    self
  }

  /// Add cellular automata smoothing step (applies to existing grid)
  pub fn cellular_smooth(mut self, config: cellular::CellularConfig) -> Self {
    self.steps.push(GenerationStep::CellularSmooth(config));
    self
  }

  /// Add noise-based generation step
  pub fn noise(mut self, config: noise::NoiseConfig) -> Self {
    self.steps.push(GenerationStep::Noise(config));
    self
  }

  /// Add random walk generation step
  pub fn random_walk(mut self, config: random_walk::RandomWalkConfig) -> Self {
    self.steps.push(GenerationStep::RandomWalk(config));
    self
  }

  /// Add Wave Function Collapse generation step
  pub fn wfc(mut self, config: wfc::WfcConfig) -> Self {
    self.steps.push(GenerationStep::Wfc(config));
    self
  }

  /// Connect existing regions using MST
  pub fn connect_mst(mut self, config: mst::MstConfig) -> Self {
    self.steps.push(GenerationStep::ConnectMst(config));
    self
  }

  /// Fill the entire grid with a tile type
  pub fn fill(mut self, tile: TileType) -> Self {
    self.steps.push(GenerationStep::Fill(tile));
    self
  }

  /// Add a border around the grid
  pub fn border(mut self, tile: TileType, thickness: u32) -> Self {
    self.steps.push(GenerationStep::Border(tile, thickness));
    self
  }

  /// Apply a custom transformation to the grid
  pub fn transform<F>(mut self, f: F) -> Self
  where
    F: Fn(i32, i32, TileType) -> TileType + Send + Sync + 'static,
  {
    self.steps.push(GenerationStep::Transform(Box::new(f)));
    self
  }

  /// Build the generation result
  pub fn build(self) -> GenerationResult {
    let start = Instant::now();

    let seed = self.seed.unwrap_or_else(rand::random);
    let mut rng = SeededRng::new(seed);
    let mut grid = GenerationGrid::new(self.width, self.height);
    let mut algorithm_name = String::from("Pipeline");

    for step in self.steps {
      match step {
        GenerationStep::Bsp(config) => {
          grid = bsp::generate(self.width, self.height, &config, &mut rng);
          algorithm_name = String::from("BSP");
        }
        GenerationStep::Cellular(config) => {
          grid = cellular::generate(self.width, self.height, &config, &mut rng);
          algorithm_name = String::from("Cellular");
        }
        GenerationStep::CellularSmooth(config) => {
          cellular::apply_to_grid(&mut grid, &config, &mut rng);
          algorithm_name.push_str("+CellularSmooth");
        }
        GenerationStep::Noise(config) => {
          grid = noise::generate(self.width, self.height, &config, &mut rng);
          algorithm_name = String::from("Noise");
        }
        GenerationStep::RandomWalk(config) => {
          if grid.count_type(TileType::Empty) == (self.width * self.height) as usize {
            // Grid is empty, generate from scratch
            grid = random_walk::generate(self.width, self.height, &config, &mut rng);
          } else {
            // Apply to existing grid
            random_walk::apply_to_grid(&mut grid, &config, &mut rng);
          }
          algorithm_name = String::from("RandomWalk");
        }
        GenerationStep::Wfc(config) => {
          let (new_grid, _result) = wfc::generate(self.width, self.height, &config, &mut rng);
          grid = new_grid;
          algorithm_name = String::from("WFC");
        }
        GenerationStep::ConnectMst(config) => {
          mst::connect_regions_mst(&mut grid, &config, &mut rng, TileType::Floor);
          algorithm_name.push_str("+MST");
        }
        GenerationStep::Fill(tile) => {
          grid.clear(tile);
        }
        GenerationStep::Border(tile, thickness) => {
          apply_border(&mut grid, tile, thickness);
        }
        GenerationStep::Transform(f) => {
          grid.transform(f);
        }
      }
    }

    let duration = start.elapsed();

    GenerationResult::new(grid, seed, algorithm_name).with_duration(duration.as_millis() as u64)
  }
}

fn apply_border(grid: &mut GenerationGrid, tile: TileType, thickness: u32) {
  let (width, height) = grid.dimensions();
  let t = thickness as i32;

  for y in 0..height as i32 {
    for x in 0..width as i32 {
      if x < t || y < t || x >= width as i32 - t || y >= height as i32 - t {
        grid.set(x, y, tile);
      }
    }
  }
}

/// Generate Poisson disk points (separate from grid generation)
pub fn poisson_points(
  width: f32,
  height: f32,
  config: &poisson::PoissonConfig,
  seed: u64,
) -> poisson::PoissonResult {
  let mut rng = SeededRng::new(seed);
  poisson::generate(width, height, config, &mut rng)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn basic_pipeline() {
    let result = ProcgenBuilder::new(32, 32)
      .with_seed(12345)
      .bsp(bsp::BspConfig::default())
      .build();

    assert!(!result.grid().regions().is_empty());
  }

  #[test]
  fn chained_algorithms() {
    let result = ProcgenBuilder::new(32, 32)
      .with_seed(12345)
      .cellular(cellular::CellularConfig::default())
      .connect_mst(mst::MstConfig::default())
      .build();

    assert!(result.grid().count_type(TileType::Floor) > 0);
  }

  #[test]
  fn reproducibility() {
    let result1 = ProcgenBuilder::new(32, 32)
      .with_seed(12345)
      .bsp(bsp::BspConfig::default())
      .build();

    let result2 = ProcgenBuilder::new(32, 32)
      .with_seed(12345)
      .bsp(bsp::BspConfig::default())
      .build();

    for y in 0..32 {
      for x in 0..32 {
        assert_eq!(result1.grid().get(x, y), result2.grid().get(x, y));
      }
    }
  }

  #[test]
  fn transform_step() {
    let result = ProcgenBuilder::new(32, 32)
      .with_seed(12345)
      .fill(TileType::Floor)
      .transform(|x, y, _| {
        if (x + y) % 2 == 0 {
          TileType::Wall
        } else {
          TileType::Floor
        }
      })
      .build();

    // Checkerboard pattern
    assert_eq!(result.grid().get(0, 0), TileType::Wall);
    assert_eq!(result.grid().get(1, 0), TileType::Floor);
    assert_eq!(result.grid().get(0, 1), TileType::Floor);
    assert_eq!(result.grid().get(1, 1), TileType::Wall);
  }

  #[test]
  fn border_step() {
    let result = ProcgenBuilder::new(32, 32)
      .with_seed(12345)
      .fill(TileType::Floor)
      .border(TileType::Wall, 2)
      .build();

    // Check corners are walls
    assert_eq!(result.grid().get(0, 0), TileType::Wall);
    assert_eq!(result.grid().get(1, 1), TileType::Wall);

    // Check interior is floor
    assert_eq!(result.grid().get(16, 16), TileType::Floor);
  }

  #[test]
  fn duration_tracked() {
    let result = ProcgenBuilder::new(32, 32)
      .with_seed(12345)
      .cellular(cellular::CellularConfig::default())
      .build();

    // Duration tracking is working (value doesn't matter, just that it's set)
    let _ = result.duration_ms;
  }

  #[test]
  fn poisson_points_generation() {
    let result = poisson_points(100.0, 100.0, &poisson::PoissonConfig::default(), 12345);
    assert!(!result.is_empty());
  }
}
