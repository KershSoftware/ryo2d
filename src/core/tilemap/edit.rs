//! Finding a spawned map's layers, and reading and editing their cells.

use glam::{Affine2, IVec2, Vec2};

use super::grid::MAX_FILL_CHUNKS;
use super::spawn::{
  ClassTile, LayerTiles, MapLayerRef, MapLayers, MapTile, TileLayerData, registered_class,
  spawn_class_tile,
};
use super::{CHUNK_SIZE, LayerInfo, Properties, Tile, TileGrid};
use crate::core::ecs::{ChildOf, DetectChangesMut, Entity, Resource, Transform, World};
use crate::core::hash::FastSet;
use crate::core::physics::CellRect;

/// How many ancestors a layer's world transform walks up before giving up:
/// far past any real hierarchy, and a cycle (which nothing else checks for)
/// ends rather than looping
const MAX_DEPTH: usize = 256;

/// A layer of a map: by name (the first layer so called) or by index
/// (bottom first: the editor's layers, then an LDtk layer's sub-layers)
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

/// The sub-layers of `map`'s layer entity `layer` (an LDtk layer's stacked,
/// offset and translucent tiles), which are part of it: empty for every
/// other layer, without allocating
pub(crate) fn parts_of(world: &World, map: Entity, layer: Entity) -> Vec<Entity> {
  let Some(id) = world
    .get::<LayerInfo>(layer)
    .filter(|info| info.part_of.is_none())
    .map(|info| info.id)
  else {
    return Vec::new();
  };
  let Some(layers) = world.get::<MapLayers>(map) else {
    return Vec::new();
  };
  layers
    .0
    .iter()
    .copied()
    .filter(|&entity| {
      world
        .get::<LayerInfo>(entity)
        .is_some_and(|info| info.part_of == Some(id))
    })
    .collect()
}

/// Clears the cells from `lo` to `hi` (both included) in `parts`; the cells
/// that held a tile in any of them. Each part's chunks in the region are
/// read where their cells lie (the region's chunks or the part's, whichever
/// are fewer), never a lookup per cell
fn clear_parts(world: &mut World, parts: &[Entity], lo: IVec2, hi: IVec2) -> FastSet<IVec2> {
  let mut cleared = FastSet::default();
  let side = CHUNK_SIZE as i32;
  for &part in parts {
    let Some(data) = world.get::<TileLayerData>(part) else {
      continue;
    };
    let grid = &data.grid;
    let extent = grid.extent();
    if extent.is_empty() {
      continue;
    }
    let (a, b) = (lo.max(extent.min), hi.min(extent.max - IVec2::ONE));
    if a.x > b.x || a.y > b.y {
      continue;
    }
    // Whether this part holds a tile in the region: a cell an earlier part
    // held too is in `cleared` already, and is this part's to clear as well
    let mut found = false;
    for chunk in grid.chunks_within(TileGrid::chunk_of(a), TileGrid::chunk_of(b)) {
      let Some(cells) = grid.chunk_cells(chunk) else {
        continue;
      };
      // A chunk of `extent` is a chunk of cells: its origin is a cell
      let origin = chunk * side;
      for (at, &cell) in cells.iter().enumerate() {
        let place = origin + IVec2::new(at as i32 % side, at as i32 / side);
        if cell != 0 && place.cmpge(a).all() && place.cmple(b).all() {
          cleared.insert(place);
          found = true;
        }
      }
    }
    if found {
      edit_grid(world, part, |grid| grid.fill(a, b, None), |n| *n > 0);
    }
  }
  cleared
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

/// A tile layer's entity and the cells its grid holds: a bounded grid's
/// bounds, or `None` for an unbounded grid, which holds any cell
fn tile_layer_bounds(
  world: &World,
  map: Entity,
  layer: LayerRef<'_>,
) -> Option<(Entity, Option<CellRect>)> {
  let entity = layer_entity(world, map, layer)?;
  let bounds = world.get::<TileLayerData>(entity)?.grid.bounds();
  Some((entity, bounds))
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

/// Puts `tile` (or nothing) at `cell` of a tile layer (any cell of an
/// unbounded one, its chunk made); see [`set_cell`]. The whole cell takes
/// it: the layer's sub-layers lose what they held there
pub(crate) fn set_tile(
  world: &mut World,
  map: Entity,
  layer: LayerRef<'_>,
  cell: IVec2,
  tile: Option<Tile>,
) -> bool {
  let Some((layer, bounds)) = tile_layer_bounds(world, map, layer) else {
    return false;
  };
  if bounds.is_some_and(|bounds| !bounds.contains(cell)) {
    return false;
  }
  let class = tile.and_then(|tile| registered_class(world, map, tile));
  let parts = parts_of(world, map, layer);
  let cleared = !clear_parts(world, &parts, cell, cell).is_empty();
  set_cell(world, map, layer, cell, tile, class.as_ref()) || cleared
}

/// Puts `tile` (or nothing) in every cell from `min` to `max`, both
/// included, clipped to a bounded layer (an unbounded one makes the chunks
/// a tile goes in); returns how many cells changed. The whole of each cell
/// takes it: the layer's sub-layers lose what they held
pub(crate) fn fill(
  world: &mut World,
  map: Entity,
  layer: LayerRef<'_>,
  min: IVec2,
  max: IVec2,
  tile: Option<Tile>,
) -> usize {
  let Some((layer, bounds)) = tile_layer_bounds(world, map, layer) else {
    return 0;
  };
  let (lo, hi) = match bounds {
    None => (min, max),
    Some(bounds) if bounds.is_empty() => return 0,
    Some(bounds) => (min.max(bounds.min), max.min(bounds.max - IVec2::ONE)),
  };
  if lo.x > hi.x || lo.y > hi.y {
    return 0;
  }
  if tile.is_some() && refused_fill(world, layer, lo, hi) {
    return 0;
  }
  // Cells that change only by losing a sub-layer's tile: the layer's own
  // grid already holds `tile` there, and no class tile is on them
  let parts = parts_of(world, map, layer);
  let cleared = clear_parts(world, &parts, lo, hi);
  let only_parts = cleared
    .iter()
    .filter(|&&cell| {
      class_tile_at(world, layer, cell).is_none()
        && world
          .get::<TileLayerData>(layer)
          .is_some_and(|data| data.grid.get(cell) == tile)
    })
    .count();
  fill_layer(world, map, layer, (lo, hi), tile) + only_parts
}

/// The layers a fill was refused on, each warned about once
#[derive(Resource, Default)]
pub(crate) struct FillRefused(FastSet<Entity>);

/// Whether a tile fill of the cells from `lo` to `hi` of the layer entity
/// `layer` would make more chunks than a map holds (`MAX_FILL_CHUNKS`): a
/// huge region of an unbounded layer, refused before anything is made
/// rather than exhausting memory, and warned once a layer. A class tile's
/// fill counts the same chunks, each cell of them spawning an entity
fn refused_fill(world: &mut World, layer: Entity, lo: IVec2, hi: IVec2) -> bool {
  let Some(data) = world.get::<TileLayerData>(layer) else {
    return false;
  };
  let missing = data.grid.missing_chunks(lo, hi);
  if missing <= MAX_FILL_CHUNKS {
    return false;
  }
  world.init_resource::<FillRefused>();
  let first = world
    .get_resource_mut::<FillRefused>()
    .is_some_and(|mut refused| refused.0.insert(layer));
  if first {
    let name = world
      .get::<LayerInfo>(layer)
      .map_or("?", |info| info.name.as_str());
    crate::engine_warn!(
      module: "Tilemap",
      "layer {name:?}: a fill from {lo} to {hi} would make {missing} chunks, past the {MAX_FILL_CHUNKS} a map holds, so it changed nothing (this layer warns once)"
    );
  }
  true
}

/// [`fill`] on the layer entity `layer` alone, the region clipped
fn fill_layer(
  world: &mut World,
  map: Entity,
  layer: Entity,
  (lo, hi): (IVec2, IVec2),
  tile: Option<Tile>,
) -> usize {
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

/// Puts `value` (0 for none) in every cell of a tile layer's IntGrid from
/// `min` to `max`, both included, clipped to the IntGrid and the grid,
/// moving on the version of only the chunks whose values changed, as one
/// change, so only they collide, cast shadows and draw again; returns how
/// many cells changed (0 for a layer with no IntGrid)
pub(crate) fn fill_intgrid(
  world: &mut World,
  map: Entity,
  layer: LayerRef<'_>,
  min: IVec2,
  max: IVec2,
  value: i32,
) -> usize {
  let Some(entity) = layer_entity(world, map, layer) else {
    return 0;
  };
  let Some(mut data) = world.get_mut::<TileLayerData>(entity) else {
    return 0;
  };
  let TileLayerData { grid, intgrid, .. } = data.bypass_change_detection();
  let Some(intgrid) = intgrid.as_mut() else {
    return 0;
  };
  // The cells both the IntGrid and the grid hold (an unbounded grid holds
  // every cell)
  let mut held = CellRect::from_size(intgrid.size);
  if let Some(bounds) = grid.bounds() {
    held = CellRect::new(held.min.max(bounds.min), held.max.min(bounds.max));
  }
  if held.is_empty() {
    return 0;
  }
  let lo = min.max(held.min);
  let hi = max.min(held.max - IVec2::ONE);
  if lo.x > hi.x || lo.y > hi.y {
    return 0;
  }
  let width = intgrid.size.x as usize;
  let mut changed = 0;
  let mut chunks: Vec<IVec2> = Vec::new();
  for y in lo.y..=hi.y {
    for x in lo.x..=hi.x {
      let Some(slot) = intgrid.values.get_mut(y as usize * width + x as usize) else {
        continue;
      };
      if *slot == value {
        continue;
      }
      *slot = value;
      changed += 1;
      let chunk = TileGrid::chunk_of(IVec2::new(x, y));
      if chunks.last() != Some(&chunk) && !chunks.contains(&chunk) {
        chunks.push(chunk);
      }
    }
  }
  if changed > 0 {
    grid.touch(&chunks);
    data.set_changed();
  }
  changed
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
