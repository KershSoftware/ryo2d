//! A navigation grid over a tile layer's own cells: how its cells
//! neighbour and where they are in the world, measured once
//! (`LayerCells`), the window's cap, and the convex-polygon tests that
//! `block_tiles` and `block_rect` place cells with.

use crate::core::ecs::{IVec2, Vec2};
use crate::core::physics::{CellRect, HexCells, TileColliders};
use crate::core::tilemap::{GridShape, Lattice, LayerGrid, StaggerAxis};
use glam::{Affine2, DAffine2, DMat2, DVec2};
use std::sync::atomic::{AtomicBool, Ordering};

/// The most cells a layer grid's window holds: past it, each side longer
/// than `WINDOW_SIDE` is cut to that about the window's centre
const MAX_WINDOW_CELLS: u64 = 4_194_304;
const WINDOW_SIDE: u32 = 2048;

/// A window was cut down (and logged): once a run, as a game may take a grid
/// again after every edit
static WINDOW_WARNED: AtomicBool = AtomicBool::new(false);

/// How near two transforms' elements are for tiles placed by one to be
/// read as the cells of a layer placed by the other
const SAME_TRANSFORM: f32 = 1e-4;

/// How deep two polygons may overlap along an axis, against the longer of
/// their extents on it, and still only touch: the rounding that points
/// taken through a turned transform carry where their edges meet
const SPILL: f64 = 1e-6;

/// The eight ways round a cell in a square lattice, cardinals first, in the
/// square grid's order
pub(super) const WAYS: [(i32, i32); 8] = [
  (-1, 0),
  (1, 0),
  (0, -1),
  (0, 1),
  (-1, -1),
  (1, -1),
  (-1, 1),
  (1, 1),
];

/// How a layer grid's cells neighbour
#[derive(Debug, Clone, Copy)]
pub(super) enum Ways {
  /// The eight ways round a cell of the grid's square lattice
  Lattice(Lattice),
  /// The six ways round a hexagon (`MapGrid::neighbours`), its lines rows
  /// on axis Y and columns on axis X
  Hex(StaggerAxis),
}

/// A layer grid's cells: how they neighbour, and where they are in the world
#[derive(Debug, Clone, Copy)]
pub(super) struct LayerCells {
  pub(super) grid: LayerGrid,
  pub(super) ways: Ways,
  /// The layer's pixels to the world, its right angles squared (`squared`)
  pub(super) to_world: DAffine2,
  /// The world to the layer's pixels; `None` where the layer is scaled to
  /// nothing
  to_local: Option<DAffine2>,
  /// The lattice cells the window's cells take (the window itself, but on a
  /// staggered grid)
  pub(super) lattice_window: CellRect,
  /// The world offset between cells' centres per lattice cell, or on hex
  /// per cell along a line and per line, over the shortest step
  per: DMat2,
  /// The world offset of a hexagon on an odd line beyond what `per` gives,
  /// over the shortest step
  odd: DVec2,
  /// The shortest world distance between neighbouring cells' centres (1 on
  /// a grid scaled or folded to nothing)
  pub(super) step: f64,
  /// Each way's step cost, its length over the shortest: in `WAYS` order,
  /// or on hex in `MapGrid::neighbours`' order
  pub(super) costs: [f32; 8],
}

impl LayerCells {
  /// The grid's cells' geometry, its steps measured once: the centres are
  /// affine in lattice cells, and on hex in a cell's position along its
  /// line and its line (but for odd lines' shift). A layer scaled to
  /// nothing, or whose transform is not finite, has no steps to measure:
  /// each costs 1, and every offset between centres is 0
  pub(super) fn new(grid: LayerGrid, window: CellRect) -> LayerCells {
    let to_world = squared(grid.to_world).as_daffine2();
    let turn = to_world.matrix2;
    let shape = grid.grid;
    let (ways, per, odd, offsets, count) = match shape.lattice() {
      // Hexagons, which make no lattice
      None => {
        let axis = match shape.shape {
          GridShape::Hexagonal { axis, .. } => axis,
          _ => StaggerAxis::Y,
        };
        let local = shape.with_origin(Vec2::ZERO);
        let centre = |pos: i32, line: i32| local.cell_center(hex_cell(axis, pos, line)).as_dvec2();
        let zero = centre(0, 0);
        let (along, one_line, two_lines) = (
          centre(1, 0) - zero,
          centre(0, 1) - zero,
          centre(0, 2) - zero,
        );
        let mut offsets = [DVec2::ZERO; 8];
        for (offset, next) in offsets.iter_mut().zip(local.neighbours(IVec2::ZERO)) {
          *offset = local.cell_center(next).as_dvec2() - zero;
        }
        let per = DMat2::from_cols(along, two_lines / 2.0);
        (Ways::Hex(axis), per, one_line - two_lines / 2.0, offsets, 6)
      }
      Some(lattice) => {
        let per = lattice.to_pixel.as_daffine2().matrix2;
        let offsets = WAYS.map(|(x, y)| per * DVec2::new(f64::from(x), f64::from(y)));
        (Ways::Lattice(lattice), per, DVec2::ZERO, offsets, 8)
      }
    };
    let lengths = offsets.map(|offset| (turn * offset).length());
    let shortest = lengths[..count]
      .iter()
      .copied()
      .fold(f64::INFINITY, f64::min);
    let measured = to_world.is_finite() && shortest.is_finite() && shortest > 1e-9;
    let (step, per, odd, costs) = if measured {
      let scale = 1.0 / shortest;
      (
        shortest,
        turn * per * scale,
        turn * odd * scale,
        lengths.map(|length| (length * scale) as f32),
      )
    } else {
      (1.0, DMat2::ZERO, DVec2::ZERO, [1.0; 8])
    };
    let invertible = to_world.is_finite() && turn.determinant() != 0.0;
    LayerCells {
      grid,
      ways,
      to_world,
      to_local: invertible.then(|| to_world.inverse()),
      lattice_window: match ways {
        Ways::Lattice(lattice) => lattice_span(lattice, window),
        Ways::Hex(_) => window,
      },
      per,
      odd,
      step,
      costs,
    }
  }

  /// `cell`'s world centre from cell (0, 0)'s, over the shortest step
  pub(super) fn offset(&self, cell: IVec2) -> DVec2 {
    match self.ways {
      Ways::Lattice(lattice) => self.per * lattice.lattice_cell(cell).as_dvec2(),
      Ways::Hex(axis) => {
        let (pos, line) = match axis {
          StaggerAxis::Y => (cell.x, cell.y),
          StaggerAxis::X => (cell.y, cell.x),
        };
        self.per * DVec2::new(f64::from(pos), f64::from(line))
          + self.odd * f64::from(line.rem_euclid(2))
      }
    }
  }

  /// The grid cell of a cell of `near`'s rectangle (a lattice cell, but on
  /// hex)
  pub(super) fn grid_cell(&self, cell: IVec2) -> IVec2 {
    match self.ways {
      Ways::Lattice(lattice) => lattice.grid_cell(cell),
      Ways::Hex(_) => cell,
    }
  }

  /// `cell`'s outline in the world and its length
  pub(super) fn outline(&self, cell: IVec2) -> ([DVec2; 6], usize) {
    let outline = self.grid.grid.cell_outline(cell);
    let points = outline.points();
    let mut out = [DVec2::ZERO; 6];
    for (point, corner) in out.iter_mut().zip(points) {
      *point = self.to_world.transform_point2(corner.as_dvec2());
    }
    (out, points.len())
  }

  /// The cells whose outlines may meet the world box `lo` to `hi`, inside
  /// `window`: a rectangle of lattice cells (`grid_cell` takes each to its
  /// grid cell), or of cells on hex
  pub(super) fn near(&self, lo: DVec2, hi: DVec2, window: CellRect) -> Option<CellRect> {
    let (lo, hi) = mapped_bounds(self.to_local?, lo, hi)?;
    match self.ways {
      Ways::Lattice(lattice) => {
        let to_pixel = lattice.to_pixel.as_daffine2();
        if to_pixel.matrix2.determinant() == 0.0 {
          return None;
        }
        let (lo, hi) = mapped_bounds(to_pixel.inverse(), lo, hi)?;
        Some(meet(lattice_cells(lo, hi), self.lattice_window))
      }
      Ways::Hex(_) => {
        let hex = self.grid.grid.hex_cells()?;
        let origin = self.grid.grid.origin.as_dvec2();
        Some(meet(hex_boxes(hex, lo - origin, hi - origin)?, window))
      }
    }
  }

  /// Whether `tiles`, placed by `to_world`, are this layer's own cells:
  /// `Some` with the lattice that takes a tile cell to its grid cell where
  /// the tiles hold lattice cells, `None` where they are other cells
  pub(super) fn same_cells(
    &self,
    tiles: &TileColliders,
    to_world: Affine2,
  ) -> Option<Option<Lattice>> {
    let near = self
      .grid
      .to_world
      .to_cols_array()
      .iter()
      .zip(to_world.to_cols_array())
      .all(|(a, b)| (a - b).abs() <= SAME_TRANSFORM);
    if !near {
      return None;
    }
    let grid = self.grid.grid;
    match (tiles.hex(), tiles.lattice(), grid.lattice()) {
      (Some((hex, origin)), _, _) => {
        (grid.hex_cells() == Some(hex) && grid.origin == origin).then_some(None)
      }
      (None, Some(to_pixel), Some(lattice)) => {
        (lattice.to_pixel == to_pixel).then_some(Some(lattice))
      }
      (None, None, Some(lattice)) => {
        let size = tiles.tile_size;
        let rects = Affine2::from_cols(Vec2::new(size.x, 0.0), Vec2::new(0.0, size.y), Vec2::ZERO);
        (lattice.to_pixel == rects).then_some(Some(lattice))
      }
      _ => None,
    }
  }
}

/// `window`, cut down about its centre if it holds over
/// `MAX_WINDOW_CELLS` (logged the first time in a run): each side longer
/// than `WINDOW_SIDE` to it
pub(super) fn capped(window: CellRect) -> CellRect {
  let size = window.size();
  if u64::from(size.x) * u64::from(size.y) <= MAX_WINDOW_CELLS {
    return window;
  }
  if !WINDOW_WARNED.swap(true, Ordering::Relaxed) {
    log::warn!(
      "a navigation window of {} x {} cells is over the 4,194,304 cells a grid holds: it keeps the middle {} x {} (logged once)",
      size.x,
      size.y,
      size.x.min(WINDOW_SIDE),
      size.y.min(WINDOW_SIDE)
    );
  }
  let side = |min: i32, len: u32| {
    let cut = len.saturating_sub(WINDOW_SIDE);
    let start = i64::from(min) + i64::from(cut / 2);
    (start as i32, (start + i64::from(len - cut)) as i32)
  };
  let (x0, x1) = side(window.min.x, size.x);
  let (y0, y1) = side(window.min.y, size.y);
  CellRect::new(IVec2::new(x0, y0), IVec2::new(x1, y1))
}

/// A hexagonal grid's cell by its position along its line and its line
fn hex_cell(axis: StaggerAxis, pos: i32, line: i32) -> IVec2 {
  match axis {
    StaggerAxis::Y => IVec2::new(pos, line),
    StaggerAxis::X => IVec2::new(line, pos),
  }
}

/// The lattice cells of the window's cells: their box, which its corner
/// cells reach (a staggered cell's lattice cell grows with its column and
/// row, one coordinate or the other the other way)
fn lattice_span(lattice: Lattice, window: CellRect) -> CellRect {
  if window.is_empty() {
    return CellRect::default();
  }
  let last = window.max - IVec2::ONE;
  let corners = [
    window.min,
    IVec2::new(last.x, window.min.y),
    IVec2::new(window.min.x, last.y),
    last,
  ]
  .map(|cell| lattice.lattice_cell(cell));
  let lo = corners.iter().fold(IVec2::MAX, |lo, &c| lo.min(c));
  let hi = corners.iter().fold(IVec2::MIN, |hi, &c| hi.max(c));
  CellRect::new(lo, hi.saturating_add(IVec2::ONE))
}

/// The hexagons that may meet the box `lo` to `hi` (from the grid's
/// origin): a cell's unshifted box corner (`HexCells::corner_of`) steps a
/// cell across along one axis and a line along the other, and its hexagon
/// lies within a cell and a half across and two lines along of it
pub(super) fn hex_boxes(hex: HexCells, lo: DVec2, hi: DVec2) -> Option<CellRect> {
  let size = DVec2::new(
    hex.corner_of(IVec2::X).x as f64,
    hex.corner_of(IVec2::Y).y as f64,
  );
  boxes(lo, hi, size)
}

/// The cells of boxes of `size` from (0, 0) that may meet the box `lo` to
/// `hi`, two more each way; `None` for boxes of no size
pub(super) fn boxes(lo: DVec2, hi: DVec2, size: DVec2) -> Option<CellRect> {
  if !(size.is_finite() && size.min_element() > 0.0) {
    return None;
  }
  let (lo, hi) = ((lo / size).floor(), (hi / size).floor());
  Some(CellRect::new(
    (lo.as_ivec2()).saturating_sub(IVec2::splat(2)),
    (hi.as_ivec2()).saturating_add(IVec2::splat(3)),
  ))
}

/// The lattice cells (unit squares) that may meet the box `lo` to `hi` in
/// lattice units, one more each way
pub(super) fn lattice_cells(lo: DVec2, hi: DVec2) -> CellRect {
  CellRect::new(
    lo.floor().as_ivec2().saturating_sub(IVec2::ONE),
    hi.floor().as_ivec2().saturating_add(IVec2::splat(2)),
  )
}

/// The cells two rectangles share
fn meet(a: CellRect, b: CellRect) -> CellRect {
  CellRect::new(a.min.max(b.min), a.max.min(b.max))
}

/// The box round `points`; `None` for none, or any not finite
pub(super) fn bounds(points: impl IntoIterator<Item = DVec2>) -> Option<(DVec2, DVec2)> {
  let mut points = points.into_iter();
  let first = points.next()?;
  let (lo, hi) = points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
  (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

/// The box round the box `lo` to `hi` taken through `to`
pub(super) fn mapped_bounds(to: DAffine2, lo: DVec2, hi: DVec2) -> Option<(DVec2, DVec2)> {
  bounds([lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)].map(|p| to.transform_point2(p)))
}

/// Whether two convex polygons share some area, or with `edges`, meet at
/// all (an edge or a corner touching counts): no axis of either's edges,
/// nor with `edges` the world's axes, parts them. Shared area needs more
/// depth on every axis than rounding leaves where edges meet (`SPILL`)
pub(super) fn overlap(a: &[DVec2], b: &[DVec2], edges: bool) -> bool {
  if !edges && (area(a) == 0.0 || area(b) == 0.0) {
    return false;
  }
  fn sides(polygon: &[DVec2]) -> impl Iterator<Item = DVec2> + '_ {
    (0..polygon.len()).map(move |i| polygon[(i + 1) % polygon.len()] - polygon[i])
  }
  let world = if edges {
    [DVec2::X, DVec2::Y]
  } else {
    [DVec2::ZERO; 2]
  };
  let axes = sides(a)
    .chain(sides(b))
    .map(DVec2::perp)
    .chain(world)
    .filter(|axis| *axis != DVec2::ZERO);
  let project = |polygon: &[DVec2], axis: DVec2| {
    polygon
      .iter()
      .map(|p| p.dot(axis))
      .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), d| {
        (lo.min(d), hi.max(d))
      })
  };
  for axis in axes {
    let ((a0, a1), (b0, b1)) = (project(a, axis), project(b, axis));
    let (lo, hi) = (a0.max(b0), a1.min(b1));
    if hi < lo || (!edges && hi - lo <= SPILL * (a1 - a0).max(b1 - b0)) {
      return false;
    }
  }
  true
}

/// A polygon's area, either way round
fn area(polygon: &[DVec2]) -> f64 {
  let twice: f64 = (0..polygon.len())
    .map(|i| polygon[i].perp_dot(polygon[(i + 1) % polygon.len()]))
    .sum();
  twice.abs() / 2.0
}
/// `to` with each column of its matrix that lies within a millionth of an
/// axis laid on that axis: f32 can't turn by a right angle exactly (its
/// `sin_cos` leaves about 4e-8 where 0 belongs), and a map turned by one
/// should meet the cells it lines up with along their edges, however far out
pub(super) fn squared(to: Affine2) -> Affine2 {
  let square = |v: Vec2| {
    if v.y.abs() <= v.x.abs() * 1e-6 {
      Vec2::new(v.x, 0.0)
    } else if v.x.abs() <= v.y.abs() * 1e-6 {
      Vec2::new(0.0, v.y)
    } else {
      v
    }
  };
  Affine2::from_cols(
    square(to.matrix2.x_axis),
    square(to.matrix2.y_axis),
    to.translation,
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::navigation::NavigationGrid;
  use crate::core::tilemap::MapGrid;
  use glam::UVec2;

  #[test]
  fn a_window_past_four_million_cells_shrinks_and_warns_once() {
    let warnings = || {
      crate::core::dev_log::get_entries()
        .iter()
        .filter(|e| e.message.contains("4,194,304 cells"))
        .count()
    };
    let before = warnings();
    let layer = LayerGrid {
      grid: MapGrid::new(GridShape::Orthogonal, UVec2::splat(16)),
      to_world: Affine2::IDENTITY,
    };
    // 3000 x 3000 cells keep the 2048 x 2048 about their centre
    let grid = NavigationGrid::for_layer(
      layer,
      CellRect::new(IVec2::new(-1000, 500), IVec2::new(2000, 3500)),
    );
    assert_eq!(grid.dimensions(), (2048, 2048));
    assert_eq!(
      grid.window(),
      CellRect::new(IVec2::new(-524, 976), IVec2::new(1524, 3024))
    );
    assert_eq!(warnings() - before, 1, "warned");
    drop(grid);
    // A long window past the cap is cut along its long side only, and
    // warns no more: a game may take a grid again after every edit
    let long = NavigationGrid::for_layer(layer, CellRect::new(IVec2::ZERO, IVec2::new(5000, 1000)));
    assert_eq!(long.dimensions(), (2048, 1000));
    assert_eq!(long.window().min, IVec2::new(1476, 0));
    drop(long);
    // One at the cap is kept
    let at_cap =
      NavigationGrid::for_layer(layer, CellRect::new(IVec2::ZERO, IVec2::new(4096, 1024)));
    assert_eq!(at_cap.dimensions(), (4096, 1024));
    assert_eq!(warnings() - before, 1, "warned once for both");
  }

  #[test]
  fn overlap_reads_a_rounding_spill_as_touching() {
    let square = |x: f64, y: f64| {
      [
        DVec2::new(x, y),
        DVec2::new(x + 16.0, y),
        DVec2::new(x + 16.0, y + 16.0),
        DVec2::new(x, y + 16.0),
      ]
    };
    for at in [0.0, -3.0e5] {
      let a = square(at, at);
      // A neighbour a millionth of a pixel into it only touches it...
      assert!(
        !overlap(&a, &square(at + 16.0 - 1e-6, at), false),
        "at {at}"
      );
      assert!(!overlap(&a, &square(at + 16.0, at), false), "at {at}");
      // ...a hundredth of a pixel in, it shares a sliver
      assert!(overlap(&a, &square(at + 16.0 - 1e-2, at), false), "at {at}");
      // With edges, touching counts, and a gap does not
      assert!(overlap(&a, &square(at + 16.0, at + 16.0), true), "at {at}");
      assert!(!overlap(&a, &square(at + 16.0 + 1e-3, at), true), "at {at}");
    }
  }
}
