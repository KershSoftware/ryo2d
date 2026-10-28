//! Every audio file a sample game ships loads through the asset registry as
//! the game loads it, and the game plays each clip through a sound or music
//! definition

use std::path::{Path, PathBuf};

use ryo2d::ecs::World;
use ryo2d::prelude::*;

/// The sample games that play sound
const GAMES: [&str; 4] = ["breakout", "bullet-hell", "flappy", "leap-king"];

/// The clip formats the engine plays
const CLIPS: [&str; 4] = [".wav", ".ogg", ".mp3", ".flac"];

/// The files under `dir`, in every folder below it
fn files_under(dir: &Path, found: &mut Vec<PathBuf>) {
  let Ok(entries) = std::fs::read_dir(dir) else {
    return;
  };
  for entry in entries.flatten() {
    let path = entry.path();
    if path.is_dir() {
      files_under(&path, found);
    } else {
      found.push(path);
    }
  }
}

/// A registry reading `assets` that loads clips, sound definitions and music
fn registry(assets: &Path) -> World {
  let mut world = World::new();
  let server = AssetServer::new(DiskSource::new(assets));
  server.register_asset::<AudioClip>(&mut world);
  server.register_loader(AudioClipLoader::default());
  server.register_asset::<Sound>(&mut world);
  server.register_loader(SoundLoader);
  server.register_asset::<Music>(&mut world);
  server.register_loader(MusicLoader);
  world.insert_resource(server);
  world
}

/// `file`'s asset path in `assets`
fn asset_path(file: &Path, assets: &Path) -> String {
  file
    .strip_prefix(assets)
    .unwrap_or(file)
    .to_string_lossy()
    .replace('\\', "/")
}

/// Load the audio file at `path` at once as what its name says it is
fn load(world: &mut World, path: &str) -> Result<(), String> {
  let server = world.resource::<AssetServer>().clone();
  let loaded = if path.ends_with(".sound.ron") {
    server.load_now::<Sound>(world, path).map(drop)
  } else if path.ends_with(".music.ron") {
    server.load_now::<Music>(world, path).map(drop)
  } else {
    server.load_now::<AudioClip>(world, path).map(drop)
  };
  loaded.map_err(|error| error.report())
}

#[test]
fn every_game_audio_file_loads() {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  let mut problems = Vec::new();
  for game in GAMES {
    let assets = root.join("examples/games").join(game).join("assets");
    let mut files = Vec::new();
    files_under(&assets, &mut files);
    files.sort();
    let paths: Vec<String> = files.iter().map(|f| asset_path(f, &assets)).collect();
    let is_clip = |path: &&String| CLIPS.iter().any(|ext| path.ends_with(ext));
    let is_definition =
      |path: &&String| path.ends_with(".sound.ron") || path.ends_with(".music.ron");
    let mut world = registry(&assets);
    for path in paths.iter().filter(|p| is_clip(p) || is_definition(p)) {
      if let Err(error) = load(&mut world, path) {
        problems.push(format!("{game}: {error}"));
      }
    }
    // Each clip is played through a definition naming it
    let definitions: Vec<String> = paths
      .iter()
      .filter(is_definition)
      .filter_map(|path| std::fs::read_to_string(assets.join(path)).ok())
      .collect();
    for clip in paths.iter().filter(is_clip) {
      let named = format!("\"{clip}\"");
      if !definitions.iter().any(|text| text.contains(&named)) {
        problems.push(format!(
          "{game}: {clip} is named by no .sound.ron or .music.ron"
        ));
      }
    }
  }
  assert!(problems.is_empty(), "{problems:#?}");
}
