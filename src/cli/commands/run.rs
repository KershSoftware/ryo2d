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

  // Determine the binary path using Cargo package name
  let binary_name = if cfg!(windows) {
    format!("{}.exe", package_name)
  } else {
    package_name.to_string()
  };

  let binary_path = workspace
    .root()
    .join("target")
    .join("release")
    .join(&binary_name);

  // Check if build exists
  if !binary_path.exists() {
    Output::warning(&format!("No production build found for '{}'", display_name));
    Output::status("Hint", "Run 'ryo2d build' first to create a release build");
    return Err(CliError::NoBuildExists(binary_path));
  }

  Output::status("Running", &format!("{} (release)...", display_name));

  // Execute the binary
  let status = Command::new(&binary_path)
    .current_dir(workspace.root())
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
