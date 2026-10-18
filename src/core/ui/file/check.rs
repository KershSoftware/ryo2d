//! Checking UI files from disk without running a game (`ryo2d check-ui`)

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::core::asset::SpriteHandle;
use crate::core::graphics::Sprite;

use super::build::SpriteSource;
use super::document::{ImportResolver, UiFile};
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

/// Check each file, resolving imports and images against `root`. Issue paths
/// are relative to `root`, with `/` separators; each issue is listed once.
pub(crate) fn check_files(root: &Path, files: &[PathBuf]) -> Vec<UiFileIssue> {
  let mut disk = Disk {
    root: root.to_path_buf(),
    loaded: HashMap::new(),
    loading: Vec::new(),
  };
  let mut issues: Vec<UiFileIssue> = Vec::new();
  for file in files {
    let name = relative(root, file);
    let result = match std::fs::read_to_string(root.join(&name)) {
      Ok(text) => {
        let mut sprites = disk.sprites();
        UiFile::check(&name, &text, &mut disk, &mut sprites).map(|_| ())
      }
      Err(e) => Err(UiFileError::one(
        &name,
        format!("cannot read the file: {e}"),
      )),
    };
    if let Err(error) = result {
      for issue in error.issues {
        if !issues.contains(&issue) {
          issues.push(issue);
        }
      }
    }
  }
  issues
}

fn relative(root: &Path, file: &Path) -> String {
  let path = file.strip_prefix(root).unwrap_or(file);
  path.to_string_lossy().replace('\\', "/")
}

/// Reads imports from disk, each file once per run, refusing circles
struct Disk {
  root: PathBuf,
  loaded: HashMap<String, UiFile>,
  loading: Vec<String>,
}

/// Images that exist under the root, as 1x1 placeholders
struct DiskSprites {
  root: PathBuf,
  handles: HashMap<String, SpriteHandle>,
}

impl Disk {
  /// A sprite source of its own, so the resolver and sprites borrow apart
  fn sprites(&self) -> DiskSprites {
    DiskSprites {
      root: self.root.clone(),
      handles: HashMap::new(),
    }
  }
}

impl SpriteSource for DiskSprites {
  fn sprite(&mut self, path: &str) -> Result<SpriteHandle, String> {
    if !self.root.join(path).is_file() {
      return Err(format!("no image at \"{path}\""));
    }
    let handle = self
      .handles
      .entry(path.to_string())
      .or_insert_with(|| SpriteHandle::new(Sprite::solid_rect(1, 1, 0, 0, 0, 0), path.into()));
    Ok(handle.clone())
  }
}

impl ImportResolver for Disk {
  fn import(&mut self, path: &str) -> Result<UiFile, UiFileError> {
    if let Some(file) = self.loaded.get(path) {
      return Ok(file.clone());
    }
    if let Some(start) = self.loading.iter().position(|p| p == path) {
      let chain: Vec<&str> = self.loading[start..]
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(path))
        .collect();
      return Err(UiFileError::one(
        path,
        format!("the imports go round in a circle: {}", chain.join(" -> ")),
      ));
    }
    let text = std::fs::read_to_string(self.root.join(path))
      .map_err(|e| UiFileError::one(path, format!("cannot read the file: {e}")))?;
    self.loading.push(path.to_string());
    let mut sprites = self.sprites();
    let file = UiFile::load(path, &text, self, &mut sprites, false);
    self.loading.pop();
    if let Ok(file) = &file {
      self.loaded.insert(path.to_string(), file.clone());
    }
    file
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
      let path = dir.path().join(name);
      std::fs::create_dir_all(path.parent().unwrap()).unwrap();
      std::fs::write(path, text).unwrap();
    }
    dir
  }

  #[test]
  fn a_clean_project_has_no_issues() {
    let dir = project(&[
      (
        "assets/ui/common.ui.ron",
        r#"(templates: {"T": (node: Image("assets/icon.png"))})"#,
      ),
      (
        "assets/ui/menu.ui.ron",
        r#"(imports: ["assets/ui/common.ui.ron"], root: T)"#,
      ),
      ("assets/icon.png", ""),
    ]);
    let files = ui_files_under(&dir.path().join("assets"));
    assert_eq!(files.len(), 2);
    assert_eq!(check_files(dir.path(), &files), []);
  }

  #[test]
  fn a_list_checks_like_any_kind() {
    const BOARD: &str = r#"(
    templates: { "LeaderRow": (node: Row(children: [Button(label: "{rank}. {name}", name: "line")])) },
    root: Column(focus_scope: true, children: [List(count: 1000, row_height: 20, height: 100, width: 200, name: "board", row_shape: LeaderRow)]),
  )"#;
    let clean = project(&[("assets/ui/board.ui.ron", BOARD)]);
    let files = ui_files_under(&clean.path().join("assets"));
    assert_eq!(check_files(clean.path(), &files), []);

    let broken = project(&[(
      "assets/ui/board.ui.ron",
      "(root: Column(children: [\n  List(count: 3),\n]))",
    )]);
    let issues = check_files(
      broken.path(),
      &ui_files_under(&broken.path().join("assets")),
    );
    let shown: Vec<String> = issues.iter().map(ToString::to_string).collect();
    assert_eq!(
      shown,
      [
        "assets/ui/board.ui.ron:2:3: List needs \"row_height\"",
        "assets/ui/board.ui.ron:2:3: List needs \"row_shape\"",
      ]
    );
  }

  #[test]
  fn broken_files_and_missing_images_are_reported_with_positions() {
    let dir = project(&[
      ("assets/ui/a.ui.ron", "(root: Column(\n  gpa: 1))"),
      ("assets/ui/b.ui.ron", r#"(root: Image("assets/gone.png"))"#),
    ]);
    let issues = check_files(dir.path(), &ui_files_under(&dir.path().join("assets")));
    let shown: Vec<String> = issues.iter().map(ToString::to_string).collect();
    assert_eq!(
      shown[0],
      "assets/ui/a.ui.ron:2:3: Column has no option \"gpa\"; did you mean \"gap\"?"
    );
    assert!(
      shown[1].starts_with("assets/ui/b.ui.ron:1:14:")
        && shown[1].contains("no image at \"assets/gone.png\""),
      "{shown:?}"
    );
  }
}
