//! Tile colliders: a grid of cells becomes seamless outlines (or merged
//! rectangles), one-way runs and 45° slopes. Coordinates here are in tiles,
//! +Y down, cell (0, 0)'s top-left at the origin.

use std::collections::{BTreeMap, BTreeSet};

use super::collider::Collider;
use crate::core::ecs::{
  Changed, ChildOf, Children, Commands, Component, Entity, EntityHashSet, Local, Query, Transform,
  Vec2, With,
};

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
pub(crate) type Corner = (i32, i32);

/// A filled cell's corners relative to its top-left, wound the same way
/// (clockwise on screen) so an edge two cells share runs both ways and cancels.
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

fn sub(a: Corner, b: Corner) -> Corner {
  (a.0 - b.0, a.1 - b.1)
}

/// The signed turn from heading `from` to heading `to`, in radians.
fn turn(from: Corner, to: Corner) -> f32 {
  let cross = (from.0 * to.1 - from.1 * to.0) as f32;
  let dot = (from.0 * to.0 + from.1 * to.1) as f32;
  cross.atan2(dot)
}

/// The closed outlines around every region of solid and slope cells (holes
/// too), straight runs merged: each is a loop of corners whose last repeats
/// its first.
pub(crate) fn outlines(width: u32, height: u32, cells: &[TileCell]) -> Vec<Vec<Corner>> {
  let (cols, height) = extent(width, height, cells);
  // Every filled cell adds its edges; an edge two cells share appears once
  // each way and cancels, leaving only the boundary
  let mut edges: BTreeSet<(Corner, Corner)> = BTreeSet::new();
  for y in 0..height {
    for x in 0..cols {
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
pub(crate) fn one_way_runs(width: u32, height: u32, cells: &[TileCell]) -> Vec<(Corner, Corner)> {
  let (cols, height) = extent(width, height, cells);
  let mut runs = Vec::new();
  for y in 0..height {
    let mut x = 0;
    while x < cols {
      if cell(width, cells, x, y) != TileCell::OneWay {
        x += 1;
        continue;
      }
      let start = x;
      while x < cols && cell(width, cells, x, y) == TileCell::OneWay {
        x += 1;
      }
      runs.push(((start as i32, y as i32), (x as i32, y as i32)));
    }
  }
  runs
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
/// The most cells a grid holds (a 8192 x 8192 map); a larger size is empty.
const MAX_CELLS: usize = 1 << 26;

/// A grid of tile cells that builds its own colliders: seamless outlines
/// around solid and slope cells (or merged rectangles with `solid_rects()`)
/// and one-way segments along runs of one-way cells. Its `Transform` places
/// cell (0, 0)'s top-left; +Y is down. The colliders are child entities,
/// rebuilt (all of them) on the next step after any change. With no body
/// they are fixed; with a kinematic `RigidBody` the whole map moves.
///
/// Outlines are hollow: a body that ends up inside a solid region is not
/// pushed out. `solid_rects()` trades the seamless floors for solid
/// interiors.
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
}

impl TileColliders {
  /// An empty grid.
  pub fn new(width: u32, height: u32, tile_size: Vec2) -> Self {
    Self::from_cells(width, height, tile_size, Vec::new())
  }

  /// A grid from its cells, row by row from the top; missing cells are
  /// empty and extra ones are dropped. A size over `MAX_CELLS` (a bad
  /// size, not a map) makes a 0 x 0 grid.
  pub fn from_cells(width: u32, height: u32, tile_size: Vec2, mut cells: Vec<TileCell>) -> Self {
    let (width, height) = match (width as usize).checked_mul(height as usize) {
      Some(count) if count <= MAX_CELLS => {
        cells.resize(count, TileCell::Empty);
        (width, height)
      }
      _ => {
        cells.clear();
        (0, 0)
      }
    };
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

  /// Change a cell (the colliders rebuild next step); false outside the grid.
  pub fn set(&mut self, x: u32, y: u32, cell: TileCell) -> bool {
    if x >= self.width || y >= self.height {
      return false;
    }
    self.cells[y as usize * self.width as usize + x as usize] = cell;
    true
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

  fn material(&self, collider: Collider) -> Collider {
    collider
      .with_layer(self.layer)
      .with_mask(self.mask)
      .with_friction(self.friction)
      .with_restitution(self.restitution)
  }

  /// The colliders the grid needs, in its own space.
  pub(crate) fn colliders(&self) -> Vec<Collider> {
    let size = self.tile_size;
    let px = |(x, y): Corner| Vec2::new(x as f32 * size.x, y as f32 * size.y);
    let mut built = Vec::new();
    if self.solid_rects {
      for (min, cells) in solid_rects(self.width, self.height, &self.cells) {
        let extent = px(cells);
        let centre = px(min) + extent / 2.0;
        built
          .push(self.material(Collider::rect(extent.x, extent.y).with_offset(centre.x, centre.y)));
      }
      for y in 0..self.height {
        for x in 0..self.width {
          let cell = self.get(x, y);
          if let TileCell::Slope(_) = cell {
            let points = corners(cell)
              .iter()
              .map(|c| px((x as i32 + c.0, y as i32 + c.1)));
            built.push(self.material(Collider::polygon(points)));
          }
        }
      }
    } else {
      for outline in outlines(self.width, self.height, &self.cells) {
        built.push(self.material(Collider::polyline(outline.into_iter().map(px))));
      }
    }
    for (a, b) in one_way_runs(self.width, self.height, &self.cells) {
      built.push(self.material(Collider::segment(px(a), px(b)).one_way()));
    }
    built
  }
}

/// Marks a collider a `TileColliders` built; its parent owns it.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct TilePart;

/// `FixedSet::Movement`: rebuild the colliders of every grid that changed.
pub(crate) fn rebuild_tile_colliders(
  maps: Query<(Entity, &TileColliders, Option<&Children>), Changed<TileColliders>>,
  parts: Query<(), With<TilePart>>,
  mut commands: Commands,
  mut warned: Local<EntityHashSet>,
) {
  for (map, tiles, children) in &maps {
    for child in children.into_iter().flat_map(|children| children.iter()) {
      if parts.contains(*child) {
        commands.entity(*child).despawn();
      }
    }
    if !(tiles.tile_size.is_finite() && tiles.tile_size.min_element() > 0.0) {
      if warned.insert(map) {
        crate::engine_warn!(module: "Physics", "{map}: TileColliders needs a positive tile size; no colliders built");
      }
      continue;
    }
    for collider in tiles.colliders() {
      commands.spawn((Transform::IDENTITY, collider, TilePart, ChildOf(map)));
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

#[cfg(test)]
mod with_physics {
  use super::*;
  use crate::core::ecs::{ChildOf, Transform, Velocity, World};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{CharacterController, Collider, ColliderShape, RigidBody};

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
      parts(&mut world, map).len(),
      2,
      "two outlines, the old one gone"
    );
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

  #[test]
  fn a_bad_tile_size_builds_nothing() {
    let (mut world, mut s) = world(60);
    let map = world
      .spawn((
        Transform::IDENTITY,
        TileColliders::from_cells(1, 1, Vec2::ZERO, vec![TileCell::Solid]),
      ))
      .id();
    run(&mut world, &mut s, 1);
    assert!(parts(&mut world, map).is_empty());
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
}
