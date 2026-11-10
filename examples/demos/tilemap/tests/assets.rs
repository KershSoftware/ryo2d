//! The demo's authored files, through the engine's own registry loaders:
//! every tileset, map, world and LDtk project loads, and every map and LDtk
//! level spawns, without an error or a warning; and each holds what the
//! demo's pages and the character demo use: layer names and flags, object
//! classes, counts and properties (the door's reference to its switch, class
//! defaults from the Tiled project), complete Wang sets with their names and
//! colours, slopes and a one-way bridge, the world's rooms, and the LDtk
//! level that is the character demo's platformer level cell for cell. The
//! ground's Wang set is complete: painting and erasing with it gives exactly
//! the corners asked for, with no seams and no correction.

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, LogCapture};

/// The character demo's `PLATFORMER_LEVEL` (examples/demos/character):
/// `#` solid, `=` one-way, `/` and `\` slopes
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

const TILESETS: [&str; 4] = [
  "tilesets/platformer.tsx",
  "tilesets/ground.tsx",
  "tilesets/dungeon.tsj",
  "tilesets/dungeon-terrain.tsj",
];

const MAPS: [&str; 5] = [
  "maps/platformer.tmx",
  "maps/topdown.tmj",
  "rooms/room-1.tmj",
  "rooms/room-2.tmx",
  "rooms/room-3.tmj",
];

const WORLD: &str = "rooms/rooms.world";
const LDTK: &str = "ldtk/dungeon.ldtk";
const LDTK_LEVELS: [&str; 2] = ["Platformer", "Vault"];

struct Empty;

impl Scene for Empty {}

/// A headless engine over the demo's assets folder, or `None` without a GPU
fn engine() -> Option<HeadlessEngine> {
  let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
  HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(320, 180)
      .asset_source(DiskSource::new(assets))
      .start_scene(Empty),
  )
}

/// Every warning and error `log` captured
fn warnings(log: &LogCapture) -> Vec<String> {
  log.warnings_and_errors()
}

fn load<T: Asset>(engine: &mut HeadlessEngine, path: &str) -> Handle<T> {
  engine
    .assets()
    .load_now::<T>(path)
    .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn map(engine: &mut HeadlessEngine, path: &str) -> TileMap {
  let handle = load::<TileMap>(engine, path);
  engine
    .world()
    .resource::<Assets<TileMap>>()
    .get(&handle)
    .cloned()
    .unwrap_or_else(|| panic!("{path} is not in its store"))
}

fn tileset(engine: &mut HeadlessEngine, path: &str) -> Tileset {
  let handle = load::<Tileset>(engine, path);
  engine
    .world()
    .resource::<Assets<Tileset>>()
    .get(&handle)
    .cloned()
    .unwrap_or_else(|| panic!("{path} is not in its store"))
}

fn ldtk(engine: &mut HeadlessEngine) -> LdtkProject {
  let handle = load::<LdtkProject>(engine, LDTK);
  engine
    .world()
    .resource::<Assets<LdtkProject>>()
    .get(&handle)
    .cloned()
    .unwrap_or_else(|| panic!("{LDTK} is not in its store"))
}

fn layer<'a>(map: &'a TileMap, name: &str) -> &'a MapLayer {
  map
    .layers
    .iter()
    .find(|layer| layer.info.name == name)
    .unwrap_or_else(|| panic!("no layer {name:?}"))
}

fn layer_names(map: &TileMap) -> Vec<&str> {
  map
    .layers
    .iter()
    .map(|layer| layer.info.name.as_str())
    .collect()
}

fn objects<'a>(map: &'a TileMap, name: &str) -> &'a [MapObjectData] {
  match &layer(map, name).kind {
    LayerKind::Objects(objects) => objects,
    _ => panic!("layer {name:?} holds no objects"),
  }
}

fn of_class<'a>(objects: &'a [MapObjectData], class: &str) -> Vec<&'a MapObjectData> {
  objects
    .iter()
    .filter(|object| object.class.as_deref() == Some(class))
    .collect()
}

fn grid(layer: &MapLayer) -> &TileGrid {
  match &layer.kind {
    LayerKind::Tiles { tiles, .. } => tiles,
    _ => panic!("layer {:?} holds no tiles", layer.info.name),
  }
}

/// Every tile in a tile layer
fn tiles(layer: &MapLayer) -> Vec<Tile> {
  let grid = grid(layer);
  let extent = grid.extent();
  let mut out = Vec::new();
  for y in extent.min.y..extent.max.y {
    for x in extent.min.x..extent.max.x {
      out.extend(grid.get(IVec2::new(x, y)));
    }
  }
  out
}

/// The corner colour combinations (top-right, bottom-right, bottom-left,
/// top-left) over `colors` that no tile of the set has
fn missing_corners(set: &WangSet, colors: &[u8]) -> Vec<[u8; 4]> {
  let mut missing = Vec::new();
  for &tr in colors {
    for &br in colors {
      for &bl in colors {
        for &tl in colors {
          if [tr, br, bl, tl] == [0; 4] {
            continue;
          }
          let wanted = [0, tr, 0, br, 0, bl, 0, tl];
          if !set.tiles().iter().any(|tile| tile.wangid == wanted) {
            missing.push([tr, br, bl, tl]);
          }
        }
      }
    }
  }
  missing
}

fn color_names(set: &WangSet) -> Vec<&str> {
  set
    .colors()
    .iter()
    .map(|color| color.name.as_str())
    .collect()
}

/// Panics unless `points` are three of a `side` px tile's corners, all but
/// `left_out`
fn assert_slope(points: &[Vec2], side: f32, left_out: Vec2, id: u32) {
  assert_eq!(points.len(), 3, "slope {id}: {points:?}");
  let corners = [
    Vec2::ZERO,
    Vec2::new(side, 0.0),
    Vec2::new(side, side),
    Vec2::new(0.0, side),
  ];
  for corner in corners {
    assert_eq!(
      points.contains(&corner),
      corner != left_out,
      "slope {id}: corner {corner} in {points:?}"
    );
  }
}

/// Spawns a root and steps until its map lands
fn land(log: &LogCapture, engine: &mut HeadlessEngine, root: MapRoot, what: &str) {
  let entity = engine.world_mut().spawn((Transform::default(), root)).id();
  for _ in 0..600 {
    engine.step().unwrap();
    if engine.world().get::<MapIds>(entity).is_some() {
      engine.world_mut().despawn(entity);
      return;
    }
  }
  panic!("{what} never landed: {:#?}", warnings(log));
}

#[test]
fn every_file_loads_and_spawns_without_an_error_or_a_warning() {
  let log = ryo2d::testing::capture_log();
  let Some(mut engine) = engine() else {
    return;
  };
  for path in TILESETS {
    load::<Tileset>(&mut engine, path);
  }
  let mut handles = Vec::new();
  for path in MAPS {
    handles.push((path, load::<TileMap>(&mut engine, path)));
  }
  load::<TileWorld>(&mut engine, WORLD);
  let project = load::<LdtkProject>(&mut engine, LDTK);
  for (path, handle) in handles {
    land(&log, &mut engine, MapRoot::new(handle), path);
  }
  for level in LDTK_LEVELS {
    land(
      &log,
      &mut engine,
      MapRoot::ldtk(project.clone(), level),
      level,
    );
  }
  engine.step_frames(10).unwrap();
  assert!(warnings(&log).is_empty(), "{:#?}", warnings(&log));
}

#[test]
fn the_platformer_map_has_its_layers_objects_and_a_door_wired_to_its_switch() {
  let Some(mut engine) = engine() else {
    return;
  };
  let map = map(&mut engine, "maps/platformer.tmx");
  assert_eq!(map.size, UVec2::new(48, 16));
  assert_eq!(map.tile_size, UVec2::new(18, 18));
  assert_eq!(
    map.tilesets.len(),
    2,
    "the Pixel Platformer sheet and the ground"
  );
  assert_eq!(
    layer_names(&map),
    [
      "backdrop", "scenery", "water", "ground", "doors", "hazards", "objects"
    ]
  );

  // Saved by Tiled with its ground layer in base64 and zlib, the other tile
  // layers in CSV, so the demo loads both encodings
  let file = std::fs::read_to_string(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/maps/platformer.tmx"
  ))
  .expect("the platformer map's file");
  let encodings: Vec<(&str, &str)> = file
    .split("<layer ")
    .skip(1)
    .map(|layer| {
      let name = layer
        .split("name=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or("");
      let data = layer
        .split("<data ")
        .nth(1)
        .and_then(|rest| rest.split('>').next())
        .unwrap_or("");
      (name, data)
    })
    .collect();
  assert_eq!(file.matches("compression=\"zlib\"").count(), 1);
  for (name, data) in &encodings {
    let wanted = if *name == "ground" {
      "encoding=\"base64\" compression=\"zlib\""
    } else {
      "encoding=\"csv\""
    };
    assert_eq!(data.trim(), wanted, "layer {name}");
  }
  assert_eq!(
    encodings.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
    ["scenery", "water", "ground", "doors", "hazards"]
  );

  let LayerKind::Image(backdrop) = &layer(&map, "backdrop").kind else {
    panic!("the backdrop is not an image layer");
  };
  assert!(backdrop.repeat_x && !backdrop.repeat_y);
  assert_eq!(layer(&map, "backdrop").info.parallax, Vec2::new(0.5, 1.0));
  for name in ["scenery", "water"] {
    assert!(!layer(&map, name).info.collides, "{name} collides");
  }
  for name in ["ground", "doors"] {
    assert!(layer(&map, name).info.collides, "{name} does not collide");
  }

  // The ground layer mixes both tilesets and holds the slopes and the bridge
  let ground = tiles(layer(&map, "ground"));
  assert!(ground.iter().any(|tile| tile.tileset == 0));
  assert!(ground.iter().any(|tile| tile.tileset == 1));
  assert_eq!(
    ground
      .iter()
      .filter(|t| t.tileset == 1 && t.id == 16)
      .count(),
    3,
    "slopes /"
  );
  assert_eq!(
    ground
      .iter()
      .filter(|t| t.tileset == 1 && t.id == 17)
      .count(),
    3,
    "slopes \\"
  );
  assert_eq!(
    ground
      .iter()
      .filter(|t| t.tileset == 0 && (90..=92).contains(&t.id))
      .count(),
    6
  );
  // Signs and bushes turned the other way
  let scenery = tiles(layer(&map, "scenery"));
  assert!(
    scenery
      .iter()
      .any(|tile| tile.flip.contains(TileFlip::HORIZONTAL))
  );
  // Animated water, a waterfall, and the spikes the game may spawn
  let water = tiles(layer(&map, "water"));
  for id in [33, 73, 34, 54, 74] {
    assert!(water.iter().any(|tile| tile.id == id), "no water tile {id}");
  }
  assert_eq!(tiles(layer(&map, "hazards")).len(), 3);
  assert_eq!(tiles(layer(&map, "doors")).len(), 2);

  let objects = objects(&map, "objects");
  assert_eq!(objects.len(), 16);
  // The coins are tile objects of the coin tile with no class of their
  // own, as Tiled saves them: they take the tile's `coin` class and its
  // value as the map lands (the Objects page's test reads them spawned).
  // Only the prize writes a value of its own
  let coins: Vec<&MapObjectData> = objects
    .iter()
    .filter(|object| object.tile.is_some_and(|t| t.id == 151))
    .collect();
  assert_eq!(coins.len(), 13);
  assert!(coins.iter().all(|coin| coin.class.is_none()));
  let mut values: Vec<Option<i64>> = coins
    .iter()
    .map(|coin| coin.properties.int("value"))
    .collect();
  values.sort_unstable();
  let mut wanted = vec![None; 12];
  wanted.push(Some(10));
  assert_eq!(values, wanted);

  let start = of_class(objects, "player_start");
  assert_eq!(start.len(), 1);
  assert_eq!(start[0].shape, ObjectShape::Point);
  assert_eq!(
    start[0].properties.get("facing"),
    Some(&PropertyValue::Enum {
      ty: "Facing".into(),
      value: "right".into()
    })
  );

  let switches = of_class(objects, "switch");
  assert_eq!(switches.len(), 1);
  assert_eq!(switches[0].properties.bool("on"), Some(false));
  let doors = of_class(objects, "door");
  assert_eq!(doors.len(), 1);
  assert_eq!(
    doors[0].properties.get("switch"),
    Some(&PropertyValue::Object(switches[0].id.clone()))
  );
  assert_eq!(doors[0].properties.bool("open"), Some(false));
  assert_eq!(doors[0].size, Vec2::new(18.0, 36.0));
}

#[test]
fn the_tilesets_carry_complete_wang_sets_animations_slopes_and_a_one_way_bridge() {
  let Some(mut engine) = engine() else {
    return;
  };
  let ground = tileset(&mut engine, "tilesets/ground.tsx");
  assert_eq!(ground.tile_size, UVec2::new(18, 18));
  assert_eq!(ground.tile_count, 20);
  let set = ground.wang_set("ground").expect("the ground Wang set");
  assert_eq!(set.kind(), WangKind::Corner);
  assert_eq!(color_names(set), ["ground"]);
  assert_eq!(missing_corners(set, &[0, 1]), Vec::<[u8; 4]>::new());
  // Two 45° slopes, each a three-corner polygon leaving out a top corner:
  // 16 rises to the right (no top left), 17 to the left (no top right)
  for (id, left_out) in [(16, Vec2::ZERO), (17, Vec2::new(18.0, 0.0))] {
    let shapes = &ground.tile(id).expect("a slope tile").shapes;
    assert_eq!(shapes.len(), 1);
    assert_slope(&shapes[0].points, 18.0, left_out, id);
  }

  let platformer = tileset(&mut engine, "tilesets/platformer.tsx");
  assert_eq!(platformer.tile_count, 180);
  for id in [90, 91, 92, 146, 147] {
    let shapes = &platformer.tile(id).expect("a bridge tile").shapes;
    assert!(shapes.iter().all(|shape| shape.one_way), "tile {id}");
    assert!(!shapes.is_empty(), "tile {id}");
  }
  for id in [151, 33, 34, 54, 74] {
    let tile = platformer.tile(id).expect("an animated tile");
    assert_eq!(tile.animation.len(), 2, "tile {id}");
  }
  assert_eq!(
    platformer.tile(151).and_then(|t| t.class.as_deref()),
    Some("coin")
  );
  assert_eq!(
    platformer.tile(68).and_then(|t| t.class.as_deref()),
    Some("spikes")
  );

  let terrain = tileset(&mut engine, "tilesets/dungeon-terrain.tsj");
  assert_eq!(terrain.tile_size, UVec2::new(16, 16));
  assert_eq!(terrain.tile_count, 40);
  let set = terrain.wang_set("dungeon").expect("the dungeon Wang set");
  assert_eq!(set.kind(), WangKind::Corner);
  assert_eq!(color_names(set), ["wall", "floor"]);
  // A higher colour covers a lower one in procgen's terrain maps: floor
  // covers wall, so corridors one cell wide keep their floor
  assert!(set.color_index("floor") > set.color_index("wall"));
  assert_eq!(missing_corners(set, &[1, 2]), Vec::<[u8; 4]>::new());
  for (id, left_out) in [(31, Vec2::ZERO), (32, Vec2::new(16.0, 0.0))] {
    let shapes = &terrain.tile(id).expect("a slope tile").shapes;
    assert_slope(&shapes[0].points, 16.0, left_out, id);
  }

  let dungeon = tileset(&mut engine, "tilesets/dungeon.tsj");
  assert_eq!(dungeon.tile_count, 132);
  for id in [64, 65] {
    let shapes = &dungeon.tile(id).expect("a statue").shapes;
    assert!(
      shapes.iter().any(|shape| shape.occluder),
      "statue {id} casts no shadow"
    );
    assert!(
      shapes.iter().any(|shape| !shape.occluder),
      "statue {id} has no collision"
    );
  }
  assert_eq!(dungeon.tile(19).map(|t| t.animation.len()), Some(2));
}

/// A tile's corner colours (top-left, top-right, bottom-right, bottom-left)
/// in `set`, 0 for an empty cell; `None` for a tile of another tileset
fn corners(set: &WangSet, tile: Option<Tile>) -> Option<[u8; 4]> {
  let Some(tile) = tile else {
    return Some([0; 4]);
  };
  if tile.tileset != 1 {
    return None;
  }
  let w = set
    .tiles()
    .iter()
    .find(|wang| wang.tile == tile.id)
    .map(|wang| wang.wangid)?;
  Some([w[7], w[1], w[3], w[5]])
}

#[test]
fn painting_and_erasing_the_ground_gives_exactly_the_corners_asked_for() {
  let log = ryo2d::testing::capture_log();
  let Some(mut engine) = engine() else {
    return;
  };
  let handle = load::<TileMap>(&mut engine, "maps/platformer.tmx");
  let set = tileset(&mut engine, "tilesets/ground.tsx")
    .wang_set("ground")
    .cloned()
    .expect("the ground Wang set");
  let root = engine
    .world_mut()
    .spawn((Transform::default(), MapRoot::new(handle)))
    .id();
  for _ in 0..600 {
    if engine.world().get::<MapIds>(root).is_some() {
      break;
    }
    engine.step().unwrap();
  }
  assert!(
    engine.world().get::<MapIds>(root).is_some(),
    "the platformer never landed: {:#?}",
    warnings(&log)
  );
  // Paint and erase cells of the open sky, far from the level's ground, in
  // a fixed order. A paint sets a cell's four corners and an erase clears
  // them; with every corner combination in the set, no neighbour needs
  // correcting, so every corner ends as the last paint or erase left it
  // and neighbours always agree (no seams)
  let (width, height) = (33, 8);
  let mut wanted = vec![0u8; (width + 1) * (height + 1)];
  let mut state = 7u32;
  for step in 0..120 {
    state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    let (cx, cy) = (
      2 + (state >> 8) as usize % 28,
      1 + (state >> 20) as usize % 4,
    );
    let paint = step % 3 != 2;
    for (vx, vy) in [(cx, cy), (cx + 1, cy), (cx, cy + 1), (cx + 1, cy + 1)] {
      wanted[vy * (width + 1) + vx] = u8::from(paint);
    }
    let cell = IVec2::new(cx as i32, cy as i32);
    MapsView::new(engine.world_mut()).set_terrain(
      root,
      "ground",
      cell,
      "ground",
      paint.then_some("ground"),
    );
    let view = MapsView::new(engine.world_mut());
    for y in 0..height {
      for x in 0..width {
        let at = IVec2::new(x as i32, y as i32);
        let found = corners(&set, view.tile(root, "ground", at))
          .unwrap_or_else(|| panic!("step {step}: a tile outside the set at {at}"));
        let corner = |vx: usize, vy: usize| wanted[vy * (width + 1) + vx];
        let asked = [
          corner(x, y),
          corner(x + 1, y),
          corner(x + 1, y + 1),
          corner(x, y + 1),
        ];
        assert_eq!(found, asked, "step {step}: corners of {at}");
      }
    }
  }
  assert!(warnings(&log).is_empty(), "{:#?}", warnings(&log));
}

#[test]
fn the_top_down_map_sorts_its_trees_and_floats_its_clouds() {
  let Some(mut engine) = engine() else {
    return;
  };
  let map = map(&mut engine, "maps/topdown.tmj");
  assert_eq!(map.size, UVec2::new(30, 20));
  assert_eq!(map.tile_size, UVec2::new(16, 16));
  assert_eq!(map.tilesets.len(), 2);
  assert_eq!(
    layer_names(&map),
    ["backdrop", "paths", "trees", "clouds", "objects"]
  );
  let LayerKind::Image(backdrop) = &layer(&map, "backdrop").kind else {
    panic!("the backdrop is not an image layer");
  };
  assert!(backdrop.repeat_x);
  let trees = layer(&map, "trees");
  assert!(trees.info.y_sorted && trees.info.collides);
  assert!(
    tiles(trees)
      .iter()
      .any(|tile| tile.tileset == 1 && tile.id == 126)
  );
  let clouds = &layer(&map, "clouds").info;
  assert_eq!(clouds.parallax, Vec2::new(1.25, 1.25));
  assert!(!clouds.collides && clouds.occludes == Some(false) && !clouds.lit);
  assert_eq!(of_class(objects(&map, "objects"), "player_start").len(), 1);
}

#[test]
fn the_world_places_three_rooms_side_by_side() {
  let Some(mut engine) = engine() else {
    return;
  };
  let handle = load::<TileWorld>(&mut engine, WORLD);
  let levels = engine
    .world()
    .resource::<Assets<TileWorld>>()
    .get(&handle)
    .map(|world| world.layout.levels.clone())
    .expect("the world");
  let names: Vec<&str> = levels.iter().map(|level| level.name.as_str()).collect();
  assert_eq!(names, ["room-1.tmj", "room-2.tmx", "room-3.tmj"]);
  for (index, level) in levels.iter().enumerate() {
    assert_eq!(level.position, Vec2::new(320.0 * index as f32, 0.0));
    assert_eq!(level.size, Vec2::new(320.0, 192.0));
  }
  assert!(
    levels[0]
      .neighbours
      .iter()
      .any(|n| n.level == 1 && n.direction == NeighbourDirection::East)
  );
  assert!(
    levels[2]
      .neighbours
      .iter()
      .any(|n| n.level == 1 && n.direction == NeighbourDirection::West)
  );
  for path in ["rooms/room-1.tmj", "rooms/room-2.tmx", "rooms/room-3.tmj"] {
    let room = map(&mut engine, path);
    assert_eq!(room.size, UVec2::new(20, 12), "{path}");
    assert_eq!(
      layer_names(&room),
      ["terrain", "faces", "props", "objects"],
      "{path}"
    );
    assert_eq!(layer(&room, "terrain").info.occludes, Some(true), "{path}");
    let props = &layer(&room, "props").info;
    assert!(props.y_sorted && props.occludes == Some(false), "{path}");
  }
  let first = map(&mut engine, "rooms/room-1.tmj");
  assert_eq!(
    of_class(objects(&first, "objects"), "player_start").len(),
    1
  );
}

#[test]
fn the_ldtk_project_holds_the_character_level_and_a_vault_of_entities() {
  let Some(mut engine) = engine() else {
    return;
  };
  let project = ldtk(&mut engine);
  let names: Vec<&str> = project
    .levels
    .iter()
    .map(|level| level.identifier.as_str())
    .collect();
  assert_eq!(names, LDTK_LEVELS);

  let level = project.level("Platformer").expect("the platformer level");
  assert_eq!(level.size, UVec2::new(480, 480));
  assert_eq!(level.map.tile_size, UVec2::new(16, 16));
  let collisions = layer(&level.map, "Collisions");
  assert!(collisions.info.collides);
  let LayerKind::Tiles {
    tiles: grid,
    intgrid: Some(intgrid),
    ..
  } = &collisions.kind
  else {
    panic!("Collisions is not an IntGrid layer");
  };
  let mut names: Vec<(i32, &str)> = intgrid
    .names
    .iter()
    .map(|(value, name)| (*value, name.as_str()))
    .collect();
  names.sort_unstable();
  assert_eq!(
    names,
    [
      (1, "solid"),
      (2, "one_way"),
      (3, "slope_bottom_right"),
      (4, "slope_bottom_left")
    ]
  );
  assert_eq!(intgrid.size, UVec2::new(30, 30));
  for (y, row) in PLATFORMER_LEVEL.iter().enumerate() {
    for (x, c) in row.chars().enumerate() {
      let wanted = match c {
        '#' => 1,
        '=' => 2,
        '/' => 3,
        '\\' => 4,
        _ => 0,
      };
      let cell = IVec2::new(x as i32, y as i32);
      assert_eq!(intgrid.values[y * 30 + x], wanted, "cell {cell}");
      // the auto-layer draws every filled cell and nothing else
      assert_eq!(grid.get(cell).is_some(), wanted != 0, "auto tile at {cell}");
    }
  }
  let entities = objects(&level.map, "Entities");
  assert_eq!(of_class(entities, "Player").len(), 1);
  assert_eq!(of_class(entities, "Pickup").len(), 9);
  assert_eq!(of_class(entities, "Checkpoint").len(), 2);

  let vault = project.level("Vault").expect("the vault");
  assert_eq!(vault.fields.string("title"), Some("The Vault"));
  let entities = objects(&vault.map, "Entities");
  let chests = of_class(entities, "Chest");
  assert_eq!(chests.len(), 2);
  assert!(chests.iter().any(|chest| {
    chest.properties.get("loot")
      == Some(&PropertyValue::Enum {
        ty: "Loot".into(),
        value: "Gold".into(),
      })
      && chest.properties.int("count") == Some(25)
  }));
  let switches = of_class(entities, "Switch");
  assert_eq!(switches.len(), 1);
  let doors = of_class(entities, "Door");
  assert_eq!(doors.len(), 1);
  assert_eq!(
    doors[0].properties.get("switch"),
    Some(&PropertyValue::Object(switches[0].id.clone()))
  );
  let signs = of_class(entities, "Sign");
  assert!(
    signs[0]
      .properties
      .string("text")
      .is_some_and(|t| t.contains('\n'))
  );
  let patrols = of_class(entities, "Patrol");
  assert!(matches!(
    patrols[0].properties.get("path"),
    Some(PropertyValue::List(points)) if points.len() == 4
  ));
  assert_eq!(patrols[0].properties.float("speed"), Some(40.0));
  let torches = of_class(entities, "Torch");
  assert_eq!(torches.len(), 2);
  assert!(
    torches
      .iter()
      .all(|torch| torch.properties.color("color").is_some())
  );

  let world = &project.worlds[0];
  let at = |name: &str| {
    world
      .levels
      .iter()
      .position(|level| level.name == name)
      .expect("a level of the world")
  };
  let (platformer, vault) = (at("Platformer"), at("Vault"));
  assert!(
    world.levels[platformer]
      .neighbours
      .iter()
      .any(|n| n.level == vault && n.direction == NeighbourDirection::East)
  );
  assert!(
    world.levels[vault]
      .neighbours
      .iter()
      .any(|n| n.level == platformer && n.direction == NeighbourDirection::West)
  );
}
