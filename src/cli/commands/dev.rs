//! Dev command - development mode with automatic rebuilding

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

use super::{CliError, CliResult, Output, discover_workspace};

/// Execute the dev command
pub fn execute(args: &[String]) -> CliResult<()> {
  // Handle --help
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }

  // Check for --skip-opt-check flag (used by engine run scripts for examples)
  let skip_opt_check = args.iter().any(|a| a == "--skip-opt-check");

  // Check for --profiler / -p flag (enables puffin profiling)
  let enable_profiler = args.iter().any(|a| a == "--profiler" || a == "-p");

  let workspace = discover_workspace()?;
  let project_name = workspace.name();

  // Check for dev profile optimization (unless skipped)
  if !skip_opt_check {
    check_dev_profile_optimization(&workspace)?;
  }

  if enable_profiler {
    Output::status("Starting", &format!("{} in dev mode with puffin profiler...", project_name));

    // Try to launch puffin_viewer automatically
    launch_puffin_viewer();
  } else {
    Output::status("Starting", &format!("{} in dev mode...", project_name));
  }

  // Find workspace root's target directory for shared dependency caching
  let target_dir = find_workspace_target_dir(workspace.root());

  // Build cargo command
  let mut cmd = Command::new("cargo");
  cmd.arg("run");

  // Add profiler feature flag if requested
  if enable_profiler {
    cmd.arg("--features").arg("profiler");
    // Also set env var to enable puffin at runtime (overrides Ryo.toml)
    cmd.env("RYO_PUFFIN_PROFILER", "1");
  }

  // Use shared target directory for dependency caching
  cmd.env("CARGO_TARGET_DIR", &target_dir);
  cmd.current_dir(workspace.root());

  // For now, just run cargo run (debug build)
  // Future: Add file watching with notify crate
  let status = cmd
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
fn check_dev_profile_optimization(workspace: &crate::core::config::Workspace) -> CliResult<()> {
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

/// Launch puffin_viewer in the background to connect to the profiler.
/// If puffin_viewer is not installed, prints installation instructions.
fn launch_puffin_viewer() {
  Output::status("Profiler", "Checking for puffin_viewer...");

  // Try to spawn puffin_viewer in the background
  match Command::new("puffin_viewer")
    .arg("--url")
    .arg("127.0.0.1:8585")
    .spawn()
  {
    Ok(_child) => {
      Output::status("Profiler", "Launching puffin_viewer app...");
      println!("           Connecting to profiler at http://127.0.0.1:8585");
      println!("           View real-time flamegraphs in the puffin_viewer window");
      println!();
    }
    Err(e) => {
      if e.kind() == std::io::ErrorKind::NotFound {
        println!();
        Output::warning("puffin_viewer is not installed!");
        println!();
        println!("  The profiler will still run, but you need puffin_viewer to see the data.");
        println!("  Install it with:");
        println!();
        println!("      cargo install puffin_viewer");
        println!();
        println!("  Then re-run `ryo2d dev --profiler` and it will auto-launch.");
        println!();
      } else {
        Output::warning(&format!("Failed to launch puffin_viewer: {}", e));
      }
    }
  }
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
ryo2d-dev - Development mode

USAGE:
    ryo2d dev [OPTIONS]

OPTIONS:
    -h, --help          Show this help message
    -p, --profiler      Enable puffin hierarchical profiler (HTTP server on port 8585)
    --skip-opt-check    Skip the dev profile optimization check

DESCRIPTION:
    Runs your game in development mode with:
    - Debug build for faster compilation
    - Automatic recompilation on file changes (coming soon)
    - Hot reload support (coming soon)

    This is the recommended command during development.

PROFILING:
    Use --profiler to enable the puffin hierarchical profiler:

        ryo2d dev --profiler

    This automatically launches puffin_viewer if installed. If not installed:

        cargo install puffin_viewer

    Alternatively, set `puffin_profiler = true` in your Ryo.toml [debug] section.

EXAMPLE:
    ryo2d dev              # Normal development mode
    ryo2d dev --profiler   # With puffin profiler enabled
"#
  );
}
