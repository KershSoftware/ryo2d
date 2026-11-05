//! Tiles: a cell's content and its flips.

use std::ops::BitOr;

use glam::{Mat2, Vec2};

/// The sines and cosines of a hexagonal tile's turns: none, 60, 120 and 180
/// degrees clockwise on screen (y down)
const HEX_TURNS: [(f32, f32); 4] = [
  (0.0, 1.0),
  (0.866_025_4, 0.5),
  (0.866_025_4, -0.5),
  (0.0, -1.0),
];

/// How a tile is mirrored or turned, as Tiled stores it: a diagonal flip
/// (swap x and y) applied first, then horizontal, then vertical. A 90 degree
/// turn is a diagonal and a horizontal flip.
///
/// On a hexagonal map the diagonal flip's bit is a 60 degree turn
/// clockwise instead ([`HEX_ROTATE_60`](Self::HEX_ROTATE_60)), and a fourth
/// bit a 120 degree one ([`HEX_ROTATE_120`](Self::HEX_ROTATE_120)): the
/// flips apply first, then the turns (both bits, a half turn). Tiled
/// ignores the 120 degree bit on every other grid, and a map loaded from
/// Tiled drops it there.
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
  /// On a hexagonal map, turned 60 degrees clockwise after any flip, about
  /// the tile's own middle where it stands unturned (its tileset's offset
  /// included), as Tiled turns it: the diagonal flip's bit, which Tiled
  /// gives this meaning there
  pub const HEX_ROTATE_60: TileFlip = TileFlip(4);
  /// On a hexagonal map, turned 120 degrees clockwise after any flip, about
  /// the tile's own middle where it stands unturned (its tileset's offset
  /// included), as Tiled turns it
  pub const HEX_ROTATE_120: TileFlip = TileFlip(8);

  /// Whether every flip in `other` is set here
  pub fn contains(self, other: TileFlip) -> bool {
    self.0 & other.0 == other.0
  }

  /// The flips of this tile turned 90 degrees clockwise, on a square grid
  /// (a hexagonal 120 degree turn is dropped: turn a hexagonal tile with
  /// [`rotated_hex_cw`](Self::rotated_hex_cw))
  pub fn rotated_cw(self) -> TileFlip {
    // Tiled's rotate-right table, re-indexed for H = 1, V = 2, D = 4
    const NEXT: [u8; 8] = [5, 7, 4, 6, 1, 3, 0, 2];
    TileFlip(NEXT[(self.0 & 7) as usize])
  }

  /// The flips of a hexagonal map's tile turned 60 degrees clockwise. A
  /// half turn is written as both flips, as on a square grid, so six turns
  /// give back the flips they started from, except from flips holding both
  /// turn bits (the same half turn written otherwise), which come back as
  /// the same turn with both flips in their place
  pub fn rotated_hex_cw(self) -> TileFlip {
    let (r60, r120) = (Self::HEX_ROTATE_60.0, Self::HEX_ROTATE_120.0);
    let both_flips = Self::HORIZONTAL.0 | Self::VERTICAL.0;
    let flips = self.0 & both_flips;
    TileFlip(match self.0 & (r60 | r120) {
      0 => flips | r60,
      t if t == r60 => flips | r120,
      // 120 and 60 make a half turn: both flips toggled
      t if t == r120 => flips ^ both_flips,
      // a half turn and 60
      _ => (flips ^ both_flips) | r60,
    })
  }

  /// The four flip bits
  pub(crate) fn bits(self) -> u8 {
    self.0
  }

  /// A flip from its four bits; the rest are dropped
  pub(crate) fn from_bits(bits: u8) -> TileFlip {
    TileFlip(bits & 15)
  }

  /// These flips without those of `other`
  pub(crate) fn without(self, other: TileFlip) -> TileFlip {
    TileFlip(self.0 & !other.0)
  }

  /// The turn these flips make of a tile's quad, as the matrix taking the
  /// tile's own axes (its width across, its height down) to the quad's: on
  /// a hexagonal grid (`hex`) the 60 and 120 degree turns clockwise (both,
  /// a half turn); elsewhere the diagonal flip, which swaps them. Drawing
  /// turns a tile's quad by it, and collision its shapes
  pub(crate) fn quad_turn(self, hex: bool) -> Mat2 {
    if hex {
      let turns = usize::from(self.contains(TileFlip::HEX_ROTATE_60))
        + 2 * usize::from(self.contains(TileFlip::HEX_ROTATE_120));
      let (sin, cos) = HEX_TURNS[turns];
      // (1, 0) turns to (cos, sin): clockwise, y being down
      return Mat2::from_cols(Vec2::new(cos, sin), Vec2::new(-sin, cos));
    }
    if self.contains(TileFlip::DIAGONAL) {
      Mat2::from_cols(Vec2::Y, Vec2::X)
    } else {
      Mat2::IDENTITY
    }
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

  /// What a hexagonal tile's flips do, as a turn clockwise in degrees after
  /// a mirror or none: the flips first, then the turns (Tiled's order). A
  /// vertical flip is a horizontal one turned half round
  fn hex_transform(flip: TileFlip) -> (u32, bool) {
    let has = |f: TileFlip| flip.contains(f);
    let mut turn = 0;
    if has(TileFlip::HEX_ROTATE_60) {
      turn += 60;
    }
    if has(TileFlip::HEX_ROTATE_120) {
      turn += 120;
    }
    if has(TileFlip::VERTICAL) {
      turn += 180;
    }
    let mirrored = has(TileFlip::HORIZONTAL) != has(TileFlip::VERTICAL);
    (turn % 360, mirrored)
  }

  #[test]
  fn four_flip_bits_round_trip_through_a_cell() {
    use crate::core::tilemap::TileGrid;
    use glam::{IVec2, UVec2};
    assert_eq!(TileFlip::HEX_ROTATE_60, TileFlip::DIAGONAL);
    assert_eq!(TileFlip::HEX_ROTATE_120.bits(), 8);
    let mut grid = TileGrid::new(UVec2::new(16, 1));
    for bits in 0..16u8 {
      let flip = TileFlip::from_bits(bits);
      assert_eq!(flip.bits(), bits);
      let tile = Tile::new(3, 77).with_flip(flip);
      let cell = TileGrid::encode(Some(tile));
      assert_eq!(cell.and_then(TileGrid::decode), Some(tile), "state {bits}");
      assert!(grid.set(IVec2::new(i32::from(bits), 0), Some(tile)));
    }
    for bits in 0..16u8 {
      assert_eq!(
        grid
          .get(IVec2::new(i32::from(bits), 0))
          .map(|t| t.flip.bits()),
        Some(bits)
      );
    }
    // the bits past the four are dropped
    assert_eq!(TileFlip::from_bits(0xF5).bits(), 5);
  }

  #[test]
  fn hex_turns_come_round_after_six() {
    let (h, v) = (TileFlip::HORIZONTAL, TileFlip::VERTICAL);
    let (r60, r120) = (TileFlip::HEX_ROTATE_60, TileFlip::HEX_ROTATE_120);
    let n = TileFlip::NONE;
    // A half turn is both flips, as on a square map
    assert_eq!(n.rotated_hex_cw(), r60);
    assert_eq!(r60.rotated_hex_cw(), r120);
    assert_eq!(r120.rotated_hex_cw(), h | v);
    assert_eq!((h | v).rotated_hex_cw(), h | v | r60);
    assert_eq!((h | v | r60).rotated_hex_cw(), h | v | r120);
    assert_eq!((h | v | r120).rotated_hex_cw(), n);
    for bits in 0..16u8 {
      let start = TileFlip::from_bits(bits);
      let mut f = start;
      for turn in 1..=6 {
        let (was, mirrored) = hex_transform(f);
        f = f.rotated_hex_cw();
        assert_eq!(
          hex_transform(f),
          ((was + 60) % 360, mirrored),
          "state {bits}, turn {turn}"
        );
      }
      // Both turn bits are a half turn written otherwise: every other state
      // comes back as it was, those as the same turn
      if start.contains(r60 | r120) {
        assert_eq!(hex_transform(f), hex_transform(start), "state {bits}");
      } else {
        assert_eq!(f, start, "state {bits}");
      }
    }
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
