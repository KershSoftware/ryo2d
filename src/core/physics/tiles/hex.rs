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
    HexCells {
      axis,
      index,
      half: i64::from(across) / 2,
      slant: raw.max(0),
      step: raw + i64::from(side),
      traceable: cell.x <= MAX_TRACED && cell.y <= MAX_TRACED,
    }
  }

  /// Whether the collision tracer takes these cells: none wider or taller
  /// than 16,384 px
  // used by the tile collision tracer
  #[allow(dead_code)]
  pub(crate) fn traceable(&self) -> bool {
    self.traceable
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
  // used by the tile collision tracer
  #[allow(dead_code)]
  pub(crate) fn across(&self, cell: IVec2, side: usize) -> IVec2 {
    let (x, y) = self.across_at(i64::from(cell.x), i64::from(cell.y), side);
    IVec2::new(clamp_i32(x), clamp_i32(y))
  }

  /// Whether every corner of `cell` and of its neighbours fits the tracer's i32 points
  // used by the tile collision tracer
  #[allow(dead_code)]
  pub(crate) fn reaches(&self, cell: IVec2) -> bool {
    let (x, y) = (i64::from(cell.x), i64::from(cell.y));
    self.fit(x, y).is_some()
      && (0..6).all(|side| {
        let (nx, ny) = self.across_at(x, y, side);
        self.fit(nx, ny).is_some()
      })
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

// used by the tile collision tracer
#[allow(dead_code)]
fn clamp_i32(v: i64) -> i32 {
  v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
