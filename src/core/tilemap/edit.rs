//! Finding a spawned map's layers, and reading and editing their cells.

use glam::{Affine2, IVec2, Vec2};

use super::spawn::{
  ClassTile, LayerTiles, MapLayerRef, MapLayers, MapTile, TileLayerData, registered_class,
  spawn_class_tile,
};
use super::{Properties, Tile, TileGrid};
use crate::core::ecs::{ChildOf, DetectChangesMut, Entity, Transform, World};

/// How many ancestors a layer's world transform walks up before giving up:
/// far past any real hierarchy, and a cycle (which nothing else checks for)
/// ends rather than looping
const MAX_DEPTH: usize = 256;

/// A layer of a map: by name (the first layer so called) or by index
/// (bottom first)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerRef<'a> {
  Name(&'a str),
  Index(usize),
}

impl<'a> From<&'a str> for LayerRef<'a> {
  fn from(name: &'a str) -> Self {
    LayerRef::Name(name)
  }
}

impl<'a> From<&'a String> for LayerRef<'a> {
  fn from(name: &'a String) -> Self {
    LayerRef::Name(name)
  }
}

impl From<usize> for LayerRef<'_> {
  fn from(index: usize) -> Self {
    LayerRef::Index(index)
  }
}

/// The entity of `map`'s layer `layer`, once the map has spawned (and while
/// the layer is still there)
pub(crate) fn layer_entity(world: &World, map: Entity, layer: LayerRef<'_>) -> Option<Entity> {
  let layers = &world.get::<MapLayers>(map)?.0;
  let is_layer = |entity: Entity, name: Option<&str>| {
    world
      .get::<MapLayerRef>(entity)
      .is_some_and(|l| l.map == map && name.is_none_or(|name| l.name == name))
  };
  match layer {
    LayerRef::Index(index) => layers
      .get(index)
      .copied()
      .filter(|&entity| is_layer(entity, None)),
    LayerRef::Name(name) => layers
      .iter()
      .copied()
      .find(|&entity| is_layer(entity, Some(name))),
  }
}

/// A tile layer's cells
fn tile_layer<'w>(world: &'w World, map: Entity, layer: LayerRef<'_>) -> Option<&'w TileLayerData> {
  world.get::<TileLayerData>(layer_entity(world, map, layer)?)
}

/// Runs `edit` on the grid of the tile layer entity `layer`; the layer is
/// marked changed only when `changed` says the edit changed something
fn edit_grid<R>(
  world: &mut World,
  layer: Entity,
  edit: impl FnOnce(&mut TileGrid) -> R,
  changed: impl FnOnce(&R) -> bool,
) -> Option<R> {
  let mut data = world.get_mut::<TileLayerData>(layer)?;
  let result = edit(&mut data.bypass_change_detection().grid);
  if changed(&result) {
    data.set_changed();
  }
  Some(result)
}

/// A tile layer's entity and grid size
fn tile_layer_size(world: &World, map: Entity, layer: LayerRef<'_>) -> Option<(Entity, IVec2)> {
  let entity = layer_entity(world, map, layer)?;
  let size = world.get::<TileLayerData>(entity)?.grid.size().as_ivec2();
  Some((entity, size))
}

/// The class tile living at `cell` of the layer entity `layer`
fn class_tile_at(world: &World, layer: Entity, cell: IVec2) -> Option<Entity> {
  let entity = *world.get::<LayerTiles>(layer)?.0.get(&cell)?;
  world
    .get::<MapTile>(entity)
    .is_some_and(|tile| tile.cell == cell)
    .then_some(entity)
}

/// Puts `tile` (or nothing) at `cell`, inside the layer entity `layer` of
/// `map`. A class tile at the cell goes first; a tile whose class the root
/// registers (`class`) spawns as a class tile and leaves the grid. True if
/// anything changed
fn set_cell(
  world: &mut World,
  map: Entity,
  layer: Entity,
  cell: IVec2,
  tile: Option<Tile>,
  class: Option<&(String, Properties)>,
) -> bool {
  // A game's observer of an earlier cell's class tile may have taken the layer
  let Some(cell_size) = world
    .get::<TileLayerData>(layer)
    .map(|data| data.cell_size.as_vec2())
  else {
    return false;
  };
  let existing = class_tile_at(world, layer, cell);
  if let (Some(entity), Some(tile), Some(_)) = (existing, tile, class)
    && world.get::<MapTile>(entity).is_some_and(|t| t.tile == tile)
  {
    return false;
  }
  if let Some(mut table) = world.get_mut::<LayerTiles>(layer) {
    table.0.remove(&cell);
  }
  let mut changed = false;
  if let Some(entity) = existing {
    let _ = world.try_despawn(entity);
    changed = true;
  }
  match (tile, class) {
    (Some(tile), Some((class, properties))) => {
      edit_grid(world, layer, |grid| grid.set(cell, None), |c| *c);
      let index = world.get::<MapLayerRef>(layer).map_or(0, |l| l.index);
      let class_tile = ClassTile {
        cell,
        tile,
        class: class.clone(),
        properties: properties.clone(),
      };
      let entity = spawn_class_tile(world, map, (layer, index), cell_size, class_tile);
      if let Some(mut table) = world.get_mut::<LayerTiles>(layer) {
        table.0.insert(cell, entity);
      }
      true
    }
    _ => edit_grid(world, layer, |grid| grid.set(cell, tile), |c| *c).unwrap_or(false) || changed,
  }
}

/// Puts `tile` (or nothing) at `cell` of a tile layer; see [`set_cell`]
pub(crate) fn set_tile(
  world: &mut World,
  map: Entity,
  layer: LayerRef<'_>,
  cell: IVec2,
  tile: Option<Tile>,
) -> bool {
  let Some((layer, size)) = tile_layer_size(world, map, layer) else {
    return false;
  };
  if cell.x < 0 || cell.y < 0 || cell.x >= size.x || cell.y >= size.y {
    return false;
  }
  let class = tile.and_then(|tile| registered_class(world, map, tile));
  set_cell(world, map, layer, cell, tile, class.as_ref())
}

/// Puts `tile` (or nothing) in every cell from `min` to `max`, both
/// included, clipped to the layer; returns how many cells changed
pub(crate) fn fill(
  world: &mut World,
  map: Entity,
  layer: LayerRef<'_>,
  min: IVec2,
  max: IVec2,
  tile: Option<Tile>,
) -> usize {
  let Some((layer, size)) = tile_layer_size(world, map, layer) else {
    return 0;
  };
  let (lo, hi) = (min.max(IVec2::ZERO), max.min(size - IVec2::ONE));
  if lo.x > hi.x || lo.y > hi.y {
    return 0;
  }
  let class = tile.and_then(|tile| registered_class(world, map, tile));
  if class.is_some() {
    // Each cell spawns its own entity
    let mut changed = 0;
    for y in lo.y..=hi.y {
      for x in lo.x..=hi.x {
        changed += usize::from(set_cell(
          world,
          map,
          layer,
          IVec2::new(x, y),
          tile,
          class.as_ref(),
        ));
      }
    }
    return changed;
  }
  // The class tiles in the region go; a cell whose grid already holds the
  // tile changed only by losing its class tile
  let inside = |cell: &IVec2| cell.cmpge(lo).all() && cell.cmple(hi).all();
  let taken: Vec<(IVec2, Entity)> = world
    .get::<LayerTiles>(layer)
    .map(|table| {
      table
        .0
        .iter()
        .filter(|(cell, _)| inside(cell))
        .map(|(cell, entity)| (*cell, *entity))
        .collect()
    })
    .unwrap_or_default();
  let mut changed = 0;
  for (cell, entity) in taken {
    if let Some(mut table) = world.get_mut::<LayerTiles>(layer) {
      table.0.remove(&cell);
    }
    let live = world.get::<MapTile>(entity).is_some_and(|t| t.cell == cell);
    let unchanged = world
      .get::<TileLayerData>(layer)
      .is_some_and(|data| data.grid.get(cell) == tile);
    if live {
      let _ = world.try_despawn(entity);
      changed += usize::from(unchanged);
    }
  }
  changed + edit_grid(world, layer, |grid| grid.fill(lo, hi, tile), |n| *n > 0).unwrap_or(0)
}

/// The tile at `cell` of a tile layer
pub(crate) fn tile_at(
  world: &World,
  map: Entity,
  layer: LayerRef<'_>,
  cell: IVec2,
) -> Option<Tile> {
  tile_layer(world, map, layer)?.grid.get(cell)
}

/// The IntGrid value at `cell` of a tile layer
pub(crate) fn intgrid_at(
  world: &World,
  map: Entity,
  layer: LayerRef<'_>,
  cell: IVec2,
) -> Option<i32> {
  tile_layer(world, map, layer)?.intgrid.as_ref()?.get(cell)
}

/// Where an entity's local space lies in the world, from its `Transform` and
/// its ancestors' as they are now: an edit made this frame counts before
/// propagation runs. A parent with no `Transform` starts its children from
/// the identity, as propagation does
fn local_to_world(world: &World, entity: Entity) -> Option<Affine2> {
  let mut affine = world.get::<Transform>(entity)?.to_affine();
  let mut child = entity;
  for _ in 0..MAX_DEPTH {
    let Some(parent) = world.get::<ChildOf>(child).map(ChildOf::parent) else {
      return Some(affine);
    };
    let Some(transform) = world.get::<Transform>(parent) else {
      return Some(affine);
    };
    affine = transform.to_affine() * affine;
    child = parent;
  }
  None
}

/// A tile layer's cell size and where its space lies in the world
fn layer_space(world: &World, map: Entity, layer: LayerRef<'_>) -> Option<(Vec2, Affine2)> {
  let entity = layer_entity(world, map, layer)?;
  let cell = world.get::<TileLayerData>(entity)?.cell_size.as_vec2();
  Some((cell, local_to_world(world, entity)?))
}

/// The cell of a tile layer under the world position `point`, inside the
/// layer or not; `None` where the layer is scaled to nothing
pub(crate) fn world_to_cell(
  world: &World,
  map: Entity,
  layer: LayerRef<'_>,
  point: Vec2,
) -> Option<IVec2> {
  let (cell, affine) = layer_space(world, map, layer)?;
  if affine.matrix2.determinant() == 0.0 {
    return None;
  }
  let local = affine.inverse().transform_point2(point) / cell;
  local.is_finite().then(|| local.floor().as_ivec2())
}

/// The world position of the centre of a tile layer's `cell`
pub(crate) fn cell_to_world(
  world: &World,
  map: Entity,
  layer: LayerRef<'_>,
  cell: IVec2,
) -> Option<Vec2> {
  let (size, affine) = layer_space(world, map, layer)?;
  Some(affine.transform_point2((cell.as_vec2() + 0.5) * size))
}
