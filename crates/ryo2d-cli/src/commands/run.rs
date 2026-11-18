//! Run command - executes the production build

use super::{CliError, CliResult, Output, discover_workspace, get_cargo_package_name};
use crate::cargo;

/// Execute the run command
pub fn execute() -> CliResult<()> {
  let workspace = discover_workspace()?;
  let package_name = get_cargo_package_name(&workspace)?;
  let display_name = workspace.name();

  // Determine the binary path - the release bundle first
  let binary_name = if cfg!(windows) {
    format!("{}.exe", package_name)
  } else {
    package_name.to_string()
  };

  let release_bundle_binary = workspace.root().join("release").join(&binary_name);

  let (binary_path, from_bundle) = if release_bundle_binary.exists() {
    (release_bundle_binary, true)
  } else {
    // Cargo says where its target folder is (a workspace's, a configured one)
    let built = cargo::target_dir(workspace.root())?
      .join("release")
      .join(&binary_name);
    if !built.exists() {
      return Err(CliError::NoBuild(release_bundle_binary));
    }
    (built, false)
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

  cargo::run_game(&binary_path, &working_dir)
}
