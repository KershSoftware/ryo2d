# Pass-Based Render Pipeline - Design

## Overview

Every GPU frame is drawn by one ordered list of render passes instead of four hand-written render paths. This is sub-project 1 of 3 for the renderer (owner decision, 2026-10-10): **1. one pass pipeline (including the raycast path)**, 2. z-interleaved layering, 3. a curated API for game-defined passes and custom shaders.

**Why:** `present_frame` (`engine.rs`) picks one of `render_with_effects`, `render_with_gpu_sprites`, `render_with_lighting` and `render_raycast` (`graphics/renderer.rs`). Each repeats the same stages (upload, clear, sprites, CPU blend, trails, lighting, post-process, UI, composite, capture) with small, easy-to-break differences, wired through up to four generic callbacks. Adding a stage means editing several paths. The experimental `render` module (`RenderPipeline` with a dependency graph) was never wired in, half its passes are stubs, and its built-in order is wrong.

**Owner decisions (2026-10-10):** an ordered pass list (not a dependency graph or a resource graph); the pipeline types are public from the start (later sub-projects may change them); breaking existing demos and games is acceptable (the engine is an experiment) - fix them, don't shim.

**Out of scope:** z-interleaving GPU sprites, CPU entities and trails (sub-project 2); curated helpers for custom shaders and insertion points (sub-project 3); multiple cameras/viewports; new visual features.

## Public API (`ryo2d::render`, in the prelude)

- `trait RenderPass: Send + Sync` - `fn id(&self) -> &str`; `fn should_run(&self, frame: &FrameInfo) -> bool { true }`; `fn execute(&mut self, ctx: &mut RenderContext<'_>)`.
- `struct RenderPipeline` - an ordered list of passes, each with an enabled flag:
  - `new()` (empty; also `Default`), `default_2d()` (the built-in passes; what the engine uses)
  - `push`, `insert_before(anchor, pass)`, `insert_after(anchor, pass)` → `Result<(), RenderPipelineError>` (duplicate id, unknown anchor)
  - `remove(id) -> Option<Box<dyn RenderPass>>`, `set_enabled(id, bool) -> Result<..>`, `is_enabled(id) -> Option<bool>`, `contains(id)`, `pass_ids()`
  - `passes_to_run(&FrameInfo) -> Vec<&str>` - the ids that would run for a frame (enabled and `should_run`), for debugging and tests
- `enum RenderPipelineError { DuplicatePass(String), UnknownPass(String) }` (`thiserror`).
- `struct FrameInfo` (`Copy`) - what this frame contains, decided before any pass runs: `game_width`, `game_height`, `raycast`, `gpu_sprites`, `trails`, `lighting`, `effects`, `ui`, `capture`.
- `struct RenderContext<'a>` - public accessors only: `device()`, `queue()`, `encoder()`, `frame()`, `game_size()`, `current_view()` (the texture holding the frame's latest image) and `encoder_and_current_view()` (both at once, to draw onto the image). Built-in passes use crate-internal fields.
- `mod pass_ids` - `UPLOAD`, `CLEAR`, `RAYCAST`, `GPU_SPRITES`, `CPU_BLEND`, `TRAILS`, `LIGHTING`, `UNLIT_TRAILS`, `POST_PROCESS`, `UI`, `COMPOSITE`, `CAPTURE`.
- `SceneContext::render_pipeline: &mut RenderPipeline` - games toggle built-in passes or insert their own.

The experimental module's old contents (dependency graph, `TextureSlot`, catch-all context, `ClearPass`/`ParallaxPass`/`UploadPass`/`LightingPass`, `RenderPassId`) are removed. Parallax already draws through GPU sprites.

## Built-in passes (`default_2d`, in order)

| Pass | Runs when | Does |
|---|---|---|
| `upload` | not raycast | CPU pixel buffer → CPU staging texture |
| `clear` | not raycast | game texture ← `DEFAULT_BACKGROUND_COLOR`; latest image = game texture |
| `raycast` | raycast | clears the game texture to black, walls/floors/ceilings, then billboard sprites; latest image = game texture |
| `gpu_sprites` | not raycast, has sprites | GPU sprite batches onto the game texture |
| `cpu_blend` | not raycast | CPU staging texture alpha-blended onto the game texture |
| `trails` | not raycast, has trails | lit (`TrailStyle::Solid`) trails and lines onto the game texture |
| `lighting` | not raycast, lighting on | lights, shadows (if shadow-casting lights and occluders), compose game texture → lit texture; latest image = lit texture |
| `unlit_trails` | not raycast, has trails | `with_lighting(false)` trails and lines onto the latest image |
| `post_process` | effects active | latest image → effect chain; latest image = chain output |
| `ui` | UI buffer non-empty | UI buffer → UI texture |
| `composite` | always | latest image, then the UI (if the `ui` pass uploaded it this frame), to the window/offscreen target with letterboxing |
| `capture` | capture requested | the same composite into the capture texture (no letterbox) |

GPU pipelines stay in the existing renderers (`SpriteGpuRenderer`, `LightingRenderer`, `TrailRenderer`, `PostProcessPipeline`, `GpuRaycastRenderer`); passes call into them. The final composite (and capture) drawing moves into a `Compositor` owned by `WgpuRenderer`. The lit texture becomes a persistent target, recreated with the game texture, instead of a per-frame allocation.

## Data flow

1. `present_frame` collects inputs exactly as today: GPU sprite batches (`render_gpu`), trails and lines, lights and occluders (with lighting auto-enable and god rays), raycast sync (`flush_pending`, camera, sprites, lights - split out of `sync_and_render_raycast` without its rendering).
2. It builds a crate-internal `FrameScene` (borrowed subsystems and per-frame data) and calls `WgpuRenderer::render_frame(&mut pipeline, scene)`.
3. `render_frame` acquires the target, prepares any capture, computes `FrameInfo`, creates one encoder and a `RenderContext`, runs the pipeline (each enabled pass whose `should_run` holds), submits once, presents and reads back the capture. Surface errors are returned exactly as today (`Outdated`/`Lost` handling in the engine is unchanged).
4. The context tracks which texture holds the latest image (game, lit, or a post-process buffer); this replaces the per-path source choices.

The engine owns the `RenderPipeline` (`default_2d()`).

## Behaviour

Frames are pixel-identical to today's, with one deliberate change: a raycast frame drawn before the wall textures are bound shows black, not the previous frame. Unchanged: the UI always composites last (lighting and effects never touch it); lit trails are lit, unlit trails draw after lighting (straight after lit trails when lighting is off); effects apply to the lit image when lighting ran.

## Error handling

Pipeline configuration errors return `RenderPipelineError`; nothing panics (the old `add_pass` panicked on duplicates). Passes can't fail; GPU validation behaves as today. A disabled built-in pass simply doesn't run (disabling `composite` leaves the screen unchanged - the game's choice).

## Testing

- **Pipeline (no GPU, integration tests):** `default_2d` order; insert before/after; duplicate and unknown-id errors; enable/disable; remove; `passes_to_run` for plain, sprites+trails, lighting, raycast, effects, UI and capture frames.
- **Migration:** the pipeline is built next to the old paths behind a crate-internal switch. Headless comparison tests render the same deterministic scenes through both and require byte-identical frames over several steps: CPU sprites + UI; GPU sprites + CPU sprites; trails and lines (lit and unlit) without lighting; lighting with shadows, sprites and both line kinds; effects (including bloom) with and without lighting, with UI; raycast with walls, sprites, an effect and UI. Then the engine switches over and the old paths, the switch and the comparison tests are deleted.
- **Lasting behaviour tests (headless pixel probes):** UI is untouched by effects and lighting; effects apply to the lit image; a raycast frame without wall textures is black; a game pass inserted after `post_process` changes the frame; disabling `post_process` removes an effect. Existing unlit-trail and headless tests keep passing.

## Documentation

Rewrite `src/core/render/AGENTS.md`; update `src/core/graphics/AGENTS.md` (render paths); the `ryo-renderer` and `ryo-api` skills (both copies, CRLF); rustdoc on every public item; fix any demo or game that breaks.

## Design decisions

- **Ordered list, not a dependency graph:** twelve built-in passes have one correct order; spelling it out avoids silent reordering from a mistyped dependency, and before/after anchors are what game passes need.
- **Thin passes:** moving GPU pipelines into passes would be a much larger, riskier change with no visible benefit now.
- **`should_run` on the trait:** keeps the run conditions declarative, so `passes_to_run` can show which passes a frame used.
- **Small public context:** games get the device, queue, encoder and latest image; internals stay crate-private so later sub-projects can reshape them.
