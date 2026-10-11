# AGENTS.md - Render Pipeline System

## Status

The engine draws every frame through its `RenderPipeline::default_2d()` (the ten built-in passes in `passes/`). `Engine::present_frame` builds a `FrameScene` and `WgpuRenderer::render_frame` runs the pipeline on **one command encoder with one submit**. The old hard-coded render paths are gone.

Games reach the pipeline through `SceneContext::render_pipeline`: toggle built-in passes (`set_enabled`) or insert their own `RenderPass`. The pipeline belongs to the engine, so a scene's changes persist across scene transitions.

## Default Order (`default_2d`)

1. `clear` - clear the game texture
2. `raycast` - GPU raycaster
3. `world` - entities, `render_gpu()` sprites (incl. parallax, particles) and lit trails and lines, one z-sorted list
4. `lighting` - 2D lighting
5. `unlit_world` - trails and lines with `with_lighting(false)`, by z (runs every non-raycast frame, draws only if there are any)
6. `transition` - scene transition fade over the whole frame (not the UI)
7. `post_process` - effects chain
8. `ui` - upload the UI layer
9. `composite` - letterboxed present to the window or offscreen target, UI blended last
10. `capture` - frame capture readback copy

Ids are constants in `pass_ids`.

## The World

`present_frame` fills one z-sorted `DrawList` with `render_gpu` sprites (`DrawKind::Sprite`), trail groups (`DrawKind::Trail`) and (`entities.rs`) the entities: shadows, sprites, `Tiled` tiles and `CompositeTile` parts as `DrawKind::Entity`, at their `Renderable.z_order` (composites at their own), submitted in render-cache order, which carries the `YSort` order within a layer. Items sort by (z, kind, submission): `z_order` picks the layer, `YSort` orders entities by y within it, and sprites, particles and trails at the same z draw after the layer's entities. Skeleton debug lines are `Line2D`s at `i32::MAX - 1`, on top of game content. Debug overlays are unlit `Line2D`s at `i32::MAX` (`config::debug::overlay_lines`); the fade is the `transition` pass (colour and opacity from `FrameScene::overlay`), which copies the frame and mixes it toward the colour in sRGB, as the CPU blended bytes; blending in linear light held bright areas back and then snapped at the end. Entity sprites are atlas textures keyed by `SpriteHandle::serial()` plus `generation()` (bumped by hot reload).

- `FrameInfo::world` means the draw list has lit items. `unlit_world` has no flag of its own and returns early without unlit items.
- Trail vertices are uploaded once per frame by the engine (`TrailRenderer::prepare`) before the pipeline runs; the passes only draw. Sprite instances are uploaded by the `world` pass.
- `DebugStats::draw_calls` counts the draws of `world` and `unlit_world`.
- `FrameInfo` carries `world` (the draw list has lit items) and `transition`; the old `gpu_sprites` / `trails` flags and the `upload`, `gpu_sprites`, `cpu_blend`, `trails` and `unlit_trails` passes are gone.
- Placement: top-left is the screen position truncated to a pixel, size `ceil(w * scale)` by `ceil(h * scale)`; rotated sprites turn about the box centre with integer halves. Blending happens in linear space on the GPU.
- Crate-internal checks that need engine internals live in `testing/gpu_world.rs`; pixel-level behaviour is in `tests/e2e/gpu_world.rs`.

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
- **`FrameScene` is built in `present_frame`:** the per-frame data (draw list, trails, lights, effects, UI) that passes read.

## Gotchas

- While raycasting, the `raycast` pass replaces the 2D world passes (`world`, `lighting`, `unlit_world`) and clears to black, so a raycast scene with no wall textures renders black.
- Disabling `post_process` removes every effect, including flashes and tints.
- Custom shader and effects APIs for game passes are not there yet: a game pass records raw wgpu work on `ctx.encoder_and_current_view()`.
