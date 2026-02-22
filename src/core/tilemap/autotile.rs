//! Bitmask-based auto-tile system.
//!
//! Automatically selects tile variants based on neighboring tiles using bitmask
//! lookups. Supports two modes:
//!
//! - **Minimal** (4-bit): Only considers cardinal neighbors (N/E/S/W), producing
//!   16 possible combinations.
//! - **Full** (8-bit): Considers all 8 neighbors with diagonal reduction — diagonal
//!   bits are only set when both adjacent cardinals also match, reducing 256 raw
//!   combinations to 47 meaningful shapes.

use std::collections::HashMap;

use super::TileId;

// ── Neighbor bit constants ──────────────────────────────────────────────

/// North neighbor (cardinal, bit 0).
pub const NORTH: u8 = 0b0000_0001;
/// East neighbor (cardinal, bit 1).
pub const EAST: u8 = 0b0000_0010;
/// South neighbor (cardinal, bit 2).
pub const SOUTH: u8 = 0b0000_0100;
/// West neighbor (cardinal, bit 3).
pub const WEST: u8 = 0b0000_1000;

/// North-east neighbor (diagonal, bit 4).
pub const NORTH_EAST: u8 = 0b0001_0000;
/// South-east neighbor (diagonal, bit 5).
pub const SOUTH_EAST: u8 = 0b0010_0000;
/// South-west neighbor (diagonal, bit 6).
pub const SOUTH_WEST: u8 = 0b0100_0000;
/// North-west neighbor (diagonal, bit 7).
pub const NORTH_WEST: u8 = 0b1000_0000;

// ── AutoTileMode ────────────────────────────────────────────────────────

/// Determines how many neighbors participate in auto-tile bitmask computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTileMode {
  /// 4-bit cardinal only (N/E/S/W), producing 16 combinations.
  Minimal,
  /// 8-bit with diagonal reduction, producing 47 unique shapes.
  Full,
}

// ── AutoTileSet ─────────────────────────────────────────────────────────

/// Maps neighbor bitmasks to specific tile IDs for a terrain type.
///
/// Create an `AutoTileSet`, add rules mapping bitmask patterns to tile IDs,
/// then call [`resolve`](AutoTileSet::resolve) at runtime to pick the correct
/// tile for a given neighbor configuration.
#[derive(Debug, Clone)]
pub struct AutoTileSet {
  /// Human-readable name for this auto-tile set.
  pub name: String,
  /// The terrain ID this auto-tile set applies to.
  pub terrain_id: u16,
  /// Whether to use 4-bit or 8-bit bitmask mode.
  pub mode: AutoTileMode,
  /// Fallback tile returned when no rule matches a bitmask.
  pub default_tile: TileId,
  rules: HashMap<u8, TileId>,
}

impl AutoTileSet {
  /// Creates a new auto-tile set.
  pub fn new(
    name: impl Into<String>,
    terrain_id: u16,
    mode: AutoTileMode,
    default_tile: TileId,
  ) -> Self {
    Self {
      name: name.into(),
      terrain_id,
      mode,
      default_tile,
      rules: HashMap::new(),
    }
  }

  /// Adds a bitmask-to-tile mapping rule.
  pub fn add_rule(&mut self, bitmask: u8, tile_id: TileId) {
    self.rules.insert(bitmask, tile_id);
  }

  /// Resolves the tile ID for a given neighbor bitmask.
  ///
  /// Returns the mapped tile if a rule exists, otherwise falls back to
  /// [`default_tile`](AutoTileSet::default_tile).
  pub fn resolve(&self, bitmask: u8) -> TileId {
    self
      .rules
      .get(&bitmask)
      .copied()
      .unwrap_or(self.default_tile)
  }
}

// ── Bitmask computation ─────────────────────────────────────────────────

/// Computes a 4-bit cardinal-only bitmask for the tile at `(x, y)`.
///
/// Checks the four cardinal neighbors (N/E/S/W). A bit is set when the
/// neighbor is in bounds and `is_same(nx, ny)` returns `true`.
///
/// - `w` and `h` are the tilemap dimensions (width and height in tiles).
/// - `is_same` should return `true` when the neighbor at `(nx, ny)` belongs
///   to the same terrain as the tile at `(x, y)`.
pub fn compute_bitmask_minimal(
  x: u32,
  y: u32,
  w: u32,
  h: u32,
  is_same: &dyn Fn(u32, u32) -> bool,
) -> u8 {
  let mut mask: u8 = 0;

  // North (y - 1)
  if y > 0 && is_same(x, y - 1) {
    mask |= NORTH;
  }
  // East (x + 1)
  if x + 1 < w && is_same(x + 1, y) {
    mask |= EAST;
  }
  // South (y + 1)
  if y + 1 < h && is_same(x, y + 1) {
    mask |= SOUTH;
  }
  // West (x - 1)
  if x > 0 && is_same(x - 1, y) {
    mask |= WEST;
  }

  mask
}

/// Computes an 8-bit bitmask with diagonal reduction for the tile at `(x, y)`.
///
/// First checks the four cardinal neighbors (same as [`compute_bitmask_minimal`]).
/// Then checks diagonal neighbors, but a diagonal bit is **only** set when
/// **both** adjacent cardinal neighbors are also set. For example, `NORTH_EAST`
/// is only set if `NORTH` and `EAST` are both set AND the tile at `(x+1, y-1)`
/// matches. This reduces the 256 raw 8-bit combinations to 47 meaningful shapes.
///
/// - `w` and `h` are the tilemap dimensions (width and height in tiles).
/// - `is_same` should return `true` when the neighbor at `(nx, ny)` belongs
///   to the same terrain as the tile at `(x, y)`.
pub fn compute_bitmask_full(
  x: u32,
  y: u32,
  w: u32,
  h: u32,
  is_same: &dyn Fn(u32, u32) -> bool,
) -> u8 {
  // Start with the cardinal bits.
  let mut mask = compute_bitmask_minimal(x, y, w, h, is_same);

  // North-East: requires NORTH and EAST both set.
  if mask & NORTH != 0 && mask & EAST != 0 && y > 0 && x + 1 < w && is_same(x + 1, y - 1) {
    mask |= NORTH_EAST;
  }
  // South-East: requires SOUTH and EAST both set.
  if mask & SOUTH != 0 && mask & EAST != 0 && y + 1 < h && x + 1 < w && is_same(x + 1, y + 1) {
    mask |= SOUTH_EAST;
  }
  // South-West: requires SOUTH and WEST both set.
  if mask & SOUTH != 0 && mask & WEST != 0 && y + 1 < h && x > 0 && is_same(x - 1, y + 1) {
    mask |= SOUTH_WEST;
  }
  // North-West: requires NORTH and WEST both set.
  if mask & NORTH != 0 && mask & WEST != 0 && y > 0 && x > 0 && is_same(x - 1, y - 1) {
    mask |= NORTH_WEST;
  }

  mask
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
  use super::*;

  // Helper: creates an `is_same` closure from a flat bool grid.
  fn grid_checker(grid: &[bool], w: u32) -> impl Fn(u32, u32) -> bool + '_ {
    move |x, y| grid[(y * w + x) as usize]
  }

  // ── Minimal bitmask tests ───────────────────────────────────────────

  #[test]
  fn minimal_bitmask_isolated() {
    // 3x3 grid, only center is set — no neighbors match.
    #[rustfmt::skip]
        let grid = [
            false, false, false,
            false, true,  false,
            false, false, false,
        ];
    let mask = compute_bitmask_minimal(1, 1, 3, 3, &grid_checker(&grid, 3));
    assert_eq!(mask, 0);
  }

  #[test]
  fn minimal_bitmask_surrounded() {
    // 3x3 grid, all cells match — all 4 cardinal bits should be set.
    let grid = [true; 9];
    let mask = compute_bitmask_minimal(1, 1, 3, 3, &grid_checker(&grid, 3));
    assert_eq!(mask, NORTH | EAST | SOUTH | WEST);
  }

  #[test]
  fn minimal_bitmask_top_left_corner() {
    // Tile at (0,0): only south and east neighbors exist and match.
    #[rustfmt::skip]
        let grid = [
            true, true, false,
            true, false, false,
            false, false, false,
        ];
    let mask = compute_bitmask_minimal(0, 0, 3, 3, &grid_checker(&grid, 3));
    assert_eq!(mask, EAST | SOUTH);
  }

  #[test]
  fn minimal_bitmask_north_only() {
    // 3x3 grid, only the cell directly north of center matches.
    #[rustfmt::skip]
        let grid = [
            false, true,  false,
            false, true,  false,
            false, false, false,
        ];
    let mask = compute_bitmask_minimal(1, 1, 3, 3, &grid_checker(&grid, 3));
    assert_eq!(mask, NORTH);
  }

  // ── Full bitmask tests ──────────────────────────────────────────────

  #[test]
  fn full_bitmask_diagonal_requires_cardinals() {
    // NE tile matches, but N and E do NOT match — diagonal bit must NOT be set.
    #[rustfmt::skip]
        let grid = [
            false, false, true,
            false, true,  false,
            false, false, false,
        ];
    let mask = compute_bitmask_full(1, 1, 3, 3, &grid_checker(&grid, 3));
    assert_eq!(mask, 0);
  }

  #[test]
  fn full_bitmask_diagonal_with_cardinals() {
    // N, E, and NE all match — NORTH, EAST, and NORTH_EAST bits should be set.
    #[rustfmt::skip]
        let grid = [
            false, true,  true,
            false, true,  true,
            false, false, false,
        ];
    let mask = compute_bitmask_full(1, 1, 3, 3, &grid_checker(&grid, 3));
    assert_eq!(mask, NORTH | EAST | NORTH_EAST);
  }

  #[test]
  fn full_bitmask_all_neighbors() {
    // All 8 neighbors match — all 8 bits set = 0xFF.
    let grid = [true; 9];
    let mask = compute_bitmask_full(1, 1, 3, 3, &grid_checker(&grid, 3));
    assert_eq!(mask, 0xFF);
  }

  // ── AutoTileSet tests ───────────────────────────────────────────────

  #[test]
  fn autotileset_resolve_with_rule() {
    let mut set = AutoTileSet::new("grass", 1, AutoTileMode::Minimal, 0);
    set.add_rule(NORTH | SOUTH, 42);
    assert_eq!(set.resolve(NORTH | SOUTH), 42);
  }

  #[test]
  fn autotileset_resolve_fallback() {
    let set = AutoTileSet::new("grass", 1, AutoTileMode::Minimal, 99);
    // No rules added — should fall back to default.
    assert_eq!(set.resolve(NORTH | EAST), 99);
  }
}
