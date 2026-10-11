# AGENTS.md - Shader System

This module provides the post-processing effect pipeline for Ryo2D, plus the compiler for game-authored WGSL (`custom.rs`).

## Ping-Pong Pattern

```
Game Texture → [Effect 1] → [Effect 2] → ... → [Effect N] → Final Output
                  ↓            ↓                   ↓
               Tex A        Tex B               Tex A/B
```

The pipeline chains effects through two intermediate textures (A and B): the first effect reads the game texture and writes A, the next reads A and writes B, and so on.

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
}                                  // Total: 64 bytes
```

GodRays uses a separate 128-byte `GodRaysParams` uniform instead of `EffectParams`.

## WGSL Effect Shader Contract

Every post-process shader is self-contained. It must:
- Declare a matching `EffectParams` struct (4 × `vec4<f32>`).
- Bind `@group(0) @binding(0)` input `texture_2d<f32>`, `@binding(1)` sampler, `@binding(2)` `var<uniform> EffectParams`.
- Define its own fullscreen `vs_main` (6-vertex quad from `vertex_index`) and `fs_main`. The pipeline hard-codes these entry point names.

## Custom (Game-Authored) Shaders

`custom.rs` compiles shaders written by games. A game file holds an optional `struct Params { .. }` and one function, `fn sprite(in: SpriteIn, params: Params) -> vec4<f32>` or `fn effect(in: EffectIn, params: Params) -> vec4<f32>`. The compiler prepends the engine prelude, appends a generated entry point (`fs_custom` for sprites, a generated `vs_main`/`fs_main` pair for effects), validates the result with naga and reflects `Params` (members `f32`/`vec2`/`vec3`/`vec4<f32>`; at most 32 bytes for sprites, 64 for effects). Without a `Params` struct the prelude supplies `struct Params { unused: f32 }`. Error positions are mapped back to the game's own line (`<id>:<line>:<column>: <message>`), logged once per shader version; a failing sprite shader draws plain and a failing effect is skipped.

Custom effect bind group (group 0): `0` scene texture, `1` sampler, `2` params uniform, `3` globals (time, resolution). `scene(uv)` samples the latest image in LINEAR colour and the effect returns linear colour. Sprite shaders work in the sprite's sRGB values; the engine tints, rounds and converts to linear afterwards.

`EffectsManager::add_custom/set_custom_param/set_custom_enabled/remove_custom` hold the custom effects. They run after the built-in effects, in insertion order, each as one ping-pong slot, and `clear()` removes them. Params are packed by name from `ShaderParams` at emit time; `Color` packs as sRGB/255. Hot reload updates the `ShaderHandle` source and bumps its generation, which triggers a rebuild the next frame.

## Multi-Pass Effects

**Bloom** runs three internal passes using a dedicated `bloom_texture` as scratch space:

```
Input in buffer X (scene)
1. Extract bright pixels:  X -> bloom_texture
2. For each blur_pass:
   a. Blur H:  bloom_texture -> Y (output buffer, used as temp)
   b. Blur V:  Y -> bloom_texture
3. Composite:  X (scene) + bloom_texture (blurred bloom) -> Y
Output in buffer Y
```

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
