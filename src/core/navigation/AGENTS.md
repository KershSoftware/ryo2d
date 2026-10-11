# AGENTS.md - Navigation Module

## Integration Points

Nothing in this module runs in the engine loop. No engine system reads `NavAgent`/`NavTarget`, moves entities, or emits navigation events. Game code must:

1. Own the `NavigationGrid` and `Pathfinder` (typically scene fields)
2. Call pathfinding when a `NavTarget` is added and drive `NavAgent` state (`start_computing`, `set_path`, `fail`, `arrive`)
3. Move the entity's `Transform` along `NavAgent.current_path`
4. Emit `PathStarted`/`PathCompleted`/`PathFailed`/`PathInvalidated` via `ctx.events.emit()`

Gotchas:
- **Time-slicing** - with `PathfinderConfig::with_time_slicing(n)`, the game keeps the request ID from `start_pathfinding` and polls `continue_pathfinding` each frame until it stops returning `InProgress`. `NavAgent`'s request field is `pub(crate)`, so store the ID yourself.
- **Stale paths** - grid edits (`set`, `clear`, `rebuild_from_world`) bump `grid.generation()`, but nothing checks it. Compare with `path.is_stale(grid.generation())` and re-path yourself.
- `rebuild_from_world` only blocks non-trigger `Collider`s that have a `Transform`.

## Design Decisions

### Why Manual Integration?

- Games have different movement models (physics, direct transform, steering)
- Pathfinding timing varies (every frame, on-demand, pooled)
- Some games want custom path following (steering, flocking)

### Why Grid-Based?

Simpler and more predictable than navmesh: easy to debug, suits tile-based and action games, low memory for small-medium worlds, easy dynamic obstacle handling.

### Why A* with Octile?

- A* is optimal with an admissible heuristic; octile is exact for 8-directional movement
- Tie-breaking produces straighter, visually pleasing paths
- Time-slicing scales to 100+ agents
