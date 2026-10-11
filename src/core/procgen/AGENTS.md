# AGENTS.md - Procedural Generation Module

API, algorithms, presets and navigation conversion are covered by the `ryo-procgen` skill.

## Gotchas

- **Random seed by default** - without `with_seed()`, `ProcgenBuilder::build()` picks a random seed. Read it back from `GenerationResult.seed` to reproduce a run.
- **Check WFC results** - `GenerationResult::wfc_result()` reports the last WFC step's `WfcResult` (`None` without a WFC step). On `Contradiction`/`IterationLimit` the grid is only partly collapsed, so regenerate with another seed. The WFC step also replaces the grid from earlier steps.
- **Events are not emitted** - `GenerationStarted`, `GenerationCompleted`, `GenerationFailed`, `RegionCreated` and `RegionsConnected` are only types. Procgen has no `Events` access, so game code emits them if needed.
