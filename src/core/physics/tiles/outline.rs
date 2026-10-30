//! A tile grid's outlines kept between steps, so an edit retraces only the
//! loops it touches.
//!
//! Every filled cell's edges, wound clockwise on screen, are kept where no
//! neighbour runs the same edge the other way: the boundary of the solid
//! and slope cells. Each corner lists the boundary edges leaving it and the
//! loop each belongs to. A loop goes on from an edge by the edge leaving its
//! end corner with the sharpest turn: the other side of the filled wedge the
//! edge bounds there. That depends on the corner's edges alone, so a loop is
//! the same whatever order the loops are traced in and wherever each starts.
//! An edit changes edges at its cells' corners only: the loops through a
//! corner whose edges changed are dropped (both loops at a corner where two
//! touch), and their edges and the new ones are traced again; every other
//! loop keeps its id and its part.

use super::{Corner, TileCell, cell, corners, extent, sub};
use crate::core::ecs::Entity;
use crate::core::hash::FastMap;

/// The steps an outline edge takes, a cell's side or diagonal, clockwise on
/// screen from +X; an edge is stored by its index here.
const STEPS: [Corner; 8] = [
  (1, 0),
  (1, 1),
  (0, 1),
  (-1, 1),
  (-1, 0),
  (-1, -1),
  (0, -1),
  (1, -1),
];

fn step_of(d: Corner) -> u8 {
  match d {
    (1, 0) => 0,
    (1, 1) => 1,
    (0, 1) => 2,
    (-1, 1) => 3,
    (-1, 0) => 4,
    (-1, -1) => 5,
    (0, -1) => 6,
    _ => 7,
  }
}

/// How sharp a turn from step `from` to step `to` is, the sharpest highest:
/// back the way it came (180°) down to 135° the other way. Steps are 45°
/// apart, clockwise on screen, so this orders turns as their signed angle
/// does.
fn sharpness(from: u8, to: u8) -> u8 {
  (to + 11 - from) % 8
}

fn add(a: Corner, d: Corner) -> Corner {
  (a.0 + d.0, a.1 + d.1)
}

fn key((x, y): Corner) -> u64 {
  (u64::from(x as u32) << 32) | u64::from(y as u32)
}

/// The loop id of an edge waiting to be traced.
const UNTRACED: u32 = u32::MAX;

/// The boundary edges leaving one corner, by step, and the loop of each: at
/// most four, one per filled wedge around the corner (four half-cells
/// alternating with four empty halves).
#[derive(Debug, Clone, Copy)]
struct Leaving {
  len: u8,
  steps: [u8; 4],
  loops: [u32; 4],
}

impl Leaving {
  const NONE: Self = Self {
    len: 0,
    steps: [0; 4],
    loops: [UNTRACED; 4],
  };

  fn find(&self, step: u8) -> Option<usize> {
    (0..usize::from(self.len)).find(|&i| self.steps[i] == step)
  }

  fn push(&mut self, step: u8) {
    let i = usize::from(self.len);
    if i < 4 {
      self.steps[i] = step;
      self.loops[i] = UNTRACED;
      self.len += 1;
    } else {
      debug_assert!(false, "more than four outline edges leave a corner");
    }
  }

  fn remove(&mut self, i: usize) {
    let last = usize::from(self.len) - 1;
    self.steps[i] = self.steps[last];
    self.loops[i] = self.loops[last];
    self.len -= 1;
  }
}

/// The cells as the outlines read them: past what `cells` holds is empty.
#[derive(Clone, Copy)]
struct Grid<'a> {
  width: u32,
  cols: i32,
  rows: i32,
  cells: &'a [TileCell],
}

impl<'a> Grid<'a> {
  fn new(width: u32, height: u32, cells: &'a [TileCell]) -> Self {
    let (cols, rows) = extent(width, height, cells);
    // A grid holds at most 8192 × 8192 cells; `cells` bounds a bare call
    let clamp = |n: u32| n.min(i32::MAX as u32 - 1) as i32;
    Self {
      width,
      cols: clamp(cols),
      rows: clamp(rows),
      cells,
    }
  }

  fn at(&self, x: i32, y: i32) -> TileCell {
    if x < 0 || y < 0 || x >= self.cols || y >= self.rows {
      return TileCell::Empty;
    }
    cell(self.width, self.cells, x as u32, y as u32)
  }

  /// Whether a cell has the edge from `a` to `b`: one of the two cells on a
  /// side, or the cell a diagonal crosses.
  fn runs(&self, a: Corner, b: Corner) -> bool {
    let (x, y) = (a.0.min(b.0), a.1.min(b.1));
    let owners = if a.1 == b.1 {
      [(x, y - 1), (x, y)]
    } else if a.0 == b.0 {
      [(x - 1, y), (x, y)]
    } else {
      [(x, y), (x, y)]
    };
    owners.iter().any(|&at| {
      let shape = corners(self.at(at.0, at.1));
      let n = shape.len();
      (0..n).any(|i| add(at, shape[i]) == a && add(at, shape[(i + 1) % n]) == b)
    })
  }

  /// Whether `a` to `b` is a boundary edge: a cell has it, and no cell has
  /// it the other way.
  fn boundary(&self, a: Corner, b: Corner) -> bool {
    self.runs(a, b) && !self.runs(b, a)
  }
}

/// One outline loop.
#[derive(Debug)]
pub(crate) struct Outline {
  /// Its corners a step apart, the last repeating the first
  pub(crate) corners: Vec<Corner>,
  /// The collider built from it
  pub(crate) part: Option<Entity>,
}

/// What an edit changed: the parts of the loops it dropped, and the ids of
/// the loops it traced.
#[derive(Debug, Default)]
pub(crate) struct Retraced {
  pub(crate) dropped: Vec<Option<Entity>>,
  pub(crate) traced: Vec<u32>,
}

/// The outlines of a grid's solid and slope cells, kept between steps.
#[derive(Debug, Default)]
pub(crate) struct Outlines {
  /// Every corner with boundary edges leaving it, by `key`
  leaving: FastMap<u64, Leaving>,
  /// The loops by id; a dropped loop's slot is empty until reused
  loops: Vec<Option<Outline>>,
  free: Vec<u32>,
}

impl Outlines {
  /// Every loop of the grid, traced whole.
  pub(crate) fn build(width: u32, height: u32, cells: &[TileCell]) -> Self {
    let grid = Grid::new(width, height, cells);
    let mut outlines = Self::default();
    let mut starts = Vec::new();
    for y in 0..grid.rows {
      for x in 0..grid.cols {
        let shape = corners(grid.at(x, y));
        let n = shape.len();
        for i in 0..n {
          let (a, b) = (add((x, y), shape[i]), add((x, y), shape[(i + 1) % n]));
          // An edge two cells share runs both ways and cancels
          if !grid.runs(b, a) {
            outlines.insert(a, b);
            starts.push((a, b));
          }
        }
      }
    }
    outlines.trace(&starts);
    outlines
  }

  /// Take in the cells at `changed` (cell indices; repeats are fine): the
  /// loops through a corner whose edges changed are dropped and traced
  /// again with the new edges.
  pub(crate) fn apply(
    &mut self,
    width: u32,
    height: u32,
    cells: &[TileCell],
    changed: &[u32],
  ) -> Retraced {
    let mut retraced = Retraced::default();
    if width == 0 {
      return retraced;
    }
    let grid = Grid::new(width, height, cells);
    // The edges an edit can change: its cells' sides and diagonals, both ways
    let mut edges: Vec<(Corner, Corner)> = Vec::with_capacity(changed.len() * 12);
    for &index in changed {
      let (x, y) = ((index % width) as i32, (index / width) as i32);
      let [c0, c1, c2, c3] = [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)];
      for (a, b) in [(c0, c1), (c1, c2), (c2, c3), (c3, c0), (c0, c2), (c1, c3)] {
        edges.push((a, b));
        edges.push((b, a));
      }
    }
    edges.sort_unstable();
    edges.dedup();
    let changes: Vec<(Corner, Corner, bool)> = edges
      .into_iter()
      .filter_map(|(a, b)| {
        let now = grid.boundary(a, b);
        (now != self.has(a, b)).then_some((a, b, now))
      })
      .collect();
    if changes.is_empty() {
      return retraced;
    }
    let mut dropped: Vec<u32> = Vec::new();
    for &(a, b, _) in &changes {
      for corner in [a, b] {
        if let Some(leaving) = self.leaving.get(&key(corner)) {
          dropped.extend(&leaving.loops[..usize::from(leaving.len)]);
        }
      }
    }
    dropped.sort_unstable();
    dropped.dedup();
    let mut starts = Vec::new();
    for id in dropped {
      let Some(outline) = self.loops.get_mut(id as usize).and_then(Option::take) else {
        continue;
      };
      self.free.push(id);
      for edge in outline.corners.windows(2) {
        self.set_loop(edge[0], edge[1], UNTRACED);
        starts.push((edge[0], edge[1]));
      }
      retraced.dropped.push(outline.part);
    }
    for (a, b, now) in changes {
      if now {
        self.insert(a, b);
        starts.push((a, b));
      } else {
        self.remove(a, b);
      }
    }
    retraced.traced = self.trace(&starts);
    retraced
  }

  /// Every loop and its id.
  pub(crate) fn loops(&self) -> impl Iterator<Item = (u32, &Outline)> {
    self
      .loops
      .iter()
      .enumerate()
      .filter_map(|(id, outline)| outline.as_ref().map(|o| (id as u32, o)))
  }

  pub(crate) fn get_mut(&mut self, id: u32) -> Option<&mut Outline> {
    self.loops.get_mut(id as usize).and_then(Option::as_mut)
  }

  fn has(&self, a: Corner, b: Corner) -> bool {
    let step = step_of(sub(b, a));
    self
      .leaving
      .get(&key(a))
      .is_some_and(|leaving| leaving.find(step).is_some())
  }

  fn insert(&mut self, a: Corner, b: Corner) {
    self
      .leaving
      .entry(key(a))
      .or_insert(Leaving::NONE)
      .push(step_of(sub(b, a)));
  }

  fn remove(&mut self, a: Corner, b: Corner) {
    let step = step_of(sub(b, a));
    if let Some(leaving) = self.leaving.get_mut(&key(a))
      && let Some(i) = leaving.find(step)
    {
      leaving.remove(i);
      if leaving.len == 0 {
        self.leaving.remove(&key(a));
      }
    }
  }

  fn set_loop(&mut self, a: Corner, b: Corner, id: u32) {
    let step = step_of(sub(b, a));
    if let Some(leaving) = self.leaving.get_mut(&key(a))
      && let Some(i) = leaving.find(step)
    {
      leaving.loops[i] = id;
    }
  }

  /// Trace the loop through each of `starts` not yet traced; returns their ids.
  fn trace(&mut self, starts: &[(Corner, Corner)]) -> Vec<u32> {
    let mut traced = Vec::new();
    for &(start, second) in starts {
      let first = step_of(sub(second, start));
      let Some(leaving) = self.leaving.get_mut(&key(start)) else {
        continue;
      };
      let Some(i) = leaving.find(first) else {
        continue;
      };
      if leaving.loops[i] != UNTRACED {
        continue;
      }
      let id = self.free.pop().unwrap_or(self.loops.len() as u32);
      leaving.loops[i] = id;
      let mut corners = vec![start, second];
      let (mut at, mut heading) = (second, first);
      // Each step takes an untraced edge, so this ends; it closes when the
      // edge taken would be the first one again
      while let Some(leaving) = self.leaving.get_mut(&key(at)) {
        let open =
          |i: usize| leaving.loops[i] == UNTRACED || (at == start && leaving.steps[i] == first);
        let Some(pick) = (0..usize::from(leaving.len))
          .filter(|&i| open(i))
          .max_by_key(|&i| sharpness(heading, leaving.steps[i]))
        else {
          debug_assert!(false, "an outline ends at {at:?}");
          break;
        };
        let step = leaving.steps[pick];
        if at == start && step == first {
          break;
        }
        leaving.loops[pick] = id;
        heading = step;
        at = add(at, STEPS[usize::from(step)]);
        corners.push(at);
      }
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
/// too), straight runs merged: each is a loop of corners whose last repeats
/// its first.
#[cfg(test)]
pub(crate) fn outlines(width: u32, height: u32, cells: &[TileCell]) -> Vec<Vec<Corner>> {
  Outlines::build(width, height, cells)
    .loops()
    .map(|(_, outline)| super::merge_straight_runs(&outline.corners))
    .collect()
}

#[cfg(test)]
mod tests {
  use super::super::{SlopeCorner, merge_straight_runs};
  use super::*;
  use TileCell::{Empty as E, OneWay as O, Solid as S};
  use std::collections::BTreeSet;

  type Edges = BTreeSet<(Corner, Corner)>;

  /// Loops as sets of directed edges, sorted: equal for the same outlines
  /// whatever order they were traced in and wherever each starts.
  fn canonical(loops: impl IntoIterator<Item = Vec<Corner>>) -> Vec<Edges> {
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

  /// Sets cells, applies them to `state` and compares it with a full trace.
  fn edit(
    state: &mut Outlines,
    (w, h): (u32, u32),
    cells: &mut [TileCell],
    edits: &[(u32, u32, TileCell)],
  ) {
    let changed: Vec<u32> = edits
      .iter()
      .map(|&(x, y, cell)| {
        let i = y * w + x;
        cells[i as usize] = cell;
        i
      })
      .collect();
    state.apply(w, h, cells, &changed);
    assert_eq!(
      traced(state),
      canonical(outlines(w, h, cells)),
      "after {edits:?}"
    );
  }

  #[test]
  fn a_loop_split_and_merged_by_edits_matches_a_full_trace() {
    use SlopeCorner::{BottomLeft, BottomRight, TopLeft, TopRight};
    use TileCell::Slope;
    let size = (12, 8);
    let (w, h) = size;
    let mut cells = vec![E; (w * h) as usize];
    // A floor two cells deep
    for x in 0..w {
      cells[(6 * w + x) as usize] = S;
      cells[(7 * w + x) as usize] = S;
    }
    let mut state = Outlines::build(w, h, &cells);
    let cells = &mut cells;
    // Split in two, then merged again a cell at a time
    edit(&mut state, size, cells, &[(5, 6, E), (5, 7, E)]);
    edit(&mut state, size, cells, &[(5, 6, S)]);
    edit(&mut state, size, cells, &[(5, 7, S)]);
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
      edit(&mut state, size, cells, &[(x, y, S)]);
    }
    edit(&mut state, size, cells, &[(3, 2, S)]);
    edit(&mut state, size, cells, &[(3, 2, E)]);
    // Touching the floor at a corner (a pinch), then joined across it
    edit(&mut state, size, cells, &[(8, 5, S)]);
    edit(&mut state, size, cells, &[(9, 4, S)]);
    edit(&mut state, size, cells, &[(9, 5, S)]);
    // Slopes on the floor and against the ring
    edit(
      &mut state,
      size,
      cells,
      &[(6, 5, Slope(BottomRight)), (7, 5, Slope(BottomLeft))],
    );
    edit(&mut state, size, cells, &[(5, 1, Slope(TopLeft))]);
    edit(&mut state, size, cells, &[(1, 3, Slope(TopRight))]);
    // One-way cells change no outline
    edit(&mut state, size, cells, &[(10, 5, O)]);
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
    let mut seed: u32 = 0x9e37_79b9;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    for _ in 0..1500 {
      let count = 1 + next(3);
      let edits: Vec<_> = (0..count)
        .map(|_| (next(w), next(h), kinds[next(kinds.len() as u32) as usize]))
        .collect();
      edit(&mut state, size, cells, &edits);
    }
  }
}
