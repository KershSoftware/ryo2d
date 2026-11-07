//! Finding a spawned map's layers, and reading and editing their cells.

use glam::{Affine2, IVec2, Vec2};

use super::floors::MapFloors;
use super::grid::fill_cap;
use super::spawn::{
  ClassTile, LayerTiles, MapIds, MapLayerRef, MapLayers, MapRoot, MapTile, TileLayerData,
  map_asset, registered_class, spawn_class_tile,
};
use super::stream::{note_class_edit, resident_chunks};
use super::view::LayerGrid;
use super::{CHUNK_SIZE, ImageLayer, LayerInfo, Properties, Tile, TileGrid};
use crate::core::ecs::{
  ChildOf, DetectChangesMut, Disabled, Entity, Floor, Resource, Transform, World,
};
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
/// are fewer), only the region's rows and columns of each, never a lookup
/// per cell: one cell of a layer of 16 sub-layers reads 16 cells
fn clear_parts(world: &mut World, parts: &[Entity], lo: IVec2, hi: IVec2) -> FastSet<IVec2> {
  let mut cleared = FastSet::default();
  let side = CHUNK_SIZE as i32;
  for &part in parts {
    let Some(data) = world.get::<TileLayerData>(part) else {
      continue;
    };
    let grid = &data.tiles;
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
      // A chunk of `extent` is a chunk of cells: its origin is a cell, and
      // so is its last (`origin + side - 1`)
      let origin = chunk * side;
      let from = a.max(origin) - origin;
      let to = b.min(origin + IVec2::splat(side - 1)) - origin;
      for y in from.y..=to.y {
        let row = (y * side) as usize;
        for x in from.x..=to.x {
          if cells[row + x as usize] != 0 {
            cleared.insert(origin + IVec2::new(x, y));
            found = true;
          }
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
  let result = edit(&mut data.bypass_change_detection().tiles);
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
  let bounds = world.get::<TileLayerData>(entity)?.tiles.bounds();
  Some((entity, bounds))
}

/// A tile layer's bounds as `MapsView::layer_bounds` gives them: a bounded
/// layer's bounds; an unbounded one's smallest rectangle holding every cell
/// with a tile as it is now, its class tiles' cells included (empty with
/// none)
pub(crate) fn layer_bounds(world: &World, map: Entity, layer: LayerRef<'_>) -> Option<CellRect> {
  layer_entity_bounds(world, layer_entity(world, map, layer)?)
}

/// [`layer_bounds`] of the layer entity `layer`
fn layer_entity_bounds(world: &World, layer: Entity) -> Option<CellRect> {
  let grid = &world.get::<TileLayerData>(layer)?.tiles;
  if let Some(bounds) = grid.bounds() {
    return Some(bounds);
  }
  let mut bounds = grid.tile_extent();
  if let Some(table) = world.get::<LayerTiles>(layer) {
    for (&cell, &entity) in &table.0 {
      if world.get::<MapTile>(entity).is_some_and(|t| t.cell == cell) {
        bounds = bounds.union(CellRect::new(cell, cell.saturating_add(IVec2::ONE)));
      }
    }
  }
  Some(bounds)
}

/// A spawned map's bounds as `MapsView::bounds` gives them: a bounded
/// map's (0, 0) to its size, as its asset says; an infinite map's the
/// union of its tile layers' [`layer_bounds`], so its tiles as they are now
pub(crate) fn map_bounds(world: &World, map: Entity) -> Option<CellRect> {
  world.get::<MapIds>(map)?;
  let asset = map_asset(world, world.get::<MapRoot>(map)?)?;
  if !asset.infinite {
    return Some(asset.bounds());
  }
  let layers = world.get::<MapLayers>(map)?;
  Some(
    layers
      .0
      .iter()
      .filter_map(|&layer| layer_entity_bounds(world, layer))
      .fold(CellRect::default(), CellRect::union),
  )
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
  let Some(grid) = world.get::<TileLayerData>(layer).map(|data| data.grid) else {
    return false;
  };
  let existing = class_tile_at(world, layer, cell);
  if let (Some(entity), Some(tile), Some(_)) = (existing, tile, class)
    && world.get::<MapTile>(entity).is_some_and(|t| t.tile == tile)
  {
    return false;
  }
  // An entry, even one naming a class tile the game despawned, is the
  // cell's class tile: on a streamed map, its chunk changed
  let listed = world
    .get_mut::<LayerTiles>(layer)
    .and_then(|mut table| table.0.remove(&cell))
    .is_some();
  if listed || (class.is_some() && tile.is_some()) {
    note_class_edit(world, map, cell);
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
      let entity = spawn_class_tile(world, map, (layer, index), &grid, class_tile);
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
  if bounds.is_none() && !streamed_holds(world, map, cell) {
    return false;
  }
  let class = tile.and_then(|tile| registered_class(world, map, tile));
  let parts = parts_of(world, map, layer);
  let cleared = !clear_parts(world, &parts, cell, cell).is_empty();
  set_cell(world, map, layer, cell, tile, class.as_ref()) || cleared
}

/// Whether an unbounded layer of `map` takes an edit at `cell`: any cell
/// of a map not streamed, and on a streamed map ([`MapStream`](super::MapStream))
/// a cell of a resident chunk only, so an edit never makes a chunk the
/// stream has not landed (it would never unload)
pub(crate) fn streamed_holds(world: &World, map: Entity, cell: IVec2) -> bool {
  resident_chunks(world, map).is_none_or(|resident| resident.holds(TileGrid::chunk_of(cell)))
}

/// Runs `edit` on each part of the cells from `lo` to `hi` (both included)
/// that lies in a resident chunk of the streamed map `map`, by row, and
/// sums what each returns; `None` for a map not streamed, whose edit takes
/// the whole region
fn per_resident_chunk(
  world: &mut World,
  map: Entity,
  (lo, hi): (IVec2, IVec2),
  mut edit: impl FnMut(&mut World, IVec2, IVec2) -> usize,
) -> Option<usize> {
  let chunks = resident_chunks(world, map)?.within(TileGrid::chunk_of(lo), TileGrid::chunk_of(hi));
  let side = CHUNK_SIZE as i32;
  let mut changed = 0;
  for chunk in chunks {
    // A resident chunk holds cells of `i32`: its first and last are cells
    let origin = chunk * side;
    let (a, b) = (lo.max(origin), hi.min(origin + IVec2::splat(side - 1)));
    changed += edit(world, a, b);
  }
  Some(changed)
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
  if bounds.is_none() && min.cmple(max).all() {
    let each = |world: &mut World, lo, hi| fill_in(world, map, (layer, None), lo, hi, tile);
    if let Some(changed) = per_resident_chunk(world, map, (min, max), each) {
      return changed;
    }
  }
  fill_in(world, map, (layer, bounds), min, max, tile)
}

/// [`fill`] on the tile layer entity `layer`, which holds the cells
/// `bounds` (`None` for any)
fn fill_in(
  world: &mut World,
  map: Entity,
  (layer, bounds): (Entity, Option<CellRect>),
  min: IVec2,
  max: IVec2,
  tile: Option<Tile>,
) -> usize {
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
          .is_some_and(|data| data.tiles.get(cell) == tile)
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
  let missing = data.tiles.missing_chunks(lo, hi);
  let cap = fill_cap();
  if missing <= cap {
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
      "layer {name:?}: a fill from {lo} to {hi} would make {missing} chunks, past the {cap} a map holds, so it changed nothing (this layer warns once)"
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
    note_class_edit(world, map, cell);
    let live = world.get::<MapTile>(entity).is_some_and(|t| t.cell == cell);
    let unchanged = world
      .get::<TileLayerData>(layer)
      .is_some_and(|data| data.tiles.get(cell) == tile);
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
  let unbounded = world
    .get::<TileLayerData>(entity)
    .is_some_and(|data| data.tiles.bounds().is_none());
  if unbounded && min.cmple(max).all() {
    let each = |world: &mut World, lo, hi| fill_intgrid_in(world, entity, lo, hi, value);
    if let Some(changed) = per_resident_chunk(world, map, (min, max), each) {
      return changed;
    }
  }
  fill_intgrid_in(world, entity, min, max, value)
}

/// [`fill_intgrid`] on the tile layer entity `entity`
fn fill_intgrid_in(world: &mut World, entity: Entity, min: IVec2, max: IVec2, value: i32) -> usize {
  let Some(mut data) = world.get_mut::<TileLayerData>(entity) else {
    return 0;
  };
  let TileLayerData { tiles, intgrid, .. } = data.bypass_change_detection();
  let Some(intgrid) = intgrid.as_mut() else {
    return 0;
  };
  // The cells both the IntGrid and the grid hold (an unbounded grid holds
  // every cell)
  let mut held = CellRect::from_size(intgrid.size);
  if let Some(bounds) = tiles.bounds() {
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
    tiles.touch(&chunks);
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
  tile_layer(world, map, layer)?.tiles.get(cell)
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
pub(crate) fn local_to_world(world: &World, entity: Entity) -> Option<Affine2> {
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

/// A tile layer's grid and where its pixels lie in the world, through its
/// transform and its ancestors' as they are now
pub(crate) fn layer_grid(world: &World, map: Entity, layer: LayerRef<'_>) -> Option<LayerGrid> {
  entity_grid(world, layer_entity(world, map, layer)?)
}

/// [`layer_grid`] of the tile layer entity `layer`
pub(crate) fn entity_grid(world: &World, layer: Entity) -> Option<LayerGrid> {
  let grid = world.get::<TileLayerData>(layer)?.grid;
  Some(LayerGrid {
    grid,
    to_world: local_to_world(world, layer)?,
  })
}

/// The z a map at `root_z` draws its layer `index` at
pub(crate) fn layer_z(root_z: i32, index: usize) -> i32 {
  root_z.saturating_add(i32::try_from(index).unwrap_or(i32::MAX))
}

/// The z `map`'s y-sorted layers draw at (`MapsView::y_sort_z`): its first
/// y-sorted tile or object layer's (an image layer sorts nothing, and an
/// LDtk sub-layer sorts as its layer), which on a map with floors every
/// y-sorted layer draws at; `None` with none, or until the map lands
pub(crate) fn y_sort_z(world: &World, map: Entity) -> Option<i32> {
  let root = world.get::<MapRoot>(map)?;
  let layers = &world.get::<MapLayers>(map)?.0;
  let sorts = |layer: Entity| {
    world.get::<ImageLayer>(layer).is_none()
      && world
        .get::<LayerInfo>(layer)
        .is_some_and(|info| info.y_sorted && info.part_of.is_none())
  };
  let first = layers.iter().find(|&&layer| sorts(layer))?;
  Some(layer_z(root.z, world.get::<MapLayerRef>(*first)?.index))
}

/// The floor and ground cell drawn under the world point `point` on `map`
/// (`MapsView::floor_cell_at_world`): floors from the highest down, each
/// through its bottom tile layer's grid lifted by its height; a floor above
/// the ground answers only where one of its tile layers on that grid that
/// draws (visible, not disabled) holds a tile (a class tile too), the
/// ground floor anywhere
pub(crate) fn floor_cell_at_world(
  world: &World,
  map: Entity,
  point: Vec2,
) -> Option<(Floor, IVec2)> {
  world.get::<MapIds>(map)?;
  let layers = &world.get::<MapLayers>(map)?.0;
  let floors = world.get::<MapFloors>(map);
  // A floor's tile layers, bottom first (a sub-layer draws as its layer)
  let on = |number: u8| {
    layers.iter().copied().filter(move |&layer| {
      world.get::<TileLayerData>(layer).is_some()
        && world
          .get::<LayerInfo>(layer)
          .is_some_and(|info| info.floor == number && info.part_of.is_none())
    })
  };
  let highest = floors
    .and_then(|floors| floors.highest().number())
    .unwrap_or(0);
  for number in (0..=highest).rev() {
    let floor = Floor::On(number);
    let lift = match floors {
      Some(floors) => floors.lift(floor),
      None => Some(Vec2::ZERO),
    };
    let (Some(lift), Some(bottom)) = (lift, on(number).next()) else {
      continue;
    };
    let Some(ground) = entity_grid(world, bottom) else {
      continue;
    };
    // The grid where the floor draws: lifted in the layer's pixels, which
    // are the map's turned and scaled, as the drawer lifts it
    let drawn = LayerGrid {
      grid: ground.grid,
      to_world: ground.to_world * Affine2::from_translation(lift),
    };
    let Some(cell) = drawn.try_cell_at_world(point) else {
      continue;
    };
    if number == 0 {
      return Some((floor, cell));
    }
    let holds = on(number).any(|layer| {
      let draws = world.get::<Disabled>(layer).is_none()
        && world
          .get::<LayerInfo>(layer)
          .is_some_and(|info| info.visible);
      draws
        && entity_grid(world, layer) == Some(ground)
        && (world
          .get::<TileLayerData>(layer)
          .is_some_and(|data| data.tiles.get(cell).is_some())
          || class_tile_at(world, layer, cell).is_some())
    });
    if holds {
      return Some((floor, cell));
    }
  }
  None
}
