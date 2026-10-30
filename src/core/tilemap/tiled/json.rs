//! The loaders for Tiled's maps and tilesets: JSON files by `serde_json`,
//! XML ones by the reader in [`xml`](super::xml).

use serde::de::DeserializeOwned;

use super::build::{build_map, build_tileset, check_map, check_tileset};
use super::project::{Props, TiledProject};
use super::raw::{RawMap, RawTileset};
use super::xml::{parse_tmx, parse_tsx};
use crate::core::asset::{AssetError, AssetLoader, AssetPath, CheckContext, LoadContext};
use crate::core::tilemap::{TileMap, Tileset};

/// Loads Tiled maps (`.tmj` JSON, `.tmx` XML) as [`TileMap`]s. Tilesets and images the
/// map names are dependencies, so the map is ready when they are
#[derive(Default)]
pub struct TiledMapLoader {
  project: Option<String>,
}

/// Loads Tiled tilesets (`.tsj` JSON, `.tsx` XML) as [`Tileset`]s
#[derive(Default)]
pub struct TiledTilesetLoader {
  project: Option<String>,
}

impl TiledMapLoader {
  /// A loader reading class and enum types from the Tiled project at
  /// `path` (an asset path) rather than looking for `project.tiled-project`
  pub fn with_project(path: impl Into<String>) -> TiledMapLoader {
    TiledMapLoader {
      project: Some(path.into()),
    }
  }
}

impl TiledTilesetLoader {
  /// A loader reading class and enum types from the Tiled project at
  /// `path` (an asset path) rather than looking for `project.tiled-project`
  pub fn with_project(path: impl Into<String>) -> TiledTilesetLoader {
    TiledTilesetLoader {
      project: Some(path.into()),
    }
  }
}

/// The file's JSON as `T`; the error reads `line L, column C: what`
pub(crate) fn parse<T: DeserializeOwned>(path: &AssetPath, bytes: &[u8]) -> Result<T, AssetError> {
  let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
  serde_json::from_slice(bytes).map_err(|e| {
    let text = e.to_string();
    let what = text
      .rsplit_once(" at line ")
      .map_or(text.as_str(), |(what, _)| what);
    AssetError::decode(
      path,
      format!("line {}, column {}: {what}", e.line(), e.column()),
    )
  })
}

/// Whether the file at `path` is Tiled XML, going by its extension
fn is_xml(path: &AssetPath, ext: &str) -> bool {
  path.as_str().to_ascii_lowercase().ends_with(ext)
}

/// The XML file's text read by `read`; the error reads `line L, column C: what`
fn from_xml<T>(
  path: &AssetPath,
  bytes: &[u8],
  read: fn(&str) -> Result<T, String>,
) -> Result<T, AssetError> {
  let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
  let text = std::str::from_utf8(bytes)
    .map_err(|e| AssetError::decode(path, format!("the file is not UTF-8: {e}")))?;
  read(text).map_err(|e| AssetError::decode(path, e))
}

impl AssetLoader for TiledMapLoader {
  type Asset = TileMap;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["tmj", "tmx"]
  }

  fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<TileMap, AssetError> {
    let path = cx.path().clone();
    let raw: RawMap = if is_xml(&path, ".tmx") {
      from_xml(&path, &bytes, parse_tmx)?
    } else {
      parse(&path, &bytes)?
    };
    let project = TiledProject::for_loader(cx, self.project.as_deref())?;
    build_map(raw, cx, &mut Props::new(path, project.as_ref()))
  }

  fn check(&self, map: &TileMap, cx: &CheckContext<'_>) -> Result<(), AssetError> {
    check_map(map, cx)
  }
}

impl AssetLoader for TiledTilesetLoader {
  type Asset = Tileset;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["tsj", "tsx"]
  }

  fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<Tileset, AssetError> {
    let path = cx.path().clone();
    let raw: RawTileset = if is_xml(&path, ".tsx") {
      from_xml(&path, &bytes, parse_tsx)?
    } else {
      parse(&path, &bytes)?
    };
    let project = TiledProject::for_loader(cx, self.project.as_deref())?;
    build_tileset(raw, cx, &mut Props::new(path, project.as_ref()))
  }

  fn check(&self, set: &Tileset, cx: &CheckContext<'_>) -> Result<(), AssetError> {
    check_tileset(set, cx)
  }
}
