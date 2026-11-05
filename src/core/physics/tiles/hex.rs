//! Hexagonal cells in whole pixels: the corners and the cell across each
//! side, as integers, so the collision tracer and the map's outlines read
//! the same points. The rules are Tiled's, as `MapGrid` measures them.

use glam::{I64Vec2, IVec2, UVec2};

/// Which lines of a staggered or hexagonal grid are shifted by half a
/// cell: its rows (Y) or its columns (X)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StaggerAxis {
  X,
  Y,
}

/// Which of those lines are shifted, counting from 0 (negative lines
/// alike): the odd ones or the even ones
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StaggerIndex {
  Odd,
  Even,
}

/// The largest cell side the collision tracer takes, in pixels
const MAX_TRACED: u32 = 16_384;

/// A hexagonal grid's cells in whole pixels from the grid's origin
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HexCells {
  axis: StaggerAxis,
  index: StaggerIndex,
  /// Half a cell across the stagger axis (rounded down)
  half: i64,
  /// How far along the axis the slanted sides reach (0 where the side is
  /// longer than the cell)
  slant: i64,
  /// The step from a line to the next
  step: i64,
  traceable: bool,
}

impl HexCells {
  /// The geometry of `cell`-sized hexagons with a `side` along the axis
  pub(crate) fn new(axis: StaggerAxis, index: StaggerIndex, cell: UVec2, side: u32) -> HexCells {
    let (across, along) = match axis {
      StaggerAxis::Y => (cell.x, cell.y),
      StaggerAxis::X => (cell.y, cell.x),
    };
    // Rust's division, like Tiled's, rounds toward zero
    let raw = (i64::from(along) - i64::from(side)) / 2;
    let (half, slant, step) = (i64::from(across) / 2, raw.max(0), raw + i64::from(side));
    // The hexagon's own reach along the axis passes its tile's where its
    // side is longer than the tile
    let cap = i64::from(MAX_TRACED);
    HexCells {
      axis,
      index,
      half,
      slant,
      step,
      traceable: cell.x <= MAX_TRACED
        && cell.y <= MAX_TRACED
        && half > 0
        && step > 0
        && step + slant <= cap,
    }
  }

  /// Whether the collision tracer takes these cells: none wider or taller
  /// than 16,384 px (the hexagon itself, where a side longer than its tile
  /// spreads it), and none that are flat: at least 2 px across and a step
  /// of a pixel from line to line. So a chunk of 32 cells is at most 2^19
  /// px a side, and its parts' points stay small
  pub(crate) fn traceable(&self) -> bool {
    self.traceable
  }

  /// Its stagger axis: lines are rows on Y, columns on X
  pub(crate) fn axis(&self) -> StaggerAxis {
    self.axis
  }

  /// The corner of `cell`'s box nearest the origin, unshifted (its position
  /// times a cell across, its line times a step), from the grid's origin:
  /// each corner of a cell, and of every cell after it in its line and
  /// lines, is at or past it either way, so a tile part sits at its first
  /// cell's (an even line count keeping the stagger's phase)
  pub(crate) fn corner_of(&self, cell: IVec2) -> I64Vec2 {
    let (line, pos) = self.split(i64::from(cell.x), i64::from(cell.y));
    let (u, v) = (pos * 2 * self.half, line * self.step);
    match self.axis {
      StaggerAxis::Y => I64Vec2::new(u, v),
      StaggerAxis::X => I64Vec2::new(v, u),
    }
  }

  /// Clockwise on screen from the top-most corner, as `CellOutline` gives
  /// them; a side of length 0 repeats its corner. Exact for any cell size
  /// and any cell (worked in `i128`; a corner past `i64` saturates, which
  /// only the very last cells of the largest sizes reach)
  pub(crate) fn corners(&self, cell: IVec2) -> [I64Vec2; 6] {
    self
      .corners_wide(i64::from(cell.x), i64::from(cell.y))
      .map(|(x, y)| I64Vec2::new(narrow(x), narrow(y)))
  }

  /// The corners as `i32` points, `None` where they don't fit
  // used by the tile collision tracer
  #[allow(dead_code)]
  pub(crate) fn points(&self, cell: IVec2) -> Option<[IVec2; 6]> {
    self.fit(i64::from(cell.x), i64::from(cell.y))
  }

  /// The cell across side `side` (corner `side` to corner `side + 1`), 0..6
  pub(crate) fn across(&self, cell: IVec2, side: usize) -> IVec2 {
    let (x, y) = self.across_at(i64::from(cell.x), i64::from(cell.y), side);
    IVec2::new(clamp_i32(x), clamp_i32(y))
  }

  /// Whether every corner of `cell` and of its neighbours fits the tracer's
  /// i32 points. Those corners span from a cell across before the cell's
  /// box to two after it, and from a step before its line to its slanted
  /// sides' end a step after: the box is worked out, not each corner
  pub(crate) fn reaches(&self, cell: IVec2) -> bool {
    let (line, pos) = self.split(i64::from(cell.x), i64::from(cell.y));
    let (h, slant, step) = (
      i128::from(self.half),
      i128::from(self.slant),
      i128::from(self.step),
    );
    let u = i128::from(pos) * 2 * h + i128::from(self.shift(line));
    let v = i128::from(line) * step;
    let fits = |lo: i128, hi: i128| lo >= i128::from(i32::MIN) && hi <= i128::from(i32::MAX);
    fits(u - 2 * h, u + 4 * h) && fits(v - step, v + 2 * step + slant)
  }

  fn fit(&self, x: i64, y: i64) -> Option<[IVec2; 6]> {
    let mut out = [IVec2::ZERO; 6];
    for (slot, (cx, cy)) in out.iter_mut().zip(self.corners_wide(x, y)) {
      *slot = IVec2::new(i32::try_from(cx).ok()?, i32::try_from(cy).ok()?);
    }
    Some(out)
  }

  /// (line, position in it)
  fn split(&self, x: i64, y: i64) -> (i64, i64) {
    match self.axis {
      StaggerAxis::Y => (y, x),
      StaggerAxis::X => (x, y),
    }
  }

  fn shift(&self, line: i64) -> i64 {
    if (line.rem_euclid(2) == 1) == (self.index == StaggerIndex::Odd) {
      self.half
    } else {
      0
    }
  }

  /// Built in the frame of axis Y, where a cell's top is a corner; axis X
  /// swaps it into place and turns it back clockwise
  fn corners_wide(&self, x: i64, y: i64) -> [(i128, i128); 6] {
    let (line, pos) = self.split(x, y);
    let u0 = i128::from(pos) * 2 * i128::from(self.half) + i128::from(self.shift(line));
    let v0 = i128::from(line) * i128::from(self.step);
    let (h, slant, step) = (
      i128::from(self.half),
      i128::from(self.slant),
      i128::from(self.step),
    );
    let frame = [
      (h, 0),
      (2 * h, slant),
      (2 * h, step),
      (h, step + slant),
      (0, step),
      (0, slant),
    ];
    let mut out = frame.map(|(u, v)| match self.axis {
      StaggerAxis::Y => (u0 + u, v0 + v),
      StaggerAxis::X => (v0 + v, u0 + u),
    });
    if self.axis == StaggerAxis::X {
      out.reverse();
    }
    out
  }

  /// How far a line's offsets run ahead of its axial coordinate
  fn half_line(&self, line: i64) -> i64 {
    match self.index {
      StaggerIndex::Odd => line.div_euclid(2),
      StaggerIndex::Even => (line + 1).div_euclid(2),
    }
  }

  fn across_at(&self, x: i64, y: i64, side: usize) -> (i64, i64) {
    // The axial way across each side, clockwise from the top
    const Y: [(i64, i64); 6] = [(1, -1), (1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1)];
    const X: [(i64, i64); 6] = [(0, -1), (1, -1), (1, 0), (0, 1), (-1, 1), (-1, 0)];
    let (line, pos) = self.split(x, y);
    let other = pos - self.half_line(line);
    let (q, r) = match self.axis {
      StaggerAxis::Y => (other, line),
      StaggerAxis::X => (line, other),
    };
    let (dq, dr) = match self.axis {
      StaggerAxis::Y => Y[side % 6],
      StaggerAxis::X => X[side % 6],
    };
    let (q, r) = (q + dq, r + dr);
    let (line, other) = match self.axis {
      StaggerAxis::Y => (r, q),
      StaggerAxis::X => (q, r),
    };
    let pos = other + self.half_line(line);
    match self.axis {
      StaggerAxis::Y => (pos, line),
      StaggerAxis::X => (line, pos),
    }
  }
}

fn narrow(v: i128) -> i64 {
  v.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

fn clamp_i32(v: i64) -> i32 {
  v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Every grid the tests here hold the helpers to, small and huge cells
  /// among them
  fn grids() -> Vec<HexCells> {
    let mut out = Vec::new();
    for ((w, h), side) in [
      ((16, 16), 8),
      ((120, 140), 70),
      ((15, 17), 7),
      ((16, 16), 0),
      ((16, 16), 24),
      ((16_384, 16_384), 8192),
      ((40_000, 30_000), 9),
    ] {
      for axis in [StaggerAxis::X, StaggerAxis::Y] {
        for index in [StaggerIndex::Odd, StaggerIndex::Even] {
          out.push(HexCells::new(axis, index, UVec2::new(w, h), side));
        }
      }
    }
    out
  }

  #[test]
  fn hex_reach_is_whether_every_corner_of_a_cell_and_its_neighbours_fits() {
    let fits = |hex: &HexCells, cell: IVec2| {
      let all = |c: IVec2| {
        hex
          .corners(c)
          .iter()
          .all(|p| i32::try_from(p.x).is_ok() && i32::try_from(p.y).is_ok())
      };
      // The cells across, unclamped: a cell at i32's end has neighbours past it
      let wide = |side: usize| {
        let (x, y) = hex.across_at(i64::from(cell.x), i64::from(cell.y), side);
        hex.fit(x, y).is_some()
      };
      all(cell) && (0..6).all(wide)
    };
    for hex in grids() {
      // About each grid's limits on either axis, and near the origin
      let limit = |v: i64| i32::try_from(v).unwrap_or(i32::MAX);
      let step = i64::from(i32::MAX) / hex.step.max(1);
      let across = i64::from(i32::MAX) / (2 * hex.half).max(1);
      let mut at: Vec<i32> = vec![
        0,
        1,
        -1,
        2,
        -2,
        i32::MAX,
        i32::MIN,
        i32::MAX - 1,
        i32::MIN + 1,
      ];
      for end in [step, across] {
        for d in -3..=3 {
          at.push(limit(end + d));
          at.push(limit(-end + d));
        }
      }
      for &x in &at {
        for &y in &at {
          let cell = IVec2::new(x, y);
          assert_eq!(hex.reaches(cell), fits(&hex, cell), "{hex:?} {cell}");
        }
      }
    }
  }

  #[test]
  fn hex_parts_boxes_start_at_their_first_cell_s_corner() {
    // A part's box: a chunk's (32 cells across by 32 lines along) or a
    // block's, from its first cell's corner, holds every corner its cells
    // have, within a cell past its far sides
    for hex in grids().into_iter().filter(HexCells::traceable) {
      let size = |n: i64| {
        let (across, along) = (n * 2 * hex.half, n * hex.step);
        match hex.axis {
          StaggerAxis::Y => I64Vec2::new(across, along),
          StaggerAxis::X => I64Vec2::new(along, across),
        }
      };
      for (side, first) in [
        (32, IVec2::ZERO),
        (32, IVec2::new(-32, 64)),
        (32, IVec2::new(96, -160)),
        (16, IVec2::new(-16, 48)),
        (8, IVec2::new(40, -8)),
      ] {
        let corner = hex.corner_of(first);
        assert_eq!(
          corner,
          first.as_i64vec2() / i64::from(side) * size(i64::from(side)),
          "{hex:?} {first}"
        );
        for y in 0..side {
          for x in 0..side {
            for p in hex.corners(first + IVec2::new(x, y)) {
              let at = p - corner;
              assert!(
                at.cmpge(I64Vec2::ZERO).all() && at.cmple(size(i64::from(side)) + size(1)).all(),
                "{hex:?} {first} {x} {y}: {at}"
              );
            }
          }
        }
      }
    }
  }

  #[test]
  fn hex_cells_too_thin_flat_or_long_for_the_tracer_are_not_traceable() {
    let cells =
      |w, h, side| HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(w, h), side);
    assert!(cells(16, 16, 8).traceable());
    assert!(cells(2, 1, 1).traceable());
    // Half a cell across rounds to nothing, or no step from line to line
    assert!(!cells(1, 16, 8).traceable());
    assert!(!cells(16, 1, 0).traceable());
    assert!(!cells(16, 0, 0).traceable());
    // A side so long the hexagon is taller than 16,384 px, though its tile is not
    assert!(cells(16, 16, 32_000).traceable());
    assert!(!cells(16, 16, 32_768).traceable());
    // The same along x
    let long = |side| HexCells::new(StaggerAxis::X, StaggerIndex::Even, UVec2::new(16, 16), side);
    assert!(long(32_000).traceable());
    assert!(!long(32_768).traceable());
  }
}
