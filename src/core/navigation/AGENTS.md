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

### NavAgent States

```
Idle → Computing → Moving → Arrived
                       ↓
                    Failed
```

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
