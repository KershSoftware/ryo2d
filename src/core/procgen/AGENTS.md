# AGENTS.md - Procedural Generation Module

API, algorithms, presets and navigation conversion are covered by the `ryo-procgen` skill.

## Gotchas

- **Random seed by default** - without `with_seed()`, `ProcgenBuilder::build()` picks a random seed. Read it back from `GenerationResult.seed` to reproduce a run.
- **WFC failures are silent** - the builder's `wfc()` step discards the `WfcResult` (`builder.rs:164`), and `wfc::generate` is private (`mod wfc` isn't `pub`), so game code can't detect a `Contradiction`/`IterationLimit`. On failure you get a partially collapsed grid. The WFC step also replaces the grid from earlier steps.
- **Events are not emitted** - `GenerationStarted`, `GenerationCompleted`, `GenerationFailed`, `RegionCreated` and `RegionsConnected` are only types. Procgen has no `Events` access, so game code emits them if needed.
