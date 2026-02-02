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
  let system = &config.system;

  // Run cargo build --release with embedded config values from [system] and [info]
  let status = Command::new("cargo")
    .arg("build")
    .arg("--release")
    // [system] section values
    .env("RYO_EMBED_RESOLUTION_WIDTH", system.resolution_width.to_string())
    .env("RYO_EMBED_RESOLUTION_HEIGHT", system.resolution_height.to_string())
    .env("RYO_EMBED_VSYNC", system.vsync.to_string())
    .env("RYO_EMBED_FULLSCREEN", system.fullscreen.to_string())
    .env("RYO_EMBED_MASTER_VOLUME", system.master_volume.to_string())
    .env("RYO_EMBED_MUSIC_VOLUME", system.music_volume.to_string())
    .env("RYO_EMBED_SFX_VOLUME", system.sfx_volume.to_string())
    // [info] section values (stored in system for embedding)
    .env("RYO_EMBED_SHOW_FPS", system.show_fps.to_string())
    .env("RYO_EMBED_TITLE", &system.title)
    // [debug] section values
    .env("RYO_EMBED_SHOW_COLLIDERS", config.debug.show_colliders.to_string())
    .env("RYO_EMBED_SHOW_VELOCITIES", config.debug.show_velocities.to_string())
    .env("RYO_EMBED_SHOW_ORIGINS", config.debug.show_origins.to_string())
    .env("RYO_EMBED_SHOW_FRAME_TIMING", config.debug.show_frame_timing.to_string())
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
  create_release_bundle(&workspace, &package_name, project_name)?;

  Output::success(&format!("release build of {}", project_name));

  Ok(())
}

/// Create a distributable release bundle
fn create_release_bundle(
  workspace: &crate::core::config::Workspace,
  package_name: &str,
  display_name: &str,
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

  // Create empty data directory (for save files)
  let data_dir = release_dir.join("data");
  fs::create_dir_all(&data_dir)
    .map_err(|e| CliError::Other(format!("Failed to create data directory: {}", e)))?;

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
