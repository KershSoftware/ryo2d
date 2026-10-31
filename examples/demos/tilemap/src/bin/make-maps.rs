//! Writes the tilemap demo's authored files under `assets/`, the same bytes
//! each run:
//!
//! - `images/ground.png`, `images/dungeon-terrain.png`, `images/backdrop.png`:
//!   tiles derived from Kenney's sheets (see `make-maps/art.rs`)
//! - `rooms/rooms.world`: the three rooms side by side, as Tiled writes a
//!   world file
//! - `ldtk/dungeon.ldtk`: the LDtk project (see `make-maps/ldtk.rs`)
//! - the character demo's copy of its `Platformer` level, under
//!   `examples/demos/character/assets/`: the project trimmed to that level,
//!   with the terrain sheet and Tiny Dungeon's licence it needs
//! - with Tiled 1.12 installed: `project.tiled-project` and every Tiled
//!   tileset and map, written by Tiled itself running `scripts/make-maps.js`;
//!   then each Tiled file is opened and exported by Tiled's command line,
//!   which fails on anything it cannot read
//!
//! Tiled is looked for in `$TILED`, then at
//! `/Applications/Tiled.app/Contents/MacOS/Tiled`, then as `tiled` on the
//! path. Without it the Tiled files are left as they are.
//!
//! `cargo run -p tilemap-demo --bin make-maps`

#[path = "make-maps/art.rs"]
mod art;
#[path = "make-maps/json.rs"]
mod json;
#[path = "make-maps/ldtk.rs"]
mod ldtk;

use std::path::{Path, PathBuf};
use std::process::Command;

use json::{J, obj};

/// The rooms of the world: file, and left edge in pixels
const ROOMS: [(&str, i64); 3] = [("room-1.tmj", 0), ("room-2.tmx", 320), ("room-3.tmj", 640)];
/// A room's size in pixels (20 x 12 cells of 16 px)
const ROOM_SIZE: (i64, i64) = (320, 192);

/// The Tiled files the script writes, relative to `assets/`
const TILESETS: [&str; 4] = [
  "tilesets/platformer.tsx",
  "tilesets/ground.tsx",
  "tilesets/dungeon.tsj",
  "tilesets/dungeon-terrain.tsj",
];
const MAPS: [&str; 5] = [
  "maps/platformer.tmx",
  "maps/topdown.tmj",
  "rooms/room-1.tmj",
  "rooms/room-2.tmx",
  "rooms/room-3.tmj",
];

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
  for line in art::write(&assets)? {
    println!("{line}");
  }
  write(&assets.join("rooms/rooms.world"), &json::qt(&world()))?;
  println!("rooms/rooms.world");
  write(&assets.join("ldtk/dungeon.ldtk"), &ldtk::project())?;
  println!("ldtk/dungeon.ldtk");
  write_character_assets(root, &assets)?;

  let Some(tiled) = find_tiled() else {
    println!("Tiled not found: the Tiled project, tilesets and maps were left as they are");
    return Ok(());
  };
  write(&assets.join("project.tiled-project"), PROJECT_STUB)?;
  // Tiled writes into folders that are there, and makes none
  for folder in ["tilesets", "maps", "rooms"] {
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

/// The character demo's copy of the Platformer level: the project trimmed
/// to it, and the art it draws with, so the level has one source
fn write_character_assets(root: &Path, assets: &Path) -> Result<(), String> {
  let character = root.join("../character/assets");
  write(
    &character.join("ldtk/platformer.ldtk"),
    &ldtk::platformer_project(),
  )?;
  println!("character: ldtk/platformer.ldtk");
  for file in [
    "images/dungeon-terrain.png",
    "kenney-tiny-dungeon/License.txt",
  ] {
    let bytes = std::fs::read(assets.join(file)).map_err(|error| format!("{file}: {error}"))?;
    let to = character.join(file);
    if let Some(folder) = to.parent() {
      std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    std::fs::write(&to, bytes).map_err(|error| format!("{}: {error}", to.display()))?;
    println!("character: {file}");
  }
  Ok(())
}

/// Opens every Tiled file with Tiled's command line and exports it to JSON
/// in a scratch folder: Tiled exits with an error on a file it cannot read
fn check(tiled: &Path, assets: &Path) -> Result<(), String> {
  let scratch = std::env::temp_dir().join(format!("tilemap-demo-check-{}", std::process::id()));
  std::fs::create_dir_all(&scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
  let mut failed: Vec<&str> = Vec::new();
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
    if status.success() && out.exists() {
      println!("checked {path}");
    } else {
      failed.push(path);
    }
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

/// The world file, as Tiled writes one
fn world() -> J {
  let maps = ROOMS
    .iter()
    .map(|(file, x)| {
      obj([
        ("fileName", (*file).into()),
        ("height", ROOM_SIZE.1.into()),
        ("width", ROOM_SIZE.0.into()),
        ("x", (*x).into()),
        ("y", 0.into()),
      ])
    })
    .collect();
  obj([
    ("maps", J::Arr(maps)),
    ("onlyShowAdjacentMaps", false.into()),
    ("type", "world".into()),
  ])
}

fn write(path: &Path, text: &str) -> Result<(), String> {
  if let Some(folder) = path.parent() {
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
  }
  std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}
