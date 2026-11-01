//! Tile colliders: a grid of cells becomes seamless outlines (or merged
//! rectangles), one-way runs, 45° slopes and authored shapes. Coordinates
//! here are in tiles, +Y down, cell (0, 0)'s top-left at the origin; the
//! outlines and runs are traced on a finer grid, `GRID` points a tile.

use std::sync::atomic::{AtomicU64, Ordering};

use glam::{IVec2, Mat2, UVec2};
use rapier2d::prelude::SharedShape;

use super::collider::{Collider, PrebuiltShape};
use super::world::Physics;
use crate::core::ecs::{
  Allow, Changed, ChildOf, Children, Commands, Component, DetectChangesMut, Disabled, Entity,
  EntityHashSet, Has, Or, Query, RemovedComponents, ResMut, Transform, Vec2, With,
};
use crate::core::hash::FastMap;

mod outline;
mod pack;
mod runs;
mod shape;

#[cfg(test)]
use outline::outlines;
use outline::{Outlines, Unit, unit_edges};
use pack::{Geometry, PackKey, Packs, ends, loop_key, ring, run_key};
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
/// it apart).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CellShape {
  /// Polygons in tile units ((0, 0) the cell's top-left, (1, 1) its
  /// bottom-right, y down), wound either way. Repeated and straight-through
  /// points are dropped and a polygon left with no area builds nothing; one
  /// that is not simple (it crosses or touches itself), or polygons that
  /// overlap one another in the cell, collide as their convex hull (logged)
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
/// the nearest. A side of `MAX_SIDE` tiles is under 2^31 points.
pub(crate) const GRID: i32 = 11520;

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

/// A grid's cells and shapes, as the outlines and runs read them.
#[derive(Clone, Copy)]
pub(crate) struct Cells<'a> {
  width: u32,
  /// The columns and rows worth reading (`extent`)
  cols: i32,
  rows: i32,
  cells: &'a [TileCell],
  shapes: &'a [(CellShape, ShapeBuild)],
}

impl<'a> Cells<'a> {
  fn new(
    width: u32,
    height: u32,
    cells: &'a [TileCell],
    shapes: &'a [(CellShape, ShapeBuild)],
  ) -> Self {
    let (cols, rows) = extent(width, height, cells);
    // A grid holds at most `MAX_SIDE` cells a side; `cells` bounds a bare call
    let most = (i32::MAX / GRID - 1) as u32;
    Self {
      width,
      cols: cols.min(most) as i32,
      rows: rows.min(most) as i32,
      cells,
      shapes,
    }
  }

  /// Cells with no shapes (the pure outline functions' tests).
  #[cfg(test)]
  fn bare(width: u32, height: u32, cells: &'a [TileCell]) -> Self {
    Self::new(width, height, cells, &[])
  }

  pub(crate) fn width(&self) -> u32 {
    self.width
  }

  pub(crate) fn extent(&self) -> (i32, i32) {
    (self.cols, self.rows)
  }

  /// The cell at (x, y); `Empty` outside.
  pub(crate) fn at(&self, x: i32, y: i32) -> TileCell {
    if x < 0 || y < 0 || x >= self.cols || y >= self.rows {
      return TileCell::Empty;
    }
    cell(self.width, self.cells, x as u32, y as u32)
  }

  /// The cell at (x, y) as it was before `changed` (sorted by index, each
  /// with its value before).
  pub(crate) fn before(&self, x: i32, y: i32, changed: &[(u32, TileCell)]) -> TileCell {
    if x < 0 || y < 0 || x >= self.cols || y >= self.rows {
      return TileCell::Empty;
    }
    let index = y as u32 * self.width + x as u32;
    match changed.binary_search_by_key(&index, |&(i, _)| i) {
      Ok(i) => changed[i].1,
      Err(_) => self.at(x, y),
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

/// The columns and rows worth scanning: a grid claiming more than its
/// `cells` hold costs nothing and cannot overflow.
fn extent(width: u32, height: u32, cells: &[TileCell]) -> (u32, u32) {
  if width == 0 {
    return (0, 0);
  }
  let len = cells.len().min(u32::MAX as usize);
  let rows = cells.len().div_ceil(width as usize).min(u32::MAX as usize) as u32;
  (width.min(len as u32), height.min(rows))
}

/// The cell at (x, y); missing cells are empty.
fn cell(width: u32, cells: &[TileCell], x: u32, y: u32) -> TileCell {
  let index = y as usize * width as usize + x as usize;
  cells.get(index).copied().unwrap_or_default()
}

/// Drop the points in the middle of straight runs; `points` is closed (its
/// last repeats its first) and so is the result.
fn merge_straight_runs(points: &[Point]) -> Vec<Point> {
  let ring = &points[..points.len().saturating_sub(1)];
  let n = ring.len();
  let step = |a: Point, b: Point| (i64::from(b.0 - a.0), i64::from(b.1 - a.1));
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

/// Solid cells greedily merged into rectangles: (min corner, size).
pub(crate) fn solid_rects(width: u32, height: u32, cells: &[TileCell]) -> Vec<(Corner, Corner)> {
  let (cols, height) = extent(width, height, cells);
  let solid = |x: u32, y: u32| cell(width, cells, x, y) == TileCell::Solid;
  let mut taken = vec![false; cols as usize * height as usize];
  let index = |x: u32, y: u32| y as usize * cols as usize + x as usize;
  let mut rects = Vec::new();
  for y in 0..height {
    for x in 0..cols {
      if !solid(x, y) || taken[index(x, y)] {
        continue;
      }
      let mut w = 1;
      while x + w < cols && solid(x + w, y) && !taken[index(x + w, y)] {
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
/// The most cells a grid holds a side (so 2^26 in all, and under 2^31
/// outline points across); a larger size is empty.
const MAX_SIDE: u32 = 8192;

/// The most cell changes a grid lists between rebuilds; past it the next
/// rebuild builds everything.
const MAX_CHANGES: usize = 1 << 16;

/// A grid of tile cells that builds its own colliders: seamless outlines
/// around solid, slope and authored shape cells (`add_shape`,
/// `TileCell::Shape`), or merged rectangles with `solid_rects()`, and
/// one-way segments along runs of one-way cells and shapes. Its `Transform`
/// places cell (0, 0)'s top-left; +Y is down. The colliders are child
/// entities, brought up to date before the next physics step after a
/// change. The outlines are packed by area: the loops starting in a chunk of
/// 32 by 32 cells are one collider, and so are the flat one-way runs
/// starting in a chunk along one row (a sloped run is a collider of its
/// own). An edit retraces the outlines and runs through the cells it
/// changed and gives the colliders of only the chunks whose outlines or
/// runs changed their new shape in place, the same entities, so what
/// stands on the rest of such a chunk keeps its contacts; every other
/// collider stays as it was. With no body they are fixed; with a kinematic
/// `RigidBody` the whole map moves.
///
/// Disabling the map entity (`Disabled`) removes its colliders and enabling
/// it builds them again from the cells as they then are; so does removing
/// the component. A grid over 8192 cells a side is logged and empty.
///
/// Outlines are hollow: a body that ends up inside a solid region is not
/// pushed out. `solid_rects()` trades the seamless floors for solid
/// interiors (and rebuilds everything on any change).
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(Transform)]
pub struct TileColliders {
  width: u32,
  height: u32,
  cells: Vec<TileCell>,
  /// One cell's size, px
  pub tile_size: Vec2,
  pub layer: u32,
  pub mask: u32,
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
  changes: Option<Vec<(u32, TileCell)>>,
  changes_since: u64,
}

static GENERATIONS: AtomicU64 = AtomicU64::new(1);

fn next_generation() -> u64 {
  GENERATIONS.fetch_add(1, Ordering::Relaxed)
}

impl TileColliders {
  /// An empty grid.
  pub fn new(width: u32, height: u32, tile_size: Vec2) -> Self {
    Self::from_cells(width, height, tile_size, Vec::new())
  }

  /// A grid from its cells, row by row from the top; missing cells are
  /// empty and extra ones are dropped. A side over 8192 cells (a bad size,
  /// not a map) is logged and makes a 0 x 0 grid.
  pub fn from_cells(width: u32, height: u32, tile_size: Vec2, mut cells: Vec<TileCell>) -> Self {
    let (width, height) = if width <= MAX_SIDE && height <= MAX_SIDE {
      cells.resize(width as usize * height as usize, TileCell::Empty);
      (width, height)
    } else {
      crate::engine_warn!(
        module: "Physics",
        "a {width} x {height} TileColliders is over the limit of 8192 cells a side; it is empty"
      );
      cells.clear();
      (0, 0)
    };
    let shape_refs = cells
      .iter()
      .filter_map(|cell| match cell {
        TileCell::Shape(index) => Some(u32::from(*index) + 1),
        _ => None,
      })
      .max()
      .unwrap_or(0);
    let generation = next_generation();
    Self {
      width,
      height,
      cells,
      tile_size,
      layer: 1,
      mask: u32::MAX,
      friction: 0.5,
      restitution: 0.0,
      solid_rects: false,
      generation,
      shapes: Vec::new(),
      shape_refs,
      changes: Some(Vec::new()),
      changes_since: generation,
    }
  }

  pub fn width(&self) -> u32 {
    self.width
  }

  pub fn height(&self) -> u32 {
    self.height
  }

  /// The cell at (x, y); `Empty` outside the grid.
  pub fn get(&self, x: u32, y: u32) -> TileCell {
    if x < self.width && y < self.height {
      self.cells[y as usize * self.width as usize + x as usize]
    } else {
      TileCell::Empty
    }
  }

  /// Change a cell (its colliders rebuild next step). Returns false, and
  /// changes nothing, for a cell outside the grid or one already `cell`.
  pub fn set(&mut self, x: u32, y: u32, cell: TileCell) -> bool {
    if x >= self.width || y >= self.height {
      return false;
    }
    let index = y as usize * self.width as usize + x as usize;
    let slot = &mut self.cells[index];
    if *slot == cell {
      return false;
    }
    let before = std::mem::replace(slot, cell);
    if let TileCell::Shape(shape) = cell {
      self.shape_refs = self.shape_refs.max(u32::from(shape) + 1);
    }
    self.generation = next_generation();
    match &mut self.changes {
      // A grid holds at most 2^26 cells, so an index fits
      Some(changes) if changes.len() < MAX_CHANGES => changes.push((index as u32, before)),
      _ => self.changes = None,
    }
    true
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
    Cells::new(self.width, self.height, &self.cells, &self.shapes)
  }

  /// The cells changed since the rebuild last took them in, sorted by
  /// index, each once with its value then; None when there were too many to
  /// list. The list starts again from now.
  pub(crate) fn take_changes(&mut self) -> Option<Vec<(u32, TileCell)>> {
    let changes = self.changes.replace(Vec::new());
    self.changes_since = self.generation;
    changes.map(|mut list| {
      // Stable: a cell's first entry holds its value before them all
      list.sort_by_key(|&(index, _)| index);
      list.dedup_by_key(|&mut (index, _)| index);
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

  fn px(&self, (x, y): Corner) -> Vec2 {
    Vec2::new(x as f32 * self.tile_size.x, y as f32 * self.tile_size.y)
  }

  fn material(&self, collider: Collider) -> Collider {
    collider
      .with_layer(self.layer)
      .with_mask(self.mask)
      .with_friction(self.friction)
      .with_restitution(self.restitution)
  }

  /// A part of the map, in its space with the grid's material.
  fn spawn(&self, commands: &mut Commands, map: Entity, collider: Collider) -> Entity {
    let collider = self.material(collider);
    commands
      .spawn((Transform::IDENTITY, collider, TilePart, ChildOf(map)))
      .id()
  }

  /// A point of the outline grid in the map's space, px.
  fn at(&self, (x, y): Point) -> Vec2 {
    let grid = f64::from(GRID);
    Vec2::new(
      (f64::from(x) / grid * f64::from(self.tile_size.x)) as f32,
      (f64::from(y) / grid * f64::from(self.tile_size.y)) as f32,
    )
  }

  /// A one-way run as a part of its own (`solid_rects` mode): a segment
  /// from its first point to its last, solid from the side it faces (up,
  /// for a flat run).
  fn spawn_run(&self, commands: &mut Commands, map: Entity, points: &[Point]) -> Entity {
    let (a, b) = (self.at(points[0]), self.at(points[points.len() - 1]));
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
    if self.solid_rects {
      let runs = Runs::build(self.cells_view());
      let (cols, rows) = extent(self.width, self.height, &self.cells);
      for (min, cells) in solid_rects(self.width, self.height, &self.cells) {
        let extent = self.px(cells);
        let centre = self.px(min) + extent / 2.0;
        let rect = Collider::rect(extent.x, extent.y).with_offset(centre.x, centre.y);
        self.spawn(commands, map, rect);
      }
      for y in 0..rows {
        for x in 0..cols {
          let origin = Vec2::new(x as f32, y as f32);
          match self.get(x, y) {
            cell @ TileCell::Slope(_) => {
              let points = corners(cell)
                .iter()
                .map(|c| self.px((x as i32 + c.0, y as i32 + c.1)));
              self.spawn(commands, map, Collider::polygon(points));
            }
            TileCell::Shape(shape) => {
              let parts = self
                .shapes
                .get(usize::from(shape))
                .map_or(&[][..], |(_, build)| &build.parts[..]);
              for part in parts {
                let points = part.iter().map(|&p| (origin + p) * self.tile_size);
                self.spawn(commands, map, Collider::polygon(points));
              }
            }
            _ => {}
          }
        }
      }
      for (_, run) in runs.runs() {
        self.spawn_run(commands, map, &run.points);
      }
      return None;
    }
    let mut kept = Kept {
      outlines: Outlines::build(self.cells_view()),
      runs: Runs::build(self.cells_view()),
      ..Kept::default()
    };
    for (key, collider) in self.pack_all(&mut kept) {
      self.spawn_pack(&mut kept, key, collider, None, commands, map);
    }
    Some(kept)
  }

  /// Puts every loop and run of `kept` in its pack; each pack's collider
  /// (without the grid's material), in the order the packs were met.
  fn pack_all(&self, kept: &mut Kept) -> Vec<(PackKey, Collider)> {
    let mut gathered: FastMap<PackKey, Geometry> = FastMap::default();
    let mut order = Vec::new();
    let mut dirty = Vec::new();
    for (id, outline) in kept.outlines.loops() {
      let key = loop_key(&outline.corners);
      let ring = ring(&outline.corners);
      kept.loop_packs.place(id, key, &mut dirty);
      dirty.clear();
      gathered
        .entry(key)
        .or_insert_with(|| {
          order.push(key);
          Geometry::default()
        })
        .add_ring(ring.into_iter().map(|p| self.at(p)));
    }
    for (id, run) in kept.runs.runs() {
      let key = run_key(id, &run.points);
      kept.run_packs.place(id, key, &mut dirty);
      dirty.clear();
      let (a, b) = ends(&run.points);
      gathered
        .entry(key)
        .or_insert_with(|| {
          order.push(key);
          Geometry::default()
        })
        .add_segment(self.at(a), self.at(b));
    }
    order
      .into_iter()
      .map(|key| {
        let geometry = gathered.remove(&key).unwrap_or_default();
        let collider = match key {
          PackKey::Loops(..) => geometry.loops(),
          _ => geometry.runs(),
        };
        (key, collider)
      })
      .collect()
  }

  /// Spawns pack `key`'s part, `collider` with the grid's material (and its
  /// shape built ahead, if any), as the part of the pack's members.
  fn spawn_pack(
    &self,
    kept: &mut Kept,
    key: PackKey,
    collider: Collider,
    shape: Option<SharedShape>,
    commands: &mut Commands,
    map: Entity,
  ) {
    let collider = self.material(collider);
    let part = (Transform::IDENTITY, collider, TilePart, ChildOf(map));
    let part = match shape {
      Some(shape) => commands.spawn((part, PrebuiltShape(shape))).id(),
      None => commands.spawn(part).id(),
    };
    self.adopt(kept, key, part);
  }

  /// Records `part` as pack `key`'s, and as the part of each of its
  /// members.
  fn adopt(&self, kept: &mut Kept, key: PackKey, part: Entity) {
    if let PackKey::Loops(..) = key {
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
    if self.solid_rects {
      return None;
    }
    let mut kept = Kept {
      outlines: Outlines::build(self.cells_view()),
      runs: Runs::build(self.cells_view()),
      ..Kept::default()
    };
    let built = self
      .pack_all(&mut kept)
      .into_iter()
      .map(|(key, collider)| {
        let shape = super::shape::build(&collider.shape, Mat2::IDENTITY);
        (key, collider, shape)
      })
      .collect();
    Some(Traced {
      generation: self.generation,
      tile_size: self.tile_size,
      content: Some((kept, built)),
    })
  }

  /// Rebuild what the cells at `changed` touch (sorted by index, each with
  /// its value before): the outline loops and one-way runs through them,
  /// and the colliders of the packs they leave or join (`alive` tells a
  /// part still there).
  fn respawn(
    &self,
    kept: &mut Kept,
    changed: &[(u32, TileCell)],
    alive: &dyn Fn(Entity) -> bool,
    commands: &mut Commands,
    map: Entity,
  ) {
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
    let collider = if loops {
      let outlines = &kept.outlines;
      let members = kept.loop_packs.settle(key, |id| outlines.get(id).is_some());
      for outline in members.iter().filter_map(|&id| outlines.get(id)) {
        geometry.add_ring(ring(&outline.corners).into_iter().map(|p| self.at(p)));
      }
      (!members.is_empty()).then(|| geometry.loops())
    } else {
      let runs = &kept.runs;
      let members = kept.run_packs.settle(key, |id| runs.get(id).is_some());
      for run in members.iter().filter_map(|&id| runs.get(id)) {
        let (a, b) = ends(&run.points);
        geometry.add_segment(self.at(a), self.at(b));
      }
      (!members.is_empty()).then(|| geometry.runs())
    };
    let packs = if loops {
      &mut kept.loop_packs
    } else {
      &mut kept.run_packs
    };
    let part = packs.part(key).filter(|&part| alive(part));
    match (collider, part) {
      (Some(collider), Some(part)) => {
        // A shape built ahead for its landing is no longer its shape
        commands
          .entity(part)
          .try_insert(self.material(collider))
          .try_remove::<PrebuiltShape>();
        self.adopt(kept, key, part);
      }
      (collider, _) => {
        if let Some(old) = packs.take_part(key) {
          commands.entity(old).try_despawn();
        }
        if let Some(collider) = collider {
          self.spawn_pack(kept, key, collider, None, commands, map);
        }
      }
    }
  }
}

/// Cells and authored shapes whose boundary is read a block at a time, by
/// the outline code's units without its loops or colliders: what a tile
/// map's light occluders are built from, so their edges meet and cancel as
/// the outlines' do, across blocks too.
#[derive(Debug, Default)]
pub(crate) struct Boundary {
  shapes: Vec<(CellShape, ShapeBuild)>,
}

impl Boundary {
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
  /// (exclusive) of a grid `size` cells own: each one's inside, the border
  /// on its left and the border above it, and, where the block reaches the
  /// grid's right or bottom, the borders there. So every border belongs to
  /// one block, and blocks laid side by side give the grid's boundary with
  /// nothing twice. `at` gives a cell by its place in the grid (a border
  /// reads the cell across it, outside the block, and every cell outside the
  /// grid is empty). Points are on the outline grid (`GRID` a tile) from
  /// `min`'s top-left; edges run clockwise on screen round what they bound.
  pub(crate) fn edges(
    &self,
    at: impl Fn(i32, i32) -> TileCell,
    min: (i32, i32),
    max: (i32, i32),
    size: (i32, i32),
    out: &mut Vec<(Point, Point)>,
  ) {
    let cells = Cells::new(0, 0, &[], &self.shapes);
    let local = |x: i32, y: i32| {
      let (gx, gy) = (min.0 + x, min.1 + y);
      if gx < 0 || gy < 0 || gx >= size.0 || gy >= size.1 {
        TileCell::Empty
      } else {
        at(gx, gy)
      }
    };
    let (w, h) = (max.0 - min.0, max.1 - min.1);
    for y in 0..h {
      for x in 0..w {
        unit_edges(Unit::Inside(x, y), &cells, &local, out);
        unit_edges(Unit::Left(x, y), &cells, &local, out);
        unit_edges(Unit::Above(x, y), &cells, &local, out);
      }
      if max.0 == size.0 {
        unit_edges(Unit::Left(w, y), &cells, &local, out);
      }
    }
    if max.1 == size.1 {
      for x in 0..w {
        unit_edges(Unit::Above(x, h), &cells, &local, out);
      }
    }
  }
}

/// Marks a collider a `TileColliders` built; its parent owns it.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct TilePart;

/// What the colliders of a map were last built from. Floats compare by
/// bits, so a NaN size does not rebuild every step.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TileRecord {
  generation: u64,
  tile_size: [u32; 2],
  layer: u32,
  mask: u32,
  friction: u32,
  restitution: u32,
  solid_rects: bool,
}

impl TileRecord {
  fn of(tiles: &TileColliders) -> Self {
    Self {
      generation: tiles.generation,
      tile_size: [tiles.tile_size.x.to_bits(), tiles.tile_size.y.to_bits()],
      layer: tiles.layer,
      mask: tiles.mask,
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
}

/// A pack's collider, without the grid's material, and its Rapier shape
/// built ahead (none when degenerate).
type BuiltPack = (PackKey, Collider, Option<SharedShape>);

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
          let current =
            traced.generation == tiles.generation && traced.tile_size == tiles.tile_size;
          (current && !tiles.solid_rects)
            .then(|| traced.content.take())
            .flatten()
        });
        let kept = if !(tiles.tile_size.is_finite() && tiles.tile_size.min_element() > 0.0) {
          physics.warn_once(
            map,
            "TileColliders needs a positive tile size; no colliders built",
          );
          None
        } else if let Some((mut kept, built)) = traced {
          for (key, collider, shape) in built {
            tiles.spawn_pack(&mut kept, key, collider, shape, &mut commands, map);
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
        tiles.set(x, y, kinds[seed as usize % kinds.len()]);
      }
    }
    let mut whole = outline::boundary_edges(tiles.cells_view());
    let at = |x: i32, y: i32| tiles.get(x as u32, y as u32);
    let mut pieced = Vec::new();
    let size = (width as i32, height as i32);
    for by in (0..size.1).step_by(4) {
      for bx in (0..size.0).step_by(4) {
        let max = ((bx + 4).min(size.0), (by + 4).min(size.1));
        let mut block = Vec::new();
        boundary.edges(at, (bx, by), max, size, &mut block);
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
  fn a_grid_too_wide_for_outlines_is_refused_or_warns() {
    // Over 8192 cells a side is refused (logged), as too many cells are
    for (w, h) in [(8193, 1), (1, 8193), (100_000, 1)] {
      let grid = TileColliders::new(w, h, Vec2::ONE);
      assert_eq!((grid.width(), grid.height()), (0, 0), "{w} x {h}");
    }
    // The widest grid it takes builds its outlines to its far end
    let mut grid = TileColliders::new(8192, 1, Vec2::ONE);
    assert!(grid.set(8191, 0, S));
    let loops = outlines(grid.width(), grid.height(), &grid.cells);
    assert_eq!(loops.len(), 1);
    assert!(loops[0].contains(&(8192, 1)), "{loops:?}");
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

#[cfg(test)]
mod with_physics {
  use super::*;
  use crate::core::ecs::{Bundle, ChildOf, Disabled, Transform, Velocity, World};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{CellShape, SlopeCorner};
  use crate::core::physics::{CharacterController, Collider, ColliderShape, Physics, RigidBody};

  fn parts(world: &mut World, map: Entity) -> Vec<Collider> {
    let mut query = world.query::<(&ChildOf, &Collider)>();
    query
      .iter(world)
      .filter(|(child_of, _)| child_of.parent() == map)
      .map(|(_, collider)| collider.clone())
      .collect()
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
      .set(1, 0, TileCell::Empty);
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
      .set(35, 0, TileCell::Empty);
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
        .set(38, 0, cell);
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
        .set(38, 1, cell);
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
            tiles.set(x, y, TileCell::Solid);
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
        skyline.set(x, y, TileCell::Solid);
      }
    }
    time_edit("a skyline", skyline, (64, tops[64] - 1));
    // Every other row across from row 1, joined at alternate ends; the edit
    // is a bump on its top
    let mut serpentine = islands();
    for y in (1..32).step_by(2) {
      for x in 0..128 {
        serpentine.set(x, y, TileCell::Solid);
      }
      if y < 31 {
        let end = if y % 4 == 1 { 127 } else { 0 };
        serpentine.set(end, y + 1, TileCell::Solid);
      }
    }
    time_edit("a serpentine", serpentine, (64, 0));
  }

  fn time_edit(name: &str, tiles: TileColliders, (column, row): (u32, u32)) {
    use crate::core::ecs::Schedule;
    use std::time::{Duration, Instant};
    let size = tiles.width();
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
      assert!(tiles.set(column, row, cell(i)));
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
    let mut cells = tiles.cells.clone();
    let index = row * size + column;
    let mut times = Vec::new();
    for i in 0..400 {
      let before = std::mem::replace(&mut cells[index as usize], cell(i));
      let view = Cells {
        cells: &cells,
        ..tiles.cells_view()
      };
      let start = Instant::now();
      let retraced = outlines.apply(view, &[(index, before)]);
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
    tiles.set(1, 0, TileCell::Empty);
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
        .set(0, 0, TileCell::Solid)
    );
    assert!(
      !world
        .get_mut::<TileColliders>(map)
        .unwrap()
        .set(99, 0, TileCell::Solid)
    );
    run(&mut world, &mut s, 2);
    assert_eq!(ids(&mut world, map), before);
    world.get_mut::<TileColliders>(map).unwrap().layer = 4;
    run(&mut world, &mut s, 1);
    let after = ids(&mut world, map);
    assert_eq!(after.len(), 1);
    assert_ne!(after, before);
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
      tiles.set(1, 0, TileCell::Solid);
      tiles.set(2, 0, TileCell::Solid);
      tiles.set(3, 0, TileCell::Solid);
      tiles.set(116, 36, TileCell::Empty);
    }
    run(&mut world, &mut s, 1);
    assert!(world.get::<Traced>(map).is_none(), "the stale trace went");
    let built = canonical(outline_loops(&parts_by_id(&mut world, map), 16.0));
    let full = canonical(outlines(
      120,
      40,
      &world.get::<TileColliders>(map).unwrap().cells,
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
      let full = canonical(outlines(
        w,
        h,
        &world.get::<TileColliders>(map).unwrap().cells,
      ));
      assert_eq!(
        built, full,
        "{step}: each loop once, as a whole build has them"
      );
    };
    let mut set = |world: &mut World, cells: &[(u32, u32)], kind: TileCell| {
      let mut tiles = world.get_mut::<TileColliders>(map).unwrap();
      for &(x, y) in cells {
        tiles.set(x, y, kind);
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
      let _ = solid_rects(w, h, &cells);
    }
    assert_eq!(solid_rects(4, u32::MAX, &cells), [((0, 0), (4, 1))]);
    assert_eq!(solid_rects(u32::MAX, 1, &cells), [((0, 0), (4, 1))]);
    assert_eq!(outlines(4, u32::MAX, &cells).len(), 1);
    assert_eq!(outlines(u32::MAX, 1, &cells).len(), 1);
    let grid = TileColliders::from_cells(u32::MAX, u32::MAX, Vec2::ONE, vec![S; 4]);
    assert_eq!((grid.width(), grid.height()), (0, 0));
    assert_eq!(grid.get(0, 0), TileCell::Empty);
  }

  /// The map's parts, by entity, sorted.
  fn parts_by_id(world: &mut World, map: Entity) -> Vec<(Entity, Collider)> {
    let mut query = world.query::<(Entity, &ChildOf, &Collider)>();
    let mut found: Vec<(Entity, Collider)> = query
      .iter(world)
      .filter(|(_, child_of, _)| child_of.parent() == map)
      .map(|(e, _, collider)| (e, collider.clone()))
      .collect();
    found.sort_by_key(|(e, _)| *e);
    found
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
    let full = outlines(tiles.width(), tiles.height(), &tiles.cells);
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
      .set(1, 0, TileCell::Empty);
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
      tiles.set(x, y, TileCell::Solid);
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
      .set(1, 0, TileCell::Slope(BottomRight));
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
    tiles.set(0, 0, TileCell::Shape(l));
    tiles.set(2, 0, TileCell::Shape(slab));
    // A shape no shape was added for builds nothing
    tiles.set(3, 0, TileCell::Shape(7));
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
          .set(36, 0, cell)
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
          .set(27, 0, cell)
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
          .set(0, 32, cell)
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
      tiles.set(x, 25, TileCell::Solid);
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
          tiles.set(10, y, TileCell::Solid);
          tiles.set(11, y, TileCell::Solid);
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
      tiles.set(x, y, TileCell::Solid);
    }
    for x in (0..4).chain(8..12) {
      tiles.set(x, 3, TileCell::OneWay);
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
      tiles.set(7, 0, TileCell::Solid);
      tiles.set(12, 3, TileCell::OneWay);
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
      tiles.set(x, 0, TileCell::OneWay);
      tiles.set(x, 2, TileCell::OneWay);
    }
    tiles.set(40, 0, TileCell::Shape(square));
    tiles.set(72, 2, TileCell::Shape(square));
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
      .set(6, 0, TileCell::Solid);
    run(&mut world, &mut s, 1);
    let ids: Vec<Entity> = parts_by_id(&mut world, map)
      .iter()
      .map(|(e, _)| *e)
      .collect();
    assert!([run0, run2, shape0, shape2].iter().all(|e| ids.contains(e)));
    // Longer run on row 0; the shape at (72, 2) replaced
    let mut entity = world.entity_mut(map);
    let mut tiles = entity.get_mut::<TileColliders>().unwrap();
    tiles.set(4, 0, TileCell::OneWay);
    tiles.set(72, 2, TileCell::Empty);
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
        tiles.set(x, y, TileCell::Solid);
      }
    }
    for (x, y) in [(10, 10), (11, 10), (100, 200), (250, 40)] {
      tiles.set(x, y, TileCell::Empty);
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
      tiles.set(x, 1, half);
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
      tiles.set(x, 1, cell);
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
      tiles.set(x, rows - 1, TileCell::Solid);
    }
    for x in 10..10 + cols {
      let k = x - 10;
      let row = rows - 2 - k / per_row;
      tiles.set(x, row, shapes[(k % per_row) as usize]);
      for below in row + 1..rows {
        tiles.set(x, below, TileCell::Solid);
      }
    }
    for x in 10 + cols..width {
      for row in rows - 1 - rise..rows {
        tiles.set(x, row, TileCell::Solid);
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
      tiles.set(x, 0, slab);
      tiles.set(x, 2, if x % 2 == 0 { flush } else { TileCell::OneWay });
    }
    for (x, y, cell) in [(0, 5, low), (1, 5, high), (2, 4, low), (3, 4, high)] {
      tiles.set(x, y, cell);
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
