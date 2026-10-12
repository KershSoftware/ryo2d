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

Inside this repository a demo uses `{ path = "../../../crates/ryo2d-procgen" }`. The crate's prelude exports only its own types; `GenerationGrid::to_navigation_grid(cell_size)` converts a result into the core's `NavigationGrid`.

## Gotchas

- **Random seed by default** - without `with_seed()`, `ProcgenBuilder::build()` picks a random seed. Read it back from `GenerationResult.seed` to reproduce a run.
- **Check WFC results** - `GenerationResult::wfc_result()` reports the last WFC step's `WfcResult` (`None` without a WFC step). On `Contradiction`/`IterationLimit` the grid is only partly collapsed, so regenerate with another seed. The WFC step also replaces the grid from earlier steps.
- **No notifications** - generation is a plain function call that returns its result; the never-written event types were deleted. A game that wants a message defines its own `#[derive(Message)]` type and writes it.
