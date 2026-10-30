//! The loaders for Tiled's JSON formats.

use serde::de::DeserializeOwned;

use super::build::{build_map, build_tileset, check_map, check_tileset};
use crate::core::asset::{AssetError, AssetLoader, AssetPath, CheckContext, LoadContext};
use crate::core::tilemap::{TileMap, Tileset};

/// Loads Tiled JSON maps (`.tmj`) as [`TileMap`]s. Tilesets and images the
/// map names are dependencies, so the map is ready when they are
pub struct TiledMapLoader;

/// Loads Tiled JSON tilesets (`.tsj`) as [`Tileset`]s
pub struct TiledTilesetLoader;

/// The file's JSON as `T`; the error reads `line L, column C: what`
fn parse<T: DeserializeOwned>(path: &AssetPath, bytes: &[u8]) -> Result<T, AssetError> {
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

impl AssetLoader for TiledMapLoader {
  type Asset = TileMap;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["tmj"]
  }

  fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<TileMap, AssetError> {
    let raw = parse(&cx.path().clone(), &bytes)?;
    build_map(raw, cx)
  }

  fn check(&self, map: &TileMap, cx: &CheckContext<'_>) -> Result<(), AssetError> {
    check_map(map, cx)
  }
}

impl AssetLoader for TiledTilesetLoader {
  type Asset = Tileset;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["tsj"]
  }

  fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<Tileset, AssetError> {
    let raw = parse(&cx.path().clone(), &bytes)?;
    build_tileset(raw, cx)
  }

  fn check(&self, set: &Tileset, cx: &CheckContext<'_>) -> Result<(), AssetError> {
    check_tileset(set, cx)
  }
}
