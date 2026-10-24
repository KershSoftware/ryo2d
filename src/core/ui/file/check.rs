//! Checking UI files from disk without running a game (`ryo2d check-ui`)

use std::path::{Path, PathBuf};

use crate::core::asset::{AssetServer, DiskSource, ImageLoader, UiFileLoader};
use crate::core::ecs::kernel::World;
use crate::core::graphics::Image;

use super::document::UiFile;
use super::error::{UiFileError, UiFileIssue};

/// Every `*.ui.ron` under `dir`, sorted
pub(crate) fn ui_files_under(dir: &Path) -> Vec<PathBuf> {
  let mut found = Vec::new();
  let mut pending = vec![dir.to_path_buf()];
  while let Some(next) = pending.pop() {
    let Ok(entries) = std::fs::read_dir(&next) else {
      continue;
    };
    for entry in entries.flatten() {
      let path = entry.path();
      if path.is_dir() {
        pending.push(path);
      } else if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".ui.ron"))
      {
        found.push(path);
      }
    }
  }
  found.sort();
  found
}

/// Load each file through a registry over the asset folder `assets`, as the
/// game would: imports and images are read from it, and each file is
/// checked against them. Issue paths are asset paths; each issue is listed
/// once.
pub(crate) fn check_files(assets: &Path, files: &[PathBuf]) -> Vec<UiFileIssue> {
  let mut world = World::new();
  let server = AssetServer::new(DiskSource::new(assets));
  server.register_asset::<Image>(&mut world);
  server.register_loader(ImageLoader);
  server.register_asset::<UiFile>(&mut world);
  server.register_loader(UiFileLoader);
  world.insert_resource(server.clone());
  let mut issues: Vec<UiFileIssue> = Vec::new();
  for file in files {
    let path = relative(assets, file);
    // A file an earlier one imported is loaded already, or failed already
    let Err(error) = server.load_now::<UiFile>(&mut world, &path) else {
      continue;
    };
    for issue in UiFileError::from(error).issues {
      if !issues.contains(&issue) {
        issues.push(issue);
      }
    }
  }
  issues
}

/// `file`'s asset path under `assets`, with `/` separators
fn relative(assets: &Path, file: &Path) -> String {
  let path = file.strip_prefix(assets).unwrap_or(file);
  path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
  use super::*;

  /// A project's asset folder holding `files`
  fn assets(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in files {
      let path = dir.path().join(name);
      std::fs::create_dir_all(path.parent().unwrap()).unwrap();
      std::fs::write(path, bytes).unwrap();
    }
    dir
  }

  fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::new(1, 1)
      .write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
      )
      .unwrap();
    bytes
  }

  fn shown(dir: &tempfile::TempDir) -> Vec<String> {
    check_files(dir.path(), &ui_files_under(dir.path()))
      .iter()
      .map(ToString::to_string)
      .collect()
  }

  #[test]
  fn a_clean_project_has_no_issues() {
    let png = png();
    let dir = assets(&[
      (
        "ui/common.ui.ron",
        br#"(templates: {"T": (node: Image("icon.png"))})"#,
      ),
      (
        "ui/menu.ui.ron",
        br#"(imports: ["ui/common.ui.ron"], root: T)"#,
      ),
      ("icon.png", &png),
    ]);
    assert_eq!(ui_files_under(dir.path()).len(), 2);
    assert_eq!(shown(&dir), Vec::<String>::new());
  }

  #[test]
  fn a_list_checks_like_any_kind() {
    const BOARD: &str = r#"(
    templates: { "LeaderRow": (node: Row(children: [Button(label: "{rank}. {name}", name: "line")])) },
    root: Column(focus_scope: true, children: [List(count: 1000, row_height: 20, height: 100, width: 200, name: "board", row_shape: LeaderRow)]),
  )"#;
    let clean = assets(&[("ui/board.ui.ron", BOARD.as_bytes())]);
    assert_eq!(shown(&clean), Vec::<String>::new());

    let broken = assets(&[(
      "ui/board.ui.ron",
      b"(root: Column(children: [\n  List(count: 3),\n]))",
    )]);
    assert_eq!(
      shown(&broken),
      [
        "ui/board.ui.ron:2:3: List needs \"row_height\"",
        "ui/board.ui.ron:2:3: List needs \"row_shape\"",
      ]
    );
  }

  #[test]
  fn broken_files_missing_images_and_import_cycles_are_reported() {
    let dir = assets(&[
      ("ui/a.ui.ron", b"(root: Column(\n  gpa: 1))"),
      ("ui/b.ui.ron", br#"(root: Image("gone.png"))"#),
      (
        "ui/c.ui.ron",
        br#"(imports: ["ui/d.ui.ron"], templates: {"C": (node: Column)})"#,
      ),
      (
        "ui/d.ui.ron",
        br#"(imports: ["ui/c.ui.ron"], templates: {"D": (node: Column)})"#,
      ),
    ]);
    assert_eq!(
      shown(&dir),
      [
        "ui/a.ui.ron:2:3: Column has no option \"gpa\"; did you mean \"gap\"?",
        "ui/b.ui.ron:1:14: cannot load the image \"gone.png\": no asset at \"gone.png\"",
        // The cycle fails both files with one error, listed once
        "\"ui/c.ui.ron\" imports itself: ui/c.ui.ron → ui/d.ui.ron → ui/c.ui.ron",
      ]
    );
  }

  #[test]
  fn a_file_outside_the_asset_folder_is_not_an_asset_path() {
    let dir = assets(&[]);
    let elsewhere = assets(&[("menu.ui.ron", b"(root: Column)")]);
    let issues = check_files(dir.path(), &[elsewhere.path().join("menu.ui.ron")]);
    assert_eq!(issues.len(), 1);
    assert!(
      issues[0].message.contains("is not an asset path"),
      "{}",
      issues[0]
    );
  }
}
