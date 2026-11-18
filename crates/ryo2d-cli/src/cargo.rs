//! Asking cargo where things are, and running what it built.
//!
//! The target folder and the executable come from cargo itself
//! (`cargo metadata`, the JSON build messages), so a `CARGO_TARGET_DIR`, a
//! `.cargo/config.toml` `target-dir` or a workspace is honoured without the
//! command guessing paths. Nothing here sets `CARGO_TARGET_DIR`.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::error::{CliError, CliResult};

/// An environment variable for a child process: set, or removed when `None`
pub type EnvVar<'a> = (&'a str, Option<&'a OsStr>);

/// Cargo's target folder for the project at `project`
pub fn target_dir(project: &Path) -> CliResult<PathBuf> {
  target_dir_with(project, &[])
}

pub(crate) fn target_dir_with(project: &Path, env: &[EnvVar]) -> CliResult<PathBuf> {
  metadata(project, env)?
    .get("target_directory")
    .and_then(serde_json::Value::as_str)
    .map(PathBuf::from)
    .ok_or_else(|| CliError::Other("cargo metadata named no target directory".to_owned()))
}

/// `cargo metadata --format-version 1 --no-deps` for the project at `project`
fn metadata(project: &Path, env: &[EnvVar]) -> CliResult<serde_json::Value> {
  let mut command = Command::new("cargo");
  command
    .args(["metadata", "--format-version", "1", "--no-deps"])
    .current_dir(project);
  apply_env(&mut command, env);
  let output = command
    .output()
    .map_err(|e| CliError::spawn("cargo metadata", e))?;
  if !output.status.success() {
    return Err(CliError::Other(format!(
      "cargo metadata failed ({}): {}",
      output.status,
      String::from_utf8_lossy(&output.stderr).trim()
    )));
  }
  serde_json::from_slice(&output.stdout)
    .map_err(|e| CliError::Other(format!("could not read cargo metadata: {e}")))
}

/// The binary of the package at `manifest` that `cargo run` would run: its
/// `default-run`, else its only binary; several without a `default-run` is an
/// error naming them
fn game_bin(metadata: &serde_json::Value, manifest: &Path) -> CliResult<String> {
  let package = metadata
    .get("packages")
    .and_then(serde_json::Value::as_array)
    .into_iter()
    .flatten()
    .find(|package| {
      package
        .get("manifest_path")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|path| canonical(Path::new(path)) == manifest)
    })
    .ok_or_else(|| {
      CliError::Other(format!(
        "cargo metadata has no package at {}",
        manifest.display()
      ))
    })?;
  if let Some(default) = package
    .get("default_run")
    .and_then(serde_json::Value::as_str)
  {
    return Ok(default.to_owned());
  }
  let bins: Vec<&str> = package
    .get("targets")
    .and_then(serde_json::Value::as_array)
    .into_iter()
    .flatten()
    .filter(|target| {
      target
        .get("kind")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("bin")))
    })
    .filter_map(|target| target.get("name")?.as_str())
    .collect();
  match bins.as_slice() {
    [only] => Ok((*only).to_owned()),
    [] => Err(CliError::Other(format!(
      "{} has no binary to run",
      manifest.display()
    ))),
    several => Err(CliError::Other(format!(
      "{} has several binaries ({}): set `default-run` in its [package] to choose the game's",
      manifest.display(),
      several.join(", ")
    ))),
  }
}

/// Runs `cargo build` with `args` in `project`, printing cargo's diagnostics
/// as it goes, and returns the executable it built for the project's binary
pub fn build(project: &Path, args: &[&str]) -> CliResult<PathBuf> {
  build_with(project, args, &[])
}

pub(crate) fn build_with(project: &Path, args: &[&str], env: &[EnvVar]) -> CliResult<PathBuf> {
  let mut command = Command::new("cargo");
  command.arg("build").args(args);
  apply_env(&mut command, env);
  execute(command, project)
}

/// Runs a prepared `cargo build` (its arguments after `build`, its
/// environment) in `project`, for the binary `cargo run` would run, and
/// returns that executable
pub fn execute(mut command: Command, project: &Path) -> CliResult<PathBuf> {
  let manifest = canonical(&project.join("Cargo.toml"));
  let bin = game_bin(&metadata(project, &[])?, &manifest)?;
  command
    .args(["--bin", &bin])
    .arg("--message-format=json-render-diagnostics")
    .current_dir(project)
    .stdout(Stdio::piped());
  let mut child = command
    .spawn()
    .map_err(|e| CliError::spawn("cargo build", e))?;
  let mut executable = None;
  if let Some(stdout) = child.stdout.take() {
    let mut lines = BufReader::new(stdout);
    let mut line = Vec::new();
    // Read to the end, so cargo's pipe is always drained
    loop {
      line.clear();
      match lines.read_until(b'\n', &mut line) {
        Ok(0) | Err(_) => break,
        Ok(_) => {
          if let Some(path) = executable_of(&String::from_utf8_lossy(&line), &manifest, &bin) {
            executable = Some(path);
          }
        }
      }
    }
  }
  let status = child
    .wait()
    .map_err(|e| CliError::spawn("cargo build", e))?;
  if !status.success() {
    return Err(CliError::Build { status });
  }
  executable.ok_or_else(|| CliError::Other(format!("cargo built no executable for {bin}")))
}

/// The executable a `compiler-artifact` message names, when it is the binary
/// `bin` of the package at `manifest`
fn executable_of(line: &str, manifest: &Path, bin: &str) -> Option<PathBuf> {
  let message: serde_json::Value = serde_json::from_str(line).ok()?;
  if message.get("reason")?.as_str()? != "compiler-artifact" {
    return None;
  }
  let target = message.get("target")?;
  let is_bin = target
    .get("kind")?
    .as_array()?
    .iter()
    .any(|kind| kind.as_str() == Some("bin"));
  let executable = message.get("executable")?.as_str()?;
  let of_project = canonical(Path::new(message.get("manifest_path")?.as_str()?)) == manifest;
  (is_bin && of_project && target.get("name")?.as_str()? == bin).then(|| PathBuf::from(executable))
}

fn canonical(path: &Path) -> PathBuf {
  path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn apply_env(command: &mut Command, env: &[EnvVar]) {
  for (key, value) in env {
    match value {
      Some(value) => command.env(key, value),
      None => command.env_remove(key),
    };
  }
}

/// Runs the game `exe` in `cwd` with the CLI's environment. A failure exit
/// is [`CliError::GameExited`] with the game's own code
/// (128 plus the signal when a Unix signal ended it)
pub fn run_game(exe: &Path, cwd: &Path) -> CliResult<()> {
  run_game_with(exe, cwd, &[])
}

/// [`run_game`] with extra environment variables for the game
pub fn run_game_with(exe: &Path, cwd: &Path, env: &[EnvVar]) -> CliResult<()> {
  let mut command = Command::new(exe);
  command.current_dir(cwd);
  apply_env(&mut command, env);
  let status = command
    .status()
    .map_err(|e| CliError::spawn(&exe.display().to_string(), e))?;
  match exit_code(status) {
    None => Ok(()),
    Some(code) => Err(CliError::GameExited { code }),
  }
}

/// The failure code a finished process counts as: `None` for success
fn exit_code(status: ExitStatus) -> Option<i32> {
  if status.success() {
    return None;
  }
  #[cfg(unix)]
  {
    use std::os::unix::process::ExitStatusExt;
    if let Some(signal) = status.signal() {
      return Some(128 + signal);
    }
  }
  Some(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;

  /// Where the temporary crates build: under the workspace's target folder,
  /// so the builds are kept between runs and cleaned with it
  fn shared_target() -> PathBuf {
    let workspace = target_dir(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let dir = workspace.join("cli-test-crates");
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
  }

  /// A crate `name` with one binary and no dependencies in a fresh folder
  fn crate_with_main(name: &str, main: &str) -> (tempfile::TempDir, PathBuf) {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().canonicalize().unwrap().join(name);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
      root.join("Cargo.toml"),
      format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    )
    .unwrap();
    fs::write(root.join("src/main.rs"), main).unwrap();
    (folder, root)
  }

  fn shared_env(target: &Path) -> [EnvVar<'_>; 1] {
    [("CARGO_TARGET_DIR", Some(target.as_os_str()))]
  }

  #[test]
  fn the_target_folder_comes_from_cargo_metadata_and_honours_cargo_target_dir() {
    let (_folder, root) = crate_with_main("tgt", "fn main() {}");
    let clean = [("CARGO_TARGET_DIR", None), ("CARGO_BUILD_TARGET_DIR", None)];
    assert_eq!(target_dir_with(&root, &clean).unwrap(), root.join("target"));

    let elsewhere = tempfile::tempdir().unwrap().keep().canonicalize().unwrap();
    let set = [("CARGO_TARGET_DIR", Some(elsewhere.as_os_str()))];
    assert_eq!(target_dir_with(&root, &set).unwrap(), elsewhere);
    fs::remove_dir_all(&elsewhere).unwrap();

    // A target-dir in .cargo/config.toml
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::write(
      root.join(".cargo/config.toml"),
      "[build]\ntarget-dir = \"out\"\n",
    )
    .unwrap();
    assert_eq!(target_dir_with(&root, &clean).unwrap(), root.join("out"));
    fs::remove_dir_all(root.join(".cargo")).unwrap();

    // A comment mentioning [workspace] does not make a workspace
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    fs::write(
      root.join("Cargo.toml"),
      format!("# [workspace]\n{manifest}"),
    )
    .unwrap();
    assert_eq!(target_dir_with(&root, &clean).unwrap(), root.join("target"));

    // A member's target folder is its workspace's
    let (_folder, root) = crate_with_main("member", "fn main() {}");
    let parent = root.parent().unwrap();
    fs::write(
      parent.join("Cargo.toml"),
      "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    assert_eq!(
      target_dir_with(&root, &clean).unwrap(),
      parent.join("target")
    );
  }

  #[test]
  fn build_returns_the_executable_cargo_reports() {
    let (_folder, root) = crate_with_main("reports", "fn main() {}");
    let target = shared_target();
    let exe = build_with(&root, &[], &shared_env(&target)).unwrap();
    assert!(exe.is_file(), "{}", exe.display());
    assert!(exe.starts_with(&target), "{}", exe.display());
    assert_eq!(
      exe.file_stem().and_then(OsStr::to_str),
      Some("reports"),
      "{}",
      exe.display()
    );
    assert!(run_game(&exe, &root).is_ok());
  }

  #[test]
  fn a_compile_error_is_a_build_failure() {
    let (_folder, root) = crate_with_main("broken", "fn main() { let x: u32 = \"no\"; }");
    let target = shared_target();
    let error = build_with(&root, &[], &shared_env(&target)).unwrap_err();
    assert!(matches!(error, CliError::Build { .. }), "{error:?}");
    assert_eq!(error.exit_code(), 1);
  }

  #[test]
  fn run_game_s_failure_is_the_game_s_code() {
    let target = shared_target();
    let (_folder, exits) = crate_with_main("exits3", "fn main() { std::process::exit(3); }");
    let exe = build_with(&exits, &[], &shared_env(&target)).unwrap();
    let error = run_game(&exe, &exits).unwrap_err();
    assert!(
      matches!(error, CliError::GameExited { code: 3 }),
      "{error:?}"
    );

    let (_folder, panics) = crate_with_main("panics", "fn main() { panic!(\"on purpose\"); }");
    let exe = build_with(&panics, &[], &shared_env(&target)).unwrap();
    let error = run_game(&exe, &panics).unwrap_err();
    assert!(
      matches!(error, CliError::GameExited { code: 101 }),
      "{error:?}"
    );
  }

  /// A crate in a folder named like neither bin, with a bin per `(name, exit code)`
  fn crate_with_bins(extra: &str, bins: &[(&str, i32)]) -> (tempfile::TempDir, PathBuf) {
    let (folder, root) = crate_with_main("folder_name", "fn main() {}");
    fs::remove_file(root.join("src/main.rs")).unwrap();
    let mut manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    manifest = manifest.replace(
      "edition = \"2021\"\n",
      &format!("edition = \"2021\"\n{extra}\n"),
    );
    for (name, code) in bins {
      fs::write(
        root.join(format!("src/{name}.rs")),
        format!("fn main() {{ std::process::exit({code}); }}"),
      )
      .unwrap();
      manifest.push_str(&format!(
        "\n[[bin]]\nname = \"{name}\"\npath = \"src/{name}.rs\"\n"
      ));
    }
    fs::write(root.join("Cargo.toml"), manifest).unwrap();
    (folder, root)
  }

  #[test]
  fn the_game_is_the_default_run_binary_or_the_only_one() {
    let target = shared_target();
    let env = shared_env(&target);

    let (_folder, root) = crate_with_bins("default-run = \"game\"", &[("tool", 4), ("game", 3)]);
    let exe = build_with(&root, &[], &env).unwrap();
    assert_eq!(exe.file_stem().and_then(OsStr::to_str), Some("game"));
    assert!(matches!(
      run_game(&exe, &root),
      Err(CliError::GameExited { code: 3 })
    ));

    let (_folder, root) = crate_with_bins("", &[("tool", 4), ("game", 3)]);
    let error = build_with(&root, &[], &env).unwrap_err();
    let said = error.report();
    assert!(
      said.contains("tool") && said.contains("game") && said.contains("default-run"),
      "{said}"
    );

    // One binary, named unlike its folder
    let (_folder, root) = crate_with_bins("", &[("lonely", 5)]);
    let exe = build_with(&root, &[], &env).unwrap();
    assert_eq!(exe.file_stem().and_then(OsStr::to_str), Some("lonely"));
  }

  #[cfg(unix)]
  #[test]
  fn a_signal_is_128_plus_its_number() {
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(exit_code(ExitStatus::from_raw(9)), Some(137));
    assert_eq!(exit_code(ExitStatus::from_raw(3 << 8)), Some(3));
    assert_eq!(exit_code(ExitStatus::from_raw(0)), None);
  }
}
