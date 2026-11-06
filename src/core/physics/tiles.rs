//! Tile colliders: a grid of cells becomes seamless outlines (or merged
//! rectangles), one-way runs, 45° slopes and authored shapes. Coordinates
//! here are in tiles, +Y down, cell (0, 0)'s top-left at the origin, signed;
//! the outlines and runs are traced on a finer grid, `GRID` points a tile.

use std::sync::atomic::{AtomicU64, Ordering};

use glam::{Affine2, DVec2, IVec2, Mat2, UVec2};
use rapier2d::prelude::SharedShape;

use super::collider::{Collider, PrebuiltShape};
use super::world::Physics;
use crate::core::ecs::{
  Allow, Changed, ChildOf, Children, Commands, Component, DetectChangesMut, Disabled, Entity,
  EntityHashSet, Floor, Has, Or, ParentPlaced, Query, RemovedComponents, ResMut, Transform, Vec2,
  With,
};
use crate::core::hash::{FastMap, FastSet};

pub(crate) mod hex;
mod outline;
mod pack;
mod runs;
mod shape;

use hex::HexCells;
pub(crate) use outline::HexSides;
use outline::{Outlines, Unit, hex_side_edge, unit_edges};
#[cfg(test)]
use outline::{outlines, traced_outlines};
use pack::{
  CHUNK, Geometry, HexParts, PackKey, Packs, ends, hex_first_cell, loop_key, ring, run_key,
};
use runs::Runs;
#[cfg(test)]
use runs::one_way_runs;
use shape::ShapeBuild;

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
  /// An authored shape: the index `TileColliders::add_shape` returned. A
  /// shape the grid does not have builds nothing
  Shape(u16),
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

/// A rectangle of cells, `min` inclusive and `max` exclusive, in signed
/// cell coordinates: empty unless `max` is past `min` on both axes. What a
/// tile grid holds (its bounds) or reaches (its extent).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CellRect {
  pub min: IVec2,
  pub max: IVec2,
}

impl CellRect {
  /// The cells from `min` up to, not including, `max`
  pub fn new(min: IVec2, max: IVec2) -> CellRect {
    CellRect { min, max }
  }

  /// The cells from (0, 0) up to `size`; a side past what an `i32` holds
  /// stops at `i32::MAX`
  pub fn from_size(size: UVec2) -> CellRect {
    CellRect {
      min: IVec2::ZERO,
      max: size.min(UVec2::splat(i32::MAX as u32)).as_ivec2(),
    }
  }

  /// Width and height in cells: zero when empty
  pub fn size(&self) -> UVec2 {
    if self.is_empty() {
      return UVec2::ZERO;
    }
    // Measured in i64: the widest rectangle spans every i32
    let side = |min: i32, max: i32| (i64::from(max) - i64::from(min)) as u32;
    UVec2::new(side(self.min.x, self.max.x), side(self.min.y, self.max.y))
  }

  /// Whether it holds no cell
  pub fn is_empty(&self) -> bool {
    self.max.x <= self.min.x || self.max.y <= self.min.y
  }

  /// Whether `cell` is one of its cells
  pub fn contains(&self, cell: IVec2) -> bool {
    cell.cmpge(self.min).all() && cell.cmplt(self.max).all()
  }

  /// The smallest rectangle holding both; an empty side is ignored (two
  /// empty sides give `self`)
  pub fn union(self, other: CellRect) -> CellRect {
    if other.is_empty() {
      return self;
    }
    if self.is_empty() {
      return other;
    }
    CellRect {
      min: self.min.min(other.min),
      max: self.max.max(other.max),
    }
  }
}

/// A tile's authored collision shape (an editor's collision polygons), for
/// `TileCell::Shape` cells to name. A solid shape joins the grid's outlines
/// as solid and slope cells do: where it meets a neighbour's edges along
/// their shared side they cancel, so a row of half blocks, or a ramp of
/// shapes over solid ground, is one seamless outline (hollow, as outlines
/// are). A one-way shape's top edges (those facing within 45° of up) join
/// the one-way runs, merged with the one-way cells and shapes they line up
/// with. In `solid_rects` mode a solid shape is convex polygons instead (a
/// concave polygon split). Points are snapped to 1/11520 of a tile, which
/// holds any whole pixel of the usual tile sizes exactly, and a polygon
/// reaching past its cell is clipped to it (into pieces, if the cell cuts
/// it apart). On a grid of hexagonal cells (a tile map's), a tile is its
/// hexagon's box, and each of a solid shape's polygons is a closed ring of
/// its own, as authored, wherever it lies, neither clipped nor merged,
/// wound clockwise on screen as outlines are; one that touches or crosses
/// itself is its convex hull.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CellShape {
  /// Polygons in tile units ((0, 0) the cell's top-left, (1, 1) its
  /// bottom-right, y down), wound either way. Repeated and straight-through
  /// points are dropped and a polygon left with no area builds nothing; one
  /// that is not simple (it crosses or touches itself), or polygons that
  /// overlap one another in the cell, collide as their convex hull (logged;
  /// on a hexagonal grid each polygon is a ring of its own, one that is not
  /// simple its hull)
  pub polygons: Vec<Vec<Vec2>>,
  /// Solid only from above, as `TileCell::OneWay` is
  pub one_way: bool,
}

/// A tile corner, in tiles.
pub(crate) type Corner = (i32, i32);

/// A point of the grid outlines and runs are traced on, `GRID` to a tile.
pub(crate) type Point = (i32, i32);

/// Outline points a tile: 2^8 · 3^2 · 5, so a whole pixel of tiles 8, 12,
/// 16, 18, 24, 32, 48, 64, 128 or 256 px wide (and many more) is a point,
/// and so is a half pixel of all of these but 256; anything else snaps to
/// the nearest. Cells within `MAX_REACH` of (0, 0) are within an `i32` of
/// points.
pub(crate) const GRID: i32 = 11520;

/// How far from cell (0, 0) a grid's cells may be, on either axis, and
/// still collide: what an i32 outline lattice of `GRID` units a cell holds,
/// with room for a shape's reach. Every point the units of a cell within it
/// reach (its edges, and the borders past it with their cells' edges) is
/// under 2^31 from the origin: 186,411 cells, about 3 million px of 16 px
/// tiles. This is the lattice's reach, not where f32 positions stay
/// precise: a `Transform`'s f32 pixels are 1/8 px apart a million px out
/// (2^20), 1/4 px at this reach and a whole pixel past 2^23 px (Rapier's
/// metres, at 100 px a metre, about 0.2 px apart here), so precise play
/// keeps within about a million px.
pub(crate) const MAX_REACH: i32 = (i32::MAX / GRID) - 2;

/// `point` taken through the lattice `to`, worked out in f64 and rounded
/// once
fn through(to: Affine2, point: DVec2) -> Vec2 {
  let (a, b) = (to.matrix2.x_axis.as_dvec2(), to.matrix2.y_axis.as_dvec2());
  (a * point.x + b * point.y + to.translation.as_dvec2()).as_vec2()
}

/// Cells in a chunk of a grid's storage: `CHUNK` a side, as the packing's
const CELLS: usize = (CHUNK * CHUNK) as usize;

/// One chunk's cells, row by row
type Chunk = [TileCell; CELLS];

/// A grid's chunks, by signed chunk coordinate
type Chunks = FastMap<IVec2, Box<Chunk>>;

/// The chunk holding `cell`, rounding toward negative infinity: cell -1 is
/// in chunk -1, cell -33 in chunk -2
fn chunk_of(cell: IVec2) -> IVec2 {
  cell.div_euclid(IVec2::splat(CHUNK))
}

/// `cell`'s place in its chunk
fn local(cell: IVec2) -> usize {
  let at = cell.rem_euclid(IVec2::splat(CHUNK));
  (at.y * CHUNK + at.x) as usize
}

/// The cells of `chunk`, a chunk of cells within `MAX_REACH`
fn chunk_rect(chunk: IVec2) -> CellRect {
  let min = chunk * CHUNK;
  CellRect::new(min, min + IVec2::splat(CHUNK))
}

/// The cells of any `chunk` of `i32` cells: the end of the last chunk
/// stops at `i32::MAX`
fn chunk_rect_of(chunk: IVec2) -> CellRect {
  let min = chunk * CHUNK;
  CellRect::new(min, min.saturating_add(IVec2::splat(CHUNK)))
}

/// Whether `cell` is within the outline lattice's reach (`MAX_REACH`)
fn reaches(cell: IVec2) -> bool {
  cell.cmpge(IVec2::splat(-MAX_REACH)).all() && cell.cmple(IVec2::splat(MAX_REACH)).all()
}

/// The outline edges of a solid cell, in cell points, clockwise on screen.
const SOLID: [(Point, Point); 4] = [
  ((0, 0), (GRID, 0)),
  ((GRID, 0), (GRID, GRID)),
  ((GRID, GRID), (0, GRID)),
  ((0, GRID), (0, 0)),
];
const SLOPE_BOTTOM_LEFT: [(Point, Point); 3] = [
  ((0, 0), (GRID, GRID)),
  ((GRID, GRID), (0, GRID)),
  ((0, GRID), (0, 0)),
];
const SLOPE_BOTTOM_RIGHT: [(Point, Point); 3] = [
  ((GRID, 0), (GRID, GRID)),
  ((GRID, GRID), (0, GRID)),
  ((0, GRID), (GRID, 0)),
];
const SLOPE_TOP_LEFT: [(Point, Point); 3] = [
  ((0, 0), (GRID, 0)),
  ((GRID, 0), (0, GRID)),
  ((0, GRID), (0, 0)),
];
const SLOPE_TOP_RIGHT: [(Point, Point); 3] = [
  ((0, 0), (GRID, 0)),
  ((GRID, 0), (GRID, GRID)),
  ((GRID, GRID), (0, 0)),
];
/// A one-way cell's piece: its top.
const ONE_WAY_TOP: [(Point, Point); 1] = [((0, 0), (GRID, 0))];

/// A cell's sides, and how its edges (in cell points, 0 to `GRID`) lie on
/// them.
pub(crate) mod sides {
  use super::{GRID, Point, SlopeCorner, TileCell};

  pub(crate) const TOP: usize = 0;
  pub(crate) const RIGHT: usize = 1;
  pub(crate) const BOTTOM: usize = 2;
  pub(crate) const LEFT: usize = 3;

  /// The interval an edge runs along `side` the way round the cell
  /// (clockwise on screen: right along the top, down the right side, left
  /// along the bottom, up the left side), measured from the side's left or
  /// top end.
  pub(crate) fn along((a, b): (Point, Point), side: usize) -> Option<(i32, i32)> {
    match side {
      TOP => (a.1 == 0 && b.1 == 0 && b.0 > a.0).then_some((a.0, b.0)),
      RIGHT => (a.0 == GRID && b.0 == GRID && b.1 > a.1).then_some((a.1, b.1)),
      BOTTOM => (a.1 == GRID && b.1 == GRID && b.0 < a.0).then_some((b.0, a.0)),
      _ => (a.0 == 0 && b.0 == 0 && b.1 < a.1).then_some((b.1, a.1)),
    }
  }

  /// Whether an edge runs along none of the cell's sides.
  pub(crate) fn inside(edge: (Point, Point)) -> bool {
    (TOP..=LEFT).all(|side| along(edge, side).is_none())
  }

  /// Whether a cell (not a shape) runs the whole of `side`.
  pub(crate) fn full(cell: TileCell, side: usize) -> bool {
    use SlopeCorner::{BottomLeft, BottomRight, TopLeft, TopRight};
    matches!(
      (cell, side),
      (TileCell::Solid, _)
        | (TileCell::Slope(BottomLeft), BOTTOM | LEFT)
        | (TileCell::Slope(BottomRight), RIGHT | BOTTOM)
        | (TileCell::Slope(TopLeft), TOP | LEFT)
        | (TileCell::Slope(TopRight), TOP | RIGHT)
    )
  }

  pub(crate) fn plus(a: Point, b: Point) -> Point {
    (a.0 + b.0, a.1 + b.1)
  }
}

/// A few items inline and more on the heap: what the outline and run maps
/// hold for one point (one per filled wedge around it, usually one or two).
#[derive(Debug, Clone)]
pub(crate) enum Few<T: Copy + Default> {
  Inline(u8, [T; 4]),
  Heap(Vec<T>),
}

impl<T: Copy + Default> Default for Few<T> {
  fn default() -> Self {
    Few::Inline(0, [T::default(); 4])
  }
}

impl<T: Copy + Default> Few<T> {
  pub(crate) fn as_slice(&self) -> &[T] {
    match self {
      Few::Inline(len, items) => &items[..usize::from(*len)],
      Few::Heap(items) => items,
    }
  }

  pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
    match self {
      Few::Inline(len, items) => &mut items[..usize::from(*len)],
      Few::Heap(items) => items,
    }
  }

  pub(crate) fn push(&mut self, item: T) {
    match self {
      Few::Inline(len, items) if usize::from(*len) < items.len() => {
        items[usize::from(*len)] = item;
        *len += 1;
      }
      Few::Inline(_, items) => {
        let mut heap = items.to_vec();
        heap.push(item);
        *self = Few::Heap(heap);
      }
      Few::Heap(items) => items.push(item),
    }
  }

  pub(crate) fn swap_remove(&mut self, i: usize) {
    match self {
      Few::Inline(len, items) => {
        let last = usize::from(*len) - 1;
        items[i] = items[last];
        *len -= 1;
      }
      Few::Heap(items) => {
        items.swap_remove(i);
      }
    }
  }

  pub(crate) fn is_empty(&self) -> bool {
    self.as_slice().is_empty()
  }
}

/// A grid's cells and shapes, as the outlines and runs read them: any
/// signed cell, `Empty` where the grid has none. A grid holds no cell past
/// `MAX_REACH`, so nothing read here is past it.
#[derive(Clone, Copy)]
pub(crate) struct Cells<'a> {
  /// None for a `Boundary`'s, whose cells come through a closure
  chunks: Option<&'a Chunks>,
  shapes: &'a [(CellShape, ShapeBuild)],
}

impl<'a> Cells<'a> {
  fn new(chunks: &'a Chunks, shapes: &'a [(CellShape, ShapeBuild)]) -> Self {
    Self {
      chunks: Some(chunks),
      shapes,
    }
  }

  /// No cells, only shapes (a `Boundary`'s)
  fn shapes_only(shapes: &'a [(CellShape, ShapeBuild)]) -> Self {
    Self {
      chunks: None,
      shapes,
    }
  }

  /// The cells of `chunk`, if the grid has it
  fn chunk(self, chunk: IVec2) -> Option<&'a Chunk> {
    self.chunks?.get(&chunk).map(|cells| &**cells)
  }

  /// The chunks there are, in row order (by row, then column)
  pub(crate) fn chunk_order(&self) -> Vec<IVec2> {
    let mut order: Vec<IVec2> = self
      .chunks
      .map_or_else(Vec::new, |chunks| chunks.keys().copied().collect());
    order.sort_unstable_by_key(|chunk| (chunk.y, chunk.x));
    order
  }

  /// The cell at (x, y); `Empty` where the grid has none.
  pub(crate) fn at(&self, x: i32, y: i32) -> TileCell {
    let cell = IVec2::new(x, y);
    self
      .chunk(chunk_of(cell))
      .map_or(TileCell::Empty, |cells| cells[local(cell)])
  }

  /// A reader of any cell, as `at`, that keeps the chunk it last looked
  /// up: reads that come in runs within a chunk (an edit's cells by column)
  /// look each chunk up once a run
  pub(crate) fn cached(self) -> impl Fn(i32, i32) -> TileCell + 'a {
    let last: std::cell::Cell<Option<(IVec2, Option<&'a Chunk>)>> = std::cell::Cell::new(None);
    move |x, y| {
      let cell = IVec2::new(x, y);
      let at = chunk_of(cell);
      let cells = match last.get() {
        Some((chunk, cells)) if chunk == at => cells,
        _ => {
          let cells = self.chunk(at);
          last.set(Some((at, cells)));
          cells
        }
      };
      cells.map_or(TileCell::Empty, |cells| cells[local(cell)])
    }
  }

  /// A reader of the cells around `chunk` without a lookup each: the
  /// chunk's own, the chunk's left of it and the chunk's above it (all its
  /// cells' units read); any other cell is looked up.
  pub(crate) fn near(self, chunk: IVec2) -> impl Fn(i32, i32) -> TileCell + 'a {
    let (left, above) = (chunk - IVec2::X, chunk - IVec2::Y);
    let found = [self.chunk(chunk), self.chunk(left), self.chunk(above)];
    move |x, y| {
      let cell = IVec2::new(x, y);
      let at = chunk_of(cell);
      let cells = if at == chunk {
        found[0]
      } else if at == left {
        found[1]
      } else if at == above {
        found[2]
      } else {
        self.chunk(at)
      };
      cells.map_or(TileCell::Empty, |cells| cells[local(cell)])
    }
  }

  /// Calls `f` with each cell holding something (not `Empty`), in row order
  /// (by row, then column): what a scan of every cell would meet, reading
  /// only the chunks there are.
  pub(crate) fn each(&self, mut f: impl FnMut(i32, i32, TileCell)) {
    let order = self.chunk_order();
    for row in order.chunk_by(|a, b| a.y == b.y) {
      let top = row[0].y * CHUNK;
      let found: Vec<(i32, &Chunk)> = row
        .iter()
        .filter_map(|&chunk| Some((chunk.x * CHUNK, self.chunk(chunk)?)))
        .collect();
      for dy in 0..CHUNK {
        for &(left, cells) in &found {
          let line = &cells[(dy * CHUNK) as usize..((dy + 1) * CHUNK) as usize];
          for (dx, &cell) in (0..CHUNK).zip(line) {
            if cell != TileCell::Empty {
              f(left + dx, top + dy, cell);
            }
          }
        }
      }
    }
  }

  /// A cell's outline edges, in cell points.
  pub(crate) fn edges(&self, cell: TileCell) -> &'a [(Point, Point)] {
    match cell {
      TileCell::Solid => &SOLID,
      TileCell::Slope(SlopeCorner::BottomLeft) => &SLOPE_BOTTOM_LEFT,
      TileCell::Slope(SlopeCorner::BottomRight) => &SLOPE_BOTTOM_RIGHT,
      TileCell::Slope(SlopeCorner::TopLeft) => &SLOPE_TOP_LEFT,
      TileCell::Slope(SlopeCorner::TopRight) => &SLOPE_TOP_RIGHT,
      TileCell::Shape(shape) => self
        .shapes
        .get(usize::from(shape))
        .map_or(&[], |(_, build)| &build.edges[..]),
      TileCell::Empty | TileCell::OneWay => &[],
    }
  }

  /// How many edges a hexagonal grid's shape cell's rings hold
  /// (`ShapeBuild::rings`): none for any other cell, a one-way shape or a
  /// shape the grid lacks
  pub(crate) fn ring_edges(&self, cell: TileCell) -> usize {
    match cell {
      TileCell::Shape(shape) => self
        .shapes
        .get(usize::from(shape))
        .map_or(0, |(_, build)| build.ring_edges),
      _ => 0,
    }
  }

  /// A cell's one-way pieces, in cell points.
  pub(crate) fn tops(&self, cell: TileCell) -> &'a [(Point, Point)] {
    match cell {
      TileCell::OneWay => &ONE_WAY_TOP,
      TileCell::Shape(shape) => self
        .shapes
        .get(usize::from(shape))
        .map_or(&[], |(_, build)| &build.tops[..]),
      _ => &[],
    }
  }
}

/// A filled cell's corners relative to its top-left, in tiles, wound
/// clockwise on screen (`solid_rects` mode's slope polygons).
pub(crate) fn corners(cell: TileCell) -> &'static [Corner] {
  match cell {
    TileCell::Solid => &[(0, 0), (1, 0), (1, 1), (0, 1)],
    TileCell::Slope(SlopeCorner::BottomLeft) => &[(0, 0), (1, 1), (0, 1)],
    TileCell::Slope(SlopeCorner::BottomRight) => &[(1, 0), (1, 1), (0, 1)],
    TileCell::Slope(SlopeCorner::TopLeft) => &[(0, 0), (1, 0), (0, 1)],
    TileCell::Slope(SlopeCorner::TopRight) => &[(0, 0), (1, 0), (1, 1)],
    TileCell::Empty | TileCell::OneWay | TileCell::Shape(_) => &[],
  }
}

/// A cell's corners in its grid's space (`TileColliders::cell_polygon`): 4
/// on a square lattice, 6 on a hexagonal grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CellPolygon {
  pub(crate) points: [Vec2; 6],
  pub(crate) len: u8,
}

impl CellPolygon {
  /// Its corners, clockwise on screen
  pub(crate) fn points(&self) -> &[Vec2] {
    &self.points[..usize::from(self.len)]
  }
}

/// Drop the points in the middle of straight runs; `points` is closed (its
/// last repeats its first) and so is the result.
fn merge_straight_runs(points: &[Point]) -> Vec<Point> {
  let ring = &points[..points.len().saturating_sub(1)];
  let n = ring.len();
  let step = |a: Point, b: Point| outline::delta(a, b);
  let mut kept: Vec<Point> = (0..n)
    .filter(|&i| {
      let (a, b) = (
        step(ring[(i + n - 1) % n], ring[i]),
        step(ring[i], ring[(i + 1) % n]),
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

/// Solid cells greedily merged into rectangles, from each solid cell not
/// yet taken in row order (by row, then column): (min corner, size).
pub(crate) fn solid_rects(cells: Cells) -> Vec<(Corner, Corner)> {
  let mut taken: FastSet<Corner> = FastSet::default();
  let free = |taken: &FastSet<Corner>, x: i32, y: i32| {
    cells.at(x, y) == TileCell::Solid && !taken.contains(&(x, y))
  };
  let mut rects = Vec::new();
  cells.each(|x, y, cell| {
    if cell != TileCell::Solid || taken.contains(&(x, y)) {
      return;
    }
    // A grid's cells are within `MAX_REACH`, so the ends cannot overflow
    let mut w = 1;
    while free(&taken, x + w, y) {
      w += 1;
    }
    let mut h = 1;
    while (x..x + w).all(|cx| free(&taken, cx, y + h)) {
      h += 1;
    }
    for ty in y..y + h {
      for tx in x..x + w {
        taken.insert((tx, ty));
      }
    }
    rects.push(((x, y), (w, h)));
  });
  rects
}

/// The most cell changes a grid lists between rebuilds; past it the next
/// rebuild builds everything.
const MAX_CHANGES: usize = 1 << 16;

/// A grid of tile cells that builds its own colliders: seamless outlines
/// around solid, slope and authored shape cells (`add_shape`,
/// `TileCell::Shape`), or merged rectangles with `solid_rects()`, and
/// one-way segments along runs of one-way cells and shapes. Its `Transform`
/// places cell (0, 0)'s top-left; +Y is down. Cells are signed and stored
/// by chunk, so an empty area costs nothing: a bounded grid (`new`,
/// `from_cells`) holds the cells from (0, 0) to its size, an unbounded one
/// (`unbounded`) any cell. A cell collides within `MAX_REACH` (186,411)
/// cells of (0, 0) on either axis; one past it is refused (logged once a
/// grid).
///
/// The colliders are child entities, brought up to date before the next
/// physics step after a change. The outlines are packed by area: the loops
/// starting in a chunk of 32 by 32 cells are one collider, and so are the
/// flat one-way runs starting in a chunk along one row (a sloped run is a
/// collider of its own); each sits at its chunk's top-left, its points
/// relative to it, so they stay small however far out the chunk is. An
/// edit retraces the outlines and runs through the cells it changed and
/// gives the colliders of only the chunks whose outlines or runs changed
/// their new shape in place, the same entities, so what stands on the rest
/// of such a chunk keeps its contacts; every other collider stays as it
/// was. With no body they are fixed; with a kinematic `RigidBody` the whole
/// map moves.
///
/// Disabling the map entity (`Disabled`) removes its colliders and enabling
/// it builds them again from the cells as they then are; so does removing
/// the component.
///
/// Its colliders are on its `floor` (`with_floor`; the ground floor by
/// default): off the ground floor each carries it as its `Floor`, and on
/// it they carry none. Neither the map entity's `Floor` nor its body's
/// reaches them. Changing it rebuilds them all.
///
/// Outlines are hollow: a body that ends up inside a solid region is not
/// pushed out. `solid_rects()` trades the seamless floors for solid
/// interiors (and rebuilds everything on any change).
///
/// A grid whose cells are not `tile_size` rectangles from the origin but
/// parallelograms of one lattice (a tile map's isometric, staggered or
/// oblique cells) takes the lattice with `with_lattice`: the outlines are
/// traced on the lattice's squares as on any grid, and their points mapped
/// through it before the colliders are built, so the parts still sit
/// unturned and unscaled at their chunks' corners.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(Transform)]
pub struct TileColliders {
  /// The cells it holds; None for any
  bounds: Option<CellRect>,
  /// Its cells by chunk; a chunk is made by the first cell written in it
  chunks: Chunks,
  /// The cells of the chunks there are (more, while `span_stale`)
  span: CellRect,
  /// A chunk on the edge of `span` was unloaded since it was last worked
  /// out: `extent` works it out from the chunks until `settle_extent`
  /// keeps it
  span_stale: bool,
  /// One cell's size, px: where its cells are, unless it has a lattice
  pub tile_size: Vec2,
  /// Lattice units to its space's pixels (`with_lattice`): its cells' place
  /// in place of `tile_size`'s rectangles
  lattice: Option<Affine2>,
  /// Hexagonal cells and the grid's origin in its space (`with_hex`): its
  /// cells' place in place of the lattice's or `tile_size`'s
  hex: Option<(HexCells, Vec2)>,
  pub layer: u32,
  pub mask: u32,
  /// The floor its parts are on (off the ground floor, each part carries
  /// it as its `Floor`)
  pub floor: Floor,
  pub friction: f32,
  pub restitution: f32,
  /// Build merged rectangles instead of outlines
  pub solid_rects: bool,
  /// Changes whenever the cells do (and is fresh for every new grid), so a
  /// rebuild can tell what it has already built
  generation: u64,
  /// The shapes `TileCell::Shape` cells name, each with what it builds
  shapes: Vec<(CellShape, ShapeBuild)>,
  /// One past the highest shape index the cells have held: adding a shape
  /// below it rebuilds the cells that may name it
  shape_refs: u32,
  /// The cells changed since generation `changes_since` (the one the
  /// rebuild last took in), each with its value before, or None when there
  /// were too many to list
  changes: Option<Vec<(IVec2, TileCell)>>,
  changes_since: u64,
  /// A cell past `MAX_REACH` was refused (and logged)
  warned_reach: bool,
}

static GENERATIONS: AtomicU64 = AtomicU64::new(1);

fn next_generation() -> u64 {
  GENERATIONS.fetch_add(1, Ordering::Relaxed)
}

impl TileColliders {
  /// An empty grid of the cells from (0, 0) up to (`width`, `height`).
  pub fn new(width: u32, height: u32, tile_size: Vec2) -> Self {
    Self::from_cells(width, height, tile_size, Vec::new())
  }

  /// A grid of the cells from (0, 0) up to (`width`, `height`), from its
  /// cells row by row from the top; missing cells are empty and extra ones
  /// are dropped. Only the chunks holding a cell are stored, so a long
  /// level costs what its cells do. A cell past `MAX_REACH` is dropped
  /// (logged).
  pub fn from_cells(width: u32, height: u32, tile_size: Vec2, cells: Vec<TileCell>) -> Self {
    let bounds = CellRect::from_size(UVec2::new(width, height));
    let mut grid = Self::with_bounds(Some(bounds), tile_size);
    if width == 0 {
      return grid;
    }
    let (side, reach) = (CHUNK as usize, MAX_REACH as usize);
    let mut past = None;
    let rows = cells.chunks(width as usize).take(height as usize);
    for (y, row) in rows.enumerate() {
      // The columns that collide: none in a row past the reach
      let within = if y <= reach {
        row.len().min(reach + 1)
      } else {
        0
      };
      if past.is_none()
        && let Some(x) = row[within..].iter().position(|&c| c != TileCell::Empty)
      {
        past = Some((within + x, y));
      }
      for (column, run) in row[..within].chunks(side).enumerate() {
        if run.iter().all(|&c| c == TileCell::Empty) {
          continue;
        }
        for &cell in run {
          if let TileCell::Shape(index) = cell {
            grid.shape_refs = grid.shape_refs.max(u32::from(index) + 1);
          }
        }
        // Both within the reach, so in an i32
        let chunk = IVec2::new(column as i32, (y / side) as i32);
        let start = (y % side) * side;
        grid.chunk_mut(chunk)[start..start + run.len()].copy_from_slice(run);
      }
    }
    if let Some((x, y)) = past {
      grid.past_reach(x, y);
    }
    grid
  }

  /// An empty grid holding any cell, signed (within `MAX_REACH`): a chunk
  /// is made the first time a cell is written in it.
  pub fn unbounded(tile_size: Vec2) -> Self {
    Self::with_bounds(None, tile_size)
  }

  fn with_bounds(bounds: Option<CellRect>, tile_size: Vec2) -> Self {
    let generation = next_generation();
    Self {
      bounds,
      chunks: Chunks::default(),
      span: CellRect::default(),
      span_stale: false,
      tile_size,
      lattice: None,
      hex: None,
      layer: 1,
      mask: u32::MAX,
      floor: Floor::GROUND,
      friction: 0.5,
      restitution: 0.0,
      solid_rects: false,
      generation,
      shapes: Vec::new(),
      shape_refs: 0,
      changes: Some(Vec::new()),
      changes_since: generation,
      warned_reach: false,
    }
  }

  /// The cells a bounded grid holds (from (0, 0) to its size); None for an
  /// unbounded one.
  pub fn bounds(&self) -> Option<CellRect> {
    self.bounds
  }

  /// The cells it holds: a bounded grid's bounds, an unbounded one's
  /// chunks' cells (empty before a cell is written).
  pub fn extent(&self) -> CellRect {
    match self.bounds {
      Some(bounds) => bounds,
      None if self.span_stale => self.chunks_span(),
      None => self.span,
    }
  }

  /// The cells of the chunks there are, worked out from them
  fn chunks_span(&self) -> CellRect {
    self
      .chunks
      .keys()
      .fold(CellRect::default(), |span, &c| span.union(chunk_rect(c)))
  }

  /// Keeps the extent worked out after chunks on its edge were unloaded
  /// (`unload_chunk` leaves that to here), so a run of unloads works it
  /// out once, not once a chunk
  pub(crate) fn settle_extent(&mut self) {
    if self.span_stale {
      self.span = self.chunks_span();
      self.span_stale = false;
    }
  }

  /// The cell at `cell`; `Empty` outside the grid.
  pub fn get(&self, cell: IVec2) -> TileCell {
    self.cells_view().at(cell.x, cell.y)
  }

  /// Change a cell (its colliders rebuild next step). Returns false, and
  /// changes nothing, for a cell outside the grid or one already `value`.
  /// A cell past `MAX_REACH` of (0, 0) does not collide: it is refused,
  /// logged the first time.
  pub fn set(&mut self, cell: IVec2, value: TileCell) -> bool {
    if self.bounds.is_some_and(|bounds| !bounds.contains(cell)) {
      return false;
    }
    if !reaches(cell) {
      if value != TileCell::Empty {
        self.past_reach(cell.x, cell.y);
      }
      return false;
    }
    let chunk = chunk_of(cell);
    if value == TileCell::Empty && !self.chunks.contains_key(&chunk) {
      return false;
    }
    let slot = &mut self.chunk_mut(chunk)[local(cell)];
    if *slot == value {
      return false;
    }
    let before = std::mem::replace(slot, value);
    if let TileCell::Shape(shape) = value {
      self.shape_refs = self.shape_refs.max(u32::from(shape) + 1);
    }
    self.generation = next_generation();
    match &mut self.changes {
      Some(changes) if changes.len() < MAX_CHANGES => changes.push((cell, before)),
      _ => self.changes = None,
    }
    true
  }

  /// Sets chunk `chunk`'s cells to `cells` (`CHUNK` rows of `CHUNK`) as a
  /// `set` of each would, in row order: each cell that changes is listed
  /// with its value before, a cell outside the grid is left out and a
  /// non-empty one past `MAX_REACH` refused (logged once a grid), and a
  /// chunk the grid lacks is made only for a cell that is not empty. One
  /// lookup and one generation for the chunk, where a `set` took both a
  /// cell (a tile map's chunk changed whole: a stream's landing). True if
  /// a cell changed
  pub(crate) fn set_chunk(&mut self, chunk: IVec2, cells: &Chunk) -> bool {
    let rect = chunk_rect_of(chunk);
    let bounds = self.bounds;
    let in_bounds = |cell: IVec2| bounds.is_none_or(|b| b.contains(cell));
    let at = |i: usize| rect.min + IVec2::new(i as i32 % CHUNK, i as i32 / CHUNK);
    // Most chunks lie wholly inside
    let whole = [rect.min, rect.max - IVec2::ONE]
      .into_iter()
      .all(|cell| reaches(cell) && in_bounds(cell));
    let taken = |cell: IVec2| whole || (reaches(cell) && in_bounds(cell));
    // A cell `set` refuses for its reach is logged, as `set` logs it
    let refused =
      |cell: IVec2, value: TileCell| value != TileCell::Empty && in_bounds(cell) && !reaches(cell);
    if !self.chunks.contains_key(&chunk)
      && !(0..CELLS).any(|i| cells[i] != TileCell::Empty && taken(at(i)))
    {
      if let Some(cell) = (0..CELLS)
        .map(at)
        .find(|&cell| refused(cell, cells[local(cell)]))
      {
        self.past_reach(cell.x, cell.y);
      }
      return false;
    }
    let TileColliders {
      chunks,
      span,
      changes,
      shape_refs,
      ..
    } = self;
    let stored = chunks.entry(chunk).or_insert_with(|| {
      *span = span.union(chunk_rect(chunk));
      Box::new([TileCell::Empty; CELLS])
    });
    let (mut changed, mut past) = (false, None);
    for (i, (&value, slot)) in cells.iter().zip(stored.iter_mut()).enumerate() {
      if *slot == value {
        continue;
      }
      let cell = at(i);
      if !taken(cell) {
        if past.is_none() && refused(cell, value) {
          past = Some(cell);
        }
        continue;
      }
      let before = std::mem::replace(slot, value);
      if let TileCell::Shape(shape) = value {
        *shape_refs = (*shape_refs).max(u32::from(shape) + 1);
      }
      changed = true;
      match changes {
        Some(list) if list.len() < MAX_CHANGES => list.push((cell, before)),
        _ => *changes = None,
      }
    }
    if let Some(cell) = past {
      self.past_reach(cell.x, cell.y);
    }
    if changed {
      self.generation = next_generation();
    }
    changed
  }

  /// `chunk`'s cells, made empty if the grid has none there
  fn chunk_mut(&mut self, chunk: IVec2) -> &mut Chunk {
    let span = &mut self.span;
    self.chunks.entry(chunk).or_insert_with(|| {
      *span = span.union(chunk_rect(chunk));
      Box::new([TileCell::Empty; CELLS])
    })
  }

  /// Logs, the first time, that the cell at (x, y) is past `MAX_REACH`
  fn past_reach(&mut self, x: impl std::fmt::Display, y: impl std::fmt::Display) {
    if std::mem::replace(&mut self.warned_reach, true) {
      return;
    }
    crate::engine_warn!(
      module: "Physics",
      "a TileColliders cell at ({x}, {y}) is past the reach of tile collision, {MAX_REACH} cells from (0, 0) on either axis, so it does not collide (logged once a grid)"
    );
  }

  /// Each cell holding something (not `Empty`) in `area`, read from the
  /// chunks overlapping it only, in no set order.
  pub(crate) fn cells_in(&self, area: CellRect) -> impl Iterator<Item = (IVec2, TileCell)> + '_ {
    self
      .chunks
      .iter()
      .filter(move |(chunk, _)| {
        let rect = chunk_rect(**chunk);
        rect.min.cmplt(area.max).all() && area.min.cmplt(rect.max).all()
      })
      .flat_map(move |(chunk, cells)| {
        let origin = *chunk * CHUNK;
        (0..CHUNK)
          .flat_map(|y| (0..CHUNK).map(move |x| IVec2::new(x, y)))
          .zip(cells.iter())
          .filter_map(move |(at, &cell)| {
            let at = origin + at;
            (cell != TileCell::Empty && area.contains(at)).then_some((at, cell))
          })
      })
  }

  /// Add an authored shape for `TileCell::Shape` cells to name; returns its
  /// index. Shapes cannot be changed once added. A grid holds up to 65,535
  /// shapes; past that it logs and returns `u16::MAX`, which names none.
  pub fn add_shape(&mut self, shape: CellShape) -> u16 {
    let index = self.shapes.len();
    if index >= usize::from(u16::MAX) {
      crate::engine_warn!(
        module: "Physics",
        "a TileColliders holds at most 65,535 shapes; this one is dropped"
      );
      return u16::MAX;
    }
    let build = ShapeBuild::of(&shape);
    self.shapes.push((shape, build));
    // Cells may already name it: they build now
    if (index as u32) < self.shape_refs {
      self.generation = next_generation();
      self.changes = None;
    }
    index as u16
  }

  pub(crate) fn cells_view(&self) -> Cells<'_> {
    Cells::new(&self.chunks, &self.shapes)
  }

  /// How many chunks of cells it stores (tests: what an unload leaves)
  #[cfg(test)]
  pub(crate) fn chunk_count(&self) -> usize {
    self.chunks.len()
  }

  /// The cells of `chunk` (`CHUNK` rows of `CHUNK`), if the grid has it:
  /// for a builder comparing a chunk without a lookup per cell
  #[cfg(test)]
  pub(crate) fn chunk_cells(&self, chunk: IVec2) -> Option<&Chunk> {
    self.chunks.get(&chunk).map(|cells| &**cells)
  }

  /// Writes `cells` (`CHUNK` rows of `CHUNK`) as chunk `chunk`'s, for a
  /// grid being made from chunks: a cell outside the grid is left out, and
  /// so is one past `MAX_REACH` (logged once). It lists no change, so a
  /// grid already built is built again whole; a chunk of nothing but
  /// `Empty` makes none.
  pub(crate) fn put_chunk(&mut self, chunk: IVec2, cells: &Chunk) {
    let rect = chunk_rect_of(chunk);
    let inside = |cell: IVec2| reaches(cell) && self.bounds.is_none_or(|b| b.contains(cell));
    // Most chunks lie wholly inside: copied as they are
    let whole = inside(rect.min) && inside(rect.max - IVec2::ONE);
    let mut kept = *cells;
    let mut past = None;
    if !whole {
      for (i, slot) in kept.iter_mut().enumerate() {
        let cell = rect.min + IVec2::new(i as i32 % CHUNK, i as i32 / CHUNK);
        if *slot != TileCell::Empty && !inside(cell) {
          if past.is_none() && !reaches(cell) {
            past = Some(cell);
          }
          *slot = TileCell::Empty;
        }
      }
    }
    if let Some(cell) = past {
      self.past_reach(cell.x, cell.y);
    }
    if kept.iter().all(|&c| c == TileCell::Empty) && !self.chunks.contains_key(&chunk) {
      return;
    }
    for &cell in &kept {
      if let TileCell::Shape(index) = cell {
        self.shape_refs = self.shape_refs.max(u32::from(index) + 1);
      }
    }
    *self.chunk_mut(chunk) = kept;
    self.generation = next_generation();
    self.changes = None;
  }

  /// Takes `chunk` out whole: each cell it held leaves as `set` would empty
  /// it, listed as a change with what it held, so the next rebuild retraces
  /// only the loops and runs through it (a loop running on into the chunk
  /// closes at its edge), and its storage goes with it, so a grid streamed
  /// across an endless world costs only the chunks it holds. A chunk the
  /// grid lacks changes nothing. The extent shrinks to the chunks left:
  /// one taken from its edge has it worked out again when read, or once by
  /// `settle_extent` after a run of unloads
  pub(crate) fn unload_chunk(&mut self, chunk: IVec2) {
    let Some(cells) = self.chunks.remove(&chunk) else {
      return;
    };
    self.forgot(chunk);
    let origin = chunk * CHUNK;
    let held = cells
      .iter()
      .enumerate()
      .filter(|&(_, &cell)| cell != TileCell::Empty);
    for (at, &before) in held {
      let cell = origin + IVec2::new(at as i32 % CHUNK, at as i32 / CHUNK);
      match &mut self.changes {
        Some(changes) if changes.len() < MAX_CHANGES => changes.push((cell, before)),
        _ => self.changes = None,
      }
    }
    self.generation = next_generation();
  }

  /// Marks the extent to be worked out again if `chunk`, just taken out,
  /// was on its edge (the cells of the chunks left are the same otherwise)
  fn forgot(&mut self, chunk: IVec2) {
    let rect = chunk_rect(chunk);
    let span = self.span;
    if rect.min.x <= span.min.x
      || rect.min.y <= span.min.y
      || rect.max.x >= span.max.x
      || rect.max.y >= span.max.y
    {
      self.span_stale = true;
    }
  }

  /// Sets each cell to its value as `set` would, in order, then takes out
  /// every chunk a cell was emptied in that holds nothing more, its storage
  /// with it, as `unload_chunk` would (its cells were listed as changes as
  /// they emptied): so cells a tile map scatters across the grid's chunks
  /// (a staggered map's, whose chunks of cells straddle the lattice's) can
  /// leave without their chunks staying. The extent is left to
  /// `settle_extent`. True if a cell changed
  pub(crate) fn set_cells(&mut self, cells: impl IntoIterator<Item = (IVec2, TileCell)>) -> bool {
    let mut changed = false;
    let mut emptied: Vec<IVec2> = Vec::new();
    for (cell, value) in cells {
      if !self.set(cell, value) {
        continue;
      }
      changed = true;
      let chunk = chunk_of(cell);
      if value == TileCell::Empty && !emptied.contains(&chunk) {
        emptied.push(chunk);
      }
    }
    for chunk in emptied {
      let left = self.chunks.get(&chunk);
      if left.is_some_and(|cells| cells.iter().all(|&c| c == TileCell::Empty)) {
        self.chunks.remove(&chunk);
        self.forgot(chunk);
      }
    }
    changed
  }

  /// The cells changed since the rebuild last took them in, in row order
  /// (by row, then column), each once with its value then; None when there
  /// were too many to list. The list starts again from now.
  pub(crate) fn take_changes(&mut self) -> Option<Vec<(IVec2, TileCell)>> {
    let changes = self.changes.replace(Vec::new());
    self.changes_since = self.generation;
    changes.map(|mut list| {
      // Stable: a cell's first entry holds its value before them all
      list.sort_by_key(|&(cell, _)| (cell.y, cell.x));
      list.dedup_by_key(|&mut (cell, _)| cell);
      list
    })
  }

  /// The shape `TileCell::Shape(index)` names.
  pub fn shape(&self, index: u16) -> Option<&CellShape> {
    self.shapes.get(usize::from(index)).map(|(shape, _)| shape)
  }

  /// Whether a `TileCell::Shape(index)` cell blocks navigation: its shape
  /// is solid and covers the cell's centre.
  pub(crate) fn shape_blocks(&self, index: u16) -> bool {
    self
      .shapes
      .get(usize::from(index))
      .is_some_and(|(_, build)| build.blocks)
  }

  pub fn with_layer(mut self, layer: u32) -> Self {
    self.layer = layer;
    self
  }

  pub fn with_mask(mut self, mask: u32) -> Self {
    self.mask = mask;
    self
  }

  /// Put its parts on `floor` (the ground floor by default).
  pub fn with_floor(mut self, floor: Floor) -> Self {
    self.floor = floor;
    self
  }

  pub fn with_friction(mut self, friction: f32) -> Self {
    self.friction = friction;
    self
  }

  pub fn with_restitution(mut self, restitution: f32) -> Self {
    self.restitution = restitution;
    self
  }

  /// Merged rectangles instead of outlines (solid interiors, seamed floors).
  pub fn solid_rects(mut self) -> Self {
    self.solid_rects = true;
    self
  }

  /// Places its cells through `to_pixel`, lattice units to its space's
  /// pixels (cell (x, y) is the unit square at (x, y), y down), in place of
  /// `tile_size` rectangles from the origin: what a tile map's isometric,
  /// staggered and oblique layers give, their grid's `Lattice::to_pixel`.
  /// Outlines, runs and shapes are worked out on the lattice's squares and
  /// their points mapped through it; `tile_size` is left as what the grid
  /// reports. A lattice that is not finite, or folds its cells flat, builds
  /// nothing (logged). One-way cells keep their runs along their tops on
  /// the lattice, facing out of them as the lattice turns them (a tile map
  /// off an orthogonal grid makes its one-way tiles solid instead).
  /// Navigation reads its cells by their outlines through the lattice
  /// (`NavigationGrid::block_tiles`).
  pub fn with_lattice(mut self, to_pixel: Affine2) -> Self {
    self.lattice = Some(to_pixel);
    self
  }

  /// The lattice its cells are placed through (`with_lattice`), if any
  pub fn lattice(&self) -> Option<Affine2> {
    self.lattice
  }

  /// Its cells are `hex`'s hexagons, `origin` being cell (0, 0)'s box
  /// corner in its space, in place of `tile_size` rectangles or a lattice's
  /// (a tile map's hexagonal layers). Its colliders hold the hexagons'
  /// boundary edges as they are, not traced into loops: each side with one
  /// solid cell, clockwise round it, held by the part of the side's earlier
  /// cell in row order (on a bounded grid, a side on its border by the cell
  /// inside it). A shape cell collides as its own shape's polygons, each a
  /// closed ring over its cell's box (a shape's tile units on a hexagonal
  /// grid: (0, 0) the box's top-left, (1, 1) its bottom-right), as
  /// authored, wherever it lies: neither clipped to the cell nor merged
  /// with its neighbours, wound clockwise on screen as outlines are (one
  /// that touches or crosses itself, its convex hull), and held by a part
  /// of rings apart from the sides. A part holds at most
  /// `pack::HEX_PART_EDGES` edges where it can, a chunk's sides and its
  /// rings each by their own count: a chunk of 32 x 32 cells is one part
  /// while they hold that many at most, else its 16 x 16 quarters are, else
  /// their 8 x 8 blocks, and a block's rings past it its 4 x 4 sub-blocks
  /// (`pack::HexParts`), so an edit rebuilds at most four parts of sides of
  /// that size (more where an edit takes a part past it, which splits it),
  /// and, where a cell's shape changed, the part of rings holding it (more
  /// where the edit takes a box of rings past it, which splits it); a part
  /// sits at `origin` plus its first cell's box corner, its points relative
  /// to it.
  /// Only `Solid` cells are solid; one-way and slope cells and one-way
  /// shapes add nothing, and there are no one-way runs (a tile map makes
  /// one-way tiles solid off orthogonal grids). `solid_rects` is ignored.
  /// A cell whose corners, or its neighbours', pass an `i32` of pixels
  /// (`HexCells::reaches`) does not collide, as a cell past `MAX_REACH`
  /// does not. A grid whose cells the tracer does not take
  /// (`HexCells::traceable`: over 16,384 px, or flat) builds nothing,
  /// logged once a map.
  pub(crate) fn with_hex(mut self, hex: HexCells, origin: Vec2) -> Self {
    self.hex = Some((hex, origin));
    self
  }

  /// Its hexagonal cells and their origin (`with_hex`), if any
  pub(crate) fn hex(&self) -> Option<(HexCells, Vec2)> {
    self.hex
  }

  /// `cell`'s corners in its space, px (before its `Transform`), clockwise
  /// on screen: the 4 of its square through the lattice (`with_lattice`)
  /// or as a `tile_size` rectangle from the origin, or the 6 of its
  /// hexagon (`with_hex`; a side of length 0 repeats its corner). Worked
  /// out in f64 and rounded once
  pub(crate) fn cell_polygon(&self, cell: IVec2) -> CellPolygon {
    let mut points = [Vec2::ZERO; 6];
    if let Some((hex, origin)) = self.hex {
      let origin = origin.as_dvec2();
      for (point, corner) in points.iter_mut().zip(hex.corners(cell)) {
        *point = (origin + corner.as_dvec2()).as_vec2();
      }
      return CellPolygon { points, len: 6 };
    }
    let at = cell.as_dvec2();
    for (point, step) in points
      .iter_mut()
      .zip([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)])
    {
      let corner = at + DVec2::new(step.0, step.1);
      *point = match self.lattice {
        Some(to) => through(to, corner),
        None => (corner * self.tile_size.as_dvec2()).as_vec2(),
      };
    }
    CellPolygon { points, len: 4 }
  }

  /// Each part's place and collider (relative to it, without the grid's
  /// material), traced now: what a build spawns (tests outside physics)
  #[cfg(test)]
  pub(crate) fn traced_parts(&self) -> Vec<(Vec2, Collider)> {
    let mut kept = self.traced_whole();
    self
      .pack_all(&mut kept)
      .into_iter()
      .map(|(_, at, collider)| (at, collider))
      .collect()
  }

  /// Rectangles in place of outlines: `solid_rects`, off hexagonal grids
  fn rects_mode(&self) -> bool {
    self.solid_rects && self.hex.is_none()
  }

  /// What a build keeps, traced whole: the outlines and one-way runs, or a
  /// hexagonal grid's parts' edges
  fn traced_whole(&self) -> Kept {
    match self.hex {
      Some((hex, _)) => Kept {
        hex: HexParts::build(self.cells_view(), hex, self.bounds()),
        ..Kept::default()
      },
      None => Kept {
        outlines: Outlines::build(self.cells_view()),
        runs: Runs::build(self.cells_view()),
        ..Kept::default()
      },
    }
  }

  /// Hexagonal part `key`'s place (its first cell's box corner from the
  /// origin) and its collider, segments from that corner: a part of
  /// sides', its edges in whole pixels, each edge going on from the last
  /// one's end sharing its point; a part of rings', each of its shape
  /// cells' rings, closed, over its cell's box (`HexCells::cell_box`: a
  /// shape's tile units)
  fn hex_pack(&self, parts: &HexParts, key: PackKey) -> (Vec2, Collider) {
    let Some((hex, origin)) = self.hex else {
      return (Vec2::ZERO, Collider::segments(Vec::new(), Vec::new()));
    };
    let corner = hex.corner_of(hex_first_cell(key));
    let at = (origin.as_dvec2() + corner.as_dvec2()).as_vec2();
    let point = |(x, y): Point| {
      Vec2::new(
        (i64::from(x) - corner.x) as f32,
        (i64::from(y) - corner.y) as f32,
      )
    };
    let (mut points, mut segments) = (Vec::new(), Vec::new());
    if let PackKey::Hex { rings: true, .. } = key {
      for (cell, shape) in parts.rings(key) {
        let Some((_, build)) = self.shapes.get(usize::from(shape)) else {
          continue;
        };
        for ring in &build.rings {
          let base = points.len() as u32;
          points.extend(hex_ring(hex, cell, corner, ring).map(DVec2::as_vec2));
          let n = ring.len() as u32;
          segments.extend((0..n).map(|i| [base + i, base + (i + 1) % n]));
        }
      }
      return (at, Collider::segments(points, segments));
    }
    let mut last = None;
    for (a, b) in parts.edges(key) {
      if last != Some(a) {
        points.push(point(a));
      }
      points.push(point(b));
      let end = points.len() as u32 - 1;
      segments.push([end - 1, end]);
      last = Some(b);
    }
    (at, Collider::segments(points, segments))
  }

  /// A hexagonal part's collider built again from its edges as they now
  /// are: its part takes it in place (the same entity), or one is spawned
  /// for a part that has none or whose part is gone (`alive` false: one
  /// still there is despawned, as `repack` does)
  fn hex_repack(
    &self,
    kept: &mut Kept,
    key: PackKey,
    alive: &dyn Fn(Entity) -> bool,
    commands: &mut Commands,
    map: Entity,
  ) {
    let (at, collider) = self.hex_pack(&kept.hex, key);
    match kept.hex.part(key).filter(|&part| alive(part)) {
      Some(part) => {
        commands
          .entity(part)
          .try_insert(self.material(collider))
          .try_remove::<PrebuiltShape>();
      }
      None => {
        if let Some(stray) = kept.hex.take_part(key) {
          commands.entity(stray).try_despawn();
        }
        self.spawn_pack(kept, key, at, collider, None, commands, map);
      }
    }
  }

  /// A cell corner in its space, px
  fn px(&self, (x, y): Corner) -> Vec2 {
    match self.lattice {
      Some(to) => through(to, DVec2::new(f64::from(x), f64::from(y))),
      None => Vec2::new(x as f32 * self.tile_size.x, y as f32 * self.tile_size.y),
    }
  }

  /// A point in tiles from cell (0, 0)'s top-left (a shape's) in its space,
  /// px
  fn place(&self, point: Vec2) -> Vec2 {
    match self.lattice {
      Some(to) => through(to, point.as_dvec2()),
      None => point * self.tile_size,
    }
  }

  /// A point of the outline grid in its space, px
  fn point(&self, p: Point) -> Vec2 {
    match self.lattice {
      Some(to) => {
        let grid = f64::from(GRID);
        through(to, DVec2::new(f64::from(p.0) / grid, f64::from(p.1) / grid))
      }
      None => self.at(p, (0, 0)),
    }
  }

  /// Whether its cells have somewhere to be: a finite, positive tile size,
  /// or a finite lattice that does not fold them flat, or hexagons the
  /// tracer takes from a finite origin
  fn placeable(&self) -> bool {
    if let Some((hex, origin)) = self.hex {
      return hex.traceable() && origin.is_finite();
    }
    match self.lattice {
      Some(to) => to.is_finite() && to.matrix2.determinant() != 0.0,
      None => self.tile_size.is_finite() && self.tile_size.min_element() > 0.0,
    }
  }

  fn material(&self, collider: Collider) -> Collider {
    collider
      .with_layer(self.layer)
      .with_mask(self.mask)
      .with_friction(self.friction)
      .with_restitution(self.restitution)
  }

  /// A part of the map, in its space with the grid's material and floor.
  fn spawn(&self, commands: &mut Commands, map: Entity, collider: Collider) -> Entity {
    let collider = self.material(collider);
    let part = commands
      .spawn((Transform::IDENTITY, collider, TilePart, ChildOf(map)))
      .id();
    self.put_on_floor(commands, part);
    part
  }

  /// Give a part off the ground floor the grid's `Floor`; one on the ground
  /// floor has none, so a map without floors costs what it always did.
  fn put_on_floor(&self, commands: &mut Commands, part: Entity) {
    if self.floor != Floor::GROUND {
      commands.entity(part).insert(self.floor);
    }
  }

  /// The top-left of `chunk` (of `CHUNK` cells a side) in the map's
  /// space, px: where a pack's part sits (on a lattice, where it takes the
  /// chunk's first corner).
  fn origin(&self, chunk: (i32, i32)) -> Vec2 {
    let side = f64::from(CHUNK);
    if let Some(to) = self.lattice {
      let corner = DVec2::new(f64::from(chunk.0), f64::from(chunk.1)) * side;
      return through(to, corner);
    }
    Vec2::new(
      (f64::from(chunk.0) * side * f64::from(self.tile_size.x)) as f32,
      (f64::from(chunk.1) * side * f64::from(self.tile_size.y)) as f32,
    )
  }

  /// A point of the outline grid from the top-left of `chunk`, px. Worked
  /// out in f64, where every lattice point and chunk origin is exact, so
  /// the point stays small (and as exact as in chunk (0, 0)) however far
  /// out the chunk is.
  fn at(&self, (x, y): Point, chunk: (i32, i32)) -> Vec2 {
    let (grid, side) = (f64::from(GRID), f64::from(GRID) * f64::from(CHUNK));
    let from = |p: i32, chunk: i32| (f64::from(p) - f64::from(chunk) * side) / grid;
    if let Some(to) = self.lattice {
      // The lattice's step from the chunk's corner, without its translation
      let step = DVec2::new(from(x, chunk.0), from(y, chunk.1));
      let (a, b) = (to.matrix2.x_axis.as_dvec2(), to.matrix2.y_axis.as_dvec2());
      return (a * step.x + b * step.y).as_vec2();
    }
    let along = |p: i32, chunk: i32, tile: f32| (from(p, chunk) * f64::from(tile)) as f32;
    Vec2::new(
      along(x, chunk.0, self.tile_size.x),
      along(y, chunk.1, self.tile_size.y),
    )
  }

  /// A one-way run as a part of its own (`solid_rects` mode): a segment
  /// from its first point to its last, solid from the side it faces (up,
  /// for a flat run).
  fn spawn_run(&self, commands: &mut Commands, map: Entity, points: &[Point]) -> Entity {
    let (a, b) = (self.point(points[0]), self.point(points[points.len() - 1]));
    let along = b - a;
    let facing = Vec2::new(along.y, -along.x).normalize_or(Vec2::NEG_Y);
    self.spawn(
      commands,
      map,
      Collider::segment(a, b).one_way_towards(facing),
    )
  }

  /// Every part of the map; what an edit can rebuild in place is kept
  /// (none in `solid_rects` mode).
  fn spawn_all(&self, commands: &mut Commands, map: Entity) -> Option<Kept> {
    if self.rects_mode() {
      let runs = Runs::build(self.cells_view());
      for (min, cells) in solid_rects(self.cells_view()) {
        let rect = if self.lattice.is_some() {
          // A rectangle of the lattice's squares: its corners mapped
          let corners = [(0, 0), (cells.0, 0), cells, (0, cells.1)];
          Collider::polygon(corners.map(|c| self.px((min.0 + c.0, min.1 + c.1))))
        } else {
          let extent = self.px(cells);
          let centre = self.px(min) + extent / 2.0;
          Collider::rect(extent.x, extent.y).with_offset(centre.x, centre.y)
        };
        self.spawn(commands, map, rect);
      }
      self.cells_view().each(|x, y, cell| match cell {
        TileCell::Slope(_) => {
          let points = corners(cell).iter().map(|c| self.px((x + c.0, y + c.1)));
          self.spawn(commands, map, Collider::polygon(points));
        }
        TileCell::Shape(shape) => {
          let origin = Vec2::new(x as f32, y as f32);
          let parts = self
            .shapes
            .get(usize::from(shape))
            .map_or(&[][..], |(_, build)| &build.parts[..]);
          for part in parts {
            let points = part.iter().map(|&p| self.place(origin + p));
            self.spawn(commands, map, Collider::polygon(points));
          }
        }
        _ => {}
      });
      for (_, run) in runs.runs() {
        self.spawn_run(commands, map, &run.points);
      }
      return None;
    }
    let mut kept = self.traced_whole();
    for (key, at, collider) in self.pack_all(&mut kept) {
      self.spawn_pack(&mut kept, key, at, collider, None, commands, map);
    }
    Some(kept)
  }

  /// Puts every loop and run of `kept` in its pack; each pack's part's
  /// place and collider (relative to it, without the grid's material), in
  /// the order the packs were met.
  fn pack_all(&self, kept: &mut Kept) -> Vec<(PackKey, Vec2, Collider)> {
    if self.hex.is_some() {
      let keys = kept.hex.keys();
      return keys
        .into_iter()
        .map(|key| {
          let (at, collider) = self.hex_pack(&kept.hex, key);
          (key, at, collider)
        })
        .collect();
    }
    type Gathered = FastMap<PackKey, ((i32, i32), Geometry)>;
    /// Pack `key`'s geometry, its part at the top-left of `chunk`
    fn gather<'g>(
      (gathered, order): (&'g mut Gathered, &mut Vec<PackKey>),
      key: PackKey,
      chunk: (i32, i32),
    ) -> &'g mut Geometry {
      let entry = gathered.entry(key).or_insert_with(|| {
        order.push(key);
        (chunk, Geometry::default())
      });
      &mut entry.1
    }
    let mut gathered = Gathered::default();
    let mut order = Vec::new();
    let mut dirty = Vec::new();
    for (id, outline) in kept.outlines.loops() {
      let key = loop_key(&outline.corners);
      let chunk = key.chunk(outline.corners.first().copied().unwrap_or_default());
      kept.loop_packs.place(id, key, &mut dirty);
      dirty.clear();
      let ring = ring(&outline.corners);
      gather((&mut gathered, &mut order), key, chunk)
        .add_ring(ring.into_iter().map(|p| self.at(p, chunk)));
    }
    for (id, run) in kept.runs.runs() {
      let key = run_key(id, &run.points);
      kept.run_packs.place(id, key, &mut dirty);
      dirty.clear();
      let (a, b) = ends(&run.points);
      let chunk = key.chunk(a);
      gather((&mut gathered, &mut order), key, chunk)
        .add_segment(self.at(a, chunk), self.at(b, chunk));
    }
    order
      .into_iter()
      .map(|key| {
        let (chunk, geometry) = gathered.remove(&key).unwrap_or_default();
        let collider = match key {
          PackKey::Loops(..) => geometry.loops(),
          _ => geometry.runs(),
        };
        (key, self.origin(chunk), collider)
      })
      .collect()
  }

  /// Spawns pack `key`'s part at `at`, `collider` with the grid's material
  /// and floor (and its shape built ahead, if any), as the part of the
  /// pack's members.
  #[allow(clippy::too_many_arguments)] // the part, then where it goes
  fn spawn_pack(
    &self,
    kept: &mut Kept,
    key: PackKey,
    at: Vec2,
    collider: Collider,
    shape: Option<SharedShape>,
    commands: &mut Commands,
    map: Entity,
  ) {
    let collider = self.material(collider);
    let part = (
      Transform::from_xy(at.x, at.y),
      collider,
      TilePart,
      ChildOf(map),
    );
    let part = match shape {
      Some(shape) => commands.spawn((part, PrebuiltShape(shape))).id(),
      None => commands.spawn(part).id(),
    };
    self.put_on_floor(commands, part);
    self.adopt(kept, key, part);
  }

  /// Records `part` as pack `key`'s, and as the part of each of its
  /// members.
  fn adopt(&self, kept: &mut Kept, key: PackKey, part: Entity) {
    if let PackKey::Hex { .. } = key {
      kept.hex.set_part(key, part);
    } else if let PackKey::Loops(..) = key {
      kept.loop_packs.set_part(key, part);
      for &id in kept.loop_packs.members(key) {
        if let Some(outline) = kept.outlines.get_mut(id) {
          outline.part = Some(part);
        }
      }
    } else {
      kept.run_packs.set_part(key, part);
      for &id in kept.run_packs.members(key) {
        if let Some(run) = kept.runs.get_mut(id) {
          run.part = Some(part);
        }
      }
    }
  }

  /// The outlines and one-way runs traced now and packed, each pack's
  /// collider with its Rapier shape built, for the build to spawn without
  /// tracing or building shapes (`Traced`); pure, so any thread can do it.
  /// None in `solid_rects` mode.
  pub(crate) fn trace(&self) -> Option<Traced> {
    if self.rects_mode() {
      return None;
    }
    let mut kept = self.traced_whole();
    let built = self
      .pack_all(&mut kept)
      .into_iter()
      .map(|(key, at, collider)| {
        let shape = super::shape::build(&collider.shape, Mat2::IDENTITY);
        (key, at, collider, shape)
      })
      .collect();
    Some(Traced {
      generation: self.generation,
      tile_size: self.tile_size,
      lattice: self.lattice,
      hex: self.hex,
      content: Some((kept, built)),
    })
  }

  /// Rebuild what the cells at `changed` touch (in row order, each with its
  /// value before): the outline loops and one-way runs through them, and
  /// the colliders of the packs they leave or join (`alive` tells a part
  /// still there).
  fn respawn(
    &self,
    kept: &mut Kept,
    changed: &[(IVec2, TileCell)],
    alive: &dyn Fn(Entity) -> bool,
    commands: &mut Commands,
    map: Entity,
  ) {
    if let Some((hex, _)) = self.hex {
      let changes = kept
        .hex
        .apply(self.cells_view(), hex, self.bounds(), changed);
      for part in changes.gone {
        commands.entity(part).try_despawn();
      }
      for key in changes.rebuilt {
        self.hex_repack(kept, key, alive, commands, map);
      }
      return;
    }
    let mut dirty = Vec::new();
    let retraced = kept.outlines.apply(self.cells_view(), changed);
    let parts = retraced.dropped.into_iter().flatten();
    for stray in kept.loop_packs.dropped(parts, &mut dirty) {
      commands.entity(stray).try_despawn();
    }
    for id in retraced.traced {
      if let Some(outline) = kept.outlines.get(id) {
        let key = loop_key(&outline.corners);
        kept.loop_packs.place(id, key, &mut dirty);
      }
    }
    let rechained = kept.runs.apply(self.cells_view(), changed);
    let parts = rechained.dropped.into_iter().flatten();
    for stray in kept.run_packs.dropped(parts, &mut dirty) {
      commands.entity(stray).try_despawn();
    }
    for id in rechained.traced {
      if let Some(run) = kept.runs.get(id) {
        kept
          .run_packs
          .place(id, run_key(id, &run.points), &mut dirty);
      }
    }
    dirty.sort_unstable();
    dirty.dedup();
    for key in dirty {
      self.repack(kept, key, alive, commands, map);
    }
  }

  /// Builds pack `key`'s collider again from its members as they now are.
  /// Its part takes the new shape in place (the same entity, so physics
  /// keeps its collider, contacts and all); a pack that had none, or whose
  /// part is gone (`alive` false), spawns one, and a pack left with no
  /// members despawns its part.
  fn repack(
    &self,
    kept: &mut Kept,
    key: PackKey,
    alive: &dyn Fn(Entity) -> bool,
    commands: &mut Commands,
    map: Entity,
  ) {
    let mut geometry = Geometry::default();
    let loops = matches!(key, PackKey::Loops(..));
    // Its members' points relative to the part's chunk (a sloped run's the
    // chunk of its first point, so its part follows the run)
    let (collider, chunk) = if loops {
      let outlines = &kept.outlines;
      let members = kept.loop_packs.settle(key, |id| outlines.get(id).is_some());
      let chunk = key.chunk((0, 0));
      for outline in members.iter().filter_map(|&id| outlines.get(id)) {
        geometry.add_ring(
          ring(&outline.corners)
            .into_iter()
            .map(|p| self.at(p, chunk)),
        );
      }
      ((!members.is_empty()).then(|| geometry.loops()), chunk)
    } else {
      let runs = &kept.runs;
      let members = kept.run_packs.settle(key, |id| runs.get(id).is_some());
      let first = members.first().and_then(|&id| runs.get(id));
      let chunk = key.chunk(first.map_or((0, 0), |run| ends(&run.points).0));
      for run in members.iter().filter_map(|&id| runs.get(id)) {
        let (a, b) = ends(&run.points);
        geometry.add_segment(self.at(a, chunk), self.at(b, chunk));
      }
      ((!members.is_empty()).then(|| geometry.runs()), chunk)
    };
    let at = self.origin(chunk);
    let packs = if loops {
      &mut kept.loop_packs
    } else {
      &mut kept.run_packs
    };
    let part = packs.part(key).filter(|&part| alive(part));
    match (collider, part) {
      (Some(collider), Some(part)) => {
        // A shape built ahead for its landing is no longer its shape
        let mut entity = commands.entity(part);
        entity
          .try_insert(self.material(collider))
          .try_remove::<PrebuiltShape>();
        // A sloped run's pack is one run, whose id an edit may give to a
        // run elsewhere: the part moves with it, keeping its entity
        if let PackKey::Run(_) = key {
          entity.try_insert(Transform::from_xy(at.x, at.y));
        }
        self.adopt(kept, key, part);
      }
      (collider, _) => {
        if let Some(old) = packs.take_part(key) {
          commands.entity(old).try_despawn();
        }
        if let Some(collider) = collider {
          self.spawn_pack(kept, key, at, collider, None, commands, map);
        }
      }
    }
  }
}

/// A hexagonal grid's shape cell `cell`'s ring (one of its shape's
/// `ShapeBuild::rings`, in the units of its cell's box) in pixels from
/// `corner` (the box corner of a part's or a block's first cell,
/// `HexCells::corner_of`): laid over `cell`'s box (`HexCells::cell_box`),
/// worked out in f64. A part's collider and a map's shadows place rings
/// alike through it
fn hex_ring(
  hex: HexCells,
  cell: IVec2,
  corner: glam::I64Vec2,
  ring: &[Vec2],
) -> impl Iterator<Item = DVec2> + '_ {
  let (min, size) = hex.cell_box(cell);
  let (from, size) = ((min - corner).as_dvec2(), size.as_dvec2());
  ring.iter().map(move |p| from + p.as_dvec2() * size)
}

/// Cells and authored shapes whose boundary is read a block at a time, by
/// the outline code's units without its loops or colliders: what a tile
/// map's light occluders are built from, so their edges meet and cancel as
/// the outlines' do, across blocks too. On a hexagonal grid (`with_hex`)
/// the units are its colliders' (each cell's sides, by their owner) and a
/// shape cell's own rings come apart (`hex_rings`).
#[derive(Debug, Default)]
pub(crate) struct Boundary {
  shapes: Vec<(CellShape, ShapeBuild)>,
  /// A hexagonal grid's cells (`with_hex`), read by their sides
  hex: Option<HexCells>,
}

impl Boundary {
  /// Reads the cells as `hex`'s hexagons, by the same units as a
  /// hexagonal grid's colliders: each cell's sides, each owned by
  /// whichever of its two cells comes first in row order, or by the cell
  /// inside a bounded grid (`bounds`) of a side on its border, a side with
  /// one solid cell an edge, clockwise round it (only `Solid` cells are
  /// solid). A block gives the units its cells own (the sides they share
  /// with the cell right of them and the row below, and a bounded grid's
  /// border), and nothing of cells outside the bounds: so an unbounded
  /// grid's blocks must reach one block left of and above its cells, and a
  /// bounded grid's need never sit outside it. So an edit dirties the
  /// blocks holding its cell and those left of it and above it (its sides'
  /// owners), the opposite of squares, whose borders belong to the cells
  /// right of and below them: `rebuilt_borders` reads a hexagonal block's
  /// rim cells again, its first column, its last column and its last row,
  /// whose sides reach the cells round it. Points are whole pixels from
  /// the box of the block's first cell
  /// (`HexCells::corner_of`). Cells the tracer does not take
  /// (`HexCells::traceable`) give nothing. A shape cell's own polygons are
  /// not among its units: `hex_rings` gives them, as its colliders hold
  /// them
  pub(crate) fn with_hex(mut self, hex: HexCells) -> Self {
    self.hex = Some(hex);
    self
  }

  /// Pushes onto `out` the rings a hexagonal grid's (`with_hex`) cell
  /// `cell` of kind `kind` collides as, one `Vec` each: a shape cell's
  /// polygons (`ShapeBuild::rings`: each a closed ring, wound clockwise on
  /// screen, its closing edge implied, laid over its cell's box as its
  /// part's collider lays it), in pixels from the box of `first`'s
  /// (`HexCells::corner_of`: the first cell of the block it is read in).
  /// Nothing for any other cell, a one-way shape, a shape the boundary
  /// lacks, off a hexagonal grid or on one the tracer does not take
  pub(crate) fn hex_rings(
    &self,
    cell: IVec2,
    kind: TileCell,
    first: IVec2,
    out: &mut Vec<Vec<DVec2>>,
  ) {
    let (Some(hex), TileCell::Shape(shape)) = (self.hex, kind) else {
      return;
    };
    let Some((_, build)) = self.shapes.get(usize::from(shape)) else {
      return;
    };
    if !hex.traceable() {
      return;
    }
    let corner = hex.corner_of(first);
    out.extend(
      build
        .rings
        .iter()
        .map(|ring| hex_ring(hex, cell, corner, ring).collect()),
    );
  }

  /// The chains `hex_chain_edges` makes, as their points (`chain_points`:
  /// straight runs merged into one segment)
  #[cfg(test)]
  pub(crate) fn hex_chains(
    &self,
    at: impl Fn(i32, i32) -> TileCell,
    min: (i32, i32),
    max: (i32, i32),
    bounds: Option<CellRect>,
    placed: &[PlacedEdge],
  ) -> Option<Vec<Vec<Point>>> {
    let chains = self.hex_chain_edges(at, min, max, bounds, placed)?;
    Some(
      chains
        .iter()
        .map(|chain| chain_points(placed, chain))
        .collect(),
    )
  }

  /// The chains the edges of a hexagonal block (`placed`: as `placed_edges`
  /// or `rebuilt_borders` gave them for the cells from `min` to `max`, read
  /// through `at`) make, each as the places of its edges in `placed` in the
  /// order it runs them (`chain_points` gives its points): a run of edges
  /// each going on from the last one's end, clockwise round the solid cells
  /// as the edges run, and every edge in exactly one: from each edge whose
  /// edge before it is not among them, in the scan's order, then round what
  /// is left, each a closed chain ending where it starts. Three cells meet
  /// at each corner, so an edge's next one is found from its cells, with no
  /// search: along the next side of its solid cell, or where the cell
  /// across that side is solid too, along that cell's side round the same
  /// corner. Any subset of a block's edges chains: an edge whose next one
  /// is not among them ends its chain, so a caller can chain again only the
  /// edges an edit reached. None off a hexagonal grid, or on one whose
  /// cells have sides of length 0 (four cells meet at a point there), whose
  /// edges a caller chains by their points
  pub(crate) fn hex_chain_edges(
    &self,
    at: impl Fn(i32, i32) -> TileCell,
    min: (i32, i32),
    max: (i32, i32),
    bounds: Option<CellRect>,
    placed: &[PlacedEdge],
  ) -> Option<Vec<Vec<u32>>> {
    const NONE: u32 = u32::MAX;
    let hex = self.hex.filter(|hex| !hex.point_sides())?;
    let sides = HexSides::of(hex, bounds);
    let inside = |cell: IVec2| bounds.is_none_or(|bounds| bounds.contains(cell));
    let solid = |cell: IVec2| inside(cell) && at(cell.x, cell.y) == TileCell::Solid;
    // Each edge by its solid cell and that cell's side, the cell within a
    // cell of the block
    let origin = IVec2::new(min.0, min.1) - IVec2::ONE;
    let (w, h) = (max.0 - min.0 + 2, max.1 - min.1 + 2);
    if w <= 2 || h <= 2 || placed.is_empty() {
      return Some(Vec::new());
    }
    let key = |cell: IVec2, side: usize| -> Option<usize> {
      let local = cell - origin;
      ((0..w).contains(&local.x) && (0..h).contains(&local.y))
        .then(|| (local.y as usize * w as usize + local.x as usize) * 6 + side)
    };
    let mut index = vec![NONE; w as usize * h as usize * 6];
    let mut owners: Vec<(IVec2, usize)> = Vec::with_capacity(placed.len());
    for (i, &(place, ..)) in placed.iter().enumerate() {
      let (x, y, side) = (
        ((place >> 3) & 0x1FFF) as i32,
        (place >> 16) as i32,
        (place & 7) as usize,
      );
      let owner = IVec2::new(min.0 + x, min.1 + y);
      let found = if solid(owner) {
        (owner, side)
      } else {
        (sides.across(owner, side).0, (side + 3) % 6)
      };
      if let Some(slot) = key(found.0, found.1) {
        index[slot] = i as u32;
      }
      owners.push(found);
    }
    // The edge each goes on to, if the block owns it
    let next: Vec<u32> = owners
      .iter()
      .map(|&(cell, side)| {
        let (across, _) = sides.across(cell, (side + 1) % 6);
        let (cell, side) = if solid(across) {
          (across, (side + 5) % 6)
        } else {
          (cell, (side + 1) % 6)
        };
        key(cell, side).map_or(NONE, |slot| index[slot])
      })
      .collect();
    let mut follows = vec![false; placed.len()];
    for &j in &next {
      if j != NONE {
        follows[j as usize] = true;
      }
    }
    let mut used = vec![false; placed.len()];
    let mut out = Vec::new();
    let heads = (0..placed.len()).filter(|&i| !follows[i]);
    let starts: Vec<usize> = heads.chain(0..placed.len()).collect();
    for start in starts {
      if used[start] {
        continue;
      }
      used[start] = true;
      let mut chain = vec![start as u32];
      let mut at = next[start];
      while at != NONE && !used[at as usize] {
        used[at as usize] = true;
        chain.push(at);
        at = next[at as usize];
      }
      out.push(chain);
    }
    Some(out)
  }

  /// Adds a shape for `TileCell::Shape` cells to name, as
  /// `TileColliders::add_shape` does (at most 65,535; past that it logs
  /// and returns `u16::MAX`, which names none).
  pub(crate) fn add_shape(&mut self, shape: CellShape) -> u16 {
    let index = self.shapes.len();
    if index >= usize::from(u16::MAX) {
      crate::engine_warn!(
        module: "Physics",
        "a tile boundary holds at most 65,535 shapes; this one is dropped"
      );
      return u16::MAX;
    }
    let build = ShapeBuild::of(&shape);
    self.shapes.push((shape, build));
    index as u16
  }

  /// Pushes onto `out` the boundary edges the cells from `min` to `max`
  /// (exclusive) of a grid own: each one's inside, the border on its left
  /// and the border above it, and, where the block reaches a bounded grid's
  /// right or bottom (`bounds`; `None` for an unbounded grid, which has
  /// none), the borders there. So every border belongs to one block, and
  /// blocks laid side by side give the grid's boundary with nothing twice
  /// (an unbounded grid's blocks reaching one past its cells right and
  /// down). `at` gives a cell by its place in the grid (a border reads the
  /// cell across it, outside the block, and every cell outside the bounds is
  /// empty). Points are on the outline grid (`GRID` a tile) from `min`'s
  /// top-left, so they stay small however far out the block is; edges run
  /// clockwise on screen round what they bound. (Tests: tile maps read
  /// `placed_edges`.)
  #[cfg(test)]
  pub(crate) fn edges(
    &self,
    at: impl Fn(i32, i32) -> TileCell,
    min: (i32, i32),
    max: (i32, i32),
    bounds: Option<CellRect>,
    out: &mut Vec<(Point, Point)>,
  ) {
    self.scan(at, min, max, bounds, &mut Plain(out));
  }

  /// The edges `edges` gives, in the same order, each with its unit's
  /// place in the block's scan (`PlacedEdge`): what `rebuilt_borders`
  /// keeps of a block whose own cells did not change. A block is at most
  /// 16,383 cells a side; on a hexagonal grid at most 8,191 cells across
  /// (and 65,535 down), its cells past that giving no edges
  pub(crate) fn placed_edges(
    &self,
    at: impl Fn(i32, i32) -> TileCell,
    min: (i32, i32),
    max: (i32, i32),
    bounds: Option<CellRect>,
    out: &mut Vec<PlacedEdge>,
  ) {
    self.scan(at, min, max, bounds, &mut Placed::new(out));
  }

  /// The edges `placed_edges` gives of a hexagonal block's (`with_hex`)
  /// cells that `read` takes (by their place from the block's top-left),
  /// in the scan's order: the sides those cells own, worked out again
  /// after an edit reached them. Nothing off a hexagonal grid
  pub(crate) fn hex_cell_edges(
    &self,
    at: impl Fn(i32, i32) -> TileCell,
    min: (i32, i32),
    max: (i32, i32),
    bounds: Option<CellRect>,
    read: impl Fn(i32, i32) -> bool,
    out: &mut Vec<PlacedEdge>,
  ) {
    if let Some(hex) = self.hex {
      Self::hex_scan(hex, at, (min, max), bounds, read, &mut Placed::new(out));
    }
  }

  /// The edges `placed_edges` gives now for a block whose own cells have
  /// not changed since `kept` was read from it (an earlier `placed_edges`;
  /// nothing for a block whose cells are all empty), when the cells across
  /// its borders on the left of its first column and above its first row
  /// may have: those borders' units are worked out again through `at`
  /// (read only at the block's first column and row and the column and row
  /// across them), and every other unit's edges, which read the block's
  /// own cells alone, are taken from `kept`, all in the order a scan meets
  /// them. On a hexagonal grid (`with_hex`), whose cells own the sides they
  /// share with the cells right of them and below them, it is the cells
  /// across its right and bottom borders, and left of its first column
  /// below its first row, that may have changed: the units of its rim
  /// cells (its first column, its last column and its last row) are worked
  /// out again, and every other cell's taken from `kept`
  pub(crate) fn rebuilt_borders(
    &self,
    at: impl Fn(i32, i32) -> TileCell,
    min: (i32, i32),
    max: (i32, i32),
    bounds: Option<CellRect>,
    kept: &[PlacedEdge],
    out: &mut Vec<PlacedEdge>,
  ) {
    if let Some(hex) = self.hex {
      return Self::hex_rebuilt_rims(hex, at, min, max, bounds, kept, out);
    }
    let cells = Cells::shapes_only(&self.shapes);
    let local = Self::local(&at, min, bounds);
    let (w, h) = (max.0 - min.0, max.1 - min.1);
    // The border units in the scan's order: the first row's border on the
    // left and its cells' borders above, then each other row's on the left
    let mut fresh = Vec::new();
    let mut placed = Placed::new(&mut fresh);
    for y in 0..h {
      placed.unit(Unit::Left(0, y), &cells, &local);
      if y == 0 {
        for x in 0..w {
          placed.unit(Unit::Above(x, 0), &cells, &local);
        }
      }
    }
    let border = |place: u32| {
      let (y, x, kind) = (place >> 16, (place >> 2) & 0x3FFF, place & 3);
      (kind == 1 && x == 0) || (kind == 2 && y == 0)
    };
    // The places are the scan's order, and the two lists hold no unit both
    let mut fresh = fresh.into_iter().peekable();
    for &edge in kept.iter().filter(|edge| !border(edge.0)) {
      while let Some(next) = fresh.next_if(|next| next.0 < edge.0) {
        out.push(next);
      }
      out.push(edge);
    }
    out.extend(fresh);
  }

  /// `at` read from `min`'s top-left, every cell outside `bounds` empty
  fn local(
    at: &impl Fn(i32, i32) -> TileCell,
    min: (i32, i32),
    bounds: Option<CellRect>,
  ) -> impl Fn(i32, i32) -> TileCell {
    move |x: i32, y: i32| {
      let cell = IVec2::new(min.0, min.1) + IVec2::new(x, y);
      if bounds.is_some_and(|bounds| !bounds.contains(cell)) {
        TileCell::Empty
      } else {
        at(cell.x, cell.y)
      }
    }
  }

  /// The block's units in the scan's order (`edges`), each handed to `out`
  fn scan(
    &self,
    at: impl Fn(i32, i32) -> TileCell,
    min: (i32, i32),
    max: (i32, i32),
    bounds: Option<CellRect>,
    out: &mut impl UnitSink,
  ) {
    if let Some(hex) = self.hex {
      return Self::hex_scan(hex, at, (min, max), bounds, |_, _| true, out);
    }
    let cells = Cells::shapes_only(&self.shapes);
    let local = Self::local(&at, min, bounds);
    let (w, h) = (max.0 - min.0, max.1 - min.1);
    let (right, bottom) = bounds.map_or((false, false), |bounds| {
      (max.0 == bounds.max.x, max.1 == bounds.max.y)
    });
    for y in 0..h {
      for x in 0..w {
        // A cell's units read it and the cells left of it and above it:
        // with all three empty they have no edges (most of an open map)
        let empty = TileCell::Empty;
        if local(x, y) == empty && local(x - 1, y) == empty && local(x, y - 1) == empty {
          continue;
        }
        out.unit(Unit::Inside(x, y), &cells, &local);
        out.unit(Unit::Left(x, y), &cells, &local);
        out.unit(Unit::Above(x, y), &cells, &local);
      }
      if right {
        out.unit(Unit::Left(w, y), &cells, &local);
      }
    }
    if bottom {
      for x in 0..w {
        out.unit(Unit::Above(x, h), &cells, &local);
      }
    }
  }

  /// The edges of a hexagonal block (`with_hex`) whose own cells have not
  /// changed since `kept` was read from it, when the cells round it may
  /// have (`rebuilt_borders`): its rim cells' units read again (the cells
  /// whose sides reach past it: its first column, whose cells share sides
  /// with the column left of it in the row below, its last column and its
  /// last row), every other cell's taken from `kept`, merged in the scan's
  /// order
  fn hex_rebuilt_rims(
    hex: HexCells,
    at: impl Fn(i32, i32) -> TileCell,
    min: (i32, i32),
    max: (i32, i32),
    bounds: Option<CellRect>,
    kept: &[PlacedEdge],
    out: &mut Vec<PlacedEdge>,
  ) {
    let mut fresh = Vec::new();
    let (w, h) = ((max.0 - min.0).min(0x1FFF), (max.1 - min.1).min(0xFFFF));
    let on_rim = |x: i32, y: i32| x == 0 || x == w - 1 || y == h - 1;
    Self::hex_scan(
      hex,
      at,
      (min, max),
      bounds,
      on_rim,
      &mut Placed::new(&mut fresh),
    );
    let rim = |place: u32| on_rim(((place >> 3) & 0x1FFF) as i32, (place >> 16) as i32);
    // The places are the scan's order, and the two lists hold no cell both
    let mut fresh = fresh.into_iter().peekable();
    for &edge in kept.iter().filter(|edge| !rim(edge.0)) {
      while let Some(next) = fresh.next_if(|next| next.0 < edge.0) {
        out.push(next);
      }
      out.push(edge);
    }
    out.extend(fresh);
  }

  /// A hexagonal block's units in row order, each cell's sides in turn
  /// (`with_hex`), each edge handed to `out`: those of the cells `read`
  /// takes, from the block's top-left (all of them for a whole scan, its
  /// rim cells for `hex_rebuilt_rims`)
  fn hex_scan(
    hex: HexCells,
    at: impl Fn(i32, i32) -> TileCell,
    (min, max): ((i32, i32), (i32, i32)),
    bounds: Option<CellRect>,
    read: impl Fn(i32, i32) -> bool,
    out: &mut impl UnitSink,
  ) {
    if !hex.traceable() {
      return;
    }
    let sides = HexSides::of(hex, bounds);
    let inside = |cell: IVec2| bounds.is_none_or(|bounds| bounds.contains(cell));
    let solid = |cell: IVec2| inside(cell) && at(cell.x, cell.y) == TileCell::Solid;
    let from = hex.corner_of(IVec2::new(min.0, min.1));
    // The places `Placed` packs: 8,191 cells across and 65,535 down at most
    let (w, h) = ((max.0 - min.0).min(0x1FFF), (max.1 - min.1).min(0xFFFF));
    for y in min.1..min.1 + h.max(0) {
      for x in min.0..min.0 + w.max(0) {
        let cell = IVec2::new(x, y);
        if !inside(cell) || !read(x - min.0, y - min.1) {
          continue;
        }
        let mine = solid(cell);
        let mut corners = None;
        for side in 0..6 {
          // The sides it owns, those it shares with a cell outside the
          // bounds among them
          let (across, owned) = sides.across(cell, side);
          if !owned || solid(across) == mine {
            continue;
          }
          let corners = corners.get_or_insert_with(|| hex.corners(cell));
          if let Some(edge) = hex_side_edge(corners, side, mine, from) {
            out.side(x - min.0, y - min.1, side, edge);
          }
        }
      }
    }
  }
}

/// A boundary edge and its unit's place in a block's scan
/// (`Boundary::placed_edges`): from the block's top-left, by row, then
/// column, then the cell's inside, the border on its left and the border
/// above it, packed as `y << 16 | x << 2 | kind` (0, 1 and 2), so the
/// scan's order is the places' order. On a hexagonal grid, by row, column
/// and the side of the cell met, `y << 16 | x << 3 | side` (a block at most
/// 8,191 cells across)
pub(crate) type PlacedEdge = (u32, Point, Point);

/// A chain of `placed`'s edges (`Boundary::hex_chain_edges`: each going on
/// from the last one's end) as its points: its first edge's start, then
/// each edge's end, a straight run merged into one segment
pub(crate) fn chain_points(placed: &[PlacedEdge], chain: &[u32]) -> Vec<Point> {
  let mut out = Vec::with_capacity(chain.len() + 1);
  for (i, &edge) in chain.iter().enumerate() {
    let (_, a, to) = placed[edge as usize];
    if i == 0 {
      out.push(a);
      out.push(to);
      continue;
    }
    // A straight run is one segment
    let (p, q) = (out[out.len() - 2], out[out.len() - 1]);
    let (u, v) = (
      (i64::from(q.0 - p.0), i64::from(q.1 - p.1)),
      (i64::from(to.0 - q.0), i64::from(to.1 - q.1)),
    );
    if u.0 * v.1 - u.1 * v.0 == 0 && u.0 * v.0 + u.1 * v.1 > 0 {
      out.pop();
    }
    out.push(to);
  }
  out
}

/// Where a block's scan hands each unit
trait UnitSink {
  fn unit(&mut self, unit: Unit, cells: &Cells, at: &impl Fn(i32, i32) -> TileCell);
  /// A hexagonal block's edge along side `side` of its cell (`x`, `y`)
  fn side(&mut self, x: i32, y: i32, side: usize, edge: (Point, Point));
}

/// A scan's edges pushed as they come
#[cfg(test)]
struct Plain<'o>(&'o mut Vec<(Point, Point)>);

#[cfg(test)]
impl UnitSink for Plain<'_> {
  fn unit(&mut self, unit: Unit, cells: &Cells, at: &impl Fn(i32, i32) -> TileCell) {
    unit_edges(unit, cells, at, self.0);
  }

  fn side(&mut self, _: i32, _: i32, _: usize, edge: (Point, Point)) {
    self.0.push(edge);
  }
}

/// A scan's edges pushed with their units' places
struct Placed<'o> {
  out: &'o mut Vec<PlacedEdge>,
  unit: Vec<(Point, Point)>,
}

impl<'o> Placed<'o> {
  fn new(out: &'o mut Vec<PlacedEdge>) -> Self {
    Placed {
      out,
      unit: Vec::new(),
    }
  }
}

impl UnitSink for Placed<'_> {
  fn unit(&mut self, unit: Unit, cells: &Cells, at: &impl Fn(i32, i32) -> TileCell) {
    self.unit.clear();
    unit_edges(unit, cells, at, &mut self.unit);
    let (x, y, kind) = match unit {
      Unit::Inside(x, y) => (x, y, 0),
      Unit::Left(x, y) => (x, y, 1),
      Unit::Above(x, y) => (x, y, 2),
    };
    debug_assert!((0..0x4000).contains(&x) && (0..0x1_0000).contains(&y));
    let place = ((y as u32) << 16) | ((x as u32) << 2) | kind;
    self
      .out
      .extend(self.unit.iter().map(|&(a, b)| (place, a, b)));
  }

  fn side(&mut self, x: i32, y: i32, side: usize, (a, b): (Point, Point)) {
    debug_assert!((0..0x2000).contains(&x) && (0..0x1_0000).contains(&y));
    let place = ((y as u32) << 16) | ((x as u32) << 3) | side as u32;
    self.out.push((place, a, b));
  }
}

/// Marks a collider a `TileColliders` built; its parent owns it. Only its
/// parent's walk places it (`ParentPlaced`): a game's change to its
/// `Transform` is not applied until the map next moves, and the map's
/// rebuilds replace it.
#[derive(Component, Debug, Clone, Copy)]
#[require(ParentPlaced)]
pub(crate) struct TilePart;

/// A part's collider moved by `by`, its part's translation, into its
/// grid's space (tests: a packed part's points are relative to its chunk).
#[cfg(test)]
pub(crate) fn in_grid_space(mut collider: Collider, by: Vec2) -> Collider {
  use super::ColliderShape;
  match &mut collider.shape {
    ColliderShape::Polyline { points }
    | ColliderShape::Polygon { points }
    | ColliderShape::Segments { points, .. } => {
      for point in points {
        *point += by;
      }
    }
    ColliderShape::Segment { a, b } => {
      *a += by;
      *b += by;
    }
    _ => collider.offset += by,
  }
  collider
}

/// What the colliders of a map were last built from. Floats compare by
/// bits, so a NaN size does not rebuild every step.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TileRecord {
  generation: u64,
  tile_size: [u32; 2],
  lattice: Option<[u32; 6]>,
  hex: Option<(HexCells, [u32; 2])>,
  layer: u32,
  mask: u32,
  floor: Floor,
  friction: u32,
  restitution: u32,
  solid_rects: bool,
}

impl TileRecord {
  fn of(tiles: &TileColliders) -> Self {
    Self {
      generation: tiles.generation,
      tile_size: [tiles.tile_size.x.to_bits(), tiles.tile_size.y.to_bits()],
      lattice: tiles.lattice.map(|to| to.to_cols_array().map(f32::to_bits)),
      hex: tiles
        .hex
        .map(|(hex, origin)| (hex, origin.to_array().map(f32::to_bits))),
      layer: tiles.layer,
      mask: tiles.mask,
      floor: tiles.floor,
      friction: tiles.friction.to_bits(),
      restitution: tiles.restitution.to_bits(),
      solid_rects: tiles.solid_rects,
    }
  }

  fn same_settings(&self, other: &Self) -> bool {
    *self
      == Self {
        generation: self.generation,
        ..other.clone()
      }
  }
}

/// What an outline build keeps, so an edit rebuilds only what it touches.
#[derive(Debug, Default)]
struct Kept {
  outlines: Outlines,
  runs: Runs,
  /// The loops each outline part packs
  loop_packs: Packs,
  /// The one-way runs each run part packs
  run_packs: Packs,
  /// A hexagonal grid's parts' edges (it has no outlines or runs)
  hex: HexParts,
}

/// A pack's part's place, its collider (relative to it, without the grid's
/// material) and its Rapier shape built ahead (none when degenerate).
type BuiltPack = (PackKey, Vec2, Collider, Option<SharedShape>);

/// A grid's outlines and one-way runs traced and packed ahead of its build,
/// the packs' shapes built (on any thread: `TileColliders::trace`), on the
/// grid's entity: its build takes them and only spawns the parts, which
/// physics registers with the shapes as built, keeping the trace and the
/// shapes off the frame the grid arrives in.
#[derive(Component)]
pub(crate) struct Traced {
  /// The grid's generation and tile size when traced: others mean it
  /// changed since
  generation: u64,
  tile_size: Vec2,
  lattice: Option<Affine2>,
  hex: Option<(HexCells, Vec2)>,
  content: Option<(Kept, Vec<BuiltPack>)>,
}

impl std::fmt::Debug for Traced {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Traced")
      .field("generation", &self.generation)
      .field("tile_size", &self.tile_size)
      .finish_non_exhaustive()
  }
}

/// A built map's record and what its edits can rebuild in place, kept on
/// the map entity.
#[derive(Component, Debug)]
pub(crate) struct TileBuild {
  record: TileRecord,
  /// None in `solid_rects` mode and when nothing was built
  kept: Option<Kept>,
}

type Maps<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    Option<&'static mut TileColliders>,
    Option<&'static mut TileBuild>,
    Option<&'static mut Traced>,
    Has<Disabled>,
  ),
  (Or<(With<TileColliders>, With<TileBuild>)>, Allow<Disabled>),
>;
type Parts<'w, 's> = Query<'w, 's, (), (With<TilePart>, Allow<Disabled>)>;
/// Parts given a parent (spawned or moved) since the rebuild last ran.
type Moved<'w, 's> =
  Query<'w, 's, (Entity, &'static ChildOf), (With<TilePart>, Changed<ChildOf>, Allow<Disabled>)>;
type Kids<'w, 's> = Query<'w, 's, &'static Children, Allow<Disabled>>;

/// `FixedSet::Movement`: reconcile every map's colliders with its component
/// each step. A map that is disabled or lost its `TileColliders` has no
/// colliders. An enabled one whose settings differ from its record, or that
/// was never built, has all its colliders rebuilt; one whose cells changed
/// rebuilds what the changed cells touch (all of it in `solid_rects` mode,
/// or when the cells changed too often to list). A grid traced ahead
/// (`Traced`, of its generation) is built from the trace. A step with
/// nothing to rebuild costs a look at each map and at the parts given a
/// parent since.
pub(crate) fn rebuild_tile_colliders(
  mut maps: Maps,
  parts: Parts,
  moved: Moved,
  kids: Kids,
  mut unbuilt: RemovedComponents<TileBuild>,
  mut physics: ResMut<Physics>,
  mut commands: Commands,
) {
  // Maps whose parts are all cleared (and built again)
  let mut cleared = EntityHashSet::default();
  for (map, tiles, build, traced, disabled) in &mut maps {
    let Some(mut tiles) = tiles.filter(|_| !disabled) else {
      if build.is_some() {
        commands.entity(map).remove::<TileBuild>();
        cleared.insert(map);
      }
      continue;
    };
    let now = TileRecord::of(&tiles);
    if build.as_ref().is_some_and(|b| b.record == now) {
      continue;
    }
    // The change list is taken in either way (marking no change)
    let tiles = tiles.bypass_change_detection();
    let since = tiles.changes_since;
    let changes = tiles.take_changes();
    // The cells changed since the build and are all listed, the settings did not
    let applies = |build: &TileBuild| {
      changes.is_some()
        && since == build.record.generation
        && build.record.same_settings(&now)
        && build.kept.is_some()
    };
    match build {
      Some(mut build) if applies(&build) => {
        let build = &mut *build;
        if let (Some(kept), Some(changed)) = (&mut build.kept, &changes) {
          let alive = |part: Entity| parts.contains(part);
          tiles.respawn(kept, changed, &alive, &mut commands, map);
        }
        build.record = now;
      }
      build => {
        cleared.insert(map);
        // A trace of this very grid is built from; one of another
        // generation is stale. Either way it goes
        let traced = traced.and_then(|mut traced| {
          commands.entity(map).try_remove::<Traced>();
          let current = traced.generation == tiles.generation
            && traced.tile_size == tiles.tile_size
            && traced.lattice == tiles.lattice
            && traced.hex == tiles.hex;
          (current && !tiles.rects_mode())
            .then(|| traced.content.take())
            .flatten()
        });
        let kept = if !tiles.placeable() {
          let message = if tiles.hex.is_some() {
            "TileColliders needs hexagons at least 2 px across, at most 16,384 px either way, with a step of a pixel or more from line to line, and a finite origin; no colliders built"
          } else if tiles.lattice.is_some() {
            "TileColliders needs a finite lattice that does not fold its cells flat; no colliders built"
          } else {
            "TileColliders needs a positive tile size; no colliders built"
          };
          physics.warn_once(map, message);
          None
        } else if let Some((mut kept, built)) = traced {
          for (key, at, collider, shape) in built {
            tiles.spawn_pack(&mut kept, key, at, collider, shape, &mut commands, map);
          }
          Some(kept)
        } else {
          tiles.spawn_all(&mut commands, map)
        };
        let fresh = TileBuild { record: now, kept };
        match build {
          Some(mut build) => *build = fresh,
          None => {
            commands.entity(map).insert(fresh);
          }
        }
      }
    }
    // The list's allocation is kept for the next edits
    if let Some(mut list) = changes {
      list.clear();
      tiles.changes = Some(list);
    }
  }
  // Parts of cleared maps go (not the ones just spawned), and so do the
  // parts of entities that stopped being maps
  cleared.extend(unbuilt.read().filter(|&entity| !maps.contains(entity)));
  for map in cleared {
    for &part in kids.get(map).map_or(&[][..], |children| &children[..]) {
      if parts.contains(part) {
        commands.entity(part).despawn();
      }
    }
  }
  // Parts moved onto something that is not a map
  for (part, child_of) in &moved {
    if !maps.contains(child_of.parent()) {
      commands.entity(part).despawn();
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use TileCell::{Empty as E, OneWay as O, Solid as S};

  /// A cell, from the unsigned place the tests lay grids out by
  pub(super) fn xy(x: u32, y: u32) -> IVec2 {
    IVec2::new(x as i32, y as i32)
  }

  /// A loop as a set of directed edges, so tests do not depend on where it starts.
  fn edges(outline: &[Corner]) -> std::collections::BTreeSet<(Corner, Corner)> {
    outline.windows(2).map(|w| (w[0], w[1])).collect()
  }

  #[test]
  fn a_cell_rect_measures_holds_and_joins_signed_cells() {
    let r = CellRect::new(IVec2::new(-3, -2), IVec2::new(2, 1));
    assert_eq!(r.size(), UVec2::new(5, 3));
    assert!(!r.is_empty());
    assert!(r.contains(IVec2::new(-3, -2)) && r.contains(IVec2::new(1, 0)));
    assert!(!r.contains(IVec2::new(2, 0)) && !r.contains(IVec2::new(0, 1)));
    let empty = CellRect::new(IVec2::new(5, 5), IVec2::new(9, 5));
    assert!(empty.is_empty());
    assert_eq!(empty.size(), UVec2::ZERO);
    assert!(!empty.contains(IVec2::new(5, 5)));
    assert_eq!(r.union(empty), r);
    assert_eq!(empty.union(r), r);
    let far = CellRect::new(IVec2::new(10, -8), IVec2::new(11, -7));
    assert_eq!(
      r.union(far),
      CellRect::new(IVec2::new(-3, -8), IVec2::new(11, 1))
    );
    assert_eq!(
      CellRect::from_size(UVec2::new(40, 20)),
      CellRect::new(IVec2::ZERO, IVec2::new(40, 20))
    );
    assert_eq!(CellRect::default().size(), UVec2::ZERO);
    // A side past what an i32 holds stops there; the widest rect measures
    assert_eq!(
      CellRect::from_size(UVec2::splat(u32::MAX)).max,
      IVec2::splat(i32::MAX)
    );
    assert_eq!(
      CellRect::new(IVec2::MIN, IVec2::MAX).size(),
      UVec2::splat(u32::MAX)
    );
  }

  #[test]
  fn boundary_blocks_side_by_side_give_the_grid_s_boundary() {
    use SlopeCorner::{BottomLeft, TopRight};
    // A grid of every kind, an authored shape among them, read in blocks
    // of 4 that do not divide it
    let (width, height) = (11u32, 9u32);
    let mut tiles = TileColliders::new(width, height, Vec2::ONE);
    let half = tiles.add_shape(CellShape {
      polygons: vec![vec![
        Vec2::new(0.0, 0.5),
        Vec2::new(1.0, 0.5),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
      ]],
      one_way: false,
    });
    let mut boundary = Boundary::default();
    let same = boundary.add_shape(tiles.shape(half).cloned().unwrap_or_default());
    assert_eq!(same, half);
    let kinds = [
      S,
      S,
      E,
      O,
      TileCell::Slope(BottomLeft),
      TileCell::Shape(half),
      S,
      TileCell::Slope(TopRight),
      E,
    ];
    let mut seed = 0x2545_f491_u32;
    for y in 0..height {
      for x in 0..width {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        tiles.set(xy(x, y), kinds[seed as usize % kinds.len()]);
      }
    }
    let mut whole = outline::boundary_edges(tiles.cells_view());
    let at = |x: i32, y: i32| tiles.get(IVec2::new(x, y));
    let mut pieced = Vec::new();
    let size = (width as i32, height as i32);
    let bounds = Some(CellRect::from_size(UVec2::new(width, height)));
    for by in (0..size.1).step_by(4) {
      for bx in (0..size.0).step_by(4) {
        let max = ((bx + 4).min(size.0), (by + 4).min(size.1));
        let mut block = Vec::new();
        boundary.edges(at, (bx, by), max, bounds, &mut block);
        let origin = (bx * GRID, by * GRID);
        pieced.extend(
          block
            .into_iter()
            .map(|(a, b)| (sides::plus(origin, a), sides::plus(origin, b))),
        );
      }
    }
    whole.sort_unstable();
    pieced.sort_unstable();
    assert!(!whole.is_empty());
    assert_eq!(pieced, whole);
  }

  #[test]
  fn an_unbounded_grid_s_blocks_give_its_boundary_at_negative_cells() {
    // Blocks of 4 cells from left of and above the cells to one block past
    // them right and down: an unbounded grid's blocks own no right or
    // bottom border, the block across it does
    let mut tiles = TileColliders::unbounded(Vec2::ONE);
    for (x, y) in [(-5, -5), (-4, -5), (-1, -1), (0, 0), (3, -2), (-6, 2)] {
      tiles.set(IVec2::new(x, y), S);
    }
    tiles.set(IVec2::new(-2, 1), TileCell::Slope(SlopeCorner::TopRight));
    tiles.set(IVec2::new(-1, 1), O);
    let mut whole = outline::boundary_edges(tiles.cells_view());
    let boundary = Boundary::default();
    let at = |x: i32, y: i32| tiles.get(IVec2::new(x, y));
    let mut pieced = Vec::new();
    for by in (-8..8).step_by(4) {
      for bx in (-8..8).step_by(4) {
        let mut block = Vec::new();
        boundary.edges(at, (bx, by), (bx + 4, by + 4), None, &mut block);
        let origin = (bx * GRID, by * GRID);
        pieced.extend(
          block
            .into_iter()
            .map(|(a, b)| (sides::plus(origin, a), sides::plus(origin, b))),
        );
      }
    }
    whole.sort_unstable();
    pieced.sort_unstable();
    assert!(!whole.is_empty());
    assert_eq!(pieced, whole);
  }

  #[test]
  fn a_block_s_borders_rebuilt_alone_give_its_edges_in_the_scan_s_order() {
    use SlopeCorner::{BottomLeft, TopRight};
    // Blocks of a grid of every kind and shapes, the cells left of and
    // above each changed at random: its borders rebuilt from what it kept
    // give its edges, places and order as a scan of it now gives them. A
    // bounded grid's blocks that reach its right or bottom keep those
    // borders; an empty block keeps nothing
    let (width, height) = (13u32, 11u32);
    let mut tiles = TileColliders::new(width, height, Vec2::ONE);
    let mut boundary = Boundary::default();
    let mut kinds = vec![S, S, S, E, E, E, O, TileCell::Slope(BottomLeft)];
    kinds.push(TileCell::Slope(TopRight));
    for polygon in [
      vec![(0.0, 0.5), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)],
      vec![(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)],
      vec![
        (0.0, 0.0),
        (0.5, 0.0),
        (0.5, 0.5),
        (1.0, 0.5),
        (1.0, 1.0),
        (0.0, 1.0),
      ],
      vec![(0.25, 0.0), (0.75, 0.0), (0.75, 1.0), (0.25, 1.0)],
    ] {
      let shape = CellShape {
        polygons: vec![polygon.into_iter().map(|(x, y)| Vec2::new(x, y)).collect()],
        one_way: false,
      };
      let index = tiles.add_shape(shape.clone());
      assert_eq!(boundary.add_shape(shape), index);
      kinds.push(TileCell::Shape(index));
    }
    let mut seed = 0x9e37_79b9_u32;
    let mut next = |n: usize| {
      seed ^= seed << 13;
      seed ^= seed >> 17;
      seed ^= seed << 5;
      seed as usize % n
    };
    for y in 0..height {
      for x in 0..width {
        tiles.set(xy(x, y), kinds[next(kinds.len())]);
      }
    }
    let bounds = Some(CellRect::from_size(UVec2::new(width, height)));
    let (mut rebuilt, mut scanned) = (0, 0);
    for round in 0..400 {
      // A block of 2 to 6 cells a side within the grid, or past its right
      // and bottom
      let (w, h) = (2 + next(5) as i32, 2 + next(5) as i32);
      let min = (
        1 + next(width as usize - 1) as i32,
        1 + next(height as usize - 1) as i32,
      );
      let max = (
        (min.0 + w).min(width as i32),
        (min.1 + h).min(height as i32),
      );
      // Every fourth block emptied first
      if round % 4 == 0 {
        for y in min.1..max.1 {
          for x in min.0..max.0 {
            tiles.set(IVec2::new(x, y), E);
          }
        }
      }
      let mut kept = Vec::new();
      let at = |x: i32, y: i32| tiles.get(IVec2::new(x, y));
      boundary.placed_edges(at, min, max, bounds, &mut kept);
      // The column left of it and the row above it, changed
      for y in min.1 - 1..max.1 {
        if next(2) == 0 {
          tiles.set(IVec2::new(min.0 - 1, y), kinds[next(kinds.len())]);
        }
      }
      for x in min.0..max.0 {
        if next(2) == 0 {
          tiles.set(IVec2::new(x, min.1 - 1), kinds[next(kinds.len())]);
        }
      }
      let at = |x: i32, y: i32| tiles.get(IVec2::new(x, y));
      let (mut now, mut borders) = (Vec::new(), Vec::new());
      boundary.placed_edges(at, min, max, bounds, &mut now);
      boundary.rebuilt_borders(at, min, max, bounds, &kept, &mut borders);
      assert_eq!(borders, now, "round {round}: block {min:?} to {max:?}");
      let mut edges = Vec::new();
      boundary.edges(at, min, max, bounds, &mut edges);
      let placed: Vec<(Point, Point)> = now.iter().map(|&(_, a, b)| (a, b)).collect();
      assert_eq!(placed, edges, "placed edges are the scan's edges");
      rebuilt += usize::from(borders != kept);
      scanned += now.len();
    }
    assert!(rebuilt > 200 && scanned > 3000, "{rebuilt} {scanned}");
  }

  #[test]
  fn a_chunk_set_whole_changes_and_lists_what_setting_each_cell_would() {
    // Random chunks written whole into one grid and cell by cell into its
    // twin: a bounded grid whose bounds cut chunks, and an unbounded one,
    // chunks there and not, empty ones among them
    let mut seed = 0x0bad_5eed_u32;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    let kinds = [E, E, E, S, O, TileCell::Slope(SlopeCorner::TopLeft)];
    for bounded in [true, false] {
      let fresh = || {
        if bounded {
          TileColliders::new(70, 45, Vec2::ONE)
        } else {
          TileColliders::unbounded(Vec2::ONE)
        }
      };
      let (mut whole, mut each) = (fresh(), fresh());
      for round in 0..200 {
        let chunk = IVec2::new(next(5) as i32 - 2, next(4) as i32 - 2);
        let mut cells = [E; CELLS];
        if next(4) > 0 {
          for cell in &mut cells {
            *cell = kinds[next(kinds.len() as u32) as usize];
          }
        }
        let wrote = whole.set_chunk(chunk, &cells);
        let mut set = false;
        for (i, &kind) in cells.iter().enumerate() {
          let cell = chunk * CHUNK + IVec2::new(i as i32 % CHUNK, i as i32 / CHUNK);
          set |= each.set(cell, kind);
        }
        assert_eq!(wrote, set, "round {round}: chunk {chunk}");
        assert_eq!(whole.take_changes(), each.take_changes(), "round {round}");
        assert_eq!(whole.chunks.len(), each.chunks.len(), "round {round}");
        assert_eq!(whole.extent(), each.extent(), "round {round}");
        for (at, cells) in &each.chunks {
          assert_eq!(
            whole.chunks.get(at),
            Some(cells),
            "round {round}: chunk {at}"
          );
        }
      }
    }
  }

  #[test]
  fn a_grid_made_from_chunks_holds_what_set_would_put_there() {
    let mut cells = [E; CELLS];
    cells[0] = S;
    cells[CELLS - 1] = O;
    // Bounded: a chunk reaching past the bounds keeps what is inside
    let mut bounded = TileColliders::new(40, 40, Vec2::ONE);
    bounded.put_chunk(IVec2::new(1, 1), &cells);
    bounded.put_chunk(IVec2::new(5, 5), &cells);
    assert_eq!(bounded.get(IVec2::new(32, 32)), S);
    assert_eq!(bounded.get(IVec2::new(63, 63)), E, "outside the bounds");
    assert!(bounded.chunk_cells(IVec2::new(5, 5)).is_none());
    // Unbounded: any chunk, but a cell past the reach is left out
    let mut free = TileColliders::unbounded(Vec2::ONE);
    free.put_chunk(IVec2::new(-3, -7), &cells);
    assert_eq!(free.get(IVec2::new(-96, -224)), S);
    assert_eq!(free.get(IVec2::new(-65, -193)), O);
    let far = IVec2::splat(MAX_REACH).div_euclid(IVec2::splat(CHUNK));
    free.put_chunk(far, &cells);
    assert_eq!(free.get(far * CHUNK), S, "within the reach");
    assert_eq!(free.get(far * CHUNK + IVec2::splat(CHUNK - 1)), E);
    // Nothing but empty cells makes no chunk
    free.put_chunk(IVec2::new(9, 9), &[E; CELLS]);
    assert!(free.chunk_cells(IVec2::new(9, 9)).is_none());
  }

  #[test]
  fn an_unloaded_chunk_s_cells_leave_as_changes_and_its_storage_goes() {
    let mut grid = TileColliders::unbounded(Vec2::splat(16.0));
    for cell in [IVec2::new(-3, 2), IVec2::new(-1, 31), IVec2::new(40, 2)] {
      assert!(grid.set(cell, S));
    }
    assert!(grid.set(IVec2::new(-20, 4), O));
    let _ = grid.take_changes();
    let generation = grid.generation;
    grid.unload_chunk(IVec2::new(-1, 0));
    assert!(grid.generation > generation, "a change");
    assert!(grid.chunk_cells(IVec2::new(-1, 0)).is_none());
    for cell in [IVec2::new(-3, 2), IVec2::new(-1, 31), IVec2::new(-20, 4)] {
      assert_eq!(grid.get(cell), E, "{cell}");
    }
    assert_eq!(grid.get(IVec2::new(40, 2)), S);
    // Each cell it held, with what it held, in row order, as `set` lists them
    assert_eq!(
      grid.take_changes(),
      Some(vec![
        (IVec2::new(-3, 2), S),
        (IVec2::new(-20, 4), O),
        (IVec2::new(-1, 31), S),
      ])
    );
    // The extent is the chunks left
    assert_eq!(
      grid.extent(),
      CellRect::new(IVec2::new(32, 0), IVec2::new(64, 32))
    );
    // A chunk it lacks changes nothing
    let generation = grid.generation;
    grid.unload_chunk(IVec2::new(-1, 0));
    grid.unload_chunk(IVec2::new(7, 7));
    assert_eq!(grid.generation, generation);
    assert_eq!(grid.take_changes(), Some(Vec::new()));
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

  /// The merged rectangles of a grid `width` by `height` from its cells
  /// row by row
  pub(super) fn rects(width: u32, height: u32, cells: &[TileCell]) -> Vec<(Corner, Corner)> {
    let grid = TileColliders::from_cells(width, height, Vec2::ONE, cells.to_vec());
    solid_rects(grid.cells_view())
  }

  #[test]
  fn solid_cells_merge_into_few_rectangles() {
    assert_eq!(rects(2, 2, &[S, S, S, S]), [((0, 0), (2, 2))]);
    assert_eq!(rects(3, 1, &[S, E, S]).len(), 2);
    // Signed cells, across chunk borders
    let mut grid = TileColliders::unbounded(Vec2::ONE);
    for x in -34..-30 {
      for y in [-1, 0] {
        grid.set(IVec2::new(x, y), S);
      }
    }
    assert_eq!(solid_rects(grid.cells_view()), [((-34, -1), (4, 2))]);
  }

  #[test]
  fn a_short_cell_list_counts_as_empty() {
    assert!(outlines(4, 4, &[S]).len() == 1);
    assert!(outlines(0, 0, &[]).is_empty());
  }

  #[test]
  fn a_grid_holds_and_traces_cells_far_from_the_origin() {
    // Sides past 6a's 8192 hold their cells to the far corner
    for (w, h) in [(8193, 1), (1, 8193), (100_000, 1)] {
      let mut grid = TileColliders::new(w, h, Vec2::ONE);
      assert_eq!(grid.bounds(), Some(CellRect::from_size(UVec2::new(w, h))));
      assert_eq!(grid.extent(), CellRect::from_size(UVec2::new(w, h)));
      assert!(grid.set(xy(w - 1, h - 1), S), "{w} x {h}");
      assert!(!grid.set(xy(w, h - 1), S), "outside {w} x {h}");
      assert!(!grid.set(IVec2::new(-1, 0), S), "outside {w} x {h}");
      let loops = traced_outlines(grid.cells_view());
      assert_eq!(loops.len(), 1);
      assert!(loops[0].contains(&(w as i32, h as i32)), "{loops:?}");
    }
    // An unbounded grid holds signed cells, each in its chunk
    let mut grid = TileColliders::unbounded(Vec2::ONE);
    assert_eq!((grid.bounds(), grid.extent()), (None, CellRect::default()));
    let cells = [(-1, -1), (-33, 0), (31, -32), (-100_000, 50_000)].map(|(x, y)| IVec2::new(x, y));
    for cell in cells {
      assert!(grid.set(cell, S));
      assert_eq!(grid.get(cell), S);
    }
    assert_eq!(grid.get(IVec2::new(-2, -1)), E);
    assert_eq!(
      grid.extent(),
      CellRect::new(IVec2::new(-100_000, -32), IVec2::new(32, 50_016))
    );
    let loops = traced_outlines(grid.cells_view());
    assert_eq!(loops.len(), 4);
    let square = |x: i32, y: i32| edges(&[(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1), (x, y)]);
    for cell in cells {
      assert!(
        loops.iter().any(|l| edges(l) == square(cell.x, cell.y)),
        "{cell}: {loops:?}"
      );
    }
  }

  /// The boundary edges a scan of every cell from `min` up to `max` (and
  /// the borders right of and below them) meets, in its order
  fn scanned(cells: Cells, min: IVec2, max: IVec2) -> Vec<(Point, Point)> {
    let mut edges = Vec::new();
    let at = |x: i32, y: i32| cells.at(x, y);
    for y in min.y..=max.y {
      for x in min.x..=max.x {
        if x < max.x && y < max.y {
          unit_edges(Unit::Inside(x, y), &cells, &at, &mut edges);
        }
        if y < max.y {
          unit_edges(Unit::Left(x, y), &cells, &at, &mut edges);
        }
        if x < max.x {
          unit_edges(Unit::Above(x, y), &cells, &at, &mut edges);
        }
      }
    }
    edges
  }

  #[test]
  fn boundary_edges_come_in_the_order_a_scan_of_every_cell_meets_them() {
    // The order decides where each loop starts, so its pack: a bounded grid
    // traces as 6a's dense scan did, and an unbounded one in the same order
    let mut bounded = TileColliders::new(100, 70, Vec2::ONE);
    let mut unbounded = TileColliders::unbounded(Vec2::ONE);
    let shape = CellShape {
      polygons: vec![vec![
        Vec2::new(0.0, 0.5),
        Vec2::new(1.0, 0.5),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
      ]],
      one_way: false,
    };
    let half = bounded.add_shape(shape.clone());
    assert_eq!(unbounded.add_shape(shape), half);
    let kinds = [
      S,
      S,
      E,
      E,
      O,
      TileCell::Slope(SlopeCorner::TopLeft),
      TileCell::Shape(half),
    ];
    let mut seed = 0x1234_5678_u32;
    let mut next = |n: u32| {
      seed ^= seed << 13;
      seed ^= seed >> 17;
      seed ^= seed << 5;
      seed % n
    };
    for _ in 0..3000 {
      let kind = kinds[next(kinds.len() as u32) as usize];
      bounded.set(xy(next(100), next(70)), kind);
      // Chunks left out in a checkerboard and along row 0, so units border
      // chunks that are not there on every side
      let cell = IVec2::new(next(192) as i32 - 96, next(160) as i32 - 64);
      let chunk = chunk_of(cell);
      if (chunk.x + chunk.y) % 2 != 0 && chunk.y != 0 {
        unbounded.set(cell, kind);
      }
    }
    let view = bounded.cells_view();
    assert_eq!(
      outline::boundary_edges(view),
      scanned(view, IVec2::ZERO, IVec2::new(100, 70))
    );
    let (view, extent) = (unbounded.cells_view(), unbounded.extent());
    assert!(extent.min.cmplt(IVec2::splat(-32)).all(), "{extent:?}");
    let edges = outline::boundary_edges(view);
    assert!(!edges.is_empty());
    assert_eq!(edges, scanned(view, extent.min, extent.max));
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

  use hex::{HexCells, StaggerAxis, StaggerIndex};
  use pack::{HEX_PART_EDGES, HexBox, HexParts};
  use std::collections::BTreeSet;

  /// The hexagonal grids the tracer is held to: every axis and index, at
  /// the cell sizes and sides that matter, sides of length 0 left out
  pub(super) fn hex_variants() -> Vec<HexCells> {
    let mut out = Vec::new();
    for ((w, h), side) in [
      ((16, 16), 8),
      ((120, 140), 70),
      ((15, 17), 7),
      ((16, 16), 24),
    ] {
      for axis in [StaggerAxis::X, StaggerAxis::Y] {
        for index in [StaggerIndex::Odd, StaggerIndex::Even] {
          out.push(HexCells::new(axis, index, UVec2::new(w, h), side));
        }
      }
    }
    out
  }

  /// A hexagon's corner as an outline point
  pub(super) fn hex_point(corner: glam::I64Vec2) -> Point {
    (corner.x as i32, corner.y as i32)
  }

  /// The sides of `solid` cells whose cell across is not solid, clockwise
  /// round the solid one, sides of length 0 left out, sorted: worked out
  /// from `HexCells` alone
  pub(super) fn sides_with_one_solid(
    hex: &HexCells,
    solid: &BTreeSet<(i32, i32)>,
  ) -> Vec<(Point, Point)> {
    let mut edges = Vec::new();
    for &(x, y) in solid {
      let cell = IVec2::new(x, y);
      let corners = hex.corners(cell);
      for side in 0..6 {
        let across = hex.across(cell, side);
        if solid.contains(&(across.x, across.y)) {
          continue;
        }
        let (a, b) = (corners[side], corners[(side + 1) % 6]);
        if a != b {
          edges.push((hex_point(a), hex_point(b)));
        }
      }
    }
    edges.sort_unstable();
    edges
  }

  /// Each part of a hexagonal grid's trace, with its edges in the grid's
  /// points (whole pixels from its origin)
  pub(super) fn hex_parts_of(tiles: &TileColliders) -> Vec<(PackKey, Vec<(Point, Point)>)> {
    let origin = tiles.hex().map_or(Vec2::ZERO, |(_, origin)| origin);
    let traced = tiles.trace().expect("a hexagonal grid traces its parts");
    let (_, built) = traced.content.expect("traced");
    built
      .into_iter()
      .map(|(key, at, collider, _)| (key, segments_of(&collider, at - origin)))
      .collect()
  }

  /// A part's segments moved by `by`, as outline points, each checked of
  /// some length
  pub(super) fn segments_of(collider: &Collider, by: Vec2) -> Vec<(Point, Point)> {
    let super::super::ColliderShape::Segments { points, segments } = &collider.shape else {
      panic!("a hexagonal part is segments: {collider:?}");
    };
    let point = |i: u32| {
      let p = points[i as usize] + by;
      (p.x.round() as i32, p.y.round() as i32)
    };
    segments
      .iter()
      .map(|&[a, b]| {
        let edge = (point(a), point(b));
        assert_ne!(edge.0, edge.1, "no edge of length 0");
        edge
      })
      .collect()
  }

  /// Every edge of a hexagonal grid's parts, sorted
  pub(super) fn hex_part_edges(tiles: &TileColliders) -> Vec<(Point, Point)> {
    let mut edges: Vec<(Point, Point)> = hex_parts_of(tiles)
      .into_iter()
      .flat_map(|(_, edges)| edges)
      .collect();
    edges.sort_unstable();
    edges
  }

  /// Every edge `parts` holds, sorted
  fn held_edges(parts: &HexParts) -> Vec<(Point, Point)> {
    let mut edges: Vec<(Point, Point)> = parts
      .keys()
      .into_iter()
      .flat_map(|key| parts.edges(key))
      .collect();
    edges.sort_unstable();
    edges
  }

  /// Every shape cell with rings `parts` holds, with its shape, sorted
  fn held_rings(parts: &HexParts) -> Vec<(i32, i32, u16)> {
    let mut rings: Vec<(i32, i32, u16)> = parts
      .keys()
      .into_iter()
      .flat_map(|key| parts.rings(key))
      .map(|(cell, shape)| (cell.x, cell.y, shape))
      .collect();
    rings.sort_unstable();
    rings
  }

  #[test]
  fn a_hex_cell_s_part_holds_its_six_sides() {
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::new(5.0, -3.0));
    assert!(tiles.set(IVec2::new(3, -2), S));
    // Row -2 is not shifted: the cell's box is 16 px from x 48, its rows
    // step 12 px and its slants 4 px. It and the cells left of it and above
    // it, which own its other sides, are all in chunk (0, -1)
    let corners = [
      (56, -24),
      (64, -20),
      (64, -12),
      (56, -8),
      (48, -12),
      (48, -20),
    ];
    let mut sides: Vec<(Point, Point)> =
      (0..6).map(|i| (corners[i], corners[(i + 1) % 6])).collect();
    sides.sort_unstable();
    let parts = hex_parts_of(&tiles);
    assert_eq!(parts.len(), 1, "{parts:?}");
    let (key, mut edges) = parts[0].clone();
    assert_eq!(
      key,
      PackKey::Hex {
        chunk: (0, -1),
        part: HexBox::Chunk,
        rings: false
      }
    );
    edges.sort_unstable();
    assert_eq!(edges, sides, "clockwise round it");
  }

  #[test]
  fn hex_parts_hold_exactly_the_sides_with_one_solid_cell() {
    let mut seed = 0x6a09_e667_u32;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    for hex in hex_variants() {
      // Sparse sets over four chunks and more, then clumps, with cells
      // that collide as nothing on a hex grid among them
      for round in 0..250 {
        let (count, reach) = if round < 200 { (30, 40) } else { (60, 6) };
        let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
        let mut solid = BTreeSet::new();
        for _ in 0..count {
          let at = (
            next(2 * reach) as i32 - reach as i32,
            next(2 * reach) as i32 - reach as i32,
          );
          let kind = [S, S, S, S, O, TileCell::Slope(SlopeCorner::TopLeft)][next(6) as usize];
          tiles.set(IVec2::new(at.0, at.1), kind);
          if kind == S {
            solid.insert(at);
          } else {
            solid.remove(&at);
          }
        }
        assert_eq!(
          hex_part_edges(&tiles),
          sides_with_one_solid(&hex, &solid),
          "{hex:?} round {round}"
        );
      }
    }
  }

  #[test]
  fn a_bounded_full_hex_grid_s_edges_are_its_border() {
    for hex in hex_variants() {
      let mut tiles = TileColliders::new(7, 5, Vec2::ONE).with_hex(hex, Vec2::ZERO);
      let mut solid = BTreeSet::new();
      for y in 0..5 {
        for x in 0..7 {
          assert!(tiles.set(xy(x, y), S));
          solid.insert((x as i32, y as i32));
        }
      }
      assert_eq!(
        hex_part_edges(&tiles),
        sides_with_one_solid(&hex, &solid),
        "{hex:?}"
      );
    }
  }

  #[test]
  fn degenerate_hex_grids_give_closed_outlines_without_zero_length_edges() {
    let each = |cell: (u32, u32), side: u32| {
      [StaggerAxis::X, StaggerAxis::Y]
        .into_iter()
        .flat_map(move |axis| {
          [StaggerIndex::Odd, StaggerIndex::Even]
            .map(|index| HexCells::new(axis, index, UVec2::new(cell.0, cell.1), side))
        })
    };
    // The parts' edges, held to the sides with one solid cell (and to no
    // edge of length 0), and as many leaving each point as reaching it
    let edges = |hex: HexCells, cells: &[(i32, i32)]| {
      let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
      for &(x, y) in cells {
        tiles.set(IVec2::new(x, y), S);
      }
      let edges = hex_part_edges(&tiles);
      let solid = cells.iter().copied().collect();
      assert_eq!(
        edges,
        sides_with_one_solid(&hex, &solid),
        "{hex:?} {cells:?}"
      );
      let mut ends: std::collections::BTreeMap<Point, i32> = Default::default();
      for &(a, b) in &edges {
        *ends.entry(a).or_default() += 1;
        *ends.entry(b).or_default() -= 1;
      }
      assert!(ends.values().all(|&n| n == 0), "closed: {hex:?} {cells:?}");
      edges
    };
    // Side 0: diamonds, two meeting only at the corner a side of length 0
    // shrinks to: eight edges, four leaving that corner and reaching it
    for hex in each((16, 16), 0) {
      let cell = IVec2::new(2, -3);
      let corners = hex.corners(cell);
      let flat = (0..6)
        .find(|&s| corners[s] == corners[(s + 1) % 6])
        .expect("a side of length 0");
      let across = hex.across(cell, flat);
      let found = edges(hex, &[(cell.x, cell.y), (across.x, across.y)]);
      assert_eq!(found.len(), 8, "{hex:?}");
      let pinch = hex_point(corners[flat]);
      assert_eq!(
        found
          .iter()
          .filter(|&&(a, b)| a == pinch || b == pinch)
          .count(),
        4,
        "{hex:?}"
      );
    }
    // A side longer than the cell: rectangles in brick rows
    for hex in each((16, 16), 24) {
      let found = edges(hex, &[(5, -7)]);
      assert!(
        found.iter().all(|&(a, b)| a.0 == b.0 || a.1 == b.1),
        "{hex:?}"
      );
      edges(hex, &[(0, 0), (1, 0), (0, 1), (1, 1), (-1, 1)]);
    }
    // Odd sizes: a region's outline closes
    for hex in each((15, 17), 7) {
      let block: Vec<(i32, i32)> = (-1..3).flat_map(|y| (-2..2).map(move |x| (x, y))).collect();
      edges(hex, &block);
    }
  }

  #[test]
  fn a_sparse_hex_chunk_is_one_part_and_a_dense_one_splits() {
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    // Four pairs of cells well inside chunk (0, 0): 40 edges, one part
    let mut sparse = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
    for y in [4, 10, 16, 22] {
      for x in [6, 7] {
        sparse.set(IVec2::new(x, y), S);
      }
    }
    let parts = hex_parts_of(&sparse);
    assert_eq!(parts.len(), 1);
    assert_eq!(
      parts[0].0,
      PackKey::Hex {
        chunk: (0, 0),
        part: HexBox::Chunk,
        rings: false
      }
    );
    assert_eq!(parts[0].1.len(), 40);
    // A checkerboard over the chunk, its owners all in it: each quarter
    // holds about 450 edges, past the budget, so a part for each of the 16
    // blocks, each holding its own
    let mut dense = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
    let mut solid = BTreeSet::new();
    for y in 1..31 {
      for x in 1..31 {
        if (x + y) % 2 == 0 {
          dense.set(IVec2::new(x, y), S);
          solid.insert((x, y));
        }
      }
    }
    let parts = hex_parts_of(&dense);
    assert_eq!(parts.len(), 16, "every block holds an edge");
    let mut quarters = [0; 4];
    for (key, edges) in &parts {
      let PackKey::Hex {
        chunk: (0, 0),
        part: HexBox::Block(block),
        rings: false,
      } = *key
      else {
        panic!("a block of chunk (0, 0): {key:?}");
      };
      assert!(!edges.is_empty() && block < 16);
      quarters[usize::from(block % 4 / 2 + block / 8 * 2)] += edges.len();
    }
    assert!(quarters.iter().all(|&n| n > HEX_PART_EDGES), "{quarters:?}");
    assert_eq!(hex_part_edges(&dense), sides_with_one_solid(&hex, &solid));
  }

  /// Cells of 16 px hexes (axis Y, odd rows) giving the box of chunk (0, 0)
  /// from column `left`, `rows` rows tall, exactly `edges` edges of its
  /// own: isolated cells on even rows and columns (from 2 past `left` to 14
  /// past it, rows from 2), with six sides each, and up to five cells on
  /// odd rows of column `left`, each with five, the sixth (its side to the
  /// left) owned by the cell left of it
  fn cells_giving(left: i32, rows: i32, edges: usize) -> Vec<IVec2> {
    let fives = (0..6)
      .find(|b| (edges - 5 * b).is_multiple_of(6))
      .expect("a count");
    let sixes = (edges - 5 * fives) / 6;
    let mut cells: Vec<IVec2> = (0..fives as i32)
      .map(|i| IVec2::new(left, 1 + 2 * i))
      .collect();
    let spots =
      (0..rows / 2 - 1).flat_map(|j| (1..8).map(move |i| IVec2::new(left + 2 * i, 2 + 2 * j)));
    cells.extend(spots.take(sixes));
    assert_eq!(cells.len(), fives + sixes, "room for {edges} edges");
    cells
  }

  #[test]
  fn a_hex_part_at_its_budget_stays_whole_and_one_edge_past_it_splits() {
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let budget = HEX_PART_EDGES;
    let grid = |cells: &[IVec2]| {
      let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
      for &cell in cells {
        assert!(tiles.set(cell, S), "{cell}");
      }
      tiles
    };
    let keys = |tiles: &TileColliders| {
      let mut parts: Vec<(PackKey, usize)> = hex_parts_of(tiles)
        .into_iter()
        .map(|(key, edges)| (key, edges.len()))
        .collect();
      parts.sort_unstable();
      parts
    };
    let at = |chunk: (i32, i32), part: HexBox| PackKey::Hex {
      chunk,
      part,
      rings: false,
    };
    // A chunk at exactly the budget is one part; one edge more splits it
    // into quarters. The side each first-column cell gives the chunk left
    // of it is that chunk's own part
    let column = |n: usize| {
      let fives = (0..6).find(|b| (n - 5 * b).is_multiple_of(6)).unwrap_or(0);
      (fives > 0).then(|| (at((-1, 0), HexBox::Chunk), fives))
    };
    for (edges, split) in [(budget, false), (budget + 1, true)] {
      let tiles = grid(&cells_giving(0, 32, edges));
      let mut want: Vec<(PackKey, usize)> = column(edges).into_iter().collect();
      if split {
        assert!(
          keys(&tiles)
            .iter()
            .all(|&(key, _)| key != at((0, 0), HexBox::Chunk))
        );
        let held: usize = keys(&tiles)
          .iter()
          .filter(|(key, _)| matches!(key, PackKey::Hex { chunk: (0, 0), .. }))
          .map(|&(_, n)| n)
          .sum();
        assert_eq!(held, edges);
      } else {
        want.push((at((0, 0), HexBox::Chunk), edges));
        want.sort_unstable();
        assert_eq!(keys(&tiles), want);
      }
    }
    // In a split chunk, a quarter at exactly the budget is one part, and
    // one edge more splits it into its blocks
    let right = cells_giving(16, 16, budget + 1);
    let spill = right.iter().filter(|cell| cell.x == 16).count();
    let mut cells = cells_giving(0, 16, budget - spill);
    cells.extend(right);
    let found = keys(&grid(&cells));
    let quarter = found
      .iter()
      .find(|(key, _)| *key == at((0, 0), HexBox::Quarter(0)))
      .map(|&(_, n)| n);
    assert_eq!(quarter, Some(budget), "{found:?}");
    let blocks: usize = found
      .iter()
      .filter(|(key, _)| matches!(key, PackKey::Hex { chunk: (0, 0), part: HexBox::Block(b), .. } if [2, 3, 6, 7].contains(b)))
      .map(|&(_, n)| n)
      .sum();
    assert_eq!(blocks, budget + 1, "{found:?}");
    assert!(
      !found
        .iter()
        .any(|(key, _)| *key == at((0, 0), HexBox::Quarter(1)))
    );
    // And by an edit: a cell giving five sides takes a chunk from four
    // under the budget to one over (it splits), or from five under to it
    for (from, split) in [(budget - 4, true), (budget - 5, false)] {
      let mut tiles = grid(&cells_giving(0, 32, from));
      let mut parts = HexParts::build(tiles.cells_view(), hex, tiles.bounds());
      tiles.take_changes();
      // On column 0, row 31: its side to the left is the chunk left's
      assert!(tiles.set(IVec2::new(0, 31), S));
      let changed = tiles.take_changes().expect("listed");
      parts.apply(tiles.cells_view(), hex, tiles.bounds(), &changed);
      let whole = parts.keys().contains(&at((0, 0), HexBox::Chunk));
      assert_eq!(whole, !split, "from {from}");
      let held: usize = parts
        .keys()
        .into_iter()
        .filter(|key| matches!(key, PackKey::Hex { chunk: (0, 0), .. }))
        .map(|key| parts.edges(key).count())
        .sum();
      assert_eq!(held, from + 5);
      assert_eq!(
        held_edges(&parts),
        held_edges(&HexParts::build(tiles.cells_view(), hex, tiles.bounds()))
      );
    }
  }

  /// Each part of a hexagonal grid's trace, with its edges in the grid's
  /// space from its origin, as they are (a shape's ring off whole pixels)
  fn hex_part_lines(tiles: &TileColliders) -> Vec<(PackKey, Vec<(Vec2, Vec2)>)> {
    let origin = tiles.hex().map_or(Vec2::ZERO, |(_, origin)| origin);
    let traced = tiles.trace().expect("a hexagonal grid traces its parts");
    let (_, built) = traced.content.expect("traced");
    built
      .into_iter()
      .map(|(key, at, collider, _)| (key, lines_of(&collider, at - origin)))
      .collect()
  }

  /// A part's segments moved by `by`
  fn lines_of(collider: &Collider, by: Vec2) -> Vec<(Vec2, Vec2)> {
    let super::super::ColliderShape::Segments { points, segments } = &collider.shape else {
      panic!("a hexagonal part is segments: {collider:?}");
    };
    segments
      .iter()
      .map(|&[a, b]| (points[a as usize] + by, points[b as usize] + by))
      .collect()
  }

  /// A convex polygon of `n` corners round the middle of a cell's box, in
  /// box units, turned by `turn` of a step
  pub(super) fn polygon(n: usize, turn: f32) -> CellShape {
    let corners = (0..n)
      .map(|i| {
        let a = (i as f32 + turn) * std::f32::consts::TAU / n as f32;
        Vec2::new(0.5 + 0.4 * a.cos(), 0.5 + 0.4 * a.sin())
      })
      .collect();
    CellShape {
      polygons: vec![corners],
      one_way: false,
    }
  }

  #[test]
  fn a_hex_shape_cell_is_a_closed_ring_in_its_own_part() {
    // 120 x 140 hexagons of side 70: row -2 is not shifted, so cell
    // (3, -2)'s box is 120 x 140 px from (360, -210)
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(120, 140), 70);
    let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::new(7.0, 5.0));
    let triangle = [
      Vec2::new(0.6, 0.6),
      Vec2::new(0.8, 100.0 / 140.0),
      Vec2::new(70.0 / 120.0, 108.0 / 140.0),
    ];
    let shape = tiles.add_shape(CellShape {
      polygons: vec![triangle.to_vec()],
      one_way: false,
    });
    assert!(tiles.set(IVec2::new(3, -2), TileCell::Shape(shape)));
    // A solid cell beside it keeps its six sides: the ring is its cell's
    // own, not merged with its neighbours. The sides are the chunk's part,
    // the ring its part of rings
    assert!(tiles.set(IVec2::new(4, -2), S));
    let parts = hex_part_lines(&tiles);
    let key = |rings| PackKey::Hex {
      chunk: (0, -1),
      part: HexBox::Chunk,
      rings,
    };
    let keys: Vec<PackKey> = parts.iter().map(|&(key, _)| key).collect();
    assert_eq!(keys, [key(false), key(true)], "{parts:?}");
    let (solid, ring) = (&parts[0].1, &parts[1].1);
    let rounded = |&(a, b): &(Vec2, Vec2)| {
      let point = |p: Vec2| (p.x.round() as i32, p.y.round() as i32);
      (point(a), point(b))
    };
    let mut solid: Vec<(Point, Point)> = solid.iter().map(rounded).collect();
    solid.sort_unstable();
    assert_eq!(
      solid,
      sides_with_one_solid(&hex, &BTreeSet::from([(4, -2)]))
    );
    // The ring: the polygon over the cell's box, closed, as authored
    let corner = Vec2::new(360.0, -210.0);
    let want: Vec<Vec2> = triangle
      .iter()
      .map(|&p| corner + p * Vec2::new(120.0, 140.0))
      .collect();
    assert_eq!(ring.len(), 3, "{ring:?}");
    for (i, &(a, b)) in ring.iter().enumerate() {
      let at = |p: Vec2| want.iter().position(|w| (*w - p).length() < 1e-3);
      let (Some(from), Some(to)) = (at(a), at(b)) else {
        panic!("edge {i} {a} {b} is not the triangle's: {want:?}");
      };
      assert_eq!(to, (from + 1) % 3, "edge {i} runs round it");
    }
    // A shape with no edges (one-way: a tile map makes those solid) adds
    // nothing, nor does one the grid lacks
    let one_way = tiles.add_shape(CellShape {
      one_way: true,
      ..polygon(5, 0.0)
    });
    assert!(tiles.set(IVec2::new(3, -2), TileCell::Shape(one_way)));
    assert!(tiles.set(IVec2::new(8, -2), TileCell::Shape(77)));
    let edges: usize = hex_part_lines(&tiles).iter().map(|(_, e)| e.len()).sum();
    assert_eq!(edges, 6);
  }

  /// An inset hexagon in a cell's box, clockwise on screen: six ring edges
  /// (the ring-heavy chunk's tile)
  pub(super) fn inset_hexagon() -> CellShape {
    let points = [
      (8.0, 2.0),
      (14.0, 5.0),
      (14.0, 11.0),
      (8.0, 14.0),
      (2.0, 11.0),
      (2.0, 5.0),
    ];
    CellShape {
      polygons: vec![
        points
          .iter()
          .map(|&(x, y)| Vec2::new(x, y) / 16.0)
          .collect(),
      ],
      one_way: false,
    }
  }

  /// Whether two lists of lines hold the same lines, in any order, each
  /// point within a thousandth of a pixel
  fn same_lines(a: &[(Vec2, Vec2)], b: &[(Vec2, Vec2)]) -> bool {
    let sorted = |lines: &[(Vec2, Vec2)]| {
      let mut lines = lines.to_vec();
      let px = |v: f32| (v * 64.0).round() as i64;
      lines.sort_by_key(|&(a, b)| (px(a.x), px(a.y), px(b.x), px(b.y)));
      lines
    };
    let near = |p: Vec2, q: Vec2| (p - q).abs().max_element() < 1e-3;
    a.len() == b.len()
      && sorted(a)
        .iter()
        .zip(&sorted(b))
        .all(|(x, y)| near(x.0, y.0) && near(x.1, y.1))
  }

  #[test]
  fn a_hex_shape_cell_s_rings_sit_in_a_ring_part_apart_from_the_sides() {
    // A shape of two polygons in cell (3, -2) of 120 x 140 hexagons, and a
    // solid cell beside it, both in chunk (0, -1): one part of its sides,
    // one of its rings
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(120, 140), 70);
    let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::new(7.0, 5.0));
    let polygons = vec![
      vec![
        Vec2::new(0.6, 0.6),
        Vec2::new(0.8, 100.0 / 140.0),
        Vec2::new(70.0 / 120.0, 108.0 / 140.0),
      ],
      vec![
        Vec2::new(0.1, 0.1),
        Vec2::new(0.3, 0.1),
        Vec2::new(0.3, 0.3),
        Vec2::new(0.1, 0.3),
      ],
    ];
    let shape = tiles.add_shape(CellShape {
      polygons: polygons.clone(),
      one_way: false,
    });
    let cell = IVec2::new(3, -2);
    assert!(tiles.set(cell, TileCell::Shape(shape)));
    assert!(tiles.set(IVec2::new(4, -2), S));
    let parts = hex_part_lines(&tiles);
    let key = |rings| PackKey::Hex {
      chunk: (0, -1),
      part: HexBox::Chunk,
      rings,
    };
    let keys: Vec<PackKey> = parts.iter().map(|&(key, _)| key).collect();
    assert_eq!(keys, [key(false), key(true)], "{parts:?}");
    // The sides part holds the solid cell's six sides alone
    let rounded = |&(a, b): &(Vec2, Vec2)| {
      let point = |p: Vec2| (p.x.round() as i32, p.y.round() as i32);
      (point(a), point(b))
    };
    let mut sides: Vec<(Point, Point)> = parts[0].1.iter().map(rounded).collect();
    sides.sort_unstable();
    assert_eq!(
      sides,
      sides_with_one_solid(&hex, &BTreeSet::from([(4, -2)]))
    );
    // and the rings part exactly the cell's two rings over its box
    let (corner, size) = hex.cell_box(cell);
    let place = |p: Vec2| corner.as_vec2() + p * size.as_vec2();
    let rings: Vec<(Vec2, Vec2)> = polygons
      .iter()
      .flat_map(|ring| {
        (0..ring.len()).map(move |i| (place(ring[i]), place(ring[(i + 1) % ring.len()])))
      })
      .collect();
    assert!(same_lines(&parts[1].1, &rings), "{:?}", parts[1].1);
  }

  #[test]
  fn scattered_shape_cells_share_one_ring_part_a_chunk() {
    // Ten inset hexagons spread over chunk (-1, 0) of 16 px hexagons, in
    // each of its quarters, 60 ring edges, among a checkerboard of solid
    // cells whose sides split the chunk into its 16 blocks' parts: the
    // rings are one part still
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
    let inset = TileCell::Shape(tiles.add_shape(inset_hexagon()));
    let first = IVec2::new(-CHUNK, 0);
    for y in 1..31 {
      for x in 1..31 {
        if (x + y) % 2 == 0 {
          assert!(tiles.set(first + IVec2::new(x, y), S));
        }
      }
    }
    let spots = [
      (1, 2),
      (9, 4),
      (19, 6),
      (29, 2),
      (5, 14),
      (23, 12),
      (3, 22),
      (13, 28),
      (21, 20),
      (27, 30),
    ];
    for (x, y) in spots {
      assert!(tiles.set(first + IVec2::new(x, y), inset));
    }
    let parts = hex_parts_of(&tiles);
    let rings: Vec<(PackKey, usize)> = parts
      .iter()
      .filter(|(key, _)| matches!(key, PackKey::Hex { rings: true, .. }))
      .map(|(key, edges)| (*key, edges.len()))
      .collect();
    let whole = PackKey::Hex {
      chunk: (-1, 0),
      part: HexBox::Chunk,
      rings: true,
    };
    assert_eq!(rings, [(whole, 60)]);
    let blocks = parts
      .iter()
      .filter(|(key, _)| {
        matches!(
          key,
          PackKey::Hex {
            chunk: (-1, 0),
            part: HexBox::Block(_),
            rings: false
          }
        )
      })
      .count();
    assert_eq!(blocks, 16, "the sides split into blocks");
  }

  #[test]
  fn ring_parts_split_by_their_own_budget() {
    // Chunk (-1, -1) of 16 px hexagons full of inset hexagons, six ring
    // edges a cell: its rings (6,144 edges), each quarter's (1,536) and
    // each block's (384) are past the budget, so its 64 sub-blocks of 4 x 4
    // cells are its ring parts, 96 edges each, and it has no part of sides
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
    let inset = TileCell::Shape(tiles.add_shape(inset_hexagon()));
    let twelve = TileCell::Shape(tiles.add_shape(polygon(12, 0.0)));
    let first = IVec2::splat(-CHUNK);
    for y in 0..CHUNK {
      for x in 0..CHUNK {
        assert!(tiles.set(first + IVec2::new(x, y), inset));
      }
    }
    let counts = |tiles: &TileColliders| -> Vec<(PackKey, usize)> {
      let mut parts: Vec<(PackKey, usize)> = hex_parts_of(tiles)
        .into_iter()
        .map(|(key, edges)| (key, edges.len()))
        .collect();
      parts.sort_unstable();
      parts
    };
    let sub = |s: u8| PackKey::Hex {
      chunk: (-1, -1),
      part: HexBox::Sub(s),
      rings: true,
    };
    let mut want: Vec<(PackKey, usize)> = (0..64).map(|s| (sub(s), 96)).collect();
    assert_eq!(counts(&tiles), want);
    // Sub-block (2, 1) of 12-gons holds 192 edges, past the budget, and
    // stays one part, built afresh and by an edit
    let mut parts = HexParts::build(tiles.cells_view(), hex, tiles.bounds());
    tiles.take_changes();
    for y in 4..8 {
      for x in 8..12 {
        assert!(tiles.set(first + IVec2::new(x, y), twelve));
      }
    }
    let changed = tiles.take_changes().expect("listed");
    let changes = parts.apply(tiles.cells_view(), hex, tiles.bounds(), &changed);
    assert_eq!(changes.rebuilt, [sub(10)]);
    assert!(changes.gone.is_empty());
    want[10].1 = 192;
    assert_eq!(counts(&tiles), want);
    let mut keys = parts.keys();
    keys.sort_unstable();
    assert!(keys.iter().copied().eq(want.iter().map(|&(key, _)| key)));
    let (_, collider) = tiles.hex_pack(&parts, sub(10));
    let super::super::ColliderShape::Segments { segments, .. } = &collider.shape else {
      panic!("segments: {collider:?}");
    };
    assert_eq!(segments.len(), 192);
  }

  #[test]
  fn a_hex_boundary_gives_a_shape_cell_s_rings_as_its_part_holds_them() {
    // Two triangles, one wound each way, in cell (3, -2) of 120 x 140
    // hexagons, read in a block whose first cell is (0, -32)
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(120, 140), 70);
    let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
    let mut boundary = Boundary::default().with_hex(hex);
    let polygons = vec![
      vec![
        Vec2::new(0.1, 0.1),
        Vec2::new(0.3, 0.2),
        Vec2::new(0.1, 0.4),
      ],
      vec![
        Vec2::new(0.6, 0.6),
        Vec2::new(70.0 / 120.0, 108.0 / 140.0),
        Vec2::new(0.8, 100.0 / 140.0),
      ],
    ];
    let shape = CellShape {
      polygons,
      one_way: false,
    };
    let kind = TileCell::Shape(tiles.add_shape(shape.clone()));
    assert_eq!(TileCell::Shape(boundary.add_shape(shape)), kind);
    let cell = IVec2::new(3, -2);
    assert!(tiles.set(cell, kind));
    let first = IVec2::new(0, -32);
    let mut rings = Vec::new();
    boundary.hex_rings(cell, kind, first, &mut rings);
    assert_eq!(rings.len(), 2, "{rings:?}");
    let corner = hex.corner_of(first).as_dvec2();
    let mut edges: Vec<(Vec2, Vec2)> = rings
      .iter()
      .flat_map(|ring| {
        (0..ring.len()).map(move |i| {
          let at = |p: DVec2| (corner + p).as_vec2();
          (at(ring[i]), at(ring[(i + 1) % ring.len()]))
        })
      })
      .collect();
    let mut held: Vec<(Vec2, Vec2)> = hex_part_lines(&tiles)
      .into_iter()
      .flat_map(|(_, lines)| lines)
      .collect();
    let key = |&(a, b): &(Vec2, Vec2)| {
      let px = |v: f32| (v * 64.0).round() as i64;
      (px(a.x), px(a.y), px(b.x), px(b.y))
    };
    edges.sort_by_key(key);
    held.sort_by_key(key);
    assert_eq!(edges.len(), held.len());
    for (got, want) in edges.iter().zip(&held) {
      let near = |p: Vec2, q: Vec2| (p - q).abs().max_element() < 1e-3;
      assert!(
        near(got.0, want.0) && near(got.1, want.1),
        "{got:?} {want:?}"
      );
    }
    // Nothing for any other cell or a one-way shape
    let one_way = boundary.add_shape(CellShape {
      one_way: true,
      ..polygon(5, 0.0)
    });
    let mut none = Vec::new();
    for other in [S, E, O, TileCell::Shape(one_way), TileCell::Shape(77)] {
      boundary.hex_rings(cell, other, first, &mut none);
    }
    assert!(none.is_empty(), "{none:?}");
  }

  #[test]
  fn a_self_crossing_hex_shape_collides_as_its_hull() {
    // A bow tie in cell (2, 1) of 120 x 140 hexagons (row 1 shifted by
    // half a cell: its box from (300, 105))
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(120, 140), 70);
    let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
    let bow_tie = [(0.25, 0.25), (0.75, 0.75), (0.75, 0.25), (0.25, 0.75)];
    let shape = tiles.add_shape(CellShape {
      polygons: vec![bow_tie.iter().map(|&(x, y)| Vec2::new(x, y)).collect()],
      one_way: false,
    });
    assert!(tiles.set(IVec2::new(2, 1), TileCell::Shape(shape)));
    let edges: Vec<(Vec2, Vec2)> = hex_part_lines(&tiles)
      .into_iter()
      .flat_map(|(_, edges)| edges)
      .collect();
    // One ring of its hull's four corners, clockwise on screen
    let corner = Vec2::new(300.0, 105.0);
    let mut held: Vec<Point> = edges
      .iter()
      .map(|&(a, _)| {
        (
          (a.x - corner.x).round() as i32,
          (a.y - corner.y).round() as i32,
        )
      })
      .collect();
    held.sort_unstable();
    assert_eq!(
      held,
      [(30, 35), (30, 105), (90, 35), (90, 105)],
      "{edges:?}"
    );
    let area: f32 = edges.iter().map(|&(a, b)| a.perp_dot(b)).sum();
    assert!(area > 0.0, "clockwise: {edges:?}");
  }

  #[test]
  fn a_hex_shape_partly_or_wholly_outside_its_box_collides_as_drawn() {
    // 16 px hexagons of side 8: cell (0, 0)'s box is 16 x 16 from the
    // origin. A triangle reaching past the box's right side, and one wholly
    // below it, each collide as drawn, uncut
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
    let partly = [(0.5, 0.25), (1.5, 0.5), (0.5, 0.75)];
    let wholly = [(0.25, 1.25), (0.75, 1.5), (0.25, 1.75)];
    for (x, points) in [(0, partly), (4, wholly)] {
      let shape = tiles.add_shape(CellShape {
        polygons: vec![points.iter().map(|&(x, y)| Vec2::new(x, y)).collect()],
        one_way: false,
      });
      assert!(tiles.set(IVec2::new(x, 0), TileCell::Shape(shape)));
    }
    let mut held: Vec<(i32, i32)> = hex_part_lines(&tiles)
      .into_iter()
      .flat_map(|(_, edges)| edges)
      .map(|(a, _)| (a.x.round() as i32, a.y.round() as i32))
      .collect();
    held.sort_unstable();
    // Cell (4, 0)'s box is 64 px along
    let mut want: Vec<(i32, i32)> = partly
      .iter()
      .map(|&(x, y)| ((x * 16.0) as i32, (y * 16.0) as i32))
      .chain(
        wholly
          .iter()
          .map(|&(x, y)| (64 + (x * 16.0) as i32, (y * 16.0) as i32)),
      )
      .collect();
    want.sort_unstable();
    assert_eq!(held, want);
  }

  #[test]
  fn a_quarter_pushed_one_edge_past_its_budget_by_an_edit_splits() {
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let half = HEX_PART_EDGES / 2;
    let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
    let rings = [
      tiles.add_shape(polygon(half, 0.0)),
      tiles.add_shape(polygon(half, 0.5)),
      tiles.add_shape(polygon(half + 1, 0.0)),
    ];
    // Two rings in each of the top quarters of chunk (0, 0), in blocks 0,
    // 1, 2 and 3: the chunk holds twice the budget, so it splits, and each
    // quarter holds the budget exactly, so each is one part
    for x in [2, 10, 18, 26] {
      assert!(tiles.set(IVec2::new(x, 2), TileCell::Shape(rings[0])));
    }
    let mut parts = HexParts::build(tiles.cells_view(), hex, tiles.bounds());
    tiles.take_changes();
    let at = |part: HexBox| PackKey::Hex {
      chunk: (0, 0),
      part,
      rings: true,
    };
    let quarters = vec![at(HexBox::Quarter(0)), at(HexBox::Quarter(1))];
    assert_eq!(parts.keys(), quarters);
    let mut edit = |parts: &mut HexParts, shape: u16| {
      assert!(tiles.set(IVec2::new(2, 2), TileCell::Shape(shape)));
      let changed = tiles.take_changes().expect("listed");
      let changes = parts.apply(tiles.cells_view(), hex, tiles.bounds(), &changed);
      let fresh = HexParts::build(tiles.cells_view(), hex, tiles.bounds());
      assert_eq!(held_edges(parts), held_edges(&fresh));
      assert_eq!(held_rings(parts), held_rings(&fresh));
      assert_eq!(held_rings(parts).len(), 4, "its four rings");
      changes.rebuilt
    };
    // An edit that keeps quarter 0 at the budget keeps it one part
    assert_eq!(edit(&mut parts, rings[1]), [at(HexBox::Quarter(0))]);
    assert_eq!(parts.keys(), quarters);
    // and one taking it one edge past splits it into its blocks
    let blocks = vec![at(HexBox::Block(0)), at(HexBox::Block(1))];
    assert_eq!(edit(&mut parts, rings[2]), blocks);
    let mut split = blocks;
    split.push(at(HexBox::Quarter(1)));
    assert_eq!(parts.keys(), split);
  }

  #[test]
  fn hex_parts_edited_with_shape_cells_hold_what_a_fresh_build_does() {
    use crate::core::ecs::{Entity, World};
    use std::collections::BTreeMap;
    let mut seed = 0x510e_527f_u32;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    // Every line of a grid's parts, rounded to a thousandth of a pixel,
    // sorted
    let lines = |tiles: &TileColliders, parts: &HexParts| {
      let origin = tiles.hex().map_or(Vec2::ZERO, |(_, origin)| origin);
      let mut all: Vec<[i64; 4]> = parts
        .keys()
        .into_iter()
        .flat_map(|key| {
          let (at, collider) = tiles.hex_pack(parts, key);
          lines_of(&collider, at - origin)
        })
        .map(|(a, b)| [a.x, a.y, b.x, b.y].map(|v| (f64::from(v) * 1000.0).round() as i64))
        .collect();
      all.sort_unstable();
      all
    };
    // What parts hold: their sides' edges and their rings' cells, sorted,
    // each part of one kind and each key once
    let held = |parts: &HexParts| {
      let keys = parts.keys();
      let unique: BTreeSet<PackKey> = keys.iter().copied().collect();
      assert_eq!(unique.len(), keys.len(), "a key twice: {keys:?}");
      for &key in &keys {
        let PackKey::Hex { rings: ring, .. } = key else {
          panic!("{key:?}");
        };
        let (e, r) = (parts.edges(key).count(), parts.rings(key).count());
        assert!(
          e + r > 0 && (!ring || e == 0) && (ring || r == 0),
          "{key:?}: {e} {r}"
        );
      }
      (held_edges(parts), held_rings(parts))
    };
    /// The ring splits the edits reached: a sub-block emptied, a block
    /// split into sub-blocks, a quarter split into blocks, and a chunk
    /// whose rings split, emptied, then one part again; and the chunks
    /// whose split rings emptied
    #[derive(Default)]
    struct Seen {
      emptied: usize,
      into_subs: usize,
      into_blocks: usize,
      merged: usize,
      reset: BTreeSet<(i32, i32)>,
    }
    // Applies the edits as the tile rebuild does, each part's entity kept:
    // the parts gone are despawned, the rebuilt ones with none spawned. The
    // parts gone are exactly those whose keys went, and every key ends with
    // an entity
    let apply = |parts: &mut HexParts,
                 tiles: &mut TileColliders,
                 hex: HexCells,
                 (world, seen): (&mut World, &mut Seen),
                 what: &str| {
      let changed = tiles.take_changes().expect("listed");
      let before: BTreeMap<PackKey, Entity> = parts.parts().collect();
      let changes = parts.apply(tiles.cells_view(), hex, tiles.bounds(), &changed);
      let after: BTreeSet<PackKey> = parts.keys().into_iter().collect();
      let went: BTreeSet<Entity> = before
        .iter()
        .filter(|(key, _)| !after.contains(key))
        .map(|(_, &part)| part)
        .collect();
      let gone: BTreeSet<Entity> = changes.gone.iter().copied().collect();
      assert_eq!(
        gone.len(),
        changes.gone.len(),
        "{hex:?} {what}: a part gone twice"
      );
      assert_eq!(
        gone, went,
        "{hex:?} {what}: the parts gone are those whose keys went"
      );
      for &key in &after {
        if !before.contains_key(&key) {
          assert!(
            changes.rebuilt.contains(&key),
            "{hex:?} {what}: {key:?} made"
          );
        }
      }
      for &key in &changes.rebuilt {
        assert!(
          after.contains(&key),
          "{hex:?} {what}: {key:?} rebuilt, not held"
        );
        if parts.part(key).is_none() {
          parts.set_part(key, world.spawn_empty().id());
        }
      }
      let kept: BTreeSet<PackKey> = parts.parts().map(|(key, _)| key).collect();
      assert_eq!(kept, after, "{hex:?} {what}: an entity a part");
      // The ring splits it reached, chunk by chunk
      let rings_of = |keys: &mut dyn Iterator<Item = PackKey>, chunk: (i32, i32)| -> Vec<HexBox> {
        keys
          .filter_map(|key| match key {
            PackKey::Hex {
              chunk: c,
              part,
              rings: true,
            } if c == chunk => Some(part),
            _ => None,
          })
          .collect()
      };
      let chunks: BTreeSet<(i32, i32)> = before
        .keys()
        .chain(&after)
        .filter_map(|key| match *key {
          PackKey::Hex { chunk, .. } => Some(chunk),
          _ => None,
        })
        .collect();
      for chunk in chunks {
        let was = rings_of(&mut before.keys().copied(), chunk);
        let now = rings_of(&mut after.iter().copied(), chunk);
        let quarter_of = |b: u8| b / 8 * 2 + b % 4 / 2;
        let block_of = |s: u8| s / 16 * 4 + s % 8 / 2;
        for &part in was.iter().filter(|part| !now.contains(part)) {
          match part {
            HexBox::Sub(_) if !now.is_empty() => seen.emptied += 1,
            HexBox::Block(b)
              if now
                .iter()
                .any(|p| matches!(*p, HexBox::Sub(s) if block_of(s) == b)) =>
            {
              seen.into_subs += 1
            }
            HexBox::Quarter(q)
              if now.iter().any(|p| match *p {
                HexBox::Block(b) => quarter_of(b) == q,
                HexBox::Sub(s) => quarter_of(block_of(s)) == q,
                _ => false,
              }) =>
            {
              seen.into_blocks += 1
            }
            _ => {}
          }
        }
        if !was.is_empty() && !was.contains(&HexBox::Chunk) && now.is_empty() {
          seen.reset.insert(chunk);
        }
        if now == [HexBox::Chunk] && seen.reset.remove(&chunk) {
          seen.merged += 1;
        }
      }
      changes
    };
    let mut grids = hex_variants();
    for (axis, index) in [
      (StaggerAxis::Y, StaggerIndex::Odd),
      (StaggerAxis::X, StaggerIndex::Even),
    ] {
      grids.push(HexCells::new(axis, index, UVec2::new(32, 32), 0));
    }
    let mut seen = Seen::default();
    for hex in grids {
      let mut world = World::new();
      let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
      let shapes = [
        tiles.add_shape(polygon(3, 0.25)),
        tiles.add_shape(polygon(40, 0.0)),
        tiles.add_shape(inset_hexagon()),
        tiles.add_shape(polygon(12, 0.5)),
      ];
      let mut parts = HexParts::build(tiles.cells_view(), hex, tiles.bounds());
      tiles.take_changes();
      // 40 x 24 cells across chunk borders either way, chunk (0, 0)'s and
      // (0, -1)'s cells in two of their quarters
      let (w, h) = (40, 24);
      let area = |x: u32, y: u32| IVec2::new(x as i32 + 4, y as i32 - 12);
      let check = |tiles: &TileColliders, parts: &HexParts, what: &str| {
        let fresh = HexParts::build(tiles.cells_view(), hex, tiles.bounds());
        assert_eq!(held(parts), held(&fresh), "{hex:?} {what}");
        // Parts in the same boxes as a fresh build's place their lines alike
        if parts.keys() == fresh.keys() {
          assert_eq!(lines(tiles, parts), lines(tiles, &fresh), "{hex:?} {what}");
        }
      };
      for round in 0..60 {
        // Every tenth round, the area's cells in one of its four chunks
        // emptied, so their rings go and come back unsplit
        if round % 10 == 9 {
          let chunk = IVec2::new(round / 10 % 2, round / 20 % 2 - 1);
          for at in (0..w * h).map(|i| area(i % w, i / w)) {
            if at.div_euclid(IVec2::splat(CHUNK)) == chunk {
              tiles.set(at, TileCell::Empty);
            }
          }
          let what = format!("round {round}, chunk {chunk} emptied");
          apply(&mut parts, &mut tiles, hex, (&mut world, &mut seen), &what);
          check(&tiles, &parts, &what);
        }
        // A handful of cells set in a small area across chunk borders,
        // rings of 40 edges among them
        for _ in 0..16 {
          let at = area(next(w), next(h));
          let kind = match next(7) {
            0 => TileCell::Empty,
            1..=4 => TileCell::Shape(shapes[next(4) as usize]),
            5 => O,
            _ => S,
          };
          tiles.set(at, kind);
        }
        let what = format!("round {round}");
        apply(&mut parts, &mut tiles, hex, (&mut world, &mut seen), &what);
        check(&tiles, &parts, &what);
        // A few shape cells turned to other shapes: only ring parts change
        let held_shapes: Vec<(IVec2, u16)> = (0..w * h)
          .map(|i| area(i % w, i / w))
          .filter_map(|at| match tiles.get(at) {
            TileCell::Shape(shape) => Some((at, shape)),
            _ => None,
          })
          .collect();
        if held_shapes.is_empty() {
          continue;
        }
        for _ in 0..4 {
          let (at, shape) = held_shapes[next(held_shapes.len() as u32) as usize];
          let from = shapes.iter().position(|&s| s == shape).unwrap_or(0);
          let other = shapes[(from + 1 + next(3) as usize) % shapes.len()];
          tiles.set(at, TileCell::Shape(other));
        }
        let what = format!("round {round}, shapes turned");
        let changes = apply(&mut parts, &mut tiles, hex, (&mut world, &mut seen), &what);
        assert!(
          changes
            .rebuilt
            .iter()
            .all(|key| matches!(key, PackKey::Hex { rings: true, .. })),
          "{hex:?} {what}: rebuilt rings alone, {:?}",
          changes.rebuilt
        );
        check(&tiles, &parts, &what);
      }
    }
    let Seen {
      emptied,
      into_subs,
      into_blocks,
      merged,
      ..
    } = seen;
    assert!(
      emptied > 0 && into_subs > 0 && into_blocks > 0 && merged > 0,
      "sub-blocks emptied {emptied}, blocks split {into_subs}, quarters split {into_blocks}, chunks merged back {merged}"
    );
  }

  #[test]
  fn a_hex_boundary_gives_the_same_edges_as_its_parts() {
    let mut seed = 0x3c6e_f372_u32;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    let variants = hex_variants();
    // Unbounded, across chunks either side of 0: blocks of 8 from a block
    // left of and above the cells to one past them, each block's edges in
    // points from its first cell's box
    for round in 0..50 {
      let hex = variants[round % variants.len()];
      let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
      let reach = if round % 2 == 0 { 40 } else { 8 };
      for _ in 0..60 {
        let at = IVec2::new(
          next(2 * reach) as i32 - reach as i32,
          next(2 * reach) as i32 - reach as i32,
        );
        tiles.set(at, [S, S, S, O][next(4) as usize]);
      }
      let held = hex_part_edges(&tiles);
      assert!(!held.is_empty());
      let boundary = Boundary::default().with_hex(hex);
      let at = |x: i32, y: i32| tiles.get(IVec2::new(x, y));
      let mut pieced = Vec::new();
      for by in (-48..48).step_by(8) {
        for bx in (-48..48).step_by(8) {
          let (min, max) = ((bx, by), (bx + 8, by + 8));
          let (mut block, mut placed, mut rebuilt) = (Vec::new(), Vec::new(), Vec::new());
          boundary.edges(at, min, max, None, &mut block);
          boundary.placed_edges(at, min, max, None, &mut placed);
          boundary.rebuilt_borders(at, min, max, None, &placed, &mut rebuilt);
          let unplaced: Vec<(Point, Point)> = placed.iter().map(|&(_, a, b)| (a, b)).collect();
          assert_eq!(unplaced, block, "placed edges are the scan's");
          assert_eq!(rebuilt, placed, "and its borders rebuilt give them");
          assert!(
            placed.windows(2).all(|pair| pair[0].0 <= pair[1].0),
            "in order"
          );
          let from = hex.corner_of(IVec2::new(bx, by));
          let moved = |(x, y): Point| {
            (
              (i64::from(x) + from.x) as i32,
              (i64::from(y) + from.y) as i32,
            )
          };
          pieced.extend(block.into_iter().map(|(a, b)| (moved(a), moved(b))));
        }
      }
      pieced.sort_unstable();
      assert_eq!(pieced, held, "{hex:?} round {round}");
    }
    // Bounded, in blocks that do not divide it: the sides on its border
    // against the cells outside it come too, owned by the cells inside
    for (round, hex) in variants.into_iter().enumerate() {
      let (w, h) = (13, 11);
      let mut tiles = TileColliders::new(w, h, Vec2::ONE).with_hex(hex, Vec2::ZERO);
      for y in 0..h {
        for x in 0..w {
          if next(3) > 0 {
            tiles.set(xy(x, y), S);
          }
        }
      }
      let held = hex_part_edges(&tiles);
      let boundary = Boundary::default().with_hex(hex);
      let bounds = Some(CellRect::from_size(UVec2::new(w, h)));
      let at = |x: i32, y: i32| tiles.get(IVec2::new(x, y));
      let mut pieced = Vec::new();
      for by in (0..h as i32).step_by(4) {
        for bx in (0..w as i32).step_by(4) {
          let max = ((bx + 4).min(w as i32), (by + 4).min(h as i32));
          let mut block = Vec::new();
          boundary.edges(at, (bx, by), max, bounds, &mut block);
          let from = hex.corner_of(IVec2::new(bx, by));
          let moved = |(x, y): Point| {
            (
              (i64::from(x) + from.x) as i32,
              (i64::from(y) + from.y) as i32,
            )
          };
          pieced.extend(block.into_iter().map(|(a, b)| (moved(a), moved(b))));
        }
      }
      pieced.sort_unstable();
      assert_eq!(pieced, held, "bounded {hex:?} {round}");
    }
  }

  #[test]
  fn hex_chains_hold_each_of_a_block_s_edges_once() {
    // Each segment cut into its whole-pixel steps, so a straight run
    // merged into one segment compares with its edges
    fn steps(segments: impl Iterator<Item = (Point, Point)>) -> Vec<(Point, Point)> {
      fn gcd(a: i32, b: i32) -> i32 {
        if b == 0 { a.abs() } else { gcd(b, a % b) }
      }
      let mut all = Vec::new();
      for (a, b) in segments {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let n = gcd(dx, dy).max(1);
        all.extend((0..n).map(|i| {
          let at = |i: i32| (a.0 + dx * i / n, a.1 + dy * i / n);
          (at(i), at(i + 1))
        }));
      }
      all.sort_unstable();
      all
    }
    let mut seed = 0x6c07_8965_u32;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    let (mut chained, mut merged) = (0, 0);
    for hex in hex_variants() {
      let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
      for _ in 0..140 {
        let at = IVec2::new(next(16) as i32 - 6, next(16) as i32 - 6);
        tiles.set(at, [S, S, E, O][next(4) as usize]);
      }
      let boundary = Boundary::default().with_hex(hex);
      let at = |x: i32, y: i32| tiles.get(IVec2::new(x, y));
      let mut blocks: Vec<((i32, i32), (i32, i32))> = (0..40)
        .map(|_| {
          let min = (next(18) as i32 - 9, next(18) as i32 - 9);
          (
            min,
            (min.0 + 1 + next(8) as i32, min.1 + 1 + next(8) as i32),
          )
        })
        .collect();
      // and, last, one holding every cell and the cells owning their sides
      blocks.push(((-8, -8), (12, 12)));
      let whole = blocks.len() - 1;
      for (block, (min, max)) in blocks.into_iter().enumerate() {
        let mut placed = Vec::new();
        boundary.placed_edges(at, min, max, None, &mut placed);
        let chains = boundary
          .hex_chains(at, min, max, None, &placed)
          .expect("three cells meet at each corner");
        let segments = chains
          .iter()
          .flat_map(|chain| chain.windows(2).map(|pair| (pair[0], pair[1])));
        let edges = placed.iter().map(|&(_, a, b)| (a, b));
        assert_eq!(steps(segments), steps(edges), "{hex:?} {min:?} to {max:?}");
        // No point a chain runs straight through: straight runs are one
        // segment (a grid whose side is the cell's length or longer has
        // them, along its rectangles' tops and bottoms)
        for chain in &chains {
          for w in chain.windows(3) {
            let (u, v) = (
              (w[1].0 - w[0].0, w[1].1 - w[0].1),
              (w[2].0 - w[1].0, w[2].1 - w[1].1),
            );
            let straight = u.0 * v.1 == u.1 * v.0 && u.0 * v.0 + u.1 * v.1 > 0;
            assert!(
              !straight,
              "{hex:?}: {chain:?} runs straight through {:?}",
              w[1]
            );
          }
        }
        merged += placed.len() - chains.iter().map(|c| c.len() - 1).sum::<usize>();
        chained += chains.len();
        if block == whole {
          // The whole boundary: closed loops alone
          assert!(!chains.is_empty());
          for chain in &chains {
            assert_eq!(chain.first(), chain.last(), "{hex:?}: {chain:?}");
          }
        }
      }
    }
    assert!(
      chained > 500 && merged > 20,
      "{chained} chains, {merged} edges merged"
    );
    // Sides of length 0, where four cells meet at a point: by points
    let diamonds = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 0);
    assert!(diamonds.point_sides());
    let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(diamonds, Vec2::ZERO);
    tiles.set(IVec2::ZERO, S);
    let boundary = Boundary::default().with_hex(diamonds);
    let at = |x: i32, y: i32| tiles.get(IVec2::new(x, y));
    let mut placed = Vec::new();
    boundary.placed_edges(at, (-1, -1), (2, 2), None, &mut placed);
    assert!(!placed.is_empty());
    assert_eq!(
      boundary.hex_chains(at, (-1, -1), (2, 2), None, &placed),
      None
    );
    assert_eq!(
      Boundary::default().hex_chains(at, (0, 0), (1, 1), None, &[]),
      None
    );
  }

  #[test]
  fn a_hex_block_s_rims_rebuilt_alone_give_its_edges_in_the_scan_s_order() {
    // Blocks of hexagonal grids, bounded and not, the cells round each
    // (above it too, which its cells' units never read) changed at random:
    // its rims rebuilt from what it kept give its edges, places and order
    // as a scan of it now gives them. An empty block keeps nothing
    let mut seed = 0x2f6b_1d55_u32;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    let kinds = [S, S, E, E, E, O];
    let (mut rebuilt, mut scanned) = (0, 0);
    for (round, hex) in hex_variants().into_iter().enumerate() {
      let bounded = round % 2 == 0;
      let (width, height) = (14u32, 12u32);
      let mut tiles = if bounded {
        TileColliders::new(width, height, Vec2::ONE)
      } else {
        TileColliders::unbounded(Vec2::ONE)
      }
      .with_hex(hex, Vec2::ZERO);
      let bounds = tiles.bounds();
      let (lo, hi) = if bounded { (0, 14) } else { (-6, 10) };
      let cell = |next: &mut dyn FnMut(u32) -> u32| {
        let span = (hi - lo) as u32;
        IVec2::new(lo + next(span) as i32, lo + next(span) as i32)
      };
      for _ in 0..120 {
        let at = cell(&mut next);
        tiles.set(at, kinds[next(kinds.len() as u32) as usize]);
      }
      let boundary = Boundary::default().with_hex(hex);
      for block in 0..60 {
        // A block of 1 to 6 cells a side, within a bounded grid or past it
        let min = (lo + next(10) as i32, lo + next(10) as i32);
        let max = (min.0 + 1 + next(6) as i32, min.1 + 1 + next(6) as i32);
        if block % 5 == 0 {
          for y in min.1..max.1 {
            for x in min.0..max.0 {
              tiles.set(IVec2::new(x, y), E);
            }
          }
        }
        let mut kept = Vec::new();
        let at = |x: i32, y: i32| tiles.get(IVec2::new(x, y));
        boundary.placed_edges(at, min, max, bounds, &mut kept);
        // The ring of cells round it, changed
        for y in min.1 - 1..=max.1 {
          for x in min.0 - 1..=max.0 {
            let outside = x < min.0 || x >= max.0 || y < min.1 || y >= max.1;
            if outside && next(2) == 0 {
              tiles.set(IVec2::new(x, y), kinds[next(kinds.len() as u32) as usize]);
            }
          }
        }
        let at = |x: i32, y: i32| tiles.get(IVec2::new(x, y));
        let (mut now, mut rims) = (Vec::new(), Vec::new());
        boundary.placed_edges(at, min, max, bounds, &mut now);
        boundary.rebuilt_borders(at, min, max, bounds, &kept, &mut rims);
        assert_eq!(rims, now, "{hex:?} block {min:?} to {max:?}");
        rebuilt += usize::from(rims != kept);
        scanned += now.len();
      }
    }
    assert!(rebuilt > 150 && scanned > 1500, "{rebuilt} {scanned}");
  }

  #[test]
  fn a_bounded_hex_grid_s_border_belongs_to_its_inside_cells() {
    // A bounded 40 x 40 grid with its row 0 and its column 0 solid: the
    // sides along its top and left borders belong to the cells inside it,
    // so every part is in a chunk inside the bounds, an edit on the border
    // rebuilds no part outside them, and the edges are still each side
    // with one solid cell
    let inside = |key: PackKey| {
      matches!(
        key,
        PackKey::Hex {
          chunk: (0..=1, 0..=1),
          ..
        }
      )
    };
    for hex in hex_variants() {
      let (w, h) = (40, 40);
      let mut tiles = TileColliders::new(w, h, Vec2::ONE).with_hex(hex, Vec2::ZERO);
      let mut solid = BTreeSet::new();
      let row = (0..40).map(|x| (x, 0));
      let column = (1..40).map(|y| (0, y));
      for (x, y) in row.chain(column) {
        assert!(tiles.set(xy(x, y), S));
        solid.insert((x as i32, y as i32));
      }
      let parts = hex_parts_of(&tiles);
      assert!(!parts.is_empty());
      for (key, _) in &parts {
        assert!(inside(*key), "{hex:?}: a part outside the bounds, {key:?}");
      }
      assert_eq!(
        hex_part_edges(&tiles),
        sides_with_one_solid(&hex, &solid),
        "{hex:?}"
      );
      tiles.take_changes();
      let mut parts = HexParts::build(tiles.cells_view(), hex, tiles.bounds());
      for (cell, kind) in [((17, 0), E), ((0, 23), E), ((17, 0), S)] {
        assert!(tiles.set(xy(cell.0, cell.1), kind));
        if kind == S {
          solid.insert((cell.0 as i32, cell.1 as i32));
        } else {
          solid.remove(&(cell.0 as i32, cell.1 as i32));
        }
        let changed = tiles.take_changes().expect("listed");
        let changes = parts.apply(tiles.cells_view(), hex, tiles.bounds(), &changed);
        assert!(!changes.rebuilt.is_empty(), "{hex:?} {cell:?}");
        assert!(
          changes.rebuilt.iter().all(|&key| inside(key)),
          "{hex:?} {cell:?}: rebuilt {:?}",
          changes.rebuilt
        );
        assert!(parts.keys().into_iter().all(inside), "{hex:?} {cell:?}");
        assert_eq!(
          held_edges(&parts),
          sides_with_one_solid(&hex, &solid),
          "{hex:?} {cell:?}"
        );
      }
      // Boundary blocks read the same way: one outside the bounds gives
      // nothing, and those inside, pieced, give the parts' edges
      let boundary = Boundary::default().with_hex(hex);
      let bounds = Some(CellRect::from_size(UVec2::new(w, h)));
      let at = |x: i32, y: i32| tiles.get(IVec2::new(x, y));
      let mut outside = Vec::new();
      for (min, max) in [((-8, -8), (0, 0)), ((0, -8), (8, 0)), ((-8, 0), (0, 8))] {
        boundary.edges(at, min, max, bounds, &mut outside);
      }
      assert!(outside.is_empty(), "{hex:?}: {outside:?}");
      let mut pieced = Vec::new();
      for by in (0..h as i32).step_by(8) {
        for bx in (0..w as i32).step_by(8) {
          let mut block = Vec::new();
          boundary.edges(at, (bx, by), (bx + 8, by + 8), bounds, &mut block);
          let from = hex.corner_of(IVec2::new(bx, by));
          let moved = |(x, y): Point| {
            (
              (i64::from(x) + from.x) as i32,
              (i64::from(y) + from.y) as i32,
            )
          };
          pieced.extend(block.into_iter().map(|(a, b)| (moved(a), moved(b))));
        }
      }
      pieced.sort_unstable();
      assert_eq!(pieced, held_edges(&parts), "{hex:?}");
    }
  }

  #[test]
  fn a_hex_cell_past_the_reach_draws_but_does_not_collide() {
    // 64 px cells: a cell 40,000,000 cells out has corners past i32 (and
    // is past the grid's own reach): no part, no panic
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(64, 64), 32);
    let far = IVec2::new(40_000_000, 0);
    assert!(!hex.reaches(far));
    let mut tiles = TileColliders::unbounded(Vec2::splat(64.0)).with_hex(hex, Vec2::ZERO);
    assert!(!tiles.set(far, S));
    assert!(hex_parts_of(&tiles).is_empty());
    // Cells of 16,384 px within the grid's reach whose corners pass i32
    // from x 131,070: empty to the parts, so the last cell that reaches is
    // bounded beside them, and edits past it change nothing
    let hex = HexCells::new(
      StaggerAxis::Y,
      StaggerIndex::Odd,
      UVec2::new(16_384, 16_384),
      8192,
    );
    assert!(hex.traceable());
    let last = (131_000..)
      .take_while(|&x| hex.reaches(IVec2::new(x, 0)))
      .last()
      .expect("cells that reach");
    assert!(last < MAX_REACH && last > 131_000, "{last}");
    let mut tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, Vec2::ZERO);
    for x in last - 1..last + 3 {
      assert!(tiles.set(IVec2::new(x, 0), S), "stored: {x}");
    }
    tiles.take_changes();
    let mut parts = HexParts::build(tiles.cells_view(), hex, tiles.bounds());
    let reached = [(last - 1, 0), (last, 0)].into_iter().collect();
    assert_eq!(held_edges(&parts), sides_with_one_solid(&hex, &reached));
    assert_eq!(hex_part_edges(&tiles), sides_with_one_solid(&hex, &reached));
    for (x, kind) in [(last + 1, E), (last + 5, S), (last, E), (last, S)] {
      tiles.set(IVec2::new(x, 0), kind);
      let changed = tiles.take_changes().expect("listed");
      parts.apply(tiles.cells_view(), hex, tiles.bounds(), &changed);
      assert_eq!(
        held_edges(&parts),
        held_edges(&HexParts::build(tiles.cells_view(), hex, tiles.bounds())),
        "{x}"
      );
    }
    assert_eq!(held_edges(&parts), sides_with_one_solid(&hex, &reached));
  }

  #[test]
  fn hex_cell_polygons_are_their_corners_from_the_origin_and_squares_their_four() {
    let hex = HexCells::new(StaggerAxis::X, StaggerIndex::Even, UVec2::new(120, 140), 70);
    let origin = Vec2::new(5.0, -3.0);
    let tiles = TileColliders::unbounded(Vec2::ONE).with_hex(hex, origin);
    assert_eq!(tiles.hex(), Some((hex, origin)));
    let cell = IVec2::new(-3, 7);
    let polygon = tiles.cell_polygon(cell);
    let corners: Vec<Vec2> = hex
      .corners(cell)
      .iter()
      .map(|c| c.as_vec2() + origin)
      .collect();
    assert_eq!(polygon.points(), corners);
    // Square grids: four corners, clockwise, through the lattice if any
    let square = TileColliders::unbounded(Vec2::new(16.0, 8.0));
    assert_eq!(square.hex(), None);
    assert_eq!(
      square.cell_polygon(IVec2::new(2, -1)).points(),
      [
        Vec2::new(32.0, -8.0),
        Vec2::new(48.0, -8.0),
        Vec2::new(48.0, 0.0),
        Vec2::new(32.0, 0.0)
      ]
    );
    let iso = Affine2::from_cols(
      Vec2::new(32.0, 16.0),
      Vec2::new(-32.0, 16.0),
      Vec2::new(100.0, 0.0),
    );
    let diamonds = TileColliders::unbounded(Vec2::new(64.0, 32.0)).with_lattice(iso);
    assert_eq!(
      diamonds.cell_polygon(IVec2::new(1, 0)).points(),
      [
        Vec2::new(132.0, 16.0),
        Vec2::new(164.0, 32.0),
        Vec2::new(132.0, 48.0),
        Vec2::new(100.0, 32.0)
      ]
    );
  }
}

#[cfg(test)]
mod with_physics {
  use super::tests::xy;
  use super::*;
  use crate::core::ecs::{Bundle, ChildOf, Disabled, GlobalTransform, Transform, Velocity, World};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{CellShape, SlopeCorner};
  use crate::core::physics::{CharacterController, Collider, ColliderShape, Physics, RigidBody};

  /// The map's parts' colliders, in its space
  fn parts(world: &mut World, map: Entity) -> Vec<Collider> {
    parts_by_id(world, map)
      .into_iter()
      .map(|(_, collider)| collider)
      .collect()
  }

  #[test]
  fn a_part_spawned_the_step_its_map_moved_is_placed_where_the_map_is_now() {
    use super::hex::{HexCells, StaggerAxis, StaggerIndex};
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0)).with_hex(hex, Vec2::ZERO);
    tiles.set(IVec2::new(1, 1), TileCell::Solid);
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 2);
    let before = hex_parts(&mut world, map);
    // Moved, and a wall in a chunk of its own, in one update
    *world.get_mut::<Transform>(map).unwrap() =
      Transform::from_xy(100.0, 50.0).with_rotation_degrees(30.0);
    world
      .get_mut::<TileColliders>(map)
      .unwrap()
      .set(IVec2::new(60, 60), TileCell::Solid);
    run(&mut world, &mut s, 1);
    let after = hex_parts(&mut world, map);
    let fresh: Vec<Entity> = after
      .iter()
      .map(|&(_, part, ..)| part)
      .filter(|part| before.iter().all(|&(_, kept, ..)| kept != *part))
      .collect();
    assert!(!fresh.is_empty(), "the wall's part is new");
    let root = *world.get::<GlobalTransform>(map).unwrap();
    for part in after.iter().map(|&(_, part, ..)| part) {
      let local = *world.get::<Transform>(part).unwrap();
      let want = root.mul_transform(&local);
      let got = *world.get::<GlobalTransform>(part).unwrap();
      assert!(
        got.affine().abs_diff_eq(want.affine(), 1e-3),
        "{part}: {got:?} against {want:?}"
      );
    }
  }

  #[test]
  fn a_collider_grid_on_a_kinematic_body_keeps_its_parts_with_it() {
    let (mut world, mut s) = world(60);
    let map = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::kinematic(),
        Velocity::new(60.0, 0.0),
        TileColliders::from_cells(4, 1, Vec2::splat(16.0), vec![TileCell::Solid; 4]),
      ))
      .id();
    run(&mut world, &mut s, 60);
    let moved = at(&world, map).translation.x;
    assert!(moved > 30.0, "the body moved: {moved}");
    let root = *world.get::<GlobalTransform>(map).unwrap();
    let physics = world.resource::<Physics>();
    let mut checked = 0;
    for (part, entry) in physics.collider_entries.iter() {
      let global = *world.get::<GlobalTransform>(*part).unwrap();
      let local = *world.get::<Transform>(*part).unwrap();
      let want = root.mul_transform(&local);
      assert!(global.affine().abs_diff_eq(want.affine(), 1e-3));
      let body = physics.colliders.get(entry.handle).unwrap();
      let at = body.position().translation;
      // Within a step's travel (1 px): the globals follow the body's
      // write-back a step behind its colliders, as they did before
      assert!(
        (at.x - want.translation().x).abs() < 1.5 && (at.y - want.translation().y).abs() < 1.5,
        "{part}: the collider at {at:?}, the part at {:?}",
        want.translation()
      );
      checked += 1;
    }
    assert!(checked > 0);
  }

  #[test]
  fn a_grid_builds_its_colliders_and_rebuilds_when_a_cell_changes() {
    let (mut world, mut s) = world(60);
    let map = world
      .spawn((
        Transform::IDENTITY,
        TileColliders::from_cells(4, 1, Vec2::splat(16.0), vec![TileCell::Solid; 4]),
      ))
      .id();
    run(&mut world, &mut s, 1);
    assert_eq!(parts(&mut world, map).len(), 1);
    world
      .get_mut::<TileColliders>(map)
      .unwrap()
      .set(IVec2::new(1, 0), TileCell::Empty);
    run(&mut world, &mut s, 1);
    assert_eq!(
      loop_count(&mut world, map),
      2,
      "two outlines, the old one gone"
    );
    assert_eq!(parts(&mut world, map).len(), 1, "one chunk's collider");
  }

  #[test]
  fn one_way_cells_become_one_way_segments_and_rects_mode_makes_rects() {
    let (mut world, mut s) = world(60);
    let cells = vec![
      TileCell::OneWay,
      TileCell::OneWay,
      TileCell::Solid,
      TileCell::Solid,
    ];
    let map = world
      .spawn((
        Transform::IDENTITY,
        TileColliders::from_cells(2, 2, Vec2::splat(16.0), cells).solid_rects(),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let built = parts(&mut world, map);
    assert_eq!(built.len(), 2);
    assert!(
      built
        .iter()
        .any(|c| c.one_way.is_some() && matches!(c.shape, ColliderShape::Segment { .. }))
    );
    assert!(
      built
        .iter()
        .any(|c| matches!(c.shape, ColliderShape::Rect { .. }))
    );
  }

  // One-way runs in a chunk share a collider only along one row: the
  // one-way rule measures a collider as a whole, so a lower platform
  // packed with a higher one would let a character fall through it
  #[test]
  fn one_way_runs_share_a_collider_only_along_one_row() {
    use crate::core::physics::CharacterState;
    let (mut world, mut s) = world(60);
    let (w, h) = (20u32, 6u32);
    let mut cells = vec![TileCell::Empty; (w * h) as usize];
    for x in (0..4).chain(8..12) {
      cells[(2 * w + x) as usize] = TileCell::OneWay;
    }
    for x in 14..18 {
      cells[(4 * w + x) as usize] = TileCell::OneWay;
    }
    let map = world
      .spawn((
        Transform::IDENTITY,
        TileColliders::from_cells(w, h, Vec2::splat(16.0), cells),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let built = parts_by_id(&mut world, map);
    assert_eq!(built.len(), 2, "{built:?}");
    let row = |y: f32| {
      built
        .iter()
        .find(|(_, c)| {
          c.one_way == Some(Vec2::NEG_Y)
            && match &c.shape {
              ColliderShape::Segments { points, .. } => points.iter().all(|p| p.y == y),
              ColliderShape::Segment { a, b } => a.y == y && b.y == y,
              _ => false,
            }
        })
        .map(|(e, c)| (*e, c.shape.clone()))
        .unwrap_or_else(|| panic!("no one-way part along y {y}: {built:?}"))
    };
    let (upper, shape) = row(32.0);
    assert!(
      matches!(&shape, ColliderShape::Segments { segments, .. } if segments.len() == 2),
      "row 2's two runs are one collider: {shape:?}"
    );
    let (lower, _) = row(64.0);
    // A character dropped on each row's far platform lands on it
    let spawn = |world: &mut World, x: f32| {
      world
        .spawn((
          Transform::from_xy(x, -40.0),
          CharacterController::platformer(),
          Collider::rect(12.0, 40.0),
        ))
        .id()
    };
    let on_upper = spawn(&mut world, 152.0);
    let on_lower = spawn(&mut world, 248.0);
    for _ in 0..90 {
      for e in [on_upper, on_lower] {
        world.get_mut::<Velocity>(e).unwrap().dy += 980.0 / 60.0;
      }
      run(&mut world, &mut s, 1);
    }
    for (e, floor, top) in [(on_upper, upper, 32.0), (on_lower, lower, 64.0)] {
      let state = world.get::<CharacterState>(e).unwrap();
      let y = world.get::<Transform>(e).unwrap().translation.y;
      assert!(
        state.on_floor && state.floor == Some(floor),
        "on {floor}: on_floor {}, floor {:?}, y {y}",
        state.on_floor,
        state.floor
      );
      assert!((y + 20.0 - top).abs() < 1.0, "stands on y {top}: {y}");
    }
  }

  #[test]
  fn a_box_sliding_across_a_hundred_tiles_never_stops() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(0.0, 100.0),
      TileColliders::from_cells(100, 1, Vec2::splat(16.0), vec![TileCell::Solid; 100])
        .with_friction(0.0),
    ));
    let crate_ = world
      .spawn((
        Transform::from_xy(20.0, 94.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(12.0, 12.0).with_friction(0.0),
        Velocity::new(300.0, 0.0),
      ))
      .id();
    run(&mut world, &mut s, 240);
    let x = at(&world, crate_).translation.x;
    assert!(x - 20.0 >= 0.95 * 300.0 * 4.0, "snagged at {x}");
  }

  #[test]
  fn a_character_running_across_a_hundred_tiles_never_snags() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(0.0, 100.0),
      TileColliders::from_cells(100, 1, Vec2::splat(16.0), vec![TileCell::Solid; 100]),
    ));
    let e = world
      .spawn((
        Transform::from_xy(20.0, 79.0),
        CharacterController::platformer(),
        Collider::rect(12.0, 40.0),
      ))
      .id();
    run(&mut world, &mut s, 10);
    let x0 = at(&world, e).translation.x;
    for _ in 0..240 {
      let mut v = world.get_mut::<Velocity>(e).unwrap();
      v.dx = 300.0;
      v.dy += 980.0 / 60.0;
      run(&mut world, &mut s, 1);
    }
    let travelled = at(&world, e).translation.x - x0;
    assert!(travelled >= 0.99 * 300.0 * 4.0, "{travelled}");
  }

  fn standing_character(world: &mut World, x: f32) -> Entity {
    world
      .spawn((
        Transform::from_xy(x, 79.0),
        CharacterController::platformer(),
        Collider::rect(12.0, 40.0),
      ))
      .id()
  }

  fn fall_step(world: &mut World, s: &mut crate::core::schedule::EngineSchedules, e: Entity) {
    world.get_mut::<Velocity>(e).unwrap().dy += 980.0 / 60.0;
    run(world, s, 1);
  }

  #[test]
  fn a_character_keeps_standing_on_a_map_whose_outline_splits() {
    use crate::core::physics::CharacterState;
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::from_xy(0.0, 100.0), row(40))).id();
    let e = standing_character(&mut world, 40.0);
    for _ in 0..30 {
      fall_step(&mut world, &mut s, e);
    }
    assert!(world.get::<CharacterState>(e).unwrap().on_floor);
    world
      .get_mut::<TileColliders>(map)
      .unwrap()
      .set(IVec2::new(35, 0), TileCell::Empty);
    for step in 0..5 {
      fall_step(&mut world, &mut s, e);
      let state = world.get::<CharacterState>(e).unwrap();
      assert!(
        state.on_floor && !state.left_floor && !state.landed,
        "step {step} after the rebuild: on_floor {}, left_floor {}, landed {}",
        state.on_floor,
        state.left_floor,
        state.landed
      );
    }
    assert_eq!(ids(&mut world, map).len(), 2, "the outline split");
  }

  #[test]
  fn a_character_running_at_a_wall_on_a_map_rebuilt_every_step_stops_at_it() {
    use crate::core::physics::CharacterState;
    let (mut world, mut s) = world(60);
    // A floor 40 cells long with a wall two cells high on its cell 20
    let mut cells = vec![TileCell::Empty; 40 * 3];
    for x in 0..40 {
      cells[2 * 40 + x] = TileCell::Solid;
    }
    cells[20] = TileCell::Solid;
    cells[40 + 20] = TileCell::Solid;
    let map = world
      .spawn((
        Transform::from_xy(0.0, 68.0),
        TileColliders::from_cells(40, 3, Vec2::splat(16.0), cells),
      ))
      .id();
    let e = standing_character(&mut world, 40.0);
    for _ in 0..30 {
      fall_step(&mut world, &mut s, e);
    }
    assert!(world.get::<CharacterState>(e).unwrap().on_floor);
    for step in 0..90 {
      // A real change far from the character every step: every step rebuilds
      let cell = if step % 2 == 0 {
        TileCell::Solid
      } else {
        TileCell::Empty
      };
      world
        .get_mut::<TileColliders>(map)
        .unwrap()
        .set(IVec2::new(38, 0), cell);
      world.get_mut::<Velocity>(e).unwrap().dx = 600.0;
      fall_step(&mut world, &mut s, e);
      let state = world.get::<CharacterState>(e).unwrap();
      assert!(state.on_floor, "fell through the floor at step {step}");
    }
    let right = at(&world, e).translation.x + 6.0;
    assert!(
      right <= 320.5,
      "the wall at 320 stopped it: right side {right}"
    );
  }

  // As above, with the cell toggled on the floor's own outline (the floor
  // and the wall are one loop), so the loop under the character and the
  // wall it runs at are rebuilt every step
  #[test]
  fn a_character_running_at_a_wall_whose_outline_is_rebuilt_every_step_stops_at_it() {
    use crate::core::physics::CharacterState;
    let (mut world, mut s) = world(60);
    let mut cells = vec![TileCell::Empty; 40 * 3];
    for x in 0..40 {
      cells[2 * 40 + x] = TileCell::Solid;
    }
    cells[20] = TileCell::Solid;
    cells[40 + 20] = TileCell::Solid;
    let map = world
      .spawn((
        Transform::from_xy(0.0, 68.0),
        TileColliders::from_cells(40, 3, Vec2::splat(16.0), cells),
      ))
      .id();
    let e = standing_character(&mut world, 40.0);
    for _ in 0..30 {
      fall_step(&mut world, &mut s, e);
    }
    assert!(world.get::<CharacterState>(e).unwrap().on_floor);
    let mut floors = Vec::new();
    for step in 0..90 {
      let cell = if step % 2 == 0 {
        TileCell::Solid
      } else {
        TileCell::Empty
      };
      // On the floor, far from the character
      world
        .get_mut::<TileColliders>(map)
        .unwrap()
        .set(IVec2::new(38, 1), cell);
      world.get_mut::<Velocity>(e).unwrap().dx = 600.0;
      fall_step(&mut world, &mut s, e);
      let state = world.get::<CharacterState>(e).unwrap();
      assert!(state.on_floor, "fell through the floor at step {step}");
      let floor = state.floor.expect("a floor");
      floors.push(world.get::<Collider>(floor).map(|c| c.shape.clone()));
    }
    // Rebuilt in place: the floor's part takes each step's new outline
    floors.dedup();
    assert!(
      floors.len() > 80,
      "the floor was rebuilt every step: {} shapes",
      floors.len()
    );
    let right = at(&world, e).translation.x + 6.0;
    assert!(
      right <= 320.5,
      "the wall at 320 stopped it: right side {right}"
    );
  }

  /// One tile edit's rebuild on a 1024 x 1024 grid of 16,000 small islands
  /// and one loop across four 32-cell chunks: a jagged skyline 128 cells
  /// wide and 32 high, and a one-cell-wide serpentine filling the same
  /// chunks (the worst a loop that size can be). Prints the rebuild
  /// system's time (the retrace, the colliders and their commands applied),
  /// the outline retrace alone, and a whole fixed step with physics against
  /// one with no edit. Run in release:
  /// `cargo test --release -p ryo2d --lib one_tile_edit_timing -- --ignored --nocapture`
  #[test]
  #[ignore = "a timing: run in release with --ignored --nocapture"]
  fn one_tile_edit_timing() {
    let size = 1024;
    let islands = || {
      let mut tiles = TileColliders::new(size, size, Vec2::splat(16.0));
      for by in (0..size).step_by(8) {
        for bx in (0..size).step_by(8) {
          if bx < 136 && by < 40 {
            continue;
          }
          for (x, y) in (0..9).map(|i| (bx + i % 3, by + i / 3)) {
            tiles.set(xy(x, y), TileCell::Solid);
          }
        }
      }
      tiles
    };
    let mut skyline = islands();
    let mut seed: u32 = 7;
    let mut tops = Vec::new();
    for x in 0..128 {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      let top = 1 + (seed >> 16) % 24;
      tops.push(top);
      for y in top..32 {
        skyline.set(xy(x, y), TileCell::Solid);
      }
    }
    time_edit("a skyline", skyline, (64, tops[64] - 1));
    // Every other row across from row 1, joined at alternate ends; the edit
    // is a bump on its top
    let mut serpentine = islands();
    for y in (1..32).step_by(2) {
      for x in 0..128 {
        serpentine.set(xy(x, y), TileCell::Solid);
      }
      if y < 31 {
        let end = if y % 4 == 1 { 127 } else { 0 };
        serpentine.set(xy(end, y + 1), TileCell::Solid);
      }
    }
    time_edit("a serpentine", serpentine, (64, 0));
  }

  fn time_edit(name: &str, tiles: TileColliders, (column, row): (u32, u32)) {
    use crate::core::ecs::Schedule;
    use std::time::{Duration, Instant};
    let built = Outlines::build(tiles.cells_view());
    let longest = built
      .loops()
      .map(|(_, outline)| outline.corners.len() - 1)
      .max()
      .unwrap_or(0);
    let median = |mut times: Vec<Duration>| {
      times.sort();
      (times[times.len() / 2], times[times.len() - 1])
    };
    let cell = |i: u32| {
      if i.is_multiple_of(2) {
        TileCell::Solid
      } else {
        TileCell::Empty
      }
    };
    let edit = |tiles: &mut TileColliders, i: u32| {
      assert!(tiles.set(xy(column, row), cell(i)));
    };

    // The rebuild system alone
    let mut world = World::new();
    world.insert_resource(Physics::new());
    let map = world.spawn((Transform::IDENTITY, tiles.clone())).id();
    let mut schedule = Schedule::default();
    schedule.add_systems(rebuild_tile_colliders);
    let start = Instant::now();
    schedule.run(&mut world);
    let first = start.elapsed();
    let mut times = Vec::new();
    for i in 0..400 {
      edit(&mut world.get_mut::<TileColliders>(map).unwrap(), i);
      let start = Instant::now();
      schedule.run(&mut world);
      times.push(start.elapsed());
    }
    let (system, system_max) = median(times);

    // The outline retrace alone
    let mut outlines = built;
    let mut grid = tiles.clone();
    let mut times = Vec::new();
    for i in 0..400 {
      edit(&mut grid, i);
      let changed = grid.take_changes().unwrap_or_default();
      let start = Instant::now();
      let retraced = outlines.apply(grid.cells_view(), &changed);
      times.push(start.elapsed());
      assert_eq!(retraced.traced.len(), 1);
    }
    let (retrace, retrace_max) = median(times);

    // A whole fixed step with physics, with and without an edit
    let (mut world, mut s) = super::super::test_util::world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 2);
    let mut still = Vec::new();
    let mut edited = Vec::new();
    for i in 0..200 {
      let start = Instant::now();
      run(&mut world, &mut s, 1);
      still.push(start.elapsed());
      edit(&mut world.get_mut::<TileColliders>(map).unwrap(), i);
      let start = Instant::now();
      run(&mut world, &mut s, 1);
      edited.push(start.elapsed());
    }
    let (still, _) = median(still);
    let (edited, edited_max) = median(edited);
    println!(
      "{name}: 1024 x 1024 grid, {} parts, the edited loop {longest} edges long\n  \
       first build: {first:?}\n  \
       one-tile edit, rebuild system: median {system:?}, max {system_max:?}\n  \
       one-tile edit, outline retrace alone: median {retrace:?}, max {retrace_max:?}\n  \
       fixed step with physics: {still:?} with no edit, {edited:?} with one (max {edited_max:?})",
      parts(&mut world, map).len()
    );
  }

  /// What a stream's chunk costs the rebuild: an unbounded grid of cave
  /// rock from value noise over 12 x 10 chunks (about a quarter of its
  /// cells solid, in islands and caves), a chunk unloaded whole
  /// (`unload_chunk`) and loaded back cell by cell (`set`, as a map's
  /// collision sync lands it), each rebuilt in the rebuild system and in the
  /// outline retrace alone. Run in release:
  /// `cargo test --release -p ryo2d --lib chunk_swap_timing -- --ignored --nocapture`
  #[test]
  #[ignore = "a timing: run in release with --ignored --nocapture"]
  fn chunk_swap_timing() {
    use crate::core::ecs::Schedule;
    use std::time::{Duration, Instant};
    let hash = |x: i32, y: i32, salt: u32| {
      let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ salt.wrapping_mul(0xCB1A_B31F);
      h ^= h >> 13;
      h = h.wrapping_mul(0x5BD1_E995);
      h ^ (h >> 15)
    };
    let noise = |x: f32, y: f32, salt: u32| {
      let (x0, y0) = (x.floor(), y.floor());
      let (fx, fy) = (x - x0, y - y0);
      let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
      let (ix, iy) = (x0 as i32, y0 as i32);
      let at = |dx: i32, dy: i32| (hash(ix + dx, iy + dy, salt) & 0xFFFF) as f32 / 65535.0;
      let top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
      let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
      top + (bottom - top) * sy
    };
    let rock = |cell: IVec2| {
      let (x, y) = (cell.x as f32, cell.y as f32);
      noise(x / 22.0, y / 22.0, 1) * 0.7 + noise(x / 7.0, y / 7.0, 2) * 0.3 > 0.6
    };
    let side = CHUNK;
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0));
    let mut solid = 0;
    for y in -5 * side..5 * side {
      for x in -6 * side..6 * side {
        if rock(IVec2::new(x, y)) {
          tiles.set(IVec2::new(x, y), TileCell::Solid);
          solid += 1;
        }
      }
    }
    // Built whole: the cells set so far are not an edit
    tiles.take_changes();
    let median = |mut times: Vec<Duration>| {
      times.sort();
      (times[times.len() / 2], times[times.len() - 1])
    };
    // The chunks swapped: one inside, one on the edge, in turn
    let chunks = [IVec2::new(0, 0), IVec2::new(-1, 2), IVec2::new(5, -1)];
    let load = |tiles: &mut TileColliders, chunk: IVec2| {
      let origin = chunk * side;
      for y in 0..side {
        for x in 0..side {
          let cell = origin + IVec2::new(x, y);
          if rock(cell) {
            tiles.set(cell, TileCell::Solid);
          }
        }
      }
    };
    // The rebuild system alone
    let mut world = World::new();
    world.insert_resource(Physics::new());
    let map = world.spawn((Transform::IDENTITY, tiles.clone())).id();
    let mut schedule = Schedule::default();
    schedule.add_systems(rebuild_tile_colliders);
    schedule.run(&mut world);
    let (mut unloads, mut loads) = (Vec::new(), Vec::new());
    for i in 0..300 {
      let chunk = chunks[i % chunks.len()];
      world
        .get_mut::<TileColliders>(map)
        .unwrap()
        .unload_chunk(chunk);
      let start = Instant::now();
      schedule.run(&mut world);
      unloads.push(start.elapsed());
      load(&mut world.get_mut::<TileColliders>(map).unwrap(), chunk);
      let start = Instant::now();
      schedule.run(&mut world);
      loads.push(start.elapsed());
    }
    // A stream's step: a chunk unloaded behind and one loaded ahead, ten
    // chunks apart, in one rebuild (then put back as they were)
    let (behind, ahead) = (IVec2::new(-5, -3), IVec2::new(4, 2));
    let mut both = Vec::new();
    for _ in 0..300 {
      let mut grid = world.get_mut::<TileColliders>(map).unwrap();
      grid.unload_chunk(ahead);
      schedule.run(&mut world);
      let mut grid = world.get_mut::<TileColliders>(map).unwrap();
      grid.unload_chunk(behind);
      load(&mut grid, ahead);
      let start = Instant::now();
      schedule.run(&mut world);
      both.push(start.elapsed());
      load(&mut world.get_mut::<TileColliders>(map).unwrap(), behind);
      schedule.run(&mut world);
    }
    // The outline retrace alone
    let mut outlines = Outlines::build(tiles.cells_view());
    let (mut unload_traces, mut load_traces) = (Vec::new(), Vec::new());
    for i in 0..300 {
      let chunk = chunks[i % chunks.len()];
      tiles.unload_chunk(chunk);
      let changed = tiles.take_changes().unwrap_or_default();
      let start = Instant::now();
      outlines.apply(tiles.cells_view(), &changed);
      unload_traces.push(start.elapsed());
      load(&mut tiles, chunk);
      let changed = tiles.take_changes().unwrap_or_default();
      let start = Instant::now();
      outlines.apply(tiles.cells_view(), &changed);
      load_traces.push(start.elapsed());
    }
    // A chunk's shadow boundary, as a map's shadows read it a chunk at a
    // time: one of the rock, and the empty one right of the grid, whose
    // only edges close the rock at its left border
    let boundary = Boundary::default();
    let at = |x: i32, y: i32| tiles.get(IVec2::new(x, y));
    let mut edges = Vec::new();
    let mut shadow = |chunk: IVec2| {
      let (min, max) = (chunk * side, (chunk + IVec2::ONE) * side);
      // Read from an array, as a map's shadows read their chunk's kinds:
      // the chunk, the column left of it and the row above
      let wide = side + 1;
      let window: Vec<TileCell> = (0..wide * wide)
        .map(|i| at(min.x - 1 + i % wide, min.y - 1 + i / wide))
        .collect();
      let read = |x: i32, y: i32| {
        let (dx, dy) = (x - min.x + 1, y - min.y + 1);
        if (0..wide).contains(&dx) && (0..wide).contains(&dy) {
          window[(dy * wide + dx) as usize]
        } else {
          TileCell::Empty
        }
      };
      let mut times = Vec::new();
      for _ in 0..2000 {
        edges.clear();
        let start = Instant::now();
        boundary.edges(read, (min.x, min.y), (max.x, max.y), None, &mut edges);
        times.push(start.elapsed());
      }
      (median(times).0, edges.len())
    };
    // The chunk of the most edges
    let densest = (-5..5)
      .flat_map(|y| (-6..6).map(move |x| IVec2::new(x, y)))
      .max_by_key(|&chunk| {
        let mut edges = Vec::new();
        let (min, max) = (chunk * side, (chunk + IVec2::ONE) * side);
        boundary.edges(at, (min.x, min.y), (max.x, max.y), None, &mut edges);
        edges.len()
      })
      .unwrap_or_default();
    let ((rock, rock_edges), (open, open_edges)) = (shadow(densest), shadow(IVec2::new(6, 0)));
    let ((unload, unload_max), (load, load_max)) = (median(unloads), median(loads));
    let (both, both_max) = median(both);
    let ((unload_trace, _), (load_trace, _)) = (median(unload_traces), median(load_traces));
    println!(
      "12 x 10 chunks of noise rock, {solid} solid cells ({} a chunk), {} parts\n  \
       a chunk unloaded, rebuild system: median {unload:?}, max {unload_max:?}; the retrace \
       alone {unload_trace:?}\n  \
       a chunk loaded back, rebuild system: median {load:?}, max {load_max:?}; the retrace \
       alone {load_trace:?}\n  \
       one unloaded and one loaded ten chunks apart, rebuild system: median {both:?}, max \
       {both_max:?}\n  \
       a chunk's shadow boundary: the rock's busiest chunk {rock:?} ({rock_edges} edges), the \
       empty chunk beside the rock {open:?} ({open_edges} edges)",
      solid / 120,
      parts(&mut world, map).len()
    );
  }

  fn ids(world: &mut World, map: Entity) -> Vec<Entity> {
    let mut query = world.query::<(Entity, &ChildOf, &Collider)>();
    let mut found: Vec<Entity> = query
      .iter(world)
      .filter(|(_, child_of, _)| child_of.parent() == map)
      .map(|(e, _, _)| e)
      .collect();
    found.sort();
    found
  }

  fn row(n: usize) -> TileColliders {
    TileColliders::from_cells(n as u32, 1, Vec2::splat(16.0), vec![TileCell::Solid; n])
  }

  #[test]
  fn a_bad_tile_size_builds_nothing_and_warns_once() {
    let (mut world, mut s) = world(60);
    let map = world
      .spawn((
        Transform::IDENTITY,
        TileColliders::from_cells(1, 1, Vec2::ZERO, vec![TileCell::Solid]),
      ))
      .id();
    run(&mut world, &mut s, 5);
    assert!(parts(&mut world, map).is_empty());
    assert_eq!(world.resource::<Physics>().warnings, 1);
  }

  #[test]
  fn a_size_made_bad_after_a_build_removes_the_parts_and_warns_once() {
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, row(3))).id();
    run(&mut world, &mut s, 1);
    assert_eq!(parts(&mut world, map).len(), 1);
    world.get_mut::<TileColliders>(map).unwrap().tile_size = Vec2::new(f32::NAN, 16.0);
    run(&mut world, &mut s, 5);
    assert!(parts(&mut world, map).is_empty());
    assert_eq!(world.resource::<Physics>().warnings, 1);
  }

  #[test]
  fn a_map_spawned_disabled_gets_colliders_when_enabled() {
    let (mut world, mut s) = world(60);
    let map = world.spawn((Disabled, Transform::IDENTITY, row(3))).id();
    run(&mut world, &mut s, 2);
    assert!(ids(&mut world, map).is_empty());
    world.entity_mut(map).remove::<Disabled>();
    run(&mut world, &mut s, 1);
    assert_eq!(ids(&mut world, map).len(), 1);
  }

  #[test]
  fn a_set_while_disabled_applies_on_enable() {
    let (mut world, mut s) = world(60);
    let map = world.spawn((Disabled, Transform::IDENTITY, row(4))).id();
    run(&mut world, &mut s, 1);
    let mut entity = world.entity_mut(map);
    let mut tiles = entity.get_mut::<TileColliders>().unwrap();
    tiles.set(IVec2::new(1, 0), TileCell::Empty);
    entity.remove::<Disabled>();
    run(&mut world, &mut s, 1);
    assert_eq!(loop_count(&mut world, map), 2);
  }

  #[test]
  fn disabling_a_map_drops_what_rests_on_it() {
    let (mut world, mut s) = world(60);
    let map = world
      .spawn((
        Transform::from_xy(0.0, 100.0),
        TileColliders::from_cells(10, 1, Vec2::splat(16.0), vec![TileCell::Solid; 10]),
      ))
      .id();
    let ball = ball(&mut world, 40.0, 85.0);
    run(&mut world, &mut s, 60);
    assert!(at(&world, ball).translation.y < 100.0);
    world.entity_mut(map).insert(Disabled);
    run(&mut world, &mut s, 60);
    assert!(
      at(&world, ball).translation.y > 150.0,
      "{}",
      at(&world, ball).translation.y
    );
    assert!(ids(&mut world, map).is_empty());
  }

  #[test]
  fn removing_the_component_removes_its_parts() {
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, row(3))).id();
    run(&mut world, &mut s, 1);
    assert_eq!(ids(&mut world, map).len(), 1);
    world.entity_mut(map).remove::<TileColliders>();
    run(&mut world, &mut s, 1);
    assert!(ids(&mut world, map).is_empty());
  }

  #[test]
  fn a_part_moved_off_its_map_or_left_by_its_build_is_despawned() {
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, row(3))).id();
    let other = world.spawn(Transform::IDENTITY).id();
    run(&mut world, &mut s, 1);
    let part = ids(&mut world, map)[0];
    world.entity_mut(part).insert(ChildOf(other));
    run(&mut world, &mut s, 1);
    assert!(
      world.get_entity(part).is_err(),
      "moved onto something not a map"
    );
    // A map that stops being one (its component and build gone) drops its parts
    let map = world.spawn((Transform::IDENTITY, row(3))).id();
    // Two steps, so its parts are not new when it goes
    run(&mut world, &mut s, 2);
    assert_eq!(ids(&mut world, map).len(), 1);
    world.entity_mut(map).remove::<(TileColliders, TileBuild)>();
    run(&mut world, &mut s, 1);
    assert!(ids(&mut world, map).is_empty());
  }

  #[test]
  fn only_a_real_change_rebuilds() {
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, row(3))).id();
    run(&mut world, &mut s, 1);
    let before = ids(&mut world, map);
    assert!(
      !world
        .get_mut::<TileColliders>(map)
        .unwrap()
        .set(IVec2::new(0, 0), TileCell::Solid)
    );
    assert!(
      !world
        .get_mut::<TileColliders>(map)
        .unwrap()
        .set(IVec2::new(99, 0), TileCell::Solid)
    );
    run(&mut world, &mut s, 2);
    assert_eq!(ids(&mut world, map), before);
    world.get_mut::<TileColliders>(map).unwrap().layer = 4;
    run(&mut world, &mut s, 1);
    let after = ids(&mut world, map);
    assert_eq!(after.len(), 1);
    assert_ne!(after, before);
  }

  #[test]
  fn a_tile_grid_s_floor_goes_on_every_part() {
    let floors = |world: &mut World, map: Entity| {
      let mut query = world.query_filtered::<(&ChildOf, Option<&Floor>), With<TilePart>>();
      query
        .iter(world)
        .filter(|(child_of, _)| child_of.parent() == map)
        .map(|(_, floor)| floor.copied())
        .collect::<Vec<_>>()
    };
    // Outlines and one-way runs in packs, and rectangles
    for grid in [islands(), islands().solid_rects()] {
      let (mut world, mut s) = world(60);
      assert_eq!(grid.floor, Floor::GROUND, "the ground floor by default");
      let map = world.spawn((Transform::IDENTITY, grid)).id();
      run(&mut world, &mut s, 1);
      let mut before = ids(&mut world, map);
      let on = floors(&mut world, map);
      assert!(
        on.len() > 1 && on.iter().all(Option::is_none),
        "ground parts carry no Floor: {on:?}"
      );
      // Off the ground floor, to another floor and back
      for (floor, want) in [
        (Floor::On(2), Some(Floor::On(2))),
        (Floor::On(3), Some(Floor::On(3))),
        (Floor::GROUND, None),
      ] {
        world.get_mut::<TileColliders>(map).unwrap().floor = floor;
        run(&mut world, &mut s, 1);
        let after = ids(&mut world, map);
        assert!(
          after.len() == before.len() && after.iter().all(|e| !before.contains(e)),
          "rebuilt for {floor:?}"
        );
        let on = floors(&mut world, map);
        assert!(on.iter().all(|&f| f == want), "{floor:?}: {on:?}");
        before = after;
      }
    }
  }

  #[test]
  fn a_map_s_own_floor_never_reaches_its_ground_parts() {
    // A ground floor grid topped at 100 on a kinematic map body on floor 1
    let (mut world, mut s) = world(60);
    let map = world
      .spawn((
        Transform::from_xy(-200.0, 100.0),
        RigidBody::kinematic(),
        Floor::On(1),
        row(25),
      ))
      .id();
    let b = ball(&mut world, 0.0, 50.0);
    run(&mut world, &mut s, 120);
    let y = at(&world, b).translation.y;
    assert!(
      (y - 90.0).abs() < 1.0,
      "a ground floor ball rests on it: {y}"
    );
    let parts = ids(&mut world, map);
    assert!(!parts.is_empty());
    let physics = world.resource::<Physics>();
    for part in parts {
      assert!(
        world.get::<Floor>(part).is_none(),
        "{part} carries no Floor"
      );
      assert_eq!(physics.collider_entries[&part].floor, Floor::GROUND);
    }
  }

  #[test]
  fn a_grid_on_another_floor_lets_a_ground_floor_ball_fall_through() {
    // A grid on floor 1 topped at 100, under balls on the ground floor and
    // on floor 1
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(-200.0, 100.0),
      row(25).with_floor(Floor::On(1)),
    ));
    let ground = ball(&mut world, -50.0, 50.0);
    let upper = ball(&mut world, 50.0, 50.0);
    world.entity_mut(upper).insert(Floor::On(1));
    run(&mut world, &mut s, 120);
    let y = at(&world, ground).translation.y;
    assert!(y > 150.0, "the ground floor ball fell through: {y}");
    let y = at(&world, upper).translation.y;
    assert!((y - 90.0).abs() < 1.0, "the floor 1 ball rests on it: {y}");
  }
  /// 600 islands and a one-way run, as a grid
  fn islands() -> TileColliders {
    let (w, h) = (120u32, 40u32);
    let mut cells = vec![TileCell::Empty; (w * h) as usize];
    for y in (0..h).step_by(2) {
      for x in (0..w).step_by(4) {
        cells[(y * w + x) as usize] = TileCell::Solid;
      }
    }
    for x in 0..w {
      cells[((h - 1) * w + x) as usize] = TileCell::OneWay;
    }
    TileColliders::from_cells(w, h, Vec2::splat(16.0), cells)
  }

  #[test]
  fn a_traced_grid_builds_from_its_trace_as_a_whole_build_would() {
    let grid = islands();
    let (mut whole, mut s1) = world(60);
    let reference = whole.spawn((Transform::IDENTITY, grid.clone())).id();
    run(&mut whole, &mut s1, 1);
    let expected = parts_by_id(&mut whole, reference);
    let (mut world, mut s) = world(60);
    let traced = grid.trace().expect("outline mode traces");
    let map = world.spawn((Transform::IDENTITY, grid, traced)).id();
    run(&mut world, &mut s, 1);
    let built = parts_by_id(&mut world, map);
    assert_eq!(built.len(), expected.len(), "every part in the first step");
    assert_eq!(
      canonical(outline_loops(&built, 16.0)),
      canonical(outline_loops(&expected, 16.0))
    );
    assert!(world.get::<Traced>(map).is_none(), "the trace was taken");
  }

  #[test]
  fn a_grid_changed_after_its_trace_builds_from_its_cells() {
    let grid = islands();
    let traced = grid.trace().expect("outline mode traces");
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, grid, traced)).id();
    // Islands joined and one cleared before the first build: the trace is
    // stale
    {
      let mut tiles = world.get_mut::<TileColliders>(map).unwrap();
      tiles.set(IVec2::new(1, 0), TileCell::Solid);
      tiles.set(IVec2::new(2, 0), TileCell::Solid);
      tiles.set(IVec2::new(3, 0), TileCell::Solid);
      tiles.set(IVec2::new(116, 36), TileCell::Empty);
    }
    run(&mut world, &mut s, 1);
    assert!(world.get::<Traced>(map).is_none(), "the stale trace went");
    let built = canonical(outline_loops(&parts_by_id(&mut world, map), 16.0));
    let full = canonical(traced_outlines(
      world.get::<TileColliders>(map).unwrap().cells_view(),
    ));
    assert_eq!(built, full, "built from the cells as they are");
  }

  /// The chunk (32 cells a side) holding a point, in tiles
  fn chunk_of((x, y): Corner) -> Corner {
    (x.div_euclid(32), y.div_euclid(32))
  }

  /// Every outline part holds loops starting in one chunk, and no chunk
  /// has two outline parts
  fn assert_one_outline_part_per_chunk(parts: &[(Entity, Collider)], tile: f32) {
    let mut seen = std::collections::BTreeSet::new();
    for (part, collider) in parts {
      if collider.one_way.is_some() {
        continue;
      }
      let chunks: std::collections::BTreeSet<Corner> = collider_loops(collider, tile)
        .iter()
        .filter_map(|l| l.first().copied())
        .map(chunk_of)
        .collect();
      assert!(
        chunks.len() <= 1,
        "{part:?} holds loops of chunks {chunks:?}"
      );
      for chunk in chunks {
        assert!(seen.insert(chunk), "chunk {chunk:?} has two outline parts");
      }
    }
  }

  #[test]
  fn edits_moving_or_joining_loops_across_chunks_keep_one_copy_of_each() {
    // Islands both sides of the chunk border at x 32, and a one-way row
    let (w, h) = (64u32, 12u32);
    let mut cells = vec![TileCell::Empty; (w * h) as usize];
    for y in (1..h - 2).step_by(3) {
      for x in (26..40).step_by(3) {
        cells[(y * w + x) as usize] = TileCell::Solid;
      }
    }
    for x in 20..44 {
      cells[((h - 1) * w + x) as usize] = TileCell::OneWay;
    }
    let (mut world, mut s) = world(60);
    let map = world
      .spawn((
        Transform::IDENTITY,
        TileColliders::from_cells(w, h, Vec2::splat(16.0), cells),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let check = |world: &mut World, step: &str| {
      let parts = parts_by_id(world, map);
      assert_one_outline_part_per_chunk(&parts, 16.0);
      let loops: Vec<(Entity, Collider)> = parts
        .into_iter()
        .filter(|(_, c)| c.one_way.is_none())
        .collect();
      let built = canonical(outline_loops(&loops, 16.0));
      let full = canonical(traced_outlines(
        world.get::<TileColliders>(map).unwrap().cells_view(),
      ));
      assert_eq!(
        built, full,
        "{step}: each loop once, as a whole build has them"
      );
    };
    let mut set = |world: &mut World, cells: &[(u32, u32)], kind: TileCell| {
      let mut tiles = world.get_mut::<TileColliders>(map).unwrap();
      for &(x, y) in cells {
        tiles.set(xy(x, y), kind);
      }
      run(world, &mut s, 1);
    };
    // The islands at 29 and 32 on row 1 join across the border: the joined
    // loop starts in chunk 0, and chunk 1 loses the island at 32
    set(&mut world, &[(30, 1), (31, 1)], TileCell::Solid);
    check(&mut world, "joined across the border");
    // Its left end cleared, the loop's first corner moves into chunk 1
    set(&mut world, &[(29, 1), (30, 1), (31, 1)], TileCell::Empty);
    check(&mut world, "first corner moved over the border");
    // Grow, join, split and clear around the border
    let mut seed = 0x9e37_79b9_u32;
    for step in 0..300 {
      seed ^= seed << 13;
      seed ^= seed >> 17;
      seed ^= seed << 5;
      let x = 24 + seed % 16;
      let y = (seed >> 8) % (h - 1);
      let kind = match (seed >> 16) % 4 {
        0 => TileCell::Empty,
        1 => TileCell::OneWay,
        _ => TileCell::Solid,
      };
      set(&mut world, &[(x, y)], kind);
      check(&mut world, &format!("step {step}"));
    }
  }

  #[test]
  fn a_huge_grid_with_few_cells_costs_nothing() {
    use super::tests::rects;
    use TileCell::Solid as S;
    let cells = [S; 4];
    for (w, h) in [
      (u32::MAX, 1),
      (4, u32::MAX),
      (u32::MAX, u32::MAX),
      (0, u32::MAX),
    ] {
      let _ = outlines(w, h, &cells);
      let _ = one_way_runs(w, h, &cells);
      let _ = rects(w, h, &cells);
    }
    assert_eq!(rects(4, u32::MAX, &cells), [((0, 0), (4, 1))]);
    assert_eq!(rects(u32::MAX, 1, &cells), [((0, 0), (4, 1))]);
    assert_eq!(outlines(4, u32::MAX, &cells).len(), 1);
    assert_eq!(outlines(u32::MAX, 1, &cells).len(), 1);
    // Its bounds reach as far as an i32 does; its cells are one chunk
    let grid = TileColliders::from_cells(u32::MAX, u32::MAX, Vec2::ONE, vec![S; 4]);
    assert_eq!(grid.bounds(), Some(CellRect::from_size(UVec2::MAX)));
    assert_eq!(grid.get(IVec2::new(3, 0)), S);
    assert_eq!(grid.get(IVec2::new(4, 0)), TileCell::Empty);
    assert_eq!(grid.chunks.len(), 1);
  }

  #[test]
  fn an_unbounded_grid_collides_at_negative_cells() {
    let (mut world, mut s) = world(60);
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0));
    // A 5-cell floor at y = -3, x = -40 to -36
    for x in -40..=-36 {
      assert!(tiles.set(IVec2::new(x, -3), TileCell::Solid));
    }
    assert_eq!(tiles.bounds(), None);
    assert_eq!(
      tiles.extent(),
      CellRect::new(IVec2::new(-64, -32), IVec2::new(-32, 0))
    );
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let bounds = part_bounds(&mut world, map);
    assert_eq!(bounds.len(), 1, "one part: {bounds:?}");
    let (min, max) = bounds[0];
    assert!(min.abs_diff_eq(Vec2::new(-640.0, -48.0), 1e-3), "{min}");
    assert!(max.abs_diff_eq(Vec2::new(-560.0, -32.0), 1e-3), "{max}");
    // And it holds a ball dropped onto it
    let ball = ball(&mut world, -600.0, -100.0);
    run(&mut world, &mut s, 90);
    let y = at(&world, ball).translation.y;
    assert!((y - (-58.0)).abs() < 1.0, "rests on the floor: {y}");
  }

  #[test]
  fn a_grid_wider_than_8192_cells_collides() {
    // 6a refused it: past 8192 cells a side
    let (w, h) = (10_000_u32, 64_u32);
    let mut cells = vec![TileCell::Empty; (w * h) as usize];
    cells[((h - 1) * w) as usize..].fill(TileCell::Solid);
    let tiles = TileColliders::from_cells(w, h, Vec2::splat(16.0), cells);
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let (min, max) = union(&part_bounds(&mut world, map));
    assert!(min.abs_diff_eq(Vec2::new(0.0, 1008.0), 1e-3), "{min}");
    assert!(max.abs_diff_eq(Vec2::new(160_000.0, 1024.0), 1e-3), "{max}");
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("10000 x 64"))
      .count();
    assert_eq!(warned, 0, "no warning");
    assert_eq!(world.resource::<Physics>().warnings, 0);
    // A ball dropped near its far end lands on it
    let ball = ball(&mut world, 150_000.0, 950.0);
    run(&mut world, &mut s, 90);
    let y = at(&world, ball).translation.y;
    assert!((y - 998.0).abs() < 1.0, "rests on the floor: {y}");
  }

  #[test]
  fn a_cell_past_the_lattice_s_reach_draws_but_does_not_collide() {
    use crate::core::dev_log::{LogEntry, get_entries};
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0));
    let past = IVec2::new(MAX_REACH + 1, 0);
    let others = [IVec2::new(-MAX_REACH - 1, 7), IVec2::new(3, i32::MIN)];
    assert!(!tiles.set(past, TileCell::Solid));
    assert!(!tiles.set(past, TileCell::Solid), "again");
    for cell in others {
      assert!(!tiles.set(cell, TileCell::Solid), "{cell}");
    }
    assert_eq!(tiles.get(past), TileCell::Empty);
    assert_eq!(tiles.extent(), CellRect::default(), "no chunk made");
    let named = |entry: &LogEntry| {
      entry.message.contains("past the reach")
        && [past, others[0], others[1]]
          .iter()
          .any(|c| entry.message.contains(&format!("({}, {})", c.x, c.y)))
    };
    assert_eq!(get_entries().iter().filter(|e| named(e)).count(), 1);
    // The farthest cells that collide build, and edit, without overflowing
    let near = IVec2::new(MAX_REACH - 1, 0);
    let corner = IVec2::splat(-MAX_REACH);
    assert!(tiles.set(near, TileCell::Solid));
    assert!(tiles.set(corner, TileCell::Solid));
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    assert_eq!(part_bounds(&mut world, map).len(), 2);
    let last = IVec2::new(MAX_REACH, 0);
    assert!(
      world
        .get_mut::<TileColliders>(map)
        .unwrap()
        .set(last, TileCell::Solid)
    );
    run(&mut world, &mut s, 1);
    let mut bounds = part_bounds(&mut world, map);
    bounds.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
    let px = |cell: i32| cell as f32 * 16.0;
    let (min, max) = bounds[1];
    assert!(
      min.abs_diff_eq(Vec2::new(px(MAX_REACH - 1), 0.0), 0.5),
      "{min}"
    );
    assert!(
      max.abs_diff_eq(Vec2::new(px(MAX_REACH + 1), 16.0), 0.5),
      "{max}"
    );
    let (min, _) = bounds[0];
    assert!(min.abs_diff_eq(Vec2::splat(px(-MAX_REACH)), 0.5), "{min}");
  }

  #[test]
  fn parts_sit_at_their_chunk_origin_with_chunk_relative_vertices() {
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0));
    // In chunk (-32, 62), 24 cells right of its left and 16 below its top
    assert!(tiles.set(IVec2::new(-1000, 2000), TileCell::Solid));
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let mut query = world.query::<(&ChildOf, &Transform, &Collider)>();
    let parts: Vec<(Transform, Collider)> = query
      .iter(&world)
      .filter(|(child_of, ..)| child_of.parent() == map)
      .map(|(_, at, collider)| (*at, collider.clone()))
      .collect();
    assert_eq!(parts.len(), 1);
    let (at, collider) = &parts[0];
    assert_eq!(at.translation, Vec2::new(-1024.0 * 16.0, 1984.0 * 16.0));
    let ColliderShape::Polyline { points } = &collider.shape else {
      panic!("one loop is a closed polyline: {collider:?}");
    };
    assert!(
      points.iter().all(|p| p.abs().max_element() <= 32.0 * 16.0),
      "{points:?}"
    );
    let mut corners: Vec<(i32, i32)> = points.iter().map(|p| (p.x as i32, p.y as i32)).collect();
    corners.sort_unstable();
    corners.dedup();
    assert_eq!(corners, [(384, 256), (384, 272), (400, 256), (400, 272)]);
  }

  /// A hexagonal map's parts by key, each with its entity, the Rapier shape
  /// physics holds for it (by address) and the tick its `Collider` was
  /// last written at
  fn hex_parts(world: &mut World, map: Entity) -> Vec<(PackKey, Entity, usize, u32)> {
    let keys: Vec<(PackKey, Entity)> = world
      .get::<TileBuild>(map)
      .and_then(|build| build.kept.as_ref())
      .map(|kept| kept.hex.parts().collect())
      .unwrap_or_default();
    let physics = world.resource::<Physics>();
    keys
      .into_iter()
      .map(|(key, part)| {
        let shape = physics
          .collider_entries
          .get(&part)
          .and_then(|entry| physics.colliders.get(entry.handle))
          .map_or(0, |collider| {
            std::sync::Arc::as_ptr(&collider.shared_shape().0) as *const () as usize
          });
        let written = world
          .get_entity(part)
          .ok()
          .and_then(|entity| entity.get_ref::<Collider>())
          .map_or(0, |collider| {
            crate::core::ecs::DetectChanges::last_changed(&collider).get()
          });
        (key, part, shape, written)
      })
      .collect()
  }

  /// The chunk a hexagonal part belongs to, and the blocks of 8 x 8 cells
  /// its box covers (by index row by row over the chunk)
  fn hex_box(key: PackKey) -> ((i32, i32), Vec<u8>) {
    use super::pack::HexBox;
    let PackKey::Hex { chunk, part, .. } = key else {
      panic!("a hexagonal part: {key:?}");
    };
    let blocks = match part {
      HexBox::Chunk => (0..16).collect(),
      HexBox::Quarter(q) => {
        let first = q / 2 * 8 + q % 2 * 2;
        vec![first, first + 1, first + 4, first + 5]
      }
      HexBox::Block(b) => vec![b],
      HexBox::Sub(s) => vec![s / 16 * 4 + s % 8 / 2],
    };
    (chunk, blocks)
  }

  /// A hexagonal map's parts' edges in the grid's points, by chunk, each
  /// chunk's sorted (a multiset: an edge twice shows)
  fn hex_edges_by_chunk(
    parts: impl IntoIterator<Item = (PackKey, Vec<(Point, Point)>)>,
  ) -> std::collections::BTreeMap<(i32, i32), Vec<(Point, Point)>> {
    let mut by_chunk: std::collections::BTreeMap<_, Vec<_>> = Default::default();
    for (key, edges) in parts {
      by_chunk.entry(hex_box(key).0).or_default().extend(edges);
    }
    for edges in by_chunk.values_mut() {
      edges.sort_unstable();
    }
    by_chunk
  }

  /// The chunk and block (8 x 8 cells, by index) of the cell owning each of
  /// `cell`'s sides: it, or the cell across where that comes first in row
  /// order
  fn hex_owners(hex: super::hex::HexCells, cell: IVec2) -> Vec<((i32, i32), u8)> {
    (0..6)
      .map(|side| {
        let across = hex.across(cell, side);
        let owner = if (across.y, across.x) > (cell.y, cell.x) {
          cell
        } else {
          across
        };
        let chunk = owner.div_euclid(IVec2::splat(CHUNK));
        let at = owner.rem_euclid(IVec2::splat(CHUNK)) / 8;
        ((chunk.x, chunk.y), (at.y * 4 + at.x) as u8)
      })
      .collect()
  }

  /// A hexagonal map's parts' edges as its entities hold them, in the
  /// grid's points (`origin` its cell (0, 0)'s box corner), by chunk
  fn hex_held(
    world: &mut World,
    map: Entity,
    origin: Vec2,
  ) -> std::collections::BTreeMap<(i32, i32), Vec<(Point, Point)>> {
    use super::tests::segments_of;
    let parts = hex_parts(world, map);
    hex_edges_by_chunk(parts.into_iter().map(|(key, part, ..)| {
      let collider = world.get::<Collider>(part).expect("a part");
      let at = world.get::<Transform>(part).expect("placed").translation;
      (key, segments_of(collider, at - origin))
    }))
  }

  #[test]
  fn edits_on_a_hex_grid_rebuild_only_their_parts() {
    use super::hex::{HexCells, StaggerAxis, StaggerIndex};
    use super::pack::HexBox;
    use super::tests::hex_parts_of;
    use crate::core::schedule::EngineSchedules;
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let origin = Vec2::new(5.0, -3.0);
    let mut seed = 0xbb67_ae85_u32;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    // Half the cells of a box over four chunks filled
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0)).with_hex(hex, origin);
    for y in -12..12 {
      for x in -12..12 {
        if next(2) == 0 {
          tiles.set(IVec2::new(x, y), TileCell::Solid);
        }
      }
    }
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    // One edit and its checks: every part's edges by chunk are a fresh
    // build's, and every part whose box holds no block owning one of the
    // cell's sides keeps its entity, its shape and its collider. The
    // chunks that split
    let edit = |world: &mut World, s: &mut EngineSchedules, cell: IVec2, kind: TileCell| {
      let before = hex_parts(world, map);
      let dirty = hex_owners(hex, cell);
      world.get_mut::<TileColliders>(map).unwrap().set(cell, kind);
      run(world, s, 1);
      let after = hex_parts(world, map);
      for &(key, part, shape, written) in &before {
        let (chunk, blocks) = hex_box(key);
        let untouched = blocks.iter().all(|&b| !dirty.contains(&(chunk, b)));
        if untouched {
          assert!(
            after.contains(&(key, part, shape, written)),
            "{key:?} kept its entity, its shape and its collider after {cell} set to {kind:?}"
          );
        }
      }
      let fresh = hex_edges_by_chunk(hex_parts_of(world.get::<TileColliders>(map).unwrap()));
      assert_eq!(
        hex_held(world, map, origin),
        fresh,
        "after {cell} set to {kind:?}"
      );
      let whole = |parts: &[(PackKey, Entity, usize, u32)], chunk: (i32, i32)| {
        parts.iter().any(|&(key, ..)| {
          key
            == PackKey::Hex {
              chunk,
              part: HexBox::Chunk,
              rings: false,
            }
        })
      };
      let mut split: Vec<(i32, i32)> = dirty
        .iter()
        .map(|&(chunk, _)| chunk)
        .filter(|&chunk| {
          whole(&before, chunk)
            && !whole(&after, chunk)
            && after.iter().any(|&(key, ..)| hex_box(key).0 == chunk)
        })
        .collect();
      split.dedup();
      split
    };
    // A walker setting one cell a step as it goes from cell to cell
    let mut at = IVec2::ZERO;
    for _ in 0..400 {
      at = hex
        .across(at, next(6) as usize)
        .clamp(IVec2::splat(-16), IVec2::splat(15));
      let kinds = [
        TileCell::Solid,
        TileCell::Solid,
        TileCell::Empty,
        TileCell::Empty,
        TileCell::OneWay,
      ];
      edit(&mut world, &mut s, at, kinds[next(5) as usize]);
    }
    // A checkerboard laid over chunk (1, 1) a cell at a time: its one part
    // splits past the budget, and stays split
    let board: Vec<IVec2> = (1..31)
      .flat_map(|y| (1..31).map(move |x| IVec2::new(x, y)))
      .filter(|c| (c.x + c.y) % 2 == 0)
      .map(|c| IVec2::splat(CHUNK) + c)
      .collect();
    let mut split_at = None;
    for &cell in &board {
      if edit(&mut world, &mut s, cell, TileCell::Solid).contains(&(1, 1)) {
        assert!(split_at.is_none(), "one split");
        split_at = Some(cell);
      }
    }
    assert!(split_at.is_some(), "the chunk split");
    let keys_of = |world: &mut World, chunk: (i32, i32)| -> Vec<PackKey> {
      hex_parts(world, map)
        .into_iter()
        .map(|(key, ..)| key)
        .filter(|&key| hex_box(key).0 == chunk)
        .collect()
    };
    let held = keys_of(&mut world, (1, 1));
    assert!(
      !held.contains(&PackKey::Hex {
        chunk: (1, 1),
        part: HexBox::Chunk,
        rings: false
      }),
      "split for good: {held:?}"
    );
    // Its bottom-right block emptied, with the cells left of it whose sides
    // its cells own (below-left of its first column): that block's part goes
    let last = PackKey::Hex {
      chunk: (1, 1),
      part: HexBox::Block(15),
      rings: false,
    };
    assert!(keys_of(&mut world, (1, 1)).contains(&last));
    let gone = hex_parts(&mut world, map)
      .into_iter()
      .find(|&(key, ..)| key == last)
      .map(|(_, part, ..)| part);
    for &cell in board
      .iter()
      .filter(|c| c.x >= CHUNK + 23 && c.y >= CHUNK + 24)
    {
      edit(&mut world, &mut s, cell, TileCell::Empty);
    }
    assert!(
      !keys_of(&mut world, (1, 1)).contains(&last),
      "the empty block's part goes"
    );
    assert!(
      gone.is_some_and(|part| world.get_entity(part).is_err()),
      "despawned"
    );
    // And the whole chunk emptied: its parts go, and so does its record
    let parts: Vec<Entity> = hex_parts(&mut world, map)
      .into_iter()
      .filter(|&(key, ..)| hex_box(key).0 == (1, 1))
      .map(|(_, part, ..)| part)
      .collect();
    for &cell in &board {
      edit(&mut world, &mut s, cell, TileCell::Empty);
    }
    assert!(keys_of(&mut world, (1, 1)).is_empty());
    assert!(
      parts.iter().all(|&part| world.get_entity(part).is_err()),
      "despawned"
    );
    // Filled again it is one part: its split went with it
    for &cell in board.iter().take(10) {
      edit(&mut world, &mut s, cell, TileCell::Solid);
    }
    assert_eq!(
      keys_of(&mut world, (1, 1)),
      [PackKey::Hex {
        chunk: (1, 1),
        part: HexBox::Chunk,
        rings: false
      }]
    );
  }

  #[test]
  fn a_hex_part_that_lost_its_marker_is_replaced_not_kept() {
    use super::hex::{HexCells, StaggerAxis, StaggerIndex};
    use super::tests::hex_parts_of;
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0)).with_hex(hex, Vec2::ZERO);
    for x in 3..6 {
      tiles.set(IVec2::new(x, 4), TileCell::Solid);
    }
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let parts = hex_parts(&mut world, map);
    assert_eq!(parts.len(), 1);
    let (key, stray, ..) = parts[0];
    // The part stops being one (it keeps its collider), then its chunk is
    // edited: the stray goes, and a new part holds the chunk's edges
    world.entity_mut(stray).remove::<TilePart>();
    world
      .get_mut::<TileColliders>(map)
      .unwrap()
      .set(IVec2::new(6, 4), TileCell::Solid);
    run(&mut world, &mut s, 1);
    assert!(world.get_entity(stray).is_err(), "the stray is despawned");
    let parts = hex_parts(&mut world, map);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].0, key);
    assert_ne!(parts[0].1, stray);
    assert!(world.get::<TilePart>(parts[0].1).is_some());
    let fresh = hex_edges_by_chunk(hex_parts_of(world.get::<TileColliders>(map).unwrap()));
    assert_eq!(hex_held(&mut world, map, Vec2::ZERO), fresh);
  }

  #[test]
  fn a_side_edit_beside_ring_cells_rebuilds_no_ring_part() {
    use super::hex::{HexCells, StaggerAxis, StaggerIndex};
    use super::pack::HexBox;
    use super::tests::{hex_parts_of, inset_hexagon};
    use crate::core::ecs::ParentPlaced;
    // The ring-heavy chunk: chunk (1, 1) of a 128 x 128 map of 16 px
    // hexagons full of inset hexagons, its rings in its 64 sub-blocks'
    // parts. Cell (40, 40), where four of its blocks meet, turned whole and
    // back: its sides' parts are rebuilt, and of its rings only the part
    // holding it, sub-block (2, 2)
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let mut tiles = TileColliders::new(128, 128, Vec2::splat(16.0)).with_hex(hex, Vec2::ZERO);
    let inset = TileCell::Shape(tiles.add_shape(inset_hexagon()));
    for y in 32..64 {
      for x in 32..64 {
        tiles.set(xy(x, y), inset);
      }
    }
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let at = IVec2::new(40, 40);
    let holding = PackKey::Hex {
      chunk: (1, 1),
      part: HexBox::Sub(18),
      rings: true,
    };
    for (kind, sides) in [(TileCell::Solid, 6), (inset, 0)] {
      let before = hex_parts(&mut world, map);
      let rings = before
        .iter()
        .filter(|(key, ..)| matches!(key, PackKey::Hex { rings: true, .. }))
        .count();
      assert_eq!(rings, 64, "a ring part a sub-block");
      let held = before.iter().find(|&&(key, ..)| key == holding).copied();
      world.get_mut::<TileColliders>(map).unwrap().set(at, kind);
      run(&mut world, &mut s, 1);
      let after = hex_parts(&mut world, map);
      // Every other ring part keeps its entity, its shape and its collider
      for part in &before {
        if matches!(part.0, PackKey::Hex { rings: true, .. }) && part.0 != holding {
          assert!(
            after.contains(part),
            "{:?} kept its entity, its shape and its collider after {at} set to {kind:?}",
            part.0
          );
        }
      }
      // The one holding the cell took its new rings in place
      let now = after.iter().find(|&&(key, ..)| key == holding).copied();
      let (Some(was), Some(now)) = (held, now) else {
        panic!("{holding:?} held before and after: {held:?} {now:?}");
      };
      assert_eq!(now.1, was.1, "the same entity");
      assert_ne!((now.2, now.3), (was.2, was.3), "a new shape");
      // The side parts rebuilt hold the cell's sides alone
      let kept = &world
        .get::<TileBuild>(map)
        .and_then(|build| build.kept.as_ref())
        .expect("built")
        .hex;
      let rebuilt: usize = after
        .iter()
        .filter(|part| !before.contains(part))
        .filter(|(key, ..)| matches!(key, PackKey::Hex { rings: false, .. }))
        .map(|&(key, ..)| kept.edges(key).count())
        .sum();
      assert_eq!(rebuilt, sides, "after {at} set to {kind:?}");
      assert!(rebuilt < 200);
      let fresh = hex_edges_by_chunk(hex_parts_of(world.get::<TileColliders>(map).unwrap()));
      assert_eq!(hex_held(&mut world, map, Vec2::ZERO), fresh);
      for &(key, part, ..) in &after {
        assert!(
          world.get::<ParentPlaced>(part).is_some(),
          "{key:?}: placed by its map"
        );
      }
    }
  }

  #[test]
  fn a_cell_beside_ring_cells_turned_whole_rebuilds_no_ring_part() {
    use super::hex::{HexCells, StaggerAxis, StaggerIndex};
    use super::pack::{HexBox, HexParts};
    use super::tests::{hex_parts_of, inset_hexagon};
    // The ring-heavy chunk (1, 1), its rings in sub-blocks, and six inset
    // hexagons round empty cell (71, 36) in chunk (2, 1), its rings one part.
    // Cells that are not shapes, beside ring cells, turned whole and back:
    // (31, 33), whose side with (32, 32) that ring cell's block owns, and
    // (71, 36), whose own block holds the rings round it. Only parts of
    // sides are rebuilt
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let mut tiles = TileColliders::new(128, 128, Vec2::splat(16.0)).with_hex(hex, Vec2::ZERO);
    let inset = TileCell::Shape(tiles.add_shape(inset_hexagon()));
    for y in 32..64 {
      for x in 32..64 {
        tiles.set(xy(x, y), inset);
      }
    }
    for (x, y) in [(70, 36), (72, 36), (70, 35), (71, 35), (70, 37), (71, 37)] {
      tiles.set(xy(x, y), inset);
    }
    tiles.take_changes();
    // The same edits on parts of their own, for what each rebuilds
    let mut grid = tiles.clone();
    let mut held = HexParts::build(grid.cells_view(), hex, grid.bounds());
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let scattered = PackKey::Hex {
      chunk: (2, 1),
      part: HexBox::Chunk,
      rings: true,
    };
    for (x, y, kind) in [
      (31, 33, TileCell::Solid),
      (31, 33, TileCell::Empty),
      (71, 36, TileCell::Solid),
      (71, 36, TileCell::Empty),
    ] {
      let at = IVec2::new(x, y);
      assert!(grid.set(at, kind));
      let changed = grid.take_changes().expect("listed");
      let changes = held.apply(grid.cells_view(), hex, grid.bounds(), &changed);
      assert_eq!(changes.rebuilt.is_empty(), kind == TileCell::Empty);
      assert!(
        changes
          .rebuilt
          .iter()
          .all(|key| matches!(key, PackKey::Hex { rings: false, .. })),
        "{at} set to {kind:?} rebuilt {:?}",
        changes.rebuilt
      );
      let before = hex_parts(&mut world, map);
      let rings: Vec<_> = before
        .iter()
        .filter(|(key, ..)| matches!(key, PackKey::Hex { rings: true, .. }))
        .copied()
        .collect();
      assert_eq!(rings.len(), 64 + 1, "{rings:?}");
      assert!(rings.iter().any(|&(key, ..)| key == scattered));
      world.get_mut::<TileColliders>(map).unwrap().set(at, kind);
      run(&mut world, &mut s, 1);
      let after = hex_parts(&mut world, map);
      // Every ring part keeps its entity, its shape and its collider, and
      // parts of sides changed
      for part in &rings {
        assert!(
          after.contains(part),
          "{:?} kept its entity, its shape and its collider after {at} set to {kind:?}",
          part.0
        );
      }
      let sides = |parts: &[(PackKey, Entity, usize, u32)]| -> Vec<(PackKey, Entity, usize, u32)> {
        let mut sides: Vec<_> = parts
          .iter()
          .filter(|(key, ..)| matches!(key, PackKey::Hex { rings: false, .. }))
          .copied()
          .collect();
        sides.sort_unstable();
        sides
      };
      assert_ne!(sides(&before), sides(&after), "{at} set to {kind:?}");
      let fresh = hex_edges_by_chunk(hex_parts_of(world.get::<TileColliders>(map).unwrap()));
      assert_eq!(
        hex_held(&mut world, map, Vec2::ZERO),
        fresh,
        "{at} {kind:?}"
      );
    }
  }

  /// A grid of `hex`'s cells with parts of every box and kind: a block of
  /// chunk (0, -1) full of inset hexagons (its rings in sub-blocks), 12-gons
  /// scattered over chunk (-1, 0) (its rings one part), a checkerboard over
  /// chunk (1, 0) (its sides in blocks) and a clump of solid cells and
  /// inset hexagons round (0, 0)
  fn hex_mixed(hex: super::hex::HexCells) -> TileColliders {
    use super::tests::{inset_hexagon, polygon};
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0)).with_hex(hex, Vec2::ZERO);
    let inset = TileCell::Shape(tiles.add_shape(inset_hexagon()));
    let twelve = TileCell::Shape(tiles.add_shape(polygon(12, 0.25)));
    for y in -32..-24 {
      for x in 8..16 {
        tiles.set(IVec2::new(x, y), inset);
      }
    }
    for i in 0..10 {
      tiles.set(IVec2::new(-30 + 3 * i, 2 + 3 * i), twelve);
    }
    for y in 1..31 {
      for x in 1..31 {
        if (x + y) % 2 == 0 {
          tiles.set(IVec2::new(CHUNK + x, y), TileCell::Solid);
        }
      }
    }
    for y in -3..3 {
      for x in -3..3 {
        let kind = if (x * y) % 3 == 0 {
          TileCell::Solid
        } else {
          inset
        };
        tiles.set(IVec2::new(x, y), kind);
      }
    }
    tiles
  }

  #[test]
  fn a_landed_hex_map_s_prebuilt_parts_match_a_fresh_build_s() {
    use super::hex::{HexCells, StaggerAxis, StaggerIndex};
    use super::pack::{HexBox, HexParts};
    let grids = [
      HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8),
      HexCells::new(StaggerAxis::X, StaggerIndex::Even, UVec2::new(120, 140), 70),
      HexCells::new(StaggerAxis::Y, StaggerIndex::Even, UVec2::new(32, 32), 0),
      HexCells::new(StaggerAxis::X, StaggerIndex::Odd, UVec2::new(15, 17), 7),
    ];
    for hex in grids {
      // The landing worker's trace: each part a fresh build's, its place
      // and its shape, its Rapier shape built
      let tiles = hex_mixed(hex);
      let fresh = HexParts::build(tiles.cells_view(), hex, tiles.bounds());
      let mut want: Vec<(PackKey, Vec2, ColliderShape)> = fresh
        .keys()
        .into_iter()
        .map(|key| {
          let (at, collider) = tiles.hex_pack(&fresh, key);
          (key, at, collider.shape)
        })
        .collect();
      let traced = tiles.trace().expect("a hexagonal grid traces");
      let (_, built) = traced.content.as_ref().expect("traced");
      let prebuilt: Vec<(PackKey, Vec2, ColliderShape)> = built
        .iter()
        .map(|(key, at, collider, shape)| {
          assert!(shape.is_some(), "{hex:?} {key:?}: its shape built");
          (*key, *at, collider.shape.clone())
        })
        .collect();
      assert_eq!(prebuilt, want, "{hex:?}");
      for (part, rings) in [
        (HexBox::Sub(0), true),
        (HexBox::Chunk, true),
        (HexBox::Block(0), false),
        (HexBox::Chunk, false),
      ] {
        assert!(
          want.iter().any(|&(key, ..)| matches!(
            key,
            PackKey::Hex { part: p, rings: r, .. }
              if r == rings && std::mem::discriminant(&p) == std::mem::discriminant(&part)
          )),
          "{hex:?}: a {part:?} part, rings {rings}"
        );
      }
      // Landed from it, each part sits as traced, with the shape built
      // ahead
      let (mut world, mut s) = world(60);
      let map = world.spawn((Transform::IDENTITY, tiles, traced)).id();
      run(&mut world, &mut s, 1);
      let parts = hex_parts(&mut world, map);
      let mut landed: Vec<(PackKey, Vec2, ColliderShape)> = parts
        .iter()
        .map(|&(key, part, ..)| {
          let at = world.get::<Transform>(part).expect("placed").translation;
          let collider = world.get::<Collider>(part).expect("a part");
          (key, at, collider.shape.clone())
        })
        .collect();
      landed.sort_by_key(|&(key, ..)| key);
      want.sort_by_key(|&(key, ..)| key);
      assert_eq!(landed, want, "{hex:?}");
      let physics = world.resource::<Physics>();
      for &(key, part, ..) in &parts {
        let prebuilt = world.get::<PrebuiltShape>(part).expect("built ahead");
        let handle = physics.collider_entries[&part].handle;
        let registered = physics.colliders.get(handle).expect("registered");
        assert!(
          std::sync::Arc::ptr_eq(&prebuilt.0.0, &registered.shared_shape().0),
          "{hex:?} {key:?} took its prebuilt shape"
        );
      }
    }
  }

  #[test]
  fn a_streamed_hex_chunk_landed_again_has_fresh_ring_parts() {
    use super::hex::{HexCells, StaggerAxis, StaggerIndex};
    use super::pack::HexBox;
    use super::tests::{hex_parts_of, inset_hexagon};
    // Chunk (1, 0) of 16 px hexagons: its left half full of inset
    // hexagons, its rings split down to sub-blocks, and a wall from chunk
    // (0, 0) across its border. Cell (31, 21) of chunk (0, 0), under the
    // wall, has a side its neighbour (32, 20) owns, so chunk (1, 0) keeps a
    // part of sides while it is away
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0)).with_hex(hex, Vec2::ZERO);
    let inset = TileCell::Shape(tiles.add_shape(inset_hexagon()));
    for y in 0..CHUNK {
      for x in 32..48 {
        tiles.set(IVec2::new(x, y), inset);
      }
    }
    for x in 26..38 {
      tiles.set(IVec2::new(x, 20), TileCell::Solid);
    }
    tiles.set(IVec2::new(31, 21), TileCell::Solid);
    tiles.set(IVec2::new(5, 5), TileCell::Solid);
    let chunk = IVec2::new(1, 0);
    let mut first = Box::new([TileCell::Empty; CELLS]);
    for (i, cell) in first.iter_mut().enumerate() {
      *cell = tiles.get(chunk * CHUNK + IVec2::new(i as i32 % CHUNK, i as i32 / CHUNK));
    }
    let mut sparse = Box::new([TileCell::Empty; CELLS]);
    for i in [40, 300, 700] {
      sparse[i] = inset;
    }
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let of_chunk = |world: &mut World, rings: bool| -> Vec<(PackKey, Entity)> {
      hex_parts(world, map)
        .into_iter()
        .filter(
          |&(key, ..)| matches!(key, PackKey::Hex { chunk: (1, 0), rings: r, .. } if r == rings),
        )
        .map(|(key, part, ..)| (key, part))
        .collect()
    };
    // Its parts' keys and edges are a fresh build's
    let as_fresh = |world: &mut World, what: &str| {
      let fresh = hex_parts_of(world.get::<TileColliders>(map).unwrap());
      let mut keys: Vec<PackKey> = fresh.iter().map(|&(key, _)| key).collect();
      keys.sort_unstable();
      let mut held: Vec<PackKey> = hex_parts(world, map)
        .into_iter()
        .map(|(key, ..)| key)
        .collect();
      held.sort_unstable();
      assert_eq!(held, keys, "{what}");
      assert_eq!(
        hex_held(world, map, Vec2::ZERO),
        hex_edges_by_chunk(fresh),
        "{what}"
      );
    };
    assert!(of_chunk(&mut world, true).len() > 4, "its rings split");
    for (round, cells) in [first, sparse].iter().enumerate() {
      // Unloaded: none of its ring parts outlives it, and its part of
      // sides holds the side it owns of the cell below the wall
      let before = of_chunk(&mut world, true);
      world
        .get_mut::<TileColliders>(map)
        .unwrap()
        .unload_chunk(chunk);
      run(&mut world, &mut s, 1);
      assert!(of_chunk(&mut world, true).is_empty(), "round {round}");
      assert!(
        before
          .iter()
          .all(|&(_, part)| world.get_entity(part).is_err()),
        "round {round}: despawned"
      );
      let sides = of_chunk(&mut world, false);
      assert_eq!(sides.len(), 1, "round {round}: {sides:?}");
      as_fresh(&mut world, &format!("round {round}, away"));
      // Landed again: a fresh build's parts, none of them from before
      assert!(
        world
          .get_mut::<TileColliders>(map)
          .unwrap()
          .set_chunk(chunk, cells)
      );
      run(&mut world, &mut s, 1);
      as_fresh(&mut world, &format!("round {round}, landed again"));
      let back = of_chunk(&mut world, true);
      assert!(!back.is_empty());
      assert!(
        back
          .iter()
          .all(|&(_, part)| before.iter().all(|&(_, old)| old != part)),
        "round {round}: no ring part from its life before"
      );
    }
    // Landed with three rings alone, its rings are one part: the split of
    // its first life went with them
    let rings: Vec<PackKey> = of_chunk(&mut world, true)
      .into_iter()
      .map(|(key, _)| key)
      .collect();
    let whole = PackKey::Hex {
      chunk: (1, 0),
      part: HexBox::Chunk,
      rings: true,
    };
    assert_eq!(rings, [whole]);
  }

  #[test]
  fn a_reloaded_hex_grid_after_ring_edits_has_a_fresh_build_s_parts() {
    use super::hex::{HexCells, StaggerAxis, StaggerIndex};
    use super::pack::HexBox;
    use super::tests::{hex_parts_of, polygon};
    // Block 0 of chunk (0, 0) full of 12-gons, 768 ring edges: its rings
    // are in its sub-blocks' parts. Edits leave three of them, and the
    // split stays, since an edit never merges parts back
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let grid = || {
      let mut tiles = TileColliders::unbounded(Vec2::splat(16.0)).with_hex(hex, Vec2::ZERO);
      let twelve = TileCell::Shape(tiles.add_shape(polygon(12, 0.0)));
      tiles.set(IVec2::new(20, 20), TileCell::Solid);
      (tiles, twelve)
    };
    let left = [IVec2::new(1, 1), IVec2::new(6, 2), IVec2::new(3, 6)];
    let (mut tiles, twelve) = grid();
    for y in 0..8 {
      for x in 0..8 {
        tiles.set(IVec2::new(x, y), twelve);
      }
    }
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    {
      let mut tiles = world.get_mut::<TileColliders>(map).unwrap();
      for y in 0..8 {
        for x in 0..8 {
          if !left.contains(&IVec2::new(x, y)) {
            tiles.set(IVec2::new(x, y), TileCell::Empty);
          }
        }
      }
    }
    run(&mut world, &mut s, 1);
    let keys = |world: &mut World| -> (Vec<PackKey>, Vec<PackKey>) {
      let mut held: Vec<PackKey> = hex_parts(world, map)
        .into_iter()
        .map(|(key, ..)| key)
        .collect();
      held.sort_unstable();
      let mut fresh: Vec<PackKey> = hex_parts_of(world.get::<TileColliders>(map).unwrap())
        .into_iter()
        .map(|(key, _)| key)
        .collect();
      fresh.sort_unstable();
      (held, fresh)
    };
    let (held, fresh) = keys(&mut world);
    assert_ne!(held, fresh, "the edits left the split");
    let before = hex_parts(&mut world, map);
    // A reload puts a new grid of the same cells on the map, as the tile
    // map's collision sync does: it builds afresh, one ring part, and no
    // part from before stays
    let (mut reloaded, _) = grid();
    for &cell in &left {
      reloaded.set(cell, twelve);
    }
    world.entity_mut(map).insert(reloaded);
    run(&mut world, &mut s, 1);
    let (held, fresh) = keys(&mut world);
    assert_eq!(held, fresh);
    let whole = PackKey::Hex {
      chunk: (0, 0),
      part: HexBox::Chunk,
      rings: true,
    };
    assert!(held.contains(&whole), "{held:?}");
    assert!(
      before
        .iter()
        .all(|&(_, part, ..)| world.get_entity(part).is_err()),
      "no part from before the reload stays"
    );
    let fresh = hex_edges_by_chunk(hex_parts_of(world.get::<TileColliders>(map).unwrap()));
    assert_eq!(hex_held(&mut world, map, Vec2::ZERO), fresh);
  }

  #[test]
  fn hex_parts_sit_at_their_chunk_or_block_corner_with_relative_vertices() {
    use super::hex::{HexCells, StaggerAxis, StaggerIndex};
    use super::pack::HexBox;
    for (axis, index) in [
      (StaggerAxis::Y, StaggerIndex::Odd),
      (StaggerAxis::X, StaggerIndex::Even),
    ] {
      let hex = HexCells::new(axis, index, UVec2::new(16, 16), 8);
      let origin = Vec2::new(5.0, -3.0);
      let mut tiles = TileColliders::unbounded(Vec2::splat(16.0)).with_hex(hex, origin);
      // A cell well inside chunk (0, 0), one well inside chunk (-1, 2), a
      // checkerboard over chunk (2, 0), which splits into blocks, and a
      // block of chunk (0, 1) full of inset hexagons, whose rings split
      // into its four sub-blocks
      let lone = [(IVec2::new(10, 12), (0, 0)), (IVec2::new(-20, 80), (-1, 2))];
      for (cell, _) in lone {
        assert!(tiles.set(cell, TileCell::Solid));
      }
      for y in 1..31 {
        for x in 1..31 {
          if (x + y) % 2 == 0 {
            tiles.set(IVec2::new(2 * CHUNK + x, y), TileCell::Solid);
          }
        }
      }
      let inset = TileCell::Shape(tiles.add_shape(super::tests::inset_hexagon()));
      for y in 40..48 {
        for x in 8..16 {
          tiles.set(IVec2::new(x, y), inset);
        }
      }
      let (mut world, mut s) = world(60);
      let map = world.spawn((Transform::IDENTITY, tiles)).id();
      run(&mut world, &mut s, 1);
      let parts = hex_parts(&mut world, map);
      assert_eq!(parts.len(), 2 + 16 + 4, "{axis:?} {parts:?}");
      // A box of `n` cells across by `n` lines along, px
      let span = |n: i32| match axis {
        StaggerAxis::Y => Vec2::new((n * 16) as f32, (n * 12) as f32),
        StaggerAxis::X => Vec2::new((n * 12) as f32, (n * 16) as f32),
      };
      for &(key, part, ..) in &parts {
        let PackKey::Hex {
          chunk,
          part: placed,
          ..
        } = key
        else {
          panic!("{key:?}");
        };
        let origin_cell = IVec2::new(chunk.0, chunk.1) * CHUNK;
        let (first, n) = match placed {
          HexBox::Chunk => (origin_cell, CHUNK),
          HexBox::Quarter(q) => {
            let at = IVec2::new(i32::from(q) % 2, i32::from(q) / 2);
            (origin_cell + at * 16, 16)
          }
          HexBox::Block(b) => {
            assert_eq!(chunk, (2, 0), "only the checkerboard splits");
            let at = IVec2::new(i32::from(b) % 4, i32::from(b) / 4);
            (origin_cell + at * 8, 8)
          }
          HexBox::Sub(s) => {
            assert_eq!(chunk, (0, 1), "only the inset hexagons' rings split");
            assert!([18, 19, 26, 27].contains(&s), "{axis:?} {key:?}");
            let at = IVec2::new(i32::from(s) % 8, i32::from(s) / 8);
            (origin_cell + at * 4, 4)
          }
        };
        let corner = hex.corner_of(first).as_vec2();
        let at = world.get::<Transform>(part).expect("placed").translation;
        assert_eq!(at, origin + corner, "{axis:?} {key:?}");
        let collider = world.get::<Collider>(part).expect("a part");
        let ColliderShape::Segments { points, .. } = &collider.shape else {
          panic!("segments: {collider:?}");
        };
        assert!(
          points
            .iter()
            .all(|p| p.cmpge(Vec2::ZERO).all() && p.cmple(span(n) + 16.0).all()),
          "{axis:?} {key:?} {points:?}"
        );
        if let Some(&(cell, _)) = lone.iter().find(|(_, c)| *c == chunk) {
          let mut got: Vec<(i32, i32)> = points.iter().map(|p| (p.x as i32, p.y as i32)).collect();
          got.sort_unstable();
          got.dedup();
          let mut want: Vec<(i32, i32)> = hex
            .corners(cell)
            .iter()
            .map(|c| (c.x as i32 - corner.x as i32, c.y as i32 - corner.y as i32))
            .collect();
          want.sort_unstable();
          assert_eq!(got, want, "{axis:?} {cell}");
        }
      }
    }
  }

  /// A grid with its name and the cell an edit sets on it
  type Edited = (&'static str, TileColliders, (u32, u32));

  /// The islands map of `one_tile_edit_timing` (3 x 3 islands every 8 cells
  /// from offset 0, none in the 136 x 40 cells top-left the loops take; not
  /// the bench's `edit_rock` layout) and its two loops spanning four chunks
  /// (a jagged skyline and a one-cell serpentine, a map each), with the
  /// cell each is edited at, on `hex`'s cells, or square ones without
  fn islands_and_loops(hex: Option<super::hex::HexCells>) -> (TileColliders, [Edited; 2]) {
    let size = 1024;
    let islands = || {
      let mut tiles = TileColliders::new(size, size, Vec2::splat(16.0));
      if let Some(hex) = hex {
        tiles = tiles.with_hex(hex, Vec2::ZERO);
      }
      for by in (0..size).step_by(8) {
        for bx in (0..size).step_by(8) {
          if bx < 136 && by < 40 {
            continue;
          }
          for (x, y) in (0..9).map(|i| (bx + i % 3, by + i / 3)) {
            tiles.set(xy(x, y), TileCell::Solid);
          }
        }
      }
      tiles
    };
    let mut skyline = islands();
    let mut seed: u32 = 7;
    let mut tops = Vec::new();
    for x in 0..128 {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      let top = 1 + (seed >> 16) % 24;
      tops.push(top);
      for y in top..32 {
        skyline.set(xy(x, y), TileCell::Solid);
      }
    }
    let mut serpentine = islands();
    for y in (1..32).step_by(2) {
      for x in 0..128 {
        serpentine.set(xy(x, y), TileCell::Solid);
      }
      if y < 31 {
        let end = if y % 4 == 1 { 127 } else { 0 };
        serpentine.set(xy(end, y + 1), TileCell::Solid);
      }
    }
    (
      islands(),
      [
        ("a skyline", skyline, (64, tops[64] - 1)),
        ("a serpentine", serpentine, (64, 0)),
      ],
    )
  }

  /// One-tile edits on 16 px hexes of side 8, odd rows, over the islands
  /// map of `one_tile_edit_timing` (`islands_and_loops`, not the bench's
  /// `edit_rock` layout; no frame passes between edits, so caches are
  /// warm), each timed in parts: the tile rebuild (`rebuild_tile_colliders`
  /// alone), the fixed step after it (physics taking in the reshaped
  /// parts), and both together, with the edges the rebuilt parts hold (of
  /// sides and of rings). The cases: the two loops' cells (in chunks split
  /// for their edges), an island's cell at a corner of four of the islands'
  /// ordinary chunks, one on a chunk's edge, 400 random cells each set and
  /// set back, and a corner of four chunks each filled to just under the
  /// budget (each edit rebuilding four parts of nearly the budget's edges).
  /// The target is both together within 100 µs at the 99th percentile.
  /// Run in release:
  /// `cargo test --release -p ryo2d --lib hex_one_tile_edit_timing -- --ignored --nocapture`
  #[test]
  #[ignore = "a timing: run in release with --ignored --nocapture"]
  fn hex_one_tile_edit_timing() {
    use super::hex::{HexCells, StaggerAxis, StaggerIndex};
    use super::pack::HEX_PART_EDGES;
    use crate::core::ecs::Schedule;
    use std::time::{Duration, Instant};
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let (islands, loops) = islands_and_loops(Some(hex));
    let mut cases: Vec<(String, TileColliders, Vec<IVec2>)> = loops
      .into_iter()
      .map(|(name, tiles, (x, y))| (name.to_string(), tiles, vec![xy(x, y); 400]))
      .collect();
    cases.push((
      "an island's corner cell at (96, 96), four chunks".into(),
      islands.clone(),
      vec![IVec2::splat(96); 400],
    ));
    cases.push((
      "an island's cell on a chunk edge at (96, 104)".into(),
      islands.clone(),
      vec![IVec2::new(96, 104); 400],
    ));
    let mut seed = 0x1f83_d9ab_u32;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    let random: Vec<IVec2> = (0..400)
      .flat_map(|_| {
        let cell = IVec2::new(next(1024) as i32, next(1024) as i32);
        [cell, cell]
      })
      .collect();
    cases.push((
      "400 random cells, set and set back".into(),
      islands.clone(),
      random,
    ));
    // Chunks (1, 1) to (4, 4) emptied, and the four round (96, 96) each filled
    // with isolated cells (six edges each, owned in the chunk) nearest the
    // corner first, to just under the budget
    let mut full = islands;
    for y in 32..160 {
      for x in 32..160 {
        full.set(IVec2::new(x, y), TileCell::Empty);
      }
    }
    for chunk in [
      IVec2::new(2, 2),
      IVec2::new(3, 2),
      IVec2::new(2, 3),
      IVec2::new(3, 3),
    ] {
      let mut spots: Vec<IVec2> = (1..15)
        .flat_map(|j| (1..15).map(move |i| chunk * CHUNK + IVec2::new(2 * i, 2 * j)))
        .collect();
      spots.sort_by_key(|c| (*c - IVec2::splat(96)).abs().max_element());
      for cell in spots.into_iter().take((HEX_PART_EDGES - 4) / 6) {
        full.set(cell, TileCell::Solid);
      }
    }
    cases.push((
      "a corner of four chunks just under the budget".into(),
      full,
      vec![IVec2::splat(96); 400],
    ));
    println!(
      "hexagonal parts of at most {HEX_PART_EDGES} edges: a chunk, else its quarters, else their blocks"
    );
    println!(
      "| case | parts | tile rebuild median / p99 | fixed step median / p99 | both median / p99 / max | edges rebuilt median / max |"
    );
    for (name, tiles, cells) in cases {
      let mut tiles = tiles;
      tiles.take_changes();
      let (mut world, mut s) = super::super::test_util::world(60);
      let map = world.spawn((Transform::IDENTITY, tiles)).id();
      run(&mut world, &mut s, 2);
      let mut rebuild = Schedule::default();
      rebuild.add_systems(rebuild_tile_colliders);
      let (mut rebuilds, mut steps, mut both, mut edges) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
      for cell in cells {
        let before = hex_parts(&mut world, map);
        let mut grid = world.get_mut::<TileColliders>(map).unwrap();
        let now = if grid.get(cell) == TileCell::Solid {
          TileCell::Empty
        } else {
          TileCell::Solid
        };
        assert!(grid.set(cell, now), "{cell}");
        let start = Instant::now();
        rebuild.run(&mut world);
        let middle = Instant::now();
        run(&mut world, &mut s, 1);
        let end = Instant::now();
        rebuilds.push(middle - start);
        steps.push(end - middle);
        both.push(end - start);
        // The edges of the parts rebuilt, of sides and of rings: each
        // part's segments
        let after = hex_parts(&mut world, map);
        edges.push(
          after
            .iter()
            .filter(|part| !before.contains(part))
            .map(
              |&(_, part, ..)| match world.get::<Collider>(part).map(|c| &c.shape) {
                Some(ColliderShape::Segments { segments, .. }) => segments.len(),
                _ => 0,
              },
            )
            .sum::<usize>(),
        );
      }
      let stats = |mut times: Vec<Duration>| {
        times.sort();
        let p99 = times[(times.len() * 99).div_ceil(100) - 1];
        (times[times.len() / 2], p99, times[times.len() - 1])
      };
      let (rebuild, rebuild99, _) = stats(rebuilds);
      let (step, step99, _) = stats(steps);
      let (total, total99, most) = stats(both);
      edges.sort_unstable();
      let parts = hex_parts(&mut world, map).len();
      println!(
        "| {name} ({} edits) | {parts} | {rebuild:.1?} / {rebuild99:.1?} | {step:.1?} / {step99:.1?} | {total:.1?} / {total99:.1?} / {most:.1?} | {} / {} |",
        edges.len(),
        edges[edges.len() / 2],
        edges[edges.len() - 1]
      );
    }
  }

  /// The parts the static guard's map (1024 x 1024 cave rock from value
  /// noise, `benches/tilemap.rs`) and the islands map land, square and on
  /// 16 px hexes of side 8, and an idle fixed step with each. Run in
  /// release:
  /// `cargo test --release -p ryo2d --lib hex_static_part_counts -- --ignored --nocapture`
  #[test]
  #[ignore = "a measurement: run in release with --ignored --nocapture"]
  fn hex_static_part_counts() {
    use super::hex::{HexCells, StaggerAxis, StaggerIndex};
    use super::pack::HEX_PART_EDGES;
    use std::time::{Duration, Instant};
    // The bench's hash and value noise, and its rock rule
    let hash = |x: i32, y: i32, salt: u32| {
      let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ salt.wrapping_mul(0xCB1A_B31F);
      h ^= h >> 13;
      h = h.wrapping_mul(0x5BD1_E995);
      h ^ (h >> 15)
    };
    let noise = |x: f32, y: f32, salt: u32| {
      let (x0, y0) = (x.floor(), y.floor());
      let (fx, fy) = (x - x0, y - y0);
      let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
      let (ix, iy) = (x0 as i32, y0 as i32);
      let at = |dx: i32, dy: i32| (hash(ix + dx, iy + dy, salt) & 0xFFFF) as f32 / 65535.0;
      let top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
      let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
      top + (bottom - top) * sy
    };
    let rocky = |x: i32, y: i32| {
      let (fx, fy) = (x as f32, y as f32);
      noise(fx / 22.0, fy / 22.0, 1) * 0.7 + noise(fx / 7.0, fy / 7.0, 2) * 0.3 > 0.6
    };
    let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, UVec2::new(16, 16), 8);
    let cave = |hex: Option<HexCells>| {
      let mut tiles = TileColliders::new(1024, 1024, Vec2::splat(16.0));
      if let Some(hex) = hex {
        tiles = tiles.with_hex(hex, Vec2::ZERO);
      }
      for y in 0..1024 {
        for x in 0..1024 {
          if rocky(x, y) {
            tiles.set(IVec2::new(x, y), TileCell::Solid);
          }
        }
      }
      tiles
    };
    let idle = |tiles: TileColliders| {
      let parts = tiles
        .trace()
        .and_then(|traced| traced.content.map(|(_, built)| built.len()))
        .unwrap_or(0);
      let (mut world, mut s) = super::super::test_util::world(60);
      world.spawn((Transform::IDENTITY, tiles));
      run(&mut world, &mut s, 3);
      let mut times: Vec<Duration> = (0..200)
        .map(|_| {
          let start = Instant::now();
          run(&mut world, &mut s, 1);
          start.elapsed()
        })
        .collect();
      times.sort();
      (parts, times[times.len() / 2])
    };
    println!("hexagonal parts of at most {HEX_PART_EDGES} edges");
    for (name, square, hexes) in [
      ("cave rock", cave(None), cave(Some(hex))),
      (
        "islands",
        islands_and_loops(None).0,
        islands_and_loops(Some(hex)).0,
      ),
    ] {
      let (square, square_idle) = idle(square);
      let (hexes, hex_idle) = idle(hexes);
      println!(
        "{name}: square {square} parts (idle step {square_idle:?}), hexagonal {hexes} parts (idle step {hex_idle:?})"
      );
    }
  }

  #[test]
  fn a_traced_grid_s_parts_take_their_prebuilt_shapes_at_their_chunk_origins() {
    // Islands in chunk (0, 0) and far out in chunk (-3, 2), traced ahead as
    // a landing does
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0));
    for cell in [IVec2::new(1, 1), IVec2::new(-90, 70)] {
      assert!(tiles.set(cell, TileCell::Solid));
    }
    let traced = tiles.trace().expect("outline mode traces");
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles, traced)).id();
    run(&mut world, &mut s, 1);
    let mut query = world.query::<(Entity, &ChildOf, &Transform, &PrebuiltShape)>();
    let mut parts: Vec<(Entity, Vec2, SharedShape)> = query
      .iter(&world)
      .filter(|(_, child_of, ..)| child_of.parent() == map)
      .map(|(part, _, at, shape)| (part, at.translation, shape.0.clone()))
      .collect();
    parts.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
    let places: Vec<Vec2> = parts.iter().map(|part| part.1).collect();
    assert_eq!(places, [Vec2::new(-3.0 * 512.0, 2.0 * 512.0), Vec2::ZERO]);
    // Physics registered each with the shape built ahead, not one of its own
    let physics = world.resource::<Physics>();
    for (part, at, shape) in &parts {
      let handle = physics.collider_entries[part].handle;
      let registered = physics.colliders.get(handle).expect("registered");
      assert!(
        std::sync::Arc::ptr_eq(&shape.0, &registered.shared_shape().0),
        "the part at {at} took its prebuilt shape"
      );
    }
  }

  #[test]
  fn a_floor_across_an_unloaded_chunk_closes_at_its_edge() {
    // A floor on row 5 from cell -32 to 63, through chunks -1, 0 and 1, and
    // a block in chunk 1 of its own. Unloading chunk 1 leaves one closed
    // loop from x -32 to 32, still in chunk -1's part, and no part in chunk 1
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0));
    for x in -32..64 {
      assert!(tiles.set(IVec2::new(x, 5), TileCell::Solid));
    }
    assert!(tiles.set(IVec2::new(50, 20), TileCell::Solid));
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let before = parts_by_id(&mut world, map);
    assert_eq!(before.len(), 2, "chunk -1's part and chunk 1's");
    let floor = part_in(&before, Vec2::new(-512.0, 80.0), Vec2::new(-512.0, 96.0));
    world
      .get_mut::<TileColliders>(map)
      .unwrap()
      .unload_chunk(IVec2::new(1, 0));
    run(&mut world, &mut s, 1);
    let after = parts_by_id(&mut world, map);
    assert_eq!(after.len(), 1, "no part left in chunk 1: {after:?}");
    assert_eq!(after[0].0, floor, "the floor's part keeps its entity");
    let loops = outline_loops(&after, 16.0);
    assert_eq!(loops.len(), 1, "one loop: {loops:?}");
    let ring = &loops[0];
    assert_eq!(ring.first(), ring.last(), "closed: {ring:?}");
    let mut corners = ring.clone();
    corners.sort_unstable();
    corners.dedup();
    assert_eq!(corners, [(-32, 5), (-32, 6), (32, 5), (32, 6)]);
    assert_matches_a_full_trace(&mut world, map);
  }

  #[test]
  fn an_edit_on_an_unbounded_grid_retraces_only_its_loops() {
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0));
    // Islands in chunks (-2, -1) and (-4, -3)
    for x in -40..-37 {
      tiles.set(IVec2::new(x, -5), TileCell::Solid);
    }
    for y in -70..-67 {
      tiles.set(IVec2::new(-100, y), TileCell::Solid);
    }
    let (mut world, mut s) = world(60);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let handles = |world: &mut World| -> Vec<(Entity, Collider, String)> {
      let parts = parts_by_id(world, map);
      let physics = world.resource::<Physics>();
      parts
        .into_iter()
        .map(|(part, collider)| {
          let handle = physics.collider_entries.get(&part).map(|e| e.handle);
          (part, collider, format!("{handle:?}"))
        })
        .collect()
    };
    let before = handles(&mut world);
    assert_eq!(before.len(), 2);
    let parts: Vec<(Entity, Collider)> = before.iter().map(|(e, c, _)| (*e, c.clone())).collect();
    let edited = part_in(&parts, Vec2::new(-640.0, -80.0), Vec2::new(-592.0, -64.0));
    let other = part_in(
      &parts,
      Vec2::new(-1600.0, -1120.0),
      Vec2::new(-1584.0, -1072.0),
    );
    assert!(
      world
        .get_mut::<TileColliders>(map)
        .unwrap()
        .set(IVec2::new(-37, -5), TileCell::Solid)
    );
    run(&mut world, &mut s, 1);
    let after = handles(&mut world);
    let find = |parts: &[(Entity, Collider, String)], part: Entity| {
      parts.iter().find(|(e, ..)| *e == part).cloned()
    };
    let (Some(was), Some(now)) = (find(&before, edited), find(&after, edited)) else {
      panic!("the edited island's part kept its entity: {after:?}");
    };
    assert!(was.2.starts_with("Some") && was.2 == now.2, "and handle");
    assert_ne!(was.1.shape, now.1.shape, "reshaped in place");
    assert!(part_bounds(&mut world, map).iter().any(|b| b.1.x == -576.0));
    assert_eq!(
      find(&after, other).map(|p| p.1),
      find(&before, other).map(|p| p.1),
      "the other island's part is untouched"
    );
    assert_eq!(after.len(), 2);
    assert_matches_a_full_trace(&mut world, map);
  }

  /// The map's parts, by entity, sorted, their colliders in its space.
  fn parts_by_id(world: &mut World, map: Entity) -> Vec<(Entity, Collider)> {
    let mut query = world.query::<(Entity, &ChildOf, &Collider, &Transform)>();
    let mut found: Vec<(Entity, Collider)> = query
      .iter(world)
      .filter(|(_, child_of, ..)| child_of.parent() == map)
      .map(|(e, _, collider, at)| (e, in_grid_space(collider.clone(), at.translation)))
      .collect();
    found.sort_by_key(|(e, _)| *e);
    found
  }

  /// Each of the map's parts' world box, as physics places it
  fn part_bounds(world: &mut World, map: Entity) -> Vec<(Vec2, Vec2)> {
    let mut query = world.query::<(&ChildOf, &Collider, &GlobalTransform)>();
    query
      .iter(world)
      .filter(|(child_of, ..)| child_of.parent() == map)
      .map(|(_, collider, global)| collider.world_bounds(global))
      .collect()
  }

  /// The box holding every one of `bounds`
  fn union(bounds: &[(Vec2, Vec2)]) -> (Vec2, Vec2) {
    bounds.iter().fold(
      (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
      |(min, max), &(a, b)| (min.min(a), max.max(b)),
    )
  }

  /// The part whose shape has a point in `region` (px, min and max).
  fn part_in(parts: &[(Entity, Collider)], min: Vec2, max: Vec2) -> Entity {
    let points = |c: &Collider| match &c.shape {
      ColliderShape::Polyline { points }
      | ColliderShape::Polygon { points }
      | ColliderShape::Segments { points, .. } => points.clone(),
      ColliderShape::Segment { a, b } => vec![*a, *b],
      _ => Vec::new(),
    };
    let inside = |p: &Vec2| p.cmpge(min).all() && p.cmple(max).all();
    let found: Vec<Entity> = parts
      .iter()
      .filter(|(_, c)| points(c).iter().any(inside))
      .map(|(e, _)| *e)
      .collect();
    assert_eq!(found.len(), 1, "one part in {min}..{max}: {parts:?}");
    found[0]
  }

  /// The outline loops the parts hold (not their one-way runs), as loops
  /// of corners (tiles).
  fn outline_loops(parts: &[(Entity, Collider)], tile: f32) -> Vec<Vec<Corner>> {
    parts
      .iter()
      .filter(|(_, c)| c.one_way.is_none())
      .flat_map(|(_, c)| collider_loops(c, tile))
      .collect()
  }

  /// How many outline loops the map's parts hold, whatever packs them
  fn loop_count(world: &mut World, map: Entity) -> usize {
    outline_loops(&parts_by_id(world, map), 1.0).len()
  }

  /// The outline loops (or open chains) of one part, in tiles: a polyline
  /// is one; packed segments are as many as their runs of edges, each edge
  /// going on from the last one's end
  fn collider_loops(collider: &Collider, tile: f32) -> Vec<Vec<Corner>> {
    let corner = |p: &Vec2| ((p.x / tile).round() as i32, (p.y / tile).round() as i32);
    match &collider.shape {
      ColliderShape::Polyline { points } => vec![points.iter().map(corner).collect()],
      ColliderShape::Segments { points, segments } => {
        let mut loops: Vec<Vec<Corner>> = Vec::new();
        let mut last: Option<u32> = None;
        for &[a, b] in segments {
          if last != Some(a) {
            loops.push(vec![corner(&points[a as usize])]);
          }
          if let Some(current) = loops.last_mut() {
            current.push(corner(&points[b as usize]));
          }
          last = Some(b);
        }
        loops
      }
      _ => Vec::new(),
    }
  }

  /// Loops as sorted sets of directed edges, whatever their order and start.
  fn canonical(loops: Vec<Vec<Corner>>) -> Vec<std::collections::BTreeSet<(Corner, Corner)>> {
    let mut sets: Vec<_> = loops
      .iter()
      .map(|l| l.windows(2).map(|w| (w[0], w[1])).collect())
      .collect();
    sets.sort();
    sets
  }

  fn assert_matches_a_full_trace(world: &mut World, map: Entity) {
    let tiles = world.get::<TileColliders>(map).unwrap().clone();
    let built = outline_loops(&parts_by_id(world, map), tiles.tile_size.x);
    let full = traced_outlines(tiles.cells_view());
    assert_eq!(canonical(built), canonical(full));
  }

  #[test]
  fn an_edit_rebuilds_only_the_chunks_it_touches() {
    let (mut world, mut s) = world(60);
    // Two islands on one row, in chunks 0 and 1: cells 0 to 3 and 40 to 43
    let mut cells = vec![TileCell::Empty; 44];
    for x in (0..4).chain(40..44) {
      cells[x] = TileCell::Solid;
    }
    let map = world
      .spawn((
        Transform::IDENTITY,
        TileColliders::from_cells(44, 1, Vec2::splat(16.0), cells),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let before = parts_by_id(&mut world, map);
    assert_eq!(before.len(), 2);
    let left = part_in(&before, Vec2::ZERO, Vec2::new(64.0, 16.0));
    let right = part_in(&before, Vec2::new(640.0, 0.0), Vec2::new(704.0, 16.0));
    world
      .get_mut::<TileColliders>(map)
      .unwrap()
      .set(IVec2::new(1, 0), TileCell::Empty);
    run(&mut world, &mut s, 1);
    let after = parts_by_id(&mut world, map);
    assert_eq!(
      loop_count(&mut world, map),
      3,
      "the left island split in two"
    );
    assert_eq!(after.len(), 2, "{after:?}");
    let shape = |parts: &[(Entity, Collider)], part: Entity| {
      parts
        .iter()
        .find(|(e, _)| *e == part)
        .map(|(_, c)| c.shape.clone())
    };
    assert!(
      shape(&after, left).is_some(),
      "the left chunk kept its part"
    );
    assert_ne!(
      shape(&after, left),
      shape(&before, left),
      "rebuilt in place"
    );
    assert_eq!(
      shape(&after, right),
      shape(&before, right),
      "the right chunk's untouched"
    );
    assert_matches_a_full_trace(&mut world, map);
  }

  #[test]
  fn a_pinch_corner_edit_retraces_both_loops() {
    use SlopeCorner::BottomRight;
    let (mut world, mut s) = world(60);
    // Cells (0, 0) and (1, 1) touch at corner (1, 1); cell (36, 1) is far
    // off, in the next chunk
    let mut tiles = TileColliders::new(40, 3, Vec2::splat(16.0));
    for (x, y) in [(0, 0), (1, 1), (36, 1)] {
      tiles.set(xy(x, y), TileCell::Solid);
    }
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    assert_eq!(loop_count(&mut world, map), 3);
    let before = parts_by_id(&mut world, map);
    assert_eq!(before.len(), 2);
    // (0, 0)'s and (1, 1)'s loops are chunk 0's part
    let ab = part_in(&before, Vec2::ZERO, Vec2::new(32.0, 32.0));
    let far = part_in(&before, Vec2::new(576.0, 16.0), Vec2::new(592.0, 32.0));
    // A slope over (1, 1) joins it and changes the edges at the pinch
    // corner; (0, 0)'s own edges stay, but its loop passes through there
    world
      .get_mut::<TileColliders>(map)
      .unwrap()
      .set(IVec2::new(1, 0), TileCell::Slope(BottomRight));
    run(&mut world, &mut s, 1);
    let after = parts_by_id(&mut world, map);
    assert_eq!(after.len(), 2, "{after:?}");
    assert_eq!(loop_count(&mut world, map), 3);
    let shape = |parts: &[(Entity, Collider)], part: Entity| {
      parts
        .iter()
        .find(|(e, _)| *e == part)
        .map(|(_, c)| c.shape.clone())
    };
    let ab_after = shape(&after, ab).expect("chunk 0 keeps its part");
    assert_ne!(
      shape(&before, ab),
      Some(ab_after),
      "the loops of (0, 0) and (1, 1) were retraced into it"
    );
    assert_eq!(
      shape(&after, far),
      shape(&before, far),
      "the far loop was not"
    );
    assert_matches_a_full_trace(&mut world, map);
  }

  #[test]
  fn shape_cells_join_the_outlines_and_one_way_shapes_are_one_way() {
    let mut tiles = TileColliders::new(4, 1, Vec2::splat(16.0));
    let polygon = |points: &[(f32, f32)]| points.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    // An L (the cell's bottom half and its left half), wound
    // counter-clockwise on screen as an editor may
    let l = tiles.add_shape(CellShape {
      polygons: vec![polygon(&[
        (0.0, 0.0),
        (0.0, 1.0),
        (1.0, 1.0),
        (1.0, 0.5),
        (0.5, 0.5),
        (0.5, 0.0),
      ])],
      one_way: false,
    });
    // A one-way slab: the cell's second quarter from the top
    let slab = tiles.add_shape(CellShape {
      polygons: vec![polygon(&[(0.0, 0.25), (1.0, 0.25), (1.0, 0.5), (0.0, 0.5)])],
      one_way: true,
    });
    assert_eq!((l, slab), (0, 1));
    assert!(tiles.shape(slab).is_some_and(|shape| shape.one_way));
    assert!(tiles.shape(2).is_none());
    tiles.set(IVec2::new(0, 0), TileCell::Shape(l));
    tiles.set(IVec2::new(2, 0), TileCell::Shape(slab));
    // A shape no shape was added for builds nothing
    tiles.set(IVec2::new(3, 0), TileCell::Shape(7));
    let segment = |built: &[Collider]| {
      let segments: Vec<&Collider> = built
        .iter()
        .filter(|c| matches!(c.shape, ColliderShape::Segment { .. }))
        .collect();
      assert_eq!(segments.len(), 1, "{built:?}");
      assert_eq!(segments[0].one_way, Some(Vec2::NEG_Y));
      let ColliderShape::Segment { a, b } = segments[0].shape else {
        unreachable!()
      };
      assert_eq!(
        (a.min(b), a.max(b)),
        (Vec2::new(32.0, 4.0), Vec2::new(48.0, 4.0))
      );
    };
    let built_from = |tiles: TileColliders| {
      let (mut world, mut s) = world(60);
      let map = world.spawn((Transform::IDENTITY, tiles)).id();
      run(&mut world, &mut s, 1);
      parts_by_id(&mut world, map)
    };
    // Outlines: the L is an outline loop, the slab a one-way segment
    let with_ids = built_from(tiles.clone());
    let built: Vec<Collider> = with_ids.iter().map(|(_, c)| c.clone()).collect();
    assert_eq!(built.len(), 2, "{built:?}");
    segment(&built);
    let loops = outline_loops(&with_ids, 8.0);
    let l_corners = [(0, 0), (1, 0), (1, 1), (2, 1), (2, 2), (0, 2)];
    assert_eq!(
      canonical(loops),
      canonical(vec![
        l_corners.iter().chain(&l_corners[..1]).copied().collect()
      ]),
      "in half tiles"
    );
    // Merged rectangles: the L is two convex parts
    let built: Vec<Collider> = built_from(tiles.solid_rects())
      .into_iter()
      .map(|(_, c)| c)
      .collect();
    assert_eq!(built.len(), 3, "two parts and the slab: {built:?}");
    segment(&built);
    let polygons: Vec<&Vec<Vec2>> = built
      .iter()
      .filter_map(|c| match &c.shape {
        ColliderShape::Polygon { points } => Some(points),
        _ => None,
      })
      .collect();
    assert_eq!(polygons.len(), 2, "an L is two convex parts: {built:?}");
    let area = |p: &Vec<Vec2>| {
      let n = p.len();
      ((0..n).map(|i| p[i].perp_dot(p[(i + 1) % n])).sum::<f32>() / 2.0).abs()
    };
    let total: f32 = polygons.iter().map(|p| area(p)).sum();
    assert!((total - 0.75 * 256.0).abs() < 1e-3, "{total} px²");
    assert!(
      polygons.iter().all(|p| p
        .iter()
        .all(|v| v.cmpge(Vec2::ZERO).all() && v.cmple(Vec2::splat(16.0)).all())),
      "in the first cell: {polygons:?}"
    );
  }

  #[test]
  fn a_floor_under_a_standing_character_keeps_its_entity_when_a_far_tile_changes() {
    use crate::core::ecs::Messages;
    use crate::core::physics::{CharacterState, CollisionStarted, CollisionStopped};
    let (mut world, mut s) = world(60);
    // A floor on row 2 (cells 0 to 19) and a block far off on row 0 (35 to 37)
    let mut cells = vec![TileCell::Empty; 40 * 3];
    for x in 0..20 {
      cells[2 * 40 + x] = TileCell::Solid;
    }
    for cell in &mut cells[35..38] {
      *cell = TileCell::Solid;
    }
    let map = world
      .spawn((
        Transform::from_xy(0.0, 68.0),
        TileColliders::from_cells(40, 3, Vec2::splat(16.0), cells),
      ))
      .id();
    let e = standing_character(&mut world, 40.0);
    for _ in 0..30 {
      fall_step(&mut world, &mut s, e);
    }
    let state = world.get::<CharacterState>(e).unwrap();
    assert!(state.on_floor);
    let floor = state.floor.expect("a floor");
    let mut started = world
      .resource::<Messages<CollisionStarted>>()
      .get_cursor_current();
    let mut stopped = world
      .resource::<Messages<CollisionStopped>>()
      .get_cursor_current();
    for step in 0..10 {
      let cell = if step % 2 == 0 {
        TileCell::Empty
      } else {
        TileCell::Solid
      };
      assert!(
        world
          .get_mut::<TileColliders>(map)
          .unwrap()
          .set(IVec2::new(36, 0), cell)
      );
      fall_step(&mut world, &mut s, e);
      let state = world.get::<CharacterState>(e).unwrap();
      assert!(
        state.on_floor && state.floor == Some(floor),
        "step {step}: on_floor {}, floor {:?} (was {floor})",
        state.on_floor,
        state.floor
      );
    }
    assert!(ids(&mut world, map).contains(&floor));
    let messages = world.resource::<Messages<CollisionStarted>>();
    let restarted: Vec<_> = started
      .read(messages)
      .filter(|m| m.involves(floor))
      .collect();
    assert!(restarted.is_empty(), "{restarted:?}");
    let messages = world.resource::<Messages<CollisionStopped>>();
    let ended: Vec<_> = stopped
      .read(messages)
      .filter(|m| m.a == floor || m.b == floor)
      .collect();
    assert!(ended.is_empty(), "{ended:?}");
  }

  #[test]
  fn an_edit_elsewhere_in_a_chunk_keeps_the_ground_a_character_stands_on() {
    use crate::core::ecs::Messages;
    use crate::core::physics::{CharacterState, CollisionStarted, CollisionStopped};
    let (mut world, mut s) = world(60);
    // Island A, a floor on row 2 (cells 0 to 19), and island B, a block on
    // row 0 (cells 24 to 26): both in chunk 0, one collider
    let mut cells = vec![TileCell::Empty; 40 * 3];
    for x in 0..20 {
      cells[2 * 40 + x] = TileCell::Solid;
    }
    for cell in &mut cells[24..27] {
      *cell = TileCell::Solid;
    }
    let map = world
      .spawn((
        Transform::from_xy(0.0, 68.0),
        TileColliders::from_cells(40, 3, Vec2::splat(16.0), cells),
      ))
      .id();
    let mut started = world
      .resource::<Messages<CollisionStarted>>()
      .get_cursor_current();
    let e = standing_character(&mut world, 40.0);
    for _ in 0..30 {
      fall_step(&mut world, &mut s, e);
    }
    let state = world.get::<CharacterState>(e).unwrap();
    assert!(state.on_floor);
    let floor = state.floor.expect("a floor");
    assert_eq!(parts_by_id(&mut world, map).len(), 1, "one chunk, one part");
    let messages = world.resource::<Messages<CollisionStarted>>();
    assert!(
      started
        .read(messages)
        .any(|m| m.involves(e) && m.involves(floor)),
      "landing on the ground is a collision"
    );
    let at = world.get::<Transform>(e).unwrap().translation;
    let mut started = world
      .resource::<Messages<CollisionStarted>>()
      .get_cursor_current();
    let mut stopped = world
      .resource::<Messages<CollisionStopped>>()
      .get_cursor_current();
    // A tile on island B comes and goes
    for step in 0..10 {
      let cell = if step % 2 == 0 {
        TileCell::Solid
      } else {
        TileCell::Empty
      };
      assert!(
        world
          .get_mut::<TileColliders>(map)
          .unwrap()
          .set(IVec2::new(27, 0), cell)
      );
      fall_step(&mut world, &mut s, e);
      let state = world.get::<CharacterState>(e).unwrap();
      assert!(
        state.on_floor && state.floor == Some(floor),
        "step {step}: on_floor {}, floor {:?} (was {floor})",
        state.on_floor,
        state.floor
      );
      let now = world.get::<Transform>(e).unwrap().translation;
      assert!(
        (now - at).length() < 1e-3,
        "step {step}: the character moved from {at} to {now}"
      );
    }
    let on_map = |world: &World, entity: Entity| {
      world
        .get::<ChildOf>(entity)
        .is_some_and(|child_of| child_of.parent() == map)
    };
    let messages = world.resource::<Messages<CollisionStarted>>();
    let restarted: Vec<_> = started
      .read(messages)
      .filter(|m| m.involves(e) && (on_map(&world, m.a) || on_map(&world, m.b)))
      .collect();
    assert!(restarted.is_empty(), "{restarted:?}");
    let messages = world.resource::<Messages<CollisionStopped>>();
    let ended: Vec<_> = stopped
      .read(messages)
      .filter(|m| {
        m.involves(e) && (m.involves(floor) || on_map(&world, m.a) || on_map(&world, m.b))
      })
      .collect();
    assert!(ended.is_empty(), "{ended:?}");
  }

  #[test]
  fn an_edit_at_a_loop_s_first_corner_keeps_the_loop_in_its_chunk_s_collider() {
    use crate::core::ecs::Messages;
    use crate::core::physics::{CharacterState, CollisionStarted, CollisionStopped};
    let (mut world, mut s) = world(60);
    // A floor along row 31, the last of chunk row 0. Its loop starts at its
    // bottom-left corner, (0, 32), on chunk row 1's first line. A cell set
    // under its left end straightens that corner (the left side then runs
    // from (0, 33) up to (0, 31)); the loop still starts there, so it stays
    // in that chunk's collider, as does the character standing far along it
    let mut cells = vec![TileCell::Empty; 40 * 34];
    for x in 0..40 {
      cells[31 * 40 + x] = TileCell::Solid;
    }
    let map = world
      .spawn((
        Transform::from_xy(0.0, 100.0 - 31.0 * 16.0),
        TileColliders::from_cells(40, 34, Vec2::splat(16.0), cells),
      ))
      .id();
    let e = standing_character(&mut world, 480.0);
    for _ in 0..30 {
      fall_step(&mut world, &mut s, e);
    }
    let state = world.get::<CharacterState>(e).unwrap();
    assert!(state.on_floor);
    let floor = state.floor.expect("a floor");
    let mut started = world
      .resource::<Messages<CollisionStarted>>()
      .get_cursor_current();
    let mut stopped = world
      .resource::<Messages<CollisionStopped>>()
      .get_cursor_current();
    for step in 0..10 {
      let cell = if step % 2 == 0 {
        TileCell::Solid
      } else {
        TileCell::Empty
      };
      assert!(
        world
          .get_mut::<TileColliders>(map)
          .unwrap()
          .set(IVec2::new(0, 32), cell)
      );
      fall_step(&mut world, &mut s, e);
      let state = world.get::<CharacterState>(e).unwrap();
      assert!(
        state.on_floor && state.floor == Some(floor),
        "step {step}: on_floor {}, floor {:?} (was {floor})",
        state.on_floor,
        state.floor
      );
    }
    let messages = world.resource::<Messages<CollisionStarted>>();
    let restarted: Vec<_> = started.read(messages).filter(|m| m.involves(e)).collect();
    assert!(restarted.is_empty(), "{restarted:?}");
    let messages = world.resource::<Messages<CollisionStopped>>();
    let ended: Vec<_> = stopped.read(messages).filter(|m| m.involves(e)).collect();
    assert!(ended.is_empty(), "{ended:?}");
  }

  // A part reshaped in place is in the queries before the step, as a new
  // part is: a block added in the air, far from the rest of its chunk,
  // stops a character flying into it on that very step
  #[test]
  fn a_block_added_in_front_of_a_flying_character_stops_it_at_once() {
    let (mut world, mut s) = world(60);
    // Ground on row 25, far below; the character flies along y 80 to 120
    let mut tiles = TileColliders::new(32, 30, Vec2::splat(16.0));
    for x in 0..32 {
      tiles.set(xy(x, 25), TileCell::Solid);
    }
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    let e = world
      .spawn((
        Transform::from_xy(40.0, 100.0),
        CharacterController::platformer(),
        Collider::rect(12.0, 40.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let mut added = false;
    for step in 0..30 {
      let right = at(&world, e).translation.x + 6.0;
      // A block from x 160 to 192, y 64 to 144, while the character is
      // under a step's run (10 px) short of it: this step's run would end
      // inside it
      if !added && right > 150.0 {
        let mut tiles = world.get_mut::<TileColliders>(map).unwrap();
        for y in 4..9 {
          tiles.set(xy(10, y), TileCell::Solid);
          tiles.set(xy(11, y), TileCell::Solid);
        }
        added = true;
      }
      world.get_mut::<Velocity>(e).unwrap().dx = 600.0;
      run(&mut world, &mut s, 1);
      if added {
        let right = at(&world, e).translation.x + 6.0;
        assert!(
          right <= 160.5,
          "step {step}: ran into the block, right side {right}"
        );
      }
    }
    assert!(added);
    assert_eq!(parts_by_id(&mut world, map).len(), 1, "one chunk, one part");
  }

  #[test]
  fn an_edit_keeps_a_chunk_collider_s_entity_and_handle() {
    let (mut world, mut s) = world(60);
    // Two islands, and two one-way runs on one row, all in chunk 0
    let mut tiles = TileColliders::new(40, 4, Vec2::splat(16.0));
    for (x, y) in [(0, 0), (1, 0), (6, 0)] {
      tiles.set(xy(x, y), TileCell::Solid);
    }
    for x in (0..4).chain(8..12) {
      tiles.set(xy(x, 3), TileCell::OneWay);
    }
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let registered = |world: &mut World| -> Vec<(Entity, Collider, String)> {
      let parts = parts_by_id(world, map);
      let physics = world.resource::<Physics>();
      parts
        .into_iter()
        .map(|(part, collider)| {
          let handle = physics
            .collider_entries
            .get(&part)
            .map(|entry| entry.handle);
          (part, collider, format!("{handle:?}"))
        })
        .collect()
    };
    let before = registered(&mut world);
    assert_eq!(before.len(), 2, "a loop part and a run part: {before:?}");
    // Island (6, 0) grows and a run gets longer: both parts keep members
    {
      let mut tiles = world.get_mut::<TileColliders>(map).unwrap();
      tiles.set(IVec2::new(7, 0), TileCell::Solid);
      tiles.set(IVec2::new(12, 3), TileCell::OneWay);
    }
    run(&mut world, &mut s, 1);
    let after = registered(&mut world);
    assert_eq!(after.len(), 2, "{after:?}");
    for ((part, old, handle), (now_part, now, now_handle)) in before.iter().zip(&after) {
      assert_eq!(part, now_part, "the part keeps its entity");
      assert!(handle.starts_with("Some"), "{handle}");
      assert_eq!(handle, now_handle, "and its physics handle");
      assert_ne!(old.shape, now.shape, "its shape is the edited one");
    }
    assert_matches_a_full_trace(&mut world, map);
  }

  #[test]
  fn one_way_runs_and_shape_outlines_rebuild_only_where_edited() {
    let (mut world, mut s) = world(60);
    let mut tiles = TileColliders::new(80, 3, Vec2::splat(16.0));
    let square = tiles.add_shape(CellShape {
      polygons: vec![vec![
        Vec2::new(0.25, 0.25),
        Vec2::new(0.75, 0.25),
        Vec2::new(0.75, 0.75),
        Vec2::new(0.25, 0.75),
      ]],
      one_way: false,
    });
    // One-way runs on rows 0 and 2, shapes at (40, 0) and (72, 2) (chunks 1
    // and 2)
    for x in 0..4 {
      tiles.set(xy(x, 0), TileCell::OneWay);
      tiles.set(xy(x, 2), TileCell::OneWay);
    }
    tiles.set(IVec2::new(40, 0), TileCell::Shape(square));
    tiles.set(IVec2::new(72, 2), TileCell::Shape(square));
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let before = parts_by_id(&mut world, map);
    assert_eq!(before.len(), 4);
    let run0 = part_in(&before, Vec2::ZERO, Vec2::new(64.0, 0.0));
    let run2 = part_in(&before, Vec2::new(0.0, 32.0), Vec2::new(64.0, 32.0));
    let shape0 = part_in(&before, Vec2::new(640.0, 0.0), Vec2::new(656.0, 16.0));
    let shape2 = part_in(&before, Vec2::new(1152.0, 32.0), Vec2::new(1168.0, 48.0));
    // A cell on row 0 that leaves its runs as they were keeps them
    world
      .get_mut::<TileColliders>(map)
      .unwrap()
      .set(IVec2::new(6, 0), TileCell::Solid);
    run(&mut world, &mut s, 1);
    let ids: Vec<Entity> = parts_by_id(&mut world, map)
      .iter()
      .map(|(e, _)| *e)
      .collect();
    assert!([run0, run2, shape0, shape2].iter().all(|e| ids.contains(e)));
    // Longer run on row 0; the shape at (72, 2) replaced
    let mut entity = world.entity_mut(map);
    let mut tiles = entity.get_mut::<TileColliders>().unwrap();
    tiles.set(IVec2::new(4, 0), TileCell::OneWay);
    tiles.set(IVec2::new(72, 2), TileCell::Empty);
    run(&mut world, &mut s, 1);
    let after = parts_by_id(&mut world, map);
    let ids: Vec<Entity> = after.iter().map(|(e, _)| *e).collect();
    assert!(
      !ids.contains(&shape2),
      "its chunk's last loop went, so its part did"
    );
    assert!(ids.contains(&run0) && ids.contains(&run2) && ids.contains(&shape0));
    let longer = part_in(&after, Vec2::new(65.0, 0.0), Vec2::new(80.0, 0.0));
    assert_eq!(
      longer, run0,
      "the run's part took its longer shape in place"
    );
    assert!(matches!(
      after.iter().find(|(e, _)| *e == longer).unwrap().1.shape,
      ColliderShape::Segment { a, b } if a.x.min(b.x) == 0.0 && a.x.max(b.x) == 80.0
    ));
  }

  #[test]
  fn a_shape_added_after_cells_name_it_builds_them() {
    let (mut world, mut s) = world(60);
    let tiles = TileColliders::from_cells(2, 1, Vec2::splat(16.0), vec![TileCell::Shape(0); 2]);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    assert!(parts(&mut world, map).is_empty());
    world
      .get_mut::<TileColliders>(map)
      .unwrap()
      .add_shape(CellShape {
        polygons: vec![vec![Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y]],
        one_way: false,
      });
    run(&mut world, &mut s, 1);
    assert_eq!(
      parts(&mut world, map).len(),
      1,
      "two squares are one outline"
    );
  }

  #[test]
  fn more_edits_than_a_step_can_list_rebuild_everything() {
    let (mut world, mut s) = world(60);
    let map = world
      .spawn((
        Transform::IDENTITY,
        TileColliders::new(300, 300, Vec2::splat(16.0)),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let mut entity = world.entity_mut(map);
    let mut tiles = entity.get_mut::<TileColliders>().unwrap();
    // A block of 78,000 cells, more than the change list holds, with holes
    for y in 0..260 {
      for x in 0..300 {
        tiles.set(xy(x, y), TileCell::Solid);
      }
    }
    for (x, y) in [(10, 10), (11, 10), (100, 200), (250, 40)] {
      tiles.set(xy(x, y), TileCell::Empty);
    }
    run(&mut world, &mut s, 1);
    assert_eq!(loop_count(&mut world, map), 4, "an outline and three holes");
    assert_matches_a_full_trace(&mut world, map);
  }

  fn polygon(points: &[(f32, f32)]) -> Vec<Vec2> {
    points.iter().map(|&(x, y)| Vec2::new(x, y)).collect()
  }

  fn solid_shape(tiles: &mut TileColliders, points: &[(f32, f32)]) -> TileCell {
    TileCell::Shape(tiles.add_shape(CellShape {
      polygons: vec![polygon(points)],
      one_way: false,
    }))
  }

  /// How far a frictionless box sent at 300 px/s slides in 4 s along `tiles`,
  /// set on it with its top at `top`.
  fn slide(tiles: TileColliders, top: f32) -> f32 {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::IDENTITY, tiles.with_friction(0.0)));
    let crate_ = world
      .spawn((
        Transform::from_xy(20.0, top - 6.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(12.0, 12.0).with_friction(0.0),
        Velocity::new(300.0, 0.0),
      ))
      .id();
    run(&mut world, &mut s, 240);
    at(&world, crate_).translation.x - 20.0
  }

  // A row of shape cells is one outline, as a row of solid cells is: a box
  // slides along it without meeting a seam (each shape its own collider
  // stopped it at the first, 6 px on)
  #[test]
  fn a_box_sliding_across_a_row_of_shape_cells_never_stops() {
    // 100 half-height blocks (the cells' bottom halves): the top at 24
    let mut tiles = TileColliders::new(100, 2, Vec2::splat(16.0));
    let half = solid_shape(
      &mut tiles,
      &[(0.0, 0.5), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)],
    );
    for x in 0..100 {
      tiles.set(xy(x, 1), half);
    }
    let travelled = slide(tiles, 24.0);
    assert!(travelled >= 0.95 * 1200.0, "half blocks: {travelled}");
    // Top-half blocks and solid cells in turn: the top at 16, one floor
    let mut tiles = TileColliders::new(100, 2, Vec2::splat(16.0));
    let top_half = solid_shape(
      &mut tiles,
      &[(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (0.0, 0.5)],
    );
    for x in 0..100 {
      let cell = if x % 3 == 0 {
        TileCell::Solid
      } else {
        top_half
      };
      tiles.set(xy(x, 1), cell);
    }
    let travelled = slide(tiles, 16.0);
    assert!(
      travelled >= 0.95 * 1200.0,
      "half blocks and solids: {travelled}"
    );
  }

  /// A level `rows` high: flat ground on its last row, and from column 10 a
  /// ramp rising `per` of a tile a cell for `cols` columns (shape cells over
  /// solid ones), flat on top after; and the same ground as one polyline.
  fn authored_ramp(per: f32, cols: u32) -> (TileColliders, Vec<Vec2>, u32) {
    let per_row = (1.0 / per).round() as u32;
    let rise = cols / per_row;
    let rows = rise + 3;
    let width = 10 + cols + 20;
    let mut tiles = TileColliders::new(width, rows, Vec2::splat(16.0));
    let shapes: Vec<TileCell> = (0..per_row)
      .map(|j| {
        let (left, right) = (1.0 - per * j as f32, 1.0 - per * (j + 1) as f32);
        solid_shape(
          &mut tiles,
          &[(0.0, left), (1.0, right), (1.0, 1.0), (0.0, 1.0)],
        )
      })
      .collect();
    for x in 0..width {
      tiles.set(xy(x, rows - 1), TileCell::Solid);
    }
    for x in 10..10 + cols {
      let k = x - 10;
      let row = rows - 2 - k / per_row;
      tiles.set(xy(x, row), shapes[(k % per_row) as usize]);
      for below in row + 1..rows {
        tiles.set(xy(x, below), TileCell::Solid);
      }
    }
    for x in 10 + cols..width {
      for row in rows - 1 - rise..rows {
        tiles.set(xy(x, row), TileCell::Solid);
      }
    }
    let (ground, top) = ((rows - 1) as f32 * 16.0, (rows - 1 - rise) as f32 * 16.0);
    let line = vec![
      Vec2::new(0.0, ground),
      Vec2::new(160.0, ground),
      Vec2::new((10 + cols) as f32 * 16.0, top),
      Vec2::new(width as f32 * 16.0, top),
    ];
    (tiles, line, rows)
  }

  /// A character run up the ground at `speed` from `x`: where it ends and
  /// how many steps it met a wall.
  fn run_up(ground: impl Bundle, x: f32, y: f32, speed: f32, steps: usize) -> (f32, usize) {
    use crate::core::physics::CharacterState;
    let (mut world, mut s) = world(60);
    world.spawn((Transform::IDENTITY, ground));
    let e = world
      .spawn((
        Transform::from_xy(x, y),
        CharacterController::platformer(),
        Collider::rect(12.0, 40.0),
      ))
      .id();
    for _ in 0..30 {
      fall_step(&mut world, &mut s, e);
    }
    let mut walls = 0;
    for _ in 0..steps {
      world.get_mut::<Velocity>(e).unwrap().dx = speed;
      fall_step(&mut world, &mut s, e);
      walls += usize::from(world.get::<CharacterState>(e).unwrap().on_wall);
    }
    (at(&world, e).translation.x, walls)
  }

  // A ramp drawn as shape cells is one outline with the ground under it, so
  // a character runs up it as up one polyline: no wall, no lost ground (each
  // shape its own collider met 1 or 2 walls in some alignments, up to 8.5 px
  // lost at 300 px/s)
  #[test]
  fn a_character_running_up_an_authored_ramp_never_snags() {
    for (per, cols, speed) in [(0.5, 20, 300.0), (0.5, 20, 60.0), (0.25, 40, 60.0)] {
      for k in 0..4 {
        let (tiles, line, rows) = authored_ramp(per, cols);
        let (x, y) = (64.0 + k as f32 * 0.37, (rows - 1) as f32 * 16.0 - 21.0);
        let steps = ((cols + 12) as f32 * 16.0 * 60.0 / speed) as usize;
        let (on_tiles, walls) = run_up(tiles, x, y, speed, steps);
        let (on_line, line_walls) = run_up(Collider::polyline(line), x, y, speed, steps);
        assert_eq!(line_walls, 0, "the polyline itself snags");
        assert!(
          walls == 0 && (on_tiles - on_line).abs() < 0.5,
          "{per} a cell at {speed} px/s from {x}: {walls} walls, at {on_tiles} (the polyline: {on_line})"
        );
      }
    }
  }

  /// The sloped one-way segments among the map's parts, each with its part
  /// and where its part sits, the segment in the map's space, left end first
  fn sloped_runs(world: &mut World, map: Entity) -> Vec<(Entity, Vec2, (Vec2, Vec2))> {
    let mut query = world.query::<(Entity, &ChildOf, &Collider, &Transform)>();
    query
      .iter(world)
      .filter(|(_, child_of, ..)| child_of.parent() == map)
      .filter_map(|(part, _, collider, at)| match collider.shape {
        ColliderShape::Segment { a, b } if collider.one_way.is_some() && a.y != b.y => {
          let (a, b) = (a + at.translation, b + at.translation);
          let ends = if a.x <= b.x { (a, b) } else { (b, a) };
          Some((part, at.translation, ends))
        }
        _ => None,
      })
      .collect()
  }

  // A sloped one-way run is a part of its own, at the chunk of its first
  // (left) point: an edit that lengthens it leftward across a chunk border
  // moves its part to the new chunk, keeping its entity (`repack` writes
  // its `Transform`), so the segment stays where its cells are
  #[test]
  fn a_sloped_one_way_run_lengthened_into_another_chunk_moves_its_part_there() {
    let (mut world, mut s) = world(60);
    let mut tiles = TileColliders::new(80, 12, Vec2::splat(16.0));
    let one_way = |tiles: &mut TileColliders, points: &[(f32, f32)]| {
      TileCell::Shape(tiles.add_shape(CellShape {
        polygons: vec![polygon(points)],
        one_way: true,
      }))
    };
    // A ramp rising half a tile a cell, a pair of cells a row: the low and
    // the high half of each step
    let low = one_way(&mut tiles, &[(0.0, 1.0), (1.0, 0.5), (1.0, 1.0)]);
    let high = one_way(
      &mut tiles,
      &[(0.0, 0.5), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
    );
    let step = |tiles: &mut TileColliders, x: u32, y: u32| {
      tiles.set(xy(x, y), low);
      tiles.set(xy(x + 1, y), high);
    };
    // Cells 34 to 37, in chunk 1
    step(&mut tiles, 34, 6);
    step(&mut tiles, 36, 5);
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let cell = 16.0;
    let before = sloped_runs(&mut world, map);
    assert_eq!(
      before.iter().map(|r| (r.1, r.2)).collect::<Vec<_>>(),
      [(
        Vec2::new(32.0 * cell, 0.0),
        (
          Vec2::new(34.0 * cell, 7.0 * cell),
          Vec2::new(38.0 * cell, 5.0 * cell)
        )
      )],
      "its part at chunk 1's top-left"
    );
    // Lengthened by two steps leftward, across the border at cell 32
    {
      let mut tiles = world.get_mut::<TileColliders>(map).unwrap();
      step(&mut tiles, 30, 8);
      step(&mut tiles, 32, 7);
    }
    run(&mut world, &mut s, 1);
    let after = sloped_runs(&mut world, map);
    assert_eq!(after.len(), 1, "{after:?}");
    assert_eq!(after[0].0, before[0].0, "the part keeps its entity");
    assert_eq!(after[0].1, Vec2::ZERO, "and moves to chunk 0's top-left");
    assert_eq!(
      after[0].2,
      (
        Vec2::new(30.0 * cell, 9.0 * cell),
        Vec2::new(38.0 * cell, 5.0 * cell)
      ),
      "the segment lies on its cells"
    );
    assert_matches_a_full_trace(&mut world, map);
  }

  // One-way shapes' tops join collinear one-way neighbours (one-way cells
  // and one-way shapes, across rows too) into single segments
  #[test]
  fn a_one_way_shape_row_is_one_segment() {
    let (mut world, mut s) = world(60);
    let mut tiles = TileColliders::new(6, 6, Vec2::splat(16.0));
    let one_way = |tiles: &mut TileColliders, points: &[(f32, f32)]| {
      TileCell::Shape(tiles.add_shape(CellShape {
        polygons: vec![polygon(points)],
        one_way: true,
      }))
    };
    // Row 0: slabs a quarter down the cells
    let slab = one_way(
      &mut tiles,
      &[(0.0, 0.25), (1.0, 0.25), (1.0, 0.5), (0.0, 0.5)],
    );
    // Row 2: slabs flush with the cells' tops, and one-way cells
    let flush = one_way(
      &mut tiles,
      &[(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (0.0, 0.5)],
    );
    // Rows 5 to 3: a one-way ramp rising half a tile a cell, across rows
    let low = one_way(&mut tiles, &[(0.0, 1.0), (1.0, 0.5), (1.0, 1.0)]);
    let high = one_way(
      &mut tiles,
      &[(0.0, 0.5), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
    );
    for x in 0..6 {
      tiles.set(xy(x, 0), slab);
      tiles.set(xy(x, 2), if x % 2 == 0 { flush } else { TileCell::OneWay });
    }
    for (x, y, cell) in [(0, 5, low), (1, 5, high), (2, 4, low), (3, 4, high)] {
      tiles.set(xy(x, y), cell);
    }
    let map = world.spawn((Transform::IDENTITY, tiles)).id();
    run(&mut world, &mut s, 1);
    let mut segments: Vec<(Vec2, Vec2, Option<Vec2>)> = parts(&mut world, map)
      .iter()
      .filter_map(|c| match c.shape {
        ColliderShape::Segment { a, b } if a.x <= b.x => Some((a, b, c.one_way)),
        ColliderShape::Segment { a, b } => Some((b, a, c.one_way)),
        _ => None,
      })
      .collect();
    segments.sort_by(|p, q| p.0.y.total_cmp(&q.0.y));
    let ramp_up = Vec2::new(-1.0, -2.0).normalize();
    assert_eq!(segments.len(), 3, "{segments:?}");
    assert_eq!(
      segments[0],
      (Vec2::new(0.0, 4.0), Vec2::new(96.0, 4.0), Some(Vec2::NEG_Y))
    );
    assert_eq!(
      segments[1],
      (
        Vec2::new(0.0, 32.0),
        Vec2::new(96.0, 32.0),
        Some(Vec2::NEG_Y)
      )
    );
    let (a, b, facing) = segments[2];
    assert_eq!((a, b), (Vec2::new(0.0, 96.0), Vec2::new(64.0, 64.0)));
    assert!(
      facing.is_some_and(|f| f.distance(ramp_up) < 1e-6),
      "{facing:?}"
    );
  }
}
