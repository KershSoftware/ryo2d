//! Tiled's world files (`.world`): maps with positions, listed or named by
//! a numbered pattern, read into a [`TileWorld`].

use std::collections::HashSet;

use glam::{IVec2, UVec2, Vec2};
use serde::Deserialize;

use super::build::{DependencyPlaces, grid_shape, infinite_bounds, join, load_dep};
use super::json::parse;
use super::raw::RawMap;
use super::xml::parse_tmx;
use crate::core::asset::{AssetError, AssetLoader, AssetPath, LoadContext};
use crate::core::physics::CellRect;
use crate::core::tilemap::world::{TileWorld, layout_of_rects};
use crate::core::tilemap::{MapGrid, TileMap, WorldLevel};

/// Most files a pattern's search tries before it stops
const MAX_PROBES: usize = 1024;

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawWorld {
  maps: Vec<RawWorldMap>,
  patterns: Vec<RawPattern>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawWorldMap {
  file_name: String,
  x: i32,
  y: i32,
  width: Option<u32>,
  height: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawPattern {
  regexp: String,
  multiplier_x: i32,
  multiplier_y: i32,
  offset_x: i32,
  offset_y: i32,
}

impl Default for RawPattern {
  fn default() -> Self {
    RawPattern {
      regexp: String::new(),
      multiplier_x: 1,
      multiplier_y: 1,
      offset_x: 0,
      offset_y: 0,
    }
  }
}

/// A piece of a supported pattern
#[derive(Debug, PartialEq)]
pub(crate) enum Part {
  Text(String),
  /// A number: `(\d+)`, or `(-?\d+)`
  Number,
}

/// A pattern's text and number groups, or why it is not supported
pub(crate) fn parse_pattern(regexp: &str) -> Result<Vec<Part>, String> {
  let unsupported = |what: &str| format!("pattern \"{regexp}\" is not supported: {what}");
  let body = regexp.strip_prefix('^').unwrap_or(regexp);
  // only an unescaped `$` anchors; `\$` is a dollar sign
  let body = match body.strip_suffix('$') {
    Some(rest) if (rest.len() - rest.trim_end_matches('\\').len()) % 2 == 0 => rest,
    _ => body,
  };
  let mut parts: Vec<Part> = Vec::new();
  let mut text = String::new();
  let mut numbers = 0;
  let mut rest = body;
  while let Some(c) = rest.chars().next() {
    if let Some(after) = ["(\\d+)", "(-?\\d+)", "(\\-?\\d+)"]
      .iter()
      .find_map(|group| rest.strip_prefix(group))
    {
      numbers += 1;
      if numbers > 2 {
        return Err(unsupported("more than two numbers"));
      }
      if !text.is_empty() {
        parts.push(Part::Text(std::mem::take(&mut text)));
      }
      parts.push(Part::Number);
      rest = after;
      continue;
    }
    let mut chars = rest.chars();
    chars.next();
    match c {
      '\\' => match chars.next() {
        Some(escaped) if !escaped.is_ascii_alphanumeric() => text.push(escaped),
        _ => return Err(unsupported("an escape that is not a punctuation mark")),
      },
      '(' | ')' | '[' | ']' | '{' | '}' | '|' | '*' | '+' | '?' | '^' | '$' => {
        return Err(unsupported(&format!(
          "`{c}` (only literal text and up to two `(\\d+)` numbers are)"
        )));
      }
      _ => text.push(c),
    }
    rest = chars.as_str();
  }
  if !text.is_empty() {
    parts.push(Part::Text(text));
  }
  Ok(parts)
}

/// The file name with `numbers` put in the pattern's groups, in order
fn name_with(parts: &[Part], numbers: &[i32]) -> String {
  let mut numbers = numbers.iter();
  let mut name = String::new();
  for part in parts {
    match part {
      Part::Text(text) => name.push_str(text),
      Part::Number => name.push_str(&numbers.next().copied().unwrap_or(0).to_string()),
    }
  }
  name
}

/// A map's rectangle in pixels from its file's header, as where its
/// top-left lies from its pixel (0, 0) (where its root spawns, the world's
/// x and y) and its size: its cells' box on its grid
/// ([`MapGrid::bounds_of`], the grid built from the header as the map's
/// load builds it). An infinite map's cells are its tiles' bounds
/// ([`infinite_bounds`], what [`TileMap::bounds`] gives the map; its
/// origin cell when it holds no tile, so it can still spawn), whatever
/// size the world lists it with; a bounded map's are its width by its
/// height, and it takes the size it is `listed` with when there is one
fn map_rect(
  path: &AssetPath,
  bytes: &[u8],
  listed: Option<Vec2>,
) -> Result<(Vec2, Vec2), AssetError> {
  let raw: RawMap = if path.as_str().to_ascii_lowercase().ends_with(".tmx") {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes)
      .map_err(|e| AssetError::decode(path, format!("the file is not UTF-8: {e}")))?;
    parse_tmx(text).map_err(|e| AssetError::decode(path, e))?
  } else {
    parse(path, bytes)?
  };
  let shape = grid_shape(path, &raw)?;
  let tile = UVec2::new(raw.tilewidth, raw.tileheight);
  // An infinite map's width and height mean nothing (Tiled writes 0)
  let size = if raw.infinite {
    UVec2::ZERO
  } else {
    UVec2::new(raw.width, raw.height)
  };
  let grid = MapGrid::for_map(shape, tile, size, raw.infinite);
  let cells = if raw.infinite {
    let bounds = infinite_bounds(path, &raw, &grid)?;
    if bounds.is_empty() {
      CellRect::new(IVec2::ZERO, IVec2::ONE)
    } else {
      bounds
    }
  } else {
    CellRect::from_size(size)
  };
  let rect = grid.bounds_of(cells);
  let boxed = Vec2::new(rect.width, rect.height);
  let size = if raw.infinite {
    boxed
  } else {
    listed.unwrap_or(boxed)
  };
  Ok((Vec2::new(rect.x, rect.y), size))
}

/// Loads Tiled world files (`.world`) as [`TileWorld`]s: the maps they list
/// or number are dependencies, so the world is ready when they are
#[derive(Default)]
pub struct TileWorldLoader;

impl AssetLoader for TileWorldLoader {
  type Asset = TileWorld;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["world"]
  }

  fn load(
    &self,
    bytes: Vec<u8>,
    _: &(),
    cx: &mut LoadContext<'_>,
  ) -> Result<TileWorld, AssetError> {
    let path = cx.path().clone();
    let raw: RawWorld = parse(&path, &bytes)?;
    // Each level's name, tile origin, rectangle's top-left from it, and size
    let mut levels: Vec<(String, Vec2, Vec2, Vec2)> = Vec::new();
    let mut maps = Vec::new();
    let mut taken: HashSet<String> = HashSet::new();
    // A map's place is in its own error, which the server's names
    let mut places = DependencyPlaces::default();

    for (i, entry) in raw.maps.iter().enumerate() {
      let place = format!("map {i} ({:?})", entry.file_name);
      let joined = join(&path, &place, &entry.file_name)?;
      let handle = load_dep::<TileMap>(cx, &path, &place, &entry.file_name, &mut places)?;
      // Read even when listed with a size: an infinite map's rectangle is
      // its tiles', and a save of it moves them
      let listed = entry
        .width
        .zip(entry.height)
        .map(|(w, h)| Vec2::new(w as f32, h as f32));
      let file = AssetPath::parse(&joined)
        .map_err(|e| AssetError::decode(&path, format!("{place}: {}", e.report())))?;
      let bytes = cx
        .read(&file)
        .map_err(|e| AssetError::decode(&path, format!("{place}: {}", e.report())))?;
      let (offset, size) = map_rect(&file, &bytes, listed)
        .map_err(|e| AssetError::decode(&path, format!("{place}: {}", e.report())))?;
      taken.insert(joined);
      levels.push((
        entry.file_name.clone(),
        Vec2::new(entry.x as f32, entry.y as f32),
        offset,
        size,
      ));
      maps.push(handle);
    }

    for pattern in &raw.patterns {
      let place = format!("pattern \"{}\"", pattern.regexp);
      let parts = parse_pattern(&pattern.regexp).map_err(|e| AssetError::decode(&path, e))?;
      let numbers = parts.iter().filter(|p| **p == Part::Number).count();
      let mut probes = 0;
      let mut found = 0;
      // Rows from y = 0, each from x = 0 to the first missing file; the
      // first row without an x = 0 file ends the search
      'rows: for y in 0.. {
        for x in 0.. {
          if probes >= MAX_PROBES {
            break 'rows;
          }
          probes += 1;
          let counters = match numbers {
            0 => &[][..],
            1 => &[x][..],
            _ => &[x, y][..],
          };
          let written = name_with(&parts, counters);
          let joined = join(&path, &place, &written)?;
          let file = AssetPath::parse(&joined)
            .map_err(|e| AssetError::decode(&path, format!("{place}: {}", e.report())))?;
          let bytes = match cx.read(&file) {
            Ok(bytes) => bytes,
            Err(AssetError::NotFound { .. }) if x == 0 => break 'rows,
            Err(AssetError::NotFound { .. }) => break,
            Err(error) => {
              return Err(AssetError::decode(
                &path,
                format!("{place}: {}", error.report()),
              ));
            }
          };
          found += 1;
          if taken.insert(joined) {
            let (offset, size) = map_rect(&file, &bytes, None)
              .map_err(|e| AssetError::decode(&path, format!("{place}: {}", e.report())))?;
            let handle = load_dep::<TileMap>(cx, &path, &place, &written, &mut places)?;
            let place_on = |n: i32, multiplier: i32, offset: i32| {
              n.checked_mul(multiplier)
                .and_then(|n| n.checked_add(offset))
                .map(|n| n as f32)
                .ok_or_else(|| {
                  AssetError::decode(
                    &path,
                    format!("{place}: the position of {written:?} does not fit (multiplier or offset too large)"),
                  )
                })
            };
            let position = Vec2::new(
              place_on(x, pattern.multiplier_x, pattern.offset_x)?,
              place_on(y, pattern.multiplier_y, pattern.offset_y)?,
            );
            levels.push((written, position, offset, size));
            maps.push(handle);
          }
          if numbers == 0 {
            break 'rows;
          }
        }
        if numbers < 2 {
          break;
        }
      }
      if found == 0 {
        return Err(AssetError::decode(
          &path,
          format!("{place}: no map matches it (it names files by number from 0, 0)"),
        ));
      }
    }

    let levels: Vec<WorldLevel> = levels
      .into_iter()
      .map(|(name, origin, offset, size)| WorldLevel {
        name,
        position: origin + offset,
        size,
        origin,
        depth: 0,
        neighbours: Vec::new(),
      })
      .collect();
    Ok(TileWorld::new(layout_of_rects(path.as_str(), levels), maps))
  }
}
