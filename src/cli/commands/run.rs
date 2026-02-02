//! Run command - executes the production build

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use super::{CliError, CliResult, Output, discover_workspace, get_cargo_package_name};

/// Execute the run command
pub fn execute(args: &[String]) -> CliResult<()> {
  // Handle --help
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }

  let workspace = discover_workspace()?;
  let package_name = get_cargo_package_name(&workspace)?;
  let display_name = workspace.name();

  // Determine the binary path - check release bundle first
  let binary_name = if cfg!(windows) {
    format!("{}.exe", package_name)
  } else {
    package_name.to_string()
  };

  let release_bundle_binary = workspace.root().join("release").join(&binary_name);

  // Check local target directory
  let local_target_binary = workspace
    .root()
    .join("target")
    .join("release")
    .join(&binary_name);

  // Check workspace root's shared target directory
  let workspace_target_dir = find_workspace_target_dir(workspace.root());
  let workspace_target_binary = workspace_target_dir.join("release").join(&binary_name);

  let (binary_path, from_bundle) = if release_bundle_binary.exists() {
    (release_bundle_binary, true)
  } else if workspace_target_binary.exists() {
    (workspace_target_binary, false)
  } else if local_target_binary.exists() {
    (local_target_binary, false)
  } else {
    Output::warning(&format!("No production build found for '{}'", display_name));
    Output::status("Hint", "Run 'ryo2d build' first to create a release build");
    return Err(CliError::NoBuildExists(release_bundle_binary));
  };

  if !from_bundle {
    Output::warning("Running from cargo target/, not release bundle");
    Output::status("Tip", "Run 'ryo2d build' to create a proper release bundle");
  }

  Output::status("Running", &format!("{} (release)...", display_name));

  // Set working directory to release bundle if running from there
  let working_dir = if from_bundle {
    workspace.root().join("release")
  } else {
    workspace.root().to_path_buf()
  };

  // Execute the binary
  let status = Command::new(&binary_path)
    .current_dir(&working_dir)
    .status()
    .map_err(|e| CliError::CommandFailed {
      command: binary_path.display().to_string(),
      message: e.to_string(),
    })?;

  if !status.success() {
    let code = status.code().unwrap_or(-1);
    Output::warning(&format!("Game exited with code {}", code));
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

fn print_help() {
  println!(
    r#"
ryo2d-run - Run the production build

USAGE:
    ryo2d run [OPTIONS]

OPTIONS:
    -h, --help    Show this help message

DESCRIPTION:
    Runs the production build of your game. If no build exists,
    you'll be prompted to run 'ryo2d build' first.

EXAMPLE:
    ryo2d build
    ryo2d run
"#
  );
}
