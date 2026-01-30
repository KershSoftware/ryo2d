//! Run command - executes the production build

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
  let cargo_build_binary = workspace
    .root()
    .join("target")
    .join("release")
    .join(&binary_name);

  let (binary_path, from_bundle) = if release_bundle_binary.exists() {
    (release_bundle_binary, true)
  } else if cargo_build_binary.exists() {
    (cargo_build_binary, false)
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
