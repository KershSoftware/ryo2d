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
      ["level", "brick", "tiled", "tmx", "tsx", "map"]
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
