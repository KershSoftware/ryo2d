//! Capture every page of every demo without a window.
//!
//! `cargo run -p demo-kit --bin demo-tour -- [demo ...]` (or
//! `./run demos:tour [demo ...]`) builds the demos, runs each page with
//! `RYO_HEADLESS=1` and `RYO_DEMO_PAGE`, and saves
//! `target/demo-tour/<demo>/page-<n>.png`. Exits non-zero if any page
//! panics, times out or captures nothing.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const CAPTURE_FRAME: u32 = 90;
const TIMEOUT: Duration = Duration::from_secs(60);

struct DemoBin {
  name: String,
  dir: PathBuf,
  package: String,
  bin: String,
}

/// How a page's run ended
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
  Code(i32),
  Signal,
  TimedOut,
}

fn main() -> ExitCode {
  let wanted: Vec<String> = std::env::args().skip(1).collect();
  match tour(&wanted) {
    Ok(true) => ExitCode::SUCCESS,
    Ok(false) => ExitCode::FAILURE,
    Err(error) => {
      eprintln!("demo-tour: {error}");
      ExitCode::FAILURE
    }
  }
}

fn tour(wanted: &[String]) -> Result<bool, String> {
  let metadata = cargo_metadata(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))?;
  let root = workspace_root(&metadata)?;
  let target_dir = PathBuf::from(
    metadata["target_directory"]
      .as_str()
      .ok_or("cargo metadata has no target_directory")?,
  );

  let mut demos = Vec::new();
  for dir in demo_dirs(&root.join("examples").join("demos"), wanted)? {
    let (package, bin) =
      bin_for(&metadata, &dir).ok_or_else(|| format!("no binary for {}", dir.display()))?;
    let name = dir
      .file_name()
      .and_then(|n| n.to_str())
      .unwrap_or_default()
      .to_string();
    demos.push(DemoBin {
      name,
      dir,
      package,
      bin,
    });
  }
  build(&root, &demos)?;

  let out_root = target_dir.join("demo-tour");
  let mut all_ok = true;
  for demo in &demos {
    let bin =
      target_dir
        .join("debug")
        .join(format!("{}{}", demo.bin, std::env::consts::EXE_SUFFIX));
    match tour_demo(demo, &bin, &out_root.join(&demo.name)) {
      Ok(summary) => println!("{}: {summary}", demo.name),
      Err(error) => {
        all_ok = false;
        println!("{}: FAILED {error}", demo.name);
      }
    }
  }
  println!("Frames: {}", out_root.display());
  Ok(all_ok)
}

fn cargo_metadata(root: &Path) -> Result<serde_json::Value, String> {
  let output = Command::new("cargo")
    .args(["metadata", "--no-deps", "--format-version", "1"])
    .current_dir(root)
    .output()
    .map_err(|e| format!("cargo metadata: {e}"))?;
  if !output.status.success() {
    return Err(format!(
      "cargo metadata failed: {}",
      String::from_utf8_lossy(&output.stderr)
    ));
  }
  serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))
}

/// The repository root as cargo writes paths, so demo folders found under it
/// compare equal to its manifest paths (`fs::canonicalize` gives `\\?\C:\`
/// paths on Windows where cargo gives `C:\`)
fn workspace_root(metadata: &serde_json::Value) -> Result<PathBuf, String> {
  metadata["workspace_root"]
    .as_str()
    .map(PathBuf::from)
    .ok_or_else(|| "cargo metadata has no workspace_root".to_string())
}

fn build(root: &Path, demos: &[DemoBin]) -> Result<(), String> {
  let mut command = Command::new("cargo");
  command.arg("build").arg("--bins").current_dir(root);
  for demo in demos {
    command.args(["-p", &demo.package]);
  }
  let status = command.status().map_err(|e| format!("cargo build: {e}"))?;
  if status.success() {
    Ok(())
  } else {
    Err("cargo build failed".to_string())
  }
}

/// Demo folders (those with a `Cargo.toml`), sorted; only `wanted` if given
fn demo_dirs(demos_dir: &Path, wanted: &[String]) -> Result<Vec<PathBuf>, String> {
  let entries = fs::read_dir(demos_dir).map_err(|e| format!("{}: {e}", demos_dir.display()))?;
  let mut dirs: Vec<PathBuf> = entries
    .filter_map(|entry| entry.ok())
    .map(|entry| entry.path())
    .filter(|path| path.join("Cargo.toml").is_file())
    .collect();
  dirs.sort();
  if wanted.is_empty() {
    return Ok(dirs);
  }
  wanted
    .iter()
    .map(|name| {
      dirs
        .iter()
        .find(|dir| dir.file_name().is_some_and(|n| n == name.as_str()))
        .cloned()
        .ok_or_else(|| format!("no demo named {name:?} in {}", demos_dir.display()))
    })
    .collect()
}

/// The package and binary target whose manifest is in `dir`
fn bin_for(metadata: &serde_json::Value, dir: &Path) -> Option<(String, String)> {
  metadata["packages"].as_array()?.iter().find_map(|package| {
    let manifest = Path::new(package["manifest_path"].as_str()?);
    if manifest.parent()? != dir {
      return None;
    }
    let bin = package["targets"].as_array()?.iter().find(|target| {
      target["kind"]
        .as_array()
        .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
    })?;
    Some((
      package["name"].as_str()?.to_string(),
      bin["name"].as_str()?.to_string(),
    ))
  })
}

fn tour_demo(demo: &DemoBin, bin: &Path, out_dir: &Path) -> Result<String, String> {
  let _ = fs::remove_dir_all(out_dir);
  fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
  let first = run_page(demo, bin, out_dir, 1)?;
  let Some(pages) = parse_page_count(&first) else {
    return Ok("1 capture (not on demo-kit)".to_string());
  };
  for page in 2..=pages {
    run_page(demo, bin, out_dir, page)?;
  }
  Ok(format!(
    "{pages} {} ok",
    if pages == 1 { "page" } else { "pages" }
  ))
}

/// Run one page, keep its frame as `page-<n>.png`, and return its output
fn run_page(demo: &DemoBin, bin: &Path, out_dir: &Path, page: usize) -> Result<String, String> {
  let capture_dir = out_dir.join(format!("page-{page}.capture"));
  let log_path = out_dir.join(format!("page-{page}.log"));
  let log = File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
  let log_err = log.try_clone().map_err(|e| e.to_string())?;
  let mut child = Command::new(bin)
    .current_dir(&demo.dir)
    .env("RYO_HEADLESS", "1")
    .env("RYO_DEMO_PAGE", page.to_string())
    .env("RYO_CAPTURE_FRAMES", CAPTURE_FRAME.to_string())
    .env("RYO_CAPTURE_EXIT", "1")
    .env("RYO_CAPTURE_DIR", &capture_dir)
    .stdout(Stdio::from(log))
    .stderr(Stdio::from(log_err))
    .spawn()
    .map_err(|e| format!("page {page}: {e}"))?;

  let started = Instant::now();
  let exit = loop {
    match child.try_wait() {
      Ok(Some(status)) => break status.code().map_or(Exit::Signal, Exit::Code),
      Ok(None) if started.elapsed() > TIMEOUT => {
        let _ = child.kill();
        let _ = child.wait();
        break Exit::TimedOut;
      }
      Ok(None) => sleep(Duration::from_millis(50)),
      Err(e) => return Err(format!("page {page}: {e}")),
    }
  };

  let output = fs::read_to_string(&log_path).unwrap_or_default();
  let frame = capture_dir.join(format!("frame-{CAPTURE_FRAME:06}.png"));
  judge(exit, &output, frame.is_file())
    .map_err(|e| format!("page {page}: {e} (log: {})", log_path.display()))?;
  fs::rename(&frame, out_dir.join(format!("page-{page}.png")))
    .map_err(|e| format!("page {page}: {e}"))?;
  let _ = fs::remove_dir_all(&capture_dir);
  Ok(output)
}

/// The page count from the kit's `[demo] <title>: <m> pages` line
fn parse_page_count(output: &str) -> Option<usize> {
  output.lines().find_map(|line| {
    let rest = line.trim().strip_prefix("[demo] ")?;
    let (_, count) = rest.rsplit_once(": ")?;
    let number = count
      .strip_suffix(" pages")
      .or_else(|| count.strip_suffix(" page"))?;
    number.parse().ok()
  })
}

/// Whether a page's run succeeded
fn judge(exit: Exit, output: &str, captured: bool) -> Result<(), String> {
  if let Some(line) = output.lines().find(|line| line.contains("panicked")) {
    return Err(format!("panicked: {}", line.trim()));
  }
  match exit {
    Exit::TimedOut => return Err(format!("timed out after {} s", TIMEOUT.as_secs())),
    Exit::Signal => return Err("killed by a signal".to_string()),
    Exit::Code(0) => {}
    Exit::Code(code) => return Err(format!("exited with code {code}")),
  }
  if !captured {
    return Err("no frame captured".to_string());
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn page_count_comes_from_the_startup_line() {
    assert_eq!(parse_page_count("noise\n[demo] Bloom: 6 pages\n"), Some(6));
    assert_eq!(parse_page_count("[demo] Fog: 1 page"), Some(1));
    assert_eq!(
      parse_page_count("[demo] RYO_DEMO_PAGE=7, but Bloom has 6 pages"),
      None
    );
    assert_eq!(parse_page_count("Particle System Demo"), None);
  }

  #[test]
  fn runs_are_judged_on_panics_exit_and_frame() {
    assert_eq!(judge(Exit::Code(0), "fine", true), Ok(()));
    assert!(
      judge(Exit::Code(0), "thread 'main' panicked at x.rs:1", true)
        .is_err_and(|e| e.contains("panicked"))
    );
    assert!(judge(Exit::Code(1), "", true).is_err_and(|e| e.contains("code 1")));
    assert!(judge(Exit::TimedOut, "", false).is_err_and(|e| e.contains("timed out")));
    assert!(judge(Exit::Signal, "", true).is_err());
    assert!(judge(Exit::Code(0), "", false).is_err_and(|e| e.contains("no frame")));
  }

  #[test]
  fn demo_dirs_are_sorted_and_filtered() {
    let root = tempfile::tempdir().unwrap();
    for name in ["bloom", "trails", "notes"] {
      std::fs::create_dir(root.path().join(name)).unwrap();
    }
    std::fs::write(root.path().join("bloom/Cargo.toml"), "").unwrap();
    std::fs::write(root.path().join("trails/Cargo.toml"), "").unwrap();

    let all = demo_dirs(root.path(), &[]).unwrap();
    let names: Vec<_> = all
      .iter()
      .map(|d| d.file_name().unwrap().to_owned())
      .collect();
    assert_eq!(names, ["bloom", "trails"]);

    let one = demo_dirs(root.path(), &["trails".to_string()]).unwrap();
    assert_eq!(one.len(), 1);
    assert!(demo_dirs(root.path(), &["nope".to_string()]).is_err());
  }

  #[test]
  fn every_demo_folder_under_cargos_root_has_a_binary() {
    let metadata = cargo_metadata(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let root = workspace_root(&metadata).unwrap();
    let dirs = demo_dirs(&root.join("examples").join("demos"), &[]).unwrap();
    assert!(!dirs.is_empty());
    for dir in dirs {
      assert!(bin_for(&metadata, &dir).is_some(), "{}", dir.display());
    }
  }

  #[test]
  fn the_binary_is_found_from_cargo_metadata() {
    let dir = Path::new("/repo/examples/demos/bloom");
    let metadata = serde_json::json!({
      "packages": [
        { "name": "other", "manifest_path": "/repo/Cargo.toml",
          "targets": [{ "name": "other", "kind": ["lib"] }] },
        { "name": "bloom-demo", "manifest_path": "/repo/examples/demos/bloom/Cargo.toml",
          "targets": [{ "name": "bloom-demo", "kind": ["bin"] }] }
      ]
    });
    assert_eq!(
      bin_for(&metadata, dir),
      Some(("bloom-demo".to_string(), "bloom-demo".to_string()))
    );
    assert_eq!(
      bin_for(&metadata, Path::new("/repo/examples/demos/fog")),
      None
    );
  }
}
