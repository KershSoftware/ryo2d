//! CLI command implementations
//!
//! Each submodule implements a specific CLI command.

mod build;
mod check_ui;
mod dev;
mod export;
mod init;
pub mod install_skills;
mod licenses;
mod netplay;
mod run;
mod sheet;

use std::process::ExitCode;

use clap::Parser;
use ryo2d::core::config::Workspace;

use crate::args::{Cli, Command, NetplayCommand};
use crate::error::{CliError, CliResult};

/// CLI output helpers
struct Output;

impl Output {
  fn status(action: &str, message: &str) {
    // Format: "   Building my_game..."
    println!("{:>12} {}", action, message);
  }

  fn success(message: &str) {
    println!("{:>12} {}", "Finished", message);
  }

  fn warning(message: &str) {
    eprintln!("{:>12} {}", "Warning", message);
  }

  fn error(message: &str) {
    eprintln!("{:>12} {}", "Error", message);
  }
}

/// Runs the command `args` name (the first is the program's name), printing
/// its error once. The exit code is 0 for success, clap's (2) for arguments it
/// cannot read, and [`CliError::exit_code`] for a failure.
pub fn run_cli(args: Vec<String>) -> ExitCode {
  ExitCode::from(run_with(&args))
}

/// [`run_cli`]'s exit code for these arguments (the first is the program's name).
fn run_with(args: &[String]) -> u8 {
  // The installed CLI is a build without tools, which writes no log file:
  // the engine's warnings (Ryo.toml's) go to the terminal
  ryo2d::tooling::set_stderr(true);
  let cli = match Cli::try_parse_from(args) {
    Ok(cli) => cli,
    Err(error) => {
      let code = error.exit_code();
      // A closed terminal leaves nowhere to say it
      let _ = error.print();
      return u8::try_from(code).unwrap_or(2);
    }
  };
  match dispatch(cli.command) {
    Ok(()) => 0,
    Err(error) => {
      eprintln!("{}", error.report());
      error.exit_code()
    }
  }
}

fn dispatch(command: Command) -> CliResult<()> {
  match command {
    Command::Init(args) => init::execute(&args),
    Command::Build(args) => build::execute(args),
    Command::Run => run::execute(),
    Command::Dev(args) => dev::execute(&args),
    Command::Export => export::execute(),
    Command::InstallSkills(args) => install_skills::execute(&args),
    Command::Netplay { command } => match command {
      NetplayCommand::Start => netplay::execute_start(),
      NetplayCommand::Ip => netplay::execute_ip(),
    },
    Command::CheckUi(args) => check_ui::execute(&args),
    Command::Sheet(args) => sheet::execute(&args),
  }
}

/// Discover the current workspace
fn discover_workspace() -> CliResult<Workspace> {
  Workspace::discover().map_err(CliError::Workspace)
}

/// Get the Cargo package name from Cargo.toml
///
/// The binary name is based on the package name in Cargo.toml, not the
/// display name in Ryo.toml.
fn get_cargo_package_name(workspace: &Workspace) -> CliResult<String> {
  use std::fs;

  let cargo_toml_path = workspace.root().join("Cargo.toml");

  if !cargo_toml_path.exists() {
    return Err(CliError::Other(
      "Cargo.toml not found in workspace".to_string(),
    ));
  }

  let contents = fs::read_to_string(&cargo_toml_path)
    .map_err(|e| CliError::Other(format!("Failed to read Cargo.toml: {}", e)))?;

  let parsed: toml::Value = toml::from_str(&contents)
    .map_err(|e| CliError::Other(format!("Failed to parse Cargo.toml: {}", e)))?;

  let package_name = parsed
    .get("package")
    .and_then(|p| p.get("name"))
    .and_then(|n| n.as_str())
    .ok_or_else(|| CliError::Other("No package.name found in Cargo.toml".to_string()))?;

  Ok(package_name.to_string())
}

#[cfg(test)]
mod tests {
  use super::*;
  use ryo2d::tooling::{set_stderr, stderr_on};
  use std::sync::Mutex;

  /// The log's stderr switch is the process's: tests that read it take turns
  static STDERR_SWITCH: Mutex<()> = Mutex::new(());

  /// Puts the stderr switch back as the test found it
  struct Restore(bool);

  impl Drop for Restore {
    fn drop(&mut self) {
      set_stderr(self.0);
    }
  }

  fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
  }

  #[test]
  fn the_cli_shows_the_engine_s_warnings_in_every_build() {
    let _turn = STDERR_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let _log = ryo2d::logging::capture_log();
    let _restore = Restore(stderr_on());
    // As the installed CLI starts: a build without tools, stderr off
    set_stderr(false);
    assert_eq!(run_with(&args(&["ryo2d", "--version"])), 0);
    assert!(stderr_on(), "Ryo.toml's warnings reach the terminal");

    assert_eq!(run_with(&args(&["ryo2d", "check-ui", "--help"])), 0);
    assert!(stderr_on());
    // check-ui prints the mistakes itself
    let assets = tempfile::tempdir().unwrap();
    let file = assets.path().join("broken.ui.ron");
    std::fs::write(&file, "not a ui file (").unwrap();
    assert!(check_ui::check(assets.path(), vec![file]).is_err());
    assert!(
      !stderr_on(),
      "check-ui turns the log's stderr off for itself"
    );
  }
}
