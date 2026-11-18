//! The `ryo2d` command's entry point.

use std::process::ExitCode;

fn main() -> ExitCode {
  ryo2d_cli::run_cli(std::env::args().collect())
}
