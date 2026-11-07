//! Navigation grid for pathfinding
//!
//! Provides a cell-based walkability map for A* pathfinding: square cells
//! over the world, or a tile layer's own cells.

use super::layer::{
  LayerCells, WAYS, Ways, bounds, boxes, capped, hex_boxes, lattice_cells, mapped_bounds, overlap,
  squared,
};
use crate::core::ecs::{Entity, GlobalTransform, IVec2, Vec2, Without, World};
use crate::core::physics::{CellRect, Collider, TileCell, TileColliders, TilePart};
use crate::core::tilemap::{Lattice, LayerGrid};
use glam::{Affine2, DAffine2, DVec2};
use std::collections::HashSet;

/// Cell walkability state
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum CellState {
  /// Cell is passable with normal cost (1.0)
  #[default]
  Walkable,
  /// Cell is completely blocked
  Blocked,
  /// Cell is passable but has increased cost
  Difficult(f32),
}

impl CellState {
  /// Get the traversal cost for this cell
  pub fn cost(&self) -> f32 {
    match self {
      CellState::Walkable => 1.0,
      CellState::Blocked => f32::INFINITY,
      CellState::Difficult(cost) => *cost,
    }
  }

  /// Check if the cell can be traversed
  pub fn is_walkable(&self) -> bool {
    !matches!(self, CellState::Blocked)
  }
}

/// Grid coordinates: a square world grid's cell from (0, 0), or a tile
/// layer's own cell (signed) on a grid made with
/// [`NavigationGrid::for_layer`]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridCoord {
  pub x: i32,
  pub y: i32,
}

impl GridCoord {
  pub fn new(x: i32, y: i32) -> Self {
    GridCoord { x, y }
  }
}

impl From<IVec2> for GridCoord {
  fn from(cell: IVec2) -> Self {
    GridCoord::new(cell.x, cell.y)
  }
}

impl From<GridCoord> for IVec2 {
  fn from(coord: GridCoord) -> Self {
    IVec2::new(coord.x, coord.y)
  }
}

/// A grid-based navigation map for pathfinding
///
/// The grid divides the world into cells and tracks walkability: square
/// cells of one size from an origin ([`new`](Self::new), [`with_origin`](Self::with_origin),
/// [`NavigationBuilder`]), or a window of a tile layer's own cells on any
/// grid ([`for_layer`](Self::for_layer)), whose neighbours, step costs,
/// line of sight and world positions follow the layer's grid.
#[derive(Debug)]
pub struct NavigationGrid {
  /// Width of the grid in cells
  width: u32,
  /// Height of the grid in cells
  height: u32,
  /// Size of each cell in world units (pixels); a layer grid's shortest step
  cell_size: f32,
  /// World origin offset (grid (0,0) corresponds to this world position)
  origin_x: f32,
  origin_y: f32,
  /// The first cell: (0, 0), or a layer grid's window's `min`
  min: IVec2,
  /// The tile layer whose cells it covers; `None` for square world cells
  layer: Option<LayerCells>,
  /// Cell states (row-major order from `min`: index = y * width + x)
  cells: Vec<CellState>,
  /// Generation counter for path invalidation
  generation: u64,
  /// Entities currently blocking cells
  blocking_entities: HashSet<Entity>,
}

impl NavigationGrid {
  /// Create a new navigation grid
  ///
  /// # Arguments
  /// * `world_width` - Width of the navigable area in world units
  /// * `world_height` - Height of the navigable area in world units
  /// * `cell_size` - Size of each cell in world units (smaller = more precision, higher memory)
  pub fn new(world_width: f32, world_height: f32, cell_size: f32) -> Self {
    Self::with_origin(world_width, world_height, cell_size, 0.0, 0.0)
  }

  /// Create a navigation grid with a custom origin
  pub fn with_origin(
    world_width: f32,
    world_height: f32,
    cell_size: f32,
    origin_x: f32,
    origin_y: f32,
  ) -> Self {
    let cell_size = cell_size.max(1.0);
    let width = ((world_width / cell_size).ceil() as u32).max(1);
    let height = ((world_height / cell_size).ceil() as u32).max(1);

    NavigationGrid {
      width,
      height,
      cell_size,
      origin_x,
      origin_y,
      min: IVec2::ZERO,
      layer: None,
      cells: vec![CellState::Walkable; (width * height) as usize],
      generation: 0,
      blocking_entities: HashSet::new(),
    }
  }

  /// A grid over `window` (max exclusive) of a tile layer's own cells:
  /// `GridCoord`s are the layer's cells, signed, and cells outside the
  /// window read `Blocked`. `grid` is the layer's grid and where its pixels
  /// are in the world, as [`MapsView::grid`](crate::core::tilemap::MapsView::grid)
  /// reads it.
  ///
  /// On a hexagonal grid a cell has six neighbours (`MapGrid::neighbours`);
  /// on every other grid the eight of its square lattice (its own cells, or
  /// on a staggered grid its diamonds' lattice), a diagonal step needing both
  /// cells beside it walkable. A step costs the world distance between the
  /// two cells' centres over the grid's shortest step (`cell_size`), times
  /// the entered cell's cost, so an isometric path takes its screen
  /// distance; the heuristic and the tie-break measure world centres the
  /// same way, and line of sight follows the grid (a hex line, or a walk
  /// through the lattice's squares). World positions convert through the
  /// layer's whole transform, turned or scaled.
  ///
  /// A layer scaled to nothing, or whose transform is not finite, has no
  /// world steps: each step costs 1 (times the entered cell's cost), with
  /// no heuristic or tie-break, so a path is the fewest steps; every world
  /// point reads cell (0, 0) and every centre is the transform's
  /// translation (not a number, where it is not finite).
  ///
  /// A window over 4,194,304 cells is cut down about its centre: each side
  /// longer than 2048 cells to 2048 (logged the first time in a run).
  pub fn for_layer(grid: LayerGrid, window: CellRect) -> NavigationGrid {
    let window = capped(window);
    let size = window.size();
    let layer = LayerCells::new(grid, window);
    NavigationGrid {
      width: size.x,
      height: size.y,
      cell_size: layer.step as f32,
      origin_x: 0.0,
      origin_y: 0.0,
      min: window.min,
      layer: Some(layer),
      cells: vec![CellState::Walkable; size.x as usize * size.y as usize],
      generation: 0,
      blocking_entities: HashSet::new(),
    }
  }

  /// The cells it covers: (0, 0)..(width, height) for a grid made with
  /// `new`, a layer grid's window (as cut down)
  pub fn window(&self) -> CellRect {
    let end =
      |min: i32, len: u32| (i64::from(min) + i64::from(len)).min(i64::from(i32::MAX)) as i32;
    CellRect::new(
      self.min,
      IVec2::new(end(self.min.x, self.width), end(self.min.y, self.height)),
    )
  }

  /// The layer it was made over (`for_layer`), `None` for a square world grid
  pub fn layer(&self) -> Option<LayerGrid> {
    self.layer.map(|layer| layer.grid)
  }

  /// Get the grid dimensions
  pub fn dimensions(&self) -> (u32, u32) {
    (self.width, self.height)
  }

  /// Get the cell size: on a layer grid, the shortest world distance
  /// between neighbouring cells' centres
  pub fn cell_size(&self) -> f32 {
    self.cell_size
  }

  /// Get the current generation (increments when obstacles change)
  pub fn generation(&self) -> u64 {
    self.generation
  }

  /// Convert world coordinates to grid coordinates (on a layer grid, the
  /// cell whose outline holds the point, `LayerGrid::cell_at_world`)
  pub fn world_to_grid(&self, world_x: f32, world_y: f32) -> GridCoord {
    if let Some(layer) = &self.layer {
      return layer.grid.cell_at_world(Vec2::new(world_x, world_y)).into();
    }
    GridCoord {
      x: ((world_x - self.origin_x) / self.cell_size).floor() as i32,
      y: ((world_y - self.origin_y) / self.cell_size).floor() as i32,
    }
  }

  /// Convert grid coordinates to world coordinates (center of cell)
  pub fn grid_to_world(&self, coord: GridCoord) -> (f32, f32) {
    if let Some(layer) = &self.layer {
      let centre = layer.grid.cell_center_world(coord.into());
      return (centre.x, centre.y);
    }
    (
      self.origin_x + (coord.x as f32 + 0.5) * self.cell_size,
      self.origin_y + (coord.y as f32 + 0.5) * self.cell_size,
    )
  }

  /// Convert grid coordinates to world coordinates (top-left of cell; on a
  /// layer grid, of the box round its outline in the world)
  pub fn grid_to_world_corner(&self, coord: GridCoord) -> (f32, f32) {
    if let Some(layer) = &self.layer {
      let outline = layer.grid.outline_world(coord.into());
      let corner = outline
        .points()
        .iter()
        .fold(Vec2::INFINITY, |corner, &p| corner.min(p));
      return (corner.x, corner.y);
    }
    (
      self.origin_x + coord.x as f32 * self.cell_size,
      self.origin_y + coord.y as f32 * self.cell_size,
    )
  }

  /// Check if grid coordinates are within bounds (a layer grid's window)
  pub fn in_bounds(&self, coord: GridCoord) -> bool {
    let x = i64::from(coord.x) - i64::from(self.min.x);
    let y = i64::from(coord.y) - i64::from(self.min.y);
    x >= 0 && y >= 0 && x < i64::from(self.width) && y < i64::from(self.height)
  }

  /// Get the cell index for grid coordinates (returns None if out of bounds)
  fn cell_index(&self, coord: GridCoord) -> Option<usize> {
    if self.in_bounds(coord) {
      let x = (i64::from(coord.x) - i64::from(self.min.x)) as usize;
      let y = (i64::from(coord.y) - i64::from(self.min.y)) as usize;
      Some(y * self.width as usize + x)
    } else {
      None
    }
  }

  /// Get the cell state at grid coordinates
  pub fn get(&self, coord: GridCoord) -> CellState {
    self
      .cell_index(coord)
      .map(|i| self.cells[i])
      .unwrap_or(CellState::Blocked)
  }

  /// Set the cell state at grid coordinates
  pub fn set(&mut self, coord: GridCoord, state: CellState) {
    if let Some(i) = self.cell_index(coord)
      && self.cells[i] != state
    {
      self.cells[i] = state;
      self.generation += 1;
    }
  }

  /// Check if a cell is walkable
  pub fn is_walkable(&self, coord: GridCoord) -> bool {
    self.get(coord).is_walkable()
  }

  /// Get the traversal cost for a cell
  pub fn cost(&self, coord: GridCoord) -> f32 {
    self.get(coord).cost()
  }

  /// Get walkable neighbors of a cell with each step's cost: 8 directions
  /// on square cells (1 or √2, times the entered cell's cost), a layer
  /// grid's own neighbours on a layer grid (`for_layer`). A diagonal needs
  /// both cells beside it walkable
  pub fn neighbors(&self, coord: GridCoord) -> Vec<(GridCoord, f32)> {
    if let Some(layer) = &self.layer {
      return self.layer_neighbors(layer, coord);
    }

    const DIRECTIONS: [(i32, i32, f32); 8] = [
      (-1, 0, 1.0),                       // Left
      (1, 0, 1.0),                        // Right
      (0, -1, 1.0),                       // Up
      (0, 1, 1.0),                        // Down
      (-1, -1, std::f32::consts::SQRT_2), // Top-left
      (1, -1, std::f32::consts::SQRT_2),  // Top-right
      (-1, 1, std::f32::consts::SQRT_2),  // Bottom-left
      (1, 1, std::f32::consts::SQRT_2),   // Bottom-right
    ];

    let mut result = Vec::with_capacity(8);

    for (dx, dy, base_cost) in DIRECTIONS {
      let neighbor = GridCoord::new(coord.x + dx, coord.y + dy);

      if !self.in_bounds(neighbor) {
        continue;
      }

      let cell_state = self.get(neighbor);
      if !cell_state.is_walkable() {
        continue;
      }

      // For diagonal movement, check that both cardinal neighbors are walkable
      // to prevent corner cutting through walls
      if dx != 0 && dy != 0 {
        let cardinal_x = GridCoord::new(coord.x + dx, coord.y);
        let cardinal_y = GridCoord::new(coord.x, coord.y + dy);
        if !self.is_walkable(cardinal_x) || !self.is_walkable(cardinal_y) {
          continue;
        }
      }

      let cost = base_cost * cell_state.cost();
      result.push((neighbor, cost));
    }

    result
  }

  /// A layer grid's walkable neighbours of `coord`, each with its step's cost
  fn layer_neighbors(&self, layer: &LayerCells, coord: GridCoord) -> Vec<(GridCoord, f32)> {
    let mut result = Vec::with_capacity(8);
    match layer.ways {
      Ways::Hex(_) => {
        for (next, base_cost) in layer.grid.grid.neighbours(coord.into()).zip(layer.costs) {
          let state = self.get(next.into());
          if state.is_walkable() {
            result.push((next.into(), base_cost * state.cost()));
          }
        }
      }
      Ways::Lattice(lattice) => {
        let at = lattice.lattice_cell(coord.into());
        let cell = |dx: i32, dy: i32| -> GridCoord {
          lattice
            .grid_cell(at.saturating_add(IVec2::new(dx, dy)))
            .into()
        };
        for ((dx, dy), base_cost) in WAYS.into_iter().zip(layer.costs) {
          let next = cell(dx, dy);
          let state = self.get(next);
          if !state.is_walkable() {
            continue;
          }
          // A diagonal needs both lattice cardinals beside it walkable, so
          // no step cuts a wall's corner
          if dx != 0 && dy != 0 && !(self.is_walkable(cell(dx, 0)) && self.is_walkable(cell(0, dy)))
          {
            continue;
          }
          result.push((next, base_cost * state.cost()));
        }
      }
    }
    result
  }

  /// Check line of sight between two grid cells: every cell after `from`
  /// up to `to` walkable, walked with Bresenham's algorithm. On a layer
  /// grid the walk follows the grid: a hex line (`MapGrid::line`), or
  /// Bresenham through its lattice's squares; there both cells must be in
  /// the window
  pub fn line_of_sight(&self, from: GridCoord, to: GridCoord) -> bool {
    if let Some(layer) = &self.layer {
      if !self.in_bounds(from) || !self.in_bounds(to) {
        return false;
      }
      return match layer.ways {
        Ways::Hex(_) => layer
          .grid
          .grid
          .line(from.into(), to.into())
          .into_iter()
          .skip(1)
          .all(|cell| self.is_walkable(cell.into())),
        Ways::Lattice(lattice) => walk(
          lattice.lattice_cell(from.into()),
          lattice.lattice_cell(to.into()),
          |at| self.is_walkable(lattice.grid_cell(at).into()),
        ),
      };
    }
    walk(from.into(), to.into(), |at| self.is_walkable(at.into()))
  }

  /// Clear all cells to walkable
  pub fn clear(&mut self) {
    self.cells.fill(CellState::Walkable);
    self.blocking_entities.clear();
    self.generation += 1;
  }

  /// Block the cells under a collider: the collider's world bounds, as
  /// physics places it.
  pub fn block_collider(&mut self, global: &GlobalTransform, collider: &Collider, entity: Entity) {
    let (min, max) = collider.world_bounds(global);
    self.block_rect(min.x, min.y, max.x, max.y);
    self.blocking_entities.insert(entity);
  }

  /// Block cells in a rectangular area; on a layer grid, every cell whose
  /// outline meets the rectangle, edges included
  pub fn block_rect(&mut self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) {
    if self.layer.is_some() {
      let (lo, hi) = (
        DVec2::new(f64::from(min_x), f64::from(min_y)),
        DVec2::new(f64::from(max_x), f64::from(max_y)),
      );
      let rect = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
      self.block_polygon(&rect, true);
      return;
    }

    let min_coord = self.world_to_grid(min_x, min_y);
    let max_coord = self.world_to_grid(max_x, max_y);

    for gy in min_coord.y..=max_coord.y {
      for gx in min_coord.x..=max_coord.x {
        self.set(GridCoord::new(gx, gy), CellState::Blocked);
      }
    }
  }

  /// Block the cells under a tile grid's solid and slope cells, and its
  /// shape cells whose shape covers the cell's centre, the tiles placed in
  /// the world by `to_world` (their entity's whole world transform, its
  /// `GlobalTransform::affine`). One-way cells and one-way shapes stay
  /// walkable.
  ///
  /// - On a layer grid whose cells the tiles' are (the same grid, and
  ///   `to_world` within 1e-4 of the layer's transform in every element),
  ///   each blocking cell blocks itself (a staggered map's tiles, held by
  ///   lattice cell, their grid cells).
  /// - On a square world grid, tiles that stay upright rectangles in the
  ///   world block every cell their rectangles cover (an edge they only
  ///   touch blocks nothing).
  /// - Otherwise each blocking cell's outline in the world (a diamond, a
  ///   skewed or turned cell, a hexagon) blocks every cell whose own
  ///   outline shares some area with it: outlines that only touch, by
  ///   rounding's hair included, share none.
  ///
  /// A transform within a millionth of a right angle is read as that right
  /// angle, which f32 can't hold exactly, so a map turned by one blocks the
  /// cells its tiles line up with. Tiles placed by a transform scaled to
  /// nothing or not finite block nothing (but a layer's own tiles by its
  /// own finite transform, which block their cells).
  ///
  /// Only the cells of the tile grid's chunks under this grid are read: the
  /// cost is a look at each of the tile grid's chunks to filter them
  /// (O(chunks)), plus the cells under this grid, so a vast or unbounded
  /// tile grid costs its chunk count and what lies under it.
  pub fn block_tiles(&mut self, tiles: &TileColliders, to_world: Affine2) {
    if let Some(layer) = self.layer
      && let Some(lattice) = layer.same_cells(tiles, to_world)
    {
      self.block_own_cells(tiles, layer, lattice);
      return;
    }
    let to_world = squared(to_world);
    if self.layer.is_none()
      && let Some((size, origin)) = upright_rects(tiles, to_world)
    {
      self.block_rects(tiles, size, origin);
      return;
    }
    self.block_overlapped(tiles, to_world.as_daffine2());
  }

  /// `block_tiles` on a layer grid whose cells the tiles' are; `lattice`
  /// takes a tile cell to its grid cell where tiles hold lattice cells
  fn block_own_cells(
    &mut self,
    tiles: &TileColliders,
    layer: LayerCells,
    lattice: Option<Lattice>,
  ) {
    let area = match lattice {
      Some(_) => layer.lattice_window,
      None => self.window(),
    };
    for (at, kind) in tiles.cells_in(area) {
      if blocks(tiles, kind) {
        let cell = lattice.map_or(at, |lattice| lattice.grid_cell(at));
        self.set(cell.into(), CellState::Blocked);
      }
    }
  }

  /// `block_tiles` on a square world grid for tiles that are upright
  /// rectangles of `size` in the world, cell (0, 0)'s top-left at `origin`
  fn block_rects(&mut self, tiles: &TileColliders, size: Vec2, origin: Vec2) {
    // The cells a tile covers, worked out in cell units from the grid's
    // origin in f64: the last cell is the one holding the far edge's inside,
    // so a tile never blocks the neighbours it only touches, at any distance
    let (cols, rows) = self.dimensions();
    let cell = f64::from(self.cell_size);
    let span = |from: f64, to: f64, count: u32| {
      let first = (from / cell).floor().max(0.0) as i64;
      let last = (((to / cell).ceil() - 1.0).min(f64::from(count) - 1.0)) as i64;
      first..=last
    };
    // The tile cells under the grid, a cell wider each way (float casts
    // saturate, so a far or non-finite grid reads nothing it should not)
    let under = |grid: f32, count: u32, map: f32, tile: f32| {
      let from = (f64::from(grid) - f64::from(map)) / f64::from(tile);
      let to = from + f64::from(count) * cell / f64::from(tile);
      (
        (from.floor() as i32).saturating_sub(1),
        (to.ceil() as i32).saturating_add(1),
      )
    };
    let (x0, x1) = under(self.origin_x, cols, origin.x, size.x);
    let (y0, y1) = under(self.origin_y, rows, origin.y, size.y);
    let area = CellRect::new(IVec2::new(x0, y0), IVec2::new(x1, y1));
    for (at, kind) in tiles.cells_in(area) {
      if !blocks(tiles, kind) {
        continue;
      }
      let min_x =
        f64::from(origin.x) + f64::from(at.x) * f64::from(size.x) - f64::from(self.origin_x);
      let min_y =
        f64::from(origin.y) + f64::from(at.y) * f64::from(size.y) - f64::from(self.origin_y);
      let xs = span(min_x, min_x + f64::from(size.x), cols);
      for gy in span(min_y, min_y + f64::from(size.y), rows) {
        for gx in xs.clone() {
          self.set(GridCoord::new(gx as i32, gy as i32), CellState::Blocked);
        }
      }
    }
  }

  /// `block_tiles` by overlap: each blocking tile's outline in the world
  /// blocks the cells sharing some area with it
  fn block_overlapped(&mut self, tiles: &TileColliders, to_world: DAffine2) {
    if !to_world.is_finite() || to_world.matrix2.determinant() == 0.0 {
      return;
    }
    let Some((lo, hi)) = self.world_bounds() else {
      return;
    };
    let Some(area) = tile_cells_near(tiles, to_world.inverse(), lo, hi) else {
      return;
    };
    for (at, kind) in tiles.cells_in(area) {
      if !blocks(tiles, kind) {
        continue;
      }
      let corners = tiles.cell_polygon(at);
      let corners = corners.points();
      let mut polygon = [DVec2::ZERO; 6];
      for (point, corner) in polygon.iter_mut().zip(corners) {
        *point = to_world.transform_point2(corner.as_dvec2());
      }
      self.block_polygon(&polygon[..corners.len()], false);
    }
  }

  /// Block every cell whose outline shares some area with the convex world
  /// polygon `polygon`, or with `edges`, meets it at all
  fn block_polygon(&mut self, polygon: &[DVec2], edges: bool) {
    let Some((lo, hi)) = bounds(polygon.iter().copied()) else {
      return;
    };
    if let Some(layer) = self.layer {
      let Some(area) = layer.near(lo, hi, self.window()) else {
        return;
      };
      for y in area.min.y..area.max.y {
        for x in area.min.x..area.max.x {
          let cell = layer.grid_cell(IVec2::new(x, y));
          if self.in_bounds(cell.into()) {
            let (outline, len) = layer.outline(cell);
            if overlap(&outline[..len], polygon, edges) {
              self.set(cell.into(), CellState::Blocked);
            }
          }
        }
      }
      return;
    }
    let size = f64::from(self.cell_size);
    let origin = DVec2::new(f64::from(self.origin_x), f64::from(self.origin_y));
    let span = |lo: f64, hi: f64, count: u32| {
      let first = (lo / size).floor().max(0.0);
      let last = (hi / size).floor().min(f64::from(count) - 1.0);
      first as i64..=last as i64
    };
    let xs = span(lo.x - origin.x, hi.x - origin.x, self.width);
    for gy in span(lo.y - origin.y, hi.y - origin.y, self.height) {
      for gx in xs.clone() {
        let corner = origin + DVec2::new(gx as f64, gy as f64) * size;
        let square = [
          corner,
          corner + DVec2::new(size, 0.0),
          corner + DVec2::splat(size),
          corner + DVec2::new(0.0, size),
        ];
        if overlap(&square, polygon, edges) {
          self.set(GridCoord::new(gx as i32, gy as i32), CellState::Blocked);
        }
      }
    }
  }

  /// The world box round this grid's cells; `None` when it has none
  fn world_bounds(&self) -> Option<(DVec2, DVec2)> {
    let Some(layer) = &self.layer else {
      let origin = DVec2::new(f64::from(self.origin_x), f64::from(self.origin_y));
      let size =
        DVec2::new(f64::from(self.width), f64::from(self.height)) * f64::from(self.cell_size);
      return Some((origin, origin + size));
    };
    let window = self.window();
    if window.is_empty() {
      return None;
    }
    let local = layer.grid.grid.bounds_of(window);
    let (lo, hi) = (
      DVec2::new(f64::from(local.x), f64::from(local.y)),
      DVec2::new(
        f64::from(local.x) + f64::from(local.width),
        f64::from(local.y) + f64::from(local.height),
      ),
    );
    mapped_bounds(layer.to_world, lo, hi)
  }

  /// The world offset from `from`'s centre to `to`'s over the shortest
  /// step, on a layer grid (the pathfinder's heuristic and tie-break);
  /// `None` on a square world grid
  pub(crate) fn steps_between(&self, from: GridCoord, to: GridCoord) -> Option<DVec2> {
    let layer = self.layer.as_ref()?;
    Some(layer.offset(to.into()) - layer.offset(from.into()))
  }

  /// Rebuild the grid from entities with Collider components
  ///
  /// Every solid (non-trigger) collider blocks the cells it covers. Tile
  /// grids block exactly their solid and slope cells (and the shape cells
  /// `block_tiles` blocks), placed by their entity's whole world transform:
  /// their outline colliders would block their whole bounding boxes, so
  /// those are skipped. A tile map storey's edge (a grid that is no
  /// surface) blocks nothing: its cells are where that storey is walked.
  pub fn rebuild_from_world(&mut self, world: &mut World) {
    self.clear();
    let mut colliders =
      world.query_filtered::<(Entity, &GlobalTransform, &Collider), Without<TilePart>>();
    for (entity, global, collider) in colliders.iter(world) {
      if !collider.is_trigger {
        self.block_collider(global, collider, entity);
      }
    }
    let mut maps = world.query::<(&GlobalTransform, &TileColliders)>();
    for (global, tiles) in maps.iter(world).filter(|(_, tiles)| tiles.surface) {
      self.block_tiles(tiles, global.affine());
    }
  }
}

/// Builder for creating NavigationGrid with a fluent API
pub struct NavigationBuilder {
  world_width: f32,
  world_height: f32,
  cell_size: f32,
  origin_x: f32,
  origin_y: f32,
}

impl NavigationBuilder {
  /// Create a new builder with world dimensions
  pub fn new(world_width: f32, world_height: f32) -> Self {
    NavigationBuilder {
      world_width,
      world_height,
      cell_size: 16.0, // Default cell size
      origin_x: 0.0,
      origin_y: 0.0,
    }
  }

  /// Set the cell size
  pub fn with_cell_size(mut self, cell_size: f32) -> Self {
    self.cell_size = cell_size;
    self
  }

  /// Set the world origin
  pub fn with_origin(mut self, origin_x: f32, origin_y: f32) -> Self {
    self.origin_x = origin_x;
    self.origin_y = origin_y;
    self
  }

  /// Build an empty NavigationGrid
  pub fn build(self) -> NavigationGrid {
    NavigationGrid::with_origin(
      self.world_width,
      self.world_height,
      self.cell_size,
      self.origin_x,
      self.origin_y,
    )
  }

  /// Build a NavigationGrid populated from the world's colliders
  pub fn build_from_world(self, world: &mut World) -> NavigationGrid {
    let mut grid = self.build();
    grid.rebuild_from_world(world);
    grid
  }
}

/// Where a tile grid's cells are upright rectangles in the world, as a
/// square world grid's are: their size and cell (0, 0)'s top-left
fn upright_rects(tiles: &TileColliders, to_world: Affine2) -> Option<(Vec2, Vec2)> {
  if tiles.hex().is_some() {
    return None;
  }
  let size = tiles.tile_size;
  let cells = tiles.lattice().unwrap_or(Affine2::from_cols(
    Vec2::new(size.x, 0.0),
    Vec2::new(0.0, size.y),
    Vec2::ZERO,
  ));
  let to = to_world * cells;
  let upright = to.matrix2.x_axis.y == 0.0 && to.matrix2.y_axis.x == 0.0;
  let size = Vec2::new(to.matrix2.x_axis.x, to.matrix2.y_axis.y);
  (upright && size.is_finite() && size.min_element() > 0.0 && to.translation.is_finite())
    .then_some((size, to.translation))
}

/// Whether a tile cell blocks: solid and slope cells, and shape cells whose
/// solid shape covers the cell's centre
fn blocks(tiles: &TileColliders, kind: TileCell) -> bool {
  match kind {
    TileCell::Solid | TileCell::Slope(_) => true,
    TileCell::Shape(shape) => tiles.shape_blocks(shape),
    TileCell::Empty | TileCell::OneWay => false,
  }
}

/// The tile cells whose outlines may meet the world box `lo` to `hi`,
/// which `to_local` takes into the tiles' space
fn tile_cells_near(
  tiles: &TileColliders,
  to_local: DAffine2,
  lo: DVec2,
  hi: DVec2,
) -> Option<CellRect> {
  let (lo, hi) = mapped_bounds(to_local, lo, hi)?;
  if let Some((hex, origin)) = tiles.hex() {
    let origin = origin.as_dvec2();
    return hex_boxes(hex, lo - origin, hi - origin);
  }
  match tiles.lattice() {
    Some(to_pixel) => {
      let to_pixel = to_pixel.as_daffine2();
      if !to_pixel.is_finite() || to_pixel.matrix2.determinant() == 0.0 {
        return None;
      }
      let (lo, hi) = mapped_bounds(to_pixel.inverse(), lo, hi)?;
      Some(lattice_cells(lo, hi))
    }
    None => boxes(lo, hi, tiles.tile_size.as_dvec2()),
  }
}

/// Bresenham's walk from `from` to `to`: whether `open` holds for every
/// cell after `from`
fn walk(from: IVec2, to: IVec2, open: impl Fn(IVec2) -> bool) -> bool {
  let mut x = from.x;
  let mut y = from.y;
  let dx = (to.x - from.x).abs();
  let dy = (to.y - from.y).abs();
  let sx = if from.x < to.x { 1 } else { -1 };
  let sy = if from.y < to.y { 1 } else { -1 };
  let mut err = dx - dy;

  loop {
    // Check current cell (skip start cell)
    if (x != from.x || y != from.y) && !open(IVec2::new(x, y)) {
      return false;
    }

    if x == to.x && y == to.y {
      return true;
    }

    let e2 = 2 * err;
    if e2 > -dy {
      err -= dy;
      x += sx;
    }
    if e2 < dx {
      err += dx;
      y += sy;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::tilemap::{GridShape, MapGrid, StaggerAxis, StaggerIndex};
  use glam::{DVec2, UVec2};

  #[test]
  fn block_tiles_blocks_exactly_the_solid_cells() {
    use crate::core::physics::{TileCell, TileColliders};
    let cells = vec![
      TileCell::Solid,
      TileCell::Empty,
      TileCell::OneWay,
      TileCell::Solid,
    ];
    let tiles = TileColliders::from_cells(2, 2, Vec2::splat(10.0), cells);
    let mut grid = NavigationGrid::new(40.0, 40.0, 10.0);
    grid.block_tiles(&tiles, Affine2::from_translation(Vec2::new(10.0, 10.0)));
    assert!(!grid.is_walkable(GridCoord::new(1, 1)));
    assert!(grid.is_walkable(GridCoord::new(2, 1)));
    assert!(
      grid.is_walkable(GridCoord::new(1, 2)),
      "one-way cells are walkable"
    );
    assert!(!grid.is_walkable(GridCoord::new(2, 2)));
    assert!(grid.is_walkable(GridCoord::new(3, 3)));
  }

  #[test]
  fn rebuilding_from_the_world_keeps_the_inside_of_a_tile_u_open() {
    use crate::core::ecs::Transform;
    use crate::core::physics::{TileCell, TileColliders, TilePart};
    let (s, e) = (TileCell::Solid, TileCell::Empty);
    let mut world = World::new();
    let map = world
      .spawn((
        Transform::IDENTITY,
        GlobalTransform::IDENTITY,
        TileColliders::from_cells(3, 2, Vec2::splat(10.0), vec![s, e, s, s, s, s]),
      ))
      .id();
    // As the engine would have built it: an outline collider child
    world.spawn((
      Transform::IDENTITY,
      GlobalTransform::IDENTITY,
      Collider::polyline([
        Vec2::ZERO,
        Vec2::new(30.0, 0.0),
        Vec2::new(30.0, 20.0),
        Vec2::ZERO,
      ]),
      TilePart,
      crate::core::ecs::ChildOf(map),
    ));
    let mut grid = NavigationGrid::new(30.0, 20.0, 10.0);
    grid.rebuild_from_world(&mut world);
    assert!(grid.is_walkable(GridCoord::new(1, 0)), "the U's inside");
    assert!(!grid.is_walkable(GridCoord::new(0, 0)));
    assert!(!grid.is_walkable(GridCoord::new(1, 1)));
  }

  #[test]
  fn a_far_tile_blocks_exactly_one_cell() {
    use crate::core::physics::{TileCell, TileColliders};
    for base in [32768.0_f32, 65536.0] {
      let tiles = TileColliders::from_cells(1, 1, Vec2::splat(16.0), vec![TileCell::Solid]);
      let mut grid = NavigationGrid::with_origin(96.0, 96.0, 16.0, base - 32.0, -32.0);
      grid.block_tiles(&tiles, Affine2::from_translation(Vec2::new(base, 0.0)));
      let (w, h) = grid.dimensions();
      let blocked: Vec<_> = (0..h as i32)
        .flat_map(|y| (0..w as i32).map(move |x| GridCoord::new(x, y)))
        .filter(|c| !grid.is_walkable(*c))
        .collect();
      assert_eq!(blocked, [GridCoord::new(2, 2)], "at {base}");
    }
  }

  #[test]
  fn block_tiles_reads_an_unbounded_grid_at_negative_cells() {
    use crate::core::ecs::IVec2;
    use crate::core::physics::{TileCell, TileColliders};
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0));
    // Two cells under the grid (one right at its top-left), one solid
    // cell left of it and a one-way cell under it
    for (x, y) in [(-10, -10), (-20, -1), (-21, 3), (5, 5)] {
      tiles.set(IVec2::new(x, y), TileCell::Solid);
    }
    tiles.set(IVec2::new(-1, -1), TileCell::OneWay);
    let mut grid = NavigationGrid::with_origin(640.0, 640.0, 16.0, -320.0, -320.0);
    grid.block_tiles(&tiles, Affine2::IDENTITY);
    let (w, h) = grid.dimensions();
    let blocked: Vec<_> = (0..h as i32)
      .flat_map(|y| (0..w as i32).map(move |x| GridCoord::new(x, y)))
      .filter(|c| !grid.is_walkable(*c))
      .collect();
    assert_eq!(
      blocked,
      [
        GridCoord::new(10, 10),
        GridCoord::new(0, 19),
        GridCoord::new(25, 25)
      ]
    );
  }

  #[test]
  fn navigation_reads_a_staggered_map_s_tiles() {
    // A staggered map's colliders hold its lattice cells: four in a row
    let grid = MapGrid::new(
      GridShape::Staggered {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Odd,
      },
      UVec2::new(64, 32),
    );
    let lattice = grid.lattice().expect("a staggered grid has a lattice");
    let mut tiles = TileColliders::unbounded(Vec2::new(64.0, 32.0)).with_lattice(lattice.to_pixel);
    for x in 0..4 {
      tiles.set(IVec2::new(x, 1), TileCell::Solid);
    }
    // A grid over the map's own cells blocks the grid cells they are
    let layer = LayerGrid {
      grid,
      to_world: Affine2::IDENTITY,
    };
    let window = CellRect::new(IVec2::new(-2, -2), IVec2::new(8, 12));
    let mut own = NavigationGrid::for_layer(layer, window);
    own.block_tiles(&tiles, Affine2::IDENTITY);
    let cells = |list: &[(i32, i32)]| -> Vec<GridCoord> {
      list.iter().map(|&(x, y)| GridCoord::new(x, y)).collect()
    };
    assert_eq!(blocked(&own), cells(&[(-1, 1), (0, 2), (0, 3), (1, 4)]));
    // A square world grid reads them by overlap: every square sharing area
    // with a diamond, none it only touches
    let mut square = NavigationGrid::with_origin(256.0, 192.0, 16.0, -64.0, -32.0);
    square.block_tiles(&tiles, Affine2::IDENTITY);
    let diamonds: Vec<Vec<Vec2>> = (0..4)
      .map(|x| {
        [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
          .iter()
          .map(|&(dx, dy)| {
            lattice
              .to_pixel
              .transform_point2(Vec2::new(x as f32 + dx, 1.0 + dy))
          })
          .collect()
      })
      .collect();
    let expected = sampled(&square, &diamonds);
    assert_eq!(blocked(&square), expected);
    assert!(expected.len() > 16, "{} squares", expected.len());
    // An upright lattice's cells are its rectangles, moved by it
    let upright = glam::Affine2::from_cols(
      Vec2::new(64.0, 0.0),
      Vec2::new(0.0, 32.0),
      Vec2::new(16.0, 0.0),
    );
    let mut walls = TileColliders::unbounded(Vec2::new(64.0, 32.0)).with_lattice(upright);
    for x in 0..4 {
      walls.set(IVec2::new(x, 1), TileCell::Solid);
    }
    let mut grid = NavigationGrid::new(512.0, 256.0, 16.0);
    grid.block_tiles(&walls, Affine2::IDENTITY);
    assert!(
      !grid.is_walkable(GridCoord::new(1, 2)),
      "x 16..272, y 32..64"
    );
    assert!(grid.is_walkable(GridCoord::new(0, 2)));
    assert!(!grid.is_walkable(GridCoord::new(16, 3)));
    assert!(grid.is_walkable(GridCoord::new(17, 3)));
  }

  #[test]
  fn block_tiles_blocks_a_layer_s_own_cells_cell_for_cell() {
    use crate::core::asset::Assets;
    use crate::core::tilemap::test_util::{MapWorld, whole_hex_set};
    use crate::core::tilemap::{
      MapRoot, MapsView, Tile, TileData, TileMap, TileShape, TilesetSource,
    };
    let mut map =
      TileMap::new(UVec2::new(8, 8), UVec2::new(120, 140)).with_shape(GridShape::Hexagonal {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Odd,
        side: 70,
      });
    // Tile 0 is its whole hexagon; tile 1's shape is a triangle by its
    // left side, missing the centre (60, 70)
    let mut set = whole_hex_set(&map.grid());
    set.insert_tile(
      1,
      TileData {
        shapes: vec![TileShape {
          points: vec![
            Vec2::new(10.0, 40.0),
            Vec2::new(40.0, 40.0),
            Vec2::new(25.0, 60.0),
          ],
          ..TileShape::default()
        }],
        ..TileData::default()
      },
    );
    let set = map.add_tileset(TilesetSource::Embedded(std::sync::Arc::new(set)));
    let walls = map.add_tile_layer("walls");
    let solid = [(6, 0), (1, 1), (2, 1), (3, 2), (5, 5)];
    let Some(cells) = map.tiles_mut(walls) else {
      panic!("a tile layer")
    };
    for (x, y) in solid {
      cells.set(IVec2::new(x, y), Some(Tile::new(set, 0)));
    }
    cells.set(IVec2::new(4, 4), Some(Tile::new(set, 1)));
    let mut maps = MapWorld::new();
    let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
    let root = maps.spawn(MapRoot::new(handle));
    maps.frame();
    let Some(layer) = MapsView::new(&mut maps.world).grid(root, "walls") else {
      panic!("the walls layer")
    };
    let mut query = maps.world.query::<(&TileColliders, &GlobalTransform)>();
    let found: Vec<_> = query
      .iter(&maps.world)
      .map(|(tiles, global)| (tiles.clone(), global.affine()))
      .collect();
    let [(tiles, to_world)] = found.as_slice() else {
      panic!("one tile grid, found {}", found.len())
    };
    assert!(
      matches!(tiles.get(IVec2::new(4, 4)), TileCell::Shape(_)),
      "the triangle is a shape cell"
    );
    let window = CellRect::new(IVec2::splat(-1), IVec2::splat(9));
    let mut grid = NavigationGrid::for_layer(layer, window);
    grid.block_tiles(tiles, *to_world);
    let expected: Vec<GridCoord> = solid.iter().map(|&(x, y)| GridCoord::new(x, y)).collect();
    assert_eq!(blocked(&grid), expected);
  }

  #[test]
  fn a_square_grid_reads_isometric_and_hex_tiles_by_overlap() {
    // One solid diamond of 64 x 32, then one solid hexagon of 64 x 64
    // with sides of 32, their corners on the squares' corners and edges
    let iso = MapGrid::new(GridShape::Isometric, UVec2::new(64, 32));
    let Some(lattice) = iso.lattice() else {
      panic!("an isometric grid has a lattice")
    };
    let mut diamond =
      TileColliders::unbounded(Vec2::new(64.0, 32.0)).with_lattice(lattice.to_pixel);
    diamond.set(IVec2::new(2, 1), TileCell::Solid);
    let hex = MapGrid::new(
      GridShape::Hexagonal {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Odd,
        side: 32,
      },
      UVec2::splat(64),
    );
    let Some(cells) = hex.hex_cells() else {
      panic!("a hexagonal grid has hex cells")
    };
    let mut hexagon = TileColliders::unbounded(Vec2::splat(64.0)).with_hex(cells, hex.origin);
    hexagon.set(IVec2::new(1, 1), TileCell::Solid);
    let to_world = Affine2::from_translation(Vec2::new(16.0, 8.0));
    let polygon = |points: &[Vec2]| -> Vec<Vec<Vec2>> {
      vec![
        points
          .iter()
          .map(|&p| to_world.transform_point2(p))
          .collect(),
      ]
    };
    let diamond_points: Vec<Vec2> = [(2.0, 1.0), (3.0, 1.0), (3.0, 2.0), (2.0, 2.0)]
      .iter()
      .map(|&(x, y)| lattice.to_pixel.transform_point2(Vec2::new(x, y)))
      .collect();
    let hexagon_points = hex.cell_outline(IVec2::new(1, 1)).points().to_vec();
    for (tiles, points) in [(&diamond, diamond_points), (&hexagon, hexagon_points)] {
      let mut grid = NavigationGrid::with_origin(256.0, 160.0, 8.0, -64.0, -16.0);
      grid.block_tiles(tiles, to_world);
      let expected = sampled(&grid, &polygon(&points));
      assert!(expected.len() > 12, "{} squares", expected.len());
      assert_eq!(blocked(&grid), expected);
    }
  }

  #[test]
  fn a_turned_map_s_navigation_follows_its_transform() {
    let to_world = Affine2::from_scale_angle_translation(
      Vec2::splat(2.0),
      30f32.to_radians(),
      Vec2::new(300.0, -120.0),
    );
    let shapes = [
      (GridShape::Isometric, UVec2::new(64, 32)),
      (
        GridShape::Hexagonal {
          axis: StaggerAxis::X,
          index: StaggerIndex::Even,
          side: 20,
        },
        UVec2::new(40, 48),
      ),
    ];
    let solid = [(-3, 2), (0, 0), (1, 0), (4, -2), (2, 5)];
    for (shape, cell) in shapes {
      let grid = MapGrid::new(shape, cell);
      let layer = LayerGrid { grid, to_world };
      let window = CellRect::new(IVec2::new(-6, -5), IVec2::new(7, 8));
      let mut nav = NavigationGrid::for_layer(layer, window);
      // Every cell's centre is where the layer puts it, and reads back as it
      for y in window.min.y..window.max.y {
        for x in window.min.x..window.max.x {
          let at = GridCoord::new(x, y);
          let (wx, wy) = nav.grid_to_world(at);
          let centre = layer.cell_center_world(IVec2::new(x, y));
          assert!(
            Vec2::new(wx, wy).distance(centre) < 1e-3,
            "{shape:?} {at:?}"
          );
          assert_eq!(nav.world_to_grid(wx, wy), at, "{shape:?}");
        }
      }
      // The map's tiles, through the same transform, block their own cells
      let mut tiles = match grid.hex_cells() {
        Some(hex) => TileColliders::unbounded(cell.as_vec2()).with_hex(hex, grid.origin),
        None => match grid.lattice() {
          Some(lattice) => TileColliders::unbounded(cell.as_vec2()).with_lattice(lattice.to_pixel),
          None => panic!("{shape:?} has a lattice"),
        },
      };
      for (x, y) in solid {
        tiles.set(IVec2::new(x, y), TileCell::Solid);
      }
      nav.block_tiles(&tiles, to_world);
      let expected: Vec<GridCoord> = {
        let mut cells: Vec<GridCoord> = solid.iter().map(|&(x, y)| GridCoord::new(x, y)).collect();
        cells.sort_by_key(|c| (c.y, c.x));
        cells
      };
      assert_eq!(blocked(&nav), expected, "{shape:?}");
      // Placed within 1e-4 of the layer they are still its own cells; a
      // hundredth of a pixel off they are other cells, which reach a sliver
      // into their neighbours
      let nudged = |by: f32| Affine2 {
        translation: to_world.translation + Vec2::splat(by),
        ..to_world
      };
      let mut near = NavigationGrid::for_layer(layer, window);
      near.block_tiles(&tiles, nudged(5e-5));
      assert_eq!(blocked(&near), expected, "{shape:?}");
      let mut off = NavigationGrid::for_layer(layer, window);
      off.block_tiles(&tiles, nudged(1e-2));
      let off = blocked(&off);
      assert!(
        off.len() > expected.len() && expected.iter().all(|cell| off.contains(cell)),
        "{shape:?}: {off:?}"
      );
      // A square world grid under the map is blocked where the turned
      // cells are, and open where none is near
      let mut square = NavigationGrid::with_origin(3200.0, 3200.0, 8.0, -1600.0, -1600.0);
      square.block_tiles(&tiles, to_world);
      for y in window.min.y..window.max.y {
        for x in window.min.x..window.max.x {
          let centre = layer.cell_center_world(IVec2::new(x, y));
          let under = square.world_to_grid(centre.x, centre.y);
          let near = solid
            .iter()
            .any(|&(sx, sy)| (sx - x).abs() <= 1 && (sy - y).abs() <= 1);
          if solid.contains(&(x, y)) {
            assert!(!square.is_walkable(under), "{shape:?} ({x}, {y})");
          } else if !near {
            assert!(square.is_walkable(under), "{shape:?} ({x}, {y})");
          }
        }
      }
    }
  }

  #[test]
  fn block_rect_on_a_layer_grid_blocks_every_cell_it_meets() {
    // The rectangle's left side runs through cell (0, 0)'s right vertex,
    // (32, 16), which it only touches, and into the three diamonds that
    // meet there
    let layer = LayerGrid {
      grid: MapGrid::new(GridShape::Isometric, UVec2::new(64, 32)),
      to_world: Affine2::IDENTITY,
    };
    let mut grid =
      NavigationGrid::for_layer(layer, CellRect::new(IVec2::splat(-4), IVec2::splat(4)));
    grid.block_rect(32.0, 12.0, 40.0, 20.0);
    let cells = [(0, -1), (1, -1), (0, 0), (1, 0)].map(|(x, y)| GridCoord::new(x, y));
    assert_eq!(blocked(&grid), cells);
  }

  #[test]
  fn rebuilding_from_the_world_places_tiles_by_their_whole_transform() {
    use crate::core::ecs::Transform;
    // Two solid cells of 10 px on a map at (10, 0) scaled twice: 40 x 20
    let mut world = World::new();
    let transform = Transform::from_xy(10.0, 0.0).with_scale(Vec2::splat(2.0));
    world.spawn((
      transform,
      GlobalTransform::from_transform(&transform),
      TileColliders::from_cells(2, 1, Vec2::splat(10.0), vec![TileCell::Solid; 2]),
    ));
    let mut grid = NavigationGrid::new(80.0, 40.0, 10.0);
    grid.rebuild_from_world(&mut world);
    let expected: Vec<GridCoord> = (0..2)
      .flat_map(|y| (1..5).map(move |x| GridCoord::new(x, y)))
      .collect();
    assert_eq!(blocked(&grid), expected);
  }

  #[test]
  fn a_map_turned_by_a_right_angle_blocks_one_cell_per_tile() {
    use crate::core::ecs::Transform;
    use std::f32::consts::{FRAC_PI_2, PI};
    // Two walls of 16 px tiles along rows 2 and 4, the row between them a
    // corridor one cell wide, and two tiles on their own, one far out
    let corridor: Vec<IVec2> = (0..12).map(|x| IVec2::new(x, 3)).collect();
    let solid: Vec<IVec2> = (0..12)
      .flat_map(|x| [IVec2::new(x, 2), IVec2::new(x, 4)])
      .chain([IVec2::new(-4, 7), IVec2::new(63, -47)])
      .collect();
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0));
    for &at in &solid {
      tiles.set(at, TileCell::Solid);
    }
    // The map stands at (48, -32), turned through its transform as an
    // entity's is; a turned tile's cell on the grid, whose cell (0, 0) is
    // at (-1280, -1280)
    type Turned = fn(IVec2) -> IVec2;
    let turns: [(f32, Turned); 2] = [
      (FRAC_PI_2, |c| IVec2::new(-c.y - 1, c.x)),
      (PI, |c| IVec2::new(-c.x - 1, -c.y - 1)),
    ];
    for (turn, turned) in turns {
      let to_world = Transform::from_xy(48.0, -32.0)
        .with_rotation(turn)
        .to_affine();
      let cell = |c: IVec2| GridCoord::from(turned(c) + IVec2::new(3, -2) + IVec2::splat(80));
      let mut grid = NavigationGrid::with_origin(2560.0, 2560.0, 16.0, -1280.0, -1280.0);
      grid.block_tiles(&tiles, to_world);
      let mut expected: Vec<GridCoord> = solid.iter().map(|&c| cell(c)).collect();
      expected.sort_by_key(|c| (c.y, c.x));
      assert_eq!(blocked(&grid), expected, "turned by {turn}");
      // The corridor stays open end to end
      let (from, to) = (cell(corridor[0]), cell(corridor[corridor.len() - 1]));
      let Some(path) = super::super::Pathfinder::new()
        .find_path_grid(&grid, from, to)
        .path()
      else {
        panic!("turned by {turn}: no path along the corridor")
      };
      assert_eq!(path.len(), corridor.len(), "turned by {turn}");
    }
  }

  #[test]
  fn tiles_under_a_coarser_layer_on_a_turned_map_block_the_cells_they_share() {
    use crate::core::ecs::Transform;
    // 16 px tiles under a layer of 32 px cells, both on a map far out and
    // turned: a tile on a cell's border meets the next cell along an edge
    // only, which rounding can put a hair either side
    let to_world = Transform::from_xy(-2048.7, 931.1)
      .with_rotation((-41f32).to_radians())
      .with_scale(Vec2::splat(0.9))
      .to_affine();
    let layer = LayerGrid {
      grid: MapGrid::new(GridShape::Orthogonal, UVec2::splat(32)),
      to_world,
    };
    let window = CellRect::new(IVec2::splat(-6), IVec2::splat(6));
    let mut grid = NavigationGrid::for_layer(layer, window);
    let mut tiles = TileColliders::unbounded(Vec2::splat(16.0));
    let solid = |x: i32, y: i32| (x * 7 + y * 13).rem_euclid(5) == 0;
    for y in -12..12 {
      for x in -12..12 {
        if solid(x, y) {
          tiles.set(IVec2::new(x, y), TileCell::Solid);
        }
      }
    }
    grid.block_tiles(&tiles, to_world);
    // The cells with one of 16 points spread over their square, in the
    // layer's pixels, inside a solid tile (no point lies on a tile's edge)
    let expected: Vec<GridCoord> = (window.min.y..window.max.y)
      .flat_map(|y| (window.min.x..window.max.x).map(move |x| GridCoord::new(x, y)))
      .filter(|cell| {
        (0..16).any(|i| {
          let p = Vec2::new(cell.x as f32, cell.y as f32) * 32.0
            + Vec2::new((i % 4) as f32 + 0.5, (i / 4) as f32 + 0.5) * 8.0;
          let tile = (p / 16.0).floor().as_ivec2();
          solid(tile.x, tile.y)
        })
      })
      .collect();
    assert_eq!(blocked(&grid), expected);
  }

  #[test]
  fn a_layer_scaled_to_nothing_or_not_finite_has_no_world_steps() {
    let iso = MapGrid::new(GridShape::Isometric, UVec2::new(64, 32));
    let Some(lattice) = iso.lattice() else {
      panic!("an isometric grid has a lattice")
    };
    let flat = Affine2::from_scale_angle_translation(Vec2::ZERO, 0.0, Vec2::new(5.0, 7.0));
    let broken = Affine2::from_cols(Vec2::NAN, Vec2::new(0.0, 1.0), Vec2::ZERO);
    for (to_world, finite) in [(flat, true), (broken, false)] {
      let layer = LayerGrid {
        grid: iso,
        to_world,
      };
      let window = CellRect::new(IVec2::splat(-4), IVec2::splat(4));
      let mut grid = NavigationGrid::for_layer(layer, window);
      // Steps of 1, no heuristic, every point cell (0, 0)
      assert_eq!(grid.cell_size(), 1.0);
      let (a, b) = (GridCoord::new(-3, -3), GridCoord::new(3, 3));
      assert_eq!(grid.steps_between(a, b), Some(DVec2::ZERO));
      assert_eq!(grid.world_to_grid(10.0, 20.0), GridCoord::new(0, 0));
      let (x, y) = grid.grid_to_world(b);
      if finite {
        assert_eq!((x, y), (5.0, 7.0), "every centre at the translation");
      }
      // Its own tiles by its own transform block as its cells when that
      // transform is a number; tiles by any other block nothing
      let mut tiles =
        TileColliders::unbounded(Vec2::new(64.0, 32.0)).with_lattice(lattice.to_pixel);
      tiles.set(IVec2::new(2, -2), TileCell::Solid);
      grid.block_tiles(&tiles, to_world);
      grid.block_tiles(&tiles, Affine2::from_translation(Vec2::new(-9.0, 4.0)));
      let expected = if finite {
        vec![GridCoord::new(2, -2)]
      } else {
        Vec::new()
      };
      assert_eq!(blocked(&grid), expected);
      // Each step costs 1: the fewest steps
      let Some(path) = super::super::Pathfinder::new()
        .find_path_grid(&grid, a, b)
        .path()
      else {
        panic!("a path of unit steps")
      };
      assert_eq!(path.len(), 7);
    }
  }

  #[test]
  fn line_of_sight_follows_the_grid() {
    // On hexagons, the sight follows the hex line, not a square walk
    let hex = MapGrid::new(
      GridShape::Hexagonal {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Odd,
        side: 70,
      },
      UVec2::new(120, 140),
    );
    let layer = LayerGrid {
      grid: hex,
      to_world: Affine2::IDENTITY,
    };
    let window = CellRect::new(IVec2::ZERO, IVec2::splat(12));
    let mut grid = NavigationGrid::for_layer(layer, window);
    let (a, b) = (IVec2::new(1, 1), IVec2::new(9, 6));
    let line = hex.line(a, b);
    assert!(grid.line_of_sight(a.into(), b.into()));
    let walk = square_walk(a, b);
    let Some(&off) = walk.iter().find(|c| !line.contains(c)) else {
      panic!("the square walk leaves the hex line somewhere")
    };
    grid.set(off.into(), CellState::Blocked);
    assert!(
      grid.line_of_sight(a.into(), b.into()),
      "{off} is off the line"
    );
    grid.set(line[line.len() / 2].into(), CellState::Blocked);
    assert!(!grid.line_of_sight(a.into(), b.into()));
    // On an isometric lattice, the square walk through its cells
    let iso = LayerGrid {
      grid: MapGrid::new(GridShape::Isometric, UVec2::new(64, 32)),
      to_world: Affine2::IDENTITY,
    };
    let mut grid = NavigationGrid::for_layer(iso, window);
    let (a, b) = (IVec2::new(0, 1), IVec2::new(9, 5));
    assert!(grid.line_of_sight(a.into(), b.into()));
    let walk = square_walk(a, b);
    grid.set(walk[walk.len() / 2].into(), CellState::Blocked);
    assert!(!grid.line_of_sight(a.into(), b.into()));
    // On a staggered grid the walk is in its lattice cells, not its indices
    let staggered = MapGrid::new(
      GridShape::Staggered {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Odd,
      },
      UVec2::new(64, 32),
    );
    let Some(lattice) = staggered.lattice() else {
      panic!("a staggered grid has a lattice")
    };
    let layer = LayerGrid {
      grid: staggered,
      to_world: Affine2::IDENTITY,
    };
    let mut grid = NavigationGrid::for_layer(layer, CellRect::new(IVec2::ZERO, IVec2::splat(20)));
    let (a, b) = (IVec2::new(2, 3), IVec2::new(7, 15));
    let through: Vec<IVec2> = square_walk(lattice.lattice_cell(a), lattice.lattice_cell(b))
      .into_iter()
      .map(|l| lattice.grid_cell(l))
      .collect();
    let Some(&off) = square_walk(a, b).iter().find(|c| !through.contains(c)) else {
      panic!("the index walk leaves the lattice walk")
    };
    grid.set(off.into(), CellState::Blocked);
    assert!(grid.line_of_sight(a.into(), b.into()));
    grid.set(through[through.len() / 2].into(), CellState::Blocked);
    assert!(!grid.line_of_sight(a.into(), b.into()));
    // Smoothing a hex path keeps each pair of waypoints left in sight
    let layer = LayerGrid {
      grid: hex,
      to_world: Affine2::IDENTITY,
    };
    let mut grid = NavigationGrid::for_layer(layer, window);
    for y in 0..9 {
      grid.set(GridCoord::new(5, y), CellState::Blocked);
    }
    let path = super::super::Pathfinder::new()
      .find_path_grid(&grid, GridCoord::new(1, 2), GridCoord::new(10, 3))
      .path();
    let Some(mut path) = path else {
      panic!("a path round the wall")
    };
    let before = path.len();
    path.smooth_line_of_sight(&grid);
    assert!(path.len() < before, "{} of {before} waypoints", path.len());
    for pair in path.waypoints().windows(2) {
      assert!(grid.line_of_sight(pair[0].grid_coord, pair[1].grid_coord));
    }
  }

  /// The cells of a Bresenham walk from `a` to `b`, both included
  fn square_walk(a: IVec2, b: IVec2) -> Vec<IVec2> {
    let (dx, dy) = ((b.x - a.x).abs(), (b.y - a.y).abs());
    let (sx, sy) = ((b.x - a.x).signum(), (b.y - a.y).signum());
    let (mut at, mut err) = (a, dx - dy);
    let mut out = vec![a];
    while at != b {
      let e2 = 2 * err;
      if e2 > -dy {
        err -= dy;
        at.x += sx;
      }
      if e2 < dx {
        err += dx;
        at.y += sy;
      }
      out.push(at);
    }
    out
  }

  /// The blocked cells of `grid`'s window, row by row
  fn blocked(grid: &NavigationGrid) -> Vec<GridCoord> {
    let window = grid.window();
    (window.min.y..window.max.y)
      .flat_map(|y| (window.min.x..window.max.x).map(move |x| GridCoord::new(x, y)))
      .filter(|&cell| !grid.is_walkable(cell))
      .collect()
  }

  /// The cells of a square world grid, row by row, with one of 16 points
  /// spread over its square (4 x 4) strictly inside one of `polygons`
  fn sampled(grid: &NavigationGrid, polygons: &[Vec<Vec2>]) -> Vec<GridCoord> {
    let inside = |p: glam::DVec2, polygon: &[Vec2]| {
      let sides: Vec<f64> = (0..polygon.len())
        .filter_map(|i| {
          let a = polygon[i].as_dvec2();
          let b = polygon[(i + 1) % polygon.len()].as_dvec2();
          (a != b).then(|| (b - a).perp_dot(p - a))
        })
        .collect();
      sides.iter().all(|&s| s > 0.0) || sides.iter().all(|&s| s < 0.0)
    };
    let size = f64::from(grid.cell_size());
    let window = grid.window();
    (window.min.y..window.max.y)
      .flat_map(|y| (window.min.x..window.max.x).map(move |x| GridCoord::new(x, y)))
      .filter(|&cell| {
        let (x, y) = grid.grid_to_world_corner(cell);
        let corner = glam::DVec2::new(f64::from(x), f64::from(y));
        (0..16).any(|i| {
          let at = glam::DVec2::new(f64::from(i % 4) + 0.5, f64::from(i / 4) + 0.5) * size / 4.0;
          polygons.iter().any(|polygon| inside(corner + at, polygon))
        })
      })
      .collect()
  }

  #[test]
  fn block_tiles_blocks_shape_cells_that_cover_their_centre() {
    use crate::core::physics::{CellShape, TileCell, TileColliders};
    let polygon = |points: &[(f32, f32)]| points.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let mut tiles = TileColliders::new(4, 1, Vec2::splat(10.0));
    // Most of the cell, then a corner of it, then a one-way slab over it all
    let most = tiles.add_shape(CellShape {
      polygons: vec![polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.2, 1.0)])],
      one_way: false,
    });
    let corner = tiles.add_shape(CellShape {
      polygons: vec![polygon(&[(0.0, 0.0), (0.3, 0.0), (0.3, 0.3), (0.0, 0.3)])],
      one_way: false,
    });
    let slab = tiles.add_shape(CellShape {
      polygons: vec![polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)])],
      one_way: true,
    });
    tiles.set(IVec2::new(0, 0), TileCell::Shape(most));
    tiles.set(IVec2::new(1, 0), TileCell::Shape(corner));
    tiles.set(IVec2::new(2, 0), TileCell::Shape(slab));
    tiles.set(IVec2::new(3, 0), TileCell::Shape(9));
    let mut grid = NavigationGrid::new(40.0, 10.0, 10.0);
    grid.block_tiles(&tiles, Affine2::IDENTITY);
    let walkable: Vec<bool> = (0..4)
      .map(|x| grid.is_walkable(GridCoord::new(x, 0)))
      .collect();
    assert_eq!(
      walkable,
      [false, true, true, true],
      "covers the centre, misses it, one-way, no such shape"
    );
  }

  #[test]
  fn grid_creation() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    assert_eq!(grid.dimensions(), (10, 10));
    assert_eq!(grid.cell_size(), 10.0);
  }

  #[test]
  fn a_collider_blocks_the_cells_around_its_origin() {
    use crate::core::ecs::Transform;
    // An 18x18 box at (50, 50) covers 41..59: cells 4 and 5 on each axis
    let mut world = World::new();
    let transform = Transform::from_xy(50.0, 50.0);
    world.spawn((
      transform,
      GlobalTransform::from_transform(&transform),
      Collider::rect(18.0, 18.0),
    ));
    world.spawn((
      Transform::from_xy(5.0, 5.0),
      GlobalTransform::from_transform(&Transform::from_xy(5.0, 5.0)),
      Collider::rect(4.0, 4.0).as_trigger(),
    ));
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    grid.rebuild_from_world(&mut world);
    for (x, y) in [(4, 4), (5, 5), (4, 5), (5, 4)] {
      assert_eq!(
        grid.get(GridCoord::new(x, y)),
        CellState::Blocked,
        "({x}, {y})"
      );
    }
    for (x, y) in [(3, 3), (6, 6), (6, 4), (0, 0)] {
      assert_eq!(
        grid.get(GridCoord::new(x, y)),
        CellState::Walkable,
        "({x}, {y})"
      );
    }
  }

  #[test]
  fn world_to_grid_conversion() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);

    assert_eq!(grid.world_to_grid(0.0, 0.0), GridCoord::new(0, 0));
    assert_eq!(grid.world_to_grid(5.0, 5.0), GridCoord::new(0, 0));
    assert_eq!(grid.world_to_grid(10.0, 10.0), GridCoord::new(1, 1));
    assert_eq!(grid.world_to_grid(95.0, 95.0), GridCoord::new(9, 9));
  }

  #[test]
  fn grid_to_world_conversion() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);

    let (x, y) = grid.grid_to_world(GridCoord::new(0, 0));
    assert_eq!((x, y), (5.0, 5.0)); // Center of cell

    let (x, y) = grid.grid_to_world_corner(GridCoord::new(0, 0));
    assert_eq!((x, y), (0.0, 0.0)); // Corner of cell
  }

  #[test]
  fn cell_state_defaults() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    assert_eq!(grid.get(GridCoord::new(0, 0)), CellState::Walkable);
    assert!(grid.is_walkable(GridCoord::new(0, 0)));
  }

  #[test]
  fn set_and_get_cell() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    let coord = GridCoord::new(5, 5);

    grid.set(coord, CellState::Blocked);
    assert_eq!(grid.get(coord), CellState::Blocked);
    assert!(!grid.is_walkable(coord));

    grid.set(coord, CellState::Difficult(2.0));
    assert!(grid.is_walkable(coord));
    assert_eq!(grid.cost(coord), 2.0);
  }

  #[test]
  fn out_of_bounds_returns_blocked() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    assert_eq!(grid.get(GridCoord::new(-1, 0)), CellState::Blocked);
    assert_eq!(grid.get(GridCoord::new(100, 0)), CellState::Blocked);
  }

  #[test]
  fn neighbors_cardinal() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    let neighbors = grid.neighbors(GridCoord::new(5, 5));

    // Should have 8 neighbors (all directions)
    assert_eq!(neighbors.len(), 8);

    // Check cardinal neighbors have cost 1.0
    let cardinals: Vec<_> = neighbors
      .iter()
      .filter(|(_, cost)| (*cost - 1.0).abs() < f32::EPSILON)
      .collect();
    assert_eq!(cardinals.len(), 4);
  }

  #[test]
  fn neighbors_diagonal_blocked_by_walls() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    let center = GridCoord::new(5, 5);

    // Block cell to the right
    grid.set(GridCoord::new(6, 5), CellState::Blocked);

    let neighbors = grid.neighbors(center);

    // Should not include top-right or bottom-right diagonals
    let has_top_right = neighbors.iter().any(|(c, _)| *c == GridCoord::new(6, 4));
    let has_bottom_right = neighbors.iter().any(|(c, _)| *c == GridCoord::new(6, 6));

    assert!(!has_top_right);
    assert!(!has_bottom_right);
  }

  #[test]
  fn line_of_sight_clear() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    assert!(grid.line_of_sight(GridCoord::new(0, 0), GridCoord::new(9, 9)));
  }

  #[test]
  fn line_of_sight_blocked() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    grid.set(GridCoord::new(5, 5), CellState::Blocked);
    assert!(!grid.line_of_sight(GridCoord::new(0, 0), GridCoord::new(9, 9)));
  }

  #[test]
  fn block_rect() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    grid.block_rect(20.0, 20.0, 40.0, 40.0);

    assert!(!grid.is_walkable(GridCoord::new(2, 2)));
    assert!(!grid.is_walkable(GridCoord::new(3, 3)));
    assert!(!grid.is_walkable(GridCoord::new(4, 4)));
    assert!(grid.is_walkable(GridCoord::new(5, 5)));
  }

  #[test]
  fn generation_increments_on_change() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    let initial_gen = grid.generation();

    grid.set(GridCoord::new(0, 0), CellState::Blocked);
    assert_eq!(grid.generation(), initial_gen + 1);

    // Setting same value shouldn't increment
    grid.set(GridCoord::new(0, 0), CellState::Blocked);
    assert_eq!(grid.generation(), initial_gen + 1);
  }

  #[test]
  fn builder_api() {
    let grid = NavigationBuilder::new(200.0, 150.0)
      .with_cell_size(25.0)
      .with_origin(-50.0, -25.0)
      .build();

    assert_eq!(grid.dimensions(), (8, 6));
    assert_eq!(grid.cell_size(), 25.0);
  }

  #[test]
  fn cell_state_cost() {
    assert_eq!(CellState::Walkable.cost(), 1.0);
    assert_eq!(CellState::Blocked.cost(), f32::INFINITY);
    assert_eq!(CellState::Difficult(2.5).cost(), 2.5);
  }
}
