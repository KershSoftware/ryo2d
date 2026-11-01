//! A streamed map's chunks generated around anchors on a thread of their
//! own, landed nearest first within a budget a frame and unloaded far
//! behind, with their messages, the edits refused outside them, a
//! generator that panics, the chunks a player changed saved and brought
//! back, and the entities that go with their chunk. Each test keeps to a
//! few chunks of 16 px cells.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use glam::{IVec2, UVec2, Vec2};

use super::{
  ChunkEvent, ChunkOwned, ChunkRequest, ChunkTiles, INLINE, MapStream, SPAN_CAP, SavedChunks,
  StreamAnchor, StreamState,
};
use crate::core::asset::Assets;
use crate::core::ecs::kernel::{ChildOf, Disabled, MessageCursor, Messages};
use crate::core::ecs::{Entity, Transform, World};
use crate::core::tilemap::test_util::{MapWorld, layers_of, load_map};
use crate::core::tilemap::{
  IntGrid, MapLayerRef, MapRoot, MapTile, MapsView, Tile, TileData, TileFlip, TileLayerData,
  TileMap, TileShape, Tileset, TilesetImage, TilesetSource,
};
use crate::core::ui::Rect;

/// A cell's side in pixels, and a chunk's
const CELL: f32 = 16.0;
const CHUNK: f32 = 32.0 * CELL;

/// A solid block, and a coin (a tile of the class "coin")
const BLOCK: Tile = Tile {
  tileset: 0,
  id: 0,
  flip: TileFlip::NONE,
};
const COIN: Tile = Tile {
  tileset: 0,
  id: 1,
  flip: TileFlip::NONE,
};

/// A collection tileset of 16 px tiles: 0 a solid block, 1 a coin
fn tiles() -> Tileset {
  let mut set = Tileset::new("stream", UVec2::splat(16), TilesetImage::Collection);
  set.tile_count = 2;
  set.insert_tile(
    0,
    TileData {
      shapes: vec![TileShape {
        points: vec![
          Vec2::ZERO,
          Vec2::new(16.0, 0.0),
          Vec2::new(16.0, 16.0),
          Vec2::new(0.0, 16.0),
        ],
        ..TileShape::default()
      }],
      ..TileData::default()
    },
  );
  set.insert_tile(
    1,
    TileData {
      class: Some("coin".to_string()),
      ..TileData::default()
    },
  );
  set
}

/// An infinite map of 16 px cells over the tiles, with the tile layers
/// `layers`
fn infinite(layers: &[&str]) -> TileMap {
  let mut map = TileMap::infinite(UVec2::splat(16));
  map.add_tileset(TilesetSource::Embedded(Arc::new(tiles())));
  for name in layers {
    map.add_tile_layer(name);
  }
  map
}

/// A generator filling the first row of every chunk's "ground" with blocks
fn floors(_: ChunkRequest) -> ChunkTiles {
  let mut tiles = ChunkTiles::default();
  for x in 0..32 {
    tiles.set("ground", UVec2::new(x, 0), Some(BLOCK));
  }
  tiles
}

/// Spawns `map`, streamed by `stream` with the cameras left out, under the
/// root `root` makes, and lets it land
fn spawn(
  maps: &mut MapWorld,
  map: TileMap,
  stream: MapStream,
  root: impl FnOnce(MapRoot) -> MapRoot,
) -> Entity {
  let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
  let stream = MapStream {
    cameras: false,
    ..stream
  };
  let map = maps.world.spawn((root(MapRoot::new(handle)), stream)).id();
  maps.settle(map);
  map
}

/// The middle of `chunk`, in world pixels (the map sits at the origin)
fn centre(chunk: IVec2) -> Vec2 {
  (chunk.as_vec2() + 0.5) * CHUNK
}

/// An anchor in the middle of `chunk`
fn anchor_at(maps: &mut MapWorld, chunk: IVec2) -> Entity {
  maps
    .world
    .spawn((Transform::from_translation(centre(chunk)), StreamAnchor))
    .id()
}

/// Moves `anchor` to the middle of `chunk`
fn move_to(world: &mut World, anchor: Entity, chunk: IVec2) {
  match world.get_mut::<Transform>(anchor) {
    Some(mut at) => at.translation = centre(chunk),
    None => panic!("{anchor} is an anchor"),
  }
}

/// `chunks` by row, then column
fn sorted(mut chunks: Vec<IVec2>) -> Vec<IVec2> {
  chunks.sort_by_key(|chunk| (chunk.y, chunk.x));
  chunks
}

/// The chunks from `middle - r` to `middle + r` on each axis, by row
fn around(middle: IVec2, r: i32) -> Vec<IVec2> {
  let chunks = (-r..=r)
    .flat_map(|y| (-r..=r).map(move |x| middle + IVec2::new(x, y)))
    .collect();
  sorted(chunks)
}

/// The chunks resident on `map`, by row: the stream's, which every tile
/// layer holds, and no other
fn resident(world: &mut World, map: Entity) -> Vec<IVec2> {
  let Some(state) = world.get::<StreamState>(map) else {
    panic!("{map} streams");
  };
  let chunks = sorted(state.resident.iter().copied().collect());
  for layer in layers_of(world, map) {
    if let Some(data) = world.get::<TileLayerData>(layer) {
      let held: Vec<IVec2> = data.grid.chunk_coords().collect();
      assert_eq!(held, chunks, "layer {layer} holds the resident chunks");
    }
  }
  chunks
}

/// Whether `map`'s stream is idle: every chunk its anchors want is
/// resident, and none is out or waiting to land
fn idle(world: &mut World, map: Entity) -> bool {
  world
    .get::<StreamState>(map)
    .is_some_and(|state| state.waiting == 0 && state.out.is_empty() && state.ready.is_empty())
}

/// Runs frames until `map`'s stream is idle
fn until_idle(maps: &mut MapWorld, map: Entity) {
  maps.frames_until(|world| idle(world, map));
}

/// The chunk messages sent since `cursor` last read
fn events(world: &World, cursor: &mut MessageCursor<ChunkEvent>) -> Vec<ChunkEvent> {
  world
    .get_resource::<Messages<ChunkEvent>>()
    .map(|messages| cursor.read(messages).cloned().collect())
    .unwrap_or_default()
}

/// The chunks of `map` that `events` say landed, by row
fn loaded(events: &[ChunkEvent], map: Entity) -> Vec<IVec2> {
  let chunks = events
    .iter()
    .filter_map(|event| match event {
      ChunkEvent::Loaded { map: of, chunk } if *of == map => Some(*chunk),
      _ => None,
    })
    .collect();
  sorted(chunks)
}

/// The chunks of `map` that `events` say unloaded, by row
fn unloading(events: &[ChunkEvent], map: Entity) -> Vec<IVec2> {
  changes(events, map)
    .into_iter()
    .map(|(chunk, changed)| {
      assert!(changed.is_none(), "nothing changed chunk {chunk}");
      chunk
    })
    .collect()
}

/// The chunks of `map` that `events` say unloaded, by row, each with the
/// tiles it carried as changed
fn changes(events: &[ChunkEvent], map: Entity) -> Vec<(IVec2, Option<ChunkTiles>)> {
  let mut chunks: Vec<(IVec2, Option<ChunkTiles>)> = events
    .iter()
    .filter_map(|event| match event {
      ChunkEvent::Unloading {
        map: of,
        chunk,
        changed,
      } if *of == map => Some((*chunk, changed.clone())),
      _ => None,
    })
    .collect();
  chunks.sort_by_key(|(chunk, _)| (chunk.y, chunk.x));
  chunks
}

/// What `chunk` carried as changed among `changes`
fn carried(changes: &[(IVec2, Option<ChunkTiles>)], chunk: IVec2) -> Option<ChunkTiles> {
  changes
    .iter()
    .find(|(of, _)| *of == chunk)
    .and_then(|(_, tiles)| tiles.clone())
}

/// The class tile entities at `cell`
fn class_tiles_at(world: &mut World, cell: IVec2) -> Vec<Entity> {
  world
    .query::<(Entity, &MapTile)>()
    .iter(world)
    .filter(|(_, tile)| tile.cell == cell)
    .map(|(entity, _)| entity)
    .collect()
}

/// The warnings in the dev log holding every one of `words`
fn warnings(words: &[&str]) -> usize {
  crate::core::dev_log::get_entries()
    .iter()
    .filter(|entry| words.iter().all(|word| entry.message.contains(word)))
    .count()
}

/// The cells of the class tiles in the world, by row
fn class_tiles(world: &mut World) -> Vec<IVec2> {
  let cells = world
    .query::<&MapTile>()
    .iter(world)
    .map(|tile| tile.cell)
    .collect();
  sorted(cells)
}

#[test]
fn chunks_load_around_an_anchor_and_unload_behind_it() {
  let mut maps = MapWorld::new();
  let map = spawn(
    &mut maps,
    infinite(&["ground"]),
    MapStream::new(floors),
    |r| r,
  );
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  // Within 512 px of the middle of chunk (0, 0): it and the chunks round it
  assert_eq!(resident(&mut maps.world, map), around(IVec2::ZERO, 1));
  let view = MapsView::new(&mut maps.world);
  assert_eq!(view.tile(map, "ground", IVec2::new(-32, -32)), Some(BLOCK));
  assert_eq!(view.tile(map, "ground", IVec2::new(63, 32)), Some(BLOCK));
  assert_eq!(view.tile(map, "ground", IVec2::new(40, 33)), None);

  // Two chunks on, the column behind is within 1,536 px still and stays
  move_to(&mut maps.world, anchor, IVec2::new(2, 0));
  until_idle(&mut maps, map);
  let kept: Vec<IVec2> = (-1..=1)
    .flat_map(|y| (-1..=3).map(move |x| IVec2::new(x, y)))
    .collect();
  assert_eq!(resident(&mut maps.world, map), sorted(kept));
  // Two more, and the two columns past 1,536 px go
  move_to(&mut maps.world, anchor, IVec2::new(4, 0));
  until_idle(&mut maps, map);
  let kept: Vec<IVec2> = (-1..=1)
    .flat_map(|y| (1..=5).map(move |x| IVec2::new(x, y)))
    .collect();
  assert_eq!(resident(&mut maps.world, map), sorted(kept));

  // Twenty chunks on: the old chunks are gone and the new ones in
  move_to(&mut maps.world, anchor, IVec2::new(24, 0));
  until_idle(&mut maps, map);
  assert_eq!(resident(&mut maps.world, map), around(IVec2::new(24, 0), 1));
  let view = MapsView::new(&mut maps.world);
  assert_eq!(view.tile(map, "ground", IVec2::new(32 * 4, 0)), None);
  assert_eq!(
    view.tile(map, "ground", IVec2::new(32 * 24, 0)),
    Some(BLOCK)
  );
}

#[test]
fn the_generator_runs_off_the_main_thread() {
  /// Each call's thread, and its name
  type Seen = Vec<(std::thread::ThreadId, Option<String>)>;
  let seen: Arc<Mutex<Seen>> = Arc::default();
  let record = Arc::clone(&seen);
  let generator = move |request| {
    let thread = std::thread::current();
    record
      .lock()
      .expect("not poisoned")
      .push((thread.id(), thread.name().map(str::to_string)));
    floors(request)
  };
  let mut maps = MapWorld::new();
  let map = spawn(
    &mut maps,
    infinite(&["ground"]),
    MapStream::new(generator),
    |r| r,
  );
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  let seen = seen.lock().expect("not poisoned");
  assert_eq!(seen.len(), 9, "a call a chunk");
  let here = std::thread::current().id();
  for (thread, name) in seen.iter() {
    assert_ne!(*thread, here, "not the main thread");
    assert_eq!(name.as_deref(), Some("ryo2d chunk builder"));
  }
}

#[test]
fn chunks_land_nearest_first_and_within_the_frame_budget() {
  let mut maps = MapWorld::new();
  let stream = MapStream {
    land_per_frame: 2,
    ..MapStream::new(floors)
  };
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  anchor_at(&mut maps, IVec2::ZERO);
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  let mut order = Vec::new();
  maps.frames_until(|world| {
    let landed: Vec<IVec2> = events(world, &mut cursor)
      .into_iter()
      .filter_map(|event| match event {
        ChunkEvent::Loaded { chunk, .. } => Some(chunk),
        ChunkEvent::Unloading { .. } => None,
      })
      .collect();
    assert!(
      landed.len() <= 2,
      "{} chunks landed in a frame",
      landed.len()
    );
    order.extend(landed);
    idle(world, map)
  });
  assert_eq!(sorted(order.clone()), around(IVec2::ZERO, 1));
  assert_eq!(order[0], IVec2::ZERO, "the anchor's own chunk first");
  let sides = [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y];
  assert!(sides.contains(&order[1]), "then one beside it: {order:?}");
  // The four beside it before the four at its corners
  assert!(
    order[1..5].iter().all(|chunk| sides.contains(chunk)),
    "{order:?}"
  );
}

#[test]
fn loaded_and_unloading_messages_follow_the_chunks() {
  let mut maps = MapWorld::new();
  let map = spawn(
    &mut maps,
    infinite(&["ground"]),
    MapStream::new(floors),
    |r| r,
  );
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  let first = events(&maps.world, &mut cursor);
  // One `Loaded` a chunk landed, and nothing unloaded
  assert_eq!(loaded(&first, map), around(IVec2::ZERO, 1));
  assert_eq!(first.len(), 9);

  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  until_idle(&mut maps, map);
  let then = events(&maps.world, &mut cursor);
  // One `Unloading` a chunk that went, one `Loaded` a chunk that came
  assert_eq!(unloading(&then, map), around(IVec2::ZERO, 1));
  assert_eq!(loaded(&then, map), around(IVec2::new(20, 0), 1));
  assert_eq!(then.len(), 18);
}

#[test]
fn a_teleported_anchor_s_area_reports_ready_once_landed() {
  let mut maps = MapWorld::new();
  let map = spawn(
    &mut maps,
    infinite(&["ground"]),
    MapStream::new(floors),
    |r| r,
  );
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  // A 640 x 360 view on the anchor
  let view = |chunk: IVec2| {
    let at = centre(chunk);
    Rect::new(at.x - 320.0, at.y - 180.0, 640.0, 360.0)
  };
  assert!(MapsView::new(&mut maps.world).stream_ready(map, view(IVec2::ZERO)));

  // A respawn a hundred chunks away: not ready until its chunks land
  let far = IVec2::new(100, 0);
  move_to(&mut maps.world, anchor, far);
  assert!(!MapsView::new(&mut maps.world).stream_ready(map, view(far)));
  maps.frames_until(|world| MapsView::new(world).stream_ready(map, view(far)));
  until_idle(&mut maps, map);
  // The old area unloaded
  assert_eq!(resident(&mut maps.world, map), around(far, 1));
  assert!(!MapsView::new(&mut maps.world).stream_ready(map, view(IVec2::ZERO)));
}

#[test]
fn a_despawned_streamed_map_takes_no_late_chunks() {
  let (calls, done) = (
    Arc::new(AtomicUsize::new(0)),
    Arc::new(AtomicBool::new(false)),
  );
  let (counted, finished) = (Arc::clone(&calls), Arc::clone(&done));
  let generator = move |request| {
    counted.fetch_add(1, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(50));
    finished.store(true, Ordering::SeqCst);
    floors(request)
  };
  let mut maps = MapWorld::new();
  // Only the anchor's own chunk
  let stream = MapStream {
    load_distance: 0.0,
    ..MapStream::new(generator)
  };
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  anchor_at(&mut maps, IVec2::ZERO);
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  maps.frame();
  let out = maps.world.get::<StreamState>(map).map(|s| s.out.len());
  assert_eq!(out, Some(1), "its chunk is out");
  assert!(maps.world.despawn(map));

  // The job finishes after the map has gone, and comes back to nothing
  maps.frames_until(|_| done.load(Ordering::SeqCst));
  for _ in 0..5 {
    std::thread::sleep(Duration::from_millis(2));
    maps.frame();
  }
  assert_eq!(calls.load(Ordering::SeqCst), 1);
  let layers = maps
    .world
    .query::<&MapLayerRef>()
    .iter(&maps.world)
    .filter(|layer| layer.map == map)
    .count();
  assert_eq!(layers, 0, "nothing of the map is left");
  assert!(class_tiles(&mut maps.world).is_empty());
  assert!(loaded(&events(&maps.world, &mut cursor), map).is_empty());
}

#[test]
fn a_panicking_generator_leaves_one_empty_chunk_and_one_warning() {
  let calls = Arc::new(AtomicUsize::new(0));
  let counted = Arc::clone(&calls);
  let broken = IVec2::new(3, 0);
  let generator = move |request: ChunkRequest| {
    if request.chunk == broken {
      counted.fetch_add(1, Ordering::SeqCst);
      panic!("a test generator that fails at chunk (3, 0)");
    }
    floors(request)
  };
  let mut maps = MapWorld::new();
  let stream = MapStream {
    load_distance: 0.0,
    unload_distance: 512.0,
    ..MapStream::new(generator)
  };
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  let anchor = anchor_at(&mut maps, broken);
  until_idle(&mut maps, map);
  // It landed empty, as a chunk the game is told of
  assert_eq!(resident(&mut maps.world, map), [broken]);
  assert_eq!(loaded(&events(&maps.world, &mut cursor), map), [broken]);
  let view = MapsView::new(&mut maps.world);
  assert_eq!(view.tile(map, "ground", IVec2::new(96, 0)), None);

  // Away and back: it lands empty again, never asked for
  move_to(&mut maps.world, anchor, IVec2::new(10, 0));
  until_idle(&mut maps, map);
  assert_eq!(resident(&mut maps.world, map), [IVec2::new(10, 0)]);
  move_to(&mut maps.world, anchor, broken);
  until_idle(&mut maps, map);
  assert_eq!(resident(&mut maps.world, map), [broken]);
  assert_eq!(calls.load(Ordering::SeqCst), 1, "asked once");
  let named = format!("map {map}");
  assert_eq!(warnings(&[&named, "chunk (3, 0)", "panicked"]), 1);
}

#[test]
fn two_far_anchors_keep_both_areas_and_nothing_between() {
  let mut maps = MapWorld::new();
  let map = spawn(
    &mut maps,
    infinite(&["ground"]),
    MapStream::new(floors),
    |r| r,
  );
  anchor_at(&mut maps, IVec2::ZERO);
  let other = anchor_at(&mut maps, IVec2::new(40, 0));
  until_idle(&mut maps, map);
  let both = [around(IVec2::ZERO, 1), around(IVec2::new(40, 0), 1)].concat();
  assert_eq!(resident(&mut maps.world, map), sorted(both));

  // One moves on: its old area goes, and the other's stays
  move_to(&mut maps.world, other, IVec2::new(0, 40));
  until_idle(&mut maps, map);
  let both = [around(IVec2::ZERO, 1), around(IVec2::new(0, 40), 1)].concat();
  assert_eq!(resident(&mut maps.world, map), sorted(both));
}

#[test]
fn an_edit_in_a_chunk_not_yet_loaded_on_a_streamed_map_is_refused() {
  let mut maps = MapWorld::new();
  let map = spawn(
    &mut maps,
    infinite(&["ground"]),
    MapStream::new(floors),
    |r| r,
  );
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  let mut view = MapsView::new(&mut maps.world);
  // Chunk (62, 0) is far from the anchor: no tile, no chunk
  assert!(!view.set_tile(map, "ground", IVec2::new(2000, 5), Some(BLOCK)));
  assert_eq!(view.tile(map, "ground", IVec2::new(2000, 5)), None);
  // In a resident chunk it goes in
  assert!(view.set_tile(map, "ground", IVec2::new(5, 5), Some(BLOCK)));
  // A fill reaching past the resident chunks fills their cells alone:
  // cells -32 to 63 of row 6
  let filled = view.fill(
    map,
    "ground",
    IVec2::new(-40, 6),
    IVec2::new(130, 6),
    Some(BLOCK),
  );
  assert_eq!(filled, 96);
  assert_eq!(resident(&mut maps.world, map), around(IVec2::ZERO, 1));
}

#[test]
fn where_there_are_no_threads_generating_keeps_to_the_frame_budget() {
  INLINE.with(|inline| inline.set(true));
  let calls = Arc::new(AtomicUsize::new(0));
  let elsewhere = Arc::new(AtomicBool::new(false));
  let (counted, off_main) = (Arc::clone(&calls), Arc::clone(&elsewhere));
  let main = std::thread::current().id();
  let generator = move |request| {
    counted.fetch_add(1, Ordering::SeqCst);
    if std::thread::current().id() != main {
      off_main.store(true, Ordering::SeqCst);
    }
    std::thread::sleep(Duration::from_millis(1));
    floors(request)
  };
  let mut maps = MapWorld::new();
  let stream = MapStream {
    web_budget: Duration::from_millis(2),
    ..MapStream::new(generator)
  };
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  anchor_at(&mut maps, IVec2::ZERO);
  let (mut made, mut frames) = (0, 0);
  maps.frames_until(|world| {
    let now = calls.swap(0, Ordering::SeqCst);
    assert!(now <= 2, "{now} chunks generated in a frame");
    made += now;
    frames += 1;
    idle(world, map)
  });
  INLINE.with(|inline| inline.set(false));
  assert_eq!(made, 9, "every chunk, once");
  assert!(frames >= 5, "spread over {frames} frames");
  assert!(!elsewhere.load(Ordering::SeqCst), "on the main thread");
  assert_eq!(resident(&mut maps.world, map), around(IVec2::ZERO, 1));
}

#[test]
fn a_class_tile_edited_into_a_layer_the_generator_left_empty_goes_with_its_chunk() {
  // The generator fills "ground" alone, with a coin in chunk (0, 0)
  let generator = |request: ChunkRequest| {
    let mut tiles = floors(request);
    if request.chunk == IVec2::ZERO {
      tiles.set("ground", UVec2::new(3, 3), Some(COIN));
    }
    tiles
  };
  let mut maps = MapWorld::new();
  let map = spawn(
    &mut maps,
    infinite(&["ground", "items"]),
    MapStream::new(generator),
    |root| root.with_entity_tiles(&["coin"]),
  );
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  maps.frame();
  // The generated coin is a class tile, out of the grid
  assert_eq!(class_tiles(&mut maps.world), [IVec2::new(3, 3)]);
  assert_eq!(
    MapsView::new(&mut maps.world).tile(map, "ground", IVec2::new(3, 3)),
    None
  );
  // A coin put in "items", which the generator left empty there
  assert!(MapsView::new(&mut maps.world).set_tile(map, "items", IVec2::new(5, 5), Some(COIN)));
  assert_eq!(
    class_tiles(&mut maps.world),
    [IVec2::new(3, 3), IVec2::new(5, 5)]
  );

  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  until_idle(&mut maps, map);
  maps.frame();
  assert!(
    class_tiles(&mut maps.world).is_empty(),
    "both went with chunk (0, 0)"
  );
}

#[test]
fn an_unload_distance_below_the_load_distance_is_raised_to_it_with_one_warning() {
  let mut maps = MapWorld::new();
  let stream = MapStream {
    load_distance: 512.0,
    unload_distance: 100.0,
    ..MapStream::new(floors)
  };
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  move_to(&mut maps.world, anchor, IVec2::new(2, 0));
  until_idle(&mut maps, map);
  // Kept within 512 px, the chunks it loads: no chunk loads and unloads
  // again and again
  assert_eq!(resident(&mut maps.world, map), around(IVec2::new(2, 0), 1));
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  events(&maps.world, &mut cursor);
  for _ in 0..5 {
    maps.frame();
  }
  assert!(events(&maps.world, &mut cursor).is_empty(), "settled");
  let named = format!("map {map}");
  assert_eq!(warnings(&[&named, "unload_distance"]), 1);
}

#[test]
fn a_layer_the_map_lacks_is_warned_once_and_ignored() {
  let generator = |request| {
    let mut tiles = floors(request);
    tiles.set("lava-5e2b", UVec2::ZERO, Some(BLOCK));
    tiles
  };
  let mut maps = MapWorld::new();
  let map = spawn(
    &mut maps,
    infinite(&["ground"]),
    MapStream::new(generator),
    |r| r,
  );
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  assert_eq!(resident(&mut maps.world, map), around(IVec2::ZERO, 1));
  assert_eq!(
    MapsView::new(&mut maps.world).tile(map, "ground", IVec2::ZERO),
    Some(BLOCK)
  );
  let named = format!("map {map}");
  assert_eq!(warnings(&[&named, "lava-5e2b"]), 1);
}

#[test]
fn an_anchor_s_load_area_is_capped_at_a_span_of_chunks_with_one_warning() {
  // The cap lowered to 3 chunks a side, where 2,048 px asks for 9
  SPAN_CAP.with(|cap| cap.set(3));
  let mut maps = MapWorld::new();
  let stream = MapStream {
    load_distance: 2048.0,
    unload_distance: 4096.0,
    ..MapStream::new(floors)
  };
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  SPAN_CAP.with(|cap| cap.set(super::MAX_SPAN));
  assert_eq!(resident(&mut maps.world, map), around(IVec2::ZERO, 1));
  let named = format!("map {map}");
  assert_eq!(warnings(&[&named, "chunks a side"]), 1);
}

#[test]
fn chunks_landing_in_the_map_s_own_landing_frame_spawn_their_class_tiles() {
  // With no thread, a chunk asked for lands in the frame it is asked for:
  // here the frame the map itself lands, the anchor waiting for it
  INLINE.with(|inline| inline.set(true));
  let generator = |request: ChunkRequest| {
    let mut tiles = floors(request);
    tiles.set("ground", UVec2::new(3, 3), Some(COIN));
    tiles
  };
  let mut maps = MapWorld::new();
  anchor_at(&mut maps, IVec2::ZERO);
  let stream = MapStream {
    land_per_frame: 9,
    ..MapStream::new(generator)
  };
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |root| {
    root.with_entity_tiles(&["coin"])
  });
  INLINE.with(|inline| inline.set(false));
  assert_eq!(resident(&mut maps.world, map), around(IVec2::ZERO, 1));
  // A coin a chunk, each a class tile and out of the grid
  let coins: Vec<IVec2> = around(IVec2::ZERO, 1)
    .into_iter()
    .map(|chunk| chunk * 32 + IVec2::new(3, 3))
    .collect();
  assert_eq!(class_tiles(&mut maps.world), sorted(coins));
  assert_eq!(
    MapsView::new(&mut maps.world).tile(map, "ground", IVec2::new(3, 3)),
    None
  );
}

#[test]
fn a_bounded_map_given_a_stream_streams_nothing_and_warns_once() {
  let calls = Arc::new(AtomicUsize::new(0));
  let counted = Arc::clone(&calls);
  let generator = move |request| {
    counted.fetch_add(1, Ordering::SeqCst);
    floors(request)
  };
  let mut map = TileMap::new(UVec2::new(8, 8), UVec2::splat(16));
  map.add_tileset(TilesetSource::Embedded(Arc::new(tiles())));
  map.add_tile_layer("ground");
  let mut maps = MapWorld::new();
  let map = spawn(&mut maps, map, MapStream::new(generator), |r| r);
  anchor_at(&mut maps, IVec2::ZERO);
  for _ in 0..3 {
    maps.frame();
  }
  assert_eq!(calls.load(Ordering::SeqCst), 0);
  let named = format!("map {map}");
  assert_eq!(warnings(&[&named, "streams nothing"]), 1);
  // Its own cells still take edits
  assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", IVec2::new(2, 2), Some(BLOCK)));
}

/// A generator laying `floors`, with a coin at (3, 3) and (7, 7) of chunk
/// (0, 0), counting its calls for that chunk in `calls`
fn coins_at_home(calls: Arc<AtomicUsize>) -> impl Fn(ChunkRequest) -> ChunkTiles + Send + Sync {
  move |request: ChunkRequest| {
    let mut tiles = floors(request);
    if request.chunk == IVec2::ZERO {
      calls.fetch_add(1, Ordering::SeqCst);
      tiles.set("ground", UVec2::new(3, 3), Some(COIN));
      tiles.set("ground", UVec2::new(7, 7), Some(COIN));
    }
    tiles
  }
}

#[test]
fn an_edit_survives_leaving_and_coming_back() {
  let calls = Arc::new(AtomicUsize::new(0));
  let saved = SavedChunks::default();
  let mut maps = MapWorld::new();
  let stream = MapStream::new(coins_at_home(Arc::clone(&calls))).with_saved(saved.clone());
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  // A block of chunk (0, 0)'s floor dug out, and one built
  let (dug, built) = (IVec2::new(4, 0), IVec2::new(6, 9));
  let mut view = MapsView::new(&mut maps.world);
  assert!(view.set_tile(map, "ground", dug, None));
  assert!(view.set_tile(map, "ground", built, Some(BLOCK)));
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  events(&maps.world, &mut cursor);

  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  until_idle(&mut maps, map);
  assert_eq!(saved.len(), 1, "the edited chunk alone");
  let gone = changes(&events(&maps.world, &mut cursor), map);
  assert_eq!(gone.len(), 9);
  let Some(tiles) = carried(&gone, IVec2::ZERO) else {
    panic!("chunk (0, 0) carried its tiles: {gone:?}");
  };
  assert_eq!(tiles.get("ground", UVec2::new(4, 0)), None);
  assert_eq!(tiles.get("ground", UVec2::new(5, 0)), Some(BLOCK));
  assert_eq!(tiles.get("ground", UVec2::new(6, 9)), Some(BLOCK));
  assert_eq!(saved.get(IVec2::ZERO), Some(tiles));
  let others = gone.iter().filter(|(chunk, _)| *chunk != IVec2::ZERO);
  assert!(others.clone().all(|(_, tiles)| tiles.is_none()), "{gone:?}");

  move_to(&mut maps.world, anchor, IVec2::ZERO);
  until_idle(&mut maps, map);
  let view = MapsView::new(&mut maps.world);
  assert_eq!(view.tile(map, "ground", dug), None);
  assert_eq!(view.tile(map, "ground", built), Some(BLOCK));
  assert_eq!(view.tile(map, "ground", IVec2::new(5, 0)), Some(BLOCK));
  assert_eq!(calls.load(Ordering::SeqCst), 1, "not generated again");
}

#[test]
fn an_unchanged_chunk_is_not_saved() {
  // Its coins taken out of the grid as it lands are not a change
  let saved = SavedChunks::default();
  let mut maps = MapWorld::new();
  let generator = coins_at_home(Arc::default());
  let stream = MapStream::new(generator).with_saved(saved.clone());
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |root| {
    root.with_entity_tiles(&["coin"])
  });
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  maps.frame();
  assert_eq!(class_tiles(&mut maps.world).len(), 2);
  let mut cursor = MessageCursor::<ChunkEvent>::default();

  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  until_idle(&mut maps, map);
  // `unloading` checks that each carried nothing
  assert_eq!(
    unloading(&events(&maps.world, &mut cursor), map),
    around(IVec2::ZERO, 1)
  );
  assert!(saved.is_empty());
}

#[test]
fn an_edit_in_the_unloading_frame_is_saved() {
  let saved = SavedChunks::default();
  let mut maps = MapWorld::new();
  let stream = MapStream::new(floors).with_saved(saved.clone());
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  // The anchor leaves, and a cell is edited before the stream next looks
  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", IVec2::new(8, 8), Some(BLOCK)));
  maps.frame();
  assert!(!resident(&mut maps.world, map).contains(&IVec2::ZERO));
  let kept = saved.get(IVec2::ZERO);
  assert_eq!(
    kept.and_then(|tiles| tiles.get("ground", UVec2::new(8, 8))),
    Some(BLOCK)
  );
}

#[test]
fn saved_chunks_round_trip_through_serde() {
  let turned = Tile::new(2, 77).with_flip(TileFlip::DIAGONAL);
  let mut first = ChunkTiles::default();
  first.set("ground", UVec2::new(0, 0), Some(BLOCK));
  first.set("ground", UVec2::new(31, 31), Some(turned));
  first.set("items", UVec2::new(3, 3), Some(COIN));
  let mut second = ChunkTiles::default();
  second.set("ground", UVec2::new(5, 6), Some(BLOCK));
  let saved = SavedChunks::default();
  saved.insert(IVec2::ZERO, first.clone());
  saved.insert(IVec2::new(-3, 7), second.clone());
  // An emptied chunk is kept as empty
  saved.insert(IVec2::new(9, -9), ChunkTiles::default());

  let text = serde_json::to_string(&saved).expect("serializes");
  let back: SavedChunks = serde_json::from_str(&text).expect("deserializes");
  assert_eq!(back.len(), 3);
  assert_eq!(back.get(IVec2::ZERO), Some(first));
  assert_eq!(back.get(IVec2::new(-3, 7)), Some(second));
  assert_eq!(back.get(IVec2::new(9, -9)), Some(ChunkTiles::default()));
  // A clone shares its store, and what was read back is a store of its own
  saved.clone().remove(IVec2::ZERO);
  assert_eq!((saved.len(), back.len()), (2, 3));

  // A broken save is an error, not a panic or a wrong chunk: text that is
  // not base64, two cells where a chunk has 1,024, a number for the text
  for broken in [
    r#"[{"chunk":[0,0],"tiles":{"ground":"not base64!"}}]"#,
    r#"[{"chunk":[0,0],"tiles":{"ground":"eJxjYIAAAAAIAAE="}}]"#,
    r#"[{"chunk":[0,0],"tiles":{"ground":7}}]"#,
  ] {
    assert!(
      serde_json::from_str::<SavedChunks>(broken).is_err(),
      "{broken}"
    );
  }
}

#[test]
fn chunk_owned_entities_unload_with_their_chunk() {
  let mut maps = MapWorld::new();
  let map = spawn(
    &mut maps,
    infinite(&["ground"]),
    MapStream::new(floors),
    |r| r,
  );
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  // A pickup on each chunk as it landed
  let mut owned = Vec::new();
  for chunk in loaded(&events(&maps.world, &mut cursor), map) {
    let at = Transform::from_translation(centre(chunk));
    let entity = maps.world.spawn((at, ChunkOwned { map, chunk })).id();
    owned.push((chunk, entity));
  }
  // One asleep (disabled), and one carrying a child
  let entity_of = |chunk: IVec2| owned.iter().find(|(c, _)| *c == chunk).map(|(_, e)| *e);
  let (Some(asleep), Some(parent)) = (entity_of(IVec2::new(0, 1)), entity_of(IVec2::new(-1, 0)))
  else {
    panic!("the chunks round (0, 0) landed");
  };
  maps.world.entity_mut(asleep).insert(Disabled);
  let child = maps
    .world
    .spawn((Transform::default(), ChildOf(parent)))
    .id();

  // Four chunks on, columns -1 and 0 unload, and column 1 stays
  move_to(&mut maps.world, anchor, IVec2::new(4, 0));
  until_idle(&mut maps, map);
  for (chunk, entity) in owned {
    let alive = maps.world.get_entity(entity).is_ok();
    assert_eq!(alive, chunk.x == 1, "the entity of chunk {chunk}");
  }
  assert!(maps.world.get_entity(child).is_err(), "the child went too");
}

#[test]
fn a_despawned_streamed_map_takes_its_chunk_owned_entities() {
  let mut maps = MapWorld::new();
  let map = spawn(
    &mut maps,
    infinite(&["ground"]),
    MapStream::new(floors),
    |r| r,
  );
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  let owned = maps
    .world
    .spawn(ChunkOwned {
      map,
      chunk: IVec2::ZERO,
    })
    .id();
  let other = maps
    .world
    .spawn(ChunkOwned {
      map: Entity::PLACEHOLDER,
      chunk: IVec2::ZERO,
    })
    .id();
  assert!(maps.world.despawn(map));
  maps.frame();
  assert!(maps.world.get_entity(owned).is_err());
  assert!(maps.world.get_entity(other).is_ok(), "another map's stays");
}

#[test]
fn a_saved_chunk_keeps_its_class_tiles_and_a_collected_one_stays_gone() {
  let calls = Arc::new(AtomicUsize::new(0));
  let saved = SavedChunks::default();
  let mut maps = MapWorld::new();
  let stream = MapStream::new(coins_at_home(Arc::clone(&calls))).with_saved(saved.clone());
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |root| {
    root.with_entity_tiles(&["coin"])
  });
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  maps.frame();
  // A tile built away from the coins saves the chunk; the coin at (7, 7)
  // is collected
  let built = IVec2::new(12, 12);
  assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", built, Some(BLOCK)));
  let collected = class_tiles_at(&mut maps.world, IVec2::new(7, 7));
  assert_eq!(collected.len(), 1);
  for coin in collected {
    assert!(maps.world.despawn(coin));
  }
  let mut cursor = MessageCursor::<ChunkEvent>::default();

  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  until_idle(&mut maps, map);
  maps.frame();
  assert!(class_tiles(&mut maps.world).is_empty());
  let Some(tiles) = carried(
    &changes(&events(&maps.world, &mut cursor), map),
    IVec2::ZERO,
  ) else {
    panic!("chunk (0, 0) carried its tiles");
  };
  assert_eq!(tiles.get("ground", UVec2::new(3, 3)), Some(COIN));
  assert_eq!(tiles.get("ground", UVec2::new(7, 7)), None);

  move_to(&mut maps.world, anchor, IVec2::ZERO);
  until_idle(&mut maps, map);
  maps.frame();
  // The coin left there is a class tile again; the collected one stays gone
  assert_eq!(class_tiles(&mut maps.world), [IVec2::new(3, 3)]);
  let view = MapsView::new(&mut maps.world);
  assert_eq!(view.tile(map, "ground", IVec2::new(3, 3)), None);
  assert_eq!(view.tile(map, "ground", built), Some(BLOCK));
  assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn class_tiles_a_game_takes_or_places_are_changes() {
  let saved = SavedChunks::default();
  let mut maps = MapWorld::new();
  let stream = MapStream::new(coins_at_home(Arc::default())).with_saved(saved.clone());
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |root| {
    root.with_entity_tiles(&["coin"])
  });
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  maps.frame();
  // Chunk (0, 0)'s coins both collected, and a coin placed in an empty
  // cell of chunk (1, 0): neither changes a grid's cells
  for cell in [IVec2::new(3, 3), IVec2::new(7, 7)] {
    for coin in class_tiles_at(&mut maps.world, cell) {
      assert!(maps.world.despawn(coin));
    }
  }
  let placed = IVec2::new(40, 9);
  assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", placed, Some(COIN)));

  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  until_idle(&mut maps, map);
  let mut kept: Vec<IVec2> = [IVec2::ZERO, IVec2::X]
    .into_iter()
    .filter(|&chunk| saved.get(chunk).is_some())
    .collect();
  kept.sort_by_key(|chunk| chunk.x);
  assert_eq!((kept, saved.len()), (vec![IVec2::ZERO, IVec2::X], 2));

  move_to(&mut maps.world, anchor, IVec2::ZERO);
  until_idle(&mut maps, map);
  maps.frame();
  assert_eq!(class_tiles(&mut maps.world), [placed]);
}

#[test]
fn saving_the_changed_chunks_round_an_anchor_keeps_them_for_a_save_game() {
  let saved = SavedChunks::default();
  let mut maps = MapWorld::new();
  let stream = MapStream::new(floors).with_saved(saved.clone());
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  let built = IVec2::new(-20, 20);
  assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", built, Some(BLOCK)));
  // Nothing has unloaded: the store is empty until the changed chunks are
  // put in, and only the changed one goes in
  assert!(saved.is_empty());
  assert_eq!(MapsView::new(&mut maps.world).save_changed_chunks(map), 1);
  assert_eq!(saved.len(), 1);
  assert_eq!(resident(&mut maps.world, map), around(IVec2::ZERO, 1));

  // Written with a save game and read back, it streams a new map with the
  // tile there
  let text = serde_json::to_string(&saved).expect("serializes");
  let loaded: SavedChunks = serde_json::from_str(&text).expect("deserializes");
  let mut again = MapWorld::new();
  let stream = MapStream::new(floors).with_saved(loaded);
  let map = spawn(&mut again, infinite(&["ground"]), stream, |r| r);
  anchor_at(&mut again, IVec2::ZERO);
  until_idle(&mut again, map);
  let view = MapsView::new(&mut again.world);
  assert_eq!(view.tile(map, "ground", built), Some(BLOCK));
  assert_eq!(view.tile(map, "ground", IVec2::new(-20, 0)), Some(BLOCK));
}

#[test]
fn painting_terrain_outside_the_resident_chunks_is_refused() {
  let mut maps = MapWorld::new();
  // The one-colour Wang set "solo", from a map Tiled saved
  let solo = load_map(&mut maps.world, "tiled/maps/wang-solo-empty.tmj");
  let tilesets = maps
    .world
    .resource::<Assets<TileMap>>()
    .get(&solo)
    .map(|m| m.tilesets.clone())
    .unwrap_or_default();
  let mut map = TileMap::infinite(UVec2::splat(16));
  map.tilesets = tilesets;
  map.add_tile_layer("ground");
  let stream = MapStream {
    load_distance: 0.0,
    ..MapStream::new(|_| ChunkTiles::default())
  };
  let map = spawn(&mut maps, map, stream, |r| r);
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  let mut view = MapsView::new(&mut maps.world);
  // Chunk (3, 0) is not resident: nothing painted, no chunk made
  assert!(!view.set_terrain(map, "ground", IVec2::new(100, 16), "solo", Some("ground")));
  assert_eq!(view.tile(map, "ground", IVec2::new(100, 16)), None);
  // Chunk (0, 0) is
  assert!(view.set_terrain(map, "ground", IVec2::new(16, 16), "solo", Some("ground")));
  assert_eq!(resident(&mut maps.world, map), [IVec2::ZERO]);
}

#[test]
fn int_grid_edits_outside_the_resident_chunks_are_refused() {
  let mut maps = MapWorld::new();
  let stream = MapStream {
    load_distance: 0.0,
    ..MapStream::new(floors)
  };
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  // A game's IntGrid over chunks (0, 0) to (2, 0), only the first resident
  let Some(ground) = MapsView::new(&mut maps.world).layer_entity(map, "ground") else {
    panic!("no ground layer");
  };
  if let Some(mut data) = maps.world.get_mut::<TileLayerData>(ground) {
    data.intgrid = Some(IntGrid {
      size: UVec2::new(96, 32),
      values: vec![0; 96 * 32],
      ..IntGrid::default()
    });
  }
  let mut view = MapsView::new(&mut maps.world);
  assert!(!view.set_intgrid(map, "ground", IVec2::new(40, 5), 1));
  assert!(view.set_intgrid(map, "ground", IVec2::new(5, 5), 1));
  assert_eq!(
    view.fill_intgrid(map, "ground", IVec2::ZERO, IVec2::new(95, 31), 2),
    1024
  );
  assert_eq!(view.intgrid(map, "ground", IVec2::new(40, 5)), None);
  assert_eq!(view.intgrid(map, "ground", IVec2::new(31, 31)), Some(2));
  assert_eq!(resident(&mut maps.world, map), [IVec2::ZERO]);
}
