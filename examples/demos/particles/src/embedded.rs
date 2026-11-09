//! The demo's assets built into it, for the web build: the web has no file
//! system, so `main` gives the engine these as its asset source there

use demo_kit::prelude::*;

/// Each file in `assets/`, by its asset path, with its bytes
pub static EMBEDDED: &[(&str, &[u8])] = &[
  (
    "fx/embers.particles.ron",
    include_bytes!("../assets/fx/embers.particles.ron"),
  ),
  (
    "fx/rain.particles.ron",
    include_bytes!("../assets/fx/rain.particles.ron"),
  ),
  (
    "fx/smoke_sheet.png",
    include_bytes!("../assets/fx/smoke_sheet.png"),
  ),
  ("fx/spark.png", include_bytes!("../assets/fx/spark.png")),
  (
    "fx/sparks.particles.ron",
    include_bytes!("../assets/fx/sparks.particles.ron"),
  ),
  (
    "fx/splash.particles.ron",
    include_bytes!("../assets/fx/splash.particles.ron"),
  ),
];

/// The demo's assets in memory, as the engine's asset source: the web
/// build's, and the tests' that read them
pub fn embedded() -> MemorySource {
  let files = MemorySource::new();
  for (path, bytes) in EMBEDDED {
    files.insert(path, *bytes);
  }
  files
}
