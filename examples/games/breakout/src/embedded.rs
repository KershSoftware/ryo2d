//! The game's level files built into it, for the web build (the web has no
//! file system, so `main` gives the engine these as its asset source there)
//! and for the tests, which run from an empty folder

use ryo2d::prelude::*;

/// Each level file, by its asset path, with its bytes: the ten maps, their
/// tileset and its image, and the Tiled project that types the bricks'
/// properties
pub static EMBEDDED: &[(&str, &[u8])] = &[
  (
    "levels/bricks.png",
    include_bytes!("../assets/levels/bricks.png"),
  ),
  (
    "levels/bricks.tsx",
    include_bytes!("../assets/levels/bricks.tsx"),
  ),
  (
    "levels/level1.tmx",
    include_bytes!("../assets/levels/level1.tmx"),
  ),
  (
    "levels/level10.tmx",
    include_bytes!("../assets/levels/level10.tmx"),
  ),
  (
    "levels/level2.tmx",
    include_bytes!("../assets/levels/level2.tmx"),
  ),
  (
    "levels/level3.tmx",
    include_bytes!("../assets/levels/level3.tmx"),
  ),
  (
    "levels/level4.tmx",
    include_bytes!("../assets/levels/level4.tmx"),
  ),
  (
    "levels/level5.tmx",
    include_bytes!("../assets/levels/level5.tmx"),
  ),
  (
    "levels/level6.tmx",
    include_bytes!("../assets/levels/level6.tmx"),
  ),
  (
    "levels/level7.tmx",
    include_bytes!("../assets/levels/level7.tmx"),
  ),
  (
    "levels/level8.tmx",
    include_bytes!("../assets/levels/level8.tmx"),
  ),
  (
    "levels/level9.tmx",
    include_bytes!("../assets/levels/level9.tmx"),
  ),
  (
    "project.tiled-project",
    include_bytes!("../assets/project.tiled-project"),
  ),
];

/// The level files in memory, as the engine's asset source
pub fn embedded() -> MemorySource {
  let files = MemorySource::new();
  for (path, bytes) in EMBEDDED {
    files.insert(path, *bytes);
  }
  files
}
