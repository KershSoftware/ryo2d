//! Build command - creates a production release

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use ryo2d::tooling::embedded_config_env;

use super::licenses::write_font_licenses;
use super::{CliError, CliResult, Output, discover_workspace, get_cargo_package_name};

/// Execute the build command
pub fn execute(args: &[String]) -> CliResult<()> {
  // Handle --help
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }

  let options = BuildOptions::from_args(args)?;
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

  for warning in build_warnings(options) {
    Output::warning(warning);
  }
  let embedded = embedded_config_env(&workspace);

  let status = release_build_command(&workspace, &target_dir, &embedded, &icon_path, options)
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

/// The `cargo build --release` that compiles the game with the `embedded`
/// Ryo.toml values baked in, without the developer tools unless asked for
/// (`--features ryo2d/dev-tools`). Every embed key it doesn't set is
/// removed, so a `RYO_EMBED_*` variable left in the shell never reaches the
/// shipped game.
fn release_build_command(
  workspace: &ryo2d::core::config::Workspace,
  target_dir: &Path,
  embedded: &[(&'static str, String)],
  icon_path: &Path,
  options: BuildOptions,
) -> Command {
  let mut command = Command::new("cargo");
  command.arg("build").arg("--release");
  if options.dev_tools {
    command.arg("--features").arg("ryo2d/dev-tools");
  }
  // Use shared target directory for dependency caching
  command.env("CARGO_TARGET_DIR", target_dir);
  for key in ryo2d::tooling::EMBED_KEYS {
    command.env_remove(key);
  }
  command
    .envs(embedded.iter().map(|(key, value)| (key, value)))
    // [release] section values - icon path for Windows .exe embedding
    .env("RYO_EMBED_ICON", icon_path)
    .current_dir(workspace.root());
  command
}

/// What `ryo2d build` was asked for on its command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BuildOptions {
  /// `--dev-tools`: keep the developer tools (the console, the dev windows,
  /// the debug keys) in the release build
  dev_tools: bool,
}

impl BuildOptions {
  /// The options in `args` (after `build`). An argument it does not know is
  /// an error naming it, so a misspelt `--dev-tool` cannot quietly make a
  /// plain build.
  fn from_args(args: &[String]) -> CliResult<Self> {
    let mut options = Self { dev_tools: false };
    for arg in args {
      match arg.as_str() {
        "--dev-tools" => options.dev_tools = true,
        "--help" | "-h" => {}
        other => {
          return Err(CliError::Other(format!(
            "unknown argument `{other}` for `ryo2d build`; valid flags: --dev-tools, --help (-h)"
          )));
        }
      }
    }
    Ok(options)
  }
}

/// The warnings printed before the build: one for a build with the tools,
/// which is not for players.
fn build_warnings(options: BuildOptions) -> Vec<&'static str> {
  if options.dev_tools {
    vec!["this build includes developer tools: do not ship it"]
  } else {
    Vec::new()
  }
}

/// [`release_save_folder`](ryo2d::tooling::release_save_folder), rejected before building when it can't name a folder.
fn checked_save_folder(config: &ryo2d::core::config::WorkspaceConfig) -> CliResult<String> {
  let folder = ryo2d::tooling::release_save_folder(config);
  if ryo2d::tooling::valid_save_folder(&folder) {
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

/// Create a distributable release bundle
fn create_release_bundle(
  workspace: &ryo2d::core::config::Workspace,
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

  // The built-in fonts' licences travel with every copy of the game
  write_font_licenses(&release_dir)?;

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
  workspace: &ryo2d::core::config::Workspace,
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
  let logo_bytes = include_bytes!("../../../../src/assets/logo.png");
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
    --dev-tools   Keep the developer tools (the console, the dev windows,
                  the debug keys and overlays) in the build, for testing a
                  release build. Not for players
    -h, --help    Show this help message

    Any other argument is an error.

DESCRIPTION:
    Builds a production-optimized release of your game and creates
    a distributable bundle in the 'release/' directory. The build has
    no developer tools: no console, dev windows or debug keys (F5 and
    the FPS counter setting stay). --dev-tools keeps them.

    The bundle includes:
    - Executable binary (with config baked in at compile time)
    - Assets directory
    - Levels directory (if present)
    - Data directory (empty, for save files)
    - README.md (if present)
    - licenses/ (the licences of the fonts the engine builds in)

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
  use ryo2d::core::config::{Workspace, WorkspaceConfig};
  use ryo2d::tooling::EMBED_KEYS;

  /// A workspace from Ryo.toml text, with no folder on disk
  fn workspace(toml: &str) -> Workspace {
    Workspace::from_toml_str(toml, PathBuf::from("game")).unwrap()
  }

  fn embedded_value(env: &[(&'static str, String)], key: &str) -> Option<String> {
    env
      .iter()
      .find(|(name, _)| *name == key)
      .map(|(_, value)| value.clone())
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
    assert_eq!(
      checked_save_folder(&config).ok().as_deref(),
      Some("Studio/Game")
    );
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
  fn an_unknown_flag_is_an_error_that_names_it_and_the_valid_flags() {
    let Err(CliError::Other(message)) = BuildOptions::from_args(&["--dev-tool".to_string()]) else {
      panic!("a misspelt flag must not build");
    };
    assert!(message.contains("`--dev-tool`"), "{message}");
    assert!(message.contains("--dev-tools"), "{message}");
    assert!(message.contains("--help"), "{message}");
    assert!(BuildOptions::from_args(&["release".to_string()]).is_err());
  }

  #[test]
  fn dev_tools_adds_the_feature_and_one_warning() {
    let ws = workspace("[project]\nname = \"g\"\n");
    let args = |options: BuildOptions| -> Vec<String> {
      release_build_command(
        &ws,
        Path::new("target"),
        &[],
        Path::new("icon.png"),
        options,
      )
      .get_args()
      .map(|arg| arg.to_string_lossy().into_owned())
      .collect()
    };
    let plain = BuildOptions::from_args(&[]).unwrap();
    let tools = BuildOptions::from_args(&["--dev-tools".to_string()]).unwrap();
    assert_eq!(args(plain), ["build", "--release"]);
    assert_eq!(
      args(tools),
      ["build", "--release", "--features", "ryo2d/dev-tools"]
    );
    assert!(build_warnings(plain).is_empty());
    assert_eq!(
      build_warnings(tools),
      ["this build includes developer tools: do not ship it"]
    );
  }

  #[test]
  fn the_release_bundle_carries_the_font_licences() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ryo.toml"), "[project]\nname = \"g\"\n").unwrap();
    let binary = if cfg!(windows) { "g.exe" } else { "g" };
    let target = dir.path().join("target").join("release");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join(binary), b"a game").unwrap();
    let workspace = Workspace::from_path(dir.path()).unwrap();
    create_release_bundle(&workspace, "g", "g", &dir.path().join("no-icon.png")).unwrap();
    let licenses = dir
      .path()
      .join("release")
      .join(super::super::licenses::LICENSES_DIR);
    for (name, text) in super::super::licenses::FONT_LICENSES {
      let written = std::fs::read_to_string(licenses.join(name));
      assert_eq!(
        written.ok().as_deref(),
        Some(text),
        "release/licenses/{name}"
      );
    }
  }

  #[test]
  fn the_release_build_sets_or_clears_every_embed_key() {
    use std::ffi::{OsStr, OsString};
    let ws = workspace("[project]\nname = \"g\"\n\n[system]\nvsync = false\n");
    let embedded = embedded_config_env(&ws);

    let command = release_build_command(
      &ws,
      std::path::Path::new("target"),
      &embedded,
      std::path::Path::new("icon.png"),
      BuildOptions::from_args(&[]).unwrap(),
    );

    let envs: std::collections::HashMap<OsString, Option<OsString>> = command
      .get_envs()
      .map(|(key, value)| (key.to_owned(), value.map(OsStr::to_owned)))
      .collect();
    for key in EMBED_KEYS {
      // Set to the project's value, or removed: never left to the shell
      let expected = embedded_value(&embedded, key).map(OsString::from);
      assert_eq!(envs.get(OsStr::new(key)), Some(&expected), "{key}");
    }
  }
}
