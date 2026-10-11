# AGENTS.md - Render Pipeline System

## Status

**Migration in progress: games still render through the old paths.** `Engine` owns a `RenderPipeline::default_2d()` (the twelve built-in passes in `passes/`), and `present_frame` runs it through `WgpuRenderer::render_frame` only when the crate-internal `Engine::legacy_render` is false. It defaults to true, so frames still come from the hard-coded paths in `graphics/renderer.rs`. `testing/render_comparison.rs` proves both draw byte-identical frames. The old paths, the switch and those tests go once the engine switches over.

## Ordering Rules

- Order is the list order. There are no dependencies or sorting: `push` adds at the end, `insert_before` / `insert_after` place a pass relative to a named one.
- Pass ids are unique. A duplicate id or an unknown anchor id returns `RenderPipelineError`; nothing panics.
- Disabled passes keep their place and are skipped at run time. An enabled pass is also skipped when its `should_run(&FrameInfo)` returns false (`should_run` is only asked for enabled passes).
- `remove` hands the boxed pass back, or `None` if the id is unknown.
