//! The command's one error type, and what each kind exits with.

use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use ryo2d::prelude::EngineError;
use thiserror::Error;

/// Why a command failed. The message says what happened and the source (when
/// there is one) says why; a command prints it once.
#[derive(Debug, Error)]
pub enum CliError {
  /// No project (`Ryo.toml`) was found, or it could not be read
  #[error("could not find a Ryo2D project: {0}")]
  Workspace(#[source] EngineError),
  /// A file, folder, terminal or program the command needed would not work
  #[error("{context}: {source}")]
  Io {
    /// What the command was doing
    context: String,
    /// The operating system's answer
    #[source]
    source: io::Error,
  },
  /// `cargo` built nothing
  #[error("cargo failed ({status}); see its output above")]
  Build {
    /// How cargo ended
    status: ExitStatus,
  },
  /// The game ran and ended with a failure code of its own
  #[error("the game exited with code {code}")]
  GameExited {
    /// The game's exit code
    code: i32,
  },
  /// There is no production build to run or package
  #[error("no production build found at {0:?}. Run 'ryo2d build' first.")]
  NoBuild(PathBuf),
  /// Anything else, as a message
  #[error("{0}")]
  Other(String),
}

/// Result type for CLI operations
pub type CliResult<T> = Result<T, CliError>;

impl CliError {
  /// An [`Io`](CliError::Io) error doing `context`
  pub fn io(context: impl Into<String>, source: io::Error) -> Self {
    CliError::Io {
      context: context.into(),
      source,
    }
  }

  /// A program that would not start: `command` is what was run
  pub fn spawn(command: &str, source: io::Error) -> Self {
    CliError::io(format!("could not run {command}"), source)
  }

  /// The process's exit code: the game's own for [`GameExited`](CliError::GameExited)
  /// (kept in 1 to 255, so a failure never reads as success), 1 for the rest
  pub fn exit_code(&self) -> u8 {
    match self {
      CliError::GameExited { code } => (*code).clamp(1, 255) as u8,
      _ => 1,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::error::Error;

  #[test]
  fn cli_errors_keep_their_cause() {
    let io = CliError::io("could not read Ryo.toml", io::Error::other("denied"));
    assert!(io.source().is_some());
    assert_eq!(io.to_string(), "could not read Ryo.toml: denied");
    let workspace = CliError::Workspace(EngineError::WorkspaceError("no Ryo.toml".to_owned()));
    assert!(workspace.source().is_some());
    assert!(CliError::Other("plain".to_owned()).source().is_none());
  }

  #[test]
  fn exit_codes_pass_the_game_s_through_and_never_read_as_success() {
    assert_eq!(CliError::GameExited { code: 3 }.exit_code(), 3);
    assert_eq!(CliError::GameExited { code: 0 }.exit_code(), 1);
    assert_eq!(CliError::GameExited { code: -1 }.exit_code(), 1);
    assert_eq!(CliError::GameExited { code: 1000 }.exit_code(), 255);
    assert_eq!(CliError::Other("x".to_owned()).exit_code(), 1);
    assert_eq!(CliError::NoBuild(PathBuf::from("release")).exit_code(), 1);
  }
}
