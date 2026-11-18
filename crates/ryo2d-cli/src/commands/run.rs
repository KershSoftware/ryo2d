//! Run command - executes the production build

use super::{CliError, CliResult, Output, discover_workspace};
use crate::cargo;

/// Execute the run command
pub fn execute() -> CliResult<()> {
  let workspace = discover_workspace()?;
  let display_name = workspace.name();
  // The binary `cargo run` would run, and where cargo builds to
  let game = cargo::game(workspace.root())?;

  // The release bundle first, then cargo's own release folder
  let release = workspace.root().join("release");
  let (binary_path, from_bundle) = if let Some(bundled) = cargo::find_binary(&release, &game.bin) {
    (bundled, true)
  } else if let Some(built) = cargo::find_binary(&game.target_dir.join("release"), &game.bin) {
    (built, false)
  } else {
    return Err(CliError::NoBuild(release.join(&game.bin)));
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
