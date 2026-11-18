//! Dev command - development mode with automatic rebuilding

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use super::{CliError, CliResult, Output, discover_workspace};
use crate::args::DevArgs;
use crate::cargo;
use crate::prompt::Prompt;

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

  // Check for dev profile optimisation (unless skipped)
  if !skip_opt_check {
    check_dev_profile_optimisation(&workspace)?;
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

  // The engine's own profiler feature, so the game needs none of its own
  let mut build_args = Vec::new();
  if enable_profiler_feature {
    build_args.extend(["--features", "ryo2d/profiler"]);
  }
  let exe = cargo::build(workspace.root(), &build_args)?;

  // The auto-open variable only when -p is explicitly used
  let mut env = Vec::new();
  if profiler_flag {
    env.push(("RYO_PROFILER_AUTO_OPEN", Some(OsStr::new("1"))));
  }

  // From the project folder, as `cargo run` ran it: assets and Ryo.toml are found there
  cargo::run_game_with(&exe, workspace.root(), &env)
}

/// Check if the project has dev profile optimisation configured
fn check_dev_profile_optimisation(workspace: &ryo2d::core::config::Workspace) -> CliResult<()> {
  let cargo_toml_path = workspace.root().join("Cargo.toml");
  offer_dev_profile(
    &cargo_toml_path,
    &mut Prompt::from_stdin(),
    &mut io::stdout(),
  )
}

/// Tells the person when `cargo_toml` has no dev profile optimising the
/// dependencies, and adds it only on a yes: with no terminal it only says so
fn offer_dev_profile(
  cargo_toml_path: &Path,
  prompt: &mut Prompt,
  out: &mut dyn Write,
) -> CliResult<()> {
  if !cargo_toml_path.exists() {
    return Ok(()); // Let cargo handle missing Cargo.toml
  }

  let contents = fs::read_to_string(cargo_toml_path)
    .map_err(|e| CliError::Other(format!("Failed to read Cargo.toml: {}", e)))?;

  // The "*" override covers every crate outside the game's workspace, the engine included
  let has_wildcard_opt = contents.contains("[profile.dev.package.\"*\"]")
    || contents.contains("[profile.dev.package.'*']");
  if has_wildcard_opt {
    return Ok(());
  }

  if !prompt.interactive {
    let _ = writeln!(
      out,
      "Hint: dev builds are slow without [profile.dev.package.\"*\"] opt-level = 2 in Cargo.toml: add it yourself, or pass --skip-opt-check to silence this."
    );
    return Ok(());
  }

  Output::warning("Dev build may be slow - dependencies are not optimised!");
  let _ = write!(
    out,
    "
  Without this setting, the engine and dependencies run unoptimised,
  causing poor performance in dev builds.

  Add this to your Cargo.toml for playable dev builds:

    [profile.dev.package.\"*\"]
    opt-level = 2

"
  );

  if prompt.yes_no("Add this setting to Cargo.toml now?", true) {
    let addition = r#"
# Optimise dependencies, the engine included, in dev builds for playable performance
# Your game code stays unoptimised for fast compile times
[profile.dev.package."*"]
opt-level = 2
"#;

    fs::write(cargo_toml_path, format!("{}{}", contents, addition))
      .map_err(|e| CliError::Other(format!("Failed to update Cargo.toml: {}", e)))?;

    println!();
    Output::status("Updated", "Cargo.toml with dev profile optimisation");
    println!("  First build will be slower (compiling optimised dependencies).");
    println!("  Subsequent builds will be fast.");
    println!();
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  const WITHOUT_PROFILE: &str = "[package]\nname = \"g\"\nversion = \"0.1.0\"\n";

  #[test]
  fn with_no_terminal_dev_leaves_cargo_toml_alone() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Cargo.toml");
    fs::write(&path, WITHOUT_PROFILE).unwrap();
    let mut said = Vec::new();
    let mut prompt = Prompt::scripted(false, "y\n");
    offer_dev_profile(&path, &mut prompt, &mut said).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), WITHOUT_PROFILE);
    let said = String::from_utf8(said).unwrap();
    assert!(said.contains("[profile.dev.package.\"*\"]"), "{said}");
    assert!(said.contains("Cargo.toml"), "{said}");
    assert_eq!(
      said.lines().count(),
      1,
      "one line, not the warning block: {said}"
    );
  }

  #[test]
  fn a_yes_adds_the_profile_and_a_no_does_not() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Cargo.toml");
    fs::write(&path, WITHOUT_PROFILE).unwrap();
    offer_dev_profile(&path, &mut Prompt::scripted(true, "n\n"), &mut Vec::new()).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), WITHOUT_PROFILE);
    offer_dev_profile(&path, &mut Prompt::scripted(true, "\n"), &mut Vec::new()).unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains("opt-level = 2"));
  }
}
