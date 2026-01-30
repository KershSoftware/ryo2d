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

  // Run cargo build --release
  let status = Command::new("cargo")
    .arg("build")
    .arg("--release")
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
  workspace: &super::super::super::core::workspace::Workspace,
  package_name: &str,
  display_name: &str,
) -> CliResult<()> {
  let workspace_root = workspace.root();
  let release_dir = workspace_root.join("release");

  Output::status("Bundling", &format!("{} to release/...", display_name));

  // Create release directory
  if release_dir.exists() {
    fs::remove_dir_all(&release_dir).map_err(|e| {
      CliError::Other(format!("Failed to clean release directory: {}", e))
    })?;
  }
  fs::create_dir_all(&release_dir)
    .map_err(|e| CliError::Other(format!("Failed to create release directory: {}", e)))?;

  // Copy executable
  let binary_name = if cfg!(windows) {
    format!("{}.exe", package_name)
  } else {
    package_name.to_string()
  };

  let source_binary = workspace_root
    .join("target")
    .join("release")
    .join(&binary_name);
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

  // Copy Ryo.toml if it exists
  if workspace.has_config_file() {
    let source_config = workspace_root.join("Ryo.toml");
    let dest_config = release_dir.join("Ryo.toml");
    if source_config.exists() {
      fs::copy(&source_config, &dest_config).map_err(|e| {
        CliError::Other(format!("Failed to copy Ryo.toml: {}", e))
      })?;
    }
  }

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
  fs::create_dir_all(dest)
    .map_err(|e| CliError::Other(format!("Failed to create directory {}: {}", dest.display(), e)))?;

  for entry in fs::read_dir(src)
    .map_err(|e| CliError::Other(format!("Failed to read directory {}: {}", src.display(), e)))?
  {
    let entry = entry.map_err(|e| CliError::Other(format!("Failed to read directory entry: {}", e)))?;
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
    - Executable binary
    - Assets directory
    - Levels directory (if present)
    - Data directory (empty, for save files)
    - Ryo.toml configuration
    - README.md (if present)

EXAMPLE:
    ryo2d build

    # The release bundle will be in ./release/
    # You can distribute this entire folder
"#
  );
}
