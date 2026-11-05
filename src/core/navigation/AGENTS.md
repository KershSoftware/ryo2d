# AGENTS.md - Navigation Module

## Files

| File | What |
|------|------|
| `grid.rs` | `NavigationGrid` (square world cells or a layer's cells), `GridCoord`, `CellState`, `NavigationBuilder`; neighbours, line of sight, conversions and the blocking (`block_tiles`, `block_rect`, `rebuild_from_world`) |
| `layer.rs` | A layer grid's geometry (`LayerCells`: ways, step costs, centre offsets, candidate cells), the window cap, `squared` (right angles made exact) and the convex-polygon helpers (`overlap`, `bounds`, `mapped_bounds`) |
| `pathfinder.rs` | A* with time-slicing; the heuristic and tie-break by the grid |
| `path.rs` | `Path`, `PathNode`, smoothing (line of sight, spline) |
| `agent.rs` | `NavAgent` / `NavTarget` state machines |
| `steering.rs` | Steering behaviours on `Vec2`s |

## Integration Points

Nothing in this module runs in the engine loop. No engine system reads `NavAgent`/`NavTarget` or moves entities. Game code must:

1. Own the `NavigationGrid` and `Pathfinder` (typically scene fields)
2. Call pathfinding when a `NavTarget` is added and drive `NavAgent` state (`start_computing`, `set_path`, `fail`, `arrive`)
3. Move the entity's `Transform` along `NavAgent.current_path`
4. Define and write its own messages if other logic must react to arrival or failure (`NavAgent::state()` already reports it)

Gotchas:
- **Time-slicing** - with `PathfinderConfig::with_time_slicing(n)`, the game keeps the request ID from `start_pathfinding` and polls `continue_pathfinding` each frame until it stops returning `InProgress`. `NavAgent`'s request field is `pub(crate)`, so store the ID yourself.
- **Stale paths** - grid edits (`set`, `clear`, `rebuild_from_world`) bump `grid.generation()`, but nothing checks it. Compare with `path.is_stale(grid.generation())` and re-path yourself.
- `rebuild_from_world` blocks each non-trigger `Collider`'s bounds, centred on its entity's `GlobalTransform` plus the offset and unscaled, as physics simulates it (on a layer grid, every cell whose outline meets those bounds, edges included). Disabled entities are skipped. Tile grids (`TileColliders`) block through `block_tiles(tiles, to_world)`, `to_world` their entity's whole `GlobalTransform::affine`, so a moved, turned or scaled map blocks where it is: their solid and slope cells, and their shape cells whose solid shape covers the cell's centre (one-way cells and one-way shapes stay walkable); the outline colliders they build (`TilePart`) are skipped, as they would block the grid's whole bounding box. `block_tiles` reads only the cells of the tile grid's chunks under the navigation grid (a cell or two wider each way), so an unbounded grid's negative cells block too; it costs a look at every chunk of the tile grid to filter them (`TileColliders::cells_in`, O(chunks)) plus the cells under the navigation grid. It takes one of three paths:
  - **Same cells:** on a layer grid whose cells the tiles' are (the tiles' hexagons or lattice equal to the layer grid's, and `to_world` within 1e-4 of the layer's transform in every element), each blocking cell blocks itself; a staggered map's tiles, held by lattice cell, go to their grid cells through `Lattice::grid_cell` (`block_tiles_blocks_a_layer_s_own_cells_cell_for_cell`, `navigation_reads_a_staggered_map_s_tiles`; `a_turned_map_s_navigation_follows_its_transform` pins the tolerance both ways).
  - **Upright rectangles:** on a square world grid, tiles that the transform leaves upright rectangles of a positive size (plain tiles, or an upright lattice's) block the cells they cover, an edge they only touch blocking nothing, worked out in cell units in f64 so a far tile blocks exactly its cells (`a_far_tile_blocks_exactly_one_cell`).
  - **Overlap:** any other tile (a diamond, a skewed or turned cell, a hexagon, or tiles on another layer's cells) blocks every cell whose outline shares some area with its outline in the world (`TileColliders::cell_polygon` through `to_world`): a separating-axis test in f64 on convex polygons, strict, so neighbours that only touch stay open (`a_square_grid_reads_isometric_and_hex_tiles_by_overlap`). Two guards keep touching from reading as sharing: an overlap no deeper on some axis than a millionth of the longer extent on it (`SPILL`) is the rounding of points through a turned transform where edges meet, not area (`tiles_under_a_coarser_layer_on_a_turned_map_block_the_cells_they_share`, `overlap_reads_a_rounding_spill_as_touching`); and a transform whose matrix columns lie within a millionth of an axis is laid on it first (`squared`, in `block_tiles` and `LayerCells::new`), because f32's `sin_cos` of a right angle leaves about 4e-8 where 0 belongs, which far out spills past any tolerance (`a_map_turned_by_a_right_angle_blocks_one_cell_per_tile`). Tiles placed by a transform scaled to nothing or not finite block nothing. The tile cells read are those whose boxes may meet the grid's world box, and the cells tested those whose boxes may meet each tile's.
- **Layer grids** (`for_layer(LayerGrid, window)`) cover a window of a tile layer's own cells, `GridCoord`s being its signed cells; outside the window reads `Blocked`, and `line_of_sight` there is false. The window's states are a dense array (8 bytes a cell), so a window over 4,194,304 cells keeps the middle 2048 of each longer side, logged once a run, as a game may take a grid again after every edit (`a_window_past_four_million_cells_shrinks_and_warns_once`). A layer scaled to nothing or with a transform not finite has no world steps: each step costs 1, the heuristic and tie-break are 0, and every point reads cell (0, 0) (`a_layer_scaled_to_nothing_or_not_finite_has_no_world_steps`). Conversions go through the layer's whole transform (`LayerGrid::cell_at_world`, `cell_center_world`; `grid_to_world_corner` is the box round the world outline's top-left). A square world grid (`new`, `with_origin`, `NavigationBuilder`) keeps its own code: its neighbours, octile costs, integer tie-break and conversions are what they always were.

## Design Decisions

### Why Manual Integration?

- Games have different movement models (physics, direct transform, steering)
- Pathfinding timing varies (every frame, on-demand, pooled)
- Some games want custom path following (steering, flocking)

### Why Grid-Based?

Simpler and more predictable than navmesh: easy to debug, suits tile-based and action games, low memory for small-medium worlds, easy dynamic obstacle handling.

### Why A* with Octile?

- A* is optimal with an admissible heuristic; octile is exact for 8-directional movement
- Tie-breaking produces straighter, visually pleasing paths, at the cost of a path sometimes a little dearer than the cheapest (the tests that compare costs with Dijkstra turn it off)
- Time-slicing scales to 100+ agents

### Layer grids: steps by world distance

A layer grid's cells neighbour as its grid's do: six on hex (`MapGrid::neighbours`), and on every other grid the eight of its square lattice (`Lattice::lattice_cell`, then `grid_cell` back on a staggered map), a diagonal needing both lattice cardinals beside it walkable. A step costs the world distance between the two cells' centres over the grid's shortest step (`cell_size()`), times the entered cell's cost, so on 116 x 55 isometric cells a step down the lattice's diagonal (55 px) costs 1 and a side step 1.17: the cheapest path is the shortest the player sees, not the fewest steps. The centres are affine in lattice cells, and on hex in a cell's position and line (an odd line's shift added), so each way's cost is measured once (`LayerCells::new`) and the offset between any two centres is worked out exactly from their cells (`steps_between`), however far out they are. The heuristic is that offset's length, which no path undercuts; the tie-break is the cross product of the offsets from the goal, the square grid's integer cross product measured in world steps (equal to it on an orthogonal layer, `a_layer_grid_breaks_ties_as_the_square_grid_does`). Costs are held to a Dijkstra over each grid's own neighbours, from `cell_center_world` distances: hex on every variant, staggered and skewed oblique lattices with the corner rule (`hex_paths_cross_negative_rows_and_columns_on_every_variant`, `staggered_and_skewed_layer_grids_find_the_cheapest_path`). Hex lines and lattice walks give line of sight, so smoothing keeps each waypoint pair in sight on the grid's own cells.
