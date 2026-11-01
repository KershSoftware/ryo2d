//! Writes the tile grids demo's Tiled files under `assets/`, the same bytes
//! each run: `project.tiled-project`, `tilesets/platformer.tsx` and
//! `maps/endless-meadow.tmx`, written by Tiled itself running
//! `scripts/make-maps.js`. Then each file is opened and exported to JSON by
//! Tiled's command line, which fails on anything it cannot read, and the
//! meadow's export is checked to be an infinite map with chunks at negative
//! cells.
//!
//! Tiled is looked for in `$TILED`, then at
//! `/Applications/Tiled.app/Contents/MacOS/Tiled`, then as `tiled` on the
//! path. Without it nothing is written.
//!
//! `cargo run -p tile-grids-demo --bin make-maps`

use std::path::{Path, PathBuf};
use std::process::Command;

/// The Tiled files the script writes, relative to `assets/`
const TILESETS: [&str; 1] = ["tilesets/platformer.tsx"];
const MAPS: [&str; 1] = ["maps/endless-meadow.tmx"];
/// The map that must be infinite, with chunks at negative cells
const INFINITE: &str = "maps/endless-meadow.tmx";

/// What the script starts the project from; Tiled adds the property types
/// and writes the rest
const PROJECT_STUB: &str = "{\n    \"folders\": [\n        \".\"\n    ]\n}\n";

fn main() {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  if let Err(error) = run(root) {
    eprintln!("make-maps: {error}");
    std::process::exit(1);
  }
}

fn run(root: &Path) -> Result<(), String> {
  let assets = root.join("assets");
  let Some(tiled) = find_tiled() else {
    return Err("Tiled not found: set $TILED or install Tiled 1.12.2; nothing was written".into());
  };
  write(&assets.join("project.tiled-project"), PROJECT_STUB)?;
  // Tiled writes into folders that are there, and makes none
  for folder in ["tilesets", "maps"] {
    let folder = assets.join(folder);
    std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
  }
  let script = root.join("scripts/make-maps.js");
  let output = Command::new(&tiled)
    .current_dir(&assets)
    .arg("--project")
    .arg("project.tiled-project")
    .arg("--evaluate")
    .arg(&script)
    .output()
    .map_err(|error| format!("{}: {error}", tiled.display()))?;
  let log = format!(
    "{}{}",
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr)
  );
  print!("{log}");
  if !output.status.success()
    || log
      .lines()
      .any(|line| line.to_lowercase().contains("error"))
  {
    return Err("Tiled's script failed (see above)".into());
  }
  check(&tiled, &assets)
}

/// Opens every Tiled file with Tiled's command line and exports it to JSON
/// in a scratch folder: Tiled exits with an error on a file it cannot read.
/// The infinite map's export must say it is infinite and hold chunks at
/// negative cells on both axes
fn check(tiled: &Path, assets: &Path) -> Result<(), String> {
  let scratch = std::env::temp_dir().join(format!("tile-grids-check-{}", std::process::id()));
  std::fs::create_dir_all(&scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
  let mut failed: Vec<String> = Vec::new();
  let jobs = TILESETS
    .iter()
    .map(|path| ("--export-tileset", *path))
    .chain(MAPS.iter().map(|path| ("--export-map", *path)));
  for (index, (flag, path)) in jobs.enumerate() {
    let out = scratch.join(format!("{index}.json"));
    let status = Command::new(tiled)
      .current_dir(assets)
      .arg("--project")
      .arg("project.tiled-project")
      .arg("--resolve-types-and-properties")
      .arg(flag)
      .arg("json")
      .arg(path)
      .arg(&out)
      .status()
      .map_err(|error| format!("{}: {error}", tiled.display()))?;
    let Ok(exported) = std::fs::read_to_string(&out) else {
      failed.push(format!("{path} (no export)"));
      continue;
    };
    if !status.success() {
      failed.push(format!("{path} (Tiled failed)"));
      continue;
    }
    if path == INFINITE {
      let compact: String = exported.split_whitespace().collect();
      let infinite = compact.contains("\"infinite\":true");
      let chunks = compact.contains("\"chunks\":[");
      let negative = compact.contains("\"x\":-") && compact.contains("\"y\":-");
      if !(infinite && chunks && negative) {
        failed.push(format!(
          "{path} (infinite {infinite}, chunks {chunks}, chunks at negative cells {negative})"
        ));
        continue;
      }
    }
    println!("checked {path}");
  }
  let _ = std::fs::remove_dir_all(&scratch);
  if failed.is_empty() {
    Ok(())
  } else {
    Err(format!("Tiled could not read {}", failed.join(", ")))
  }
}

fn find_tiled() -> Option<PathBuf> {
  let candidates = std::env::var_os("TILED")
    .map(PathBuf::from)
    .into_iter()
    .chain([PathBuf::from(
      "/Applications/Tiled.app/Contents/MacOS/Tiled",
    )]);
  for candidate in candidates {
    if candidate.is_file() {
      return Some(candidate);
    }
  }
  Command::new("tiled")
    .arg("--version")
    .output()
    .ok()
    .filter(|output| output.status.success())
    .map(|_| PathBuf::from("tiled"))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
  if let Some(folder) = path.parent() {
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
  }
  std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}
