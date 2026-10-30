# AGENTS.md - Shader System

This module provides the post-processing effect pipeline for Ryo2D, the engine's WGSL (`core/`, `post-process/`, shared `include/`s composed in `compose.rs`), plus the compiler for game-authored WGSL (`custom.rs`).

## Shared WGSL (`include/`, `compose.rs`)

- `camera.wgsl` (the `CameraGlobals` struct and its group-0 binding), `color.wgsl` (`srgb_to_linear`, `linear_to_srgb`, `premultiply`, and `ui_out` with the `UI_GAMMA` override), `sample.wgsl` (`sample_masked`: a slot-masked sample by gradient; `sample_wrapped`: a texture repeating inside its slot), `snap.wgsl` (`to_pixel`, `from_pixel`: pixel-art snapping through the camera; `pixel_of`: a clip position's target pixel), `quad.wgsl` (`quad_corner`: a quad's corner through its transform and the camera, snapped on the pixel-art target, its translation moved by `by` first, a UI scroll slot's offset and zero for a sprite, so the snap places the moved quad; `sprite.wgsl`'s `vs_main` and `ui.wgsl`'s `vs_ui` both place quads with it, so UI quads land exactly where sprites do, and `sprites_and_the_ui_place_quads_with_one_function` keeps either from copying it) and `fullscreen.wgsl` (`FullscreenVertex` and the fullscreen `vs_main`).
- **Fragment bodies shared by the world and the UI:** `tint.wgsl` (`finish`, `finish_linear`, `finish_premultiplied`, `straight_srgb`: tinting a sampled sprite texel), `glyph.wgsl` (`glyph_mask`, `glyph_color`, `glyph_msdf`), `ui_rect.wgsl` (`ui_rect`: the anti-aliased rounded rect and border) and `line_style.wgsl` (`styled`: shading across a line). The world's entries (`sprite.wgsl`, `text.wgsl`, `line.wgsl`) and the UI's `fs_ui` call the same functions, so the UI renderer draws the world's math, written once. A body takes its derivatives as arguments, so a caller takes them where control flow is uniform.
- Built-in modules are compile-time `concat!` constants of their includes and body (`compose::SPRITE`, `LINE`, `UI`, `VIGNETTE`, ...); each struct and function is written once. `compose::UI` is the UI renderer's one pipeline (`core/ui.wgsl`: every include above but `fullscreen.wgsl`, render module notes, "The UI Renderer"). `every_built_in_shader_parses_and_validates` checks every module with naga and `the_camera_struct_is_declared_once` that bodies don't redeclare the camera.
- Sprite and line fragment entries take the UV's derivatives first thing, where control flow is uniform, and pass them to the samplers, so any sample picks the right mip level.

## Ping-Pong Pattern

```
Game Texture → [Effect 1] → [Effect 2] → ... → [Effect N] → Final Output
                  ↓            ↓                   ↓
               Tex A        Tex B               Tex A/B
```

A pipeline chains effects through two intermediate textures (A and B): the first effect reads its input image and writes A, the next reads A and writes B, and so on.

## Effect Stages

Every effect runs in one `EffectStage` (`PostProcessEffect::stage()`; a custom effect takes its stage from `EffectsManager::add_custom_in`, and `add_custom` means `Display`):

- **`Hdr`**: linear light that may exceed 1, on the scene image before the `tonemap` pass (the `hdr_effects` pass): bloom, god rays, ambient occlusion, and custom effects added to this stage.
- **`Display`**: the display image the `tonemap` pass wrote, linear 0 to 1 (the `post_process` pass): fade, tint, vignette, flash, CRT, chromatic aberration, and custom effects by default.

There are two `PostProcessPipeline`s, one per format (`Rgba16Float` for `Hdr`, `Rgba8UnormSrgb` for `Display`), each with its own ping-pong pair and bloom scratch. `EffectsManager::active_effects_in(stage)`, `has_active_effects_in(stage)` and `enabled_custom_in(stage)` say what runs where. The HDR stage copies the latest scene image (lit when lighting ran) into its buffer A; its output is the image the `tonemap` pass reads.

Thresholds (bloom's, god rays', and the atmosphere's `god_rays_threshold`) are linear luminance (0.2126 R + 0.7152 G + 0.0722 B) where 1.0 is white, unbounded above. Light passes with a knee over the half threshold above the threshold, and is never clamped, so a bright source stays bright.

**Critical:** `apply_effects()` returns an `OutputBuffer` that must identify whichever texture holds the final processed output, not the input to the last effect.

## EffectParams Layout

GPU uniform struct. **Must be 64 bytes with 16-byte alignment.**

```rust
#[repr(C)]
pub struct EffectParams {
  pub color: [f32; 4],           // offset 0
  pub params: [f32; 4],          // offset 16
  pub time_resolution: [f32; 4], // offset 32 (x=time, y=width, z=height)
  pub _padding: [f32; 4],        // offset 48
}                                // Total: 64 bytes
```

`time` is the `EffectsManager` clock, advanced with real time by `update_effects_system` (`PostUpdateSet::Extract`), which also counts down timed effects, so they run through pause and slow motion.

GodRays uses a separate 128-byte `GodRaysParams` uniform instead of `EffectParams`.

## WGSL Effect Shader Contract

Every post-process shader body is composed after `include/fullscreen.wgsl`, which supplies `vs_main` and `FullscreenVertex`. It must:
- Declare a matching `EffectParams` struct (4 × `vec4<f32>`).
- Bind `@group(0) @binding(0)` input `texture_2d<f32>`, `@binding(1)` sampler, `@binding(2)` `var<uniform> EffectParams`.
- Define `fs_main(in: FullscreenVertex)` (`in.uv` runs 0..1 over the screen). The pipeline hard-codes the entry point names.
- Bind groups are made through `graphics::bind_groups::create` and looked up per pass from the chain's `BindGroupCache` (by input view and uniform buffer), so steady frames make none; a resize clears the cache.

## Custom (Game-Authored) Shaders

`custom.rs` compiles shaders written by games. A game file holds an optional `struct Params { .. }` and one function, `fn sprite(in: SpriteIn, params: Params) -> vec4<f32>` or `fn effect(in: EffectIn, params: Params) -> vec4<f32>`. The compiler prepends the engine prelude, appends a generated entry point (`fs_custom` and `fs_custom_premultiplied` for sprites, `fs_main` after the shared fullscreen `vs_main` for effects), validates the result with naga and reflects `Params` (members `f32`/`vec2`/`vec3`/`vec4<f32>`; at most 32 bytes for sprites, 64 for effects). Without a `Params` struct the prelude supplies `struct Params { unused: f32 }`. Error positions are mapped back to the game's own line and column. Game shaders are `Shader` assets (`ecs/material.rs`: loaded from `.wgsl` files by `ShaderLoader`, named by their asset path, or added with `Shader::from_wgsl`). `ShaderLoader` compiles the file on the loader's thread (`Shader::check`, through `custom::check_with`: as a sprite shader if it defines `fn sprite`, as an effect if it defines `fn effect`; one defining neither fails) and keeps what it compiled with the shader (`Shader::compiled`, made once per kind and shared by the value's clones; a shader made in code compiles on first use), so the renderer does not compile it again (`custom::compile`); wgpu still parses and validates the composed WGSL, with naga, as it makes the module (passing the naga module would take wgpu's `naga-ir` feature). A file that does not compile fails its load like any other asset: a first load is `Failed`, a reload is `ReloadFailed` and keeps the last good shader, and the asset server warns once, `could not load "<path>": <line>:<column>: <message>`. A version that fails in the renderer anyway (one made in code, or a pipeline the GPU rejects, such as a binding the pipeline layout lacks) draws the shader's last version the GPU accepted, which the cache holds until a newer one is accepted (`ShaderCache`: `accepted`, `rejected`, from the pipelines' checks); it is warned about once per shader and version in the same words, from the `asset` module. A shader with no accepted version draws as one that is not loaded: a sprite plain, an effect skipped. The sprite renderer and each effect stage keep theirs in a `ShaderCache` (`cache.rs`): an entry per shader id for the version last drawn, pointing at that version's source, compiled once per `custom::source_key` (kind + source text), so shaders with equal sources compile once and their sprites batch together. A freed shader's entry goes on its `AssetEvent::Removed` (read at extract), and a source no shader uses loses its pipelines in the same frame (the batch's `retain`, an effect's own); its compiled WGSL stays until 600 frames unused, for a shader of the same text added again. The 2D batch makes a sprite shader's pipeline per blend mode it is drawn with (checked by the GPU without blocking; the sprite draws plain until the check passes, and its pipelines go with its source). A check's outcome goes back to the sprite renderer's cache (`Pipelines::settle`, `SpriteGpuRenderer::custom_shader_settled`), and an effect stage settles its own.

**A new version compiles off the main thread** (`compile.rs`). Metal compiles a pipeline in the call that makes it, 70 to 130 ms for WGSL it has not seen, so a save used to stall the frame it landed in.
- A shader drawn with a source the GPU accepted keeps drawing it when its version moves on. The new version waits in its entry (`Entry::next`, counted in `waiting`, so `in_use` keeps what is made for it), and the renderer makes its module and pipelines on a thread of its own through the shared `Device` (`Batch2D::compile_next`, `CustomEffectPipelines::compile_next`), inside a validation error scope popped there (`compile::checked`), so a rejection is known before anything draws it.
- What came back is taken in at the start of a frame, before anything draws (`Batch2D::sync_shaders` and `CustomEffectPipelines::take_compiled`, from extract): the pipelines go in accepted, and `ShaderCache::advance` moves each waiting shader to its new source once that is made for every key its old source has (the batch's `custom_keys`; an effect's one pipeline). A new version that does not compile, or that the GPU rejects, moves the shader to the failed state above (its last good version draws, warned once per shader and version). No frame draws a sprite plain or skips an effect for a save.
- Twins: when the new source comes back while a pipeline the main thread made from the same text still has its check out (a twin shader of that text compiled at once, or a blend mode first drawn), the source is not taken as accepted until that check is back (`landed`'s `checking`: the batch's `Pipelines::checking`, an effect's pending pipeline). The shaders moving to it keep their last good version meanwhile, so a rejection by that check still finds it.
- Only such a shader waits. A first compile, and a new version of a shader with no accepted version, compile at once on the main thread as before, so first frames (and the tour) are unchanged; a shader first drawn during play still compiles in that frame. Going back to the text it draws waits for nothing.
- A blend mode or page kind first drawn while the new version compiles is made for the old source at once, as any first use is (drawn plain that frame), and the new version waits until it is made for that key too (a second job). So all of a shader's sprites take the new version in one frame, and none makes a pipeline on the main thread after the switch.
- One job is out per renderer (`Compiler::busy`); the next waits in the cache, at most one per shader, so repeated saves queue nothing. A version saved over while it compiles, or whose shader is freed, is dropped when it comes back (`landed` keeps only a source a shader uses or waits for).
- Each renderer (the batch, each effect stage) has its own thread (`ryo2d shader compiler`), started with its first job, rather than the asset workers: a compile holds its thread for up to 130 ms, which would hold up loads, and the renderers own the pipelines and the device, which the workers cannot reach. It ends once its renderer is dropped and its job is done; nothing joins it, so shutdown never waits on a compile. A job that panics comes back empty, and each shader waiting for that version gives it up (warned once) and draws its last good version (`ShaderCache::panicked`); the source is left as it is, since another shader may hold it.
- The web has no threads to compile on: there every version compiles at once on the main thread, as before (`compile::OFF_THREAD`).
- The cache's sweep (every 120 frames) forgets only shaders no longer loaded, so a shader changed while it went undrawn keeps its last good version until its new one is made.

Custom effect bind group (group 0): `0` scene texture, `1` sampler, `2` params uniform, `3` globals (time, resolution). An effect's `in.time` is the effects clock (real time); a sprite shader's `in.time` is the sprite globals' `x`, game time, so it freezes under pause. `scene(uv)` samples the latest image in LINEAR colour (above 1 in an `Hdr` effect) and the effect returns linear colour. Effect `Color` params are sRGB bytes/255 (`From<Color> for ParamValue`), so an effect that mixes one with `scene(uv)` decodes it first (`srgb_to_linear`). Sprite shaders work in the sprite's sRGB values and return straight colour; the engine tints, rounds, converts to linear and premultiplies afterwards.

`EffectsManager::add_custom/set_custom_param/set_custom_enabled/remove_custom` hold the custom effects. They run after the built-in effects of their stage, in insertion order, each as one ping-pong slot, and `clear()` removes them. Params are packed by name from `ShaderParams` at emit time; `Color` packs as sRGB/255. The built-in display effects' colours (fade, tint, flash) are decoded to linear before they reach their shaders. A reloaded shader (the registry's watcher reloads `.wgsl` files) has a new version: one drawn with a version the GPU accepted keeps drawing it while the new one compiles off the main thread (above); any other compiles the next frame it is drawn.

## Sprite Shader: Linear Pages and Emission

- `sprite.wgsl` draws onto `Rgba16Float` images and writes linear colour premultiplied by alpha. Entry points: `fs_base` / `fs_effect` (atlas and standalone pages: raw sRGB bytes, tinted in sRGB, then decoded), `*_premultiplied` (premultiplied textures) and `fs_base_linear` / `fs_effect_linear` for render-target pages, which hold linear premultiplied colour (`PageKind::Linear`). The linear entries set `linear_page`, so `sample_sprite` hands the texel through `straight_srgb` (un-premultiply, re-encode to sRGB) to the sprite effects, which work in sRGB; `finish_linear` multiplies the linear colour by the decoded tint directly. `compose::SPRITE` also holds `core/tile.wgsl`: `vs_tile`, the tile chunks' vertex stage, which places a chunk's instances through its tile slot (group 2) and steps animated tiles on `game_time`, before the sprite's fragment entries (render module notes, Tile chunks). It snaps the slot's origin with `to_pixel`, a rule of its own, so it lives apart from `sprite.wgsl`, which must leave snapping to `quad_corner`.
- **Emission** rides in the instance's `transform0.w` as `emission - 1` (a zeroed lane means 1, no glow); the vertex shader adds the 1 back and `finish`, `finish_premultiplied` and `finish_linear` multiply the colour (not the alpha) by it. Sprite tints and sprite-effect colours stay sRGB by design.

## Multi-Pass Effects

**Bloom** runs three internal passes using two half-resolution scratch textures, bloom A and B:

```
Input in buffer X (scene)
1. Extract bright pixels:  X -> A (each texel averages 2x2 scene texels, thresholded one by one)
2. For each blur_pass:
   a. Blur H:  A -> B
   b. Blur V:  B -> A
3. Composite:  X (scene) + A (bilinear upsample) -> Y
Output in buffer Y
```

Each blur pass is a Gaussian with sigma = 1.7 x radius screen pixels (`BLOOM_SIGMA_PER_RADIUS`), the spread of the original 9-tap kernel, sampled at every half-resolution texel out to 3 sigma with paired bilinear taps. Half resolution keeps wide glows cheap: at the default radius 5 a pass costs fewer samples per screen pixel than the old 9 taps.

Bloom consumes one slot in the A/B ping-pong chain. Its three pipelines use different bind group layouts:
- **Extract**: standard layout (1 texture + sampler + uniform)
- **Blur**: standard layout (direction controlled by `params.w`: 0.0=H, 1.0=V)
- **Composite**: custom layout (2 textures + sampler + uniform) via `bloom_composite_bind_group_layout`

## Adding a New Effect

1. `effects.rs`: add a `PostProcessEffect` variant and constructor. Add an arm in `EffectParams::from_effect()` returning its `(color, params)` tuple.
2. Create `post-process/<name>.wgsl` following the contract above. Register it as a const in `post_process_shaders` (`mod.rs`).
3. `pipeline.rs`: add an `Option<EffectPipeline>` field, a lazy-creation arm in `ensure_pipeline_exists()`, and an arm in `get_pipeline()`.
4. `manager.rs`: add an `EffectCategory` variant and `EffectCategory::from_effect()` arm. Add an `EffectsManager` convenience method that calls `set_effect()`.

## Common Issues

- **Buffer alignment errors**: WGSL requires 16-byte alignment for uniform struct members. `array<f32, N>` has stride 4 and fails validation. Always pad with `vec4<f32>`.
- **Effects not visible**: check the ping-pong output tracking in `pipeline.rs`.
- **Texture usage flags**: ping-pong textures need `RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_DST | COPY_SRC` for copying between them.
- **Passes reading each other's settings**: `queue.write_buffer` calls all land before the frame's commands run, so passes sharing one uniform buffer all read the last value written. Take one buffer per pass per frame from `UniformPool` (`write` returns the buffer index to bind).
