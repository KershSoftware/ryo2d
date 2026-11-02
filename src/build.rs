//! Build script that tracks the Ryo.toml values `ryo2d build` embeds, and
//! lists the agent skills the CLI ships.
//!
//! `ryo2d build` passes RYO_EMBED_* environment variables to cargo; the engine
//! reads them with `option_env!` (see `core::config::resolve::COMPILED_EMBEDDED`).
//! Build scripts can't import crate code, so this list is kept in sync by a test.

const EMBEDDED_KEYS: &[&str] = &[
  "RYO_EMBED_RESOLUTION",
  "RYO_EMBED_PIXEL_ART",
  "RYO_EMBED_VSYNC",
  "RYO_EMBED_DISPLAY_MODE",
  "RYO_EMBED_MASTER_VOLUME",
  "RYO_EMBED_MUSIC_VOLUME",
  "RYO_EMBED_SFX_VOLUME",
  "RYO_EMBED_UI_VOLUME",
  "RYO_EMBED_VOICE_VOLUME",
  "RYO_EMBED_AMBIENCE_VOLUME",
  "RYO_EMBED_AUDIO_BUFFER_FRAMES",
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
  "RYO_EMBED_TILED_PROJECT",
  "RYO_EMBED_PORTABLE",
  "RYO_EMBED_SAVE_FOLDER",
  "RYO_EMBED_ENV",
];

fn main() {
  for key in EMBEDDED_KEYS {
    println!("cargo:rerun-if-env-changed={key}");
    if let Ok(value) = std::env::var(key) {
      println!("cargo:rustc-env={key}={value}");
    }
  }
  list_skills();
}

/// Write `embedded_skills.rs` to `OUT_DIR`: every `.claude/skills/<name>/SKILL.md`
/// as `(name, include_str!(..))`, by name, for `ryo2d init` and
/// `ryo2d install-skills` (`cli::commands::install_skills`). Editing a skill
/// rebuilds through `include_str!`; adding or removing one reruns this.
fn list_skills() {
  use std::path::Path;

  let root = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
  let skills = Path::new(&root).join(".claude/skills");
  println!("cargo:rerun-if-changed={}", skills.display());
  let mut names: Vec<String> = std::fs::read_dir(&skills)
    .expect(".claude/skills is readable")
    .filter_map(Result::ok)
    .filter(|entry| entry.path().join("SKILL.md").is_file())
    .map(|entry| entry.file_name().to_string_lossy().into_owned())
    .collect();
  names.sort();
  let entries: String = names
    .iter()
    .map(|name| {
      let path = format!("/.claude/skills/{name}/SKILL.md");
      format!("  ({name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {path:?}))),\n")
    })
    .collect();
  let out = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
  std::fs::write(
    Path::new(&out).join("embedded_skills.rs"),
    format!("&[\n{entries}]\n"),
  )
  .expect("OUT_DIR is writable");
}
