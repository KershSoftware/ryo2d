# AGENTS.md - Render Pipeline System

## Status

The engine draws every frame through its `RenderPipeline::default_2d()` (the twelve built-in passes in `passes/`). `Engine::present_frame` builds a `FrameScene` and `WgpuRenderer::render_frame` runs the pipeline on **one command encoder with one submit**. The old hard-coded render paths are gone.

Games reach the pipeline through `SceneContext::render_pipeline`: toggle built-in passes (`set_enabled`) or insert their own `RenderPass`. The pipeline belongs to the engine, so a scene's changes persist across scene transitions.

## Default Order (`default_2d`)

1. `upload` - CPU pixel buffer to GPU
2. `clear` - clear the game texture
3. `raycast` - GPU raycaster
4. `gpu_sprites` - `render_gpu()` sprite batches (incl. parallax)
5. `cpu_blend` - CPU-rendered content alpha-blended on top
6. `trails` - lit trails and lines
7. `lighting` - 2D lighting
8. `unlit_trails` - trails and lines with `with_lighting(false)`
9. `post_process` - effects chain
10. `ui` - upload the UI layer
11. `composite` - letterboxed present to the window or offscreen target, UI blended last
12. `capture` - frame capture readback copy

Ids are constants in `pass_ids`.

## GPU World Path (crate-internal, `Engine::use_gpu_world`)

`RenderPipeline::world_2d()` replaces the CPU draw: `clear`, `raycast`, `world`, `lighting`, `unlit_world`, `transition`, `post_process`, `ui`, `composite`, `capture`. `present_frame` fills one z-sorted `DrawList` with `render_gpu` sprites, trail groups and (`entities.rs`) the entities: shadows, sprites, `Tiled` tiles and `CompositeTile` parts as `DrawKind::Entity`, at the render-cache sort key (composites at their `z_order`). Debug overlays become unlit `Line2D`s at `i32::MAX` (`config::debug::overlay_lines`); the fade is the `transition` pass (colour and opacity from `FrameScene::overlay`). Entity sprites are atlas textures keyed by `SpriteHandle::serial()` plus `generation()` (bumped by hot reload).

- Unrotated, whole-number-scale, opaque content matches the CPU renderer byte for byte (`testing/gpu_world_comparison.rs`). Allowed to differ: translucent overlaps (GPU blends in linear space, the CPU in sRGB bytes, also for the fade), rotation edges and fractional scales.
- Placement mirrors the CPU: top-left is the screen position truncated to a pixel, size `ceil(w * scale)` by `ceil(h * scale)`; rotated sprites turn about the box centre with integer halves.

## Ordering Rules

- Order is the list order. There are no dependencies or sorting: `push` adds at the end, `insert_before` / `insert_after` place a pass relative to a named one.
- Pass ids are unique. A duplicate id or an unknown anchor id returns `RenderPipelineError`; nothing panics.
- `enabled` and `should_run` are different: `set_enabled` is the caller's switch (a disabled pass keeps its place and is skipped), while `should_run(&FrameInfo)` is the pass's own per-frame condition. `should_run` is only asked for enabled passes.
- `remove` hands the boxed pass back, or `None` if the id is unknown.

## Design Decisions

- **Thin passes.** GPU pipelines, bind groups and buffers live in the subsystem renderers (lighting, trails, post-process, sprites, raycast, compositor). A pass only calls into them with the `RenderContext`.
- **Latest-image tracking.** `RenderContext` tracks which texture holds the latest image (`FrameImage`: the game texture, the lit texture, or a post-process output). Passes that draw on "the frame" use `current_view()` / `encoder_and_current_view()`, so a game pass inserted after `post_process` draws on the post-processed image.
- **The UI is composited only if the `ui` pass uploaded it this frame.** The UI texture is persistent, so without that flag a disabled or skipped `ui` pass would blend the previous frame's UI.
- **A capture is read back only if the `capture` pass ran.** Otherwise `render_frame` reports `CaptureError::Readback` rather than returning a stale image.
- **The lit texture is persistent** and every game-size texture follows `resize_buffer` (ResizableStretch).
- **`FrameScene` is built in `present_frame`:** the per-frame data (sprite batches, trails, lights, effects, UI) that passes read.

## Gotchas

- While raycasting, the `raycast` pass replaces the 2D scene passes (`gpu_sprites`, `cpu_blend`, `trails`, `lighting`, `unlit_trails`) and clears to black, so a raycast scene with no wall textures renders black.
- Disabling `post_process` removes every effect, including flashes and tints.
- Custom shader and effects APIs for game passes are not there yet: a game pass records raw wgpu work on `ctx.encoder_and_current_view()`.
