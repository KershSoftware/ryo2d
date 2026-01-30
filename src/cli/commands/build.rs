//! Build command - creates a production release

use std::process::Command;

use super::{CliError, CliResult, Output, discover_workspace};

/// Execute the build command
pub fn execute(args: &[String]) -> CliResult<()> {
  // Handle --help
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }

  let workspace = discover_workspace()?;
  let project_name = workspace.name();

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

  Output::success(&format!("release build of {}", project_name));

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
    Builds a production-optimized release of your game.
    The output is placed in target/release/

EXAMPLE:
    ryo2d build
"#
  );
}
