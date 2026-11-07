//! Tilemaps. A [`TileMap`] holds layers of tiles, objects or an image over
//! one or more [`Tileset`]s, with typed [`Properties`] on every level. Tiles
//! live in [`TileGrid`]s of chunks by signed chunk coordinate, bounded or
//! unbounded, whose edits mark chunks dirty. A [`MapRoot`]
//! entity spawns a map's layers, objects and class tiles as its children,
//! read and edited through [`MapsView`] (`ctx.maps()`); layers on floors
//! above the ground stand in ground space ([`MapFloors`]). A [`MapStream`]
//! streams an infinite map's chunks from a generator around the cameras
//! and [`StreamAnchor`]s, keeping the chunks a player changed in
//! [`SavedChunks`].

mod collision;
mod edit;
mod floors;
mod grid;
mod grid_shape;
mod jobs;
mod kinds;
mod ldtk;
mod map;
mod occluders;
#[doc(hidden)]
pub mod padded;
mod property;
mod reload;
mod spawn;
mod stairs;
mod stream;
#[cfg(test)]
pub(crate) mod test_util;
mod tile;
mod tiled;
mod tileset;
mod view;
mod wang;
mod world;

use crate::core::ecs::{IntoScheduleConfigs, Schedule};
use crate::core::schedule::{EngineSchedules, FixedSet, Phase, PostUpdateSet};

pub use edit::LayerRef;
pub(crate) use edit::{layer_z, y_sort_z};
pub use floors::{MapFloor, MapFloors};
pub use grid::{CHUNK_SIZE, TileGrid};
pub use grid_shape::{CellOutline, GridShape, Lattice, MapGrid, StaggerAxis, StaggerIndex};
pub use ldtk::{
  LdtkLevel, LdtkLoader, LdtkProject, Neighbour, NeighbourDirection, WorldLayout, WorldLevel,
};
pub use map::{
  ImageLayer, IntGrid, LayerInfo, LayerKind, MapLayer, MapObjectData, ObjectShape, TileMap,
  TilesetSource,
};
pub use occluders::MapOccluder;
pub use property::{Properties, PropertyValue};
pub(crate) use reload::reload_maps;
pub use spawn::{
  IntGridKey, MapEvent, MapIds, MapLayerRef, MapObject, MapRoot, MapTile, TileLayerData,
};
pub(crate) use spawn::{MapLayers, follow_chunk_swaps, map_asset, spawn_maps};
pub use stairs::{Climber, FloorChanged};
pub(crate) use stream::stream_maps;
pub use stream::{
  ChunkEvent, ChunkOwned, ChunkRequest, ChunkTiles, MapStream, SavedChunks, StreamAnchor,
  StreamStatus,
};
pub use tile::{Tile, TileFlip};
pub use tiled::{TileWorldLoader, TiledMapLoader, TiledTilesetLoader};
pub use tileset::{
  TileData, TileFrame, TileShape, TileTransformations, Tileset, TilesetImage, URect, WangColor,
  WangKind, WangSet, WangTile,
};
pub use view::{LayerGrid, MapsView};
pub use world::{TileWorld, WorldLevelOf, WorldRef};

/// The systems a tile edit runs through, each in a schedule of its own:
/// the map collision sync (the edited chunk classified again), physics'
/// tile collider rebuild with its collider sync after it (the loops through
/// the cell retraced, their chunk colliders reshaped and the new shapes
/// built for Rapier) and the map occluder sync. For a bench to time each on
/// a world the engine runs (`benches/tilemap.rs`); not for games
#[doc(hidden)]
pub fn edit_schedules() -> [Schedule; 3] {
  let mut collision = Schedule::default();
  collision.add_systems(collision::sync_map_colliders);
  let mut rebuild = Schedule::default();
  rebuild.add_systems(
    (
      crate::core::physics::rebuild_tile_colliders,
      crate::core::physics::sync::sync_colliders,
    )
      .chain(),
  );
  let mut occluders = Schedule::default();
  occluders.add_systems(occluders::sync_map_occluders);
  [collision, rebuild, occluders]
}

/// The map systems that run each frame and step: collision kept in step
/// with the tiles before the tile colliders rebuild (so an edit made in
/// `update` collides in that step), occluders before transforms propagate
/// (so an edit casts its shadow that frame), and the entities on floors
/// lifted after transforms propagate: in each fixed step before trails
/// record (so a trail spawned on a floor records its first point lifted,
/// and an emitter emits lifted from its next step), and each frame before
/// the cameras (so they draw lifted where they stand that frame)
pub(crate) fn register_tilemap_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::FixedUpdate,
    collision::sync_map_colliders
      .in_set(FixedSet::Movement)
      .before(crate::core::physics::rebuild_tile_colliders),
  );
  schedules.add_systems(
    Phase::FixedUpdate,
    (stairs::sync_stair_runs, stairs::climb_stairs)
      .chain()
      .in_set(FixedSet::Movement)
      .before(crate::core::gameplay::apply_kill_zones),
  );
  schedules.add_systems(
    Phase::PostUpdate,
    occluders::sync_map_occluders.before(PostUpdateSet::Transforms),
  );
  schedules.add_systems(
    Phase::FixedUpdate,
    floors::lift_floored
      .after(FixedSet::Transforms)
      .before(FixedSet::Trails),
  );
  schedules.add_systems(
    Phase::PostUpdate,
    floors::lift_floored
      .after(PostUpdateSet::Transforms)
      .before(PostUpdateSet::Camera),
  );
}
