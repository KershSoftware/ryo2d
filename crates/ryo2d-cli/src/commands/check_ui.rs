//! Check-ui command - checks UI files without running the game

use std::path::{Path, PathBuf};

use super::{CliError, CliResult, Output, discover_workspace};
use crate::args::CheckUiArgs;
use ryo2d::tooling::{check_files, ui_files_under};

/// Execute the check-ui command
pub fn execute(args: &CheckUiArgs) -> CliResult<()> {
  let workspace = discover_workspace()?;
  let assets = workspace.assets_dir();
  let files: Vec<PathBuf> = if args.files.is_empty() {
    ui_files_under(&assets)
  } else {
    let cwd =
      std::env::current_dir().map_err(|e| CliError::io("could not find the current folder", e))?;
    args.files.iter().map(|file| cwd.join(file)).collect()
  };

  if files.is_empty() {
    Output::warning(&format!("no *.ui.ron files under {}", assets.display()));
    return Ok(());
  }
  check(&assets, files)
}

/// Checks `files` against the assets directory, printing each problem.
pub(super) fn check(assets: &Path, files: Vec<PathBuf>) -> CliResult<()> {
  // The registry warns about each file that does not load; the command
  // prints those mistakes itself, once each
  ryo2d::tooling::set_stderr(false);
  let issues = check_files(assets, &files);
  for issue in &issues {
    eprintln!("{issue}");
  }
  if issues.is_empty() {
    Output::success(&format!("checked {} UI file(s)", files.len()));
    Ok(())
  } else {
    Err(CliError::Other(summary(issues.len(), files.len())))
  }
}

fn summary(problems: usize, files: usize) -> String {
  format!("{problems} problem(s) in {files} UI file(s)")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_summary_counts_problems_and_files() {
    assert_eq!(summary(3, 2), "3 problem(s) in 2 UI file(s)");
  }
}
