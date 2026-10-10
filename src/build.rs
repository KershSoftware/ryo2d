//! Build script that tracks the Ryo.toml values `ryo2d build` embeds.
//!
//! `ryo2d build` passes RYO_EMBED_* environment variables to cargo; the engine
//! reads them with `option_env!` (see `core::config::resolve::COMPILED_EMBEDDED`).
//! Build scripts can't import crate code, so this list is kept in sync by a test.

const EMBEDDED_KEYS: &[&str] = &[
  "RYO_EMBED_RESOLUTION",
  "RYO_EMBED_VSYNC",
  "RYO_EMBED_DISPLAY_MODE",
  "RYO_EMBED_MASTER_VOLUME",
  "RYO_EMBED_MUSIC_VOLUME",
  "RYO_EMBED_SFX_VOLUME",
  "RYO_EMBED_CONSOLE",
  "RYO_EMBED_TITLE",
  "RYO_EMBED_SHOW_FPS",
  "RYO_EMBED_SHOW_COLLIDERS",
  "RYO_EMBED_SHOW_VELOCITIES",
  "RYO_EMBED_SHOW_ORIGINS",
  "RYO_EMBED_SHOW_FRAME_TIMING",
  "RYO_EMBED_DISABLE_DEBUG_KEYS",
  "RYO_EMBED_ENABLE_PROFILER",
  "RYO_EMBED_DEV_WINDOW_KEYS",
  "RYO_EMBED_LOGGER_WINDOW",
  "RYO_EMBED_BACKGROUND_FPS",
  "RYO_EMBED_BACKGROUND_RENDERING",
];

fn main() {
  for key in EMBEDDED_KEYS {
    println!("cargo:rerun-if-env-changed={key}");
    if let Ok(value) = std::env::var(key) {
      println!("cargo:rustc-env={key}={value}");
    }
  }
}
