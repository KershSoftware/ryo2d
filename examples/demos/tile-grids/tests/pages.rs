//! The demo's pages without a window: each plays for five seconds, landing
//! its map without a warning or a failed map; the meadow's chunks lie at
//! negative cells, none of them empty, and the HUD shows its bounds and
//! chunks; its runner runs left across chunks at negative cells and back,
//! the camera keeping it in view; a click on the paint page paints the cell
//! under the pointer (at negative cells), making its chunk, and a right
//! click erases it; and the ball dropped over painted ground comes to rest
//! on it

use std::collections::BTreeSet;

use ryo2d::core::dev_log::{self, LogLevel};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;
use tile_grids_demo::{Ball, Runner, demo};

/// The pages, in the demo's order
const MEADOW: usize = 0;
const PAINT: usize = 1;
const PAGES: usize = 2;

/// The meadow's file
const MEADOW_FILE: &str = "maps/endless-meadow.tmx";

/// The map each page lands within five seconds, named by layers only it
/// has: the meadow's `scenery` and `ground`, the paint page's `ground`
const LANDS: [&[&str]; PAGES] = [&["scenery", "water", "ground"], &["ground"]];

/// Five seconds at the demo's 60 Hz
const FRAMES: u32 = 300;

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

/// The layer names of each map that has landed, and how many roots there are
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
      layers
        .iter(world)
        .filter(|layer| layer.map == *root)
        .map(|layer| layer.name.clone())
        .collect()
    })
    .collect();
  (landed, roots.len())
}

#[test]
fn every_page_plays_for_five_seconds_without_a_warning() {
  for (page, expected) in LANDS.iter().enumerate() {
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    let mut cursor = MessageCursor::<MapEvent>::default();
    let mut spawned = 0;
    for _ in 0..FRAMES {
      engine.step().unwrap();
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
  for _ in 0..600 {
    engine.step().unwrap();
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
    .map(|data| data.grid.clone())
    .unwrap_or_else(|| panic!("layer {name:?} has no tiles"))
}

/// Every tile of a chunk of `grid`
fn chunk_tiles(grid: &TileGrid, chunk: IVec2) -> usize {
  let side = CHUNK_SIZE as i32;
  let origin = chunk * side;
  (0..side)
    .flat_map(|y| (0..side).map(move |x| origin + IVec2::new(x, y)))
    .filter(|cell| grid.get(*cell).is_some())
    .count()
}

#[test]
fn the_meadow_s_chunks_lie_at_negative_cells_and_the_hud_shows_its_bounds() {
  let Some(mut engine) = engine_on(MEADOW) else {
    return;
  };
  let map = spawned_map(&mut engine);
  engine.step_frames(3).unwrap();

  // The asset: an infinite map, its bounds reaching negative cells
  let bounds = {
    let handle = engine
      .assets()
      .load_now::<TileMap>(MEADOW_FILE)
      .expect("the meadow");
    let assets = engine.world().resource::<Assets<TileMap>>();
    let meadow = assets.get(&handle).expect("the meadow is loaded");
    assert!(meadow.infinite, "the meadow is an infinite map");
    assert_eq!(meadow.size, UVec2::ZERO, "an infinite map has no size");
    meadow.bounds()
  };
  assert!(
    bounds.min.x < 0 && bounds.min.y < 0,
    "bounds reach negative cells: {bounds:?}"
  );

  // Its layers: unbounded, with chunks left of and above the origin, and
  // none empty (Tiled leaves empty chunks out of the file, and the engine
  // makes a chunk only where a tile lands)
  let mut chunks = BTreeSet::new();
  for name in ["scenery", "water", "ground"] {
    let grid = layer_grid(&mut engine, map, name);
    assert_eq!(grid.bounds(), None, "{name} is unbounded");
    for chunk in grid.chunk_coords() {
      assert!(
        chunk_tiles(&grid, chunk) > 0,
        "{name}'s chunk {chunk} is empty"
      );
      chunks.insert((chunk.x, chunk.y));
    }
  }
  let ground = layer_grid(&mut engine, map, "ground");
  let ground_chunks: Vec<IVec2> = ground.chunk_coords().collect();
  assert!(
    ground_chunks.iter().any(|c| c.x < -1),
    "ground left of the origin by more than a chunk: {ground_chunks:?}"
  );
  assert!(
    ground_chunks.iter().any(|c| c.y < 0),
    "ground above the origin: {ground_chunks:?}"
  );

  // The HUD names the bounds (last cells included) and counts the chunks
  // the layers have of those the bounds reach
  let shown = engine.ui_snapshot();
  let last = bounds.max - IVec2::ONE;
  let size = TileGrid::chunk_of(last) - TileGrid::chunk_of(bounds.min) + IVec2::ONE;
  let bounds_line = format!(
    "Bounds: cells ({}, {}) to ({}, {})",
    bounds.min.x, bounds.min.y, last.x, last.y
  );
  assert!(shown.contains(&bounds_line), "{bounds_line:?} in {shown}");
  let chunks_line = format!(
    "Chunks: {} of the {} the bounds reach",
    chunks.len(),
    size.x * size.y
  );
  assert!(shown.contains(&chunks_line), "{chunks_line:?} in {shown}");
  assert!(
    chunks.len() < (size.x * size.y) as usize,
    "the bounds hold chunks the map does not have"
  );
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
fn the_meadow_s_overlay_and_bounds_follow_a_save() {
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
  let chunks_shown = |engine: &mut HeadlessEngine| {
    let shown = engine.ui_snapshot();
    let at = shown.find("Chunks: ").expect("the chunks line") + "Chunks: ".len();
    let count: String = shown[at..]
      .chars()
      .take_while(char::is_ascii_digit)
      .collect();
    count.parse::<usize>().expect("a count")
  };
  let before = chunks_shown(&mut engine);

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
    chunks_shown(&mut engine),
    before + 1,
    "the overlay has the new chunk"
  );
  let bounds = MapsView::new(engine.world_mut())
    .bounds(map)
    .expect("bounds");
  assert_eq!(bounds.max.x, far.x + 1, "the bounds reach the new tile");
  let shown = engine.ui_snapshot();
  let last = bounds.max - IVec2::ONE;
  let line = format!(
    "Bounds: cells ({}, {}) to ({}, {})",
    bounds.min.x, bounds.min.y, last.x, last.y
  );
  assert!(shown.contains(&line), "{line:?} in {shown}");
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The meadow's runner, once it has one
fn runner(engine: &mut HeadlessEngine) -> Option<Vec2> {
  let world = engine.world_mut();
  let mut runners = world.query_filtered::<&Transform, With<Runner>>();
  runners.iter(world).next().map(|t| t.translation)
}

#[test]
fn the_runner_runs_left_across_negative_chunks_and_back_in_view() {
  let Some(mut engine) = engine_on(MEADOW) else {
    return;
  };
  let mut start = None;
  for _ in 0..600 {
    engine.step().unwrap();
    start = runner(&mut engine);
    if start.is_some() {
      break;
    }
  }
  let start = start.expect("the runner spawned");
  let cell = 18.0;
  let chunk = CHUNK_SIZE as f32 * cell;

  // Left, across a chunk edge into chunk -2, then back right
  let (mut min_x, mut now) = (start.x, start);
  for _ in 0..1500 {
    engine.step().unwrap();
    now = runner(&mut engine).expect("the runner");
    min_x = min_x.min(now.x);
    let screen = CameraMut::main(engine.world_mut())
      .expect("the main camera")
      .world_to_screen(now);
    assert!(
      (0.0..=960.0).contains(&screen.x) && (0.0..=600.0).contains(&screen.y),
      "the runner at {now} is in view (on screen at {screen})"
    );
    if min_x < -chunk && now.x > min_x + 10.0 * cell {
      break;
    }
  }
  assert!(
    min_x < -chunk,
    "the runner reached chunk -2 (x < {}): it got to {min_x}",
    -chunk
  );
  assert!(
    now.x > min_x + 10.0 * cell,
    "the runner turned back: at {} after reaching {min_x}",
    now.x
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The paint page's layer
const LAYER: &str = "ground";

/// Where `cell` of the paint page's layer is on screen
fn cell_on_screen(engine: &mut HeadlessEngine, map: Entity, cell: IVec2) -> Vec2 {
  let at = MapsView::new(engine.world_mut())
    .cell_to_world(map, LAYER, cell)
    .expect("the paint layer");
  CameraMut::main(engine.world_mut())
    .expect("the main camera")
    .world_to_screen(at)
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
