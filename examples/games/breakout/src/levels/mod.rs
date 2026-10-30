//! Level Loading
//!
//! Each level is a Tiled map under `assets/levels/`, loaded through the asset
//! registry. Its tile layer holds the bricks: tiles of the class `brick`
//! (`bricks.tsx`, one tile a colour) with the properties `color` (the enum
//! `BlockColor`, typed by `project.tiled-project`), `hits` and `points`. The
//! map's cells are the game's brick grid, so a tile's cell centre is a brick's
//! centre. The game spawns the map with [`BRICK_CLASS`] as an entity tile and
//! turns each [`MapTile`] into a block.

use crate::components::BlockColor;
use crate::constants::{
  BLOCK_HEIGHT, BLOCK_LEFT_OFFSET, BLOCK_PADDING, BLOCK_TOP_OFFSET, BLOCK_WIDTH,
};
use ryo2d::prelude::*;

/// The class of a brick tile
pub const BRICK_CLASS: &str = "brick";

/// Where a level's map sits (its top-left corner): the corner of the first
/// brick's cell
pub const LEVEL_ORIGIN: (f32, f32) = (
  BLOCK_LEFT_OFFSET - (BLOCK_WIDTH + BLOCK_PADDING) / 2.0,
  BLOCK_TOP_OFFSET - (BLOCK_HEIGHT + BLOCK_PADDING) / 2.0,
);

/// What a brick tile says: colour, hits and points
pub fn read_brick(tile: &MapTile) -> Result<(BlockColor, u32, u32), String> {
  // The project's enum `BlockColor` reads as a named value of that enum
  let Some(PropertyValue::Enum { value, .. }) = tile.properties.get("color") else {
    return Err("no `color` property of the enum BlockColor".to_string());
  };
  let color = BlockColor::from_name(value).ok_or_else(|| format!("no colour named {value:?}"))?;
  let count = |name: &str| {
    tile
      .properties
      .int(name)
      .and_then(|n| u32::try_from(n).ok())
      .ok_or_else(|| format!("no whole `{name}` property"))
  };
  Ok((color, count("hits")?, count("points")?))
}

/// The asset path of a numbered level's map
pub fn level_path(level: u32) -> String {
  format!("levels/level{}.tmx", level)
}
