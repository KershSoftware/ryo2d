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

  let entity = world.spawn((
    Transform::new(100.0, 100.0),
    NavAgent::new(80.0)
      .with_arrival_threshold(8.0)
      .with_smoothing(PathSmoothing::LineOfSight),
  ));

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

  let player = world.spawn((Transform::new(500.0, 500.0),));

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
      Transform::new(100.0 + i as f32 * 10.0, 100.0 + i as f32 * 5.0),
      Collider::box_collider(16.0, 16.0),
    ));
  }

  // Measure rebuild time (just verify it completes)
  let grid = NavigationBuilder::new(800.0, 600.0)
    .with_cell_size(16.0)
    .build_from_world(&world);

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
