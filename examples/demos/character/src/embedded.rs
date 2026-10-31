//! The demo's assets built into it, for the web build: the web has no file
//! system, so `main` gives the engine these as its asset source there. The
//! pack's licence rides along, so the shipped build carries it

use demo_kit::prelude::*;

/// Each file in `assets/` but its README, by its asset path, with its bytes
pub static EMBEDDED: &[(&str, &[u8])] = &[
  (
    "images/dungeon-terrain.png",
    include_bytes!("../assets/images/dungeon-terrain.png"),
  ),
  (
    "kenney-tiny-dungeon/License.txt",
    include_bytes!("../assets/kenney-tiny-dungeon/License.txt"),
  ),
  (
    "ldtk/platformer.ldtk",
    include_bytes!("../assets/ldtk/platformer.ldtk"),
  ),
];

/// The demo's assets in memory, as the engine's asset source: the web
/// build's, and the tests' that read the embedded files
pub fn embedded() -> MemorySource {
  let files = MemorySource::new();
  for (path, bytes) in EMBEDDED {
    files.insert(path, *bytes);
  }
  files
}
