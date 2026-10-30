//! Tilemaps: the data model. A [`TileMap`] holds layers of tiles, objects or
//! an image over one or more [`Tileset`]s, with typed [`Properties`] on every
//! level. Tiles live in dense [`TileGrid`]s whose edits mark chunks dirty.

mod grid;
mod map;
mod property;
mod tile;
mod tileset;

pub use grid::{CHUNK_SIZE, TileGrid};
pub use map::{
  ImageLayer, IntGrid, LayerInfo, LayerKind, MapLayer, MapObjectData, ObjectShape, TileMap,
  TilesetSource,
};
pub use property::{Properties, PropertyValue};
pub use tile::{Tile, TileFlip};
pub use tileset::{
  TileData, TileFrame, TileShape, Tileset, TilesetImage, URect, WangColor, WangKind, WangSet,
};
