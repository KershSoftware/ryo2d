//! Tile colliders: a grid of cells becomes seamless outlines (or merged
//! rectangles), one-way runs and 45° slopes. Coordinates here are in tiles,
//! +Y down, cell (0, 0)'s top-left at the origin.

use std::collections::{BTreeMap, BTreeSet};

/// One cell of a tile collision grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileCell {
  #[default]
  Empty,
  Solid,
  /// Solid only from above: things jump up through it and land on it
  OneWay,
  /// A 45° half-cell, filled at this corner
  Slope(SlopeCorner),
}

/// Which corner of a slope cell is filled (its right angle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlopeCorner {
  BottomLeft,
  BottomRight,
  TopLeft,
  TopRight,
}

impl SlopeCorner {
  /// From `"bottom-left"`, `"bottom-right"`, `"top-left"` or `"top-right"`
  /// (any case), as a Tiled `slope` property spells it.
  pub fn from_name(name: &str) -> Option<Self> {
    match name.to_ascii_lowercase().as_str() {
      "bottom-left" => Some(Self::BottomLeft),
      "bottom-right" => Some(Self::BottomRight),
      "top-left" => Some(Self::TopLeft),
      "top-right" => Some(Self::TopRight),
      _ => None,
    }
  }
}

/// A tile corner, in tiles.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) type Corner = (i32, i32);

/// A filled cell's corners relative to its top-left, wound the same way
/// (clockwise on screen) so an edge two cells share runs both ways and cancels.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn corners(cell: TileCell) -> &'static [Corner] {
  match cell {
    TileCell::Solid => &[(0, 0), (1, 0), (1, 1), (0, 1)],
    TileCell::Slope(SlopeCorner::BottomLeft) => &[(0, 0), (1, 1), (0, 1)],
    TileCell::Slope(SlopeCorner::BottomRight) => &[(1, 0), (1, 1), (0, 1)],
    TileCell::Slope(SlopeCorner::TopLeft) => &[(0, 0), (1, 0), (0, 1)],
    TileCell::Slope(SlopeCorner::TopRight) => &[(0, 0), (1, 0), (1, 1)],
    TileCell::Empty | TileCell::OneWay => &[],
  }
}

/// The cell at (x, y); missing cells are empty.
#[cfg_attr(not(test), allow(dead_code))]
fn cell(width: u32, cells: &[TileCell], x: u32, y: u32) -> TileCell {
  let index = y as usize * width as usize + x as usize;
  cells.get(index).copied().unwrap_or_default()
}

#[cfg_attr(not(test), allow(dead_code))]
fn sub(a: Corner, b: Corner) -> Corner {
  (a.0 - b.0, a.1 - b.1)
}

/// The signed turn from heading `from` to heading `to`, in radians.
#[cfg_attr(not(test), allow(dead_code))]
fn turn(from: Corner, to: Corner) -> f32 {
  let cross = (from.0 * to.1 - from.1 * to.0) as f32;
  let dot = (from.0 * to.0 + from.1 * to.1) as f32;
  cross.atan2(dot)
}

/// The closed outlines around every region of solid and slope cells (holes
/// too), straight runs merged: each is a loop of corners whose last repeats
/// its first.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn outlines(width: u32, height: u32, cells: &[TileCell]) -> Vec<Vec<Corner>> {
  // Every filled cell adds its edges; an edge two cells share appears once
  // each way and cancels, leaving only the boundary
  let mut edges: BTreeSet<(Corner, Corner)> = BTreeSet::new();
  for y in 0..height {
    for x in 0..width {
      let shape = corners(cell(width, cells, x, y));
      for i in 0..shape.len() {
        let at = |c: Corner| (x as i32 + c.0, y as i32 + c.1);
        let (a, b) = (at(shape[i]), at(shape[(i + 1) % shape.len()]));
        if !edges.remove(&(b, a)) {
          edges.insert((a, b));
        }
      }
    }
  }
  let mut outgoing: BTreeMap<Corner, Vec<Corner>> = BTreeMap::new();
  for &(a, b) in &edges {
    outgoing.entry(a).or_default().push(b);
  }
  let mut loops = Vec::new();
  while let Some(&start) = outgoing.keys().next() {
    let mut points = vec![start];
    let (mut at, mut heading) = (start, None);
    while let Some(nexts) = outgoing.get_mut(&at) {
      // Where two regions touch at a corner, the sharpest turn keeps their
      // outlines apart
      let pick = match heading {
        Some(heading) if nexts.len() > 1 => (0..nexts.len())
          .max_by(|&i, &j| {
            turn(heading, sub(nexts[i], at)).total_cmp(&turn(heading, sub(nexts[j], at)))
          })
          .unwrap_or(0),
        _ => 0,
      };
      let next = nexts.swap_remove(pick);
      if nexts.is_empty() {
        outgoing.remove(&at);
      }
      heading = Some(sub(next, at));
      at = next;
      points.push(at);
      if at == start {
        break;
      }
    }
    loops.push(merge_straight_runs(&points));
  }
  loops
}

/// Drop the corners in the middle of straight runs; `points` is closed
/// (its last repeats its first) and so is the result.
#[cfg_attr(not(test), allow(dead_code))]
fn merge_straight_runs(points: &[Corner]) -> Vec<Corner> {
  let ring = &points[..points.len().saturating_sub(1)];
  let n = ring.len();
  let mut kept: Vec<Corner> = (0..n)
    .filter(|&i| {
      let (a, b) = (
        sub(ring[i], ring[(i + n - 1) % n]),
        sub(ring[(i + 1) % n], ring[i]),
      );
      let straight = a.0 * b.1 - a.1 * b.0 == 0 && a.0 * b.0 + a.1 * b.1 > 0;
      !straight
    })
    .map(|i| ring[i])
    .collect();
  if let Some(&first) = kept.first() {
    kept.push(first);
  }
  kept
}

/// Each horizontal run of one-way cells as the left and right ends of its top edge.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn one_way_runs(width: u32, height: u32, cells: &[TileCell]) -> Vec<(Corner, Corner)> {
  let mut runs = Vec::new();
  for y in 0..height {
    let mut x = 0;
    while x < width {
      if cell(width, cells, x, y) != TileCell::OneWay {
        x += 1;
        continue;
      }
      let start = x;
      while x < width && cell(width, cells, x, y) == TileCell::OneWay {
        x += 1;
      }
      runs.push(((start as i32, y as i32), (x as i32, y as i32)));
    }
  }
  runs
}

/// Solid cells greedily merged into rectangles: (min corner, size).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn solid_rects(width: u32, height: u32, cells: &[TileCell]) -> Vec<(Corner, Corner)> {
  let solid = |x: u32, y: u32| cell(width, cells, x, y) == TileCell::Solid;
  let mut taken = vec![false; width as usize * height as usize];
  let index = |x: u32, y: u32| y as usize * width as usize + x as usize;
  let mut rects = Vec::new();
  for y in 0..height {
    for x in 0..width {
      if !solid(x, y) || taken[index(x, y)] {
        continue;
      }
      let mut w = 1;
      while x + w < width && solid(x + w, y) && !taken[index(x + w, y)] {
        w += 1;
      }
      let mut h = 1;
      while y + h < height && (x..x + w).all(|cx| solid(cx, y + h) && !taken[index(cx, y + h)]) {
        h += 1;
      }
      for ty in y..y + h {
        for tx in x..x + w {
          taken[index(tx, ty)] = true;
        }
      }
      rects.push(((x as i32, y as i32), (w as i32, h as i32)));
    }
  }
  rects
}
#[cfg(test)]
mod tests {
  use super::*;
  use TileCell::{Empty as E, OneWay as O, Solid as S};

  /// A loop as a set of directed edges, so tests do not depend on where it starts.
  fn edges(outline: &[Corner]) -> std::collections::BTreeSet<(Corner, Corner)> {
    outline.windows(2).map(|w| (w[0], w[1])).collect()
  }

  #[test]
  fn one_cell_is_one_square() {
    let loops = outlines(1, 1, &[S]);
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].len(), 5, "four corners, closed");
    assert_eq!(loops[0].first(), loops[0].last());
  }

  #[test]
  fn a_row_merges_into_one_rectangle() {
    let loops = outlines(3, 1, &[S, S, S]);
    assert_eq!(loops.len(), 1);
    assert_eq!(
      edges(&loops[0]),
      edges(&[(0, 0), (3, 0), (3, 1), (0, 1), (0, 0)])
    );
  }

  #[test]
  fn an_l_shape_has_six_corners() {
    let loops = outlines(2, 2, &[S, E, S, S]);
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].len(), 7);
  }

  #[test]
  fn a_ring_has_an_outline_and_a_hole() {
    let loops = outlines(3, 3, &[S, S, S, S, E, S, S, S, S]);
    assert_eq!(loops.len(), 2);
    assert!(loops.iter().all(|l| l.len() == 5));
  }

  #[test]
  fn cells_touching_at_a_corner_stay_two_outlines() {
    let loops = outlines(2, 2, &[S, E, E, S]);
    assert_eq!(loops.len(), 2);
    assert!(loops.iter().all(|l| l.len() == 5), "{loops:?}");
  }

  #[test]
  fn a_slope_meets_the_floor_without_a_seam() {
    // Top row: empty, a slope filled at its bottom right; bottom row solid
    let cells = [E, TileCell::Slope(SlopeCorner::BottomRight), S, S];
    let loops = outlines(2, 2, &cells);
    assert_eq!(loops.len(), 1);
    assert_eq!(
      edges(&loops[0]),
      edges(&[(0, 1), (1, 1), (2, 0), (2, 2), (0, 2), (0, 1)])
    );
  }

  #[test]
  fn one_way_cells_make_runs_along_their_tops() {
    let runs = one_way_runs(6, 2, &[O, O, O, E, O, O, E, E, E, E, E, E]);
    assert_eq!(runs, [((0, 0), (3, 0)), ((4, 0), (6, 0))]);
  }

  #[test]
  fn solid_cells_merge_into_few_rectangles() {
    assert_eq!(solid_rects(2, 2, &[S, S, S, S]), [((0, 0), (2, 2))]);
    assert_eq!(solid_rects(3, 1, &[S, E, S]).len(), 2);
  }

  #[test]
  fn a_short_cell_list_counts_as_empty() {
    assert!(outlines(4, 4, &[S]).len() == 1);
    assert!(outlines(0, 0, &[]).is_empty());
  }

  #[test]
  fn slope_names_parse() {
    assert_eq!(
      SlopeCorner::from_name("bottom-left"),
      Some(SlopeCorner::BottomLeft)
    );
    assert_eq!(
      SlopeCorner::from_name("TOP-RIGHT"),
      Some(SlopeCorner::TopRight)
    );
    assert_eq!(SlopeCorner::from_name("sideways"), None);
  }
}
