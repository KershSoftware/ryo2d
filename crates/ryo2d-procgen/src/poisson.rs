//! Poisson disk sampling for even point distribution
//!
//! Generates points with a minimum distance between them, useful for
//! placing objects, spawn points, or trees in a natural-looking distribution.

use super::config::SeededRng;

/// Configuration for Poisson disk sampling
#[derive(Debug, Clone)]
pub struct PoissonConfig {
  /// Minimum distance between points
  pub min_distance: f32,
  /// Maximum attempts to place a point around an active point
  pub max_attempts: u32,
}

impl Default for PoissonConfig {
  fn default() -> Self {
    PoissonConfig {
      min_distance: 10.0,
      max_attempts: 30,
    }
  }
}

impl PoissonConfig {
  pub fn new() -> Self {
    Self::default()
  }

  /// Set the minimum distance between points
  pub fn with_min_distance(mut self, distance: f32) -> Self {
    self.min_distance = distance.max(1.0);
    self
  }

  /// Set the maximum attempts per active point
  pub fn with_max_attempts(mut self, attempts: u32) -> Self {
    self.max_attempts = attempts.max(1);
    self
  }
}

/// Result of Poisson disk sampling
#[derive(Debug, Clone)]
pub struct PoissonResult {
  /// Generated points
  pub points: Vec<(f32, f32)>,
  /// Width of the sampling area
  pub width: f32,
  /// Height of the sampling area
  pub height: f32,
}

impl PoissonResult {
  /// Get the number of points generated
  pub fn len(&self) -> usize {
    self.points.len()
  }

  /// Check if no points were generated
  pub fn is_empty(&self) -> bool {
    self.points.is_empty()
  }

  /// Iterate over points
  pub fn iter(&self) -> impl Iterator<Item = &(f32, f32)> {
    self.points.iter()
  }

  /// Convert points to grid coordinates
  pub fn to_grid_coords(&self, cell_size: f32) -> Vec<(i32, i32)> {
    self
      .points
      .iter()
      .map(|&(x, y)| ((x / cell_size) as i32, (y / cell_size) as i32))
      .collect()
  }
}

/// Generate points using Poisson disk sampling
///
/// Uses Bridson's algorithm for O(n) generation.
pub fn generate(
  width: f32,
  height: f32,
  config: &PoissonConfig,
  rng: &mut SeededRng,
) -> PoissonResult {
  let cell_size = config.min_distance / std::f32::consts::SQRT_2;
  let grid_width = (width / cell_size).ceil() as usize;
  let grid_height = (height / cell_size).ceil() as usize;

  // Grid for spatial lookup (-1 means empty, otherwise index into points)
  let mut grid = vec![-1i32; grid_width * grid_height];
  let mut points: Vec<(f32, f32)> = Vec::new();
  let mut active: Vec<usize> = Vec::new();

  // Start with a random point
  let start_x = rng.gen_range(0.0..width);
  let start_y = rng.gen_range(0.0..height);
  add_point(
    start_x,
    start_y,
    &mut points,
    &mut active,
    &mut grid,
    grid_width,
    cell_size,
  );

  // Process active list
  while !active.is_empty() {
    let active_idx = rng.gen_range(0..active.len());
    let point_idx = active[active_idx];
    let (px, py) = points[point_idx];

    let mut found = false;

    for _ in 0..config.max_attempts {
      // Generate random point in annulus around active point
      let angle = rng.gen_range(0.0..std::f32::consts::TAU);
      let radius = rng.gen_range(config.min_distance..config.min_distance * 2.0);

      let new_x = px + radius * angle.cos();
      let new_y = py + radius * angle.sin();

      // Check bounds
      if new_x < 0.0 || new_x >= width || new_y < 0.0 || new_y >= height {
        continue;
      }

      // Check grid neighborhood for conflicts
      if !has_nearby_point(
        new_x,
        new_y,
        &points,
        &grid,
        grid_width,
        grid_height,
        cell_size,
        config.min_distance,
      ) {
        add_point(
          new_x,
          new_y,
          &mut points,
          &mut active,
          &mut grid,
          grid_width,
          cell_size,
        );
        found = true;
        break;
      }
    }

    // If no valid point found, remove from active list
    if !found {
      active.swap_remove(active_idx);
    }
  }

  PoissonResult {
    points,
    width,
    height,
  }
}

fn add_point(
  x: f32,
  y: f32,
  points: &mut Vec<(f32, f32)>,
  active: &mut Vec<usize>,
  grid: &mut [i32],
  grid_width: usize,
  cell_size: f32,
) {
  let idx = points.len();
  points.push((x, y));
  active.push(idx);

  let gx = (x / cell_size) as usize;
  let gy = (y / cell_size) as usize;
  grid[gy * grid_width + gx] = idx as i32;
}

#[allow(clippy::too_many_arguments)]
fn has_nearby_point(
  x: f32,
  y: f32,
  points: &[(f32, f32)],
  grid: &[i32],
  grid_width: usize,
  grid_height: usize,
  cell_size: f32,
  min_distance: f32,
) -> bool {
  let gx = (x / cell_size) as i32;
  let gy = (y / cell_size) as i32;

  // Check 5x5 neighborhood
  for dy in -2..=2 {
    for dx in -2..=2 {
      let nx = gx + dx;
      let ny = gy + dy;

      if nx < 0 || ny < 0 || nx >= grid_width as i32 || ny >= grid_height as i32 {
        continue;
      }

      let idx = grid[ny as usize * grid_width + nx as usize];
      if idx >= 0 {
        let (px, py) = points[idx as usize];
        let dist_sq = (x - px) * (x - px) + (y - py) * (y - py);
        if dist_sq < min_distance * min_distance {
          return true;
        }
      }
    }
  }

  false
}

/// Generate points within a circular area
pub fn generate_in_circle(
  center_x: f32,
  center_y: f32,
  radius: f32,
  config: &PoissonConfig,
  rng: &mut SeededRng,
) -> PoissonResult {
  // Generate in bounding box, then filter
  let size = radius * 2.0;
  let mut result = generate(size, size, config, rng);

  // Offset points and filter to circle
  let offset_x = center_x - radius;
  let offset_y = center_y - radius;

  result.points = result
    .points
    .into_iter()
    .filter_map(|(x, y)| {
      let wx = x + offset_x;
      let wy = y + offset_y;
      let dx = wx - center_x;
      let dy = wy - center_y;
      if dx * dx + dy * dy <= radius * radius {
        Some((wx, wy))
      } else {
        None
      }
    })
    .collect();

  result.width = radius * 2.0;
  result.height = radius * 2.0;

  result
}

/// Generate points that avoid specific areas
pub fn generate_with_mask<F>(
  width: f32,
  height: f32,
  config: &PoissonConfig,
  rng: &mut SeededRng,
  is_valid: F,
) -> PoissonResult
where
  F: Fn(f32, f32) -> bool,
{
  let mut result = generate(width, height, config, rng);

  // Filter points by mask
  result.points.retain(|&(x, y)| is_valid(x, y));

  result
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn basic_generation() {
    let mut rng = SeededRng::new(12345);
    let config = PoissonConfig::default();
    let result = generate(100.0, 100.0, &config, &mut rng);

    // Should generate some points
    assert!(!result.is_empty());
  }

  #[test]
  fn reproducibility() {
    let config = PoissonConfig::default();

    let mut rng1 = SeededRng::new(12345);
    let result1 = generate(100.0, 100.0, &config, &mut rng1);

    let mut rng2 = SeededRng::new(12345);
    let result2 = generate(100.0, 100.0, &config, &mut rng2);

    assert_eq!(result1.points.len(), result2.points.len());
    for (p1, p2) in result1.points.iter().zip(result2.points.iter()) {
      assert!((p1.0 - p2.0).abs() < 0.001);
      assert!((p1.1 - p2.1).abs() < 0.001);
    }
  }

  #[test]
  fn minimum_distance_respected() {
    let mut rng = SeededRng::new(12345);
    let min_dist = 15.0;
    let config = PoissonConfig::default().with_min_distance(min_dist);
    let result = generate(100.0, 100.0, &config, &mut rng);

    // Check all pairs
    for i in 0..result.points.len() {
      for j in (i + 1)..result.points.len() {
        let (x1, y1) = result.points[i];
        let (x2, y2) = result.points[j];
        let dist = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
        assert!(
          dist >= min_dist - 0.001,
          "Points too close: {} < {}",
          dist,
          min_dist
        );
      }
    }
  }

  #[test]
  fn points_within_bounds() {
    let mut rng = SeededRng::new(12345);
    let config = PoissonConfig::default();
    let result = generate(100.0, 80.0, &config, &mut rng);

    for &(x, y) in &result.points {
      assert!((0.0..100.0).contains(&x));
      assert!((0.0..80.0).contains(&y));
    }
  }

  #[test]
  fn smaller_distance_more_points() {
    let mut rng1 = SeededRng::new(12345);
    let sparse = PoissonConfig::default().with_min_distance(20.0);
    let sparse_result = generate(100.0, 100.0, &sparse, &mut rng1);

    let mut rng2 = SeededRng::new(12345);
    let dense = PoissonConfig::default().with_min_distance(5.0);
    let dense_result = generate(100.0, 100.0, &dense, &mut rng2);

    assert!(dense_result.len() > sparse_result.len());
  }

  #[test]
  fn circle_generation() {
    let mut rng = SeededRng::new(12345);
    let config = PoissonConfig::default().with_min_distance(10.0);
    let result = generate_in_circle(50.0, 50.0, 40.0, &config, &mut rng);

    // All points should be within circle
    for &(x, y) in &result.points {
      let dist = ((x - 50.0).powi(2) + (y - 50.0).powi(2)).sqrt();
      assert!(dist <= 40.0 + 0.001);
    }
  }

  #[test]
  fn masked_generation() {
    let mut rng = SeededRng::new(12345);
    let config = PoissonConfig::default().with_min_distance(8.0);

    // Only allow points in right half
    let result = generate_with_mask(100.0, 100.0, &config, &mut rng, |x, _y| x >= 50.0);

    for &(x, _) in &result.points {
      assert!(x >= 50.0);
    }
  }

  #[test]
  fn grid_coord_conversion() {
    let mut rng = SeededRng::new(12345);
    let config = PoissonConfig::default();
    let result = generate(100.0, 100.0, &config, &mut rng);

    let grid_coords = result.to_grid_coords(10.0);
    assert_eq!(grid_coords.len(), result.points.len());
  }
}
