//! Build command - creates a production release

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use super::{CliError, CliResult, Output, discover_workspace, get_cargo_package_name};

/// Execute the build command
pub fn execute(args: &[String]) -> CliResult<()> {
  // Handle --help
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }

  let workspace = discover_workspace()?;
  let project_name = workspace.name();
  let package_name = get_cargo_package_name(&workspace)?;

  Output::status("Building", &format!("{} (release)...", project_name));

  // Read Ryo.toml config to embed at compile time
  let config = workspace.config();
  let release = &config.release;

  // Fail before compiling if the save folder name can't be used on every OS
  checked_save_folder(config)?;

  // Resolve icon path: custom from [release].logo, or default Ryo2D logo
  let icon_path = resolve_icon_path(&workspace, release.logo.as_ref())?;

  // Find workspace root's target directory for shared dependency caching
  // This allows all examples to share compiled dependencies
  let target_dir = find_workspace_target_dir(workspace.root());

  let embedded = embedded_config_env(config, workspace.dev_window_keys_setting());
  for warning in release_tool_warnings(&embedded) {
    Output::warning(&warning);
  }

  // Run cargo build --release with embedded config values from [system] and [info]
  let status = Command::new("cargo")
    .arg("build")
    .arg("--release")
    // Use shared target directory for dependency caching
    .env("CARGO_TARGET_DIR", &target_dir)
    .envs(embedded)
    // [release] section values - icon path for Windows .exe embedding
    .env("RYO_EMBED_ICON", &icon_path)
    .current_dir(workspace.root())
    .status()
    .map_err(|e| CliError::CommandFailed {
      command: "cargo build".to_string(),
      message: e.to_string(),
    })?;

  if !status.success() {
    return Err(CliError::BuildFailed(
      "Cargo build failed. See output above for details.".to_string(),
    ));
  }

  // Create release bundle
  create_release_bundle(&workspace, &package_name, project_name, &icon_path)?;

  Output::success(&format!("release build of {}", project_name));

  Ok(())
}

/// Warnings for developer tools the embedded config would ship to players.
fn release_tool_warnings(env: &[(&'static str, String)]) -> Vec<String> {
  const TOOLS: [(&str, &str, &str); 3] = [
    (
      "RYO_EMBED_DEV_WINDOW_KEYS",
      "[debug] dev_window_keys = true",
      "F9/F10/F12 developer windows will work in this release build",
    ),
    (
      "RYO_EMBED_LOGGER_WINDOW",
      "[debug] logger_window = true",
      "the logger window will open in this release build",
    ),
    (
      "RYO_EMBED_ENABLE_PROFILER",
      "[debug] enable_profiler = true",
      "the profiler window will open in this release build",
    ),
  ];
  TOOLS
    .iter()
    .filter(|(name, _, _)| {
      env
        .iter()
        .any(|(key, value)| key == name && value == "true")
    })
    .map(|(_, setting, effect)| format!("{setting}: {effect}"))
    .collect()
}

/// The per-game save folder name: `[release] save_folder` as written, else
/// `[project] name` turned into a single folder name.
fn release_save_folder(config: &crate::core::config::WorkspaceConfig) -> String {
  config
    .release
    .save_folder
    .clone()
    .unwrap_or_else(|| folder_from_project_name(&config.name))
}

/// Make a project name one folder: path separators, control characters and
/// `<>:"|?*` become `-`, then surrounding whitespace and trailing dots go.
fn folder_from_project_name(name: &str) -> String {
  let replaced: String = name
    .chars()
    .map(|c| {
      if c.is_control() || "/\\<>:\"|?*".contains(c) {
        '-'
      } else {
        c
      }
    })
    .collect();
  replaced.trim().trim_end_matches(['.', ' ']).to_string()
}

/// [`release_save_folder`], rejected before building when it can't name a folder.
fn checked_save_folder(config: &crate::core::config::WorkspaceConfig) -> CliResult<String> {
  let folder = release_save_folder(config);
  if crate::core::config::save_location::valid_save_folder(&folder) {
    return Ok(folder);
  }
  let source = if config.release.save_folder.is_some() {
    "[release] save_folder".to_string()
  } else {
    "[project] name; set [release] save_folder to choose another".to_string()
  };
  Err(CliError::BuildFailed(format!(
    "Invalid save folder \"{folder}\" (from {source}). \
     Use a relative name like \"MyStudio/MyGame\" without \"..\", a drive, any of <>:\"|?*\\ \
     or a reserved Windows name (CON, PRN, AUX, NUL, COM1-9, LPT1-9)"
  )))
}

/// Ryo.toml values passed to the release build; the engine reads them with
/// `option_env!` (see `core::config::resolve::COMPILED_EMBEDDED`).
fn embedded_config_env(
  config: &crate::core::config::WorkspaceConfig,
  dev_window_keys: Option<bool>,
) -> Vec<(&'static str, String)> {
  let system = &config.system;
  let debug = &config.debug;
  let display_mode = match system.display_mode {
    crate::core::config::DisplayMode::Windowed => "windowed",
    crate::core::config::DisplayMode::ExclusiveFullscreen => "exclusive_fullscreen",
    crate::core::config::DisplayMode::BorderlessFullscreen => "borderless_fullscreen",
  };
  vec![
    (
      "RYO_EMBED_RESOLUTION",
      system
        .resolution
        .map(|(w, h)| format!("{}x{}", w, h))
        .unwrap_or_else(|| "1280x720".to_string()),
    ),
    ("RYO_EMBED_PIXEL_ART", system.pixel_art.to_string()),
    ("RYO_EMBED_VSYNC", system.vsync.to_string()),
    ("RYO_EMBED_DISPLAY_MODE", display_mode.to_string()),
    ("RYO_EMBED_MASTER_VOLUME", system.master_volume.to_string()),
    ("RYO_EMBED_MUSIC_VOLUME", system.music_volume.to_string()),
    ("RYO_EMBED_SFX_VOLUME", system.sfx_volume.to_string()),
    ("RYO_EMBED_CONSOLE", system.dev_console.to_string()),
    ("RYO_EMBED_TITLE", config.title.clone()),
    ("RYO_EMBED_SHOW_FPS", system.show_fps.to_string()),
    ("RYO_EMBED_SHOW_COLLIDERS", debug.show_colliders.to_string()),
    (
      "RYO_EMBED_SHOW_VELOCITIES",
      debug.show_velocities.to_string(),
    ),
    ("RYO_EMBED_SHOW_ORIGINS", debug.show_origins.to_string()),
    (
      "RYO_EMBED_SHOW_FRAME_TIMING",
      debug.show_frame_timing.to_string(),
    ),
    (
      "RYO_EMBED_DISABLE_DEBUG_KEYS",
      debug.disable_debug_keys.to_string(),
    ),
    (
      "RYO_EMBED_ENABLE_PROFILER",
      debug.enable_profiler.to_string(),
    ),
    (
      "RYO_EMBED_DEV_WINDOW_KEYS",
      dev_window_keys.unwrap_or(false).to_string(),
    ),
    ("RYO_EMBED_LOGGER_WINDOW", debug.logger_window.to_string()),
    (
      "RYO_EMBED_BACKGROUND_FPS",
      system.target_background_fps.to_string(),
    ),
    (
      "RYO_EMBED_BACKGROUND_RENDERING",
      system.background_rendering.to_string(),
    ),
    ("RYO_EMBED_PORTABLE", config.release.portable.to_string()),
    ("RYO_EMBED_SAVE_FOLDER", release_save_folder(config)),
    (
      "RYO_EMBED_ENV",
      serde_json::to_string(&system.env).unwrap_or_else(|_| "{}".to_string()),
    ),
  ]
}

/// Create a distributable release bundle
fn create_release_bundle(
  workspace: &crate::core::config::Workspace,
  package_name: &str,
  display_name: &str,
  icon_path: &PathBuf,
) -> CliResult<()> {
  let workspace_root = workspace.root();
  let release_dir = workspace_root.join("release");

  Output::status("Bundling", &format!("{} to release/...", display_name));

  // Create release directory
  if release_dir.exists() {
    fs::remove_dir_all(&release_dir)
      .map_err(|e| CliError::Other(format!("Failed to clean release directory: {}", e)))?;
  }
  fs::create_dir_all(&release_dir)
    .map_err(|e| CliError::Other(format!("Failed to create release directory: {}", e)))?;

  // Copy executable
  let binary_name = if cfg!(windows) {
    format!("{}.exe", package_name)
  } else {
    package_name.to_string()
  };

  // Find all candidate binaries and use the newest one
  // (handles workspace members where binary is in parent workspace's target)
  let local_binary = workspace_root
    .join("target")
    .join("release")
    .join(&binary_name);

  let mut candidates: Vec<PathBuf> = vec![];

  // Check local target directory
  if local_binary.exists() {
    candidates.push(local_binary.clone());
  }

  // Check parent directories for workspace root's target
  let mut search_dir = workspace_root.to_path_buf();
  for _ in 0..10 {
    if let Some(parent) = search_dir.parent() {
      search_dir = parent.to_path_buf();
      let candidate = search_dir.join("target").join("release").join(&binary_name);
      if candidate.exists() {
        candidates.push(candidate);
      }
    } else {
      break;
    }
  }

  // Use the most recently modified binary
  let source_binary = candidates
    .into_iter()
    .filter_map(|path| {
      fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .map(|mtime| (path, mtime))
    })
    .max_by_key(|(_, mtime)| *mtime)
    .map(|(path, _)| path)
    .unwrap_or(local_binary); // Fall back to local path for error message

  let dest_binary = release_dir.join(&binary_name);

  fs::copy(&source_binary, &dest_binary).map_err(|e| {
    CliError::Other(format!(
      "Failed to copy executable from {} to {}: {}",
      source_binary.display(),
      dest_binary.display(),
      e
    ))
  })?;

  // Make executable on Unix
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(&dest_binary)
      .map_err(|e| CliError::Other(format!("Failed to get file permissions: {}", e)))?
      .permissions();
    perms.set_mode(0o755); // rwxr-xr-x
    fs::set_permissions(&dest_binary, perms)
      .map_err(|e| CliError::Other(format!("Failed to set executable permissions: {}", e)))?;
  }

  // Copy assets directory if it exists
  let assets_dir = workspace.assets_dir();
  if assets_dir.exists() {
    let dest_assets = release_dir.join("assets");
    copy_dir_recursive(&assets_dir, &dest_assets)?;
  }

  // Copy levels directory if it exists
  let levels_dir = workspace_root.join("levels");
  if levels_dir.exists() {
    let dest_levels = release_dir.join("levels");
    copy_dir_recursive(&levels_dir, &dest_levels)?;
  }

  // Portable builds keep save data beside the game
  if workspace.config().release.portable {
    let data_dir = release_dir.join("data");
    fs::create_dir_all(&data_dir)
      .map_err(|e| CliError::Other(format!("Failed to create data directory: {}", e)))?;
  }

  // Note: Ryo.toml is NOT included in release bundles
  // Configuration values are compiled into the binary at build time
  // If runtime configuration is needed, games should provide their own config system

  // Copy README if it exists
  let readme_path = workspace_root.join("README.md");
  if readme_path.exists() {
    let dest_readme = release_dir.join("README.md");
    fs::copy(&readme_path, &dest_readme)
      .map_err(|e| CliError::Other(format!("Failed to copy README.md: {}", e)))?;
  }

  // Copy icon to release directory with consistent name for runtime discovery
  if icon_path.exists() {
    let dest_icon = release_dir.join("icon.png");
    fs::copy(icon_path, &dest_icon)
      .map_err(|e| CliError::Other(format!("Failed to copy icon: {}", e)))?;
  }

  Output::status(
    "Created",
    &format!("release bundle at {}", release_dir.display()),
  );

  Ok(())
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &PathBuf, dest: &PathBuf) -> CliResult<()> {
  fs::create_dir_all(dest).map_err(|e| {
    CliError::Other(format!(
      "Failed to create directory {}: {}",
      dest.display(),
      e
    ))
  })?;

  for entry in fs::read_dir(src)
    .map_err(|e| CliError::Other(format!("Failed to read directory {}: {}", src.display(), e)))?
  {
    let entry =
      entry.map_err(|e| CliError::Other(format!("Failed to read directory entry: {}", e)))?;
    let path = entry.path();
    let file_name = entry.file_name();
    let dest_path = dest.join(&file_name);

    if path.is_dir() {
      copy_dir_recursive(&path, &dest_path)?;
    } else {
      fs::copy(&path, &dest_path).map_err(|e| {
        CliError::Other(format!(
          "Failed to copy {} to {}: {}",
          path.display(),
          dest_path.display(),
          e
        ))
      })?;
    }
  }

  Ok(())
}

/// Find the workspace root's target directory for shared dependency caching.
/// Searches parent directories for a Cargo.toml with a [workspace] section.
fn find_workspace_target_dir(start_dir: &std::path::Path) -> PathBuf {
  let mut search_dir = start_dir.to_path_buf();

  // Search up to 10 parent directories for workspace root
  for _ in 0..10 {
    let cargo_toml = search_dir.join("Cargo.toml");
    if cargo_toml.exists() {
      // Check if this Cargo.toml has a [workspace] section
      if let Ok(content) = fs::read_to_string(&cargo_toml)
        && content.contains("[workspace]")
      {
        return search_dir.join("target");
      }
    }

    if let Some(parent) = search_dir.parent() {
      search_dir = parent.to_path_buf();
    } else {
      break;
    }
  }

  // Fallback to local target directory
  start_dir.join("target")
}

/// Resolve the icon path for embedding.
///
/// Returns the absolute path to the PNG icon file. If a custom logo is specified
/// in [release].logo, validates it exists. Otherwise, extracts the default
/// Ryo2D logo to a temporary location.
///
/// The icon is used for:
/// - Runtime window icon (title bar, taskbar) via the engine's icon loading
/// - Windows .exe embedding (requires game project's build.rs to use winres)
fn resolve_icon_path(
  workspace: &crate::core::config::Workspace,
  custom_logo: Option<&PathBuf>,
) -> CliResult<PathBuf> {
  if let Some(logo) = custom_logo {
    // Custom icon specified - make path absolute and validate
    let absolute_path = if logo.is_absolute() {
      logo.clone()
    } else {
      workspace.root().join(logo)
    };

    if !absolute_path.exists() {
      return Err(CliError::Other(format!(
        "Custom logo not found: {} (specified in [release].logo)",
        absolute_path.display()
      )));
    }

    Output::status("Using", &format!("custom icon: {}", logo.display()));
    return Ok(absolute_path);
  }

  // No custom icon - use default Ryo2D logo
  // Extract embedded logo to a temp file that the game's build.rs can use
  let temp_dir = std::env::temp_dir().join("ryo2d-build");
  fs::create_dir_all(&temp_dir)
    .map_err(|e| CliError::Other(format!("Failed to create temp directory: {}", e)))?;

  let logo_path = temp_dir.join("ryo2d-logo.png");

  // Write embedded logo bytes
  let logo_bytes = include_bytes!("../../assets/logo.png");
  fs::write(&logo_path, logo_bytes)
    .map_err(|e| CliError::Other(format!("Failed to write default logo: {}", e)))?;

  Output::status("Using", "default Ryo2D icon");
  Ok(logo_path)
}

fn print_help() {
  println!(
    r#"
ryo2d-build - Build a production release

USAGE:
    ryo2d build [OPTIONS]

OPTIONS:
    -h, --help    Show this help message

DESCRIPTION:
    Builds a production-optimized release of your game and creates
    a distributable bundle in the 'release/' directory.

    The bundle includes:
    - Executable binary (with config baked in at compile time)
    - Assets directory
    - Levels directory (if present)
    - Data directory (empty, for save files)
    - README.md (if present)

    Note: Ryo.toml is NOT included in the release bundle. All
    configuration values (paths, env variables) are compiled into
    the binary at build time.

EXAMPLE:
    ryo2d build

    # The release bundle will be in ./release/
    # You can distribute this entire folder
"#
  );
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::config::WorkspaceConfig;
  use crate::core::config::resolve::{COMPILED_EMBEDDED, ProjectLayer};

  fn embedded_value(env: &[(&'static str, String)], key: &str) -> String {
    env
      .iter()
      .find(|(name, _)| *name == key)
      .map(|(_, value)| value.clone())
      .unwrap()
  }

  #[test]
  fn save_folder_defaults_to_the_project_name() {
    let config = WorkspaceConfig {
      name: "Star Hopper".to_string(),
      ..Default::default()
    };
    let env = embedded_config_env(&config, None);
    assert_eq!(embedded_value(&env, "RYO_EMBED_SAVE_FOLDER"), "Star Hopper");
    assert_eq!(embedded_value(&env, "RYO_EMBED_PORTABLE"), "false");
  }

  #[test]
  fn save_folder_override_and_portable_are_embedded() {
    let mut config = WorkspaceConfig::default();
    config.release.save_folder = Some("Studio/Hopper".to_string());
    config.release.portable = true;
    let env = embedded_config_env(&config, None);
    assert_eq!(
      embedded_value(&env, "RYO_EMBED_SAVE_FOLDER"),
      "Studio/Hopper"
    );
    assert_eq!(embedded_value(&env, "RYO_EMBED_PORTABLE"), "true");
  }

  #[test]
  fn project_names_become_a_single_folder() {
    assert_eq!(
      folder_from_project_name("Squash/Stretch Demo"),
      "Squash-Stretch Demo"
    );
    assert_eq!(folder_from_project_name("Game: Remix"), "Game- Remix");
    assert_eq!(folder_from_project_name("Breakout"), "Breakout");
    assert_eq!(folder_from_project_name("  Hopper. . "), "Hopper");
  }

  #[test]
  fn default_save_folder_is_sanitized_but_explicit_is_not() {
    let mut config = WorkspaceConfig {
      name: "Squash/Stretch Demo".to_string(),
      ..Default::default()
    };
    assert_eq!(
      checked_save_folder(&config).ok().as_deref(),
      Some("Squash-Stretch Demo")
    );
    config.release.save_folder = Some("Studio/Game".to_string());
    let env = embedded_config_env(&config, None);
    assert_eq!(embedded_value(&env, "RYO_EMBED_SAVE_FOLDER"), "Studio/Game");
    config.release.save_folder = None;
    config.name = "..".to_string();
    assert!(matches!(
      checked_save_folder(&config),
      Err(CliError::BuildFailed(_))
    ));
  }

  #[test]
  fn invalid_save_folder_stops_the_build() {
    let mut config = WorkspaceConfig::default();
    config.release.save_folder = Some("../escape".to_string());
    assert!(matches!(
      checked_save_folder(&config),
      Err(CliError::BuildFailed(_))
    ));

    config.release.save_folder = None;
    config.name = "Hopper".to_string();
    assert!(matches!(checked_save_folder(&config), Ok(ref folder) if folder == "Hopper"));
  }

  #[test]
  fn embedded_env_covers_every_key_the_engine_reads() {
    let mut names: Vec<&str> = embedded_config_env(&WorkspaceConfig::default(), None)
      .into_iter()
      .map(|(name, _)| name)
      .collect();
    let mut expected: Vec<&str> = COMPILED_EMBEDDED.iter().map(|(key, _)| *key).collect();
    names.sort_unstable();
    expected.sort_unstable();
    assert_eq!(names, expected);
  }

  #[test]
  fn embedded_display_title_and_volumes_round_trip() {
    use crate::core::config::DisplayMode;
    let mut config = WorkspaceConfig {
      title: "Hopper".to_string(),
      ..Default::default()
    };
    config.system.display_mode = DisplayMode::BorderlessFullscreen;
    config.system.resolution = Some((800, 600));
    config.system.master_volume = 0.3;
    config.system.music_volume = 0.4;
    config.system.sfx_volume = 0.5;

    let env = embedded_config_env(&config, None);
    let layer = ProjectLayer::from_embedded(|name| {
      env
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.as_str())
    })
    .unwrap();

    assert_eq!(layer.display_mode, Some(DisplayMode::BorderlessFullscreen));
    assert_eq!(layer.resolution, Some((800, 600)));
    assert_eq!(layer.title.as_deref(), Some("Hopper"));
    assert_eq!(layer.master_volume, Some(0.3));
    assert_eq!(layer.music_volume, Some(0.4));
    assert_eq!(layer.sfx_volume, Some(0.5));
  }

  #[test]
  fn invalid_save_folder_error_names_where_it_came_from() {
    let mut config = WorkspaceConfig::default();
    config.release.save_folder = Some("../escape".to_string());
    let Err(CliError::BuildFailed(message)) = checked_save_folder(&config) else {
      panic!("expected an invalid save folder error");
    };
    assert!(message.contains("[release] save_folder"), "{message}");
    assert!(message.contains("reserved"), "{message}");

    config.release.save_folder = None;
    config.name = "CON".to_string();
    let Err(CliError::BuildFailed(message)) = checked_save_folder(&config) else {
      panic!("expected an invalid save folder error");
    };
    assert!(message.contains("[project] name"), "{message}");
    assert!(message.contains("set [release] save_folder"), "{message}");
  }

  #[test]
  fn embedded_debug_and_background_settings_reach_the_engine() {
    let mut config = WorkspaceConfig::default();
    config.debug.disable_debug_keys = true;
    config.debug.enable_profiler = true;
    config.debug.dev_window_keys = true;
    config.debug.logger_window = true;
    config.system.target_background_fps = 15;
    config.system.background_rendering = false;

    let env = embedded_config_env(&config, Some(true));
    let layer = ProjectLayer::from_embedded(|name| {
      env
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.as_str())
    })
    .unwrap();

    assert_eq!(layer.disable_debug_keys, Some(true));
    assert_eq!(layer.enable_profiler, Some(true));
    assert_eq!(layer.dev_window_keys, Some(true));
    assert_eq!(layer.logger_window, Some(true));
    assert_eq!(layer.background_fps, Some(15));
    assert_eq!(layer.background_rendering, Some(false));
  }

  fn embedded_dev_window_keys(toml: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ryo.toml"), toml).unwrap();
    let workspace = crate::core::config::Workspace::from_path(dir.path()).unwrap();
    embedded_config_env(workspace.config(), workspace.dev_window_keys_setting())
      .into_iter()
      .find(|(key, _)| *key == "RYO_EMBED_DEV_WINDOW_KEYS")
      .map(|(_, value)| value)
      .unwrap()
  }

  #[test]
  fn absent_dev_window_keys_embeds_false() {
    assert_eq!(
      embedded_dev_window_keys("[project]\nname = \"g\"\n\n[debug]\nshow_fps = true\n"),
      "false"
    );
  }

  #[test]
  fn explicit_dev_window_keys_is_embedded() {
    assert_eq!(
      embedded_dev_window_keys("[project]\nname = \"g\"\n\n[debug]\ndev_window_keys = true\n"),
      "true"
    );
  }

  #[test]
  fn release_tool_warnings_name_each_enabled_key() {
    let env = |v: &str| {
      vec![
        ("RYO_EMBED_DEV_WINDOW_KEYS", v.to_string()),
        ("RYO_EMBED_LOGGER_WINDOW", v.to_string()),
        ("RYO_EMBED_ENABLE_PROFILER", v.to_string()),
        ("RYO_EMBED_VSYNC", "true".to_string()),
      ]
    };
    let warnings = release_tool_warnings(&env("true"));
    assert_eq!(warnings.len(), 3);
    assert!(warnings[0].contains("dev_window_keys"));
    assert!(release_tool_warnings(&env("false")).is_empty());
  }

  #[test]
  fn env_section_is_embedded_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
      dir.path().join("Ryo.toml"),
      "[env]\ndifficulty = \"hard\"\nlives = 3\nhard_mode = true\nspeed = 1.5\n",
    )
    .unwrap();
    let workspace = crate::core::config::Workspace::from_path(dir.path()).unwrap();
    let env = embedded_config_env(workspace.config(), None);

    let shipped = crate::core::config::Workspace::from_embedded(dir.path(), |name| {
      env
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.as_str())
    })
    .unwrap();

    let config = shipped.config();
    assert_eq!(config.env_str("difficulty"), Some("hard"));
    assert_eq!(config.env_int("lives"), Some(3));
    assert_eq!(config.env_bool("hard_mode"), Some(true));
    assert_eq!(config.env_float("speed"), Some(1.5));
  }

  #[test]
  fn build_script_tracks_exactly_the_embedded_keys() {
    // The quoted names inside src/build.rs's EMBEDDED_KEYS array
    let script = include_str!("../../build.rs");
    let list = script
      .split_once("const EMBEDDED_KEYS: &[&str] = &[")
      .and_then(|(_, rest)| rest.split_once("];"))
      .map(|(list, _)| list)
      .expect("src/build.rs declares EMBEDDED_KEYS");
    let mut tracked: Vec<&str> = list
      .lines()
      .map(str::trim)
      .filter(|line| !line.starts_with("//"))
      .filter_map(|line| line.strip_prefix('"')?.split_once('"').map(|(key, _)| key))
      .collect();
    let mut compiled: Vec<&str> = COMPILED_EMBEDDED.iter().map(|(key, _)| *key).collect();
    tracked.sort_unstable();
    compiled.sort_unstable();
    assert_eq!(tracked, compiled);
  }
}
