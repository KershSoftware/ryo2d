//! The demo's assets built into it, for the web build: the web has no file
//! system, so `main` gives the engine these as its asset source there. The
//! packs' licences ride along, so the shipped build carries them

use demo_kit::prelude::*;

/// Each file in `assets/` but its README, by its asset path, with its bytes
pub static EMBEDDED: &[(&str, &[u8])] = &[
  (
    "images/backdrop.png",
    include_bytes!("../assets/images/backdrop.png"),
  ),
  (
    "images/dungeon-terrain.png",
    include_bytes!("../assets/images/dungeon-terrain.png"),
  ),
  (
    "images/ground.png",
    include_bytes!("../assets/images/ground.png"),
  ),
  (
    "kenney-pixel-platformer/License.txt",
    include_bytes!("../assets/kenney-pixel-platformer/License.txt"),
  ),
  (
    "kenney-pixel-platformer/tilemap-backgrounds_packed.png",
    include_bytes!("../assets/kenney-pixel-platformer/tilemap-backgrounds_packed.png"),
  ),
  (
    "kenney-pixel-platformer/tilemap_packed.png",
    include_bytes!("../assets/kenney-pixel-platformer/tilemap_packed.png"),
  ),
  (
    "kenney-tiny-dungeon/License.txt",
    include_bytes!("../assets/kenney-tiny-dungeon/License.txt"),
  ),
  (
    "kenney-tiny-dungeon/tilemap_packed.png",
    include_bytes!("../assets/kenney-tiny-dungeon/tilemap_packed.png"),
  ),
  (
    "ldtk/dungeon.ldtk",
    include_bytes!("../assets/ldtk/dungeon.ldtk"),
  ),
  (
    "maps/platformer.tmx",
    include_bytes!("../assets/maps/platformer.tmx"),
  ),
  (
    "maps/topdown.tmj",
    include_bytes!("../assets/maps/topdown.tmj"),
  ),
  (
    "project.tiled-project",
    include_bytes!("../assets/project.tiled-project"),
  ),
  (
    "rooms/room-1.tmj",
    include_bytes!("../assets/rooms/room-1.tmj"),
  ),
  (
    "rooms/room-2.tmx",
    include_bytes!("../assets/rooms/room-2.tmx"),
  ),
  (
    "rooms/room-3.tmj",
    include_bytes!("../assets/rooms/room-3.tmj"),
  ),
  (
    "rooms/rooms.world",
    include_bytes!("../assets/rooms/rooms.world"),
  ),
  (
    "tilesets/dungeon-terrain.tsj",
    include_bytes!("../assets/tilesets/dungeon-terrain.tsj"),
  ),
  (
    "tilesets/dungeon.tsj",
    include_bytes!("../assets/tilesets/dungeon.tsj"),
  ),
  (
    "tilesets/ground.tsx",
    include_bytes!("../assets/tilesets/ground.tsx"),
  ),
  (
    "tilesets/platformer.tsx",
    include_bytes!("../assets/tilesets/platformer.tsx"),
  ),
];

/// The demo's assets in memory, as the engine's asset source: the web
/// build's, and the tests' that play from what it embeds
pub fn embedded() -> MemorySource {
  let files = MemorySource::new();
  for (path, bytes) in EMBEDDED {
    files.insert(path, *bytes);
  }
  files
}
