//! A tile grid's one-way runs kept between steps, so an edit rebuilds only
//! the runs it touches.
//!
//! Each one-way cell has a piece along its top, and each one-way shape its
//! top edges (no steeper than 45°), on the outline grid. A run is a chain of
//! pieces each starting where the last ends, along one line: a row of
//! one-way cells and one-way shapes flush with their tops, a row of slabs at
//! one height, or a one-way ramp across cells and rows, each one segment.
//! Pieces run left to right, so a chain never comes back on itself. A
//! piece's neighbours in its run depend only on the pieces at its two ends,
//! so an edit drops the runs through the ends of the pieces it changed and
//! chains their pieces and the new ones again; every other run keeps its id
//! and its part.

use super::outline::{Retraced, cross, delta, key};
use super::{Cells, Few, GRID, Point, TileCell};
use crate::core::ecs::Entity;
use crate::core::hash::FastMap;

/// The run id of a piece waiting to be chained.
const UNCHAINED: u32 = u32::MAX;

/// A piece leaving a point: where it ends and its run.
#[derive(Debug, Clone, Copy, Default)]
struct Piece {
  end: Point,
  id: u32,
}

/// One run: its points, piece by piece, left to right.
#[derive(Debug)]
pub(crate) struct Run {
  pub(crate) points: Vec<Point>,
  /// The collider built from it
  pub(crate) part: Option<Entity>,
}

/// The one-way runs of a grid, kept between steps.
#[derive(Debug, Default)]
pub(crate) struct Runs {
  /// The pieces leaving each point, by `key`
  from: FastMap<u64, Few<Piece>>,
  /// Where the pieces reaching each point start, by `key`
  to: FastMap<u64, Few<Point>>,
  /// The runs by id; a dropped run's slot is empty until reused
  runs: Vec<Option<Run>>,
  free: Vec<u32>,
}

/// The pieces of the cell at (x, y), the cells read through `at`, pushed onto `out`.
fn cell_pieces(
  cells: &Cells,
  at: &impl Fn(i32, i32) -> TileCell,
  x: i32,
  y: i32,
  out: &mut Vec<(Point, Point)>,
) {
  let origin = (x * GRID, y * GRID);
  out.extend(
    cells
      .tops(at(x, y))
      .iter()
      .map(|&(a, b)| (super::sides::plus(origin, a), super::sides::plus(origin, b))),
  );
}

// No piece is ever inserted twice: a cell's pieces are distinct (a shape's
// tops are edges of a well-formed boundary), and two cells never share one,
// as a piece runs left to right inside its cell's box and a top on a side
// of the box is on its top side (a well-formed shape's bottom edges run
// right to left), which no other cell's pieces reach.

impl Runs {
  /// Every run of the grid, chained whole.
  pub(crate) fn build(cells: Cells) -> Self {
    let (cols, rows) = cells.extent();
    let at = |x: i32, y: i32| cells.at(x, y);
    let mut pieces = Vec::new();
    for y in 0..rows {
      for x in 0..cols {
        cell_pieces(&cells, &at, x, y, &mut pieces);
      }
    }
    let mut runs = Self::default();
    for &(a, b) in &pieces {
      runs.insert(a, b);
    }
    runs.chain(&pieces);
    runs
  }

  /// Take in the cells at `changed` (sorted by index, each with the value it
  /// had when the runs were last brought up to date).
  pub(crate) fn apply(&mut self, cells: Cells, changed: &[(u32, TileCell)]) -> Retraced {
    let mut retraced = Retraced::default();
    let width = cells.width();
    if width == 0 || changed.is_empty() {
      return retraced;
    }
    let before = |x: i32, y: i32| cells.before(x, y, changed);
    let now = |x: i32, y: i32| cells.at(x, y);
    let (mut was, mut is) = (Vec::new(), Vec::new());
    let mut changes: Vec<(Point, Point, bool)> = Vec::new();
    for &(index, _) in changed {
      let (x, y) = ((index % width) as i32, (index / width) as i32);
      was.clear();
      is.clear();
      cell_pieces(&cells, &before, x, y, &mut was);
      cell_pieces(&cells, &now, x, y, &mut is);
      if was == is {
        continue;
      }
      changes.extend(
        was
          .iter()
          .filter(|e| !is.contains(e))
          .map(|&(a, b)| (a, b, false)),
      );
      changes.extend(
        is.iter()
          .filter(|e| !was.contains(e))
          .map(|&(a, b)| (a, b, true)),
      );
    }
    if changes.is_empty() {
      return retraced;
    }
    // The runs through the ends of a changed piece
    let mut dropped: Vec<u32> = Vec::new();
    for &(a, b, _) in &changes {
      for point in [a, b] {
        if let Some(pieces) = self.from.get(&key(point)) {
          dropped.extend(pieces.as_slice().iter().map(|piece| piece.id));
        }
        for &start in self.to.get(&key(point)).map_or(&[][..], Few::as_slice) {
          dropped.extend(self.piece(start, point).map(|piece| piece.id));
        }
      }
    }
    dropped.sort_unstable();
    dropped.dedup();
    let mut starts = Vec::new();
    for id in dropped {
      let Some(run) = self.runs.get_mut(id as usize).and_then(Option::take) else {
        continue;
      };
      self.free.push(id);
      for piece in run.points.windows(2) {
        self.set_run(piece[0], piece[1], UNCHAINED);
        starts.push((piece[0], piece[1]));
      }
      retraced.dropped.push(run.part);
    }
    for (a, b, now) in changes {
      if now {
        self.insert(a, b);
        starts.push((a, b));
      } else {
        self.remove(a, b);
      }
    }
    retraced.traced = self.chain(&starts);
    retraced
  }

  /// Every run and its id.
  pub(crate) fn runs(&self) -> impl Iterator<Item = (u32, &Run)> {
    self
      .runs
      .iter()
      .enumerate()
      .filter_map(|(id, run)| run.as_ref().map(|r| (id as u32, r)))
  }

  pub(crate) fn get(&self, id: u32) -> Option<&Run> {
    self.runs.get(id as usize).and_then(Option::as_ref)
  }

  pub(crate) fn get_mut(&mut self, id: u32) -> Option<&mut Run> {
    self.runs.get_mut(id as usize).and_then(Option::as_mut)
  }

  fn piece(&self, a: Point, b: Point) -> Option<Piece> {
    self
      .from
      .get(&key(a))?
      .as_slice()
      .iter()
      .copied()
      .find(|piece| piece.end == b)
  }

  fn insert(&mut self, a: Point, b: Point) {
    if self.piece(a, b).is_some() {
      debug_assert!(false, "the one-way piece {a:?} -> {b:?} is already there");
      return;
    }
    self.from.entry(key(a)).or_default().push(Piece {
      end: b,
      id: UNCHAINED,
    });
    self.to.entry(key(b)).or_default().push(a);
  }

  fn remove(&mut self, a: Point, b: Point) {
    if let Some(pieces) = self.from.get_mut(&key(a))
      && let Some(i) = pieces.as_slice().iter().position(|p| p.end == b)
    {
      pieces.swap_remove(i);
      if pieces.is_empty() {
        self.from.remove(&key(a));
      }
    }
    if let Some(starts) = self.to.get_mut(&key(b))
      && let Some(i) = starts.as_slice().iter().position(|&s| s == a)
    {
      starts.swap_remove(i);
      if starts.is_empty() {
        self.to.remove(&key(b));
      }
    }
  }

  fn set_run(&mut self, a: Point, b: Point, id: u32) {
    if let Some(pieces) = self.from.get_mut(&key(a))
      && let Some(piece) = pieces.as_mut_slice().iter_mut().find(|p| p.end == b)
    {
      piece.id = id;
    }
  }

  fn unchained(&self, a: Point, b: Point) -> bool {
    self.piece(a, b).is_some_and(|piece| piece.id == UNCHAINED)
  }

  /// The piece before `a`-`b` in its run: one ending at `a` along the same line.
  fn before(&self, a: Point, b: Point) -> Option<Point> {
    let along = delta(a, b);
    self
      .to
      .get(&key(a))?
      .as_slice()
      .iter()
      .copied()
      .filter(|&start| cross(delta(start, a), along) == 0)
      .min()
  }

  /// The piece after `a`-`b` in its run: one starting at `b` along the same line.
  fn after(&self, a: Point, b: Point) -> Option<Point> {
    let along = delta(a, b);
    self
      .from
      .get(&key(b))?
      .as_slice()
      .iter()
      .map(|piece| piece.end)
      .filter(|&end| cross(along, delta(b, end)) == 0)
      .min()
  }

  /// Chain the run through each of `starts` not yet chained; returns their ids.
  fn chain(&mut self, starts: &[(Point, Point)]) -> Vec<u32> {
    let mut chained = Vec::new();
    for &(a, b) in starts {
      if !self.unchained(a, b) {
        continue;
      }
      // Back to the run's first piece (each step goes left, so this ends)
      let (mut from, mut to) = (a, b);
      while let Some(start) = self.before(from, to) {
        if !self.unchained(start, from) {
          break;
        }
        (from, to) = (start, from);
      }
      let id = self.free.pop().unwrap_or(self.runs.len() as u32);
      let mut points = vec![from];
      loop {
        self.set_run(from, to, id);
        points.push(to);
        match self.after(from, to) {
          Some(end) if self.unchained(to, end) => (from, to) = (to, end),
          _ => break,
        }
      }
      let run = Some(Run { points, part: None });
      match self.runs.get_mut(id as usize) {
        Some(slot) => *slot = run,
        None => self.runs.push(run),
      }
      chained.push(id);
    }
    chained
  }
}

/// Each one-way run, as its first and last corner in tiles.
#[cfg(test)]
pub(crate) fn one_way_runs(
  width: u32,
  height: u32,
  cells: &[TileCell],
) -> Vec<(super::Corner, super::Corner)> {
  Runs::build(Cells::bare(width, height, cells))
    .runs()
    .map(|(_, run)| {
      let tiles = |(x, y): Point| (x / GRID, y / GRID);
      (
        tiles(run.points[0]),
        tiles(run.points[run.points.len() - 1]),
      )
    })
    .collect()
}
