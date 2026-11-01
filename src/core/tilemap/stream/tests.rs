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
  StreamAnchor, StreamState, StreamStatus,
};
use crate::core::asset::{Assets, AssetsView, Handle};
use crate::core::ecs::kernel::{ChildOf, Disabled, MessageCursor, Messages};
use crate::core::ecs::world_ext::despawn_game_entities;
use crate::core::ecs::{Entity, Transform, World};
use crate::core::graphics::{Camera2d, CameraView};
use crate::core::tilemap::test_util::{FIXTURES, MapWorld, files_with, layers_of, load_map};
use crate::core::tilemap::{
  IntGrid, MapEvent, MapLayerRef, MapRoot, MapTile, MapsView, Tile, TileData, TileFlip,
  TileLayerData, TileMap, TileShape, Tileset, TilesetImage, TilesetSource,
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
  generated_tracked(world, map);
  for layer in layers_of(world, map) {
    if let Some(data) = world.get::<TileLayerData>(layer) {
      let held: Vec<IVec2> = data.grid.chunk_coords().collect();
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
  // Every chunk left behind unloads in the one frame
  let stream = MapStream {
    unload_per_frame: 9,
    ..MapStream::new(floors).with_saved(saved.clone())
  };
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
  // others drain, and the new chunks are in flight
  move_to(&mut maps.world, anchor, IVec2::new(20, 0));
  maps.frame();
  let Some(moving) = status(&mut maps.world) else {
    panic!("{map} streams");
  };
  assert_eq!(moving.draining, 7, "{moving:?}");
  assert!(moving.in_flight > 0, "{moving:?}");
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
      if data.grid.has_chunk(chunk) {
        found.push((layer, chunk, data.grid.chunk_version(chunk)));
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
