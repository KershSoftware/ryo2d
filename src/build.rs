//! Build script that tracks the Ryo.toml values `ryo2d build` embeds.
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
