//! Block Component

use ryo2d::prelude::*;

/// Marker component for blocks
#[derive(Component, Debug, Clone)]
pub struct Block {
  /// Hits remaining before destruction
  pub hits: u32,
  /// Base color for this block
  pub color: BlockColor,
  /// Point value when destroyed
  pub points: u32,
  /// Whether this block spawns an extra ball when destroyed
  pub is_multiball: bool,
  /// Whether this block grants an extra life when destroyed
  pub is_heart: bool,
}

impl Block {
  /// Create a new block
  pub fn new(color: BlockColor, hits: u32, points: u32) -> Self {
    Self {
      hits,
      color,
      points,
      is_multiball: false,
      is_heart: false,
    }
  }
}

/// Block colors with associated RGBA values
#[derive(Debug, Clone, Copy)]
pub enum BlockColor {
  Red,
  Orange,
  Yellow,
  Green,
  Blue,
  Purple,
  White,
  Silver,
}

impl BlockColor {
  /// Get the RGBA color values for this block color
  pub fn to_rgba(self) -> (u8, u8, u8, u8) {
    match self {
      BlockColor::Red => (0xff, 0x44, 0x44, 0xff),
      BlockColor::Orange => (0xff, 0x88, 0x00, 0xff),
      BlockColor::Yellow => (0xff, 0xff, 0x00, 0xff),
      BlockColor::Green => (0x00, 0xcc, 0x44, 0xff),
      BlockColor::Blue => (0x44, 0x88, 0xff, 0xff),
      BlockColor::Purple => (0xaa, 0x44, 0xff, 0xff),
      BlockColor::White => (0xff, 0xff, 0xff, 0xff),
      BlockColor::Silver => (0xaa, 0xaa, 0xbb, 0xff),
    }
  }

  /// Get a dimmed version of the color (for damaged blocks)
  pub fn dimmed(self) -> (u8, u8, u8, u8) {
    let (r, g, b, a) = self.to_rgba();
    (
      (r as u16 * 2 / 3) as u8,
      (g as u16 * 2 / 3) as u8,
      (b as u16 * 2 / 3) as u8,
      a,
    )
  }

  /// The colour a level file names, as the enum `BlockColor` of the Tiled
  /// project spells it (the variant's name)
  pub fn from_name(name: &str) -> Option<BlockColor> {
    match name {
      "Red" => Some(BlockColor::Red),
      "Orange" => Some(BlockColor::Orange),
      "Yellow" => Some(BlockColor::Yellow),
      "Green" => Some(BlockColor::Green),
      "Blue" => Some(BlockColor::Blue),
      "Purple" => Some(BlockColor::Purple),
      "White" => Some(BlockColor::White),
      "Silver" => Some(BlockColor::Silver),
      _ => None,
    }
  }
}
