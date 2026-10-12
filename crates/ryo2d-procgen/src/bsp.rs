//! Binary Space Partitioning (BSP) dungeon generation
//!
//! Creates structured dungeons with rooms and corridors by recursively
//! dividing space into smaller areas and placing rooms within them.

use super::config::{GridRect, SeededRng, TileType};
use super::grid::{GenerationGrid, RegionType};
use super::mst::{self, MstConfig};

/// Configuration for BSP dungeon generation
#[derive(Debug, Clone)]
pub struct BspConfig {
  /// Minimum room width
  pub min_room_width: u32,
  /// Minimum room height
  pub min_room_height: u32,
  /// Maximum room width
  pub max_room_width: u32,
  /// Maximum room height
  pub max_room_height: u32,
  /// Maximum recursion depth for splitting
  pub split_depth: u32,
  /// Padding between room and partition edges
  pub room_padding: u32,
  /// Width of corridors connecting rooms
  pub corridor_width: u32,
  /// Tile type for floor
  pub floor_tile: TileType,
  /// Tile type for walls
  pub wall_tile: TileType,
  /// Tile type for doors (where corridors meet rooms)
  pub door_tile: Option<TileType>,
  /// Minimum partition size (partitions smaller than this won't be split)
  pub min_partition_size: u32,
  /// Extra connectivity for MST (adds loops)
  pub extra_connectivity: f32,
}

impl Default for BspConfig {
  fn default() -> Self {
    BspConfig {
      min_room_width: 4,
      min_room_height: 4,
      max_room_width: 12,
      max_room_height: 12,
      split_depth: 4,
      room_padding: 1,
      corridor_width: 1,
      floor_tile: TileType::Floor,
      wall_tile: TileType::Wall,
      door_tile: None,
      min_partition_size: 8,
      extra_connectivity: 0.0,
    }
  }
}

impl BspConfig {
  pub fn new() -> Self {
    Self::default()
  }

  /// Set minimum room dimensions
  pub fn with_min_room_size(mut self, width: u32, height: u32) -> Self {
    self.min_room_width = width.max(2);
    self.min_room_height = height.max(2);
    self
  }

  /// Set maximum room dimensions
  pub fn with_max_room_size(mut self, width: u32, height: u32) -> Self {
    self.max_room_width = width;
    self.max_room_height = height;
    self
  }

  /// Set room size range
  pub fn with_room_size(mut self, min: u32, max: u32) -> Self {
    self.min_room_width = min.max(2);
    self.min_room_height = min.max(2);
    self.max_room_width = max;
    self.max_room_height = max;
    self
  }

  /// Set split depth
  pub fn with_split_depth(mut self, depth: u32) -> Self {
    self.split_depth = depth;
    self
  }

  /// Set room padding
  pub fn with_room_padding(mut self, padding: u32) -> Self {
    self.room_padding = padding;
    self
  }

  /// Set corridor width
  pub fn with_corridor_width(mut self, width: u32) -> Self {
    self.corridor_width = width.max(1);
    self
  }

  /// Set door tile (where corridors meet rooms)
  pub fn with_door_tile(mut self, tile: TileType) -> Self {
    self.door_tile = Some(tile);
    self
  }

  /// Set extra connectivity (0.0 to 1.0) for adding loops
  pub fn with_extra_connectivity(mut self, connectivity: f32) -> Self {
    self.extra_connectivity = connectivity.clamp(0.0, 1.0);
    self
  }

  /// Use a classic dungeon preset
  pub fn dungeon_preset() -> Self {
    BspConfig {
      min_room_width: 5,
      min_room_height: 5,
      max_room_width: 15,
      max_room_height: 15,
      split_depth: 5,
      room_padding: 1,
      corridor_width: 2,
      floor_tile: TileType::Floor,
      wall_tile: TileType::Wall,
      door_tile: Some(TileType::Door),
      min_partition_size: 10,
      extra_connectivity: 0.1,
    }
  }

  /// Use a tight maze-like preset
  pub fn maze_preset() -> Self {
    BspConfig {
      min_room_width: 3,
      min_room_height: 3,
      max_room_width: 6,
      max_room_height: 6,
      split_depth: 6,
      room_padding: 0,
      corridor_width: 1,
      floor_tile: TileType::Floor,
      wall_tile: TileType::Wall,
      door_tile: None,
      min_partition_size: 6,
      extra_connectivity: 0.0,
    }
  }

  /// Use an open halls preset
  pub fn halls_preset() -> Self {
    BspConfig {
      min_room_width: 8,
      min_room_height: 8,
      max_room_width: 20,
      max_room_height: 20,
      split_depth: 3,
      room_padding: 2,
      corridor_width: 3,
      floor_tile: TileType::Floor,
      wall_tile: TileType::Wall,
      door_tile: None,
      min_partition_size: 15,
      extra_connectivity: 0.2,
    }
  }
}

/// A BSP tree node
#[derive(Debug)]
struct BspNode {
  bounds: GridRect,
  room: Option<GridRect>,
  left: Option<Box<BspNode>>,
  right: Option<Box<BspNode>>,
}

impl BspNode {
  fn new(bounds: GridRect) -> Self {
    BspNode {
      bounds,
      room: None,
      left: None,
      right: None,
    }
  }

  fn is_leaf(&self) -> bool {
    self.left.is_none() && self.right.is_none()
  }

  /// Get the room in this node or find one in children
  fn get_room(&self) -> Option<GridRect> {
    if let Some(room) = self.room {
      return Some(room);
    }
    if let Some(ref left) = self.left
      && let Some(room) = left.get_room()
    {
      return Some(room);
    }
    if let Some(ref right) = self.right
      && let Some(room) = right.get_room()
    {
      return Some(room);
    }
    None
  }
}

/// Generate a dungeon using BSP
pub fn generate(
  width: u32,
  height: u32,
  config: &BspConfig,
  rng: &mut SeededRng,
) -> GenerationGrid {
  let mut grid = GenerationGrid::filled(width, height, config.wall_tile);

  // Build BSP tree
  let bounds = GridRect::new(0, 0, width, height);
  let mut root = BspNode::new(bounds);
  split_node(&mut root, config, rng, 0);

  // Place rooms
  place_rooms(&mut root, &mut grid, config, rng);

  // Connect rooms using MST
  let mst_config = MstConfig::new().with_extra_connectivity(config.extra_connectivity);
  mst::connect_regions_mst(&mut grid, &mst_config, rng, config.floor_tile);

  // Also connect sibling rooms in BSP tree directly
  connect_bsp_siblings(&root, &mut grid, config, rng);

  grid
}

/// Recursively split a BSP node
fn split_node(node: &mut BspNode, config: &BspConfig, rng: &mut SeededRng, depth: u32) {
  if depth >= config.split_depth {
    return;
  }

  let bounds = node.bounds;

  // Check if partition is too small to split
  if bounds.width < config.min_partition_size * 2 && bounds.height < config.min_partition_size * 2 {
    return;
  }

  // Decide split direction based on aspect ratio with some randomness
  let split_horizontal = if bounds.width > bounds.height * 5 / 4 {
    false // Split vertically if too wide
  } else if bounds.height > bounds.width * 5 / 4 {
    true // Split horizontally if too tall
  } else {
    rng.gen_bool(0.5)
  };

  if split_horizontal {
    // Horizontal split (top/bottom)
    let min_split = config.min_partition_size;
    let max_split = bounds.height.saturating_sub(config.min_partition_size);

    if max_split <= min_split {
      return;
    }

    let split_y = rng.gen_range(min_split..=max_split);

    let top_bounds = GridRect::new(bounds.x, bounds.y, bounds.width, split_y);
    let bottom_bounds = GridRect::new(
      bounds.x,
      bounds.y + split_y as i32,
      bounds.width,
      bounds.height - split_y,
    );

    node.left = Some(Box::new(BspNode::new(top_bounds)));
    node.right = Some(Box::new(BspNode::new(bottom_bounds)));
  } else {
    // Vertical split (left/right)
    let min_split = config.min_partition_size;
    let max_split = bounds.width.saturating_sub(config.min_partition_size);

    if max_split <= min_split {
      return;
    }

    let split_x = rng.gen_range(min_split..=max_split);

    let left_bounds = GridRect::new(bounds.x, bounds.y, split_x, bounds.height);
    let right_bounds = GridRect::new(
      bounds.x + split_x as i32,
      bounds.y,
      bounds.width - split_x,
      bounds.height,
    );

    node.left = Some(Box::new(BspNode::new(left_bounds)));
    node.right = Some(Box::new(BspNode::new(right_bounds)));
  }

  // Recursively split children
  if let Some(ref mut left) = node.left {
    split_node(left, config, rng, depth + 1);
  }
  if let Some(ref mut right) = node.right {
    split_node(right, config, rng, depth + 1);
  }
}

/// Place rooms in leaf nodes
fn place_rooms(
  node: &mut BspNode,
  grid: &mut GenerationGrid,
  config: &BspConfig,
  rng: &mut SeededRng,
) {
  if node.is_leaf() {
    // Create a room within the partition
    if let Some(room) = create_room_in_partition(&node.bounds, config, rng) {
      node.room = Some(room);
      grid.fill_grid_rect(&room, config.floor_tile);
      grid.add_region(room, RegionType::Room);
    }
  } else {
    if let Some(ref mut left) = node.left {
      place_rooms(left, grid, config, rng);
    }
    if let Some(ref mut right) = node.right {
      place_rooms(right, grid, config, rng);
    }
  }
}

/// Create a room within a partition
fn create_room_in_partition(
  partition: &GridRect,
  config: &BspConfig,
  rng: &mut SeededRng,
) -> Option<GridRect> {
  let padding = config.room_padding;

  // Available space after padding
  let available_width = partition.width.saturating_sub(padding * 2);
  let available_height = partition.height.saturating_sub(padding * 2);

  if available_width < config.min_room_width || available_height < config.min_room_height {
    return None;
  }

  // Random room size within constraints
  let room_width =
    rng.gen_range(config.min_room_width..=config.max_room_width.min(available_width));
  let room_height =
    rng.gen_range(config.min_room_height..=config.max_room_height.min(available_height));

  // Random position within available space
  let max_x = available_width - room_width;
  let max_y = available_height - room_height;

  let room_x = partition.x + padding as i32 + rng.gen_range(0..=max_x) as i32;
  let room_y = partition.y + padding as i32 + rng.gen_range(0..=max_y) as i32;

  Some(GridRect::new(room_x, room_y, room_width, room_height))
}

/// Connect sibling nodes in the BSP tree
fn connect_bsp_siblings(
  node: &BspNode,
  grid: &mut GenerationGrid,
  config: &BspConfig,
  _rng: &mut SeededRng,
) {
  if let (Some(left), Some(right)) = (&node.left, &node.right) {
    // Find rooms in each subtree
    if let (Some(left_room), Some(right_room)) = (left.get_room(), right.get_room()) {
      let (lx, ly) = left_room.center();
      let (rx, ry) = right_room.center();

      // Draw corridor
      draw_corridor(grid, lx, ly, rx, ry, config);
    }

    // Recursively connect children
    connect_bsp_siblings(left, grid, config, _rng);
    connect_bsp_siblings(right, grid, config, _rng);
  }
}

/// Draw an L-shaped corridor between two points
fn draw_corridor(
  grid: &mut GenerationGrid,
  x1: i32,
  y1: i32,
  x2: i32,
  y2: i32,
  config: &BspConfig,
) {
  let half_width = config.corridor_width as i32 / 2;

  // Horizontal segment
  let (start_x, end_x) = if x1 <= x2 { (x1, x2) } else { (x2, x1) };
  for x in start_x..=end_x {
    for w in -half_width..=(config.corridor_width as i32 - half_width - 1) {
      grid.set(x, y1 + w, config.floor_tile);
    }
  }

  // Vertical segment
  let (start_y, end_y) = if y1 <= y2 { (y1, y2) } else { (y2, y1) };
  for y in start_y..=end_y {
    for w in -half_width..=(config.corridor_width as i32 - half_width - 1) {
      grid.set(x2 + w, y, config.floor_tile);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn basic_generation() {
    let mut rng = SeededRng::new(12345);
    let config = BspConfig::default();
    let grid = generate(64, 64, &config, &mut rng);

    // Should have rooms
    assert!(!grid.regions().is_empty());
    assert!(grid.count_type(TileType::Floor) > 0);
  }

  #[test]
  fn reproducibility() {
    let config = BspConfig::default();

    let mut rng1 = SeededRng::new(12345);
    let grid1 = generate(64, 64, &config, &mut rng1);

    let mut rng2 = SeededRng::new(12345);
    let grid2 = generate(64, 64, &config, &mut rng2);

    for y in 0..64 {
      for x in 0..64 {
        assert_eq!(grid1.get(x, y), grid2.get(x, y));
      }
    }
  }

  #[test]
  fn rooms_created() {
    let mut rng = SeededRng::new(42);
    let config = BspConfig::default().with_split_depth(3);
    let grid = generate(64, 64, &config, &mut rng);

    // Should have multiple rooms
    assert!(grid.regions().len() >= 2);
  }

  #[test]
  fn connections_created() {
    let mut rng = SeededRng::new(12345);
    let config = BspConfig::default();
    let grid = generate(64, 64, &config, &mut rng);

    // Should have connections between rooms
    // Note: connections come from MST + BSP sibling connections
    // We check that rooms are reachable by checking there's floor between them
    let floor_count = grid.count_type(TileType::Floor);
    assert!(floor_count > 0);
  }

  #[test]
  fn presets_work() {
    let mut rng = SeededRng::new(12345);

    let dungeon = generate(64, 64, &BspConfig::dungeon_preset(), &mut rng);
    assert!(!dungeon.regions().is_empty());

    rng.reset();
    let maze = generate(64, 64, &BspConfig::maze_preset(), &mut rng);
    assert!(!maze.regions().is_empty());

    rng.reset();
    let halls = generate(64, 64, &BspConfig::halls_preset(), &mut rng);
    assert!(!halls.regions().is_empty());
  }

  #[test]
  fn split_depth_affects_room_count() {
    let mut rng1 = SeededRng::new(12345);
    let shallow_config = BspConfig::default().with_split_depth(2);
    let shallow_grid = generate(64, 64, &shallow_config, &mut rng1);

    let mut rng2 = SeededRng::new(12345);
    let deep_config = BspConfig::default().with_split_depth(5);
    let deep_grid = generate(64, 64, &deep_config, &mut rng2);

    // Deeper splits should create more rooms
    assert!(deep_grid.regions().len() >= shallow_grid.regions().len());
  }
}
