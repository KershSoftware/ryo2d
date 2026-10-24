//! Every `*.ui.ron` file in the repository is valid RON and a valid UI file,
//! loaded through the asset registry as its game loads it

use std::path::{Path, PathBuf};

use ryo2d::ecs::World;
use ryo2d::prelude::*;

/// The `*.ui.ron` files under `dir`, skipping build output
fn ui_files(dir: &Path, found: &mut Vec<PathBuf>) {
  let Ok(entries) = std::fs::read_dir(dir) else {
    return;
  };
  for entry in entries.flatten() {
    let path = entry.path();
    if path.is_dir() {
      if path.file_name().is_some_and(|name| name != "target") {
        ui_files(&path, found);
      }
    } else if path.to_string_lossy().ends_with(".ui.ron") {
      found.push(path);
    }
  }
}

/// The asset folder a file belongs to: its project's `assets` directory,
/// where its imports and images resolve from, else its own folder
fn assets_of(file: &Path) -> PathBuf {
  file
    .ancestors()
    .find(|dir| dir.file_name().is_some_and(|name| name == "assets"))
    .or_else(|| file.parent())
    .map(Path::to_path_buf)
    .unwrap_or_default()
}

/// A registry reading `assets` that loads UI files and images
fn registry(assets: &Path) -> (World, AssetServer) {
  let mut world = World::new();
  let server = AssetServer::new(DiskSource::new(assets));
  server.register_asset::<Image>(&mut world);
  server.register_loader(ImageLoader);
  server.register_asset::<UiFile>(&mut world);
  server.register_loader(UiFileLoader);
  world.insert_resource(server.clone());
  (world, server)
}

#[test]
fn every_ui_file_is_valid_ron_and_loads_from_its_asset_folder() {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  let mut files = Vec::new();
  ui_files(&root.join("examples"), &mut files);
  ui_files(&root.join("tests"), &mut files);
  files.sort();
  assert!(
    !files.is_empty(),
    "no *.ui.ron files found under examples/ and tests/"
  );

  let mut problems = Vec::new();
  for file in &files {
    let shown = file.strip_prefix(root).unwrap_or(file).display();
    let text = match std::fs::read_to_string(file) {
      Ok(text) => text,
      Err(error) => {
        problems.push(format!("{shown}: {error}"));
        continue;
      }
    };
    if let Err(error) = ron::from_str::<ron::Value>(&text) {
      problems.push(format!("{shown}: not valid RON: {error}"));
      continue;
    }
    let assets = assets_of(file);
    let path = file
      .strip_prefix(&assets)
      .unwrap_or(file)
      .to_string_lossy()
      .replace('\\', "/");
    let (mut world, server) = registry(&assets);
    if let Err(error) = server.load_now::<UiFile>(&mut world, &path) {
      problems.push(format!("{shown}: {}", UiFileError::from(error)));
    }
  }
  assert!(problems.is_empty(), "{}", problems.join("\n"));
}
