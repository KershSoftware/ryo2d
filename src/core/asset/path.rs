use std::borrow::{Borrow, Cow};
use std::fmt;
use std::sync::Arc;

use super::error::AssetError;

/// An asset's path: relative to the asset root, `/`-separated, normalised
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetPath(Arc<str>);

impl AssetPath {
  /// Normalises `path`: `\` to `/`, drops `.` and empty segments, resolves
  /// `..` within the path; an absolute path or one leaving the root is an error
  pub fn parse(path: &str) -> Result<AssetPath, AssetError> {
    normalise(path)
      .map(AssetPath::from_normal)
      .map_err(|reason| AssetError::InvalidPath {
        path: path.to_owned(),
        reason,
      })
  }

  /// A path [`normalise`] made
  pub(crate) fn from_normal(normal: Cow<'_, str>) -> AssetPath {
    AssetPath(Arc::from(normal))
  }

  /// The path as text
  pub fn as_str(&self) -> &str {
    &self.0
  }

  /// The extensions to try, longest first: "menu.ui.ron" gives ["ui.ron", "ron"]
  pub fn extensions(&self) -> impl Iterator<Item = &str> {
    extensions(&self.0)
  }

  /// The folder of this path, for resolving nothing (paths in files are asset paths)
  pub fn parent(&self) -> Option<&str> {
    let end = self.0.rfind('/')?;
    self.0.get(..end)
  }
}

impl Borrow<str> for AssetPath {
  fn borrow(&self) -> &str {
    &self.0
  }
}

impl fmt::Display for AssetPath {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.0)
  }
}

impl fmt::Debug for AssetPath {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Debug::fmt(&*self.0, f)
  }
}

/// `path` normalised as [`AssetPath::parse`] does; borrowed when it is
/// already normal, so looking up a loaded path allocates nothing. An error
/// is the reason the path is not an asset path
pub(crate) fn normalise(path: &str) -> Result<Cow<'_, str>, &'static str> {
  let drive = matches!(path.as_bytes(), [letter, b':', ..] if letter.is_ascii_alphabetic());
  if path.starts_with(['/', '\\']) || drive {
    return Err("it is absolute");
  }
  if !path.is_empty()
    && !path.contains('\\')
    && path.split('/').all(|s| !matches!(s, "" | "." | ".."))
  {
    return Ok(Cow::Borrowed(path));
  }
  let mut segments: Vec<&str> = Vec::new();
  for segment in path.split(['/', '\\']) {
    match segment {
      "" | "." => {}
      ".." => {
        if segments.pop().is_none() {
          return Err("it leaves the asset root");
        }
      }
      segment => segments.push(segment),
    }
  }
  if segments.is_empty() {
    return Err("it names no file");
  }
  Ok(Cow::Owned(segments.join("/")))
}

/// The extensions of a normalised path's file name, longest first. A dot
/// that starts the name (a hidden file) does not start an extension
pub(crate) fn extensions(path: &str) -> impl Iterator<Item = &str> {
  let name = path.rsplit('/').next().unwrap_or(path);
  name
    .char_indices()
    .filter(|&(at, c)| c == '.' && at > 0)
    .filter_map(move |(at, _)| name.get(at + 1..))
    .filter(|extension| !extension.is_empty())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn parsed(path: &str) -> Result<String, String> {
    AssetPath::parse(path)
      .map(|p| p.as_str().to_owned())
      .map_err(|e| e.to_string())
  }

  #[test]
  fn paths_are_normalised() {
    assert_eq!(parsed("a//b/./c.png"), Ok("a/b/c.png".into()));
    assert_eq!(parsed("a\\b.png"), Ok("a/b.png".into()));
    assert_eq!(parsed("a/../b.png"), Ok("b.png".into()));
    assert_eq!(parsed("./ui/menu.ui.ron"), Ok("ui/menu.ui.ron".into()));
    assert_eq!(parsed("a/b/"), Ok("a/b".into()));
  }

  #[test]
  fn a_path_leaving_the_root_or_absolute_is_invalid() {
    for path in [
      "../b.png",
      "a/../../b.png",
      "/b.png",
      "\\b.png",
      "C:/b.png",
      "",
      "./",
    ] {
      assert!(
        matches!(AssetPath::parse(path), Err(AssetError::InvalidPath { .. })),
        "{path:?} should be invalid"
      );
    }
  }

  #[test]
  fn a_normal_path_is_borrowed_as_it_is() {
    assert!(matches!(normalise("ui/menu.ui.ron"), Ok(Cow::Borrowed(_))));
    assert!(matches!(normalise("ui//menu.ui.ron"), Ok(Cow::Owned(_))));
  }

  #[test]
  fn extensions_come_longest_first() {
    let path = AssetPath::parse("x/menu.ui.ron").map_err(|e| e.to_string());
    let extensions: Vec<&str> = path.iter().flat_map(AssetPath::extensions).collect();
    assert_eq!(extensions, ["ui.ron", "ron"]);
    let hidden = AssetPath::parse(".hidden.txt").map_err(|e| e.to_string());
    let extensions: Vec<&str> = hidden.iter().flat_map(AssetPath::extensions).collect();
    assert_eq!(extensions, ["txt"]);
    let bare = AssetPath::parse("x.d/README").map_err(|e| e.to_string());
    assert_eq!(bare.iter().flat_map(AssetPath::extensions).count(), 0);
  }

  #[test]
  fn parent_is_the_folder() {
    let path = AssetPath::parse("ui/menus/main.ui.ron").map_err(|e| e.to_string());
    assert_eq!(
      path.as_ref().ok().and_then(AssetPath::parent),
      Some("ui/menus")
    );
    let top = AssetPath::parse("a.png").map_err(|e| e.to_string());
    assert_eq!(top.as_ref().ok().and_then(AssetPath::parent), None);
  }
}
