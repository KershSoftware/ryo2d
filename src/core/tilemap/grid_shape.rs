//! Grid geometry: where a tile layer's cells are in its pixels on every grid
//! Tiled saves (orthogonal, isometric, staggered, hexagonal and oblique),
//! and the board questions about them: picking, neighbours, distance,
//! lines, ranges and rings. Placement is Tiled 1.12's as measured
//! (`docs/research/2026-10-31-tile-shapes-formats.md`, and
//! `tests/fixtures/tilemap/tiled/measured.json`, which the tests hold it
//! to); the hexagon maths is Red Blob Games'.
//!
//! The arithmetic is in `f64` on whole and half pixels, which it holds
//! exactly, and a point is rounded to `f32` once, at the end: so a vertex
//! two cells share is the same point from either cell's outline, and a
//! point on an edge picks the same cell along the whole edge. An
//! orthogonal grid keeps the `f32` maths the engine has always used.

use glam::{Affine2, DVec2, IVec2, UVec2, Vec2};

use super::grid::CHUNK_SIZE;
use crate::core::physics::CellRect;
use crate::core::ui::Rect;

/// How a map lays its cells out: Tiled's `orientation` and its parameters
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GridShape {
  /// Rectangles in rows and columns
  #[default]
  Orthogonal,
  /// Diamonds; a cell's box is the diamond's bounding box
  Isometric,
  /// Diamonds in rows (axis Y) or columns (axis X), every other one shifted by half a cell
  Staggered {
    axis: StaggerAxis,
    index: StaggerIndex,
  },
  /// Hexagons; `side` is the length in pixels of the side parallel to the
  /// stagger axis (Tiled's `hexsidelength`): pointy-topped hexagons in
  /// rows for axis Y, flat-topped ones in columns for axis X. Side 0 makes
  /// the diamonds of the staggered grid, which keep a hexagon's six
  /// neighbours
  Hexagonal {
    axis: StaggerAxis,
    index: StaggerIndex,
    side: u32,
  },
  /// Rectangles skewed: each row shifts x by `skew.x` pixels, each column shifts y by `skew.y`
  Oblique { skew: IVec2 },
}

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

/// A layer's grid in its own pixels (y down): the shape, a cell's size, and
/// where the shape's (0, 0) sits.
///
/// Every question of where a cell is goes through it: a cell's centre,
/// outline, box and where its tile stands, the cell under a point, and on
/// any grid the cells around a cell, the steps between two, a line of
/// cells, and every cell within some steps. `origin` is a plain translation
/// added to every position it gives (and taken off every point it reads).
///
/// ```
/// use ryo2d::prelude::*;
///
/// let board = MapGrid::new(
///   GridShape::Hexagonal { axis: StaggerAxis::Y, index: StaggerIndex::Odd, side: 12 },
///   UVec2::new(32, 32),
/// );
/// let home = IVec2::new(2, 3);
/// assert_eq!(board.cell_at(board.cell_center(home)), home);
/// assert_eq!(board.neighbours(home).len(), 6);
/// assert_eq!(board.range(home, 2).len(), 19);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapGrid {
  pub shape: GridShape,
  /// A cell's size in pixels: Tiled's `tilewidth` and `tileheight` (an
  /// isometric cell's diamond box, a hexagon's box)
  pub cell: UVec2,
  /// Where the shape's (0, 0) sits in the layer's pixels: cell (0, 0)'s
  /// top-left corner, or on an isometric grid its top vertex
  pub origin: Vec2,
}

/// A cell's outline, clockwise on screen from its top-most point (the left
/// one of a level top): 4 points, or 6 on hex maps, where a side of length
/// 0 repeats its point
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellOutline {
  points: [Vec2; 6],
  len: u8,
}

impl CellOutline {
  /// The outline's points, in order
  pub fn points(&self) -> &[Vec2] {
    &self.points[..usize::from(self.len)]
  }

  fn of(points: &[Vec2]) -> CellOutline {
    let mut out = [Vec2::ZERO; 6];
    let len = points.len().min(out.len());
    out[..len].copy_from_slice(&points[..len]);
    CellOutline {
      points: out,
      len: len as u8,
    }
  }
}

/// A square lattice: lattice cell (x, y) is the unit square at (x, y) in
/// lattice units, which `to_pixel` takes to the layer's pixels. The index
/// space of orthogonal, isometric and oblique grids; on a staggered grid
/// the isometric lattice its diamonds make, indexed otherwise
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))] // nothing outside the tests reads it yet
pub(crate) struct Lattice {
  pub to_pixel: Affine2,
  stagger: Option<(StaggerAxis, StaggerIndex)>,
}

#[cfg_attr(not(test), allow(dead_code))] // nothing outside the tests reads it yet
impl Lattice {
  /// The lattice cell of a grid cell: the same cell, except on staggered maps
  pub(crate) fn lattice_cell(&self, cell: IVec2) -> IVec2 {
    match self.stagger {
      None => cell,
      Some((axis, index)) => {
        let (x, y) = to_lattice(axis, index, cell);
        IVec2::new(clamp_i32(x), clamp_i32(y))
      }
    }
  }

  /// The grid cell of a lattice cell: `lattice_cell` undone
  pub(crate) fn grid_cell(&self, lattice: IVec2) -> IVec2 {
    match self.stagger {
      None => lattice,
      Some((axis, index)) => {
        from_lattice(axis, index, (i64::from(lattice.x), i64::from(lattice.y)))
      }
    }
  }
}

/// The four neighbours on a square lattice, clockwise from the top
const SQUARE: [(i64, i64); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

impl MapGrid {
  /// A grid of `shape` and `cell`-sized cells, its (0, 0) at the layer's
  /// pixel (0, 0)
  pub fn new(shape: GridShape, cell: UVec2) -> MapGrid {
    MapGrid {
      shape,
      cell,
      origin: Vec2::ZERO,
    }
  }

  /// The grid with its (0, 0) at `origin`
  pub fn with_origin(self, origin: Vec2) -> MapGrid {
    MapGrid { origin, ..self }
  }

  /// Tiled's origin for a bounded map of `size` cells: on isometric maps
  /// the map's height times half a cell's width across (cell (0, 0)'s top
  /// vertex, in whole pixels rounded down, as Tiled places it), zero for
  /// every other shape and for an infinite map
  pub fn for_map(shape: GridShape, cell: UVec2, size: UVec2, infinite: bool) -> MapGrid {
    let grid = MapGrid::new(shape, cell);
    if shape != GridShape::Isometric || infinite {
      return grid;
    }
    let across = u64::from(size.y) * u64::from(cell.x) / 2;
    grid.with_origin(Vec2::new(across as f32, 0.0))
  }

  /// The centre of `cell`: the middle of its box, which on every shape is
  /// the middle of its outline
  pub fn cell_center(&self, cell: IVec2) -> Vec2 {
    let (c, r) = (f64::from(cell.x), f64::from(cell.y));
    let size = self.cell.as_dvec2();
    match self.shape {
      GridShape::Orthogonal => (cell.as_vec2() + 0.5) * self.cell.as_vec2() + self.origin,
      GridShape::Isometric => self.place(DVec2::new(
        (c - r) * size.x / 2.0,
        (c + r + 1.0) * size.y / 2.0,
      )),
      GridShape::Oblique { skew } => {
        let skew = skew.as_dvec2();
        self.place(DVec2::new(
          (c + 0.5) * size.x + (r + 0.5) * skew.x,
          (c + 0.5) * skew.y + (r + 0.5) * size.y,
        ))
      }
      GridShape::Staggered { .. } | GridShape::Hexagonal { .. } => {
        let lines = self.lines();
        let (line, pos) = lines.split(cell);
        let middle = DVec2::new(lines.half, (lines.step + lines.slant) / 2.0);
        self.place(lines.xy(lines.corner(line, pos) + middle))
      }
    }
  }

  /// `cell`'s outline, clockwise on screen from its top-most point
  pub fn cell_outline(&self, cell: IVec2) -> CellOutline {
    let (c, r) = (i64::from(cell.x), i64::from(cell.y));
    match self.shape {
      GridShape::Orthogonal => {
        let size = self.cell.as_vec2();
        let (x0, y0) = (cell.x as f32 * size.x, cell.y as f32 * size.y);
        let (x1, y1) = (
          (cell.x as f32 + 1.0) * size.x,
          (cell.y as f32 + 1.0) * size.y,
        );
        let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
        CellOutline::of(&corners.map(|(x, y)| Vec2::new(x, y) + self.origin))
      }
      GridShape::Isometric => {
        let corners = [(c, r), (c + 1, r), (c + 1, r + 1), (c, r + 1)];
        CellOutline::of(&corners.map(|corner| self.place(self.diamond_corner(corner))))
      }
      GridShape::Oblique { .. } => {
        let mut corners =
          [(c, r), (c + 1, r), (c + 1, r + 1), (c, r + 1)].map(|corner| self.skewed_corner(corner));
        // Skewed past folding, the corners turn the other way round
        if self.skew_det() < 0.0 {
          corners.reverse();
        }
        let top = (0..4)
          .min_by(|&a, &b| {
            corners[a]
              .y
              .total_cmp(&corners[b].y)
              .then(corners[a].x.total_cmp(&corners[b].x))
          })
          .unwrap_or(0);
        corners.rotate_left(top);
        CellOutline::of(&corners.map(|corner| self.place(corner)))
      }
      GridShape::Staggered { .. } | GridShape::Hexagonal { .. } => {
        let (outline, count) = self.lines().outline(cell);
        let mut points = [Vec2::ZERO; 6];
        for (point, &local) in points.iter_mut().zip(&outline[..count]) {
          *point = self.place(local);
        }
        CellOutline::of(&points[..count])
      }
    }
  }

  /// The bounding box of `cell`'s outline
  pub fn cell_bounds(&self, cell: IVec2) -> Rect {
    if self.shape == GridShape::Orthogonal {
      let size = self.cell.as_vec2();
      let at = Vec2::new(cell.x as f32 * size.x, cell.y as f32 * size.y) + self.origin;
      return Rect::new(at.x, at.y, size.x, size.y);
    }
    let (lo, hi) = self.local_box((i64::from(cell.x), i64::from(cell.y)));
    self.rect(lo, hi)
  }

  /// Where a tile image's bottom-left corner goes, before its tileset's
  /// offset (Tiled's rule on every grid): the bottom-left of the cell's box;
  /// on an oblique grid, the skewed corner below the cell's top-left (the
  /// image is moved, not skewed).
  /// On staggered and hexagonal grids of odd sizes, the box's height is the
  /// even one Tiled steps by, so a tile a pixel taller than it reaches a
  /// pixel above, as in Tiled
  pub fn tile_anchor(&self, cell: IVec2) -> Vec2 {
    let (c, r) = (f64::from(cell.x), f64::from(cell.y));
    let size = self.cell.as_dvec2();
    match self.shape {
      GridShape::Orthogonal => {
        let size = self.cell.as_vec2();
        Vec2::new(cell.x as f32 * size.x, (cell.y as f32 + 1.0) * size.y) + self.origin
      }
      GridShape::Isometric => self.place(DVec2::new(
        (c - r - 1.0) * size.x / 2.0,
        (c + r + 2.0) * size.y / 2.0,
      )),
      GridShape::Oblique { .. } => {
        self.place(self.skewed_corner((i64::from(cell.x), i64::from(cell.y) + 1)))
      }
      GridShape::Staggered { .. } | GridShape::Hexagonal { .. } => {
        let lines = self.lines();
        let (line, pos) = lines.split(cell);
        let top_left = lines.xy(lines.corner(line, pos));
        let height = match lines.axis {
          StaggerAxis::Y => lines.depth,
          StaggerAxis::X => 2.0 * lines.half,
        };
        self.place(top_left + DVec2::new(0.0, height))
      }
    }
  }

  /// The cell whose outline holds `point`: what draws there.
  ///
  /// A point on an edge or a corner that cells share goes to one of them,
  /// the same one from either cell's outline, and every point of an edge to
  /// the same cell: on staggered and hexagonal grids a slanted side goes to
  /// the later row (column on axis X) and any other side to the later cell
  /// along its row; on the other grids, a side goes to the later cell along
  /// the axis it crosses. On staggered and hexagonal grids the candidates
  /// are the cells of the band of rows (columns) the point is in, found by
  /// division, and the cell is the one whose outline holds it. Tiled picks
  /// the nearest of the candidates' centres instead, which on hexagons whose
  /// side is not the regular one hands a sliver by each vertex to a
  /// neighbour (measured). A point that is not a number picks cell (0, 0)
  pub fn cell_at(&self, point: Vec2) -> IVec2 {
    if self.shape == GridShape::Orthogonal {
      return ((point - self.origin) / self.cell.as_vec2())
        .floor()
        .as_ivec2();
    }
    let local = point.as_dvec2() - self.origin.as_dvec2();
    match self.shape {
      GridShape::Staggered { .. } | GridShape::Hexagonal { .. } => self.lines().pick(local),
      _ => {
        let (x, y) = self.lattice_at(local);
        IVec2::new(to_i32(x), to_i32(y))
      }
    }
  }

  /// The cells sharing an edge with `cell`, clockwise from the top: 4, or 6
  /// on hex maps.
  ///
  /// Where a cell's top is a vertex (isometric and staggered diamonds,
  /// pointy-topped hexagons), the first is the cell across the edge right
  /// of it; oblique grids keep the order of the grid unskewed (the cells
  /// above, right, below and left in index). On staggered grids the four
  /// are the diamonds' through their lattice; a hexagonal grid of side 0
  /// keeps all six, two of them meeting the cell at a point
  pub fn neighbours(&self, cell: IVec2) -> impl ExactSizeIterator<Item = IVec2> + use<> {
    let mut out = [cell; 6];
    let count = if let GridShape::Hexagonal { .. } = self.shape {
      let lines = self.lines();
      let (q, r) = lines.axial(cell);
      for (slot, (dq, dr)) in out.iter_mut().zip(lines.directions()) {
        *slot = lines.axial_cell((q + dq, r + dr));
      }
      6
    } else {
      let (x, y) = self.square(cell);
      for (slot, (dx, dy)) in out.iter_mut().zip(SQUARE) {
        *slot = self.unsquare((x + dx, y + dy));
      }
      4
    };
    out.into_iter().take(count)
  }

  /// Steps between two cells through `neighbours`: on a square lattice the
  /// steps along each axis (so cells at most n steps away make a diamond),
  /// on a hexagonal grid Red Blob's cube distance
  pub fn distance(&self, a: IVec2, b: IVec2) -> u32 {
    let steps = if let GridShape::Hexagonal { .. } = self.shape {
      let lines = self.lines();
      let ((q0, r0), (q1, r1)) = (lines.axial(a), lines.axial(b));
      let (dq, dr) = (q1 - q0, r1 - r0);
      (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
    } else {
      let ((x0, y0), (x1, y1)) = (self.square(a), self.square(b));
      (x1 - x0).abs() + (y1 - y0).abs()
    };
    u32::try_from(steps).unwrap_or(u32::MAX)
  }

  /// `a` to `b`, both included, each step a neighbour (hex maps: Red Blob's
  /// cube line, nudged off edges).
  ///
  /// It holds `distance(a, b) + 1` cells, so a far line is a long one. On
  /// a square lattice it steps along the axis the straight line crosses
  /// next, and where it crosses a corner, along x first
  pub fn line(&self, a: IVec2, b: IVec2) -> Vec<IVec2> {
    if let GridShape::Hexagonal { .. } = self.shape {
      return self.lines().line(a, b);
    }
    let ((x0, y0), (x1, y1)) = (self.square(a), self.square(b));
    let (nx, ny) = ((x1 - x0).abs(), (y1 - y0).abs());
    let (sx, sy) = ((x1 - x0).signum(), (y1 - y0).signum());
    let mut out = Vec::with_capacity(capacity(nx + ny + 1));
    out.push(a);
    let (mut x, mut y, mut ix, mut iy) = (x0, y0, 0i64, 0i64);
    while ix < nx || iy < ny {
      // Whether the line through the cells' middles leaves this cell
      // through its side along x before its side along y
      let decision =
        i128::from(1 + 2 * ix) * i128::from(ny) - i128::from(1 + 2 * iy) * i128::from(nx);
      if iy == ny || (ix < nx && decision <= 0) {
        x += sx;
        ix += 1;
      } else {
        y += sy;
        iy += 1;
      }
      out.push(self.unsquare((x, y)));
    }
    out
  }

  /// Every cell within `radius` steps, `center` first, then each ring
  /// outward (`ring`'s order): 1 + 2n(n + 1) cells on a square lattice,
  /// 1 + 3n(n + 1) on a hexagonal grid
  pub fn range(&self, center: IVec2, radius: u32) -> Vec<IVec2> {
    let n = u64::from(radius);
    let per = if self.is_hex() { 3 } else { 2 };
    let count = n
      .saturating_mul(n + 1)
      .saturating_mul(per)
      .saturating_add(1);
    let mut out = Vec::with_capacity(capacity(i64::try_from(count).unwrap_or(i64::MAX)));
    out.push(center);
    for ring in 1..=radius {
      self.ring_into(center, ring, &mut out);
    }
    out
  }

  /// The cells exactly `radius` steps away, clockwise; just `center` at
  /// radius 0.
  ///
  /// It starts at the cell `radius` steps along the first neighbour's way:
  /// 4n cells on a square lattice (a diamond, as `distance` counts), 6n on
  /// a hexagonal grid
  pub fn ring(&self, center: IVec2, radius: u32) -> Vec<IVec2> {
    if radius == 0 {
      return vec![center];
    }
    let per = if self.is_hex() { 6 } else { 4 };
    let mut out = Vec::with_capacity(capacity(i64::from(radius) * per));
    self.ring_into(center, radius, &mut out);
    out
  }

  /// The bounding box of a rectangle of cells, outlines included; an empty
  /// rectangle's is the point at the origin
  pub fn bounds_of(&self, cells: CellRect) -> Rect {
    if cells.is_empty() {
      return Rect::new(self.origin.x, self.origin.y, 0.0, 0.0);
    }
    let lo = (i64::from(cells.min.x), i64::from(cells.min.y));
    let hi = (i64::from(cells.max.x) - 1, i64::from(cells.max.y) - 1);
    self.bounds_between(lo, hi)
  }

  /// The bounding box of a storage chunk's cells (`CHUNK_SIZE` a side, by
  /// cell index), outlines included
  #[cfg_attr(not(test), allow(dead_code))] // nothing outside the tests reads it yet
  pub(crate) fn chunk_bounds(&self, chunk: IVec2) -> Rect {
    let side = i64::from(CHUNK_SIZE);
    let lo = (i64::from(chunk.x) * side, i64::from(chunk.y) * side);
    self.bounds_between(lo, (lo.0 + side - 1, lo.1 + side - 1))
  }

  /// The storage chunks (`CHUNK_SIZE` cells a side, by cell index) whose
  /// cells' outlines may meet `rect`: a superset, never missing one.
  ///
  /// Both ends included, within the chunks a map holds: every chunk holding
  /// a cell whose outline overlaps the rectangle, and the chunk of the cell
  /// under each of its points (a point's or a line's), and at most a chunk
  /// more each side. Empty (`first` past `last`) for a rectangle that is
  /// not finite
  #[cfg_attr(not(test), allow(dead_code))] // nothing outside the tests reads it yet
  pub(crate) fn chunks_over(&self, rect: Rect) -> (IVec2, IVec2) {
    let ends = [rect.x, rect.x + rect.width, rect.y, rect.y + rect.height];
    if !ends.iter().all(|v| v.is_finite()) {
      return (IVec2::ONE, IVec2::ZERO);
    }
    let (x0, x1) = (ends[0].min(ends[1]), ends[0].max(ends[1]));
    let (y0, y1) = (ends[2].min(ends[3]), ends[2].max(ends[3]));
    let (lo, hi) = (Vec2::new(x0, y0), Vec2::new(x1, y1));
    // The cells, by index, both ends included
    let (first, last) = match self.shape {
      GridShape::Orthogonal => {
        // As `cell_at` reads them, so the cell under a point is always in
        let (a, b) = (self.cell_at(lo), self.cell_at(hi));
        (
          (i64::from(a.x), i64::from(a.y)),
          (i64::from(b.x), i64::from(b.y)),
        )
      }
      GridShape::Staggered { .. } | GridShape::Hexagonal { .. } => {
        let origin = self.origin.as_dvec2();
        self
          .lines()
          .cells_over(lo.as_dvec2() - origin, hi.as_dvec2() - origin)
      }
      GridShape::Isometric | GridShape::Oblique { .. } => {
        // The lattice is the index space, so the corners' cells bound
        // every cell the rectangle meets
        let origin = self.origin.as_dvec2();
        let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
          .map(|(x, y)| self.lattice_at(DVec2::new(f64::from(x), f64::from(y)) - origin));
        let (mut a, mut b) = (
          (f64::INFINITY, f64::INFINITY),
          (f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for (x, y) in corners {
          a = (a.0.min(x), a.1.min(y));
          b = (b.0.max(x), b.1.max(y));
        }
        (
          (clamp_cell(a.0), clamp_cell(a.1)),
          (clamp_cell(b.0), clamp_cell(b.1)),
        )
      }
    };
    let side = CHUNK_SIZE as i32;
    let chunk =
      |(x, y): (i64, i64)| IVec2::new(clamp_i32(x).div_euclid(side), clamp_i32(y).div_euclid(side));
    (chunk(first), chunk(last))
  }

  /// The square lattice collision and shadows trace in: None on hex maps
  #[cfg_attr(not(test), allow(dead_code))] // nothing outside the tests reads it yet
  pub(crate) fn lattice(&self) -> Option<Lattice> {
    let size = self.cell.as_vec2();
    let (x_axis, y_axis, at, stagger) = match self.shape {
      GridShape::Orthogonal => (
        Vec2::new(size.x, 0.0),
        Vec2::new(0.0, size.y),
        Vec2::ZERO,
        None,
      ),
      GridShape::Isometric => (
        size * 0.5,
        Vec2::new(-size.x, size.y) * 0.5,
        Vec2::ZERO,
        None,
      ),
      GridShape::Oblique { skew } => (
        Vec2::new(size.x, skew.y as f32),
        Vec2::new(skew.x as f32, size.y),
        Vec2::ZERO,
        None,
      ),
      GridShape::Staggered { axis, index } => {
        let lines = self.lines();
        // Half a diamond across and half down, in Tiled's whole pixels
        let (hx, hy) = match axis {
          StaggerAxis::Y => (lines.half as f32, lines.step as f32),
          StaggerAxis::X => (lines.step as f32, lines.half as f32),
        };
        // Lattice cell (0, 0) is grid cell (0, 0)
        let at = match (axis, index) {
          (StaggerAxis::Y, StaggerIndex::Even) => Vec2::new(2.0 * hx, 0.0),
          (StaggerAxis::X, StaggerIndex::Even) => Vec2::new(hx, hy),
          (_, StaggerIndex::Odd) => Vec2::new(hx, 0.0),
        };
        (
          Vec2::new(hx, hy),
          Vec2::new(-hx, hy),
          at,
          Some((axis, index)),
        )
      }
      GridShape::Hexagonal { .. } => return None,
    };
    Some(Lattice {
      to_pixel: Affine2::from_cols(x_axis, y_axis, self.origin + at),
      stagger,
    })
  }

  /// Tiled's order inside a plain layer: compare keys, smaller draws first.
  ///
  /// Rows top to bottom and columns left to right on every grid (oblique
  /// grids too, whatever the skew, as Tiled's right-down); on axis-X
  /// staggered and hexagonal grids a row's unshifted columns first, then
  /// its shifted ones, which sit half a cell lower (measured)
  #[cfg_attr(not(test), allow(dead_code))] // nothing outside the tests reads it yet
  pub(crate) fn paint_key(&self, cell: IVec2) -> (i32, i32, i32) {
    match self.shape {
      GridShape::Staggered {
        axis: StaggerAxis::X,
        ..
      }
      | GridShape::Hexagonal {
        axis: StaggerAxis::X,
        ..
      } => (
        cell.y,
        i32::from(self.lines().shifted(i64::from(cell.x))),
        cell.x,
      ),
      _ => (cell.y, 0, cell.x),
    }
  }

  /// Cells of one band share a sort point: c + r on isometric maps, the row
  /// elsewhere.
  ///
  /// On axis-X staggered and hexagonal grids a band is half a row: its
  /// unshifted cells (2r) or its shifted ones (2r + 1), which sit half a
  /// cell lower. On an oblique grid skewed down along its rows (`skew.y`)
  /// a row's cells step down by the skew, so they share no sort point
  #[cfg_attr(not(test), allow(dead_code))] // nothing outside the tests reads it yet
  pub(crate) fn sort_band(&self, cell: IVec2) -> i32 {
    match self.shape {
      GridShape::Isometric => clamp_i32(i64::from(cell.x) + i64::from(cell.y)),
      GridShape::Staggered {
        axis: StaggerAxis::X,
        ..
      }
      | GridShape::Hexagonal {
        axis: StaggerAxis::X,
        ..
      } => {
        let shifted = self.lines().shifted(i64::from(cell.x));
        clamp_i32(2 * i64::from(cell.y) + i64::from(shifted))
      }
      _ => cell.y,
    }
  }

  /// The layer y a band sorts at: on orthogonal maps the row's bottom edge
  /// (the rule they have always had); on other grids its cells' centre, on
  /// an oblique grid skewed down along its rows that of the row's cell in
  /// column 0
  #[cfg_attr(not(test), allow(dead_code))] // nothing outside the tests reads it yet
  pub(crate) fn band_y(&self, band: i32) -> f32 {
    let b = f64::from(band);
    let size = self.cell.as_dvec2();
    let local = match self.shape {
      GridShape::Orthogonal => return (band as f32 + 1.0) * self.cell.y as f32 + self.origin.y,
      GridShape::Isometric => (b + 1.0) * size.y / 2.0,
      GridShape::Oblique { skew } => b * size.y + (size.y + f64::from(skew.y)) / 2.0,
      GridShape::Staggered { .. } | GridShape::Hexagonal { .. } => {
        let lines = self.lines();
        match lines.axis {
          StaggerAxis::Y => b * lines.step + (lines.step + lines.slant) / 2.0,
          StaggerAxis::X => (b + 1.0) * lines.half,
        }
      }
    };
    (local + f64::from(self.origin.y)) as f32
  }

  // ---- internals ------------------------------------------------------------

  fn is_hex(&self) -> bool {
    matches!(self.shape, GridShape::Hexagonal { .. })
  }

  /// A local point (relative to the origin) in the layer's pixels
  fn place(&self, local: DVec2) -> Vec2 {
    (local + self.origin.as_dvec2()).as_vec2()
  }

  /// The rectangle from `lo` to `hi`, local points
  fn rect(&self, lo: DVec2, hi: DVec2) -> Rect {
    let at = self.place(lo);
    let size = (hi - lo).as_vec2();
    Rect::new(at.x, at.y, size.x, size.y)
  }

  /// The staggered or hexagonal grid's lines; any other shape reads as a
  /// staggered grid, which the callers never ask for
  fn lines(&self) -> Lines {
    Lines::of(self.shape, self.cell)
  }

  /// An isometric lattice point (a diamond's corner), local
  fn diamond_corner(&self, (i, j): (i64, i64)) -> DVec2 {
    let size = self.cell.as_dvec2();
    DVec2::new((i - j) as f64 * size.x / 2.0, (i + j) as f64 * size.y / 2.0)
  }

  /// An oblique lattice point (a cell's corner), local
  fn skewed_corner(&self, (i, j): (i64, i64)) -> DVec2 {
    let size = self.cell.as_dvec2();
    let skew = match self.shape {
      GridShape::Oblique { skew } => skew.as_dvec2(),
      _ => DVec2::ZERO,
    };
    DVec2::new(
      i as f64 * size.x + j as f64 * skew.x,
      i as f64 * skew.y + j as f64 * size.y,
    )
  }

  /// How much an oblique cell's area is: negative where the skew folds the
  /// grid over, 0 where it folds it flat
  fn skew_det(&self) -> f64 {
    let size = self.cell.as_dvec2();
    match self.shape {
      GridShape::Oblique { skew } => size.x * size.y - f64::from(skew.x) * f64::from(skew.y),
      _ => size.x * size.y,
    }
  }

  /// The lattice cell under a local point on an orthogonal, isometric or
  /// oblique grid, rounded down exactly, as whole numbers
  fn lattice_at(&self, p: DVec2) -> (f64, f64) {
    let size = self.cell.as_dvec2();
    match self.shape {
      // x/w + y/h and y/h - x/w, over one divisor
      GridShape::Isometric => {
        let d = size.x * size.y;
        (
          floor_div(p.x * size.y + p.y * size.x, d),
          floor_div(p.y * size.x - p.x * size.y, d),
        )
      }
      GridShape::Oblique { skew } => {
        let det = self.skew_det();
        if det == 0.0 {
          // Folded flat: read as unskewed
          return (floor_div(p.x, size.x), floor_div(p.y, size.y));
        }
        let skew = skew.as_dvec2();
        (
          floor_div(p.x * size.y - p.y * skew.x, det),
          floor_div(p.y * size.x - p.x * skew.y, det),
        )
      }
      _ => (floor_div(p.x, size.x), floor_div(p.y, size.y)),
    }
  }

  /// A cell's local box: (top-left, bottom-right), on an i64 cell
  fn local_box(&self, (c, r): (i64, i64)) -> (DVec2, DVec2) {
    let size = self.cell.as_dvec2();
    match self.shape {
      GridShape::Orthogonal => {
        let at = DVec2::new(c as f64 * size.x, r as f64 * size.y);
        (at, at + size)
      }
      GridShape::Isometric => {
        let top = self.diamond_corner((c, r));
        (
          top - DVec2::new(size.x / 2.0, 0.0),
          top + DVec2::new(size.x / 2.0, size.y),
        )
      }
      GridShape::Oblique { .. } => {
        let corners =
          [(c, r), (c + 1, r), (c + 1, r + 1), (c, r + 1)].map(|corner| self.skewed_corner(corner));
        let lo = corners.iter().fold(DVec2::INFINITY, |m, &p| m.min(p));
        let hi = corners.iter().fold(DVec2::NEG_INFINITY, |m, &p| m.max(p));
        (lo, hi)
      }
      GridShape::Staggered { .. } | GridShape::Hexagonal { .. } => {
        let lines = self.lines();
        let (line, pos) = match lines.axis {
          StaggerAxis::Y => (r, c),
          StaggerAxis::X => (c, r),
        };
        let corner = lines.corner(line, pos);
        let extent = DVec2::new(2.0 * lines.half, lines.step + lines.slant);
        (lines.xy(corner), lines.xy(corner + extent))
      }
    }
  }

  /// The box holding the outlines of the cells from `lo` to `hi`, both
  /// included
  fn bounds_between(&self, lo: (i64, i64), hi: (i64, i64)) -> Rect {
    let (min, max) = self.local_box_between(lo, hi);
    self.rect(min, max)
  }

  /// The cells from `first` to `last` (both included) as `bounds_of` boxes
  /// them, in exact `f64` pixels with the origin added: (top-left,
  /// bottom-right). For checks that a box fits a range, which `f32` rounds
  /// past at its ends
  pub(crate) fn exact_box(&self, first: IVec2, last: IVec2) -> (DVec2, DVec2) {
    let (min, max) = self.local_box_between(
      (i64::from(first.x), i64::from(first.y)),
      (i64::from(last.x), i64::from(last.y)),
    );
    let origin = self.origin.as_dvec2();
    (min + origin, max + origin)
  }

  /// `bounds_between`'s box, local: the cells of the first and last two
  /// rows and columns hold every extreme on every grid
  fn local_box_between(&self, lo: (i64, i64), hi: (i64, i64)) -> (DVec2, DVec2) {
    let ends = |a: i64, b: i64| [a, (a + 1).min(b), (b - 1).max(a), b];
    let (mut min, mut max) = (DVec2::INFINITY, DVec2::NEG_INFINITY);
    for y in ends(lo.1, hi.1) {
      for x in ends(lo.0, hi.0) {
        let (a, b) = self.local_box((x, y));
        min = min.min(a);
        max = max.max(b);
      }
    }
    (min, max)
  }

  /// A cell on the square lattice: its index, or on a staggered grid its
  /// lattice cell
  fn square(&self, cell: IVec2) -> (i64, i64) {
    match self.shape {
      GridShape::Staggered { axis, index } => to_lattice(axis, index, cell),
      _ => (i64::from(cell.x), i64::from(cell.y)),
    }
  }

  /// `square` undone
  fn unsquare(&self, (x, y): (i64, i64)) -> IVec2 {
    match self.shape {
      GridShape::Staggered { axis, index } => from_lattice(axis, index, (x, y)),
      _ => IVec2::new(clamp_i32(x), clamp_i32(y)),
    }
  }

  fn ring_into(&self, center: IVec2, radius: u32, out: &mut Vec<IVec2>) {
    let n = i64::from(radius);
    if self.is_hex() {
      let lines = self.lines();
      let ways = lines.directions();
      let (q, r) = lines.axial(center);
      let mut at = (q + ways[0].0 * n, r + ways[0].1 * n);
      // Each side runs toward the next corner: from the corner along way
      // i, two ways round
      for side in 0..6 {
        let (dq, dr) = ways[(side + 2) % 6];
        for _ in 0..n {
          out.push(lines.axial_cell(at));
          at = (at.0 + dq, at.1 + dr);
        }
      }
    } else {
      let (x, y) = self.square(center);
      let mut at = (x, y - n);
      for (dx, dy) in [(1, 1), (-1, 1), (-1, -1), (1, -1)] {
        for _ in 0..n {
          out.push(self.unsquare(at));
          at = (at.0 + dx, at.1 + dy);
        }
      }
    }
  }
}

/// A staggered or hexagonal grid in a frame of its own: `u` across the
/// stagger axis and `v` along it (x and y for axis Y, y and x for axis X),
/// so lines (rows for axis Y, columns for axis X) stack along `v`, and a
/// cell is (line, position in it). The steps are whole pixels, as Tiled
/// works them out: half a cell rounds down, and a hexagon's slant
/// (the tile less its side, halved) rounds toward zero (measured)
#[derive(Clone, Copy, Debug)]
struct Lines {
  axis: StaggerAxis,
  index: StaggerIndex,
  /// Hexagons (6 points to an outline), not a staggered grid's diamonds
  hex: bool,
  /// Half a cell across: a shifted line's shift, and half the step from a
  /// cell to the next along its line
  half: f64,
  /// The step from a line to the next
  step: f64,
  /// How far along the axis a hexagon's slanted sides reach (0 where the
  /// side is longer than the tile; a staggered diamond's is its step)
  slant: f64,
  /// The cell's length along the axis as Tiled draws tiles against it
  /// (the slant twice and the side, unclamped)
  depth: f64,
}

impl Lines {
  fn of(shape: GridShape, cell: UVec2) -> Lines {
    let (axis, index, side, hex) = match shape {
      GridShape::Hexagonal { axis, index, side } => (axis, index, side, true),
      GridShape::Staggered { axis, index } => (axis, index, 0, false),
      _ => (StaggerAxis::Y, StaggerIndex::Odd, 0, false),
    };
    let (across, along) = match axis {
      StaggerAxis::Y => (cell.x, cell.y),
      StaggerAxis::X => (cell.y, cell.x),
    };
    let half = i64::from(across) / 2;
    // Rust's division, like Tiled's, rounds toward zero
    let slant = (i64::from(along) - i64::from(side)) / 2;
    let step = slant + i64::from(side);
    Lines {
      axis,
      index,
      hex,
      half: half as f64,
      step: step as f64,
      slant: slant.max(0) as f64,
      depth: (slant + step) as f64,
    }
  }

  fn shifted(&self, line: i64) -> bool {
    (line.rem_euclid(2) == 1) == (self.index == StaggerIndex::Odd)
  }

  fn shift(&self, line: i64) -> f64 {
    if self.shifted(line) { self.half } else { 0.0 }
  }

  /// A cell's (line, position)
  fn split(&self, cell: IVec2) -> (i64, i64) {
    let (x, y) = (i64::from(cell.x), i64::from(cell.y));
    match self.axis {
      StaggerAxis::Y => (y, x),
      StaggerAxis::X => (x, y),
    }
  }

  /// `split` undone
  fn join(&self, line: i64, pos: i64) -> IVec2 {
    let (line, pos) = (clamp_i32(line), clamp_i32(pos));
    match self.axis {
      StaggerAxis::Y => IVec2::new(pos, line),
      StaggerAxis::X => IVec2::new(line, pos),
    }
  }

  /// A local point between the frame and the layer's pixels (one swap, its
  /// own inverse)
  fn xy(&self, p: DVec2) -> DVec2 {
    match self.axis {
      StaggerAxis::Y => p,
      StaggerAxis::X => DVec2::new(p.y, p.x),
    }
  }

  /// A cell's box corner nearest (0, 0), in the frame
  fn corner(&self, line: i64, pos: i64) -> DVec2 {
    DVec2::new(
      pos as f64 * 2.0 * self.half + self.shift(line),
      line as f64 * self.step,
    )
  }

  /// A cell's outline, local, clockwise on screen from its top-most point:
  /// built for axis Y, where a cell's top is a vertex, and for axis X
  /// swapped into place and turned back clockwise
  fn outline(&self, cell: IVec2) -> ([DVec2; 6], usize) {
    let (line, pos) = self.split(cell);
    let corner = self.corner(line, pos);
    let (h, step, slant) = (self.half, self.step, self.slant);
    let mut frame = [DVec2::ZERO; 6];
    let count = if !self.hex {
      frame[..4].copy_from_slice(&[
        DVec2::new(h, 0.0),
        DVec2::new(2.0 * h, slant),
        DVec2::new(h, step + slant),
        DVec2::new(0.0, slant),
      ]);
      4
    } else {
      frame = [
        DVec2::new(h, 0.0),
        DVec2::new(2.0 * h, slant),
        DVec2::new(2.0 * h, step),
        DVec2::new(h, step + slant),
        DVec2::new(0.0, step),
        DVec2::new(0.0, slant),
      ];
      6
    };
    let mut out = [DVec2::ZERO; 6];
    for (slot, rel) in out.iter_mut().zip(&frame[..count]) {
      *slot = self.xy(corner + *rel);
    }
    if self.axis == StaggerAxis::X {
      out[..count].reverse();
    }
    (out, count)
  }

  /// The cell whose outline holds the local point `p`: the band of lines it
  /// is in, by division, then where a band's lines interlock (the hexagons'
  /// slanted sides), the side of the slant it is on. A point on a slanted
  /// side goes to the later line, on a side along the axis to the later
  /// position
  fn pick(&self, p: DVec2) -> IVec2 {
    let p = self.xy(p);
    // Cells with no size pick something rather than nothing
    let (half, step) = (self.half.max(0.5), self.step.max(1.0));
    let band = floor_div(p.y, step);
    let t = p.y - band * step;
    let mut line = clamp_cell(band);
    if t < self.slant {
      // Distance across from the nearest top vertex of the band's line,
      // against how far down the slant reaches there
      let across = (p.x - self.shift(line) - half).rem_euclid(2.0 * half);
      let off = across.min(2.0 * half - across);
      if t * half < off * self.slant {
        line -= 1;
      }
    }
    let pos = floor_div(p.x - self.shift(line), 2.0 * half);
    self.join(line, clamp_cell(pos))
  }

  /// The cells, by index and both ends included, whose outlines may meet the
  /// local rectangle `lo` to `hi`: a line meets it where its cells' reach
  /// along the axis does, and a position where it would in either phase
  fn cells_over(&self, lo: DVec2, hi: DVec2) -> ((i64, i64), (i64, i64)) {
    let (lo, hi) = (self.xy(lo), self.xy(hi));
    let (half, step) = (self.half.max(0.5), self.step.max(1.0));
    let lines = (
      clamp_cell(floor_div(lo.y - step - self.slant, step)),
      clamp_cell(floor_div(hi.y, step)),
    );
    let positions = (
      clamp_cell(floor_div(lo.x - half, 2.0 * half)),
      clamp_cell(floor_div(hi.x, 2.0 * half)),
    );
    match self.axis {
      StaggerAxis::Y => ((positions.0, lines.0), (positions.1, lines.1)),
      StaggerAxis::X => ((lines.0, positions.0), (lines.1, positions.1)),
    }
  }

  /// A hexagonal cell's axial coordinates (Red Blob's q and r; s is
  /// -q - r): odd-r or even-r offsets for axis Y, odd-q or even-q for
  /// axis X
  fn axial(&self, cell: IVec2) -> (i64, i64) {
    let (line, pos) = self.split(cell);
    let other = pos - self.half_line(line);
    match self.axis {
      StaggerAxis::Y => (other, line),
      StaggerAxis::X => (line, other),
    }
  }

  /// `axial` undone
  fn axial_cell(&self, (q, r): (i64, i64)) -> IVec2 {
    let (line, other) = match self.axis {
      StaggerAxis::Y => (r, q),
      StaggerAxis::X => (q, r),
    };
    self.join(line, other + self.half_line(line))
  }

  /// How far a line's offsets run ahead of its axial coordinate: half the
  /// line, rounded down where the odd lines are shifted, up where the even
  /// ones are
  fn half_line(&self, line: i64) -> i64 {
    match self.index {
      StaggerIndex::Odd => line.div_euclid(2),
      StaggerIndex::Even => (line + 1).div_euclid(2),
    }
  }

  /// The six axial ways to a neighbour, clockwise on screen from the top:
  /// for pointy-topped hexagons (axis Y) from the edge right of the top
  /// vertex, for flat-topped ones (axis X) from the top edge
  fn directions(&self) -> [(i64, i64); 6] {
    match self.axis {
      StaggerAxis::Y => [(1, -1), (1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1)],
      StaggerAxis::X => [(0, -1), (1, -1), (1, 0), (0, 1), (-1, 1), (-1, 0)],
    }
  }

  /// Red Blob's cube line: the cells nearest evenly spaced points from `a`
  /// to `b`, both ends nudged by the same tiny amount so a line along the
  /// edges between cells always takes the same side
  fn line(&self, a: IVec2, b: IVec2) -> Vec<IVec2> {
    let ((q0, r0), (q1, r1)) = (self.axial(a), self.axial(b));
    let (dq, dr) = (q1 - q0, r1 - r0);
    let n = (dq.abs() + dr.abs() + (dq + dr).abs()) / 2;
    let mut out = Vec::with_capacity(capacity(n + 1));
    let start = (q0 as f64 + 1e-6, r0 as f64 + 1e-6, (-q0 - r0) as f64 - 2e-6);
    let end = (q1 as f64 + 1e-6, r1 as f64 + 1e-6, (-q1 - r1) as f64 - 2e-6);
    for i in 0..=n {
      let t = if n == 0 { 0.0 } else { i as f64 / n as f64 };
      let lerp = |a: f64, b: f64| a + (b - a) * t;
      let cube = cube_round(
        lerp(start.0, end.0),
        lerp(start.1, end.1),
        lerp(start.2, end.2),
      );
      out.push(self.axial_cell(cube));
    }
    out
  }
}

/// The nearest cube coordinate to a fractional one, as axial (q, r): each
/// rounded, the one rounded furthest set from the other two (Red Blob)
fn cube_round(q: f64, r: f64, s: f64) -> (i64, i64) {
  let (mut rq, mut rr, rs) = (q.round(), r.round(), s.round());
  let (dq, dr, ds) = ((rq - q).abs(), (rr - r).abs(), (rs - s).abs());
  if dq > dr && dq > ds {
    rq = -rr - rs;
  } else if dr > ds {
    rr = -rq - rs;
  }
  (rq as i64, rr as i64)
}

/// A staggered cell's lattice cell, the grid's (0, 0) the lattice's
fn to_lattice(axis: StaggerAxis, index: StaggerIndex, cell: IVec2) -> (i64, i64) {
  let (c, r) = (i64::from(cell.x), i64::from(cell.y));
  let down = |n: i64| n.div_euclid(2);
  let up = |n: i64| (n + 1).div_euclid(2);
  match (axis, index) {
    (StaggerAxis::Y, StaggerIndex::Odd) => (c + up(r), down(r) - c),
    (StaggerAxis::Y, StaggerIndex::Even) => (c + down(r), up(r) - c),
    (StaggerAxis::X, StaggerIndex::Odd) => (r + up(c), r - down(c)),
    (StaggerAxis::X, StaggerIndex::Even) => (r + down(c), r - up(c)),
  }
}

/// `to_lattice` undone
fn from_lattice(axis: StaggerAxis, index: StaggerIndex, (x, y): (i64, i64)) -> IVec2 {
  let half = |n: i64| match index {
    StaggerIndex::Odd => (n + 1).div_euclid(2),
    StaggerIndex::Even => n.div_euclid(2),
  };
  let (c, r) = match axis {
    StaggerAxis::Y => {
      let r = x + y;
      (x - half(r), r)
    }
    StaggerAxis::X => {
      let c = x - y;
      (c, x - half(c))
    }
  };
  IVec2::new(clamp_i32(c), clamp_i32(r))
}

/// `n / d` rounded down, exactly where both are exact: the quotient can
/// round up onto a whole number it does not reach, so that one is stepped
/// back from
fn floor_div(n: f64, d: f64) -> f64 {
  let (n, d) = if d < 0.0 { (-n, -d) } else { (n, d) };
  let q = (n / d).floor();
  if q * d > n { q - 1.0 } else { q }
}

/// A whole number as a cell coordinate, within what a cell holds; a value
/// that is not a number reads 0
fn clamp_cell(v: f64) -> i64 {
  v.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i64
}

fn to_i32(v: f64) -> i32 {
  clamp_cell(v) as i32
}

fn clamp_i32(v: i64) -> i32 {
  v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// A list's first allocation: what it will hold, within reason
fn capacity(count: i64) -> usize {
  usize::try_from(count).unwrap_or(0).min(4096)
}

#[cfg(test)]
mod tests;
