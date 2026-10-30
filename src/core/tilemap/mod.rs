//! Tilemaps. A [`TileMap`] holds layers of tiles, objects or an image over
//! one or more [`Tileset`]s, with typed [`Properties`] on every level. Tiles
//! live in dense [`TileGrid`]s whose edits mark chunks dirty. A [`MapRoot`]
//! entity spawns a map's layers, objects and class tiles as its children,
//! read and edited through [`MapsView`] (`ctx.maps()`).

mod edit;
mod grid;
mod ldtk;
mod map;
#[allow(dead_code)] // drawn by the chunk renderer
pub(crate) mod padded;
mod property;
mod spawn;
mod tile;
mod tiled;
mod tileset;
mod view;

pub use edit::LayerRef;
pub use grid::{CHUNK_SIZE, TileGrid};
pub use ldtk::{
  LdtkLevel, LdtkLoader, LdtkProject, Neighbour, NeighbourDirection, WorldLayout, WorldLevel,
};
pub use map::{
  ImageLayer, IntGrid, LayerInfo, LayerKind, MapLayer, MapObjectData, ObjectShape, TileMap,
  TilesetSource,
};
pub use property::{Properties, PropertyValue};
pub(crate) use spawn::spawn_maps;
pub use spawn::{
  IntGridKey, MapEvent, MapIds, MapLayerRef, MapObject, MapRoot, MapTile, TileLayerData,
};
pub use tile::{Tile, TileFlip};
pub use tiled::{TiledMapLoader, TiledTilesetLoader};
pub use tileset::{
  TileData, TileFrame, TileShape, Tileset, TilesetImage, URect, WangColor, WangKind, WangSet,
};
pub use view::MapsView;
