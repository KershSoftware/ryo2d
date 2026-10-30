use std::time::{Duration, Instant};

use glam::{IVec2, UVec2, Vec2};

use super::*;
use crate::core::asset::{AssetEvent, Assets, AssetsView, MemorySource, pump};
use crate::core::ecs::kernel::{Add, MessageCursor, Messages};
use crate::core::ecs::{
  ChildOf, Component, Disabled, Entity, On, Query, ResMut, Resource, Transform, Velocity, World,
};
use crate::core::engine::EngineBuilder;
use crate::core::physics::{
  CharacterController, CharacterState, Collider, CollisionStopped, Physics, TileCell, TileColliders,
};
use crate::core::render::Tonemap;
use crate::core::scene::{Scene, SceneContext};
use crate::core::testing::HeadlessEngine;
use crate::core::tilemap::test_util::{MapWorld, files_with, layers_of, load_map, maps_landed};
use crate::core::tilemap::{
  LdtkProject, MapEvent, MapIds, MapLayerRef, MapObject, MapOccluder, MapRoot, MapTile, MapsView,
  Tile, TileLayerData, TileMap,
};

/// What a game put on an entity
#[derive(Component, Debug, PartialEq)]
struct Marker(u32);

/// 16 px tiles over the fixture image: tile 0 a solid block, tile 5 of
/// class "coin"
const SET: &str = r#"{"type":"tileset","name":"set","tilewidth":16,"tileheight":16,
  "tilecount":16,"columns":4,"image":"../tiled/images/tiles.png","imagewidth":64,"imageheight":64,
  "tiles":[
    {"id":0,"objectgroup":{"type":"objectgroup","objects":[{"id":1,"x":0,"y":0,"width":16,"height":16}]}},
    {"id":5,"type":"coin"}]}"#;

/// A tile layer's JSON: `data` holds gids, row by row
fn tiles(id: u32, name: &str, size: (u32, u32), data: &[u32], extra: &str) -> String {
  let data: Vec<String> = data.iter().map(u32::to_string).collect();
  format!(
    r#"{{"id":{id},"name":"{name}","type":"tilelayer","width":{},"height":{},"data":[{}]{extra}}}"#,
    size.0,
    size.1,
    data.join(",")
  )
}

/// An object layer's JSON
fn things(id: u32, objects: &[String]) -> String {
  format!(
    r#"{{"id":{id},"name":"things","type":"objectgroup","objects":[{}]}}"#,
    objects.join(",")
  )
}

/// A 16 x 16 rectangle object at (`x`, `y`), with `extra` fields
fn object(id: u32, name: &str, (x, y): (u32, u32), extra: &str) -> String {
  format!(r#"{{"id":{id},"name":"{name}","x":{x},"y":{y},"width":16,"height":16{extra}}}"#)
}

/// A map of `size` cells of 16 px over `set.tsj` in its folder
fn map_of(size: (u32, u32), layers: &[String]) -> String {
  format!(
    r#"{{"type":"map","orientation":"orthogonal","width":{},"height":{},
      "tilewidth":16,"tileheight":16,"tilesets":[{{"firstgid":1,"source":"set.tsj"}}],
      "layers":[{}]}}"#,
    size.0,
    size.1,
    layers.join(",")
  )
}

/// A 4 x 3 map: tile layer "ground" of `ground` (gids) and object layer
/// "things" of `objects`
fn small(ground: [u32; 12], objects: &[String]) -> String {
  map_of(
    (4, 3),
    &[tiles(1, "ground", (4, 3), &ground, ""), things(2, objects)],
  )
}

/// The cells `gids` stand for, in a layer over one tileset
fn cells(gids: &[u32]) -> Vec<Option<Tile>> {
  gids
    .iter()
    .map(|&gid| (gid != 0).then(|| Tile::new(0, gid - 1)))
    .collect()
}

/// A tile layer's cells, row by row
fn grid_of(world: &World, layer: Entity) -> Vec<Option<Tile>> {
  let Some(data) = world.get::<TileLayerData>(layer) else {
    panic!("{layer:?} is not a tile layer")
  };
  let size = data.grid.size().as_ivec2();
  (0..size.y)
    .flat_map(|y| (0..size.x).map(move |x| IVec2::new(x, y)))
    .map(|cell| data.grid.get(cell))
    .collect()
}

/// The map's object called `name`
fn object_named(world: &mut World, map: Entity, name: &str) -> (Entity, MapObject) {
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

fn translation(world: &World, entity: Entity) -> Vec2 {
  match world.get::<Transform>(entity) {
    Some(t) => t.translation,
    None => panic!("{entity:?} has no transform"),
  }
}

fn id_of(world: &World, map: Entity, id: &str) -> Option<Entity> {
  world.get::<MapIds>(map).and_then(|ids| ids.get(id))
}

/// The map events sent from its making on
struct Watch {
  cursor: MessageCursor<MapEvent>,
  sent: Vec<MapEvent>,
}

impl Watch {
  /// Reads only what is sent from now on
  fn new(world: &World) -> Watch {
    let cursor = world
      .get_resource::<Messages<MapEvent>>()
      .map(Messages::get_cursor_current)
      .unwrap_or_default();
    Watch {
      cursor,
      sent: Vec::new(),
    }
  }

  fn read(&mut self, world: &World) {
    if let Some(messages) = world.get_resource::<Messages<MapEvent>>() {
      self.sent.extend(self.cursor.read(messages).cloned());
    }
  }

  fn reloaded(&self) -> Vec<Entity> {
    self
      .sent
      .iter()
      .filter_map(|e| match e {
        MapEvent::Reloaded { map } => Some(*map),
        _ => None,
      })
      .collect()
  }

  fn spawned(&self) -> Vec<Entity> {
    self
      .sent
      .iter()
      .filter_map(|e| match e {
        MapEvent::Spawned { map } => Some(*map),
        _ => None,
      })
      .collect()
  }

  /// Frames until `map` is reloaded
  fn until_reloaded(&mut self, maps: &mut MapWorld, map: Entity) {
    maps.frames_until(|world| {
      self.read(world);
      self.reloaded().contains(&map)
    });
  }
}

/// A map world over the fixtures and `files`, and the map at `path`
/// spawned at the origin
fn spawned(
  files: &MemorySource,
  path: &str,
  root: impl Fn(Handle) -> MapRoot,
) -> (MapWorld, Entity) {
  let mut maps = MapWorld::over(files.clone());
  let handle = load_map(&mut maps.world, path);
  let map = maps.spawn(root(handle));
  (maps, map)
}

type Handle = crate::core::asset::Handle<TileMap>;

#[test]
fn saving_a_map_updates_its_tiles_and_keeps_unchanged_objects() {
  let objects = [object(1, "a", (16, 0), ""), object(2, "b", (32, 16), "")];
  let first = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1];
  let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &small(first, &objects))]);
  let (mut maps, map) = spawned(&files, "r/m.tmj", MapRoot::new);
  let layers = layers_of(&mut maps.world, map);
  let (a, a_object) = object_named(&mut maps.world, map, "a");
  let (b, _) = object_named(&mut maps.world, map, "b");
  // The game marks one object and moves the other, and digs a tile
  maps.world.entity_mut(a).insert(Marker(7));
  let moved = Vec2::new(99.0, 5.0);
  if let Some(mut t) = maps.world.get_mut::<Transform>(b) {
    t.translation = moved;
  }
  assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", IVec2::new(1, 2), None));

  let mut watch = Watch::new(&maps.world);
  let saved = [1, 0, 0, 2, 0, 0, 0, 0, 1, 1, 0, 1];
  files.insert("r/m.tmj", small(saved, &objects));
  watch.until_reloaded(&mut maps, map);

  // The tiles are the file's, the dug cell's too, and they collide so
  assert_eq!(grid_of(&maps.world, layers[0]), cells(&saved));
  let tiles = maps.world.get::<TileColliders>(map).expect("colliders");
  assert_eq!(tiles.get(0, 0), TileCell::Solid);
  assert_eq!(tiles.get(1, 2), TileCell::Solid);
  assert_eq!(tiles.get(2, 2), TileCell::Empty);
  // The same layers, and the objects the save left alone as the game has them
  assert_eq!(layers_of(&mut maps.world, map), layers);
  assert_eq!(id_of(&maps.world, map, "1"), Some(a));
  assert_eq!(id_of(&maps.world, map, "2"), Some(b));
  assert_eq!(maps.world.get::<Marker>(a), Some(&Marker(7)));
  assert_eq!(
    format!("{:?}", maps.world.get::<MapObject>(a)),
    format!("{:?}", Some(&a_object))
  );
  assert_eq!(translation(&maps.world, b), moved);
  assert_eq!(watch.reloaded(), [map], "one reload");
  assert!(watch.spawned().is_empty());
}

#[test]
fn a_moved_object_moves_and_keeps_game_components() {
  let hp = |n: i32| format!(r#","properties":[{{"name":"hp","type":"int","value":{n}}}]"#);
  let ground = [0; 12];
  let objects = |a: String| [a, object(2, "b", (32, 16), "")];
  let first = objects(object(1, "a", (16, 0), &hp(1)));
  let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &small(ground, &first))]);
  let (mut maps, map) = spawned(&files, "r/m.tmj", MapRoot::new);
  let (a, _) = object_named(&mut maps.world, map, "a");
  let (b, _) = object_named(&mut maps.world, map, "b");
  maps.world.entity_mut(a).insert(Marker(7));
  let moved = Vec2::new(99.0, 5.0);
  if let Some(mut t) = maps.world.get_mut::<Transform>(b) {
    t.translation = moved;
  }

  let mut watch = Watch::new(&maps.world);
  let saved = objects(object(1, "a", (48, 32), &hp(2)));
  files.insert("r/m.tmj", small(ground, &saved));
  watch.until_reloaded(&mut maps, map);

  // Moved and changed in place, keeping what the game gave it
  assert_eq!(id_of(&maps.world, map, "1"), Some(a));
  assert_eq!(maps.world.get::<Marker>(a), Some(&Marker(7)));
  assert_eq!(translation(&maps.world, a), Vec2::new(56.0, 40.0));
  let a_object = maps.world.get::<MapObject>(a).expect("an object");
  assert_eq!(a_object.properties.int("hp"), Some(2));
  // The other stays where the game put it
  assert_eq!(id_of(&maps.world, map, "2"), Some(b));
  assert_eq!(translation(&maps.world, b), moved);
}

/// The ids of the objects whose `MapObject` arrived
#[derive(Resource, Default)]
struct Arrived(Vec<String>);

fn note_arrival(add: On<Add<MapObject>>, objects: Query<&MapObject>, mut arrived: ResMut<Arrived>) {
  if let Ok(object) = objects.get(add.entity) {
    arrived.0.push(object.id.clone());
  }
}

#[test]
fn new_and_removed_objects_spawn_and_despawn() {
  let ground = [0; 12];
  let at = |id: u32, name: &str| object(id, name, (16 * (id % 4), 16), "");
  let first = [at(1, "a"), at(2, "b"), at(3, "c")];
  let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &small(ground, &first))]);
  let mut maps = MapWorld::over(files.clone());
  maps.world.init_resource::<Arrived>();
  maps.world.add_observer(note_arrival);
  let handle = load_map(&mut maps.world, "r/m.tmj");
  let map = maps.spawn(MapRoot::new(handle));
  let (a, _) = object_named(&mut maps.world, map, "a");
  let (b, _) = object_named(&mut maps.world, map, "b");
  let (c, _) = object_named(&mut maps.world, map, "c");
  let layer = maps.world.get::<ChildOf>(a).map(ChildOf::parent);
  maps.world.resource_mut::<Arrived>().0.clear();
  // The game takes c away (an enemy killed)
  maps.world.despawn(c);

  let mut watch = Watch::new(&maps.world);
  let saved = [at(1, "a"), at(3, "c"), at(4, "d")];
  files.insert("r/m.tmj", small(ground, &saved));
  watch.until_reloaded(&mut maps, map);

  assert_eq!(id_of(&maps.world, map, "1"), Some(a), "kept");
  assert!(maps.world.get_entity(b).is_err(), "removed from the file");
  assert_eq!(id_of(&maps.world, map, "2"), None);
  // A new object spawns in its layer; one the game took away stays away,
  // the save having left it as it was
  let (d, d_object) = object_named(&mut maps.world, map, "d");
  assert_eq!(d_object.id, "4");
  assert_eq!(id_of(&maps.world, map, "4"), Some(d));
  assert_eq!(maps.world.get::<ChildOf>(d).map(ChildOf::parent), layer);
  assert_eq!(id_of(&maps.world, map, "3"), None, "c stays despawned");
  let count = |world: &mut World| {
    world
      .query::<&MapObject>()
      .iter(world)
      .filter(|o| o.map == map)
      .count()
  };
  assert_eq!(count(&mut maps.world), 2);
  let arrived = std::mem::take(&mut maps.world.resource_mut::<Arrived>().0);
  assert_eq!(arrived, ["4"], "only the new object arrives");

  // A save that changes it brings it back
  let mut watch = Watch::new(&maps.world);
  let moved_c = object(3, "c", (48, 32), "");
  files.insert("r/m.tmj", small(ground, &[at(1, "a"), moved_c, at(4, "d")]));
  watch.until_reloaded(&mut maps, map);
  let (again, _) = object_named(&mut maps.world, map, "c");
  assert_ne!(again, c);
  assert_eq!(id_of(&maps.world, map, "3"), Some(again));
  assert_eq!(translation(&maps.world, again), Vec2::new(56.0, 40.0));
  assert_eq!(id_of(&maps.world, map, "4"), Some(d));
  let arrived = std::mem::take(&mut maps.world.resource_mut::<Arrived>().0);
  assert_eq!(arrived, ["3"]);
  assert_eq!(count(&mut maps.world), 3);
}

#[test]
fn an_object_saved_into_another_layer_moves_there_with_its_entity() {
  let layers = |a_in: u32| {
    let a = vec![object(1, "a", (16, 0), "")];
    let (two, three) = if a_in == 2 { (a, vec![]) } else { (vec![], a) };
    map_of(
      (4, 3),
      &[
        things(2, &two),
        things(3, &three).replace(r#""name":"things""#, r#""name":"more""#),
      ],
    )
  };
  let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &layers(2))]);
  let (mut maps, map) = spawned(&files, "r/m.tmj", MapRoot::new);
  let found = layers_of(&mut maps.world, map);
  let (a, _) = object_named(&mut maps.world, map, "a");
  assert_eq!(
    maps.world.get::<ChildOf>(a).map(ChildOf::parent),
    Some(found[0])
  );
  maps.world.entity_mut(a).insert(Marker(2));

  let mut watch = Watch::new(&maps.world);
  files.insert("r/m.tmj", layers(3));
  watch.until_reloaded(&mut maps, map);

  assert_eq!(layers_of(&mut maps.world, map), found);
  assert_eq!(id_of(&maps.world, map, "1"), Some(a));
  assert_eq!(
    maps.world.get::<ChildOf>(a).map(ChildOf::parent),
    Some(found[1])
  );
  assert_eq!(maps.world.get::<Marker>(a), Some(&Marker(2)));
}

#[test]
fn objects_reconcile_by_id() {
  let mut world = World::new();
  let [e1, e2, e3] = [(); 3].map(|_| world.spawn_empty().id());
  let data = |id: &str| MapObjectData {
    id: id.to_string(),
    name: String::new(),
    class: None,
    position: Vec2::ZERO,
    size: Vec2::ZERO,
    rotation: 0.0,
    shape: crate::core::tilemap::ObjectShape::Point,
    tile: None,
    visible: true,
    properties: crate::core::tilemap::Properties::default(),
  };
  let existing = [
    ("1".to_string(), e1),
    ("2".to_string(), e2),
    ("3".to_string(), e3),
  ];
  // "5" has no live entity (the game despawned it) and the file left it
  let new = [data("1"), data("3"), data("4"), data("5")];
  let matched = reconcile(&existing, &new, |i| i == 0 || i == 3);
  assert_eq!(
    matched,
    Reconcile {
      keep: vec![e1],
      update: vec![(e3, 1)],
      spawn: vec![2],
      despawn: vec![e2],
    }
  );
}

#[test]
fn class_tiles_reconcile_by_layer_and_cell() {
  // Gid 6 is tile 5, a coin
  let first = [0, 6, 6, 0, 0, 0, 0, 0, 1, 1, 1, 1];
  let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &small(first, &[]))]);
  let (mut maps, map) = spawned(&files, "r/m.tmj", |h| {
    MapRoot::new(h).with_entity_tiles(&["coin"])
  });
  let coins = |world: &mut World| -> Vec<(IVec2, Entity)> {
    let mut found: Vec<(IVec2, Entity)> = world
      .query::<(Entity, &MapTile)>()
      .iter(world)
      .filter(|(_, t)| t.map == map)
      .map(|(e, t)| (t.cell, e))
      .collect();
    found.sort_by_key(|(cell, _)| (cell.y, cell.x));
    found
  };
  let before = coins(&mut maps.world);
  assert_eq!(before.len(), 2);
  let (kept, gone) = (before[0].1, before[1].1);
  maps.world.entity_mut(kept).insert(Marker(1));

  let mut watch = Watch::new(&maps.world);
  let saved = [0, 6, 1, 0, 0, 0, 0, 6, 1, 1, 1, 1];
  files.insert("r/m.tmj", small(saved, &[]));
  watch.until_reloaded(&mut maps, map);

  let after = coins(&mut maps.world);
  let cells_now: Vec<IVec2> = after.iter().map(|(cell, _)| *cell).collect();
  assert_eq!(cells_now, [IVec2::new(1, 0), IVec2::new(3, 1)]);
  assert_eq!(after[0].1, kept, "the coin the save left keeps its entity");
  assert_eq!(maps.world.get::<Marker>(kept), Some(&Marker(1)));
  assert!(maps.world.get_entity(gone).is_err());
  // Class tiles stay out of the grid
  let ground = layers_of(&mut maps.world, map)[0];
  assert_eq!(
    grid_of(&maps.world, ground),
    cells(&[0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1])
  );

  // The game collects the new coin and marks the kept one's tile taken; a
  // save of another cell leaves both as the game has them
  let collected = after[1].1;
  maps.world.despawn(collected);
  if let Some(mut tile) = maps.world.get_mut::<MapTile>(kept) {
    tile
      .properties
      .insert("taken", crate::core::tilemap::PropertyValue::Bool(true));
  }
  let elsewhere = [0, 6, 1, 0, 0, 0, 0, 6, 1, 1, 1, 0];
  let mut watch = Watch::new(&maps.world);
  files.insert("r/m.tmj", small(elsewhere, &[]));
  watch.until_reloaded(&mut maps, map);
  let now = coins(&mut maps.world);
  assert_eq!(
    now,
    [(IVec2::new(1, 0), kept)],
    "the collected coin stays away"
  );
  let taken = maps
    .world
    .get::<MapTile>(kept)
    .map(|t| t.properties.clone());
  assert!(taken.is_some_and(|p| p.get("taken").is_some()));
  assert_eq!(
    grid_of(&maps.world, ground),
    cells(&[0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0])
  );

  // A save that changes the collected coin's cell (the coin flipped) brings
  // it back
  let flipped = 6 | 0x8000_0000;
  let changed = [0, 6, 1, 0, 0, 0, 0, flipped, 1, 1, 1, 0];
  let mut watch = Watch::new(&maps.world);
  files.insert("r/m.tmj", small(changed, &[]));
  watch.until_reloaded(&mut maps, map);
  let now = coins(&mut maps.world);
  assert_eq!(now.len(), 2);
  assert_eq!(now[0], (IVec2::new(1, 0), kept));
  assert_eq!(now[1].0, IVec2::new(3, 1));
  let back = maps.world.get::<MapTile>(now[1].1).map(|t| t.tile.flip);
  assert_eq!(back, Some(crate::core::tilemap::TileFlip::HORIZONTAL));
}

#[test]
fn a_tile_layer_spawned_afresh_keeps_away_the_class_tiles_the_game_took() {
  let level = |offset: &str| {
    let ground = [6, 6, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1];
    map_of((4, 3), &[tiles(1, "ground", (4, 3), &ground, offset)])
  };
  let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &level(""))]);
  let (mut maps, map) = spawned(&files, "r/m.tmj", |h| {
    MapRoot::new(h).with_entity_tiles(&["coin"])
  });
  let coins = |world: &mut World| -> Vec<IVec2> {
    let mut found: Vec<IVec2> = world
      .query::<&MapTile>()
      .iter(world)
      .filter(|t| t.map == map)
      .map(|t| t.cell)
      .collect();
    found.sort_by_key(|cell| (cell.y, cell.x));
    found
  };
  let collected = maps
    .world
    .query::<(Entity, &MapTile)>()
    .iter(&maps.world)
    .find(|(_, t)| t.cell == IVec2::new(1, 0))
    .map(|(e, _)| e)
    .expect("a coin at (1, 0)");
  maps.world.despawn(collected);

  // The layer moves (a new entity); its coins come with it, but not the
  // one collected
  let mut watch = Watch::new(&maps.world);
  files.insert("r/m.tmj", level(r#","offsetx":8"#));
  watch.until_reloaded(&mut maps, map);
  let ground = layers_of(&mut maps.world, map)[0];
  assert_eq!(translation(&maps.world, ground), Vec2::new(8.0, 0.0));
  assert_eq!(coins(&mut maps.world), [IVec2::new(0, 0)]);
}

#[test]
fn kept_object_and_image_layers_move_with_a_saved_offset() {
  let level = |objects_at: (u32, u32), sky_at: (u32, u32)| {
    let things = things(2, &[object(1, "a", (16, 0), "")]).replace(
      r#""type":"objectgroup""#,
      &format!(
        r#""type":"objectgroup","offsetx":{},"offsety":{}"#,
        objects_at.0, objects_at.1
      ),
    );
    let sky = format!(
      r#"{{"id":4,"name":"sky","type":"imagelayer","image":"../tiled/images/bg.png",
        "offsetx":{},"offsety":{}}}"#,
      sky_at.0, sky_at.1
    );
    map_of((4, 3), &[things, sky])
  };
  let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &level((0, 0), (0, 0)))]);
  let (mut maps, map) = spawned(&files, "r/m.tmj", MapRoot::new);
  let before = layers_of(&mut maps.world, map);
  let (a, _) = object_named(&mut maps.world, map, "a");
  maps.world.entity_mut(a).insert(Marker(6));

  // A Tiled group holding both moved: its offset lands on each layer
  let mut watch = Watch::new(&maps.world);
  files.insert("r/m.tmj", level((40, 8), (12, 2)));
  watch.until_reloaded(&mut maps, map);

  assert_eq!(layers_of(&mut maps.world, map), before, "both kept");
  assert_eq!(
    (
      translation(&maps.world, before[0]),
      translation(&maps.world, before[1])
    ),
    (Vec2::new(40.0, 8.0), Vec2::new(12.0, 2.0)),
    "the object layer and the image layer"
  );
  // The object moves with its layer, keeping its entity and place in it
  assert_eq!(id_of(&maps.world, map, "1"), Some(a));
  assert_eq!(translation(&maps.world, a), Vec2::new(24.0, 8.0));
  assert_eq!(maps.world.get::<Marker>(a), Some(&Marker(6)));
}

#[test]
fn layers_new_removed_moved_or_reshaped_respawn_and_the_rest_keep_their_entities() {
  let row = |gid: u32| [gid; 12];
  let first = map_of(
    (4, 3),
    &[
      tiles(1, "ground", (4, 3), &row(1), ""),
      things(2, &[object(1, "a", (0, 0), "")]),
      tiles(3, "top", (4, 3), &row(2), ""),
    ],
  );
  let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &first)]);
  let (mut maps, map) = spawned(&files, "r/m.tmj", MapRoot::new);
  let before = layers_of(&mut maps.world, map);
  let (a, _) = object_named(&mut maps.world, map, "a");

  // "top" goes below "ground" and gets an offset, "things" goes, "sky" comes
  let mut watch = Watch::new(&maps.world);
  let saved = map_of(
    (4, 3),
    &[
      tiles(3, "top", (4, 3), &row(2), r#","offsetx":4"#),
      tiles(1, "ground", (4, 3), &row(3), ""),
      tiles(5, "sky", (4, 3), &row(4), ""),
    ],
  );
  files.insert("r/m.tmj", saved);
  watch.until_reloaded(&mut maps, map);

  let after = layers_of(&mut maps.world, map);
  assert_eq!(after.len(), 3);
  let names: Vec<String> = after
    .iter()
    .filter_map(|&l| maps.world.get::<MapLayerRef>(l).map(|r| r.name.clone()))
    .collect();
  assert_eq!(names, ["top", "ground", "sky"]);
  assert_eq!(after[1], before[0], "ground is kept, now second");
  assert_eq!(grid_of(&maps.world, after[1]), cells(&row(3)));
  assert_ne!(after[0], before[2], "top moved: a new entity");
  assert_eq!(translation(&maps.world, after[0]), Vec2::new(4.0, 0.0));
  assert!(maps.world.get_entity(before[1]).is_err(), "things went");
  assert!(maps.world.get_entity(before[2]).is_err());
  assert!(maps.world.get_entity(a).is_err(), "with its object");
  assert_eq!(grid_of(&maps.world, after[2]), cells(&row(4)));
  assert!(MapsView::new(&mut maps.world).is_ready(map));
  assert_eq!(
    MapsView::new(&mut maps.world).tile(map, "sky", IVec2::ZERO),
    Some(Tile::new(0, 3))
  );
}

#[test]
fn a_broken_save_keeps_the_last_good_map() {
  let objects = [object(1, "a", (16, 0), "")];
  let first = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1];
  let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &small(first, &objects))]);
  let (mut maps, map) = spawned(&files, "r/m.tmj", MapRoot::new);
  let ground = layers_of(&mut maps.world, map)[0];
  let (a, _) = object_named(&mut maps.world, map, "a");
  let mut watch = Watch::new(&maps.world);
  let mut assets = maps
    .world
    .resource::<Messages<AssetEvent<TileMap>>>()
    .get_cursor_current();

  files.insert("r/m.tmj", "{ nope");
  maps.frames_until(|world| {
    let messages = world.resource::<Messages<AssetEvent<TileMap>>>();
    assets
      .read(messages)
      .any(|e| matches!(e, AssetEvent::ReloadFailed { .. }))
  });
  for _ in 0..3 {
    maps.frame();
  }
  watch.read(&maps.world);
  assert!(watch.sent.is_empty(), "{:?}", watch.sent);
  assert_eq!(grid_of(&maps.world, ground), cells(&first));
  assert_eq!(id_of(&maps.world, map, "1"), Some(a));

  // Saved again, fixed
  let saved = [1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1];
  files.insert("r/m.tmj", small(saved, &objects));
  watch.until_reloaded(&mut maps, map);
  assert_eq!(grid_of(&maps.world, ground), cells(&saved));
  assert_eq!(id_of(&maps.world, map, "1"), Some(a));
}

#[test]
fn saving_the_tiled_project_reapplies_class_defaults() {
  let project = |hp: i32| {
    format!(
      r#"{{"propertyTypes":[{{"name":"Crate","type":"class","members":[{{"name":"hp","type":"int","value":{hp}}}]}}]}}"#
    )
  };
  let crate_object = [object(1, "crate", (16, 0), r#","type":"Crate""#)];
  let files = files_with(&[
    ("project.tiled-project", &project(1)),
    ("r/set.tsj", SET),
    ("r/m.tmj", &small([0; 12], &crate_object)),
  ]);
  let (mut maps, map) = spawned(&files, "r/m.tmj", MapRoot::new);
  let (entity, object) = object_named(&mut maps.world, map, "crate");
  assert_eq!(object.properties.int("hp"), Some(1));
  maps.world.entity_mut(entity).insert(Marker(3));

  let mut watch = Watch::new(&maps.world);
  files.insert("project.tiled-project", project(2));
  watch.until_reloaded(&mut maps, map);

  let (now, object) = object_named(&mut maps.world, map, "crate");
  assert_eq!(now, entity);
  assert_eq!(object.properties.int("hp"), Some(2));
  assert_eq!(maps.world.get::<Marker>(entity), Some(&Marker(3)));
}

#[test]
fn saving_a_template_changes_its_instances() {
  let template = |hp: i32, width: u32| {
    format!(
      r#"{{"type":"template","object":{{"id":1,"name":"t","width":{width},"height":16,
        "properties":[{{"name":"hp","type":"int","value":{hp}}}]}}}}"#
    )
  };
  let instance = [r#"{"id":1,"template":"t.tj","x":16,"y":0}"#.to_string()];
  let files = files_with(&[
    ("r/t.tj", &template(1, 16)),
    ("r/set.tsj", SET),
    ("r/m.tmj", &small([0; 12], &instance)),
  ]);
  let (mut maps, map) = spawned(&files, "r/m.tmj", MapRoot::new);
  let (entity, object) = object_named(&mut maps.world, map, "t");
  assert_eq!(object.properties.int("hp"), Some(1));
  maps.world.entity_mut(entity).insert(Marker(4));

  let mut watch = Watch::new(&maps.world);
  files.insert("r/t.tj", template(2, 32));
  watch.until_reloaded(&mut maps, map);

  let (now, object) = object_named(&mut maps.world, map, "t");
  assert_eq!(now, entity);
  assert_eq!(object.properties.int("hp"), Some(2));
  assert_eq!(object.size, Vec2::new(32.0, 16.0));
  assert_eq!(translation(&maps.world, entity), Vec2::new(32.0, 8.0));
  assert_eq!(maps.world.get::<Marker>(entity), Some(&Marker(4)));
}

#[test]
fn saving_an_ldtk_level_file_reloads_its_project() {
  const LEVEL: &str = "ldtk/external/Level_0.ldtkl";
  const MARKED: &str = "11111111-0000-4000-8000-00000000d00a";
  let text = std::fs::read_to_string(format!(
    "{}/{LEVEL}",
    crate::core::tilemap::test_util::FIXTURES
  ))
  .expect("the fixture level");
  let files = files_with(&[]);
  let mut maps = MapWorld::over(files.clone());
  let project = AssetsView::new(&mut maps.world)
    .load_now::<LdtkProject>("ldtk/external.ldtk")
    .expect("external.ldtk loads");
  let root = MapRoot::ldtk(project, "Level_0").with_intgrid_collision("Wall", TileCell::Solid);
  let map = maps.spawn(root);
  let entity = id_of(&maps.world, map, MARKED).expect("the entity");
  assert_eq!(translation(&maps.world, entity), Vec2::new(24.0, 24.0));
  maps.world.entity_mut(entity).insert(Marker(5));
  let cell = IVec2::new(1, 1);
  assert_eq!(
    MapsView::new(&mut maps.world).intgrid(map, "Collision", cell),
    None
  );
  let solid_at = |world: &World| world.get::<TileColliders>(map).map(|t| t.get(1, 1));
  assert_eq!(solid_at(&maps.world), Some(TileCell::Empty));

  // The level file: an IntGrid value painted, the entity moved
  let mut level: serde_json::Value = serde_json::from_str(&text).expect("json");
  let Some(layers) = level["layerInstances"].as_array_mut() else {
    panic!("no layers")
  };
  for layer in layers {
    if layer["__identifier"] == "Collision" {
      layer["intGridCsv"][5] = serde_json::json!(1);
    }
    if let Some(entities) = layer["entityInstances"].as_array_mut() {
      for e in entities.iter_mut().filter(|e| e["iid"] == MARKED) {
        e["px"] = serde_json::json!([40, 24]);
      }
    }
  }
  let mut watch = Watch::new(&maps.world);
  files.insert(LEVEL, level.to_string());
  watch.until_reloaded(&mut maps, map);

  assert_eq!(
    MapsView::new(&mut maps.world).intgrid(map, "Collision", cell),
    Some(1)
  );
  // Its auto-tiles did not change, and the value collides all the same
  assert_eq!(solid_at(&maps.world), Some(TileCell::Solid));
  assert_eq!(id_of(&maps.world, map, MARKED), Some(entity));
  assert_eq!(translation(&maps.world, entity), Vec2::new(40.0, 24.0));
  assert_eq!(maps.world.get::<Marker>(entity), Some(&Marker(5)));
}

#[test]
fn a_disabled_map_reloads_and_is_current_once_enabled() {
  let first = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1];
  let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &small(first, &[]))]);
  let (mut maps, map) = spawned(&files, "r/m.tmj", MapRoot::new);
  let ground = layers_of(&mut maps.world, map)[0];
  maps.world.entity_mut(map).insert(Disabled);
  maps.frame();
  assert!(maps.world.get::<TileColliders>(map).is_none());

  let mut watch = Watch::new(&maps.world);
  let saved = [1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1];
  files.insert("r/m.tmj", small(saved, &[]));
  watch.until_reloaded(&mut maps, map);
  assert_eq!(grid_of(&maps.world, ground), cells(&saved));
  assert!(
    maps.world.get::<TileColliders>(map).is_none(),
    "still disabled"
  );

  maps.world.entity_mut(map).remove::<Disabled>();
  maps.frame();
  maps.frame();
  let tiles = maps.world.get::<TileColliders>(map).expect("colliders");
  assert_eq!(tiles.get(0, 0), TileCell::Solid);
}

/// The parts of the map's colliders: each one's entity, physics handle and
/// shape
fn parts(world: &mut World, map: Entity) -> Vec<(Entity, String, String)> {
  let mut found: Vec<(Entity, String, String)> = world
    .query::<(Entity, &ChildOf, &Collider)>()
    .iter(world)
    .filter(|(_, child_of, _)| child_of.parent() == map)
    .map(|(e, _, c)| {
      let handle = world
        .resource::<Physics>()
        .collider_entries
        .get(&e)
        .map(|entry| entry.handle);
      (e, format!("{handle:?}"), format!("{:?}", c.shape))
    })
    .collect();
  found.sort_by_key(|(e, ..)| *e);
  found
}

#[test]
fn a_reload_keeps_the_ground_a_character_stands_on() {
  // Three chunks wide: a floor on row 5, and a block in the last chunk
  let (width, height) = (96, 6);
  let level = |block: u32| {
    let mut data = vec![0u32; (width * height) as usize];
    for x in 0..width {
      data[(5 * width + x) as usize] = 1;
    }
    data[(2 * width + block) as usize] = 1;
    map_of(
      (width, height),
      &[tiles(1, "ground", (width, height), &data, "")],
    )
  };
  let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &level(80))]);
  let (mut maps, map) = spawned(&files, "r/m.tmj", MapRoot::new);
  let character = maps
    .world
    .spawn((
      Transform::from_xy(40.0, 50.0),
      CharacterController::platformer(),
      Collider::rect(12.0, 40.0),
    ))
    .id();
  let fall = |world: &mut World| {
    if let Some(mut v) = world.get_mut::<Velocity>(character) {
      v.dy += 980.0 / 60.0;
    }
  };
  for _ in 0..40 {
    fall(&mut maps.world);
    maps.frame();
  }
  let state = maps.world.get::<CharacterState>(character).cloned();
  let floor = state.as_ref().and_then(|s| s.floor).expect("on the floor");
  let before = parts(&mut maps.world, map);
  let ground = layers_of(&mut maps.world, map)[0];
  let chunk_versions = |world: &World| {
    let grid = &world.get::<TileLayerData>(ground).expect("tiles").grid;
    [0, 1, 2].map(|x| grid.chunk_version(UVec2::new(x, 0)))
  };
  let shadows = |world: &mut World, chunk: u32| {
    let mut found: Vec<Entity> = world
      .query::<(Entity, &MapOccluder)>()
      .iter(world)
      .filter(|(_, o)| o.chunk == UVec2::new(chunk, 0))
      .map(|(e, _)| e)
      .collect();
    found.sort();
    found
  };
  let versions = chunk_versions(&maps.world);
  let lit: Vec<Vec<Entity>> = (0..3).map(|c| shadows(&mut maps.world, c)).collect();
  assert!(lit.iter().all(|chunk| !chunk.is_empty()));
  let mut stopped = maps
    .world
    .resource::<Messages<CollisionStopped>>()
    .get_cursor_current();

  // The block in the last chunk moves over by a cell
  let mut watch = Watch::new(&maps.world);
  files.insert("r/m.tmj", level(81));
  maps.frames_until(|world| {
    fall(world);
    let state = world.get::<CharacterState>(character);
    assert!(
      state.is_some_and(|s| s.on_floor && s.floor == Some(floor)),
      "{state:?}"
    );
    watch.read(world);
    watch.reloaded().contains(&map)
  });
  for _ in 0..3 {
    fall(&mut maps.world);
    maps.frame();
    let state = maps.world.get::<CharacterState>(character);
    assert!(state.is_some_and(|s| s.on_floor && s.floor == Some(floor)));
  }
  let tiles = maps.world.get::<TileColliders>(map).expect("colliders");
  assert_eq!(
    (tiles.get(80, 2), tiles.get(81, 2)),
    (TileCell::Empty, TileCell::Solid)
  );
  // Every part kept its entity and handle; only the last chunk's changed shape
  let after = parts(&mut maps.world, map);
  assert_eq!(after.len(), before.len());
  let mut reshaped = 0;
  for ((e, handle, shape), (now, now_handle, now_shape)) in before.iter().zip(&after) {
    assert_eq!((e, handle), (now, now_handle));
    reshaped += usize::from(shape != now_shape);
  }
  assert_eq!(reshaped, 1, "only the block's chunk");
  // Only that chunk took a new version, and only its shadows were built again
  let now = chunk_versions(&maps.world);
  assert_eq!((now[0], now[1]), (versions[0], versions[1]));
  assert!(now[2] > versions[2]);
  assert_eq!(shadows(&mut maps.world, 0), lit[0]);
  assert_eq!(shadows(&mut maps.world, 1), lit[1]);
  assert_ne!(shadows(&mut maps.world, 2), lit[2]);
  let messages = maps.world.resource::<Messages<CollisionStopped>>();
  let ended: Vec<_> = stopped
    .read(messages)
    .filter(|m| m.involves(character))
    .collect();
  assert!(ended.is_empty(), "{ended:?}");
}

#[test]
fn a_save_while_a_map_lands_lands_the_saved_map_once() {
  // Four full 256 x 256 layers: its landing is still out when the frame
  // that sent it ends
  let side = 256;
  let level = |first: u32| {
    let mut data = vec![1u32; (side * side) as usize];
    data[0] = first;
    let layers: Vec<String> = (1..=4)
      .map(|id| tiles(id, &format!("layer {id}"), (side, side), &data, ""))
      .collect();
    map_of((side, side), &layers)
  };
  let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &level(0))]);
  let mut maps = MapWorld::over(files.clone());
  let handle = load_map(&mut maps.world, "r/m.tmj");
  let map = maps.world.spawn(MapRoot::new(handle.clone())).id();
  let mut watch = Watch::new(&maps.world);
  // Its landing goes out
  maps.pre_update();
  assert!(!maps_landed(&mut maps.world), "the landing is out");

  // The save lands before the landing comes back
  files.insert("r/m.tmj", level(2));
  let start = Instant::now();
  loop {
    pump(&mut maps.world);
    let value = maps.world.resource::<Assets<TileMap>>().get(&handle);
    let first_cell = value
      .and_then(|m| m.tiles(0))
      .and_then(|g| g.get(IVec2::ZERO));
    if first_cell.is_some() {
      break;
    }
    assert!(
      start.elapsed() < Duration::from_secs(5),
      "the save never landed"
    );
    std::thread::sleep(Duration::from_millis(1));
  }
  maps.settle(map);
  for _ in 0..3 {
    maps.frame();
  }
  watch.read(&maps.world);
  for layer in 0..4 {
    let tile = MapsView::new(&mut maps.world).tile(map, layer, IVec2::ZERO);
    assert_eq!(tile, Some(Tile::new(0, 1)), "layer {layer}");
  }
  assert_eq!(watch.spawned(), [map], "{:?}", watch.sent);
  assert!(watch.reloaded().is_empty(), "{:?}", watch.sent);
}

/// The maps at `maps` spawned with their top-left corners at the places
/// given
struct TwoMaps(Vec<(&'static str, Vec2)>);

impl Scene for TwoMaps {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    for (path, at) in &self.0 {
      let map = match ctx.assets().load_now::<TileMap>(path) {
        Ok(map) => map,
        Err(error) => panic!("{path}: {}", error.report()),
      };
      ctx
        .world
        .spawn((MapRoot::new(map), Transform::from_translation(*at)));
    }
  }
}

/// A PNG of `size` whose pixels `colour` gives
fn png(size: UVec2, colour: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
  let pixels = image::RgbaImage::from_fn(size.x, size.y, |x, y| image::Rgba(colour(x, y)));
  let mut bytes = std::io::Cursor::new(Vec::new());
  if let Err(error) = pixels.write_to(&mut bytes, image::ImageFormat::Png) {
    panic!("{error}");
  }
  bytes.into_inner()
}

/// Steps `engine` until `done` holds of its frame, failing after 5 s; the
/// map events of every frame go to `watch` (the engine keeps them for two
/// frames)
fn step_until(
  engine: &mut HeadlessEngine,
  watch: &mut Watch,
  what: &str,
  done: impl Fn(&image::RgbaImage) -> bool,
) {
  let start = Instant::now();
  loop {
    engine.step().expect("a frame");
    watch.read(engine.world());
    let frame = engine.frame().expect("the frame");
    if done(&frame) {
      return;
    }
    if start.elapsed() > Duration::from_secs(5) {
      let (w, h) = frame.dimensions();
      let samples: Vec<_> = [(w / 4, h / 2), (3 * w / 4, h / 2), (8, 8), (w - 8, h - 8)]
        .iter()
        .map(|&(x, y)| ((x, y), frame.get_pixel(x, y).0))
        .collect();
      panic!("never {what}: {w} x {h}, {samples:?}");
    }
    std::thread::sleep(Duration::from_millis(1));
  }
}

/// Every map root in the world
fn roots(world: &mut World) -> Vec<Entity> {
  let mut found: Vec<Entity> = world
    .query::<(Entity, &MapRoot)>()
    .iter(world)
    .map(|(e, _)| e)
    .collect();
  found.sort();
  found
}

#[test]
fn saving_a_tileset_or_its_image_redraws_every_map_using_it() {
  const RED: [u8; 4] = [220, 30, 40, 255];
  const BLUE: [u8; 4] = [30, 60, 230, 255];
  const GREEN: [u8; 4] = [40, 200, 60, 255];
  let solid = |colour: [u8; 4]| png(UVec2::splat(16), move |_, _| colour);
  let set = |image: &str, label: &str| {
    format!(
      r#"{{"type":"tileset","name":"set","tilewidth":16,"tileheight":16,"tilecount":1,
        "columns":1,"image":"{image}","imagewidth":16,"imageheight":16,
        "tiles":[{{"id":0,"properties":[{{"name":"label","type":"string","value":"{label}"}}],
          "objectgroup":{{"type":"objectgroup","objects":[{{"id":1,"x":0,"y":0,"width":16,"height":16}}]}}}}]}}"#
    )
  };
  let map = map_of((2, 2), &[tiles(1, "ground", (2, 2), &[1; 4], "")]);
  let files = files_with(&[
    ("r/set.tsj", &set("a.png", "one")),
    ("r/one.tmj", &map),
    ("r/two.tmj", &map),
  ]);
  files.insert("r/a.png", solid(RED));
  files.insert("r/b.png", solid(GREEN));
  let builder = EngineBuilder::new()
    .resolution(64, 32)
    .asset_source(files.clone())
    .hot_reload(true)
    .start_scene(TwoMaps(vec![
      ("r/one.tmj", Vec2::ZERO),
      ("r/two.tmj", Vec2::new(32.0, 0.0)),
    ]));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  let both = |frame: &image::RgbaImage, colour: [u8; 4]| {
    frame.get_pixel(16, 16).0 == colour && frame.get_pixel(48, 16).0 == colour
  };
  let mut watch = Watch::new(engine.world());
  step_until(&mut engine, &mut watch, "drew red", |f| both(f, RED));
  let maps = roots(engine.world_mut());
  assert_eq!(maps.len(), 2);
  let colliders: Vec<_> = maps
    .iter()
    .map(|&map| parts(engine.world_mut(), map))
    .collect();
  assert!(colliders.iter().all(|parts| !parts.is_empty()));
  let shadows = |world: &mut World| {
    let mut found: Vec<Entity> = world
      .query::<(Entity, &MapOccluder)>()
      .iter(world)
      .map(|(e, _)| e)
      .collect();
    found.sort();
    found
  };
  let lit = shadows(engine.world_mut());
  assert!(!lit.is_empty());
  watch.sent.clear();

  // The image saved: both maps draw it, and nothing else is built again
  files.insert("r/a.png", solid(BLUE));
  step_until(&mut engine, &mut watch, "drew blue", |f| both(f, BLUE));
  for _ in 0..3 {
    engine.step().expect("a frame");
    watch.read(engine.world());
  }
  assert!(
    watch.sent.is_empty(),
    "an image redraws only: {:?}",
    watch.sent
  );
  for (map, parts_before) in maps.iter().zip(&colliders) {
    assert_eq!(&parts(engine.world_mut(), *map), parts_before);
  }
  assert_eq!(shadows(engine.world_mut()), lit);

  // The tileset saved, naming another image: both maps follow it
  files.insert("r/set.tsj", set("b.png", "two"));
  step_until(&mut engine, &mut watch, "drew green", |f| both(f, GREEN));
  for _ in 0..2 {
    engine.step().expect("a frame");
    watch.read(engine.world());
  }
  let mut reloaded = watch.reloaded();
  reloaded.sort();
  assert_eq!(reloaded, maps);
}

/// A tileset of `count` 32 px tiles in rows of 8, each a colour of its own
fn many(count: u32) -> (String, Vec<u8>) {
  let rows = count.div_ceil(8);
  let size = UVec2::new(8 * 32, rows * 32);
  let image = png(size, |x, y| colour_of((y / 32) * 8 + x / 32));
  let set = format!(
    r#"{{"type":"tileset","name":"many","tilewidth":32,"tileheight":32,"tilecount":{count},
      "columns":8,"image":"many.png","imagewidth":{},"imageheight":{}}}"#,
    size.x, size.y
  );
  (set, image)
}

fn colour_of(tile: u32) -> [u8; 4] {
  [
    (tile * 3) as u8,
    255 - (tile * 3) as u8,
    64 + (tile % 7) as u8 * 20,
    255,
  ]
}

#[test]
fn a_tileset_padded_again_never_draws_its_old_image_with_new_places() {
  // 100 tiles of 32 px pad to 752 px, a texture of its own and over a
  // frame's upload budget; 130 pad to more columns, so every tile's place
  // in the padded image moves
  let (set, image) = many(100);
  let data: Vec<u32> = (1..=32).collect();
  let layer = tiles(1, "ground", (8, 4), &data, "");
  let map = format!(
    r#"{{"type":"map","orientation":"orthogonal","width":8,"height":4,"tilewidth":32,
      "tileheight":32,"tilesets":[{{"firstgid":1,"source":"many.tsj"}}],"layers":[{layer}]}}"#
  );
  let files = files_with(&[("r/many.tsj", &set), ("r/m.tmj", &map)]);
  files.insert("r/many.png", image);
  let builder = EngineBuilder::new()
    .resolution(256, 128)
    .asset_source(files.clone())
    .hot_reload(true)
    .start_scene(TwoMaps(vec![("r/m.tmj", Vec2::ZERO)]));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  // Each cell's centre shows its tile's colour
  let wrong = |frame: &image::RgbaImage| -> Option<String> {
    (0..32u32).find_map(|tile| {
      let (x, y) = ((tile % 8) * 32 + 16, (tile / 8) * 32 + 16);
      let seen = frame.get_pixel(x, y).0;
      (seen != colour_of(tile)).then(|| format!("tile {tile} at ({x}, {y}) shows {seen:?}"))
    })
  };
  let mut watch = Watch::new(engine.world());
  step_until(&mut engine, &mut watch, "drew the tiles", |f| {
    wrong(f).is_none()
  });

  // The image saved first (a tileset saved before its larger image fails
  // its check and keeps its last good value), then the tileset. Until each
  // reload lands every frame shows the old padding, and from then on the
  // new one: no frame may mix them
  let (set, image) = many(130);
  let picture = engine
    .assets()
    .load::<crate::core::graphics::Image>("r/many.png");
  files.insert("r/many.png", image);
  let mut frames = 0;
  let mut saved = false;
  let start = Instant::now();
  while start.elapsed() < Duration::from_millis(2000) {
    engine.step().expect("a frame");
    let frame = engine.frame().expect("the frame");
    if let Some(what) = wrong(&frame) {
      panic!("frame {frames} after the save: {what}");
    }
    frames += 1;
    let height = engine.assets().get(&picture).map(|image| image.height());
    if !saved && height == Some(17 * 32) {
      files.insert("r/many.tsj", set.clone());
      saved = true;
    }
  }
  let tilesets = engine
    .world()
    .resource::<Assets<crate::core::tilemap::Tileset>>();
  let counts: Vec<u32> = tilesets.iter().map(|(_, t)| t.tile_count).collect();
  assert!(counts.contains(&130), "the save landed: {counts:?}");
}

/// The reload of a 1024 x 1024 map of four layers, one tile changed: how
/// long the frame that applies it takes on the main thread
#[test]
#[ignore = "timing: run with --release --ignored --nocapture"]
fn reload_timing() {
  let side = 1024u32;
  let mut map = TileMap::new(UVec2::splat(side), UVec2::splat(16));
  let files = files_with(&[("r/set.tsj", SET)]);
  let mut maps = MapWorld::over(files);
  let tileset = AssetsView::new(&mut maps.world)
    .load_now::<crate::core::tilemap::Tileset>("r/set.tsj")
    .expect("set.tsj loads");
  map.add_tileset(crate::core::tilemap::TilesetSource::Asset(tileset));
  for name in ["back", "ground", "front", "top"] {
    let layer = map.add_tile_layer(name);
    if let Some(grid) = map.tiles_mut(layer) {
      for y in 0..side as i32 {
        for x in 0..side as i32 {
          // Terrain: solid below a wavy line, a stripe of tile 1 above
          let ground = 600 + ((x as f32 / 40.0).sin() * 30.0) as i32;
          let tile = if y > ground {
            Some(Tile::new(0, 0))
          } else if y % 50 == 0 {
            Some(Tile::new(0, 1))
          } else {
            None
          };
          grid.set(IVec2::new(x, y), tile);
        }
      }
    }
  }
  let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
  let root = maps.spawn(MapRoot::new(handle.clone()));
  for _ in 0..120 {
    maps.frame();
  }
  let mut times = Vec::new();
  for round in 0..10 {
    if let Some(map) = maps
      .world
      .resource_mut::<Assets<TileMap>>()
      .get_mut(&handle)
      && let Some(grid) = map.tiles_mut(1)
    {
      let cell = IVec2::new(500 + round, 100);
      let tile = grid.get(cell);
      grid.set(
        cell,
        if tile.is_some() {
          None
        } else {
          Some(Tile::new(0, 0))
        },
      );
    }
    pump(&mut maps.world);
    let start = Instant::now();
    apply_reload(&mut maps.world, root);
    let applied = start.elapsed();
    let start = Instant::now();
    maps.step();
    maps.post_update();
    let rest = start.elapsed();
    times.push((applied, rest));
  }
  for (applied, rest) in &times {
    println!("one tile: apply {applied:?}, then the step and PostUpdate {rest:?}");
  }
  // A save that changes every chunk of every layer: the terrain becomes
  // columns, one cell wide every eight
  if let Some(map) = maps
    .world
    .resource_mut::<Assets<TileMap>>()
    .get_mut(&handle)
  {
    for layer in 0..4 {
      if let Some(grid) = map.tiles_mut(layer) {
        for y in 0..side as i32 {
          for x in 0..side as i32 {
            let tile = (x % 8 == 0 && y > 100).then(|| Tile::new(0, 0));
            grid.set(IVec2::new(x, y), tile);
          }
        }
      }
    }
  }
  pump(&mut maps.world);
  let start = Instant::now();
  apply_reload(&mut maps.world, root);
  let applied = start.elapsed();
  let start = Instant::now();
  maps.step();
  maps.post_update();
  let rest = start.elapsed();
  println!("every chunk: apply {applied:?}, then the step and PostUpdate {rest:?}");
}
