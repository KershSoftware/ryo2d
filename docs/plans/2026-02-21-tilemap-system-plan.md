# Tilemap System Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a full tilemap system to Ryo2D with chunked GPU rendering, collision generation, Tiled JSON import, and auto-tiling.

**Architecture:** Layered tilemap with dense grid storage per layer, 16x16 chunk entities for GPU batch rendering, greedy-merged collision, and bitmask auto-tiling. Integrates via `TilemapManager` on `SceneContext` and renders through existing `BatchCollector`/`SpriteBatch` pipeline.

**Tech Stack:** Rust, hecs ECS, wgpu (via existing SpriteBatch), serde_json (for Tiled .tmj/.tsj import)

**Design Doc:** `docs/plans/2026-02-21-tilemap-system-design.md`

---

## Phase 1: Core Data Model

### Task 1: Tileset and TileId types

**Files:**
- Create: `src/core/tilemap/mod.rs`
- Create: `src/core/tilemap/tileset.rs`
- Modify: `src/core/mod.rs:18` (add `pub mod tilemap;` after particles line)

**Step 1: Create the module structure**

Create `src/core/tilemap/mod.rs`:
```rust
//! Tilemap system for grid-based 2D worlds.
//!
//! Provides efficient chunked rendering, collision generation,
//! Tiled editor import, and auto-tiling support.

pub mod tileset;

pub use tileset::{TileId, TileAnimation, TileProperties, Tileset};
```

**Step 2: Create tileset types with unit tests**

Create `src/core/tilemap/tileset.rs`:
```rust
use std::collections::HashMap;

/// Tile index into a tileset. 0 is the first tile in the atlas.
pub type TileId = u32;

/// A sprite atlas divided into a uniform grid of tiles.
#[derive(Debug, Clone)]
pub struct Tileset {
  /// Name of this tileset (from Tiled or user-defined).
  pub name: String,
  /// Path to the atlas texture image.
  pub image_path: String,
  /// Pixel width of each tile in the atlas.
  pub tile_width: u32,
  /// Pixel height of each tile in the atlas.
  pub tile_height: u32,
  /// Number of tile columns in the atlas.
  pub columns: u32,
  /// Total number of tiles in the atlas.
  pub tile_count: u32,
  /// Pixel spacing between tiles in the atlas.
  pub spacing: u32,
  /// Pixel margin around the atlas edges.
  pub margin: u32,
  /// Full atlas image width in pixels.
  pub image_width: u32,
  /// Full atlas image height in pixels.
  pub image_height: u32,
  /// Per-tile metadata (collision, animation, custom properties).
  pub properties: HashMap<TileId, TileProperties>,
}

/// Metadata for an individual tile in a tileset.
#[derive(Debug, Clone, Default)]
pub struct TileProperties {
  /// Whether this tile generates collision geometry.
  pub solid: bool,
  /// Whether this tile acts as a one-way platform (solid from above only).
  pub platform: bool,
  /// Animation frames for animated tiles.
  pub animation: Option<TileAnimation>,
  /// Custom key-value properties from Tiled.
  pub custom: HashMap<String, String>,
}

/// Animation data for an animated tile.
#[derive(Debug, Clone)]
pub struct TileAnimation {
  /// Sequence of (tile_id, duration_ms) frames.
  pub frames: Vec<(TileId, u32)>,
}

impl Tileset {
  /// Calculate the UV rectangle (0.0-1.0) for a tile by its local ID.
  ///
  /// Returns `(u_min, v_min, u_max, v_max)` or `None` if the ID is out of range.
  pub fn tile_uv(&self, tile_id: TileId) -> Option<(f32, f32, f32, f32)> {
    if tile_id >= self.tile_count {
      return None;
    }
    let col = tile_id % self.columns;
    let row = tile_id / self.columns;
    let px_x = self.margin + col * (self.tile_width + self.spacing);
    let px_y = self.margin + row * (self.tile_height + self.spacing);
    let iw = self.image_width as f32;
    let ih = self.image_height as f32;
    Some((
      px_x as f32 / iw,
      px_y as f32 / ih,
      (px_x + self.tile_width) as f32 / iw,
      (px_y + self.tile_height) as f32 / ih,
    ))
  }

  /// Get properties for a tile, or default if none set.
  pub fn get_properties(&self, tile_id: TileId) -> TileProperties {
    self.properties.get(&tile_id).cloned().unwrap_or_default()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn make_tileset() -> Tileset {
    Tileset {
      name: "test".into(),
      image_path: "test.png".into(),
      tile_width: 16,
      tile_height: 16,
      columns: 4,
      tile_count: 8,
      spacing: 0,
      margin: 0,
      image_width: 64,
      image_height: 32,
      properties: HashMap::new(),
    }
  }

  #[test]
  fn tile_uv_first_tile() {
    let ts = make_tileset();
    let (u0, v0, u1, v1) = ts.tile_uv(0).unwrap();
    assert_eq!((u0, v0, u1, v1), (0.0, 0.0, 0.25, 0.5));
  }

  #[test]
  fn tile_uv_second_tile() {
    let ts = make_tileset();
    let (u0, v0, u1, v1) = ts.tile_uv(1).unwrap();
    assert_eq!((u0, v0, u1, v1), (0.25, 0.0, 0.5, 0.5));
  }

  #[test]
  fn tile_uv_second_row() {
    let ts = make_tileset();
    let (u0, v0, u1, v1) = ts.tile_uv(4).unwrap();
    assert_eq!((u0, v0, u1, v1), (0.0, 0.5, 0.25, 1.0));
  }

  #[test]
  fn tile_uv_out_of_range() {
    let ts = make_tileset();
    assert!(ts.tile_uv(8).is_none());
    assert!(ts.tile_uv(100).is_none());
  }

  #[test]
  fn tile_uv_with_spacing_and_margin() {
    let ts = Tileset {
      name: "spaced".into(),
      image_path: "test.png".into(),
      tile_width: 16,
      tile_height: 16,
      columns: 2,
      tile_count: 4,
      spacing: 2,
      margin: 1,
      image_width: 51, // 1 + 16 + 2 + 16 + 2 + 16 (doesn't matter, just big enough)
      image_height: 51,
      properties: HashMap::new(),
    };
    let (u0, v0, _, _) = ts.tile_uv(0).unwrap();
    // First tile starts at margin (1px)
    assert!((u0 - 1.0 / 51.0).abs() < 0.001);
    assert!((v0 - 1.0 / 51.0).abs() < 0.001);

    let (u0, _, _, _) = ts.tile_uv(1).unwrap();
    // Second tile starts at margin + tile_width + spacing = 1 + 16 + 2 = 19
    assert!((u0 - 19.0 / 51.0).abs() < 0.001);
  }

  #[test]
  fn get_properties_default() {
    let ts = make_tileset();
    let props = ts.get_properties(0);
    assert!(!props.solid);
    assert!(!props.platform);
    assert!(props.animation.is_none());
  }
}
```

**Step 3: Register the module**

In `src/core/mod.rs`, add after line 18 (`pub mod particles;`):
```rust
pub mod tilemap;
```

**Step 4: Run tests**

Run: `cargo test --lib tilemap`
Expected: All 6 tests pass.

**Step 5: Commit**

```
feat(tilemap): add tileset data model with UV calculation
```

---

### Task 2: TilemapLayer and Tilemap types

**Files:**
- Create: `src/core/tilemap/layer.rs`
- Create: `src/core/tilemap/tilemap.rs`
- Modify: `src/core/tilemap/mod.rs`

**Step 1: Create layer types**

Create `src/core/tilemap/layer.rs`:
```rust
use super::TileId;

/// A single grid layer in a tilemap.
#[derive(Debug, Clone)]
pub struct TilemapLayer {
  /// Layer name (from Tiled or user-defined).
  pub name: String,
  /// Dense flat grid of tile IDs. `None` = empty cell.
  /// Indexed as `tiles[y * width + x]`.
  pub tiles: Vec<Option<TileId>>,
  /// Grid width in tiles.
  pub width: u32,
  /// Grid height in tiles.
  pub height: u32,
  /// Index into the parent tilemap's `tilesets` vec.
  pub tileset_index: usize,
  /// Whether this layer is rendered.
  pub visible: bool,
  /// Layer opacity (0.0 = transparent, 1.0 = opaque).
  pub opacity: f32,
  /// Render depth ordering.
  pub z_order: i32,
  /// Tiled parallax scrolling factor (1.0, 1.0 = no parallax).
  pub parallax_x: f32,
  pub parallax_y: f32,
}

impl TilemapLayer {
  /// Create a new empty layer.
  pub fn new(name: impl Into<String>, width: u32, height: u32) -> Self {
    Self {
      name: name.into(),
      tiles: vec![None; (width * height) as usize],
      width,
      height,
      tileset_index: 0,
      visible: true,
      opacity: 1.0,
      z_order: 0,
      parallax_x: 1.0,
      parallax_y: 1.0,
    }
  }

  /// Get the tile at (x, y), or `None` if out of bounds or empty.
  pub fn get_tile(&self, x: u32, y: u32) -> Option<TileId> {
    if x >= self.width || y >= self.height {
      return None;
    }
    self.tiles[(y * self.width + x) as usize]
  }

  /// Set the tile at (x, y). Returns the previous tile, or `None` if out of bounds.
  pub fn set_tile(&mut self, x: u32, y: u32, tile: Option<TileId>) -> Option<Option<TileId>> {
    if x >= self.width || y >= self.height {
      return None;
    }
    let idx = (y * self.width + x) as usize;
    let prev = self.tiles[idx];
    self.tiles[idx] = tile;
    Some(prev)
  }

  /// Check if a position is within the layer bounds.
  pub fn in_bounds(&self, x: u32, y: u32) -> bool {
    x < self.width && y < self.height
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn new_layer_is_empty() {
    let layer = TilemapLayer::new("ground", 10, 10);
    assert_eq!(layer.tiles.len(), 100);
    assert!(layer.tiles.iter().all(|t| t.is_none()));
  }

  #[test]
  fn set_and_get_tile() {
    let mut layer = TilemapLayer::new("ground", 10, 10);
    layer.set_tile(5, 3, Some(42));
    assert_eq!(layer.get_tile(5, 3), Some(42));
  }

  #[test]
  fn get_tile_empty() {
    let layer = TilemapLayer::new("ground", 10, 10);
    assert_eq!(layer.get_tile(0, 0), None);
  }

  #[test]
  fn get_tile_out_of_bounds() {
    let layer = TilemapLayer::new("ground", 10, 10);
    assert_eq!(layer.get_tile(10, 0), None);
    assert_eq!(layer.get_tile(0, 10), None);
    assert_eq!(layer.get_tile(100, 100), None);
  }

  #[test]
  fn set_tile_out_of_bounds() {
    let mut layer = TilemapLayer::new("ground", 10, 10);
    assert!(layer.set_tile(10, 0, Some(1)).is_none());
  }

  #[test]
  fn set_tile_returns_previous() {
    let mut layer = TilemapLayer::new("ground", 10, 10);
    let prev = layer.set_tile(0, 0, Some(1));
    assert_eq!(prev, Some(None)); // was empty
    let prev = layer.set_tile(0, 0, Some(2));
    assert_eq!(prev, Some(Some(1))); // was tile 1
  }
}
```

**Step 2: Create tilemap type**

Create `src/core/tilemap/tilemap.rs`:
```rust
use super::{TileId, TilemapLayer, Tileset};

/// Tilemap orientation (how tiles are arranged spatially).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TilemapOrientation {
  #[default]
  Orthogonal,
  Isometric,
  HexagonalFlat,
  HexagonalPointy,
}

/// A complete tilemap with multiple layers and tilesets.
#[derive(Debug, Clone)]
pub struct Tilemap {
  /// Tilesets referenced by layers.
  pub tilesets: Vec<Tileset>,
  /// Tile layers (ordered bottom to top).
  pub layers: Vec<TilemapLayer>,
  /// Pixel width of each tile in world space.
  pub tile_width: u32,
  /// Pixel height of each tile in world space.
  pub tile_height: u32,
  /// Map width in tiles.
  pub map_width: u32,
  /// Map height in tiles.
  pub map_height: u32,
  /// Tile arrangement orientation.
  pub orientation: TilemapOrientation,
}

impl Tilemap {
  /// Create a new empty tilemap.
  pub fn new(tile_width: u32, tile_height: u32, map_width: u32, map_height: u32) -> Self {
    Self {
      tilesets: Vec::new(),
      layers: Vec::new(),
      tile_width,
      tile_height,
      map_width,
      map_height,
      orientation: TilemapOrientation::Orthogonal,
    }
  }

  /// Total pixel width of the map.
  pub fn pixel_width(&self) -> f32 {
    (self.map_width * self.tile_width) as f32
  }

  /// Total pixel height of the map.
  pub fn pixel_height(&self) -> f32 {
    (self.map_height * self.tile_height) as f32
  }

  /// Convert a world position to tile coordinates.
  /// Returns `None` if out of map bounds.
  pub fn world_to_tile(&self, world_x: f32, world_y: f32) -> Option<(u32, u32)> {
    if world_x < 0.0 || world_y < 0.0 {
      return None;
    }
    let tx = (world_x / self.tile_width as f32) as u32;
    let ty = (world_y / self.tile_height as f32) as u32;
    if tx >= self.map_width || ty >= self.map_height {
      return None;
    }
    Some((tx, ty))
  }

  /// Convert tile coordinates to the world position of the tile's top-left corner.
  pub fn tile_to_world(&self, tile_x: u32, tile_y: u32) -> (f32, f32) {
    (
      (tile_x * self.tile_width) as f32,
      (tile_y * self.tile_height) as f32,
    )
  }

  /// Convert tile coordinates to the world position of the tile's center.
  pub fn tile_to_world_center(&self, tile_x: u32, tile_y: u32) -> (f32, f32) {
    (
      (tile_x * self.tile_width) as f32 + self.tile_width as f32 / 2.0,
      (tile_y * self.tile_height) as f32 + self.tile_height as f32 / 2.0,
    )
  }

  /// Get tile at a world position from a specific layer.
  pub fn tile_at_world(&self, layer_index: usize, world_x: f32, world_y: f32) -> Option<TileId> {
    let (tx, ty) = self.world_to_tile(world_x, world_y)?;
    self.layers.get(layer_index)?.get_tile(tx, ty)
  }

  /// Check if a world position has a solid tile on any layer.
  pub fn is_solid_at(&self, world_x: f32, world_y: f32) -> bool {
    let Some((tx, ty)) = self.world_to_tile(world_x, world_y) else {
      return false;
    };
    for layer in &self.layers {
      if let Some(tile_id) = layer.get_tile(tx, ty) {
        let tileset = &self.tilesets[layer.tileset_index];
        if tileset.get_properties(tile_id).solid {
          return true;
        }
      }
    }
    false
  }

  /// Add a layer and return its index.
  pub fn add_layer(&mut self, layer: TilemapLayer) -> usize {
    let idx = self.layers.len();
    self.layers.push(layer);
    idx
  }

  /// Add a tileset and return its index.
  pub fn add_tileset(&mut self, tileset: Tileset) -> usize {
    let idx = self.tilesets.len();
    self.tilesets.push(tileset);
    idx
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::HashMap;

  fn make_tilemap() -> Tilemap {
    let mut tm = Tilemap::new(16, 16, 10, 10);
    tm.add_tileset(Tileset {
      name: "test".into(),
      image_path: "test.png".into(),
      tile_width: 16,
      tile_height: 16,
      columns: 4,
      tile_count: 16,
      spacing: 0,
      margin: 0,
      image_width: 64,
      image_height: 64,
      properties: HashMap::new(),
    });
    let mut layer = TilemapLayer::new("ground", 10, 10);
    layer.set_tile(5, 5, Some(1));
    tm.add_layer(layer);
    tm
  }

  #[test]
  fn pixel_dimensions() {
    let tm = make_tilemap();
    assert_eq!(tm.pixel_width(), 160.0);
    assert_eq!(tm.pixel_height(), 160.0);
  }

  #[test]
  fn world_to_tile_basic() {
    let tm = make_tilemap();
    assert_eq!(tm.world_to_tile(80.0, 80.0), Some((5, 5)));
    assert_eq!(tm.world_to_tile(0.0, 0.0), Some((0, 0)));
    assert_eq!(tm.world_to_tile(159.0, 159.0), Some((9, 9)));
  }

  #[test]
  fn world_to_tile_out_of_bounds() {
    let tm = make_tilemap();
    assert_eq!(tm.world_to_tile(-1.0, 0.0), None);
    assert_eq!(tm.world_to_tile(0.0, -1.0), None);
    assert_eq!(tm.world_to_tile(160.0, 0.0), None);
  }

  #[test]
  fn tile_to_world_roundtrip() {
    let tm = make_tilemap();
    let (wx, wy) = tm.tile_to_world(5, 3);
    assert_eq!(tm.world_to_tile(wx, wy), Some((5, 3)));
  }

  #[test]
  fn tile_at_world_returns_tile() {
    let tm = make_tilemap();
    assert_eq!(tm.tile_at_world(0, 80.0, 80.0), Some(1));
    assert_eq!(tm.tile_at_world(0, 0.0, 0.0), None); // empty cell
  }

  #[test]
  fn is_solid_at_no_solid_tiles() {
    let tm = make_tilemap();
    // No tiles have solid=true in properties, so nothing is solid
    assert!(!tm.is_solid_at(80.0, 80.0));
  }

  #[test]
  fn is_solid_at_with_solid_tile() {
    let mut tm = make_tilemap();
    tm.tilesets[0].properties.insert(1, super::super::TileProperties {
      solid: true,
      ..Default::default()
    });
    assert!(tm.is_solid_at(80.0, 80.0));
    assert!(!tm.is_solid_at(0.0, 0.0)); // empty cell
  }
}
```

**Step 3: Update mod.rs**

Update `src/core/tilemap/mod.rs`:
```rust
//! Tilemap system for grid-based 2D worlds.
//!
//! Provides efficient chunked rendering, collision generation,
//! Tiled editor import, and auto-tiling support.

pub mod layer;
pub mod tilemap;
pub mod tileset;

pub use layer::TilemapLayer;
pub use tilemap::{Tilemap, TilemapOrientation};
pub use tileset::{TileAnimation, TileId, TileProperties, Tileset};
```

**Step 4: Run tests**

Run: `cargo test --lib tilemap`
Expected: All tests pass (tileset + layer + tilemap).

**Step 5: Commit**

```
feat(tilemap): add TilemapLayer, Tilemap, and coordinate conversion
```

---

## Phase 2: Chunk System

### Task 3: Chunk data structures and subdivision

**Files:**
- Create: `src/core/tilemap/chunk.rs`
- Modify: `src/core/tilemap/mod.rs`

**Step 1: Create chunk types and subdivision logic**

Create `src/core/tilemap/chunk.rs`:
```rust
/// Number of tiles per chunk side.
pub const CHUNK_SIZE: u32 = 16;

/// Identifies a chunk by its grid position and layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkCoord {
  pub chunk_x: u32,
  pub chunk_y: u32,
  pub layer_index: usize,
}

/// Tracks per-chunk state for dirty checking and rendering.
#[derive(Debug, Clone)]
pub struct ChunkState {
  pub coord: ChunkCoord,
  /// Whether this chunk needs its sprite batch rebuilt.
  pub dirty: bool,
  /// Whether this chunk has any non-empty tiles (skip rendering if false).
  pub has_tiles: bool,
}

impl ChunkState {
  pub fn new(coord: ChunkCoord) -> Self {
    Self {
      coord,
      dirty: true,
      has_tiles: false,
    }
  }
}

/// Calculate how many chunks are needed along one axis.
pub fn chunks_along_axis(map_tiles: u32) -> u32 {
  (map_tiles + CHUNK_SIZE - 1) / CHUNK_SIZE
}

/// Calculate the tile range for a chunk along one axis.
/// Returns `(start_tile, end_tile_exclusive)`.
pub fn chunk_tile_range(chunk_index: u32, map_tiles: u32) -> (u32, u32) {
  let start = chunk_index * CHUNK_SIZE;
  let end = (start + CHUNK_SIZE).min(map_tiles);
  (start, end)
}

/// Check if a chunk's world-space AABB is visible within a viewport rect.
pub fn chunk_visible(
  chunk_x: u32,
  chunk_y: u32,
  tile_width: u32,
  tile_height: u32,
  view_min_x: f32,
  view_min_y: f32,
  view_max_x: f32,
  view_max_y: f32,
) -> bool {
  let cmin_x = (chunk_x * CHUNK_SIZE * tile_width) as f32;
  let cmin_y = (chunk_y * CHUNK_SIZE * tile_height) as f32;
  let cmax_x = cmin_x + (CHUNK_SIZE * tile_width) as f32;
  let cmax_y = cmin_y + (CHUNK_SIZE * tile_height) as f32;
  cmax_x > view_min_x && cmin_x < view_max_x && cmax_y > view_min_y && cmin_y < view_max_y
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn chunks_along_axis_exact() {
    assert_eq!(chunks_along_axis(16), 1);
    assert_eq!(chunks_along_axis(32), 2);
    assert_eq!(chunks_along_axis(64), 4);
  }

  #[test]
  fn chunks_along_axis_remainder() {
    assert_eq!(chunks_along_axis(17), 2);
    assert_eq!(chunks_along_axis(33), 3);
    assert_eq!(chunks_along_axis(1), 1);
  }

  #[test]
  fn chunk_tile_range_basic() {
    assert_eq!(chunk_tile_range(0, 100), (0, 16));
    assert_eq!(chunk_tile_range(1, 100), (16, 32));
  }

  #[test]
  fn chunk_tile_range_last_partial() {
    // Map is 20 tiles wide. Chunk 1 covers tiles 16..20 (not 16..32).
    assert_eq!(chunk_tile_range(1, 20), (16, 20));
  }

  #[test]
  fn chunk_visibility_fully_visible() {
    assert!(chunk_visible(0, 0, 16, 16, 0.0, 0.0, 512.0, 512.0));
  }

  #[test]
  fn chunk_visibility_off_screen_right() {
    // Chunk at (10, 0) starts at x=2560. Viewport ends at 512.
    assert!(!chunk_visible(10, 0, 16, 16, 0.0, 0.0, 512.0, 512.0));
  }

  #[test]
  fn chunk_visibility_partially_visible() {
    // Chunk at (1, 0) spans x=256..512. Viewport is 200..300.
    assert!(chunk_visible(1, 0, 16, 16, 200.0, 0.0, 300.0, 512.0));
  }

  #[test]
  fn chunk_visibility_off_screen_left() {
    // Chunk at (0, 0) spans x=0..256. Viewport starts at 300.
    assert!(!chunk_visible(0, 0, 16, 16, 300.0, 0.0, 600.0, 600.0));
  }
}
```

**Step 2: Update mod.rs exports**

Add to `src/core/tilemap/mod.rs`:
```rust
pub mod chunk;

pub use chunk::{ChunkCoord, ChunkState, CHUNK_SIZE, chunk_tile_range, chunk_visible, chunks_along_axis};
```

**Step 3: Run tests**

Run: `cargo test --lib tilemap::chunk`
Expected: All chunk tests pass.

**Step 4: Commit**

```
feat(tilemap): add chunk data structures and visibility culling
```

---

## Phase 3: TilemapManager and Rendering

### Task 4: TilemapManager core

**Files:**
- Create: `src/core/tilemap/manager.rs`
- Modify: `src/core/tilemap/mod.rs`

**Step 1: Create TilemapManager**

Create `src/core/tilemap/manager.rs`. This is the central manager that owns tilemap data, tracks chunks, and handles rendering. It does NOT live on SceneContext yet — that integration comes in Task 6.

```rust
use std::collections::HashMap;

use crate::core::asset::{AssetManager, SpriteHandle};
use crate::core::ecs::World;
use crate::core::graphics::{BatchCollector, SpriteInstance};
use hecs::Entity;

use super::chunk::*;
use super::tilemap::Tilemap;
use super::tileset::Tileset;

/// Internal record for a spawned tilemap.
struct TilemapRecord {
  tilemap: Tilemap,
  /// Loaded texture handles per tileset index.
  tileset_handles: Vec<SpriteHandle>,
  /// Chunk states indexed by ChunkCoord.
  chunks: HashMap<ChunkCoord, ChunkState>,
  /// Object layer data from Tiled import.
  objects: Vec<super::tiled::TiledObject>,
}

/// Manages all active tilemaps — chunk lifecycle, dirty tracking, and rendering.
pub struct TilemapManager {
  tilemaps: HashMap<Entity, TilemapRecord>,
}

impl TilemapManager {
  pub fn new() -> Self {
    Self {
      tilemaps: HashMap::new(),
    }
  }

  /// Spawn a tilemap into the world. Loads tileset textures and creates chunk state.
  /// Returns the tilemap entity.
  pub fn spawn(
    &mut self,
    tilemap: Tilemap,
    world: &mut World,
    assets: &mut AssetManager,
  ) -> Entity {
    self.spawn_with_objects(tilemap, Vec::new(), world, assets)
  }

  /// Spawn a tilemap with associated Tiled objects.
  pub fn spawn_with_objects(
    &mut self,
    tilemap: Tilemap,
    objects: Vec<super::tiled::TiledObject>,
    world: &mut World,
    assets: &mut AssetManager,
  ) -> Entity {
    // Load tileset textures
    let tileset_handles: Vec<SpriteHandle> = tilemap
      .tilesets
      .iter()
      .filter_map(|ts| assets.load_sprite(&ts.image_path).ok())
      .collect();

    // Create chunk states
    let chunks_x = chunks_along_axis(tilemap.map_width);
    let chunks_y = chunks_along_axis(tilemap.map_height);
    let mut chunks = HashMap::new();

    for layer_index in 0..tilemap.layers.len() {
      for cy in 0..chunks_y {
        for cx in 0..chunks_x {
          let coord = ChunkCoord { chunk_x: cx, chunk_y: cy, layer_index };
          let mut state = ChunkState::new(coord);
          // Check if chunk has any tiles
          state.has_tiles = self.chunk_has_tiles(&tilemap, coord);
          chunks.insert(coord, state);
        }
      }
    }

    // Spawn a marker entity
    let entity = world.spawn(());

    self.tilemaps.insert(entity, TilemapRecord {
      tilemap,
      tileset_handles,
      chunks,
      objects,
    });

    entity
  }

  /// Despawn a tilemap and clean up its data.
  pub fn despawn(&mut self, entity: Entity, world: &mut World) {
    self.tilemaps.remove(&entity);
    let _ = world.despawn(entity);
  }

  /// Get a tile from a spawned tilemap.
  pub fn get_tile(&self, entity: Entity, layer: usize, x: u32, y: u32) -> Option<super::TileId> {
    let record = self.tilemaps.get(&entity)?;
    record.tilemap.layers.get(layer)?.get_tile(x, y)
  }

  /// Set a tile in a spawned tilemap. Marks the affected chunk dirty.
  pub fn set_tile(
    &mut self,
    entity: Entity,
    layer: usize,
    x: u32,
    y: u32,
    tile: Option<super::TileId>,
  ) {
    let Some(record) = self.tilemaps.get_mut(&entity) else { return };
    let Some(layer_data) = record.tilemap.layers.get_mut(layer) else { return };
    layer_data.set_tile(x, y, tile);

    // Mark chunk dirty
    let coord = ChunkCoord {
      chunk_x: x / CHUNK_SIZE,
      chunk_y: y / CHUNK_SIZE,
      layer_index: layer,
    };
    if let Some(chunk) = record.chunks.get_mut(&coord) {
      chunk.dirty = true;
      chunk.has_tiles = true; // Conservative — may have tiles now
    }
  }

  /// Convert world position to tile coordinates for a tilemap.
  pub fn world_to_tile(&self, entity: Entity, world_x: f32, world_y: f32) -> Option<(u32, u32)> {
    self.tilemaps.get(&entity)?.tilemap.world_to_tile(world_x, world_y)
  }

  /// Convert tile coordinates to world position (top-left of tile).
  pub fn tile_to_world(&self, entity: Entity, tile_x: u32, tile_y: u32) -> Option<(f32, f32)> {
    let record = self.tilemaps.get(&entity)?;
    Some(record.tilemap.tile_to_world(tile_x, tile_y))
  }

  /// Check if a world position has a solid tile.
  pub fn is_solid(&self, entity: Entity, world_x: f32, world_y: f32) -> bool {
    self.tilemaps
      .get(&entity)
      .map(|r| r.tilemap.is_solid_at(world_x, world_y))
      .unwrap_or(false)
  }

  /// Get object layer data for a tilemap (from Tiled import).
  pub fn objects(&self, entity: Entity) -> &[super::tiled::TiledObject] {
    self.tilemaps
      .get(&entity)
      .map(|r| r.objects.as_slice())
      .unwrap_or(&[])
  }

  /// Render visible tilemap chunks into the batch collector.
  pub fn render(
    &mut self,
    batch_collector: &mut BatchCollector,
    view_min_x: f32,
    view_min_y: f32,
    view_max_x: f32,
    view_max_y: f32,
  ) {
    for record in self.tilemaps.values_mut() {
      let tm = &record.tilemap;
      for chunk_state in record.chunks.values() {
        if !chunk_state.has_tiles {
          continue;
        }
        let coord = chunk_state.coord;
        let layer = &tm.layers[coord.layer_index];
        if !layer.visible {
          continue;
        }
        if !chunk_visible(
          coord.chunk_x, coord.chunk_y,
          tm.tile_width, tm.tile_height,
          view_min_x, view_min_y, view_max_x, view_max_y,
        ) {
          continue;
        }

        // Get tileset handle for this layer
        let Some(handle) = record.tileset_handles.get(layer.tileset_index) else {
          continue;
        };
        let tileset = &tm.tilesets[layer.tileset_index];
        let texture_id = handle.id();

        // Emit sprite instances for each tile in this chunk
        let (start_x, end_x) = chunk_tile_range(coord.chunk_x, tm.map_width);
        let (start_y, end_y) = chunk_tile_range(coord.chunk_y, tm.map_height);

        for ty in start_y..end_y {
          for tx in start_x..end_x {
            let Some(tile_id) = layer.get_tile(tx, ty) else { continue };
            let Some((u0, v0, u1, v1)) = tileset.tile_uv(tile_id) else { continue };

            let world_x = (tx * tm.tile_width) as f32;
            let world_y = (ty * tm.tile_height) as f32;

            let instance = SpriteInstance::new()
              .with_transform(
                world_x, world_y, 0.0,
                tm.tile_width as f32, tm.tile_height as f32,
              )
              .with_uv(u0, v0, u1, v1)
              .with_tint(crate::core::ecs::components::Color::WHITE, layer.opacity);

            batch_collector.add(&texture_id, false, instance, layer.z_order);
          }
        }
      }
    }
  }

  fn chunk_has_tiles(&self, tilemap: &Tilemap, coord: ChunkCoord) -> bool {
    let Some(layer) = tilemap.layers.get(coord.layer_index) else {
      return false;
    };
    let (start_x, end_x) = chunk_tile_range(coord.chunk_x, tilemap.map_width);
    let (start_y, end_y) = chunk_tile_range(coord.chunk_y, tilemap.map_height);
    for ty in start_y..end_y {
      for tx in start_x..end_x {
        if layer.get_tile(tx, ty).is_some() {
          return true;
        }
      }
    }
    false
  }
}

impl Default for TilemapManager {
  fn default() -> Self {
    Self::new()
  }
}
```

**Note:** This references `super::tiled::TiledObject` which doesn't exist yet. For now, create a stub in the next step. The `with_tint` call uses `Color::WHITE` — verify the exact import path matches `crate::core::ecs::components::Color` or adjust based on the actual Color type location.

**Step 2: Update mod.rs**

```rust
pub mod chunk;
pub mod layer;
pub mod manager;
pub mod tilemap;
pub mod tileset;

// Re-exports
pub use chunk::{ChunkCoord, ChunkState, CHUNK_SIZE, chunk_tile_range, chunk_visible, chunks_along_axis};
pub use layer::TilemapLayer;
pub use manager::TilemapManager;
pub use tilemap::{Tilemap, TilemapOrientation};
pub use tileset::{TileAnimation, TileId, TileProperties, Tileset};
```

**Step 3: Run tests**

Run: `cargo test --lib tilemap`
Expected: All previous tests still pass. Manager has no unit tests yet (requires World/Assets — integration tested in Task 6).

**Step 4: Commit**

```
feat(tilemap): add TilemapManager with chunk rendering
```

---

### Task 5: Collision generation with greedy rect merge

**Files:**
- Create: `src/core/tilemap/collision.rs`
- Modify: `src/core/tilemap/mod.rs`

**Step 1: Create collision module with greedy merge algorithm**

Create `src/core/tilemap/collision.rs`:
```rust
use super::tilemap::Tilemap;

/// A merged collision rectangle in tile coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct CollisionRect {
  /// Tile X of the left edge.
  pub x: u32,
  /// Tile Y of the top edge.
  pub y: u32,
  /// Width in tiles.
  pub w: u32,
  /// Height in tiles.
  pub h: u32,
}

impl CollisionRect {
  /// Convert to world-space pixel coordinates given tile dimensions.
  pub fn to_world(&self, tile_width: u32, tile_height: u32) -> (f32, f32, f32, f32) {
    (
      (self.x * tile_width) as f32,
      (self.y * tile_height) as f32,
      (self.w * tile_width) as f32,
      (self.h * tile_height) as f32,
    )
  }
}

/// Generate merged collision rectangles for solid tiles using greedy meshing.
///
/// Scans the tilemap for solid tiles and merges adjacent ones into larger
/// rectangles to minimize the number of collision bodies.
pub fn generate_collision_rects(tilemap: &Tilemap) -> Vec<CollisionRect> {
  let w = tilemap.map_width;
  let h = tilemap.map_height;

  // Build a solid bitmap by checking all layers
  let mut solid = vec![false; (w * h) as usize];
  for layer in &tilemap.layers {
    for ty in 0..h {
      for tx in 0..w {
        if let Some(tile_id) = layer.get_tile(tx, ty) {
          let tileset = &tilemap.tilesets[layer.tileset_index];
          if tileset.get_properties(tile_id).solid {
            solid[(ty * w + tx) as usize] = true;
          }
        }
      }
    }
  }

  greedy_merge(&solid, w, h)
}

/// Generate collision rects for a single chunk region only.
pub fn generate_collision_rects_region(
  tilemap: &Tilemap,
  start_x: u32,
  start_y: u32,
  end_x: u32,
  end_y: u32,
) -> Vec<CollisionRect> {
  let region_w = end_x - start_x;
  let region_h = end_y - start_y;

  let mut solid = vec![false; (region_w * region_h) as usize];
  for layer in &tilemap.layers {
    for ty in start_y..end_y {
      for tx in start_x..end_x {
        if let Some(tile_id) = layer.get_tile(tx, ty) {
          let tileset = &tilemap.tilesets[layer.tileset_index];
          if tileset.get_properties(tile_id).solid {
            let local_x = tx - start_x;
            let local_y = ty - start_y;
            solid[(local_y * region_w + local_x) as usize] = true;
          }
        }
      }
    }
  }

  let mut rects = greedy_merge(&solid, region_w, region_h);
  // Offset rects back to map coordinates
  for rect in &mut rects {
    rect.x += start_x;
    rect.y += start_y;
  }
  rects
}

/// Greedy rectangle merging on a boolean grid.
fn greedy_merge(solid: &[bool], w: u32, h: u32) -> Vec<CollisionRect> {
  let mut visited = vec![false; solid.len()];
  let mut rects = Vec::new();

  for y in 0..h {
    for x in 0..w {
      let idx = (y * w + x) as usize;
      if !solid[idx] || visited[idx] {
        continue;
      }

      // Expand right
      let mut rect_w = 1u32;
      while x + rect_w < w {
        let next_idx = (y * w + x + rect_w) as usize;
        if !solid[next_idx] || visited[next_idx] {
          break;
        }
        rect_w += 1;
      }

      // Expand down
      let mut rect_h = 1u32;
      'expand_down: while y + rect_h < h {
        for dx in 0..rect_w {
          let check_idx = ((y + rect_h) * w + x + dx) as usize;
          if !solid[check_idx] || visited[check_idx] {
            break 'expand_down;
          }
        }
        rect_h += 1;
      }

      // Mark visited
      for dy in 0..rect_h {
        for dx in 0..rect_w {
          visited[((y + dy) * w + x + dx) as usize] = true;
        }
      }

      rects.push(CollisionRect { x, y, w: rect_w, h: rect_h });
    }
  }

  rects
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn greedy_merge_empty() {
    let solid = vec![false; 9];
    let rects = greedy_merge(&solid, 3, 3);
    assert!(rects.is_empty());
  }

  #[test]
  fn greedy_merge_single_tile() {
    let mut solid = vec![false; 9];
    solid[4] = true; // center of 3x3
    let rects = greedy_merge(&solid, 3, 3);
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0], CollisionRect { x: 1, y: 1, w: 1, h: 1 });
  }

  #[test]
  fn greedy_merge_horizontal_run() {
    // Row of 5 solid tiles
    let solid = vec![true; 5];
    let rects = greedy_merge(&solid, 5, 1);
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0], CollisionRect { x: 0, y: 0, w: 5, h: 1 });
  }

  #[test]
  fn greedy_merge_vertical_run() {
    // Column of 5 solid tiles
    let mut solid = vec![false; 5];
    for i in 0..5 {
      solid[i] = true;
    }
    let rects = greedy_merge(&solid, 1, 5);
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0], CollisionRect { x: 0, y: 0, w: 1, h: 5 });
  }

  #[test]
  fn greedy_merge_full_block() {
    let solid = vec![true; 9]; // 3x3 all solid
    let rects = greedy_merge(&solid, 3, 3);
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0], CollisionRect { x: 0, y: 0, w: 3, h: 3 });
  }

  #[test]
  fn greedy_merge_two_separate_tiles() {
    #[rustfmt::skip]
    let solid = vec![
      true,  false, false,
      false, false, false,
      false, false, true,
    ];
    let rects = greedy_merge(&solid, 3, 3);
    assert_eq!(rects.len(), 2);
  }

  #[test]
  fn greedy_merge_l_shape() {
    #[rustfmt::skip]
    let solid = vec![
      true,  false,
      true,  false,
      true,  true,
    ];
    let rects = greedy_merge(&solid, 2, 3);
    // Should produce 2 rects: vertical bar (0,0 1x3) won't work because
    // bottom row has both filled. Greedy goes right-first:
    // (0,0) expands right: only 1 wide. Expand down: rows 0,1 match (only col 0 solid).
    // Row 2 col 0 is solid but col 0 is still only 1 wide... actually row 2 has
    // two solid tiles so the greedy goes: (0,0) w=1, expand down to h=3 (col 0 is solid in all 3 rows)
    // Then (1,2) w=1 h=1.
    assert_eq!(rects.len(), 2);
    assert_eq!(rects[0], CollisionRect { x: 0, y: 0, w: 1, h: 3 });
    assert_eq!(rects[1], CollisionRect { x: 1, y: 2, w: 1, h: 1 });
  }

  #[test]
  fn greedy_merge_ground_plane() {
    // 10-wide ground at the bottom of a 10x5 grid
    let mut solid = vec![false; 50];
    for x in 0..10 {
      solid[4 * 10 + x] = true; // bottom row
    }
    let rects = greedy_merge(&solid, 10, 5);
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0], CollisionRect { x: 0, y: 4, w: 10, h: 1 });
  }

  #[test]
  fn collision_rect_to_world() {
    let rect = CollisionRect { x: 2, y: 3, w: 4, h: 2 };
    let (wx, wy, ww, wh) = rect.to_world(16, 16);
    assert_eq!((wx, wy, ww, wh), (32.0, 48.0, 64.0, 32.0));
  }
}
```

**Step 2: Update mod.rs**

Add to exports in `src/core/tilemap/mod.rs`:
```rust
pub mod collision;

pub use collision::{CollisionRect, generate_collision_rects, generate_collision_rects_region};
```

**Step 3: Run tests**

Run: `cargo test --lib tilemap::collision`
Expected: All collision tests pass.

**Step 4: Commit**

```
feat(tilemap): add greedy rect-merge collision generation
```

---

## Phase 4: Tiled JSON Import

### Task 6: Tiled .tmj/.tsj loader

**Files:**
- Create: `src/core/tilemap/tiled.rs`
- Modify: `src/core/tilemap/mod.rs`

**Step 1: Create Tiled loader**

Create `src/core/tilemap/tiled.rs`. This parses Tiled JSON map files (.tmj) and tileset files (.tsj) into our `Tilemap` types.

```rust
use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::{Tilemap, TilemapLayer, TilemapOrientation, Tileset, TileAnimation, TileId, TileProperties};

/// Errors that can occur during Tiled map loading.
#[derive(Debug)]
pub enum TilemapError {
  Io(std::io::Error),
  Parse(String),
  MissingTileset(String),
  UnsupportedFeature(String),
}

impl std::fmt::Display for TilemapError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Io(e) => write!(f, "Tilemap IO error: {e}"),
      Self::Parse(msg) => write!(f, "Tilemap parse error: {msg}"),
      Self::MissingTileset(name) => write!(f, "Missing tileset: {name}"),
      Self::UnsupportedFeature(feat) => write!(f, "Unsupported tilemap feature: {feat}"),
    }
  }
}

impl std::error::Error for TilemapError {}

impl From<std::io::Error> for TilemapError {
  fn from(e: std::io::Error) -> Self { Self::Io(e) }
}

impl From<serde_json::Error> for TilemapError {
  fn from(e: serde_json::Error) -> Self { Self::Parse(e.to_string()) }
}

/// An object from a Tiled object layer.
#[derive(Debug, Clone)]
pub struct TiledObject {
  pub name: String,
  pub obj_type: String,
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  pub rotation: f32,
  pub shape: TiledShape,
  pub properties: HashMap<String, TiledValue>,
}

/// Shape of a Tiled object.
#[derive(Debug, Clone)]
pub enum TiledShape {
  Rect,
  Ellipse,
  Point,
  Polygon(Vec<(f32, f32)>),
  Polyline(Vec<(f32, f32)>),
}

/// Property value from Tiled.
#[derive(Debug, Clone)]
pub enum TiledValue {
  Bool(bool),
  Int(i64),
  Float(f64),
  String(String),
}

/// Load a Tiled .tmj map file.
pub fn load_tiled_map(path: &str) -> Result<(Tilemap, Vec<TiledObject>), TilemapError> {
  let base_dir = Path::new(path).parent().unwrap_or(Path::new("."));
  let json_str = std::fs::read_to_string(path)?;
  let raw: RawMap = serde_json::from_str(&json_str)?;

  if raw.infinite.unwrap_or(false) {
    return Err(TilemapError::UnsupportedFeature("infinite maps".into()));
  }

  let orientation = match raw.orientation.as_str() {
    "orthogonal" => TilemapOrientation::Orthogonal,
    "isometric" => TilemapOrientation::Isometric,
    "hexagonal" => {
      if raw.stagger_axis.as_deref() == Some("x") {
        TilemapOrientation::HexagonalPointy
      } else {
        TilemapOrientation::HexagonalFlat
      }
    }
    other => return Err(TilemapError::UnsupportedFeature(format!("orientation: {other}"))),
  };

  // Load tilesets
  let mut tilesets = Vec::new();
  let mut first_gids: Vec<u32> = Vec::new();
  for raw_ts in &raw.tilesets {
    let ts = load_tileset(raw_ts, base_dir)?;
    first_gids.push(raw_ts.firstgid);
    tilesets.push(ts);
  }

  // Parse layers
  let mut layers = Vec::new();
  let mut objects = Vec::new();
  let mut z_order = 0i32;

  for raw_layer in &raw.layers {
    match raw_layer.layer_type.as_str() {
      "tilelayer" => {
        if let Some(ref data) = raw_layer.data {
          let mut layer = TilemapLayer::new(&raw_layer.name, raw.width, raw.height);
          layer.visible = raw_layer.visible.unwrap_or(true);
          layer.opacity = raw_layer.opacity.unwrap_or(1.0);
          layer.z_order = z_order;
          layer.parallax_x = raw_layer.parallaxx.unwrap_or(1.0);
          layer.parallax_y = raw_layer.parallaxy.unwrap_or(1.0);

          // Determine which tileset this layer uses (by first non-zero GID)
          let tileset_idx = data.iter()
            .find(|&&gid| gid > 0)
            .map(|&gid| resolve_tileset_index(gid, &first_gids))
            .unwrap_or(0);
          layer.tileset_index = tileset_idx;

          let first_gid = first_gids[tileset_idx];
          for (i, &gid) in data.iter().enumerate() {
            if gid > 0 {
              let local_id = gid - first_gid;
              let x = (i as u32) % raw.width;
              let y = (i as u32) / raw.width;
              layer.set_tile(x, y, Some(local_id));
            }
          }

          layers.push(layer);
          z_order += 1;
        }
      }
      "objectgroup" => {
        if let Some(ref raw_objs) = raw_layer.objects {
          for raw_obj in raw_objs {
            objects.push(parse_object(raw_obj));
          }
        }
      }
      _ => {} // Skip image layers, group layers
    }
  }

  let mut tilemap = Tilemap::new(raw.tilewidth, raw.tileheight, raw.width, raw.height);
  tilemap.orientation = orientation;
  tilemap.tilesets = tilesets;
  tilemap.layers = layers;

  Ok((tilemap, objects))
}

fn resolve_tileset_index(gid: u32, first_gids: &[u32]) -> usize {
  for i in (0..first_gids.len()).rev() {
    if gid >= first_gids[i] {
      return i;
    }
  }
  0
}

fn load_tileset(raw_ts: &RawTilesetRef, base_dir: &Path) -> Result<Tileset, TilemapError> {
  // If the tileset is external (.tsj), load it
  if let Some(ref source) = raw_ts.source {
    let ts_path = base_dir.join(source);
    let ts_json = std::fs::read_to_string(&ts_path)
      .map_err(|_| TilemapError::MissingTileset(source.clone()))?;
    let raw: RawTileset = serde_json::from_str(&ts_json)?;
    return parse_tileset(&raw, ts_path.parent().unwrap_or(base_dir));
  }

  // Inline tileset (embedded in the map file)
  if let Some(ref inline) = raw_ts.inline_data {
    return parse_tileset(inline, base_dir);
  }

  Err(TilemapError::Parse("Tileset has no source or inline data".into()))
}

fn parse_tileset(raw: &RawTileset, base_dir: &Path) -> Result<Tileset, TilemapError> {
  let image_path = raw.image.as_ref()
    .ok_or_else(|| TilemapError::Parse("Tileset has no image".into()))?;

  // Resolve relative image path
  let resolved_image = base_dir.join(image_path).to_string_lossy().into_owned();

  let mut properties = HashMap::new();

  // Parse per-tile properties
  if let Some(ref tiles) = raw.tiles {
    for raw_tile in tiles {
      let mut props = TileProperties::default();

      // Parse custom properties
      if let Some(ref raw_props) = raw_tile.properties {
        for p in raw_props {
          match p.property_type.as_str() {
            "bool" => {
              if p.name == "solid" {
                props.solid = p.value.as_bool().unwrap_or(false);
              } else if p.name == "platform" {
                props.platform = p.value.as_bool().unwrap_or(false);
              }
              props.custom.insert(p.name.clone(), p.value.to_string());
            }
            _ => {
              props.custom.insert(p.name.clone(), p.value.to_string());
            }
          }
        }
      }

      // Parse animation
      if let Some(ref raw_anim) = raw_tile.animation {
        let frames: Vec<(TileId, u32)> = raw_anim
          .iter()
          .map(|f| (f.tileid, f.duration))
          .collect();
        if !frames.is_empty() {
          props.animation = Some(TileAnimation { frames });
        }
      }

      properties.insert(raw_tile.id, props);
    }
  }

  Ok(Tileset {
    name: raw.name.clone().unwrap_or_default(),
    image_path: resolved_image,
    tile_width: raw.tilewidth,
    tile_height: raw.tileheight,
    columns: raw.columns,
    tile_count: raw.tilecount,
    spacing: raw.spacing.unwrap_or(0),
    margin: raw.margin.unwrap_or(0),
    image_width: raw.imagewidth.unwrap_or(0),
    image_height: raw.imageheight.unwrap_or(0),
    properties,
  })
}

fn parse_object(raw: &RawObject) -> TiledObject {
  let shape = if raw.point.unwrap_or(false) {
    TiledShape::Point
  } else if raw.ellipse.unwrap_or(false) {
    TiledShape::Ellipse
  } else if let Some(ref polygon) = raw.polygon {
    TiledShape::Polygon(polygon.iter().map(|p| (p.x, p.y)).collect())
  } else if let Some(ref polyline) = raw.polyline {
    TiledShape::Polyline(polyline.iter().map(|p| (p.x, p.y)).collect())
  } else {
    TiledShape::Rect
  };

  let mut properties = HashMap::new();
  if let Some(ref raw_props) = raw.properties {
    for p in raw_props {
      let value = match p.property_type.as_str() {
        "bool" => TiledValue::Bool(p.value.as_bool().unwrap_or(false)),
        "int" => TiledValue::Int(p.value.as_i64().unwrap_or(0)),
        "float" => TiledValue::Float(p.value.as_f64().unwrap_or(0.0)),
        _ => TiledValue::String(p.value.as_str().unwrap_or("").into()),
      };
      properties.insert(p.name.clone(), value);
    }
  }

  TiledObject {
    name: raw.name.clone().unwrap_or_default(),
    obj_type: raw.obj_type.clone().unwrap_or_default(),
    x: raw.x,
    y: raw.y,
    width: raw.width.unwrap_or(0.0),
    height: raw.height.unwrap_or(0.0),
    rotation: raw.rotation.unwrap_or(0.0),
    shape,
    properties,
  }
}

// --- Raw JSON structures for serde deserialization ---

#[derive(Deserialize)]
struct RawMap {
  width: u32,
  height: u32,
  tilewidth: u32,
  tileheight: u32,
  orientation: String,
  #[serde(default)]
  infinite: Option<bool>,
  #[serde(default)]
  stagger_axis: Option<String>,
  #[serde(default)]
  layers: Vec<RawLayer>,
  #[serde(default)]
  tilesets: Vec<RawTilesetRef>,
}

#[derive(Deserialize)]
struct RawLayer {
  name: String,
  #[serde(rename = "type")]
  layer_type: String,
  #[serde(default)]
  data: Option<Vec<u32>>,
  #[serde(default)]
  visible: Option<bool>,
  #[serde(default)]
  opacity: Option<f32>,
  #[serde(default)]
  parallaxx: Option<f32>,
  #[serde(default)]
  parallaxy: Option<f32>,
  #[serde(default)]
  objects: Option<Vec<RawObject>>,
}

#[derive(Deserialize)]
struct RawTilesetRef {
  firstgid: u32,
  #[serde(default)]
  source: Option<String>,
  // Inline tileset fields (when embedded in the map file)
  #[serde(flatten)]
  inline_data: Option<RawTileset>,
}

#[derive(Deserialize)]
struct RawTileset {
  #[serde(default)]
  name: Option<String>,
  tilewidth: u32,
  tileheight: u32,
  columns: u32,
  tilecount: u32,
  #[serde(default)]
  spacing: Option<u32>,
  #[serde(default)]
  margin: Option<u32>,
  #[serde(default)]
  image: Option<String>,
  #[serde(default)]
  imagewidth: Option<u32>,
  #[serde(default)]
  imageheight: Option<u32>,
  #[serde(default)]
  tiles: Option<Vec<RawTile>>,
}

#[derive(Deserialize)]
struct RawTile {
  id: u32,
  #[serde(default)]
  properties: Option<Vec<RawProperty>>,
  #[serde(default)]
  animation: Option<Vec<RawAnimationFrame>>,
}

#[derive(Deserialize)]
struct RawProperty {
  name: String,
  #[serde(rename = "type")]
  property_type: String,
  value: serde_json::Value,
}

#[derive(Deserialize)]
struct RawAnimationFrame {
  tileid: u32,
  duration: u32,
}

#[derive(Deserialize)]
struct RawObject {
  #[serde(default)]
  name: Option<String>,
  #[serde(rename = "type", default)]
  obj_type: Option<String>,
  x: f32,
  y: f32,
  #[serde(default)]
  width: Option<f32>,
  #[serde(default)]
  height: Option<f32>,
  #[serde(default)]
  rotation: Option<f32>,
  #[serde(default)]
  point: Option<bool>,
  #[serde(default)]
  ellipse: Option<bool>,
  #[serde(default)]
  polygon: Option<Vec<RawPoint>>,
  #[serde(default)]
  polyline: Option<Vec<RawPoint>>,
  #[serde(default)]
  properties: Option<Vec<RawProperty>>,
}

#[derive(Deserialize)]
struct RawPoint {
  x: f32,
  y: f32,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn resolve_tileset_index_single() {
    assert_eq!(resolve_tileset_index(1, &[1]), 0);
    assert_eq!(resolve_tileset_index(5, &[1]), 0);
  }

  #[test]
  fn resolve_tileset_index_multiple() {
    // First tileset starts at GID 1, second at GID 100
    assert_eq!(resolve_tileset_index(1, &[1, 100]), 0);
    assert_eq!(resolve_tileset_index(99, &[1, 100]), 0);
    assert_eq!(resolve_tileset_index(100, &[1, 100]), 1);
    assert_eq!(resolve_tileset_index(150, &[1, 100]), 1);
  }

  #[test]
  fn parse_minimal_map_json() {
    let json = r#"{
      "width": 3, "height": 2,
      "tilewidth": 16, "tileheight": 16,
      "orientation": "orthogonal",
      "layers": [{
        "name": "ground",
        "type": "tilelayer",
        "data": [1, 2, 3, 0, 0, 0]
      }],
      "tilesets": [{
        "firstgid": 1,
        "name": "test",
        "tilewidth": 16, "tileheight": 16,
        "columns": 4, "tilecount": 16,
        "image": "test.png",
        "imagewidth": 64, "imageheight": 64
      }]
    }"#;
    let raw: RawMap = serde_json::from_str(json).unwrap();
    assert_eq!(raw.width, 3);
    assert_eq!(raw.height, 2);
    assert_eq!(raw.layers.len(), 1);
    assert_eq!(raw.layers[0].data.as_ref().unwrap().len(), 6);
  }

  #[test]
  fn parse_object_shapes() {
    let rect_obj = RawObject {
      name: Some("spawn".into()),
      obj_type: Some("spawn_point".into()),
      x: 100.0, y: 200.0,
      width: Some(32.0), height: Some(32.0),
      rotation: None, point: None, ellipse: None,
      polygon: None, polyline: None, properties: None,
    };
    let parsed = parse_object(&rect_obj);
    assert!(matches!(parsed.shape, TiledShape::Rect));
    assert_eq!(parsed.x, 100.0);

    let point_obj = RawObject {
      point: Some(true),
      ..rect_obj
    };
    let parsed = parse_object(&point_obj);
    assert!(matches!(parsed.shape, TiledShape::Point));
  }
}
```

**Step 2: Update mod.rs**

Add to `src/core/tilemap/mod.rs`:
```rust
pub mod tiled;

pub use tiled::{TiledObject, TiledShape, TiledValue, TilemapError, load_tiled_map};
```

**Step 3: Run tests**

Run: `cargo test --lib tilemap::tiled`
Expected: All tiled parser tests pass.

**Step 4: Commit**

```
feat(tilemap): add Tiled JSON (.tmj/.tsj) map loader
```

---

## Phase 5: Engine Integration

### Task 7: Wire TilemapManager into SceneContext and engine render loop

**Files:**
- Modify: `src/core/scene.rs:148` (add `tilemap` field to SceneContext)
- Modify: `src/core/engine.rs` (add TilemapManager field, construct it, pass to SceneContext, call render)
- Modify: `src/lib.rs` (add tilemap prelude exports)

**Step 1: Add tilemap field to SceneContext**

In `src/core/scene.rs`, add to the `SceneContext` struct (after the `events` field around line 188):
```rust
  /// Tilemap manager for loading, spawning, and querying tilemaps
  pub tilemap: &'a mut super::tilemap::TilemapManager,
```

**Step 2: Add TilemapManager to Engine**

In `src/core/engine.rs`, add a field to the Engine struct:
```rust
  tilemap: crate::core::tilemap::TilemapManager,
```

Initialize it in the constructor:
```rust
  tilemap: crate::core::tilemap::TilemapManager::new(),
```

**Step 3: Pass TilemapManager to SceneContext**

In every location where `SceneContext` is constructed (search for `SceneContext {` in engine.rs — there are ~2 locations), add:
```rust
  tilemap: &mut self.tilemap,
```

**Step 4: Call tilemap render in the render path**

In the engine's render method, after batch collection for regular sprites but before the GPU sprite pass executes, add the tilemap render call:
```rust
// Render tilemap chunks into the batch collector
let (view_min_x, view_min_y, view_max_x, view_max_y) = self.camera.visible_rect();
self.tilemap.render(
    &mut self.batch_collector,
    view_min_x, view_min_y, view_max_x, view_max_y,
);
```

**Note:** The exact camera viewport method name may differ — check `Camera` for `visible_rect()`, `viewport()`, `bounds()`, or similar. If none exists, compute from camera position and screen size.

**Step 5: Add prelude exports**

In `src/lib.rs`, add after the particle system exports (line 219):
```rust
  // Tilemap system
  pub use crate::core::tilemap::{
    ChunkCoord, ChunkState, CollisionRect, TileAnimation, TileId, TileProperties,
    Tilemap, TilemapError, TilemapLayer, TilemapManager, TilemapOrientation, Tileset,
    TiledObject, TiledShape, TiledValue, load_tiled_map,
    generate_collision_rects, generate_collision_rects_region,
  };
```

**Step 6: Build check**

Run: `cargo build --lib`
Expected: Compiles without errors. Fix any import path issues.

**Step 7: Run all tests**

Run: `cargo test --lib`
Expected: All existing tests still pass plus all new tilemap tests.

**Step 8: Commit**

```
feat(tilemap): wire TilemapManager into SceneContext and render loop
```

---

### Task 8: Integration test

**Files:**
- Create: `tests/integration/tilemap.rs`

**Step 1: Write integration test**

Create `tests/integration/tilemap.rs`:
```rust
//! Integration tests for the tilemap system.

use ryo2d::prelude::*;
use std::collections::HashMap;

fn make_test_tileset() -> Tileset {
  Tileset {
    name: "test".into(),
    image_path: "test.png".into(),
    tile_width: 16,
    tile_height: 16,
    columns: 4,
    tile_count: 16,
    spacing: 0,
    margin: 0,
    image_width: 64,
    image_height: 64,
    properties: HashMap::new(),
  }
}

#[test]
fn tilemap_create_and_query() {
  let mut tm = Tilemap::new(16, 16, 20, 15);
  tm.add_tileset(make_test_tileset());
  let mut layer = TilemapLayer::new("ground", 20, 15);
  for x in 0..20 {
    layer.set_tile(x, 14, Some(0)); // ground row
  }
  tm.add_layer(layer);

  assert_eq!(tm.pixel_width(), 320.0);
  assert_eq!(tm.pixel_height(), 240.0);
  assert_eq!(tm.tile_at_world(0, 16.0, 224.0), Some(0)); // tile at (1, 14)
  assert_eq!(tm.tile_at_world(0, 0.0, 0.0), None); // empty
}

#[test]
fn tilemap_coordinate_conversion() {
  let tm = Tilemap::new(16, 16, 100, 100);
  assert_eq!(tm.world_to_tile(0.0, 0.0), Some((0, 0)));
  assert_eq!(tm.world_to_tile(16.0, 32.0), Some((1, 2)));
  assert_eq!(tm.world_to_tile(1600.0, 0.0), None); // out of bounds

  let (wx, wy) = tm.tile_to_world(5, 10);
  assert_eq!((wx, wy), (80.0, 160.0));
}

#[test]
fn collision_rects_ground_plane() {
  let mut tm = Tilemap::new(16, 16, 10, 5);
  let mut ts = make_test_tileset();
  ts.properties.insert(0, TileProperties {
    solid: true,
    ..Default::default()
  });
  tm.add_tileset(ts);

  let mut layer = TilemapLayer::new("ground", 10, 5);
  for x in 0..10 {
    layer.set_tile(x, 4, Some(0));
  }
  tm.add_layer(layer);

  let rects = generate_collision_rects(&tm);
  // Should merge into a single rect for the ground row
  assert_eq!(rects.len(), 1);
  assert_eq!(rects[0].x, 0);
  assert_eq!(rects[0].y, 4);
  assert_eq!(rects[0].w, 10);
  assert_eq!(rects[0].h, 1);
}

#[test]
fn is_solid_at_checks_properties() {
  let mut tm = Tilemap::new(16, 16, 5, 5);
  let mut ts = make_test_tileset();
  ts.properties.insert(1, TileProperties {
    solid: true,
    ..Default::default()
  });
  tm.add_tileset(ts);

  let mut layer = TilemapLayer::new("ground", 5, 5);
  layer.set_tile(2, 2, Some(0)); // not solid
  layer.set_tile(3, 3, Some(1)); // solid
  tm.add_layer(layer);

  assert!(!tm.is_solid_at(32.0 + 8.0, 32.0 + 8.0)); // tile (2,2) - not solid
  assert!(tm.is_solid_at(48.0 + 8.0, 48.0 + 8.0));   // tile (3,3) - solid
  assert!(!tm.is_solid_at(0.0, 0.0));                   // empty
}
```

**Step 2: Run integration tests**

Run: `cargo test --test integration tilemap`
Expected: All integration tests pass.

**Step 3: Commit**

```
test(tilemap): add integration tests for tilemap system
```

---

## Phase 6: Auto-Tiling

### Task 9: Bitmask auto-tile system

**Files:**
- Create: `src/core/tilemap/autotile.rs`
- Modify: `src/core/tilemap/mod.rs`

**Step 1: Create auto-tile module**

Create `src/core/tilemap/autotile.rs`:
```rust
use std::collections::HashMap;

use super::TileId;

/// How many neighbors to check for auto-tiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTileMode {
  /// 4-bit bitmask using cardinal neighbors only (up, right, down, left).
  /// Produces 16 possible combinations.
  Minimal,
  /// 8-bit bitmask using cardinal + diagonal neighbors.
  /// With reduction (diagonals only count when both adjacent cardinals match),
  /// produces 47 unique combinations.
  Full,
}

/// A mapping from bitmask value to the tile variant to use.
#[derive(Debug, Clone)]
pub struct AutoTileRule {
  pub bitmask: u8,
  pub tile_id: TileId,
}

/// A set of auto-tile rules for a terrain type.
#[derive(Debug, Clone)]
pub struct AutoTileSet {
  pub name: String,
  pub terrain_id: u16,
  pub mode: AutoTileMode,
  pub default_tile: TileId,
  rules: HashMap<u8, TileId>,
}

impl AutoTileSet {
  /// Create a new auto-tile set.
  pub fn new(name: impl Into<String>, terrain_id: u16, mode: AutoTileMode, default_tile: TileId) -> Self {
    Self {
      name: name.into(),
      terrain_id,
      mode,
      default_tile,
      rules: HashMap::new(),
    }
  }

  /// Add a rule mapping a bitmask to a tile ID.
  pub fn add_rule(&mut self, bitmask: u8, tile_id: TileId) {
    self.rules.insert(bitmask, tile_id);
  }

  /// Look up the tile for a given bitmask, falling back to default.
  pub fn resolve(&self, bitmask: u8) -> TileId {
    self.rules.get(&bitmask).copied().unwrap_or(self.default_tile)
  }
}

/// Cardinal direction bits for minimal (4-bit) auto-tiling.
pub const NORTH: u8 = 0b0001;
pub const EAST: u8 = 0b0010;
pub const SOUTH: u8 = 0b0100;
pub const WEST: u8 = 0b1000;

/// Cardinal + diagonal bits for full (8-bit) auto-tiling.
pub const NORTH_EAST: u8 = 0b0001_0000;
pub const SOUTH_EAST: u8 = 0b0010_0000;
pub const SOUTH_WEST: u8 = 0b0100_0000;
pub const NORTH_WEST: u8 = 0b1000_0000;

/// Compute the 4-bit cardinal bitmask for a tile position.
///
/// `is_same` should return true if the tile at (x, y) belongs to the same terrain.
pub fn compute_bitmask_minimal(x: u32, y: u32, w: u32, h: u32, is_same: &dyn Fn(u32, u32) -> bool) -> u8 {
  let mut mask = 0u8;
  if y > 0 && is_same(x, y - 1) { mask |= NORTH; }
  if x + 1 < w && is_same(x + 1, y) { mask |= EAST; }
  if y + 1 < h && is_same(x, y + 1) { mask |= SOUTH; }
  if x > 0 && is_same(x - 1, y) { mask |= WEST; }
  mask
}

/// Compute the reduced 8-bit bitmask for a tile position.
///
/// Diagonal bits are only set when both adjacent cardinal neighbors match.
/// This reduces 256 combinations to 47 unique tile shapes.
pub fn compute_bitmask_full(x: u32, y: u32, w: u32, h: u32, is_same: &dyn Fn(u32, u32) -> bool) -> u8 {
  let n = y > 0 && is_same(x, y - 1);
  let e = x + 1 < w && is_same(x + 1, y);
  let s = y + 1 < h && is_same(x, y + 1);
  let west = x > 0 && is_same(x - 1, y);

  let mut mask = 0u8;
  if n { mask |= NORTH; }
  if e { mask |= EAST; }
  if s { mask |= SOUTH; }
  if west { mask |= WEST; }

  // Diagonals only count if both adjacent cardinals match
  if n && e && y > 0 && x + 1 < w && is_same(x + 1, y - 1) { mask |= NORTH_EAST; }
  if s && e && y + 1 < h && x + 1 < w && is_same(x + 1, y + 1) { mask |= SOUTH_EAST; }
  if s && west && y + 1 < h && x > 0 && is_same(x - 1, y + 1) { mask |= SOUTH_WEST; }
  if n && west && y > 0 && x > 0 && is_same(x - 1, y - 1) { mask |= NORTH_WEST; }

  mask
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn minimal_bitmask_isolated() {
    let mask = compute_bitmask_minimal(5, 5, 10, 10, &|_, _| false);
    assert_eq!(mask, 0);
  }

  #[test]
  fn minimal_bitmask_surrounded() {
    let mask = compute_bitmask_minimal(5, 5, 10, 10, &|_, _| true);
    assert_eq!(mask, NORTH | EAST | SOUTH | WEST);
  }

  #[test]
  fn minimal_bitmask_top_left_corner() {
    // Only south and east neighbors exist and match
    let mask = compute_bitmask_minimal(0, 0, 10, 10, &|_, _| true);
    assert_eq!(mask, EAST | SOUTH);
  }

  #[test]
  fn minimal_bitmask_north_only() {
    let mask = compute_bitmask_minimal(5, 5, 10, 10, &|x, y| x == 5 && y == 4);
    assert_eq!(mask, NORTH);
  }

  #[test]
  fn full_bitmask_diagonal_requires_cardinals() {
    // Only NE diagonal neighbor matches, but N and E don't — diagonal should NOT be set
    let mask = compute_bitmask_full(5, 5, 10, 10, &|x, y| x == 6 && y == 4);
    assert_eq!(mask, 0); // NE is not a cardinal, so no bit set
  }

  #[test]
  fn full_bitmask_diagonal_with_cardinals() {
    // N, E, and NE all match — diagonal SHOULD be set
    let is_same = |x: u32, y: u32| {
      (x == 5 && y == 4) || // N
      (x == 6 && y == 5) || // E
      (x == 6 && y == 4)    // NE
    };
    let mask = compute_bitmask_full(5, 5, 10, 10, &is_same);
    assert_eq!(mask, NORTH | EAST | NORTH_EAST);
  }

  #[test]
  fn full_bitmask_all_neighbors() {
    let mask = compute_bitmask_full(5, 5, 10, 10, &|_, _| true);
    assert_eq!(mask, 0xFF); // All 8 bits set
  }

  #[test]
  fn autotileset_resolve_with_rule() {
    let mut ats = AutoTileSet::new("ground", 1, AutoTileMode::Minimal, 0);
    ats.add_rule(NORTH | SOUTH, 5);
    assert_eq!(ats.resolve(NORTH | SOUTH), 5);
  }

  #[test]
  fn autotileset_resolve_fallback() {
    let ats = AutoTileSet::new("ground", 1, AutoTileMode::Minimal, 42);
    assert_eq!(ats.resolve(0xFF), 42); // No rule — uses default
  }
}
```

**Step 2: Update mod.rs exports**

Add to `src/core/tilemap/mod.rs`:
```rust
pub mod autotile;

pub use autotile::{AutoTileMode, AutoTileSet, compute_bitmask_full, compute_bitmask_minimal};
```

**Step 3: Run tests**

Run: `cargo test --lib tilemap::autotile`
Expected: All auto-tile tests pass.

**Step 4: Commit**

```
feat(tilemap): add bitmask auto-tile system
```

---

## Phase 7: Demo and Documentation

### Task 10: Tilemap demo example

**Files:**
- Create: `examples/demos/tilemap/main.rs`
- Create: `examples/demos/tilemap/Cargo.toml`

**Step 1: Create demo Cargo.toml**

Create `examples/demos/tilemap/Cargo.toml`:
```toml
[package]
name = "demo-tilemap"
version = "0.1.0"
edition = "2021"

[dependencies]
ryo2d = { path = "../../.." }
```

**Step 2: Create demo main.rs**

Create a simple demo that programmatically creates a tilemap (no Tiled file needed) and renders it. The demo should:
- Create a 20x15 tilemap with 16x16 tiles
- Fill the bottom 2 rows with ground tiles
- Add some platforms
- Show camera panning with arrow keys
- Display tile coordinates on hover

The exact code should be written to match the engine's demo patterns (check existing demos like `examples/demos/lighting/main.rs` for the Scene trait usage pattern).

**Step 3: Add to workspace**

Add to root `Cargo.toml` workspace members:
```toml
"examples/demos/tilemap",
```

**Step 4: Build and run**

Run: `cargo build -p demo-tilemap`
Expected: Compiles successfully.

**Step 5: Commit**

```
feat(tilemap): add tilemap demo example
```

---

### Task 11: AGENTS.md and skill file

**Files:**
- Create: `src/core/tilemap/AGENTS.md`

**Step 1: Create AGENTS.md**

Write architecture documentation for the tilemap module following the pattern of `src/core/particles/AGENTS.md`. Cover:
- Module purpose and architecture
- Key types and their relationships
- Chunk system and rendering flow
- Collision generation
- Tiled import
- Auto-tiling
- Integration points (SceneContext, BatchCollector)

**Step 2: Commit**

```
docs(tilemap): add AGENTS.md architecture guide
```

---

### Task 12: Final verification

**Step 1: Run full test suite**

Run: `cargo test --lib`
Expected: All tests pass (existing + new tilemap tests).

**Step 2: Run integration tests**

Run: `cargo test --test integration`
Expected: All integration tests pass.

**Step 3: Build workspace**

Run: `cargo build --workspace`
Expected: All crates compile, including the new tilemap demo.

**Step 4: Commit any fixes needed**

If any issues found, fix and commit.

---

## Summary

| Phase | Tasks | Description |
|-------|-------|-------------|
| 1: Data Model | 1-2 | Tileset, TilemapLayer, Tilemap types with UV calculation and coordinate conversion |
| 2: Chunks | 3 | Chunk data structures, subdivision math, visibility culling |
| 3: Manager & Rendering | 4 | TilemapManager with spawn/query/render through BatchCollector |
| 4: Collision | 5 | Greedy rect-merge collision generation |
| 5: Tiled Import | 6 | .tmj/.tsj JSON parser with layer/object/property support |
| 6: Engine Integration | 7-8 | Wire into SceneContext, render loop, prelude exports, integration tests |
| 7: Auto-Tiling | 9 | Bitmask computation, rule sets, terrain placement |
| 8: Demo & Docs | 10-12 | Working demo, AGENTS.md, full verification |
