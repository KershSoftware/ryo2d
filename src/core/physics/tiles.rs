//! Tile colliders: a grid of cells becomes seamless outlines (or merged
//! rectangles), one-way runs and 45° slopes. Coordinates here are in tiles,
//! +Y down, cell (0, 0)'s top-left at the origin.

use std::collections::{BTreeMap, BTreeSet};

use super::collider::Collider;
use std::sync::atomic::{AtomicU64, Ordering};

use super::world::Physics;
use crate::core::ecs::{
  Allow, ChildOf, Commands, Component, Disabled, Entity, EntityHashMap, Has, Or, Query, ResMut,
  Transform, Vec2, With,
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
/// The most cells a grid holds (8192 × 8192); a larger size is empty.
const MAX_CELLS: usize = 1 << 26;

/// A grid of tile cells that builds its own colliders: seamless outlines
/// around solid and slope cells (or merged rectangles with `solid_rects()`)
/// and one-way segments along runs of one-way cells. Its `Transform` places
/// cell (0, 0)'s top-left; +Y is down. The colliders are child entities,
/// rebuilt (all of them) before the next physics step after a change. With
/// no body they are fixed; with a kinematic `RigidBody` the whole map moves.
///
/// Disabling the map entity (`Disabled`) removes its colliders and enabling
/// it builds them again from the cells as they then are; so does removing
/// the component. A grid over 8192 × 8192 cells is logged and empty.
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
  /// Changes whenever the cells do (and is fresh for every new grid), so a
  /// rebuild can tell what it has already built
  generation: u64,
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
  /// empty and extra ones are dropped. A size over 8192 × 8192 cells (a bad
  /// size, not a map) is logged and makes a 0 x 0 grid.
  pub fn from_cells(width: u32, height: u32, tile_size: Vec2, mut cells: Vec<TileCell>) -> Self {
    let (width, height) = match (width as usize).checked_mul(height as usize) {
      Some(count) if count <= MAX_CELLS => {
        cells.resize(count, TileCell::Empty);
        (width, height)
      }
      _ => {
        crate::engine_warn!(
          module: "Physics",
          "a {width} x {height} TileColliders is over the limit of 8192 x 8192 cells; it is empty"
        );
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
      generation: next_generation(),
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

  /// Change a cell (the colliders rebuild next step). Returns false, and
  /// changes nothing, for a cell outside the grid or one already `cell`.
  pub fn set(&mut self, x: u32, y: u32, cell: TileCell) -> bool {
    if x >= self.width || y >= self.height {
      return false;
    }
    let slot = &mut self.cells[y as usize * self.width as usize + x as usize];
    if *slot == cell {
      return false;
    }
    *slot = cell;
    self.generation = next_generation();
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

/// What the colliders of a map were last built from, kept on the map entity.
/// Floats compare by bits, so a NaN size does not rebuild every step.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub(crate) struct TileRecord {
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
}

type Maps<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    Option<&'static TileColliders>,
    Option<&'static TileRecord>,
    Has<Disabled>,
  ),
  (Or<(With<TileColliders>, With<TileRecord>)>, Allow<Disabled>),
>;
type Parts<'w, 's> = Query<'w, 's, (Entity, &'static ChildOf), (With<TilePart>, Allow<Disabled>)>;

/// `FixedSet::Movement`: reconcile every map's colliders with its component
/// each step. A map that is disabled or lost its `TileColliders` has no
/// colliders; an enabled one whose cells or settings differ from its
/// `TileRecord` has its colliders rebuilt (all of them).
pub(crate) fn rebuild_tile_colliders(
  maps: Maps,
  parts: Parts,
  mut physics: ResMut<Physics>,
  mut commands: Commands,
) {
  // Maps whose parts are cleared; Some(tiles) when they are then rebuilt
  let mut stale: EntityHashMap<Option<&TileColliders>> = EntityHashMap::default();
  for (map, tiles, record, disabled) in &maps {
    match (tiles, disabled) {
      (Some(tiles), false) => {
        let now = TileRecord::of(tiles);
        if record != Some(&now) {
          stale.insert(map, Some(tiles));
          commands.entity(map).insert(now);
        }
      }
      _ => {
        if record.is_some() {
          commands.entity(map).remove::<TileRecord>();
        }
        stale.insert(map, None);
      }
    }
  }
  // Parts of cleared maps go, and so do orphans of entities that are not maps
  for (part, child_of) in &parts {
    let parent = child_of.parent();
    if stale.contains_key(&parent) || !maps.contains(parent) {
      commands.entity(part).despawn();
    }
  }
  for (map, tiles) in stale {
    let Some(tiles) = tiles else {
      continue;
    };
    if !(tiles.tile_size.is_finite() && tiles.tile_size.min_element() > 0.0) {
      physics.warn_once(
        map,
        "TileColliders needs a positive tile size; no colliders built",
      );
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
  use crate::core::ecs::{ChildOf, Disabled, Transform, Velocity, World};
  use crate::core::physics::test_util::*;
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
    assert_eq!(ids(&mut world, map).len(), 2);
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
