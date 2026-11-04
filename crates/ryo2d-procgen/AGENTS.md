# AGENTS.md - ryo2d-procgen

Procedural generation for Ryo2D (BSP dungeons, cellular caves, noise terrain, random walks, Poisson sampling, WFC, MST corridors), as an optional crate on top of `ryo2d`. The core never depends on it. API, algorithms and presets are covered by the `ryo-procgen` skill.

## Using It

```toml
[dependencies]
ryo2d = { path = "<ryo2d checkout>" }
ryo2d-procgen = { path = "<ryo2d checkout>/crates/ryo2d-procgen" }
```

```rust
use ryo2d::prelude::*;
use ryo2d_procgen::prelude::*;
```

Inside this repository a demo uses `{ path = "../../../crates/ryo2d-procgen" }`. The crate's prelude exports only its own types; `GenerationGrid::to_navigation_grid(cell_size)` converts a result into the core's `NavigationGrid`, and `to_tile_map` / `to_terrain_map` (in `src/tilemap.rs`) into a `TileMap` of a `GridShape` (the latter picks Wang edge tiles, a corner or edge taking the highest-precedence colour of the cells that share it). The generators are square, so the grid is the map's square lattice (`MapGrid::lattice`): orthogonal, isometric and oblique maps take cell `(x, y)` as it is, a staggered map takes each cell at `Lattice::grid_cell` of it (`Placement`: the lattice shifted so the lines start at 0, the cells moved along their lines to start at 0, the map as big as holds them), so cells side by side stay neighbours, and a hexagonal map gives `None` (`procgen_builds_isometric_and_staggered_maps_from_its_square_grid`). Wang positions are worked out in the grid, the lattice.

## Gotchas

- **Random seed by default** - without `with_seed()`, `ProcgenBuilder::build()` picks a random seed. Read it back from `GenerationResult.seed` to reproduce a run.
- **Check WFC results** - `GenerationResult::wfc_result()` reports the last WFC step's `WfcResult` (`None` without a WFC step). On `Contradiction`/`IterationLimit` the grid is only partly collapsed, so regenerate with another seed. The WFC step also replaces the grid from earlier steps.
- **No notifications** - generation is a plain function call that returns its result; the never-written event types were deleted. A game that wants a message defines its own `#[derive(Message)]` type and writes it.
