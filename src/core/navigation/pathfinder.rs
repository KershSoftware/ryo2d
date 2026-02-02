//! A* pathfinding algorithm with time-slicing support
//!
//! Provides efficient pathfinding for grid-based navigation.

use super::grid::{GridCoord, NavigationGrid};
use super::path::{Path, PathNode, PathSmoothing};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// Result of a pathfinding request
#[derive(Debug, Clone, PartialEq)]
pub enum PathResult {
  /// Path was found
  Found(Path),
  /// Pathfinding is still in progress (time-sliced)
  InProgress,
  /// No valid path exists
  NoPath,
  /// Start or goal position is invalid (blocked or out of bounds)
  InvalidEndpoints,
  /// Path search exceeded maximum cost/nodes limit
  TooExpensive,
}

impl PathResult {
  /// Check if a valid path was found
  pub fn is_found(&self) -> bool {
    matches!(self, PathResult::Found(_))
  }

  /// Get the path if found
  pub fn path(self) -> Option<Path> {
    match self {
      PathResult::Found(path) => Some(path),
      _ => None,
    }
  }

  /// Check if pathfinding is still in progress
  pub fn is_in_progress(&self) -> bool {
    matches!(self, PathResult::InProgress)
  }
}

/// Node in the A* open set
#[derive(Debug, Clone)]
struct AStarNode {
  coord: GridCoord,
  f_score: f32, // g + h
  g_score: f32, // Cost from start
}

impl PartialEq for AStarNode {
  fn eq(&self, other: &Self) -> bool {
    self.coord == other.coord
  }
}

impl Eq for AStarNode {}

impl Ord for AStarNode {
  fn cmp(&self, other: &Self) -> Ordering {
    // Reverse ordering for min-heap (BinaryHeap is max-heap by default)
    other
      .f_score
      .partial_cmp(&self.f_score)
      .unwrap_or(Ordering::Equal)
  }
}

impl PartialOrd for AStarNode {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

/// Configuration for the pathfinder
#[derive(Debug, Clone)]
pub struct PathfinderConfig {
  /// Maximum nodes to expand per frame (for time-slicing, 0 = unlimited)
  pub max_nodes_per_frame: usize,
  /// Maximum total nodes to expand before giving up
  pub max_total_nodes: usize,
  /// Heuristic weight (>1.0 = faster but less optimal paths)
  pub heuristic_weight: f32,
  /// Default smoothing to apply to found paths
  pub default_smoothing: PathSmoothing,
  /// Tie-breaking factor for straighter paths
  pub tie_breaking: f32,
}

impl Default for PathfinderConfig {
  fn default() -> Self {
    PathfinderConfig {
      max_nodes_per_frame: 0, // No limit by default
      max_total_nodes: 10000,
      heuristic_weight: 1.0,
      default_smoothing: PathSmoothing::None,
      tie_breaking: 0.001,
    }
  }
}

impl PathfinderConfig {
  /// Enable time-slicing with a specific node limit per frame
  pub fn with_time_slicing(mut self, max_nodes: usize) -> Self {
    self.max_nodes_per_frame = max_nodes;
    self
  }

  /// Set the default smoothing method
  pub fn with_smoothing(mut self, smoothing: PathSmoothing) -> Self {
    self.default_smoothing = smoothing;
    self
  }

  /// Set heuristic weight (>1.0 for faster but suboptimal paths)
  pub fn with_heuristic_weight(mut self, weight: f32) -> Self {
    self.heuristic_weight = weight.max(1.0);
    self
  }
}

/// State for an in-progress pathfinding request (for time-slicing)
#[derive(Debug)]
pub struct PathfindingState {
  start: GridCoord,
  goal: GridCoord,
  open_set: BinaryHeap<AStarNode>,
  closed_set: HashSet<GridCoord>,
  came_from: HashMap<GridCoord, GridCoord>,
  g_scores: HashMap<GridCoord, f32>,
  nodes_expanded: usize,
  grid_generation: u64,
}

impl PathfindingState {
  fn new(start: GridCoord, goal: GridCoord, grid_generation: u64) -> Self {
    let mut state = PathfindingState {
      start,
      goal,
      open_set: BinaryHeap::new(),
      closed_set: HashSet::new(),
      came_from: HashMap::new(),
      g_scores: HashMap::new(),
      nodes_expanded: 0,
      grid_generation,
    };

    state.g_scores.insert(start, 0.0);
    state.open_set.push(AStarNode {
      coord: start,
      f_score: octile_distance(start, goal),
      g_score: 0.0,
    });

    state
  }
}

/// A* pathfinder with optional time-slicing
pub struct Pathfinder {
  config: PathfinderConfig,
  /// Active pathfinding requests (for time-slicing)
  active_requests: HashMap<u64, PathfindingState>,
  next_request_id: u64,
}

impl Default for Pathfinder {
  fn default() -> Self {
    Self::new()
  }
}

impl Pathfinder {
  /// Create a new pathfinder with default configuration
  pub fn new() -> Self {
    Pathfinder {
      config: PathfinderConfig::default(),
      active_requests: HashMap::new(),
      next_request_id: 0,
    }
  }

  /// Create a pathfinder with custom configuration
  pub fn with_config(config: PathfinderConfig) -> Self {
    Pathfinder {
      config,
      active_requests: HashMap::new(),
      next_request_id: 0,
    }
  }

  /// Get the current configuration
  pub fn config(&self) -> &PathfinderConfig {
    &self.config
  }

  /// Find a path synchronously (blocks until complete or fails)
  pub fn find_path(
    &self,
    grid: &NavigationGrid,
    start_x: f32,
    start_y: f32,
    goal_x: f32,
    goal_y: f32,
  ) -> PathResult {
    let start = grid.world_to_grid(start_x, start_y);
    let goal = grid.world_to_grid(goal_x, goal_y);

    self.find_path_grid(grid, start, goal)
  }

  /// Find a path using grid coordinates
  pub fn find_path_grid(
    &self,
    grid: &NavigationGrid,
    start: GridCoord,
    goal: GridCoord,
  ) -> PathResult {
    // Validate endpoints
    if !grid.in_bounds(start) || !grid.in_bounds(goal) {
      return PathResult::InvalidEndpoints;
    }
    if !grid.is_walkable(start) || !grid.is_walkable(goal) {
      return PathResult::InvalidEndpoints;
    }

    // Same cell = already there
    if start == goal {
      let (x, y) = grid.grid_to_world(start);
      return PathResult::Found(Path::new(
        vec![PathNode::new(x, y, start)],
        grid.generation(),
      ));
    }

    let mut state = PathfindingState::new(start, goal, grid.generation());

    loop {
      match self.step_pathfinding(grid, &mut state, usize::MAX) {
        PathResult::InProgress => continue,
        result => {
          return self.apply_smoothing(result, grid);
        }
      }
    }
  }

  /// Start an async pathfinding request (for time-slicing)
  /// Returns a request ID to use with `continue_pathfinding`
  pub fn start_pathfinding(
    &mut self,
    grid: &NavigationGrid,
    start_x: f32,
    start_y: f32,
    goal_x: f32,
    goal_y: f32,
  ) -> (u64, PathResult) {
    let start = grid.world_to_grid(start_x, start_y);
    let goal = grid.world_to_grid(goal_x, goal_y);

    // Validate endpoints
    if !grid.in_bounds(start) || !grid.in_bounds(goal) {
      return (0, PathResult::InvalidEndpoints);
    }
    if !grid.is_walkable(start) || !grid.is_walkable(goal) {
      return (0, PathResult::InvalidEndpoints);
    }

    // Same cell = already there
    if start == goal {
      let (x, y) = grid.grid_to_world(start);
      let result = PathResult::Found(Path::new(
        vec![PathNode::new(x, y, start)],
        grid.generation(),
      ));
      return (0, self.apply_smoothing(result, grid));
    }

    let request_id = self.next_request_id;
    self.next_request_id += 1;

    let state = PathfindingState::new(start, goal, grid.generation());
    self.active_requests.insert(request_id, state);

    (request_id, PathResult::InProgress)
  }

  /// Continue an in-progress pathfinding request
  pub fn continue_pathfinding(&mut self, grid: &NavigationGrid, request_id: u64) -> PathResult {
    let max_nodes = if self.config.max_nodes_per_frame > 0 {
      self.config.max_nodes_per_frame
    } else {
      usize::MAX
    };

    if let Some(mut state) = self.active_requests.remove(&request_id) {
      // Check if grid has changed since we started
      if state.grid_generation != grid.generation() {
        return PathResult::NoPath; // Path is invalid, caller should restart
      }

      let result = self.step_pathfinding(grid, &mut state, max_nodes);

      match &result {
        PathResult::InProgress => {
          self.active_requests.insert(request_id, state);
          result
        }
        _ => self.apply_smoothing(result, grid),
      }
    } else {
      PathResult::NoPath
    }
  }

  /// Cancel an in-progress pathfinding request
  pub fn cancel_pathfinding(&mut self, request_id: u64) {
    self.active_requests.remove(&request_id);
  }

  /// Get the number of active pathfinding requests
  pub fn active_request_count(&self) -> usize {
    self.active_requests.len()
  }

  /// Perform one step of A* pathfinding
  fn step_pathfinding(
    &self,
    grid: &NavigationGrid,
    state: &mut PathfindingState,
    max_nodes: usize,
  ) -> PathResult {
    let mut nodes_this_step = 0;

    while let Some(current) = state.open_set.pop() {
      // Already processed?
      if state.closed_set.contains(&current.coord) {
        continue;
      }

      // Found goal?
      if current.coord == state.goal {
        return PathResult::Found(self.reconstruct_path(grid, state));
      }

      state.closed_set.insert(current.coord);
      state.nodes_expanded += 1;
      nodes_this_step += 1;

      // Check limits
      if state.nodes_expanded > self.config.max_total_nodes {
        return PathResult::TooExpensive;
      }

      if nodes_this_step >= max_nodes {
        // Put current back for next iteration
        state.open_set.push(current);
        return PathResult::InProgress;
      }

      // Expand neighbors
      for (neighbor, move_cost) in grid.neighbors(current.coord) {
        if state.closed_set.contains(&neighbor) {
          continue;
        }

        let tentative_g = current.g_score + move_cost;

        let is_better = state
          .g_scores
          .get(&neighbor)
          .map(|&g| tentative_g < g)
          .unwrap_or(true);

        if is_better {
          state.came_from.insert(neighbor, current.coord);
          state.g_scores.insert(neighbor, tentative_g);

          let h = octile_distance(neighbor, state.goal) * self.config.heuristic_weight;

          // Tie-breaking using cross-product for straighter paths
          let tie_break = {
            let dx1 = neighbor.x - state.goal.x;
            let dy1 = neighbor.y - state.goal.y;
            let dx2 = state.start.x - state.goal.x;
            let dy2 = state.start.y - state.goal.y;
            ((dx1 * dy2 - dx2 * dy1) as f32).abs() * self.config.tie_breaking
          };

          let f = tentative_g + h + tie_break;

          state.open_set.push(AStarNode {
            coord: neighbor,
            f_score: f,
            g_score: tentative_g,
          });
        }
      }
    }

    // Open set empty = no path exists
    PathResult::NoPath
  }

  /// Reconstruct path from the came_from map
  fn reconstruct_path(&self, grid: &NavigationGrid, state: &PathfindingState) -> Path {
    let mut path_coords = Vec::new();
    let mut current = state.goal;

    while current != state.start {
      path_coords.push(current);
      if let Some(&prev) = state.came_from.get(&current) {
        current = prev;
      } else {
        break;
      }
    }
    path_coords.push(state.start);
    path_coords.reverse();

    let waypoints: Vec<PathNode> = path_coords
      .into_iter()
      .map(|coord| {
        let (x, y) = grid.grid_to_world(coord);
        PathNode::new(x, y, coord)
      })
      .collect();

    Path::new(waypoints, state.grid_generation)
  }

  /// Apply configured smoothing to a path result
  fn apply_smoothing(&self, result: PathResult, grid: &NavigationGrid) -> PathResult {
    match result {
      PathResult::Found(mut path) => {
        match self.config.default_smoothing {
          PathSmoothing::None => {}
          PathSmoothing::LineOfSight => {
            path.smooth_line_of_sight(grid);
          }
          PathSmoothing::Spline => {
            path.smooth_spline(4); // 4 segments per waypoint
          }
        }
        PathResult::Found(path)
      }
      other => other,
    }
  }
}

/// Octile distance heuristic (diagonal movement allowed)
fn octile_distance(from: GridCoord, to: GridCoord) -> f32 {
  let dx = (from.x - to.x).abs() as f32;
  let dy = (from.y - to.y).abs() as f32;
  let (min, max) = if dx < dy { (dx, dy) } else { (dy, dx) };

  // Diagonal moves cost sqrt(2), cardinal moves cost 1
  // Total = diagonals * sqrt(2) + remaining cardinals * 1
  // = min * sqrt(2) + (max - min) * 1
  // = max + min * (sqrt(2) - 1)
  max + min * (std::f32::consts::SQRT_2 - 1.0)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn octile_distance_cardinal() {
    let a = GridCoord::new(0, 0);
    let b = GridCoord::new(5, 0);
    assert_eq!(octile_distance(a, b), 5.0);
  }

  #[test]
  fn octile_distance_diagonal() {
    let a = GridCoord::new(0, 0);
    let b = GridCoord::new(3, 3);
    // Pure diagonal: 3 * sqrt(2)
    let expected = 3.0 * std::f32::consts::SQRT_2;
    assert!((octile_distance(a, b) - expected).abs() < 0.001);
  }

  #[test]
  fn find_path_same_cell() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    let pathfinder = Pathfinder::new();

    let result = pathfinder.find_path(&grid, 5.0, 5.0, 5.0, 5.0);
    assert!(result.is_found());

    let path = result.path().unwrap();
    assert_eq!(path.len(), 1);
  }

  #[test]
  fn find_path_straight_line() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    let pathfinder = Pathfinder::new();

    let result = pathfinder.find_path(&grid, 5.0, 5.0, 95.0, 5.0);
    assert!(result.is_found());

    let path = result.path().unwrap();
    assert!(path.len() > 1);
  }

  #[test]
  fn find_path_blocked_goal() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
    grid.set(GridCoord::new(9, 0), super::super::grid::CellState::Blocked);

    let pathfinder = Pathfinder::new();
    let result = pathfinder.find_path(&grid, 5.0, 5.0, 95.0, 5.0);

    assert_eq!(result, PathResult::InvalidEndpoints);
  }

  #[test]
  fn find_path_no_path() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);

    // Block entire column
    for y in 0..10 {
      grid.set(GridCoord::new(5, y), super::super::grid::CellState::Blocked);
    }

    let pathfinder = Pathfinder::new();
    let result = pathfinder.find_path(&grid, 5.0, 5.0, 95.0, 5.0);

    assert_eq!(result, PathResult::NoPath);
  }

  #[test]
  fn find_path_around_obstacle() {
    let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);

    // Block middle cells but leave gap
    for y in 0..8 {
      grid.set(GridCoord::new(5, y), super::super::grid::CellState::Blocked);
    }

    let pathfinder = Pathfinder::new();
    let result = pathfinder.find_path(&grid, 5.0, 5.0, 95.0, 5.0);

    assert!(result.is_found());
    let path = result.path().unwrap();
    // Path should go around the obstacle
    assert!(path.len() > 9);
  }

  #[test]
  fn time_sliced_pathfinding() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    let config = PathfinderConfig::default().with_time_slicing(5);
    let mut pathfinder = Pathfinder::with_config(config);

    let (request_id, initial_result) = pathfinder.start_pathfinding(&grid, 5.0, 5.0, 95.0, 95.0);

    // Should start in progress
    assert!(initial_result.is_in_progress());
    assert_eq!(pathfinder.active_request_count(), 1);

    // Continue until complete
    let mut result = PathResult::InProgress;
    let mut iterations = 0;
    while result.is_in_progress() && iterations < 100 {
      result = pathfinder.continue_pathfinding(&grid, request_id);
      iterations += 1;
    }

    assert!(result.is_found());
    assert_eq!(pathfinder.active_request_count(), 0);
  }

  #[test]
  fn cancel_pathfinding() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0);
    let config = PathfinderConfig::default().with_time_slicing(5);
    let mut pathfinder = Pathfinder::with_config(config);

    let (request_id, _) = pathfinder.start_pathfinding(&grid, 5.0, 5.0, 95.0, 95.0);
    assert_eq!(pathfinder.active_request_count(), 1);

    pathfinder.cancel_pathfinding(request_id);
    assert_eq!(pathfinder.active_request_count(), 0);
  }

  #[test]
  fn path_result_helpers() {
    let path = Path::empty();
    let found = PathResult::Found(path);
    let no_path = PathResult::NoPath;
    let in_progress = PathResult::InProgress;

    assert!(found.is_found());
    assert!(!no_path.is_found());
    assert!(in_progress.is_in_progress());
  }

  #[test]
  fn pathfinder_config_builders() {
    let config = PathfinderConfig::default()
      .with_time_slicing(100)
      .with_smoothing(PathSmoothing::LineOfSight)
      .with_heuristic_weight(1.5);

    assert_eq!(config.max_nodes_per_frame, 100);
    assert_eq!(config.default_smoothing, PathSmoothing::LineOfSight);
    assert_eq!(config.heuristic_weight, 1.5);
  }
}
