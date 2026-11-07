//! Navigation and pathfinding integration tests
//!
//! These tests verify the navigation system works correctly:
//! - Grid-based pathfinding
//! - NavAgent movement simulation
//! - Path smoothing
//! - Time-sliced pathfinding for many agents
//! - Performance with 100+ agents

use ryo2d::prelude::*;

// ============================================================================
// NAVIGATION GRID TESTS
// ============================================================================

/// Test that NavigationGrid correctly converts between world and grid coordinates
#[test]
fn grid_coordinate_conversion_roundtrip() {
  let grid = NavigationGrid::new(800.0, 600.0, 16.0);

  // Test various world positions
  let test_points = [(0.0, 0.0), (100.0, 100.0), (400.0, 300.0), (799.0, 599.0)];

  for (wx, wy) in test_points {
    let coord = grid.world_to_grid(wx, wy);
    let (cx, cy) = grid.grid_to_world(coord);

    // World position should be within the cell that contains original point
    let (corner_x, corner_y) = grid.grid_to_world_corner(coord);
    assert!(
      wx >= corner_x && wx < corner_x + grid.cell_size(),
      "X {} should be in cell starting at {}",
      wx,
      corner_x
    );
    assert!(
      wy >= corner_y && wy < corner_y + grid.cell_size(),
      "Y {} should be in cell starting at {}",
      wy,
      corner_y
    );

    // Center should be offset by half cell size from corner
    assert_eq!(cx, corner_x + grid.cell_size() / 2.0);
    assert_eq!(cy, corner_y + grid.cell_size() / 2.0);
  }
}

/// Test that blocking cells affects pathfinding correctly
#[test]
fn blocked_cells_prevent_pathfinding() {
  let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
  let pathfinder = Pathfinder::new();

  // Block entire vertical line
  for y in 0..10 {
    grid.set(GridCoord::new(5, y), CellState::Blocked);
  }

  // Path should fail - no way to get from left to right
  let result = pathfinder.find_path(&grid, 15.0, 50.0, 85.0, 50.0);
  assert_eq!(result, PathResult::NoPath);
}

/// Test that gaps in walls allow pathfinding
#[test]
fn path_through_gap_in_wall() {
  let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
  let pathfinder = Pathfinder::new();

  // Block vertical line with gap in middle
  for y in 0..10 {
    if y != 5 {
      // Leave gap at y=5
      grid.set(GridCoord::new(5, y), CellState::Blocked);
    }
  }

  // Path should succeed through the gap
  let result = pathfinder.find_path(&grid, 15.0, 55.0, 85.0, 55.0);
  assert!(result.is_found(), "Path should exist through gap");
}

/// Test difficult terrain affects path cost
#[test]
fn difficult_terrain_affects_path() {
  let mut grid = NavigationGrid::new(100.0, 50.0, 10.0);
  let pathfinder = Pathfinder::new();

  // Make middle row difficult
  for x in 0..10 {
    grid.set(GridCoord::new(x, 2), CellState::Difficult(5.0));
  }

  // Path from left to right should exist
  let result = pathfinder.find_path(&grid, 5.0, 25.0, 95.0, 25.0);
  assert!(result.is_found());

  // The path might go around the difficult terrain if it's cheaper
  // This test mainly verifies difficult terrain doesn't block
}

// ============================================================================
// PATHFINDER TESTS
// ============================================================================

/// Test A* finds optimal path on open grid
#[test]
fn astar_finds_direct_path() {
  let grid = NavigationGrid::new(100.0, 100.0, 10.0);
  let pathfinder = Pathfinder::new();

  let result = pathfinder.find_path(&grid, 5.0, 5.0, 95.0, 5.0);
  assert!(result.is_found());

  let path = result.path().unwrap();
  // Direct path should be roughly 9 cells (90 / 10)
  assert!(path.len() >= 2, "Path should have at least start and end");
  assert!(path.len() <= 12, "Direct path should be efficient");
}

/// Test A* navigates around L-shaped obstacle
#[test]
fn astar_navigates_l_shaped_obstacle() {
  let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
  let pathfinder = Pathfinder::new();

  // Create L-shaped obstacle
  for x in 3..8 {
    grid.set(GridCoord::new(x, 5), CellState::Blocked);
  }
  for y in 2..6 {
    grid.set(GridCoord::new(5, y), CellState::Blocked);
  }

  // Path from bottom-left to top-right should go around
  let result = pathfinder.find_path(&grid, 15.0, 85.0, 85.0, 15.0);
  assert!(result.is_found());

  let path = result.path().unwrap();
  assert!(path.len() > 2, "Path should have intermediate waypoints");
}

/// Test pathfinding to same cell returns single-point path
#[test]
fn same_cell_path_is_single_point() {
  let grid = NavigationGrid::new(100.0, 100.0, 10.0);
  let pathfinder = Pathfinder::new();

  // Both points in same cell
  let result = pathfinder.find_path(&grid, 5.0, 5.0, 8.0, 8.0);
  assert!(result.is_found());

  let path = result.path().unwrap();
  assert_eq!(path.len(), 1, "Same-cell path should have one point");
}

/// Test invalid endpoints return appropriate error
#[test]
fn invalid_endpoints_return_error() {
  let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
  let pathfinder = Pathfinder::new();

  // Block the goal
  grid.set(GridCoord::new(9, 5), CellState::Blocked);

  let result = pathfinder.find_path(&grid, 5.0, 55.0, 95.0, 55.0);
  assert_eq!(result, PathResult::InvalidEndpoints);

  // Block the start
  grid.set(GridCoord::new(0, 5), CellState::Blocked);
  let result = pathfinder.find_path(&grid, 5.0, 55.0, 50.0, 55.0);
  assert_eq!(result, PathResult::InvalidEndpoints);
}

// ============================================================================
// PATH SMOOTHING TESTS
// ============================================================================

/// Test line-of-sight smoothing reduces waypoints
#[test]
fn line_of_sight_smoothing_reduces_waypoints() {
  let grid = NavigationGrid::new(200.0, 100.0, 10.0);
  let pathfinder = Pathfinder::new();

  // Find path on open grid
  let result = pathfinder.find_path(&grid, 5.0, 50.0, 195.0, 50.0);
  assert!(result.is_found());

  let mut path = result.path().unwrap();
  let original_len = path.len();

  // Apply smoothing
  path.smooth_line_of_sight(&grid);

  // Smoothed path should have fewer waypoints
  assert!(
    path.len() <= original_len,
    "Smoothed path ({}) should have <= original ({})",
    path.len(),
    original_len
  );

  // On completely open grid, should reduce to just start and end
  assert_eq!(path.len(), 2, "Open grid path should smooth to 2 points");
}

/// Test line-of-sight smoothing preserves path around obstacles
#[test]
fn smoothing_preserves_obstacle_avoidance() {
  let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
  let pathfinder = Pathfinder::new();

  // Create obstacle in the middle
  for x in 4..7 {
    for y in 4..7 {
      grid.set(GridCoord::new(x, y), CellState::Blocked);
    }
  }

  let result = pathfinder.find_path(&grid, 15.0, 15.0, 85.0, 85.0);
  assert!(result.is_found());

  let mut path = result.path().unwrap();
  path.smooth_line_of_sight(&grid);

  // Path should still be valid (not pass through obstacle)
  for waypoint in path.waypoints() {
    let coord = grid.world_to_grid(waypoint.x, waypoint.y);
    assert!(
      grid.is_walkable(coord),
      "Smoothed path passes through blocked cell at {:?}",
      coord
    );
  }
}

// ============================================================================
// TIME-SLICED PATHFINDING TESTS
// ============================================================================

/// Test time-sliced pathfinding completes correctly
#[test]
fn time_sliced_pathfinding_completes() {
  let grid = NavigationGrid::new(200.0, 200.0, 10.0);
  let config = PathfinderConfig::default().with_time_slicing(5); // Very small slice
  let mut pathfinder = Pathfinder::with_config(config);

  let (request_id, initial) = pathfinder.start_pathfinding(&grid, 5.0, 5.0, 195.0, 195.0);

  assert!(
    initial.is_in_progress(),
    "Should start in progress with small slice"
  );

  // Continue until complete
  let mut result = PathResult::InProgress;
  let mut iterations = 0;
  while result.is_in_progress() && iterations < 1000 {
    result = pathfinder.continue_pathfinding(&grid, request_id);
    iterations += 1;
  }

  assert!(
    result.is_found(),
    "Time-sliced path should eventually complete"
  );
  assert!(
    iterations > 1,
    "Should have taken multiple iterations with tiny slice"
  );
}

/// Test canceling time-sliced pathfinding
#[test]
fn time_sliced_pathfinding_cancellation() {
  let grid = NavigationGrid::new(200.0, 200.0, 10.0);
  let config = PathfinderConfig::default().with_time_slicing(5);
  let mut pathfinder = Pathfinder::with_config(config);

  let (request_id, _) = pathfinder.start_pathfinding(&grid, 5.0, 5.0, 195.0, 195.0);

  assert_eq!(pathfinder.active_request_count(), 1);

  pathfinder.cancel_pathfinding(request_id);

  assert_eq!(pathfinder.active_request_count(), 0);

  // Continuing canceled request should return NoPath
  let result = pathfinder.continue_pathfinding(&grid, request_id);
  assert_eq!(result, PathResult::NoPath);
}

// ============================================================================
// NAVAGENT COMPONENT TESTS
// ============================================================================

/// Test NavAgent state transitions
#[test]
fn nav_agent_state_machine() {
  let mut agent = NavAgent::new(100.0);

  assert_eq!(agent.state(), NavAgentState::Idle);
  assert!(!agent.has_path());

  // Set a path
  let path = Path::new(
    vec![
      PathNode::new(0.0, 0.0, GridCoord::new(0, 0)),
      PathNode::new(100.0, 0.0, GridCoord::new(10, 0)),
    ],
    1,
  );
  agent.set_path(path);

  assert_eq!(agent.state(), NavAgentState::Moving);
  assert!(agent.has_path());

  // Fail
  agent.fail();
  assert_eq!(agent.state(), NavAgentState::Failed);
  assert!(!agent.has_path());

  // Stop and go idle
  agent.stop();
  assert_eq!(agent.state(), NavAgentState::Idle);
}

/// Test NavAgent with World integration
#[test]
fn nav_agent_world_integration() {
  let mut world = World::new();

  let entity = world
    .spawn((
      Transform::from_xy(100.0, 100.0),
      NavAgent::new(80.0)
        .with_arrival_threshold(8.0)
        .with_smoothing(PathSmoothing::LineOfSight),
    ))
    .id();

  // Verify components exist
  let agent = world.get::<NavAgent>(entity);
  assert!(agent.is_some());

  let agent = agent.unwrap();
  assert_eq!(agent.speed, 80.0);
  assert_eq!(agent.arrival_threshold, 8.0);
  assert_eq!(agent.path_smoothing, PathSmoothing::LineOfSight);
}

/// Test NavTarget component
#[test]
fn nav_target_position_and_follow() {
  let mut world = World::new();

  let player = world.spawn((Transform::from_xy(500.0, 500.0),)).id();

  // Position target
  let target = NavTarget::position(200.0, 300.0);
  assert_eq!(target.x, 200.0);
  assert_eq!(target.y, 300.0);
  assert!(!target.dynamic_target);

  // Follow target
  let follow_target = NavTarget::follow(player);
  assert!(follow_target.dynamic_target);
  assert_eq!(follow_target.follow_entity, Some(player));
}

// ============================================================================
// PERFORMANCE TESTS
// ============================================================================

/// Test pathfinding performance with 100 simultaneous agents
#[test]
fn performance_100_agents() {
  let grid = NavigationGrid::new(500.0, 500.0, 16.0);
  let config = PathfinderConfig::default().with_time_slicing(50);
  let mut pathfinder = Pathfinder::with_config(config);

  let mut requests = Vec::new();

  // Start 100 pathfinding requests
  for i in 0..100 {
    let start_x = 20.0 + (i % 10) as f32 * 20.0;
    let start_y = 20.0 + (i / 10) as f32 * 20.0;
    let goal_x = 480.0 - (i % 10) as f32 * 20.0;
    let goal_y = 480.0 - (i / 10) as f32 * 20.0;

    let (id, _) = pathfinder.start_pathfinding(&grid, start_x, start_y, goal_x, goal_y);
    requests.push(id);
  }

  // Process all requests
  let mut completed = 0;
  let mut iterations = 0;
  let max_iterations = 10000;

  while completed < 100 && iterations < max_iterations {
    for &id in &requests {
      let result = pathfinder.continue_pathfinding(&grid, id);
      if result.is_found() || matches!(result, PathResult::NoPath) {
        completed += 1;
      }
    }
    iterations += 1;
  }

  assert_eq!(completed, 100, "All 100 paths should complete");
  println!(
    "100 agents completed in {} iterations ({} paths/iteration avg)",
    iterations,
    100.0 / iterations as f32
  );
}

/// Test grid rebuild performance
#[test]
fn performance_grid_rebuild() {
  let mut world = World::new();

  // Spawn 50 obstacles
  for i in 0..50 {
    world.spawn((
      Transform::from_xy(100.0 + i as f32 * 10.0, 100.0 + i as f32 * 5.0),
      Collider::rect(16.0, 16.0),
    ));
  }

  // Measure rebuild time (just verify it completes)
  let grid = NavigationBuilder::new(800.0, 600.0)
    .with_cell_size(16.0)
    .build_from_world(&mut world);

  // Verify obstacles are blocked
  let (w, h) = grid.dimensions();
  let mut blocked_count = 0;
  for y in 0..h as i32 {
    for x in 0..w as i32 {
      if !grid.is_walkable(GridCoord::new(x, y)) {
        blocked_count += 1;
      }
    }
  }

  assert!(
    blocked_count > 0,
    "Grid should have some blocked cells from colliders"
  );
}

// ============================================================================
// PATH GENERATION TESTS
// ============================================================================

/// Test that grid generation changes when cells are modified
#[test]
fn grid_generation_tracking() {
  let mut grid = NavigationGrid::new(100.0, 100.0, 10.0);
  let initial_gen = grid.generation();

  grid.set(GridCoord::new(5, 5), CellState::Blocked);
  assert!(
    grid.generation() > initial_gen,
    "Generation should increase on modification"
  );

  let gen_after_block = grid.generation();

  // Same value doesn't increment
  grid.set(GridCoord::new(5, 5), CellState::Blocked);
  assert_eq!(
    grid.generation(),
    gen_after_block,
    "Same value shouldn't change generation"
  );
}

/// Test path staleness detection
#[test]
fn path_staleness_detection() {
  let grid = NavigationGrid::new(100.0, 100.0, 10.0);
  let pathfinder = Pathfinder::new();

  let result = pathfinder.find_path(&grid, 5.0, 5.0, 95.0, 95.0);
  let path = result.path().unwrap();

  assert!(!path.is_stale(grid.generation()));
  assert!(path.is_stale(grid.generation() + 1));
}

// ============================================================================
// GRIDS OVER A TILE LAYER'S CELLS
// ============================================================================

/// Numbers from a seed (xorshift), the same on every run
struct Seeded(u32);

impl Seeded {
  fn below(&mut self, n: u32) -> u32 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 17;
    self.0 ^= self.0 << 5;
    self.0 % n
  }

  /// A cell of `area`
  fn cell(&mut self, area: CellRect) -> IVec2 {
    let size = area.size();
    area.min + IVec2::new(self.below(size.x) as i32, self.below(size.y) as i32)
  }

  /// A walkable cell of `area` on `grid`
  fn open_cell(&mut self, grid: &NavigationGrid, area: CellRect) -> IVec2 {
    loop {
      let cell = self.cell(area);
      if grid.is_walkable(cell.into()) {
        return cell;
      }
    }
  }
}

/// A pathfinder that finds the cheapest path exactly: the tie-break, which
/// trades a little cost for straighter paths, is off
fn exact() -> Pathfinder {
  Pathfinder::with_config(PathfinderConfig {
    tie_breaking: 0.0,
    ..PathfinderConfig::default()
  })
}

fn layer_of(grid: MapGrid) -> LayerGrid {
  LayerGrid {
    grid,
    to_world: glam::Affine2::IDENTITY,
  }
}

/// What a path costs: each step's world length over the shortest step,
/// times the cost of the cell it enters
fn path_cost(grid: &NavigationGrid, path: &Path) -> f64 {
  path
    .waypoints()
    .windows(2)
    .map(|pair| {
      f64::from(pair[0].distance(&pair[1])) / f64::from(grid.cell_size())
        * f64::from(grid.cost(pair[1].grid_coord))
    })
    .sum()
}

/// The shortest world distance between the centres of neighbouring cells of
/// `area` (`neighbours`), by the layer's own geometry
fn shortest_step(
  layer: &LayerGrid,
  area: CellRect,
  neighbours: impl Fn(IVec2) -> Vec<IVec2>,
) -> f64 {
  let mut shortest = f64::INFINITY;
  for y in area.min.y..area.max.y {
    for x in area.min.x..area.max.x {
      let cell = IVec2::new(x, y);
      for next in neighbours(cell) {
        let length = layer
          .cell_center_world(cell)
          .as_dvec2()
          .distance(layer.cell_center_world(next).as_dvec2());
        shortest = shortest.min(length);
      }
    }
  }
  shortest
}

/// The cheapest cost from `start` to `goal` through `neighbours`, as
/// `path_cost` counts it: Dijkstra over the walkable cells
fn cheapest(
  grid: &NavigationGrid,
  layer: &LayerGrid,
  step: f64,
  (start, goal): (IVec2, IVec2),
  neighbours: impl Fn(IVec2) -> Vec<IVec2>,
) -> Option<f64> {
  use std::cmp::Reverse;
  use std::collections::{BinaryHeap, HashMap};
  let mut best: HashMap<IVec2, f64> = HashMap::from([(start, 0.0)]);
  // Costs are never negative, so their bits sort as they do
  let mut open = BinaryHeap::from([Reverse((0.0f64.to_bits(), start.x, start.y))]);
  while let Some(Reverse((bits, x, y))) = open.pop() {
    let (cost, cell) = (f64::from_bits(bits), IVec2::new(x, y));
    if cell == goal {
      return Some(cost);
    }
    if best.get(&cell).is_some_and(|&b| b < cost) {
      continue;
    }
    for next in neighbours(cell) {
      if !grid.is_walkable(next.into()) {
        continue;
      }
      let length = layer
        .cell_center_world(cell)
        .as_dvec2()
        .distance(layer.cell_center_world(next).as_dvec2());
      let through = cost + length / step * f64::from(grid.cost(next.into()));
      if best.get(&next).is_none_or(|&b| through < b) {
        best.insert(next, through);
        open.push(Reverse((through.to_bits(), next.x, next.y)));
      }
    }
  }
  None
}

#[test]
fn an_orthogonal_layer_grid_paths_as_the_square_grid_does() {
  let layer = layer_of(MapGrid::new(GridShape::Orthogonal, UVec2::splat(16)));
  let window = CellRect::new(IVec2::ZERO, IVec2::new(30, 20));
  let mut on_layer = NavigationGrid::for_layer(layer, window);
  let mut square = NavigationGrid::new(480.0, 320.0, 16.0);
  assert_eq!(on_layer.dimensions(), square.dimensions());
  assert_eq!(on_layer.cell_size(), square.cell_size());
  let mut seeded = Seeded(0x9e37_79b9);
  for _ in 0..150 {
    let wall = GridCoord::from(seeded.cell(window));
    on_layer.set(wall, CellState::Blocked);
    square.set(wall, CellState::Blocked);
  }
  let usual = Pathfinder::new();
  let mut found = 0;
  for _ in 0..50 {
    let from = GridCoord::from(seeded.open_cell(&square, window));
    let to = GridCoord::from(seeded.open_cell(&square, window));
    let (a, b) = (
      exact().find_path_grid(&on_layer, from, to),
      exact().find_path_grid(&square, from, to),
    );
    match (&a, &b) {
      (PathResult::Found(a), PathResult::Found(b)) => {
        found += 1;
        let (a, b) = (path_cost(&on_layer, a), path_cost(&square, b));
        assert!((a - b).abs() < 1e-3, "{from:?} to {to:?}: {a} and {b}");
      }
      _ => assert_eq!(a, b, "{from:?} to {to:?}"),
    }
    assert_eq!(
      usual.find_path_grid(&on_layer, from, to).is_found(),
      usual.find_path_grid(&square, from, to).is_found()
    );
  }
  assert!(found >= 30, "{found} paths found");
}

#[test]
fn hex_cells_have_six_neighbours_and_path_around_a_wall() {
  let grid = MapGrid::new(
    GridShape::Hexagonal {
      axis: StaggerAxis::Y,
      index: StaggerIndex::Odd,
      side: 70,
    },
    UVec2::new(120, 140),
  );
  let layer = layer_of(grid);
  let window = CellRect::new(IVec2::ZERO, IVec2::new(12, 8));
  let mut nav = NavigationGrid::for_layer(layer, window);
  // The grid's own six neighbours, each costing its world length over the
  // shortest step (120 px, along a row)
  let at = IVec2::new(5, 4);
  let around = nav.neighbors(at.into());
  let mut cells: Vec<(i32, i32)> = around.iter().map(|(c, _)| (c.x, c.y)).collect();
  let mut theirs: Vec<(i32, i32)> = grid.neighbours(at).map(|c| (c.x, c.y)).collect();
  cells.sort_unstable();
  theirs.sort_unstable();
  assert_eq!(cells, theirs);
  assert_eq!(cells.len(), 6);
  let step = shortest_step(&layer, window, |c| grid.neighbours(c).collect());
  assert_eq!(step, 120.0);
  assert_eq!(nav.cell_size(), 120.0);
  for &(cell, cost) in &around {
    let length = layer
      .cell_center_world(at)
      .distance(layer.cell_center_world(cell.into()));
    assert!((cost - length / 120.0).abs() < 1e-5, "{cell:?}: {cost}");
  }
  // A wall of 7 hexes down column 6, its gap at row 3
  for y in (0..8).filter(|&y| y != 3) {
    nav.set(GridCoord::new(6, y), CellState::Blocked);
  }
  let (from, to) = (IVec2::new(2, 5), IVec2::new(10, 1));
  let Some(path) = exact().find_path_grid(&nav, from.into(), to.into()).path() else {
    panic!("a path through the gap")
  };
  let cells: Vec<IVec2> = path
    .waypoints()
    .iter()
    .map(|w| w.grid_coord.into())
    .collect();
  assert!(cells.contains(&IVec2::new(6, 3)), "{cells:?}");
  for pair in cells.windows(2) {
    assert!(grid.neighbours(pair[0]).any(|n| n == pair[1]), "{pair:?}");
  }
  let Some(best) = cheapest(&nav, &layer, step, (from, to), |c| {
    grid.neighbours(c).collect()
  }) else {
    panic!("a way through the gap")
  };
  assert!((path_cost(&nav, &path) - best).abs() < 1e-3);
}

#[test]
fn hex_paths_cross_negative_rows_and_columns_on_every_variant() {
  let window = CellRect::new(IVec2::splat(-12), IVec2::splat(12));
  let (below, above) = (
    CellRect::new(IVec2::splat(-12), IVec2::ZERO),
    CellRect::new(IVec2::ZERO, IVec2::splat(12)),
  );
  let mut seeded = Seeded(0x2545_f491);
  let mut found = 0;
  for (cell, side) in [
    ((16, 16), 8),
    ((120, 140), 70),
    ((15, 17), 7),
    ((16, 16), 24),
  ] {
    for axis in [StaggerAxis::X, StaggerAxis::Y] {
      for index in [StaggerIndex::Odd, StaggerIndex::Even] {
        for origin in [Vec2::ZERO, Vec2::new(5.0, -3.0)] {
          let grid = MapGrid::new(
            GridShape::Hexagonal { axis, index, side },
            UVec2::new(cell.0, cell.1),
          )
          .with_origin(origin);
          let layer = layer_of(grid);
          let mut nav = NavigationGrid::for_layer(layer, window);
          for _ in 0..140 {
            nav.set(seeded.cell(window).into(), CellState::Blocked);
          }
          let step = shortest_step(&layer, window, |c| grid.neighbours(c).collect());
          assert!(
            (step - f64::from(nav.cell_size())).abs() < 1e-4 * step,
            "{grid:?}"
          );
          for _ in 0..5 {
            let from = seeded.open_cell(&nav, below);
            let to = seeded.open_cell(&nav, above);
            let best = cheapest(&nav, &layer, step, (from, to), |c| {
              grid.neighbours(c).collect()
            });
            match exact().find_path_grid(&nav, from.into(), to.into()) {
              PathResult::Found(path) => {
                found += 1;
                let cells: Vec<IVec2> = path
                  .waypoints()
                  .iter()
                  .map(|w| w.grid_coord.into())
                  .collect();
                assert_eq!((cells[0], cells[cells.len() - 1]), (from, to));
                for pair in cells.windows(2) {
                  assert!(
                    grid.neighbours(pair[0]).any(|n| n == pair[1]),
                    "{grid:?} {pair:?}"
                  );
                }
                let Some(best) = best else {
                  panic!("{grid:?}: a path Dijkstra does not find")
                };
                let cost = path_cost(&nav, &path);
                assert!(
                  (cost - best).abs() < 1e-3,
                  "{grid:?} {from} to {to}: {cost} for {best}"
                );
              }
              PathResult::NoPath => assert_eq!(best, None, "{grid:?} {from} to {to}"),
              other => panic!("{grid:?} {from} to {to}: {other:?}"),
            }
          }
        }
      }
    }
  }
  assert!(found >= 120, "{found} of 160 paths found");
}

#[test]
fn a_staggered_layer_grid_paths_in_its_own_cells() {
  let grid = MapGrid::new(
    GridShape::Staggered {
      axis: StaggerAxis::Y,
      index: StaggerIndex::Odd,
    },
    UVec2::new(64, 32),
  );
  let Some(lattice) = grid.lattice() else {
    panic!("a staggered grid has a lattice")
  };
  let layer = layer_of(grid);
  let window = CellRect::new(IVec2::new(-4, -6), IVec2::new(10, 18));
  let mut nav = NavigationGrid::for_layer(layer, window);
  let mut seeded = Seeded(0x0bad_cafe);
  for _ in 0..70 {
    nav.set(seeded.cell(window).into(), CellState::Blocked);
  }
  let mut found = 0;
  for _ in 0..20 {
    let (from, to) = (
      seeded.open_cell(&nav, window),
      seeded.open_cell(&nav, window),
    );
    let Some(path) = Pathfinder::new()
      .find_path_grid(&nav, from.into(), to.into())
      .path()
    else {
      continue;
    };
    found += 1;
    let cells: Vec<IVec2> = path
      .waypoints()
      .iter()
      .map(|w| w.grid_coord.into())
      .collect();
    assert_eq!((cells[0], cells[cells.len() - 1]), (from, to));
    for waypoint in path.waypoints() {
      let cell = IVec2::from(waypoint.grid_coord);
      assert!(window.contains(cell) && nav.is_walkable(waypoint.grid_coord));
      let centre = layer.cell_center_world(cell);
      assert_eq!((waypoint.x, waypoint.y), (centre.x, centre.y));
    }
    for pair in cells.windows(2) {
      let (a, b) = (lattice.lattice_cell(pair[0]), lattice.lattice_cell(pair[1]));
      let d = b - a;
      assert!(
        d != IVec2::ZERO && d.x.abs() <= 1 && d.y.abs() <= 1,
        "{pair:?} steps {d}"
      );
      if d.x != 0 && d.y != 0 {
        for side in [IVec2::new(d.x, 0), IVec2::new(0, d.y)] {
          let beside = lattice.grid_cell(a + side);
          assert!(
            nav.is_walkable(beside.into()),
            "{pair:?} cuts the corner at {beside}"
          );
        }
      }
    }
  }
  assert!(found >= 12, "{found} of 20 paths found");
}

/// The eight cells round `cell` in `lattice`'s squares (a staggered grid's
/// diamonds' lattice, or the grid's own cells), a diagonal only where both
/// cells beside it are walkable on `grid` (any, without one)
fn lattice_ways(lattice: Lattice, grid: Option<&NavigationGrid>, cell: IVec2) -> Vec<IVec2> {
  let at = lattice.lattice_cell(cell);
  let open = |d: IVec2| grid.is_none_or(|g| g.is_walkable(lattice.grid_cell(at + d).into()));
  let mut out = Vec::new();
  for dy in -1..=1 {
    for dx in -1..=1 {
      let way = IVec2::new(dx, dy);
      if way == IVec2::ZERO
        || (dx != 0 && dy != 0 && !(open(IVec2::new(dx, 0)) && open(IVec2::new(0, dy))))
      {
        continue;
      }
      out.push(lattice.grid_cell(at + way));
    }
  }
  out
}

#[test]
fn staggered_and_skewed_layer_grids_find_the_cheapest_path() {
  let grids = [
    MapGrid::new(
      GridShape::Staggered {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Odd,
      },
      UVec2::new(64, 32),
    ),
    MapGrid::new(
      GridShape::Oblique {
        skew: IVec2::new(12, -5),
      },
      UVec2::new(32, 24),
    ),
  ];
  let window = CellRect::new(IVec2::new(-8, -10), IVec2::new(12, 14));
  let mut seeded = Seeded(0x51ed_270b);
  for grid in grids {
    let Some(lattice) = grid.lattice() else {
      panic!("{grid:?} has a lattice")
    };
    let layer = layer_of(grid);
    let mut nav = NavigationGrid::for_layer(layer, window);
    for _ in 0..90 {
      nav.set(seeded.cell(window).into(), CellState::Blocked);
    }
    // The shortest step by the oracle's own measure, over an open grid
    let step = shortest_step(&layer, window, |c| lattice_ways(lattice, None, c));
    assert!(
      (step - f64::from(nav.cell_size())).abs() < 1e-4 * step,
      "{grid:?}"
    );
    let mut found = 0;
    for _ in 0..40 {
      let (from, to) = (
        seeded.open_cell(&nav, window),
        seeded.open_cell(&nav, window),
      );
      let best = cheapest(&nav, &layer, step, (from, to), |c| {
        lattice_ways(lattice, Some(&nav), c)
      });
      match exact().find_path_grid(&nav, from.into(), to.into()) {
        PathResult::Found(path) => {
          found += 1;
          let Some(best) = best else {
            panic!("{grid:?}: a path Dijkstra does not find")
          };
          let cost = path_cost(&nav, &path);
          assert!(
            (cost - best).abs() < 1e-3,
            "{grid:?} {from} to {to}: {cost} for {best}"
          );
        }
        PathResult::NoPath => assert_eq!(best, None, "{grid:?} {from} to {to}"),
        other => panic!("{grid:?} {from} to {to}: {other:?}"),
      }
    }
    assert!(found >= 30, "{grid:?}: {found} of 40 paths found");
  }
}

// ============================================================================
// PATHS ACROSS FLOORS
// ============================================================================

/// The cells of `area`, by row
fn cells_of(area: CellRect) -> Vec<IVec2> {
  (area.min.y..area.max.y)
    .flat_map(|y| (area.min.x..area.max.x).map(move |x| IVec2::new(x, y)))
    .collect()
}

/// Two floors over `window` of `layer`'s cells: the ground open, and floor
/// 1 open only over `platform` and `stairs`, joined to the ground at each
/// of `stairs`
fn two_floors(
  layer: LayerGrid,
  window: CellRect,
  platform: CellRect,
  stairs: &[IVec2],
) -> FloorNavigation {
  let ground = NavigationGrid::for_layer(layer, window);
  let mut upper = NavigationGrid::for_layer(layer, window).on_floor(Floor::On(1));
  for cell in cells_of(window) {
    if !platform.contains(cell) && !stairs.contains(&cell) {
      upper.set(cell.into(), CellState::Blocked);
    }
  }
  let links = stairs
    .iter()
    .map(|&cell| StairLink {
      cell: cell.into(),
      lower: Floor::GROUND,
      upper: Floor::On(1),
    })
    .collect();
  FloorNavigation::new(vec![ground, upper], links)
}

/// The cells a cell of `grid` steps to: its lattice's eight, the
/// diagonals by the corner rule on `nav` (any, without one), or its hex's
/// six
fn ways_of(grid: MapGrid, nav: Option<&NavigationGrid>, cell: IVec2) -> Vec<IVec2> {
  match grid.lattice() {
    Some(lattice) => lattice_ways(lattice, nav, cell),
    None => grid.neighbours(cell).collect(),
  }
}

/// The links `path` crosses on `nav`, each with the place of the node it
/// leaves; every other step goes to a neighbouring cell (`ways_of` on
/// `grid`) on one floor, walkable there, and every node is its cell's
/// centre
fn crossings(nav: &FloorNavigation, grid: MapGrid, path: &Path) -> Vec<(usize, StairLink)> {
  let nodes = path.waypoints();
  let mut crossed = Vec::new();
  for node in nodes {
    let Some(on) = nav.grid(node.floor) else {
      panic!("{node:?} is on a floor without a grid")
    };
    assert!(on.is_walkable(node.grid_coord), "{node:?}");
    let (x, y) = on.grid_to_world(node.grid_coord);
    assert_eq!((node.x, node.y), (x, y), "{node:?}");
  }
  for (at, pair) in nodes.windows(2).enumerate() {
    let (a, b) = (pair[0], pair[1]);
    if a.floor == b.floor {
      let Some(on) = nav.grid(a.floor) else {
        panic!("{a:?} is on a floor without a grid")
      };
      let (from, to) = (IVec2::from(a.grid_coord), IVec2::from(b.grid_coord));
      assert!(
        ways_of(grid, Some(on), from).contains(&to),
        "{from} to {to} on {:?}",
        a.floor
      );
      continue;
    }
    assert_eq!(a.grid_coord, b.grid_coord, "a floor changes at one cell");
    let link = nav.links().iter().find(|link| {
      link.cell == a.grid_coord
        && ((link.lower, link.upper) == (a.floor, b.floor)
          || (link.upper, link.lower) == (a.floor, b.floor))
    });
    let Some(&link) = link else {
      panic!("{a:?} to {b:?} crosses no link")
    };
    crossed.push((at, link));
  }
  crossed
}

#[test]
fn a_path_climbs_stairs_to_another_floor_and_back() {
  // Isometric cells: floor 1 a platform from (8, 3) to (12, 6), its stairs
  // up from the ground at (6, 4) and (7, 4)
  let grid = MapGrid::new(GridShape::Isometric, UVec2::new(64, 32));
  let layer = layer_of(grid);
  let window = CellRect::new(IVec2::ZERO, IVec2::new(14, 10));
  let platform = CellRect::new(IVec2::new(8, 3), IVec2::new(13, 7));
  let stairs = [IVec2::new(6, 4), IVec2::new(7, 4)];
  let nav = two_floors(layer, window, platform, &stairs);
  let floors: Vec<Floor> = nav.grids().map(|g| g.floor()).collect();
  assert_eq!(floors, [Floor::GROUND, Floor::On(1)]);
  let below = (Floor::GROUND, GridCoord::new(1, 4));
  let above = (Floor::On(1), GridCoord::new(11, 5));
  // The ground under the platform is open too: only the floors the path's
  // nodes carry say it climbed
  assert!(
    nav
      .grid(Floor::GROUND)
      .is_some_and(|g| g.is_walkable(above.1))
  );
  for (from, to) in [(below, above), (above, below)] {
    let Some(path) = Pathfinder::new()
      .find_floor_path_grid(&nav, from, to)
      .path()
    else {
      panic!("a way from {from:?} to {to:?} by the stairs")
    };
    let nodes = path.waypoints();
    let ends = (nodes[0], nodes[nodes.len() - 1]);
    assert_eq!((ends.0.floor, ends.0.grid_coord), from);
    assert_eq!((ends.1.floor, ends.1.grid_coord), to);
    // Its floor changes once, between two nodes at a stair cell
    let crossed = crossings(&nav, grid, &path);
    assert_eq!(crossed.len(), 1, "{nodes:?}");
    let (at, link) = crossed[0];
    assert!(stairs.contains(&link.cell.into()), "{link:?}");
    assert_eq!((nodes[at].floor, nodes[at + 1].floor), (from.0, to.0));
    assert!(nodes[..=at].iter().all(|node| node.floor == from.0));
    assert!(nodes[at + 1..].iter().all(|node| node.floor == to.0));
    // Found again from world points, each through its floor's grid
    let (start, goal) = (
      layer.cell_center_world(from.1.into()),
      layer.cell_center_world(to.1.into()),
    );
    let again = Pathfinder::new().find_floor_path(&nav, start, from.0, goal, to.0);
    assert_eq!(
      again.path().map(|p| p.waypoints().to_vec()),
      Some(nodes.to_vec())
    );
  }
  // Without its stairs floor 1 is out of reach; a cell off the platform,
  // or on a floor without a grid, is no end
  let unlinked = FloorNavigation::new(nav.grids().cloned().collect(), Vec::new());
  let search = Pathfinder::new();
  assert_eq!(
    search.find_floor_path_grid(&unlinked, below, above),
    PathResult::NoPath
  );
  let off = (Floor::On(1), GridCoord::new(2, 2));
  assert_eq!(
    search.find_floor_path_grid(&nav, below, off),
    PathResult::InvalidEndpoints
  );
  let nowhere = (Floor::On(2), above.1);
  assert_eq!(
    search.find_floor_path_grid(&nav, below, nowhere),
    PathResult::InvalidEndpoints
  );
  // One floor's cell to itself is a path of that one node
  let Some(stay) = search.find_floor_path_grid(&nav, above, above).path() else {
    panic!("a path of one node")
  };
  let node = stay.waypoints();
  assert_eq!(node.len(), 1);
  assert_eq!((node[0].floor, node[0].grid_coord), above);
}

/// What the search charges for a step across a stair link, in steps: a
/// thousandth of one, so that of paths otherwise as cheap it takes the one
/// changing floor least
const LINK_COST: f64 = 1e-3;

/// What a path across floors costs: `path_cost` with each step on the
/// floor it enters, a step onto another floor at a link costing
/// `LINK_COST`
fn floor_path_cost(nav: &FloorNavigation, path: &Path) -> f64 {
  path
    .waypoints()
    .windows(2)
    .map(|pair| match nav.grid(pair[1].floor) {
      Some(_) if pair[0].floor != pair[1].floor => LINK_COST,
      Some(grid) => {
        f64::from(pair[0].distance(&pair[1])) / f64::from(grid.cell_size())
          * f64::from(grid.cost(pair[1].grid_coord))
      }
      None => f64::INFINITY,
    })
    .sum()
}

/// A floor and a cell on it
type FloorCell = (Floor, IVec2);

/// The cheapest cost from `start` to `goal` across `nav`'s floors, as
/// `floor_path_cost` counts it: `cheapest`'s Dijkstra over the walkable
/// cells of every floor, each stepping to its `neighbours` on its floor's
/// grid, and from a link's cell onto the same cell of the link's other
/// floor for `LINK_COST`
fn cheapest_across_floors(
  nav: &FloorNavigation,
  layer: &LayerGrid,
  step: f64,
  (start, goal): (FloorCell, FloorCell),
  neighbours: impl Fn(&NavigationGrid, IVec2) -> Vec<IVec2>,
) -> Option<f64> {
  use std::cmp::Reverse;
  use std::collections::{BinaryHeap, HashMap};
  let number = |floor: Floor| floor.number().unwrap_or(u8::MAX);
  let mut best: HashMap<(u8, IVec2), f64> = HashMap::from([((number(start.0), start.1), 0.0)]);
  // Costs are never negative, so their bits sort as they do
  let mut open = BinaryHeap::from([Reverse((
    0.0f64.to_bits(),
    number(start.0),
    start.1.x,
    start.1.y,
  ))]);
  while let Some(Reverse((bits, on, x, y))) = open.pop() {
    let (cost, floor, cell) = (f64::from_bits(bits), Floor::On(on), IVec2::new(x, y));
    if (floor, cell) == goal {
      return Some(cost);
    }
    if best.get(&(on, cell)).is_some_and(|&b| b < cost) {
      continue;
    }
    let Some(grid) = nav.grid(floor) else {
      continue;
    };
    let mut next = Vec::new();
    for to in neighbours(grid, cell) {
      if !grid.is_walkable(to.into()) {
        continue;
      }
      let length = layer
        .cell_center_world(cell)
        .as_dvec2()
        .distance(layer.cell_center_world(to).as_dvec2());
      next.push((
        on,
        to,
        cost + length / step * f64::from(grid.cost(to.into())),
      ));
    }
    for link in nav
      .links()
      .iter()
      .filter(|link| IVec2::from(link.cell) == cell)
    {
      let other = if link.lower == floor {
        link.upper
      } else if link.upper == floor {
        link.lower
      } else {
        continue;
      };
      if nav.grid(other).is_some_and(|g| g.is_walkable(link.cell)) {
        next.push((number(other), cell, cost + LINK_COST));
      }
    }
    for (on, to, through) in next {
      if best.get(&(on, to)).is_none_or(|&b| through < b) {
        best.insert((on, to), through);
        open.push(Reverse((through.to_bits(), on, to.x, to.y)));
      }
    }
  }
  None
}

/// Two random floors over `window` of `layer`'s cells: the ground with
/// walls and dear cells about it, floor 1 open only over a few rooms, and
/// links between them at a few cells, open on both
fn random_floors(seeded: &mut Seeded, layer: LayerGrid, window: CellRect) -> FloorNavigation {
  let mut ground = NavigationGrid::for_layer(layer, window);
  for _ in 0..18 {
    ground.set(seeded.cell(window).into(), CellState::Blocked);
  }
  for _ in 0..10 {
    ground.set(seeded.cell(window).into(), CellState::Difficult(2.5));
  }
  let mut upper = NavigationGrid::for_layer(layer, window).on_floor(Floor::On(1));
  for cell in cells_of(window) {
    upper.set(cell.into(), CellState::Blocked);
  }
  for _ in 0..3 {
    let min = seeded.cell(window);
    let size = IVec2::new(2 + seeded.below(4) as i32, 2 + seeded.below(3) as i32);
    let room = CellRect::new(min, (min + size).min(window.max));
    for cell in cells_of(room) {
      let dear = seeded.below(6) == 0;
      let state = if dear {
        CellState::Difficult(1.5)
      } else {
        CellState::Walkable
      };
      upper.set(cell.into(), state);
    }
  }
  let mut links = Vec::new();
  for _ in 0..4 {
    let cell = GridCoord::from(seeded.cell(window));
    ground.set(cell, CellState::Walkable);
    upper.set(cell, CellState::Walkable);
    links.push(StairLink {
      cell,
      lower: Floor::GROUND,
      upper: Floor::On(1),
    });
  }
  FloorNavigation::new(vec![ground, upper], links)
}

#[test]
fn floor_paths_are_the_cheapest() {
  let window = CellRect::new(IVec2::new(-3, -2), IVec2::new(9, 8));
  let grids = [
    MapGrid::new(GridShape::Orthogonal, UVec2::splat(16)),
    MapGrid::new(GridShape::Isometric, UVec2::new(64, 32)),
    MapGrid::new(
      GridShape::Staggered {
        axis: StaggerAxis::X,
        index: StaggerIndex::Even,
      },
      UVec2::new(64, 32),
    ),
    MapGrid::new(
      GridShape::Hexagonal {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Odd,
        side: 8,
      },
      UVec2::splat(16),
    ),
  ];
  let mut seeded = Seeded(0x6d2b_79f5);
  let (mut found, mut climbed, mut asked) = (0, 0, 0);
  for grid in grids {
    let layer = layer_of(grid);
    let step = shortest_step(&layer, window, |c| ways_of(grid, None, c));
    for _ in 0..6 {
      let nav = random_floors(&mut seeded, layer, window);
      for on in nav.grids() {
        assert!(
          (step - f64::from(on.cell_size())).abs() < 1e-4 * step,
          "{grid:?}"
        );
      }
      for _ in 0..8 {
        let mut end = || {
          let floor = Floor::On(seeded.below(2) as u8);
          let Some(on) = nav.grid(floor) else {
            panic!("{floor:?} has a grid")
          };
          (floor, seeded.open_cell(on, window))
        };
        let (from, to) = (end(), end());
        asked += 1;
        let best = cheapest_across_floors(&nav, &layer, step, (from, to), |on, c| {
          ways_of(grid, Some(on), c)
        });
        let ends = ((from.0, from.1.into()), (to.0, to.1.into()));
        match exact().find_floor_path_grid(&nav, ends.0, ends.1) {
          PathResult::Found(path) => {
            found += 1;
            if !crossings(&nav, grid, &path).is_empty() {
              climbed += 1;
            }
            let Some(best) = best else {
              panic!("{grid:?}: a path Dijkstra does not find")
            };
            let cost = floor_path_cost(&nav, &path);
            assert!(
              (cost - best).abs() < 1e-3,
              "{grid:?} {from:?} to {to:?}: {cost} for {best}"
            );
          }
          PathResult::NoPath => assert_eq!(best, None, "{grid:?} {from:?} to {to:?}"),
          other => panic!("{grid:?} {from:?} to {to:?}: {other:?}"),
        }
      }
    }
  }
  assert!(found >= 100, "{found} of {asked} paths found");
  assert!(climbed >= 40, "{climbed} of {found} paths changed floor");
}

#[test]
fn smoothing_never_skips_across_a_stair_link() {
  // Orthogonal cells, every cell of row 4 in sight of every other on either
  // floor: floor 1 a platform from (8, 2) to (13, 6), its stairs at (6, 4)
  // and (7, 4)
  let grid = MapGrid::new(GridShape::Orthogonal, UVec2::splat(16));
  let layer = layer_of(grid);
  let window = CellRect::new(IVec2::ZERO, IVec2::new(16, 9));
  let platform = CellRect::new(IVec2::new(8, 2), IVec2::new(14, 7));
  let stairs = [IVec2::new(6, 4), IVec2::new(7, 4)];
  let nav = two_floors(layer, window, platform, &stairs);
  let Some(ground) = nav.grid(Floor::GROUND) else {
    panic!("a ground floor")
  };
  let (from, to) = (
    (Floor::GROUND, GridCoord::new(1, 4)),
    (Floor::On(1), GridCoord::new(13, 4)),
  );
  // On one grid the two ends are in sight of each other: smoothing that
  // looked across the link would join them straight
  assert!(ground.line_of_sight(from.1, to.1));
  let smoothed = |smoothing: PathSmoothing| {
    let config = PathfinderConfig::default().with_smoothing(smoothing);
    match Pathfinder::with_config(config).find_floor_path_grid(&nav, from, to) {
      PathResult::Found(path) => path,
      other => panic!("{smoothing:?}: {other:?}"),
    }
  };
  let raw = smoothed(PathSmoothing::None);
  let crossed = crossings(&nav, grid, &raw);
  assert_eq!(crossed.len(), 1, "{:?}", raw.waypoints());
  let (at, link) = crossed[0];

  // Line of sight straightens each floor's run to its ends, and keeps the
  // two nodes at the link
  let sighted = smoothed(PathSmoothing::LineOfSight);
  let kept: Vec<(Floor, GridCoord)> = sighted
    .waypoints()
    .iter()
    .map(|node| (node.floor, node.grid_coord))
    .collect();
  assert_eq!(
    kept,
    [
      from,
      (Floor::GROUND, link.cell),
      (Floor::On(1), link.cell),
      to
    ]
  );

  // A spline curves within each run, its nodes on their run's floor, and
  // keeps the two nodes at the link where they are
  let curved = smoothed(PathSmoothing::Spline);
  let nodes = curved.waypoints();
  let Some(change) = nodes.windows(2).position(|p| p[0].floor != p[1].floor) else {
    panic!("the spline changes floor: {nodes:?}")
  };
  assert!(nodes[..=change].iter().all(|n| n.floor == Floor::GROUND));
  assert!(nodes[change + 1..].iter().all(|n| n.floor == Floor::On(1)));
  assert!(nodes.len() > raw.len(), "the spline adds nodes");
  let centre = layer.cell_center_world(link.cell.into());
  for node in [nodes[change], nodes[change + 1]] {
    assert_eq!(node.grid_coord, link.cell);
    assert!(
      (Vec2::new(node.x, node.y) - centre).length() < 1e-3,
      "{node:?}"
    );
  }

  // `Path::smooth_line_of_sight` on one floor's grid straightens that
  // floor's run alone
  let mut on_ground = raw.clone();
  on_ground.smooth_line_of_sight(ground);
  let mut expected = vec![raw.waypoints()[0], raw.waypoints()[at]];
  expected.extend_from_slice(&raw.waypoints()[at + 1..]);
  assert_eq!(on_ground.waypoints(), &expected[..]);
}

#[test]
fn a_path_crosses_a_long_stair_run_once() {
  // A run of stairs leading right into a platform on floor 1: along it
  // every cell is open on both floors at the same cost, so the ground cost
  // alone does not say where to change floor
  let window = CellRect::new(IVec2::ZERO, IVec2::new(22, 12));
  let grids = [
    MapGrid::new(GridShape::Orthogonal, UVec2::splat(16)),
    MapGrid::new(GridShape::Isometric, UVec2::new(64, 32)),
    MapGrid::new(
      GridShape::Hexagonal {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Odd,
        side: 8,
      },
      UVec2::splat(16),
    ),
  ];
  let mut seeded = Seeded(0x1357_9bdf);
  let mut found = 0;
  for grid in grids {
    let layer = layer_of(grid);
    for _ in 0..150 {
      // From 3 to 12 cells long and 1 to 3 wide, with walls about the
      // ground and stray open cells on floor 1, off the run
      let (len, wide) = (3 + seeded.below(10) as i32, 1 + seeded.below(3) as i32);
      let foot = IVec2::new(1 + seeded.below(4) as i32, 1 + seeded.below(6) as i32);
      let run = cells_of(CellRect::new(foot, foot + IVec2::new(len, wide)));
      let platform = CellRect::new(IVec2::new(foot.x + len, 0), window.max);
      let mut ground = NavigationGrid::for_layer(layer, window);
      let mut upper = NavigationGrid::for_layer(layer, window).on_floor(Floor::On(1));
      for cell in cells_of(window) {
        if !platform.contains(cell) && !run.contains(&cell) {
          upper.set(cell.into(), CellState::Blocked);
        }
      }
      for _ in 0..seeded.below(30) {
        let cell = seeded.cell(window);
        if !run.contains(&cell) {
          ground.set(cell.into(), CellState::Blocked);
        }
      }
      for _ in 0..seeded.below(20) {
        let cell = seeded.cell(window);
        if !run.contains(&cell) {
          upper.set(cell.into(), CellState::Walkable);
        }
      }
      let links = run
        .iter()
        .map(|&cell| StairLink {
          cell: cell.into(),
          lower: Floor::GROUND,
          upper: Floor::On(1),
        })
        .collect();
      let nav = FloorNavigation::new(vec![ground, upper], links);
      let below = CellRect::new(IVec2::ZERO, IVec2::new(platform.min.x, window.max.y));
      let ends = [
        (Floor::GROUND, GridCoord::from(seeded.cell(below))),
        (Floor::On(1), GridCoord::from(seeded.cell(platform))),
      ];
      let open =
        |(floor, cell): (Floor, GridCoord)| nav.grid(floor).is_some_and(|g| g.is_walkable(cell));
      if !ends.into_iter().all(open) {
        continue;
      }
      for pathfinder in [Pathfinder::new(), exact()] {
        for (from, to) in [(ends[0], ends[1]), (ends[1], ends[0])] {
          let Some(path) = pathfinder.find_floor_path_grid(&nav, from, to).path() else {
            continue;
          };
          found += 1;
          let crossed = crossings(&nav, grid, &path);
          assert_eq!(
            crossed.len(),
            1,
            "{grid:?} {from:?} to {to:?}: {:?}",
            path
              .waypoints()
              .iter()
              .map(|n| (n.floor, n.grid_coord))
              .collect::<Vec<_>>()
          );
        }
      }
    }
  }
  assert!(found >= 1000, "{found} paths found");
}
