//! `ctx.maps()`: reading and editing spawned maps.

use glam::{IVec2, Vec2};

use super::edit::{self, LayerRef};
use super::spawn::{self, MapIds, MapRoot};
use super::{Tile, TileData, wang};
use crate::core::ecs::{Entity, World};

/// The spawned maps of a world: read and edit their tiles, read their
/// IntGrids and tile data, and convert between world positions and cells.
/// Every call names the map entity and, where it reads cells, a layer by
/// name or index; a map not yet spawned, a layer it does not have, a layer
/// that is not a tile layer, or a cell outside the layer reads `None` and
/// changes nothing.
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn dig(ctx: &mut SceneContext<'_>, map: Entity, at: Vec2) -> bool {
///   let mut maps = ctx.maps();
///   match maps.world_to_cell(map, "ground", at) {
///     Some(cell) => maps.set_tile(map, "ground", cell, None),
///     None => false,
///   }
/// }
/// ```
pub struct MapsView<'w> {
  world: &'w mut World,
}

impl<'w> MapsView<'w> {
  /// The maps of `world`
  pub fn new(world: &'w mut World) -> Self {
    Self { world }
  }

  /// Whether `map` has spawned its layers
  pub fn is_ready(&self, map: Entity) -> bool {
    self.world.get::<MapIds>(map).is_some()
  }

  /// The entity of `map`'s layer `layer`
  pub fn layer_entity<'a>(&self, map: Entity, layer: impl Into<LayerRef<'a>>) -> Option<Entity> {
    edit::layer_entity(self.world, map, layer.into())
  }

  /// The tile at `cell` of a tile layer's grid: a cell holding a class
  /// tile ([`MapTile`](super::MapTile)) reads `None`, the tile being its entity
  pub fn tile<'a>(&self, map: Entity, layer: impl Into<LayerRef<'a>>, cell: IVec2) -> Option<Tile> {
    edit::tile_at(self.world, map, layer.into(), cell)
  }

  /// Puts `tile` at `cell` of a tile layer (`None` clears it); false if
  /// nothing changed. A tile whose class the map's root registers
  /// ([`MapRoot::with_entity_tiles`]) spawns as a [`MapTile`](super::MapTile)
  /// there instead of going in the grid, as the map's own do; a class tile
  /// already on the cell is despawned first
  pub fn set_tile<'a>(
    &mut self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    cell: IVec2,
    tile: Option<Tile>,
  ) -> bool {
    edit::set_tile(self.world, map, layer.into(), cell, tile)
  }

  /// Puts `tile` in every cell from `min` to `max`, both included, clipped
  /// to the layer, as [`set_tile`](Self::set_tile) puts one; returns how
  /// many cells changed
  pub fn fill<'a>(
    &mut self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    min: IVec2,
    max: IVec2,
    tile: Option<Tile>,
  ) -> usize {
    edit::fill(self.world, map, layer.into(), min, max, tile)
  }

  /// Paints `cell` of a tile layer with the color `color` of the Wang set
  /// `wang_set` (named in one of the map's tilesets; `None` paints no color
  /// and empties the cell) as Tiled's terrain brush does with corrections
  /// on: the cell's corners (a corner set), edges (an edge set) or both (a
  /// mixed set) take the color, then the cell and the cells sharing one of
  /// them pick the tile that fits, a transformed one if the tileset allows,
  /// and a change that leaves a neighbour disagreeing is passed on to it,
  /// as far as the set's longest chain of colors plus one cells from `cell`
  /// (two for two colors that share a tile, four for deep water, water,
  /// sand and grass). Cells that nothing fits (an incomplete set) keep their
  /// tile and the set warns once per tileset version: so does erasing in a
  /// set with no tile that has a bare corner or edge, where the neighbours
  /// have nothing to turn into. False if nothing changed, the map, layer,
  /// set or color is not there, or the cell is outside the layer.
  ///
  /// This matches Tiled's results on the cases checked against it (complete,
  /// one-color, incomplete and multi-color sets, color chains, flipped
  /// neighbours and transformations; the fixtures in
  /// `tests/fixtures/tilemap/tiled` were painted by Tiled 1.12). Tiled draws
  /// at random among equal fits and this by a hash of the cell, so those
  /// picks differ. A class tile ([`MapTile`](super::MapTile)) keeps its
  /// colors out of a neighbour's correction, so painting beside one meets
  /// it with a seam; painting that very cell replaces it.
  pub fn set_terrain<'a>(
    &mut self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    cell: IVec2,
    wang_set: &str,
    color: Option<&str>,
  ) -> bool {
    wang::set_terrain(self.world, map, layer.into(), cell, wang_set, color)
  }

  /// The IntGrid value at `cell` of a tile layer; `None` where it is 0
  pub fn intgrid<'a>(
    &self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    cell: IVec2,
  ) -> Option<i32> {
    edit::intgrid_at(self.world, map, layer.into(), cell)
  }

  /// What `tile` carries in its tileset (class, properties, shapes,
  /// animation), if anything
  pub fn tile_data(&self, map: Entity, tile: Tile) -> Option<TileData> {
    spawn::tile_data(self.world, self.world.get::<MapRoot>(map)?, tile).cloned()
  }

  /// The cell of a tile layer under the world position `world`, whether or
  /// not the layer reaches it; `None` for a map scaled to nothing
  pub fn world_to_cell<'a>(
    &self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    world: Vec2,
  ) -> Option<IVec2> {
    edit::world_to_cell(self.world, map, layer.into(), world)
  }

  /// The world position of the centre of `cell` of a tile layer
  pub fn cell_to_world<'a>(
    &self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    cell: IVec2,
  ) -> Option<Vec2> {
    edit::cell_to_world(self.world, map, layer.into(), cell)
  }
}
