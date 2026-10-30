//! Maps as entities. A [`MapRoot`] names a map asset (a [`TileMap`], or a
//! level of an [`LdtkProject`]); once the asset and its tilesets are loaded,
//! `spawn_maps` gives the map entity a child per layer, and each object and
//! class tile a child of its layer.

use std::borrow::Cow;
use std::sync::Arc;

use glam::{IVec2, UVec2, Vec2};

use super::{
  ImageLayer, IntGrid, LayerInfo, LayerKind, LdtkProject, MapObjectData, ObjectShape, Properties,
  PropertyValue, Tile, TileData, TileGrid, TileMap, Tileset, TilesetSource,
};
use crate::core::asset::{AssetError, AssetPath, AssetServer, Assets, Handle, LoadState};
use crate::core::ecs::kernel::{DeferredWorld, HookContext, QueryState};
use crate::core::ecs::{ChildOf, Component, Entity, Message, Messages, Transform, Without, World};
use crate::core::hash::FastMap;
use crate::core::messages::register_message;
use crate::core::physics::TileCell;

/// An IntGrid value, by number or by the name the editor gave it
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IntGridKey {
  Value(i32),
  Name(String),
}

impl From<i32> for IntGridKey {
  fn from(value: i32) -> Self {
    IntGridKey::Value(value)
  }
}

impl From<&str> for IntGridKey {
  fn from(name: &str) -> Self {
    IntGridKey::Name(name.to_string())
  }
}

/// Where a map's layers come from
#[derive(Clone, Debug)]
pub(crate) enum MapSource {
  Map(Handle<TileMap>),
  /// A level of a project, by identifier or iid
  Ldtk {
    project: Handle<LdtkProject>,
    level: String,
  },
}

/// A map in the world. The entity's `Transform` places the map's top-left
/// corner; once its asset (and the tilesets it names) is loaded, its layers
/// spawn as its children, each object and class tile a child of its layer,
/// and a [`MapEvent::Spawned`] is sent ([`MapEvent::Failed`] if the asset
/// cannot load). Read and edit it through `ctx.maps()`. Despawning the
/// entity despawns all of it; putting another root over it, or taking the
/// root away, takes its layers down first.
///
/// The map collides: its colliding tile layers ([`LayerInfo::collides`])
/// keep a [`TileColliders`](crate::core::physics::TileColliders) in step
/// with their tiles, on the map entity for the layers of the map's own tile
/// size and no offset (a cell taking the strongest kind across them:
/// solid, then a shape, then a slope, then one-way; of two shapes the upper
/// layer's), and on a layer's entity for a layer of another cell size or
/// offset. A layer with an IntGrid collides by its values' names, any other
/// by its tiles' collision shapes, placed where each tile draws. The
/// properties `collision_layer`, `collision_mask`, `friction` and
/// `restitution` give the colliders' material, a layer's over the map's (a
/// layer with a material of its own collides on its own entity). Its
/// occluding layers ([`LayerInfo::occludes`], else as they collide) cast
/// 2D shadows through
/// [`MapOccluder`](super::MapOccluder) children. Edits re-collide and
/// re-shadow only the chunks they touch; a disabled map has neither.
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn spawn_level(ctx: &mut SceneContext<'_>) -> Entity {
///   let level = ctx.assets().load::<TileMap>("levels/one.tmx");
///   let root = MapRoot::new(level).with_entity_tiles(&["coin"]).with_z(-10);
///   ctx.world.spawn((root, Transform::from_xy(0.0, 0.0))).id()
/// }
/// ```
#[derive(Component, Clone, Debug)]
#[require(Transform)]
#[component(on_discard = root_discarded)]
pub struct MapRoot {
  pub(crate) source: MapSource,
  /// Tile classes whose tiles spawn as entities
  pub(crate) entity_tiles: Vec<String>,
  /// IntGrid values that collide as a cell kind, over the defaults
  pub(crate) intgrid_collision: Vec<(IntGridKey, TileCell)>,
  pub(crate) collision: bool,
  /// The bottom layer's z; layer `i` draws at `z + i`
  pub(crate) z: i32,
}

impl MapRoot {
  /// The map `map`
  pub fn new(map: Handle<TileMap>) -> MapRoot {
    MapRoot::from_source(MapSource::Map(map))
  }

  /// The level of `project` whose identifier or iid is `level`
  pub fn ldtk(project: Handle<LdtkProject>, level: &str) -> MapRoot {
    MapRoot::from_source(MapSource::Ldtk {
      project,
      level: level.to_string(),
    })
  }

  fn from_source(source: MapSource) -> MapRoot {
    MapRoot {
      source,
      entity_tiles: Vec::new(),
      intgrid_collision: Vec::new(),
      collision: true,
      z: 0,
    }
  }

  /// Tiles whose tileset class is one of `classes` spawn as entities with a
  /// [`MapTile`], and leave their layer: neither drawn nor collided
  pub fn with_entity_tiles(mut self, classes: &[&str]) -> MapRoot {
    for class in classes {
      if !self.entity_tiles.iter().any(|c| c == class) {
        self.entity_tiles.push(class.to_string());
      }
    }
    self
  }

  /// Cells holding the IntGrid value `key` collide as `cell`, over the
  /// defaults the value's name gives and any earlier call for the same key.
  /// By default the names `solid`, `one_way`, `slope_bottom_left`,
  /// `slope_bottom_right`, `slope_top_left` and `slope_top_right` collide as
  /// they say and every other value as nothing; names compare ignoring ASCII
  /// case (LDtk capitalises identifiers), and a value's own override wins
  /// over its name's. A `TileCell::Shape` here names no shape and collides
  /// as nothing
  pub fn with_intgrid_collision(mut self, key: impl Into<IntGridKey>, cell: TileCell) -> MapRoot {
    let key = key.into();
    match self.intgrid_collision.iter_mut().find(|(k, _)| *k == key) {
      Some(entry) => entry.1 = cell,
      None => self.intgrid_collision.push((key, cell)),
    }
    self
  }

  /// The map collides with nothing; its layers still cast shadows, as
  /// their `occludes` (else `collides`) says
  pub fn without_collision(mut self) -> MapRoot {
    self.collision = false;
    self
  }

  /// The z of the bottom layer; each layer above draws one higher
  pub fn with_z(mut self, z: i32) -> MapRoot {
    self.z = z;
    self
  }
}

/// A layer of a spawned map: a child of the map entity, carrying the layer's
/// [`LayerInfo`] and a `Transform` at its offset, and [`TileLayerData`] for a
/// tile layer or [`ImageLayer`] for an image layer
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct MapLayerRef {
  pub map: Entity,
  /// Bottom first, as in the asset
  pub index: usize,
  pub name: String,
}

/// A tile layer's cells, the spawned map's own copy: edits here change this
/// map only (`ctx.maps()` makes them)
#[derive(Component, Clone, Debug)]
pub struct TileLayerData {
  pub grid: TileGrid,
  pub intgrid: Option<IntGrid>,
  /// A cell's size in pixels in the layer's space
  pub cell_size: UVec2,
}

/// An object of a spawned map: a child of its object layer, its `Transform`
/// at the centre of its rectangle (its point, for a point; the origin of its
/// points, for a polygon or polyline, which Tiled gives no size) and turned
/// by its rotation. A polygon's or polyline's points are relative to the
/// `Transform`
#[derive(Component, Clone, Debug)]
pub struct MapObject {
  /// The map entity it belongs to
  pub map: Entity,
  /// Unique in the map: Tiled's object id, LDtk's iid
  pub id: String,
  pub name: String,
  pub class: Option<String>,
  pub shape: ObjectShape,
  /// The rectangle's width and height in pixels
  pub size: Vec2,
  /// The tile shown, for a tile object
  pub tile: Option<Tile>,
  /// False for an object the editor hides: it spawns all the same (games
  /// use hidden markers), and is not drawn
  pub visible: bool,
  pub properties: Properties,
}

impl MapObject {
  /// The object the object-reference property `field` names, in the same
  /// map; `None` if the field is not a reference, is unset, or names an
  /// object the map does not have (or no longer has)
  pub fn reference(&self, field: &str, world: &World) -> Option<Entity> {
    let PropertyValue::Object(id) = self.properties.get(field)? else {
      return None;
    };
    // Tiled writes an unset reference as object 0
    if id.is_empty() || id == "0" {
      return None;
    }
    let entity = world.get::<MapIds>(self.map)?.get(id)?;
    world.get_entity(entity).is_ok().then_some(entity)
  }
}

/// A tile whose class the map's root registered
/// ([`MapRoot::with_entity_tiles`]), spawned as an entity instead of going
/// in its layer's grid: a child of its layer, its `Transform` at the cell's
/// centre. The map spawns one for each such tile in the file, and
/// `ctx.maps()` one for each it places; placing another tile on the cell,
/// or clearing it, despawns it
#[derive(Component, Clone, Debug)]
pub struct MapTile {
  pub map: Entity,
  /// The layer's index
  pub layer: usize,
  pub cell: IVec2,
  /// The tile, with its flips
  pub tile: Tile,
  pub class: String,
  /// The tile's properties, from its tileset
  pub properties: Properties,
}

/// A spawned map's objects by their id, on the map entity
#[derive(Component, Clone, Debug, Default)]
pub struct MapIds {
  objects: FastMap<String, Entity>,
}

impl MapIds {
  /// The entity of the object `id`
  pub fn get(&self, id: &str) -> Option<Entity> {
    self.objects.get(id).copied()
  }
}

/// A spawned map's layer entities, bottom first, on the map entity
#[derive(Component, Clone, Debug, Default)]
pub(crate) struct MapLayers(pub(crate) Vec<Entity>);

/// A tile layer's class tiles by cell, on the layer entity. An entry may
/// name an entity a game has since despawned; readers check
#[derive(Component, Clone, Debug, Default)]
pub(crate) struct LayerTiles(pub(crate) FastMap<IVec2, Entity>);

/// What happened to a map entity's asset
#[derive(Message, Clone, Debug)]
pub enum MapEvent {
  /// Its layers, objects and class tiles spawned
  Spawned { map: Entity },
  /// Its asset changed and its layers and objects follow it
  Reloaded { map: Entity },
  /// Its asset did not load (or has no such level): nothing spawned
  Failed { map: Entity, error: Arc<AssetError> },
}

/// The failure a waiting map reported, so it reports each failure once
#[derive(Component, Clone, Debug)]
pub(crate) struct MapFailed {
  error: Arc<AssetError>,
  /// The project's version, for a level the project lacks
  version: Option<u64>,
}

/// The maps not spawned yet, with the failure each reported
pub(crate) type WaitingMaps =
  QueryState<(Entity, &'static MapRoot, Option<&'static MapFailed>), Without<MapIds>>;

/// The map a root names, once its asset is in its store
pub(crate) fn map_asset<'w>(world: &'w World, root: &MapRoot) -> Option<&'w TileMap> {
  root_map(
    root,
    world.get_resource::<Assets<TileMap>>(),
    world.get_resource::<Assets<LdtkProject>>(),
  )
}

/// The map a root names, from the stores a system holds
pub(crate) fn root_map<'a>(
  root: &MapRoot,
  maps: Option<&'a Assets<TileMap>>,
  projects: Option<&'a Assets<LdtkProject>>,
) -> Option<&'a TileMap> {
  match &root.source {
    MapSource::Map(handle) => maps?.get(handle),
    MapSource::Ldtk { project, level } => Some(&projects?.get(project)?.level(level)?.map),
  }
}

/// What `tile` carries in the root's map's tileset
pub(crate) fn tile_data<'w>(world: &'w World, root: &MapRoot, tile: Tile) -> Option<&'w TileData> {
  let map = map_asset(world, root)?;
  let tileset = match map.tilesets.get(tile.tileset as usize)? {
    TilesetSource::Asset(handle) => world.get_resource::<Assets<Tileset>>()?.get(handle)?,
    TilesetSource::Embedded(tileset) => tileset,
  };
  tileset.tile(tile.id)
}

/// The class and properties of `tile`, if `map`'s root registers its class
/// to spawn as entities
pub(crate) fn registered_class(
  world: &World,
  map: Entity,
  tile: Tile,
) -> Option<(String, Properties)> {
  let root = world.get::<MapRoot>(map)?;
  if root.entity_tiles.is_empty() {
    return None;
  }
  let data = tile_data(world, root, tile)?;
  let class = data
    .class
    .as_ref()
    .filter(|c| root.entity_tiles.contains(c))?;
  Some((class.clone(), data.properties.clone()))
}

/// `PreUpdate`, after the asset pump: each map whose asset is ready spawns
/// its layers, objects and class tiles (so a map whose asset lands this
/// frame spawns this frame), and each whose asset failed reports it once.
/// With no map waiting it is one query over no entities
pub(crate) fn spawn_maps(world: &mut World, waiting: &mut WaitingMaps) {
  let decided: Vec<(Entity, Ready)> = waiting
    .iter(world)
    .filter_map(|(map, root, failed)| readiness(world, root, failed).map(|ready| (map, ready)))
    .collect();
  for (map, ready) in decided {
    match ready {
      Ready::Spawn => spawn_map(world, map),
      // A game's observer, as another map spawned, may have despawned it
      Ready::Failed(failure) => {
        let error = Arc::clone(&failure.error);
        if let Ok(mut entity) = world.get_entity_mut(map) {
          entity.insert(failure);
          send(world, MapEvent::Failed { map, error });
        }
      }
    }
  }
}

/// What a waiting map does this frame
enum Ready {
  Spawn,
  /// A failure it has not reported
  Failed(MapFailed),
}

/// Whether the root's asset is ready to spawn, failed in a way not yet
/// reported, or neither (still loading, or failed as already reported)
fn readiness(world: &World, root: &MapRoot, failed: Option<&MapFailed>) -> Option<Ready> {
  let server = world.get_resource::<AssetServer>()?;
  // The registry shares one error per failure, so the same failure is the same `Arc`
  let unreported = |error: Arc<AssetError>| {
    let reported = failed.is_some_and(|f| Arc::ptr_eq(&f.error, &error));
    (!reported).then_some(Ready::Failed(MapFailed {
      error,
      version: None,
    }))
  };
  match &root.source {
    MapSource::Map(handle) => match server.state(handle) {
      LoadState::Failed(error) => unreported(error),
      LoadState::Loaded => {
        let map = world.get_resource::<Assets<TileMap>>()?.get(handle)?;
        // A loaded file's tilesets are loaded; a map built in code may name
        // some that are not yet
        for source in &map.tilesets {
          if let TilesetSource::Asset(tileset) = source {
            match server.state(tileset) {
              LoadState::Loaded => {}
              LoadState::Failed(error) => return unreported(error),
              LoadState::Loading | LoadState::NotLoaded => return None,
            }
          }
        }
        Some(Ready::Spawn)
      }
      LoadState::Loading | LoadState::NotLoaded => None,
    },
    MapSource::Ldtk { project, level } => match server.state(project) {
      LoadState::Failed(error) => unreported(error),
      LoadState::Loaded => {
        let projects = world.get_resource::<Assets<LdtkProject>>()?;
        if projects.get(project)?.level(level).is_some() {
          return Some(Ready::Spawn);
        }
        // Reported once per version of the project
        let version = projects.version(project);
        if failed.is_some_and(|f| f.version.is_some() && f.version == version) {
          return None;
        }
        let path = server
          .path(project)
          .unwrap_or_else(|| AssetPath::from_normal(Cow::Borrowed("ldtk")));
        let error = AssetError::decode(&path, format!("the project has no level {level:?}"));
        Some(Ready::Failed(MapFailed {
          error: Arc::new(error),
          version,
        }))
      }
      LoadState::Loading | LoadState::NotLoaded => None,
    },
  }
}

/// A tile to spawn as an entity
pub(crate) struct ClassTile {
  pub(crate) cell: IVec2,
  pub(crate) tile: Tile,
  pub(crate) class: String,
  pub(crate) properties: Properties,
}

/// Spawns `tile` as a child of `layer` (the map's layer `index`, of cells
/// `cell_size` pixels) at its cell's centre
pub(crate) fn spawn_class_tile(
  world: &mut World,
  map: Entity,
  (layer, index): (Entity, usize),
  cell_size: Vec2,
  tile: ClassTile,
) -> Entity {
  let centre = (tile.cell.as_vec2() + 0.5) * cell_size;
  let component = MapTile {
    map,
    layer: index,
    cell: tile.cell,
    tile: tile.tile,
    class: tile.class,
    properties: tile.properties,
  };
  let entity = child_of(world, layer, Transform::from_translation(centre));
  if let Ok(mut spawned) = world.get_entity_mut(entity) {
    spawned.insert(component);
  }
  entity
}

/// A child of `parent` at `transform`, linked into `parent`'s `Children`
/// before anything a game observes goes on it. bevy runs `Add` observers
/// before `ChildOf`'s insert hook, so an observer of a component spawned
/// with the `ChildOf` that despawned the parent would leave the child
/// orphaned and the hook's queued update failing on the gone parent
fn child_of(world: &mut World, parent: Entity, transform: Transform) -> Entity {
  world.spawn((transform, ChildOf(parent))).id()
}

/// A layer as it spawns: its info and what it holds
struct PlannedLayer {
  info: LayerInfo,
  content: Content,
}

enum Content {
  Tiles(TileLayerData, Vec<ClassTile>),
  Objects(Vec<MapObjectData>),
  Image(ImageLayer),
}

/// For each of a map's tilesets, its tiles of a registered class, with
/// their class and properties
type ClassTiles = Vec<FastMap<u32, (String, Properties)>>;

fn class_tiles(world: &World, map: &TileMap, classes: &[String]) -> ClassTiles {
  if classes.is_empty() {
    return Vec::new();
  }
  let assets = world.get_resource::<Assets<Tileset>>();
  map
    .tilesets
    .iter()
    .map(|source| {
      let tileset = match source {
        TilesetSource::Asset(handle) => assets.and_then(|a| a.get(handle)),
        TilesetSource::Embedded(tileset) => Some(&**tileset),
      };
      tileset
        .map(|tileset| {
          tileset
            .tiles()
            .filter_map(|(id, data)| {
              let class = data.class.as_ref().filter(|c| classes.contains(c))?;
              Some((id, (class.clone(), data.properties.clone())))
            })
            .collect()
        })
        .unwrap_or_default()
    })
    .collect()
}

/// Takes the tiles of a registered class out of `grid`
fn take_class_tiles(grid: &mut TileGrid, classes: &ClassTiles) -> Vec<ClassTile> {
  let mut found = Vec::new();
  if classes.iter().all(FastMap::is_empty) {
    return found;
  }
  let size = grid.size().as_ivec2();
  for y in 0..size.y {
    for x in 0..size.x {
      let cell = IVec2::new(x, y);
      let Some(tile) = grid.get(cell) else {
        continue;
      };
      let Some((class, properties)) = classes
        .get(tile.tileset as usize)
        .and_then(|tiles| tiles.get(&tile.id))
      else {
        continue;
      };
      found.push(ClassTile {
        cell,
        tile,
        class: class.clone(),
        properties: properties.clone(),
      });
      grid.set(cell, None);
    }
  }
  found
}

/// The map's layers as they will spawn: each tile layer a copy of its grid
/// (class tiles taken out, every chunk dirty so it is built once)
fn plan(world: &World, root: &MapRoot) -> Option<Vec<PlannedLayer>> {
  let map = map_asset(world, root)?;
  let classes = class_tiles(world, map, &root.entity_tiles);
  let layers = map
    .layers
    .iter()
    .map(|layer| PlannedLayer {
      info: layer.info.clone(),
      content: match &layer.kind {
        LayerKind::Tiles {
          grid,
          intgrid,
          cell_size,
        } => {
          let mut grid = grid.clone();
          let found = take_class_tiles(&mut grid, &classes);
          grid.mark_all_dirty();
          let data = TileLayerData {
            grid,
            intgrid: intgrid.clone(),
            cell_size: *cell_size,
          };
          Content::Tiles(data, found)
        }
        LayerKind::Objects(objects) => Content::Objects(objects.clone()),
        LayerKind::Image(image) => Content::Image(image.clone()),
      },
    })
    .collect();
  Some(layers)
}

/// An object's component and its `Transform` in its layer
fn object_parts(map: Entity, object: MapObjectData) -> (MapObject, Transform) {
  let half = object.size * 0.5;
  // The rectangle turns about its top-left corner
  let centre = object.position + Vec2::from_angle(object.rotation).rotate(half);
  let from_centre = |points: Vec<Vec2>| points.into_iter().map(|p| p - half).collect();
  let shape = match object.shape {
    ObjectShape::Polygon(points) => ObjectShape::Polygon(from_centre(points)),
    ObjectShape::Polyline(points) => ObjectShape::Polyline(from_centre(points)),
    shape => shape,
  };
  let transform = Transform::from_translation(centre).with_rotation(object.rotation);
  let component = MapObject {
    map,
    id: object.id,
    name: object.name,
    class: object.class,
    shape,
    size: object.size,
    tile: object.tile,
    visible: object.visible,
    properties: object.properties,
  };
  (component, transform)
}

/// Spawns the layers, objects and class tiles of `map`, if it is still a
/// map waiting to spawn: an entity despawned since (with its scene) or no
/// longer a map spawns nothing
fn spawn_map(world: &mut World, map: Entity) {
  let Some(root) = world
    .get_entity(map)
    .ok()
    .and_then(|entity| entity.get::<MapRoot>().cloned())
  else {
    return;
  };
  if world.get::<MapIds>(map).is_some() {
    return;
  }
  let Some(planned) = plan(world, &root) else {
    return;
  };

  // A game's observers run as each part arrives, and may despawn the map, a
  // layer or an object before its turn: what is gone is skipped, and a map
  // gone takes all of it with it
  let alive = |world: &World, entity: Entity| world.get_entity(entity).is_ok();
  let mut layers = Vec::with_capacity(planned.len());
  let mut object_layers = Vec::new();
  let mut tile_layers = Vec::new();
  for (index, PlannedLayer { info, content }) in planned.into_iter().enumerate() {
    if !alive(world, map) {
      return;
    }
    let layer = child_of(world, map, Transform::from_translation(info.offset));
    layers.push(layer);
    let layer_ref = MapLayerRef {
      map,
      index,
      name: info.name.clone(),
    };
    let Ok(mut entity) = world.get_entity_mut(layer) else {
      continue;
    };
    match content {
      Content::Tiles(data, found) => {
        let cell_size = data.cell_size.as_vec2();
        entity.insert((layer_ref, info, data, LayerTiles::default()));
        tile_layers.push((layer, index, cell_size, found));
      }
      Content::Objects(objects) => {
        entity.insert((layer_ref, info));
        object_layers.push((layer, objects));
      }
      Content::Image(image) => {
        entity.insert((layer_ref, info, image));
      }
    }
  }

  // The objects' entities first, so the id table is whole by the time their
  // components arrive, for observers that resolve references
  let mut ids = FastMap::default();
  let mut objects = Vec::new();
  for (layer, list) in object_layers {
    for object in list {
      if !alive(world, layer) {
        break;
      }
      let (component, transform) = object_parts(map, object);
      let entity = child_of(world, layer, transform);
      ids.insert(component.id.clone(), entity);
      objects.push((entity, component));
    }
  }
  let Ok(mut root) = world.get_entity_mut(map) else {
    return;
  };
  root
    .insert((MapIds { objects: ids }, MapLayers(layers)))
    .remove::<MapFailed>();
  for (entity, component) in objects {
    if let Ok(mut object) = world.get_entity_mut(entity) {
      object.insert(component);
    }
  }
  for (layer, index, cell_size, found) in tile_layers {
    let mut spawned = FastMap::default();
    for tile in found {
      if !alive(world, layer) {
        break;
      }
      let cell = tile.cell;
      let entity = spawn_class_tile(world, map, (layer, index), cell_size, tile);
      spawned.insert(cell, entity);
    }
    if let Some(mut table) = world.get_mut::<LayerTiles>(layer) {
      table.0.extend(spawned);
    }
  }
  if alive(world, map) {
    send(world, MapEvent::Spawned { map });
  }
}

/// A root going (put over by another, taken away, or despawned with its
/// entity): a map that spawned takes its layers down, so a new root spawns
/// afresh. Despawned, the hierarchy takes them anyway
fn root_discarded(mut world: DeferredWorld, context: HookContext) {
  let map = context.entity;
  if world.get::<MapLayers>(map).is_none() && world.get::<MapFailed>(map).is_none() {
    return;
  }
  world.commands().queue(move |world: &mut World| {
    let Ok(mut entity) = world.get_entity_mut(map) else {
      return;
    };
    let layers = entity.take::<MapLayers>();
    entity.remove::<(MapIds, MapFailed)>();
    for layer in layers.map(|l| l.0).unwrap_or_default() {
      let _ = world.try_despawn(layer);
    }
  });
}

fn send(world: &mut World, event: MapEvent) {
  register_message::<MapEvent>(world);
  world.resource_mut::<Messages<MapEvent>>().write(event);
}

#[cfg(test)]
mod tests {
  use std::f32::consts::FRAC_PI_2;
  use std::sync::Mutex;
  use std::sync::atomic::{AtomicBool, Ordering};
  use std::time::{Duration, Instant};

  use glam::{IVec2, UVec2, Vec2};

  use super::*;
  use crate::core::asset::{AssetsView, pump};
  use crate::core::ecs::kernel::{Add, MessageCursor, Messages, Schedule};
  use crate::core::ecs::world_ext::despawn_game_entities;
  use crate::core::ecs::{ChildOf, Commands, GlobalTransform, On, Query, Res, Resource};
  use crate::core::engine::EngineBuilder;
  use crate::core::scene::{Scene, SceneAction, SceneContext};
  use crate::core::testing::HeadlessEngine;
  use crate::core::tilemap::test_util::{files_with, world_over};
  use crate::core::tilemap::{
    LayerKind, LayerRef, MapLayer, MapObjectData, MapsView, PropertyValue, TileData, TileFlip,
    Tileset, TilesetImage, TilesetSource,
  };

  /// A frame's map work: the asset pump, then the spawn system
  struct Frames(Schedule);

  impl Frames {
    fn new() -> Frames {
      let mut schedule = Schedule::default();
      schedule.add_systems(spawn_maps);
      Frames(schedule)
    }

    /// The spawn system alone, with no pump before it
    fn spawn(&mut self, world: &mut World) {
      self.0.run(world);
    }

    fn run(&mut self, world: &mut World) {
      pump(world);
      self.0.run(world);
    }

    /// Frames until `done`, which is then checked after the frame's spawn
    fn until(&mut self, world: &mut World, done: impl Fn(&World) -> bool) {
      let start = Instant::now();
      loop {
        self.run(world);
        if done(world) {
          return;
        }
        assert!(
          start.elapsed() < Duration::from_secs(5),
          "timed out waiting"
        );
        std::thread::sleep(Duration::from_millis(1));
      }
    }
  }

  /// The map events sent since `cursor` last read
  fn events(world: &World, cursor: &mut MessageCursor<MapEvent>) -> Vec<MapEvent> {
    match world.get_resource::<Messages<MapEvent>>() {
      Some(messages) => cursor.read(messages).cloned().collect(),
      None => Vec::new(),
    }
  }

  fn spawned(events: &[MapEvent]) -> Vec<Entity> {
    events
      .iter()
      .filter_map(|e| match e {
        MapEvent::Spawned { map } => Some(*map),
        _ => None,
      })
      .collect()
  }

  fn failed(events: &[MapEvent]) -> Vec<(Entity, String)> {
    events
      .iter()
      .filter_map(|e| match e {
        MapEvent::Failed { map, error } => Some((*map, error.report())),
        _ => None,
      })
      .collect()
  }

  /// The map's layer entities, by index
  fn layers_of(world: &mut World, map: Entity) -> Vec<Entity> {
    let mut found: Vec<(usize, Entity)> = world
      .query::<(Entity, &MapLayerRef)>()
      .iter(world)
      .filter(|(_, l)| l.map == map)
      .map(|(e, l)| (l.index, e))
      .collect();
    found.sort();
    found.into_iter().map(|(_, e)| e).collect()
  }

  fn layer_names(world: &mut World, map: Entity) -> Vec<String> {
    layers_of(world, map)
      .into_iter()
      .map(|e| world.get::<MapLayerRef>(e).map(|l| l.name.clone()))
      .collect::<Option<_>>()
      .unwrap_or_default()
  }

  fn count<C: Component>(world: &mut World) -> usize {
    world.query::<&C>().iter(world).count()
  }

  /// The map's object called `name`
  fn object(world: &mut World, map: Entity, name: &str) -> (Entity, MapObject) {
    let found = world
      .query::<(Entity, &MapObject)>()
      .iter(world)
      .find(|(_, o)| o.map == map && o.name == name)
      .map(|(e, o)| (e, o.clone()));
    match found {
      Some(found) => found,
      None => panic!("no object {name}"),
    }
  }

  fn parent(world: &World, entity: Entity) -> Option<Entity> {
    world.get::<ChildOf>(entity).map(ChildOf::parent)
  }

  fn translation(world: &World, entity: Entity) -> Vec2 {
    match world.get::<Transform>(entity) {
      Some(t) => t.translation,
      None => panic!("{entity:?} has no transform"),
    }
  }

  fn global(world: &World, entity: Entity) -> Vec2 {
    match world.get::<GlobalTransform>(entity) {
      Some(t) => t.translation(),
      None => panic!("{entity:?} has no global transform"),
    }
  }

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 1e-3
  }

  /// A map of three layers over the fixture tileset: tiles (offset 4, 8),
  /// two objects, and an image
  const THREE_LAYERS: &str = r#"{"type":"map","orientation":"orthogonal","width":3,"height":2,
    "tilewidth":16,"tileheight":16,
    "tilesets":[{"firstgid":1,"source":"tiled/tilesets/tiles.tsj"}],
    "layers":[
      {"id":1,"name":"ground","type":"tilelayer","width":3,"height":2,"offsetx":4,"offsety":8,
        "data":[1,2,0,0,0,3]},
      {"id":2,"name":"things","type":"objectgroup","objects":[
        {"id":1,"name":"a","x":16,"y":0,"width":16,"height":16},
        {"id":2,"name":"b","x":0,"y":16,"width":0,"height":0,"point":true}]},
      {"id":3,"name":"sky","type":"imagelayer","image":"tiled/images/bg.png"}]}"#;

  #[test]
  fn a_map_spawns_its_layers_and_objects_when_ready() {
    let mut world = world_over(files_with(&[("m.tmj", THREE_LAYERS)]));
    let server = world.resource::<AssetServer>().clone();
    server.limit_workers(1);
    let handle = server.load::<TileMap>("m.tmj");
    let map = world
      .spawn((
        MapRoot::new(handle.clone()),
        Transform::from_xy(100.0, 50.0),
      ))
      .id();
    let mut cursor = MessageCursor::<MapEvent>::default();
    let mut frames = Frames::new();

    // Still loading: nothing yet
    frames.spawn(&mut world);
    assert!(layers_of(&mut world, map).is_empty());
    assert!(!MapsView::new(&mut world).is_ready(map));
    assert!(events(&world, &mut cursor).is_empty());

    frames.until(&mut world, |_| server.is_ready(&handle));
    let layers = layers_of(&mut world, map);
    assert_eq!(layer_names(&mut world, map), ["ground", "things", "sky"]);
    for (index, &layer) in layers.iter().enumerate() {
      assert_eq!(parent(&world, layer), Some(map));
      let layer_ref = world.get::<MapLayerRef>(layer).expect("layer ref");
      assert_eq!((layer_ref.map, layer_ref.index), (map, index));
      let info = world.get::<LayerInfo>(layer).expect("layer info");
      assert_eq!(info.name, layer_ref.name);
    }

    // The tile layer: its own copy of the grid, every chunk to build, at its offset
    let (ground, things, sky) = (layers[0], layers[1], layers[2]);
    let mut data = world
      .get::<TileLayerData>(ground)
      .expect("tile data")
      .clone();
    assert_eq!(data.cell_size, UVec2::splat(16));
    assert!(data.intgrid.is_none());
    assert_eq!(data.grid.get(IVec2::new(0, 0)), Some(Tile::new(0, 0)));
    assert_eq!(data.grid.get(IVec2::new(1, 0)), Some(Tile::new(0, 1)));
    assert_eq!(data.grid.get(IVec2::new(2, 1)), Some(Tile::new(0, 2)));
    assert_eq!(data.grid.get(IVec2::new(2, 0)), None);
    let mut dirty = Vec::new();
    data.grid.drain_dirty(&mut dirty);
    assert_eq!(dirty, [UVec2::ZERO]);
    assert_eq!(translation(&world, ground), Vec2::new(4.0, 8.0));
    assert_eq!(global(&world, ground), Vec2::new(104.0, 58.0));
    assert!(world.get::<TileLayerData>(things).is_none());

    // The object layer: an entity per object, found by id
    let (a, object_a) = object(&mut world, map, "a");
    let (b, _) = object(&mut world, map, "b");
    assert_eq!(parent(&world, a), Some(things));
    assert_eq!(parent(&world, b), Some(things));
    assert_eq!(object_a.id, "1");
    assert_eq!(
      world.get::<MapIds>(map).and_then(|ids| ids.get("1")),
      Some(a)
    );
    assert_eq!(
      world.get::<MapIds>(map).and_then(|ids| ids.get("2")),
      Some(b)
    );

    // The image layer
    assert!(world.get::<ImageLayer>(sky).is_some());

    assert_eq!(spawned(&events(&world, &mut cursor)), [map]);
    assert!(MapsView::new(&mut world).is_ready(map));

    // Spawned once
    frames.run(&mut world);
    frames.run(&mut world);
    assert_eq!(layers_of(&mut world, map).len(), 3);
    assert_eq!(count::<MapObject>(&mut world), 2);
    assert!(events(&world, &mut cursor).is_empty());

    // Despawning the map despawns all of it
    world.despawn(map);
    assert_eq!(count::<MapLayerRef>(&mut world), 0);
    assert_eq!(count::<MapObject>(&mut world), 0);
  }

  #[test]
  fn objects_carry_their_fields_and_resolve_references() {
    let mut world = world_over(files_with(&[]));
    let handle = AssetsView::new(&mut world)
      .load_now::<TileMap>("tiled/maps/objects.tmj")
      .expect("objects.tmj loads");
    let map = world.spawn(MapRoot::new(handle)).id();
    let mut frames = Frames::new();
    frames.run(&mut world);

    let (crate_entity, crate_object) = object(&mut world, map, "crate");
    assert_eq!(crate_object.map, map);
    assert_eq!(crate_object.id, "1");
    assert_eq!(crate_object.class.as_deref(), Some("Crate"));
    assert_eq!(crate_object.shape, ObjectShape::Rect);
    assert_eq!(crate_object.size, Vec2::new(32.0, 16.0));
    assert_eq!(crate_object.tile, None);
    assert_eq!(crate_object.properties.int("hp"), Some(25));
    // The centre of its rectangle, whose top-left is (16, 32)
    assert_eq!(translation(&world, crate_entity), Vec2::new(32.0, 40.0));
    let things = parent(&world, crate_entity).expect("a layer's child");
    assert_eq!(
      world.get::<MapLayerRef>(things).map(|l| l.name.as_str()),
      Some("things")
    );

    // References resolve to the object's entity
    let (spawn, _) = object(&mut world, map, "spawn");
    assert_eq!(crate_object.reference("target", &world), Some(spawn));
    assert_eq!(crate_object.reference("hp", &world), None);
    assert_eq!(crate_object.reference("nothing", &world), None);

    // Turned 90 degrees clockwise about its top-left (50, 50): the centre
    // of its 20 x 10 rectangle swings to (45, 60)
    let (turned, _) = object(&mut world, map, "turned");
    let t = world.get::<Transform>(turned).copied().expect("transform");
    assert!(close(t.translation, Vec2::new(45.0, 60.0)), "{t:?}");
    assert!((t.rotation - FRAC_PI_2).abs() < 1e-5);

    // A polygon sits at its origin, its points relative to it
    let (wedge, wedge_object) = object(&mut world, map, "wedge");
    assert_eq!(translation(&world, wedge), Vec2::new(40.0, 60.0));
    assert_eq!(
      wedge_object.shape,
      ObjectShape::Polygon(vec![Vec2::ZERO, Vec2::new(16.0, 0.0), Vec2::new(8.0, 12.0)])
    );
    let (point, _) = object(&mut world, map, "spawn");
    assert_eq!(translation(&world, point), Vec2::new(5.0, 6.0));

    // A tile object keeps its tile
    let (_, gem) = object(&mut world, map, "gem");
    assert_eq!(gem.tile, Some(Tile::new(1, 0)));
  }

  /// A 4 x 3 map of 16 px cells: tile layer "ground", object layer "things"
  fn code_map() -> TileMap {
    let mut map = TileMap::new(UVec2::new(4, 3), UVec2::splat(16));
    map.add_tile_layer("ground");
    map.add_object_layer("things");
    map
  }

  fn object_data(id: &str, name: &str, properties: &[(&str, PropertyValue)]) -> MapObjectData {
    let mut props = Properties::default();
    for (key, value) in properties {
      props.insert(*key, value.clone());
    }
    MapObjectData {
      id: id.to_string(),
      name: name.to_string(),
      class: None,
      position: Vec2::ZERO,
      size: Vec2::ZERO,
      rotation: 0.0,
      shape: ObjectShape::Point,
      tile: None,
      visible: true,
      properties: props,
    }
  }

  fn spawn_code_map(
    world: &mut World,
    map: TileMap,
    root: impl Fn(Handle<TileMap>) -> MapRoot,
  ) -> Entity {
    let handle = world.resource_mut::<Assets<TileMap>>().add(map);
    world.spawn(root(handle)).id()
  }

  #[test]
  fn unset_and_dangling_references_resolve_to_nothing() {
    let mut world = crate::core::testing::asset_world();
    let mut map = code_map();
    let refs = [
      ("unset", PropertyValue::Object("0".into())),
      ("empty", PropertyValue::Object(String::new())),
      ("missing", PropertyValue::Object("99".into())),
      ("me", PropertyValue::Object("7".into())),
      ("other", PropertyValue::Object("8".into())),
    ];
    if let LayerKind::Objects(objects) = &mut map.layers[1].kind {
      objects.push(object_data("7", "seven", &refs));
      objects.push(object_data("8", "eight", &[]));
    }
    let map = spawn_code_map(&mut world, map, MapRoot::new);
    Frames::new().run(&mut world);
    let (seven, seven_object) = object(&mut world, map, "seven");
    let (eight, _) = object(&mut world, map, "eight");
    assert_eq!(seven_object.reference("unset", &world), None);
    assert_eq!(seven_object.reference("empty", &world), None);
    assert_eq!(seven_object.reference("missing", &world), None);
    assert_eq!(seven_object.reference("me", &world), Some(seven));
    assert_eq!(seven_object.reference("other", &world), Some(eight));
    // An object despawned since is no longer there to name
    world.despawn(eight);
    assert_eq!(seven_object.reference("other", &world), None);
  }

  /// A collection tileset whose tiles 1, 2 and 3 have the classes "brick"
  /// (2 hits), "spike" and "door"
  fn classed_tileset() -> Tileset {
    let mut set = Tileset::new("things", UVec2::splat(16), TilesetImage::Collection);
    set.tile_count = 4;
    for (id, class) in [(1, "brick"), (2, "spike"), (3, "door")] {
      let mut properties = Properties::default();
      if class == "brick" {
        properties.insert("hits", PropertyValue::Int(2));
      }
      set.insert_tile(
        id,
        TileData {
          class: Some(class.to_string()),
          properties,
          ..TileData::default()
        },
      );
    }
    set
  }

  #[test]
  fn tiles_of_a_registered_class_spawn_as_entities_and_leave_the_grid() {
    let mut world = crate::core::testing::asset_world();
    let mut map = code_map();
    map.add_tileset(TilesetSource::Embedded(Arc::new(classed_tileset())));
    map.layers[0].info.offset = Vec2::new(2.0, 0.0);
    let flipped = Tile::new(0, 1).with_flip(TileFlip::HORIZONTAL);
    let cells = [
      (IVec2::new(0, 0), Tile::new(0, 0)),
      (IVec2::new(1, 0), Tile::new(0, 1)),
      (IVec2::new(2, 1), flipped),
      (IVec2::new(0, 1), Tile::new(0, 2)),
      (IVec2::new(3, 2), Tile::new(0, 3)),
    ];
    for (cell, tile) in cells {
      map.tiles_mut(0).expect("ground").set(cell, Some(tile));
    }
    let handle = world.resource_mut::<Assets<TileMap>>().add(map);
    let with = world
      .spawn((
        MapRoot::new(handle.clone()).with_entity_tiles(&["brick", "spike"]),
        Transform::from_xy(100.0, 0.0),
      ))
      .id();
    let without = world.spawn(MapRoot::new(handle.clone())).id();
    Frames::new().run(&mut world);

    let mut tiles: Vec<(Entity, MapTile)> = world
      .query::<(Entity, &MapTile)>()
      .iter(&world)
      .map(|(e, t)| (e, t.clone()))
      .collect();
    tiles.sort_by_key(|(_, t)| (t.cell.y, t.cell.x));
    let found: Vec<(IVec2, Tile, &str)> = tiles
      .iter()
      .map(|(_, t)| (t.cell, t.tile, t.class.as_str()))
      .collect();
    assert_eq!(
      found,
      [
        (IVec2::new(1, 0), Tile::new(0, 1), "brick"),
        (IVec2::new(0, 1), Tile::new(0, 2), "spike"),
        (IVec2::new(2, 1), flipped, "brick"),
      ]
    );
    let ground = layers_of(&mut world, with)[0];
    for (entity, tile) in &tiles {
      assert_eq!((tile.map, tile.layer), (with, 0));
      assert_eq!(parent(&world, *entity), Some(ground));
    }
    let (brick, brick_tile) = &tiles[0];
    assert_eq!(brick_tile.properties.int("hits"), Some(2));
    // The cell's centre, in the layer (offset 2, 0) of the map at (100, 0)
    assert_eq!(translation(&world, *brick), Vec2::new(24.0, 8.0));
    assert_eq!(global(&world, *brick), Vec2::new(126.0, 8.0));

    // They left the grid; the rest stayed
    let maps = MapsView::new(&mut world);
    for cell in [IVec2::new(1, 0), IVec2::new(2, 1), IVec2::new(0, 1)] {
      assert_eq!(maps.tile(with, "ground", cell), None, "{cell}");
    }
    assert_eq!(
      maps.tile(with, "ground", IVec2::new(0, 0)),
      Some(Tile::new(0, 0))
    );
    assert_eq!(
      maps.tile(with, "ground", IVec2::new(3, 2)),
      Some(Tile::new(0, 3))
    );
    // A map registering no class keeps them, and the asset is untouched
    assert_eq!(
      maps.tile(without, 0, IVec2::new(1, 0)),
      Some(Tile::new(0, 1))
    );
    assert_eq!(
      maps.tile_data(with, Tile::new(0, 1)).and_then(|d| d.class),
      Some("brick".to_string())
    );
    assert!(maps.tile_data(with, Tile::new(0, 0)).is_none());
    let asset = world
      .resource::<Assets<TileMap>>()
      .get(&handle)
      .expect("map");
    assert_eq!(
      asset.tiles(0).and_then(|g| g.get(IVec2::new(1, 0))),
      Some(Tile::new(0, 1))
    );
  }

  #[test]
  fn edits_outside_a_layer_or_on_a_missing_layer_change_nothing() {
    let mut world = crate::core::testing::asset_world();
    let map = spawn_code_map(&mut world, code_map(), MapRoot::new);
    let nowhere = world
      .resource::<AssetServer>()
      .load::<TileMap>("nowhere.tmj");
    let loading = world.spawn(MapRoot::new(nowhere)).id();
    let stranger = world.spawn(Transform::default()).id();
    let gone = world.spawn(Transform::default()).id();
    world.despawn(gone);
    Frames::new().run(&mut world);
    let ground = layers_of(&mut world, map)[0];
    let mut dirty = Vec::new();
    if let Some(mut data) = world.get_mut::<TileLayerData>(ground) {
      data.grid.drain_dirty(&mut dirty);
    }

    let tile = Some(Tile::new(0, 1));
    let mut maps = MapsView::new(&mut world);
    // Outside the layer, at negative cells, far out
    for cell in [
      IVec2::new(-1, 0),
      IVec2::new(0, -1),
      IVec2::new(4, 0),
      IVec2::new(0, 3),
      IVec2::new(i32::MIN, i32::MAX),
    ] {
      assert!(!maps.set_tile(map, "ground", cell, tile), "{cell}");
      assert_eq!(maps.tile(map, "ground", cell), None, "{cell}");
      assert_eq!(maps.intgrid(map, "ground", cell), None, "{cell}");
    }
    // A layer that does not exist, or holds no tiles
    for layer in [
      LayerRef::Name("nope"),
      LayerRef::Index(7),
      LayerRef::Name("things"),
      LayerRef::Index(1),
    ] {
      assert!(!maps.set_tile(map, layer, IVec2::ZERO, tile), "{layer:?}");
      assert_eq!(maps.tile(map, layer, IVec2::ZERO), None, "{layer:?}");
      assert_eq!(
        maps.fill(map, layer, IVec2::ZERO, IVec2::ONE, tile),
        0,
        "{layer:?}"
      );
      assert_eq!(
        maps.world_to_cell(map, layer, Vec2::ZERO),
        None,
        "{layer:?}"
      );
      assert_eq!(
        maps.cell_to_world(map, layer, IVec2::ZERO),
        None,
        "{layer:?}"
      );
    }
    assert_eq!(maps.layer_entity(map, "nope"), None);
    assert_eq!(maps.layer_entity(map, 7), None);
    // Fills wholly outside, or with min past max
    assert_eq!(
      maps.fill(map, "ground", IVec2::new(10, 10), IVec2::new(20, 20), tile),
      0
    );
    assert_eq!(
      maps.fill(map, "ground", IVec2::new(-9, -9), IVec2::new(-1, -1), tile),
      0
    );
    assert_eq!(
      maps.fill(map, "ground", IVec2::new(3, 2), IVec2::new(0, 0), tile),
      0
    );
    // A tileset the map does not have
    assert!(maps.tile_data(map, Tile::new(9, 0)).is_none());
    // Not a spawned map: loading, not a map at all, despawned
    for other in [loading, stranger, gone] {
      assert!(!maps.is_ready(other));
      assert!(!maps.set_tile(other, "ground", IVec2::ZERO, tile));
      assert_eq!(maps.fill(other, "ground", IVec2::ZERO, IVec2::ONE, tile), 0);
      assert_eq!(maps.tile(other, "ground", IVec2::ZERO), None);
      assert_eq!(maps.layer_entity(other, 0), None);
      assert_eq!(maps.cell_to_world(other, 0, IVec2::ZERO), None);
      assert!(maps.tile_data(other, Tile::new(0, 0)).is_none());
    }
    // Nothing changed
    if let Some(mut data) = world.get_mut::<TileLayerData>(ground) {
      data.grid.drain_dirty(&mut dirty);
    }
    assert!(dirty.is_empty(), "{dirty:?}");

    // Edits inside do, once
    let mut maps = MapsView::new(&mut world);
    assert_eq!(maps.layer_entity(map, "ground"), Some(ground));
    assert_eq!(maps.layer_entity(map, 0), Some(ground));
    assert!(maps.set_tile(map, "ground", IVec2::new(1, 1), tile));
    assert!(!maps.set_tile(map, "ground", IVec2::new(1, 1), tile));
    assert_eq!(maps.tile(map, 0, IVec2::new(1, 1)), tile);
    let other = Some(Tile::new(0, 2));
    assert_eq!(
      maps.fill(map, "ground", IVec2::new(-2, -2), IVec2::new(1, 0), other),
      2
    );
    assert_eq!(maps.tile(map, "ground", IVec2::new(1, 0)), other);
    assert!(maps.set_tile(map, "ground", IVec2::new(1, 1), None));
    assert_eq!(maps.tile(map, "ground", IVec2::new(1, 1)), None);
  }

  #[test]
  fn world_and_cell_conversions_follow_the_map_s_transform() {
    let mut world = crate::core::testing::asset_world();
    let mut map = code_map();
    map.layers[0].info.offset = Vec2::new(8.0, 0.0);
    // A layer of its own cell size (LDtk's)
    map.layers.push(MapLayer {
      info: LayerInfo::new(9, "fine"),
      kind: LayerKind::Tiles {
        grid: TileGrid::new(UVec2::new(8, 6)),
        intgrid: None,
        cell_size: UVec2::splat(8),
      },
    });
    let handle = world.resource_mut::<Assets<TileMap>>().add(map);
    let map = world
      .spawn((
        MapRoot::new(handle),
        Transform::from_xy(100.0, 50.0).with_uniform_scale(2.0),
      ))
      .id();
    Frames::new().run(&mut world);

    let maps = MapsView::new(&mut world);
    // Translated and scaled, through the layer's offset, to the cell's centre
    let c = |cell: IVec2| maps.cell_to_world(map, "ground", cell);
    assert_eq!(c(IVec2::new(0, 0)), Some(Vec2::new(132.0, 66.0)));
    assert_eq!(c(IVec2::new(1, 2)), Some(Vec2::new(164.0, 130.0)));
    let w = |point: Vec2| maps.world_to_cell(map, "ground", point);
    assert_eq!(w(Vec2::new(132.0, 66.0)), Some(IVec2::new(0, 0)));
    assert_eq!(w(Vec2::new(164.0, 130.0)), Some(IVec2::new(1, 2)));
    // A cell's edges: its top-left corner is in it, the next pixel row past
    // its bottom is not
    assert_eq!(w(Vec2::new(116.0, 50.0)), Some(IVec2::new(0, 0)));
    assert_eq!(w(Vec2::new(115.9, 50.0)), Some(IVec2::new(-1, 0)));
    assert_eq!(w(Vec2::new(116.0, 82.0)), Some(IVec2::new(0, 1)));
    // Past the layer, the cell is still given
    assert_eq!(w(Vec2::new(0.0, 0.0)), Some(IVec2::new(-4, -2)));
    // Each layer in its own cell size
    assert_eq!(
      maps.cell_to_world(map, "fine", IVec2::new(1, 1)),
      Some(Vec2::new(124.0, 74.0))
    );
    assert_eq!(
      maps.world_to_cell(map, 2, Vec2::new(124.0, 74.0)),
      Some(IVec2::new(1, 1))
    );
    for y in -2..5 {
      for x in -2..6 {
        let cell = IVec2::new(x, y);
        for layer in ["ground", "fine"] {
          let at = maps.cell_to_world(map, layer, cell).expect("a centre");
          assert_eq!(
            maps.world_to_cell(map, layer, at),
            Some(cell),
            "{layer} {cell}"
          );
        }
      }
    }

    // Moved and turned this frame, before any propagation: the conversions follow
    if let Some(mut t) = world.get_mut::<Transform>(map) {
      *t = Transform::default().with_rotation(FRAC_PI_2);
    }
    let maps = MapsView::new(&mut world);
    // (16, 8) in the map turns to (-8, 16)
    let at = maps
      .cell_to_world(map, "ground", IVec2::ZERO)
      .expect("a centre");
    assert!(close(at, Vec2::new(-8.0, 16.0)), "{at}");
    assert_eq!(
      maps.world_to_cell(map, "ground", Vec2::new(-8.0, 16.0)),
      Some(IVec2::ZERO)
    );
    // Under a parent
    let holder = world.spawn(Transform::from_xy(1000.0, 0.0)).id();
    world.entity_mut(map).insert(ChildOf(holder));
    let maps = MapsView::new(&mut world);
    let at = maps
      .cell_to_world(map, "ground", IVec2::ZERO)
      .expect("a centre");
    assert!(close(at, Vec2::new(992.0, 16.0)), "{at}");
    // A map scaled to nothing has no cells
    if let Some(mut t) = world.get_mut::<Transform>(map) {
      t.scale = Vec2::ZERO;
    }
    let maps = MapsView::new(&mut world);
    assert_eq!(maps.world_to_cell(map, "ground", Vec2::ZERO), None);
  }

  #[test]
  fn a_map_spawns_when_its_asset_lands_and_not_after_its_scene_left() {
    let mut world = world_over(files_with(&[
      ("a.tmj", THREE_LAYERS),
      ("b.tmj", THREE_LAYERS),
      ("c.tmj", THREE_LAYERS),
    ]));
    let server = world.resource::<AssetServer>().clone();
    server.limit_workers(1);
    let mut cursor = MessageCursor::<MapEvent>::default();
    let mut frames = Frames::new();

    // Spawned while its asset loads: it spawns in the frame the asset lands
    let a_handle = server.load::<TileMap>("a.tmj");
    let a = world.spawn(MapRoot::new(a_handle.clone())).id();
    assert!(!server.is_ready(&a_handle));
    let start = Instant::now();
    loop {
      frames.run(&mut world);
      let ready = server.is_ready(&a_handle);
      assert_eq!(!layers_of(&mut world, a).is_empty(), ready);
      if ready {
        break;
      }
      assert!(start.elapsed() < Duration::from_secs(5), "timed out");
      std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(spawned(&events(&world, &mut cursor)), [a]);

    // Its scene leaves (`SceneCleanup::WORLD`) while the asset loads; the
    // next scene holds the same asset, so it lands. Nothing spawns
    let b_handle = server.load::<TileMap>("b.tmj");
    let b = world.spawn(MapRoot::new(b_handle.clone())).id();
    despawn_game_entities(&mut world);
    // A map entity that stops being a map before its asset lands
    let c_handle = server.load::<TileMap>("c.tmj");
    let c = world.spawn(MapRoot::new(c_handle.clone())).id();
    world.entity_mut(c).remove::<MapRoot>();
    frames.until(&mut world, |_| {
      server.is_ready(&b_handle) && server.is_ready(&c_handle)
    });
    frames.run(&mut world);
    assert!(world.get_entity(a).is_err() && world.get_entity(b).is_err());
    assert_eq!(count::<MapLayerRef>(&mut world), 0);
    assert_eq!(count::<MapObject>(&mut world), 0);
    assert!(world.get::<crate::core::ecs::Children>(c).is_none());
    assert!(events(&world, &mut cursor).is_empty());
  }

  #[test]
  fn a_map_whose_asset_fails_reports_failed_once() {
    let files = files_with(&[("bad.tmj", "{ not a map")]);
    let mut world = world_over(files.clone());
    let server = world.resource::<AssetServer>().clone();
    server.limit_workers(1);
    let bad_handle = server.load::<TileMap>("bad.tmj");
    let gone_handle = server.load::<TileMap>("gone.tmj");
    let bad = world.spawn(MapRoot::new(bad_handle.clone())).id();
    let gone = world.spawn(MapRoot::new(gone_handle.clone())).id();
    let mut cursor = MessageCursor::<MapEvent>::default();
    let mut frames = Frames::new();
    let failing = |handle: &Handle<TileMap>| {
      matches!(
        server.state(handle),
        crate::core::asset::LoadState::Failed(_)
      )
    };
    frames.until(&mut world, |_| {
      failing(&bad_handle) && failing(&gone_handle)
    });
    for _ in 0..5 {
      frames.run(&mut world);
    }
    let sent = events(&world, &mut cursor);
    let mut reports = failed(&sent);
    reports.sort();
    assert_eq!(reports.len(), 2, "{reports:?}");
    assert!(spawned(&sent).is_empty());
    for (map, path) in [(bad, "bad.tmj"), (gone, "gone.tmj")] {
      let Some((_, report)) = reports.iter().find(|(m, _)| *m == map) else {
        panic!("no failure for {path}")
      };
      assert!(report.contains(path), "{report}");
      assert!(layers_of(&mut world, map).is_empty());
      assert!(!MapsView::new(&mut world).is_ready(map));
    }

    // Saved again, fixed: it spawns
    files.insert("bad.tmj", THREE_LAYERS);
    server.reload("bad.tmj");
    frames.until(&mut world, |_| server.is_ready(&bad_handle));
    let sent = events(&world, &mut cursor);
    assert_eq!(spawned(&sent), [bad]);
    assert!(failed(&sent).is_empty());
    assert_eq!(layer_names(&mut world, bad), ["ground", "things", "sky"]);
  }

  #[test]
  fn an_ldtk_level_spawns_by_identifier_and_by_iid() {
    let mut world = world_over(files_with(&[]));
    let project = AssetsView::new(&mut world)
      .load_now::<LdtkProject>("ldtk/internal.ldtk")
      .expect("internal.ldtk loads");
    let by_name = world.spawn(MapRoot::ldtk(project.clone(), "Level_0")).id();
    let by_iid = world
      .spawn(MapRoot::ldtk(
        project.clone(),
        "11111111-0000-4000-8000-000000000002",
      ))
      .id();
    let missing = world.spawn(MapRoot::ldtk(project.clone(), "Level_9")).id();
    let mut cursor = MessageCursor::<MapEvent>::default();
    let mut frames = Frames::new();
    for _ in 0..3 {
      frames.run(&mut world);
    }
    let sent = events(&world, &mut cursor);
    let mut spawned_maps = spawned(&sent);
    spawned_maps.sort();
    let mut expected = vec![by_name, by_iid];
    expected.sort();
    assert_eq!(spawned_maps, expected);

    // By identifier: its layers, IntGrid, tiles and entities
    assert_eq!(
      layer_names(&mut world, by_name),
      ["Decor", "Collision", "Props", "Entities"]
    );
    let maps = MapsView::new(&mut world);
    assert_eq!(
      maps.intgrid(by_name, "Collision", IVec2::new(2, 1)),
      Some(2)
    );
    assert_eq!(maps.intgrid(by_name, "Collision", IVec2::new(1, 1)), None);
    assert_eq!(
      maps.tile(by_name, "Collision", IVec2::new(1, 0)),
      Some(Tile::new(0, 1).with_flip(TileFlip::HORIZONTAL))
    );
    let level_objects = world
      .query::<&MapObject>()
      .iter(&world)
      .filter(|o| o.map == by_name)
      .count();
    assert_eq!(level_objects, 3);
    let chest = world
      .query::<&MapObject>()
      .iter(&world)
      .find(|o| o.map == by_name && o.id.ends_with("c0de"))
      .cloned()
      .expect("chest");
    let door = world
      .get::<MapIds>(by_name)
      .and_then(|ids| ids.get("11111111-0000-4000-8000-00000000d00a"));
    assert!(door.is_some());
    assert_eq!(chest.reference("target", &world), door);
    let lone = world
      .query::<&MapObject>()
      .iter(&world)
      .find(|o| o.map == by_name && o.id.ends_with("01ee"))
      .cloned()
      .expect("lone");
    assert_eq!(lone.reference("target", &world), None);

    // By iid: the second level
    let second: Vec<String> = world
      .resource::<Assets<LdtkProject>>()
      .get(&project)
      .and_then(|p| p.level("Level_1"))
      .map(|l| l.map.layers.iter().map(|l| l.info.name.clone()).collect())
      .expect("Level_1");
    assert_eq!(layer_names(&mut world, by_iid), second);

    // No such level: reported once, naming it and the project
    let reports = failed(&sent);
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].0, missing);
    assert!(
      reports[0].1.contains("Level_9") && reports[0].1.contains("internal.ldtk"),
      "{}",
      reports[0].1
    );
    assert!(layers_of(&mut world, missing).is_empty());
  }

  #[test]
  fn a_new_root_over_a_spawned_map_replaces_its_layers() {
    let mut world = crate::core::testing::asset_world();
    let map = spawn_code_map(&mut world, code_map(), MapRoot::new);
    let mut frames = Frames::new();
    frames.run(&mut world);
    let old = layers_of(&mut world, map);
    assert_eq!(old.len(), 2);

    let mut other = TileMap::new(UVec2::new(2, 2), UVec2::splat(8));
    other.add_tile_layer("other");
    let other = world.resource_mut::<Assets<TileMap>>().add(other);
    world.entity_mut(map).insert(MapRoot::new(other));
    frames.run(&mut world);
    assert_eq!(layer_names(&mut world, map), ["other"]);
    assert!(old.iter().all(|&layer| world.get_entity(layer).is_err()));
    assert!(MapsView::new(&mut world).is_ready(map));

    // Taking the root away takes the layers with it
    world.entity_mut(map).remove::<MapRoot>();
    frames.run(&mut world);
    assert!(layers_of(&mut world, map).is_empty());
    assert!(!MapsView::new(&mut world).is_ready(map));
  }

  #[test]
  fn a_root_keeps_its_settings() {
    let mut world = crate::core::testing::asset_world();
    let handle = world
      .resource_mut::<Assets<TileMap>>()
      .add(TileMap::new(UVec2::ONE, UVec2::ONE));
    let root = MapRoot::new(handle.clone())
      .with_entity_tiles(&["coin", "brick", "coin"])
      .with_intgrid_collision(3, TileCell::OneWay)
      .with_intgrid_collision("lava", TileCell::Solid)
      .with_intgrid_collision(3, TileCell::Solid)
      .without_collision()
      .with_z(-4);
    assert_eq!(root.entity_tiles, ["coin", "brick"]);
    assert_eq!(
      root.intgrid_collision,
      [
        (IntGridKey::Value(3), TileCell::Solid),
        (IntGridKey::Name("lava".into()), TileCell::Solid)
      ]
    );
    assert!(!root.collision);
    assert_eq!(root.z, -4);
    let fresh = MapRoot::new(handle);
    assert!(fresh.collision && fresh.z == 0 && fresh.entity_tiles.is_empty());
  }

  #[test]
  fn hidden_objects_spawn_marked_hidden() {
    let mut world = world_over(files_with(&[]));
    let handle = AssetsView::new(&mut world)
      .load_now::<TileMap>("tiled/maps/objects.tmj")
      .expect("objects.tmj loads");
    let map = world.spawn(MapRoot::new(handle)).id();
    Frames::new().run(&mut world);
    let (_, hidden) = object(&mut world, map, "hidden");
    assert!(!hidden.visible);
    let (_, shown) = object(&mut world, map, "crate");
    assert!(shown.visible);
  }

  /// A level's maps for the observer tests: which to cancel as its first
  /// layer arrives (with another map), and which layer to drop as it arrives
  #[derive(Resource)]
  struct Cancel {
    map: Entity,
    victim: Entity,
    layer: &'static str,
  }

  fn despawn_later(commands: &mut Commands, entity: Entity) {
    commands.queue(move |world: &mut World| {
      let _ = world.try_despawn(entity);
    });
  }

  /// A turret takes its waypoint marker away as it arrives
  fn take_waypoint(
    add: On<Add<MapObject>>,
    objects: Query<&MapObject>,
    ids: Query<&MapIds>,
    mut commands: Commands,
  ) {
    let Ok(object) = objects.get(add.entity) else {
      return;
    };
    let Some(PropertyValue::Object(id)) = object.properties.get("waypoint") else {
      return;
    };
    if let Some(marker) = ids.get(object.map).ok().and_then(|ids| ids.get(id)) {
      despawn_later(&mut commands, marker);
    }
  }

  /// A level cancelled as its first layer arrives, taking another map with
  /// it; and a layer dropped as it arrives
  fn cancel_level(
    add: On<Add<MapLayerRef>>,
    layers: Query<&MapLayerRef>,
    cancel: Option<Res<Cancel>>,
    mut commands: Commands,
  ) {
    let (Ok(layer), Some(cancel)) = (layers.get(add.entity), cancel) else {
      return;
    };
    if layer.map == cancel.map {
      despawn_later(&mut commands, cancel.map);
      despawn_later(&mut commands, cancel.victim);
    }
    if layer.name == cancel.layer {
      despawn_later(&mut commands, add.entity);
    }
  }

  #[test]
  fn an_observer_despawning_a_sibling_mid_spawn_does_not_panic() {
    let mut world = crate::core::testing::asset_world();
    world.add_observer(take_waypoint);
    world.add_observer(cancel_level);
    let mut frames = Frames::new();
    // A map that will have failed by the time the others spawn
    let missing = world
      .resource::<AssetServer>()
      .load::<TileMap>("missing.tmj");
    frames.until(&mut world, |world| {
      matches!(
        world.resource::<AssetServer>().state(&missing),
        LoadState::Failed(_)
      )
    });

    // A turret whose waypoint (the next object) is taken away as it arrives
    let mut turrets = code_map();
    if let LayerKind::Objects(objects) = &mut turrets.layers[1].kind {
      let waypoint = [("waypoint", PropertyValue::Object("2".into()))];
      objects.push(object_data("1", "turret", &waypoint));
      objects.push(object_data("2", "marker", &[]));
      objects.push(object_data("3", "crate", &[]));
    }
    let turrets = spawn_code_map(&mut world, turrets, MapRoot::new);
    // An object layer dropped as it arrives, under a class tile layer
    let mut dropping = code_map();
    dropping.add_tileset(TilesetSource::Embedded(Arc::new(classed_tileset())));
    dropping.layers[1].info.name = "dropped".into();
    if let LayerKind::Objects(objects) = &mut dropping.layers[1].kind {
      objects.push(object_data("1", "lost", &[]));
    }
    if let Some(grid) = dropping.tiles_mut(0) {
      grid.set(IVec2::new(1, 1), Some(Tile::new(0, 1)));
    }
    let dropping = spawn_code_map(&mut world, dropping, |h| {
      MapRoot::new(h).with_entity_tiles(&["brick"])
    });
    // A level cancelled as its first layer arrives, taking the failed map with it
    let cancelled = spawn_code_map(&mut world, code_map(), MapRoot::new);
    let victim = world.spawn(MapRoot::new(missing.clone())).id();
    world.insert_resource(Cancel {
      map: cancelled,
      victim,
      layer: "dropped",
    });
    let mut cursor = MessageCursor::<MapEvent>::default();
    frames.run(&mut world);

    // The turret's map spawned, the marker went, the objects after it spawned
    let (_, turret) = object(&mut world, turrets, "turret");
    assert_eq!(turret.reference("waypoint", &world), None);
    let markers = world
      .query::<&MapObject>()
      .iter(&world)
      .filter(|o| o.name == "marker")
      .count();
    assert_eq!(markers, 0);
    object(&mut world, turrets, "crate");
    // The dropped layer took its objects with it; the class tile spawned
    assert!(MapsView::new(&mut world).is_ready(dropping));
    assert_eq!(layer_names(&mut world, dropping), ["ground"]);
    let lost = world
      .query::<&MapObject>()
      .iter(&world)
      .filter(|o| o.map == dropping)
      .count();
    assert_eq!(lost, 0);
    assert_eq!(count::<MapTile>(&mut world), 1);
    // The cancelled level and the map it took: gone, nothing left behind
    assert!(world.get_entity(cancelled).is_err() && world.get_entity(victim).is_err());
    let orphans = world
      .query::<&MapLayerRef>()
      .iter(&world)
      .filter(|l| world.get_entity(l.map).is_err())
      .count();
    assert_eq!(orphans, 0);
    let sent = events(&world, &mut cursor);
    let mut maps = spawned(&sent);
    maps.sort();
    let mut expected = vec![turrets, dropping];
    expected.sort();
    assert_eq!(maps, expected);
    assert!(failed(&sent).is_empty(), "{sent:?}");
  }

  /// A map built in code with the classed tileset and a brick at (1, 0), whose
  /// root registers bricks, at (100, 0) with its ground offset by (2, 0)
  fn brick_map(world: &mut World) -> Entity {
    let mut map = code_map();
    map.add_tileset(TilesetSource::Embedded(Arc::new(classed_tileset())));
    map.layers[0].info.offset = Vec2::new(2.0, 0.0);
    if let Some(grid) = map.tiles_mut(0) {
      grid.set(IVec2::new(1, 0), Some(Tile::new(0, 1)));
    }
    let handle = world.resource_mut::<Assets<TileMap>>().add(map);
    world
      .spawn((
        MapRoot::new(handle).with_entity_tiles(&["brick"]),
        Transform::from_xy(100.0, 0.0),
      ))
      .id()
  }

  /// The map's class tiles by cell
  fn class_tiles_of(world: &mut World, map: Entity) -> Vec<(IVec2, Entity, MapTile)> {
    let mut found: Vec<(IVec2, Entity, MapTile)> = world
      .query::<(Entity, &MapTile)>()
      .iter(world)
      .filter(|(_, t)| t.map == map)
      .map(|(e, t)| (t.cell, e, t.clone()))
      .collect();
    found.sort_by_key(|(cell, _, _)| (cell.y, cell.x));
    found
  }

  #[test]
  fn setting_a_class_tile_at_runtime_spawns_its_entity() {
    let mut world = crate::core::testing::asset_world();
    let map = brick_map(&mut world);
    Frames::new().run(&mut world);
    let ground = layers_of(&mut world, map)[0];
    let brick = Tile::new(0, 1);
    let flipped = brick.with_flip(TileFlip::VERTICAL);
    let plain = Tile::new(0, 0);

    let mut maps = MapsView::new(&mut world);
    assert!(maps.set_tile(map, "ground", IVec2::new(2, 1), Some(plain)));
    // Over a plain tile, and on an empty cell
    assert!(maps.set_tile(map, "ground", IVec2::new(2, 1), Some(brick)));
    assert!(maps.set_tile(map, 0, IVec2::new(3, 2), Some(flipped)));
    // The same again changes nothing
    assert!(!maps.set_tile(map, "ground", IVec2::new(2, 1), Some(brick)));
    // Outside the layer, nothing
    assert!(!maps.set_tile(map, "ground", IVec2::new(4, 0), Some(brick)));
    assert!(!maps.set_tile(map, "ground", IVec2::new(-1, 0), Some(brick)));
    // A class the root does not register is a plain tile
    let spike = Some(Tile::new(0, 2));
    assert!(maps.set_tile(map, "ground", IVec2::new(0, 2), spike));
    assert_eq!(maps.tile(map, "ground", IVec2::new(0, 2)), spike);
    // They leave the grid
    assert_eq!(maps.tile(map, "ground", IVec2::new(2, 1)), None);
    assert_eq!(maps.tile(map, "ground", IVec2::new(3, 2)), None);

    let tiles = class_tiles_of(&mut world, map);
    let cells: Vec<(IVec2, Tile)> = tiles.iter().map(|(c, _, t)| (*c, t.tile)).collect();
    assert_eq!(
      cells,
      [
        (IVec2::new(1, 0), brick),
        (IVec2::new(2, 1), brick),
        (IVec2::new(3, 2), flipped),
      ]
    );
    let (_, entity, tile) = &tiles[1];
    assert_eq!((tile.map, tile.layer), (map, 0));
    assert_eq!(tile.class, "brick");
    assert_eq!(tile.properties.int("hits"), Some(2));
    assert_eq!(parent(&world, *entity), Some(ground));
    // The cell's centre in the layer (offset 2, 0) of the map at (100, 0)
    assert_eq!(translation(&world, *entity), Vec2::new(40.0, 24.0));
    assert_eq!(global(&world, *entity), Vec2::new(142.0, 24.0));

    // A fill of bricks spawns one in each cell that had none
    let mut maps = MapsView::new(&mut world);
    assert_eq!(
      maps.fill(
        map,
        "ground",
        IVec2::new(-3, 0),
        IVec2::new(2, 0),
        Some(brick)
      ),
      2
    );
    assert_eq!(
      maps.fill(
        map,
        "ground",
        IVec2::new(0, 0),
        IVec2::new(2, 0),
        Some(brick)
      ),
      0
    );
    let cells: Vec<IVec2> = class_tiles_of(&mut world, map)
      .iter()
      .map(|(c, _, _)| *c)
      .collect();
    assert_eq!(
      cells,
      [
        IVec2::new(0, 0),
        IVec2::new(1, 0),
        IVec2::new(2, 0),
        IVec2::new(2, 1),
        IVec2::new(3, 2),
      ]
    );
  }

  #[test]
  fn replacing_a_class_tile_despawns_its_entity() {
    let mut world = crate::core::testing::asset_world();
    let map = brick_map(&mut world);
    Frames::new().run(&mut world);
    let brick = Tile::new(0, 1);
    let plain = Some(Tile::new(0, 0));
    let spawned_brick = class_tiles_of(&mut world, map)[0].1;

    // A plain tile over the brick from the file
    let mut maps = MapsView::new(&mut world);
    assert!(maps.set_tile(map, "ground", IVec2::new(1, 0), plain));
    assert_eq!(maps.tile(map, "ground", IVec2::new(1, 0)), plain);
    assert!(world.get_entity(spawned_brick).is_err());

    // Cleared: the entity goes, the cell stays empty; again, nothing
    let mut maps = MapsView::new(&mut world);
    assert!(maps.set_tile(map, "ground", IVec2::new(2, 0), Some(brick)));
    assert!(maps.set_tile(map, "ground", IVec2::new(2, 0), None));
    assert!(!maps.set_tile(map, "ground", IVec2::new(2, 0), None));
    assert_eq!(maps.tile(map, "ground", IVec2::new(2, 0)), None);
    assert!(class_tiles_of(&mut world, map).is_empty());

    // A brick over a brick, flipped: the old entity goes, a new one comes
    let mut maps = MapsView::new(&mut world);
    assert!(maps.set_tile(map, "ground", IVec2::new(3, 0), Some(brick)));
    let first = class_tiles_of(&mut world, map)[0].1;
    let flipped = brick.with_flip(TileFlip::HORIZONTAL);
    let mut maps = MapsView::new(&mut world);
    assert!(maps.set_tile(map, "ground", IVec2::new(3, 0), Some(flipped)));
    let tiles = class_tiles_of(&mut world, map);
    assert_eq!(tiles.len(), 1);
    assert_eq!(tiles[0].2.tile, flipped);
    assert!(world.get_entity(first).is_err());

    // A fill clearing a region takes the bricks in it, counting each
    let mut maps = MapsView::new(&mut world);
    assert_eq!(
      maps.fill(
        map,
        "ground",
        IVec2::new(0, 1),
        IVec2::new(1, 1),
        Some(brick)
      ),
      2
    );
    // (1, 0)'s plain tile, (3, 0)'s brick and the two just filled
    assert_eq!(
      maps.fill(map, "ground", IVec2::new(0, 0), IVec2::new(3, 1), None),
      4
    );
    assert!(class_tiles_of(&mut world, map).is_empty());

    // A brick the game despawned itself leaves nothing to clear
    let mut maps = MapsView::new(&mut world);
    assert!(maps.set_tile(map, "ground", IVec2::new(0, 2), Some(brick)));
    let gone = class_tiles_of(&mut world, map)[0].1;
    world.despawn(gone);
    let mut maps = MapsView::new(&mut world);
    assert!(!maps.set_tile(map, "ground", IVec2::new(0, 2), None));
    assert!(maps.set_tile(map, "ground", IVec2::new(0, 2), Some(brick)));
    assert_eq!(class_tiles_of(&mut world, map).len(), 1);
  }

  /// Loads `m.tmj` at once and spawns it as it enters; records the map
  /// events it reads and whether `ctx.maps()` finds the map ready
  struct Level {
    map: Arc<Mutex<Option<Entity>>>,
    seen: Arc<Mutex<Vec<MapEvent>>>,
    ready: Arc<AtomicBool>,
  }

  impl Scene for Level {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let handle = ctx
        .assets()
        .load_now::<TileMap>("m.tmj")
        .expect("m.tmj loads");
      let map = ctx.world.spawn(MapRoot::new(handle)).id();
      if let Ok(mut held) = self.map.lock() {
        *held = Some(map);
      }
    }

    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      let events = ctx.messages::<MapEvent>();
      if let Ok(mut seen) = self.seen.lock() {
        seen.extend(events);
      }
      let map = self.map.lock().ok().and_then(|m| *m);
      if let Some(map) = map {
        self.ready.store(ctx.maps().is_ready(map), Ordering::SeqCst);
      }
      SceneAction::Continue
    }
  }

  #[test]
  fn a_map_spawns_through_the_engine_s_frame() {
    let map = Arc::new(Mutex::new(None));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let ready = Arc::new(AtomicBool::new(false));
    let scene = Level {
      map: map.clone(),
      seen: seen.clone(),
      ready: ready.clone(),
    };
    let Some(mut engine) = HeadlessEngine::new_or_skip(
      EngineBuilder::new()
        .resolution(160, 120)
        .asset_source(files_with(&[("m.tmj", THREE_LAYERS)]))
        .start_scene(scene),
    ) else {
      return;
    };
    engine.step().expect("a frame");
    let map = map
      .lock()
      .ok()
      .and_then(|m| *m)
      .expect("the scene spawned a map");
    assert_eq!(
      layer_names(engine.world_mut(), map),
      ["ground", "things", "sky"]
    );
    engine.step().expect("a frame");
    assert!(ready.load(Ordering::SeqCst));
    let seen = seen.lock().map(|s| s.clone()).unwrap_or_default();
    assert_eq!(spawned(&seen), [map]);
  }
}
