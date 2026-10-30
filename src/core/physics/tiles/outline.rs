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
//! loop keeps its id and its part.

use super::{Cells, Few, GRID, Point, TileCell, sides};
use crate::core::ecs::Entity;
use crate::core::hash::FastMap;

pub(super) fn key((x, y): Point) -> u64 {
  (u64::from(x as u32) << 32) | u64::from(y as u32)
}

pub(super) fn delta(a: Point, b: Point) -> (i64, i64) {
  (i64::from(b.0 - a.0), i64::from(b.1 - a.1))
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
enum Unit {
  /// A cell's edges inside it
  Inside(i32, i32),
  /// The border on the left of a cell
  Left(i32, i32),
  /// The border above a cell
  Above(i32, i32),
}

/// The intervals a cell's edges run along one of its sides, in the side's
/// own way (`sides`), from its edges in cell points.
fn along(edges: &[(Point, Point)], side: usize) -> Vec<(i32, i32)> {
  edges
    .iter()
    .filter_map(|&edge| sides::along(edge, side))
    .collect()
}

/// Whether an interval of `cover` holds `lo..hi`.
fn covered(cover: &[(i32, i32)], lo: i32, hi: i32) -> bool {
  cover.iter().any(|&(l, h)| l <= lo && hi <= h)
}

/// The pieces of a border one of its cells runs and the other does not:
/// `first` the cell before it (left of or above it), `second` the one after.
/// Calls `emit(lo, hi, first's)` for each, in border units from its start.
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
    match (covered(first, lo, hi), covered(second, lo, hi)) {
      (true, false) => emit(lo, hi, true),
      (false, true) => emit(lo, hi, false),
      _ => {}
    }
  }
}

/// The boundary edges of `unit`, the cells read through `at`, pushed onto `out`.
fn unit_edges(
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

/// Every boundary edge of the grid, unit by unit, cell by cell.
pub(super) fn boundary_edges(cells: Cells) -> Vec<(Point, Point)> {
  let mut edges = Vec::new();
  let (cols, rows) = cells.extent();
  let at = |x: i32, y: i32| cells.at(x, y);
  for y in 0..=rows {
    for x in 0..=cols {
      if x < cols && y < rows {
        unit_edges(Unit::Inside(x, y), &cells, &at, &mut edges);
      }
      if y < rows {
        unit_edges(Unit::Left(x, y), &cells, &at, &mut edges);
      }
      if x < cols {
        unit_edges(Unit::Above(x, y), &cells, &at, &mut edges);
      }
    }
  }
  edges
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

  /// Take in the cells at `changed` (sorted by index, each with the value it
  /// had when the outlines were last brought up to date): the loops through
  /// a point whose edges changed are dropped and traced again with the new
  /// edges.
  pub(crate) fn apply(&mut self, cells: Cells, changed: &[(u32, TileCell)]) -> Retraced {
    let mut retraced = Retraced::default();
    let width = cells.width();
    if width == 0 || changed.is_empty() {
      return retraced;
    }
    let before = |x: i32, y: i32| cells.before(x, y, changed);
    let now = |x: i32, y: i32| cells.at(x, y);
    // An edit changes the units of its cells only
    let mut units: Vec<Unit> = Vec::with_capacity(changed.len() * 5);
    for &(index, _) in changed {
      let (x, y) = ((index % width) as i32, (index / width) as i32);
      units.extend([
        Unit::Inside(x, y),
        Unit::Left(x, y),
        Unit::Left(x + 1, y),
        Unit::Above(x, y),
        Unit::Above(x, y + 1),
      ]);
    }
    units.sort_unstable();
    units.dedup();
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
      changes.extend(
        was
          .iter()
          .filter(|e| is.binary_search(e).is_err())
          .map(|&(a, b)| (a, b, false)),
      );
      changes.extend(
        is.iter()
          .filter(|e| was.binary_search(e).is_err())
          .map(|&(a, b)| (a, b, true)),
      );
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
    let mut starts = Vec::new();
    for &id in &dropped {
      if let Some(outline) = self.loops.get_mut(id as usize).and_then(Option::take) {
        starts.extend(outline.corners.windows(2).map(|edge| (edge[0], edge[1])));
        retraced.dropped.push(outline.part);
      }
    }
    // Every edge to trace: the dropped loops' (the removed ones were among
    // them) and the new ones
    let mut count = starts.len();
    for (a, b, now) in changes {
      if now {
        self.insert(a, b);
        starts.push((a, b));
        count += 1;
      } else {
        self.remove(a, b);
        count -= 1;
      }
    }
    retraced.traced = self.trace(&starts, &dropped, count);
    // Their ids are free once the trace no longer reads them
    self.free.extend(dropped);
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

  fn find(&self, a: Point, b: Point) -> Option<usize> {
    let step = delta(a, b);
    self
      .leaving
      .get(&key(a))?
      .as_slice()
      .iter()
      .position(|out| out.step() == step)
  }

  fn insert(&mut self, a: Point, b: Point) {
    if self.find(a, b).is_some() {
      debug_assert!(false, "the outline edge {a:?} -> {b:?} is already there");
      return;
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    self.leaving.entry(key(a)).or_default().push(Out {
      dx: dx as i16,
      dy: dy as i16,
      id: UNTRACED,
    });
  }

  fn remove(&mut self, a: Point, b: Point) {
    if let Some(i) = self.find(a, b)
      && let Some(leaving) = self.leaving.get_mut(&key(a))
    {
      leaving.swap_remove(i);
      if leaving.is_empty() {
        self.leaving.remove(&key(a));
      }
    }
  }

  /// Trace the loop through each of `starts` not yet traced (an edge is
  /// untraced while its id is `UNTRACED` or one of `dropped`, sorted),
  /// stopping once `count` edges are traced (all the untraced ones); returns
  /// their ids.
  fn trace(&mut self, starts: &[(Point, Point)], dropped: &[u32], count: usize) -> Vec<u32> {
    let untraced = |id: u32| id == UNTRACED || dropped.binary_search(&id).is_ok();
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
        .find(|out| out.step() == first)
      else {
        continue;
      };
      if !untraced(out.id) {
        continue;
      }
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
/// too), straight runs merged, in tiles: each is a loop of corners whose
/// last repeats its first.
#[cfg(test)]
pub(crate) fn outlines(width: u32, height: u32, cells: &[TileCell]) -> Vec<Vec<super::Corner>> {
  Outlines::build(Cells::bare(width, height, cells))
    .loops()
    .map(|(_, outline)| {
      super::merge_straight_runs(&outline.corners)
        .into_iter()
        .map(|(x, y)| (x / GRID, y / GRID))
        .collect()
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::super::{CellShape, GRID, Point, SlopeCorner, TileColliders, merge_straight_runs};
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
      tiles.set(x, y, cell);
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

  #[test]
  fn shape_edges_cancel_against_solid_and_shape_neighbours() {
    use SlopeCorner::BottomRight;
    let bottom = [(0.0, 0.5), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)];
    let band = [(0.0, 0.25), (1.0, 0.25), (1.0, 0.75), (0.0, 0.75)];
    let loops = |cells: &[TileCell], tiles: &mut TileColliders| {
      for (x, &cell) in cells.iter().enumerate() {
        tiles.set(x as u32, 0, cell);
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
  fn a_loop_split_and_merged_by_edits_matches_a_full_trace() {
    use SlopeCorner::{BottomLeft, BottomRight, TopLeft, TopRight};
    use TileCell::Slope;
    let (w, h) = (12, 8);
    let mut tiles = TileColliders::new(w, h, Vec2::ONE);
    // A floor two cells deep
    for x in 0..w {
      tiles.set(x, 6, S);
      tiles.set(x, 7, S);
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
  }

  /// `count` steps of one to three random cells set to random `kinds`.
  fn walk(
    state: &mut Outlines,
    tiles: &mut TileColliders,
    kinds: &[TileCell],
    mut seed: u32,
    count: usize,
  ) {
    let (w, h) = (tiles.width(), tiles.height());
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
    tiles.take_changes();
    let mut state = Outlines::build(tiles.cells_view());
    walk(&mut state, &mut tiles, &kinds, 0x2545_f491, 1500);
    assert_traced_by_the_rule(&state, &tiles);
  }
}
