# AGENTS.md - Shader System

This module provides the post-processing effect pipeline for Ryo2D, the engine's WGSL (`core/`, `post-process/`, shared `include/`s composed in `compose.rs`), plus the compiler for game-authored WGSL (`custom.rs`).

## Shared WGSL (`include/`, `compose.rs`)

- `camera.wgsl` (the `CameraGlobals` struct and its group-0 binding), `color.wgsl` (`srgb_to_linear`, `linear_to_srgb`, `premultiply`), `sample.wgsl` (`sample_masked`: a slot-masked sample by gradient; `sample_wrapped`: a texture repeating inside its slot) and `fullscreen.wgsl` (`FullscreenVertex` and the fullscreen `vs_main`).
- Built-in modules are compile-time `concat!` constants of their includes and body (`compose::SPRITE`, `LINE`, `VIGNETTE`, ...); each struct and function is written once. `every_built_in_shader_parses_and_validates` checks every module with naga and `the_camera_struct_is_declared_once` that bodies don't redeclare the camera.
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

`custom.rs` compiles shaders written by games. A game file holds an optional `struct Params { .. }` and one function, `fn sprite(in: SpriteIn, params: Params) -> vec4<f32>` or `fn effect(in: EffectIn, params: Params) -> vec4<f32>`. The compiler prepends the engine prelude, appends a generated entry point (`fs_custom` and `fs_custom_premultiplied` for sprites, `fs_main` after the shared fullscreen `vs_main` for effects), validates the result with naga and reflects `Params` (members `f32`/`vec2`/`vec3`/`vec4<f32>`; at most 32 bytes for sprites, 64 for effects). Without a `Params` struct the prelude supplies `struct Params { unused: f32 }`. Error positions are mapped back to the game's own line (`<id>:<line>:<column>: <message>`), logged once per source; a failing sprite shader draws plain and a failing effect is skipped. Compiled sprite shaders are cached in the sprite renderer by `custom::source_key` (kind + source text), so handles with equal sources compile once and their sprites batch together; per-handle entries (by `ShaderHandle::serial()` and generation) point at them, and both are pruned after 600 unused frames. The 2D batch makes a sprite shader's pipeline per blend mode it is drawn with (checked by the GPU without blocking; the sprite draws plain until the check passes, and its pipelines go with its source).

Custom effect bind group (group 0): `0` scene texture, `1` sampler, `2` params uniform, `3` globals (time, resolution). An effect's `in.time` is the effects clock (real time); a sprite shader's `in.time` is the sprite globals' `x`, game time, so it freezes under pause. `scene(uv)` samples the latest image in LINEAR colour (above 1 in an `Hdr` effect) and the effect returns linear colour. Effect `Color` params are sRGB bytes/255 (`From<Color> for ParamValue`), so an effect that mixes one with `scene(uv)` decodes it first (`srgb_to_linear`). Sprite shaders work in the sprite's sRGB values and return straight colour; the engine tints, rounds, converts to linear and premultiplies afterwards.

`EffectsManager::add_custom/set_custom_param/set_custom_enabled/remove_custom` hold the custom effects. They run after the built-in effects of their stage, in insertion order, each as one ping-pong slot, and `clear()` removes them. Params are packed by name from `ShaderParams` at emit time; `Color` packs as sRGB/255. The built-in display effects' colours (fade, tint, flash) are decoded to linear before they reach their shaders. Hot reload updates the `ShaderHandle` source and bumps its generation, which triggers a rebuild the next frame.

## Sprite Shader: Linear Pages and Emission

- `sprite.wgsl` draws onto `Rgba16Float` images and writes linear colour premultiplied by alpha. Entry points: `fs_base` / `fs_effect` (atlas and standalone pages: raw sRGB bytes, tinted in sRGB, then decoded), `*_premultiplied` (premultiplied textures) and `fs_base_linear` / `fs_effect_linear` for render-target pages, which hold linear premultiplied colour (`PageKind::Linear`). The linear entries set `linear_page`, so `sample_sprite` hands the texel through `straight_srgb` (un-premultiply, re-encode to sRGB) to the sprite effects, which work in sRGB; `finish_linear` multiplies the linear colour by the decoded tint directly.
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
