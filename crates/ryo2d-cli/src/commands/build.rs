//! Build command - creates a production release

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use ryo2d::tooling::embedded_config_env;

use super::licenses::write_font_licenses;
use super::{CliError, CliResult, Output, discover_workspace};
use crate::args::BuildArgs;
use crate::cargo;

/// Execute the build command
pub fn execute(options: BuildArgs) -> CliResult<()> {
  let workspace = discover_workspace()?;
  let project_name = workspace.name();

  Output::status("Building", &format!("{} (release)...", project_name));

  // Read Ryo.toml config to embed at compile time
  let config = workspace.config();
  let release = &config.release;

  // Fail before compiling if the save folder name can't be used on every OS
  checked_save_folder(config)?;

  // Resolve icon path: custom from [release].logo, or default Ryo2D logo
  let icon_path = resolve_icon_path(&workspace, release.logo.as_ref())?;

  for warning in build_warnings(options) {
    Output::warning(warning);
  }
  let embedded = embedded_config_env(&workspace);

  let built = cargo::execute(
    release_build_command(&workspace, &embedded, &icon_path, options),
    workspace.root(),
  )?;

  // Create release bundle
  create_release_bundle(&workspace, &built, project_name, &icon_path)?;

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
  embedded: &[(&'static str, String)],
  icon_path: &Path,
  options: BuildArgs,
) -> Command {
  let mut command = Command::new("cargo");
  command.arg("build").arg("--release");
  if options.dev_tools {
    command.arg("--features").arg("ryo2d/dev-tools");
  }
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

/// The warnings printed before the build: one for a build with the tools,
/// which is not for players.
fn build_warnings(options: BuildArgs) -> Vec<&'static str> {
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
  Err(CliError::Other(format!(
    "Invalid save folder \"{folder}\" (from {source}). \
     Use a relative name like \"MyStudio/MyGame\" without \"..\", a drive, any of <>:\"|?*\\ \
     or a reserved Windows name (CON, PRN, AUX, NUL, COM1-9, LPT1-9)"
  )))
}

/// Create a distributable release bundle
fn create_release_bundle(
  workspace: &ryo2d::core::config::Workspace,
  source_binary: &Path,
  display_name: &str,
  icon_path: &PathBuf,
) -> CliResult<()> {
  let workspace_root = workspace.root();
  let release_dir = workspace_root.join("release");

  Output::status("Bundling", &format!("{} to release/...", display_name));

  // Replace everything in release/, but for data/ when portable: a portable
  // build keeps its saves there
  clean_release_dir(&release_dir, workspace.config().release.portable)?;
  fs::create_dir_all(&release_dir)
    .map_err(|e| CliError::Other(format!("Failed to create release directory: {}", e)))?;

  // Copy the executable under the name cargo gave it (`.exe` or not)
  let binary_name = source_binary.file_name().ok_or_else(|| {
    CliError::Other(format!(
      "cargo's executable has no file name: {}",
      source_binary.display()
    ))
  })?;
  let dest_binary = release_dir.join(binary_name);

  fs::copy(source_binary, &dest_binary).map_err(|e| {
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

/// Remove everything in `release_dir`, except its `data` folder when
/// `keep_data` (a portable game's saves)
fn clean_release_dir(release_dir: &Path, keep_data: bool) -> CliResult<()> {
  let entries = match fs::read_dir(release_dir) {
    Ok(entries) => entries,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
    Err(e) => {
      return Err(CliError::Other(format!(
        "Failed to clean release directory: {}",
        e
      )));
    }
  };
  for entry in entries {
    let entry =
      entry.map_err(|e| CliError::Other(format!("Failed to clean release directory: {}", e)))?;
    if keep_data && entry.file_name() == "data" {
      continue;
    }
    let path = entry.path();
    // A link is removed as a file, never followed
    let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
    let removed = if is_dir {
      fs::remove_dir_all(&path)
    } else {
      fs::remove_file(&path)
    };
    removed.map_err(|e| CliError::Other(format!("Failed to clean release directory: {}", e)))?;
  }
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

#[cfg(test)]
mod tests {
  use super::*;
  use ryo2d::core::config::{Workspace, WorkspaceConfig};
  use ryo2d::tooling::EMBED_KEYS;

  /// A workspace from Ryo.toml text, in a temporary folder kept while it lives
  fn workspace(toml: &str) -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ryo.toml"), toml).unwrap();
    let workspace = Workspace::from_path(dir.path()).unwrap();
    (dir, workspace)
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
      Err(CliError::Other(_))
    ));
  }

  #[test]
  fn invalid_save_folder_stops_the_build() {
    let mut config = WorkspaceConfig::default();
    config.release.save_folder = Some("../escape".to_string());
    assert!(matches!(
      checked_save_folder(&config),
      Err(CliError::Other(_))
    ));

    config.release.save_folder = None;
    config.name = "Hopper".to_string();
    assert!(matches!(checked_save_folder(&config), Ok(ref folder) if folder == "Hopper"));
  }

  #[test]
  fn invalid_save_folder_error_names_where_it_came_from() {
    let mut config = WorkspaceConfig::default();
    config.release.save_folder = Some("../escape".to_string());
    let Err(CliError::Other(message)) = checked_save_folder(&config) else {
      panic!("expected an invalid save folder error");
    };
    assert!(message.contains("[release] save_folder"), "{message}");
    assert!(message.contains("reserved"), "{message}");

    config.release.save_folder = None;
    config.name = "CON".to_string();
    let Err(CliError::Other(message)) = checked_save_folder(&config) else {
      panic!("expected an invalid save folder error");
    };
    assert!(message.contains("[project] name"), "{message}");
    assert!(message.contains("set [release] save_folder"), "{message}");
  }

  #[test]
  fn dev_tools_adds_the_feature_and_one_warning() {
    let (_dir, ws) = workspace("[project]\nname = \"g\"\n");
    let args = |options: BuildArgs| -> Vec<String> {
      release_build_command(&ws, &[], Path::new("icon.png"), options)
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
    };
    let plain = BuildArgs { dev_tools: false };
    let tools = BuildArgs { dev_tools: true };
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
    let built = dir.path().join("built-game");
    std::fs::write(&built, b"a game").unwrap();
    let workspace = Workspace::from_path(dir.path()).unwrap();
    create_release_bundle(&workspace, &built, "g", &dir.path().join("no-icon.png")).unwrap();
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
    let (_dir, ws) = workspace("[project]\nname = \"g\"\n\n[system]\nvsync = false\n");
    let embedded = embedded_config_env(&ws);

    let command = release_build_command(
      &ws,
      &embedded,
      std::path::Path::new("icon.png"),
      BuildArgs { dev_tools: false },
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

  #[test]
  fn rebuilding_keeps_a_portable_build_s_saves() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
      dir.path().join("Ryo.toml"),
      "[project]\nname = \"g\"\n\n[release]\nportable = true\n",
    )
    .unwrap();
    let built = dir.path().join("built-game");
    std::fs::write(&built, b"a game").unwrap();
    let workspace = Workspace::from_path(dir.path()).unwrap();
    let bundle =
      || create_release_bundle(&workspace, &built, "g", &dir.path().join("no-icon.png")).unwrap();
    bundle();
    let release = dir.path().join("release");
    std::fs::write(release.join("data").join("save.json"), b"progress").unwrap();
    std::fs::create_dir_all(release.join("old")).unwrap();
    std::fs::write(release.join("old").join("stale.txt"), b"stale").unwrap();
    std::fs::write(release.join("stale.txt"), b"stale").unwrap();
    bundle();
    assert_eq!(
      std::fs::read(release.join("data").join("save.json"))
        .ok()
        .as_deref(),
      Some(&b"progress"[..])
    );
    assert!(!release.join("stale.txt").exists());
    assert!(!release.join("old").exists());
  }

  #[test]
  fn the_bundle_s_binary_keeps_the_name_cargo_gave_it() {
    // A Windows build cross-compiled on any host: `.exe` stays
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ryo.toml"), "[project]\nname = \"g\"\n").unwrap();
    let built = dir.path().join("the_game.exe");
    std::fs::write(&built, b"a game").unwrap();
    let workspace = Workspace::from_path(dir.path()).unwrap();
    create_release_bundle(&workspace, &built, "g", &dir.path().join("no-icon.png")).unwrap();
    assert!(dir.path().join("release").join("the_game.exe").is_file());
  }

  #[test]
  fn a_build_that_is_not_portable_removes_the_data_folder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ryo.toml"), "[project]\nname = \"g\"\n").unwrap();
    let built = dir.path().join("built-game");
    std::fs::write(&built, b"a game").unwrap();
    let workspace = Workspace::from_path(dir.path()).unwrap();
    let release = dir.path().join("release");
    std::fs::create_dir_all(release.join("data")).unwrap();
    std::fs::write(release.join("data").join("save.json"), b"progress").unwrap();
    create_release_bundle(&workspace, &built, "g", &dir.path().join("no-icon.png")).unwrap();
    assert!(!release.join("data").exists());
  }

  #[cfg(unix)]
  #[test]
  fn cleaning_removes_a_link_to_a_folder_and_not_what_it_points_at() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), b"mine").unwrap();
    let release = dir.path().join("release");
    std::fs::create_dir_all(&release).unwrap();
    std::os::unix::fs::symlink(&outside, release.join("link")).unwrap();
    clean_release_dir(&release, false).unwrap();
    assert!(!release.join("link").exists());
    assert!(outside.join("keep.txt").exists());
  }

  #[test]
  fn a_release_folder_that_cannot_be_read_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("release");
    std::fs::write(&file, b"not a folder").unwrap();
    assert!(clean_release_dir(&file, false).is_err());
    assert!(clean_release_dir(&dir.path().join("missing"), false).is_ok());
  }
}
