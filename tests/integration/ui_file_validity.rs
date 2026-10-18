//! Every `*.ui.ron` file in the repository is valid RON and a valid UI file

use std::path::{Path, PathBuf};

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

/// The project a file belongs to: the parent of its `assets` directory,
/// where its imports and images resolve from
fn project_of(file: &Path) -> PathBuf {
  file
    .ancestors()
    .find(|dir| dir.file_name().is_some_and(|name| name == "assets"))
    .and_then(Path::parent)
    .or_else(|| file.parent())
    .map(Path::to_path_buf)
    .unwrap_or_default()
}

#[test]
fn every_ui_file_is_valid_ron_and_a_valid_ui_file() {
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
    if text.contains("imports") {
      let project = project_of(file);
      let relative = file.strip_prefix(&project).unwrap_or(file);
      let mut assets = AssetManager::with_base_path(&project);
      if let Err(error) = assets.load_ui(relative) {
        problems.push(format!("{shown}: {error}"));
      }
    } else if let Err(error) = UiFile::parse(&shown.to_string(), &text) {
      problems.push(format!("{shown}: {error}"));
    }
  }
  assert!(problems.is_empty(), "{}", problems.join("\n"));
}
