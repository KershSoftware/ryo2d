//! `ryo2d init` makes a project that builds, and nothing it prints or
//! documents promises what is not there.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

// Keep in step with `COMMANDS` in binary.rs, whose `ryo2d_help_lists_every_command`
// fails when this list is out of date
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

/// `ryo2d` run in `folder`, with `home` as the person's home folder
fn ryo2d_in(folder: &Path, home: &Path, args: &[&str]) -> Output {
  Command::new(env!("CARGO_BIN_EXE_ryo2d"))
    .args(args)
    .current_dir(folder)
    .env("HOME", home)
    .env("LOCALAPPDATA", home.join("AppData"))
    .output()
    .expect("the ryo2d binary runs")
}

/// Where `init` looks for an installed engine under `home`
/// (`~/.ryo2d/engine`, `%LOCALAPPDATA%/ryo2d/engine` on Windows)
fn engine_folder(home: &Path) -> std::path::PathBuf {
  if cfg!(windows) {
    home.join("AppData").join("ryo2d").join("engine")
  } else {
    home.join(".ryo2d").join("engine")
  }
}

/// A home folder with an engine installed where `init` looks for it
fn home_with_engine() -> tempfile::TempDir {
  let home = tempfile::tempdir().expect("a temporary folder");
  fs::create_dir_all(engine_folder(home.path())).expect("the engine folder");
  home
}

#[test]
fn init_makes_a_project_that_builds() {
  // The repository, which holds the engine this test's `ryo2d` is built from
  // (not canonicalised: on Windows that adds a `\\?\` prefix cargo misreads)
  let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
    .parent()
    .and_then(Path::parent)
    .expect("the repository folder");
  let repo_text = repo.display().to_string().replace('\\', "/");

  // Outside the repository, so its workspace does not claim the project
  let work = tempfile::tempdir().expect("a temporary folder");
  let home = home_with_engine();
  let output = ryo2d_in(
    work.path(),
    home.path(),
    &["init", "game", "--skills", "none"],
  );
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );

  // Point the dependency at this checkout in place of the installed engine
  let project = work.path().join("game");
  let manifest = project.join("Cargo.toml");
  let text = fs::read_to_string(&manifest).expect("Cargo.toml");
  let installed = engine_folder(home.path())
    .display()
    .to_string()
    .replace('\\', "/");
  assert!(text.contains(&installed), "{text}");
  fs::write(&manifest, text.replace(&installed, &repo_text)).expect("Cargo.toml");

  // The workspace's target folder, so the engine is not built from scratch
  let metadata = Command::new("cargo")
    .args(["metadata", "--format-version", "1", "--no-deps"])
    .current_dir(repo)
    .output()
    .expect("cargo metadata runs");
  let metadata: serde_json::Value =
    serde_json::from_slice(&metadata.stdout).expect("metadata is JSON");
  let target = metadata["target_directory"]
    .as_str()
    .expect("a target folder");

  // The workspace's lock, so the engine's dependencies are the versions it
  // builds with; only what it lacks (the project itself and its build
  // dependency `winresource`, with that one's own dependencies) is resolved
  fs::copy(repo.join("Cargo.lock"), project.join("Cargo.lock")).expect("Cargo.lock");

  let check = Command::new("cargo")
    // not `--locked` or `--offline`: the lock lacks the project and winresource
    .arg("check")
    .current_dir(&project)
    .env("CARGO_TARGET_DIR", target)
    .output()
    .expect("cargo check runs");
  assert!(
    check.status.success(),
    "{}",
    String::from_utf8_lossy(&check.stderr)
  );
}

#[test]
fn help_text_promises_nothing_that_is_not_there() {
  let folder = tempfile::tempdir().expect("a temporary folder");
  let home = home_with_engine();
  for command in COMMANDS {
    let output = ryo2d_in(folder.path(), home.path(), &[command, "--help"]);
    let help = String::from_utf8_lossy(&output.stdout).to_lowercase();
    assert!(!help.contains("coming soon"), "{command}");
    if command == "dev" {
      assert!(help.contains("asset hot reload"), "{help}");
    }
  }
  // What init says to do next is as true
  let output = ryo2d_in(
    folder.path(),
    home.path(),
    &["init", "game", "--skills", "none"],
  );
  let said = String::from_utf8_lossy(&output.stdout).to_lowercase();
  assert!(
    said.contains("next steps") && !said.contains("coming soon"),
    "{said}"
  );
}
