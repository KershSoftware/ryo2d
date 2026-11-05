//! The demo's pages without a window: each lands its map and plays for
//! five seconds, without a warning or a failed map, and the speed page
//! does on its hexagonal grid too, its map colliding and casting shadows.
//! The meadow is a Tiled
//! infinite map streamed with a generator: its hand-made chunks lie at
//! negative cells, the HUD tells them from the generated ones and counts
//! the chunks resident and in flight, its runner runs on past the meadow's
//! left end and a player runs it past the right end, onto generated ground
//! streamed in ahead, without falling, and the file's chunks stay as the
//! file has them; a save of the map adds a hand-made chunk; the chunks in
//! view are labelled with world text on whole screen pixels, saying which
//! are hand-made and which generated. A click on the paint page paints the
//! cell under the pointer (at negative cells), making its chunk, and a
//! right click erases it; the ball dropped over painted ground comes to
//! rest on it. On the endless page the digger digs and builds with the
//! mouse; leaving takes the changed chunks into the stream's `SavedChunks`
//! (the HUD counts them, and the chunks in flight as new ground streams
//! in), and coming back finds them as the player left them. The town and
//! the hex board draw Kenney's pictures at their own size, each tileset
//! padded to at most twice its art, and the town's, the island's and the
//! board's tile layers sample them linearly. The pages on the other grids
//! have their own tests (`tests/grids.rs`)

mod common;

use std::collections::BTreeSet;

use common::{
  LANDING, click, engine_on, landed_maps, layer_grid, spawned_map, step_resting, warnings,
};
use ryo2d::core::tilemap::padded::{PaddedSize, padded_sizes};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;
use tile_grids_demo::{Ball, ChunkLabel, Digger, Runner, demo};

/// The pages this file tests, in the demo's order, and how many there are
const MEADOW: usize = 0;
const PAINT: usize = 1;
const ENDLESS: usize = 2;
const TOWN: usize = 3;
const STAGGERED: usize = 4;
const HEX: usize = 5;
const SPEED: usize = 7;
const PAGES: usize = 8;

/// The meadow's file
const MEADOW_FILE: &str = "maps/endless-meadow.tmx";

/// The map each page lands within five seconds, named by its tile layers:
/// the meadow's `scenery`, `water` and `ground`, the paint page's `ground`,
/// the endless world's `backdrop`, `scenery` and `ground`, the town's and
/// the island's `ground` and `props`, the hex board's `board` and its
/// marks, the oblique dungeon's `floor` and `walls`, and the speed page's
/// four layers
const LANDS: [&[&str]; PAGES] = [
  &["scenery", "water", "ground"],
  &["ground"],
  &["backdrop", "scenery", "ground"],
  &["ground", "props"],
  &["ground", "props"],
  &["board", "range", "ring", "neighbours", "line"],
  &["floor", "walls"],
  &["background", "ground", "props", "front"],
];

/// Five seconds at the demo's 60 Hz
const FRAMES: u32 = 300;

/// A cell's side, and a chunk's, in pixels
const CELL: f32 = 18.0;
const CHUNK_PX: f32 = CHUNK_SIZE as f32 * CELL;

/// The map events sent since `cursor` last read
fn map_events(engine: &HeadlessEngine, cursor: &mut MessageCursor<MapEvent>) -> Vec<MapEvent> {
  engine
    .world()
    .get_resource::<Messages<MapEvent>>()
    .map(|messages| cursor.read(messages).cloned().collect())
    .unwrap_or_default()
}

/// The chunk events sent since `cursor` last read
fn chunk_events(
  engine: &HeadlessEngine,
  cursor: &mut MessageCursor<ChunkEvent>,
) -> Vec<ChunkEvent> {
  engine
    .world()
    .get_resource::<Messages<ChunkEvent>>()
    .map(|messages| cursor.read(messages).cloned().collect())
    .unwrap_or_default()
}

/// Whether the page's map has landed
fn has_landed(engine: &mut HeadlessEngine) -> bool {
  let world = engine.world_mut();
  let mut roots = world.query_filtered::<Entity, (With<MapRoot>, With<MapIds>)>();
  roots.iter(world).next().is_some()
}

#[test]
fn every_page_plays_for_five_seconds_without_a_warning() {
  for (page, expected) in LANDS.iter().enumerate() {
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    let mut cursor = MessageCursor::<MapEvent>::default();
    let mut spawned = 0;
    // Its map lands, then the page plays for five seconds
    let (mut waited, mut played) = (0, 0);
    while played < FRAMES {
      if has_landed(&mut engine) {
        played += 1;
        engine.step().unwrap();
      } else {
        waited += 1;
        assert!(waited < LANDING, "page {}: the map never landed", page + 1);
        step_resting(&mut engine);
      }
      for event in map_events(&engine, &mut cursor) {
        match event {
          MapEvent::Spawned { .. } => spawned += 1,
          MapEvent::Failed { error, .. } => panic!("page {}: a map failed: {error}", page + 1),
          MapEvent::Reloaded { .. } => {}
        }
      }
    }
    let (landed, roots) = landed_maps(&mut engine);
    assert_eq!(
      (landed.len(), roots),
      (1, 1),
      "page {}: maps landed {landed:?} of {roots} roots",
      page + 1
    );
    let mut names = landed[0].clone();
    names.retain(|name| !name.starts_with("objects"));
    assert_eq!(names, *expected, "page {}: the map's tile layers", page + 1);
    assert_eq!(spawned, 1, "page {}: Spawned events", page + 1);
    let warned = warnings();
    assert!(warned.is_empty(), "page {}: {warned:#?}", page + 1);
  }
}

#[test]
fn the_speed_page_plays_for_five_seconds_on_its_hexagonal_grid_without_a_warning() {
  let Some(mut engine) = engine_on(SPEED) else {
    return;
  };
  spawned_map(&mut engine);
  // The Grid setting's fourth choice
  for _ in 0..3 {
    engine.tap_key(KeyCode::ArrowRight);
    engine.step_frames(2).unwrap();
  }
  let mut waited = 0;
  loop {
    let (landed, roots) = landed_maps(&mut engine);
    if landed.len() == 1 && roots == 1 {
      let map = spawned_map(&mut engine);
      let grid = MapsView::new(engine.world_mut()).grid(map, "ground");
      if grid.is_some_and(|grid| matches!(grid.grid.shape, GridShape::Hexagonal { .. })) {
        break;
      }
    }
    waited += 1;
    assert!(waited < LANDING, "the hexagonal map never landed");
    step_resting(&mut engine);
  }
  // Five seconds of its camera flying over the map
  engine.step_frames(FRAMES).unwrap();
  let (landed, roots) = landed_maps(&mut engine);
  assert_eq!(
    (landed.len(), roots),
    (1, 1),
    "maps landed {landed:?} of {roots} roots"
  );
  let warned = warnings();
  assert!(warned.is_empty(), "{warned:#?}");
}

/// The tile size of the tileset `name` the map `file` draws with
fn tile_size_of(engine: &mut HeadlessEngine, file: &str, name: &str) -> UVec2 {
  let handle = engine
    .assets()
    .load_now::<TileMap>(file)
    .unwrap_or_else(|_| panic!("the map {file}"));
  let world = engine.world();
  let map = world
    .resource::<Assets<TileMap>>()
    .get(&handle)
    .unwrap_or_else(|| panic!("{file} is loaded"));
  let tilesets = world.resource::<Assets<Tileset>>();
  map
    .tilesets
    .iter()
    .find_map(|source| match source {
      TilesetSource::Asset(tileset) => tilesets
        .get(tileset)
        .filter(|tileset| tileset.name == name)
        .map(|tileset| tileset.tile_size),
      TilesetSource::Embedded(tileset) => (tileset.name == name).then_some(tileset.tile_size),
    })
    .unwrap_or_else(|| panic!("{file} draws with the tileset {name:?}"))
}

/// What the padding of the tileset `name` takes, once the page's map has
/// landed with it
fn padded_size(engine: &HeadlessEngine, name: &str) -> PaddedSize {
  let sizes = padded_sizes(engine.world());
  sizes
    .iter()
    .find(|size| size.name == name)
    .cloned()
    .unwrap_or_else(|| panic!("a padding of {name:?} in {sizes:?}"))
}

#[test]
fn the_town_draws_kenney_s_pictures_at_their_own_size() {
  let Some(mut engine) = engine_on(TOWN) else {
    return;
  };
  spawned_map(&mut engine);
  engine.step_frames(3).unwrap();
  // The pack's pictures cut to their bottom 256 rows, not halved
  for name in ["sketch-town-ground", "sketch-town-props"] {
    assert_eq!(
      tile_size_of(&mut engine, "maps/town.tmx", name),
      UVec2::new(256, 256),
      "{name}'s pictures"
    );
  }
  // Padded at about their own size: each at most twice its art, both
  // within 20 MiB
  let mut total = 0;
  for name in ["sketch-town-ground", "sketch-town-props"] {
    let size = padded_size(&engine, name);
    assert!(
      size.bytes <= 2 * size.art_bytes,
      "{name} pads to {} bytes, its art {}",
      size.bytes,
      size.art_bytes
    );
    total += size.bytes;
  }
  assert!(
    total <= 20 * 1024 * 1024,
    "the town's tilesets pad to {total} bytes"
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// Each tile layer of the page's map, by name, with how it is sampled
fn tile_layer_filters(engine: &mut HeadlessEngine, map: Entity) -> Vec<(String, Filter)> {
  let world = engine.world_mut();
  let mut layers = world.query::<(&MapLayerRef, &LayerInfo, Has<TileLayerData>)>();
  let mut found: Vec<(usize, String, Filter)> = layers
    .iter(world)
    .filter(|(layer, _, tiles)| layer.map == map && *tiles)
    .map(|(layer, info, _)| (layer.index, info.name.clone(), info.filter))
    .collect();
  found.sort_by_key(|(index, ..)| *index);
  found
    .into_iter()
    .map(|(_, name, filter)| (name, filter))
    .collect()
}

#[test]
fn the_town_island_and_hex_board_sample_their_painted_art_smoothly() {
  // The demo is pixel art, which samples tiles nearest: Kenney's painted
  // pictures, drawn at a half and a quarter, ask their layers for linear
  // sampling, so they read the smaller copies
  for page in [TOWN, STAGGERED, HEX] {
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    let map = spawned_map(&mut engine);
    let layers = tile_layer_filters(&mut engine, map);
    assert!(!layers.is_empty(), "page {}: tile layers", page + 1);
    for (name, filter) in layers {
      assert_eq!(filter, Filter::Linear, "page {}: layer {name:?}", page + 1);
    }
  }
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_hex_board_draws_its_hexagons_at_their_own_size() {
  let Some(mut engine) = engine_on(HEX) else {
    return;
  };
  spawned_map(&mut engine);
  engine.step_frames(3).unwrap();
  // The terrain sheet's hexagons as the pack draws them
  assert_eq!(
    tile_size_of(&mut engine, "maps/hex-board.tmx", "hexagon-board"),
    UVec2::new(120, 140),
    "the board's hexagons"
  );
  let size = padded_size(&engine, "hexagon-board");
  assert!(
    size.bytes <= 2 * size.art_bytes && size.bytes <= 8 * 1024 * 1024,
    "the board pads to {} bytes, its art {}",
    size.bytes,
    size.art_bytes
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The chunks resident on `map`: those its tile layers have, each once
fn resident(engine: &mut HeadlessEngine, map: Entity) -> BTreeSet<(i32, i32)> {
  let world = engine.world_mut();
  let mut layers = world.query::<(&MapLayerRef, &TileLayerData)>();
  layers
    .iter(world)
    .filter(|(layer, _)| layer.map == map)
    .flat_map(|(_, data)| data.tiles.chunk_coords().map(|c| (c.x, c.y)))
    .collect()
}

/// The cells of `chunk`
fn chunk_cells(chunk: IVec2) -> impl Iterator<Item = IVec2> {
  let side = CHUNK_SIZE as i32;
  let origin = chunk * side;
  (0..side).flat_map(move |y| (0..side).map(move |x| origin + IVec2::new(x, y)))
}

/// Every tile of a chunk of `grid`
fn chunk_tiles(grid: &TileGrid, chunk: IVec2) -> usize {
  chunk_cells(chunk)
    .filter(|cell| grid.get(*cell).is_some())
    .count()
}

/// The meadow's asset as its file has it
fn meadow_asset(engine: &mut HeadlessEngine) -> TileMap {
  let handle = engine
    .assets()
    .load_now::<TileMap>(MEADOW_FILE)
    .expect("the meadow");
  let assets = engine.world().resource::<Assets<TileMap>>();
  assets.get(&handle).expect("the meadow is loaded").clone()
}

/// The file's tile layers, each with its name and grid
fn file_layers(meadow: &TileMap) -> Vec<(String, TileGrid)> {
  meadow
    .layers
    .iter()
    .enumerate()
    .filter_map(|(index, layer)| {
      let grid = meadow.tiles(index)?;
      Some((layer.info.name.clone(), grid.clone()))
    })
    .collect()
}

/// The hand-made chunks: every engine chunk the file's layers hold tiles in
fn file_chunks(meadow: &TileMap) -> BTreeSet<(i32, i32)> {
  file_layers(meadow)
    .iter()
    .flat_map(|(_, grid)| grid.chunk_coords().map(|c| (c.x, c.y)).collect::<Vec<_>>())
    .collect()
}

/// The meadow's ends, in pixels: the left edge of its leftmost hand-made
/// chunk on the grass's chunk row, and the right edge of its rightmost
fn meadow_ends(file: &BTreeSet<(i32, i32)>) -> (f32, f32) {
  let row: Vec<i32> = file
    .iter()
    .filter(|(_, y)| *y == 0)
    .map(|(x, _)| *x)
    .collect();
  let first = *row.iter().min().expect("hand-made chunks on row 0");
  let last = *row.iter().max().expect("hand-made chunks on row 0");
  (first as f32 * CHUNK_PX, (last + 1) as f32 * CHUNK_PX)
}

/// Every cell of every hand-made chunk holds what the file has there, in
/// every tile layer
fn assert_file_chunks_unchanged(engine: &mut HeadlessEngine, map: Entity, meadow: &TileMap) {
  let file = file_chunks(meadow);
  for (name, saved) in file_layers(meadow) {
    let live = layer_grid(engine, map, &name);
    for &(x, y) in &file {
      for cell in chunk_cells(IVec2::new(x, y)) {
        assert_eq!(
          live.get(cell),
          saved.get(cell),
          "{name}'s cell {cell}, in hand-made chunk ({x}, {y}), is as the file has it"
        );
      }
    }
  }
}

/// The number written just before `marker` in the HUD (`"3 in flight"`),
/// or just after it (`"Resident: 14"`)
fn number_before(shown: &str, marker: &str) -> Option<usize> {
  let at = shown.find(marker)?;
  let digits: String = shown[..at]
    .chars()
    .rev()
    .take_while(char::is_ascii_digit)
    .collect::<Vec<_>>()
    .into_iter()
    .rev()
    .collect();
  digits.parse().ok()
}

fn number_after(shown: &str, marker: &str) -> Option<usize> {
  let at = shown.find(marker)? + marker.len();
  let digits: String = shown[at..]
    .chars()
    .take_while(char::is_ascii_digit)
    .collect();
  digits.parse().ok()
}

/// What the stream of `map` has now, as the engine counts it
fn status(engine: &mut HeadlessEngine, map: Entity) -> StreamStatus {
  MapsView::new(engine.world_mut())
    .stream_status(map)
    .expect("the page's map streams")
}

/// Steps until `map`'s stream has had nothing in flight and nothing
/// draining for 20 frames running
fn settle(engine: &mut HeadlessEngine, map: Entity) {
  let (mut quiet, mut last) = (0, None);
  for _ in 0..900 {
    engine.step().unwrap();
    let now = status(engine, map);
    quiet = if now.in_flight == 0 && now.draining == 0 {
      quiet + 1
    } else {
      0
    };
    last = Some(now);
    if quiet >= 20 {
      return;
    }
  }
  panic!("the stream never settled: last {last:?}");
}

/// The meadow's runner, once it has one
fn runner(engine: &mut HeadlessEngine) -> Option<Vec2> {
  let world = engine.world_mut();
  let mut runners = world.query_filtered::<&Transform, With<Runner>>();
  runners.iter(world).next().map(|t| t.translation)
}

/// Steps until the meadow's runner has spawned
fn wait_for_runner(engine: &mut HeadlessEngine) -> Vec2 {
  for _ in 0..600 {
    engine.step().unwrap();
    if let Some(at) = runner(engine) {
      return at;
    }
  }
  panic!("the runner never spawned: {:#?}", warnings());
}

#[test]
fn the_meadow_s_hand_made_chunks_lie_at_negative_cells_and_the_hud_tells_them_from_generated() {
  let Some(mut engine) = engine_on(MEADOW) else {
    return;
  };
  let map = spawned_map(&mut engine);
  wait_for_runner(&mut engine);
  // Space takes the runner over (it jumps once and stands), so the view
  // and the chunks round it stay put
  engine.tap_key(KeyCode::Space);
  engine.step_frames(90).unwrap();
  settle(&mut engine, map);

  // The asset: an infinite map, its bounds reaching negative cells
  let meadow = meadow_asset(&mut engine);
  assert!(meadow.infinite, "the meadow is an infinite map");
  assert_eq!(meadow.size, UVec2::ZERO, "an infinite map has no size");
  let bounds = meadow.bounds();
  assert!(
    bounds.min.x < 0 && bounds.min.y < 0,
    "bounds reach negative cells: {bounds:?}"
  );

  // Its layers: unbounded, the hand-made chunks left of and above the
  // origin, each holding tiles, and all resident
  for (name, _) in file_layers(&meadow) {
    assert_eq!(
      layer_grid(&mut engine, map, &name).bounds(),
      None,
      "{name} is unbounded"
    );
  }
  let file = file_chunks(&meadow);
  assert!(
    file.iter().any(|c| c.0 < -1) && file.iter().any(|c| c.1 < 0),
    "hand-made chunks left of and above the origin: {file:?}"
  );
  let layers = file_layers(&meadow);
  for &(x, y) in &file {
    let tiles: usize = layers
      .iter()
      .map(|(_, grid)| chunk_tiles(grid, IVec2::new(x, y)))
      .sum();
    assert!(tiles > 0, "hand-made chunk ({x}, {y}) holds tiles");
  }
  let now = resident(&mut engine, map);
  assert!(file.is_subset(&now), "every hand-made chunk is resident");
  let generated: BTreeSet<(i32, i32)> = now.difference(&file).copied().collect();
  assert!(
    !generated.is_empty(),
    "the stream generated the chunks round the view beyond the file's: {now:?}"
  );

  // The engine's count of them
  let counted = status(&mut engine, map);
  assert_eq!(
    (counted.resident, counted.hand_made, counted.generated),
    (now.len(), file.len(), generated.len()),
    "the stream's status: {counted:?}"
  );

  // The HUD reads it: the hand-made chunks and the file's bounds (last
  // cells included), the generated chunks resident, and every chunk
  // resident and in flight
  let shown = engine.ui_snapshot();
  let last = bounds.max - IVec2::ONE;
  for line in [
    format!("Hand-made: {} chunks", counted.hand_made),
    format!(
      "The file's bounds: cells ({}, {}) to ({}, {})",
      bounds.min.x, bounds.min.y, last.x, last.y
    ),
    format!("Generated: {} resident", counted.generated),
    format!(
      "Resident: {} chunks, {} in flight",
      counted.resident, counted.in_flight
    ),
  ] {
    assert!(shown.contains(&line), "{line:?} in the HUD");
  }
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The demo's assets folder, read into memory: a test saves over a file
/// there without touching the disk
fn assets_in_memory() -> MemorySource {
  fn read(root: &std::path::Path, dir: &std::path::Path, files: &MemorySource) {
    for entry in std::fs::read_dir(dir).expect("the assets folder").flatten() {
      let path = entry.path();
      if path.is_dir() {
        read(root, &path, files);
      } else if let (Ok(rel), Ok(bytes)) = (path.strip_prefix(root), std::fs::read(&path)) {
        files.insert(&rel.to_string_lossy().replace('\\', "/"), bytes);
      }
    }
  }
  let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
  let files = MemorySource::new();
  read(&root, &root, &files);
  files
}

#[test]
fn a_save_of_the_meadow_adds_a_hand_made_chunk_and_the_hud_follows() {
  let files = assets_in_memory();
  let builder = demo()
    .start_page(MEADOW)
    .configure({
      let files = files.clone();
      move |b| b.asset_source(files)
    })
    .engine_builder();
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  let map = spawned_map(&mut engine);
  engine.step_frames(3).unwrap();
  let hand_made = |engine: &mut HeadlessEngine| {
    number_after(&engine.ui_snapshot(), "Hand-made: ").expect("the hand-made line")
  };
  let before = hand_made(&mut engine);

  // A save adds a tile far right of the meadow, in a chunk of its own
  let text = std::fs::read_to_string(
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("assets")
      .join(MEADOW_FILE),
  )
  .expect("the meadow's file");
  let far = IVec2::new(300, -100);
  let chunk = format!(
    "<chunk x=\"{}\" y=\"{}\" width=\"1\" height=\"1\">\n5\n</chunk>\n",
    far.x, far.y
  );
  let ground = text.find("name=\"ground\"").expect("the ground layer");
  let data = ground
    + text[ground..]
      .find("<data encoding=\"csv\">\n")
      .expect("its data");
  let at = data + "<data encoding=\"csv\">\n".len();
  let saved = format!("{}{chunk}{}", &text[..at], &text[at..]);
  files.insert(MEADOW_FILE, saved);
  engine.assets().server().reload(MEADOW_FILE);
  let mut cursor = MessageCursor::<MapEvent>::default();
  let mut reloaded = false;
  for _ in 0..600 {
    engine.step().unwrap();
    if map_events(&engine, &mut cursor)
      .iter()
      .any(|e| matches!(e, MapEvent::Reloaded { map: m } if *m == map))
    {
      reloaded = true;
      break;
    }
  }
  assert!(reloaded, "the save reloaded the meadow: {:#?}", warnings());
  engine.step_frames(2).unwrap();
  assert_eq!(
    hand_made(&mut engine),
    before + 1,
    "the new chunk is hand-made"
  );
  assert_eq!(
    status(&mut engine, map).hand_made,
    before + 1,
    "the stream counts it hand-made"
  );
  let far_chunk = TileGrid::chunk_of(far);
  assert!(
    resident(&mut engine, map).contains(&(far_chunk.x, far_chunk.y)),
    "the file's new chunk is resident, far from the view"
  );
  let bounds = meadow_asset(&mut engine).bounds();
  assert_eq!(
    bounds.max.x,
    far.x + 1,
    "the file's bounds reach the new tile"
  );
  let shown = engine.ui_snapshot();
  let last = bounds.max - IVec2::ONE;
  let line = format!(
    "The file's bounds: cells ({}, {}) to ({}, {})",
    bounds.min.x, bounds.min.y, last.x, last.y
  );
  assert!(shown.contains(&line), "{line:?} in the HUD");
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The chunk labels there are, each with its place in the world and text
fn chunk_labels(engine: &mut HeadlessEngine) -> Vec<(IVec2, Vec2, String)> {
  let world = engine.world_mut();
  let mut labels = world.query::<(&ChunkLabel, &Transform, &Text2d)>();
  labels
    .iter(world)
    .map(|(label, transform, text)| {
      (
        label.chunk,
        transform.translation,
        text.text.as_str().to_string(),
      )
    })
    .collect()
}

#[test]
fn the_chunk_labels_are_world_text_on_the_chunks_in_view_on_whole_pixels() {
  let Some(mut engine) = engine_on(MEADOW) else {
    return;
  };
  let map = spawned_map(&mut engine);
  engine.step_frames(10).unwrap();
  let chunks = resident(&mut engine, map);
  let file = file_chunks(&meadow_asset(&mut engine));
  let labels = chunk_labels(&mut engine);
  assert!(!labels.is_empty(), "chunks in view are labelled");
  for (chunk, at, text) in &labels {
    assert!(
      chunks.contains(&(chunk.x, chunk.y)),
      "{chunk} is a chunk the map has"
    );
    let kind = if file.contains(&(chunk.x, chunk.y)) {
      "hand-made"
    } else {
      "generated"
    };
    assert_eq!(
      *text,
      format!("chunk {}, {}, {kind}", chunk.x, chunk.y),
      "chunk {chunk}'s label"
    );
    let min = chunk.as_vec2() * CHUNK_PX;
    assert!(
      at.cmpge(min).all() && at.cmplt(min + CHUNK_PX).all(),
      "chunk {chunk}'s label at {at} lies in it"
    );
    let screen = CameraMut::main(engine.world_mut())
      .expect("the main camera")
      .world_to_screen(*at);
    assert!(
      (0.0..960.0).contains(&screen.x) && (0.0..600.0).contains(&screen.y),
      "chunk {chunk}'s label is on screen, at {screen}"
    );
    assert!(
      (screen - screen.round()).abs().max_element() < 0.01,
      "chunk {chunk}'s label sits on a whole screen pixel: {screen}"
    );
  }

  // The Chunk overlay row has the panel's focus: off, the labels go
  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(3).unwrap();
  assert!(
    chunk_labels(&mut engine).is_empty(),
    "no labels with the overlay off"
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The topmost ground cell in the column under `at` (world pixels), from a
/// few cells above it down to the meadow's floor
fn ground_under(engine: &mut HeadlessEngine, map: Entity, layer: &str, at: Vec2) -> Option<IVec2> {
  let maps = MapsView::new(engine.world_mut());
  let cell = maps.world_to_cell(map, layer, at)?;
  (cell.y - 4..cell.y + 12)
    .map(|y| IVec2::new(cell.x, y))
    .find(|c| maps.tile(map, layer, *c).is_some())
}

/// Whether `at` is on screen
fn on_screen(engine: &mut HeadlessEngine, at: Vec2) -> Option<Vec2> {
  let screen = CameraMut::main(engine.world_mut())
    .expect("the main camera")
    .world_to_screen(at);
  ((0.0..=960.0).contains(&screen.x) && (0.0..=600.0).contains(&screen.y)).then_some(screen)
}

#[test]
fn the_runner_runs_on_past_the_meadow_s_left_end_onto_generated_ground() {
  let Some(mut engine) = engine_on(MEADOW) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let start = wait_for_runner(&mut engine);
  let meadow = meadow_asset(&mut engine);
  let file = file_chunks(&meadow);
  let (left_end, _) = meadow_ends(&file);
  assert!(start.x > left_end, "the runner starts on the meadow");

  // By itself it runs left, past the meadow's end and two chunks on, in
  // view and never falling, chunks streaming in ahead of it
  let past = left_end - 2.0 * CHUNK_PX;
  let fallen = 6.0 * CELL;
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  let mut landed_beyond = BTreeSet::new();
  let mut now = start;
  for _ in 0..3000 {
    engine.step().unwrap();
    now = runner(&mut engine).expect("the runner");
    assert!(now.y < fallen, "the runner fell, at {now}");
    assert!(
      on_screen(&mut engine, now).is_some(),
      "the runner at {now} is in view"
    );
    for event in chunk_events(&engine, &mut cursor) {
      if let ChunkEvent::Loaded { map: m, chunk } = event
        && m == map
        && (chunk.x as f32 + 1.0) * CHUNK_PX <= left_end
      {
        landed_beyond.insert((chunk.x, chunk.y));
      }
    }
    if now.x < past {
      break;
    }
  }
  assert!(
    now.x < past,
    "the runner ran two chunks past the meadow's left end (x {past}): it got to {}",
    now.x
  );
  assert!(
    !landed_beyond.is_empty(),
    "chunks streamed in left of the meadow"
  );
  let under = ground_under(&mut engine, map, "ground", now).expect("ground under the runner");
  let chunk = TileGrid::chunk_of(under);
  assert!(
    !file.contains(&(chunk.x, chunk.y)),
    "the runner runs on generated ground: {under} is in chunk {chunk}"
  );

  // The labels in view say which chunks are generated
  let labels = chunk_labels(&mut engine);
  assert!(
    labels
      .iter()
      .any(|(_, _, text)| text.ends_with(", generated")),
    "a generated chunk's label is in view: {labels:?}"
  );
  assert_file_chunks_unchanged(&mut engine, map, &meadow);
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn a_player_runs_the_runner_past_the_meadow_s_right_end_and_the_file_s_chunks_stay_as_saved() {
  let Some(mut engine) = engine_on(MEADOW) else {
    return;
  };
  let map = spawned_map(&mut engine);
  wait_for_runner(&mut engine);
  let meadow = meadow_asset(&mut engine);
  let file = file_chunks(&meadow);
  let (_, right_end) = meadow_ends(&file);

  // D takes the runner over; held, with a full jump now and then (a tap is
  // cut short) for the hills and out of the dip, past the meadow's right
  // end and two chunks on
  let past = right_end + 2.0 * CHUNK_PX;
  let fallen = 6.0 * CELL;
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  let mut landed_beyond = BTreeSet::new();
  engine.press_key(KeyCode::KeyD);
  let mut now = Vec2::ZERO;
  for frame in 0..4000 {
    match frame % 40 {
      0 => engine.press_key(KeyCode::Space),
      25 => engine.release_key(KeyCode::Space),
      _ => {}
    }
    engine.step().unwrap();
    now = runner(&mut engine).expect("the runner");
    assert!(now.y < fallen, "the runner fell, at {now}");
    for event in chunk_events(&engine, &mut cursor) {
      if let ChunkEvent::Loaded { map: m, chunk } = event
        && m == map
        && chunk.x as f32 * CHUNK_PX >= right_end
      {
        landed_beyond.insert((chunk.x, chunk.y));
      }
    }
    if now.x > past {
      break;
    }
  }
  engine.release_key(KeyCode::KeyD);
  engine.release_key(KeyCode::Space);
  assert!(
    now.x > past,
    "the runner ran two chunks past the meadow's right end (x {past}): it got to {}",
    now.x
  );
  assert!(
    !landed_beyond.is_empty(),
    "chunks streamed in right of the meadow"
  );
  let under = ground_under(&mut engine, map, "ground", now).expect("ground under the runner");
  let chunk = TileGrid::chunk_of(under);
  assert!(
    !file.contains(&(chunk.x, chunk.y)),
    "the runner runs on generated ground: {under} is in chunk {chunk}"
  );
  assert_file_chunks_unchanged(&mut engine, map, &meadow);
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The paint page's layer
const LAYER: &str = "ground";

/// Where `cell` of the page's layer `layer` is on screen
fn cell_on_screen_in(engine: &mut HeadlessEngine, map: Entity, layer: &str, cell: IVec2) -> Vec2 {
  let at = MapsView::new(engine.world_mut())
    .cell_to_world(map, layer, cell)
    .expect("the layer");
  CameraMut::main(engine.world_mut())
    .expect("the main camera")
    .world_to_screen(at)
}

/// Where `cell` of the paint page's layer is on screen
fn cell_on_screen(engine: &mut HeadlessEngine, map: Entity, cell: IVec2) -> Vec2 {
  cell_on_screen_in(engine, map, LAYER, cell)
}

/// The cell of the paint page's layer under screen point `at`
fn cell_at(engine: &mut HeadlessEngine, map: Entity, at: Vec2) -> IVec2 {
  let world = CameraMut::main(engine.world_mut())
    .expect("the main camera")
    .screen_to_world(at);
  MapsView::new(engine.world_mut())
    .world_to_cell(map, LAYER, world)
    .expect("the paint layer")
}

#[test]
fn a_click_paints_the_cell_under_the_pointer_at_negative_cells() {
  let Some(mut engine) = engine_on(PAINT) else {
    return;
  };
  let map = spawned_map(&mut engine);
  engine.step_frames(3).unwrap();

  // A point up and left of the view's middle: a cell at negative
  // coordinates, in a chunk the map does not have yet
  let at = Vec2::new(120.0, 140.0);
  let cell = cell_at(&mut engine, map, at);
  assert!(cell.x < 0 && cell.y < 0, "{cell} is at negative cells");
  let before = layer_grid(&mut engine, map, LAYER);
  let chunk = TileGrid::chunk_of(cell);
  assert!(
    before.get(cell).is_none() && !before.chunk_coords().any(|c| c == chunk),
    "{cell} starts empty, its chunk {chunk} not made"
  );

  click(&mut engine, at, MouseButton::Left);
  let after = layer_grid(&mut engine, map, LAYER);
  assert!(after.get(cell).is_some(), "the click painted {cell}");
  assert!(
    after.chunk_coords().any(|c| c == chunk),
    "the paint made chunk {chunk}"
  );
  let shown = engine.ui_snapshot();
  let pointer = format!("cell {}, {}", cell.x, cell.y);
  assert!(shown.contains(&pointer), "{pointer:?} in {shown}");
  let count = format!("Chunks: {}", after.chunk_coords().count());
  assert!(shown.contains(&count), "{count:?} in {shown}");

  // A right click erases it; the chunk stays
  click(&mut engine, at, MouseButton::Right);
  let erased = layer_grid(&mut engine, map, LAYER);
  assert!(erased.get(cell).is_none(), "the right click erased {cell}");
  assert!(erased.chunk_coords().any(|c| c == chunk), "a chunk stays");
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_ball_comes_to_rest_on_painted_ground() {
  let Some(mut engine) = engine_on(PAINT) else {
    return;
  };
  let map = spawned_map(&mut engine);
  engine.step_frames(3).unwrap();

  // Drag a floor of nine cells at negative cells, left of the view's middle
  let from = cell_at(&mut engine, map, Vec2::new(100.0, 360.0));
  let row: Vec<IVec2> = (0..9).map(|i| from + IVec2::new(i, 0)).collect();
  assert!(
    row.iter().all(|c| c.x < 0),
    "the floor is at negative cells"
  );
  let first = cell_on_screen(&mut engine, map, row[0]);
  engine.mouse_move(first.x, first.y);
  engine.step().unwrap();
  engine.press_mouse(MouseButton::Left);
  for cell in &row {
    let at = cell_on_screen(&mut engine, map, *cell);
    engine.mouse_move(at.x, at.y);
    engine.step_frames(2).unwrap();
  }
  engine.release_mouse(MouseButton::Left);
  engine.step_frames(2).unwrap();
  let grid = layer_grid(&mut engine, map, LAYER);
  for cell in &row {
    assert!(grid.get(*cell).is_some(), "{cell} is painted");
  }

  // Space drops the ball at the pointer, five cells over the floor's middle
  let above = cell_on_screen(&mut engine, map, row[4] - IVec2::new(0, 5));
  engine.mouse_move(above.x, above.y);
  engine.step().unwrap();
  engine.tap_key(KeyCode::Space);
  engine.step().unwrap();

  // It falls, bounces and settles
  let mut still = 0;
  let mut ball = None;
  for _ in 0..900 {
    engine.step().unwrap();
    let world = engine.world_mut();
    let mut balls = world.query_filtered::<(&Transform, &Velocity), With<Ball>>();
    let (transform, velocity) = balls.iter(world).next().expect("the ball");
    ball = Some(transform.translation);
    still = if velocity.as_vec2().length() < 1.0 {
      still + 1
    } else {
      0
    };
    if still >= 30 {
      break;
    }
  }
  assert!(still >= 30, "the ball came to rest (last at {ball:?})");
  let at = ball.expect("the ball");
  let below = MapsView::new(engine.world_mut())
    .world_to_cell(map, LAYER, at + Vec2::new(0.0, 12.0))
    .expect("the paint layer");
  assert!(
    row.contains(&below),
    "the ball rests on the painted floor: {at} is over {below}, the floor is {row:?}"
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The endless page's digger, once it has one
fn digger(engine: &mut HeadlessEngine) -> Option<Vec2> {
  let world = engine.world_mut();
  let mut diggers = world.query_filtered::<&Transform, With<Digger>>();
  diggers.iter(world).next().map(|t| t.translation)
}

/// The endless page's ground layer
const GROUND: &str = "ground";

/// The tile at `cell` of the endless world's ground
fn ground_at(engine: &mut HeadlessEngine, map: Entity, cell: IVec2) -> Option<Tile> {
  MapsView::new(engine.world_mut()).tile(map, GROUND, cell)
}

/// The store the endless world's stream keeps changed chunks in
fn saved_chunks(engine: &HeadlessEngine, map: Entity) -> SavedChunks {
  engine
    .world()
    .get::<MapStream>(map)
    .and_then(|stream| stream.saved.clone())
    .expect("the endless world's stream keeps SavedChunks")
}

#[test]
fn the_endless_world_keeps_what_you_dig_and_build_when_you_leave_and_come_back() {
  let Some(mut engine) = engine_on(ENDLESS) else {
    return;
  };
  let map = spawned_map(&mut engine);
  for _ in 0..600 {
    if digger(&mut engine).is_some() {
      break;
    }
    engine.step().unwrap();
  }
  assert!(digger(&mut engine).is_some(), "the digger spawned");
  settle(&mut engine, map);

  // The HUD reads the stream's status: the chunks resident, none in
  // flight, and none saved
  let counted = status(&mut engine, map);
  assert_eq!(counted.resident, resident(&mut engine, map).len());
  let shown = engine.ui_snapshot();
  for line in [
    format!("Resident: {} chunks, 0 in flight", counted.resident),
    "Saved: 0 chunks".to_string(),
  ] {
    assert!(shown.contains(&line), "{line:?} in the HUD");
  }

  // Dig the top ground cell two columns right of the digger, and build a
  // cell of open air three left and two up
  let at = digger(&mut engine).expect("the digger");
  let here = MapsView::new(engine.world_mut())
    .world_to_cell(map, GROUND, at)
    .expect("the ground layer");
  let dig = ground_under(
    &mut engine,
    map,
    GROUND,
    at + Vec2::new(2.0 * CELL, -2.0 * CELL),
  )
  .expect("ground two columns right");
  let screen = cell_on_screen_in(&mut engine, map, GROUND, dig);
  click(&mut engine, screen, MouseButton::Left);
  assert_eq!(
    ground_at(&mut engine, map, dig),
    None,
    "the click dug {dig}"
  );
  let build = (2..=5)
    .map(|up| here + IVec2::new(-3, -up))
    .find(|cell| ground_at(&mut engine, map, *cell).is_none())
    .expect("open air up and left");
  let screen = cell_on_screen_in(&mut engine, map, GROUND, build);
  click(&mut engine, screen, MouseButton::Right);
  assert!(
    ground_at(&mut engine, map, build).is_some(),
    "the right click built {build}"
  );
  // Building where the digger stands builds nothing
  let screen = cell_on_screen_in(&mut engine, map, GROUND, here);
  click(&mut engine, screen, MouseButton::Right);
  assert_eq!(
    ground_at(&mut engine, map, here),
    None,
    "no ground built over the digger"
  );
  let shown = engine.ui_snapshot();
  assert!(shown.contains("Dug 1, built 1"), "the HUD counts the edits");

  // Leaving: E travels far right; the chunks left behind unload, those
  // changed into the stream's SavedChunks, while new ones stream in: the
  // HUD shows the stream's own count in flight each frame, and a chunk
  // lands after it reads more than none
  let edited: BTreeSet<(i32, i32)> = [dig, build]
    .iter()
    .map(|cell| TileGrid::chunk_of(*cell))
    .map(|c| (c.x, c.y))
    .collect();
  engine.tap_key(KeyCode::KeyE);
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  let mut kept = BTreeSet::new();
  let (mut most_in_flight, mut landed_after) = (0, false);
  for _ in 0..600 {
    engine.step().unwrap();
    let events = chunk_events(&engine, &mut cursor);
    for event in &events {
      if let ChunkEvent::Unloading {
        map: m,
        chunk,
        changed: Some(_),
      } = event
        && *m == map
      {
        kept.insert((chunk.x, chunk.y));
      }
    }
    if most_in_flight > 0 {
      landed_after |= events
        .iter()
        .any(|e| matches!(e, ChunkEvent::Loaded { map: m, .. } if *m == map));
    }
    let flying = status(&mut engine, map).in_flight;
    assert_eq!(
      number_before(&engine.ui_snapshot(), " in flight"),
      Some(flying),
      "the HUD shows the stream's count in flight"
    );
    most_in_flight = most_in_flight.max(flying);
    if edited.is_subset(&kept) && landed_after {
      break;
    }
  }
  assert!(
    edited.is_subset(&kept),
    "the edited chunks {edited:?} unloaded changed: {kept:?}"
  );
  assert!(
    most_in_flight > 0 && landed_after,
    "chunks were in flight after the travel ({most_in_flight} at most), and landed after"
  );
  let away = digger(&mut engine).expect("the digger");
  assert!(
    away.x > at.x + 3000.0,
    "the digger travelled far right: from {at} to {away}"
  );
  settle(&mut engine, map);
  let store = saved_chunks(&engine, map);
  for &(x, y) in &edited {
    assert!(
      store.get(IVec2::new(x, y)).is_some(),
      "the store keeps chunk ({x}, {y})"
    );
  }
  let line = format!("Saved: {} chunk", store.len());
  assert!(engine.ui_snapshot().contains(&line), "{line:?} in the HUD");

  // Coming back: Q travels back, and the edited chunks land from the store
  // as the player left them
  engine.tap_key(KeyCode::KeyQ);
  let mut back = BTreeSet::new();
  for _ in 0..600 {
    engine.step().unwrap();
    for event in chunk_events(&engine, &mut cursor) {
      if let ChunkEvent::Loaded { map: m, chunk } = event
        && m == map
      {
        back.insert((chunk.x, chunk.y));
      }
    }
    if edited.is_subset(&back) {
      break;
    }
  }
  assert!(
    edited.is_subset(&back),
    "the edited chunks came back: {back:?}"
  );
  settle(&mut engine, map);
  assert_eq!(ground_at(&mut engine, map, dig), None, "{dig} is still dug");
  assert!(
    ground_at(&mut engine, map, build).is_some(),
    "{build} is still built"
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}
