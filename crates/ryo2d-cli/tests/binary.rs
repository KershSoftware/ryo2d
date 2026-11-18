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

#[test]
fn a_bare_ryo2d_prints_the_help_and_exits_2() {
  let output = ryo2d(&[]);
  assert_eq!(output.status.code(), Some(2));
  let said = String::from_utf8_lossy(&output.stderr);
  assert!(said.contains("Usage"), "{said}");
  assert!(said.contains("netplay"), "{said}");
}

#[test]
fn version_is_a_flag_and_help_has_no_colour() {
  let output = ryo2d(&["--version"]);
  assert_eq!(output.status.code(), Some(0));
  assert!(String::from_utf8_lossy(&output.stdout).starts_with("ryo2d "));
  assert!(!ryo2d(&["bogus"]).stderr.contains(&0x1b));
}

/// `ryo2d init` in a fresh folder, with no terminal on stdin
fn init_in_new_folder(args: &[&str]) -> (tempfile::TempDir, Output) {
  let folder = tempfile::tempdir().expect("a temporary folder");
  let output = Command::new(env!("CARGO_BIN_EXE_ryo2d"))
    .arg("init")
    .args(args)
    .current_dir(folder.path())
    .stdin(std::process::Stdio::null())
    .output()
    .expect("the ryo2d binary runs");
  (folder, output)
}

#[test]
fn init_with_no_terminal_and_no_flag_installs_no_skills_and_says_how() {
  let (folder, output) = init_in_new_folder(&["my-game"]);
  assert_eq!(output.status.code(), Some(0));
  let game = folder.path().join("my-game");
  assert!(game.join("Ryo.toml").exists());
  assert!(!game.join(".claude").exists() && !game.join(".github").exists());
  let said = String::from_utf8_lossy(&output.stdout);
  assert!(said.contains("--skills"), "{said}");
}

#[test]
fn init_with_no_terminal_needs_a_name() {
  let (folder, output) = init_in_new_folder(&[]);
  assert_eq!(output.status.code(), Some(1));
  assert!(String::from_utf8_lossy(&output.stderr).contains("ryo2d init NAME"));
  assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
}

#[test]
fn init_skills_flag_installs_what_it_names() {
  for (value, claude, copilot) in [
    ("claude", true, false),
    ("copilot", false, true),
    ("both", true, true),
    ("none", false, false),
  ] {
    let (folder, output) = init_in_new_folder(&["my-game", "--skills", value]);
    assert_eq!(output.status.code(), Some(0), "{value}");
    let game = folder.path().join("my-game");
    let has = |skills: &str| game.join(skills).join("ryo-api/SKILL.md").exists();
    assert_eq!(has(".claude/skills"), claude, "{value}");
    assert_eq!(has(".github/skills"), copilot, "{value}");
  }
}

#[test]
fn install_skills_with_no_terminal_and_no_flag_installs_nothing() {
  let (folder, output) = init_in_new_folder(&["my-game"]);
  assert_eq!(output.status.code(), Some(0));
  let game = folder.path().join("my-game");
  let output = Command::new(env!("CARGO_BIN_EXE_ryo2d"))
    .arg("install-skills")
    .current_dir(&game)
    .stdin(std::process::Stdio::null())
    .output()
    .expect("the ryo2d binary runs");
  assert_eq!(output.status.code(), Some(1));
  assert!(String::from_utf8_lossy(&output.stderr).contains("--skills"));
  assert!(!game.join(".claude").exists());
}
