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

  // Check for --profiler / -p flag (enables puffin profiling with auto-open)
  let profiler_flag = args.iter().any(|a| a == "--profiler" || a == "-p");

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
      &format!("{} in dev mode (profiler available via F10)...", project_name),
    );
  } else {
    Output::status("Starting", &format!("{} in dev mode...", project_name));
  }

  // Find workspace root's target directory for shared dependency caching
  let target_dir = find_workspace_target_dir(workspace.root());

  // Build cargo command
  let mut cmd = Command::new("cargo");
  cmd.arg("run");

  // Add profiler feature flag if requested (via -p flag or Ryo.toml config)
  if enable_profiler_feature {
    cmd.arg("--features").arg("profiler");
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
  let status = cmd.status().map_err(|e| CliError::CommandFailed {
    command: "cargo run".to_string(),
    message: e.to_string(),
  })?;

  if !status.success() {
    let code = status.code().unwrap_or(-1);
    if code != 0 {
      // Check if the failure was due to missing profiler feature
      if enable_profiler_feature {
        print_profiler_feature_help(&workspace);
      }
      return Err(CliError::BuildFailed(format!(
        "Development build failed with exit code {}",
        code
      )));
    }
  }

  Ok(())
}

/// Print help for adding the profiler feature if it's missing
fn print_profiler_feature_help(workspace: &crate::core::config::Workspace) {
  let cargo_toml_path = workspace.root().join("Cargo.toml");

  // Check if Cargo.toml has the profiler feature defined
  if let Ok(contents) = fs::read_to_string(&cargo_toml_path) {
    let has_profiler_feature = contents.contains("profiler")
      && (contents.contains("[features]") || contents.contains("features ="));

    if !has_profiler_feature {
      println!();
      Output::warning("The 'profiler' feature may not be configured in your project.");
      println!();
      println!("  Add this to your Cargo.toml to enable profiling:");
      println!();
      println!("      [features]");
      println!("      profiler = [\"ryo2d/profiler\"]");
      println!();
      println!("  This forwards the profiler feature to the ryo2d dependency.");
      println!();
    }
  }
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
    Use --profiler to enable and auto-open the profiler window:

        ryo2d dev --profiler

    Alternatively, set `enable_profiler = true` in your Ryo.toml [debug] section.
    This enables the profiler feature but doesn't auto-open - press F10 to open.

EXAMPLE:
    ryo2d dev              # Normal development mode
    ryo2d dev --profiler   # With profiler window auto-opened
"#
  );
}
