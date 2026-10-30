//! The demo's pages without a window: each plays for two seconds, spawning
//! its maps without a warning or a failed map, from the demo's folder and
//! from the files the web build embeds; the web build embeds every asset
//! file; the objects page's character walks and jumps into a coin and
//! collects it; the editing page paints terrain where the mouse clicks,
//! through the ground's Wang set with no seams; the world page spawns room
//! 2 as the player walks toward it; and the hot reload page counts a saved
//! map

use ryo2d::core::dev_log::{self, LogLevel};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;
use tilemap_demo::{Coins, MapReloads, Player, demo, embedded};

/// The pages, in the demo's order
const OBJECTS: usize = 5;
const EDITING: usize = 6;
const WORLD: usize = 8;
const BIG_MAP: usize = 9;
const HOT_RELOAD: usize = 10;
const PAGES: usize = 11;

/// The maps each page lands within two seconds, each named by a layer only
/// it has: the platformer's `ground`, the vault's `Collisions`, the
/// garden's `trees`, the procgen map's `tiles`, a room's `terrain` (the
/// first room only, at the start), the big map's `walls`
const LANDS: [&[&str]; PAGES] = [
  &["ground", "Collisions"],
  &["ground"],
  &["ground"],
  &["trees"],
  &["ground"],
  &["ground"],
  &["ground"],
  &["tiles"],
  &["terrain"],
  &["walls"],
  &["ground"],
];

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

/// Step every page for two seconds, checking each lands the maps it should
/// and only those, none fails and none warns
fn every_page_plays(mut engine_on: impl FnMut(usize) -> Option<HeadlessEngine>) {
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
      (expected.len(), expected.len()),
      "page {}: maps landed {landed:?} of {roots} roots",
      page + 1
    );
    for name in *expected {
      assert!(
        landed
          .iter()
          .any(|layers| layers.iter().any(|layer| layer == name)),
        "page {}: no landed map has a layer {name:?}: {landed:?}",
        page + 1
      );
    }
    assert_eq!(spawned, expected.len(), "page {}: Spawned events", page + 1);
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
  let mut embedded: Vec<String> = tilemap_demo::EMBEDDED
    .iter()
    .map(|(path, _)| path.to_string())
    .collect();
  embedded.sort();
  assert_eq!(embedded, on_disk);
  for (path, bytes) in tilemap_demo::EMBEDDED {
    assert_eq!(*bytes, std::fs::read(root.join(path)).unwrap(), "{path}");
  }
}

/// The page's player, once it has one
fn player(engine: &mut HeadlessEngine) -> Option<(Entity, Vec2)> {
  let world = engine.world_mut();
  let mut players = world.query_filtered::<(Entity, &Transform), With<Player>>();
  players
    .iter(world)
    .next()
    .map(|(entity, transform)| (entity, transform.translation))
}

/// Steps until the page has its player
fn wait_for_player(engine: &mut HeadlessEngine) -> (Entity, Vec2) {
  for _ in 0..600 {
    engine.step().unwrap();
    if let Some(found) = player(engine) {
      return found;
    }
  }
  panic!("the page never spawned its player: {:#?}", warnings());
}

/// The coins of the platformer map still in the world, with their `value`
fn coins(engine: &mut HeadlessEngine) -> Vec<(Entity, Option<i64>)> {
  let world = engine.world_mut();
  let mut objects = world.query::<(Entity, &MapObject)>();
  objects
    .iter(world)
    .filter(|(_, object)| object.class.as_deref() == Some("coin"))
    .map(|(entity, object)| (entity, object.properties.int("value")))
    .collect()
}

#[test]
fn the_objects_page_collects_a_coin() {
  let Some(mut engine) = engine_on(OBJECTS) else {
    return;
  };
  wait_for_player(&mut engine);
  let before = coins(&mut engine);
  // Twelve take the project's class default, one is worth 10
  let mut values: Vec<Option<i64>> = before.iter().map(|(_, value)| *value).collect();
  values.sort_unstable();
  let mut wanted = vec![Some(1); 12];
  wanted.push(Some(10));
  assert_eq!(values, wanted, "the map's thirteen coins and their values");
  assert_eq!(
    engine.world().get_resource::<Coins>().map(|c| c.collected),
    Some(0)
  );

  // Run right over the hill, jumping now and then, until a coin is taken
  engine.press_key(KeyCode::KeyD);
  let mut collected = 0;
  for frame in 0..900 {
    if frame % 24 == 0 {
      engine.tap_key(KeyCode::Space);
    }
    engine.step().unwrap();
    collected = engine
      .world()
      .get_resource::<Coins>()
      .map_or(0, |coins| coins.collected);
    if collected > 0 {
      break;
    }
  }
  engine.release_key(KeyCode::KeyD);
  engine.step_frames(2).unwrap();
  assert!(collected > 0, "no coin was collected");
  let tally = *engine.world().get_resource::<Coins>().unwrap();
  let left: Vec<Entity> = coins(&mut engine).iter().map(|(coin, _)| *coin).collect();
  let gone: Vec<i64> = before
    .iter()
    .filter(|(coin, _)| !left.contains(coin))
    .map(|(_, value)| value.unwrap_or_default())
    .collect();
  assert_eq!(
    gone.len(),
    tally.collected as usize,
    "each coin collected is gone: {tally:?}"
  );
  assert_eq!(
    tally.score,
    gone.iter().sum::<i64>(),
    "the score is the gone coins' values"
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// A tile's corner colours in `set` (top-left, top-right, bottom-right,
/// bottom-left), 0 for an empty cell
fn corners(set: &WangSet, tile: Option<Tile>, cell: IVec2) -> [u8; 4] {
  let Some(tile) = tile else {
    return [0; 4];
  };
  assert_eq!(tile.tileset, 1, "a tile from another tileset at {cell}");
  assert_eq!(tile.flip, TileFlip::NONE, "a flipped tile at {cell}");
  let wang = set
    .tiles()
    .iter()
    .find(|wang| wang.tile == tile.id)
    .unwrap_or_else(|| panic!("tile {} at {cell} is not in the Wang set", tile.id));
  let w = wang.wangid;
  [w[7], w[1], w[3], w[5]]
}

/// The root of the page's one map, once it has spawned
fn spawned_map(engine: &mut HeadlessEngine) -> Entity {
  for _ in 0..600 {
    engine.step().unwrap();
    let world = engine.world_mut();
    let mut roots = world.query_filtered::<Entity, (With<MapRoot>, With<MapIds>)>();
    if let Some(root) = roots.iter(world).next() {
      return root;
    }
  }
  panic!("the page's map never spawned: {:#?}", warnings());
}

#[test]
fn the_editing_page_paints_terrain() {
  let Some(mut engine) = engine_on(EDITING) else {
    return;
  };
  let map = spawned_map(&mut engine);
  engine.step_frames(3).unwrap();
  let set = {
    let handle = engine
      .assets()
      .load_now::<Tileset>("tilesets/ground.tsx")
      .expect("the ground tileset");
    engine
      .world()
      .resource::<Assets<Tileset>>()
      .get(&handle)
      .and_then(|tileset| tileset.wang_set("ground").cloned())
      .expect("the ground Wang set")
  };

  // Click the middle of a cell of the open sky
  let cell = IVec2::new(10, 3);
  let at = MapsView::new(engine.world_mut())
    .cell_to_world(map, "ground", cell)
    .expect("the ground layer");
  let screen = CameraMut::main(engine.world_mut())
    .expect("the main camera")
    .world_to_screen(at);
  assert!(
    MapsView::new(engine.world_mut())
      .tile(map, "ground", cell)
      .is_none(),
    "the cell starts empty"
  );
  engine.mouse_move(screen.x, screen.y);
  engine.step().unwrap();
  engine.press_mouse(MouseButton::Left);
  engine.step_frames(2).unwrap();
  engine.release_mouse(MouseButton::Left);
  engine.step_frames(2).unwrap();

  // The open sky (rows 0-6, columns 0-40) holds only the set's tiles, and
  // every two neighbours agree on the corners they share
  let view = MapsView::new(engine.world_mut());
  let (width, height) = (41, 7);
  let mut grid = vec![[0u8; 4]; width * height];
  for y in 0..height {
    for x in 0..width {
      let at = IVec2::new(x as i32, y as i32);
      grid[y * width + x] = corners(&set, view.tile(map, "ground", at), at);
    }
  }
  let corner = |x: usize, y: usize| grid[y * width + x];
  assert_eq!(corner(10, 3), [1; 4], "the clicked cell is ground");
  for y in 2..=4 {
    for x in 9..=11 {
      assert_ne!(corner(x, y), [0; 4], "({x}, {y}) is part of the 3x3 block");
    }
  }
  for y in 0..height {
    for x in 0..width {
      let [_, tr, br, bl] = corner(x, y);
      if x + 1 < width {
        let [rtl, _, _, rbl] = corner(x + 1, y);
        assert_eq!(
          (tr, br),
          (rtl, rbl),
          "a seam between ({x}, {y}) and its right"
        );
      }
      if y + 1 < height {
        let [dtl, dtr, _, _] = corner(x, y + 1);
        assert_eq!((bl, br), (dtl, dtr), "a seam between ({x}, {y}) and below");
      }
    }
  }
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// Where cell `cell` of the editing page's ground layer is on screen; a
/// cell off the map too
fn cell_on_screen(engine: &mut HeadlessEngine, map: Entity, cell: IVec2) -> Vec2 {
  let origin = MapsView::new(engine.world_mut())
    .cell_to_world(map, "ground", IVec2::ZERO)
    .expect("the ground layer");
  let at = origin + cell.as_vec2() * 18.0;
  CameraMut::main(engine.world_mut())
    .expect("the main camera")
    .world_to_screen(at)
}

#[test]
fn the_editing_page_fills_a_drag_begun_off_the_map_and_reads_only_its_cells() {
  let Some(mut engine) = engine_on(EDITING) else {
    return;
  };
  let map = spawned_map(&mut engine);
  engine.step_frames(3).unwrap();

  // Off the map the readout names no cell
  let above = cell_on_screen(&mut engine, map, IVec2::new(6, -3));
  engine.mouse_move(above.x, above.y);
  engine.step_frames(2).unwrap();
  assert!(
    engine.ui_snapshot().contains("Cell -   painted"),
    "a cell off the map is not shown"
  );

  // Shift and a drag from there into the sky paints the rectangle's part
  // on the map, rows 0-2 of columns 6-9
  engine.press_key(KeyCode::ShiftLeft);
  engine.step().unwrap();
  engine.press_mouse(MouseButton::Left);
  engine.step_frames(2).unwrap();
  let inside = cell_on_screen(&mut engine, map, IVec2::new(9, 2));
  engine.mouse_move(inside.x, inside.y);
  engine.step_frames(2).unwrap();
  engine.release_mouse(MouseButton::Left);
  engine.step_frames(2).unwrap();
  engine.release_key(KeyCode::ShiftLeft);
  engine.step().unwrap();
  let view = MapsView::new(engine.world_mut());
  for y in 0..=2 {
    for x in 6..=9 {
      assert!(
        view.tile(map, "ground", IVec2::new(x, y)).is_some(),
        "({x}, {y}) is filled"
      );
    }
  }
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The world levels spawned now, by their index in the world
fn rooms(engine: &mut HeadlessEngine) -> Vec<usize> {
  let world = engine.world_mut();
  let mut levels = world.query::<&WorldLevelOf>();
  let mut rooms: Vec<usize> = levels.iter(world).map(|level| level.level).collect();
  rooms.sort_unstable();
  rooms
}

#[test]
fn the_world_page_spawns_the_next_room() {
  let Some(mut engine) = engine_on(WORLD) else {
    return;
  };
  let (_, start) = wait_for_player(&mut engine);
  engine.step_frames(10).unwrap();
  assert_eq!(
    rooms(&mut engine),
    [0],
    "only the first room, near the player"
  );

  engine.press_key(KeyCode::KeyD);
  let mut reached = false;
  for _ in 0..900 {
    engine.step().unwrap();
    if rooms(&mut engine).contains(&1) {
      reached = true;
      break;
    }
  }
  engine.release_key(KeyCode::KeyD);
  assert!(reached, "room 2 never spawned: {:?}", rooms(&mut engine));
  let (_, now) = player(&mut engine).expect("the player");
  assert!(
    now.x > start.x + 100.0,
    "walked right from {start} to {now}"
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The hot reload page's count of `MapEvent::Reloaded`
fn reloads(engine: &HeadlessEngine) -> Option<u32> {
  engine
    .world()
    .get_resource::<MapReloads>()
    .map(|reloads| reloads.0)
}

#[test]
fn the_hot_reload_page_counts_a_saved_map() {
  const MAP: &str = "maps/platformer.tmx";
  let files = embedded();
  let Some(mut engine) = engine_over(HOT_RELOAD, files.clone()) else {
    return;
  };
  spawned_map(&mut engine);
  assert_eq!(reloads(&engine), Some(0));

  // As a save in the folder does, with hot reload on: the background a
  // shade greener
  let text = tilemap_demo::EMBEDDED
    .iter()
    .find(|(path, _)| *path == MAP)
    .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
    .expect("the map is embedded");
  let saved = text.replacen(
    "backgroundcolor=\"#7ff0a5\"",
    "backgroundcolor=\"#7ff0a6\"",
    1,
  );
  assert_ne!(saved, text);
  files.insert(MAP, saved);
  engine.assets().server().reload(MAP);
  for _ in 0..600 {
    engine.step().unwrap();
    if reloads(&engine) == Some(1) {
      break;
    }
  }
  assert_eq!(reloads(&engine), Some(1), "the reload was counted");
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The number after `label` in the page's UI, up to " ms"
fn ms_after(snapshot: &str, label: &str) -> Option<f64> {
  let start = snapshot.find(label)? + label.len();
  let rest = &snapshot[start..];
  rest[..rest.find(" ms")?].trim().parse().ok()
}

#[test]
fn the_big_map_page_shows_what_a_frame_costs_with_the_map_on_and_off() {
  let Some(mut engine) = engine_on(BIG_MAP) else {
    return;
  };
  engine.step_frames(240).unwrap();
  let shown = engine.ui_snapshot();
  let on = ms_after(&shown, "Map on: total");
  assert!(on.is_some_and(|ms| ms > 0.0), "map on: {on:?}");
  assert!(shown.contains("  render "), "the split is shown");
  assert!(
    shown.contains("Map off: not measured yet"),
    "the map has not been off"
  );

  // Map is the panel's first row, which has the focus
  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(240).unwrap();
  let shown = engine.ui_snapshot();
  let off = ms_after(&shown, "Map off: total");
  assert!(off.is_some_and(|ms| ms > 0.0), "map off: {off:?}");
  assert!(
    ms_after(&shown, "Map on: total").is_some(),
    "the map on's numbers stay"
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}
