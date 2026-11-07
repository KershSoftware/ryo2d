//! Path structures for navigation
//!
//! Represents computed paths and provides smoothing options.

use super::grid::{GridCoord, NavigationGrid};
use crate::core::ecs::Floor;

/// A waypoint in a path
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathNode {
  /// World X coordinate
  pub x: f32,
  /// World Y coordinate
  pub y: f32,
  /// Grid coordinate this node corresponds to
  pub grid_coord: GridCoord,
  /// The floor it is on: its grid's ([`NavigationGrid::floor`]). A path
  /// across floors ([`Pathfinder::find_floor_path`](super::Pathfinder::find_floor_path))
  /// changes floor between two nodes at one stair cell, one on each floor,
  /// so a character following it changes floor as the stairs do
  pub floor: Floor,
}

impl PathNode {
  /// A node at the world point (`x`, `y`) of `grid_coord`, on the ground
  /// floor
  pub fn new(x: f32, y: f32, grid_coord: GridCoord) -> Self {
    PathNode {
      x,
      y,
      grid_coord,
      floor: Floor::GROUND,
    }
  }

  /// Calculate squared distance to another node
  pub fn distance_squared(&self, other: &PathNode) -> f32 {
    let dx = self.x - other.x;
    let dy = self.y - other.y;
    dx * dx + dy * dy
  }

  /// Calculate distance to another node
  pub fn distance(&self, other: &PathNode) -> f32 {
    self.distance_squared(other).sqrt()
  }
}

/// Path smoothing options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathSmoothing {
  /// No smoothing - use raw A* waypoints
  #[default]
  None,
  /// Skip waypoints when line-of-sight is clear
  LineOfSight,
  /// Smooth path with Catmull-Rom spline (more waypoints, smoother curves)
  Spline,
}

/// A computed path from A* pathfinding
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
  /// Waypoints from start to goal (in world coordinates)
  waypoints: Vec<PathNode>,
  /// Current waypoint index being navigated to
  current_index: usize,
  /// Grid generation when this path was computed (for invalidation)
  grid_generation: u64,
  /// Total path length in world units
  total_length: f32,
}

impl Path {
  /// Create a new path from waypoints
  pub fn new(waypoints: Vec<PathNode>, grid_generation: u64) -> Self {
    let total_length = Self::calculate_length(&waypoints);
    Path {
      waypoints,
      current_index: 0,
      grid_generation,
      total_length,
    }
  }

  /// Create an empty/invalid path
  pub fn empty() -> Self {
    Path {
      waypoints: Vec::new(),
      current_index: 0,
      grid_generation: 0,
      total_length: 0.0,
    }
  }

  /// Calculate total length of a waypoint sequence
  fn calculate_length(waypoints: &[PathNode]) -> f32 {
    if waypoints.len() < 2 {
      return 0.0;
    }

    let mut length = 0.0;
    for i in 1..waypoints.len() {
      length += waypoints[i - 1].distance(&waypoints[i]);
    }
    length
  }

  /// Check if the path is valid (has waypoints)
  pub fn is_valid(&self) -> bool {
    !self.waypoints.is_empty()
  }

  /// Check if the path has been fully traversed
  pub fn is_complete(&self) -> bool {
    self.current_index >= self.waypoints.len()
  }

  /// Get the number of waypoints
  pub fn len(&self) -> usize {
    self.waypoints.len()
  }

  /// Check if path is empty
  pub fn is_empty(&self) -> bool {
    self.waypoints.is_empty()
  }

  /// Get all waypoints
  pub fn waypoints(&self) -> &[PathNode] {
    &self.waypoints
  }

  /// Get the current target waypoint
  pub fn current_waypoint(&self) -> Option<&PathNode> {
    self.waypoints.get(self.current_index)
  }

  /// Get the final destination
  pub fn destination(&self) -> Option<&PathNode> {
    self.waypoints.last()
  }

  /// Get the current waypoint index
  pub fn current_index(&self) -> usize {
    self.current_index
  }

  /// Get the grid generation this path was computed for
  pub fn grid_generation(&self) -> u64 {
    self.grid_generation
  }

  /// Get the total path length
  pub fn total_length(&self) -> f32 {
    self.total_length
  }

  /// Get remaining path length from current position
  pub fn remaining_length(&self) -> f32 {
    if self.current_index >= self.waypoints.len() {
      return 0.0;
    }

    let mut length = 0.0;
    for i in (self.current_index + 1)..self.waypoints.len() {
      length += self.waypoints[i - 1].distance(&self.waypoints[i]);
    }
    length
  }

  /// Advance to the next waypoint
  pub fn advance(&mut self) {
    if self.current_index < self.waypoints.len() {
      self.current_index += 1;
    }
  }

  /// Reset to the beginning of the path
  pub fn reset(&mut self) {
    self.current_index = 0;
  }

  /// Check if the path is stale (grid has changed)
  pub fn is_stale(&self, current_generation: u64) -> bool {
    self.grid_generation != current_generation
  }

  /// Apply line-of-sight smoothing to remove unnecessary waypoints: each
  /// waypoint is joined to the furthest after it in sight on `grid`.
  /// Only the runs of waypoints on the grid's floor are smoothed, each on
  /// its own: a path across floors keeps the two waypoints where it
  /// changes floor, and its runs on other floors as they are
  pub fn smooth_line_of_sight(&mut self, grid: &NavigationGrid) {
    self.smooth_runs_line_of_sight(|floor| (floor == grid.floor()).then_some(grid));
  }

  /// Line-of-sight smoothing of each run of waypoints on one floor, on
  /// that floor's grid (`grid_of`; a run on a floor without one is kept as
  /// it is), never across a change of floor
  pub(super) fn smooth_runs_line_of_sight<'g>(
    &mut self,
    grid_of: impl Fn(Floor) -> Option<&'g NavigationGrid>,
  ) {
    if self.waypoints.len() < 3 {
      return;
    }

    let mut smoothed = Vec::with_capacity(self.waypoints.len());
    for run in self.waypoints.chunk_by(|a, b| a.floor == b.floor) {
      match grid_of(run[0].floor) {
        Some(grid) => smooth_run_line_of_sight(run, grid, &mut smoothed),
        None => smoothed.extend_from_slice(run),
      }
    }

    self.waypoints = smoothed;
    self.total_length = Self::calculate_length(&self.waypoints);
    self.current_index = self
      .current_index
      .min(self.waypoints.len().saturating_sub(1));
  }

  /// Apply Catmull-Rom spline smoothing
  ///
  /// This interpolates additional points along the path for smoother
  /// movement: within each run of waypoints on one floor, each keeping its
  /// first and last, so a path across floors changes floor between the
  /// same two waypoints as before. An added waypoint is on its run's floor.
  pub fn smooth_spline(&mut self, segments_per_waypoint: usize) {
    if self.waypoints.len() < 2 || segments_per_waypoint == 0 {
      return;
    }

    let mut smoothed = Vec::new();
    for run in self.waypoints.chunk_by(|a, b| a.floor == b.floor) {
      smooth_run_spline(run, segments_per_waypoint, &mut smoothed);
    }

    self.waypoints = smoothed;
    self.total_length = Self::calculate_length(&self.waypoints);
    self.current_index = 0;
  }
}

/// `run`'s waypoints into `out`, each joined to the furthest after it in
/// sight on `grid`
fn smooth_run_line_of_sight(run: &[PathNode], grid: &NavigationGrid, out: &mut Vec<PathNode>) {
  out.push(run[0]);

  let mut current_idx = 0;

  while current_idx < run.len() - 1 {
    // Find the furthest waypoint with line-of-sight from current
    let mut furthest_visible = current_idx + 1;

    for check_idx in (current_idx + 2)..run.len() {
      if grid.line_of_sight(run[current_idx].grid_coord, run[check_idx].grid_coord) {
        furthest_visible = check_idx;
      }
    }

    out.push(run[furthest_visible]);
    current_idx = furthest_visible;
  }
}

/// `run`'s waypoints into `out` along a Catmull-Rom spline through them,
/// `segments` to each pair, its last waypoint kept
fn smooth_run_spline(run: &[PathNode], segments: usize, out: &mut Vec<PathNode>) {
  if run.len() < 2 {
    out.extend_from_slice(run);
    return;
  }

  // Add extra control points at start and end for the spline
  let last = run.len() - 1;
  let p0 = PathNode {
    x: 2.0 * run[0].x - run[1].x,
    y: 2.0 * run[0].y - run[1].y,
    ..run[0]
  };
  let pn = PathNode {
    x: 2.0 * run[last].x - run[last - 1].x,
    y: 2.0 * run[last].y - run[last - 1].y,
    ..run[last]
  };

  // Build extended points array
  let mut points = vec![p0];
  points.extend(run.iter().cloned());
  points.push(pn);

  // Interpolate between each pair of original waypoints
  for i in 1..(points.len() - 2) {
    let p0 = &points[i - 1];
    let p1 = &points[i];
    let p2 = &points[i + 1];
    let p3 = &points[i + 2];

    for j in 0..segments {
      let t = j as f32 / segments as f32;
      let (x, y) = catmull_rom((p0.x, p0.y), (p1.x, p1.y), (p2.x, p2.y), (p3.x, p3.y), t);
      out.push(PathNode { x, y, ..*p1 });
    }
  }

  // Always include the final waypoint
  out.push(run[last]);
}

/// Catmull-Rom spline interpolation
fn catmull_rom(
  p0: (f32, f32),
  p1: (f32, f32),
  p2: (f32, f32),
  p3: (f32, f32),
  t: f32,
) -> (f32, f32) {
  let t2 = t * t;
  let t3 = t2 * t;

  let x = 0.5
    * ((2.0 * p1.0)
      + (-p0.0 + p2.0) * t
      + (2.0 * p0.0 - 5.0 * p1.0 + 4.0 * p2.0 - p3.0) * t2
      + (-p0.0 + 3.0 * p1.0 - 3.0 * p2.0 + p3.0) * t3);

  let y = 0.5
    * ((2.0 * p1.1)
      + (-p0.1 + p2.1) * t
      + (2.0 * p0.1 - 5.0 * p1.1 + 4.0 * p2.1 - p3.1) * t2
      + (-p0.1 + 3.0 * p1.1 - 3.0 * p2.1 + p3.1) * t3);

  (x, y)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn make_path() -> Path {
    let waypoints = vec![
      PathNode::new(0.0, 0.0, GridCoord::new(0, 0)),
      PathNode::new(10.0, 0.0, GridCoord::new(1, 0)),
      PathNode::new(20.0, 0.0, GridCoord::new(2, 0)),
      PathNode::new(30.0, 0.0, GridCoord::new(3, 0)),
    ];
    Path::new(waypoints, 1)
  }

  #[test]
  fn path_creation() {
    let path = make_path();
    assert!(path.is_valid());
    assert!(!path.is_complete());
    assert_eq!(path.len(), 4);
    assert_eq!(path.total_length(), 30.0);
  }

  #[test]
  fn path_navigation() {
    let mut path = make_path();

    assert_eq!(path.current_index(), 0);
    assert_eq!(path.current_waypoint().unwrap().x, 0.0);

    path.advance();
    assert_eq!(path.current_index(), 1);
    assert_eq!(path.current_waypoint().unwrap().x, 10.0);

    path.advance();
    path.advance();
    path.advance();
    assert!(path.is_complete());
    assert!(path.current_waypoint().is_none());
  }

  #[test]
  fn path_reset() {
    let mut path = make_path();
    path.advance();
    path.advance();
    assert_eq!(path.current_index(), 2);

    path.reset();
    assert_eq!(path.current_index(), 0);
  }

  #[test]
  fn path_destination() {
    let path = make_path();
    let dest = path.destination().unwrap();
    assert_eq!(dest.x, 30.0);
  }

  #[test]
  fn empty_path() {
    let path = Path::empty();
    assert!(!path.is_valid());
    assert!(path.is_complete());
    assert!(path.current_waypoint().is_none());
  }

  #[test]
  fn path_staleness() {
    let path = make_path();
    assert!(!path.is_stale(1));
    assert!(path.is_stale(2));
  }

  #[test]
  fn remaining_length() {
    let mut path = make_path();
    assert_eq!(path.remaining_length(), 30.0);

    path.advance();
    assert_eq!(path.remaining_length(), 20.0);
  }

  #[test]
  fn path_node_distance() {
    let a = PathNode::new(0.0, 0.0, GridCoord::new(0, 0));
    let b = PathNode::new(3.0, 4.0, GridCoord::new(1, 1));
    assert_eq!(a.distance(&b), 5.0);
  }

  #[test]
  fn line_of_sight_smoothing() {
    // Create a grid where all cells are walkable
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);

    // Create a zigzag path that should be smoothed to straight line
    let waypoints = vec![
      PathNode::new(5.0, 5.0, GridCoord::new(0, 0)),
      PathNode::new(15.0, 5.0, GridCoord::new(1, 0)),
      PathNode::new(25.0, 5.0, GridCoord::new(2, 0)),
      PathNode::new(35.0, 5.0, GridCoord::new(3, 0)),
    ];
    let mut path = Path::new(waypoints, grid.generation());

    path.smooth_line_of_sight(&grid);

    // Should reduce to just start and end since all have line of sight
    assert_eq!(path.len(), 2);
  }
}
