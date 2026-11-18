//! Dev command - development mode with automatic rebuilding

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

use super::{CliError, CliResult, Output, discover_workspace};
use crate::args::DevArgs;

/// Execute the dev command
pub fn execute(args: &DevArgs) -> CliResult<()> {
  // --skip-opt-check is used by the engine's run scripts for examples;
  // --profiler / -p enables puffin profiling with auto-open
  let skip_opt_check = args.skip_opt_check;
  let profiler_flag = args.profiler;

  let workspace = discover_workspace()?;

  // Check if Ryo.toml has enable_profiler = true (profiler available but not auto-open)
  let config_wants_profiler = workspace.config().debug.enable_profiler;

  // Enable profiler feature if either -p flag or Ryo.toml config
  let enable_profiler_feature = profiler_flag || config_wants_profiler;
  let project_name = workspace.name();

  // Check for dev profile optimization (unless skipped)
  if !skip_opt_check {
    check_dev_profile_optimization(&workspace)?;
  }

  if profiler_flag {
    Output::status(
      "Starting",
      &format!("{} in dev mode with profiler...", project_name),
    );
    println!("           A separate profiler window will open with real-time flamegraphs.");
    println!();
  } else if config_wants_profiler {
    Output::status(
      "Starting",
      &format!(
        "{} in dev mode (profiler available via F10)...",
        project_name
      ),
    );
  } else {
    Output::status("Starting", &format!("{} in dev mode...", project_name));
  }

  // Find workspace root's target directory for shared dependency caching
  let target_dir = find_workspace_target_dir(workspace.root());

  // Build cargo command
  let mut cmd = Command::new("cargo");
  cmd.arg("run");

  // The engine's own profiler feature, so the game needs none of its own
  if enable_profiler_feature {
    cmd.arg("--features").arg("ryo2d/profiler");
  }

  // Set auto-open env var only when -p flag is explicitly used
  if profiler_flag {
    cmd.env("RYO_PROFILER_AUTO_OPEN", "1");
  }

  // Use shared target directory for dependency caching
  cmd.env("CARGO_TARGET_DIR", &target_dir);
  cmd.current_dir(workspace.root());

  // For now, just run cargo run (debug build)
  // Future: Add file watching with notify crate
  let status = cmd.status().map_err(|e| CliError::spawn("cargo run", e))?;

  if !status.success() {
    return Err(CliError::Build { status });
  }

  Ok(())
}

/// Check if the project has dev profile optimization configured
fn check_dev_profile_optimization(workspace: &ryo2d::core::config::Workspace) -> CliResult<()> {
  let cargo_toml_path = workspace.root().join("Cargo.toml");

  if !cargo_toml_path.exists() {
    return Ok(()); // Let cargo handle missing Cargo.toml
  }

  let contents = fs::read_to_string(&cargo_toml_path)
    .map_err(|e| CliError::Other(format!("Failed to read Cargo.toml: {}", e)))?;

  // The "*" override covers every crate outside the game's workspace, the engine included
  let has_wildcard_opt = contents.contains("[profile.dev.package.\"*\"]")
    || contents.contains("[profile.dev.package.'*']");

  if !has_wildcard_opt {
    println!();
    Output::warning("Dev build may be slow - dependencies are not optimized!");
    println!();
    println!("  Without this setting, the engine and dependencies run unoptimized,");
    println!("  causing poor performance in dev builds.");
    println!();
    println!("  Add this to your Cargo.toml for playable dev builds:");
    println!();
    println!("    [profile.dev.package.\"*\"]");
    println!("    opt-level = 2");
    println!();

    // Offer to add it automatically
    print!("  Add this setting now? [Y/n] ");
    io::stdout().flush().ok();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
      let response = input.trim().to_lowercase();
      if response.is_empty() || response == "y" || response == "yes" {
        let addition = r#"
# Optimize dependencies, the engine included, in dev builds for playable performance
# Your game code stays unoptimized for fast compile times
[profile.dev.package."*"]
opt-level = 2
"#;

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
