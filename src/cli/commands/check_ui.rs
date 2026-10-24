//! Check-ui command - checks UI files without running the game

use std::path::PathBuf;

use super::{CliError, CliResult, Output, discover_workspace};
use crate::core::ui::file::check::{check_files, ui_files_under};

/// Execute the check-ui command
pub fn execute(args: &[String]) -> CliResult<()> {
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }

  let workspace = discover_workspace()?;
  let assets = workspace.assets_dir();
  let files: Vec<PathBuf> = if args.is_empty() {
    ui_files_under(&assets)
  } else {
    let cwd = std::env::current_dir().map_err(CliError::Io)?;
    args.iter().map(|a| cwd.join(a)).collect()
  };

  if files.is_empty() {
    Output::warning(&format!("no *.ui.ron files under {}", assets.display()));
    return Ok(());
  }

  // The registry warns about each file that does not load; the command
  // prints those mistakes itself, once each
  crate::core::dev_log::set_stdout_enabled(false);
  let issues = check_files(&assets, &files);
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

fn print_help() {
  println!(
    r#"
ryo2d-check-ui - Check UI files without running the game

USAGE:
    ryo2d check-ui [FILES...]

OPTIONS:
    -h, --help    Show this help message

DESCRIPTION:
    Checks every *.ui.ron file under the project's assets directory, or the
    files given, loading each as the game does. Imports and images are read
    from the assets directory, and an image that does not load is reported
    at its value. Each problem is printed as path:line:column: message, and
    the command exits with an error when any file has a problem. Paths in UI
    files, and the paths printed, are relative to the assets directory
    (ui/menu.ui.ron for assets/ui/menu.ui.ron).

EXAMPLE:
    ryo2d check-ui
    ryo2d check-ui assets/ui/menu.ui.ron
"#
  );
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn help_prints_and_succeeds() {
    assert!(execute(&["--help".to_string()]).is_ok());
  }

  #[test]
  fn the_summary_counts_problems_and_files() {
    assert_eq!(summary(3, 2), "3 problem(s) in 2 UI file(s)");
  }
}
