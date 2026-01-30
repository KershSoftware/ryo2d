//! Ryo2D CLI entry point
//!
//! This binary provides the `ryo2d` command-line interface.

use std::process::ExitCode;

fn main() -> ExitCode {
  match ryo2d::cli::run_cli() {
    Ok(()) => ExitCode::SUCCESS,
    Err(e) => {
      eprintln!("{}", e);
      ExitCode::FAILURE
    }
  }
}
