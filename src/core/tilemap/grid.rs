//! The dense grid of tiles in a tile layer.

use std::sync::atomic::{AtomicU64, Ordering};

use glam::{IVec2, UVec2};

use super::tile::{Tile, TileFlip};

/// Where every grid's versions come from: one counter for the process, so
/// no version is given twice, across grids and the clones of one
static VERSIONS: AtomicU64 = AtomicU64::new(1);

/// A version no grid has had
fn next_version() -> u64 {
  VERSIONS.fetch_add(1, Ordering::Relaxed)
}

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
#[derive(Clone)]
pub struct TileGrid {
  size: UVec2,
  cells: Vec<u32>,
  chunks: UVec2,
  dirty: Vec<bool>,
  dirty_count: usize,
  /// Each chunk's version: the version of the change that last touched it
  versions: Vec<u64>,
  /// The version of the grid's last change (0 before any): taken from the
  /// process-wide counter at every change, so a version is a stamp of one
  /// state of one chunk of one grid
  counter: u64,
}

/// The size and the cells: the dirty marks and versions are bookkeeping, and
/// versions come from a process-wide counter, so grids holding the same
/// tiles print alike
impl std::fmt::Debug for TileGrid {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("TileGrid")
      .field("size", &self.size)
      .field("cells", &self.cells)
      .finish_non_exhaustive()
  }
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

  /// Marks `cell`'s chunk changed by the change of version `version`
  fn mark(&mut self, cell: IVec2, version: u64) {
    let c = cell.as_uvec2() / CHUNK_SIZE;
    let i = c.y as usize * self.chunks.x as usize + c.x as usize;
    self.counter = version;
    self.versions[i] = version;
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
    self.mark(cell, next_version());
    true
  }

  /// Sets every cell from `min` to `max`, both inclusive and clipped to the
  /// grid; returns how many changed
  pub fn fill(&mut self, min: IVec2, max: IVec2, tile: Option<Tile>) -> usize {
    let Some(v) = encode(tile) else { return 0 };
    let lo = min.max(IVec2::ZERO);
    let hi = max.min(self.size.as_ivec2() - IVec2::ONE);
    let mut changed = 0;
    // One change, one version, taken only if a cell changes
    let mut version = None;
    for y in lo.y..=hi.y {
      for x in lo.x..=hi.x {
        let cell = IVec2::new(x, y);
        let i = y as usize * self.size.x as usize + x as usize;
        if self.cells[i] != v {
          self.cells[i] = v;
          let version = *version.get_or_insert_with(next_version);
          self.mark(cell, version);
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
  /// to a value larger than any before and never given to any grid (one
  /// counter serves them all, clones included), so a consumer that keeps the
  /// last version it saw knows whether to rebuild, even when the layer's
  /// grid was swapped for another. 0 for a chunk outside the grid and for
  /// one never changed (a new grid: empty, like any other new grid's)
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

  /// Takes `from`'s cells (a grid of the same size: another size changes
  /// nothing), moving on the version of only the chunks whose cells differ
  /// or that `also` names as changed for a reason of its own, as one
  /// change, and marking them dirty. Whether any chunk moved
  pub(crate) fn assign(&mut self, from: &TileGrid, also: impl Fn(UVec2) -> bool) -> bool {
    if from.size != self.size {
      return false;
    }
    let width = self.size.x as usize;
    let mut version = None;
    for cy in 0..self.chunks.y {
      for cx in 0..self.chunks.x {
        let chunk = UVec2::new(cx, cy);
        let x0 = (cx * CHUNK_SIZE) as usize;
        let x1 = ((cx + 1) * CHUNK_SIZE).min(self.size.x) as usize;
        let rows = cy * CHUNK_SIZE..((cy + 1) * CHUNK_SIZE).min(self.size.y);
        let mut changed = also(chunk);
        for y in rows {
          let row = y as usize * width;
          let (old, new) = (
            &mut self.cells[row + x0..row + x1],
            &from.cells[row + x0..row + x1],
          );
          if old != new {
            old.copy_from_slice(new);
            changed = true;
          }
        }
        if changed {
          let version = *version.get_or_insert_with(next_version);
          let i = cy as usize * self.chunks.x as usize + cx as usize;
          self.versions[i] = version;
          if !self.dirty[i] {
            self.dirty[i] = true;
            self.dirty_count += 1;
          }
        }
      }
    }
    if let Some(version) = version {
      self.counter = version;
    }
    version.is_some()
  }

  /// Moves on the version of each of `chunks` inside the grid, as one
  /// change, and marks them dirty, the cells as they are: for what is kept
  /// beside the grid and read through its versions (an IntGrid's values).
  /// Whether any chunk moved
  pub(crate) fn touch(&mut self, chunks: &[UVec2]) -> bool {
    let mut version = None;
    for chunk in chunks {
      if chunk.x >= self.chunks.x || chunk.y >= self.chunks.y {
        continue;
      }
      let version = *version.get_or_insert_with(next_version);
      let i = chunk.y as usize * self.chunks.x as usize + chunk.x as usize;
      self.versions[i] = version;
      if !self.dirty[i] {
        self.dirty[i] = true;
        self.dirty_count += 1;
      }
    }
    if let Some(version) = version {
      self.counter = version;
    }
    version.is_some()
  }

  /// Marks every chunk dirty, and moves every chunk's version on
  pub fn mark_all_dirty(&mut self) {
    let version = next_version();
    self.versions.fill(version);
    self.counter = version;
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
  fn cloned_grids_never_share_a_version_after_an_edit() {
    let mut g = TileGrid::new(UVec2::splat(64));
    g.set(IVec2::new(1, 1), Some(Tile::new(0, 1)));
    let mut h = g.clone();
    // A clone holds the same state, so the same versions
    assert_eq!(h.chunk_version(UVec2::ZERO), g.chunk_version(UVec2::ZERO));
    g.set(IVec2::new(2, 2), Some(Tile::new(0, 2)));
    h.set(IVec2::new(2, 2), Some(Tile::new(0, 3)));
    assert_ne!(g.chunk_version(UVec2::ZERO), h.chunk_version(UVec2::ZERO));
    assert_ne!(g.version(), h.version());
    // Each grid's versions still only move on
    let before = g.chunk_version(UVec2::ZERO);
    g.set(IVec2::new(3, 3), Some(Tile::new(0, 4)));
    assert!(g.chunk_version(UVec2::ZERO) > before);
  }

  #[test]
  fn grids_filled_alike_never_share_a_version() {
    // Two grids made and changed in the same order, as a level generated
    // again would be: a layer swapping one for the other must see a change
    let made = || {
      let mut g = TileGrid::new(UVec2::splat(64));
      g.fill(IVec2::ZERO, IVec2::splat(63), Some(Tile::new(0, 1)));
      g.set(IVec2::new(40, 40), Some(Tile::new(0, 2)));
      g.mark_all_dirty();
      g
    };
    let (a, b) = (made(), made());
    for y in 0..2 {
      for x in 0..2 {
        let chunk = UVec2::new(x, y);
        assert_ne!(a.chunk_version(chunk), b.chunk_version(chunk), "{chunk}");
      }
    }
    assert_ne!(a.version(), b.version());
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

  #[test]
  fn assigning_a_grid_moves_only_the_chunks_whose_cells_differ() {
    let chunks = [
      UVec2::new(0, 0),
      UVec2::new(1, 0),
      UVec2::new(0, 1),
      UVec2::new(1, 1),
    ];
    let versions = |g: &TileGrid| chunks.map(|c| g.chunk_version(c));
    let mut g = TileGrid::new(UVec2::new(40, 40));
    g.fill(IVec2::ZERO, IVec2::splat(39), Some(Tile::new(0, 1)));
    let mut out = Vec::new();
    g.drain_dirty(&mut out);
    let before = versions(&g);
    let mut saved = g.clone();
    saved.set(IVec2::new(35, 2), Some(Tile::new(0, 2)));
    saved.set(IVec2::new(39, 39), None);

    assert!(g.assign(&saved, |_| false));
    let after = versions(&g);
    assert_eq!((after[0], after[2]), (before[0], before[2]));
    assert!(after[1] > before[1] && after[3] > before[3]);
    assert_eq!(after[1], after[3], "one change");
    assert_eq!(g.version(), after[1]);
    assert_eq!(g.get(IVec2::new(35, 2)), Some(Tile::new(0, 2)));
    assert_eq!(g.get(IVec2::new(39, 39)), None);
    assert_eq!(format!("{g:?}"), format!("{saved:?}"));
    g.drain_dirty(&mut out);
    assert_eq!(out, [UVec2::new(1, 0), UVec2::new(1, 1)]);

    // The same cells again change nothing, unless a chunk is named
    assert!(!g.assign(&saved, |_| false));
    assert_eq!(versions(&g), after);
    assert!(g.assign(&saved, |c| c == UVec2::new(0, 1)));
    let named = versions(&g);
    assert_eq!(
      (named[0], named[1], named[3]),
      (after[0], after[1], after[3])
    );
    assert!(named[2] > after[2]);

    // Another size changes nothing
    let other = TileGrid::new(UVec2::new(8, 8));
    assert!(!g.assign(&other, |_| true));
    assert_eq!(versions(&g), named);
    assert_eq!(g.get(IVec2::new(35, 2)), Some(Tile::new(0, 2)));
  }

  #[test]
  fn touching_chunks_moves_only_them_as_one_change() {
    let mut g = TileGrid::new(UVec2::new(40, 40));
    g.fill(IVec2::ZERO, IVec2::splat(39), Some(Tile::new(0, 1)));
    let mut out = Vec::new();
    g.drain_dirty(&mut out);
    let cells = format!("{g:?}");
    let (kept, before) = (g.chunk_version(UVec2::ZERO), g.version());
    assert!(g.touch(&[UVec2::new(1, 0), UVec2::new(1, 1), UVec2::new(5, 5)]));
    let moved = g.chunk_version(UVec2::new(1, 0));
    assert!(moved > before);
    assert_eq!(g.chunk_version(UVec2::new(1, 1)), moved, "one change");
    assert_eq!((g.chunk_version(UVec2::ZERO), g.version()), (kept, moved));
    assert_eq!(format!("{g:?}"), cells, "the cells as they were");
    g.drain_dirty(&mut out);
    assert_eq!(out, [UVec2::new(1, 0), UVec2::new(1, 1)]);
    // Nothing inside: nothing moves
    assert!(!g.touch(&[UVec2::new(2, 0)]));
    assert_eq!(g.version(), moved);
  }
}
