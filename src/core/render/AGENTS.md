# AGENTS.md - Render Pipeline System

## Status

The engine draws every frame through its `RenderPipeline::default_2d()` (the twelve built-in passes in `passes/`): `present_frame` runs it with `WgpuRenderer::render_frame`. The hard-coded render paths are gone. A requested capture is read back only if the `capture` pass drew; otherwise `render_frame` reports `CaptureError::Readback`.

## Ordering Rules

- Order is the list order. There are no dependencies or sorting: `push` adds at the end, `insert_before` / `insert_after` place a pass relative to a named one.
- Pass ids are unique. A duplicate id or an unknown anchor id returns `RenderPipelineError`; nothing panics.
- Disabled passes keep their place and are skipped at run time. An enabled pass is also skipped when its `should_run(&FrameInfo)` returns false (`should_run` is only asked for enabled passes).
- `remove` hands the boxed pass back, or `None` if the id is unknown.
