//! Tilemap system for grid-based 2D worlds.
//!
//! Provides efficient chunked rendering, collision generation,
//! Tiled editor import, and auto-tiling support.

pub mod tileset;

pub use tileset::{TileAnimation, TileId, TileProperties, Tileset};
