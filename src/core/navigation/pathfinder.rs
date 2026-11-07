//! A* pathfinding algorithm with time-slicing support
//!
//! Provides efficient pathfinding for grid-based navigation.

use super::floors::FloorNavigation;
use super::grid::{GridCoord, NavigationGrid};
use super::path::{Path, PathNode, PathSmoothing};
use crate::core::ecs::{Floor, Vec2};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;

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
struct AStarNode<K> {
  coord: K,
  f_score: f32, // g + h
  g_score: f32, // Cost from start
}

impl<K: PartialEq> PartialEq for AStarNode<K> {
  fn eq(&self, other: &Self) -> bool {
    self.coord == other.coord
  }
}

impl<K: Eq> Eq for AStarNode<K> {}

impl<K: Eq> Ord for AStarNode<K> {
  fn cmp(&self, other: &Self) -> Ordering {
    // Reverse ordering for min-heap (BinaryHeap is max-heap by default)
    other
      .f_score
      .partial_cmp(&self.f_score)
      .unwrap_or(Ordering::Equal)
  }
}

impl<K: Eq> PartialOrd for AStarNode<K> {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

/// What the A* core searches: one grid's cells (`GridCoord`), or a
/// `FloorNavigation`'s floors' cells
pub(super) trait Graph {
  /// A node: a cell, or a floor's cell
  type Key: Copy + Eq + Hash + Debug;

  /// Each node a step from `at` leads to, with the step's cost
  fn neighbors(&self, at: Self::Key) -> Vec<(Self::Key, f32)>;

  /// The heuristic from `from` to `to`, which no path between them undercuts
  fn distance(&self, from: Self::Key, to: Self::Key) -> f32;

  /// How far `at` strays from the line from `start` to `goal`
  fn tie_break(&self, at: Self::Key, start: Self::Key, goal: Self::Key) -> f32;

  /// The path node standing for `at`
  fn node(&self, at: Self::Key) -> PathNode;

  /// What changes whenever any of its cells does
  fn generation(&self) -> u64;

  /// The grid of `floor`'s cells, for smoothing a path's run on it
  fn grid_of(&self, floor: Floor) -> Option<&NavigationGrid>;
}

impl Graph for NavigationGrid {
  type Key = GridCoord;

  fn neighbors(&self, at: GridCoord) -> Vec<(GridCoord, f32)> {
    NavigationGrid::neighbors(self, at)
  }

  fn distance(&self, from: GridCoord, to: GridCoord) -> f32 {
    distance(self, from, to)
  }

  fn tie_break(&self, at: GridCoord, start: GridCoord, goal: GridCoord) -> f32 {
    tie_break(self, at, start, goal)
  }

  fn node(&self, at: GridCoord) -> PathNode {
    let (x, y) = self.grid_to_world(at);
    PathNode {
      floor: self.floor(),
      ..PathNode::new(x, y, at)
    }
  }

  fn generation(&self) -> u64 {
    NavigationGrid::generation(self)
  }

  fn grid_of(&self, floor: Floor) -> Option<&NavigationGrid> {
    (floor == self.floor()).then_some(self)
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

/// A search in progress over a graph's nodes `K`: the A* core's state
#[derive(Debug)]
struct Search<K> {
  start: K,
  goal: K,
  open_set: BinaryHeap<AStarNode<K>>,
  closed_set: HashSet<K>,
  came_from: HashMap<K, K>,
  g_scores: HashMap<K, f32>,
  nodes_expanded: usize,
  grid_generation: u64,
}

impl<K: Copy + Eq + Hash> Search<K> {
  fn new<G: Graph<Key = K>>(start: K, goal: K, graph: &G) -> Self {
    let mut search = Search {
      start,
      goal,
      open_set: BinaryHeap::new(),
      closed_set: HashSet::new(),
      came_from: HashMap::new(),
      g_scores: HashMap::new(),
      nodes_expanded: 0,
      grid_generation: graph.generation(),
    };

    search.g_scores.insert(start, 0.0);
    search.open_set.push(AStarNode {
      coord: start,
      f_score: graph.distance(start, goal),
      g_score: 0.0,
    });

    search
  }
}

/// State for an in-progress pathfinding request (for time-slicing)
#[derive(Debug)]
pub struct PathfindingState {
  search: Search<GridCoord>,
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
      return PathResult::Found(Path::new(vec![Graph::node(grid, start)], grid.generation()));
    }

    let mut search = Search::new(start, goal, grid);

    loop {
      match self.step_pathfinding(grid, &mut search, usize::MAX) {
        PathResult::InProgress => continue,
        result => {
          return self.apply_smoothing(result, grid);
        }
      }
    }
  }

  /// Find a path across floors, from the world point `start` on
  /// `start_floor` to `goal` on `goal_floor`, each point read through its
  /// floor's grid ([`find_floor_path_grid`](Self::find_floor_path_grid)).
  ///
  /// The points are in ground space: where a character on that floor
  /// stands (its `Transform`), not where it draws. A storey draws its
  /// height above its ground, so a point picked on a storey's art is a
  /// storey's height off its cell: a click goes through
  /// [`MapsView::floor_cell_at_world`](crate::core::tilemap::MapsView::floor_cell_at_world)
  /// into `find_floor_path_grid` instead.
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// /// The way from where the hero stands to where the guard stands,
  /// /// each on its own storey
  /// fn route_to(ctx: &mut SceneContext<'_>, map: Entity, hero: Entity, guard: Entity) -> Option<Path> {
  ///   let stand = |entity: Entity| {
  ///     let at = ctx.world.get::<Transform>(entity)?.translation;
  ///     let floor = ctx.world.get::<Floor>(entity).copied().unwrap_or_default();
  ///     Some((at, floor))
  ///   };
  ///   let ((from, from_floor), (to, to_floor)) = (stand(hero)?, stand(guard)?);
  ///   let mut maps = ctx.maps();
  ///   let bounds = maps.bounds(map)?;
  ///   let floors = maps.floor_navigation(map, bounds)?;
  ///   Pathfinder::new()
  ///     .find_floor_path(&floors, from, from_floor, to, to_floor)
  ///     .path()
  /// }
  /// ```
  pub fn find_floor_path(
    &self,
    nav: &FloorNavigation,
    start: Vec2,
    start_floor: Floor,
    goal: Vec2,
    goal_floor: Floor,
  ) -> PathResult {
    let cell = |floor: Floor, at: Vec2| Some(nav.grid(floor)?.world_to_grid(at.x, at.y));
    match (cell(start_floor, start), cell(goal_floor, goal)) {
      (Some(from), Some(to)) => {
        self.find_floor_path_grid(nav, (start_floor, from), (goal_floor, to))
      }
      _ => PathResult::InvalidEndpoints,
    }
  }

  /// Find a path across floors from a floor's cell to a floor's cell: on
  /// each floor through its grid's steps, and from one floor to another
  /// at a stair link, a step between the link's cell on the two floors
  /// for next to nothing (a thousandth of a step, so of paths otherwise as
  /// cheap the one changing floor least: a path crosses a run of stairs
  /// once). The path's nodes carry their floor, which changes between two
  /// nodes at the link's cell. An end on a floor without a grid, out of
  /// its grid or blocked is `InvalidEndpoints`.
  ///
  /// The search is the same A* as [`find_path_grid`](Self::find_path_grid)'s,
  /// with its heuristic and tie-break measured on the lowest floor's grid
  /// (every floor's cells lie where its cells do, and a link joins one
  /// cell), and its node budget (`max_total_nodes`) for the whole search.
  /// Smoothing works within each run of nodes on one floor, on that
  /// floor's grid, and keeps the two nodes at each link. It runs to the end
  /// in one call: a floor search is not time-sliced (there is no
  /// `start_pathfinding` across floors), whatever `max_nodes_per_frame`
  /// says.
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// /// The way from where the hero stands to the cell clicked, on
  /// /// whichever storey is drawn under the pointer
  /// fn route_up(ctx: &mut SceneContext<'_>, map: Entity, hero: Entity) -> Option<Path> {
  ///   let at = ctx.world.get::<Transform>(hero)?.translation;
  ///   let on = ctx.world.get::<Floor>(hero).copied().unwrap_or_default();
  ///   let (x, y) = ctx.input.mouse_position();
  ///   let pointer = ctx.camera().screen_to_world(Vec2::new(x, y));
  ///   let mut maps = ctx.maps();
  ///   let (floor, cell) = maps.floor_cell_at_world(map, pointer)?;
  ///   let bounds = maps.bounds(map)?;
  ///   let floors = maps.floor_navigation(map, bounds)?;
  ///   let start = floors.grid(on)?.world_to_grid(at.x, at.y);
  ///   Pathfinder::new()
  ///     .find_floor_path_grid(&floors, (on, start), (floor, cell.into()))
  ///     .path()
  /// }
  /// ```
  pub fn find_floor_path_grid(
    &self,
    nav: &FloorNavigation,
    start: (Floor, GridCoord),
    goal: (Floor, GridCoord),
  ) -> PathResult {
    let (Some(from), Some(to)) = (nav.key(start), nav.key(goal)) else {
      return PathResult::InvalidEndpoints;
    };
    for (floor, cell) in [start, goal] {
      let open = nav.grid(floor).is_some_and(|grid| grid.is_walkable(cell));
      if !open {
        return PathResult::InvalidEndpoints;
      }
    }
    if from == to {
      return PathResult::Found(Path::new(vec![nav.node(from)], Graph::generation(nav)));
    }
    let mut search = Search::new(from, to, nav);
    loop {
      match self.step_pathfinding(nav, &mut search, usize::MAX) {
        PathResult::InProgress => continue,
        result => return self.apply_smoothing(result, nav),
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
      let result = PathResult::Found(Path::new(vec![Graph::node(grid, start)], grid.generation()));
      return (0, self.apply_smoothing(result, grid));
    }

    let request_id = self.next_request_id;
    self.next_request_id += 1;

    let search = Search::new(start, goal, grid);
    self
      .active_requests
      .insert(request_id, PathfindingState { search });

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
      if state.search.grid_generation != grid.generation() {
        return PathResult::NoPath; // Path is invalid, caller should restart
      }

      let result = self.step_pathfinding(grid, &mut state.search, max_nodes);

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
  fn step_pathfinding<G: Graph>(
    &self,
    graph: &G,
    search: &mut Search<G::Key>,
    max_nodes: usize,
  ) -> PathResult {
    let mut nodes_this_step = 0;

    while let Some(current) = search.open_set.pop() {
      // Already processed?
      if search.closed_set.contains(&current.coord) {
        continue;
      }

      // Found goal?
      if current.coord == search.goal {
        return PathResult::Found(self.reconstruct_path(graph, search));
      }

      search.closed_set.insert(current.coord);
      search.nodes_expanded += 1;
      nodes_this_step += 1;

      // Check limits
      if search.nodes_expanded > self.config.max_total_nodes {
        return PathResult::TooExpensive;
      }

      if nodes_this_step >= max_nodes {
        // Put current back for next iteration
        search.open_set.push(current);
        return PathResult::InProgress;
      }

      // Expand neighbors
      for (neighbor, move_cost) in graph.neighbors(current.coord) {
        if search.closed_set.contains(&neighbor) {
          continue;
        }

        let tentative_g = current.g_score + move_cost;

        let is_better = search
          .g_scores
          .get(&neighbor)
          .map(|&g| tentative_g < g)
          .unwrap_or(true);

        if is_better {
          search.came_from.insert(neighbor, current.coord);
          search.g_scores.insert(neighbor, tentative_g);

          let h = graph.distance(neighbor, search.goal) * self.config.heuristic_weight;

          // Tie-breaking using cross-product for straighter paths
          let tie_break =
            graph.tie_break(neighbor, search.start, search.goal) * self.config.tie_breaking;

          let f = tentative_g + h + tie_break;

          search.open_set.push(AStarNode {
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
  fn reconstruct_path<G: Graph>(&self, graph: &G, search: &Search<G::Key>) -> Path {
    let mut path_coords = Vec::new();
    let mut current = search.goal;

    while current != search.start {
      path_coords.push(current);
      if let Some(&prev) = search.came_from.get(&current) {
        current = prev;
      } else {
        break;
      }
    }
    path_coords.push(search.start);
    path_coords.reverse();

    let waypoints: Vec<PathNode> = path_coords
      .into_iter()
      .map(|coord| graph.node(coord))
      .collect();

    Path::new(waypoints, search.grid_generation)
  }

  /// Apply configured smoothing to a path result
  fn apply_smoothing<G: Graph>(&self, result: PathResult, graph: &G) -> PathResult {
    match result {
      PathResult::Found(mut path) => {
        match self.config.default_smoothing {
          PathSmoothing::None => {}
          PathSmoothing::LineOfSight => {
            path.smooth_runs_line_of_sight(|floor| graph.grid_of(floor));
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

/// The heuristic's distance from `from` to `to`, in steps: octile on a
/// square world grid; on a layer grid (`NavigationGrid::for_layer`) the
/// straight world distance between the cells' centres over the shortest
/// step, which no path of its steps undercuts
pub(super) fn distance(grid: &NavigationGrid, from: GridCoord, to: GridCoord) -> f32 {
  match grid.steps_between(from, to) {
    Some(offset) => offset.length() as f32,
    None => octile_distance(from, to),
  }
}

/// How far `neighbor` strays from the line from `start` to `goal`, for
/// breaking ties toward straight paths: the cross product of their offsets
/// from the goal, in cells on a square world grid, and on a layer grid
/// between their world centres over the shortest step
pub(super) fn tie_break(
  grid: &NavigationGrid,
  neighbor: GridCoord,
  start: GridCoord,
  goal: GridCoord,
) -> f32 {
  if let (Some(a), Some(b)) = (
    grid.steps_between(goal, neighbor),
    grid.steps_between(goal, start),
  ) {
    return a.perp_dot(b).abs() as f32;
  }
  let dx1 = neighbor.x - goal.x;
  let dy1 = neighbor.y - goal.y;
  let dx2 = start.x - goal.x;
  let dy2 = start.y - goal.y;
  ((dx1 * dy2 - dx2 * dy1) as f32).abs()
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
  use crate::core::ecs::{IVec2, UVec2, Vec2};
  use crate::core::physics::CellRect;
  use crate::core::tilemap::{GridShape, LayerGrid, MapGrid, StaggerAxis, StaggerIndex};
  use glam::Affine2;

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
  fn a_path_on_a_floor_s_grid_carries_its_floor_one_node_or_more() {
    let grid = NavigationGrid::new(100.0, 100.0, 10.0).on_floor(Floor::On(1));
    let mut pathfinder = Pathfinder::new();
    let floors = |result: PathResult| -> Vec<Floor> {
      match result {
        PathResult::Found(path) => path.waypoints().iter().map(|n| n.floor).collect(),
        other => panic!("{other:?}"),
      }
    };
    let (here, there) = (GridCoord::new(2, 3), GridCoord::new(7, 3));
    // One node, from each entry point: its cell is its goal
    assert_eq!(
      floors(pathfinder.find_path_grid(&grid, here, here)),
      [Floor::On(1)]
    );
    assert_eq!(
      floors(pathfinder.find_path(&grid, 25.0, 35.0, 25.0, 35.0)),
      [Floor::On(1)]
    );
    let (_, sliced) = pathfinder.start_pathfinding(&grid, 25.0, 35.0, 25.0, 35.0);
    assert_eq!(floors(sliced), [Floor::On(1)]);
    // More
    let found = floors(pathfinder.find_path_grid(&grid, here, there));
    assert!(found.len() > 1 && found.iter().all(|&f| f == Floor::On(1)));
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

  fn layer(shape: GridShape, cell: UVec2) -> LayerGrid {
    LayerGrid {
      grid: MapGrid::new(shape, cell),
      to_world: Affine2::IDENTITY,
    }
  }

  #[test]
  fn isometric_steps_cost_their_screen_distance() {
    // Lattice axes (58, 27.5) and (-58, 27.5): a step along the lattice's
    // diagonal is 55 px straight down, the shortest; a side step is 64.2 px,
    // and one along the other diagonal 116 px across
    let layer = layer(GridShape::Isometric, UVec2::new(116, 55));
    let grid = NavigationGrid::for_layer(layer, CellRect::new(IVec2::splat(-3), IVec2::splat(10)));
    let step_min = grid.cell_size();
    assert_eq!(step_min, 55.0);
    let around = grid.neighbors(GridCoord::new(0, 0));
    assert_eq!(around.len(), 8);
    let cost_to = |x: i32, y: i32| {
      around
        .iter()
        .find(|(cell, _)| *cell == GridCoord::new(x, y))
        .map_or(f32::NAN, |&(_, cost)| cost)
    };
    assert_eq!(cost_to(1, 1), 1.0);
    assert!((cost_to(1, 0) - Vec2::new(58.0, 27.5).length() / 55.0).abs() < 1e-5);
    assert!((cost_to(1, -1) - 116.0 / 55.0).abs() < 1e-5);
    let Some(path) = Pathfinder::new()
      .find_path_grid(&grid, GridCoord::new(0, 0), GridCoord::new(6, 6))
      .path()
    else {
      panic!("a path")
    };
    let cells: Vec<GridCoord> = path.waypoints().iter().map(|w| w.grid_coord).collect();
    let diagonal: Vec<GridCoord> = (0..=6).map(|i| GridCoord::new(i, i)).collect();
    assert_eq!(cells, diagonal, "along the lattice diagonal");
    let cost: f32 = path
      .waypoints()
      .windows(2)
      .map(|pair| pair[0].distance(&pair[1]) / step_min)
      .sum();
    assert!((cost - 6.0 * 55.0 / step_min).abs() < 1e-3, "{cost}");
  }

  #[test]
  fn a_layer_grid_breaks_ties_as_the_square_grid_does() {
    // On an orthogonal layer of 16 px cells, the world centres over the
    // shortest step are the cells' indices (plus a half), so the cross
    // product is the square grid's, while the heuristic is the straight
    // distance in steps where the square grid's is octile
    let square = NavigationGrid::new(480.0, 320.0, 16.0);
    let window = CellRect::new(IVec2::ZERO, IVec2::new(30, 20));
    let on_layer =
      NavigationGrid::for_layer(layer(GridShape::Orthogonal, UVec2::splat(16)), window);
    let mut seed = 0x1234_5678_u32;
    let mut cell = || {
      let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
      };
      GridCoord::new((next() % 30) as i32, (next() % 20) as i32)
    };
    for _ in 0..200 {
      let (n, start, goal) = (cell(), cell(), cell());
      assert_eq!(
        tie_break(&on_layer, n, start, goal),
        tie_break(&square, n, start, goal)
      );
    }
    let (a, b) = (GridCoord::new(0, 0), GridCoord::new(3, 4));
    assert_eq!(distance(&on_layer, a, b), 5.0);
    assert_eq!(distance(&square, a, b), octile_distance(a, b));
    // On a turned, scaled hex layer it is the cross product of the world
    // centres' offsets, over the shortest step squared
    let hex = LayerGrid {
      grid: MapGrid::new(
        GridShape::Hexagonal {
          axis: StaggerAxis::X,
          index: StaggerIndex::Even,
          side: 20,
        },
        UVec2::new(40, 48),
      ),
      to_world: Affine2::from_scale_angle_translation(
        Vec2::splat(1.5),
        0.4,
        Vec2::new(-30.0, 70.0),
      ),
    };
    let on_hex = NavigationGrid::for_layer(hex, CellRect::new(IVec2::splat(-8), IVec2::splat(8)));
    let step = on_hex.cell_size();
    for _ in 0..200 {
      let (n, start, goal) = (cell(), cell(), cell());
      let centre = |c: GridCoord| hex.cell_center_world(IVec2::new(c.x - 15, c.y - 10));
      let shifted = |c: GridCoord| GridCoord::new(c.x - 15, c.y - 10);
      let (dn, ds) = (centre(n) - centre(goal), centre(start) - centre(goal));
      let expected = dn.perp_dot(ds).abs() / (step * step);
      let found = tie_break(&on_hex, shifted(n), shifted(start), shifted(goal));
      assert!(
        (found - expected).abs() <= 1e-3 * expected.max(1.0),
        "{found} {expected}"
      );
    }
  }

  /// Times 100 paths across a 256 x 256 window of each of three layer
  /// grids, the speed guards' (`benches/tilemap.rs`): orthogonal cells of
  /// 16 px, isometric ones of 32 x 16 and hexagons of 16 x 16 with sides of
  /// 8 (pointy-topped, odd rows shifted), each with a 3 x 3 clump of
  /// blocked cells every 8 (the edit guard's islands). Each path joins two
  /// walkable cells picked from a fixed seed, with the pathfinder's
  /// defaults but a node budget of the window's cells, so every one is
  /// found. Prints each grid's median, 99th percentile and longest, and
  /// the paths' median length; then how many came out dearer than the
  /// cheapest (the same paths found with the tie-break off), and by how
  /// much at most. Run it in release:
  /// `cargo test --release -p ryo2d --lib path_timing -- --ignored --nocapture`
  #[test]
  #[ignore = "timing; run in release by hand"]
  fn path_timing() {
    use super::super::grid::CellState;
    use std::time::Instant;
    const SIDE: i32 = 256;
    const PATHS: usize = 100;
    let window = CellRect::new(IVec2::ZERO, IVec2::splat(SIDE));
    let grids = [
      ("orthogonal 16 px", GridShape::Orthogonal, UVec2::splat(16)),
      (
        "isometric 32 x 16",
        GridShape::Isometric,
        UVec2::new(32, 16),
      ),
      (
        "hexagonal 16 x 16, side 8",
        GridShape::Hexagonal {
          axis: StaggerAxis::Y,
          index: StaggerIndex::Odd,
          side: 8,
        },
        UVec2::splat(16),
      ),
    ];
    let pathfinder = Pathfinder::with_config(PathfinderConfig {
      max_total_nodes: (SIDE * SIDE) as usize,
      ..PathfinderConfig::default()
    });
    let cheapest = Pathfinder::with_config(PathfinderConfig {
      tie_breaking: 0.0,
      ..*pathfinder.config()
    });
    // A path's cost in steps: every cell here costs 1
    let cost = |grid: &NavigationGrid, path: &Path| {
      let steps: f32 = path
        .waypoints()
        .windows(2)
        .map(|pair| pair[0].distance(&pair[1]))
        .sum();
      steps / grid.cell_size()
    };
    for (name, shape, cell) in grids {
      let mut grid = NavigationGrid::for_layer(layer(shape, cell), window);
      for y in 0..SIDE {
        for x in 0..SIDE {
          if (2..5).contains(&(x % 8)) && (2..5).contains(&(y % 8)) {
            grid.set(GridCoord::new(x, y), CellState::Blocked);
          }
        }
      }
      let mut seed = 0x9e37_79b9_u32;
      let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % SIDE as u32) as i32
      };
      let mut walkable = || loop {
        let cell = GridCoord::new(next(), next());
        if grid.is_walkable(cell) {
          return cell;
        }
      };
      let pairs: Vec<(GridCoord, GridCoord)> =
        (0..PATHS).map(|_| (walkable(), walkable())).collect();
      let (mut times, mut lengths) = (Vec::with_capacity(PATHS), Vec::with_capacity(PATHS));
      let (mut dearer, mut most) = (0, (0.0f32, 0.0f32));
      for (start, goal) in pairs {
        let begin = Instant::now();
        let found = pathfinder.find_path_grid(&grid, start, goal);
        times.push(begin.elapsed());
        let (Some(path), Some(best)) = (
          found.path(),
          cheapest.find_path_grid(&grid, start, goal).path(),
        ) else {
          panic!("{name}: no path from {start:?} to {goal:?}");
        };
        lengths.push(path.waypoints().len());
        let (paid, least) = (cost(&grid, &path), cost(&grid, &best));
        if paid > least + 1e-3 {
          dearer += 1;
          if paid / least > 1.0 + most.0 {
            most = (paid / least - 1.0, paid - least);
          }
        }
      }
      times.sort_unstable();
      lengths.sort_unstable();
      let p99 = times[(PATHS * 99).div_ceil(100) - 1];
      println!(
        "{name}: {PATHS} paths across a 256 x 256 window: median {:?}, 99th percentile {p99:?}, \
         longest {:?}; the paths' median length {} cells; {dearer} dearer than the cheapest, \
         the most by {:.2}% ({:.2} steps)",
        times[PATHS / 2],
        times[PATHS - 1],
        lengths[PATHS / 2],
        most.0 * 100.0,
        most.1,
      );
    }
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
