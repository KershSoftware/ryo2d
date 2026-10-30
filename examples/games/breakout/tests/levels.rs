//! Breakout's levels: the Tiled maps hold the bricks the text files held (but
//! for the seven Cyrillic cells of levels 5 and 7, which are the bricks they
//! were meant to be), spawned as the game's blocks; and they load from the
//! files the web build embeds, with no file on disk

use std::path::PathBuf;
use std::sync::OnceLock;

use breakout::prelude::*;
use breakout::{EMBEDDED, embedded};
use ryo2d::core::dev_log::{self, LogLevel};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// What each letter of the table is: colour, hits and points
const LETTERS: [(char, &str, u32, u32); 8] = [
  ('R', "Red", 1, 10),
  ('O', "Orange", 1, 20),
  ('Y', "Yellow", 1, 30),
  ('G', "Green", 1, 40),
  ('B', "Blue", 1, 50),
  ('P', "Purple", 1, 60),
  ('W', "White", 2, 100),
  ('S', "Silver", 3, 200),
];

/// Runs from an empty folder (the process's working directory, so every
/// test shares it): a game over saves the high score there, the player's is
/// never touched, and no asset is on disk, so every file the game reads is
/// the embedded one
fn empty_folder() {
  static FOLDER: OnceLock<PathBuf> = OnceLock::new();
  FOLDER.get_or_init(|| {
    let folder = std::env::temp_dir().join(format!("breakout-levels-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::env::set_current_dir(&folder).unwrap();
    folder
  });
}

/// A game on `level`, its files read from the embedded copies as on the web
fn engine_on(level: u32) -> Option<HeadlessEngine> {
  empty_folder();
  HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(SCREEN_WIDTH, SCREEN_HEIGHT)
      .fixed_hz(120)
      .asset_source(embedded())
      .start_scene(GameScene::new(0).at_level(level)),
  )
}

/// Every warning or error about the levels in the dev log (it is the
/// process's, so other tests' land in it; the sounds are not embedded, and
/// their warnings are not the levels')
fn level_problems() -> Vec<String> {
  dev_log::get_entries()
    .into_iter()
    .filter(|entry| entry.level >= LogLevel::Warn)
    .map(|entry| entry.message)
    .filter(|message| {
      let message = message.to_lowercase();
      // (the broken maps of the fallback tests warn on purpose)
      !message.contains("nothere")
        && !message.contains("broken")
        && ["level", "brick", "tiled", "tmx", "tsx", "map"]
          .iter()
          .any(|word| message.contains(word))
    })
    .collect()
}

/// A brick as the game holds it: centre, colour, hits and points
type Brick = (f32, f32, String, u32, u32);

/// The bricks in the world, by row then column
fn bricks(engine: &mut HeadlessEngine) -> Vec<Brick> {
  let world = engine.world_mut();
  let mut query = world.query::<(&Block, &Transform)>();
  let mut found: Vec<Brick> = query
    .iter(world)
    .map(|(block, at)| {
      (
        at.translation.x,
        at.translation.y,
        format!("{:?}", block.color),
        block.hits,
        block.points,
      )
    })
    .collect();
  found.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
  found
}

/// The bricks a level's table says: each letter's cell centre, as the text
/// loader placed them
fn expected(rows: &[&str]) -> Vec<Brick> {
  let mut bricks = Vec::new();
  for (row, line) in rows.iter().enumerate() {
    for (col, letter) in line.chars().enumerate() {
      let Some(&(_, color, hits, points)) = LETTERS.iter().find(|(l, ..)| *l == letter) else {
        continue;
      };
      bricks.push((
        BLOCK_LEFT_OFFSET + col as f32 * (BLOCK_WIDTH + BLOCK_PADDING),
        BLOCK_TOP_OFFSET + row as f32 * (BLOCK_HEIGHT + BLOCK_PADDING),
        color.to_string(),
        hits,
        points,
      ));
    }
  }
  bricks
}

/// Steps until the level's bricks are in the world (the map lands a few
/// frames in), then a few frames more
fn land(engine: &mut HeadlessEngine) -> Vec<Brick> {
  for _ in 0..600 {
    engine.step().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    if !bricks(engine).is_empty() {
      break;
    }
  }
  engine.step_frames(5).unwrap();
  bricks(engine)
}

#[test]
fn every_level_spawns_the_bricks_its_text_file_held() {
  for (index, rows) in LEVELS.iter().enumerate() {
    let level = index as u32 + 1;
    let Some(mut engine) = engine_on(level) else {
      return;
    };
    let found = land(&mut engine);
    let want = expected(rows);
    assert!(!want.is_empty(), "level {level}'s table has bricks");
    assert_eq!(found.len(), want.len(), "level {level}: how many bricks");
    for (found, want) in found.iter().zip(&want) {
      assert_eq!(
        found, want,
        "level {level}: a brick (centre x, y, colour, hits, points)"
      );
    }
  }
}

#[test]
fn a_brick_is_a_fixed_bouncing_rectangle_drawn_over_the_background() {
  let Some(mut engine) = engine_on(1) else {
    return;
  };
  assert_eq!(land(&mut engine).len(), 19);
  let world = engine.world_mut();
  let mut query = world.query::<(&Block, &Collider, &Renderable, Has<RigidBody>)>();
  let mut seen = 0;
  for (_, collider, renderable, body) in query.iter(world) {
    assert!(body, "a brick is a rigid body");
    assert_eq!(
      *collider,
      Collider::rect(BLOCK_WIDTH, BLOCK_HEIGHT)
        .with_restitution(1.0)
        .with_friction(0.0)
    );
    assert_eq!(renderable.z_order, 5);
    seen += 1;
  }
  assert_eq!(seen, 19);
}

#[test]
fn levels_load_on_the_web_from_embedded_files() {
  // The embedded files are the files in assets/ the levels read, no more
  // and no fewer
  let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
  let mut on_disk: Vec<String> = std::fs::read_dir(assets.join("levels"))
    .unwrap()
    .map(|entry| format!("levels/{}", entry.unwrap().file_name().to_string_lossy()))
    .collect();
  on_disk.push("project.tiled-project".to_string());
  on_disk.sort();
  let mut embedded_paths: Vec<String> = EMBEDDED.iter().map(|(path, _)| path.to_string()).collect();
  embedded_paths.sort();
  assert_eq!(embedded_paths, on_disk);
  for (path, bytes) in EMBEDDED {
    assert_eq!(*bytes, std::fs::read(assets.join(path)).unwrap(), "{path}");
  }

  // With nothing on disk, every level plays from them: its bricks spawn,
  // and nothing about the levels is warned
  for level in 1..=MAX_LEVEL {
    let Some(mut engine) = engine_on(level) else {
      return;
    };
    let found = land(&mut engine);
    assert_eq!(
      found.len(),
      expected(LEVELS[level as usize - 1]).len(),
      "level {level}"
    );
  }
  assert!(level_problems().is_empty(), "{:#?}", level_problems());
}

/// A game on `level` over `files`
fn engine_over(files: MemorySource, level: u32) -> Option<HeadlessEngine> {
  empty_folder();
  HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(SCREEN_WIDTH, SCREEN_HEIGHT)
      .fixed_hz(120)
      .asset_source(files)
      .start_scene(GameScene::new(0).at_level(level)),
  )
}

/// A map whose tileset has no image: it loads, and its landing fails
const BROKEN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.11" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" width="1" height="1" tilewidth="84" tileheight="29" infinite="0" nextlayerid="2" nextobjectid="1">
 <tileset firstgid="1" source="nothere.tsx"/>
 <layer id="1" name="bricks" width="1" height="1">
  <data encoding="csv">
1
</data>
 </layer>
</map>
"#;

/// The broken map's tileset
const NO_IMAGE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.11" tiledversion="1.12.2" name="nothere" tilewidth="84" tileheight="29" tilecount="1" columns="1">
 <image source="nothere.png" width="84" height="29"/>
</tileset>
"#;

/// Steps `frames` frames, a few milliseconds each so the landing worker keeps up
fn steps(engine: &mut HeadlessEngine, frames: u32) {
  for _ in 0..frames {
    engine.step().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
  }
}

#[test]
fn a_level_that_fails_to_load_falls_back_to_level_1_and_is_not_cleared() {
  let files = embedded();
  files.insert("levels/level3.tmx", BROKEN);
  files.insert("levels/nothere.tsx", NO_IMAGE);
  let Some(mut engine) = engine_over(files, 3) else {
    return;
  };
  let found = land(&mut engine);
  assert_eq!(found, expected(LEVELS[0]), "level 1's bricks stand in");
  // Not cleared, so not advanced to level 4 or won
  steps(&mut engine, 120);
  assert_eq!(bricks(&mut engine), expected(LEVELS[0]));
}

#[test]
fn a_map_that_fails_to_land_falls_back_to_level_1_and_is_not_cleared() {
  let Some(mut engine) = engine_on(3) else {
    return;
  };
  // The first frame spawns level 3's map; its landing fails
  engine.step().unwrap();
  let world = engine.world_mut();
  let mut roots = world.query_filtered::<Entity, With<MapRoot>>();
  let root = roots.iter(world).next().expect("level 3's map is spawned");
  let error = std::sync::Arc::new(AssetError::NotFound {
    path: AssetPath::parse("levels/broken-on-purpose.tmx").unwrap(),
  });
  world
    .resource_mut::<Messages<MapEvent>>()
    .write(MapEvent::Failed { map: root, error });
  let found = land(&mut engine);
  assert_eq!(found, expected(LEVELS[0]), "level 1's bricks stand in");
  steps(&mut engine, 120);
  assert_eq!(bricks(&mut engine), expected(LEVELS[0]), "not cleared");
}

#[test]
fn when_level_1_fails_too_the_level_is_never_cleared() {
  let files = embedded();
  files.insert("levels/level1.tmx", BROKEN);
  files.insert("levels/nothere.tsx", NO_IMAGE);
  let Some(mut engine) = engine_over(files, 1) else {
    return;
  };
  // Many frames: a level counted cleared would put level 2's bricks up
  steps(&mut engine, 200);
  assert!(bricks(&mut engine).is_empty());
}

#[test]
fn a_level_is_not_cleared_before_its_bricks_land() {
  for before in 1..=3 {
    let Some(mut engine) = engine_on(1) else {
      return;
    };
    // The frames before the map lands (it may be quick) press nothing
    for _ in 0..before {
      engine.step().unwrap();
    }
    let found = land(&mut engine);
    assert_eq!(
      found,
      expected(LEVELS[0]),
      "still level 1 after {before} frames"
    );
  }
}

#[test]
fn the_seven_cyrillic_cells_are_the_bricks_they_were_meant_to_be() {
  // (level, line, column from 0, colour): the text files held a Cyrillic
  // look-alike there, which was no brick. Lines count the rows of bricks
  // from 1 as the table does; the files' own line numbers add comments
  let fixed: [(u32, usize, usize, &str); 7] = [
    (5, 2, 11, "Orange"),
    (7, 2, 2, "Purple"),
    (7, 2, 4, "Purple"),
    (7, 2, 6, "Purple"),
    (7, 2, 8, "Purple"),
    (7, 2, 10, "Purple"),
    (7, 7, 9, "Purple"),
  ];
  for level in [5, 7] {
    let Some(mut engine) = engine_on(level) else {
      return;
    };
    let found = land(&mut engine);
    for &(_, row, col, color) in fixed.iter().filter(|f| f.0 == level) {
      let x = BLOCK_LEFT_OFFSET + col as f32 * (BLOCK_WIDTH + BLOCK_PADDING);
      let y = BLOCK_TOP_OFFSET + (row - 1) as f32 * (BLOCK_HEIGHT + BLOCK_PADDING);
      assert!(
        found.iter().any(|b| b.0 == x && b.1 == y && b.2 == color),
        "level {level}: a {color} brick at row {row}, column {col}"
      );
    }
  }
}

// What `convert-levels` printed from the text files (the seven Cyrillic cells
// put right): each level's rows, a letter a brick and a dot an empty cell
const LEVELS: [&[&str]; 10] = [
  // level 1: 19 bricks
  &[
    "..RRRRRRRR..",
    "..O.O.O.O...",
    "...........",
    ".Y.Y.Y.Y.Y.",
    "..G.....G..",
  ],
  // level 2: 32 bricks
  &[
    ".RRRRRRRRRR.",
    ".O.O.O.O.O.",
    ".YYYYYYYYYY.",
    "..G.G.G.G..",
    "...B.B.B...",
  ],
  // level 3: 48 bricks
  &[
    "WWWWWWWWWWWW",
    "OOOOOOOOOOOO",
    ".Y.Y.Y.Y.Y.Y",
    ".G.G.G.G.G.G",
    "BBBBBBBBBBBB",
  ],
  // level 4: 54 bricks
  &[
    "RRRR..RRRR..",
    ".OOOOOOOOOO.",
    "YYWWYYWWYYWY",
    "GGGGGGGGGGGG",
    ".B.B.B.B.B.B",
    ".P.P.P.P.P.P",
  ],
  // level 5: 72 bricks
  &[
    "WWWWWWWWWWWW",
    "RORORORORORO",
    "WYYYYYYYYYWW",
    "WGGGGGGGGGGW",
    "WBBBBBBBBBBW",
    "PPPPPPPPPPPP",
  ],
  // level 6: 80 bricks
  &[
    "SSSS..SSSS..",
    "WWWWWWWWWWWW",
    "OOWOOWOOWOOW",
    "YYYYYYYYYYYY",
    "GWGWGWGWGWGW",
    "BBBBBBBBBBBB",
    "PPWPPWPPWPPW",
  ],
  // level 7: 83 bricks
  &[
    "SWSWSWSWSWSW",
    "WSPSPSPSPSPS",
    "SWOWOWOWOWS",
    "WSYSYSYSYSYW",
    "SWGWGWGWGWGS",
    "WSBSBSBSBSBW",
    "PPPPPPPPPPPP",
  ],
  // level 8: 87 bricks
  &[
    "SSSSSSSSSSSS",
    "WWWWWWWWWWWW",
    "SSSSSSSSSSSS",
    "WWWWWWWWWWWW",
    "O.O.O.O.O.O.",
    "YYYYYYYYYY.Y",
    "G.GGGGGGGG.G",
    "BBBBBBBBBBBB",
  ],
  // level 9: 108 bricks
  &[
    "SSSWWWWWWSSS",
    "SWWWWWWWWWWS",
    "WSSSSSSSSSWS",
    "WSWOOOOOOWSW",
    "WSWYYYYYYWSW",
    "WSWGGGGGGGWS",
    "WSWBBBBBBBWS",
    "WSPPPPPPPPSW",
    "SWWWWWWWWWWS",
  ],
  // level 10: 139 bricks
  &[
    "SSSSSSSSSSSS",
    "SSSSSSSSSSSS",
    "SWWWWWWWWWWS",
    "SWSSSSSSSSWS",
    "SWSWWWWWSWWS",
    "SWSWOOOOWSWS",
    "SWSWOYYWOSWS",
    "SWSWOYYSWS.S",
    "SWSWOGGSWS.S",
    "SWSWSSSWS..S",
    "SWSWWWWSSSSS",
    "SWSSSSSSSSS.",
  ],
];
