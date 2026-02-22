//! Tilemap system for grid-based 2D worlds.
//!
//! Provides efficient chunked rendering, collision generation,
//! Tiled editor import, and auto-tiling support.

pub mod layer;
pub mod tileset;
pub mod tilemap;

pub use layer::TilemapLayer;
pub use tilemap::{Tilemap, TilemapOrientation};
pub use tileset::{TileAnimation, TileId, TileProperties, Tileset};
