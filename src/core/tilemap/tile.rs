//! Tiles: a cell's content and its flips.

use std::ops::BitOr;

/// How a tile is mirrored or turned, as Tiled stores it: a diagonal flip
/// (swap x and y) applied first, then horizontal, then vertical. A 90 degree
/// turn is a diagonal and a horizontal flip.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct TileFlip(u8);

impl TileFlip {
  /// Drawn as in the tileset
  pub const NONE: TileFlip = TileFlip(0);
  /// Mirrored left to right
  pub const HORIZONTAL: TileFlip = TileFlip(1);
  /// Mirrored top to bottom
  pub const VERTICAL: TileFlip = TileFlip(2);
  /// Mirrored across the top-left to bottom-right diagonal
  pub const DIAGONAL: TileFlip = TileFlip(4);

  /// Whether every flip in `other` is set here
  pub fn contains(self, other: TileFlip) -> bool {
    self.0 & other.0 == other.0
  }

  /// The flips of this tile turned 90 degrees clockwise
  pub fn rotated_cw(self) -> TileFlip {
    // Tiled's rotate-right table, re-indexed for H = 1, V = 2, D = 4
    const NEXT: [u8; 8] = [5, 7, 4, 6, 1, 3, 0, 2];
    TileFlip(NEXT[(self.0 & 7) as usize])
  }

  /// The three flip bits
  pub(crate) fn bits(self) -> u8 {
    self.0
  }

  /// A flip from its three bits; the rest are dropped
  pub(crate) fn from_bits(bits: u8) -> TileFlip {
    TileFlip(bits & 7)
  }
}

impl BitOr for TileFlip {
  type Output = TileFlip;

  fn bitor(self, rhs: TileFlip) -> TileFlip {
    TileFlip(self.0 | rhs.0)
  }
}

/// What a cell holds: a tile of one of the map's tilesets, flipped or not
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Tile {
  /// Index into the map's tilesets
  pub tileset: u16,
  /// The tile's id within its tileset
  pub id: u32,
  /// Mirroring and turning
  pub flip: TileFlip,
}

impl Tile {
  /// Tile `id` of tileset `tileset`, unflipped
  pub fn new(tileset: u16, id: u32) -> Tile {
    Tile {
      tileset,
      id,
      flip: TileFlip::NONE,
    }
  }

  /// This tile with `flip`
  pub fn with_flip(self, flip: TileFlip) -> Tile {
    Tile { flip, ..self }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn flips_compose_as_tiled_does() {
    assert_eq!(
      TileFlip::DIAGONAL | TileFlip::HORIZONTAL,
      TileFlip::NONE.rotated_cw()
    );
    let mut f = TileFlip::NONE;
    for _ in 0..4 {
      f = f.rotated_cw();
    }
    assert_eq!(f, TileFlip::NONE);
    assert!(f.rotated_cw().contains(TileFlip::DIAGONAL));
  }

  #[test]
  fn rotating_cycles_through_tileds_states() {
    let (h, v, d) = (TileFlip::HORIZONTAL, TileFlip::VERTICAL, TileFlip::DIAGONAL);
    let n = TileFlip::NONE;
    assert_eq!(n.rotated_cw(), d | h);
    assert_eq!((d | h).rotated_cw(), h | v);
    assert_eq!((h | v).rotated_cw(), v | d);
    assert_eq!((v | d).rotated_cw(), n);
    assert_eq!(h.rotated_cw(), h | v | d);
  }

  #[test]
  fn every_flip_state_returns_after_four_turns() {
    for bits in 0..8u8 {
      let start = TileFlip::from_bits(bits);
      let mut f = start;
      let mut seen = Vec::new();
      for _ in 0..4 {
        seen.push(f);
        f = f.rotated_cw();
      }
      assert_eq!(f, start, "state {bits}");
      // an unmirrored or mirrored orbit has four distinct states
      seen.sort_by_key(|f| f.bits());
      seen.dedup();
      assert_eq!(seen.len(), 4, "state {bits}");
    }
  }
}
