//! Every `*.ui.ron` file in the repository is valid RON and a valid UI file,
//! and every `*.theme.ron` file a valid theme, loaded through the asset
//! registry as its game loads it

use std::path::{Path, PathBuf};

use ryo2d::ecs::World;
use ryo2d::prelude::*;

/// The files under `dir` whose names end in `suffix`, skipping build output
fn files_ending(dir: &Path, suffix: &str, found: &mut Vec<PathBuf>) {
  let Ok(entries) = std::fs::read_dir(dir) else {
    return;
  };
  for entry in entries.flatten() {
    let path = entry.path();
    if path.is_dir() {
      if path.file_name().is_some_and(|name| name != "target") {
        files_ending(&path, suffix, found);
      }
    } else if path.to_string_lossy().ends_with(suffix) {
      found.push(path);
    }
  }
}

/// The files under `examples/` and `tests/` whose names end in `suffix`
fn repo_files(suffix: &str) -> Vec<PathBuf> {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  let mut files = Vec::new();
  files_ending(&root.join("examples"), suffix, &mut files);
  files_ending(&root.join("tests"), suffix, &mut files);
  files.sort();
  assert!(
    !files.is_empty(),
    "no *{suffix} files found under examples/ and tests/"
  );
  files
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

/// A registry reading `assets` that loads UI files, themes and images
fn registry(assets: &Path) -> (World, AssetServer) {
  let mut world = World::new();
  let server = AssetServer::new(DiskSource::new(assets));
  server.register_asset::<Image>(&mut world);
  server.register_loader(ImageLoader);
  server.register_asset::<UiFile>(&mut world);
  server.register_loader(UiFileLoader);
  server.register_asset::<Theme>(&mut world);
  server.register_loader(ThemeLoader);
  world.insert_resource(server.clone());
  (world, server)
}

/// `file`'s asset path in its asset folder
fn asset_path(file: &Path, assets: &Path) -> String {
  file
    .strip_prefix(assets)
    .unwrap_or(file)
    .to_string_lossy()
    .replace('\\', "/")
}

#[test]
fn every_ui_file_is_valid_ron_and_loads_from_its_asset_folder() {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  let mut problems = Vec::new();
  for file in &repo_files(".ui.ron") {
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
    let (mut world, server) = registry(&assets);
    if let Err(error) = server.load_now::<UiFile>(&mut world, &asset_path(file, &assets)) {
      problems.push(format!("{shown}: {}", UiFileError::from(error)));
    }
  }
  assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_theme_file_loads_from_its_asset_folder() {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  let mut problems = Vec::new();
  for file in &repo_files(".theme.ron") {
    let shown = file.strip_prefix(root).unwrap_or(file).display();
    let assets = assets_of(file);
    let (mut world, server) = registry(&assets);
    if let Err(error) = server.load_now::<Theme>(&mut world, &asset_path(file, &assets)) {
      problems.push(format!("{shown}: {}", error.report()));
    }
  }
  assert!(problems.is_empty(), "{}", problems.join("\n"));
}
