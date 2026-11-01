//! The grid of tiles in a tile layer: chunks of cells by signed chunk
//! coordinate, made where tiles are written.

use std::collections::hash_map::Entry;
use std::sync::atomic::{AtomicU64, Ordering};

use glam::{IVec2, UVec2};

use super::tile::{Tile, TileFlip};
use crate::core::hash::FastMap;
use crate::core::physics::CellRect;

/// Where every grid's versions come from: one counter for the process, so
/// no version is given twice, across grids and the clones of one
static VERSIONS: AtomicU64 = AtomicU64::new(1);

/// A version no grid has had
fn next_version() -> u64 {
  VERSIONS.fetch_add(1, Ordering::Relaxed)
}

/// The side of a chunk in cells: the unit a layer redraws and rebuilds by
pub const CHUNK_SIZE: u32 = 32;

/// Most chunks one tile fill makes: the chunks of the cells a map holds
/// (`MAX_MAP_CELLS`, 2^27 cells, 512 MiB), 131,072, so a fill over a huge
/// region of an unbounded grid is refused rather than exhausting memory
pub(crate) const MAX_FILL_CHUNKS: u64 =
  super::tiled::build::MAX_MAP_CELLS / (CHUNK_SIZE as u64 * CHUNK_SIZE as u64);

#[cfg(test)]
thread_local! {
  /// A test's own cap on the chunks a fill makes, lower than
  /// `MAX_FILL_CHUNKS`: a test of the refusal then asks for a few chunks
  /// past it, so it allocates kilobytes, not gigabytes, should the guard
  /// ever be gone
  pub(crate) static FILL_CAP: std::cell::Cell<u64> = const { std::cell::Cell::new(MAX_FILL_CHUNKS) };
}

/// The most chunks a tile fill may make: `MAX_FILL_CHUNKS` (in a test, the
/// thread's `FILL_CAP`)
pub(crate) fn fill_cap() -> u64 {
  #[cfg(test)]
  return FILL_CAP.with(std::cell::Cell::get);
  #[cfg(not(test))]
  MAX_FILL_CHUNKS
}

/// A chunk's side as a cell coordinate
const SIDE: i32 = CHUNK_SIZE as i32;
/// The cells of a chunk
const CELLS: usize = (CHUNK_SIZE * CHUNK_SIZE) as usize;

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

/// A chunk's key in row order: by row, then by column
fn row_order(chunk: &IVec2) -> (i32, i32) {
  (chunk.y, chunk.x)
}

/// The cells of `chunk`: its first cell (a multiple of `SIDE`) never
/// overflows, and the end of the last chunk `i32` reaches stops at
/// `i32::MAX`
fn chunk_rect(chunk: IVec2) -> CellRect {
  let min = chunk * SIDE;
  CellRect::new(min, min.saturating_add(IVec2::splat(SIDE)))
}

/// The first and last chunk (both included) holding cells of `rect`, which
/// is not empty
fn chunk_span(rect: CellRect) -> (IVec2, IVec2) {
  (
    TileGrid::chunk_of(rect.min),
    TileGrid::chunk_of(rect.max - IVec2::ONE),
  )
}

/// `cell`'s place among its chunk's cells
fn local(cell: IVec2) -> usize {
  let at = cell.rem_euclid(IVec2::splat(SIDE));
  (at.y * SIDE + at.x) as usize
}

/// A chunk's cells, every one empty
fn empty_cells() -> Box<[u32; CELLS]> {
  Box::new([0; CELLS])
}

/// One chunk of a grid
#[derive(Clone, Default)]
struct Chunk {
  /// Row by row; `None` while every cell is empty (a chunk marked,
  /// touched or cleared, which needs a version but no cells)
  cells: Option<Box<[u32; CELLS]>>,
  /// The version of the change that last touched it
  version: u64,
  dirty: bool,
}

impl Chunk {
  /// Stamps it with the change of version `version` and marks it dirty,
  /// counting it in `dirty` if it was not
  fn mark(&mut self, version: u64, dirty: &mut usize) {
    self.version = version;
    if !self.dirty {
      self.dirty = true;
      *dirty += 1;
    }
  }

  /// Its cells, an empty chunk's as none
  fn cells(&self) -> Option<&[u32; CELLS]> {
    self.cells.as_deref()
  }
}

/// Whether `cells` (none for an empty chunk) holds a tile
fn holds_tiles(cells: Option<&[u32; CELLS]>) -> bool {
  cells.is_some_and(|cells| cells.iter().any(|&c| c != 0))
}

/// A layer's cells, 4 bytes each, in chunks of `CHUNK_SIZE` cells a side
/// keyed by signed chunk coordinate: 0 is empty, otherwise the flips sit in
/// bits 28 to 31, the tileset in 20 to 27 and the tile id plus one in 0 to
/// 19. A map holds at most 256 tilesets of 1,048,575 tiles each; a tile
/// outside that is refused.
///
/// A bounded grid ([`new`](Self::new)) holds the cells from (0, 0) to its
/// size and refuses the rest; an unbounded one ([`unbounded`](Self::unbounded))
/// holds any cell. Either makes a chunk the first time a tile is written in
/// it, so an empty chunk costs nothing; clearing leaves the chunk. Edits
/// mark the chunks they touch dirty and move on their versions.
pub struct TileGrid {
  /// Never given to another grid, a clone included: a consumer that keeps
  /// the version it last read a grid at knows the chunks changed since are
  /// those of a later version, while the grid is the one it read
  id: u64,
  /// The cells it holds; `None` for any
  bounds: Option<CellRect>,
  chunks: FastMap<IVec2, Chunk>,
  /// The coordinates of `chunks` in row order, kept as chunks are made
  order: Vec<IVec2>,
  /// The cells of the chunks there are
  span: CellRect,
  /// How many chunks are dirty
  dirty: usize,
  /// The version of the grid's last change (0 before any): taken from the
  /// process-wide counter at every change, so a version is a stamp of one
  /// state of one chunk of one grid
  counter: u64,
}

/// A copy with an id of its own: its versions are the original's, but
/// edits to either give the other's consumers nothing to read
impl Clone for TileGrid {
  fn clone(&self) -> TileGrid {
    TileGrid {
      id: next_version(),
      bounds: self.bounds,
      chunks: self.chunks.clone(),
      order: self.order.clone(),
      span: self.span,
      dirty: self.dirty,
      counter: self.counter,
    }
  }
}

/// The bounds and the chunks holding tiles, with their cells: the dirty
/// marks and versions are bookkeeping, versions come from a process-wide
/// counter and an empty chunk holds nothing, so grids holding the same
/// tiles print alike
impl std::fmt::Debug for TileGrid {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let chunks: Vec<(IVec2, &[u32])> = self
      .order
      .iter()
      .filter_map(|chunk| {
        let cells = self.chunks.get(chunk)?.cells()?;
        holds_tiles(Some(cells)).then_some((*chunk, &cells[..]))
      })
      .collect();
    f.debug_struct("TileGrid")
      .field("bounds", &self.bounds)
      .field("chunks", &chunks)
      .finish_non_exhaustive()
  }
}

impl TileGrid {
  /// An empty bounded grid of the cells from (0, 0) to `size`, no chunk
  /// dirty
  pub fn new(size: UVec2) -> TileGrid {
    TileGrid::with_bounds(Some(CellRect::from_size(size)))
  }

  /// An empty grid holding any cell, signed: each chunk is made the first
  /// time a tile is written in it
  pub fn unbounded() -> TileGrid {
    TileGrid::with_bounds(None)
  }

  fn with_bounds(bounds: Option<CellRect>) -> TileGrid {
    TileGrid {
      id: next_version(),
      bounds,
      chunks: FastMap::default(),
      order: Vec::new(),
      span: CellRect::default(),
      dirty: 0,
      counter: 0,
    }
  }

  /// The cells a bounded grid holds; `None` for an unbounded one
  pub fn bounds(&self) -> Option<CellRect> {
    self.bounds
  }

  /// The cells to look at for its tiles: a bounded grid's bounds, an
  /// unbounded one's chunks' cells (empty before any is made; the end of a
  /// chunk at the far end of `i32` stops at `i32::MAX`)
  pub fn extent(&self) -> CellRect {
    self.bounds.unwrap_or(self.span)
  }

  /// The smallest rectangle holding every cell with a tile; empty when the
  /// grid holds none (a cell at `i32::MAX` is left out, as `extent` leaves
  /// it). Found from the chunks along the edges of those holding tiles: a
  /// look at each chunk for a tile, then the cells of the edge chunks
  pub(crate) fn tile_extent(&self) -> CellRect {
    let tiled = || {
      self.order.iter().filter_map(|chunk| {
        let cells = self.chunks.get(chunk)?.cells()?;
        holds_tiles(Some(cells)).then_some((*chunk, cells))
      })
    };
    let Some((first, last)) = tiled().fold(None, |span, (chunk, _)| match span {
      None => Some((chunk, chunk)),
      Some((first, last)) => Some((chunk.min(first), chunk.max(last))),
    }) else {
      return CellRect::default();
    };
    // Each side's outermost tile is in a chunk on that side, so the tiles
    // of the edge chunks give every side
    let (mut min, mut max) = (IVec2::MAX, IVec2::MIN);
    for (chunk, cells) in tiled() {
      let edge = chunk.x == first.x || chunk.x == last.x || chunk.y == first.y || chunk.y == last.y;
      if !edge {
        continue;
      }
      let origin = chunk * SIDE;
      for (at, _) in cells.iter().enumerate().filter(|&(_, &c)| c != 0) {
        let cell = origin + IVec2::new(at as i32 % SIDE, at as i32 / SIDE);
        min = min.min(cell);
        max = max.max(cell);
      }
    }
    CellRect::new(min, max.saturating_add(IVec2::ONE))
  }

  /// The chunk `cell` is in, rounding toward negative infinity: cell -1 is
  /// in chunk -1, cell -33 in chunk -2
  pub fn chunk_of(cell: IVec2) -> IVec2 {
    cell.div_euclid(IVec2::splat(SIDE))
  }

  /// Whether the grid holds `cell`
  fn holds(&self, cell: IVec2) -> bool {
    self.bounds.is_none_or(|bounds| bounds.contains(cell))
  }

  /// Whether `chunk` holds cells of the grid
  fn holds_chunk(&self, chunk: IVec2) -> bool {
    match self.bounds {
      None => true,
      Some(bounds) if bounds.is_empty() => false,
      Some(bounds) => {
        let (first, last) = chunk_span(bounds);
        chunk.cmpge(first).all() && chunk.cmple(last).all()
      }
    }
  }

  /// The tile at `cell`; `None` if empty or outside
  pub fn get(&self, cell: IVec2) -> Option<Tile> {
    if !self.holds(cell) {
      return None;
    }
    let cells = self.chunks.get(&Self::chunk_of(cell))?.cells()?;
    decode(cells[local(cell)])
  }

  /// Puts `tile` (or clears) at `cell`, making its chunk if a tile goes
  /// where there is none. False if the cell is outside, the tile does not
  /// fit a cell, or nothing changed
  pub fn set(&mut self, cell: IVec2, tile: Option<Tile>) -> bool {
    let Some(value) = encode(tile) else {
      return false;
    };
    if !self.holds(cell) {
      return false;
    }
    let (chunk, at) = (Self::chunk_of(cell), local(cell));
    let before = self.order.len();
    let Self {
      chunks,
      order,
      span,
      dirty,
      counter,
      ..
    } = self;
    // One lookup: clearing reads the chunk if there is one, a tile makes it
    let entry = if value == 0 {
      match chunks.get_mut(&chunk) {
        Some(entry) => entry,
        None => return false,
      }
    } else {
      make_chunk(chunks, order, span, chunk)
    };
    let cells = match &mut entry.cells {
      Some(cells) => cells,
      None if value == 0 => return false,
      cells @ None => cells.insert(empty_cells()),
    };
    if cells[at] == value {
      return false;
    }
    cells[at] = value;
    let version = next_version();
    entry.mark(version, dirty);
    *counter = version;
    settle(order, before);
    true
  }

  /// Sets every cell from `min` to `max`, both inclusive (clipped to a
  /// bounded grid); returns how many changed. A tile makes the chunks it
  /// goes in; clearing reads only the chunks there are, and leaves them. A
  /// tile fill that would make more chunks than a map holds (131,072 new
  /// chunks, the 2^27 cells a map may hold) changes nothing and returns 0,
  /// so a huge region of an unbounded grid cannot exhaust memory
  pub fn fill(&mut self, min: IVec2, max: IVec2, tile: Option<Tile>) -> usize {
    let Some(value) = encode(tile) else { return 0 };
    let (lo, hi) = match self.bounds {
      None => (min, max),
      Some(bounds) if bounds.is_empty() => return 0,
      Some(bounds) => (min.max(bounds.min), max.min(bounds.max - IVec2::ONE)),
    };
    if lo.x > hi.x || lo.y > hi.y {
      return 0;
    }
    if value != 0 && self.missing_chunks(lo, hi) > fill_cap() {
      return 0;
    }
    let (first, last) = (Self::chunk_of(lo), Self::chunk_of(hi));
    let before = self.order.len();
    let Self {
      chunks,
      order,
      span,
      dirty,
      counter,
      ..
    } = self;
    // One change, one version, taken only if a cell changes
    let mut version = None;
    let mut changed = 0;
    let mut write = |chunk: IVec2, entry: &mut Chunk| {
      let Some(cells) = (match (&mut entry.cells, value) {
        (Some(cells), _) => Some(cells),
        (None, 0) => None,
        (cells @ None, _) => Some(cells.insert(empty_cells())),
      }) else {
        return;
      };
      // The part of the region in the chunk: neither corner overflows
      let origin = chunk * SIDE;
      let (a, b) = (lo.max(origin), hi.min(origin + IVec2::splat(SIDE - 1)));
      let mut any = false;
      for y in a.y..=b.y {
        let row = ((y - origin.y) * SIDE) as usize;
        for x in a.x..=b.x {
          let slot = &mut cells[row + (x - origin.x) as usize];
          if *slot != value {
            *slot = value;
            changed += 1;
            any = true;
          }
        }
      }
      if any {
        let version = *version.get_or_insert_with(next_version);
        entry.mark(version, dirty);
        *counter = version;
      }
    };
    if value == 0 {
      // Only the chunks there are hold a tile to clear: the region's, or
      // the list of them, whichever is shorter
      let (ranged, listed) = within(order, chunks.len(), first, last);
      for chunk in ranged.chain(listed) {
        if let Some(entry) = chunks.get_mut(&chunk) {
          write(chunk, entry);
        }
      }
    } else {
      for y in first.y..=last.y {
        for x in first.x..=last.x {
          let chunk = IVec2::new(x, y);
          write(chunk, make_chunk(chunks, order, span, chunk));
        }
      }
    }
    settle(order, before);
    changed
  }

  /// How many chunks holding the cells from `lo` to `hi` (both included,
  /// `lo` not past `hi`) the grid has yet to make: what a tile fill of
  /// them would make. Counted without making or walking more than the
  /// range or the list of chunks, whichever is shorter
  pub(crate) fn missing_chunks(&self, lo: IVec2, hi: IVec2) -> u64 {
    let (first, last) = (Self::chunk_of(lo), Self::chunk_of(hi));
    let side = |a: i32, b: i32| (i64::from(b) - i64::from(a) + 1).max(0) as u64;
    let area = side(first.x, last.x) * side(first.y, last.y);
    let (ranged, listed) = within(&self.order, self.chunks.len(), first, last);
    let had = ranged.filter(|c| self.chunks.contains_key(c)).count() + listed.count();
    area.saturating_sub(had as u64)
  }

  /// Empties every cell, keeping the chunks
  pub fn clear(&mut self) {
    if self.chunks.values().any(|chunk| holds_tiles(chunk.cells())) {
      for chunk in self.chunks.values_mut() {
        chunk.cells = None;
      }
      self.mark_all_dirty();
    }
  }

  /// The chunks there are, in row order (by row, then column): each made
  /// by the first tile written in it, or by a change that reached it
  /// without one (`mark_all_dirty` makes every chunk of a bounded grid's
  /// bounds). A chunk stays once made, cleared or not
  pub fn chunk_coords(&self) -> impl Iterator<Item = IVec2> + '_ {
    self.order.iter().copied()
  }

  /// Appends to `out` each chunk changed since the last drain, once, in row
  /// order, and clears the marks. `out` is cleared first
  pub fn drain_dirty(&mut self, out: &mut Vec<IVec2>) {
    out.clear();
    if self.dirty == 0 {
      return;
    }
    for chunk in &self.order {
      if let Some(entry) = self.chunks.get_mut(chunk)
        && entry.dirty
      {
        entry.dirty = false;
        out.push(*chunk);
      }
    }
    self.dirty = 0;
  }

  /// The version of `chunk`: it changes whenever a cell of the chunk does,
  /// to a value larger than any before and never given to any grid (one
  /// counter serves them all, clones included), so a consumer that keeps the
  /// last version it saw knows whether to rebuild, even when the layer's
  /// grid was swapped for another. 0 for a chunk never changed (a new
  /// grid's: empty, like any other new grid's) and for one outside the grid
  pub fn chunk_version(&self, chunk: IVec2) -> u64 {
    self.chunks.get(&chunk).map_or(0, |entry| entry.version)
  }

  /// A version of the whole grid: it changes whenever any chunk's does
  pub fn version(&self) -> u64 {
    self.counter
  }

  /// Marks every chunk dirty, and moves every chunk's version on: every
  /// chunk of a bounded grid's bounds (an empty one made, holding no
  /// cells), every chunk there is of an unbounded one
  pub fn mark_all_dirty(&mut self) {
    let version = next_version();
    self.counter = version;
    if let Some(bounds) = self.bounds.filter(|bounds| !bounds.is_empty()) {
      let (first, last) = chunk_span(bounds);
      let count = (i64::from(last.x - first.x) + 1) * (i64::from(last.y - first.y) + 1);
      if self.chunks.len() as i64 != count {
        // A bounded grid's chunks are all inside its bounds, so they are
        // made by row
        self.order.clear();
        for y in first.y..=last.y {
          for x in first.x..=last.x {
            let chunk = IVec2::new(x, y);
            self.chunks.entry(chunk).or_default();
            self.order.push(chunk);
            self.span = self.span.union(chunk_rect(chunk));
          }
        }
      }
    }
    for entry in self.chunks.values_mut() {
      entry.version = version;
      entry.dirty = true;
    }
    self.dirty = self.chunks.len();
  }

  /// Takes `from`'s cells (a grid of the same bounds, or unbounded as
  /// `from` is: any other changes nothing), moving on the version of only
  /// the chunks whose cells differ or that `also` names as changed for a
  /// reason of its own, as one change, and marking them dirty. A bounded
  /// grid asks `also` of every chunk of its bounds, an unbounded one of the
  /// chunks either grid has; a chunk `from` lacks is emptied and kept.
  /// Whether any chunk moved
  pub(crate) fn assign(&mut self, from: &TileGrid, also: impl Fn(IVec2) -> bool) -> bool {
    if from.bounds != self.bounds {
      return false;
    }
    let candidates: Vec<IVec2> = match self.bounds {
      Some(bounds) if bounds.is_empty() => Vec::new(),
      Some(bounds) => {
        let (first, last) = chunk_span(bounds);
        (first.y..=last.y)
          .flat_map(|y| (first.x..=last.x).map(move |x| IVec2::new(x, y)))
          .collect()
      }
      None => {
        let mut both: Vec<IVec2> = self.order.iter().chain(&from.order).copied().collect();
        both.sort_unstable_by_key(row_order);
        both.dedup();
        both
      }
    };
    let before = self.order.len();
    let Self {
      chunks,
      order,
      span,
      dirty,
      counter,
      ..
    } = self;
    let mut version = None;
    for chunk in candidates {
      let new = from.chunks.get(&chunk).and_then(Chunk::cells);
      let old = chunks.get(&chunk).and_then(Chunk::cells);
      let named = also(chunk);
      let differ = match (old, new) {
        (Some(old), Some(new)) => old != new,
        (Some(cells), None) | (None, Some(cells)) => holds_tiles(Some(cells)),
        (None, None) => false,
      };
      if !named && !differ {
        continue;
      }
      let entry = make_chunk(chunks, order, span, chunk);
      if differ {
        match (&mut entry.cells, new) {
          (Some(cells), Some(new)) => cells.copy_from_slice(new),
          (cells, Some(new)) => *cells = Some(Box::new(*new)),
          (cells, None) => *cells = None,
        }
      }
      let version = *version.get_or_insert_with(next_version);
      entry.mark(version, dirty);
      *counter = version;
    }
    settle(order, before);
    version.is_some()
  }

  /// Moves on the version of each of `chunks` holding cells of the grid, as
  /// one change, and marks them dirty, the cells as they are (a chunk not
  /// made yet is made, holding none): for what is kept beside the grid and
  /// read through its versions (an IntGrid's values). Whether any chunk
  /// moved
  pub(crate) fn touch(&mut self, chunks: &[IVec2]) -> bool {
    let before = self.order.len();
    let mut version = None;
    for &chunk in chunks {
      if !self.holds_chunk(chunk) {
        continue;
      }
      let version = *version.get_or_insert_with(next_version);
      let entry = make_chunk(&mut self.chunks, &mut self.order, &mut self.span, chunk);
      entry.mark(version, &mut self.dirty);
      self.counter = version;
    }
    settle(&mut self.order, before);
    version.is_some()
  }

  /// The cells of `chunk`, `CHUNK_SIZE` rows of `CHUNK_SIZE`, for a builder
  /// walking a chunk without looking it up for every cell (each read with
  /// [`decode`](Self::decode)); `None` for a chunk holding no cells
  pub(crate) fn chunk_cells(&self, chunk: IVec2) -> Option<&[u32; CELLS]> {
    self.chunks.get(&chunk)?.cells()
  }

  /// The tile a cell of [`chunk_cells`](Self::chunk_cells) holds
  pub(crate) fn decode(cell: u32) -> Option<Tile> {
    decode(cell)
  }

  /// The cell `tile` (0 for none) is held as, for
  /// [`write_rows`](Self::write_rows); `None` for a tile that does not fit
  /// a cell
  pub(crate) fn encode(tile: Option<Tile>) -> Option<u32> {
    encode(tile)
  }

  /// Writes `cells` (cells as [`encode`](Self::encode) gives them, in rows
  /// of `width`; a last row short of it is left out) with the first at
  /// `origin`, looking each chunk it reaches up once rather than each cell,
  /// as one change: what a loader writes a decoded layer or chunk with. A
  /// chunk is made only where a tile goes; cells outside a bounded grid, or
  /// past what an `i32` holds, are left out. Whether any cell changed
  pub(crate) fn write_rows(&mut self, origin: IVec2, width: usize, cells: &[u32]) -> bool {
    let rows = cells.len().checked_div(width).unwrap_or(0);
    if rows == 0 {
      return false;
    }
    // The block's corners, both included, clipped to the cells the grid
    // holds: measured in i64, so a block running past `i32` cannot wrap
    let (mut lo, mut hi) = (
      origin.as_i64vec2(),
      origin.as_i64vec2() + glam::I64Vec2::new(width as i64 - 1, rows as i64 - 1),
    );
    let (min, max) = match self.bounds {
      Some(bounds) => (bounds.min.as_i64vec2(), bounds.max.as_i64vec2() - 1),
      None => (
        glam::I64Vec2::splat(i64::from(i32::MIN)),
        glam::I64Vec2::splat(i64::from(i32::MAX)),
      ),
    };
    lo = lo.max(min);
    hi = hi.min(max);
    if lo.x > hi.x || lo.y > hi.y {
      return false;
    }
    let (lo, hi) = (lo.as_ivec2(), hi.as_ivec2());
    let (first, last) = (Self::chunk_of(lo), Self::chunk_of(hi));
    let before = self.order.len();
    let Self {
      chunks,
      order,
      span,
      dirty,
      counter,
      ..
    } = self;
    let mut version = None;
    for cy in first.y..=last.y {
      for cx in first.x..=last.x {
        let chunk = IVec2::new(cx, cy);
        // The part of the block in the chunk: neither corner overflows,
        // the last chunk of `i32` ending at `i32::MAX`
        let corner = chunk * SIDE;
        let (a, b) = (lo.max(corner), hi.min(corner + IVec2::splat(SIDE - 1)));
        let run = (b.x - a.x + 1) as usize;
        let source = |y: i32| {
          let at = (y - origin.y) as usize * width + (a.x - origin.x) as usize;
          &cells[at..at + run]
        };
        let tiles = (a.y..=b.y).any(|y| source(y).iter().any(|&c| c != 0));
        let entry = if tiles {
          make_chunk(chunks, order, span, chunk)
        } else {
          match chunks.get_mut(&chunk) {
            Some(entry) if entry.cells.is_some() => entry,
            _ => continue,
          }
        };
        let target = entry.cells.get_or_insert_with(empty_cells);
        let mut changed = false;
        for y in a.y..=b.y {
          let at = ((y - corner.y) * SIDE + (a.x - corner.x)) as usize;
          let row = &mut target[at..at + run];
          let from = source(y);
          if row != from {
            row.copy_from_slice(from);
            changed = true;
          }
        }
        if changed {
          let version = *version.get_or_insert_with(next_version);
          entry.mark(version, dirty);
          *counter = version;
        }
      }
    }
    settle(order, before);
    version.is_some()
  }

  /// What tells this grid from every other, its clones included
  pub(crate) fn id(&self) -> u64 {
    self.id
  }

  /// How many chunks there are
  pub(crate) fn chunk_count(&self) -> usize {
    self.order.len()
  }

  /// Whether there is a chunk `chunk` (holding cells or not)
  pub(crate) fn has_chunk(&self, chunk: IVec2) -> bool {
    self.chunks.contains_key(&chunk)
  }

  /// Each chunk there is with its version, in no set order, without a
  /// lookup each
  pub(crate) fn chunk_versions(&self) -> impl Iterator<Item = (IVec2, u64)> + '_ {
    self
      .chunks
      .iter()
      .map(|(&chunk, entry)| (chunk, entry.version))
  }

  /// The chunks there are from `first` to `last` (both included), in row
  /// order: found by walking that range or the list of chunks, whichever
  /// is shorter, so neither a wide range over a few chunks nor a small one
  /// over many walks far
  pub(crate) fn chunks_within(
    &self,
    first: IVec2,
    last: IVec2,
  ) -> impl Iterator<Item = IVec2> + '_ {
    let (ranged, listed) = within(&self.order, self.chunks.len(), first, last);
    ranged
      .filter(|chunk| self.chunks.contains_key(chunk))
      .chain(listed)
  }
}

/// The chunks from `first` to `last` (both included) to look at, in row
/// order, as two parts of which one is empty: the range itself, when it
/// holds no more chunks than the `count` there are, else those of `order`
/// (the chunks there are, in row order) inside it
fn within(
  order: &[IVec2],
  count: usize,
  first: IVec2,
  last: IVec2,
) -> (
  impl Iterator<Item = IVec2> + use<>,
  impl Iterator<Item = IVec2> + '_,
) {
  let side = |a: i32, b: i32| (i64::from(b) - i64::from(a) + 1).max(0);
  let area = side(first.x, last.x).saturating_mul(side(first.y, last.y));
  let walk = area <= count as i64;
  let ranged = walk
    .then_some(first.y..=last.y)
    .into_iter()
    .flatten()
    .flat_map(move |y| (first.x..=last.x).map(move |x| IVec2::new(x, y)));
  let rows = if walk {
    &order[..0]
  } else {
    // `order` is by row, then column: the rows of the range are one run
    let from = order.partition_point(|c| c.y < first.y);
    let to = order.partition_point(|c| c.y <= last.y);
    &order[from..to.max(from)]
  };
  let listed = rows
    .iter()
    .copied()
    .filter(move |c| (first.x..=last.x).contains(&c.x));
  (ranged, listed)
}

/// `chunk`'s entry in `chunks`, made (holding no cells) if there is none:
/// its coordinates then go on the end of `order` for [`settle`] to place,
/// and its cells join `span`
fn make_chunk<'a>(
  chunks: &'a mut FastMap<IVec2, Chunk>,
  order: &mut Vec<IVec2>,
  span: &mut CellRect,
  chunk: IVec2,
) -> &'a mut Chunk {
  match chunks.entry(chunk) {
    Entry::Occupied(entry) => entry.into_mut(),
    Entry::Vacant(entry) => {
      order.push(chunk);
      *span = span.union(chunk_rect(chunk));
      entry.insert(Chunk::default())
    }
  }
}

/// Puts the chunks pushed onto `order` past its first `before` (which are
/// in row order) in their places: one moved into place, more sorted
fn settle(order: &mut [IVec2], before: usize) {
  match order.len() - before {
    0 => {}
    1 => {
      let new = order[before];
      let at = order[..before].partition_point(|c| row_order(c) < row_order(&new));
      order[at..].rotate_right(1);
    }
    _ => order.sort_unstable_by_key(row_order),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::physics::CellRect;
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
    assert_eq!(out, [IVec2::new(1, 0)]);
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
    assert_eq!(out, [IVec2::new(0, 0), IVec2::new(1, 0), IVec2::new(2, 1)]);
    // Only the chunks written exist; marking all reaches every chunk of
    // the bounds, the empty ones given a version but no cells
    assert_eq!(g.chunk_coords().count(), 3);
    g.mark_all_dirty();
    g.drain_dirty(&mut out);
    assert_eq!(out.len(), 16);
    assert!(out.windows(2).all(|p| (p[0].y, p[0].x) < (p[1].y, p[1].x)));
    assert!(g.chunk_version(IVec2::new(3, 3)) > 0);
    assert!(g.chunk_cells(IVec2::new(3, 3)).is_none());
    assert_eq!(g.chunk_version(IVec2::new(4, 3)), 0, "past the bounds");
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
    let t = Some(Tile::new(0, 1));
    let mut g = TileGrid::new(UVec2::new(u32::MAX, 0));
    assert_eq!(
      g.bounds(),
      Some(CellRect::new(IVec2::ZERO, IVec2::new(i32::MAX, 0)))
    );
    assert!(g.extent().is_empty());
    assert!(!g.set(IVec2::ZERO, t));
    assert_eq!(g.fill(IVec2::MIN, IVec2::MAX, t), 0);
    g.mark_all_dirty();
    let mut out = Vec::new();
    g.drain_dirty(&mut out);
    assert!(out.is_empty());
    let g = TileGrid::new(UVec2::new(0, u32::MAX));
    assert_eq!(g.extent().size(), UVec2::ZERO);

    // A side past what an i32 holds stops there, and makes no chunk it does
    // not write
    let mut g = TileGrid::new(UVec2::splat(u32::MAX));
    let far = IVec2::splat(i32::MAX - 1);
    assert!(g.set(far, t));
    assert!(!g.set(IVec2::MAX, t));
    assert_eq!(g.get(far), t);
    assert_eq!(g.fill(far - IVec2::ONE, IVec2::MAX, t), 3);
    assert_eq!(
      g.chunk_coords().collect::<Vec<_>>(),
      [TileGrid::chunk_of(far)]
    );

    // An unbounded grid takes the cells at both ends of i32
    let mut g = TileGrid::unbounded();
    assert!(g.set(IVec2::MIN, t));
    assert!(g.set(IVec2::MAX, t));
    assert_eq!(g.fill(IVec2::MAX - IVec2::ONE, IVec2::MAX, t), 3);
    assert_eq!((g.get(IVec2::MIN), g.get(IVec2::MAX)), (t, t));
    assert_eq!(g.extent().min, IVec2::MIN);
    assert_eq!(g.fill(IVec2::MIN, IVec2::MAX, None), 5);
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
    let chunk = IVec2::new(1, 0);
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
    assert_eq!(h.chunk_version(IVec2::ZERO), g.chunk_version(IVec2::ZERO));
    g.set(IVec2::new(2, 2), Some(Tile::new(0, 2)));
    h.set(IVec2::new(2, 2), Some(Tile::new(0, 3)));
    assert_ne!(g.chunk_version(IVec2::ZERO), h.chunk_version(IVec2::ZERO));
    assert_ne!(g.version(), h.version());
    // Each grid's versions still only move on
    let before = g.chunk_version(IVec2::ZERO);
    g.set(IVec2::new(3, 3), Some(Tile::new(0, 4)));
    assert!(g.chunk_version(IVec2::ZERO) > before);
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
        let chunk = IVec2::new(x, y);
        assert_ne!(a.chunk_version(chunk), b.chunk_version(chunk), "{chunk}");
      }
    }
    assert_ne!(a.version(), b.version());
  }

  #[test]
  fn untouched_chunks_keep_their_version() {
    let mut g = TileGrid::new(UVec2::new(100, 40));
    g.set(IVec2::new(70, 3), Some(Tile::new(0, 1)));
    let far = g.chunk_version(IVec2::new(2, 0));
    let other = g.chunk_version(IVec2::new(0, 0));
    g.set(IVec2::new(40, 3), Some(Tile::new(0, 1)));
    g.fill(IVec2::new(32, 0), IVec2::new(60, 5), Some(Tile::new(0, 2)));
    assert_eq!(g.chunk_version(IVec2::new(2, 0)), far);
    assert_eq!(g.chunk_version(IVec2::new(0, 0)), other);
    assert_ne!(g.chunk_version(IVec2::new(1, 0)), 0);
    assert_eq!(g.chunk_version(IVec2::new(9, 9)), 0);
    // draining the dirty marks is not a change
    let v = g.version();
    let mut out = Vec::new();
    g.drain_dirty(&mut out);
    assert_eq!(g.version(), v);
  }

  #[test]
  fn assigning_a_grid_moves_only_the_chunks_whose_cells_differ() {
    let chunks = [
      IVec2::new(0, 0),
      IVec2::new(1, 0),
      IVec2::new(0, 1),
      IVec2::new(1, 1),
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
    assert_eq!(out, [IVec2::new(1, 0), IVec2::new(1, 1)]);

    // The same cells again change nothing, unless a chunk is named
    assert!(!g.assign(&saved, |_| false));
    assert_eq!(versions(&g), after);
    assert!(g.assign(&saved, |c| c == IVec2::new(0, 1)));
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
    let (kept, before) = (g.chunk_version(IVec2::ZERO), g.version());
    assert!(g.touch(&[IVec2::new(1, 0), IVec2::new(1, 1), IVec2::new(5, 5)]));
    let moved = g.chunk_version(IVec2::new(1, 0));
    assert!(moved > before);
    assert_eq!(g.chunk_version(IVec2::new(1, 1)), moved, "one change");
    assert_eq!((g.chunk_version(IVec2::ZERO), g.version()), (kept, moved));
    assert_eq!(format!("{g:?}"), cells, "the cells as they were");
    g.drain_dirty(&mut out);
    assert_eq!(out, [IVec2::new(1, 0), IVec2::new(1, 1)]);
    // Nothing inside: nothing moves
    assert!(!g.touch(&[IVec2::new(2, 0), IVec2::new(-1, 0)]));
    assert_eq!(g.version(), moved);
  }

  #[test]
  fn an_unbounded_grid_takes_any_cell_and_makes_its_chunk() {
    let mut g = TileGrid::unbounded();
    assert!(g.set(IVec2::new(-33, -1), Some(Tile::new(0, 5))));
    assert_eq!(g.get(IVec2::new(-33, -1)), Some(Tile::new(0, 5)));
    assert_eq!(
      g.chunk_coords().collect::<Vec<_>>(),
      vec![IVec2::new(-2, -1)]
    );
    assert_eq!(
      g.extent(),
      CellRect::new(IVec2::new(-64, -32), IVec2::new(-32, 0))
    );
    assert_eq!(g.bounds(), None);
  }

  #[test]
  fn clearing_an_absent_cell_makes_no_chunk() {
    let mut g = TileGrid::unbounded();
    assert!(!g.set(IVec2::new(100, 100), None));
    assert_eq!(g.chunk_coords().count(), 0);
  }

  #[test]
  fn a_bounded_grid_refuses_cells_outside_it_as_before() {
    let mut g = TileGrid::new(UVec2::new(40, 20));
    assert!(!g.set(IVec2::new(-1, 0), Some(Tile::new(0, 1))));
    assert!(!g.set(IVec2::new(40, 0), Some(Tile::new(0, 1))));
    assert!(g.set(IVec2::new(39, 19), Some(Tile::new(0, 1))));
    assert_eq!(g.bounds(), Some(CellRect::from_size(UVec2::new(40, 20))));
  }

  #[test]
  fn chunk_of_floors_toward_negative_infinity() {
    assert_eq!(TileGrid::chunk_of(IVec2::new(-1, 0)), IVec2::new(-1, 0));
    assert_eq!(TileGrid::chunk_of(IVec2::new(-32, 31)), IVec2::new(-1, 0));
    assert_eq!(TileGrid::chunk_of(IVec2::new(-33, 32)), IVec2::new(-2, 1));
  }

  #[test]
  fn an_unbounded_fill_makes_the_chunks_it_writes_and_clears_only_what_exists() {
    let mut g = TileGrid::unbounded();
    assert_eq!(
      g.fill(
        IVec2::new(-40, -2),
        IVec2::new(-30, 1),
        Some(Tile::new(0, 2))
      ),
      11 * 4
    );
    assert_eq!(g.chunk_coords().count(), 4); // chunks (-2,-1) (-1,-1) (-2,0) (-1,0)
    assert_eq!(
      g.fill(IVec2::new(-1000, -1000), IVec2::new(1000, 1000), None),
      11 * 4
    );
    assert_eq!(g.chunk_coords().count(), 4); // clearing leaves the chunks; only a streaming unload removes one
  }

  #[test]
  fn an_unbounded_fill_making_more_chunks_than_a_map_holds_is_refused() {
    // A map holds 2^27 cells, 131,072 chunks: a fill making more is
    // refused before anything is made, where every `i32` cell was an
    // out-of-memory abort. The test lowers the cap to 4 chunks, so were
    // the guard gone it would make 5 chunks (20 KB), not gigabytes
    assert_eq!(MAX_FILL_CHUNKS, 131_072);
    FILL_CAP.with(|cap| cap.set(4));
    let mut g = TileGrid::unbounded();
    let tile = Some(Tile::new(0, 2));
    let five = IVec2::new(5 * 32 - 1, 0);
    assert_eq!(g.fill(IVec2::ZERO, five, tile), 0, "5 chunks");
    let square = IVec2::new(-33, -33);
    assert_eq!(g.fill(square, IVec2::ZERO, tile), 0, "3 x 3 chunks");
    assert_eq!(g.chunk_coords().count(), 0, "nothing made");
    assert_eq!(g.version(), 0, "nothing changed");
    // A fill within the cap writes as before; the chunks it has count for
    // nothing against the next
    assert_eq!(g.fill(IVec2::new(-40, 0), IVec2::new(40, 0), tile), 81);
    assert_eq!(g.chunk_coords().count(), 4, "chunks -2 to 1");
    assert_eq!(g.missing_chunks(IVec2::new(-64, 0), five), 3);
    assert_eq!(g.fill(IVec2::new(-64, 0), five, None), 81);
    assert_eq!(
      g.fill(IVec2::new(-64, 1), IVec2::new(5 * 32 - 1, 1), tile),
      224
    );
    // Clearing any range reads only the chunks there are
    assert_eq!(g.fill(IVec2::MIN, IVec2::MAX, None), 224);
    assert_eq!(g.chunk_coords().count(), 7);
    FILL_CAP.with(|cap| cap.set(MAX_FILL_CHUNKS));
  }

  #[test]
  fn drain_dirty_and_versions_work_on_signed_chunks() {
    let mut g = TileGrid::unbounded();
    g.set(IVec2::new(-1, -1), Some(Tile::new(0, 1)));
    g.set(IVec2::new(32, 0), Some(Tile::new(0, 1)));
    let mut out = Vec::new();
    g.drain_dirty(&mut out);
    assert_eq!(out, vec![IVec2::new(-1, -1), IVec2::new(1, 0)]); // row order
    assert!(g.chunk_version(IVec2::new(-1, -1)) > 0);
    assert_eq!(g.chunk_version(IVec2::new(5, 5)), 0);
  }

  #[test]
  fn assign_diffs_two_unbounded_grids_by_their_chunk_sets() {
    // a has chunks A (unchanged in b) and B (dropped in b); b adds C
    let (in_a, in_b, in_c) = (IVec2::new(-5, -5), IVec2::new(40, -5), IVec2::new(-100, 70));
    let [chunk_a, chunk_b, chunk_c] = [in_a, in_b, in_c].map(TileGrid::chunk_of);
    assert_eq!(
      [chunk_a, chunk_b, chunk_c],
      [IVec2::new(-1, -1), IVec2::new(1, -1), IVec2::new(-4, 2)]
    );
    let mut a = TileGrid::unbounded();
    a.set(in_a, Some(Tile::new(0, 1)));
    a.fill(
      IVec2::new(32, -32),
      IVec2::new(63, -1),
      Some(Tile::new(0, 2)),
    );
    let mut b = TileGrid::unbounded();
    b.set(in_a, Some(Tile::new(0, 1)));
    b.set(in_c, Some(Tile::new(0, 3)));
    b.set(in_c + IVec2::X, Some(Tile::new(0, 4)));
    let mut out = Vec::new();
    a.drain_dirty(&mut out);
    let (kept, dropped) = (a.chunk_version(chunk_a), a.chunk_version(chunk_b));

    assert!(a.assign(&b, |_| false));
    // A: as it was
    assert_eq!(a.chunk_version(chunk_a), kept);
    assert_eq!(a.get(in_a), Some(Tile::new(0, 1)));
    // B: every cell empty, with a new version
    let moved = a.chunk_version(chunk_b);
    assert!(moved > dropped);
    for y in -32..0 {
      for x in 32..64 {
        assert_eq!(a.get(IVec2::new(x, y)), None, "({x}, {y})");
      }
    }
    // C: present with b's cells, and the same new version: one change
    assert!(a.chunk_coords().any(|c| c == chunk_c));
    assert_eq!(a.get(in_c), Some(Tile::new(0, 3)));
    assert_eq!(a.get(in_c + IVec2::X), Some(Tile::new(0, 4)));
    assert_eq!(a.chunk_version(chunk_c), moved);
    assert_eq!(a.version(), moved);
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    a.drain_dirty(&mut out);
    assert_eq!(out, [chunk_b, chunk_c]);

    // The same chunks again change nothing, and a bounded grid is not
    // taken by an unbounded one
    assert!(!a.assign(&b, |_| false));
    assert!(!a.assign(&TileGrid::new(UVec2::splat(64)), |_| true));
    assert_eq!(a.version(), moved);
  }

  #[test]
  fn chunk_cells_hold_a_chunk_row_by_row() {
    let mut g = TileGrid::unbounded();
    g.set(IVec2::new(-31, -2), Some(Tile::new(0, 5)));
    let cells = g
      .chunk_cells(IVec2::new(-1, -1))
      .expect("the chunk written");
    // (-31, -2) is column 1, row 30 of chunk (-1, -1)
    assert_eq!(
      TileGrid::decode(cells[30 * CHUNK_SIZE as usize + 1]),
      Some(Tile::new(0, 5))
    );
    assert_eq!(cells.iter().filter(|&&c| c != 0).count(), 1);
    assert!(g.chunk_cells(IVec2::ZERO).is_none());
  }

  #[test]
  fn chunks_within_a_range_come_in_row_order_however_they_are_found() {
    let mut g = TileGrid::unbounded();
    let t = Some(Tile::new(0, 1));
    for cell in [(-40, -40), (0, 0), (40, 0), (-40, 40), (5000, 3000)] {
      g.set(IVec2::new(cell.0, cell.1), t);
    }
    let all: Vec<IVec2> = g.chunk_coords().collect();
    let inside = |first: IVec2, last: IVec2| -> Vec<IVec2> {
      let wanted = |c: &IVec2| c.cmpge(first).all() && c.cmple(last).all();
      all.iter().copied().filter(wanted).collect()
    };
    // A small range is walked, a wide one read from the list: both alike
    for (first, last) in [
      (IVec2::new(-2, -2), IVec2::new(1, 1)),
      (IVec2::new(0, 0), IVec2::new(0, 0)),
      (
        IVec2::new(-1_000_000, -1_000_000),
        IVec2::new(1_000_000, 1_000_000),
      ),
      (IVec2::new(-2, -1_000_000), IVec2::new(1, 1_000_000)),
      (
        IVec2::new(i32::MIN, i32::MIN),
        IVec2::new(i32::MAX, i32::MAX),
      ),
      (IVec2::new(3, 3), IVec2::new(2, 2)),
    ] {
      let found: Vec<IVec2> = g.chunks_within(first, last).collect();
      assert_eq!(found, inside(first, last), "{first} to {last}");
    }
  }

  #[test]
  fn written_rows_land_in_their_chunks_as_one_change_making_only_chunks_with_tiles() {
    // A block of 40 x 3 from (-20, -2), reaching chunks -1 and 0 both
    // ways, with tiles in three of those four chunks
    let (width, rows) = (40usize, 3usize);
    let tile = |id: u32| TileGrid::encode(Some(Tile::new(0, id))).expect("fits");
    let mut cells = vec![0u32; width * rows];
    cells[0] = tile(1); // (-20, -2)
    cells[width - 1] = tile(2); // (19, -2)
    cells[2 * width] = tile(3); // (-20, 0)
    let mut g = TileGrid::unbounded();
    let before = g.version();
    assert!(g.write_rows(IVec2::new(-20, -2), width, &cells));
    assert_eq!(g.get(IVec2::new(-20, -2)), Some(Tile::new(0, 1)));
    assert_eq!(g.get(IVec2::new(19, -2)), Some(Tile::new(0, 2)));
    assert_eq!(g.get(IVec2::new(-20, 0)), Some(Tile::new(0, 3)));
    assert_eq!(g.get(IVec2::new(0, -1)), None);
    // chunk (0, 0) gets only empty cells: it is not made
    let chunks: Vec<IVec2> = g.chunk_coords().collect();
    assert_eq!(
      chunks,
      [IVec2::new(-1, -1), IVec2::new(0, -1), IVec2::new(-1, 0)]
    );
    // one change, one version, every chunk it wrote dirty
    let version = g.version();
    assert!(version > before);
    for &chunk in &chunks {
      assert_eq!(g.chunk_version(chunk), version);
    }
    let mut dirty = Vec::new();
    g.drain_dirty(&mut dirty);
    assert_eq!(dirty, chunks);
    // the same rows again change nothing; empty rows clear what is there
    assert!(!g.write_rows(IVec2::new(-20, -2), width, &cells));
    assert_eq!(g.version(), version);
    assert!(g.write_rows(IVec2::new(-20, -2), width, &vec![0; width]));
    assert_eq!(g.get(IVec2::new(-20, -2)), None);
    assert_eq!(g.get(IVec2::new(-20, 0)), Some(Tile::new(0, 3)));
    assert!(g.chunk_version(IVec2::new(-1, 0)) < g.version());
    // a short last row is left out, and so is a block of no row
    assert!(!g.write_rows(IVec2::new(100, 0), 4, &[tile(1); 3]));
    assert!(!g.write_rows(IVec2::ZERO, 0, &cells));
    assert!(!g.has_chunk(IVec2::new(3, 0)));
  }

  #[test]
  fn written_rows_keep_to_a_bounded_grid_and_to_what_i32_holds() {
    let t = TileGrid::encode(Some(Tile::new(0, 4))).expect("fits");
    let mut g = TileGrid::new(UVec2::new(10, 4));
    // a 4 x 4 block from (8, 2): only (8..10, 2..4) is the grid's
    assert!(g.write_rows(IVec2::new(8, 2), 4, &[t; 16]));
    let held: Vec<IVec2> = (0..4)
      .flat_map(|y| (0..10).map(move |x| IVec2::new(x, y)))
      .filter(|&cell| g.get(cell).is_some())
      .collect();
    assert_eq!(
      held,
      [
        IVec2::new(8, 2),
        IVec2::new(9, 2),
        IVec2::new(8, 3),
        IVec2::new(9, 3)
      ]
    );
    assert!(
      !g.write_rows(IVec2::new(-8, -8), 4, &[t; 16]),
      "all outside"
    );
    // an unbounded grid writes up to the last cell `i32` holds, no further
    let mut far = TileGrid::unbounded();
    let edge = IVec2::splat(i32::MAX - 1);
    assert!(far.write_rows(edge, 4, &[t; 16]));
    assert_eq!(far.get(edge), Some(Tile::new(0, 4)));
    assert_eq!(far.get(IVec2::splat(i32::MAX)), Some(Tile::new(0, 4)));
    assert_eq!(far.chunk_coords().count(), 1);
    let near = IVec2::splat(i32::MIN);
    assert!(far.write_rows(near, 2, &[t; 4]));
    assert_eq!(far.get(near + IVec2::ONE), Some(Tile::new(0, 4)));
  }

  #[test]
  fn a_clone_is_a_grid_of_its_own() {
    let mut g = TileGrid::new(UVec2::splat(40));
    g.set(IVec2::new(3, 3), Some(Tile::new(0, 1)));
    let copy = g.clone();
    assert_ne!(copy.id(), g.id());
    assert_eq!(copy.version(), g.version());
    assert_eq!(
      copy.chunk_version(IVec2::ZERO),
      g.chunk_version(IVec2::ZERO)
    );
    assert_eq!(format!("{copy:?}"), format!("{g:?}"));
    let versions: Vec<(IVec2, u64)> = g.chunk_versions().collect();
    assert_eq!(versions, [(IVec2::ZERO, g.version())]);
  }
}
