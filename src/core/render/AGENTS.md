# AGENTS.md - Render Pipeline

The engine draws every frame through one `RenderPipeline` (a resource, `RenderPipeline::default_2d()`): `Engine::present_frame` (`engine/extract.rs`) builds a `FrameScene` and `WgpuRenderer::render_frame` runs the enabled passes on **one command encoder with one submit**: the camera passes (`PassScope::Camera`) once per camera in render order, each on that camera's image, then the frame passes (`PassScope::Frame`) once. Games reach it with `ctx.render_pipeline()` (a `Mut<RenderPipeline>`): toggle built-in passes or insert their own. The pipeline belongs to the engine, so a scene's changes persist across scene transitions.

## Default Order (`default_2d`)

| # | Id (`pass_ids`) | Scope | Runs when (`should_run`) | Draws |
|---|-----------------|-------|--------------------------|-------|
| 1 | `CLEAR` | camera | always | clears the camera's image per `CameraClear` (transparent for `None`) |
| 2 | `WORLD` | camera | the draw list has lit items | the camera's lit draws from the batch: entities, `Scene::render` sprites (particles), parallax, trails, lines and debug shapes, one z-sorted list |
| 3 | `LIGHTING` | camera | lighting is on | the camera's lights and shadows, composed into the lit image (straight-in camera) or back into its own image |
| 4 | `UNLIT_WORLD` | camera | always (returns early with no unlit draws) | the camera's unlit draws (anything `with_lighting(false)`, and debug shapes), by z |
| 5 | `CAMERAS` | frame | always | window cameras' own images into their viewports, in order; clears the scene image first when no camera drew straight into it |
| 6 | `TRANSITION` | frame | a transition fade is visible | scene transition fade over the whole frame (not the UI) |
| 7 | `POST_PROCESS` | frame | effects are active | the effects chain |
| 8 | `UI` | frame | the UI buffer has content | uploads the UI layer |
| 9 | `COMPOSITE` | frame | always | to the window or offscreen target through `ScreenMapping` (bars for pixel art), UI blended last |
| 10 | `CAPTURE` | frame | a capture was requested | frame capture readback copy |

## Cameras in the Frame

- `plan_cameras` (`frame.rs`) orders the cameras: texture cameras, then window cameras, each by `order` (ties by entity). The first window camera draws straight into the scene image when its viewport is `Viewport::FULL` and it clears; every other window camera draws into an image of its own (`CameraImages`, `graphics/camera_images.rs`), sized to its viewport in render pixels (`PixelRect::of_viewport`, edges rounded so viewports that meet share them). A texture camera draws straight into its render target when its viewport covers it, otherwise into an image of its own that `leave_camera` places in its rectangle of the target (premultiplied blending, like the `cameras` pass); one whose target is missing is left out (warned once).
- Each `CameraDraw` carries the view, layers, clear, target, image rectangle and whether it is lit. `RenderContext::enter_camera` points `current_view()` at the camera's image (`FrameImage::Camera`) for its passes; `leave_camera` goes back to the scene image, which a straight-in camera may have left lit.
- Per-camera GPU state: a uniform buffer per camera (`CameraUniforms`: `view_proj`, image size, game and real time, snap), its bind group in the batch and in lighting, camera images and light targets. All of it follows the planned cameras each frame, so a despawned camera's state goes with it.

## Building the Frame (`present_frame`)

Everything is read from the world by query, every frame. There is no render cache: nothing is invalidated, and an entity draws where its `GlobalTransform` says on the frame it exists. Drawables are gathered once, in world space, and sorted once; each camera keeps what it sees.

1. `Scene::render(gpu)` for the visible scenes, bottom-up (`GpuRenderContext`: `gpu.batch`, `gpu.camera()` = the main camera's view, read-only `gpu.world`); the `BatchCollector` keeps submission order.
2. Lights (`(&GlobalTransform, &Light2D)`, at their world translation) and occluders (`(&GlobalTransform, &LightOccluder2D)`, keeping rotation); `LightingSettings` is copied into the lighting renderer.
3. Render targets' textures follow `RenderTargets`; the cameras are planned and their uniforms, bind groups and light targets synced.
4. The `Scene::render` sprites join the draw list; each `ParallaxLayer` is laid out for every camera on its layers (items for that camera alone).
5. `EntityDrawer::collect` (`entities.rs`): one query over the visible renderables (with their `RenderLayers`), then the composite tiles. Quads no camera on their layers can see are skipped before their texture uploads. Disabled entities are skipped by the queries themselves.
6. Trails (`&Trail2D`: world-space points) and lines (`(&GlobalTransform, &Line2D)`: at the entity's translation) become polylines on the batch's triangle stream, one item each. The debug overlays are added to `DebugDraw`, whose shapes are built for each camera in its own screen pixels (items for that camera alone).
7. The list is sorted; `Batch2D::prepare` makes the frame's textures resident, lays each camera's lit and unlit items (on its layers, inside its `CameraView::bounds`, not showing its own target, `only_camera`) into draws and uploads the instances and triangles once; then the pipeline runs. `present_frame` clears the frame's debug shapes afterwards, drawn or skipped.

`FrameScene.time` (lighting, trails, the transition) is real elapsed time; post-process effects run on `EffectsManager`'s own clock, advanced with real time in `PostUpdateSet::Extract`. Only `CameraGlobals::game_time` (sprite effects, custom sprite shaders) is game time.

## The 2D Batch (`batch/`)

One batch draws the world: sprites, shadows, tiles, composite parts, particles, `Scene::render` sprites, parallax tiles, trails, lines and debug shapes.

- **Two streams**, each uploaded once per frame into a buffer that doubles when a frame needs more: instanced quads (the 128-byte `SpriteInstance`, remapped into its atlas slot) and indexed triangles (`Vertex2D`, 72 bytes: position, uv, colour, params, across, the slot a textured line repeats).
- **Keys and planning** (`key.rs`, `plan.rs`): every item resolves to a `DrawState` (page, `BatchShader`, `BlendMode`, `Sampling`, scissor). Per camera, `plan_draws` merges consecutive items with equal states (and, for triangles, adjacent index ranges) into `Draw`s; each camera's quads have a range of the instance buffer of their own, its lit and unlit draws in one range.
- **Pipelines** (`pipelines.rs`) are made on first use per `(BatchShader, BlendMode, Stream, format)`; a custom sprite shader gets one per blend mode it is drawn with, checked without blocking (`Checked`), and draws plain until its check passes. Every batch shader writes colour premultiplied by alpha, so each `BlendMode` is one fixed blend state (`blend_state`); `Premultiplied` picks the shader entries that take a texture's colour as it is.
- **Bind groups are made with their resource:** one per camera (its uniform buffer) and one per texture page and sampler (made with the page); none per frame. Every bind group goes through `graphics::bind_groups::create`, which counts them in tests (`steady_frames_create_no_bind_groups`, and a guard test that fails on any other `create_bind_group`); passes that bind frame resources look them up in a `BindGroupCache` keyed by the resources themselves, cleared when those are recreated.
- **Lines** (`lines.rs`, `polyline.rs`): trails and lines become polylines with miter, bevel or round joins (each corner one wedge, fanned from the corner point, so nothing is covered twice) and butt, square or round caps.
- **Debug draw** (`render/debug_draw.rs`): `DebugDraw` keeps world-space shapes in a fixed-step list (cleared when the next step begins) and a frame list (cleared after the frame); each camera builds them as polylines in its own screen pixels.

## Draw Order

- Items sort by (z, kind, submission), once per frame; each camera draws the sorted items it sees. `z_order` picks the layer; at equal z entities draw first, then `Scene::render` sprites, then trails and lines (`DrawKind` order).
- Within a layer, plain entities keep query order and y-sorted ones follow, ordered by their sort key: the point `YSort.foot_offset` of the way down the sprite (default the feet), computed from the anchor, so the key is the same whichever `Anchor` places the sprite (`y_sort_key`).
- Shadows draw just before their entity. Composite tiles draw at their own `z_order`, after the layer's entities.
- Debug shapes (`DebugDraw`, the overlays, `draw_skeleton`) are unlit at `i32::MAX`.

## Placement (`entities.rs`)

- Positions come from `render_global(global, previous, time.fixed.overstep_fraction())`: entities with `PreviousGlobalTransform` (the `Interpolated` ones and their descendants) draw blended between the last two fixed steps. This covers single sprites, tiled runs and composite tiles (lights, occluders and lines too, in `engine/extract.rs`).
- **Single sprite:** the anchor point (`Renderable::anchor`, default `Anchor::Center`) sits on the position; scale and rotation happen about it.
- **Tiled run** (`RenderStyle::Tiled`): laid out from the entity's position, which is the first tile's top-left (Up/Left runs grow backwards); the anchor is ignored; the run rotates and scales about that position.
- **Composite tile:** parts at `origin + offset` (their top-left), scaled by the global scale, never rotated; the origin is blended for interpolated entities.
- Instances carry world-space quads: centre, size (texels times scale, in world units), rotation and flips. The camera's `view_proj` takes them to its image; in pixel-art mode the sprite vertex shader snaps axis-aligned quads to whole target pixels. A quad is culled per camera by its half-diagonal box. Blending happens in linear space on the GPU.
- Entity sprites are registry textures (`TextureId`) keyed by `SpriteHandle::serial()`, re-uploaded when `generation()` changes (hot reload). A renderable with `target: Some(t)` draws the render target's texture at the target's size; the camera drawing into `t` skips it (warned once per entity).

## Ordering Rules

- Order is the list order. `push` adds at the end; `insert_before` / `insert_after` place a pass relative to a named one.
- Pass ids are unique. A duplicate id or an unknown anchor id returns `RenderPipelineError`; nothing panics.
- Camera passes run before frame passes wherever they sit in the list; within a scope, list order holds.
- `set_enabled` is the caller's switch (a disabled pass keeps its place and is skipped); `should_run(&FrameInfo)` is the pass's own per-frame condition, asked only for enabled passes. `passes_to_run(&frame)` says what would run, in run order.
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

- **Thin passes.** GPU pipelines, bind groups and buffers live in the subsystem renderers (the batch, lighting, post-process, compositor). A pass only calls into them with the `RenderContext`; `world` and `unlit_world` replay the camera's batch draws.
- **A camera image per camera, then a frame-level composite.** Lighting and the passes keep working on whole images, so no pass needs scissor logic; the common single full-window camera draws straight into the scene image and pays nothing extra. Post-processing runs once per frame, not per camera.
- **Latest-image tracking.** `RenderContext` tracks which texture holds the latest image (`FrameImage`: game, lit, a camera's own image, or a post-process output). A pass drawing on "the frame" uses `current_view()` / `encoder_and_current_view()`, so a game pass after `post_process` draws on the post-processed image, and a camera pass on its camera's image. Only passes before `composite` reach the screen.
- **The UI is composited only if the `ui` pass uploaded it this frame.** The UI texture is persistent; without that flag a disabled or skipped `ui` pass would blend the previous frame's UI.
- **A capture is read back only if the `capture` pass ran.** Otherwise `render_frame` reports `CaptureError::Readback` rather than returning a stale image.
- **Scene images follow the render size** (`ScreenMapping::render_size`: the window's physical size, or the pixel-art target); the lit texture is persistent. Camera images and light targets follow their camera's image size.
- **Images are premultiplied.** A camera image clears to a premultiplied colour (transparent for `CameraClear::None`) and sprites blend over it, so the `cameras` pass places images with premultiplied blending.
- **The fade** is the `transition` pass (colour and opacity from `FrameScene::overlay`): it copies the frame and mixes toward the colour in sRGB; blending in linear light held bright areas back and then snapped at the end.

## Presenting

- Present mode: vsync on (the default) is `Fifo`; vsync off is `Mailbox`, else `Immediate`, else `Fifo`. The engine paces frames to `target_fps` (sleep, then spin) only when presenting does not block (`should_pace_frames`), which is vsync off unless the surface fell back to `Fifo`.
- Surface states (`gpu_error.rs`): `Suboptimal` draws, then reconfigures; `Timeout` and `Occluded` skip the frame; `Outdated` reconfigures; `Lost` recreates the surface; a validation error is fatal and ends the run, as is a lost device. A skipped frame still ran its update; it just presented nothing.

## Gotchas

- `FrameInfo::world` means the draw list has lit items; `unlit_world` has no flag of its own. `FrameInfo` describes content, not passes: `lighting` and `effects` stay true when the game disabled those passes.
- Disabling `post_process` removes every effect, including flashes and tints.
- A game pass records raw wgpu work on `ctx.encoder_and_current_view()` (`ryo2d::wgpu`); there is no higher-level API for game passes. Custom sprite and effect shaders go through `Material` / `EffectsManager::add_custom` instead (`src/core/shaders/AGENTS.md`).
- `DebugStats::draw_calls` counts the draws of `world` and `unlit_world`, over every camera.
- Crate-internal checks that need engine internals live in `src/core/testing/gpu_world.rs`; pixel-level behaviour is in `tests/e2e/gpu_world.rs` and `tests/e2e/render_pipeline.rs`.
