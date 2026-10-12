//! Core configuration types for procedural generation
//!
//! Provides the foundational types used across all generation algorithms.

use rand::Rng;
use rand::SeedableRng;
use rand::distributions::uniform::{SampleRange, SampleUniform};
use rand::distributions::{Distribution, Standard};
use rand::rngs::StdRng;

/// A seeded random number generator for reproducible procedural generation
///
/// Wraps `StdRng` to provide seed tracking and reset functionality,
/// ensuring the same seed always produces the same generation output.
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// // Create with explicit seed for reproducibility
/// let mut rng = SeededRng::new(12345);
/// println!("Seed: {}", rng.seed());
///
/// // Reset to regenerate the same sequence
/// rng.reset();
///
/// // Create with random entropy for variety
/// let mut rng = SeededRng::from_entropy();
/// ```
#[derive(Debug)]
pub struct SeededRng {
  seed: u64,
  rng: StdRng,
}

impl SeededRng {
  /// Create a new RNG with a specific seed
  pub fn new(seed: u64) -> Self {
    SeededRng {
      seed,
      rng: StdRng::seed_from_u64(seed),
    }
  }

  /// Create a new RNG with random entropy
  pub fn from_entropy() -> Self {
    // Extract a seed-like value for tracking (not the actual internal state)
    let seed: u64 = rand::random();
    SeededRng {
      seed,
      rng: StdRng::seed_from_u64(seed),
    }
  }

  /// Get the seed used to create this RNG
  pub fn seed(&self) -> u64 {
    self.seed
  }

  /// Reset the RNG to its initial state (same seed)
  pub fn reset(&mut self) {
    self.rng = StdRng::seed_from_u64(self.seed);
  }

  /// Get a mutable reference to the underlying RNG
  pub fn rng(&mut self) -> &mut StdRng {
    &mut self.rng
  }

  /// Generate a random value of type T
  pub fn random<T>(&mut self) -> T
  where
    Standard: Distribution<T>,
  {
    self.rng.sample(Standard)
  }

  /// Generate a random value in a range
  pub fn gen_range<T, R>(&mut self, range: R) -> T
  where
    T: SampleUniform,
    R: SampleRange<T>,
  {
    self.rng.gen_range(range)
  }

  /// Generate a random boolean with given probability
  pub fn gen_bool(&mut self, p: f64) -> bool {
    self.rng.gen_bool(p)
  }

  /// Create a child RNG derived from this one
  ///
  /// Useful for passing to sub-algorithms while maintaining
  /// reproducibility of the parent generator.
  pub fn derive(&mut self) -> SeededRng {
    let child_seed: u64 = self.random();
    SeededRng::new(child_seed)
  }
}

impl Clone for SeededRng {
  fn clone(&self) -> Self {
    SeededRng::new(self.seed)
  }
}

/// Tile types for procedural generation
///
/// Represents different terrain/tile states in generated maps.
/// Use `Custom(u32)` for game-specific tile types.
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// let tile = TileType::Floor;
/// assert!(tile.is_walkable());
///
/// // Custom tile for a specific game
/// let lava = TileType::Custom(100);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TileType {
  /// Empty/void space (typically not walkable, not rendered)
  #[default]
  Empty,
  /// Walkable floor
  Floor,
  /// Impassable wall
  Wall,
  /// Door (potentially walkable, special handling)
  Door,
  /// Water (may be walkable with cost or not at all)
  Water,
  /// Custom tile type for game-specific use
  Custom(u32),
}

impl TileType {
  /// Check if this tile type is typically walkable
  ///
  /// Note: This is a heuristic. Games may override this based on context
  /// (e.g., water walkable for boats, doors walkable when unlocked).
  pub fn is_walkable(&self) -> bool {
    matches!(self, TileType::Floor | TileType::Door)
  }

  /// Check if this tile type is solid (blocks movement and line of sight)
  pub fn is_solid(&self) -> bool {
    matches!(self, TileType::Wall)
  }

  /// Check if this tile type is empty (void/nothing)
  pub fn is_empty(&self) -> bool {
    matches!(self, TileType::Empty)
  }

  /// Convert to navigation CellState for pathfinding integration
  #[cfg(any())] // MIGRATION: Task 16
  pub fn to_cell_state(&self) -> crate::core::navigation::CellState {
    use crate::core::navigation::CellState;
    match self {
      TileType::Empty => CellState::Blocked,
      TileType::Floor => CellState::Walkable,
      TileType::Wall => CellState::Blocked,
      TileType::Door => CellState::Walkable,
      TileType::Water => CellState::Difficult(2.0),
      TileType::Custom(_) => CellState::Walkable,
    }
  }
}

/// Rectangular bounds in grid coordinates
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridRect {
  pub x: i32,
  pub y: i32,
  pub width: u32,
  pub height: u32,
}

impl GridRect {
  pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
    GridRect {
      x,
      y,
      width,
      height,
    }
  }

  /// Get the center point of this rectangle
  pub fn center(&self) -> (i32, i32) {
    (
      self.x + self.width as i32 / 2,
      self.y + self.height as i32 / 2,
    )
  }

  /// Get the right edge x coordinate (exclusive)
  pub fn right(&self) -> i32 {
    self.x + self.width as i32
  }

  /// Get the bottom edge y coordinate (exclusive)
  pub fn bottom(&self) -> i32 {
    self.y + self.height as i32
  }

  /// Check if a point is within this rectangle
  pub fn contains(&self, x: i32, y: i32) -> bool {
    x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
  }

  /// Check if this rectangle overlaps with another
  pub fn overlaps(&self, other: &GridRect) -> bool {
    self.x < other.right()
      && self.right() > other.x
      && self.y < other.bottom()
      && self.bottom() > other.y
  }

  /// Get the area of this rectangle
  pub fn area(&self) -> u32 {
    self.width * self.height
  }

  /// Shrink the rectangle by a padding amount on all sides
  pub fn shrink(&self, padding: u32) -> Option<GridRect> {
    let p = padding as i32;
    let new_width = self.width.saturating_sub(padding * 2);
    let new_height = self.height.saturating_sub(padding * 2);

    if new_width == 0 || new_height == 0 {
      return None;
    }

    Some(GridRect {
      x: self.x + p,
      y: self.y + p,
      width: new_width,
      height: new_height,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn seeded_rng_reproducibility() {
    let mut rng1 = SeededRng::new(12345);
    let mut rng2 = SeededRng::new(12345);

    let values1: Vec<u32> = (0..10).map(|_| rng1.random()).collect();
    let values2: Vec<u32> = (0..10).map(|_| rng2.random()).collect();

    assert_eq!(values1, values2);
  }

  #[test]
  fn seeded_rng_reset() {
    let mut rng = SeededRng::new(12345);

    let first: u32 = rng.random();
    let _: u32 = rng.random(); // Advance state

    rng.reset();
    let after_reset: u32 = rng.random();

    assert_eq!(first, after_reset);
  }

  #[test]
  fn seeded_rng_derive() {
    let mut rng = SeededRng::new(12345);
    let mut child1 = rng.derive();
    let mut child2 = rng.derive();

    let v1: u32 = child1.random();
    let v2: u32 = child2.random();

    // Children should have different sequences
    assert_ne!(v1, v2);
  }

  #[test]
  fn tile_type_walkability() {
    assert!(TileType::Floor.is_walkable());
    assert!(TileType::Door.is_walkable());
    assert!(!TileType::Wall.is_walkable());
    assert!(!TileType::Empty.is_walkable());
    assert!(!TileType::Water.is_walkable());
  }

  #[test]
  fn tile_type_solid() {
    assert!(TileType::Wall.is_solid());
    assert!(!TileType::Floor.is_solid());
    assert!(!TileType::Empty.is_solid());
  }

  #[test]
  fn grid_rect_center() {
    let rect = GridRect::new(10, 20, 8, 6);
    assert_eq!(rect.center(), (14, 23));
  }

  #[test]
  fn grid_rect_contains() {
    let rect = GridRect::new(10, 20, 5, 5);
    assert!(rect.contains(10, 20));
    assert!(rect.contains(14, 24));
    assert!(!rect.contains(15, 20)); // Right edge is exclusive
    assert!(!rect.contains(10, 25)); // Bottom edge is exclusive
    assert!(!rect.contains(9, 20));
  }

  #[test]
  fn grid_rect_overlaps() {
    let rect1 = GridRect::new(0, 0, 10, 10);
    let rect2 = GridRect::new(5, 5, 10, 10);
    let rect3 = GridRect::new(20, 20, 5, 5);

    assert!(rect1.overlaps(&rect2));
    assert!(rect2.overlaps(&rect1));
    assert!(!rect1.overlaps(&rect3));
  }

  #[test]
  fn grid_rect_shrink() {
    let rect = GridRect::new(0, 0, 10, 10);
    let shrunk = rect.shrink(2).unwrap();

    assert_eq!(shrunk.x, 2);
    assert_eq!(shrunk.y, 2);
    assert_eq!(shrunk.width, 6);
    assert_eq!(shrunk.height, 6);

    // Too much padding
    let tiny = GridRect::new(0, 0, 4, 4);
    assert!(tiny.shrink(3).is_none());
  }
}
