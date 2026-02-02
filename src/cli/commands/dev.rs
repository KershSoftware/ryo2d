//! Dev command - development mode with automatic rebuilding

use std::fs;
use std::io::{self, Write};
use std::process::Command;

use super::{CliError, CliResult, Output, discover_workspace};

/// Execute the dev command
pub fn execute(args: &[String]) -> CliResult<()> {
  // Handle --help
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }

  let workspace = discover_workspace()?;
  let project_name = workspace.name();

  // Check for dev profile optimization
  check_dev_profile_optimization(&workspace)?;

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

/// Check if the project has dev profile optimization configured
fn check_dev_profile_optimization(
  workspace: &crate::core::config::Workspace,
) -> CliResult<()> {
  let cargo_toml_path = workspace.root().join("Cargo.toml");

  if !cargo_toml_path.exists() {
    return Ok(()); // Let cargo handle missing Cargo.toml
  }

  let contents = fs::read_to_string(&cargo_toml_path)
    .map_err(|e| CliError::Other(format!("Failed to read Cargo.toml: {}", e)))?;

  // Check if the dev profile optimization is present (need both wildcard and ryo2d-specific)
  let has_wildcard_opt = contents.contains("[profile.dev.package.\"*\"]")
    || contents.contains("[profile.dev.package.'*']");
  let has_ryo2d_opt = contents.contains("[profile.dev.package.ryo2d]");

  if !has_wildcard_opt || !has_ryo2d_opt {
    println!();
    Output::warning("Dev build may be slow - optimization not fully configured!");
    println!();
    println!("  Without these settings, the engine and dependencies run unoptimized,");
    println!("  causing poor performance in dev builds.");
    println!();
    println!("  Add this to your Cargo.toml for playable dev builds:");
    println!();
    println!("    [profile.dev.package.\"*\"]");
    println!("    opt-level = 2");
    println!();
    println!("    [profile.dev.package.ryo2d]");
    println!("    opt-level = 2");
    println!();

    // Offer to add it automatically
    print!("  Add these settings now? [Y/n] ");
    io::stdout().flush().ok();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
      let response = input.trim().to_lowercase();
      if response.is_empty() || response == "y" || response == "yes" {
        // Add the missing settings to Cargo.toml
        let mut addition = String::new();

        if !has_wildcard_opt {
          addition.push_str(
            r#"
# Optimize dependencies in dev builds for playable performance
# Your game code stays unoptimized for fast compile times
[profile.dev.package."*"]
opt-level = 2
"#,
          );
        }

        if !has_ryo2d_opt {
          addition.push_str(
            r#"
[profile.dev.package.ryo2d]
opt-level = 2
"#,
          );
        }

        let new_contents = format!("{}{}", contents, addition);
        fs::write(&cargo_toml_path, new_contents)
          .map_err(|e| CliError::Other(format!("Failed to update Cargo.toml: {}", e)))?;

        println!();
        Output::status("Updated", "Cargo.toml with dev profile optimization");
        println!("  First build will be slower (compiling optimized dependencies).");
        println!("  Subsequent builds will be fast.");
        println!();
      }
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
