//! The demo's pages without a window: each plays for two seconds without a
//! warning or a failed map, from the demo's folder and from the files the
//! web build embeds; the web build embeds every asset file; and the
//! platformer's LDtk level collides exactly as the ASCII level it replaced
//! did (kept below as the reference)

use ryo2d::core::dev_log::{self, LogLevel};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

use character_demo::{ORIGIN, TILE, demo, embedded};

/// The pages, in the demo's order
const PLATFORMER: usize = 0;
const PAGES: usize = 3;

/// Two seconds at the demo's 60 Hz
const FRAMES: u32 = 120;

fn engine_on(page: usize) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(demo().start_page(page).engine_builder())
}

/// The engine on `page`, its assets read from `files` as on the web
fn engine_over(page: usize, files: MemorySource) -> Option<HeadlessEngine> {
  let builder = demo()
    .start_page(page)
    .configure(move |b| b.asset_source(files))
    .engine_builder();
  HeadlessEngine::new_or_skip(builder)
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

// --- The old level, as it was -------------------------------------------------

/// 30 x 30 tiles. The ground has a gap (a lift of its own crosses it, with
/// spikes' worth of kill zone below); a hill of slopes, one-way shelves, a
/// ledge reached by a lift, and a shaft to wall-jump up to the top ledge.
const PLATFORMER_LEVEL: [&str; 30] = [
  "##############################",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#...................######...#",
  "#........................#...#",
  "#........................#...#",
  "#........................#...#",
  "#........................#...#",
  "#........................#...#",
  "#........................#...#",
  "#...#####................#...#",
  "#........................#...#",
  "#..........======........#...#",
  "#........................#...#",
  "#........................#...#",
  "#.................======.#...#",
  "#........................#...#",
  "#............................#",
  "#........====................#",
  "#..../\\......................#",
  "#.../##\\.....................#",
  "#############........#########",
  "#############........#########",
];

/// A tile level from a string grid: `#` solid, `=` one-way, `/` and `\` 45°
/// slopes, anything else empty.
fn level(rows: &[&str]) -> TileColliders {
  let width = rows
    .iter()
    .map(|row| row.chars().count())
    .max()
    .unwrap_or(0);
  let cells = rows
    .iter()
    .flat_map(|row| {
      row
        .chars()
        .chain(std::iter::repeat('.'))
        .take(width)
        .map(|c| match c {
          '#' => TileCell::Solid,
          '=' => TileCell::OneWay,
          '/' => TileCell::Slope(SlopeCorner::BottomRight),
          '\\' => TileCell::Slope(SlopeCorner::BottomLeft),
          _ => TileCell::Empty,
        })
    })
    .collect();
  TileColliders::from_cells(width as u32, rows.len() as u32, Vec2::splat(TILE), cells)
}

// --- The tests ----------------------------------------------------------------

/// Step every page for two seconds, checking none warns and no map fails
fn every_page_plays(mut engine_on: impl FnMut(usize) -> Option<HeadlessEngine>) {
  for page in 0..PAGES {
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
    // Only the platformer's level is a map
    assert_eq!(
      spawned,
      usize::from(page == PLATFORMER),
      "page {}",
      page + 1
    );
    let warned = warnings();
    assert!(warned.is_empty(), "page {}: {warned:#?}", page + 1);
  }
}

#[test]
fn every_page_plays_for_two_seconds_without_a_warning() {
  every_page_plays(engine_on);
}

#[test]
fn every_page_plays_from_the_files_the_web_build_embeds() {
  every_page_plays(|page| engine_over(page, embedded()));
}

/// What the system and the editors leave in the assets folder, which the
/// web build does not embed: dotfiles (macOS's `.DS_Store`) and Tiled's
/// session files
fn is_left_beside_the_assets(name: &str) -> bool {
  name.starts_with('.') || name.ends_with(".tiled-session")
}

#[test]
fn the_web_build_embeds_every_asset_file() {
  let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
  let mut on_disk = Vec::new();
  let mut folders = vec![root.clone()];
  while let Some(folder) = folders.pop() {
    for entry in std::fs::read_dir(&folder).unwrap() {
      let path = entry.unwrap().path();
      let name = path.file_name().unwrap_or_default().to_string_lossy();
      if is_left_beside_the_assets(&name) {
        continue;
      }
      if path.is_dir() {
        folders.push(path);
      } else if name != "README.md" {
        let relative = path.strip_prefix(&root).unwrap();
        on_disk.push(relative.to_string_lossy().replace('\\', "/"));
      }
    }
  }
  on_disk.sort();
  let mut embedded: Vec<String> = character_demo::EMBEDDED
    .iter()
    .map(|(path, _)| path.to_string())
    .collect();
  embedded.sort();
  assert_eq!(embedded, on_disk);
  for (path, bytes) in character_demo::EMBEDDED {
    assert_eq!(*bytes, std::fs::read(root.join(path)).unwrap(), "{path}");
  }
}

/// The platformer's map: its `TileColliders` (on the map's entity, as the
/// layer cells share the map's cell size) and the entity's translation
fn map_colliders(engine: &mut HeadlessEngine) -> (TileColliders, Vec2) {
  let world = engine.world_mut();
  let mut query = world.query::<(&MapRoot, &TileColliders, &Transform)>();
  let mut found = query.iter(world);
  let (_, colliders, transform) = found.next().expect("the platformer's map has colliders");
  assert!(found.next().is_none(), "one map with colliders");
  (colliders.clone(), transform.translation)
}

#[test]
fn the_ldtk_level_collides_exactly_as_the_ascii_level_did() {
  let Some(mut engine) = engine_on(PLATFORMER) else {
    return;
  };
  let mut cursor = MessageCursor::<MapEvent>::default();
  let mut landed = false;
  for _ in 0..FRAMES {
    engine.step().unwrap();
    for event in map_events(&engine, &mut cursor) {
      match event {
        MapEvent::Spawned { .. } => landed = true,
        MapEvent::Failed { error, .. } => panic!("the map failed: {error}"),
        MapEvent::Reloaded { .. } => {}
      }
    }
    if landed {
      break;
    }
  }
  assert!(landed, "the map never landed");

  let (colliders, at) = map_colliders(&mut engine);
  let old = level(&PLATFORMER_LEVEL);
  assert_eq!(at, ORIGIN, "the level sits at ORIGIN");
  assert_eq!((colliders.width(), colliders.height()), (30, 30));
  assert_eq!(
    (colliders.width(), colliders.height()),
    (old.width(), old.height())
  );
  assert_eq!(colliders.tile_size, Vec2::splat(TILE));
  assert_eq!(colliders.tile_size, old.tile_size);
  let wrong: Vec<String> = (0..old.height())
    .flat_map(|y| (0..old.width()).map(move |x| (x, y)))
    .filter(|&(x, y)| colliders.get(x, y) != old.get(x, y))
    .map(|(x, y)| {
      format!(
        "({x}, {y}): {:?}, was {:?}",
        colliders.get(x, y),
        old.get(x, y)
      )
    })
    .collect();
  assert!(wrong.is_empty(), "{wrong:#?}");
}
