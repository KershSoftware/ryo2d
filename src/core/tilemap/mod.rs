//! Tilemaps. A [`TileMap`] holds layers of tiles, objects or an image over
//! one or more [`Tileset`]s, with typed [`Properties`] on every level. Tiles
//! live in dense [`TileGrid`]s whose edits mark chunks dirty. A [`MapRoot`]
//! entity spawns a map's layers, objects and class tiles as its children,
//! read and edited through [`MapsView`] (`ctx.maps()`).

mod collision;
mod edit;
mod grid;
mod jobs;
mod kinds;
mod ldtk;
mod map;
mod occluders;
pub(crate) mod padded;
mod property;
mod spawn;
#[cfg(test)]
pub(crate) mod test_util;
mod tile;
mod tiled;
mod tileset;
mod view;

use crate::core::ecs::IntoScheduleConfigs;
use crate::core::schedule::{EngineSchedules, FixedSet, Phase, PostUpdateSet};

pub use edit::LayerRef;
pub use grid::{CHUNK_SIZE, TileGrid};
pub use ldtk::{
  LdtkLevel, LdtkLoader, LdtkProject, Neighbour, NeighbourDirection, WorldLayout, WorldLevel,
};
pub use map::{
  ImageLayer, IntGrid, LayerInfo, LayerKind, MapLayer, MapObjectData, ObjectShape, TileMap,
  TilesetSource,
};
pub use occluders::MapOccluder;
pub use property::{Properties, PropertyValue};
pub use spawn::{
  IntGridKey, MapEvent, MapIds, MapLayerRef, MapObject, MapRoot, MapTile, TileLayerData,
};
pub(crate) use spawn::{MapLayers, map_asset, spawn_maps};
pub use tile::{Tile, TileFlip};
pub use tiled::{TiledMapLoader, TiledTilesetLoader};
pub use tileset::{
  TileData, TileFrame, TileShape, Tileset, TilesetImage, URect, WangColor, WangKind, WangSet,
};
pub use view::MapsView;

/// The map systems that run each frame and step: collision kept in step
/// with the tiles before the tile colliders rebuild (so an edit made in
/// `update` collides in that step), and occluders before transforms
/// propagate (so an edit casts its shadow that frame)
pub(crate) fn register_tilemap_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::FixedUpdate,
    collision::sync_map_colliders
      .in_set(FixedSet::Movement)
      .before(crate::core::physics::rebuild_tile_colliders),
  );
  schedules.add_systems(
    Phase::PostUpdate,
    occluders::sync_map_occluders.before(PostUpdateSet::Transforms),
  );
}
