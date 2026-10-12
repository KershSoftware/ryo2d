# AGENTS.md - Procedural Generation Module

API, algorithms, presets and navigation conversion are covered by the `ryo-procgen` skill.

## Gotchas

- **Random seed by default** - without `with_seed()`, `ProcgenBuilder::build()` picks a random seed. Read it back from `GenerationResult.seed` to reproduce a run.
- **Check WFC results** - `GenerationResult::wfc_result()` reports the last WFC step's `WfcResult` (`None` without a WFC step). On `Contradiction`/`IterationLimit` the grid is only partly collapsed, so regenerate with another seed. The WFC step also replaces the grid from earlier steps.
- **No notifications** - generation is a plain function call that returns its result; the never-written event types were deleted. A game that wants a message defines its own `#[derive(Message)]` type and writes it.
