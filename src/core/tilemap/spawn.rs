//! Maps as entities. A [`MapRoot`] names a map asset (a [`TileMap`], or a
//! level of an [`LdtkProject`]); once the asset and its tilesets are loaded,
//! `spawn_maps` gives the map entity a child per layer, and each object and
//! class tile a child of its layer.

use std::borrow::Cow;
use std::sync::Arc;

use glam::{IVec2, UVec2, Vec2};

use super::collision::{self, LayerView};
use super::jobs::Worker;
use super::kinds::{sources, stamps};
use super::occluders;
use super::padded::{self, PaddedCache, ToPad};
use super::reload::{MapApplied, content_stamp};
use super::world::{WorldRef, resolve_world_levels};
use super::{
  ImageLayer, IntGrid, LayerInfo, LayerKind, LdtkProject, MapLayer, MapObjectData, ObjectShape,
  Properties, PropertyValue, Tile, TileData, TileGrid, TileMap, Tileset, TilesetSource,
};
use crate::core::asset::{AssetError, AssetPath, AssetServer, Assets, Handle, LoadState};
use crate::core::ecs::kernel::{DeferredWorld, HookContext, QueryState};
use crate::core::ecs::{
  Allow, ChildOf, Component, Disabled, Entity, Local, Message, Messages, Transform, Without, World,
};
use crate::core::graphics::Image;
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
  /// A level of a world, standing for its map once the world has loaded
  WorldLevel {
    world: WorldRef,
    level: usize,
  },
}

/// A map in the world. The entity's `Transform` places the map's top-left
/// corner; once its asset (and the tilesets it names) is loaded and its
/// colliders are worked out (off the main thread, over a frame or so), its
/// layers spawn as its children, each object and class tile a child of its
/// layer, and a [`MapEvent::Spawned`] is sent ([`MapEvent::Failed`] if the
/// asset cannot load or its landing fails). Read and edit it through
/// `ctx.maps()`. Despawning the
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
/// re-shadow only the chunks they touch; a disabled map has neither. A
/// map lands whole: its colliders and shadows are worked out off the main
/// thread once its asset is ready, and its layers, objects, colliders,
/// shadows and [`MapEvent::Spawned`] arrive together in one frame, so a
/// body put down by an observer of its objects (`On<Add<MapObject>>`, see
/// [`MapObject`]) or on `Spawned` stands on its ground.
///
/// With hot reload on, saving one of the map's files updates it in place
/// in the frame the save lands ([`MapEvent::Reloaded`]): its layers keep
/// their entities and only the chunks the save changed redraw, collide and
/// occlude again; objects and class tiles the save left alone keep their
/// entities and everything the game put on them, and those the game
/// despawned stay despawned unless the save changed them. The file wins
/// over the game's tile edits (dug tiles come back).
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

  pub(crate) fn from_source(source: MapSource) -> MapRoot {
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
  /// over its name's. A `TileCell::Shape` here names no shape: its cells
  /// collide as nothing, and the map warns once in the dev log
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
/// `Transform`.
///
/// An observer of `Add<MapObject>` sees each object as it arrives, its
/// fields filled. By then the map's layers, its colliders and shadows and
/// every object's entity are in, so [`reference`](Self::reference)
/// resolves, and a body put down there stands on the map's ground once
/// physics steps; objects later in the file have their entities but not
/// yet their `MapObject`, the class tiles come after the objects, and
/// [`MapEvent::Spawned`] follows in the same frame. An object a reload
/// spawns is seen too; one it updates in place fires `Insert<MapObject>`
/// only.
///
/// ```
/// use ryo2d::prelude::*;
///
/// /// Coins become pickups as they arrive, from any map
/// fn coins_arrive(add: On<Add<MapObject>>, objects: Query<&MapObject>, mut commands: Commands) {
///   if objects.get(add.entity).is_ok_and(|o| o.class.as_deref() == Some("coin")) {
///     commands
///       .entity(add.entity)
///       .insert((Collider::circle(6.0).as_trigger(), Pickup::new()));
///   }
/// }
///
/// fn on_enter(ctx: &mut SceneContext<'_>) -> Entity {
///   // Despawn the observer's entity as the scene exits: observers outlive scenes
///   ctx.world.add_observer(coins_arrive).id()
/// }
/// ```
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
  pub(super) objects: FastMap<String, Entity>,
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
  /// It landed whole: its layers, objects and class tiles, its colliders
  /// and its shadows all arrived this frame, so a body put on it now (or by
  /// an observer of its objects as they arrived) stands on its ground
  Spawned { map: Entity },
  /// A file it was made from was saved (its map or LDtk project, a
  /// template, the Tiled project, an LDtk level file or a tileset; not a
  /// tileset's image, which only redraws it) and it follows the file: its
  /// tiles are the file's; objects and class tiles the save left alone keep
  /// their entities untouched, changed ones are updated in place and new
  /// ones spawn. Sent in the frame the save lands, before the game's
  /// update; a save that fails to load sends nothing. A file that several
  /// of the map's files read (the Tiled project, which its map and its
  /// tilesets read) loads each of them again, and those loads can land in
  /// different frames: each is followed by its own `Reloaded`, the last
  /// with the save in full
  Reloaded { map: Entity },
  /// Its asset did not load (or has no such level), or its landing
  /// panicked (tried again once the map or a tileset it uses changes):
  /// nothing spawned
  Failed { map: Entity, error: Arc<AssetError> },
}

/// The failure a waiting map reported, so it reports each failure once
#[derive(Component, Clone, Debug)]
pub(crate) struct MapFailed {
  error: Arc<AssetError>,
  /// The project's version, for a level the project lacks
  version: Option<u64>,
  /// For a landing that panicked: what it was worked out from
  /// (`landing_stamp`); it is not tried again until that changes
  landing: Option<Vec<Option<u64>>>,
}

/// A map whose landing is being worked out off the main thread, by its
/// job's id
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct MapLanding {
  job: u64,
}

/// The maps not spawned yet, with the failure each reported and the
/// landing out for it
pub(crate) type WaitingMaps = QueryState<
  (
    Entity,
    &'static MapRoot,
    Option<&'static MapFailed>,
    Option<&'static MapLanding>,
  ),
  Without<MapIds>,
>;

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
    // Not yet standing for its map: `spawn_maps` resolves it
    MapSource::WorldLevel { .. } => None,
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

/// `layers` (the root's map's, copied) with each Tiled tile object given
/// what its tile carries, as Tiled shows it: an object with no class of its
/// own takes its tile's, and its properties lie over the tile's (which hold
/// the tile class's defaults). A map's external tilesets are not loaded
/// while it loads, so this is done as it lands, and again on a reload, whose
/// objects are compared as given. An LDtk entity's tile is its picture in
/// the editor, and gives it nothing
pub(crate) fn inherit_tile_data(world: &World, root: &MapRoot, layers: &mut [MapLayer]) {
  if matches!(root.source, MapSource::Ldtk { .. }) {
    return;
  }
  for layer in layers {
    let LayerKind::Objects(objects) = &mut layer.kind else {
      continue;
    };
    for object in objects {
      let Some(data) = object.tile.and_then(|tile| tile_data(world, root, tile)) else {
        continue;
      };
      if object.class.is_none() {
        object.class = data.class.clone();
      }
      if !data.properties.is_empty() {
        let own = std::mem::replace(&mut object.properties, data.properties.clone());
        for (name, value) in own.iter() {
          object.properties.insert(name, value.clone());
        }
      }
    }
  }
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

/// `PreUpdate`, after the asset pump: each map whose asset is ready has
/// its landing worked out on the worker (its layers' copies with class
/// tiles taken out, its colliders and its shadows), and each whose asset
/// failed reports it once; each landing that came back lands whole (its
/// layers, objects, class tiles, colliders and shadows, and
/// `MapEvent::Spawned`), or is worked out again here if it panicked, and
/// reported once as failed if that panics too. Where there is no thread (the web) a landing is
/// worked out here and lands in the same frame. With no map waiting it is
/// one query over no entities
pub(crate) fn spawn_maps(
  world: &mut World,
  waiting: &mut WaitingMaps,
  mut worker: Local<Worker<Landed>>,
) {
  resolve_world_levels(world, waiting);
  let decided: Vec<(Entity, Ready)> = waiting
    .iter(world)
    .filter(|(.., landing)| landing.is_none())
    .filter_map(|(map, root, failed, _)| {
      let ready = readiness(world, root, failed)?;
      // A landing that panicked waits for its map or a tileset to change
      let given_up = matches!(ready, Ready::Spawn)
        && failed
          .and_then(|f| f.landing.as_ref())
          .is_some_and(|stamp| *stamp == landing_stamp(world, root));
      (!given_up).then_some((map, ready))
    })
    .collect();
  for (map, ready) in decided {
    match ready {
      Ready::Spawn => {
        if let Some(job) = landing_job(world, map) {
          let job = worker.send(move || job.run());
          world.entity_mut(map).insert(MapLanding { job });
        }
      }
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
  let arrived: Vec<(u64, Option<Landed>)> = worker.finished().collect();
  for (job, landed) in arrived {
    // Disabled roots too: one disabled while its landing was out drops it
    // here, and is worked out again once enabled
    let mut landing = world.query_filtered::<(Entity, &MapLanding), Allow<Disabled>>();
    let Some(map) = landing
      .iter(world)
      .find(|(_, l)| l.job == job)
      .map(|(map, _)| map)
    else {
      // Its map went (or was given another root) while it was out
      continue;
    };
    world.entity_mut(map).remove::<MapLanding>();
    if !waits_to_land(world, map) {
      continue;
    }
    let landed = match landed {
      // A save landed while it was out: worked out again from the files as
      // they are now, so the map lands as saved, once
      Some(landed) if is_stale(world, map, &landed) => {
        if let Some(job) = landing_job(world, map) {
          let job = worker.send(move || job.run());
          world.entity_mut(map).insert(MapLanding { job });
        }
        continue;
      }
      Some(landed) => Some(landed),
      None => {
        crate::engine_warn!(
          module: "Tilemap",
          "map {map}: its landing could not be worked out off the main thread; worked out here"
        );
        landing_job(world, map).and_then(|job| {
          std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || job.run())).ok()
        })
      }
    };
    match landed {
      Some(landed) => land(world, map, landed),
      None => landing_failed(world, map),
    }
  }
}

/// Whether a load of one of `map`'s files (its map or LDtk project, or a
/// tileset asset it uses) landed since its landing was worked out
fn is_stale(world: &World, map: Entity, landed: &Landed) -> bool {
  world
    .get::<MapRoot>(map)
    .is_some_and(|root| content_stamp(world, root) != landed.applied.stamp())
}

/// Whether `map` is a map waiting to land: an enabled root not spawned yet
fn waits_to_land(world: &World, map: Entity) -> bool {
  world.get_entity(map).is_ok_and(|entity| {
    entity.contains::<MapRoot>() && !entity.contains::<MapIds>() && !entity.contains::<Disabled>()
  })
}

/// Both tries at `map`'s landing panicked (off the main thread and on it):
/// the failure is reported once, and the landing is not tried again until
/// the root's map (or LDtk project) or a tileset it uses changes
fn landing_failed(world: &mut World, map: Entity) {
  let Some(root) = world.get::<MapRoot>(map) else {
    return;
  };
  let landing = landing_stamp(world, root);
  let server = world.get_resource::<AssetServer>();
  let path = match &root.source {
    MapSource::Map(handle) => server.and_then(|s| s.path(handle)),
    MapSource::Ldtk { project, .. } => server.and_then(|s| s.path(project)),
    MapSource::WorldLevel { world, .. } => server.and_then(|s| world.path(s)),
  }
  .unwrap_or_else(|| AssetPath::from_normal(Cow::Borrowed("map")));
  let error = Arc::new(AssetError::decode(
    &path,
    "its landing panicked; it is tried again once the map or a tileset it uses changes",
  ));
  crate::engine_warn!(module: "Tilemap", "map {map}: {}", error.report());
  world.entity_mut(map).insert(MapFailed {
    error: Arc::clone(&error),
    version: None,
    landing: Some(landing),
  });
  send(world, MapEvent::Failed { map, error });
}

/// What a landing is worked out from, as versions: the root's map (or LDtk
/// project), then each tileset asset the map uses
fn landing_stamp(world: &World, root: &MapRoot) -> Vec<Option<u64>> {
  let mut stamp = vec![match &root.source {
    MapSource::Map(handle) => world
      .get_resource::<Assets<TileMap>>()
      .and_then(|maps| maps.version(handle)),
    MapSource::Ldtk { project, .. } => world
      .get_resource::<Assets<LdtkProject>>()
      .and_then(|projects| projects.version(project)),
    MapSource::WorldLevel { world: source, .. } => source.version(world),
  }];
  let tilesets = world.get_resource::<Assets<Tileset>>();
  if let Some(map) = map_asset(world, root) {
    stamp.extend(map.tilesets.iter().filter_map(|source| match source {
      TilesetSource::Asset(handle) => Some(tilesets.and_then(|t| t.version(handle))),
      TilesetSource::Embedded(_) => None,
    }));
  }
  stamp
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
      landing: None,
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
          landing: None,
        }))
      }
      LoadState::Loading | LoadState::NotLoaded => None,
    },
    // `resolve_world_levels` already stood in for a level the world has
    MapSource::WorldLevel {
      world: source,
      level,
    } => match source.state(server) {
      LoadState::Failed(error) => unreported(error),
      LoadState::Loaded => {
        // Reported once per version of the world
        let version = source.version(world);
        if failed.is_some_and(|f| f.version.is_some() && f.version == version) {
          return None;
        }
        let path = source
          .path(server)
          .unwrap_or_else(|| AssetPath::from_normal(Cow::Borrowed("world")));
        let error = AssetError::decode(&path, format!("the world has no level {level}"));
        Some(Ready::Failed(MapFailed {
          error: Arc::new(error),
          version,
          landing: None,
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
pub(crate) fn child_of(world: &mut World, parent: Entity, transform: Transform) -> Entity {
  world.spawn((transform, ChildOf(parent))).id()
}

/// A layer as it spawns: its info and what it holds
pub(crate) struct PlannedLayer {
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
pub(crate) type ClassTiles = Vec<FastMap<u32, (String, Properties)>>;

pub(crate) fn class_tiles(world: &World, map: &TileMap, classes: &[String]) -> ClassTiles {
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
pub(crate) fn take_class_tiles(grid: &mut TileGrid, classes: &ClassTiles) -> Vec<ClassTile> {
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

/// A map's landing, worked out off the main thread from copies of its
/// asset's layers, its root's settings and its tilesets' shapes
pub(crate) struct LandingJob {
  map: Entity,
  /// The loads of the map's files it is worked out from (`content_stamp`)
  stamp: Vec<Option<u64>>,
  layers: Vec<MapLayer>,
  classes: ClassTiles,
  collision: collision::Inputs,
  shadows: occluders::Inputs,
  /// The tilesets the map drawer has no padding of yet
  pads: Vec<ToPad>,
}

/// What a map lands with: its layers as they spawn, its colliders and
/// shadows worked out for them, what a reload compares with, and its
/// tilesets' paddings made ahead
pub(crate) struct Landed {
  layers: Vec<PlannedLayer>,
  collision: collision::Prepared,
  shadows: occluders::Prepared,
  applied: MapApplied,
  paddings: Vec<padded::Prepared>,
}

/// The landing job of the map `map`, its root's asset ready
fn landing_job(world: &World, map: Entity) -> Option<LandingJob> {
  let root = world.get::<MapRoot>(map)?;
  let asset = map_asset(world, root)?;
  let tilesets = world.get_resource::<Assets<Tileset>>();
  let shapes = sources(asset, tilesets);
  let tileset_stamps = stamps(asset, tilesets);
  // Where maps are drawn, the tilesets not padded yet are padded with the
  // landing, so its frame only writes their textures
  let pads = match (
    world.get_resource::<PaddedCache>(),
    tilesets,
    world.get_resource::<Assets<Image>>(),
  ) {
    (Some(padded), Some(tilesets), Some(images)) => {
      padded.lock().to_pad(&asset.tilesets, tilesets, images)
    }
    _ => Vec::new(),
  };
  let mut layers = asset.layers.clone();
  inherit_tile_data(world, root, &mut layers);
  Some(LandingJob {
    map,
    stamp: content_stamp(world, root),
    layers,
    classes: class_tiles(world, asset, &root.entity_tiles),
    collision: collision::Inputs {
      collision: root.collision,
      overrides: root.intgrid_collision.clone(),
      properties: asset.properties.clone(),
      tile_size: asset.tile_size,
      shapes: Arc::clone(&shapes),
      tilesets: tileset_stamps.clone(),
    },
    shadows: occluders::Inputs {
      overrides: root.intgrid_collision.clone(),
      shapes,
      tilesets: tileset_stamps,
    },
    pads,
  })
}

/// A map property that makes a test's landing panic, on every try
#[cfg(test)]
const PANICKING_LANDING: &str = "test: the landing panics";

impl LandingJob {
  /// The map's layers as they will spawn (each tile layer's grid with class
  /// tiles taken out, every chunk dirty so it is drawn once), and its
  /// colliders and shadows for them
  fn run(self) -> Landed {
    // A test's map whose landing always panics
    #[cfg(test)]
    if self.collision.properties.get(PANICKING_LANDING).is_some() {
      panic!("a test's landing that always panics");
    }
    let mut applied = MapApplied::new(self.stamp, &self.layers);
    let layers: Vec<PlannedLayer> = self
      .layers
      .into_iter()
      .map(|layer| plan_layer(layer, &self.classes))
      .collect();
    for layer in &layers {
      if let Content::Tiles(_, found) = &layer.content {
        applied.note_class_tiles(layer.info.id, found);
      }
    }
    let views: Vec<LayerView<'_>> = layers
      .iter()
      .enumerate()
      .filter_map(|(index, layer)| match &layer.content {
        Content::Tiles(data, _) => Some(LayerView {
          index,
          entity: Entity::PLACEHOLDER,
          info: &layer.info,
          data,
        }),
        _ => None,
      })
      .collect();
    let collision = collision::prepare(self.map, &views, self.collision);
    let shadows = occluders::prepare(&views, self.shadows);
    drop(views);
    let paddings = self.pads.into_iter().filter_map(ToPad::pad).collect();
    Landed {
      layers,
      collision,
      shadows,
      applied,
      paddings,
    }
  }
}

/// `layer` as it spawns: a tile layer's grid without the tiles of a
/// registered class (and every chunk dirty, so it is drawn once), with
/// those tiles beside it
pub(crate) fn plan_layer(layer: MapLayer, classes: &ClassTiles) -> PlannedLayer {
  PlannedLayer {
    info: layer.info,
    content: match layer.kind {
      LayerKind::Tiles {
        mut grid,
        intgrid,
        cell_size,
      } => {
        let found = take_class_tiles(&mut grid, classes);
        grid.mark_all_dirty();
        let data = TileLayerData {
          grid,
          intgrid,
          cell_size,
        };
        Content::Tiles(data, found)
      }
      LayerKind::Objects(objects) => Content::Objects(objects),
      LayerKind::Image(image) => Content::Image(image),
    },
  }
}

/// What a layer `spawn_layer` spawned holds that spawns once the map's
/// other layers are in: a tile layer's class tiles (with its cell size),
/// or an object layer's objects
pub(crate) enum LayerRest {
  Tiles(Vec2, Vec<ClassTile>),
  Objects(Vec<MapObjectData>),
  Nothing,
}

/// Spawns `planned` as layer `index` of `map`, a child of it at the
/// layer's offset
pub(crate) fn spawn_layer(
  world: &mut World,
  map: Entity,
  index: usize,
  PlannedLayer { info, content }: PlannedLayer,
) -> (Entity, LayerRest) {
  let layer = child_of(world, map, Transform::from_translation(info.offset));
  let layer_ref = MapLayerRef {
    map,
    index,
    name: info.name.clone(),
  };
  let Ok(mut entity) = world.get_entity_mut(layer) else {
    return (layer, LayerRest::Nothing);
  };
  let rest = match content {
    Content::Tiles(data, found) => {
      let cell_size = data.cell_size.as_vec2();
      entity.insert((layer_ref, info, data, LayerTiles::default()));
      LayerRest::Tiles(cell_size, found)
    }
    Content::Objects(objects) => {
      entity.insert((layer_ref, info));
      LayerRest::Objects(objects)
    }
    Content::Image(image) => {
      entity.insert((layer_ref, info, image));
      LayerRest::Nothing
    }
  };
  (layer, rest)
}

/// Spawns `found`, the class tiles of the map's layer `layer` (its `index`,
/// of cells `cell_size` pixels), and lists them in its `LayerTiles`
pub(crate) fn spawn_class_tiles(
  world: &mut World,
  map: Entity,
  (layer, index): (Entity, usize),
  cell_size: Vec2,
  found: Vec<ClassTile>,
) {
  let mut spawned = FastMap::default();
  for tile in found {
    if world.get_entity(layer).is_err() {
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

/// An object's component and its `Transform` in its layer
pub(crate) fn object_parts(map: Entity, object: MapObjectData) -> (MapObject, Transform) {
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

/// Lands `map` whole, if it is still a map waiting to spawn (an entity
/// despawned since, no longer a map, or disabled spawns nothing; a
/// disabled one is worked out again once enabled): its layers, the
/// objects' entities, its colliders and shadows, then the objects'
/// components (observers run as each arrives), its class tiles, and
/// `MapEvent::Spawned`
fn land(world: &mut World, map: Entity, landed: Landed) {
  if !waits_to_land(world, map) {
    return;
  }
  let Landed {
    layers: planned,
    collision,
    shadows,
    applied,
    paddings,
  } = landed;
  // Its tilesets' paddings, made with it, for the drawer to write
  if !paddings.is_empty()
    && let Some(cache) = world.get_resource::<PaddedCache>().cloned()
    && let Some(mut images) = world.get_resource_mut::<Assets<Image>>()
  {
    let mut padded = cache.lock();
    for prepared in paddings {
      padded.adopt(prepared, &mut images);
    }
  }

  // A game's observers run as each part arrives, and may despawn the map, a
  // layer or an object before its turn: what is gone is skipped, and a map
  // gone takes all of it with it
  let alive = |world: &World, entity: Entity| world.get_entity(entity).is_ok();
  let mut layers = Vec::with_capacity(planned.len());
  let mut object_layers = Vec::new();
  let mut tile_layers = Vec::new();
  for (index, planned) in planned.into_iter().enumerate() {
    if !alive(world, map) {
      return;
    }
    let (layer, rest) = spawn_layer(world, map, index, planned);
    layers.push(layer);
    match rest {
      LayerRest::Tiles(cell_size, found) => tile_layers.push((layer, index, cell_size, found)),
      LayerRest::Objects(objects) => object_layers.push((layer, objects)),
      LayerRest::Nothing => {}
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
    .insert((MapIds { objects: ids }, MapLayers(layers.clone()), applied))
    .remove::<MapFailed>();
  // Its colliders and shadows before anything observes its objects
  collision::land(world, map, &layers, collision);
  occluders::land(world, map, &layers, shadows);
  for (entity, component) in objects {
    if let Ok(mut object) = world.get_entity_mut(entity) {
      object.insert(component);
    }
  }
  for (layer, index, cell_size, found) in tile_layers {
    spawn_class_tiles(world, map, (layer, index), cell_size, found);
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
  if world.get::<MapLayers>(map).is_none()
    && world.get::<MapFailed>(map).is_none()
    && world.get::<MapLanding>(map).is_none()
  {
    return;
  }
  world.commands().queue(move |world: &mut World| {
    let Ok(mut entity) = world.get_entity_mut(map) else {
      return;
    };
    let layers = entity.take::<MapLayers>();
    entity.remove::<(MapIds, MapFailed, MapLanding, MapApplied)>();
    for layer in layers.map(|l| l.0).unwrap_or_default() {
      let _ = world.try_despawn(layer);
    }
  });
}

pub(crate) fn send(world: &mut World, event: MapEvent) {
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

    /// The spawn system alone, with no pump before it, run until no
    /// landing it started is still out (each is worked out on another
    /// thread), so a map that starts landing lands in this frame
    fn spawn(&mut self, world: &mut World) {
      let start = Instant::now();
      loop {
        self.0.run(world);
        let out = world.query::<&MapLanding>().iter(world).count();
        if out == 0 {
          return;
        }
        assert!(
          start.elapsed() < Duration::from_secs(5),
          "a landing never came"
        );
        std::thread::yield_now();
      }
    }

    fn run(&mut self, world: &mut World) {
      pump(world);
      self.spawn(world);
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
  fn a_landing_pads_its_tilesets_off_the_frame_and_hands_them_over() {
    use crate::core::graphics::Image;
    use crate::core::tilemap::TileMap;
    use crate::core::tilemap::padded::PaddedCache;
    use crate::core::tilemap::test_util::MapWorld;
    // An atlas of 400 16 px tiles: its padding is a texture of its own
    // (680 px), whose levels are worth making off the frame
    let mut maps = MapWorld::new();
    maps.world.insert_resource(PaddedCache::default());
    let (cols, rows) = (20, 20);
    let image = maps
      .world
      .resource_mut::<Assets<Image>>()
      .add(Image::solid_rect(16 * cols, 16 * rows, 90, 160, 60, 255));
    let mut set = Tileset::new(
      "atlas",
      UVec2::splat(16),
      TilesetImage::Atlas {
        image,
        size: UVec2::new(16 * cols, 16 * rows),
      },
    );
    set.columns = cols;
    set.tile_count = cols * rows;
    let set = TilesetSource::Asset(maps.world.resource_mut::<Assets<Tileset>>().add(set));
    let mut map = TileMap::new(UVec2::splat(8), UVec2::splat(16));
    let index = map.add_tileset(set.clone());
    let layer = map.add_tile_layer("ground");
    map
      .tiles_mut(layer)
      .expect("a layer")
      .set(IVec2::ZERO, Some(Tile::new(index, 3)));
    let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
    // No drawer runs here: whatever padding there is, the landing made
    maps.spawn(MapRoot::new(handle));
    let padded = maps.world.resource::<PaddedCache>().lock();
    assert_eq!(padded.len(), 1, "the landing handed its padding over");
    let tilesets = maps.world.resource::<Assets<Tileset>>();
    assert!(
      padded
        .serial(&set, tilesets)
        .is_some_and(|serial| serial > 0)
    );
    // The padded image came with its levels made: uploading it only copies
    let images = maps.world.resource::<Assets<Image>>();
    let made = images
      .iter()
      .filter(|(_, image)| image.width() > 320 && image.mips().is_some())
      .count();
    assert_eq!(made, 1, "the padded image's levels made ahead");
  }

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

  /// An object as a game's observer saw its `MapObject` added: its entity,
  /// id, class, `hp` and the entity its `target` reference resolved to
  #[derive(Clone)]
  struct Arrival {
    entity: Entity,
    id: String,
    class: Option<String>,
    hp: Option<i64>,
    target: Option<Entity>,
  }

  /// What a game's observers saw of the objects as they arrived and went
  #[derive(Default)]
  struct Arrivals {
    added: Vec<Arrival>,
    inserted: Vec<Entity>,
    removed: Vec<Entity>,
  }

  #[test]
  fn a_game_s_observers_see_each_object_arrive_whole_and_go() {
    // The names a game has, from the prelude
    use crate::prelude::{Add, Insert, Remove};
    let mut world = world_over(files_with(&[]));
    let seen = Arc::new(Mutex::new(Arrivals::default()));
    let arrivals = Arc::clone(&seen);
    world.add_observer(
      move |add: On<Add<MapObject>>, objects: Query<&MapObject>, world: &World| {
        let Ok(object) = objects.get(add.entity) else {
          return;
        };
        let mut seen = arrivals.lock().unwrap_or_else(|e| e.into_inner());
        seen.added.push(Arrival {
          entity: add.entity,
          id: object.id.clone(),
          class: object.class.clone(),
          hp: object.properties.int("hp"),
          target: object.reference("target", world),
        });
      },
    );
    let arrivals = Arc::clone(&seen);
    world.add_observer(move |insert: On<Insert<MapObject>>| {
      let mut seen = arrivals.lock().unwrap_or_else(|e| e.into_inner());
      seen.inserted.push(insert.entity);
    });
    let arrivals = Arc::clone(&seen);
    world.add_observer(move |remove: On<Remove<MapObject>>| {
      let mut seen = arrivals.lock().unwrap_or_else(|e| e.into_inner());
      seen.removed.push(remove.entity);
    });
    let handle = AssetsView::new(&mut world)
      .load_now::<TileMap>("tiled/maps/objects.tmj")
      .expect("objects.tmj loads");
    let map = world.spawn(MapRoot::new(handle)).id();
    Frames::new().until(&mut world, |world| world.get::<MapIds>(map).is_some());

    // Once each, as it arrived
    let mut objects: Vec<Entity> = world
      .query::<(Entity, &MapObject)>()
      .iter(&world)
      .map(|(e, _)| e)
      .collect();
    objects.sort();
    let (spawn, _) = object(&mut world, map, "spawn");
    let got = seen.lock().unwrap_or_else(|e| e.into_inner());
    let mut added: Vec<Entity> = got.added.iter().map(|a| a.entity).collect();
    added.sort();
    assert_eq!(objects.len(), 17);
    assert_eq!(added, objects, "one Add per object");
    let mut inserted = got.inserted.clone();
    inserted.sort();
    assert_eq!(inserted, objects, "one Insert per object");
    // Its fields filled, and its reference to an object later in the file
    // resolved, as it arrived
    let crate_seen = got.added.iter().find(|a| a.id == "1").cloned();
    let Some(Arrival {
      class, hp, target, ..
    }) = crate_seen
    else {
      panic!("the crate never arrived");
    };
    assert_eq!(
      (class.as_deref(), hp, target),
      (Some("Crate"), Some(25), Some(spawn))
    );

    drop(got);

    // Despawning the map: one Remove per object
    world.despawn(map);
    let mut removed = seen
      .lock()
      .unwrap_or_else(|e| e.into_inner())
      .removed
      .clone();
    removed.sort();
    assert_eq!(removed, objects);
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

    // By identifier: its layers (Collision's stacked tile a sub-layer of
    // its own), IntGrid, tiles and entities
    assert_eq!(
      layer_names(&mut world, by_name),
      ["Decor", "Collision", "Collision#1", "Props", "Entities"]
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

  #[test]
  fn a_tile_object_takes_its_tile_s_class_and_properties() {
    // Saved by Tiled 1.12.2; Tiled's own resolution of each (its export
    // with types and properties resolved) is what is asserted. The fixture
    // project, where the loaders look for it
    let project = std::fs::read_to_string(concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/fixtures/tilemap/tiled/project.tiled-project"
    ))
    .expect("the fixture project");
    let mut world = world_over(files_with(&[("project.tiled-project", &project)]));
    let handle = AssetsView::new(&mut world)
      .load_now::<TileMap>("tiled/maps/tile-objects.tmj")
      .expect("tile-objects.tmj loads");
    let map = world.spawn(MapRoot::new(handle)).id();
    Frames::new().run(&mut world);
    // No class of its own: the tile's Crate, with the tile's hp and the
    // class's defaults
    let (_, crate_) = object(&mut world, map, "crate");
    assert_eq!(crate_.class.as_deref(), Some("Crate"));
    assert_eq!(crate_.properties.int("hp"), Some(40));
    assert_eq!(crate_.properties.float("speed"), Some(1.5));
    assert_eq!(crate_.properties.string("label"), Some("plain"));
    // Its own hp lies over the tile's
    let (_, weak) = object(&mut world, map, "weak crate");
    assert_eq!(weak.class.as_deref(), Some("Crate"));
    assert_eq!(weak.properties.int("hp"), Some(7));
    assert_eq!(weak.properties.float("speed"), Some(1.5));
    // A tile with properties and no class
    let (_, gem) = object(&mut world, map, "gem");
    assert_eq!(gem.class, None);
    assert_eq!(gem.properties.bool("shiny"), Some(true));
    // A class of its own wins, and the tile's hp comes with it
    let (_, drop) = object(&mut world, map, "drop crate");
    assert_eq!(drop.class.as_deref(), Some("Drop"));
    assert_eq!(drop.properties.int("hp"), Some(40));
    // A tile with no data adds nothing
    let (_, plain) = object(&mut world, map, "plain");
    assert_eq!(plain.class, None);
    assert!(plain.properties.is_empty());
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

  /// A 256 x 256 map of four full tile layers: big enough that its landing
  /// is still out when the frame that sent it ends
  fn big_map() -> TileMap {
    let mut map = TileMap::new(UVec2::splat(256), UVec2::splat(16));
    let tileset = Tileset::new("plain", UVec2::splat(16), TilesetImage::Collection);
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(tileset)));
    for name in ["ground", "decor 1", "decor 2", "decor 3"] {
      let layer = map.add_tile_layer(name);
      if let Some(grid) = map.tiles_mut(layer) {
        grid.fill(IVec2::ZERO, IVec2::splat(255), Some(Tile::new(set, 0)));
      }
    }
    map
  }

  #[test]
  fn a_root_disabled_while_its_landing_is_out_lands_once_enabled() {
    use crate::core::ecs::Disabled;
    let mut world = world_over(files_with(&[]));
    let handle = world.resource_mut::<Assets<TileMap>>().add(big_map());
    let mut frames = Frames::new();
    let mut cursor = MessageCursor::<MapEvent>::default();
    let map = world.spawn(MapRoot::new(handle)).id();
    // The frame that sends its landing
    pump(&mut world);
    frames.0.run(&mut world);
    assert!(world.get::<MapLanding>(map).is_some(), "its landing is out");
    world.entity_mut(map).insert(Disabled);
    // The landing comes back while the root is disabled: nothing lands
    let start = Instant::now();
    while world.get::<MapLanding>(map).is_some() {
      frames.0.run(&mut world);
      assert!(
        start.elapsed() < Duration::from_secs(5),
        "its landing never came back while the root was disabled"
      );
      std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..3 {
      frames.0.run(&mut world);
    }
    assert!(
      world.get::<MapIds>(map).is_none(),
      "a disabled root lands nothing"
    );
    assert!(spawned(&events(&world, &mut cursor)).is_empty());
    // Enabled again, it is worked out again and lands, once
    world.entity_mut(map).remove::<Disabled>();
    frames.until(&mut world, |world| world.get::<MapIds>(map).is_some());
    frames.run(&mut world);
    frames.run(&mut world);
    assert!(MapsView::new(&mut world).is_ready(map));
    assert_eq!(layers_of(&mut world, map).len(), 4);
    assert_eq!(spawned(&events(&world, &mut cursor)), [map]);
  }

  #[test]
  fn a_root_despawned_or_left_with_its_scene_while_its_landing_is_out_spawns_nothing() {
    let mut world = world_over(files_with(&[]));
    let handle = world.resource_mut::<Assets<TileMap>>().add(big_map());
    let mut frames = Frames::new();
    let mut cursor = MessageCursor::<MapEvent>::default();
    let all = |world: &mut World| {
      world
        .query_filtered::<Entity, Without<crate::core::ecs::IsResource>>()
        .iter(world)
        .count()
    };
    let before = all(&mut world);
    let gone = world.spawn(MapRoot::new(handle.clone())).id();
    let left = world.spawn(MapRoot::new(handle)).id();
    // The frame that sends their landings
    pump(&mut world);
    frames.0.run(&mut world);
    assert!(
      world.get::<MapLanding>(gone).is_some() && world.get::<MapLanding>(left).is_some(),
      "both landings are out"
    );
    // One root despawned, then its scene left (`SceneCleanup::WORLD`)
    world.despawn(gone);
    despawn_game_entities(&mut world);
    assert!(world.get_entity(left).is_err());
    // A map sent after them lands after them: the builder works in order
    let after = world.resource_mut::<Assets<TileMap>>().add(code_map());
    let next = world.spawn(MapRoot::new(after)).id();
    frames.until(&mut world, |world| world.get::<MapIds>(next).is_some());
    assert_eq!(spawned(&events(&world, &mut cursor)), [next]);
    assert_eq!(
      count::<MapLayerRef>(&mut world),
      2,
      "the next map's layers only"
    );
    // Nothing of the two is left behind
    world.despawn(next);
    frames.run(&mut world);
    assert_eq!(all(&mut world), before);
    assert!(events(&world, &mut cursor).is_empty());
  }

  #[test]
  fn a_landing_that_always_panics_is_reported_once_and_not_retried() {
    let mut world = world_over(files_with(&[]));
    let mut asset = code_map();
    asset
      .properties
      .insert(PANICKING_LANDING, PropertyValue::Bool(true));
    let handle = world.resource_mut::<Assets<TileMap>>().add(asset);
    let mut frames = Frames::new();
    let mut cursor = MessageCursor::<MapEvent>::default();
    let map = world.spawn(MapRoot::new(handle.clone())).id();
    frames.until(&mut world, |world| world.get::<MapFailed>(map).is_some());
    // Not tried again: no landing goes out
    for _ in 0..10 {
      pump(&mut world);
      frames.0.run(&mut world);
      assert!(
        world.get::<MapLanding>(map).is_none(),
        "its landing was sent again"
      );
    }
    let sent = events(&world, &mut cursor);
    let failures = failed(&sent);
    assert_eq!(failures.len(), 1, "reported once: {failures:?}");
    assert_eq!(failures[0].0, map);
    assert!(failures[0].1.contains("panicked"), "{}", failures[0].1);
    assert!(spawned(&sent).is_empty());
    // Its map changes: it is tried again, and lands
    if let Some(asset) = world.resource_mut::<Assets<TileMap>>().get_mut(&handle) {
      asset.properties = Properties::default();
    }
    frames.until(&mut world, |world| world.get::<MapIds>(map).is_some());
    assert_eq!(spawned(&events(&world, &mut cursor)), [map]);
    assert!(world.get::<MapFailed>(map).is_none());
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
    // The missing map's failure is reported as the others start landing,
    // before the level that takes it lands
    assert!(
      failed(&sent).iter().all(|(map, _)| *map == victim),
      "{sent:?}"
    );
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
    // It lands a frame or so in: its landing is worked out off the main thread
    crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
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
