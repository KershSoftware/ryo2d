//! Writes the tile grids demo's Tiled files under `assets/`, the same bytes
//! each run: `project.tiled-project`, the tilesets in `tilesets/` and the
//! maps in `maps/` (the meadow, the isometric town, the island on two
//! staggered grids, the hex board, and the oblique dungeon and its mirror),
//! written by Tiled itself running `scripts/make-maps.js`. Then each file is
//! opened and exported to JSON by Tiled's command line, which fails on
//! anything it cannot read; the meadow's export is checked to be an
//! infinite map with chunks at negative cells, and each map's to have the
//! grid it was written with.
//!
//! Tiled is looked for in `$TILED`, then at
//! `/Applications/Tiled.app/Contents/MacOS/Tiled`, then as `tiled` on the
//! path. Without it nothing is written.
//!
//! `cargo run -p tile-grids-demo --bin make-grid-maps`

use std::path::{Path, PathBuf};
use std::process::Command;

/// The Tiled files the script writes, relative to `assets/`
const TILESETS: [&str; 5] = [
  "tilesets/platformer.tsx",
  "tilesets/sketch-town-ground.tsx",
  "tilesets/sketch-town-props.tsx",
  "tilesets/hexagon-board.tsx",
  "tilesets/dungeon.tsx",
];
const MAPS: [&str; 7] = [
  "maps/endless-meadow.tmx",
  "maps/town.tmx",
  "maps/island-rows.tmx",
  "maps/island-columns.tmx",
  "maps/hex-board.tmx",
  "maps/dungeon.tmx",
  "maps/dungeon-mirror.tmx",
];
/// What each map's export must say of its grid (its JSON, spaces taken out)
const GRIDS: [(&str, &[&str]); 6] = [
  ("maps/town.tmx", &["\"orientation\":\"isometric\""]),
  (
    "maps/island-rows.tmx",
    &["\"orientation\":\"staggered\"", "\"staggeraxis\":\"y\""],
  ),
  (
    "maps/island-columns.tmx",
    &["\"orientation\":\"staggered\"", "\"staggeraxis\":\"x\""],
  ),
  (
    "maps/hex-board.tmx",
    &["\"orientation\":\"hexagonal\"", "\"hexsidelength\":35"],
  ),
  (
    "maps/dungeon.tmx",
    &["\"orientation\":\"oblique\"", "\"skewx\":4"],
  ),
  (
    "maps/dungeon-mirror.tmx",
    &["\"orientation\":\"oblique\"", "\"skewx\":-4"],
  ),
];
/// The map that must be infinite, with chunks at negative cells
const INFINITE: &str = "maps/endless-meadow.tmx";

/// What the script starts the project from; Tiled adds the property types
/// and writes the rest
const PROJECT_STUB: &str = "{\n    \"folders\": [\n        \".\"\n    ]\n}\n";

fn main() {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  if let Err(error) = run(root) {
    eprintln!("make-grid-maps: {error}");
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
/// negative cells on both axes, and each other map's its grid
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
    let compact: String = exported.split_whitespace().collect();
    if let Some((_, says)) = GRIDS.iter().find(|(map, _)| *map == path) {
      let missing: Vec<&str> = says
        .iter()
        .copied()
        .filter(|said| !compact.contains(said))
        .collect();
      if !missing.is_empty() {
        failed.push(format!("{path} (its export lacks {})", missing.join(", ")));
        continue;
      }
    }
    if path == INFINITE {
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
