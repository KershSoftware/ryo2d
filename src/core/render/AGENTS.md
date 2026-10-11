# AGENTS.md - Render Pipeline System

## Status

**`RenderPipeline` is not wired into the engine yet.** `engine.rs` never builds or executes it, and actual frame rendering is still hard-coded in `engine.rs` / `graphics/renderer.rs`. Changing passes here does not change what the game renders.

The module holds the core API only: `RenderPipeline` (ordered list), `RenderPass` (with `should_run`), `FrameInfo`, `RenderContext`, `RenderPipelineError` and the `pass_ids` constants. There are no built-in passes and no `RenderPipeline::default_2d()` yet.

## Ordering Rules

- Order is the list order. There are no dependencies or sorting: `push` adds at the end, `insert_before` / `insert_after` place a pass relative to a named one.
- Pass ids are unique. A duplicate id or an unknown anchor id returns `RenderPipelineError`; nothing panics.
- Disabled passes keep their place and are skipped at run time. An enabled pass is also skipped when its `should_run(&FrameInfo)` returns false (`should_run` is only asked for enabled passes).
- `remove` hands the boxed pass back, or `None` if the id is unknown.
