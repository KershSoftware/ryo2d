//! A streamed map's chunks generated around anchors on a thread of their
//! own, landed nearest first within a budget a frame and unloaded far
//! behind, with their messages, the edits refused outside them, a
//! generator that panics, the chunks a player changed saved and brought
//! back, and the entities that go with their chunk. Each test keeps to a
//! few chunks of 16 px cells.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use glam::{IVec2, UVec2, Vec2};

use super::{
  ChunkEvent, ChunkOwned, ChunkRequest, ChunkTiles, INLINE, MapStream, SPAN_CAP, SavedChunks,
  StreamAnchor, StreamState, StreamStatus,
};
use crate::core::asset::{Assets, AssetsView, Handle};
use crate::core::ecs::kernel::{ChildOf, Disabled, MessageCursor, Messages};
use crate::core::ecs::world_ext::despawn_game_entities;
use crate::core::ecs::{Entity, Transform, World};
use crate::core::graphics::{Camera2d, CameraView};
use crate::core::physics::CellRect;
use crate::core::tilemap::test_util::{FIXTURES, MapWorld, files_with, layers_of, load_map};
use crate::core::tilemap::{
  GridShape, IntGrid, LayerKind, MapEvent, MapGrid, MapLayerRef, MapRoot, MapTile, MapsView,
  StaggerAxis, StaggerIndex, Tile, TileData, TileFlip, TileGrid, TileLayerData, TileMap, TileShape,
  Tileset, TilesetImage, TilesetSource,
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

/// `floors`, each chunk waiting while `open` is false (5 s at most): a test
/// holds the generator over the frames it asserts, so no chunk lands in
/// them however the builder thread is scheduled
fn floors_while(
  open: &Arc<AtomicBool>,
) -> impl Fn(ChunkRequest) -> ChunkTiles + Send + Sync + use<> {
  let open = Arc::clone(open);
  move |request: ChunkRequest| {
    let start = std::time::Instant::now();
    while !open.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(5) {
      std::thread::sleep(Duration::from_millis(1));
    }
    floors(request)
  }
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
  generated_tracked(world, map);
  for layer in layers_of(world, map) {
    if let Some(data) = world.get::<TileLayerData>(layer) {
      let held: Vec<IVec2> = data.tiles.chunk_coords().collect();
      assert_eq!(held, chunks, "layer {layer} holds the resident chunks");
    }
  }
  chunks
}

/// Asserts that `map`'s stream keeps its landed chunks apart: the resident
/// chunks that are not the file's, the only ones it looks at to unload
fn generated_tracked(world: &World, map: Entity) {
  let Some(state) = world.get::<StreamState>(map) else {
    panic!("{map} streams");
  };
  let landed: Vec<IVec2> = state
    .resident
    .iter()
    .copied()
    .filter(|chunk| !state.file.contains(chunk))
    .collect();
  assert_eq!(
    sorted(state.generated.iter().copied().collect()),
    sorted(landed),
    "the generated chunks are the resident ones not the file's"
  );
}

/// Whether `map`'s stream is idle: every chunk its anchors want is
/// resident, none is out or waiting to land, and none waits to unload
fn idle(world: &mut World, map: Entity) -> bool {
  world.get::<StreamState>(map).is_some_and(|state| {
    state.waiting == 0 && state.out.is_empty() && state.ready.is_empty() && state.draining == 0
  })
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
fn a_streamed_request_carries_the_grid() {
  let seen: Arc<Mutex<Vec<ChunkRequest>>> = Arc::default();
  let record = Arc::clone(&seen);
  let generator = move |request: ChunkRequest| {
    record.lock().expect("not poisoned").push(request);
    floors(request)
  };
  let cell = UVec2::new(64, 32);
  let mut map = TileMap::infinite(cell).with_shape(GridShape::Isometric);
  map.add_tileset(TilesetSource::Embedded(Arc::new(tiles())));
  map.add_tile_layer("ground");
  let mut maps = MapWorld::new();
  let map = spawn(&mut maps, map, MapStream::new(generator), |r| r);
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  let seen = seen.lock().expect("not poisoned");
  assert!(!seen.is_empty(), "the generator was asked");
  let grid = MapGrid::for_map(GridShape::Isometric, cell, UVec2::ZERO, true);
  for request in seen.iter() {
    assert_eq!(request.grid, grid, "chunk {}", request.chunk);
    assert_eq!(request.grid.shape, GridShape::Isometric);
    assert_eq!(request.first_cell, request.chunk * 32);
  }
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
fn a_teleport_s_unloads_spread_over_frames_farthest_first() {
  let mut maps = MapWorld::new();
  let stream = MapStream {
    unload_per_frame: 2,
    ..MapStream::new(floors)
  };
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  events(&maps.world, &mut cursor);

  // Twenty chunks on: the nine left behind go two a frame, the farthest
  // (the column at x -1) first, then the column at 0, then the one at 1,
  // staying resident until their turn
  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  let old = around(IVec2::ZERO, 1);
  let mut frames: Vec<Vec<IVec2>> = Vec::new();
  maps.frames_until(|world| {
    let gone = unloading(&events(world, &mut cursor), map);
    assert!(gone.len() <= 2, "{} chunks unloaded in a frame", gone.len());
    if !gone.is_empty() {
      frames.push(gone);
    }
    // Those not gone yet are still resident
    let went = frames.concat();
    let Some(state) = world.get::<StreamState>(map) else {
      panic!("{map} streams");
    };
    let still: Vec<IVec2> = old
      .iter()
      .copied()
      .filter(|chunk| state.resident.contains(chunk))
      .collect();
    let waiting: Vec<IVec2> = old
      .iter()
      .copied()
      .filter(|chunk| !went.contains(chunk))
      .collect();
    assert_eq!(
      still, waiting,
      "the old chunks stay resident until their turn"
    );
    idle(world, map)
  });
  assert_eq!(frames.len(), 5, "nine chunks, two a frame: {frames:?}");
  assert_eq!(
    sorted(frames.concat()),
    around(IVec2::ZERO, 1),
    "each went once"
  );
  // How far a chunk's box is from the anchor
  let far = |chunk: &IVec2| {
    let (min, at) = (chunk.as_vec2() * CHUNK, centre(IVec2::new(20, 0)));
    (min - at).max(at - (min + CHUNK)).max(Vec2::ZERO).length()
  };
  for pair in frames.windows(2) {
    let nearest = pair[0].iter().map(far).fold(f32::INFINITY, f32::min);
    let farthest = pair[1].iter().map(far).fold(0.0, f32::max);
    assert!(nearest >= farthest, "farthest first: {frames:?}");
  }
  // Settled: within the distances again
  assert_eq!(resident(&mut maps.world, map), around(IVec2::new(20, 0), 1));
}

#[test]
fn a_frame_that_lands_chunks_unloads_none_unless_many_wait() {
  // Four chunks on: nine chunks land and six are left behind, so few wait
  // that the frames landing unload nothing and the unloads come after
  let mut maps = MapWorld::new();
  let map = spawn(
    &mut maps,
    infinite(&["ground"]),
    MapStream::new(floors),
    |r| r,
  );
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  events(&maps.world, &mut cursor);
  move_to(&mut maps.world, anchor, IVec2::new(4, 0));
  let (mut landed, mut gone, mut both) = (Vec::new(), Vec::new(), 0);
  maps.frames_until(|world| {
    let happened = events(world, &mut cursor);
    let (now_landed, now_gone) = (loaded(&happened, map), unloading(&happened, map));
    both += usize::from(!now_landed.is_empty() && !now_gone.is_empty());
    landed.extend(now_landed);
    gone.extend(now_gone);
    idle(world, map)
  });
  assert_eq!(both, 0, "no frame both landed and unloaded");
  assert_eq!(sorted(landed), around(IVec2::new(4, 0), 1));
  let behind: Vec<IVec2> = around(IVec2::ZERO, 1)
    .into_iter()
    .filter(|chunk| chunk.x < 1)
    .collect();
  assert_eq!(sorted(gone), behind, "the chunks left behind all went");
  assert_eq!(
    resident(&mut maps.world, map),
    sorted(
      around(IVec2::new(4, 0), 1)
        .into_iter()
        .chain(around(IVec2::ZERO, 1).into_iter().filter(|c| c.x == 1))
        .collect()
    )
  );

  // A teleport leaving 25 chunks behind, more than eight: they unload in
  // the frames landing chunks too, until no more than eight wait
  let mut maps = MapWorld::new();
  let calls = Arc::new(AtomicUsize::new(0));
  let counted = Arc::clone(&calls);
  let stream = MapStream {
    load_distance: 2.0 * CHUNK,
    land_per_frame: 1,
    unload_per_frame: 1,
    ..MapStream::new(move |request: ChunkRequest| {
      counted.fetch_add(1, Ordering::SeqCst);
      floors(request)
    })
  };
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  events(&maps.world, &mut cursor);
  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  // The first frame asks for the 25 chunks round the new place and unloads
  // one; then the builder thread makes them all before the next frame, so
  // they land one a frame from it while more than eight still wait,
  // however the thread is scheduled
  let asked = calls.load(Ordering::SeqCst) + 25;
  let (mut gone, mut both) = (Vec::new(), Vec::new());
  let mut first = true;
  maps.frames_until(|world| {
    if std::mem::replace(&mut first, false) {
      let start = std::time::Instant::now();
      while calls.load(Ordering::SeqCst) < asked && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(1));
      }
    }
    let happened = events(world, &mut cursor);
    let (now_landed, now_gone) = (loaded(&happened, map), unloading(&happened, map));
    if !now_landed.is_empty() && !now_gone.is_empty() {
      both.push(gone.len());
      // The frame's landings come first, then its unloads
      let unloading = |event: &ChunkEvent| matches!(event, ChunkEvent::Unloading { .. });
      let first = happened.iter().position(unloading);
      assert!(
        first.is_some_and(|first| happened[first..].iter().all(unloading)),
        "Loaded before Unloading: {happened:?}"
      );
    }
    gone.extend(now_gone);
    idle(world, map)
  });
  assert!(
    !both.is_empty() && both.iter().all(|&went| went < 17),
    "frames landing unloaded while more than eight waited, and only then: {both:?}"
  );
  assert_eq!(sorted(gone), around(IVec2::ZERO, 2), "and all went");
}

#[test]
fn a_raised_unload_budget_leaves_no_more_than_eight_waiting_while_chunks_land() {
  // Eight unloading a frame, one landing: a teleport leaves 25 chunks
  // behind, eight go in its first frame (the new chunks held back, none
  // landing), and the frames landing after it unload too while more than
  // eight wait, however large the budget
  let open = Arc::new(AtomicBool::new(true));
  let calls = Arc::new(AtomicUsize::new(0));
  let (gate, counted) = (floors_while(&open), Arc::clone(&calls));
  let stream = MapStream {
    load_distance: 2.0 * CHUNK,
    land_per_frame: 1,
    unload_per_frame: 8,
    ..MapStream::new(move |request: ChunkRequest| {
      let tiles = gate(request);
      counted.fetch_add(1, Ordering::SeqCst);
      tiles
    })
  };
  let mut maps = MapWorld::new();
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  let status = |world: &mut World| MapsView::new(world).stream_status(map);
  open.store(false, Ordering::SeqCst);
  let asked = calls.load(Ordering::SeqCst) + 25;
  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  maps.frame();
  assert_eq!(status(&mut maps.world).map(|s| s.draining), Some(17));
  // The builder thread makes all 25 before the frames landing them
  open.store(true, Ordering::SeqCst);
  let start = std::time::Instant::now();
  while calls.load(Ordering::SeqCst) < asked && start.elapsed() < Duration::from_secs(5) {
    std::thread::sleep(Duration::from_millis(1));
  }
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  events(&maps.world, &mut cursor);
  let mut waiting = Vec::new();
  for _ in 0..3 {
    maps.frame();
    let happened = events(&maps.world, &mut cursor);
    assert_eq!(loaded(&happened, map).len(), 1, "one lands a frame");
    waiting.push(status(&mut maps.world).map_or(0, |s| s.draining));
  }
  assert_eq!(
    waiting,
    [9, 1, 1],
    "eight more go in each frame landing while more than eight wait"
  );
  until_idle(&mut maps, map);
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
fn a_point_on_a_chunk_boundary_is_ready_once_the_chunk_holding_it_lands() {
  let mut maps = MapWorld::new();
  // Only the anchor's own chunk
  let stream = MapStream {
    load_distance: 0.0,
    unload_distance: 0.0,
    ..MapStream::new(floors)
  };
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  let point = |x: f32, y: f32| Rect::new(x, y, 0.0, 0.0);
  // The origin, where four chunks meet, and a point on the line between
  // chunks -2 and -1: nothing has landed, so neither is ready
  let (origin, west) = (point(0.0, 0.0), point(-CHUNK, 0.0));
  assert!(!MapsView::new(&mut maps.world).stream_ready(map, origin));
  assert!(!MapsView::new(&mut maps.world).stream_ready(map, west));
  // A line with no width along a chunk boundary, and none with no height
  let (down, across) = (
    Rect::new(-CHUNK, 10.0, 0.0, 100.0),
    Rect::new(10.0, 0.0, 100.0, 0.0),
  );
  assert!(!MapsView::new(&mut maps.world).stream_ready(map, down));
  assert!(!MapsView::new(&mut maps.world).stream_ready(map, across));

  // Chunk (0, 0) holds the origin and the line across
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  assert_eq!(resident(&mut maps.world, map), [IVec2::ZERO]);
  let view = MapsView::new(&mut maps.world);
  assert!(view.stream_ready(map, origin));
  assert!(view.stream_ready(map, across));
  assert!(!view.stream_ready(map, west));
  assert!(!view.stream_ready(map, down));

  // Chunk (-1, 0) holds the point at x = -512 and the line down it
  move_to(&mut maps.world, anchor, IVec2::new(-1, 0));
  until_idle(&mut maps, map);
  assert_eq!(resident(&mut maps.world, map), [IVec2::new(-1, 0)]);
  let view = MapsView::new(&mut maps.world);
  assert!(view.stream_ready(map, west));
  assert!(view.stream_ready(map, down));
  assert!(!view.stream_ready(map, origin));

  // An anchor exactly on the boundary loads the chunk holding it
  match maps.world.get_mut::<Transform>(anchor) {
    Some(mut at) => at.translation = Vec2::ZERO,
    None => panic!("{anchor} is an anchor"),
  }
  until_idle(&mut maps, map);
  assert_eq!(resident(&mut maps.world, map), [IVec2::ZERO]);
}

/// The chunks under `area` (world pixels, the map at the origin), by row
fn chunks_under(area: Rect) -> Vec<IVec2> {
  let first = (Vec2::new(area.x, area.y) / CHUNK).floor().as_ivec2();
  let last =
    ((Vec2::new(area.x + area.width, area.y + area.height) / CHUNK).ceil() - 1.0).as_ivec2();
  let chunks = (first.y..=last.y)
    .flat_map(|y| (first.x..=last.x).map(move |x| IVec2::new(x, y)))
    .collect();
  sorted(chunks)
}

#[test]
fn a_camera_far_out_loads_what_parallax_and_offset_layers_show() {
  let mut maps = MapWorld::new();
  // A background following half the camera's motion, the ground, and a
  // front layer drawn 2,048 px right of its cells (past the load distance)
  let mut map = infinite(&["background", "ground", "front"]);
  map.layers[0].info.parallax = Vec2::splat(0.5);
  map.layers[2].info.offset = Vec2::new(2048.0, 0.0);
  let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
  let root = maps
    .world
    .spawn((MapRoot::new(handle), MapStream::new(floors)))
    .id();
  maps.settle(root);
  // A 640 x 360 camera 20,000 px from the parallax origin, (0, 0)
  let (camera, center) = (Camera2d::default(), Vec2::new(20_000.0, 0.0));
  let view = CameraView::compute(center, 0.0, &camera, Vec2::ZERO, Vec2::new(640.0, 360.0));
  maps
    .world
    .spawn((camera, view, Transform::from_translation(center)));
  until_idle(&mut maps, root);

  let bounds = view.bounds;
  let shown = |moved: Vec2| {
    Rect::new(
      bounds.left - moved.x,
      bounds.top - moved.y,
      bounds.width(),
      bounds.height(),
    )
  };
  // Each layer's cells on screen: the ground's under the view; the
  // background's half the camera's way back towards the origin; the
  // front's 2,048 px left
  let (ground, background, front) = (
    shown(Vec2::ZERO),
    shown(center * 0.5),
    shown(Vec2::new(2048.0, 0.0)),
  );
  let chunks = resident(&mut maps.world, root);
  for (layer, area) in [
    ("ground", ground),
    ("background", background),
    ("front", front),
  ] {
    assert!(
      MapsView::new(&mut maps.world).stream_ready(root, area),
      "the {layer}'s cells on screen ({area:?}) have landed"
    );
    for chunk in chunks_under(area) {
      assert!(
        chunks.contains(&chunk),
        "the {layer}'s chunk {chunk} is resident"
      );
    }
  }
  // Nothing between the background's chunks and the camera's: a union of
  // areas, not a span
  assert!(!chunks.contains(&IVec2::new(28, 0)), "{chunks:?}");
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
  // Every chunk left behind unloads in the one frame; the chunks round the
  // anchor's new place are held back, so none lands in it
  let open = Arc::new(AtomicBool::new(true));
  let stream = MapStream {
    unload_per_frame: 9,
    ..MapStream::new(floors_while(&open)).with_saved(saved.clone())
  };
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |r| r);
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  // The anchor leaves, and a cell is edited before the stream next looks
  open.store(false, Ordering::SeqCst);
  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", IVec2::new(8, 8), Some(BLOCK)));
  maps.frame();
  open.store(true, Ordering::SeqCst);
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
fn saved_chunks_round_trip_through_a_format_that_needs_lengths() {
  // bincode, like postcard, writes a map's length before its entries
  let mut tiles = ChunkTiles::default();
  tiles.set("ground", UVec2::new(0, 0), Some(BLOCK));
  tiles.set("items", UVec2::new(3, 3), Some(COIN));
  // A layer emptied again is left out, and the length counts only the rest
  tiles.set("gone", UVec2::new(1, 1), Some(BLOCK));
  tiles.set("gone", UVec2::new(1, 1), None);
  let saved = SavedChunks::default();
  saved.insert(IVec2::new(-3, 7), tiles.clone());
  saved.insert(IVec2::ZERO, ChunkTiles::default());

  let bytes = match bincode::serialize(&saved) {
    Ok(bytes) => bytes,
    Err(error) => panic!("SavedChunks serializes with bincode: {error}"),
  };
  let back: SavedChunks = match bincode::deserialize(&bytes) {
    Ok(back) => back,
    Err(error) => panic!("SavedChunks deserializes with bincode: {error}"),
  };
  assert_eq!(back.keys(), [IVec2::ZERO, IVec2::new(-3, 7)]);
  assert_eq!(back.get(IVec2::new(-3, 7)), Some(tiles));
  assert_eq!(back.get(IVec2::ZERO), Some(ChunkTiles::default()));
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

/// A generator laying `floors` with a coin at (3, 3) of every chunk
fn a_coin_a_chunk(request: ChunkRequest) -> ChunkTiles {
  let mut tiles = floors(request);
  tiles.set("ground", UVec2::new(3, 3), Some(COIN));
  tiles
}

/// A block built in chunk (0, 0)
const BUILT: IVec2 = IVec2::new(6, 9);

/// A streamed map round chunk (0, 0), its coins class tiles, keeping its
/// changed chunks in `saved`, with a block built in chunk (0, 0) (`BUILT`)
/// and nothing unloaded yet
fn edited_world(saved: &SavedChunks) -> (MapWorld, Entity) {
  let mut maps = MapWorld::new();
  let stream = MapStream::new(a_coin_a_chunk).with_saved(saved.clone());
  let map = spawn(&mut maps, infinite(&["ground"]), stream, |root| {
    root.with_entity_tiles(&["coin"])
  });
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  maps.frame();
  assert_eq!(class_tiles(&mut maps.world).len(), 9, "a coin a chunk");
  assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", BUILT, Some(BLOCK)));
  assert!(saved.is_empty());
  (maps, map)
}

/// Asserts that `saved` holds chunk (0, 0) alone, as the player left it:
/// the block built, its floor and its coin
fn kept_as_left(saved: &SavedChunks) {
  assert_eq!(saved.keys(), [IVec2::ZERO], "the changed chunk alone");
  let Some(tiles) = saved.get(IVec2::ZERO) else {
    panic!("chunk (0, 0) is kept");
  };
  assert_eq!(tiles.get("ground", BUILT.as_uvec2()), Some(BLOCK));
  assert_eq!(tiles.get("ground", UVec2::new(5, 0)), Some(BLOCK));
  assert_eq!(tiles.get("ground", UVec2::new(3, 3)), Some(COIN));
}

#[test]
fn a_despawned_streamed_map_keeps_its_changed_chunks() {
  let saved = SavedChunks::default();
  let (mut maps, map) = edited_world(&saved);
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  events(&maps.world, &mut cursor);
  assert!(maps.world.despawn(map));
  kept_as_left(&saved);
  maps.frame();
  // No chunk tells of its unloading as the map goes
  assert!(changes(&events(&maps.world, &mut cursor), map).is_empty());
  kept_as_left(&saved);
}

#[test]
fn leaving_the_scene_keeps_a_streamed_map_s_changed_chunks() {
  // A scene's world cleanup despawns every entity, the map's layers and
  // class tiles among them, in the order the world lists them
  let saved = SavedChunks::default();
  let (mut maps, _) = edited_world(&saved);
  despawn_game_entities(&mut maps.world);
  maps.frame();
  kept_as_left(&saved);
}

#[test]
fn a_streamed_map_put_over_keeps_its_changed_chunks() {
  let saved = SavedChunks::default();
  let (mut maps, map) = edited_world(&saved);
  let other = maps
    .world
    .resource_mut::<Assets<TileMap>>()
    .add(infinite(&["ground"]));
  maps.world.entity_mut(map).insert(MapRoot::new(other));
  kept_as_left(&saved);
  // The new map streams the kept chunk back
  maps.settle(map);
  until_idle(&mut maps, map);
  let view = MapsView::new(&mut maps.world);
  assert_eq!(view.tile(map, "ground", BUILT), Some(BLOCK));
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
fn the_stream_status_counts_the_chunks_resident_in_flight_and_draining() {
  // The generator waits until the test lets it go (5 s at most), so the
  // chunks asked for stay in flight
  let open = Arc::new(AtomicBool::new(false));
  let gate = Arc::clone(&open);
  let held = move |request: ChunkRequest| {
    let start = std::time::Instant::now();
    while !gate.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(5) {
      std::thread::sleep(Duration::from_millis(1));
    }
    floors(request)
  };
  // A hand-made chunk: a tile in the map's asset, far from the anchor
  let mut map = infinite(&["ground"]);
  if let Some(grid) = map.tiles_mut(0) {
    grid.set(IVec2::new(32 * 10 + 4, 4), Some(BLOCK));
  }
  let mut maps = MapWorld::new();
  let map = spawn(&mut maps, map, MapStream::new(held), |r| r);
  let status = |world: &mut World| MapsView::new(world).stream_status(map);

  // The nine chunks round a new anchor are asked for in its first frame
  // and stay in flight while the generator waits
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  maps.frame();
  assert_eq!(
    status(&mut maps.world),
    Some(StreamStatus {
      resident: 1,
      generated: 0,
      hand_made: 1,
      in_flight: 9,
      draining: 0,
    })
  );

  // Let go, they land, and none is in flight once it settles
  open.store(true, Ordering::SeqCst);
  until_idle(&mut maps, map);
  let chunks = resident(&mut maps.world, map);
  assert_eq!(
    status(&mut maps.world),
    Some(StreamStatus {
      resident: chunks.len(),
      generated: 9,
      hand_made: 1,
      in_flight: 0,
      draining: 0,
    })
  );

  // A teleport: two of the nine left behind unload in its first frame, the
  // others drain, and the new chunks are in flight, held back so none
  // lands in it
  open.store(false, Ordering::SeqCst);
  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  maps.frame();
  let Some(moving) = status(&mut maps.world) else {
    panic!("{map} streams");
  };
  open.store(true, Ordering::SeqCst);
  assert_eq!(moving.draining, 7, "{moving:?}");
  assert_eq!(moving.in_flight, 9, "{moving:?}");
  until_idle(&mut maps, map);
  let chunks = resident(&mut maps.world, map);
  assert_eq!(
    status(&mut maps.world),
    Some(StreamStatus {
      resident: chunks.len(),
      generated: 9,
      hand_made: 1,
      in_flight: 0,
      draining: 0,
    })
  );

  // A map with no stream has no status
  let plain = maps
    .world
    .resource_mut::<Assets<TileMap>>()
    .add(infinite(&["ground"]));
  let plain = maps.world.spawn(MapRoot::new(plain)).id();
  maps.settle(plain);
  assert_eq!(MapsView::new(&mut maps.world).stream_status(plain), None);
}

#[test]
fn saved_chunks_say_which_chunks_they_keep() {
  let saved = SavedChunks::default();
  assert!(saved.keys().is_empty());
  for chunk in [IVec2::new(3, 1), IVec2::new(-2, 1), IVec2::new(5, -4)] {
    saved.insert(chunk, ChunkTiles::default());
  }
  assert!(saved.contains(IVec2::new(-2, 1)));
  assert!(!saved.contains(IVec2::new(0, 0)));
  // By row, then column
  assert_eq!(
    saved.keys(),
    vec![IVec2::new(5, -4), IVec2::new(-2, 1), IVec2::new(3, 1)]
  );
  // A clone is the same store
  saved.clone().remove(IVec2::new(3, 1));
  assert!(!saved.contains(IVec2::new(3, 1)));
  assert_eq!(saved.keys().len(), 2);
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

/// The Tiled infinite fixture (saved by Tiled), and its tileset
const INFINITE: &str = "tiled/maps/infinite.tmj";
const TILESET: &str = "tiled/tilesets/tiles.tsj";

/// The fixture's tile layers
const LAYERS: [&str; 3] = ["ground", "walls", "decor"];

/// The engine chunks the fixture's Tiled chunks touch, by row: ground's at
/// cells (-32, -16), (-16, -16), (0, 0), (32, 0) and (-32, 16), walls' at
/// (16, -32) and (-64, 16), and decor's at (0, 0), each 16 cells a side
const FILE_CHUNKS: [IVec2; 6] = [
  IVec2::new(-1, -1),
  IVec2::new(0, -1),
  IVec2::new(-2, 0),
  IVec2::new(-1, 0),
  IVec2::new(0, 0),
  IVec2::new(1, 0),
];

/// The chunks within 512 px of the middle of chunk (0, 0) that the file
/// never touched
const GENERATED: [IVec2; 4] = [
  IVec2::new(1, -1),
  IVec2::new(-1, 1),
  IVec2::new(0, 1),
  IVec2::new(1, 1),
];

/// What the generator lays: tile 1 of the fixture's tileset
const BRICK: Tile = Tile {
  tileset: 0,
  id: 1,
  flip: TileFlip::NONE,
};

/// A generator filling every cell of "ground" with bricks, noting each
/// chunk it is asked for in `asked`
fn bricks(asked: Arc<Mutex<Vec<IVec2>>>) -> impl Fn(ChunkRequest) -> ChunkTiles + Send + Sync {
  move |request: ChunkRequest| {
    asked.lock().expect("not poisoned").push(request.chunk);
    let mut tiles = ChunkTiles::default();
    for y in 0..32 {
      for x in 0..32 {
        tiles.set("ground", UVec2::new(x, y), Some(BRICK));
      }
    }
    tiles
  }
}

/// The chunks the generator was asked for, by row
fn asked_for(asked: &Mutex<Vec<IVec2>>) -> Vec<IVec2> {
  sorted(asked.lock().expect("not poisoned").clone())
}

/// Spawns the map at `path`, streamed by `stream` with the cameras left
/// out, and lets it land: its root and its asset
fn spawn_file(maps: &mut MapWorld, path: &str, stream: MapStream) -> (Entity, Handle<TileMap>) {
  let handle = load_map(&mut maps.world, path);
  let stream = MapStream {
    cameras: false,
    ..stream
  };
  let map = maps
    .world
    .spawn((MapRoot::new(handle.clone()), stream))
    .id();
  maps.settle(map);
  (map, handle)
}

/// The cells of `chunk`, row by row
fn cells_of(chunk: IVec2) -> impl Iterator<Item = IVec2> {
  let first = chunk * 32;
  (0..32).flat_map(move |y| (0..32).map(move |x| first + IVec2::new(x, y)))
}

/// The tiles of `layer` in `chunk` of the spawned map `map`, row by row
fn tiles_in(world: &mut World, map: Entity, layer: &str, chunk: IVec2) -> Vec<Option<Tile>> {
  let view = MapsView::new(world);
  cells_of(chunk)
    .map(|cell| view.tile(map, layer, cell))
    .collect()
}

/// Whether every cell of "ground" in `chunk` holds a brick
fn bricked(world: &mut World, map: Entity, chunk: IVec2) -> bool {
  tiles_in(world, map, "ground", chunk)
    .iter()
    .all(|tile| *tile == Some(BRICK))
}

/// The tiles of `layer` in `chunk` as the file `file` has them
fn file_tiles(
  world: &World,
  file: &Handle<TileMap>,
  layer: &str,
  chunk: IVec2,
) -> Vec<Option<Tile>> {
  let assets = world.resource::<Assets<TileMap>>();
  let grid = assets
    .get(file)
    .and_then(|map| map.tiles(map.layer_index(layer)?));
  let Some(grid) = grid else {
    panic!("the file has no tile layer {layer}");
  };
  cells_of(chunk).map(|cell| grid.get(cell)).collect()
}

/// `chunk`'s world rectangle (the map sits at the origin)
fn rect_of(chunk: IVec2) -> Rect {
  let at = chunk.as_vec2() * CHUNK;
  Rect::new(at.x, at.y, CHUNK, CHUNK)
}

/// Each of `chunks` that a tile layer of `map` holds, with its version
fn versions(world: &mut World, map: Entity, chunks: &[IVec2]) -> Vec<(Entity, IVec2, u64)> {
  let mut found = Vec::new();
  for layer in layers_of(world, map) {
    let Some(data) = world.get::<TileLayerData>(layer) else {
      continue;
    };
    for &chunk in chunks {
      if data.tiles.has_chunk(chunk) {
        found.push((layer, chunk, data.tiles.chunk_version(chunk)));
      }
    }
  }
  found
}

/// The infinite fixture as Tiled saved it, changed by `edit` (its JSON)
fn infinite_saved_as(edit: impl FnOnce(&mut serde_json::Value)) -> String {
  let path = format!("{FIXTURES}/{INFINITE}");
  let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
  let mut json: serde_json::Value =
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
  edit(&mut json);
  json.to_string()
}

/// The Tiled chunks of the fixture's layer `layer`, as JSON
fn chunks_of(json: &mut serde_json::Value, layer: usize) -> &mut Vec<serde_json::Value> {
  match json["layers"][layer]["chunks"].as_array_mut() {
    Some(chunks) => chunks,
    None => panic!("layer {layer} has no chunks"),
  }
}

/// Whether the Tiled chunk `chunk` (JSON) starts at cell `at`
fn starts_at(chunk: &serde_json::Value, (x, y): (i64, i64)) -> bool {
  (chunk["x"].as_i64(), chunk["y"].as_i64()) == (Some(x), Some(y))
}

/// Runs frames until every one of `maps_reloaded` reloads, reading the map
/// messages from `cursor`
fn until_reloaded(
  maps: &mut MapWorld,
  maps_reloaded: &[Entity],
  cursor: &mut MessageCursor<MapEvent>,
) {
  let mut seen = Vec::new();
  maps.frames_until(|world| {
    if let Some(messages) = world.get_resource::<Messages<MapEvent>>() {
      seen.extend(cursor.read(messages).filter_map(|event| match event {
        MapEvent::Reloaded { map } => Some(*map),
        _ => None,
      }));
    }
    maps_reloaded.iter().all(|map| seen.contains(map))
  });
}

#[test]
fn a_tiled_infinite_map_streams_around_its_hand_made_chunks() {
  let asked = Arc::default();
  let mut maps = MapWorld::new();
  let stream = MapStream::new(bricks(Arc::clone(&asked)));
  let (map, file) = spawn_file(&mut maps, INFINITE, stream);
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  // Every chunk the file touches holds the file's tiles, its empty cells
  // empty: ground's in chunk (0, -1), which only walls' Tiled chunk
  // touches, too
  for chunk in FILE_CHUNKS {
    for layer in LAYERS {
      let file = file_tiles(&maps.world, &file, layer, chunk);
      assert_eq!(
        tiles_in(&mut maps.world, map, layer, chunk),
        file,
        "{layer} in chunk {chunk}"
      );
    }
  }
  // The chunks round (0, 0) the file never touched are the generator's,
  // and it was asked for those alone
  for chunk in GENERATED {
    assert!(bricked(&mut maps.world, map, chunk), "chunk {chunk}");
  }
  assert_eq!(asked_for(&asked), sorted(GENERATED.to_vec()));
}

#[test]
fn the_file_s_chunks_stay_resident_when_the_anchor_leaves() {
  let mut maps = MapWorld::new();
  let stream = MapStream::new(bricks(Arc::default()));
  let (map, file) = spawn_file(&mut maps, INFINITE, stream);
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  generated_tracked(&maps.world, map);
  move_to(&mut maps.world, anchor, IVec2::new(40, 40));
  until_idle(&mut maps, map);
  generated_tracked(&maps.world, map);
  // The generated chunks went; the file's stayed, as the file has them
  for chunk in GENERATED {
    assert!(!MapsView::new(&mut maps.world).stream_ready(map, rect_of(chunk)));
  }
  for chunk in FILE_CHUNKS {
    assert!(MapsView::new(&mut maps.world).stream_ready(map, rect_of(chunk)));
    for layer in LAYERS {
      let file = file_tiles(&maps.world, &file, layer, chunk);
      assert_eq!(tiles_in(&mut maps.world, map, layer, chunk), file);
    }
  }
  // and they take edits, however far the anchor is
  let mut view = MapsView::new(&mut maps.world);
  assert!(view.set_tile(map, "ground", IVec2::new(-60, 20), Some(BRICK)));
}

#[test]
fn saving_a_streamed_tiled_map_keeps_the_generated_chunks() {
  let files = files_with(&[]);
  let mut maps = MapWorld::over(files.clone());
  let (asked, saved) = (Arc::default(), SavedChunks::default());
  let stream = MapStream::new(bricks(Arc::clone(&asked))).with_saved(saved.clone());
  let (map, _) = spawn_file(&mut maps, INFINITE, stream);
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  let before = versions(&mut maps.world, map, &GENERATED);
  assert_eq!(before.len(), 12, "every layer holds the generated chunks");

  // A save changing ground's tile at (5, 2), in the file's chunk (0, 0)
  let mut cursor = MessageCursor::<MapEvent>::default();
  let changed = infinite_saved_as(|json| {
    let Some(chunk) = chunks_of(json, 0).iter_mut().find(|c| starts_at(c, (0, 0))) else {
      panic!("ground has a chunk at (0, 0)");
    };
    chunk["data"][2 * 16 + 5] = serde_json::json!(5);
  });
  files.insert(INFINITE, changed);
  until_reloaded(&mut maps, &[map], &mut cursor);

  // The changed chunk follows the file; the generated ones are untouched,
  // their versions too, and ready
  let view = MapsView::new(&mut maps.world);
  assert_eq!(
    view.tile(map, "ground", IVec2::new(5, 2)),
    Some(Tile::new(0, 4))
  );
  assert_eq!(versions(&mut maps.world, map, &GENERATED), before);
  for chunk in GENERATED {
    assert!(bricked(&mut maps.world, map, chunk), "chunk {chunk}");
    assert!(MapsView::new(&mut maps.world).stream_ready(map, rect_of(chunk)));
  }
  // The stream keeps going, and the generated chunks unload as they
  // landed: none is saved
  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  until_idle(&mut maps, map);
  assert!(bricked(&mut maps.world, map, IVec2::new(20, 0)));
  assert!(saved.is_empty(), "{saved:?}");
  let asked = asked_for(&asked);
  assert!(!FILE_CHUNKS.iter().any(|c| asked.contains(c)), "{asked:?}");
}

#[test]
fn a_tileset_save_keeps_the_generated_chunks_of_streamed_maps() {
  let files = files_with(&[]);
  let mut maps = MapWorld::over(files.clone());
  // The Tiled map, and an endless map built in code over its tileset file
  let (tiled_saved, built_saved) = (SavedChunks::default(), SavedChunks::default());
  let stream = MapStream::new(bricks(Arc::default())).with_saved(tiled_saved.clone());
  let (tiled, _) = spawn_file(&mut maps, INFINITE, stream);
  let tileset = match AssetsView::new(&mut maps.world).load_now::<Tileset>(TILESET) {
    Ok(tileset) => tileset,
    Err(error) => panic!("{TILESET}: {}", error.report()),
  };
  let mut built = TileMap::infinite(UVec2::splat(16));
  built.add_tileset(TilesetSource::Asset(tileset));
  built.add_tile_layer("ground");
  let stream = MapStream::new(bricks(Arc::default())).with_saved(built_saved.clone());
  let built = spawn(&mut maps, built, stream, |r| r);
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, tiled);
  until_idle(&mut maps, built);
  let tiled_before = versions(&mut maps.world, tiled, &GENERATED);
  let built_before = versions(&mut maps.world, built, &around(IVec2::ZERO, 1));
  assert_eq!((tiled_before.len(), built_before.len()), (12, 9));

  // The tileset saved with a tile's property changed: both maps reload,
  // and only their tilesets change
  let mut cursor = MessageCursor::<MapEvent>::default();
  let path = format!("{FIXTURES}/{TILESET}");
  let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
  files.insert(TILESET, text.replace("a brick", "a red brick"));
  until_reloaded(&mut maps, &[tiled, built], &mut cursor);
  assert_eq!(versions(&mut maps.world, tiled, &GENERATED), tiled_before);
  assert_eq!(
    versions(&mut maps.world, built, &around(IVec2::ZERO, 1)),
    built_before
  );
  let area = Rect::new(-CHUNK, -CHUNK, 3.0 * CHUNK, 3.0 * CHUNK);
  for map in [tiled, built] {
    assert!(MapsView::new(&mut maps.world).stream_ready(map, area));
    assert!(bricked(&mut maps.world, map, IVec2::new(1, 1)));
  }
  assert!(bricked(&mut maps.world, built, IVec2::ZERO));

  // Unloaded, the generated chunks are not saved as changed
  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  until_idle(&mut maps, tiled);
  until_idle(&mut maps, built);
  assert!(tiled_saved.is_empty(), "{tiled_saved:?}");
  assert!(built_saved.is_empty(), "{built_saved:?}");
}

#[test]
fn an_edit_in_a_file_chunk_is_kept_for_a_save_game() {
  let saved = SavedChunks::default();
  let mut maps = MapWorld::new();
  let stream = MapStream::new(bricks(Arc::default())).with_saved(saved.clone());
  let (map, _) = spawn_file(&mut maps, INFINITE, stream);
  anchor_at(&mut maps, IVec2::new(40, 0));
  until_idle(&mut maps, map);
  // Walls' tile at (-50, 20), in the file's chunk (-2, 0) far from the
  // anchor, dug out
  let dug = IVec2::new(-50, 20);
  let mut view = MapsView::new(&mut maps.world);
  assert_eq!(view.tile(map, "walls", dug), Some(Tile::new(0, 7)));
  assert!(view.set_tile(map, "walls", dug, None));
  // The file's chunks never unload, so only saving the changed chunks
  // keeps it: the dug chunk alone
  assert!(saved.is_empty());
  assert_eq!(view.save_changed_chunks(map), 1);
  assert!(saved.get(IVec2::new(-2, 0)).is_some());

  // A new map of the file, streamed with the save, has the tile dug and
  // the rest of the file as it was
  let mut again = MapWorld::new();
  let stream = MapStream::new(bricks(Arc::default())).with_saved(saved);
  let (map, _) = spawn_file(&mut again, INFINITE, stream);
  let view = MapsView::new(&mut again.world);
  assert_eq!(view.tile(map, "walls", dug), None);
  assert_eq!(
    view.tile(map, "walls", IVec2::new(17, -17)),
    Some(Tile::new(0, 8))
  );
  assert_eq!(
    view.tile(map, "ground", IVec2::new(-17, 18)),
    Some(Tile::new(0, 6).with_flip(TileFlip::VERTICAL))
  );
}

#[test]
fn every_engine_chunk_a_tiled_chunk_touches_is_the_file_s() {
  // Tiled's 24-cell chunks straddle the engine's: ground's at (-48, -48)
  // holds tiles in chunks (-2, -1) and (-1, -1), and touches (-2, -2) and
  // (-1, -2) with none
  let asked = Arc::default();
  let mut maps = MapWorld::new();
  let stream = MapStream::new(bricks(Arc::clone(&asked)));
  let (map, _) = spawn_file(&mut maps, "tiled/maps/infinite-chunks24.tmj", stream);
  anchor_at(&mut maps, IVec2::new(-2, -2));
  until_idle(&mut maps, map);
  let generated = [(-3, -3), (-2, -3), (-1, -3), (-3, -2), (-3, -1)];
  let generated: Vec<IVec2> = generated.map(IVec2::from).to_vec();
  assert_eq!(asked_for(&asked), sorted(generated));
  for chunk in [IVec2::new(-2, -2), IVec2::new(-1, -2)] {
    assert_eq!(
      tiles_in(&mut maps.world, map, "ground", chunk),
      [None; 1024]
    );
    assert!(MapsView::new(&mut maps.world).stream_ready(map, rect_of(chunk)));
  }
}

#[test]
fn a_save_that_moves_the_file_s_chunks_hands_them_to_and_from_the_generator() {
  let files = files_with(&[]);
  let mut maps = MapWorld::over(files.clone());
  let asked = Arc::default();
  let stream = MapStream::new(bricks(Arc::clone(&asked)));
  let (map, _) = spawn_file(&mut maps, INFINITE, stream);
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  let mut chunk_cursor = MessageCursor::<ChunkEvent>::default();
  events(&maps.world, &mut chunk_cursor);

  // A save dropping ground's Tiled chunk at (32, 0), the file's only one
  // in chunk (1, 0), and adding one to walls at (32, 32), in the generated
  // chunk (1, 1)
  let mut cursor = MessageCursor::<MapEvent>::default();
  let changed = infinite_saved_as(|json| {
    chunks_of(json, 0).retain(|c| !starts_at(c, (32, 0)));
    let mut data = vec![0u32; 256];
    data[0] = 7;
    let chunk = serde_json::json!({"x": 32, "y": 32, "width": 16, "height": 16, "data": data});
    chunks_of(json, 1).push(chunk);
  });
  files.insert(INFINITE, changed);
  until_reloaded(&mut maps, &[map], &mut cursor);
  until_idle(&mut maps, map);

  // Chunk (1, 1) is the file's: the generator's left it, and it holds the
  // file's tile alone
  let sent = events(&maps.world, &mut chunk_cursor);
  assert_eq!(unloading(&sent, map), [IVec2::new(1, 1)]);
  assert_eq!(
    tiles_in(&mut maps.world, map, "ground", IVec2::new(1, 1)),
    [None; 1024]
  );
  let view = MapsView::new(&mut maps.world);
  assert_eq!(
    view.tile(map, "walls", IVec2::new(32, 32)),
    Some(Tile::new(0, 6))
  );
  // Chunk (1, 0) is the generator's now
  assert_eq!(loaded(&sent, map), [IVec2::new(1, 0)]);
  assert!(bricked(&mut maps.world, map, IVec2::new(1, 0)));
  assert!(asked_for(&asked).contains(&IVec2::new(1, 0)));
  // and only the generator's chunks are looked at to unload
  generated_tracked(&maps.world, map);
  let Some(state) = maps.world.get::<StreamState>(map) else {
    panic!("{map} streams");
  };
  assert!(state.generated.contains(&IVec2::new(1, 0)));
  assert!(!state.generated.contains(&IVec2::new(1, 1)));
}

#[test]
fn a_save_that_adds_a_tile_layer_streams_the_generated_chunks_into_it() {
  let files = files_with(&[]);
  let mut maps = MapWorld::over(files.clone());
  let asked = Arc::default();
  let stream = MapStream::new(bricks(Arc::clone(&asked)));
  let (map, _) = spawn_file(&mut maps, INFINITE, stream);
  anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  asked.lock().expect("not poisoned").clear();

  // A save adding a copy of decor, "extra", on top
  let mut cursor = MessageCursor::<MapEvent>::default();
  let changed = infinite_saved_as(|json| {
    let mut extra = json["layers"][2].clone();
    extra["id"] = serde_json::json!(5);
    extra["name"] = serde_json::json!("extra");
    if let Some(layers) = json["layers"].as_array_mut() {
      layers.insert(3, extra);
    }
    json["nextlayerid"] = serde_json::json!(6);
  });
  files.insert(INFINITE, changed);
  until_reloaded(&mut maps, &[map], &mut cursor);
  until_idle(&mut maps, map);

  // The generated chunks streamed again, into every tile layer
  assert_eq!(asked_for(&asked), sorted(GENERATED.to_vec()));
  assert_eq!(versions(&mut maps.world, map, &GENERATED).len(), 16);
  for chunk in GENERATED {
    assert!(bricked(&mut maps.world, map, chunk), "chunk {chunk}");
  }
}

#[test]
fn a_reload_leaves_a_class_tile_taken_from_a_generated_chunk_taken() {
  let files = files_with(&[]);
  let mut maps = MapWorld::over(files.clone());
  let saved = SavedChunks::default();
  let tileset = match AssetsView::new(&mut maps.world).load_now::<Tileset>(TILESET) {
    Ok(tileset) => tileset,
    Err(error) => panic!("{TILESET}: {}", error.report()),
  };
  let mut map = TileMap::infinite(UVec2::splat(16));
  map.add_tileset(TilesetSource::Asset(tileset));
  map.add_tile_layer("ground");
  // A brick, of the class "brick", at (3, 3) of chunk (0, 0)
  let generator = |request: ChunkRequest| {
    let mut tiles = ChunkTiles::default();
    if request.chunk == IVec2::ZERO {
      tiles.set("ground", UVec2::new(3, 3), Some(BRICK));
    }
    tiles
  };
  let stream = MapStream::new(generator).with_saved(saved.clone());
  let map = spawn(&mut maps, map, stream, |root| {
    root.with_entity_tiles(&["brick"])
  });
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  until_idle(&mut maps, map);
  maps.frame();
  // The game takes the brick, then the tileset is saved
  let taken = class_tiles_at(&mut maps.world, IVec2::new(3, 3));
  assert_eq!(taken.len(), 1);
  for brick in taken {
    assert!(maps.world.despawn(brick));
  }
  let mut cursor = MessageCursor::<MapEvent>::default();
  let path = format!("{FIXTURES}/{TILESET}");
  let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
  files.insert(TILESET, text.replace("a brick", "a red brick"));
  until_reloaded(&mut maps, &[map], &mut cursor);

  // The chunk still counts as changed: kept as it unloads, the brick gone,
  // and back so
  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  until_idle(&mut maps, map);
  let kept = saved.get(IVec2::ZERO);
  assert_eq!(kept.map(|tiles| tiles.is_empty()), Some(true));
  move_to(&mut maps.world, anchor, IVec2::ZERO);
  until_idle(&mut maps, map);
  maps.frame();
  assert!(class_tiles_at(&mut maps.world, IVec2::new(3, 3)).is_empty());
}

/// What a streamed map's hand-made chunks cost its frames: a map built in
/// code holding a tile in each of 64 x 64 chunks (its asset's chunks, so
/// the file's, which never unload), streamed round an anchor far from
/// them, its idle `PreUpdate` (the stream's look at what to unload, ask
/// for and land) timed. Run in release:
/// `cargo test --release -p ryo2d --lib hand_made_chunks_idle_timing -- --ignored --nocapture`
#[test]
#[ignore = "a timing: run in release with --ignored --nocapture"]
fn hand_made_chunks_idle_timing() {
  let mut maps = MapWorld::new();
  let mut map = infinite(&["ground"]);
  if let Some(grid) = map.tiles_mut(0) {
    for y in 0..64 {
      for x in 0..64 {
        grid.set(IVec2::new(x, y) * 32 + IVec2::splat(5), Some(COIN));
      }
    }
  }
  let map = spawn(&mut maps, map, MapStream::new(floors), |r| r);
  anchor_at(&mut maps, IVec2::new(-500, -500));
  until_idle(&mut maps, map);
  let mut times = Vec::new();
  for _ in 0..2000 {
    let start = std::time::Instant::now();
    maps.pre_update();
    times.push(start.elapsed());
  }
  times.sort();
  let resident = maps
    .world
    .get::<StreamState>(map)
    .map_or(0, |state| state.resident.len());
  println!(
    "a streamed map of 4,096 hand-made chunks, {resident} resident: an idle PreUpdate, median \
     {:?}, 99th percentile {:?}",
    times[times.len() / 2],
    times[times.len() * 99 / 100]
  );
}

/// Whether cell (`x`, `y`) is cave rock: smooth value noise at two scales,
/// high where about a quarter of the cells are (the streaming guard's)
fn rocky(x: i32, y: i32) -> bool {
  let hash = |x: i32, y: i32, salt: u32| {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
      ^ (y as u32).wrapping_mul(0xD816_3841)
      ^ salt.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^ (h >> 15)
  };
  let noise = |x: f32, y: f32, salt: u32| {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let at = |dx: i32, dy: i32| (hash(ix + dx, iy + dy, salt) & 0xFFFF) as f32 / 65535.0;
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
    top + (bottom - top) * sy
  };
  let (fx, fy) = (x as f32, y as f32);
  noise(fx / 22.0, fy / 22.0, 1) * 0.7 + noise(fx / 7.0, fy / 7.0, 2) * 0.3 > 0.6
}

/// A cave tileset of 16 px tiles: 0 to 3 rock, each a full-cell shape, and
/// 4 to 11 background with none
fn cave_tiles() -> Tileset {
  let mut set = Tileset::new("cave", UVec2::splat(16), TilesetImage::Collection);
  set.tile_count = 12;
  for id in 0..4 {
    set.insert_tile(
      id,
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
  }
  set
}

/// A chunk of the endless cave: a background on every cell and rock on the
/// ground where `rocky` says, each tile picked by the cell
fn cave(request: ChunkRequest) -> ChunkTiles {
  let mut tiles = ChunkTiles::default();
  for y in 0..32 {
    for x in 0..32 {
      let at = UVec2::new(x, y);
      let cell = request.first_cell + at.as_ivec2();
      let pick = (cell.x.wrapping_mul(7) ^ cell.y.wrapping_mul(13)).rem_euclid(8) as u32;
      tiles.set("background", at, Some(Tile::new(0, 4 + pick)));
      if rocky(cell.x, cell.y) {
        tiles.set("ground", at, Some(Tile::new(0, pick % 4)));
      }
    }
  }
  tiles
}

/// What crossing an endless world costs a frame on the main thread, as the
/// streaming guard crosses it without drawing: a cave of two unbounded
/// layers (both colliding, the ground's rock casting shadows) streamed round
/// a 1920 x 1080 camera crossing it at 2,000 px/s heading (0.8, 0.6), each
/// frame paced so the generator keeps up. Prints each phase's median for
/// the frames landing or unloading chunks and for the others, and the cost
/// of a chunk landed or unloaded. Run in release:
/// `cargo test --release -p ryo2d --lib crossing_timing -- --ignored --nocapture`
/// (for a profile, `RYO_CROSSING_ROUNDS=40000 RYO_CROSSING_PACE_US=0` runs
/// a longer crossing back to back; `RYO_CROSSING_THRASH_MB=48` writes over
/// 48 MB between frames, evicting the caches as the guard's other engines
/// and drawing do)
#[test]
#[ignore = "a timing: run in release with --ignored --nocapture"]
fn crossing_timing() {
  use std::time::Instant;
  let mut maps = MapWorld::new();
  let mut map = TileMap::infinite(UVec2::splat(16));
  map.add_tileset(TilesetSource::Embedded(Arc::new(cave_tiles())));
  map.add_tile_layer("background");
  map.add_tile_layer("ground");
  let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
  let stream = MapStream {
    load_distance: 512.0,
    unload_distance: 1536.0,
    ..MapStream::new(cave)
  };
  let root = maps.world.spawn((MapRoot::new(handle), stream)).id();
  maps.settle(root);
  let camera = Camera2d::default();
  let (heading, speed) = (Vec2::new(0.8, 0.6), 2000.0 / 60.0);
  let rounds = std::env::var("RYO_CROSSING_ROUNDS")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(2400);
  let warm = 120;
  let start = heading * -((warm + rounds) as f32 / 2.0) * speed;
  let view_at = |frame: usize| {
    let center = start + heading * frame as f32 * speed;
    CameraView::compute(center, 0.0, &camera, Vec2::ZERO, Vec2::new(1920.0, 1080.0))
  };
  let eye = maps
    .world
    .spawn((camera, view_at(0), Transform::from_translation(start)))
    .id();
  until_idle(&mut maps, root);
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  events(&maps.world, &mut cursor);
  // Each frame's phases (stream, fixed step, PostUpdate and Last) in µs,
  // and the chunks it landed and unloaded
  type Frame = ([f64; 4], usize, usize);
  let mut frames: Vec<Frame> = Vec::new();
  let pace = Duration::from_micros(
    std::env::var("RYO_CROSSING_PACE_US")
      .ok()
      .and_then(|v| v.parse().ok())
      .unwrap_or(4000),
  );
  // What a frame's drawing reads, evicting the caches as the guard's do
  let thrash: usize = std::env::var("RYO_CROSSING_THRASH_MB")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(0);
  let mut evict = vec![0u64; thrash * 131_072];
  let mut next = Instant::now();
  for frame in 0..warm + rounds {
    for (i, word) in evict.iter_mut().enumerate().step_by(8) {
      *word = word.wrapping_add(i as u64);
    }
    std::hint::black_box(&evict);
    while Instant::now() < next {
      std::hint::spin_loop();
    }
    next = Instant::now() + pace;
    if let Some(mut view) = maps.world.get_mut::<CameraView>(eye) {
      *view = view_at(frame);
    }
    let mut times = [0.0; 4];
    let t = Instant::now();
    maps.pre_update();
    times[0] = t.elapsed().as_secs_f64() * 1e6;
    let t = Instant::now();
    maps.step();
    times[1] = t.elapsed().as_secs_f64() * 1e6;
    let t = Instant::now();
    maps.post_update();
    times[2] = t.elapsed().as_secs_f64() * 1e6;
    let t = Instant::now();
    maps.last();
    times[3] = t.elapsed().as_secs_f64() * 1e6;
    let happened = events(&maps.world, &mut cursor);
    let landed = loaded(&happened, root).len();
    let unloaded = unloading(&happened, root).len();
    if frame >= warm {
      frames.push((times, landed, unloaded));
    }
  }
  let median = |mut v: Vec<f64>| {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
  };
  let percentile = |mut v: Vec<f64>, p: usize| {
    v.sort_by(f64::total_cmp);
    v.get((v.len() * p / 100).min(v.len().saturating_sub(1)))
      .copied()
      .unwrap_or(0.0)
  };
  let busy = |f: &&Frame| f.1 + f.2 > 0;
  let (active, idle): (Vec<_>, Vec<_>) = frames.iter().partition(busy);
  let phase = |set: &[&Frame], i: usize| -> Vec<f64> { set.iter().map(|f| f.0[i]).collect() };
  let total = |set: &[&Frame]| -> Vec<f64> { set.iter().map(|f| f.0.iter().sum()).collect() };
  let ops: usize = frames.iter().map(|f| f.1 + f.2).sum();
  let landed: usize = frames.iter().map(|f| f.1).sum();
  let idle_total = median(total(&idle));
  let added: f64 = active
    .iter()
    .map(|f| f.0.iter().sum::<f64>() - idle_total)
    .sum();
  println!(
    "crossing an endless cave at 2,000 px/s, {rounds} frames: {landed} chunks landed and {} \
     unloaded, in {} frames",
    ops - landed,
    active.len()
  );
  for (name, set) in [("busy", &active), ("idle", &idle)] {
    println!(
      "  {name} frames ({}): stream {:.1} µs, fixed step {:.1}, PostUpdate {:.1}, Last {:.1}; \
       whole median {:.1}, 99th percentile {:.1}, max {:.1}",
      set.len(),
      median(phase(set, 0)),
      median(phase(set, 1)),
      median(phase(set, 2)),
      median(phase(set, 3)),
      median(total(set)),
      percentile(total(set), 99),
      percentile(total(set), 100),
    );
  }
  for (name, pick) in [
    (
      "landing only",
      (|f: &Frame| f.1 > 0 && f.2 == 0) as fn(&_) -> bool,
    ),
    ("unloading only", |f| f.1 == 0 && f.2 > 0),
    ("both", |f| f.1 > 0 && f.2 > 0),
  ] {
    let set: Vec<_> = frames.iter().filter(|f| pick(f)).collect();
    let each: usize = set.iter().map(|f| f.1 + f.2).sum();
    println!(
      "  {name} ({} frames, {each} chunks): stream {:.1} µs, fixed step {:.1}, PostUpdate \
       {:.1}; whole median {:.1}",
      set.len(),
      median(phase(&set, 0)),
      median(phase(&set, 1)),
      median(phase(&set, 2)),
      median(total(&set)),
    );
  }
  println!(
    "  a chunk landed or unloaded adds {:.1} µs on average",
    added / ops.max(1) as f64
  );
}

/// A jumble of 16 px tiles: 0 a solid block, 1 a coin, 2 a half block, 3 a
/// slope, 4 a one-way block, 5 a block that only casts a small shadow, 6 an
/// L, 7 a diamond touching each side at its middle, 8 a one-way slab, 9
/// plain decor
fn jumble_tiles() -> Tileset {
  let mut set = Tileset::new("jumble", UVec2::splat(16), TilesetImage::Collection);
  set.tile_count = 10;
  let shaped = |points: &[(f32, f32)], one_way: bool, occluder: bool| TileData {
    shapes: vec![TileShape {
      points: points.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
      one_way,
      occluder,
      ..TileShape::default()
    }],
    ..TileData::default()
  };
  let square = [(0.0, 0.0), (16.0, 0.0), (16.0, 16.0), (0.0, 16.0)];
  set.insert_tile(0, shaped(&square, false, false));
  set.insert_tile(
    1,
    TileData {
      class: Some("coin".to_string()),
      ..TileData::default()
    },
  );
  let half = [(0.0, 8.0), (16.0, 8.0), (16.0, 16.0), (0.0, 16.0)];
  set.insert_tile(2, shaped(&half, false, false));
  let slope = [(16.0, 0.0), (16.0, 16.0), (0.0, 16.0)];
  set.insert_tile(3, shaped(&slope, false, false));
  set.insert_tile(4, shaped(&square, true, false));
  let small = [(4.0, 2.0), (12.0, 2.0), (12.0, 14.0), (4.0, 14.0)];
  set.insert_tile(5, shaped(&small, false, true));
  let l = [
    (0.0, 0.0),
    (8.0, 0.0),
    (8.0, 8.0),
    (16.0, 8.0),
    (16.0, 16.0),
    (0.0, 16.0),
  ];
  set.insert_tile(6, shaped(&l, false, false));
  let diamond = [(8.0, 0.0), (16.0, 8.0), (8.0, 16.0), (0.0, 8.0)];
  set.insert_tile(7, shaped(&diamond, false, false));
  let slab = [(0.0, 4.0), (16.0, 4.0), (16.0, 8.0), (0.0, 8.0)];
  set.insert_tile(8, shaped(&slab, true, false));
  set
}

/// A hash of a cell, for the jumble's scatter
fn jumble_hash(cell: IVec2, salt: u32) -> u32 {
  let mut h = (cell.x as u32).wrapping_mul(0x8DA6_B343)
    ^ (cell.y as u32).wrapping_mul(0xD816_3841)
    ^ salt.wrapping_mul(0xCB1A_B31F);
  h ^= h >> 13;
  h = h.wrapping_mul(0x5BD1_E995);
  h ^ (h >> 15)
}

/// A random tile of the jumble (or none), most of them solid blocks
fn jumble_tile(h: u32) -> Option<Tile> {
  let id = match h % 32 {
    0..=13 => 0,
    14 => 1,
    15 | 16 => 2,
    17 | 18 => 3,
    19 => 4,
    20 => 5,
    21 => 6,
    22 => 7,
    23 => 8,
    24 => 9,
    _ => return None,
  };
  Some(Tile::new(0, id))
}

/// A chunk of a jumbled world: on the ground, rock in caves crossing chunk
/// borders with shapes, slopes, one-way tiles and coins scattered through
/// it; in front, a few tiles more
fn jumble(request: ChunkRequest) -> ChunkTiles {
  let mut tiles = ChunkTiles::default();
  for y in 0..32 {
    for x in 0..32 {
      let at = UVec2::new(x, y);
      let cell = request.first_cell + at.as_ivec2();
      let h = jumble_hash(cell, 7);
      let ground = if rocky(cell.x * 2, cell.y * 2) {
        if h.is_multiple_of(5) {
          jumble_tile(h >> 8)
        } else {
          Some(Tile::new(0, 0))
        }
      } else if h.is_multiple_of(9) {
        jumble_tile(h >> 8)
      } else {
        None
      };
      tiles.set("ground", at, ground);
      if jumble_hash(cell, 11).is_multiple_of(23) {
        tiles.set("front", at, jumble_tile(h >> 16));
      }
    }
  }
  tiles
}

/// Whole-pixel steps (from, to) and how many times each is there
type Steps = BTreeMap<((i32, i32), (i32, i32)), usize>;

/// Every edge of the map's tile parts (each in px, cut into whole-pixel
/// steps so edges compare however loops and packs split them), solid and
/// one-way apart, each with how many times it is there
fn part_steps(world: &mut World, map: Entity) -> [Steps; 2] {
  use crate::core::physics::{Collider, ColliderShape, TilePart, in_grid_space};
  let mut steps = [BTreeMap::new(), BTreeMap::new()];
  let mut query =
    world.query_filtered::<(&ChildOf, &Collider, &Transform), crate::core::ecs::With<TilePart>>();
  let rounded = |v: Vec2| {
    assert!(
      (v - v.round()).abs().max_element() < 1e-3,
      "{v} off the pixel grid"
    );
    (v.x.round() as i32, v.y.round() as i32)
  };
  for (child_of, collider, at) in query.iter(world) {
    if child_of.parent() != map {
      continue;
    }
    let collider = in_grid_space(collider.clone(), at.translation);
    let pairs: Vec<(Vec2, Vec2)> = match &collider.shape {
      ColliderShape::Polyline { points } => points.windows(2).map(|w| (w[0], w[1])).collect(),
      ColliderShape::Segments { points, segments } => segments
        .iter()
        .map(|&[a, b]| (points[a as usize], points[b as usize]))
        .collect(),
      ColliderShape::Segment { a, b } => vec![(*a, *b)],
      other => panic!("a tile part of {other:?}"),
    };
    let into = &mut steps[usize::from(collider.one_way.is_some())];
    for (a, b) in pairs {
      let (a, b) = (rounded(a), rounded(b));
      let (dx, dy) = (b.0 - a.0, b.1 - a.1);
      let gcd = |mut a: i32, mut b: i32| {
        (a, b) = (a.abs(), b.abs());
        while b != 0 {
          (a, b) = (b, a % b);
        }
        a.max(1)
      };
      let n = gcd(dx, dy);
      for i in 0..n {
        let from = (a.0 + dx * i / n, a.1 + dy * i / n);
        let to = (a.0 + dx * (i + 1) / n, a.1 + dy * (i + 1) / n);
        *into.entry((from, to)).or_insert(0) += 1;
      }
    }
  }
  steps
}

/// The map's shadows: each occluder's chunk, place and vertices, sorted
fn shadows(world: &mut World, map: Entity) -> Vec<(IVec2, Vec2, Vec<Vec2>)> {
  use crate::core::lighting::LightOccluder2D;
  use crate::core::tilemap::MapOccluder;
  let layers = layers_of(world, map);
  let mut found: Vec<(IVec2, Vec2, Vec<Vec2>)> = world
    .query::<(&ChildOf, &MapOccluder, &Transform, &LightOccluder2D)>()
    .iter(world)
    .filter(|(child_of, ..)| layers.contains(&child_of.parent()))
    .map(|(_, marker, at, occluder)| (marker.chunk, at.translation, occluder.vertices().to_vec()))
    .collect();
  let key = |v: &Vec2| (v.x.to_bits(), v.y.to_bits());
  found.sort_by(|a, b| {
    (
      a.0.y,
      a.0.x,
      key(&a.1),
      a.2.iter().map(key).collect::<Vec<_>>(),
    )
      .cmp(&(
        b.0.y,
        b.0.x,
        key(&b.1),
        b.2.iter().map(key).collect::<Vec<_>>(),
      ))
  });
  found
}

/// The map's class tiles: each one's layer, cell and tile, sorted
fn class_tiles_of(world: &mut World, map: Entity) -> Vec<(usize, IVec2, Tile)> {
  let mut found: Vec<(usize, IVec2, Tile)> = world
    .query::<&MapTile>()
    .iter(world)
    .filter(|tile| tile.map == map)
    .map(|tile| (tile.layer, tile.cell, tile.tile))
    .collect();
  found.sort_by_key(|&(layer, cell, tile)| (layer, cell.y, cell.x, tile.id));
  found
}

/// Asserts that the streamed map `map`'s collision, shadows and class tiles
/// are those of a map built whole from what it holds now (its grids' tiles
/// and its class tiles, put into an infinite map of the same layers) and
/// landed in a world of its own
fn assert_as_built_whole(maps: &mut MapWorld, map: Entity, at: usize) {
  let mut whole = TileMap::infinite(UVec2::splat(16));
  whole.add_tileset(TilesetSource::Embedded(Arc::new(jumble_tiles())));
  let layers = layers_of(&mut maps.world, map);
  for (index, &layer) in layers.iter().enumerate() {
    let name = maps
      .world
      .get::<MapLayerRef>(layer)
      .map(|of| of.name.clone());
    let added = whole.add_tile_layer(name.as_deref().unwrap_or("layer"));
    let cells: Vec<(IVec2, Tile)> = maps
      .world
      .get::<TileLayerData>(layer)
      .map(|data| {
        let grid = &data.tiles;
        grid
          .chunk_coords()
          .flat_map(|chunk| cells_of(chunk).filter_map(move |cell| Some((cell, grid.get(cell)?))))
          .collect()
      })
      .unwrap_or_default();
    let classes: Vec<(IVec2, Tile)> = class_tiles_of(&mut maps.world, map)
      .into_iter()
      .filter(|&(of, ..)| of == index)
      .map(|(_, cell, tile)| (cell, tile))
      .collect();
    if let Some(grid) = whole.tiles_mut(added) {
      for (cell, tile) in cells.into_iter().chain(classes) {
        grid.set(cell, Some(tile));
      }
    }
  }
  let mut apart = MapWorld::new();
  let handle = apart.world.resource_mut::<Assets<TileMap>>().add(whole);
  let fresh = apart
    .world
    .spawn(MapRoot::new(handle).with_entity_tiles(&["coin"]))
    .id();
  apart.settle(fresh);
  apart.frame();
  let (streamed, built) = (
    part_steps(&mut maps.world, map),
    part_steps(&mut apart.world, fresh),
  );
  assert!(
    streamed[0] == built[0],
    "step {at}: the collision outlines are a whole build's"
  );
  assert!(
    streamed[1] == built[1],
    "step {at}: the one-way runs are a whole build's"
  );
  let (streamed, built) = (
    shadows(&mut maps.world, map),
    shadows(&mut apart.world, fresh),
  );
  assert!(
    streamed == built,
    "step {at}: the shadows are a whole build's"
  );
  let (streamed, built) = (
    class_tiles_of(&mut maps.world, map),
    class_tiles_of(&mut apart.world, fresh),
  );
  assert_eq!(
    streamed, built,
    "step {at}: the class tiles are a whole build's"
  );
}

/// A streamed map's collision, shadows and class tiles after any sequence
/// of loads, unloads and edits are those of the map built whole from what
/// it then holds: an anchor jumping about a jumbled world (caves of rock
/// across chunk borders, shapes, slopes, one-way tiles, coins, two layers)
/// with edits at and near chunk sides, compared every ten frames. Twice:
/// with the default budgets, where a frame landing chunks rarely unloads,
/// and with one of each a frame over wider areas, where a jump leaves more
/// than eight chunks waiting and frames do both
#[test]
fn collision_shadows_and_class_tiles_match_a_whole_build_after_random_streaming() {
  random_streaming(MapStream {
    load_distance: 300.0,
    unload_distance: 600.0,
    ..MapStream::new(jumble)
  });
  let both = random_streaming(MapStream {
    load_distance: 600.0,
    unload_distance: 900.0,
    land_per_frame: 1,
    unload_per_frame: 1,
    ..MapStream::new(jumble)
  });
  assert!(both > 10, "{both} frames both landed and unloaded");
}

/// The jumbled world streamed by `stream` under a jumping anchor and random
/// edits, checked against a whole build every ten frames: how many frames
/// both landed and unloaded
fn random_streaming(stream: MapStream) -> usize {
  let mut maps = MapWorld::new();
  let mut map = TileMap::infinite(UVec2::splat(16));
  map.add_tileset(TilesetSource::Embedded(Arc::new(jumble_tiles())));
  map.add_tile_layer("ground");
  map.add_tile_layer("front");
  let map = spawn(&mut maps, map, stream, |root| {
    root.with_entity_tiles(&["coin"])
  });
  let anchor = anchor_at(&mut maps, IVec2::ZERO);
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  let (mut landed, mut unloaded, mut both) = (0, 0, 0);
  let mut seed = 0x2545_f491_u32;
  let mut next = |n: u32| {
    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (seed >> 8) % n
  };
  for step in 0..300 {
    // The anchor jumps about a region of 6 x 6 chunks, now and then
    if next(4) == 0 {
      let to = Vec2::new(next(6 * 512) as f32, next(6 * 512) as f32) - 3.0 * CHUNK;
      if let Some(mut at) = maps.world.get_mut::<Transform>(anchor) {
        at.translation = to;
      }
    }
    // Edits in resident chunks, most at or near a chunk's side
    let resident: Vec<IVec2> = maps
      .world
      .get::<StreamState>(map)
      .map(|state| sorted(state.resident.iter().copied().collect()))
      .unwrap_or_default();
    if !resident.is_empty() {
      for _ in 0..next(4) {
        let chunk = resident[next(resident.len() as u32) as usize];
        let near = |next: &mut dyn FnMut(u32) -> u32| match next(3) {
          0 => next(32) as i32,
          1 => next(2) as i32,
          _ => 30 + next(2) as i32,
        };
        let cell = chunk * 32 + IVec2::new(near(&mut next), near(&mut next));
        let layer = if next(5) == 0 { "front" } else { "ground" };
        let tile = jumble_tile(next(1 << 20));
        MapsView::new(&mut maps.world).set_tile(map, layer, cell, tile);
      }
    }
    maps.frame();
    let happened = events(&maps.world, &mut cursor);
    let (now_landed, now_unloaded) = (loaded(&happened, map).len(), changes(&happened, map).len());
    both += usize::from(now_landed > 0 && now_unloaded > 0);
    landed += now_landed;
    unloaded += now_unloaded;
    std::thread::sleep(Duration::from_micros(500));
    if step % 10 == 9 {
      assert_as_built_whole(&mut maps, map, step);
    }
  }
  assert!(
    landed > 100 && unloaded > 100,
    "{landed} landed, {unloaded} unloaded"
  );
  both
}

// ---- other grids ------------------------------------------------------------

/// An infinite map of `shape` with `cell`-sized cells over the tiles, with
/// the tile layers `layers`
fn infinite_on(shape: GridShape, cell: UVec2, layers: &[&str]) -> TileMap {
  let mut map = TileMap::infinite(cell).with_shape(shape);
  map.add_tileset(TilesetSource::Embedded(Arc::new(tiles())));
  for name in layers {
    map.add_tile_layer(name);
  }
  map
}

/// The box of `chunk`'s cells' outlines on `grid`
fn chunk_box(grid: &MapGrid, chunk: IVec2) -> Rect {
  let first = chunk * 32;
  grid.bounds_of(CellRect::new(first, first + IVec2::splat(32)))
}

/// Whether `rect` meets the box `lo..hi`, touching included
fn meets(rect: Rect, (lo, hi): (Vec2, Vec2)) -> bool {
  rect.x <= hi.x && rect.x + rect.width >= lo.x && rect.y <= hi.y && rect.y + rect.height >= lo.y
}

/// The chunks of the cells under points of the box `lo..hi` on `grid`,
/// every `step` pixels and at its corners: chunks a stream over the box
/// must have (some of them, never one it may leave out)
fn chunks_under_box(grid: &MapGrid, (lo, hi): (Vec2, Vec2), step: f32) -> Vec<IVec2> {
  let mut found = std::collections::BTreeSet::new();
  let (nx, ny) = (
    ((hi.x - lo.x) / step).ceil() as i32,
    ((hi.y - lo.y) / step).ceil() as i32,
  );
  for j in 0..=ny {
    for i in 0..=nx {
      let at = Vec2::new(
        (lo.x + i as f32 * step).min(hi.x),
        (lo.y + j as f32 * step).min(hi.y),
      );
      let chunk = TileGrid::chunk_of(grid.cell_at(at));
      found.insert((chunk.y, chunk.x));
    }
  }
  found.into_iter().map(|(y, x)| IVec2::new(x, y)).collect()
}

/// Asserts that what `map` holds round the box `lo..hi` (world pixels, the
/// map at the origin) is what a stream over it should: every chunk with a
/// cell under the box resident, and no resident chunk whose box misses it
fn holds_the_box(maps: &mut MapWorld, map: Entity, area: (Vec2, Vec2), label: &str) {
  let grid = streamed_grid(&mut maps.world, map);
  let held = resident(&mut maps.world, map);
  assert!(!held.is_empty(), "{label}: something landed");
  for chunk in chunks_under_box(&grid, area, 8.0) {
    assert!(
      held.contains(&chunk),
      "{label}: chunk {chunk}, under the box, is resident ({held:?})"
    );
  }
  for &chunk in &held {
    assert!(
      meets(chunk_box(&grid, chunk), area),
      "{label}: resident chunk {chunk}'s box {:?} meets {area:?}",
      chunk_box(&grid, chunk)
    );
  }
  // Each landed whole, its generated row in place
  let view = MapsView::new(&mut maps.world);
  for &chunk in &held {
    assert_eq!(view.tile(map, "ground", chunk * 32), Some(BLOCK), "{label}");
  }
}

/// The grid of `map`'s streamed layers
fn streamed_grid(world: &mut World, map: Entity) -> MapGrid {
  let layers = layers_of(world, map);
  match layers
    .first()
    .and_then(|&layer| world.get::<TileLayerData>(layer))
  {
    Some(data) => data.grid,
    None => panic!("{map} has a tile layer"),
  }
}

/// Moves `anchor` to `at`
fn move_anchor(world: &mut World, anchor: Entity, at: Vec2) {
  match world.get_mut::<Transform>(anchor) {
    Some(mut placed) => placed.translation = at,
    None => panic!("{anchor} is an anchor"),
  }
}

/// A stream loading and keeping the chunks within `reach` of its anchors,
/// filling every chunk's first row of "ground"
fn reaching(reach: f32) -> MapStream {
  MapStream {
    load_distance: reach,
    unload_distance: reach,
    ..MapStream::new(floors)
  }
}

#[test]
fn an_isometric_stream_loads_the_chunks_around_a_moving_anchor() {
  let mut maps = MapWorld::new();
  let iso = infinite_on(GridShape::Isometric, UVec2::new(64, 32), &["ground"]);
  let reach = 300.0;
  let map = spawn(&mut maps, iso, reaching(reach), |r| r);
  let anchor = maps
    .world
    .spawn((Transform::from_xy(40.0, 30.0), StreamAnchor))
    .id();
  // A view smaller than the load area round each place
  let view = |at: Vec2| Rect::new(at.x - 200.0, at.y - 120.0, 400.0, 240.0);
  let around = |at: Vec2| (at - reach, at + reach);
  until_idle(&mut maps, map);
  holds_the_box(
    &mut maps,
    map,
    around(Vec2::new(40.0, 30.0)),
    "at the start",
  );
  assert!(MapsView::new(&mut maps.world).stream_ready(map, view(Vec2::new(40.0, 30.0))));

  // Along a diagonal of the diamonds, then off to negative chunks: the
  // chunks follow, those left behind unload
  for at in [
    Vec2::new(1500.0, 700.0),
    Vec2::new(3000.0, 1400.0),
    Vec2::new(-5000.0, -2600.0),
    Vec2::new(-200.0, 4100.0),
  ] {
    move_anchor(&mut maps.world, anchor, at);
    assert!(
      !MapsView::new(&mut maps.world).stream_ready(map, view(at)),
      "{at}: not landed yet"
    );
    maps.frames_until(|world| MapsView::new(world).stream_ready(map, view(at)));
    until_idle(&mut maps, map);
    holds_the_box(&mut maps, map, around(at), &format!("at {at}"));
  }
}

#[test]
fn every_grid_streams_the_chunks_its_anchor_needs_and_no_far_ones() {
  let shapes = [
    (
      GridShape::Staggered {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Odd,
      },
      UVec2::new(64, 32),
    ),
    (
      GridShape::Staggered {
        axis: StaggerAxis::X,
        index: StaggerIndex::Even,
      },
      UVec2::new(32, 16),
    ),
    (
      GridShape::Hexagonal {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Even,
        side: 12,
      },
      UVec2::new(32, 28),
    ),
    (
      GridShape::Hexagonal {
        axis: StaggerAxis::X,
        index: StaggerIndex::Odd,
        side: 10,
      },
      UVec2::new(28, 32),
    ),
    (
      GridShape::Oblique {
        skew: IVec2::new(-12, 6),
      },
      UVec2::new(32, 32),
    ),
  ];
  for (shape, cell) in shapes {
    let label = format!("{shape:?} {cell}");
    let mut maps = MapWorld::new();
    let reach = 250.0;
    let map = spawn(
      &mut maps,
      infinite_on(shape, cell, &["ground"]),
      reaching(reach),
      |r| r,
    );
    let anchor = maps
      .world
      .spawn((Transform::from_xy(0.0, 0.0), StreamAnchor))
      .id();
    for at in [
      Vec2::ZERO,
      Vec2::new(-1700.0, 900.0),
      Vec2::new(2600.0, -2100.0),
    ] {
      move_anchor(&mut maps.world, anchor, at);
      until_idle(&mut maps, map);
      holds_the_box(
        &mut maps,
        map,
        (at - reach, at + reach),
        &format!("{label} at {at}"),
      );
      let point = Rect::new(at.x, at.y, 0.0, 0.0);
      assert!(
        MapsView::new(&mut maps.world).stream_ready(map, point),
        "{label}"
      );
    }
  }
}

/// The world points a camera's `view` shows: its rectangle, turned and
/// zoomed, at every `step` of the target's pixels and along its edges
fn shown_points(view: &CameraView, step: f32) -> Vec<Vec2> {
  let to_world = view.view_proj.inverse();
  let (nx, ny) = (
    (view.rect_size.x / step).ceil() as i32,
    (view.rect_size.y / step).ceil() as i32,
  );
  let mut points = Vec::new();
  for j in 0..=ny {
    for i in 0..=nx {
      let clip = glam::Vec4::new(
        (-1.0 + 2.0 * i as f32 / nx as f32).min(1.0),
        (-1.0 + 2.0 * j as f32 / ny as f32).min(1.0),
        0.0,
        1.0,
      );
      let at = to_world * clip;
      points.push(Vec2::new(at.x, at.y) / at.w);
    }
  }
  points
}

#[test]
fn a_turned_camera_over_an_isometric_stream_shows_no_unfilled_chunk() {
  let mut maps = MapWorld::new();
  let map = infinite_on(GridShape::Isometric, UVec2::new(32, 16), &["ground"]);
  let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
  // The camera's view alone loads: nothing round it
  let stream = MapStream {
    load_distance: 0.0,
    unload_distance: 0.0,
    land_per_frame: 8,
    unload_per_frame: 8,
    ..MapStream::new(floors)
  };
  let root = maps.world.spawn((MapRoot::new(handle), stream)).id();
  maps.settle(root);
  // Turned 30 degrees and zoomed out to half: a 640 x 360 target shows
  // 1,280 x 720 world pixels at a slant
  let camera = Camera2d {
    zoom: 0.5,
    ..Camera2d::default()
  };
  let turn = 30f32.to_radians();
  let target = Vec2::new(640.0, 360.0);
  let place = |center: Vec2| CameraView::compute(center, turn, &camera, Vec2::ZERO, target);
  let start = Vec2::new(300.0, -200.0);
  let eye = maps
    .world
    .spawn((
      camera,
      place(start),
      Transform::from_translation(start).with_rotation(turn),
    ))
    .id();
  for center in [start, Vec2::new(4100.0, 2300.0), Vec2::new(-6000.0, 1000.0)] {
    let view = place(center);
    if let Some(mut seen) = maps.world.get_mut::<CameraView>(eye) {
      *seen = view;
    }
    if let Some(mut at) = maps.world.get_mut::<Transform>(eye) {
      at.translation = center;
    }
    until_idle(&mut maps, root);
    let bounds = view.bounds;
    let area = Rect::new(bounds.left, bounds.top, bounds.width(), bounds.height());
    let view_rs = MapsView::new(&mut maps.world);
    assert!(
      view_rs.stream_ready(root, area),
      "{center}: the view landed"
    );
    // Every cell on screen is in a resident chunk, the turned view's
    // corners and edges included
    let layer = match view_rs.grid(root, "ground") {
      Some(layer) => layer,
      None => panic!("a ground layer"),
    };
    let held = resident(&mut maps.world, root);
    for at in shown_points(&view, 4.0) {
      let chunk = TileGrid::chunk_of(layer.cell_at_world(at));
      assert!(
        held.contains(&chunk),
        "{center}: {at} on screen is in chunk {chunk}, resident ({held:?})"
      );
    }
    // And nothing whose box misses the view's box
    let grid = layer.grid;
    let boxed = (
      Vec2::new(bounds.left, bounds.top),
      Vec2::new(bounds.right, bounds.bottom),
    );
    for &chunk in &held {
      assert!(meets(chunk_box(&grid, chunk), boxed), "{center}: {chunk}");
    }
  }
}

#[test]
fn a_map_whose_streamed_layers_differ_in_grid_streams_by_the_first_and_warns_once() {
  let seen: Arc<Mutex<Vec<ChunkRequest>>> = Arc::default();
  let record = Arc::clone(&seen);
  let generator = move |request: ChunkRequest| {
    record.lock().expect("not poisoned").push(request);
    floors(request)
  };
  let cell = UVec2::new(64, 32);
  let mut map = infinite_on(GridShape::Isometric, cell, &["ground", "grid-apart-front"]);
  let iso = map.grid();
  if let LayerKind::Tiles { grid, .. } = &mut map.layers[1].kind {
    *grid = MapGrid::new(GridShape::Orthogonal, UVec2::splat(16));
  }
  let mut maps = MapWorld::new();
  let stream = MapStream {
    load_distance: 200.0,
    unload_distance: 200.0,
    ..MapStream::new(generator)
  };
  let before = warnings(&["different grids"]);
  let map = spawn(&mut maps, map, stream, |r| r);
  let anchor = maps
    .world
    .spawn((Transform::from_xy(10.0, 10.0), StreamAnchor))
    .id();
  until_idle(&mut maps, map);
  move_anchor(&mut maps.world, anchor, Vec2::new(2000.0, 900.0));
  until_idle(&mut maps, map);
  for _ in 0..5 {
    maps.frame();
  }
  assert_eq!(warnings(&["different grids"]), before + 1, "warned once");
  // The first streamed layer's grid lays the chunks out
  holds_the_box(
    &mut maps,
    map,
    (Vec2::new(1800.0, 700.0), Vec2::new(2200.0, 1100.0)),
    "by the first layer's grid",
  );
  let seen = seen.lock().expect("not poisoned");
  assert!(seen.iter().all(|request| request.grid == iso));
}
