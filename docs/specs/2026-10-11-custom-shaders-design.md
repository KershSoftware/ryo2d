# Custom Shaders - Design

## Overview

Games can write their own WGSL for sprites and for full-screen post-process effects, load shaders as assets with hot reload, and never crash from a bad shader. Renderer sub-project 3 (owner decisions, 2026-10-11): **custom sprite shaders, custom post-process effects, and shader loading / hot reload / safe errors**; a game writes **one function**, the engine supplies the rest. Richer raw render passes (camera/time/scratch targets for `RenderPass`) are out of scope.

**Why:** `SpriteShader::Custom` and `Material` exist but nothing renders them (`render/entities.rs` maps only built-in effects); `ShaderParams` is floats packed alphabetically into 16 slots; `assets.load_shader` is documented but doesn't exist; post-process effects are a closed enum that needs engine edits to extend; and a WGSL error would hit wgpu's default error handler, which panics.

**Out of scope:** extra texture inputs for custom shaders; custom vertex stages; ordering custom effects among the built-ins; exposing the pre-lighting image to effects.

## Custom sprite shaders

A sprite shader file declares a `Params` struct and one function:

```wgsl
struct Params { amount: f32, edge: vec4<f32> }

fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  let c = sample(in.uv);
  ...
}
```

- `SpriteIn { uv: vec2<f32> /* 0..1 across the sprite */, size: vec2<f32> /* pixels */, screen_pos: vec2<f32>, time: f32 }`.
- `sample(uv)` samples the sprite (atlas-safe: transparent outside the sprite's own rect).
- The returned colour is in the same space as `sample` (sRGB values, straight alpha). The engine then applies the instance tint, the CPU-matching quantisation and the linear conversion (`finish`), exactly as for built-in sprites.
- `Params` members may be `f32`, `vec2<f32>`, `vec3<f32>`, `vec4<f32>`; total size (WGSL layout) ≤ 32 bytes. The struct may be omitted (no params).
- Rust: `SpriteShader::custom(handle)` with `.with_param(name, value)`; values `f32`, `Vec2`, `Vec3`, `Vec4`, `Color` (as sRGB/255 like tint). Used via `Renderable::with_shader(..)`, a `Material` component (wins over `Renderable::shader` when both exist), or `render_gpu` sprites (`BatchCollector::add_custom`).
- Per-sprite param values ride in the instance's `effect_params`/`effect_extra` (8 floats); the engine generates WGSL that rebuilds `Params` from them using the reflected layout.
- One pipeline per shader version, cached; draw runs split by pipeline. Custom shaders draw within the sprite's quad.

## Custom post-process effects

```wgsl
struct Params { strength: f32, tint: vec4<f32> }

fn effect(in: EffectIn, params: Params) -> vec4<f32> {
  let c = scene(in.uv);
  ...
}
```

- `EffectIn { uv, time, resolution }`; `scene(uv)` samples the latest image (clamped).
- `Params` ≤ 64 bytes, in a per-pass uniform from the post-process `UniformPool`; time and resolution in a second per-pass uniform.
- Rust (`EffectsManager`): `add_custom(name, handle) -> Result<(), EffectError>` (duplicate name), `set_custom_param(name, param, value)`, `set_custom_enabled(name, bool)`, `remove_custom(name)`, `has_custom(name)`.
- Custom effects run after the built-in effects, in the order added, in the same ping-pong chain.

## Shader assets, hot reload, safe errors

- `assets.load_shader(path) -> AssetResult<ShaderHandle>` resolves like sprites and caches by path; only IO errors are `Err`. `ShaderHandle::from_source(id, wgsl)` stays for inline shaders.
- `ShaderHandle` shares its source behind a lock with a serial (identity) and a generation bumped on every change; `PartialEq` compares serial and generation.
- With hot reload enabled, a modified `.wgsl` file reloads into its handle (`AssetManager::process_reloads`); pipelines rebuild when they see a new generation.
- Compilation: the engine assembles the full WGSL (engine prelude + the game's code + a generated entry point) and validates it with naga (the version wgpu uses) before creating anything. The `Params` layout is reflected from the validated module.
- Errors (parse, validation, missing `sprite`/`effect` function, unsupported param types, size limits, wgpu errors caught by an error scope around pipeline creation) are logged once per shader version to the dev log with the game's file/id and the line and column **in the game's code**. The sprite then draws with the plain shader; the effect is skipped. Unknown param names are warned once per (shader, name). Nothing panics; fixing the file under hot reload recovers it.

## Testing

- **Unit (no GPU):** layout reflection (offsets/size per WGSL rules: `{a: f32, b: vec4}` → 0/16/32; `vec3` aligns to 16); size limits; missing function; unsupported member type; error line mapping into the game's code; packing by name; unknown name reported; `Color` packs as sRGB/255.
- **Headless (GPU):** a colour-inverting custom shader changes exactly its sprite's pixels (opaque, white tint: exact `255 - c`); a param changes output; same shader on a `render_gpu` sprite; `Material` wins over `Renderable::shader`; two shaders split runs (`DebugStats.draw_calls`); broken WGSL renders the plain sprite, logs one error naming the line, and doesn't panic; replacing a handle's source changes the next frame; a custom grayscale effect with a `strength` param; a custom effect after a built-in vignette (order); a broken effect is skipped and logged; removing/disabling an effect.

## Documentation

Renderer and API skills ("Custom shaders": the two templates, params, loading, hot reload, errors), `src/core/ecs/material.rs` docs (fix the stale `load_shader`/`with_param` examples), `src/core/shaders/AGENTS.md` (custom effects; generated entry points), the sprite-shaders demo (a custom dissolve shader asset) and the post-processing demo (a custom effect asset).

## Design decisions

- **One function, engine-owned wrapper:** short, agent-friendly code that keeps atlas masking, tint and colour handling correct and survives engine changes.
- **Params reflected from WGSL:** one source of truth (the struct the shader already declares); names checked at runtime.
- **naga validation before wgpu:** readable errors with real line numbers, and no panic path; the error scope is a backstop.
