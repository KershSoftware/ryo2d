//! Tilemap system for grid-based 2D worlds.
//!
//! Provides efficient chunked rendering, collision generation,
//! Tiled editor import, and auto-tiling support.

pub mod autotile;
pub mod chunk;
pub mod collision;
pub mod layer;
pub mod manager;
pub mod map;
pub mod tiled;
pub mod tileset;

pub use autotile::{AutoTileMode, AutoTileSet, compute_bitmask_full, compute_bitmask_minimal};
pub use chunk::{
  CHUNK_SIZE, ChunkCoord, ChunkState, chunk_tile_range, chunk_visible, chunks_along_axis,
};
pub use collision::{CollisionRect, generate_collision_rects, generate_collision_rects_region};
pub use layer::TilemapLayer;
pub use manager::TilemapManager;
pub use map::{Tilemap, TilemapOrientation};
pub use tiled::{TiledObject, TiledShape, TiledValue, TilemapError, load_tiled_map};
pub use tileset::{TileAnimation, TileId, TileProperties, Tileset};
