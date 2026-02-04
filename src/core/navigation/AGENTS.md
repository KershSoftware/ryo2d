# AGENTS.md - Navigation Module

## Overview

This module provides grid-based A* pathfinding for game entities. It's designed for AI agents, enemies, NPCs, and any entities that need to navigate around obstacles.

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Public exports and module documentation |
| `grid.rs` | `NavigationGrid`, `CellState`, `GridCoord`, `NavigationBuilder` |
| `pathfinder.rs` | A* algorithm, `Pathfinder`, `PathfinderConfig`, `PathResult` |
| `path.rs` | `Path`, `PathNode`, `PathSmoothing` |
| `agent.rs` | `NavAgent`, `NavTarget`, `NavAgentState` components |
| `events.rs` | `PathCompleted`, `PathFailed`, `PathInvalidated`, `PathStarted` |
| `steering.rs` | Steering behaviors: `separation`, `seek`, `flee`, `arrive`, `cohesion`, `alignment` |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Game Code                                │
│  - Spawn entities with NavAgent                             │
│  - Add NavTarget to request pathfinding                     │
│  - Listen for PathCompleted/PathFailed events               │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                  Navigation System                          │
│  - Detects NavTarget, triggers pathfinding                  │
│  - Updates NavAgent state and position                      │
│  - Emits navigation events                                  │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                     Pathfinder                              │
│  - A* algorithm with time-slicing                           │
│  - Octile distance heuristic                                │
│  - Tie-breaking for straighter paths                        │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                   NavigationGrid                            │
│  - Cell-based walkability map                               │
│  - Generation counter for path invalidation                 │
│  - Neighbor queries with corner-cutting prevention          │
└─────────────────────────────────────────────────────────────┘
```

## Key Concepts

### NavigationGrid

The grid divides the world into cells and tracks walkability:

- **CellState::Walkable** - Normal traversal (cost 1.0)
- **CellState::Blocked** - Impassable
- **CellState::Difficult(cost)** - Passable but slower (cost > 1.0)

Grid operations:
- `world_to_grid()` / `grid_to_world()` - Coordinate conversion
- `is_walkable()` - Check cell passability
- `neighbors()` - Get 8-directional neighbors with costs
- `line_of_sight()` - Bresenham line test for smoothing
- `rebuild_from_world()` - Auto-populate from Collider components

### Pathfinder

A* implementation with:

- **Octile heuristic** - Accurate for 8-directional movement
- **Tie-breaking** - Cross-product bias for straighter paths
- **Time-slicing** - Process N nodes/frame for many agents
- **Generation tracking** - Detect stale paths when grid changes

### Path Smoothing

Three smoothing modes:

| Mode | Description | Use Case |
|------|-------------|----------|
| `None` | Raw A* waypoints | Precise grid movement |
| `LineOfSight` | Skip redundant waypoints | Most games |
| `Spline` | Catmull-Rom interpolation | Smooth curves |

### NavAgent State Machine

`NavAgent` uses the generic `StateMachine<NavAgentState>` framework internally, providing validated transitions and state tracking:

```
┌───────┐          ┌───────────┐
│ Idle  │─────────>│ Computing │
└───┬───┘          └─────┬─────┘
    ^                    │
    │         ┌─────────>v──────────┐
    │         │      ┌───────┐      │
    │         │      │Moving │      │
    │         │      └───┬───┘      │
    │         │          │          │
    │    ┌────┴───┐  ┌───v────┐     │
    └────│ Failed │  │Arrived │     │
         └────────┘  └────────┘     │
               ^                    │
               └────────────────────┘
```

**State Descriptions:**

| State | Description | Valid Transitions |
|-------|-------------|-------------------|
| `Idle` | No target, stationary | → Computing, Moving |
| `Computing` | Path being calculated | → Moving, Failed |
| `Moving` | Following computed path | → Arrived, Failed |
| `Arrived` | Reached destination | → Idle, Computing |
| `Failed` | Path not found or blocked | → Idle, Computing |

**State Machine Features:**

```rust
// Check current state
if agent.state() == NavAgentState::Moving {
    // Currently navigating
}

// Check previous state
if let Some(prev) = agent.previous_state() {
    println!("Was in {:?}", prev);
}

// One-frame transition flags
if agent.just_entered_state() {
    // First frame in new state - play sound, emit event, etc.
}

if agent.just_exited_state() {
    // Just left previous state
}

// Time tracking
let time = agent.time_in_state();  // Seconds in current state

// Transition validation
if agent.can_transition_to(NavAgentState::Moving) {
    agent.set_path(path);
}

// Frame update (clears flags, advances time)
agent.update(dt);
```

**State Helper Methods:**

```rust
// Check if actively navigating
assert!(NavAgentState::Computing.is_active());  // true
assert!(NavAgentState::Moving.is_active());     // true
assert!(!NavAgentState::Idle.is_active());      // false

// Check if navigation ended
assert!(NavAgentState::Arrived.is_finished());  // true
assert!(NavAgentState::Failed.is_finished());   // true
assert!(!NavAgentState::Moving.is_finished());  // false
```

### Steering Behaviors

The `steering` module provides standalone functions for agent movement that can be combined with pathfinding or used independently:

| Function | Purpose |
|----------|---------|
| `separation(pos, radius, neighbors, strength)` | Push away from nearby agents |
| `seek(pos, target, max_speed)` | Move toward a target |
| `flee(pos, threat, max_speed)` | Move away from a threat |
| `arrive(pos, target, max_speed, slow_radius)` | Seek but slow down near target |
| `cohesion(pos, neighbors, max_speed)` | Move toward group center |
| `alignment(neighbors_with_vel, max_speed)` | Match velocity with group |

All functions return a `Vec2` force/velocity that games apply to movement.

## Usage Patterns

### Basic Setup

```rust
// In scene struct
struct GameScene {
    nav_grid: NavigationGrid,
    pathfinder: Pathfinder,
}

// In on_enter
self.nav_grid = NavigationBuilder::new(800.0, 600.0)
    .with_cell_size(16.0)
    .build_from_world(&ctx.world);

self.pathfinder = Pathfinder::new();
```

### Spawning Navigating Entities

```rust
ctx.world.spawn((
    Transform::new(100.0, 100.0),
    Renderable::sprite(enemy_sprite),
    NavAgent::new(80.0)  // 80 px/second
        .with_smoothing(PathSmoothing::LineOfSight),
));
```

### Setting Destinations

```rust
// Fixed position
ctx.world.add_component(entity, NavTarget::position(500.0, 400.0));

// Follow another entity
ctx.world.add_component(entity, NavTarget::follow(player_entity));
```

### Handling Events

```rust
fn update(&mut self, ctx: &mut SceneContext) {
    for event in ctx.events.read::<PathCompleted>() {
        println!("Entity {:?} arrived at ({}, {})",
            event.entity, event.x, event.y);
    }

    for event in ctx.events.read::<PathFailed>() {
        match event.reason {
            PathFailureReason::NoPath => {
                // Entity is stuck, maybe teleport or destroy
            }
            PathFailureReason::TargetLost => {
                // Target entity was despawned
            }
            _ => {}
        }
    }
}
```

### Time-Sliced Pathfinding

For 100+ agents, use time-slicing to avoid frame drops:

```rust
let config = PathfinderConfig::default()
    .with_time_slicing(50);  // 50 nodes per frame max

let mut pathfinder = Pathfinder::with_config(config);

// Pathfinding may take multiple frames
let (request_id, initial) = pathfinder.start_pathfinding(&grid, x1, y1, x2, y2);

// Each frame
if let Some(id) = pending_request {
    match pathfinder.continue_pathfinding(&grid, id) {
        PathResult::Found(path) => {
            agent.set_path(path);
            pending_request = None;
        }
        PathResult::InProgress => { /* wait */ }
        _ => { pending_request = None; }
    }
}
```

### Dynamic Obstacles

When obstacles move, rebuild the grid:

```rust
// Detect when grid needs update
if self.nav_grid.needs_rebuild(&ctx.world) {
    self.nav_grid.rebuild_from_world(&ctx.world);
    // Generation increments, stale paths auto-detected
}
```

### Agent Avoidance with Steering

Combine pathfinding with separation steering for agents that avoid each other:

```rust
fn move_agent_with_avoidance(&self, world: &mut World, entity: Entity, dt: f32) {
    // Get agent info
    let (pos, radius, waypoint, speed) = /* ... get from NavAgent ... */;

    // Collect nearby agents
    let neighbors: Vec<(Vec2, f32)> = self.agents.iter()
        .filter(|&&e| e != entity)
        .filter_map(|&e| {
            let t = world.get::<Transform>(e)?;
            let a = world.get::<NavAgent>(e)?;
            Some((Vec2::new(t.x, t.y), a.radius))
        })
        .filter(|(p, _)| pos.distance(*p) < 50.0)  // Only nearby
        .collect();

    // Calculate path direction
    let path_dir = (waypoint - pos).normalize_or_zero();

    // Calculate avoidance force
    let avoid = steering::separation(pos, radius, &neighbors, 100.0);

    // Blend path following with avoidance
    let velocity = (path_dir * speed + avoid).clamp_length_max(speed);

    // Apply movement
    if let Some(mut t) = world.get_mut::<Transform>(entity) {
        t.x += velocity.x * dt;
        t.y += velocity.y * dt;
    }
}
```

### Flocking Behavior

Use steering functions together for classic boids:

```rust
fn update_boid(&self, pos: Vec2, vel: Vec2, neighbors: &[(Vec2, f32, Vec2)]) -> Vec2 {
    let neighbor_pos: Vec<(Vec2, f32)> = neighbors.iter()
        .map(|(p, r, _)| (*p, *r)).collect();
    let neighbor_vel: Vec<(Vec2, Vec2)> = neighbors.iter()
        .map(|(p, _, v)| (*p, *v)).collect();

    let sep = steering::separation(pos, 8.0, &neighbor_pos, 50.0);
    let coh = steering::cohesion(pos, &neighbor_pos, 30.0);
    let ali = steering::alignment(&neighbor_vel, 30.0);

    // Weight and combine
    sep * 1.5 + coh * 1.0 + ali * 1.0
}
```

## Performance Notes

- **Cell size trade-off**: Smaller cells = more precision but more memory and slower pathfinding
- **Time-slicing**: Essential for 50+ simultaneous pathfinding requests
- **Path caching**: Paths are stored on NavAgent, not recomputed each frame
- **Generation tracking**: O(1) staleness check, no full path validation
- **Neighbor caching**: Grid neighbors are computed on-demand, not stored

## Integration Points

The navigation system is standalone and doesn't auto-integrate with the engine loop. Game code must:

1. Create and maintain `NavigationGrid` and `Pathfinder` instances
2. Call pathfinding methods when `NavTarget` is added
3. Update entity `Transform` based on `NavAgent.current_path`
4. Emit events via `ctx.events.emit()`

This manual approach gives games full control over when pathfinding runs and how movement is applied.

## Design Decisions

### Why Manual Integration?

The navigation system doesn't auto-run in the engine loop because:
- Games have different movement models (physics, direct transform, etc.)
- Pathfinding timing varies (every frame, on-demand, pooled)
- Some games want custom path following (steering, flocking)

### Why Grid-Based?

Grid-based navigation is simpler and more predictable than navmesh:
- Easy to understand and debug
- Works well for tile-based and action games
- Lower memory overhead for small-medium worlds
- Easier dynamic obstacle handling

### Why A* with Octile?

- A* is optimal for weighted graphs with admissible heuristics
- Octile distance is exact for 8-directional movement
- Tie-breaking produces visually pleasing paths
- Time-slicing enables scalability to 100+ agents
