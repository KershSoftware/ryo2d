# AGENTS.md - Render Pipeline

The engine draws every frame through one `RenderPipeline` (a resource, `RenderPipeline::default_2d()`): `Engine::present_frame` (`engine/extract.rs`) builds a `FrameScene` and `WgpuRenderer::render_frame` runs the enabled passes in order on **one command encoder with one submit**. Games reach it with `ctx.render_pipeline()` (a `Mut<RenderPipeline>`): toggle built-in passes or insert their own. The pipeline belongs to the engine, so a scene's changes persist across scene transitions.

## Default Order (`default_2d`)

| # | Id (`pass_ids`) | Runs when (`should_run`) | Draws |
|---|-----------------|--------------------------|-------|
| 1 | `CLEAR` | always | clears the game texture |
| 2 | `WORLD` | the draw list has lit items | entities, `Scene::render` sprites (particles, parallax) and lit trails and lines, one z-sorted list |
| 3 | `LIGHTING` | lighting is on | 2D lighting onto the lit texture |
| 4 | `UNLIT_WORLD` | always (returns early with no unlit items) | trails and lines with `with_lighting(false)`, by z |
| 5 | `TRANSITION` | a transition fade is visible | scene transition fade over the whole frame (not the UI) |
| 6 | `POST_PROCESS` | effects are active | the effects chain |
| 7 | `UI` | the UI buffer has content | uploads the UI layer |
| 8 | `COMPOSITE` | always | letterboxed present to the window or offscreen target, UI blended last |
| 9 | `CAPTURE` | a capture was requested | frame capture readback copy |

## Building the Frame (`present_frame`)

Everything is read from the world by query, every frame. There is no render cache: nothing is invalidated, and an entity draws where its `GlobalTransform` says on the frame it exists.

1. `Scene::render(gpu)` for the visible scenes, bottom-up (`GpuRenderContext`: `gpu.batch`, `gpu.camera`, read-only `gpu.world`); the `BatchCollector` keeps submission order.
2. Trails (`&Trail2D`: world-space points), lines (`(&GlobalTransform, &Line2D)`: at the entity's translation) and the debug overlays (unlit `Line2D`s at `i32::MAX`, added last so they are what gets dropped when the vertex budget runs out).
3. Lights (`(&GlobalTransform, &Light2D)`, at their world translation) and occluders (`(&GlobalTransform, &LightOccluder2D)`, keeping rotation); `LightingSettings` is copied into the lighting renderer.
4. Sprite globals: `x` = game elapsed time, `w` = real elapsed time (`SpriteGpuRenderer::set_viewport`).
5. `EntityDrawer::collect` (`entities.rs`): one query over `(Entity, &Renderable, &GlobalTransform, Option<&YSort>)` for visible renderables, then `(Entity, &GlobalTransform, &CompositeTile)`. Disabled entities are skipped by the queries themselves.
6. Trail groups join the draw list; trail and line vertices are uploaded once (`TrailRenderer::prepare`) before the pipeline runs; the passes only draw.
7. The list is sorted and the sprite instances uploaded (`prepare_world`), then the pipeline runs.

`FrameScene.time` (lighting, trails, the transition) is real elapsed time; post-process effects run on `EffectsManager`'s own clock, advanced with real time in `PostUpdateSet::Extract`. Only the sprite globals' `x` is game time.

## Draw Order

- Items sort by (z, kind, submission): `z_order` picks the layer; at equal z entities draw first, then `Scene::render` sprites, then trails and lines (`DrawKind` order).
- Within a layer, plain entities keep query order and y-sorted ones follow, ordered by their sort key: the point `YSort.foot_offset` of the way down the sprite (default the feet), computed from the anchor, so the key is the same whichever `Anchor` places the sprite (`y_sort_key`).
- Shadows draw just before their entity. Composite tiles draw at their own `z_order`, after the layer's entities.
- Skeleton debug lines are `Line2D`s at `i32::MAX - 1`; the engine's overlays sit at `i32::MAX`.

## Placement (`entities.rs`)

- Positions come from `render_global(global, previous, time.fixed.overstep_fraction())`: entities with `PreviousGlobalTransform` (the `Interpolated` ones and their descendants) draw blended between the last two fixed steps. This covers single sprites and tiled runs; composite tiles draw at `GlobalTransform`.
- **Single sprite:** the anchor point (`Renderable::anchor`, default `Anchor::Center`) sits on the position; scale and rotation happen about it.
- **Tiled run** (`RenderStyle::Tiled`): laid out from the entity's position, which is the first tile's top-left (Up/Left runs grow backwards); the anchor is ignored; the run rotates and scales about that position.
- **Composite tile:** parts at `origin + offset` (their top-left), scaled by the global scale, never rotated; the origin is blended for interpolated entities.
- The top-left is the screen position truncated to a pixel, the size `ceil(w * scale)` by `ceil(h * scale)`. Blending happens in linear space on the GPU.
- Entity sprites are atlas textures keyed by `SpriteHandle::serial()` plus `generation()` (bumped by hot reload).

## Ordering Rules

- Order is the list order. `push` adds at the end; `insert_before` / `insert_after` place a pass relative to a named one.
- Pass ids are unique. A duplicate id or an unknown anchor id returns `RenderPipelineError`; nothing panics.
- `set_enabled` is the caller's switch (a disabled pass keeps its place and is skipped); `should_run(&FrameInfo)` is the pass's own per-frame condition, asked only for enabled passes. `passes_to_run(&frame)` says what would run.
- `remove` hands the boxed pass back (`Box<dyn RenderPass>` is itself a `RenderPass`), or `None` for an unknown id.

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  let mut pipeline = ctx.render_pipeline();
  // Passes persist across scenes: add once, or remove in on_exit
  if !pipeline.contains("flash") {
    let _ = pipeline.insert_before(pass_ids::COMPOSITE, Flash);
  }
}
```

## Design Decisions

- **Thin passes.** GPU pipelines, bind groups and buffers live in the subsystem renderers (sprites, trails, lighting, post-process, compositor). A pass only calls into them with the `RenderContext`.
- **Latest-image tracking.** `RenderContext` tracks which texture holds the latest image (`FrameImage`: game, lit, or a post-process output). A pass drawing on "the frame" uses `current_view()` / `encoder_and_current_view()`, so a game pass after `post_process` draws on the post-processed image. Only passes before `composite` reach the screen.
- **The UI is composited only if the `ui` pass uploaded it this frame.** The UI texture is persistent; without that flag a disabled or skipped `ui` pass would blend the previous frame's UI.
- **A capture is read back only if the `capture` pass ran.** Otherwise `render_frame` reports `CaptureError::Readback` rather than returning a stale image.
- **The lit texture is persistent** and every game-size texture follows `resize_buffer` (ResizableStretch).
- **The fade** is the `transition` pass (colour and opacity from `FrameScene::overlay`): it copies the frame and mixes toward the colour in sRGB; blending in linear light held bright areas back and then snapped at the end.

## Presenting

- Present mode: vsync on (the default) is `Fifo`; vsync off is `Mailbox`, else `Immediate`, else `Fifo`. The engine paces frames to `target_fps` (sleep, then spin) only when presenting does not block (`should_pace_frames`), which is vsync off unless the surface fell back to `Fifo`.
- Surface errors: `Outdated` and `Timeout` skip the frame and reconfigure before the next; `Lost` resizes (reconfigures) and skips; `OutOfMemory` is fatal and ends the run. A skipped frame still ran its update; it just presented nothing.

## Gotchas

- `FrameInfo::world` means the draw list has lit items; `unlit_world` has no flag of its own. `FrameInfo` describes content, not passes: `lighting` and `effects` stay true when the game disabled those passes.
- Disabling `post_process` removes every effect, including flashes and tints.
- A game pass records raw wgpu work on `ctx.encoder_and_current_view()` (`ryo2d::wgpu`); there is no higher-level API for game passes. Custom sprite and effect shaders go through `Material` / `EffectsManager::add_custom` instead (`src/core/shaders/AGENTS.md`).
- `DebugStats::draw_calls` counts the draws of `world` and `unlit_world`.
- Crate-internal checks that need engine internals live in `src/core/testing/gpu_world.rs`; pixel-level behaviour is in `tests/e2e/gpu_world.rs` and `tests/e2e/render_pipeline.rs`.
