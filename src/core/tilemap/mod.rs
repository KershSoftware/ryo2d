//! Tilemap system for grid-based 2D worlds.
//!
//! Provides efficient chunked rendering, collision generation,
//! Tiled editor import, and auto-tiling support.

pub mod chunk;
pub mod collision;
pub mod layer;
pub mod manager;
pub mod tiled;
pub mod tileset;
pub mod tilemap;

pub use chunk::{ChunkCoord, ChunkState, CHUNK_SIZE, chunk_tile_range, chunk_visible, chunks_along_axis};
pub use collision::{CollisionRect, generate_collision_rects, generate_collision_rects_region};
pub use layer::TilemapLayer;
pub use manager::TilemapManager;
pub use tiled::{TiledObject, TiledShape, TiledValue, TilemapError, load_tiled_map};
pub use tilemap::{Tilemap, TilemapOrientation};
pub use tileset::{TileAnimation, TileId, TileProperties, Tileset};
