//! Build script that tracks the Ryo.toml values `ryo2d build` embeds, and
//! lists the agent skills the CLI ships.
//!
//! `ryo2d build` passes RYO_EMBED_* environment variables to cargo; the engine
//! reads them with `option_env!`. Build scripts can't import crate code, so
//! this script includes the project table (`core/config/project_table.rs`),
//! which the crate reads too, with a `project_settings!` of its own that keeps
//! only the embed keys.

macro_rules! project_settings {
  (
    settings {
      $(
        $field:ident : $ty:ty = $section:ident . $key:ident
        $(, embed $embed:literal)? , sample $sample:expr ;
      )*
    }
    workspace_keys {
      $( $ws_section:ident . $ws_key:ident $(embed $ws_embed:literal)? ),* $(,)?
    }
    env embed $env_embed:literal ;
  ) => {
    const EMBEDDED_KEYS: &[&str] = &[
      $($($embed,)?)*
      $($($ws_embed,)?)*
      $env_embed,
    ];
  };
}

include!("core/config/project_table.rs");

fn main() {
  for key in EMBEDDED_KEYS {
    println!("cargo:rerun-if-env-changed={key}");
    if let Ok(value) = std::env::var(key) {
      println!("cargo:rustc-env={key}={value}");
    }
  }
  list_skills();
  dev_tools_cfg();
}

/// The `dev_tools` cfg: on when debug assertions are on or the `dev-tools`
/// feature is set, so a release build without the feature carries no dev tools.
fn dev_tools_cfg() {
  println!("cargo::rustc-check-cfg=cfg(dev_tools)");
  if std::env::var_os("CARGO_CFG_DEBUG_ASSERTIONS").is_some()
    || std::env::var_os("CARGO_FEATURE_DEV_TOOLS").is_some()
  {
    println!("cargo::rustc-cfg=dev_tools");
  }
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
