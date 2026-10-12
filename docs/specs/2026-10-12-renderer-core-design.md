# Renderer Core (2a): GPU Foundation, Web Build, Cameras, Resolution - Design

## Overview

Sub-project 2 of the professional-grade overhaul (`docs/research/2026-10-11-engine-audit.md`, Part 1 §3 and Part 5 step 4) rebuilds the renderer's foundations. It is split into three specs, each with its own plan and branch:

- **2a (this spec):** wgpu 30, a wasm build, one texture registry with integer handles, a logical/physical resolution model with an opt-in pixel-art mode, cameras as entities with viewports, and render-to-texture.
- **2b:** one general 2D batch (textured quads and triangle lists with per-batch texture, pipeline, blend and scissor) that sprites, trails, lines, particles and debug drawing feed; blend and filter modes per material; shared WGSL includes; no per-frame bind group creation.
- **2c:** an `Rgba16Float` scene target with tonemapping in the composite; lighting and the post-process chain on it.

**Why:** the renderer has no camera matrix. Entities, particles, parallax and `Scene::render` sprites are transformed to screen pixels on the CPU (with shake), trails and lights use a hand-built GPU matrix (without shake), god rays use CPU screen UVs (without shake). Every target is one fixed "game size"; the window only enters through the compositor's letterbox uniform, and the viewport maths exists three times. Positions are truncated to integer pixels. Textures are keyed four ways (atlas keys, a string map in the sprite renderer, `TextureManager`, the trail renderer's cache), with `TextureRef = Arc<str>` cloned per draw item. GPU init panics on failure, and custom shader compilation blocks mid-frame on `pollster::block_on(pop_error_scope())`. wgpu is eight majors behind.

**Owner decisions (2026-10-12):** resolution-independent rendering is the default, pixel art an opt-in mode; the browser is a target now through WebGPU, with "done" meaning the engine builds for wasm and one demo runs; cameras are entities with viewports and render-to-texture; approach A (rebuild in place in ordered steps, every step green); breaking APIs, demos and games is acceptable; networking is untouched beyond a compile gate.

**Out of scope:** the unified batch, per-material blend and filter modes, per-frame bind group removal (2b); HDR and tonemapping (2c); a GPU UI, real text and widget identity (sub-project 4); web asset loading over fetch, web audio from files, browser saves (sub-project 5); the tilemap renderer, the shadow rewrite, trails as ribbons (feature modules); WebGL2; multiple windows; winit 0.31 (still beta).

## GPU foundation

- `wgpu` and `naga` move from 22 to 30.0.1. The first step changes no behaviour: every e2e pixel test and every demo-tour capture is byte-identical to `main` before anything else changes.
- Initialisation returns errors instead of panicking. `EngineError::Gpu(GpuError)` with `NoAdapter`, `RequestDevice`, `CreateSurface` and `DeviceLost`; `EngineBuilder::run()` returns it. The device's lost and uncaptured-error callbacks log and end the run cleanly.
- Custom sprite and post-process shaders are parsed and validated with `naga` on the CPU before `create_shader_module`; an invalid shader draws plain (sprite) or is skipped (effect) and warns once. No `push_error_scope`/`pop_error_scope` blocking remains on any path.
- `CurrentSurfaceTexture` handling: `Timeout` and `Occluded` skip the frame; `Outdated` and `Suboptimal` reconfigure; `Lost` recreates the surface; `Validation` is fatal. Present through `queue.present`.
- Surface format: the first sRGB format; if there is none, the composite shader encodes to sRGB itself so output is not dark.
- Limits and features: `Limits::default()` (WebGPU's) and no features. Nothing native-only is used.
- Readback (capture, `HeadlessEngine::frame`) stays synchronous on native; it is unavailable on the web.

## Web build

The `ryo2d` library compiles for `wasm32-unknown-unknown` with the WebGPU backend.

- `web-time` replaces `std::time::{Instant, SystemTime}` crate-wide (it is `std` on native).
- On wasm the event loop runs through `EventLoopExtWebSys::spawn_app`. The window is created in `resumed` with an appended canvas; adapter and device creation are awaited in `wasm_bindgen_futures::spawn_local` and delivered back through an `EventLoopProxy` user event. The browser paces frames: no sleep or spin pacing on the web.
- Compiled out on wasm (`cfg(target_arch = "wasm32")`): asset hot reload, the dev and profiler windows, asset loader threads (loads are synchronous from bytes), capture to disk and its writer thread, disk saves (the save API returns an "unsupported on the web" error), the CLI module, and the `network` module (a `cfg` on its declaration and prelude block; its code is not touched).
- Kept: audio (kira from memory) and gamepads (gilrs' `wasm-bindgen` feature). Loading from embedded bytes works; loading by path does not on the web.
- `getrandom` gets its `wasm_js` feature for wasm targets.
- Tooling: `./dev.sh web <demo>` and `./dev.ps1 web <demo>` build and serve one demo with `trunk` (installed once with `cargo install trunk`); the demo crate carries an `index.html`. `./dev check` and `./dev lint` also check `ryo2d` for `wasm32-unknown-unknown`.
- Acceptance: the `lighting` demo (it loads no files and exercises sprites, the `Rgba16Float` light buffer, shadows and the demo-kit UI) runs in Chrome, Safari and Firefox on desktop, checked by hand, plus a Playwright screenshot in Chromium where WebGPU is available.

## Texture registry

- A renderer-internal `TextureRegistry` assigns every GPU texture a `TextureId` (`index: u32`, `generation: u32`, `Copy`). An entry is a `SpriteHandle` (keyed by its serial; a generation bump from hot reload re-uploads) or a render target. It records residency (an atlas slot or a standalone texture), size and last use; the current atlas packing, eviction and re-upload rules are kept.
- `TextureId` replaces `AtlasKey`, `SpriteGpuRenderer.textures` (`HashMap<String, CachedTexture>`), `TextureRef = Arc<str>` in the draw list, and the trail renderer's own texture cache. No string is hashed or cloned per draw item.
- Games keep passing `SpriteHandle`. `GpuRenderContext`'s batch takes `&SpriteHandle` instead of `sprite.id()`; parallax layers and particles store the handle. `SpriteHandle::id()` remains the path, for display only.
- Deleted: `TextureManager`, `TextureContext`, `TextureHandle`, `TextureArrayHandle`, `ctx.textures()`. Nothing draws through them; the assets sub-project builds typed asset handles.

## Resolution model

Three spaces:

- **Physical pixels:** the window surface (`inner_size`).
- **Screen space:** what UI, mouse input and `ctx.screen_size()` use.
- **World space:** what cameras look at.

`ScreenMapping` (one type, one implementation) takes the physical size, the scale factor and the presentation mode and gives the screen-space size, the window rectangle screen space occupies (letterbox included) and both conversions. The compositor, mouse mapping and tests all use it; the three copies of the viewport maths are deleted.

Two presentation modes replace `WindowMode` (`Fixed`, `ResizableAspectRatio`, `ResizableIntegerScale`, `ResizableStretch`) and the `max_width`/`max_height` scale caps; `EngineBuilder::resizable(bool)` (default `true`) is separate.

- **Resolution-independent (default).** Cameras render at the window's physical resolution. Screen space is logical pixels (physical divided by the scale factor). `EngineBuilder::resolution(w, h)` (and `resolution` in `Ryo.toml`) is the initial logical window size and the main camera's `Fit` area. Positions are sub-pixel; sprites sample linearly. Resizing resizes the window targets and updates cameras.
- **Pixel art (opt-in).** `EngineBuilder::pixel_art(w, h)`, or `pixel_art = true` in `Ryo.toml` with `resolution` as the target size. Window cameras render into a fixed `w x h` target with nearest sampling, scaled to the window by the largest integer that fits, centred with black bars. Sprite origins and cameras snap to whole target pixels. Screen space is target pixels, so UI and the mouse share the art's grid. The initial window is the target times the largest integer scale that fits within 1280x720 logical (at least 1); `EngineBuilder::window_size(w, h)` overrides it. The bundled demos and games opt in.

UI keeps its CPU rasteriser in 2a. Its buffer is screen-space sized (not physical), composited with nearest sampling; `ui_scale` defaults to 1 because high-DPI sharpness now comes from compositing at physical size.

`Input` reports the pointer in screen space, including over letterbox bars (no move is dropped). World positions come from a camera.

## Cameras

`Camera2d` (component) replaces the `Camera` resource. A camera is an entity with `Transform` (translation is the world point at the view's centre, rotation turns the view) and:

- `zoom: f32` (clamped to 0.01..100).
- `projection: Projection`: `Fit { width, height }` keeps that world area fully visible at any target aspect and shows extra world on the longer axis, without bars; `ScreenUnits` maps one world unit to one screen unit (a larger window shows more). In pixel-art mode the main camera uses `Fit` with the target size, which is one world unit per target pixel.
- `viewport: Viewport` (a rectangle in 0..1 of the target; default the whole target).
- `order: i32` (draw order among cameras sharing a target).
- `target: CameraTarget::Window | CameraTarget::Texture(RenderTarget)`.
- `clear: CameraClear::Background | Color(Color) | None` (`None` for overlay cameras).

`RenderLayers(u32)` (component on cameras and drawables; `RenderLayers::layer(n)`, `all()`, `none()`, `with(n)`): a camera draws the entities whose layers intersect its own. An entity without the component is on layer 0; a camera without it sees layer 0. The existing z-order constants `RenderLayer` are renamed `ZLayer`.

`CameraShake` (component) holds the trauma state of today's shake, decays on real time in `PostUpdateSet::Camera`, and offsets the view matrix, so every draw path shakes together.

`CameraView` (component, engine-maintained in `PostUpdateSet::Camera` from the interpolated transform, zoom, shake, projection, viewport and target size) holds the view-projection matrix, the visible world bounds (rotation-aware), the viewport rectangle in screen space (window targets) or target pixels (texture targets), and `world_to_screen` / `screen_to_world`. It is what the renderer draws with, so conversions match what is on screen.

The main camera: the engine spawns one at startup with `Camera2d` (window target, order 0, `Fit` over the configured resolution), `MainCamera`, `CameraShake` and `Interpolated`, positioned at the centre of the resolution so world coordinates equal screen coordinates at zoom 1 (today's convention). `despawn_game_entities` keeps `MainCamera` entities, as it keeps resources. `SceneCleanup::CAMERA` resets the main camera's transform, zoom, rotation, shake, viewport and projection. Cameras a game spawns are ordinary entities and leave with `SceneCleanup::WORLD`.

`ctx.camera()` returns `CameraMut<'_>`, a handle onto the main camera with `position`/`set_position`, `zoom`/`set_zoom`, `rotation`/`set_rotation`, `follow(target, lerp)`, `snap_to`, `clamp_to_bounds`, `add_trauma`, `configure_shake`, `world_to_screen`, `screen_to_world` and `visible_bounds`. `ctx.camera` stops being a field. Other cameras are queried like any component (`Query<(&Camera2d, &CameraView)>`). Moving a camera in `update` is smooth because it is interpolated; a camera moved per frame instead should drop `Interpolated`.

## Rendering per camera and render-to-texture

Each frame:

1. Drawables are gathered once in world space and sorted once (z order, then y-sort, as now).
2. Cameras run in order: texture-target cameras first, then window cameras, each group by `order`. A camera keeps the drawables on its layers inside its visible bounds and runs the **camera passes** into its image: `clear` (per `CameraClear`), `world` (sprites and lit trails), `lighting` (lights in the camera's view, light buffers sized to the camera's image), `unlit_world`. A texture camera's image is its render target. A window camera's image is sized to its viewport in physical pixels (target pixels in pixel-art mode); a camera covering the whole window with a clearing `CameraClear` draws straight into the scene image.
3. The **frame passes** run once: `cameras` (places window camera images in their viewports, alpha-blending cameras with `CameraClear::None`), `transition`, `post_process`, `ui`, `composite` (to the surface through `ScreenMapping`), `capture`.

Render targets:

- `RenderTargets` (resource; `ctx.render_targets()`): `create(width, height) -> RenderTarget`, `resize(target, width, height)`, `remove(target)`, `size(target)`. `RenderTarget` is a `Copy` handle backed by a texture-registry entry (`Rgba8UnormSrgb` until 2c). A camera aiming at a removed target draws nothing and warns once.
- `Renderable::render_target(target)` shows a target on a sprite, with anchor, tint, alpha, scale, rotation and flips like any sprite.
- Because texture cameras run first, a sprite shows this frame's image. When texture cameras show each other, the later one sees the earlier one's current image and the earlier one sees the later one's previous image.
- A camera never draws a drawable that shows its own target; such drawables are skipped for that camera, with one warning per drawable.

Custom passes: `RenderPass` gains `fn scope(&self) -> PassScope` (`PassScope::Frame` by default, which is today's behaviour, or `PassScope::Camera`). A camera-scope pass runs once per camera on that camera's image; `RenderContext::camera()` returns its `CameraView` (and `None` in a frame pass). `insert_before`, `insert_after` and the pass ids work as now; the new frame pass id is `pass_ids::CAMERAS`.

Capture reads the final scene image before the letterbox: target size in pixel-art mode, the window's physical size otherwise. `HeadlessEngine` uses scale factor 1, so existing pixel coordinates hold.

## Draw paths

All move to world space and draw with the camera's view-projection (one uniform buffer and bind group per camera, created when the camera appears and reused):

- **Sprites, tiled runs, composite tiles, drop/blob/contact shadows:** instances carry their world transform (rotation, scale, anchor); the sprite shader applies the view-projection. The `as i32` truncation goes. Culling is per camera against rotation-aware bounds. In pixel-art mode the vertex shader snaps each instance's origin to whole target pixels.
- **Particles:** world-space instances, centred on the particle like other sprites (today's top-left convention is removed).
- **Parallax:** per camera, scrolled by the camera's x and y and scaled by its zoom (today only x).
- **`Scene::render` (`GpuRenderContext`):** instances are in world space; the context exposes the camera view it draws for.
- **Trails and lines; lights and shadows:** already world space; they take each camera's `CameraView` instead of the hand-built matrix, so they rotate and shake with everything else.
- **God rays:** light positions come from the main window camera's view.
- **Debug overlays:** built as world-space lines (no screen-to-world round trip).

## Public API changes

Removed or replaced: the `Camera` resource and `ctx.camera` field (now `Camera2d`, `CameraView`, `CameraShake`, `MainCamera`, `ctx.camera()`); `WindowMode` and `max_width`/`max_height` (now the presentation modes and `resizable`); `TextureManager` and its handles; `RenderLayer` (renamed `ZLayer`); screen-pixel instances in `GpuRenderContext` (now world space); `sprite.id()` as a GPU key. Added: `Camera2d`, `Projection`, `Viewport`, `CameraTarget`, `CameraClear`, `CameraShake`, `CameraView`, `MainCamera`, `CameraMut`, `RenderLayers`, `RenderTarget`, `RenderTargets`, `Renderable::render_target`, `PassScope`, `RenderContext::camera`, `EngineBuilder::{pixel_art, window_size, resizable}`, `ScreenMapping`, `GpuError`.

## Migration (approach A: in place, every step green)

1. wgpu 22 to 30, no behaviour change; e2e and demo-tour captures byte-identical to `main`.
2. Web build: `web-time`, wasm gates, async init and `spawn_app`, `trunk` tooling, the `lighting` demo on the web, wasm checks in `./dev check`/`lint`.
3. Texture registry and `TextureId`; `TextureManager` deleted; string keys gone from the draw path.
4. Resolution model: `ScreenMapping`, the two presentation modes, screen-space UI buffer and input; the bundled demos and games opt into pixel art.
5. Cameras: `Camera2d` and friends, the main camera, `CameraView`, `ctx.camera()`; every draw path in world space with the camera uniform.
6. Per-camera passes, `RenderLayers`, render targets, `PassScope`.
7. Docs: skills, AGENTS.md files, `docs/architecture/overview.md`, the CLI's skill copies, the `init` template (resolution-independent).

## Testing

Test-first for each new piece:

- `ScreenMapping`: window-to-screen-to-window round trips for both modes; letterbox placement for integer scaling; scale factor 2.
- Camera conversions: `screen_to_world(world_to_screen(p)) == p` with rotation, zoom, viewport and shake; `Fit` keeps the design area visible at wide and tall aspects.
- Pixel tests (`tests/e2e`): camera rotation; zoom; two cameras in split-screen viewports; an overlay camera with `CameraClear::None`; a sprite showing a render target written this frame; `RenderLayers` filtering; a sprite showing its own camera's target is skipped; pixel-art integer scaling, bars and snapping; resolution-independent output at scale factor 2; shake moves sprites and trails by the same amount; an interpolated camera renders between its fixed steps.
- GPU init errors are returned, not panics (forced with an impossible adapter request where the platform allows).
- Custom shaders: an invalid WGSL source draws plain without blocking.
- Regression: all existing tests migrated; step 1 byte-identical; later demo-tour captures compared against `main`, with differences reviewed and explained (pixel-art positions now round rather than truncate).
- Web: `cargo check -p ryo2d --target wasm32-unknown-unknown` in `./dev check` and `./dev lint`; the `lighting` demo checked in three browsers.

## Documentation

Rustdoc on every new public type. The `ryo-api` skill is the canonical reference and is rewritten for cameras, render targets and the resolution model; `ryo-renderer`, `ryo-game-dev`, `ryo-engine-dev` and `ryo-cli` (the `web` command) follow; the CLI's embedded skill copies stay verbatim. `src/core/render/AGENTS.md`, `src/core/graphics/AGENTS.md` and `src/core/AGENTS.md` describe the per-camera frame, the registry and the resolution model; `src/core/texture/AGENTS.md` goes with the module. `docs/architecture/overview.md` gets the new frame diagram.

## Design decisions

- **Cameras as entities:** viewports, ordering, interpolation and per-camera visibility fall out of the ECS; a camera following an interpolated player is smooth by the same mechanism as any sprite.
- **A camera image per camera, then a frame-level composite:** lighting and the passes keep working on whole images, so per-camera rendering needs no scissor logic in every pass; the common single full-window camera pays nothing extra.
- **Post-processing once per frame, not per camera:** effects are about the picture, and a minimap with its own vignette is rarely wanted; a camera-scope custom pass covers the exceptions.
- **Texture cameras first:** a sprite shows this frame's render target; cycles degrade to one frame of latency instead of an error.
- **Screen space as the UI and mouse space:** one coordinate system for everything not in the world; in pixel-art mode it is the art's grid, so pixel UIs line up.
- **Validate custom shaders with naga:** removes the only mid-frame blocking GPU call and is the only option on the web.
- **trunk for the web build:** one cross-platform tool for wasm-bindgen and serving, for an owner who develops on Windows and macOS.
- **The texture registry is the renderer's, not the asset system's:** integer identity on the hot path now, without pre-empting the typed asset handles of sub-project 5.

## Risks

- The wgpu 22 to 30 migration touches every GPU file; step 1 is isolated and gated on identical pixels.
- Web specifics (winit `spawn_app`, async init, browser WebGPU differences) are new to the codebase; the acceptance bar is one demo, and the wasm check in `./dev check` keeps the build from rotting.
- Lighting per camera multiplies its cost by the number of lit cameras; a minimap camera can leave lighting off by layer choice, and 2c revisits the lighting targets.
- The draw-path move touches sprites, particles, parallax, trails, lighting and overlays at once in step 5; pixel tests for each path are written before the change.
