//! The `ryo2d` binary's command line: what it lists, rejects and exits with.

use std::process::{Command, Output};

const COMMANDS: [&str; 9] = [
  "init",
  "dev",
  "build",
  "run",
  "export",
  "install-skills",
  "check-ui",
  "sheet",
  "netplay",
];

fn ryo2d(args: &[&str]) -> Output {
  // From an empty folder: no command here may find a project
  let folder = tempfile::tempdir().expect("a temporary folder");
  Command::new(env!("CARGO_BIN_EXE_ryo2d"))
    .args(args)
    .current_dir(folder.path())
    .output()
    .expect("the ryo2d binary runs")
}

#[test]
fn ryo2d_help_lists_every_command() {
  let output = ryo2d(&["--help"]);
  assert_eq!(output.status.code(), Some(0));
  let help = String::from_utf8_lossy(&output.stdout);
  for command in COMMANDS {
    assert!(help.contains(command), "{command} is not in\n{help}");
  }
}

#[test]
fn every_command_rejects_a_flag_it_does_not_know() {
  for command in COMMANDS {
    let output = ryo2d(&[command, "--no-such-flag"]);
    assert_eq!(output.status.code(), Some(2), "{command}");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("--no-such-flag"), "{command}: {said}");
  }
}

#[test]
fn an_unknown_command_is_one_error_line() {
  let output = ryo2d(&["bogus-command"]);
  assert_eq!(output.status.code(), Some(2));
  let said = String::from_utf8_lossy(&output.stderr);
  assert_eq!(said.matches("bogus-command").count(), 1, "{said}");
  assert!(output.stdout.is_empty(), "nothing on stdout");
}
