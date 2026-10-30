//! The dense grid of tiles in a tile layer.

use glam::{IVec2, UVec2};

use super::tile::{Tile, TileFlip};

/// The side of a chunk in cells: the unit a layer redraws and rebuilds by
pub const CHUNK_SIZE: u32 = 32;

const ID_BITS: u32 = 20;
const SET_BITS: u32 = 8;
const ID_MASK: u32 = (1 << ID_BITS) - 1;
const SET_MASK: u32 = (1 << SET_BITS) - 1;

fn encode(tile: Option<Tile>) -> Option<u32> {
  let Some(t) = tile else { return Some(0) };
  if t.tileset > SET_MASK as u16 || t.id >= ID_MASK {
    return None;
  }
  Some(
    ((t.flip.bits() as u32) << (ID_BITS + SET_BITS)) | ((t.tileset as u32) << ID_BITS) | (t.id + 1),
  )
}

fn decode(cell: u32) -> Option<Tile> {
  let id1 = cell & ID_MASK;
  if id1 == 0 {
    return None;
  }
  Some(Tile {
    tileset: ((cell >> ID_BITS) & SET_MASK) as u16,
    id: id1 - 1,
    flip: TileFlip::from_bits((cell >> (ID_BITS + SET_BITS)) as u8),
  })
}

/// A dense grid of cells, 4 bytes each: 0 is empty, otherwise the flips sit
/// in bits 28 to 31, the tileset in 20 to 27 and the tile id plus one in 0
/// to 19. A map holds at most 256 tilesets of 1,048,575 tiles each; a tile
/// outside that is refused. Edits mark the chunk they touch dirty.
#[derive(Clone, Debug)]
pub struct TileGrid {
  size: UVec2,
  cells: Vec<u32>,
  chunks: UVec2,
  dirty: Vec<bool>,
  dirty_count: usize,
  /// Each chunk's version: the grid's counter when it last changed
  versions: Vec<u64>,
  /// Moves on at every change and never repeats, so a version is a stamp of
  /// one state of one chunk
  counter: u64,
}

impl TileGrid {
  /// An empty grid of `size` cells, no chunk dirty
  pub fn new(size: UVec2) -> TileGrid {
    // div_ceil cannot overflow the way adding to the size can
    let chunks = UVec2::new(size.x.div_ceil(CHUNK_SIZE), size.y.div_ceil(CHUNK_SIZE));
    let n = chunks.x as usize * chunks.y as usize;
    TileGrid {
      size,
      cells: vec![0; (size.x as usize) * (size.y as usize)],
      chunks,
      dirty: vec![false; n],
      dirty_count: 0,
      versions: vec![0; n],
      counter: 0,
    }
  }

  /// Width and height in cells
  pub fn size(&self) -> UVec2 {
    self.size
  }

  /// Width and height in chunks
  pub fn chunks(&self) -> UVec2 {
    self.chunks
  }

  fn index(&self, cell: IVec2) -> Option<usize> {
    if cell.x < 0 || cell.y < 0 || cell.x as u32 >= self.size.x || cell.y as u32 >= self.size.y {
      return None;
    }
    Some(cell.y as usize * self.size.x as usize + cell.x as usize)
  }

  fn mark(&mut self, cell: IVec2) {
    let c = cell.as_uvec2() / CHUNK_SIZE;
    let i = c.y as usize * self.chunks.x as usize + c.x as usize;
    self.counter += 1;
    self.versions[i] = self.counter;
    if !self.dirty[i] {
      self.dirty[i] = true;
      self.dirty_count += 1;
    }
  }

  /// The tile at `cell`; `None` if empty or outside
  pub fn get(&self, cell: IVec2) -> Option<Tile> {
    decode(self.cells[self.index(cell)?])
  }

  /// Puts `tile` (or clears) at `cell`. False if the cell is outside, the
  /// tile does not fit a cell, or nothing changed
  pub fn set(&mut self, cell: IVec2, tile: Option<Tile>) -> bool {
    let (Some(i), Some(v)) = (self.index(cell), encode(tile)) else {
      return false;
    };
    if self.cells[i] == v {
      return false;
    }
    self.cells[i] = v;
    self.mark(cell);
    true
  }

  /// Sets every cell from `min` to `max`, both inclusive and clipped to the
  /// grid; returns how many changed
  pub fn fill(&mut self, min: IVec2, max: IVec2, tile: Option<Tile>) -> usize {
    let Some(v) = encode(tile) else { return 0 };
    let lo = min.max(IVec2::ZERO);
    let hi = max.min(self.size.as_ivec2() - IVec2::ONE);
    let mut changed = 0;
    for y in lo.y..=hi.y {
      for x in lo.x..=hi.x {
        let cell = IVec2::new(x, y);
        let i = y as usize * self.size.x as usize + x as usize;
        if self.cells[i] != v {
          self.cells[i] = v;
          self.mark(cell);
          changed += 1;
        }
      }
    }
    changed
  }

  /// Empties every cell
  pub fn clear(&mut self) {
    if self.cells.iter().any(|&c| c != 0) {
      self.cells.fill(0);
      self.mark_all_dirty();
    }
  }

  /// Appends to `out` each chunk changed since the last drain, once, in row
  /// order, and clears the marks. `out` is cleared first
  pub fn drain_dirty(&mut self, out: &mut Vec<UVec2>) {
    out.clear();
    if self.dirty_count == 0 {
      return;
    }
    for (i, d) in self.dirty.iter_mut().enumerate() {
      if *d {
        *d = false;
        out.push(UVec2::new(
          (i % self.chunks.x as usize) as u32,
          (i / self.chunks.x as usize) as u32,
        ));
      }
    }
    self.dirty_count = 0;
  }

  /// The version of `chunk`: it changes whenever a cell of the chunk does,
  /// to a value the grid has not given before, so a consumer that keeps the
  /// last version it saw knows whether to rebuild. 0 for a chunk outside
  /// the grid and for one never changed (a new grid)
  pub fn chunk_version(&self, chunk: UVec2) -> u64 {
    if chunk.x >= self.chunks.x || chunk.y >= self.chunks.y {
      return 0;
    }
    self.versions[chunk.y as usize * self.chunks.x as usize + chunk.x as usize]
  }

  /// A version of the whole grid: it changes whenever any chunk's does
  pub fn version(&self) -> u64 {
    self.counter
  }

  /// Marks every chunk dirty, and moves every chunk's version on
  pub fn mark_all_dirty(&mut self) {
    for v in &mut self.versions {
      self.counter += 1;
      *v = self.counter;
    }
    self.dirty.fill(true);
    self.dirty_count = self.dirty.len();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::tilemap::{Tile, TileFlip};
  use glam::{IVec2, UVec2};

  #[test]
  fn a_cell_round_trips_every_field() {
    let mut g = TileGrid::new(UVec2::splat(4));
    let t = Tile {
      tileset: 255,
      id: 1_048_574,
      flip: TileFlip::HORIZONTAL | TileFlip::VERTICAL | TileFlip::DIAGONAL,
    };
    assert!(g.set(IVec2::new(1, 2), Some(t)));
    assert_eq!(g.get(IVec2::new(1, 2)), Some(t));
    let z = Tile::new(0, 0);
    assert!(g.set(IVec2::new(0, 0), Some(z)));
    assert_eq!(g.get(IVec2::new(0, 0)), Some(z));
    assert_eq!(g.get(IVec2::new(3, 3)), None);
    assert!(g.set(IVec2::new(0, 0), None));
    assert_eq!(g.get(IVec2::new(0, 0)), None);
  }

  #[test]
  fn set_reports_change_and_marks_one_chunk() {
    let mut g = TileGrid::new(UVec2::new(100, 40));
    let t = Tile::new(0, 1);
    assert!(g.set(IVec2::new(40, 3), Some(t)));
    assert!(!g.set(IVec2::new(40, 3), Some(t)));
    let mut out = Vec::new();
    g.drain_dirty(&mut out);
    assert_eq!(out, [UVec2::new(1, 0)]);
    g.drain_dirty(&mut out);
    assert!(out.is_empty());
  }

  #[test]
  fn dirty_chunks_come_once_in_row_order() {
    let mut g = TileGrid::new(UVec2::new(100, 100));
    let t = Some(Tile::new(0, 1));
    g.set(IVec2::new(70, 40), t);
    g.set(IVec2::new(1, 1), t);
    g.set(IVec2::new(2, 2), t);
    g.set(IVec2::new(40, 1), t);
    let mut out = Vec::new();
    g.drain_dirty(&mut out);
    assert_eq!(out, [UVec2::new(0, 0), UVec2::new(1, 0), UVec2::new(2, 1)]);
    assert_eq!(g.chunks(), UVec2::new(4, 4));
    g.mark_all_dirty();
    g.drain_dirty(&mut out);
    assert_eq!(out.len(), 16);
  }

  #[test]
  fn fill_clips_and_counts() {
    let mut g = TileGrid::new(UVec2::splat(10));
    let t = Tile::new(0, 7);
    assert_eq!(g.fill(IVec2::new(-5, -5), IVec2::new(2, 2), Some(t)), 9);
    assert_eq!(g.get(IVec2::new(2, 2)), Some(t));
    assert_eq!(g.get(IVec2::new(3, 2)), None);
    assert_eq!(g.get(IVec2::new(2, 3)), None);
    assert_eq!(g.fill(IVec2::new(0, 0), IVec2::new(2, 2), Some(t)), 0);
    g.clear();
    assert_eq!(g.get(IVec2::new(0, 0)), None);
  }

  #[test]
  fn edits_outside_change_nothing() {
    let mut g = TileGrid::new(UVec2::splat(10));
    let t = Some(Tile::new(0, 1));
    assert!(!g.set(IVec2::new(-1, 0), t));
    assert!(!g.set(IVec2::new(10, 0), t));
    assert!(!g.set(IVec2::new(0, 10), t));
    assert_eq!(g.get(IVec2::new(-1, 0)), None);
    assert_eq!(g.get(IVec2::new(10, 0)), None);
    let mut out = Vec::new();
    g.drain_dirty(&mut out);
    assert!(out.is_empty());
  }

  #[test]
  fn a_grid_of_absurd_size_does_not_overflow() {
    let g = TileGrid::new(UVec2::new(u32::MAX, 0));
    assert_eq!(g.chunks(), UVec2::new(u32::MAX / CHUNK_SIZE + 1, 0));
    let g = TileGrid::new(UVec2::new(0, u32::MAX));
    assert_eq!(g.size(), UVec2::new(0, u32::MAX));
  }

  #[test]
  fn a_tile_that_does_not_fit_a_cell_is_refused() {
    let mut g = TileGrid::new(UVec2::splat(2));
    assert!(!g.set(IVec2::ZERO, Some(Tile::new(256, 0))));
    assert!(!g.set(IVec2::ZERO, Some(Tile::new(0, 1_048_575))));
    assert_eq!(g.get(IVec2::ZERO), None);
  }

  #[test]
  fn every_change_bumps_its_chunk_s_version() {
    let mut g = TileGrid::new(UVec2::new(100, 40));
    let t = Some(Tile::new(0, 1));
    let chunk = UVec2::new(1, 0);
    let mut seen = vec![g.chunk_version(chunk)];
    let mut whole = vec![g.version()];
    let mut note = |g: &TileGrid| {
      seen.push(g.chunk_version(chunk));
      whole.push(g.version());
    };
    g.set(IVec2::new(40, 3), t);
    note(&g);
    g.set(IVec2::new(40, 3), None);
    note(&g);
    g.fill(IVec2::new(32, 0), IVec2::new(33, 0), t);
    note(&g);
    g.clear();
    note(&g);
    g.mark_all_dirty();
    note(&g);
    for pair in seen.windows(2) {
      assert_ne!(pair[0], pair[1], "versions {seen:?}");
    }
    let mut sorted = seen.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), seen.len(), "a version came back: {seen:?}");
    assert!(whole.windows(2).all(|p| p[0] != p[1]));
    // A change that changes nothing keeps the version
    let before = g.chunk_version(chunk);
    assert!(!g.set(IVec2::new(40, 3), None));
    assert_eq!(g.chunk_version(chunk), before);
  }

  #[test]
  fn untouched_chunks_keep_their_version() {
    let mut g = TileGrid::new(UVec2::new(100, 40));
    g.set(IVec2::new(70, 3), Some(Tile::new(0, 1)));
    let far = g.chunk_version(UVec2::new(2, 0));
    let other = g.chunk_version(UVec2::new(0, 0));
    g.set(IVec2::new(40, 3), Some(Tile::new(0, 1)));
    g.fill(IVec2::new(32, 0), IVec2::new(60, 5), Some(Tile::new(0, 2)));
    assert_eq!(g.chunk_version(UVec2::new(2, 0)), far);
    assert_eq!(g.chunk_version(UVec2::new(0, 0)), other);
    assert_ne!(g.chunk_version(UVec2::new(1, 0)), 0);
    assert_eq!(g.chunk_version(UVec2::new(9, 9)), 0);
    // draining the dirty marks is not a change
    let v = g.version();
    let mut out = Vec::new();
    g.drain_dirty(&mut out);
    assert_eq!(g.version(), v);
  }
}
