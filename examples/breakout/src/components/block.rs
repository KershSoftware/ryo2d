//! Block Component

/// Marker component for blocks
#[derive(Debug, Clone)]
pub struct Block {
  /// Hits remaining before destruction
  pub hits: u32,
  /// Base color for this block
  pub color: BlockColor,
  /// Point value when destroyed
  pub points: u32,
}

impl Block {
  /// Create a new block
  pub fn new(color: BlockColor, hits: u32, points: u32) -> Self {
    Self {
      hits,
      color,
      points,
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

  /// Parse a block character from a level file
  ///
  /// Returns (color, hits, points) if the character is a valid block
  pub fn from_char(c: char) -> Option<(BlockColor, u32, u32)> {
    match c {
      'R' => Some((BlockColor::Red, 1, 10)),
      'O' => Some((BlockColor::Orange, 1, 20)),
      'Y' => Some((BlockColor::Yellow, 1, 30)),
      'G' => Some((BlockColor::Green, 1, 40)),
      'B' => Some((BlockColor::Blue, 1, 50)),
      'P' => Some((BlockColor::Purple, 1, 60)),
      'W' => Some((BlockColor::White, 2, 100)),
      'S' => Some((BlockColor::Silver, 3, 200)),
      _ => None,
    }
  }
}
