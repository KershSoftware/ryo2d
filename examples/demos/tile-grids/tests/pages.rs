//! The demo's pages without a window: each lands its map and plays for
//! five seconds, without a warning or a failed map. The meadow is a Tiled
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
//! in), and coming back finds them as the player left them.
//!
//! On the other grids: the isometric town's character draws behind a tall
//! wall from the cell behind it and over it from the cell in front (pixels
//! sampled where the two overlap), and the town's walls stop it at their
//! diamonds; the hex board picks the hex under the pointer, a click selects
//! one, and the board marks the selected hex's range and ring, the line to
//! the hovered hex and the hovered hex's neighbours, each as the layer's
//! grid (`MapsView::grid`) works them out, with the distance in the HUD;
//! and the oblique map's character stops at a skewed wall, on the map and
//! on its mirror

use std::collections::BTreeSet;

use image::RgbaImage;
use ryo2d::core::dev_log::{self, LogLevel};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;
use tile_grids_demo::{Ball, ChunkLabel, Digger, Runner, demo};

/// The pages, in the demo's order
const MEADOW: usize = 0;
const PAINT: usize = 1;
const ENDLESS: usize = 2;
const TOWN: usize = 3;
const STAGGERED: usize = 4;
const HEX: usize = 5;
const OBLIQUE: usize = 6;
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

fn engine_on(page: usize) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(demo().start_page(page).engine_builder())
}

/// Every warning in the dev log. It is the process's, so the other tests'
/// land in it too; none of them expects a warning either
fn warnings() -> Vec<String> {
  dev_log::get_entries()
    .into_iter()
    .filter(|entry| entry.level >= LogLevel::Warn)
    .map(|entry| entry.message)
    .collect()
}

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

/// The layer names of each map that has landed, bottom first, and how many
/// roots there are
fn landed_maps(engine: &mut HeadlessEngine) -> (Vec<Vec<String>>, usize) {
  let world = engine.world_mut();
  let mut roots = world.query::<(Entity, Has<MapIds>, &MapRoot)>();
  let roots: Vec<(Entity, bool)> = roots
    .iter(world)
    .map(|(entity, landed, _)| (entity, landed))
    .collect();
  let mut layers = world.query::<&MapLayerRef>();
  let landed = roots
    .iter()
    .filter(|(_, landed)| *landed)
    .map(|(root, _)| {
      // By index, bottom first: a query's order is the world's layout
      let mut found: Vec<(usize, String)> = layers
        .iter(world)
        .filter(|layer| layer.map == *root)
        .map(|layer| (layer.index, layer.name.clone()))
        .collect();
      found.sort();
      found.into_iter().map(|(_, name)| name).collect()
    })
    .collect();
  (landed, roots.len())
}

/// Whether the page's map has landed
fn has_landed(engine: &mut HeadlessEngine) -> bool {
  let world = engine.world_mut();
  let mut roots = world.query_filtered::<Entity, (With<MapRoot>, With<MapIds>)>();
  roots.iter(world).next().is_some()
}

/// A step, then a millisecond's rest: a map's padded tilesets are made off
/// the main thread, a few hundred milliseconds for big pictures, which a
/// headless run stepping as fast as it can outpaces
fn step_resting(engine: &mut HeadlessEngine) {
  engine.step().unwrap();
  std::thread::sleep(std::time::Duration::from_millis(1));
}

/// The most steps a page's map may take to land (about 20 s)
const LANDING: u32 = 15_000;

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

/// The root of the page's one map, once it has landed
fn spawned_map(engine: &mut HeadlessEngine) -> Entity {
  for _ in 0..LANDING {
    step_resting(engine);
    let world = engine.world_mut();
    let mut roots = world.query_filtered::<Entity, (With<MapRoot>, With<MapIds>)>();
    if let Some(root) = roots.iter(world).next() {
      return root;
    }
  }
  panic!("the page's map never landed: {:#?}", warnings());
}

/// A copy of the spawned layer `name`'s grid
fn layer_grid(engine: &mut HeadlessEngine, map: Entity, name: &str) -> TileGrid {
  let layer = MapsView::new(engine.world_mut())
    .layer_entity(map, name)
    .unwrap_or_else(|| panic!("no layer {name:?}"));
  engine
    .world()
    .get::<TileLayerData>(layer)
    .map(|data| data.tiles.clone())
    .unwrap_or_else(|| panic!("layer {name:?} has no tiles"))
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

fn click(engine: &mut HeadlessEngine, at: Vec2, button: MouseButton) {
  engine.mouse_move(at.x, at.y);
  engine.step().unwrap();
  engine.press_mouse(button);
  engine.step_frames(2).unwrap();
  engine.release_mouse(button);
  engine.step_frames(2).unwrap();
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

// ---- the other grids --------------------------------------------------------

/// The page's character (the one top-down walker), once it has one
fn walker(engine: &mut HeadlessEngine) -> Option<Entity> {
  let world = engine.world_mut();
  let mut walkers = world.query_filtered::<Entity, With<TopDownMovement>>();
  walkers.iter(world).next()
}

/// Steps until the page's character has spawned
fn wait_for_walker(engine: &mut HeadlessEngine) -> Entity {
  for _ in 0..600 {
    engine.step().unwrap();
    if let Some(walker) = walker(engine) {
      return walker;
    }
  }
  panic!("the character never spawned: {:#?}", warnings());
}

/// The character's sprite: its y-sorted child
fn walker_sprite(engine: &mut HeadlessEngine, walker: Entity) -> Entity {
  let world = engine.world_mut();
  let mut sprites = world.query_filtered::<(Entity, &ChildOf), (With<Renderable>, With<YSort>)>();
  sprites
    .iter(world)
    .find(|(_, parent)| parent.parent() == walker)
    .map(|(sprite, _)| sprite)
    .expect("the character's y-sorted sprite")
}

/// The character's collider radius, and the gap its controller keeps from
/// surfaces (1% of its height unless set)
fn walker_reach(engine: &HeadlessEngine, walker: Entity) -> (f32, f32) {
  let world = engine.world();
  let radius = match world.get::<Collider>(walker).map(|c| &c.shape) {
    Some(ColliderShape::Circle { radius }) => *radius,
    other => panic!("the character's collider is a circle: {other:?}"),
  };
  let skin = world
    .get::<CharacterController>(walker)
    .and_then(|controller| controller.skin)
    .unwrap_or(0.02 * radius);
  (radius, skin)
}

/// Puts the character's feet at `at`, still
fn teleport(engine: &mut HeadlessEngine, walker: Entity, at: Vec2) {
  let world = engine.world_mut();
  if let Some(mut transform) = world.get_mut::<Transform>(walker) {
    transform.translation = at;
  }
  if let Some(mut velocity) = world.get_mut::<Velocity>(walker) {
    *velocity = Velocity::zero();
  }
}

fn walker_at(engine: &HeadlessEngine, walker: Entity) -> Vec2 {
  engine
    .world()
    .get::<Transform>(walker)
    .expect("the character's transform")
    .translation
}

/// The grid of `layer` of `map`, as the page reads it
fn grid_of(engine: &mut HeadlessEngine, map: Entity, layer: &str) -> LayerGrid {
  MapsView::new(engine.world_mut())
    .grid(map, layer)
    .unwrap_or_else(|| panic!("the layer {layer:?}"))
}

/// Every cell of the page's bounded map
fn map_cells(engine: &mut HeadlessEngine, map: Entity) -> Vec<IVec2> {
  let bounds = MapsView::new(engine.world_mut())
    .bounds(map)
    .expect("the map's bounds");
  (bounds.min.y..bounds.max.y)
    .flat_map(|y| (bounds.min.x..bounds.max.x).map(move |x| IVec2::new(x, y)))
    .collect()
}

/// Whether the tile at `cell` of `layer` has collision shapes: a wall, or
/// on the town's ground, water
fn collides_at(engine: &mut HeadlessEngine, map: Entity, layer: &str, cell: IVec2) -> bool {
  let maps = MapsView::new(engine.world_mut());
  maps
    .tile(map, layer, cell)
    .and_then(|tile| maps.tile_data(map, tile))
    .is_some_and(|data| !data.shapes.is_empty())
}

/// Whether the town's or the island's `cell` is open ground: ground there,
/// not water, and no prop
fn open_ground(engine: &mut HeadlessEngine, map: Entity, cell: IVec2) -> bool {
  let ground = MapsView::new(engine.world_mut())
    .tile(map, "ground", cell)
    .is_some();
  let prop = MapsView::new(engine.world_mut())
    .tile(map, "props", cell)
    .is_some();
  ground && !prop && !collides_at(engine, map, "ground", cell)
}

/// How far `point` is from the convex polygon `outline` (0 inside it)
fn distance_to(outline: &[Vec2], point: Vec2) -> f32 {
  let n = outline.len();
  let mut inside = true;
  let mut nearest = f32::INFINITY;
  for i in 0..n {
    let (a, b) = (outline[i], outline[(i + 1) % n]);
    let edge = b - a;
    if edge.length_squared() == 0.0 {
      continue;
    }
    let t = ((point - a).dot(edge) / edge.length_squared()).clamp(0.0, 1.0);
    nearest = nearest.min(point.distance(a + edge * t));
    // Clockwise on screen (+y down): the inside is on each edge's right
    if edge.perp_dot(point - a) < 0.0 {
      inside = false;
    }
  }
  if inside { 0.0 } else { nearest }
}

/// Where `at` (world pixels) is in the frame
fn to_screen(engine: &mut HeadlessEngine, at: Vec2) -> Vec2 {
  CameraMut::main(engine.world_mut())
    .expect("the main camera")
    .world_to_screen(at)
}

/// The frame's pixels in `area` (screen pixels) where `a` and `b` differ
fn differing(a: &RgbaImage, b: &RgbaImage, area: Rect) -> BTreeSet<(u32, u32)> {
  let (x0, y0) = (area.x.max(0.0) as u32, area.y.max(0.0) as u32);
  let x1 = (area.right() as u32).min(a.width());
  let y1 = (area.bottom() as u32).min(a.height());
  (y0..y1)
    .flat_map(|y| (x0..x1).map(move |x| (x, y)))
    .filter(|&(x, y)| a.get_pixel(x, y) != b.get_pixel(x, y))
    .collect()
}

/// The frame with the character's feet at `at`, its sprite shown or not
fn shot(
  engine: &mut HeadlessEngine,
  walker: Entity,
  sprite: Entity,
  at: Vec2,
  shown: bool,
) -> RgbaImage {
  teleport(engine, walker, at);
  if let Some(mut renderable) = engine.world_mut().get_mut::<Renderable>(sprite) {
    renderable.visible = shown;
  }
  engine.step_frames(10).unwrap();
  assert!(
    walker_at(engine, walker).distance(at) < 0.01,
    "the character stays where it was put"
  );
  engine.frame().unwrap()
}

#[test]
fn the_town_character_draws_behind_a_wall_and_in_front_of_it() {
  let Some(mut engine) = engine_on(TOWN) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let walker = wait_for_walker(&mut engine);
  let sprite = walker_sprite(&mut engine, walker);
  // The Zoom row has the panel's focus: in to a half, so the knight's
  // pixels are two a texel; and the pointer over the panel, clear of the
  // view
  engine.tap_key(KeyCode::ArrowRight);
  engine.mouse_move(900.0, 100.0);

  // A tall wall with open ground in the cell behind it (above it on
  // screen) and in the cell in front of it (below it)
  let wall = map_cells(&mut engine, map)
    .into_iter()
    .find(|&cell| {
      collides_at(&mut engine, map, "props", cell)
        && open_ground(&mut engine, map, cell - IVec2::ONE)
        && open_ground(&mut engine, map, cell + IVec2::ONE)
    })
    .expect("a wall with open ground behind it and in front of it");
  let layer = grid_of(&mut engine, map, "props");
  let outline = layer.outline_world(wall);
  let (top, bottom) = outline
    .points()
    .iter()
    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
      (lo.min(p.y), hi.max(p.y))
    });
  // Straight above and below its middle, clear of its diamond by more
  // than the character's radius
  let centre = layer.cell_center_world(wall);
  let (radius, _) = walker_reach(&engine, walker);
  let behind = Vec2::new(centre.x, top - radius - 15.0);
  let front = Vec2::new(centre.x, bottom + radius + 5.0);
  assert_eq!(layer.cell_at_world(behind), wall - IVec2::ONE, "behind");
  assert_eq!(layer.cell_at_world(front), wall + IVec2::ONE, "in front");

  // The frames at each place, the sprite hidden and shown, with the wall
  // and then without it
  let mut frames = Vec::new();
  for at in [behind, front] {
    frames.push((
      shot(&mut engine, walker, sprite, at, false),
      shot(&mut engine, walker, sprite, at, true),
    ));
  }
  MapsView::new(engine.world_mut()).set_tile(map, "props", wall, None);
  let mut bare = Vec::new();
  for at in [behind, front] {
    bare.push((
      shot(&mut engine, walker, sprite, at, false),
      shot(&mut engine, walker, sprite, at, true),
    ));
  }

  for (i, (at, name)) in [(behind, "behind"), (front, "in front of")]
    .into_iter()
    .enumerate()
  {
    // Around the character on screen: where it draws (shown against
    // hidden, the wall gone) and where the wall draws (with it against
    // without), and where both do
    teleport(&mut engine, walker, at);
    engine.step_frames(10).unwrap();
    let (a, b) = (
      to_screen(&mut engine, at - Vec2::new(60.0, 90.0)),
      to_screen(&mut engine, at + Vec2::new(60.0, 10.0)),
    );
    let area = Rect::new(a.x, a.y, b.x - a.x, b.y - a.y);
    let (hidden, shown) = &frames[i];
    let (bare_hidden, bare_shown) = &bare[i];
    let character = differing(bare_hidden, bare_shown, area);
    let wall_art = differing(hidden, bare_hidden, area);
    let both: Vec<(u32, u32)> = character.intersection(&wall_art).copied().collect();
    assert!(
      both.len() >= 20,
      "{name} the wall: the character and the wall overlap on screen ({} pixels)",
      both.len()
    );
    let covered = both
      .iter()
      .filter(|&&(x, y)| shown.get_pixel(x, y) == hidden.get_pixel(x, y))
      .count();
    let over = both
      .iter()
      .filter(|&&(x, y)| shown.get_pixel(x, y) == bare_shown.get_pixel(x, y))
      .count();
    if at == behind {
      assert_eq!(
        covered,
        both.len(),
        "behind the wall, the wall covers the character where they overlap"
      );
    } else {
      assert_eq!(
        over,
        both.len(),
        "in front of the wall, the character draws over it where they overlap"
      );
      assert!(covered < both.len(), "and so it shows");
    }
  }
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The cells within two steps of `cell` whose tiles collide: the town's
/// and island's walls (props) and water (ground), the oblique dungeon's
/// walls
fn solid_near(
  engine: &mut HeadlessEngine,
  map: Entity,
  layers: &[&str],
  cell: IVec2,
) -> Vec<IVec2> {
  let mut out = Vec::new();
  for dy in -2..=2 {
    for dx in -2..=2 {
      let at = cell + IVec2::new(dx, dy);
      if layers
        .iter()
        .any(|layer| collides_at(engine, map, layer, at))
      {
        out.push(at);
      }
    }
  }
  out
}

/// The character's distance from the nearest outline of `cells` on
/// `layer`'s grid, after it holds `keys` for a second; asserts it is not
/// inside one
fn pushed_into(
  engine: &mut HeadlessEngine,
  walker: Entity,
  layer: &LayerGrid,
  cells: &[IVec2],
  keys: &[KeyCode],
) -> f32 {
  for key in keys {
    engine.press_key(*key);
  }
  engine.step_frames(60).unwrap();
  let at = walker_at(engine, walker);
  for key in keys {
    engine.release_key(*key);
  }
  engine.step_frames(5).unwrap();
  let mut nearest = f32::INFINITY;
  for &cell in cells {
    let outline = layer.outline_world(cell);
    let d = distance_to(outline.points(), at);
    assert!(d > 0.0, "the character at {at} went into cell {cell}");
    nearest = nearest.min(d);
  }
  nearest
}

#[test]
fn the_town_walls_stop_the_character() {
  let Some(mut engine) = engine_on(TOWN) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let walker = wait_for_walker(&mut engine);
  engine.mouse_move(900.0, 100.0);
  // A run of walls along x (a band down and right on screen), with open
  // ground down and left of its second wall, where the character starts
  let wall = map_cells(&mut engine, map)
    .into_iter()
    .find(|&cell| {
      (-1..=2).all(|dx| collides_at(&mut engine, map, "props", cell + IVec2::new(dx, 0)))
        && open_ground(&mut engine, map, cell + IVec2::Y)
    })
    .expect("a run of four walls with open ground down and left of one");
  let layer = grid_of(&mut engine, map, "props");
  let start = layer.cell_center_world(wall + IVec2::Y);
  teleport(&mut engine, walker, start);
  engine.step_frames(5).unwrap();

  // W and D walk it up and right into the band's lower edge: it stops
  // touching the walls' diamonds, its skin away, not short of them and not
  // through them
  let (radius, skin) = walker_reach(&engine, walker);
  let solid = solid_near(&mut engine, map, &["props", "ground"], wall);
  let gap = pushed_into(
    &mut engine,
    walker,
    &layer,
    &solid,
    &[KeyCode::KeyW, KeyCode::KeyD],
  );
  assert!(
    (gap - (radius + skin)).abs() < 1.0,
    "the character stops at the walls' diamonds: {gap} from them, its radius {radius} and skin {skin}"
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The cells of `layer` holding a tile, within the map's bounds
fn marked(engine: &mut HeadlessEngine, map: Entity, layer: &str) -> BTreeSet<(i32, i32)> {
  let grid = layer_grid(engine, map, layer);
  map_cells(engine, map)
    .into_iter()
    .filter(|cell| grid.get(*cell).is_some())
    .map(|cell| (cell.x, cell.y))
    .collect()
}

/// `cells` within the map's bounds
fn on_board(engine: &mut HeadlessEngine, map: Entity, cells: Vec<IVec2>) -> BTreeSet<(i32, i32)> {
  let bounds = MapsView::new(engine.world_mut())
    .bounds(map)
    .expect("the board's bounds");
  cells
    .into_iter()
    .filter(|c| c.cmpge(bounds.min).all() && c.cmplt(bounds.max).all())
    .map(|c| (c.x, c.y))
    .collect()
}

#[test]
fn the_hex_board_picks_the_hex_under_the_pointer_and_shows_its_range() {
  let Some(mut engine) = engine_on(HEX) else {
    return;
  };
  let map = spawned_map(&mut engine);
  engine.step_frames(3).unwrap();
  let layer = grid_of(&mut engine, map, "board");
  let world_at = |engine: &mut HeadlessEngine, screen: Vec2| {
    CameraMut::main(engine.world_mut())
      .expect("the main camera")
      .screen_to_world(screen)
  };

  // A click selects the hex under the view's middle
  let middle = world_at(&mut engine, Vec2::new(260.0, 320.0));
  let selected = layer.cell_at_world(middle);
  let at = to_screen(&mut engine, layer.cell_center_world(selected));
  click(&mut engine, at, MouseButton::Left);
  let shown = engine.ui_snapshot();
  let line = format!("Selected: hex {}, {}", selected.x, selected.y);
  assert!(shown.contains(&line), "{line:?} in {shown}");

  // The pointer over a hex three steps away, on the board and in the view,
  // near one of its corners: the hex under it is picked
  let bounds = MapsView::new(engine.world_mut())
    .bounds(map)
    .expect("the board's bounds");
  let hovered = layer
    .grid
    .ring(selected, 3)
    .into_iter()
    .find(|cell| {
      let screen = to_screen(&mut engine, layer.cell_center_world(*cell));
      cell.cmpge(bounds.min).all()
        && cell.cmplt(bounds.max).all()
        && (40.0..480.0).contains(&screen.x)
        && (110.0..540.0).contains(&screen.y)
    })
    .expect("a hex three steps away in the view");
  let corner = layer.outline_world(hovered).points()[0];
  let centre = layer.cell_center_world(hovered);
  let near_corner = to_screen(&mut engine, centre + (corner - centre) * 0.8);
  engine.mouse_move(near_corner.x, near_corner.y);
  engine.step_frames(3).unwrap();
  let shown = engine.ui_snapshot();
  for line in [
    format!("Pointer: hex {}, {}", hovered.x, hovered.y),
    format!("Distance: {} steps", layer.grid.distance(selected, hovered)),
  ] {
    assert!(shown.contains(&line), "{line:?} in {shown}");
  }

  // The marks are the grid's: the selected hex's range and ring at the
  // radius, the line to the hovered hex, and the hovered hex's neighbours
  let check = |engine: &mut HeadlessEngine, radius: u32| {
    let grid = layer.grid;
    let expected = [
      ("range", grid.range(selected, radius)),
      ("ring", grid.ring(selected, radius)),
      ("line", grid.line(selected, hovered)),
      ("neighbours", grid.neighbours(hovered).collect()),
    ];
    for (name, cells) in expected {
      let cells = on_board(engine, map, cells);
      assert!(!cells.is_empty(), "{name} has cells on the board");
      assert_eq!(
        marked(engine, map, name),
        cells,
        "the {name} at radius {radius}"
      );
    }
  };
  check(&mut engine, 2);
  // The Radius row has the panel's focus: one more
  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(3).unwrap();
  check(&mut engine, 3);
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_oblique_character_collides_with_a_skewed_wall() {
  let Some(mut engine) = engine_on(OBLIQUE) else {
    return;
  };
  // The map's own skew, then (the Skew row has the panel's focus) its
  // mirror's
  for mirror in [false, true] {
    if mirror {
      engine.tap_key(KeyCode::ArrowRight);
    }
    let map = spawned_map(&mut engine);
    let walker = wait_for_walker(&mut engine);
    engine.mouse_move(900.0, 100.0);
    let skew = MapsView::new(engine.world_mut())
      .grid(map, "walls")
      .map(|layer| layer.grid.shape);
    let Some(GridShape::Oblique { skew }) = skew else {
      panic!("the dungeon is an oblique map: {skew:?}");
    };
    assert_ne!(skew.x, 0, "skewed across its rows");
    assert_eq!(skew.x < 0, mirror, "the mirror skews the other way");

    // A wall in a column of walls, with floor two cells to its left: D
    // walks the character from there into it, and it stops at the wall's
    // skewed edge
    let wall = map_cells(&mut engine, map)
      .into_iter()
      .find(|&cell| {
        (-1..=1).all(|dy| collides_at(&mut engine, map, "walls", cell + IVec2::new(0, dy)))
          && (1..=2).all(|dx| {
            let floor = cell - IVec2::new(dx, 0);
            !collides_at(&mut engine, map, "walls", floor)
              && MapsView::new(engine.world_mut())
                .tile(map, "floor", floor)
                .is_some()
          })
      })
      .expect("a wall in a column with floor to its left");
    let layer = grid_of(&mut engine, map, "walls");
    teleport(
      &mut engine,
      walker,
      layer.cell_center_world(wall - IVec2::new(2, 0)),
    );
    engine.step_frames(5).unwrap();
    let (radius, skin) = walker_reach(&engine, walker);
    let solid = solid_near(&mut engine, map, &["walls"], wall);
    let gap = pushed_into(&mut engine, walker, &layer, &solid, &[KeyCode::KeyD]);
    assert!(
      (gap - (radius + skin)).abs() < 1.0,
      "skew {skew}: the character stops at the skewed wall: {gap} from it, its radius {radius} and skin {skin}"
    );
    // The wall's left edge leans by the skew: the character met it there
    let outline = layer.outline_world(wall);
    let left: Vec<Vec2> = {
      let mut points = outline.points().to_vec();
      points.sort_by(|a, b| a.x.total_cmp(&b.x));
      points.truncate(2);
      points.sort_by(|a, b| a.y.total_cmp(&b.y));
      points
    };
    assert!(
      ((left[1].x - left[0].x) - skew.x as f32).abs() < 0.01,
      "the wall's left edge leans by the skew, its bottom further right: {left:?}"
    );
  }
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_staggered_and_speed_pages_land_their_maps() {
  // The island's setting switches it to its staggered-X copy, a map of its
  // own; the speed page's choice builds the chosen grid's map, and only it
  for (page, layer) in [(STAGGERED, "props"), (SPEED, "ground")] {
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    let first = spawned_map(&mut engine);
    let shape = |engine: &mut HeadlessEngine, map: Entity| grid_of(engine, map, layer).grid.shape;
    let before = shape(&mut engine, first);
    engine.tap_key(KeyCode::ArrowRight);
    engine.step_frames(2).unwrap();
    let mut after = None;
    for _ in 0..LANDING {
      step_resting(&mut engine);
      let (landed, roots) = landed_maps(&mut engine);
      if landed.len() == 1 && roots == 1 {
        let world = engine.world_mut();
        let mut maps = world.query_filtered::<Entity, (With<MapRoot>, With<MapIds>)>();
        let map = maps.iter(world).next().expect("the map");
        if map != first {
          after = Some(shape(&mut engine, map));
          break;
        }
      }
    }
    let after = after.expect("the setting spawned another map");
    assert_ne!(before, after, "page {}: another grid", page + 1);
  }
  assert!(warnings().is_empty(), "{:#?}", warnings());
}
