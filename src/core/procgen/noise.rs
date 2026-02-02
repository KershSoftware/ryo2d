//! Noise-based procedural generation
//!
//! Uses Perlin/Simplex noise for terrain generation and height maps.

use super::config::{SeededRng, TileType};
use super::grid::GenerationGrid;
use noise::{NoiseFn, Perlin, SuperSimplex};

/// Type of noise function to use
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoiseType {
  /// Perlin noise - classic smooth noise
  #[default]
  Perlin,
  /// Simplex noise - similar to Perlin but faster and fewer artifacts
  Simplex,
}

/// A threshold mapping for converting noise values to tiles
#[derive(Debug, Clone)]
pub struct NoiseThreshold {
  /// Maximum noise value for this tile type (exclusive for next threshold)
  pub max_value: f64,
  /// Tile type to use
  pub tile: TileType,
}

impl NoiseThreshold {
  pub fn new(max_value: f64, tile: TileType) -> Self {
    NoiseThreshold { max_value, tile }
  }
}

/// Configuration for noise-based generation
#[derive(Debug, Clone)]
pub struct NoiseConfig {
  /// Type of noise function
  pub noise_type: NoiseType,
  /// Scale of the noise (smaller = more zoomed in)
  pub scale: f64,
  /// Number of octaves (layers of detail)
  pub octaves: usize,
  /// Persistence (how much each octave contributes)
  pub persistence: f64,
  /// Lacunarity (frequency multiplier between octaves)
  pub lacunarity: f64,
  /// Thresholds for converting noise to tiles (sorted by max_value)
  pub thresholds: Vec<NoiseThreshold>,
  /// Offset X for noise sampling
  pub offset_x: f64,
  /// Offset Y for noise sampling
  pub offset_y: f64,
}

impl Default for NoiseConfig {
  fn default() -> Self {
    NoiseConfig {
      noise_type: NoiseType::Perlin,
      scale: 0.05,
      octaves: 4,
      persistence: 0.5,
      lacunarity: 2.0,
      thresholds: vec![
        NoiseThreshold::new(-0.2, TileType::Water),
        NoiseThreshold::new(0.3, TileType::Floor),
        NoiseThreshold::new(f64::MAX, TileType::Wall),
      ],
      offset_x: 0.0,
      offset_y: 0.0,
    }
  }
}

impl NoiseConfig {
  pub fn new() -> Self {
    Self::default()
  }

  /// Set the noise type
  pub fn with_noise_type(mut self, noise_type: NoiseType) -> Self {
    self.noise_type = noise_type;
    self
  }

  /// Set the scale (zoom level)
  pub fn with_scale(mut self, scale: f64) -> Self {
    self.scale = scale.max(0.001);
    self
  }

  /// Set the number of octaves
  pub fn with_octaves(mut self, octaves: usize) -> Self {
    self.octaves = octaves.clamp(1, 16);
    self
  }

  /// Set persistence (amplitude decay per octave)
  pub fn with_persistence(mut self, persistence: f64) -> Self {
    self.persistence = persistence.clamp(0.0, 1.0);
    self
  }

  /// Set lacunarity (frequency increase per octave)
  pub fn with_lacunarity(mut self, lacunarity: f64) -> Self {
    self.lacunarity = lacunarity.max(1.0);
    self
  }

  /// Set the thresholds for tile conversion
  pub fn with_thresholds(mut self, thresholds: Vec<NoiseThreshold>) -> Self {
    self.thresholds = thresholds;
    self
  }

  /// Set offset for seamless tiling or exploration
  pub fn with_offset(mut self, x: f64, y: f64) -> Self {
    self.offset_x = x;
    self.offset_y = y;
    self
  }

  /// Use an island preset (water around edges, land in middle)
  pub fn island_preset() -> Self {
    NoiseConfig {
      noise_type: NoiseType::Simplex,
      scale: 0.03,
      octaves: 4,
      persistence: 0.5,
      lacunarity: 2.0,
      thresholds: vec![
        NoiseThreshold::new(-0.3, TileType::Water),
        NoiseThreshold::new(0.1, TileType::Floor),
        NoiseThreshold::new(0.5, TileType::Wall),
        NoiseThreshold::new(f64::MAX, TileType::Custom(1)), // Mountains
      ],
      offset_x: 0.0,
      offset_y: 0.0,
    }
  }

  /// Use a cave preset (binary floor/wall)
  pub fn cave_preset() -> Self {
    NoiseConfig {
      noise_type: NoiseType::Perlin,
      scale: 0.1,
      octaves: 3,
      persistence: 0.5,
      lacunarity: 2.0,
      thresholds: vec![
        NoiseThreshold::new(0.0, TileType::Floor),
        NoiseThreshold::new(f64::MAX, TileType::Wall),
      ],
      offset_x: 0.0,
      offset_y: 0.0,
    }
  }

  /// Use a terrain preset with multiple biomes
  pub fn terrain_preset() -> Self {
    NoiseConfig {
      noise_type: NoiseType::Simplex,
      scale: 0.02,
      octaves: 6,
      persistence: 0.5,
      lacunarity: 2.0,
      thresholds: vec![
        NoiseThreshold::new(-0.4, TileType::Water), // Deep water
        NoiseThreshold::new(-0.2, TileType::Custom(1)), // Shallow water
        NoiseThreshold::new(0.0, TileType::Custom(2)), // Sand/beach
        NoiseThreshold::new(0.3, TileType::Floor),  // Grass
        NoiseThreshold::new(0.5, TileType::Custom(3)), // Forest
        NoiseThreshold::new(0.7, TileType::Wall),   // Mountain
        NoiseThreshold::new(f64::MAX, TileType::Custom(4)), // Snow
      ],
      offset_x: 0.0,
      offset_y: 0.0,
    }
  }
}

/// Generate terrain using noise
pub fn generate(
  width: u32,
  height: u32,
  config: &NoiseConfig,
  rng: &mut SeededRng,
) -> GenerationGrid {
  let mut grid = GenerationGrid::new(width, height);
  let seed = rng.seed() as u32;

  // Create noise source
  match config.noise_type {
    NoiseType::Perlin => {
      let noise = Perlin::new(seed);
      fill_grid_with_noise(&mut grid, &noise, config, width, height);
    }
    NoiseType::Simplex => {
      let noise = SuperSimplex::new(seed);
      fill_grid_with_noise(&mut grid, &noise, config, width, height);
    }
  }

  grid
}

/// Fill grid using a noise function
fn fill_grid_with_noise<N: NoiseFn<f64, 2>>(
  grid: &mut GenerationGrid,
  noise: &N,
  config: &NoiseConfig,
  width: u32,
  height: u32,
) {
  for y in 0..height as i32 {
    for x in 0..width as i32 {
      let noise_value = sample_octaves(
        noise,
        x as f64 + config.offset_x,
        y as f64 + config.offset_y,
        config,
      );

      let tile = noise_to_tile(noise_value, &config.thresholds);
      grid.set(x, y, tile);
    }
  }
}

/// Sample noise with octaves (fractal Brownian motion)
fn sample_octaves<N: NoiseFn<f64, 2>>(noise: &N, x: f64, y: f64, config: &NoiseConfig) -> f64 {
  let mut total = 0.0;
  let mut amplitude = 1.0;
  let mut frequency = config.scale;
  let mut max_value = 0.0;

  for _ in 0..config.octaves {
    total += noise.get([x * frequency, y * frequency]) * amplitude;
    max_value += amplitude;
    amplitude *= config.persistence;
    frequency *= config.lacunarity;
  }

  // Normalize to [-1, 1]
  total / max_value
}

/// Convert noise value to tile type based on thresholds
fn noise_to_tile(value: f64, thresholds: &[NoiseThreshold]) -> TileType {
  for threshold in thresholds {
    if value < threshold.max_value {
      return threshold.tile;
    }
  }
  // Default to last threshold's tile if no match
  thresholds.last().map(|t| t.tile).unwrap_or(TileType::Empty)
}

/// Generate a height map as f32 values (0.0 to 1.0)
pub fn generate_heightmap(
  width: u32,
  height: u32,
  config: &NoiseConfig,
  rng: &mut SeededRng,
) -> Vec<f32> {
  let seed = rng.seed() as u32;
  let mut heightmap = vec![0.0f32; (width * height) as usize];

  match config.noise_type {
    NoiseType::Perlin => {
      let noise = Perlin::new(seed);
      fill_heightmap(&noise, &mut heightmap, config, width, height);
    }
    NoiseType::Simplex => {
      let noise = SuperSimplex::new(seed);
      fill_heightmap(&noise, &mut heightmap, config, width, height);
    }
  }

  heightmap
}

fn fill_heightmap<N: NoiseFn<f64, 2>>(
  noise: &N,
  heightmap: &mut [f32],
  config: &NoiseConfig,
  width: u32,
  height: u32,
) {
  for y in 0..height {
    for x in 0..width {
      let value = sample_octaves(
        noise,
        x as f64 + config.offset_x,
        y as f64 + config.offset_y,
        config,
      );
      // Convert from [-1, 1] to [0, 1]
      let normalized = ((value + 1.0) / 2.0) as f32;
      heightmap[(y * width + x) as usize] = normalized.clamp(0.0, 1.0);
    }
  }
}

/// Apply island gradient (falloff from center)
pub fn apply_island_gradient(grid: &mut GenerationGrid, falloff_strength: f64) {
  let (width, height) = grid.dimensions();
  let cx = width as f64 / 2.0;
  let cy = height as f64 / 2.0;
  let max_dist = (cx * cx + cy * cy).sqrt();

  for y in 0..height as i32 {
    for x in 0..width as i32 {
      let dx = x as f64 - cx;
      let dy = y as f64 - cy;
      let dist = (dx * dx + dy * dy).sqrt() / max_dist;

      // Apply falloff - edges become water
      if dist > (1.0 - falloff_strength) {
        grid.set(x, y, TileType::Water);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn basic_generation() {
    let mut rng = SeededRng::new(12345);
    let config = NoiseConfig::default();
    let grid = generate(32, 32, &config, &mut rng);

    // Should have various tile types based on thresholds
    let floor_count = grid.count_type(TileType::Floor);
    assert!(floor_count > 0);
  }

  #[test]
  fn reproducibility() {
    let config = NoiseConfig::default();

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
  fn simplex_noise_works() {
    let mut rng = SeededRng::new(12345);
    let config = NoiseConfig::default().with_noise_type(NoiseType::Simplex);
    let grid = generate(32, 32, &config, &mut rng);

    assert!(grid.count_type(TileType::Floor) > 0);
  }

  #[test]
  fn scale_affects_result() {
    let mut rng1 = SeededRng::new(12345);
    let small_scale = NoiseConfig::default().with_scale(0.01);
    let grid1 = generate(32, 32, &small_scale, &mut rng1);

    let mut rng2 = SeededRng::new(12345);
    let large_scale = NoiseConfig::default().with_scale(0.2);
    let grid2 = generate(32, 32, &large_scale, &mut rng2);

    // Different scales should produce different results
    let mut different = false;
    for y in 0..32 {
      for x in 0..32 {
        if grid1.get(x, y) != grid2.get(x, y) {
          different = true;
          break;
        }
      }
    }
    assert!(different);
  }

  #[test]
  fn presets_work() {
    let mut rng = SeededRng::new(12345);

    let island = generate(32, 32, &NoiseConfig::island_preset(), &mut rng);
    assert!(island.count_type(TileType::Water) > 0);

    rng.reset();
    let cave = generate(32, 32, &NoiseConfig::cave_preset(), &mut rng);
    assert!(cave.count_type(TileType::Floor) > 0);
    assert!(cave.count_type(TileType::Wall) > 0);

    rng.reset();
    let terrain = generate(32, 32, &NoiseConfig::terrain_preset(), &mut rng);
    assert!(terrain.count_type(TileType::Floor) > 0);
  }

  #[test]
  fn heightmap_generation() {
    let mut rng = SeededRng::new(12345);
    let config = NoiseConfig::default();
    let heightmap = generate_heightmap(32, 32, &config, &mut rng);

    assert_eq!(heightmap.len(), 32 * 32);

    // All values should be in [0, 1]
    for &value in &heightmap {
      assert!(value >= 0.0 && value <= 1.0);
    }
  }

  #[test]
  fn island_gradient() {
    let mut rng = SeededRng::new(12345);
    let config = NoiseConfig::default();
    let mut grid = generate(32, 32, &config, &mut rng);

    apply_island_gradient(&mut grid, 0.3);

    // Corners should be water
    assert_eq!(grid.get(0, 0), TileType::Water);
    assert_eq!(grid.get(31, 0), TileType::Water);
    assert_eq!(grid.get(0, 31), TileType::Water);
    assert_eq!(grid.get(31, 31), TileType::Water);
  }
}
