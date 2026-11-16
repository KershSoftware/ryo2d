//! The demo's assets built into it, for the web build: the web has no file
//! system, so `main` gives the engine these as its asset source there

use demo_kit::prelude::*;

/// Each file in `assets/`, by its asset path, with its bytes
pub static EMBEDDED: &[(&str, &[u8])] = &[
  (
    "audio/footstep.sound.ron",
    include_bytes!("../assets/audio/footstep.sound.ron"),
  ),
  (
    "audio/step-1.wav",
    include_bytes!("../assets/audio/step-1.wav"),
  ),
  (
    "audio/step-2.wav",
    include_bytes!("../assets/audio/step-2.wav"),
  ),
  (
    "audio/step-3.wav",
    include_bytes!("../assets/audio/step-3.wav"),
  ),
  (
    "audio/step-4.wav",
    include_bytes!("../assets/audio/step-4.wav"),
  ),
  ("banner.png", include_bytes!("../assets/banner.png")),
  (
    "banner.skeleton.ron",
    include_bytes!("../assets/banner.skeleton.ron"),
  ),
  ("cape.png", include_bytes!("../assets/cape.png")),
  ("hero.png", include_bytes!("../assets/hero.png")),
  (
    "hero.skeleton.ron",
    include_bytes!("../assets/hero.skeleton.ron"),
  ),
  ("reacher.png", include_bytes!("../assets/reacher.png")),
  (
    "reacher.skeleton.ron",
    include_bytes!("../assets/reacher.skeleton.ron"),
  ),
  ("sword.png", include_bytes!("../assets/sword.png")),
  ("tail.png", include_bytes!("../assets/tail.png")),
  (
    "tail.skeleton.ron",
    include_bytes!("../assets/tail.skeleton.ron"),
  ),
  ("walker.png", include_bytes!("../assets/walker.png")),
  (
    "walker.skeleton.ron",
    include_bytes!("../assets/walker.skeleton.ron"),
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
