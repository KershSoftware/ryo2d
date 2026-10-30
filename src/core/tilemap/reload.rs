//! Hot reload: a spawned map follows the files it was made from. Its files
//! are its map file (or LDtk project), with what their loaders read
//! (templates, the Tiled project, LDtk level files), and its tileset files.
//! When a load of one of them lands (its `AssetServer::load_job` moves:
//! not when a reload starts or fails, nor when a dependency's reload, such
//! as a tileset image's, only checks it again), `reload_maps` brings the
//! map to its asset as it now is, in the frame the load lands, before the
//! game's update.
//!
//! A reload keeps what it can, so the game keeps its state: each layer the
//! file still has (by editor id: Tiled's layer id, LDtk's layer definition)
//! keeps its entity and takes the file's cells, and only the chunks whose
//! tiles or IntGrid values differ move their versions, so only they draw,
//! collide and occlude again, each in place (a chunk collider keeps its
//! entity and handle, so contacts and a character's floor carry through).
//! A kept object or image layer whose offset changed moves (a Tiled
//! group's offset lands on its layers). A layer new to the file, or whose
//! kind changed, or a tile layer whose size, cell size or offset changed,
//! spawns afresh, and one gone despawns. Objects reconcile by id
//! (`reconcile`) and class tiles by layer and cell, each judged against
//! what the file said before (`MapApplied`): one the file left as it was
//! keeps its entity untouched, one it changed is updated in place (its
//! components put over, so it keeps what the game added), a new one spawns
//! and a removed one despawns. The file wins over the game's tile edits
//! (dug tiles come back), but an object or class tile the game despawned
//! stays despawned until a save adds or changes it.

use glam::{IVec2, UVec2, Vec2};

use super::spawn::{
  ClassTile, ClassTiles, LayerRest, LayerTiles, MapLayers, MapSource, child_of, class_tiles,
  map_asset, object_parts, plan_layer, send, spawn_class_tile, spawn_layer, take_class_tiles,
};
use super::{
  CHUNK_SIZE, ImageLayer, IntGrid, LayerInfo, LayerKind, LdtkProject, MapEvent, MapIds, MapLayer,
  MapLayerRef, MapObject, MapObjectData, MapRoot, MapTile, Properties, Tile, TileGrid,
  TileLayerData, TileMap, Tileset, TilesetSource,
};
use crate::core::asset::{Asset, AssetEvent, AssetServer, UntypedAssetId};
use crate::core::ecs::kernel::{MessageCursor, Messages, QueryState};
use crate::core::ecs::{
  Allow, ChildOf, Component, DetectChangesMut, Disabled, Entity, Local, Transform, World,
};
use crate::core::hash::{FastMap, FastSet};

/// The loads of a root's files its spawned map follows: its map's (or its
/// LDtk project's), then those of the tileset assets the map uses, as
/// `AssetServer::load_job` gives them (`None` for an asset made in code,
/// which no file reloads)
pub(crate) fn content_stamp(world: &World, root: &MapRoot) -> Vec<Option<u64>> {
  let server = world.get_resource::<AssetServer>();
  let load = |id: UntypedAssetId| server.and_then(|server| server.load_job(id));
  let mut stamp = vec![load(source_id(root))];
  if let Some(map) = map_asset(world, root) {
    stamp.extend(map.tilesets.iter().filter_map(|source| match source {
      TilesetSource::Asset(tileset) => Some(load(tileset.id().untyped())),
      TilesetSource::Embedded(_) => None,
    }));
  }
  stamp
}

/// The asset a root names: its map, or its LDtk project
fn source_id(root: &MapRoot) -> UntypedAssetId {
  match &root.source {
    MapSource::Map(map) => map.id().untyped(),
    MapSource::Ldtk { project, .. } => project.id().untyped(),
    MapSource::WorldLevel { world, .. } => world.asset_id(),
  }
}

/// What keeps a layer's entity through a reload: the same kind, and for a
/// tile layer the same size, cell size and offset (an object or image
/// layer keeps its entity through an offset change, and moves)
#[derive(Clone, Debug, PartialEq)]
enum LayerShape {
  Tiles {
    size: UVec2,
    cell_size: UVec2,
    offset: Vec2,
  },
  Objects,
  Image,
}

/// A layer as the file had it: its editor id and shape
#[derive(Clone, Debug, PartialEq)]
struct FileLayer {
  id: u32,
  shape: LayerShape,
  /// A kept object or image layer whose offset changed moves (a Tiled
  /// group's offset lands on its layers); a tile layer's is in its shape
  offset: Vec2,
}

impl FileLayer {
  fn of(layer: &MapLayer) -> FileLayer {
    let shape = match &layer.kind {
      LayerKind::Tiles {
        grid, cell_size, ..
      } => LayerShape::Tiles {
        size: grid.size(),
        cell_size: *cell_size,
        offset: layer.info.offset,
      },
      LayerKind::Objects(_) => LayerShape::Objects,
      LayerKind::Image(_) => LayerShape::Image,
    };
    FileLayer {
      id: layer.info.id,
      shape,
      offset: layer.info.offset,
    }
  }
}

/// A class tile as the file had it: its tile, class and properties
type FileClassTile = (Tile, String, Properties);

/// What a spawned map was last made from, on the map entity: the loads of
/// its files, and its layers and objects as the files had them, so a
/// reload tells what a save changed from what the game changed
#[derive(Component, Debug)]
pub(crate) struct MapApplied {
  stamp: Vec<Option<u64>>,
  layers: Vec<FileLayer>,
  /// Each object by id: its layer's editor id and its data
  objects: FastMap<String, (u32, MapObjectData)>,
  /// Each class tile by its layer's editor id and its cell
  class_tiles: FastMap<(u32, IVec2), FileClassTile>,
}

impl MapApplied {
  /// Of a map's layers as its files give them, at the loads `stamp`
  /// (its class tiles are noted as they are taken out, `note_class_tiles`)
  pub(crate) fn new(stamp: Vec<Option<u64>>, layers: &[MapLayer]) -> MapApplied {
    let mut objects = FastMap::default();
    for layer in layers {
      if let LayerKind::Objects(list) = &layer.kind {
        for object in list {
          // Ids are unique in a map that Tiled or LDtk wrote. Of a repeated
          // one the first is noted here, while a landing's `MapIds` names
          // the last spawned and a reload counts the id once
          objects
            .entry(object.id.clone())
            .or_insert_with(|| (layer.info.id, object.clone()));
        }
      }
    }
    MapApplied {
      stamp,
      layers: layers.iter().map(FileLayer::of).collect(),
      objects,
      class_tiles: FastMap::default(),
    }
  }

  /// Notes `found`, the class tiles of the layer of editor id `layer`
  pub(crate) fn note_class_tiles(&mut self, layer: u32, found: &[ClassTile]) {
    for tile in found {
      let file = (tile.tile, tile.class.clone(), tile.properties.clone());
      self.class_tiles.insert((layer, tile.cell), file);
    }
  }

  /// The loads of the files it was made from (`content_stamp`)
  pub(crate) fn stamp(&self) -> &[Option<u64>] {
    &self.stamp
  }
}

/// How a reload's objects meet a spawned map's, by id
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Reconcile {
  /// Entities whose object the file left as it was: left untouched
  pub(crate) keep: Vec<Entity>,
  /// Entities whose object the file changed, with the index of its new
  /// data: they take a new `Transform` and `MapObject`, keeping the rest
  pub(crate) update: Vec<(Entity, usize)>,
  /// Indices of the objects the map has no live entity for that the file
  /// added or changed: one the game despawned stays despawned while the
  /// file leaves it as it was
  pub(crate) spawn: Vec<usize>,
  /// Entities whose object the file no longer has
  pub(crate) despawn: Vec<Entity>,
}

/// Matches the map's live objects (`existing`, by id) with the file's
/// (`new`). `unchanged(i)` says whether the file left `new[i]` as it was
/// (its data and its layer): an entity's own components cannot tell, as
/// the game may have moved it, and an object the game despawned has none
pub(crate) fn reconcile(
  existing: &[(String, Entity)],
  new: &[MapObjectData],
  unchanged: impl Fn(usize) -> bool,
) -> Reconcile {
  let mut by_id: FastMap<&str, Entity> = existing
    .iter()
    .map(|(id, entity)| (id.as_str(), *entity))
    .collect();
  let mut matched = Reconcile::default();
  for (index, object) in new.iter().enumerate() {
    match by_id.remove(object.id.as_str()) {
      Some(entity) if unchanged(index) => matched.keep.push(entity),
      Some(entity) => matched.update.push((entity, index)),
      None if unchanged(index) => {}
      None => matched.spawn.push(index),
    }
  }
  matched.despawn = existing
    .iter()
    .filter(|(id, _)| by_id.contains_key(id.as_str()))
    .map(|(_, entity)| *entity)
    .collect();
  matched
}

/// The asset events that can mean a map's files loaded again
#[derive(Default)]
pub(crate) struct Changes {
  maps: MessageCursor<AssetEvent<TileMap>>,
  tilesets: MessageCursor<AssetEvent<Tileset>>,
  projects: MessageCursor<AssetEvent<LdtkProject>>,
}

impl Changes {
  /// Whether a map, tileset or LDtk project landed or changed since the
  /// last look (every cursor read to its end)
  fn any(&mut self, world: &World) -> bool {
    let maps = landed(world, &mut self.maps);
    let tilesets = landed(world, &mut self.tilesets);
    let projects = landed(world, &mut self.projects);
    maps || tilesets || projects
  }
}

/// Whether an asset of the type landed or changed since `cursor` last
/// read; it reads to the end, so each event counts once
fn landed<T: Asset>(world: &World, cursor: &mut MessageCursor<AssetEvent<T>>) -> bool {
  let Some(messages) = world.get_resource::<Messages<AssetEvent<T>>>() else {
    return false;
  };
  let mut any = false;
  for event in cursor.read(messages) {
    any |= matches!(
      event,
      AssetEvent::Added { .. } | AssetEvent::Modified { .. }
    );
  }
  any
}

/// The spawned maps, disabled ones too (a disabled map follows its files,
/// so it is as they are when enabled)
pub(crate) type AppliedMaps =
  QueryState<(Entity, &'static MapRoot, &'static MapApplied), Allow<Disabled>>;

/// `PreUpdate`, after the spawn: every spawned map a load of whose files
/// landed is brought to its asset as it now is (`apply_reload`). A map
/// whose asset keeps its last good value after a failed check (a tileset
/// saved that its map no longer fits) waits for the next good load. With
/// no map, tileset or LDtk project changed it reads three cursors
pub(crate) fn reload_maps(world: &mut World, mut changes: Local<Changes>, maps: &mut AppliedMaps) {
  if !changes.any(world) {
    return;
  }
  let server = world.get_resource::<AssetServer>();
  let due: Vec<Entity> = maps
    .iter(world)
    .filter(|(_, root, applied)| {
      content_stamp(world, root) != applied.stamp
        && !server.is_some_and(|server| server.kept_after_a_failure(source_id(root)))
    })
    .map(|(map, ..)| map)
    .collect();
  for map in due {
    apply_reload(world, map);
  }
}

/// A layer's work left for after the layers are in place
enum Pending {
  Tiles {
    layer: (Entity, usize),
    id: u32,
    cell_size: Vec2,
    found: Vec<ClassTile>,
    /// The class tiles alive on the layer's entity before (on the old one,
    /// for a layer spawned afresh), by cell
    live: FastMap<IVec2, Entity>,
    kept: bool,
  },
  Objects {
    layer: Entity,
    id: u32,
    kept: bool,
    objects: Vec<MapObjectData>,
  },
}

/// Brings the spawned map `map` to its asset as it now is, with its
/// tilesets (see the module docs), then sends `MapEvent::Reloaded`. A map
/// whose LDtk project no longer has its level stays as it is, warned once
/// per load of the project. Game observers run as objects and class tiles
/// change and may despawn the map or its parts: what is gone is skipped
pub(crate) fn apply_reload(world: &mut World, map: Entity) {
  let Some(root) = world.get::<MapRoot>(map).cloned() else {
    return;
  };
  let stamp = content_stamp(world, &root);
  let Some(asset) = map_asset(world, &root) else {
    // Only an LDtk project saved without the level gets here: a root holds
    // its map, and a map that fails a reload keeps its last good value
    if let Some(mut applied) = world.get_mut::<MapApplied>(map)
      && applied.stamp != stamp
    {
      applied.stamp = stamp;
      if let MapSource::Ldtk { level, .. } = &root.source {
        crate::engine_warn!(
          module: "Tilemap",
          "map {map}: its LDtk project was saved without level {level:?}; the map stays as it was"
        );
      }
    }
    return;
  };
  let layers = asset.layers.clone();
  let classes = class_tiles(world, asset, &root.entity_tiles);
  let Some(applied) = world
    .get_entity_mut(map)
    .ok()
    .and_then(|mut entity| entity.take::<MapApplied>())
  else {
    return;
  };
  let old_layers = world
    .get::<MapLayers>(map)
    .map(|layers| layers.0.clone())
    .unwrap_or_default();
  let mut old: FastMap<u32, (Entity, &FileLayer)> = FastMap::default();
  for (file, &entity) in applied.layers.iter().zip(&old_layers) {
    old.entry(file.id).or_insert((entity, file));
  }

  // The layers: kept ones brought up to the file, new or reshaped ones
  // spawned, none despawned yet (objects may move out of them)
  let mut entities = Vec::with_capacity(layers.len());
  let mut files = Vec::with_capacity(layers.len());
  let mut pending = Vec::new();
  for (index, layer) in layers.into_iter().enumerate() {
    if world.get_entity(map).is_err() {
      return;
    }
    let file = FileLayer::of(&layer);
    let was = old.remove(&file.id).filter(|(entity, _)| {
      world
        .get::<MapLayerRef>(*entity)
        .is_some_and(|layer| layer.map == map)
    });
    let live = was.map_or_else(FastMap::default, |(entity, _)| {
      live_class_tiles(world, entity)
    });
    let kept = was.filter(|(_, then)| then.shape == file.shape);
    let (entity, rest) = match kept {
      Some((entity, then)) => {
        if then.offset != file.offset
          && let Some(mut transform) = world.get_mut::<Transform>(entity)
        {
          transform.translation = file.offset;
        }
        let rest = update_layer(world, map, (entity, index), layer, &classes);
        (entity, rest)
      }
      None => spawn_layer(world, map, index, plan_layer(layer, &classes)),
    };
    let kept = kept.is_some();
    match rest {
      LayerRest::Tiles(cell_size, found) => pending.push(Pending::Tiles {
        layer: (entity, index),
        id: file.id,
        cell_size,
        found,
        live,
        kept,
      }),
      LayerRest::Objects(objects) => pending.push(Pending::Objects {
        layer: entity,
        id: file.id,
        kept,
        objects,
      }),
      LayerRest::Nothing => {}
    }
    entities.push(entity);
    files.push(file);
  }

  // The objects: each of the file's once (ids are unique in a map), with
  // the layer it lives in, its editor id, and whether that layer was kept
  let mut seen = FastSet::default();
  let mut objects = Vec::new();
  let mut homes: Vec<(Entity, u32, bool)> = Vec::new();
  let mut tiles = Vec::new();
  for work in pending {
    match work {
      Pending::Objects {
        layer,
        id,
        kept,
        objects: list,
      } => {
        for object in list {
          if seen.insert(object.id.clone()) {
            homes.push((layer, id, kept));
            objects.push(object);
          }
        }
      }
      Pending::Tiles {
        layer,
        id,
        cell_size,
        found,
        live,
        kept,
      } => tiles.push((layer, id, cell_size, found, live, kept)),
    }
  }
  let mut existing: Vec<(String, Entity)> = world
    .get::<MapIds>(map)
    .map(|ids| {
      ids
        .objects
        .iter()
        .filter(|&(_, &entity)| {
          world
            .get::<MapObject>(entity)
            .is_some_and(|object| object.map == map)
        })
        .map(|(id, &entity)| (id.clone(), entity))
        .collect()
    })
    .unwrap_or_default();
  existing.sort_by_key(|(_, entity)| *entity);
  let was_in = |id: &str| applied.objects.get(id);
  let matched = reconcile(&existing, &objects, |i| {
    let (_, layer, kept) = homes[i];
    kept && was_in(&objects[i].id).is_some_and(|(at, was)| *at == layer && *was == objects[i])
  });

  // An object changed into another layer (or whose layer spawned afresh)
  // moves there before the layers that went despawn
  for &(entity, i) in &matched.update {
    let (layer, id, kept) = homes[i];
    let moved = !kept || was_in(&objects[i].id).is_none_or(|(at, _)| *at != id);
    if moved && let Ok(mut object) = world.get_entity_mut(entity) {
      object.insert(ChildOf(layer));
    }
  }
  // The new objects' entities, so the id table is whole before any of the
  // objects' components arrive (observers resolve references through it)
  let gone: FastSet<Entity> = matched.despawn.iter().copied().collect();
  let mut ids: FastMap<String, Entity> = existing
    .into_iter()
    .filter(|(_, entity)| !gone.contains(entity))
    .collect();
  let mut arriving = Vec::with_capacity(matched.spawn.len());
  for &i in &matched.spawn {
    let (layer, ..) = homes[i];
    if world.get_entity(layer).is_err() {
      continue;
    }
    let (component, transform) = object_parts(map, objects[i].clone());
    let entity = child_of(world, layer, transform);
    ids.insert(component.id.clone(), entity);
    arriving.push((entity, component));
  }
  let Ok(mut root_entity) = world.get_entity_mut(map) else {
    return;
  };
  root_entity.insert((MapIds { objects: ids }, MapLayers(entities.clone())));
  for &entity in &matched.despawn {
    let _ = world.try_despawn(entity);
  }
  for layer in old_layers.into_iter().filter(|l| !entities.contains(l)) {
    let _ = world.try_despawn(layer);
  }
  for &(entity, i) in &matched.update {
    let (component, transform) = object_parts(map, objects[i].clone());
    if let Ok(mut object) = world.get_entity_mut(entity) {
      object.insert((component, transform));
    }
  }
  for (entity, component) in arriving {
    if let Ok(mut object) = world.get_entity_mut(entity) {
      object.insert(component);
    }
  }

  // The class tiles, by layer and cell
  let mut class_tiles = FastMap::default();
  for (layer, id, cell_size, found, live, kept) in tiles {
    if world.get_entity(map).is_err() {
      return;
    }
    for tile in &found {
      let file = (tile.tile, tile.class.clone(), tile.properties.clone());
      class_tiles.insert((id, tile.cell), file);
    }
    let was = |cell: IVec2| applied.class_tiles.get(&(id, cell));
    let layer = ClassLayer {
      layer,
      cell_size,
      live,
      kept,
    };
    reconcile_class_tiles(world, map, layer, found, was);
  }

  let objects = objects
    .into_iter()
    .zip(homes)
    .map(|(object, (_, layer, _))| (object.id.clone(), (layer, object)))
    .collect();
  let Ok(mut root_entity) = world.get_entity_mut(map) else {
    return;
  };
  root_entity.insert(MapApplied {
    stamp,
    layers: files,
    objects,
    class_tiles,
  });
  send(world, MapEvent::Reloaded { map });
}

/// Puts `value` on `entity` if it holds another (or none)
fn replace<C: Component + PartialEq>(world: &mut World, entity: Entity, value: C) {
  if let Ok(mut held) = world.get_entity_mut(entity)
    && held.get::<C>() != Some(&value)
  {
    held.insert(value);
  }
}

/// Brings the kept layer `entity`, now layer `index`, up to the file's
/// `layer`: its place, info and cells (or picture). What spawns once the
/// layers are in place (its objects, its class tiles) is handed back
fn update_layer(
  world: &mut World,
  map: Entity,
  (entity, index): (Entity, usize),
  MapLayer { info, kind }: MapLayer,
  classes: &ClassTiles,
) -> LayerRest {
  let layer_ref = MapLayerRef {
    map,
    index,
    name: info.name.clone(),
  };
  replace(world, entity, layer_ref);
  replace::<LayerInfo>(world, entity, info);
  match kind {
    LayerKind::Tiles {
      mut grid,
      intgrid,
      cell_size,
    } => {
      let found = take_class_tiles(&mut grid, classes);
      take_cells(world, entity, &grid, intgrid);
      LayerRest::Tiles(cell_size.as_vec2(), found)
    }
    LayerKind::Objects(objects) => LayerRest::Objects(objects),
    LayerKind::Image(image) => {
      replace::<ImageLayer>(world, entity, image);
      LayerRest::Nothing
    }
  }
}

/// The kept tile layer `entity` takes the file's cells and IntGrid: only
/// the chunks whose tiles or values differ move their versions, so only
/// they draw, collide and occlude again
fn take_cells(world: &mut World, entity: Entity, grid: &TileGrid, intgrid: Option<IntGrid>) {
  let Some(mut data) = world.get_mut::<TileLayerData>(entity) else {
    return;
  };
  let TileLayerData {
    grid: cells,
    intgrid: values,
    ..
  } = data.bypass_change_detection();
  let moved = cells.assign(grid, |chunk| {
    values_differ(values.as_ref(), intgrid.as_ref(), chunk)
  });
  let new_values = *values != intgrid;
  if new_values {
    *values = intgrid;
  }
  if moved || new_values {
    data.set_changed();
  }
}

/// Whether two IntGrids' values differ in `chunk` (where one is missing, or
/// they differ in size, everywhere)
fn values_differ(old: Option<&IntGrid>, new: Option<&IntGrid>, chunk: UVec2) -> bool {
  match (old, new) {
    (None, None) => false,
    (Some(a), Some(b)) if a.size == b.size => {
      let width = a.size.x as usize;
      let x0 = (chunk.x * CHUNK_SIZE).min(a.size.x) as usize;
      let x1 = ((chunk.x + 1) * CHUNK_SIZE).min(a.size.x) as usize;
      let rows = (chunk.y * CHUNK_SIZE).min(a.size.y)..((chunk.y + 1) * CHUNK_SIZE).min(a.size.y);
      rows.into_iter().any(|y| {
        let row = y as usize * width;
        a.values.get(row + x0..row + x1) != b.values.get(row + x0..row + x1)
      })
    }
    _ => true,
  }
}

/// The class tiles alive on the layer entity `layer`, by cell
fn live_class_tiles(world: &World, layer: Entity) -> FastMap<IVec2, Entity> {
  world
    .get::<LayerTiles>(layer)
    .map(|table| {
      table
        .0
        .iter()
        .filter(|&(&cell, &entity)| world.get::<MapTile>(entity).is_some_and(|t| t.cell == cell))
        .map(|(&cell, &entity)| (cell, entity))
        .collect()
    })
    .unwrap_or_default()
}

/// A tile layer whose class tiles a reload reconciles
struct ClassLayer {
  /// Its entity and index
  layer: (Entity, usize),
  cell_size: Vec2,
  /// The class tiles alive on it before, by cell (on its old entity, for a
  /// layer spawned afresh: those went with it)
  live: FastMap<IVec2, Entity>,
  /// Whether it kept its entity
  kept: bool,
}

/// The class tiles of a reloaded tile layer meet the file's (`found`) by
/// cell, each judged against the file's last value (`was`), not its live
/// `MapTile`, which the game may have changed. On a kept layer, one the
/// file left as it was keeps its entity untouched (its `MapTile` told its
/// layer's new index) and one it changed takes a new `MapTile` and
/// `Transform`; any other live one (a cell the file emptied, or one an
/// edit placed) despawns. A tile without a live one spawns if the file
/// added or changed it, or if one lived on the layer's old entity; one the
/// game despawned (or an edit took away) stays away while the file leaves
/// it as it was
fn reconcile_class_tiles<'a>(
  world: &mut World,
  map: Entity,
  ClassLayer {
    layer: (layer, index),
    cell_size,
    mut live,
    kept,
  }: ClassLayer,
  found: Vec<ClassTile>,
  was: impl Fn(IVec2) -> Option<&'a FileClassTile>,
) {
  let mut table = FastMap::default();
  for tile in found {
    if world.get_entity(layer).is_err() {
      return;
    }
    let cell = tile.cell;
    let unchanged = was(cell).is_some_and(|(then, class, properties)| {
      *then == tile.tile && *class == tile.class && *properties == tile.properties
    });
    match live.remove(&cell) {
      Some(entity) if kept => {
        if unchanged {
          if let Some(mut held) = world.get_mut::<MapTile>(entity)
            && held.layer != index
          {
            held.layer = index;
          }
        } else if let Ok(mut held) = world.get_entity_mut(entity) {
          let centre = (cell.as_vec2() + 0.5) * cell_size;
          held.insert((
            MapTile {
              map,
              layer: index,
              cell,
              tile: tile.tile,
              class: tile.class,
              properties: tile.properties,
            },
            Transform::from_translation(centre),
          ));
        }
        table.insert(cell, entity);
      }
      lived if lived.is_some() || !unchanged => {
        let entity = spawn_class_tile(world, map, (layer, index), cell_size, tile);
        table.insert(cell, entity);
      }
      _ => {}
    }
  }
  if kept {
    for entity in live.into_values() {
      let _ = world.try_despawn(entity);
    }
  }
  if let Some(mut tiles) = world.get_mut::<LayerTiles>(layer) {
    tiles.0 = table;
  }
}

#[cfg(test)]
mod tests;
