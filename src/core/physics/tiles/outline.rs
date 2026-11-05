//! A tile grid's outlines kept between steps, so an edit retraces only the
//! loops it touches.
//!
//! Points are on a grid of `GRID` points a tile, so authored shapes join
//! the outlines exactly. The boundary is worked out in units that depend
//! on one or two cells each: a cell's edges inside it (slope diagonals,
//! shape edges off its sides), and each border between two cells (the outer
//! ones against nothing). On a border each side's edges are split at the
//! ends of both cells' edges there, and a piece both cells run (each its own
//! way) cancels, leaving the pieces only one runs: two half blocks side by
//! side, or a half block beside a solid cell, are one outline. Each point
//! lists the boundary edges leaving it and the loop each belongs to. A loop
//! goes on from an edge by the edge leaving its end with the sharpest turn
//! (exact, in integers): the other side of the filled wedge the edge bounds
//! there. That depends on the point's edges alone, so a loop is the same
//! whatever order the loops are traced in and wherever each starts, and a
//! loop closes when its next edge would be its first. An edit changes only
//! the units of its cells (each cell's inside and its four borders): the
//! loops through a point whose edges changed are dropped (all of them where
//! loops touch) and their edges and the new ones traced again; every other
//! loop keeps its id and its part. Where the edit touched one loop and it
//! stays one, the retrace keeps its id and copies its old corners up to
//! each point whose edges changed, looking up only those points (`splice`):
//! the rest of the loop goes on as before, since a point's edges alone
//! decide it. Walking every edge of a long loop through the map of points,
//! about a cache miss a step on a big map once a frame has passed, cost
//! several times the edit's own work (a one-cell serpentine over four
//! chunks: about 0.5 ms).
//!
//! A hexagonal grid traces no loops here: its units are its cells' six
//! sides, each owned by whichever of its two cells comes first in row
//! order, a side with one solid cell an edge clockwise round it
//! (`HexSides`, `hex_owned_edges`, `each_hex_owner`), and its colliders
//! hold those edges as they are (`pack::HexParts`), as `Boundary` reads them.

use glam::{I64Vec2, IVec2};

use super::hex::{HexCells, StaggerAxis};
use super::{CELLS, CHUNK, Cells, Few, GRID, MAX_REACH, Point, TileCell, chunk_of, local, sides};
use crate::core::ecs::Entity;
use crate::core::hash::FastMap;

pub(super) fn key((x, y): Point) -> u64 {
  (u64::from(x as u32) << 32) | u64::from(y as u32)
}

/// The step from `a` to `b`, in i64: any two points of the lattice
pub(super) fn delta(a: Point, b: Point) -> (i64, i64) {
  (
    i64::from(b.0) - i64::from(a.0),
    i64::from(b.1) - i64::from(a.1),
  )
}

pub(super) fn cross(u: (i64, i64), v: (i64, i64)) -> i64 {
  u.0 * v.1 - u.1 * v.0
}

fn dot(u: (i64, i64), v: (i64, i64)) -> i64 {
  u.0 * v.0 + u.1 * v.1
}

/// Whether turning from `heading` to `a` is sharper than to `b`: further
/// round clockwise on screen, back the way it came (180°) the sharpest, a
/// turn just short of 180° the other way the least.
fn sharper(heading: (i64, i64), a: (i64, i64), b: (i64, i64)) -> bool {
  // Turns of 0 (exclusive) to 180° (inclusive) are on the sharp half
  let sharp_half = |d: (i64, i64)| {
    let (c, t) = (cross(heading, d), dot(heading, d));
    c > 0 || (c == 0 && t < 0)
  };
  match (sharp_half(a), sharp_half(b)) {
    (true, false) => true,
    (false, true) => false,
    _ => cross(b, a) > 0,
  }
}

/// The loop id of an edge waiting to be traced.
const UNTRACED: u32 = u32::MAX;
/// The mark of a new edge a splice has taken, before its loop has an id;
/// read as untraced, as `UNTRACED` is, by a whole trace after a splice that
/// gave up.
const TAKEN: u32 = u32::MAX - 1;

/// A boundary edge leaving a point: its step (an edge lies within one cell,
/// so a step is at most `GRID` either way) and its loop.
#[derive(Debug, Clone, Copy, Default)]
struct Out {
  dx: i16,
  dy: i16,
  id: u32,
}

impl Out {
  fn step(&self) -> (i64, i64) {
    (i64::from(self.dx), i64::from(self.dy))
  }

  fn end(&self, from: Point) -> Point {
    (from.0 + i32::from(self.dx), from.1 + i32::from(self.dy))
  }
}

/// A part of the boundary that one or two cells decide: an edit to a cell
/// changes only its own units (its inside and its four borders).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Unit {
  /// A cell's edges inside it
  Inside(i32, i32),
  /// The border on the left of a cell
  Left(i32, i32),
  /// The border above a cell
  Above(i32, i32),
}

/// The units of the cells `changed` (each cell's inside and its four
/// borders), each once, in the units' order (by kind, then x, then y). The
/// cells are sorted by column once, and each kind's units are then those
/// cells or two shifted copies merged, so a big edit (a streamed chunk's
/// thousand cells) never sorts five units a cell
fn units_of(changed: &[(IVec2, TileCell)]) -> Vec<Unit> {
  let cells = by_column(changed);
  let mut units = Vec::with_capacity(cells.len() * 4);
  units.extend(cells.iter().map(|&(x, y)| Unit::Inside(x, y)));
  // Each of a kind's two copies is sorted as the cells are (a shift keeps
  // the order), so merging them sorts the kind
  let mut merged = |shift: (i32, i32), unit: fn(i32, i32) -> Unit| {
    let shifted = |&(x, y): &(i32, i32)| (x + shift.0, y + shift.1);
    let (mut i, mut j) = (0, 0);
    while i < cells.len() || j < cells.len() {
      let (a, b) = (cells.get(i).copied(), cells.get(j).map(shifted));
      let next = match (a, b) {
        (Some(a), Some(b)) if a == b => {
          (i, j) = (i + 1, j + 1);
          a
        }
        (Some(a), Some(b)) if a < b => {
          i += 1;
          a
        }
        (Some(a), None) => {
          i += 1;
          a
        }
        (_, Some(b)) => {
          j += 1;
          b
        }
        (None, None) => break,
      };
      units.push(unit(next.0, next.1));
    }
  };
  merged((1, 0), Unit::Left);
  merged((0, 1), Unit::Above);
  units
}

/// The cells `changed` (in row order: by row, then column) by column, then
/// row. Over a few columns for their number (a streamed chunk's thousand
/// cells over 32, a fill's), counted into their columns, which keeps each
/// column's rows in order; else sorted
fn by_column(changed: &[(IVec2, TileCell)]) -> Vec<(i32, i32)> {
  let (low, high) = changed
    .iter()
    .fold((i32::MAX, i32::MIN), |(low, high), &(cell, _)| {
      (low.min(cell.x), high.max(cell.x))
    });
  let columns = i64::from(high) - i64::from(low) + 1;
  if changed.len() < 64 || columns > (changed.len() as i64) * 4 {
    // The sign bit flipped: signed order as unsigned
    let flip = |v: i32| u64::from(v.cast_unsigned() ^ 0x8000_0000);
    let unflip = |v: u64| ((v as u32) ^ 0x8000_0000).cast_signed();
    let mut keys: Vec<u64> = changed
      .iter()
      .map(|&(cell, _)| (flip(cell.x) << 32) | flip(cell.y))
      .collect();
    keys.sort_unstable();
    return keys
      .into_iter()
      .map(|key| (unflip(key >> 32), unflip(key)))
      .collect();
  }
  // Each column's first place, then each cell put at its column's next
  let column = |cell: IVec2| (i64::from(cell.x) - i64::from(low)) as usize;
  let mut start = vec![0usize; columns as usize + 1];
  for &(cell, _) in changed {
    start[column(cell) + 1] += 1;
  }
  for i in 1..start.len() {
    start[i] += start[i - 1];
  }
  let mut cells = vec![(0, 0); changed.len()];
  for &(cell, _) in changed {
    let place = &mut start[column(cell)];
    cells[*place] = (cell.x, cell.y);
    *place += 1;
  }
  cells
}

/// The values an edit's cells had before it, by cell: a window for each
/// chunk holding changed cells, when those chunks are few for how many
/// cells changed (a streamed chunk's thousand cells, a fill, several
/// chunks landing and unloading in one step), so each read is an index,
/// else the list itself, searched (cells scattered over many chunks)
struct Before<'c> {
  changed: &'c [(IVec2, TileCell)],
  /// Each chunk holding changed cells, by row, with its cells' values
  /// before (`None` for a cell not changed)
  chunks: Vec<(IVec2, Box<[Option<TileCell>]>)>,
  /// The chunk last read: reads come in runs within a chunk
  last: std::cell::Cell<usize>,
}

impl<'c> Before<'c> {
  /// The cells the windows may hold for each changed cell, past a floor
  const SPREAD: usize = 8;
  const FLOOR: usize = 4096;

  /// `changed` sorted by row, then column, each once with its value before
  fn new(changed: &'c [(IVec2, TileCell)]) -> Self {
    let row = |chunk: &IVec2| (chunk.y, chunk.x);
    let mut keys: Vec<IVec2> = changed.iter().map(|&(cell, _)| chunk_of(cell)).collect();
    keys.sort_unstable_by_key(row);
    keys.dedup();
    let mut chunks = Vec::new();
    let room = (changed.len() * Self::SPREAD).max(Self::FLOOR);
    if keys.len().saturating_mul(CELLS) <= room {
      chunks = keys
        .into_iter()
        .map(|chunk| (chunk, vec![None; CELLS].into_boxed_slice()))
        .collect();
      for &(cell, was) in changed {
        let chunk = chunk_of(cell);
        if let Ok(i) = chunks.binary_search_by_key(&row(&chunk), |(c, _)| row(c)) {
          chunks[i].1[local(cell)] = Some(was);
        }
      }
    }
    Self {
      changed,
      chunks,
      last: std::cell::Cell::new(0),
    }
  }

  /// What (`x`, `y`) held before the edit, if the edit changed it
  fn get(&self, x: i32, y: i32) -> Option<TileCell> {
    if self.chunks.is_empty() {
      let i = self
        .changed
        .binary_search_by_key(&(y, x), |&(cell, _)| (cell.y, cell.x))
        .ok()?;
      return Some(self.changed[i].1);
    }
    let cell = IVec2::new(x, y);
    let chunk = chunk_of(cell);
    let mut i = self.last.get();
    if self.chunks.get(i).is_none_or(|(c, _)| *c != chunk) {
      i = self
        .chunks
        .binary_search_by_key(&(chunk.y, chunk.x), |(c, _)| (c.y, c.x))
        .ok()?;
      self.last.set(i);
    }
    self.chunks[i].1[local(cell)]
  }
}

/// The intervals a cell's edges run along one of its sides, in the side's
/// own way (`sides`), from its edges in cell points.
fn along(edges: &[(Point, Point)], side: usize) -> Vec<(i32, i32)> {
  edges
    .iter()
    .filter_map(|&edge| sides::along(edge, side))
    .collect()
}

/// How many intervals of `cover` hold `lo..hi` (one or none, from a shape
/// `add_shape` made).
fn covered(cover: &[(i32, i32)], lo: i32, hi: i32) -> usize {
  cover.iter().filter(|&&(l, h)| l <= lo && hi <= h).count()
}

/// The pieces of a border one of its cells runs and the other does not:
/// `first` the cell before it (left of or above it), `second` the one after.
/// Calls `emit(lo, hi, first's)` for each, in border units from its start.
/// A piece run more often one way than the other is emitted that many
/// times more: what cancels is a pair run each way, so every point keeps as
/// many edges in as out, whatever edges a shape gave.
fn border(first: &[(i32, i32)], second: &[(i32, i32)], mut emit: impl FnMut(i32, i32, bool)) {
  let mut cuts: Vec<i32> = first
    .iter()
    .chain(second)
    .flat_map(|&(lo, hi)| [lo, hi])
    .collect();
  cuts.sort_unstable();
  cuts.dedup();
  for cut in cuts.windows(2) {
    let (lo, hi) = (cut[0], cut[1]);
    let (firsts, seconds) = (covered(first, lo, hi), covered(second, lo, hi));
    for _ in seconds..firsts {
      emit(lo, hi, true);
    }
    for _ in firsts..seconds {
      emit(lo, hi, false);
    }
  }
}

/// The boundary edges of `unit`, the cells read through `at`, pushed onto `out`.
pub(super) fn unit_edges(
  unit: Unit,
  cells: &Cells,
  at: &impl Fn(i32, i32) -> TileCell,
  out: &mut Vec<(Point, Point)>,
) {
  let g = GRID;
  match unit {
    Unit::Inside(x, y) => {
      let origin = (x * g, y * g);
      out.extend(
        cells
          .edges(at(x, y))
          .iter()
          .filter(|&&edge| sides::inside(edge))
          .map(|&(a, b)| (sides::plus(origin, a), sides::plus(origin, b))),
      );
    }
    Unit::Left(x, y) => {
      let (left, right) = (at(x - 1, y), at(x, y));
      let (x0, y0) = (x * g, y * g);
      if !matches!(left, TileCell::Shape(_)) && !matches!(right, TileCell::Shape(_)) {
        // Whole sides or none
        match (
          sides::full(left, sides::RIGHT),
          sides::full(right, sides::LEFT),
        ) {
          (true, false) => out.push(((x0, y0), (x0, y0 + g))),
          (false, true) => out.push(((x0, y0 + g), (x0, y0))),
          _ => {}
        }
        return;
      }
      let first = along(cells.edges(left), sides::RIGHT);
      let second = along(cells.edges(right), sides::LEFT);
      border(&first, &second, |lo, hi, firsts| {
        // The left cell's run down its right side, the right cell's up its left
        out.push(if firsts {
          ((x0, y0 + lo), (x0, y0 + hi))
        } else {
          ((x0, y0 + hi), (x0, y0 + lo))
        })
      });
    }
    Unit::Above(x, y) => {
      let (above, below) = (at(x, y - 1), at(x, y));
      let (x0, y0) = (x * g, y * g);
      if !matches!(above, TileCell::Shape(_)) && !matches!(below, TileCell::Shape(_)) {
        match (
          sides::full(above, sides::BOTTOM),
          sides::full(below, sides::TOP),
        ) {
          (true, false) => out.push(((x0 + g, y0), (x0, y0))),
          (false, true) => out.push(((x0, y0), (x0 + g, y0))),
          _ => {}
        }
        return;
      }
      let first = along(cells.edges(above), sides::BOTTOM);
      let second = along(cells.edges(below), sides::TOP);
      border(&first, &second, |lo, hi, firsts| {
        // The upper cell's run left along its bottom, the lower's right along its top
        out.push(if firsts {
          ((x0 + hi, y0), (x0 + lo, y0))
        } else {
          ((x0 + lo, y0), (x0 + hi, y0))
        })
      });
    }
  }
}

/// Every boundary edge of the grid, unit by unit, cell by cell in row order
/// (by row, then column: each cell's inside, the border left of it, the
/// border above it), in the order a scan of every cell meets them. Only the
/// units that can have an edge are read: those of the chunks there are, the
/// row below each (its bottom border) and the column right of each (its
/// right border). Each chunk's units read its own cells, the chunk's left
/// of it and the chunk's above it without a lookup a cell. Units are read
/// within the lattice's reach, and a border past the farthest cells, so no
/// point leaves an `i32`.
pub(super) fn boundary_edges(cells: Cells) -> Vec<(Point, Point)> {
  let mut edges = Vec::new();
  let chunks = cells.chunk_order();
  let in_row = |row: i32| {
    let start = chunks.partition_point(|c| c.y < row);
    let end = chunks.partition_point(|c| c.y <= row);
    &chunks[start..end]
  };
  // Each row of chunks, and the row below it
  let mut rows: Vec<i32> = chunks.iter().flat_map(|c| [c.y, c.y + 1]).collect();
  rows.sort_unstable();
  rows.dedup();
  let (lo, hi) = (-MAX_REACH, MAX_REACH + 1);
  for row in rows {
    let (now, above) = (in_row(row), in_row(row - 1));
    // The row's top line reads the chunks above too (their bottom borders)
    let mut columns: Vec<i32> = now.iter().chain(above).map(|c| c.x).collect();
    columns.sort_unstable();
    columns.dedup();
    let top_line: Vec<_> = columns
      .into_iter()
      .map(|x| (x, cells.near(IVec2::new(x, row))))
      .collect();
    let lines: Vec<_> = now.iter().map(|&c| (c.x, cells.near(c))).collect();
    let top = row * CHUNK;
    for y in top.max(lo)..=(top + CHUNK - 1).min(hi) {
      let scan = if y == top { &top_line } else { &lines };
      for (i, (column, at)) in scan.iter().enumerate() {
        let left = column * CHUNK;
        for x in left.max(lo)..=(left + CHUNK - 1).min(hi) {
          unit_edges(Unit::Inside(x, y), &cells, at, &mut edges);
          unit_edges(Unit::Left(x, y), &cells, at, &mut edges);
          unit_edges(Unit::Above(x, y), &cells, at, &mut edges);
        }
        // The border right of the chunk, where the next column is not read
        let right = left + CHUNK;
        if right <= hi && scan.get(i + 1).is_none_or(|(next, _)| *next != column + 1) {
          unit_edges(Unit::Left(right, y), &cells, at, &mut edges);
        }
      }
    }
  }
  edges
}

/// A hexagonal grid's sides as steps between cells: the step to the cell
/// across each side, and whether a cell owns that side (the cell across
/// comes after it in row order: a later row, or the same row further
/// right), by the parity of the cell's line. Every cell of a line steps
/// alike, so two tables hold every cell's.
#[derive(Clone, Copy, Debug)]
pub(super) struct HexSides {
  pub(super) hex: HexCells,
  across: [[IVec2; 6]; 2],
  owns: [[bool; 6]; 2],
}

impl HexSides {
  pub(super) fn of(hex: HexCells) -> Self {
    let mut across = [[IVec2::ZERO; 6]; 2];
    let mut owns = [[false; 6]; 2];
    for parity in 0..2 {
      let probe = match hex.axis() {
        StaggerAxis::Y => IVec2::new(0, parity as i32),
        StaggerAxis::X => IVec2::new(parity as i32, 0),
      };
      for side in 0..6 {
        let step = hex.across(probe, side) - probe;
        across[parity][side] = step;
        owns[parity][side] = (step.y, step.x) > (0, 0);
      }
    }
    Self { hex, across, owns }
  }

  /// The table `cell`'s line reads
  fn parity(&self, cell: IVec2) -> usize {
    let line = match self.hex.axis() {
      StaggerAxis::Y => cell.y,
      StaggerAxis::X => cell.x,
    };
    line.rem_euclid(2) as usize
  }

  /// The cell across `cell`'s side `side`, and whether `cell` owns it (one
  /// cell from it, so within an `i32` for a cell a grid holds)
  pub(super) fn across(&self, cell: IVec2, side: usize) -> (IVec2, bool) {
    let parity = self.parity(cell);
    (cell + self.across[parity][side], self.owns[parity][side])
  }
}

/// The edge along side `side` of a cell whose corners are `corners` (from
/// the grid's origin), where only one of the cell and the cell across is
/// solid (`solid`: the cell itself): clockwise round the solid one, in
/// points from `from`. None for a side of length 0, or one whose points
/// pass an `i32` (never a side of a cell that reaches).
pub(super) fn hex_side_edge(
  corners: &[I64Vec2; 6],
  side: usize,
  solid: bool,
  from: I64Vec2,
) -> Option<(Point, Point)> {
  let (a, b) = (corners[side], corners[(side + 1) % 6]);
  if a == b {
    return None;
  }
  let point = |p: I64Vec2| {
    let p = p.saturating_sub(from);
    Some((i32::try_from(p.x).ok()?, i32::try_from(p.y).ok()?))
  };
  let (a, b) = (point(a)?, point(b)?);
  Some(if solid { (a, b) } else { (b, a) })
}

/// The boundary edges along the sides `cell` owns (each side with one solid
/// cell, read through `solid`), in side order, pushed onto `out`: its units
pub(super) fn hex_owned_edges(
  sides: &HexSides,
  cell: IVec2,
  solid: &impl Fn(i32, i32) -> bool,
  out: &mut Vec<(Point, Point)>,
) {
  let mine = solid(cell.x, cell.y);
  let mut corners = None;
  for side in 0..6 {
    let (across, owned) = sides.across(cell, side);
    if !owned || solid(across.x, across.y) == mine {
      continue;
    }
    let corners = corners.get_or_insert_with(|| sides.hex.corners(cell));
    out.extend(hex_side_edge(corners, side, mine, I64Vec2::ZERO));
  }
}

/// Calls `f` with every cell of a hexagonal grid that can own a side with
/// one solid cell, in row order (by row, then column): each chunk's cells,
/// and the cells just outside it that own sides of its cells, the column
/// either side of it on its rows and the row above it, a cell past it each
/// way (the owner of a side being its earlier cell)
pub(super) fn each_hex_owner(cells: Cells, mut f: impl FnMut(IVec2)) {
  let chunks = cells.chunk_order();
  let in_row = |row: i32| {
    let start = chunks.partition_point(|c| c.y < row);
    let end = chunks.partition_point(|c| c.y <= row);
    &chunks[start..end]
  };
  // Each row of chunks, and the row above it (its last line only)
  let mut rows: Vec<i32> = chunks.iter().flat_map(|c| [c.y - 1, c.y]).collect();
  rows.sort_unstable();
  rows.dedup();
  let mut spans: Vec<(i32, i32)> = Vec::new();
  for row in rows {
    let (now, below) = (in_row(row), in_row(row + 1));
    let last = row * CHUNK + CHUNK - 1;
    let first = if now.is_empty() { last } else { row * CHUNK };
    for y in first..=last {
      // The owners on this line: each chunk's cells and one either side,
      // and on the row's last line those over the next row's chunks
      let over = if y == last { below } else { &[] };
      spans.clear();
      spans.extend(
        now
          .iter()
          .chain(over)
          .map(|c| (c.x * CHUNK - 1, c.x * CHUNK + CHUNK)),
      );
      spans.sort_unstable();
      let mut i = 0;
      while i < spans.len() {
        let (lo, mut hi) = spans[i];
        i += 1;
        while i < spans.len() && spans[i].0 <= hi + 1 {
          hi = hi.max(spans[i].1);
          i += 1;
        }
        for x in lo..=hi {
          f(IVec2::new(x, y));
        }
      }
    }
  }
}

/// One outline loop.
#[derive(Debug)]
pub(crate) struct Outline {
  /// Its points, edge by edge, the last repeating the first
  pub(crate) corners: Vec<Point>,
  /// The collider built from it
  pub(crate) part: Option<Entity>,
}

/// What an edit changed: the parts of the loops (or runs) it dropped, and
/// the ids of the ones it traced.
#[derive(Debug, Default)]
pub(crate) struct Retraced {
  pub(crate) dropped: Vec<Option<Entity>>,
  pub(crate) traced: Vec<u32>,
}

/// The outlines of a grid's solid, slope and shape cells, kept between steps.
#[derive(Debug, Default)]
pub(crate) struct Outlines {
  /// Every point with boundary edges leaving it, by `key`
  leaving: FastMap<u64, Few<Out>>,
  /// The loops by id; a dropped loop's slot is empty until reused
  loops: Vec<Option<Outline>>,
  free: Vec<u32>,
  /// What the edits did, for the tests to see the splice taken
  #[cfg(test)]
  pub(crate) splices: Splices,
}

/// How the edits' retraces went (tests)
#[cfg(test)]
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Splices {
  /// Spliced, the loops as many after as before
  pub(crate) kept: usize,
  /// Spliced, more loops after (a split)
  pub(crate) splits: usize,
  /// Spliced, fewer loops after (a join)
  pub(crate) joins: usize,
  /// Traced whole with loops dropped (the splice gave up)
  pub(crate) whole: usize,
  /// Loops dropped whole before the splice, all their edges gone
  pub(crate) gone: usize,
}

#[cfg(test)]
impl Splices {
  /// An edit that dropped `dropped` loops and made `made`
  fn count(&mut self, made: usize, dropped: usize) {
    match made.cmp(&dropped) {
      std::cmp::Ordering::Equal => self.kept += 1,
      std::cmp::Ordering::Greater => self.splits += 1,
      std::cmp::Ordering::Less => self.joins += 1,
    }
  }
}

/// An edge, from its start to its end
type Edge = (Point, Point);

/// Where a splice took a loop's edge from: an old loop's (by its place among
/// the dropped loops, and the edge's place in it), or the edit's new edges
#[derive(Clone, Copy, PartialEq, Eq)]
enum Took {
  Old(usize, usize),
  New,
}

/// A run of a loop a splice made: edges `from..to` of old loop `k` (by its
/// place among the dropped loops), or one of the edit's new edges
#[derive(Clone, Copy)]
enum Piece {
  Old { k: usize, from: usize, to: usize },
  New(Point, Point),
}

/// What a splice picks at a point whose edges changed
#[derive(Clone, Copy)]
enum Pick {
  /// The loop's first edge: it closes
  Close,
  Old(usize, usize),
  New,
}

impl Outlines {
  /// Every loop of the grid, traced whole.
  pub(crate) fn build(cells: Cells) -> Self {
    let edges = boundary_edges(cells);
    let mut outlines = Self::default();
    for &(a, b) in &edges {
      outlines.insert(a, b);
    }
    outlines.trace(&edges, &[], edges.len());
    outlines
  }

  /// Take in the cells at `changed` (in row order, each with the value it
  /// had when the outlines were last brought up to date; every one within
  /// `MAX_REACH`, as a grid holds no other): the loops through a point
  /// whose edges changed are dropped and traced again with the new edges.
  pub(crate) fn apply(&mut self, cells: Cells, changed: &[(IVec2, TileCell)]) -> Retraced {
    let mut retraced = Retraced::default();
    if changed.is_empty() {
      return retraced;
    }
    // Units come by column, so the reads keep to a chunk for runs of them
    let now = cells.cached();
    let prior = Before::new(changed);
    let before = |x: i32, y: i32| prior.get(x, y).unwrap_or_else(|| now(x, y));
    // An edit changes the units of its cells only
    let units = units_of(changed);
    let (mut was, mut is) = (Vec::new(), Vec::new());
    let mut changes: Vec<(Point, Point, bool)> = Vec::new();
    for unit in units {
      was.clear();
      is.clear();
      unit_edges(unit, &cells, &before, &mut was);
      unit_edges(unit, &cells, &now, &mut is);
      if was == is {
        continue;
      }
      was.sort_unstable();
      is.sort_unstable();
      // What went and what came, as many times as each
      let (mut i, mut j) = (0, 0);
      while i < was.len() || j < is.len() {
        match (was.get(i), is.get(j)) {
          (Some(a), Some(b)) if a == b => (i, j) = (i + 1, j + 1),
          (Some(&(a, b)), Some(later)) if (a, b) < *later => {
            changes.push((a, b, false));
            i += 1;
          }
          (Some(&(a, b)), None) => {
            changes.push((a, b, false));
            i += 1;
          }
          (_, Some(&(a, b))) => {
            changes.push((a, b, true));
            j += 1;
          }
          (None, None) => break,
        }
      }
    }
    if changes.is_empty() {
      return retraced;
    }
    let mut dropped: Vec<u32> = Vec::new();
    for &(a, b, _) in &changes {
      for point in [a, b] {
        if let Some(leaving) = self.leaving.get(&key(point)) {
          dropped.extend(leaving.as_slice().iter().map(|out| out.id));
        }
      }
    }
    dropped.sort_unstable();
    dropped.dedup();
    dropped.retain(|&id| id != UNTRACED);
    // The dropped loops' edges keep their ids, which count as untraced
    // until the trace takes them (cheaper than marking each one)
    let mut olds: Vec<Vec<Point>> = Vec::with_capacity(dropped.len());
    let mut old_ids: Vec<u32> = Vec::with_capacity(dropped.len());
    for &id in &dropped {
      if let Some(outline) = self.loops.get_mut(id as usize).and_then(Option::take) {
        retraced.dropped.push(outline.part);
        olds.push(outline.corners);
        old_ids.push(id);
      }
    }
    // Every edge to trace: the dropped loops' (the removed ones were among
    // them) and the new ones
    let mut count: usize = olds.iter().map(|old| old.len().saturating_sub(1)).sum();
    let mut dirty: Vec<Point> = Vec::with_capacity(changes.len() * 2);
    let (mut new_edges, mut removed) = (Vec::new(), Vec::new());
    // How many of each dropped loop's edges went
    let mut gone = vec![0; olds.len()];
    for (a, b, now) in changes {
      dirty.extend([a, b]);
      if now {
        self.insert(a, b);
        new_edges.push((a, b));
        count += 1;
      } else if let Some(id) = self.remove(a, b) {
        removed.push((a, b, id));
        count = count.saturating_sub(1);
        if let Ok(k) = old_ids.binary_search(&id) {
          gone[k] += 1;
        }
      }
    }
    // A loop all of whose edges went (an island under a chunk unloaded, a
    // block cleared) leaves nothing to trace: it is dropped, its id free
    #[cfg(test)]
    let dropped_loops = olds.len();
    if (0..olds.len()).any(|k| gone[k] + 1 == olds[k].len()) {
      let whole = |k: usize| gone[k] + 1 == olds[k].len();
      let left: Vec<usize> = (0..olds.len()).filter(|&k| !whole(k)).collect();
      removed.retain(|&(_, _, id)| old_ids.binary_search(&id).map_or(true, |k| !whole(k)));
      #[cfg(test)]
      {
        self.splices.gone += olds.len() - left.len();
      }
      olds = left.iter().map(|&k| std::mem::take(&mut olds[k])).collect();
      old_ids = left.iter().map(|&k| old_ids[k]).collect();
    }
    let removed: Vec<Edge> = removed.into_iter().map(|(a, b, _)| (a, b)).collect();
    // Sorted, for the splice to look a point up in
    dirty.sort_unstable();
    dirty.dedup();
    // The loops the edit touched, spliced: unchanged stretches copied
    if let Some(made) = self.splice(&old_ids, &olds, (&dirty, &removed, &new_edges), count) {
      #[cfg(test)]
      {
        self.splices.count(made.len(), dropped_loops);
      }
      for (id, corners) in made {
        self.loops[id as usize] = Some(Outline {
          corners,
          part: None,
        });
        retraced.traced.push(id);
      }
      // The dropped ids no new loop kept are free
      let kept = &retraced.traced;
      self
        .free
        .extend(dropped.iter().copied().filter(|id| !kept.contains(id)));
      return retraced;
    }
    #[cfg(test)]
    if !olds.is_empty() {
      self.splices.whole += 1;
    }
    let mut starts: Vec<(Point, Point)> = Vec::with_capacity(count);
    for old in &olds {
      starts.extend(old.windows(2).map(|edge| (edge[0], edge[1])));
    }
    starts.extend(new_edges);
    retraced.traced = self.trace(&starts, &dropped, count);
    // Every loop it dropped was gone whole: the new edges, if any, traced
    // as the splice would have
    #[cfg(test)]
    if olds.is_empty() && dropped_loops > 0 {
      self.splices.count(retraced.traced.len(), dropped_loops);
    }
    // Their ids are free once the trace no longer reads them
    self.free.extend(dropped);
    retraced
  }

  /// Retraces the loops an edit touched (`ids`, whose corners were
  /// `olds`) after its changed edges, ending at the `dirty` points (sorted,
  /// each once), went into `leaving` (the `new` ones added, the `removed`
  /// ones taken out):
  /// the loops they now make, `count` edges in all, each with its id. Each
  /// loop starts at the first edge left in the order a whole trace takes
  /// them (the old loops' edges in order, then the new ones), so it starts
  /// where a whole trace would. The stretches between dirty points are
  /// copied from `olds`, as a point's edges alone decide where a loop goes
  /// on from it, and only the dirty points are looked up. Each loop keeps
  /// the id of the old loop it holds the most edges of (the first come, the
  /// first served; a loop holding none, or whose old ones are taken, gets a
  /// free id), and only the edges of the other old loops in it, and its new
  /// edges, are given its id. `None` for an edit that dropped no loop (its
  /// new edges are a new loop, which a trace makes as cheaply), and where
  /// the walk does not come out as loops of `count` edges: a whole trace
  /// sorts that out, reading the new edges this marked `TAKEN` as untraced.
  fn splice(
    &mut self,
    ids: &[u32],
    olds: &[Vec<Point>],
    (dirty, removed, new): (&[Point], &[Edge], &[Edge]),
    count: usize,
  ) -> Option<Vec<(u32, Vec<Point>)>> {
    if olds.is_empty() || dirty.is_empty() {
      return None;
    }
    let sizes: Vec<usize> = olds
      .iter()
      .map(|old| old.len().checked_sub(1).filter(|&n| n > 0))
      .collect::<Option<_>>()?;
    let (mut low, mut high) = (dirty[0], dirty[0]);
    for &(x, y) in dirty {
      low = (low.0.min(x), low.1.min(y));
      high = (high.0.max(x), high.1.max(y));
    }
    let is_dirty = |p: Point| {
      p.0 >= low.0
        && p.1 >= low.1
        && p.0 <= high.0
        && p.1 <= high.1
        && dirty.binary_search(&p).is_ok()
    };
    let edge = |k: usize, j: usize| (olds[k][j], olds[k][j + 1]);
    // Each old loop's edges that leave a dirty corner, by place
    let dirty_at: Vec<Vec<usize>> = olds
      .iter()
      .zip(&sizes)
      .map(|(old, &n)| (0..n).filter(|&j| is_dirty(old[j])).collect())
      .collect();
    // An edge twice at a dirty point (a broken shape's doubled edges) is
    // left to the whole trace: which copy the edit removed is not known
    let mut at_points: Vec<Edge> = new.to_vec();
    for (k, places) in dirty_at.iter().enumerate() {
      at_points.extend(places.iter().map(|&j| edge(k, j)));
    }
    at_points.sort_unstable();
    if at_points.windows(2).any(|pair| pair[0] == pair[1]) {
      return None;
    }
    // So each old edge at a dirty point is there once: found by its ends
    let mut old_at: Vec<(Edge, usize, usize)> = dirty_at
      .iter()
      .enumerate()
      .flat_map(|(k, places)| places.iter().map(move |&j| (k, j)))
      .map(|(k, j)| (edge(k, j), k, j))
      .collect();
    old_at.sort_unstable_by_key(|&(edge, _, _)| edge);
    let mut taken: Vec<Vec<bool>> = sizes.iter().map(|&n| vec![false; n]).collect();
    // The old edge leaving dirty point `at` by `step`, not yet taken (its
    // end is within an `i32`, as every point of an old loop is)
    let find = |taken: &[Vec<bool>], at: Point, step: (i64, i64)| {
      let end = (
        i32::try_from(i64::from(at.0) + step.0).ok()?,
        i32::try_from(i64::from(at.1) + step.1).ok()?,
      );
      let i = old_at
        .binary_search_by_key(&(at, end), |&(edge, _, _)| edge)
        .ok()?;
      let (_, k, j) = old_at[i];
      (!taken[k][j]).then_some((k, j))
    };
    // The removed edges were old ones at dirty points: none to take
    for &(a, b) in removed {
      let (k, j) = find(&taken, a, delta(a, b))?;
      taken[k][j] = true;
    }
    let mut made: Vec<(Vec<Point>, Vec<Piece>)> = Vec::new();
    let mut total = 0;
    // Where the search for the next loop's first edge goes on from
    let (mut next_k, mut next_j, mut next_new) = (0, 0, 0);
    while total < count {
      // The next loop's first edge: the first old edge left, in order, else
      // the first new edge left
      let mut first: Option<(Point, Point, Took)> = None;
      while next_k < olds.len() && first.is_none() {
        if next_j >= sizes[next_k] {
          (next_k, next_j) = (next_k + 1, 0);
          continue;
        }
        if !taken[next_k][next_j] {
          let (a, b) = edge(next_k, next_j);
          first = Some((a, b, Took::Old(next_k, next_j)));
        } else {
          next_j += 1;
        }
      }
      while first.is_none() && next_new < new.len() {
        let (a, b) = new[next_new];
        next_new += 1;
        let step = delta(a, b);
        let left = self.leaving.get(&key(a)).is_some_and(|outs| {
          outs
            .as_slice()
            .iter()
            .any(|out| out.id == UNTRACED && out.step() == step)
        });
        if left {
          first = Some((a, b, Took::New));
        }
      }
      let (start, second, took) = first?;
      let first_step = delta(start, second);
      let mut pieces = Vec::new();
      match took {
        Took::Old(k, j) => {
          taken[k][j] = true;
          pieces.push(Piece::Old {
            k,
            from: j,
            to: j + 1,
          });
        }
        Took::New => {
          let outs = self.leaving.get_mut(&key(start))?.as_mut_slice();
          let out = outs
            .iter_mut()
            .find(|out| out.id == UNTRACED && out.step() == first_step)?;
          out.id = TAKEN;
          pieces.push(Piece::New(start, second));
        }
      }
      let mut corners = Vec::with_capacity(count - total + 1);
      corners.extend([start, second]);
      let mut length = 1;
      let (mut at, mut heading, mut came) = (second, first_step, took);
      'walk: loop {
        if total + length > count {
          return None;
        }
        if !is_dirty(at) {
          // An old edge's end the edit left alone: the old loop goes on,
          // copied up to its next edge leaving a dirty corner (or its
          // first edge, closing)
          let Took::Old(k, j) = came else {
            // A new edge ends at a dirty point
            return None;
          };
          let n = sizes[k];
          let mut from = if j + 1 == n { 0 } else { j + 1 };
          let mut wrapped = false;
          loop {
            let places = &dirty_at[k];
            let next_dirty = places.get(places.partition_point(|&d| d < from)).copied();
            // The first edge closes the loop here only from a clean
            // corner: at a dirty one the pick decides, as another edge
            // leaving it may be the sharper turn
            let closes = match took {
              Took::Old(first_k, first_j)
                if first_k == k && first_j >= from && !is_dirty(start) =>
              {
                Some(first_j)
              }
              _ => None,
            };
            let to = next_dirty.unwrap_or(n).min(closes.unwrap_or(n));
            if to > from {
              let run = &mut taken[k][from..to];
              if run.iter().any(|&taken| taken) {
                return None;
              }
              run.fill(true);
              corners.extend_from_slice(&olds[k][from + 1..=to]);
              pieces.push(Piece::Old { k, from, to });
              length += to - from;
              if total + length > count {
                return None;
              }
            }
            if closes == Some(to) {
              break 'walk;
            }
            if to == n {
              // Round to the old loop's first corner
              if took == Took::Old(k, 0) && !is_dirty(start) {
                break 'walk;
              }
              if places.first() == Some(&0) {
                (at, came) = (olds[k][n], Took::Old(k, n - 1));
                heading = delta(olds[k][n - 1], olds[k][n]);
                continue 'walk;
              }
              if std::mem::replace(&mut wrapped, true) {
                return None;
              }
              from = 0;
              continue;
            }
            // At a dirty corner
            (at, came) = (olds[k][to], Took::Old(k, to - 1));
            heading = delta(olds[k][to - 1], olds[k][to]);
            continue 'walk;
          }
        }
        // A dirty point: pick as the trace would, the sharpest turn among
        // the edges leaving it not yet taken (the first, to close)
        let outs = self.leaving.get_mut(&key(at))?.as_mut_slice();
        let mut pick: Option<(usize, Pick)> = None;
        for (i, out) in outs.iter().enumerate() {
          let step = out.step();
          let open = if at == start && step == first_step {
            Some(Pick::Close)
          } else if out.id == UNTRACED {
            Some(Pick::New)
          } else if out.id == TAKEN {
            None
          } else if ids.contains(&out.id) {
            find(&taken, at, step).map(|(k, j)| Pick::Old(k, j))
          } else {
            // A loop the edit did not drop, at a point it changed
            return None;
          };
          if let Some(open) = open
            && pick.is_none_or(|(p, _)| sharper(heading, step, outs[p].step()))
          {
            pick = Some((i, open));
          }
        }
        match pick? {
          (_, Pick::Close) => break,
          (_, Pick::Old(k, j)) => {
            taken[k][j] = true;
            let (a, b) = edge(k, j);
            (at, heading, came) = (b, delta(a, b), Took::Old(k, j));
            pieces.push(Piece::Old {
              k,
              from: j,
              to: j + 1,
            });
          }
          (i, Pick::New) => {
            outs[i].id = TAKEN;
            let from = at;
            heading = outs[i].step();
            at = outs[i].end(at);
            came = Took::New;
            pieces.push(Piece::New(from, at));
          }
        }
        corners.push(at);
        length += 1;
      }
      total += length;
      made.push((corners, pieces));
    }
    if total != count {
      return None;
    }
    // Ids: each loop the old one it holds the most edges of, the loops
    // holding the most first; the others free ones
    let held = |pieces: &[Piece], k: usize| -> usize {
      pieces
        .iter()
        .map(|piece| match *piece {
          Piece::Old { k: kk, from, to } if kk == k => to - from,
          _ => 0,
        })
        .sum()
    };
    let mut order: Vec<(usize, usize, usize)> = made
      .iter()
      .enumerate()
      .map(|(m, (_, pieces))| {
        let (k, n) = (0..olds.len())
          .map(|k| (k, held(pieces, k)))
          .max_by_key(|&(_, n)| n)
          .unwrap_or((0, 0));
        (n, m, k)
      })
      .collect();
    order.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut loop_ids: Vec<Option<u32>> = vec![None; made.len()];
    let mut used = vec![false; olds.len()];
    for &(n, m, k) in &order {
      if n > 0 && !used[k] {
        used[k] = true;
        loop_ids[m] = Some(ids[k]);
      }
    }
    let mut out = Vec::with_capacity(made.len());
    for (m, (corners, pieces)) in made.into_iter().enumerate() {
      let id = match loop_ids[m] {
        Some(id) => id,
        None => self.free.pop().unwrap_or_else(|| {
          self.loops.push(None);
          (self.loops.len() - 1) as u32
        }),
      };
      // Its edges from other old loops, and its new ones, take its id
      let mut give = |(a, b): Edge, from: u32| {
        let step = delta(a, b);
        let out = self.leaving.get_mut(&key(a)).and_then(|outs| {
          outs
            .as_mut_slice()
            .iter_mut()
            .find(|out| out.id == from && out.step() == step)
        });
        match out {
          Some(out) => out.id = id,
          None => debug_assert!(false, "an edge a splice took is gone: {a:?} to {b:?}"),
        }
      };
      for piece in pieces {
        match piece {
          Piece::Old { k, .. } if ids[k] == id => {}
          Piece::Old { k, from, to } => {
            for j in from..to {
              give(edge(k, j), ids[k]);
            }
          }
          Piece::New(a, b) => give((a, b), TAKEN),
        }
      }
      out.push((id, corners));
    }
    Some(out)
  }

  /// Every loop and its id.
  pub(crate) fn loops(&self) -> impl Iterator<Item = (u32, &Outline)> {
    self
      .loops
      .iter()
      .enumerate()
      .filter_map(|(id, outline)| outline.as_ref().map(|o| (id as u32, o)))
  }

  pub(crate) fn get(&self, id: u32) -> Option<&Outline> {
    self.loops.get(id as usize).and_then(Option::as_ref)
  }

  pub(crate) fn get_mut(&mut self, id: u32) -> Option<&mut Outline> {
    self.loops.get_mut(id as usize).and_then(Option::as_mut)
  }

  /// Add an edge. The edges are a multiset: a shape `add_shape` made never
  /// gives an edge twice (its polygons are simple and do not overlap, and
  /// its edges along a side run that side's way, so no unit's edges meet
  /// another's), but an edge here twice is traced twice rather than lost.
  fn insert(&mut self, a: Point, b: Point) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    self.leaving.entry(key(a)).or_default().push(Out {
      dx: dx as i16,
      dy: dy as i16,
      id: UNTRACED,
    });
  }

  /// Take one copy of an edge away: the id of the loop it was in, none
  /// when there is no copy.
  fn remove(&mut self, a: Point, b: Point) -> Option<u32> {
    let step = delta(a, b);
    let leaving = self.leaving.get_mut(&key(a))?;
    let i = leaving
      .as_slice()
      .iter()
      .position(|out| out.step() == step)?;
    let id = leaving.as_slice()[i].id;
    leaving.swap_remove(i);
    if leaving.is_empty() {
      self.leaving.remove(&key(a));
    }
    Some(id)
  }

  /// Trace the loop through each of `starts` not yet traced (an edge is
  /// untraced while its id is `UNTRACED` or one of `dropped`, sorted),
  /// stopping once `count` edges are traced (all the untraced ones); returns
  /// their ids.
  fn trace(&mut self, starts: &[(Point, Point)], dropped: &[u32], count: usize) -> Vec<u32> {
    let untraced = |id: u32| id == UNTRACED || id == TAKEN || dropped.binary_search(&id).is_ok();
    let mut traced = Vec::new();
    let mut left = count;
    for &(start, second) in starts {
      // A one-tile edit's new loop is usually all of them: the rest of the
      // starts need not be looked up
      if left == 0 {
        break;
      }
      let first = delta(start, second);
      let Some(leaving) = self.leaving.get_mut(&key(start)) else {
        continue;
      };
      let Some(out) = leaving
        .as_mut_slice()
        .iter_mut()
        .find(|out| out.step() == first && untraced(out.id))
      else {
        continue;
      };
      let id = self.free.pop().unwrap_or(self.loops.len() as u32);
      out.id = id;
      let mut corners = vec![start, second];
      let (mut at, mut heading) = (second, first);
      // Each step takes an untraced edge, so this ends; it closes when the
      // edge taken would be the first one again
      while let Some(leaving) = self.leaving.get_mut(&key(at)) {
        let outs = leaving.as_mut_slice();
        let open = |out: &Out| untraced(out.id) || (at == start && out.step() == first);
        let mut pick: Option<usize> = None;
        for (i, out) in outs.iter().enumerate() {
          if open(out) && pick.is_none_or(|p| sharper(heading, out.step(), outs[p].step())) {
            pick = Some(i);
          }
        }
        // Every point has as many edges in as out (cells' and shapes'
        // edges are closed rings and a border cancels pairs), among the
        // untraced edges too, so a loop can only end back at its start
        let Some(pick) = pick else {
          debug_assert!(false, "an outline ends at {at:?}");
          break;
        };
        let out = outs[pick];
        if at == start && out.step() == first {
          break;
        }
        outs[pick].id = id;
        heading = out.step();
        at = out.end(at);
        corners.push(at);
      }
      left = left.saturating_sub(corners.len() - 1);
      let outline = Some(Outline {
        corners,
        part: None,
      });
      match self.loops.get_mut(id as usize) {
        Some(slot) => *slot = outline,
        None => self.loops.push(outline),
      }
      traced.push(id);
    }
    traced
  }
}

/// The closed outlines around every region of solid and slope cells (holes
/// too) of a grid `width` by `height` from its cells row by row, straight
/// runs merged, in tiles: each is a loop of corners whose last repeats its
/// first.
#[cfg(test)]
pub(crate) fn outlines(width: u32, height: u32, cells: &[TileCell]) -> Vec<Vec<super::Corner>> {
  let grid = super::TileColliders::from_cells(width, height, glam::Vec2::ONE, cells.to_vec());
  traced_outlines(grid.cells_view())
}

/// The outlines of `cells` traced whole, as `outlines` gives them.
#[cfg(test)]
pub(crate) fn traced_outlines(cells: Cells) -> Vec<Vec<super::Corner>> {
  Outlines::build(cells)
    .loops()
    .map(|(_, outline)| {
      super::merge_straight_runs(&outline.corners)
        .into_iter()
        .map(|(x, y)| (x.div_euclid(GRID), y.div_euclid(GRID)))
        .collect()
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::super::tests::xy;
  use super::super::{
    CellShape, GRID, Point, ShapeBuild, SlopeCorner, TileColliders, merge_straight_runs,
  };
  use super::*;
  use crate::core::ecs::Vec2;
  use TileCell::{Empty as E, OneWay as O, Solid as S};
  use std::collections::BTreeSet;

  type Edges = BTreeSet<(Point, Point)>;

  /// Loops as sets of directed edges, sorted: equal for the same outlines
  /// whatever order they were traced in and wherever each starts.
  fn canonical(loops: impl IntoIterator<Item = Vec<Point>>) -> Vec<Edges> {
    let mut sets: Vec<Edges> = loops
      .into_iter()
      .map(|l| l.windows(2).map(|w| (w[0], w[1])).collect())
      .collect();
    sets.sort();
    sets
  }

  fn traced(state: &Outlines) -> Vec<Edges> {
    canonical(state.loops().map(|(_, o)| merge_straight_runs(&o.corners)))
  }

  /// A closed loop through points given in tiles, as edges in grid units.
  fn expect(points: &[(f32, f32)]) -> Edges {
    let at = |&(x, y): &(f32, f32)| {
      (
        (x * GRID as f32).round() as i32,
        (y * GRID as f32).round() as i32,
      )
    };
    let n = points.len();
    (0..n)
      .map(|i| (at(&points[i]), at(&points[(i + 1) % n])))
      .collect()
  }

  fn add(tiles: &mut TileColliders, points: &[(f32, f32)], one_way: bool) -> TileCell {
    TileCell::Shape(tiles.add_shape(CellShape {
      polygons: vec![points.iter().map(|&(x, y)| Vec2::new(x, y)).collect()],
      one_way,
    }))
  }

  /// Sets cells, applies them to `state` as the rebuild does and compares it
  /// with a full trace.
  fn edit(state: &mut Outlines, tiles: &mut TileColliders, edits: &[(u32, u32, TileCell)]) {
    for &(x, y, cell) in edits {
      tiles.set(xy(x, y), cell);
    }
    let changed = tiles.take_changes().expect("listed");
    state.apply(tiles.cells_view(), &changed);
    assert_eq!(
      traced(state),
      traced(&Outlines::build(tiles.cells_view())),
      "after {edits:?}"
    );
  }

  /// The loops checked against the cells without the tracer: their edges
  /// are the boundary edges, each once, and each step is the sharpest turn
  /// among the edges leaving its corner (angles in floating point), the
  /// last one leading back to the first.
  fn assert_traced_by_the_rule(state: &Outlines, tiles: &TileColliders) {
    let boundary = boundary_edges(tiles.cells_view());
    let mut leaving: std::collections::BTreeMap<Point, Vec<Point>> = Default::default();
    for &(a, b) in &boundary {
      leaving.entry(a).or_default().push(b);
    }
    let mut seen: Vec<(Point, Point)> = Vec::new();
    for (_, outline) in state.loops() {
      let c = &outline.corners;
      assert_eq!(c.first(), c.last(), "closed");
      let steps: Vec<(Point, Point)> = c.windows(2).map(|w| (w[0], w[1])).collect();
      for (i, &(a, b)) in steps.iter().enumerate() {
        let next = steps[(i + 1) % steps.len()];
        let heading = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
        let turn = |to: Point| {
          let d = ((to.0 - b.0) as f64, (to.1 - b.1) as f64);
          (heading.0 * d.1 - heading.1 * d.0).atan2(heading.0 * d.0 + heading.1 * d.1)
        };
        let sharpest = leaving[&b]
          .iter()
          .copied()
          .max_by(|&p, &q| turn(p).total_cmp(&turn(q)));
        assert_eq!(sharpest, Some(next.1), "the step after {a:?} -> {b:?}");
      }
      seen.extend(steps);
    }
    let mut boundary = boundary;
    boundary.sort();
    seen.sort();
    assert_eq!(seen, boundary, "the loops' edges are the boundary's");
  }

  /// Whole chunks loaded and unloaded under an unbounded grid's outlines,
  /// as a stream lands and unloads them, with edits at chunk sides between
  /// and steps unloading a chunk and loading its neighbour together:
  /// the outlines stay a full trace's, the islands under a chunk unloaded
  /// dropped whole and the loops crossing its sides spliced
  #[test]
  fn chunks_loaded_and_unloaded_whole_keep_a_full_trace() {
    use SlopeCorner::{BottomLeft, TopRight};
    use TileCell::Slope;
    let mut tiles = TileColliders::unbounded(Vec2::ONE);
    let half = add(
      &mut tiles,
      &[(0.0, 0.5), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)],
      false,
    );
    let diamond = add(
      &mut tiles,
      &[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)],
      false,
    );
    let kinds = [
      S,
      S,
      S,
      S,
      S,
      S,
      S,
      S,
      S,
      E,
      O,
      Slope(BottomLeft),
      Slope(TopRight),
      half,
      diamond,
    ];
    let mut seed = 0x51ed_270b_u32;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    let side = CHUNK;
    // A chunk loaded whole: about half its cells filled, most solid
    let load = |tiles: &mut TileColliders, chunk: IVec2, next: &mut dyn FnMut(u32) -> u32| {
      for y in 0..side {
        for x in 0..side {
          if next(20) < 9 {
            let kind = kinds[next(kinds.len() as u32) as usize];
            tiles.set(chunk * side + IVec2::new(x, y), kind);
          }
        }
      }
    };
    // Chunks of a 4 x 3 area, signed, three in four loaded to start
    let area: Vec<IVec2> = (-1..2)
      .flat_map(|y| (-2..2).map(move |x| IVec2::new(x, y)))
      .collect();
    let mut loaded: Vec<IVec2> = Vec::new();
    for &chunk in &area {
      if next(4) > 0 {
        load(&mut tiles, chunk, &mut next);
        loaded.push(chunk);
      }
    }
    tiles.take_changes();
    let mut state = Outlines::build(tiles.cells_view());
    let mut together = 0;
    for round in 0..150 {
      let chunk = area[next(area.len() as u32) as usize];
      if next(4) == 0 {
        // A chunk unloaded, a neighbour loaded (afresh, if it was) and
        // their shared side edited on either side, in one apply: a step
        // landing and unloading side by side
        let dir = [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y][next(4) as usize];
        let neighbour = chunk + dir;
        for gone in [chunk, neighbour] {
          if let Some(i) = loaded.iter().position(|&c| c == gone) {
            tiles.unload_chunk(gone);
            loaded.swap_remove(i);
          }
        }
        load(&mut tiles, neighbour, &mut next);
        loaded.push(neighbour);
        // A cell `depth` from a chunk's side facing `facing`
        let beside = |facing: IVec2, along: i32, depth: i32| match (facing.x, facing.y) {
          (1, _) => IVec2::new(side - 1 - depth, along),
          (-1, _) => IVec2::new(depth, along),
          (_, 1) => IVec2::new(along, side - 1 - depth),
          _ => IVec2::new(along, depth),
        };
        for _ in 0..1 + next(6) {
          let (along, depth) = (next(side as u32) as i32, next(2) as i32);
          let (at, facing) = if next(2) == 0 {
            (neighbour, -dir)
          } else {
            (chunk, dir)
          };
          let kind = kinds[next(kinds.len() as u32) as usize];
          tiles.set(at * side + beside(facing, along, depth), kind);
          if at == chunk && !loaded.contains(&chunk) {
            loaded.push(chunk);
          }
        }
        together += 1;
      } else {
        match loaded.iter().position(|&c| c == chunk) {
          Some(i) if next(3) > 0 => {
            tiles.unload_chunk(chunk);
            loaded.swap_remove(i);
          }
          Some(_) => {
            // Cells at its sides edited
            for _ in 0..1 + next(6) {
              let along = next(side as u32) as i32;
              let at = match next(4) {
                0 => IVec2::new(0, along),
                1 => IVec2::new(side - 1, along),
                2 => IVec2::new(along, 0),
                _ => IVec2::new(along, side - 1),
              };
              let kind = kinds[next(kinds.len() as u32) as usize];
              tiles.set(chunk * side + at, kind);
            }
          }
          None => {
            load(&mut tiles, chunk, &mut next);
            loaded.push(chunk);
          }
        }
      }
      let changed = tiles.take_changes().expect("listed");
      state.apply(tiles.cells_view(), &changed);
      assert_eq!(
        traced(&state),
        traced(&Outlines::build(tiles.cells_view())),
        "round {round}, chunk {chunk}"
      );
      if round % 30 == 29 {
        assert_traced_by_the_rule(&state, &tiles);
      }
    }
    let s = state.splices;
    assert!(s.gone > 100 && s.kept + s.splits + s.joins > 50, "{s:?}");
    assert_eq!(s.whole, 0, "{s:?}");
    assert!(
      together > 25,
      "{together} steps unloading and loading side by side"
    );
  }

  #[test]
  fn shape_edges_cancel_against_solid_and_shape_neighbours() {
    use SlopeCorner::BottomRight;
    let bottom = [(0.0, 0.5), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)];
    let band = [(0.0, 0.25), (1.0, 0.25), (1.0, 0.75), (0.0, 0.75)];
    let loops = |cells: &[TileCell], tiles: &mut TileColliders| {
      for (x, &cell) in cells.iter().enumerate() {
        tiles.set(xy(x as u32, 0), cell);
      }
      traced(&Outlines::build(tiles.cells_view()))
    };
    // A half block on each side of a solid cell: one loop
    let mut tiles = TileColliders::new(3, 1, Vec2::ONE);
    let half = add(&mut tiles, &bottom, false);
    assert_eq!(
      loops(&[half, S, half], &mut tiles),
      [expect(&[
        (0.0, 0.5),
        (1.0, 0.5),
        (1.0, 0.0),
        (2.0, 0.0),
        (2.0, 0.5),
        (3.0, 0.5),
        (3.0, 1.0),
        (0.0, 1.0)
      ])]
    );
    // Two half blocks: one loop, its top one edge
    let mut tiles = TileColliders::new(2, 1, Vec2::ONE);
    let half = add(&mut tiles, &bottom, false);
    assert_eq!(
      loops(&[half, half], &mut tiles),
      [expect(&[(0.0, 0.5), (2.0, 0.5), (2.0, 1.0), (0.0, 1.0)])]
    );
    // A slope's full side against a half block's half side
    let mut tiles = TileColliders::new(2, 1, Vec2::ONE);
    let half = add(&mut tiles, &bottom, false);
    assert_eq!(
      loops(&[TileCell::Slope(BottomRight), half], &mut tiles),
      [expect(&[
        (0.0, 1.0),
        (1.0, 0.0),
        (1.0, 0.5),
        (2.0, 0.5),
        (2.0, 1.0)
      ])]
    );
    // Shapes overlapping on their shared side in part: split there and cancelled
    let mut tiles = TileColliders::new(2, 1, Vec2::ONE);
    let (band, half) = (
      add(&mut tiles, &band, false),
      add(&mut tiles, &bottom, false),
    );
    assert_eq!(
      loops(&[band, half], &mut tiles),
      [expect(&[
        (0.0, 0.25),
        (1.0, 0.25),
        (1.0, 0.5),
        (2.0, 0.5),
        (2.0, 1.0),
        (1.0, 1.0),
        (1.0, 0.75),
        (0.0, 0.75)
      ])]
    );
    // A shape's polygons touching along an edge inside the cell: one loop
    let mut tiles = TileColliders::new(1, 1, Vec2::ONE);
    let l = TileCell::Shape(tiles.add_shape(CellShape {
      polygons: vec![
        vec![
          Vec2::ZERO,
          Vec2::new(0.5, 0.0),
          Vec2::new(0.5, 1.0),
          Vec2::Y,
        ],
        vec![
          Vec2::new(0.5, 0.5),
          Vec2::new(1.0, 0.5),
          Vec2::ONE,
          Vec2::new(0.5, 1.0),
        ],
      ],
      one_way: false,
    }));
    assert_eq!(
      loops(&[l], &mut tiles),
      [expect(&[
        (0.0, 0.0),
        (0.5, 0.0),
        (0.5, 0.5),
        (1.0, 0.5),
        (1.0, 1.0),
        (0.0, 1.0)
      ])]
    );
  }

  #[test]
  fn an_edit_that_keeps_one_loop_keeps_its_id_and_copies_the_rest() {
    let (w, h) = (40, 8);
    let mut tiles = TileColliders::new(w, h, Vec2::ONE);
    for x in 0..w {
      tiles.set(xy(x, 6), S);
      tiles.set(xy(x, 7), S);
    }
    tiles.take_changes();
    let mut state = Outlines::build(tiles.cells_view());
    let ids = |state: &Outlines| state.loops().map(|(id, _)| id).collect::<Vec<_>>();
    let before = ids(&state);
    assert_eq!(before.len(), 1);
    let tiles = &mut tiles;
    // A bump on the floor, at its first corner too, a dent, and both gone:
    // still one loop, the same one, as a whole trace has it
    for edits in [
      &[(20, 5, S)][..],
      &[(0, 5, S)],
      &[(30, 6, E)],
      &[(20, 5, E), (0, 5, E), (30, 6, S)],
    ] {
      edit(&mut state, tiles, edits);
      assert_eq!(ids(&state), before, "after {edits:?}");
      assert_traced_by_the_rule(&state, tiles);
    }
    // A cut through it: two loops, spliced, the longer keeping the id
    edit(&mut state, tiles, &[(10, 6, E), (10, 7, E)]);
    let after = ids(&state);
    assert_eq!(after.len(), 2);
    let longest = state
      .loops()
      .max_by_key(|(_, outline)| outline.corners.len())
      .map(|(id, _)| id);
    assert_eq!(longest, Some(before[0]));
    // And joined again: one loop, its id the longer part's
    edit(&mut state, tiles, &[(10, 6, S), (10, 7, S)]);
    assert_eq!(ids(&state), before);
    assert_eq!(state.splices.whole, 0, "{:?}", state.splices);
  }

  #[test]
  fn an_edit_s_units_come_each_once_in_the_units_order() {
    let mut seed: u32 = 11;
    let mut next = |below: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % below
    };
    for round in 0..200 {
      // A few cells or a chunk's worth, some apart and some touching,
      // either side of 0
      let count = if round % 10 == 0 { 600 } else { 1 + next(40) };
      let mut cells: Vec<IVec2> = (0..count)
        .map(|_| IVec2::new(next(70) as i32 - 35, next(70) as i32 - 35))
        .collect();
      cells.sort_by_key(|cell| (cell.y, cell.x));
      cells.dedup();
      let changed: Vec<(IVec2, TileCell)> = cells.iter().map(|&cell| (cell, S)).collect();
      let mut expected: Vec<Unit> = cells
        .iter()
        .flat_map(|&IVec2 { x, y }| {
          [
            Unit::Inside(x, y),
            Unit::Left(x, y),
            Unit::Left(x + 1, y),
            Unit::Above(x, y),
            Unit::Above(x, y + 1),
          ]
        })
        .collect();
      expected.sort_unstable();
      expected.dedup();
      assert_eq!(units_of(&changed), expected, "round {round}");
    }
  }

  #[test]
  fn a_spliced_loop_starts_where_a_whole_trace_would() {
    // A floor from column 2: a whole trace starts its loop at (2, 7), going
    // up the left side of cell (2, 6). A cell set at (1, 7) joins the floor
    // and changes that corner's edges but leaves that first edge: the loop
    // still starts there (as a whole trace from the old loop's edges would),
    // so it stays in its chunk's pack
    let (w, h) = (40, 8);
    let mut tiles = TileColliders::new(w, h, Vec2::ONE);
    for x in 2..w {
      tiles.set(xy(x, 6), S);
      tiles.set(xy(x, 7), S);
    }
    tiles.take_changes();
    let mut state = Outlines::build(tiles.cells_view());
    let (id, first, second) = state
      .loops()
      .map(|(id, outline)| (id, outline.corners[0], outline.corners[1]))
      .next()
      .expect("the floor");
    let g = GRID;
    assert_eq!((first, second), ((2 * g, 7 * g), (2 * g, 6 * g)));
    edit(&mut state, &mut tiles, &[(1, 7, S)]);
    assert_eq!(state.loops().count(), 1);
    assert_eq!(state.get(id).map(|outline| outline.corners[0]), Some(first));
    assert_eq!(state.splices.whole, 0);
  }

  #[test]
  fn a_loop_split_and_merged_by_edits_matches_a_full_trace() {
    use SlopeCorner::{BottomLeft, BottomRight, TopLeft, TopRight};
    use TileCell::Slope;
    let (w, h) = (12, 8);
    let mut tiles = TileColliders::new(w, h, Vec2::ONE);
    // A floor two cells deep
    for x in 0..w {
      tiles.set(xy(x, 6), S);
      tiles.set(xy(x, 7), S);
    }
    tiles.take_changes();
    let mut state = Outlines::build(tiles.cells_view());
    let tiles = &mut tiles;
    // Split in two, then merged again a cell at a time
    edit(&mut state, tiles, &[(5, 6, E), (5, 7, E)]);
    edit(&mut state, tiles, &[(5, 6, S)]);
    edit(&mut state, tiles, &[(5, 7, S)]);
    // A ring above it, built a cell at a time, its hole filled and opened
    for (x, y) in [
      (2, 1),
      (3, 1),
      (4, 1),
      (2, 2),
      (4, 2),
      (2, 3),
      (3, 3),
      (4, 3),
    ] {
      edit(&mut state, tiles, &[(x, y, S)]);
    }
    edit(&mut state, tiles, &[(3, 2, S)]);
    edit(&mut state, tiles, &[(3, 2, E)]);
    // Touching the floor at a corner (a pinch), then joined across it
    edit(&mut state, tiles, &[(8, 5, S)]);
    edit(&mut state, tiles, &[(9, 4, S)]);
    edit(&mut state, tiles, &[(9, 5, S)]);
    // Slopes on the floor and against the ring
    edit(
      &mut state,
      tiles,
      &[(6, 5, Slope(BottomRight)), (7, 5, Slope(BottomLeft))],
    );
    edit(&mut state, tiles, &[(5, 1, Slope(TopLeft))]);
    edit(&mut state, tiles, &[(1, 3, Slope(TopRight))]);
    // One-way cells change no outline
    edit(&mut state, tiles, &[(10, 5, O)]);
    // A long walk of edits, one to three cells at a time, of every kind
    let kinds = [
      E,
      E,
      S,
      S,
      O,
      Slope(BottomLeft),
      Slope(BottomRight),
      Slope(TopLeft),
      Slope(TopRight),
    ];
    walk(&mut state, tiles, &kinds, 0x9e37_79b9, 1500);
    assert_traced_by_the_rule(&state, tiles);
    assert_spliced(&state);
  }

  /// The walk's edits were spliced, splits and joins among them, and none
  /// that dropped loops was traced whole (1,380 and 1,856 splices when
  /// written, about a third of them splits and a third joins)
  fn assert_spliced(state: &Outlines) {
    let s = state.splices;
    assert!(s.kept > 100 && s.splits > 100 && s.joins > 100, "{s:?}");
    assert_eq!(s.whole, 0, "{s:?}");
  }

  /// `count` steps of one to three random cells set to random `kinds`.
  fn walk(
    state: &mut Outlines,
    tiles: &mut TileColliders,
    kinds: &[TileCell],
    mut seed: u32,
    count: usize,
  ) {
    let size = tiles
      .bounds()
      .map_or(glam::UVec2::ZERO, |bounds| bounds.size());
    let (w, h) = (size.x, size.y);
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    for step in 0..count {
      let many = 1 + next(3);
      let edits: Vec<_> = (0..many)
        .map(|_| (next(w), next(h), kinds[next(kinds.len() as u32) as usize]))
        .collect();
      edit(state, tiles, &edits);
      if step % 100 == 0 {
        assert_traced_by_the_rule(state, tiles);
      }
    }
  }

  #[test]
  fn edits_with_shapes_match_a_full_trace() {
    use SlopeCorner::{BottomLeft, TopRight};
    use TileCell::Slope;
    let mut tiles = TileColliders::new(10, 7, Vec2::ONE);
    let kinds = [
      E,
      E,
      S,
      O,
      Slope(BottomLeft),
      Slope(TopRight),
      // Halves, a band, a ramp, a diamond touching each side at its middle,
      // an L, a sliver off a side, and a one-way slab
      add(
        &mut tiles,
        &[(0.0, 0.5), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)],
        false,
      ),
      add(
        &mut tiles,
        &[(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (0.0, 0.5)],
        false,
      ),
      add(
        &mut tiles,
        &[(0.0, 0.0), (0.5, 0.0), (0.5, 1.0), (0.0, 1.0)],
        false,
      ),
      add(
        &mut tiles,
        &[(0.0, 0.25), (1.0, 0.25), (1.0, 0.75), (0.0, 0.75)],
        false,
      ),
      add(&mut tiles, &[(0.0, 1.0), (1.0, 0.5), (1.0, 1.0)], false),
      add(
        &mut tiles,
        &[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)],
        false,
      ),
      add(
        &mut tiles,
        &[
          (0.0, 0.0),
          (0.5, 0.0),
          (0.5, 0.5),
          (1.0, 0.5),
          (1.0, 1.0),
          (0.0, 1.0),
        ],
        false,
      ),
      add(&mut tiles, &[(0.3, 0.0), (0.7, 0.0), (0.5, 0.2)], false),
      add(
        &mut tiles,
        &[(0.0, 0.25), (1.0, 0.25), (1.0, 0.5), (0.0, 0.5)],
        true,
      ),
    ];
    // And shapes made at random, many of them broken
    let mut kinds = kinds.to_vec();
    kinds.extend(broken_shapes(&mut tiles, 0x0bad_5eed, 40));
    tiles.take_changes();
    let mut state = Outlines::build(tiles.cells_view());
    walk(&mut state, &mut tiles, &kinds, 0x2545_f491, 2000);
    assert_traced_by_the_rule(&state, &tiles);
    assert_spliced(&state);
  }

  /// Shapes made at random on a lattice of quarter tiles, some corners a
  /// hair off it (under half a grid step) and some repeated, one or two
  /// polygons each: many touch or cross themselves or each other, or flatten
  /// when snapped.
  fn broken_shapes(tiles: &mut TileColliders, mut seed: u32, count: usize) -> Vec<TileCell> {
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    let hair = 0.1 / GRID as f32;
    let mut shapes = Vec::new();
    for shape in 0..count {
      let mut polygons = Vec::new();
      // A third are one polygon reaching past the cell (the star below)
      let reaching = shape % 3 == 0;
      for _ in 0..if reaching { 1 } else { 1 + next(2) } {
        let point = |next: &mut dyn FnMut(u32) -> u32| {
          let mut p = Vec2::new(next(5) as f32 / 4.0, next(5) as f32 / 4.0);
          if next(3) == 0 {
            p += Vec2::new(next(9) as f32 - 4.0, next(9) as f32 - 4.0) * hair;
          }
          p
        };
        let count = 3 + next(5);
        let mut points: Vec<Vec2> = (0..count).map(|_| point(&mut next)).collect();
        match if reaching { 3 } else { next(3) } {
          // Reaching past the cell, and simple: a comb whose base is past
          // one side and whose teeth come in, so the cell cuts it into a
          // piece a tooth
          3 => {
            let teeth = 2 + next(3);
            let width = 1.0 / (2 * teeth) as f32;
            let mut comb = vec![
              Vec2::new(-0.2, -0.5),
              Vec2::new(1.2, -0.5),
              Vec2::new(1.2, -0.2),
            ];
            for tooth in (0..teeth).rev() {
              let x = (2 * tooth) as f32 * width + next(3) as f32 * width / 4.0;
              let depth = 0.2 + next(4) as f32 * 0.2;
              comb.extend([
                Vec2::new(x + width, -0.2),
                Vec2::new(x + width, depth),
                Vec2::new(x, depth),
                Vec2::new(x, -0.2),
              ]);
            }
            comb.push(Vec2::new(-0.2, -0.2));
            if next(3) == 0 {
              let i = next(comb.len() as u32) as usize;
              comb[i] += Vec2::new(next(9) as f32 - 4.0, next(9) as f32 - 4.0) * hair;
            }
            // Past the top, the bottom, the left or the right
            let side = next(4);
            points = comb
              .into_iter()
              .map(|p| match side {
                0 => p,
                1 => Vec2::new(p.x, 1.0 - p.y),
                2 => Vec2::new(p.y, p.x),
                _ => Vec2::new(1.0 - p.y, p.x),
              })
              .collect();
          }
          // Pinched: through a point inside twice, between two lobes each
          // with an edge along a side of the cell, wound opposite ways
          0 => {
            let quarter = |next: &mut dyn FnMut(u32) -> u32| (1 + next(3)) as f32 / 4.0;
            let pinch = Vec2::new(quarter(&mut next), quarter(&mut next));
            let on_side = |t: f32, side: u32| match side {
              0 => Vec2::new(t, 0.0),
              1 => Vec2::new(1.0, t),
              2 => Vec2::new(t, 1.0),
              _ => Vec2::new(0.0, t),
            };
            let lobe = |next: &mut dyn FnMut(u32) -> u32| {
              let u = next(5);
              let v = (u + 1 + next(4)) % 5;
              let side = next(4);
              (on_side(u as f32 / 4.0, side), on_side(v as f32 / 4.0, side))
            };
            let winding = |(a, b): (Vec2, Vec2)| (a - pinch).perp_dot(b - pinch).signum();
            let first = lobe(&mut next);
            let (mut a, mut b) = lobe(&mut next);
            if winding((a, b)) == winding(first) {
              (a, b) = (b, a);
            }
            points = vec![pinch, first.0, first.1, pinch, a, b];
          }
          // A corner on another of its edges, or a corner repeated
          1 => {
            let i = next(points.len() as u32) as usize;
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            let at = (i + 2 + next(points.len() as u32 - 1) as usize) % points.len();
            let corner = if next(2) == 0 { (a + b) / 2.0 } else { a };
            points.insert(at, corner);
          }
          _ => {}
        }
        polygons.push(points);
      }
      let one_way = next(8) == 0;
      shapes.push(TileCell::Shape(
        tiles.add_shape(CellShape { polygons, one_way }),
      ));
    }
    shapes
  }

  /// A shape set among solid cells (either side of it and above), edited
  /// around and compared with a full trace each time. It must build as the
  /// hull of its points, a whole square here, and so join the solids.
  fn hull_beside_solids(points: &[(f32, f32)]) {
    let mut tiles = TileColliders::new(3, 3, Vec2::ONE);
    let shape = add(&mut tiles, points, false);
    let TileCell::Shape(index) = shape else {
      unreachable!()
    };
    let square = ShapeBuild::of(&CellShape {
      polygons: vec![vec![Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y]],
      one_way: false,
    });
    let sorted = |edges: &[(Point, Point)]| {
      let mut edges = edges.to_vec();
      edges.sort_unstable();
      edges
    };
    // Set among solids and edited round, the shape cell among them
    for (x, y, cell) in [(0, 1, S), (1, 1, shape), (2, 1, S), (1, 0, S)] {
      tiles.set(xy(x, y), cell);
    }
    tiles.take_changes();
    let mut state = Outlines::build(tiles.cells_view());
    walk(
      &mut state,
      &mut tiles,
      &[E, S, shape, shape],
      0x51ed_270b,
      300,
    );
    assert_traced_by_the_rule(&state, &tiles);
    assert_eq!(
      sorted(&tiles.shapes[usize::from(index)].1.edges),
      sorted(&square.edges),
      "{points:?}"
    );
    for (x, y, cell) in [(0, 1, S), (1, 1, shape), (2, 1, S), (1, 0, S), (1, 2, E)] {
      tiles.set(xy(x, y), cell);
    }
    tiles.take_changes();
    let state = Outlines::build(tiles.cells_view());
    assert_eq!(traced(&state).len(), 1, "one loop round the four cells");
  }

  // An arch with its bar above the cell: the cell's top cuts it into two
  // legs, which collide as two legs, with no warning (the clip runs along
  // the top both ways between them, and that cancels)
  #[test]
  fn a_shape_cut_into_pieces_by_its_cell_collides_as_the_pieces() {
    let arch = [
      (0.1, -0.5),
      (0.9, -0.5),
      (0.9, 0.8),
      (0.7, 0.8),
      (0.7, -0.2),
      (0.3, -0.2),
      (0.3, 0.8),
      (0.1, 0.8),
    ];
    let mut tiles = TileColliders::new(3, 2, Vec2::ONE);
    let (solid, one_way) = (add(&mut tiles, &arch, false), add(&mut tiles, &arch, true));
    for (_, build) in &tiles.shapes {
      assert!(!build.hulled, "no hull, no warning");
    }
    tiles.set(IVec2::new(1, 1), solid);
    let legs = [
      expect(&[(1.1, 1.0), (1.3, 1.0), (1.3, 1.8), (1.1, 1.8)]),
      expect(&[(1.7, 1.0), (1.9, 1.0), (1.9, 1.8), (1.7, 1.8)]),
    ];
    assert_eq!(traced(&Outlines::build(tiles.cells_view())), legs);
    // Beside solid cells, edited round
    tiles.take_changes();
    let mut state = Outlines::build(tiles.cells_view());
    walk(
      &mut state,
      &mut tiles,
      &[E, S, solid, solid],
      0x0a7c_4e55,
      300,
    );
    // A one-way arch is two one-way legs' tops
    let TileCell::Shape(index) = one_way else {
      unreachable!()
    };
    let g = |v: f32| (v * GRID as f32).round() as i32;
    assert_eq!(
      tiles.shapes[usize::from(index)].1.tops,
      [((g(0.1), 0), (g(0.3), 0)), ((g(0.7), 0), (g(0.9), 0))]
    );
  }

  #[test]
  fn a_shape_touching_itself_at_a_vertex_collides_as_its_hull() {
    // Through (0.5, 0.4) twice: a lobe above it and a larger one below,
    // wound the other way
    hull_beside_solids(&[
      (0.0, 0.0),
      (1.0, 0.0),
      (0.5, 0.4),
      (0.0, 1.0),
      (1.0, 1.0),
      (0.5, 0.4),
    ]);
  }

  #[test]
  fn a_shape_vertex_on_its_own_edge_collides_as_its_hull() {
    // Its corner (0.5, 0.5) on its own edge from (0, 0) to (1, 1)
    hull_beside_solids(&[
      (0.0, 0.0),
      (1.0, 1.0),
      (1.0, 0.0),
      (0.75, 0.0),
      (0.5, 0.5),
      (0.0, 1.0),
    ]);
  }

  #[test]
  fn shapes_that_only_become_degenerate_after_snapping_collide_as_their_hull() {
    // Each under half a grid step from the case above it: snapping lands
    // it there
    let off = 0.4 / GRID as f32;
    for y in [0.5 + off, 0.5 - off] {
      hull_beside_solids(&[
        (0.0, 0.0),
        (1.0, 1.0),
        (1.0, 0.0),
        (0.75, 0.0),
        (0.5, y),
        (0.0, 1.0),
      ]);
    }
    hull_beside_solids(&[
      (0.0, 0.0),
      (1.0, 0.0),
      (0.5, 0.4),
      (0.0, 1.0),
      (1.0, 1.0),
      (0.5 + off, 0.4 - off),
    ]);
    // A slit thinner than the grid closes
    hull_beside_solids(&[
      (0.0, 0.0),
      (1.0, 0.0),
      (1.0, 0.5 - off),
      (0.5, 0.5 - off),
      (0.5, 0.5 + off),
      (1.0, 0.5 + off),
      (1.0, 1.0),
      (0.0, 1.0),
    ]);
  }

  // Whatever closed edges a shape gave (these are ones `add_shape` never
  // makes), the outlines neither panic nor lose or gain an edge: every
  // boundary edge is in one loop, as often as the boundary has it
  #[test]
  fn the_outlines_stay_whole_whatever_edges_a_shape_gives() {
    use SlopeCorner::TopLeft;
    let g = GRID;
    let mut tiles = TileColliders::new(5, 4, Vec2::ONE);
    // A triangle wound the wrong way (its sides run against their cells'
    // ways), and a square's edges twice
    let wrong_way = vec![((0, 0), (0, g)), ((0, g), (g, 0)), ((g, 0), (0, 0))];
    let square = vec![
      ((0, 0), (g, 0)),
      ((g, 0), (g, g)),
      ((g, g), (0, g)),
      ((0, g), (0, 0)),
    ];
    for edges in [wrong_way, [square.clone(), square].concat()] {
      tiles.shapes.push((
        CellShape::default(),
        ShapeBuild {
          edges,
          ..ShapeBuild::default()
        },
      ));
    }
    let kinds = [
      E,
      S,
      S,
      TileCell::Slope(TopLeft),
      TileCell::Shape(0),
      TileCell::Shape(1),
    ];
    let mut state = Outlines::build(tiles.cells_view());
    let mut seed: u32 = 0x7e57_ab1e;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    for _ in 0..800 {
      for _ in 0..1 + next(3) {
        tiles.set(
          xy(next(5), next(4)),
          kinds[next(kinds.len() as u32) as usize],
        );
      }
      let changed = tiles.take_changes().expect("listed");
      state.apply(tiles.cells_view(), &changed);
      let mut traced: Vec<(Point, Point)> = state
        .loops()
        .flat_map(|(_, o)| {
          o.corners
            .windows(2)
            .map(|w| (w[0], w[1]))
            .collect::<Vec<_>>()
        })
        .collect();
      let mut boundary = boundary_edges(tiles.cells_view());
      traced.sort_unstable();
      boundary.sort_unstable();
      assert_eq!(traced, boundary);
    }
  }
}
