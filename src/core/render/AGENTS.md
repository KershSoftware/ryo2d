# AGENTS.md - Render Pipeline System

## Status

**`RenderPipeline` is not wired into the engine.** `engine.rs` never builds or executes it. The pipeline is only re-exported from `lib.rs`, and actual frame rendering is hard-coded in `engine.rs` / `graphics/renderer.rs`. Changing passes here does not change what the game renders.

Only four passes exist (`passes/`): `ClearPass`, `ParallaxPass`, `UploadPass`, `LightingPass`. The other `pass_ids` (`cpu_sprite`, `gpu_sprite`, `trail`, `ui`, `effects`, `present`) are ID constants with no implementation. There is also no `RenderPipeline::default_2d()`, despite the `mod.rs` rustdoc.

## Ordering Gotchas

- Passes declare dependencies, not explicit indices. Order is a topological sort (Kahn's) with deterministic ID-based tie-breaking. Don't rely on tie order: declare every real dependency.
- **Missing dependencies are silently ignored.** With only the built-in passes, `upload` (depends on missing `cpu_sprite`) and `lighting` (depends on missing `gpu_sprite`) become roots, giving the order `clear → parallax → lighting → upload`.
- `add_pass()` **panics** on a duplicate ID. A dependency **cycle panics** when the order is next computed (`execute()`/`pass_ids()`), not at `add_pass()`.
- Disabled passes still take part in ordering. They are only skipped at execute time (as are passes whose `should_execute()` returns false).
