//! CLI command implementations
//!
//! Each submodule implements a specific CLI command.

mod build;
mod dev;
mod init;
pub mod install_skills;
mod run;

use std::env;
use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::core::workspace::Workspace;

/// CLI-specific error type
#[derive(Debug)]
pub enum CliError {
  /// Workspace discovery failed
  WorkspaceNotFound(String),
  /// Build failed
  BuildFailed(String),
  /// No production build exists
  NoBuildExists(PathBuf),
  /// Command execution failed
  CommandFailed { command: String, message: String },
  /// IO error
  Io(io::Error),
  /// Generic error with message
  Other(String),
}

impl fmt::Display for CliError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      CliError::WorkspaceNotFound(msg) => {
        write!(f, "Could not find Ryo2D workspace: {}", msg)
      }
      CliError::BuildFailed(msg) => {
        write!(f, "Build failed: {}", msg)
      }
      CliError::NoBuildExists(path) => {
        write!(
          f,
          "No production build found at {:?}. Run 'ryo2d build' first.",
          path
        )
      }
      CliError::CommandFailed { command, message } => {
        write!(f, "Command '{}' failed: {}", command, message)
      }
      CliError::Io(e) => write!(f, "IO error: {}", e),
      CliError::Other(msg) => write!(f, "{}", msg),
    }
  }
}

impl std::error::Error for CliError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      CliError::Io(e) => Some(e),
      _ => None,
    }
  }
}

impl From<io::Error> for CliError {
  fn from(e: io::Error) -> Self {
    CliError::Io(e)
  }
}

/// Result type for CLI operations
pub type CliResult<T> = Result<T, CliError>;

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

/// Parse command-line arguments and run the appropriate command
pub fn run_cli() -> CliResult<()> {
  let args: Vec<String> = env::args().collect();

  if args.len() < 2 {
    print_help();
    return Ok(());
  }

  match args[1].as_str() {
    "init" => init::execute(&args[2..]),
    "build" => build::execute(&args[2..]),
    "run" => run::execute(&args[2..]),
    "dev" => dev::execute(&args[2..]),
    "install-skills" => install_skills::execute(&args[2..]),
    "help" | "--help" | "-h" => {
      print_help();
      Ok(())
    }
    "version" | "--version" | "-V" => {
      print_version();
      Ok(())
    }
    unknown => {
      Output::error(&format!("Unknown command: {}", unknown));
      print_help();
      Err(CliError::Other(format!("Unknown command: {}", unknown)))
    }
  }
}

fn print_help() {
  println!(
    r#"
ryo2d - Ryo2D Game Engine CLI

USAGE:
    ryo2d <COMMAND>

COMMANDS
    init            Create a new Ryo2D project
    build           Build a production release
    run             Run the production build
    dev             Development mode (build + run with watching)
    install-skills  Install Agent Skills for AI assistants
    help            Show this help message
    version         Show version information

Run 'ryo2d <COMMAND> --help' for more information about a command.
"#
  );
}

fn print_version() {
  println!("ryo2d {}", env!("CARGO_PKG_VERSION"));
}

/// Discover the current workspace
fn discover_workspace() -> CliResult<Workspace> {
  Workspace::discover().map_err(|e| CliError::WorkspaceNotFound(e.to_string()))
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
