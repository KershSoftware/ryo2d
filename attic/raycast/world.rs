//! Map representation for the raycasting system.
//!
//! Provides [`MapCell`] for individual map tiles and [`RaycastWorld`] for the
//! complete map grid. Data is packed into GPU-friendly u32 arrays using f16
//! for compact storage of height and lighting values.

use bitflags::bitflags;
use half::f16;

bitflags! {
    /// Per-cell flags that modify raycasting behaviour.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MapFlags: u8 {
        /// No special behaviour.
        const NONE        = 0b0000_0000;
        /// Cell contains a door.
        const DOOR        = 0b0000_0001;
        /// Thin wall (rendered as a plane at the cell midpoint).
        const THIN_WALL   = 0b0000_0010;
        /// Portal to another location in the map. **Not yet implemented.**
        const PORTAL      = 0b0000_0100;
        /// Wall is transparent / see-through. **Not yet implemented.**
        const TRANSPARENT = 0b0000_1000;
    }
}

impl Default for MapFlags {
  fn default() -> Self {
    MapFlags::NONE
  }
}

/// A single cell in the raycasting map grid.
///
/// ## GPU pack layout
///
/// The cell is packed into 3 x u32 for upload to the GPU:
///
/// | Word | Bits 31..24 | Bits 23..16 | Bits 15..8 | Bits 7..0 |
/// |------|-------------|-------------|------------|-----------|
/// | [0]  | wall_texture | floor_texture | ceiling_texture | flags |
/// | [1]  | floor_height (f16, upper 16) | ceiling_height (f16, lower 16) |
/// | [2]  | door_state (f16, upper 16) | light_level (f16, lower 16) |
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapCell {
  /// Wall texture index (0 = no wall / air).
  pub wall_texture: u8,
  /// Floor texture index.
  pub floor_texture: u8,
  /// Ceiling texture index.
  pub ceiling_texture: u8,
  /// Per-cell flags.
  pub flags: MapFlags,
  /// Floor height in world units (default 0.0).
  pub floor_height: f32,
  /// Ceiling height in world units (default 1.0).
  pub ceiling_height: f32,
  /// Door openness: 0.0 = closed, 1.0 = fully open.
  pub door_state: f32,
  /// Light level: 0.0 = dark, 1.0 = fully lit.
  pub light_level: f32,
}

impl Default for MapCell {
  fn default() -> Self {
    Self {
      wall_texture: 0,
      floor_texture: 0,
      ceiling_texture: 0,
      flags: MapFlags::NONE,
      floor_height: 0.0,
      ceiling_height: 1.0,
      door_state: 0.0,
      light_level: 1.0,
    }
  }
}

impl MapCell {
  /// Create an empty (air) cell with default heights and full lighting.
  pub fn empty() -> Self {
    Self::default()
  }

  /// Create a solid wall cell with the given texture index.
  pub fn wall(texture: u8) -> Self {
    Self {
      wall_texture: texture,
      ..Self::default()
    }
  }

  /// Pack this cell into 3 x u32 for GPU upload.
  ///
  /// See the struct-level docs for the bit layout.
  /// **GPU counterpart:** `read_cell()` and `get_*()` helpers in `shaders/core/raycast.wgsl`.
  pub fn pack(&self) -> [u32; 3] {
    let word0 = (self.wall_texture as u32) << 24
      | (self.floor_texture as u32) << 16
      | (self.ceiling_texture as u32) << 8
      | (self.flags.bits() as u32);

    let floor_h = f16::from_f32(self.floor_height).to_bits() as u32;
    let ceil_h = f16::from_f32(self.ceiling_height).to_bits() as u32;
    let word1 = (floor_h << 16) | ceil_h;

    let door = f16::from_f32(self.door_state).to_bits() as u32;
    let light = f16::from_f32(self.light_level).to_bits() as u32;
    let word2 = (door << 16) | light;

    [word0, word1, word2]
  }

  /// Unpack a cell from 3 x u32 (inverse of [`pack`](Self::pack)).
  pub fn unpack(data: [u32; 3]) -> Self {
    let wall_texture = ((data[0] >> 24) & 0xFF) as u8;
    let floor_texture = ((data[0] >> 16) & 0xFF) as u8;
    let ceiling_texture = ((data[0] >> 8) & 0xFF) as u8;
    let flags = MapFlags::from_bits_truncate((data[0] & 0xFF) as u8);

    let floor_height = f16::from_bits(((data[1] >> 16) & 0xFFFF) as u16).to_f32();
    let ceiling_height = f16::from_bits((data[1] & 0xFFFF) as u16).to_f32();

    let door_state = f16::from_bits(((data[2] >> 16) & 0xFFFF) as u16).to_f32();
    let light_level = f16::from_bits((data[2] & 0xFFFF) as u16).to_f32();

    Self {
      wall_texture,
      floor_texture,
      ceiling_texture,
      flags,
      floor_height,
      ceiling_height,
      door_state,
      light_level,
    }
  }
}

/// Describes a portal linking two map cells. **Not yet implemented** in the shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Portal {
  /// Source cell (column, row).
  pub from: (usize, usize),
  /// Destination cell (column, row).
  pub to: (usize, usize),
}

/// Grid-based map used by the raycasting renderer.
///
/// Stores a flat array of [`MapCell`]s in row-major order and tracks whether
/// the map has been modified since the last GPU upload.
pub struct RaycastWorld {
  cells: Vec<MapCell>,
  width: usize,
  height: usize,
  portals: Vec<Portal>,
  dirty: bool,
}

impl RaycastWorld {
  /// Create a new world filled with empty cells.
  pub fn new(width: usize, height: usize) -> Self {
    Self {
      cells: vec![MapCell::default(); width * height],
      width,
      height,
      portals: Vec::new(),
      dirty: true,
    }
  }

  /// Build a world from a grid of wall texture indices.
  ///
  /// Each inner `Vec<u8>` is one *row*. A value of 0 produces an empty
  /// cell; any other value produces a wall with that texture index.
  pub fn from_grid(grid: &[Vec<u8>]) -> Self {
    let height = grid.len();
    assert!(height > 0, "Grid must have at least one row");
    let width = grid[0].len();
    assert!(width > 0, "Grid must have at least one column");
    for (i, row) in grid.iter().enumerate() {
      assert_eq!(
        row.len(),
        width,
        "Row {} has length {}, expected {}",
        i,
        row.len(),
        width
      );
    }
    let mut cells = Vec::with_capacity(width * height);
    for row in grid {
      for &tex in row {
        if tex == 0 {
          cells.push(MapCell::empty());
        } else {
          cells.push(MapCell::wall(tex));
        }
      }
    }
    Self {
      cells,
      width,
      height,
      portals: Vec::new(),
      dirty: true,
    }
  }

  /// Map width in cells.
  pub fn width(&self) -> usize {
    self.width
  }

  /// Map height in cells.
  pub fn height(&self) -> usize {
    self.height
  }

  /// Get an immutable reference to a cell, or `None` if out of bounds.
  pub fn get(&self, x: usize, y: usize) -> Option<&MapCell> {
    if x < self.width && y < self.height {
      Some(&self.cells[y * self.width + x])
    } else {
      None
    }
  }

  /// Get a mutable reference to a cell, or `None` if out of bounds.
  ///
  /// Marks the world as dirty.
  pub fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut MapCell> {
    if x < self.width && y < self.height {
      self.dirty = true;
      Some(&mut self.cells[y * self.width + x])
    } else {
      None
    }
  }

  /// Set a cell at the given position. Returns `false` if out of bounds.
  pub fn set(&mut self, x: usize, y: usize, cell: MapCell) -> bool {
    if x < self.width && y < self.height {
      self.cells[y * self.width + x] = cell;
      self.dirty = true;
      true
    } else {
      false
    }
  }

  /// Pack the entire map into a flat `Vec<u32>` for GPU upload.
  ///
  /// Each cell occupies 3 consecutive u32s. Total length = width * height * 3.
  pub fn pack_for_gpu(&self) -> Vec<u32> {
    let mut buf = Vec::with_capacity(self.cells.len() * 3);
    for cell in &self.cells {
      let packed = cell.pack();
      buf.extend_from_slice(&packed);
    }
    buf
  }

  /// Register a portal linking two cells.
  pub fn add_portal(&mut self, from: (usize, usize), to: (usize, usize)) {
    self.portals.push(Portal { from, to });
    self.dirty = true;
  }

  /// Return the current list of portals.
  pub fn portals(&self) -> &[Portal] {
    &self.portals
  }

  /// Whether the map has been modified since the last [`clear_dirty`](Self::clear_dirty) call.
  pub fn is_dirty(&self) -> bool {
    self.dirty
  }

  /// Clear the dirty flag (typically after uploading to the GPU).
  pub fn clear_dirty(&mut self) {
    self.dirty = false;
  }

  /// Generate lightmap data for GPU upload as R8Unorm texture.
  /// Returns a Vec<u8> of width x height values.
  pub fn generate_lightmap(&self) -> Vec<u8> {
    self
      .cells
      .iter()
      .map(|cell| (cell.light_level.clamp(0.0, 1.0) * 255.0) as u8)
      .collect()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn pack_unpack_roundtrip() {
    let cell = MapCell {
      wall_texture: 42,
      floor_texture: 7,
      ceiling_texture: 13,
      flags: MapFlags::DOOR | MapFlags::THIN_WALL,
      floor_height: 0.25,
      ceiling_height: 0.75,
      door_state: 0.5,
      light_level: 0.8,
    };

    let packed = cell.pack();
    let unpacked = MapCell::unpack(packed);

    assert_eq!(unpacked.wall_texture, cell.wall_texture);
    assert_eq!(unpacked.floor_texture, cell.floor_texture);
    assert_eq!(unpacked.ceiling_texture, cell.ceiling_texture);
    assert_eq!(unpacked.flags, cell.flags);

    // f16 precision: values should be very close but not necessarily bit-identical
    assert!((unpacked.floor_height - cell.floor_height).abs() < 0.001);
    assert!((unpacked.ceiling_height - cell.ceiling_height).abs() < 0.001);
    assert!((unpacked.door_state - cell.door_state).abs() < 0.001);
    assert!((unpacked.light_level - cell.light_level).abs() < 0.01);
  }

  #[test]
  fn empty_cell() {
    let cell = MapCell::empty();
    assert_eq!(cell.wall_texture, 0);
    assert_eq!(cell.floor_texture, 0);
    assert_eq!(cell.ceiling_texture, 0);
    assert_eq!(cell.flags, MapFlags::NONE);
    assert_eq!(cell.floor_height, 0.0);
    assert_eq!(cell.ceiling_height, 1.0);
    assert_eq!(cell.door_state, 0.0);
    assert_eq!(cell.light_level, 1.0);
  }

  #[test]
  fn wall_cell() {
    let cell = MapCell::wall(5);
    assert_eq!(cell.wall_texture, 5);
    assert_eq!(cell.floor_texture, 0);
    assert_eq!(cell.ceiling_height, 1.0);
    assert_eq!(cell.light_level, 1.0);
  }

  #[test]
  fn world_from_grid() {
    let grid = vec![vec![1, 1, 1], vec![1, 0, 1], vec![1, 1, 1]];
    let world = RaycastWorld::from_grid(&grid);
    assert_eq!(world.width(), 3);
    assert_eq!(world.height(), 3);

    // Centre cell should be empty
    let centre = world.get(1, 1).unwrap();
    assert_eq!(centre.wall_texture, 0);

    // Corner cell should be a wall
    let corner = world.get(0, 0).unwrap();
    assert_eq!(corner.wall_texture, 1);
  }

  #[test]
  fn world_get_set() {
    let mut world = RaycastWorld::new(4, 4);
    world.clear_dirty();
    assert!(!world.is_dirty());

    let cell = MapCell::wall(10);
    assert!(world.set(2, 3, cell));
    assert!(world.is_dirty());

    let retrieved = world.get(2, 3).unwrap();
    assert_eq!(retrieved.wall_texture, 10);
  }

  #[test]
  fn world_out_of_bounds() {
    let mut world = RaycastWorld::new(4, 4);

    assert!(world.get(4, 0).is_none());
    assert!(world.get(0, 4).is_none());
    assert!(world.get_mut(4, 0).is_none());
    assert!(!world.set(4, 0, MapCell::empty()));
  }

  #[test]
  fn world_pack_for_gpu() {
    let mut world = RaycastWorld::new(2, 2);
    world.set(0, 0, MapCell::wall(1));
    world.set(1, 0, MapCell::wall(2));

    let packed = world.pack_for_gpu();
    // 2x2 = 4 cells x 3 u32s each = 12
    assert_eq!(packed.len(), 12);

    // First cell word0 should encode wall_texture=1 in the top byte
    assert_eq!((packed[0] >> 24) & 0xFF, 1);
    // Second cell (offset 3) should encode wall_texture=2
    assert_eq!((packed[3] >> 24) & 0xFF, 2);
  }

  #[test]
  fn world_portals() {
    let mut world = RaycastWorld::new(8, 8);
    world.clear_dirty();

    world.add_portal((1, 2), (5, 6));
    assert!(world.is_dirty());
    assert_eq!(world.portals().len(), 1);
    assert_eq!(world.portals()[0].from, (1, 2));
    assert_eq!(world.portals()[0].to, (5, 6));
  }
}
