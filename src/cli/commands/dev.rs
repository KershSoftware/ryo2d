//! Dev command - development mode with automatic rebuilding

use std::process::Command;

use super::{discover_workspace, CliError, CliResult, Output};

/// Execute the dev command
pub fn execute(args: &[String]) -> CliResult<()> {
  // Handle --help
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }

  let workspace = discover_workspace()?;
  let project_name = workspace.name();

  Output::status("Starting", &format!("{} in dev mode...", project_name));

  // For now, just run cargo run (debug build)
  // Future: Add file watching with notify crate
  let status = Command::new("cargo")
    .arg("run")
    .current_dir(workspace.root())
    .status()
    .map_err(|e| CliError::CommandFailed {
      command: "cargo run".to_string(),
      message: e.to_string(),
    })?;

  if !status.success() {
    let code = status.code().unwrap_or(-1);
    if code != 0 {
      return Err(CliError::BuildFailed(format!(
        "Development build failed with exit code {}",
        code
      )));
    }
  }

  Ok(())
}

fn print_help() {
  println!(
    r#"
ryo2d-dev - Development mode

USAGE:
    ryo2d dev [OPTIONS]

OPTIONS:
    -h, --help    Show this help message

DESCRIPTION:
    Runs your game in development mode with:
    - Debug build for faster compilation
    - Automatic recompilation on file changes (coming soon)
    - Hot reload support (coming soon)

    This is the recommended command during development.

EXAMPLE:
    ryo2d dev
"#
  );
}
