# Custom Shaders Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Games write one WGSL function for a custom sprite shader or a custom post-process effect, load it as an asset with hot reload, and a bad shader logs a readable error and falls back instead of crashing.

**Architecture:** `src/core/shaders/custom.rs` assembles engine prelude + game code + a generated entry point, validates it with naga, reflects the `Params` struct and maps error lines back to the game's code. The sprite renderer caches one pipeline per custom shader version (params ride in the instance's 8 effect floats); the post-process pipeline runs registered custom effects after the built-ins (params in a per-pass uniform bound as the game's own `Params` struct). `ShaderHandle` becomes a shared, generation-counted asset loaded by `AssetManager::load_shader` and refreshed by hot reload.

**Tech Stack:** Rust 2024, wgpu 22, naga 22 (`wgsl-in`), pollster.

**Spec:** `docs/plans/2026-10-11-custom-shaders-design.md`

## Global Constraints

- Rust 2024, 2-space indentation, no `unwrap`/`expect`/`panic!` in library code (tests may).
- Format only lines you change; never plain-rustfmt a file declaring `mod x;` children (`src/lib.rs`, any `mod.rs`): `rustfmt --edition 2024 --check` and fix only your hunks. New files may be fully formatted.
- Line endings: check `file <path>` before editing; keep each file's endings on every line (CRLF today include `src/core/shaders/core/sprite.wgsl`, `src/core/scene.rs`, `src/core/render/mod.rs`, all skill files). New files LF.
- Limits: sprite `Params` ≤ 32 bytes, effect `Params` ≤ 64 bytes; member types `f32`, `vec2<f32>`, `vec3<f32>`, `vec4<f32>` only.
- Function signatures: `fn sprite(in: SpriteIn, params: Params) -> vec4<f32>`, `fn effect(in: EffectIn, params: Params) -> vec4<f32>`; if the game declares no `Params`, the engine supplies `struct Params { unused: f32 }`.
- Errors are logged once per shader version (`serial`, `generation`) through `crate::engine_error!(module: "Shaders", …)`; never panic.
- Breaking API is allowed (fix callers). One commit per task, no trailer lines, don't push.
- Done = `cargo test --workspace` 0 failed; `cargo clippy --workspace --all-targets` no new warnings in touched files; `cargo check --workspace --all-targets` builds.

## Review Focus

1. A shader with a syntax error, an unknown function, or a param over the limit never panics, and the error names the line in the game's file — Task 1 `errors_point_at_the_games_line`, Task 2 `broken_sprite_shader_falls_back`, Task 3 `broken_effect_is_skipped`.
2. A custom sprite shader keeps atlas masking (no neighbour pixels) and the tint pipeline (tinted custom sprite matches the CPU-style quantisation) — Task 2 `custom_shader_samples_only_its_sprite`, `tint_applies_after_the_custom_shader`.
3. Editing a shader at runtime (hot reload / source replace) takes effect next frame without a restart — Task 2 `replaced_source_changes_the_next_frame`.
4. Params set by name reach the shader in the right slots for mixed types (f32 + vec4, vec3 alignment) — Task 1 layout/packing tests, Task 2 `params_reach_the_shader`.
5. Custom effects stack with built-ins in a defined order — Task 3 `custom_effect_runs_after_built_ins`.

---

### Task 1: Shader assets, assembly, validation and params

**Files:**
- Modify: `Cargo.toml` (`naga = { version = "22", features = ["wgsl-in"] }` - the version wgpu 22 uses; check with `cargo tree -i naga`)
- Modify: `src/core/ecs/material.rs` (`ShaderHandle` shared + generation; typed `ShaderParams`; `SpriteShader::custom`, `with_param`; fix the module docs' stale examples)
- Modify: `src/core/asset/manager.rs` (`load_shader`, cache by path, hot reload of `.wgsl` in `process_reloads`)
- Create: `src/core/shaders/custom.rs` (+ `mod custom;` and crate-internal re-exports in `src/core/shaders/mod.rs`)

**Interfaces:**

```rust
// material.rs (public)
#[derive(Clone)]
pub struct ShaderHandle { inner: Arc<ShaderSourceCell> }   // shared; Debug prints id + generation
impl ShaderHandle {
  pub fn from_source(id: impl Into<String>, source: impl Into<String>) -> Self;
  pub fn id(&self) -> &str;
  pub fn source(&self) -> String;               // current source (clone)
  pub fn generation(&self) -> u64;
  pub(crate) fn serial(&self) -> u64;           // identity, from a global counter
  pub(crate) fn path(&self) -> Option<&Path>;   // set by load_shader
  pub fn replace_source(&self, source: impl Into<String>);  // bumps generation (used by hot reload; public so tools/tests can update inline shaders)
}
impl PartialEq for ShaderHandle  // serial && generation

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamValue { F32(f32), Vec2([f32; 2]), Vec3([f32; 3]), Vec4([f32; 4]) }
// From<f32>, From<glam::Vec2>, From<glam::Vec3>, From<glam::Vec4>, From<Color> (sRGB bytes / 255, alpha / 255)

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShaderParams { values: Vec<(String, ParamValue)> }  // insertion order, later set replaces
impl ShaderParams { pub fn new() -> Self; pub fn set(&mut self, name: impl Into<String>, value: impl Into<ParamValue>); pub fn with(self, name, value) -> Self; pub fn get(&self, name: &str) -> Option<ParamValue>; }
// Remove to_uniform_array (alphabetical packing); fix callers.

impl SpriteShader {
  pub fn custom(shader: ShaderHandle) -> Self;          // Custom { shader, params: default }
  pub fn with_param(self, name: impl Into<String>, value: impl Into<ParamValue>) -> Self; // no-op on BuiltIn
}
// Material keeps `shader: SpriteShader`; add `Material::custom(handle)` + `with_param` if missing.

// asset/manager.rs
pub fn load_shader<P: AsRef<Path>>(&mut self, path: P) -> AssetResult<ShaderHandle>; // id = resolved path string

// shaders/custom.rs (crate-internal)
pub(crate) enum ShaderKind { Sprite, Effect }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParamKind { F32, Vec2, Vec3, Vec4 }
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParamField { pub(crate) name: String, pub(crate) offset: u32, pub(crate) kind: ParamKind }
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParamLayout { pub(crate) fields: Vec<ParamField>, pub(crate) size: u32 }
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShaderError { pub(crate) shader: String, pub(crate) line: Option<u32>, pub(crate) column: Option<u32>, pub(crate) message: String }
impl std::fmt::Display for ShaderError // "{shader}:{line}:{column}: {message}" (omit missing parts)

/// The validated, ready-to-compile module for a game shader.
pub(crate) struct CompiledShader { pub(crate) wgsl: String, pub(crate) layout: ParamLayout }

/// Assemble prelude + game code + generated entry, validate with naga, reflect Params.
pub(crate) fn compile(kind: ShaderKind, id: &str, user_source: &str) -> Result<CompiledShader, ShaderError>;

/// Pack named values into the layout's bytes (len = layout.size rounded up to 16 for effects, exactly 32 for sprites); unknown names returned for warning.
pub(crate) fn pack(layout: &ParamLayout, params: &ShaderParams, out: &mut [u8]) -> Vec<String>;

/// Log a shader error once per (serial, generation); returns whether it was logged now.
pub(crate) fn report_once(serial: u64, generation: u64, error: &ShaderError) -> bool;
```

**Assembly:**
- Sprite: `SPRITE_SHADER` (the existing `sprite.wgsl`, which already has `vs_main`, `sample_masked`, `texel_size`, `finish`) + the sprite prelude below + `[struct Params { unused: f32 }` if the game's code declares no `Params`] + game code + the generated entry:

```wgsl
// prelude (engine)
struct SpriteIn { uv: vec2<f32>, size: vec2<f32>, screen_pos: vec2<f32>, time: f32 }
var<private> ry_bounds: vec4<f32>;
fn sample(uv: vec2<f32>) -> vec4<f32> {
  return sample_masked(ry_bounds.xy + uv * (ry_bounds.zw - ry_bounds.xy), ry_bounds);
}

// generated entry (after the game's code); FIELDS built from the reflected layout
@fragment
fn fs_custom(in: VertexOutput) -> @location(0) vec4<f32> {
  ry_bounds = in.uv_bounds;
  let extent = in.uv_bounds.zw - in.uv_bounds.xy;
  let local = (in.uv - in.uv_bounds.xy) / extent;
  let size = abs(extent) / texel_size();
  let f = array<f32, 8>(in.effect_params.x, in.effect_params.y, in.effect_params.z, in.effect_params.w,
                        in.effect_extra.x, in.effect_extra.y, in.effect_extra.z, in.effect_extra.w);
  let params = Params(FIELDS); // e.g. f[0], vec4<f32>(f[4], f[5], f[6], f[7])
  return finish(sprite(SpriteIn(local, size, in.position.xy, globals.time_resolution.x), params), in.tint);
}
```

  (Check `finish`'s real signature/name in `sprite.wgsl` and adapt; `sample_masked`'s argument order too.)
- Effect: a self-contained module:

```wgsl
struct EffectGlobals { time_resolution: vec4<f32> }
@group(0) @binding(0) var ry_scene: texture_2d<f32>;
@group(0) @binding(1) var ry_sampler: sampler;
@group(0) @binding(2) var<uniform> ry_params: Params;
@group(0) @binding(3) var<uniform> ry_globals: EffectGlobals;
struct EffectIn { uv: vec2<f32>, time: f32, resolution: vec2<f32> }
fn scene(uv: vec2<f32>) -> vec4<f32> { return textureSample(ry_scene, ry_sampler, clamp(uv, vec2(0.0), vec2(1.0))); }
struct RyVsOut { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> RyVsOut { /* full-screen triangle or the quad the built-ins use (src/core/shaders/core/fullscreen_quad.wgsl) */ }
// [struct Params { unused: f32 } if absent] + game code, then:
@fragment fn fs_main(in: RyVsOut) -> @location(0) vec4<f32> {
  return effect(EffectIn(in.uv, ry_globals.time_resolution.x, ry_globals.time_resolution.yz), ry_params);
}
```

**Compile steps:** detect a game-declared `Params` by parsing `prelude + game code` with naga and looking for a struct type named `Params` (if that parse fails, report the error, line-mapped); generate the entry; parse + validate the full module (`naga::valid::Validator::new(ValidationFlags::all(), Capabilities::empty())`); check the `sprite`/`effect` function exists with the signature; reflect `Params` members (names, offsets from `naga::proc::Layouter`, kinds; anything else → error), enforce the size limit; map error spans to lines/columns and subtract the line offset of the game's code (an error outside the game's code is reported with `line: None` and the message prefixed "in engine-generated code:").

- [ ] **Step 1: Write the failing tests** (`custom.rs` unit tests, no GPU):
  - `layout_follows_wgsl_rules` (`struct Params { a: f32, b: vec4<f32> }` → a@0 F32, b@16 Vec4, size 32; `{ a: vec3<f32>, b: f32 }` → a@0, b@12, size 16)
  - `sprite_params_over_32_bytes_are_rejected`, `effect_params_up_to_64_bytes_are_accepted`
  - `missing_entry_function_is_an_error` (`fn not_sprite…`)
  - `unsupported_param_type_is_an_error` (`struct Params { m: mat2x2<f32> }`)
  - `errors_point_at_the_games_line` (game code with a typo on its line 3 → `line == Some(3)`)
  - `shaders_without_params_compile` (no struct, function uses `params.unused` not at all)
  - `pack_places_values_by_name` (f32 + vec4 bytes at 0 and 16; unknown name returned; `Color::rgb(255, 128, 0)` → `[1.0, 128/255, 0.0, 1.0]`)
  - `report_once_logs_once_per_version`
  - In `material.rs`: `replace_source_bumps_generation`, `clones_share_identity`.
  - In `manager.rs` (or an integration test with a temp dir): `load_shader_caches_by_path` and `missing_shader_file_is_an_error`.
- [ ] **Step 2:** watch them fail.
- [ ] **Step 3: Implement** as specified. Hot reload: in `process_reloads`, a modified path matching a cached shader → `handle.replace_source(new_text)`.
- [ ] **Step 4:** run them; `cargo test --lib shaders::custom`.
- [ ] **Step 5: Suite, lint, check, commit:** `feat(shaders): compile game shaders with readable errors and typed params`.

---

### Task 2: Custom sprite shaders in the renderer

**Files:**
- Modify: `src/core/graphics/sprite_gpu.rs` (custom pipeline cache keyed by handle serial → (generation, `Option<wgpu::RenderPipeline>`, `ParamLayout`); compile on first use or new generation with `custom::compile`, `device.push_error_scope(Validation)` around `create_shader_module`/`create_render_pipeline` and `pollster::block_on(device.pop_error_scope())`; on any error `report_once` and treat as plain; draw runs keyed by (page, pipeline: Plain | Effect | Custom(serial)); `BatchCollector::add_custom(texture_id, shader: &ShaderHandle, params: &ShaderParams, instance, z_order)` packing params into `effect_params`/`effect_extra`)
- Modify: `src/core/render/entities.rs` (an entity's `Material` component wins over `Renderable::shader`; `SpriteShader::Custom` → custom draw item with packed params)
- Modify: `src/core/render/draw_list.rs` if items need the custom shader key
- Test: `tests/e2e/custom_shaders.rs` (register in `tests/e2e.rs`; CRLF check)

- [ ] **Step 1: Write the failing tests** (headless, public API):
  - `custom_shader_inverts_its_sprite` (opaque sprite colour c, shader returns `vec4(1.0 - c.rgb, c.a)`: its pixels are exactly `255 - c`; neighbouring background unchanged)
  - `params_reach_the_shader` (`struct Params { a: f32, color: vec4<f32> }`, shader returns `params.color` where `sample(uv).a > 0`; set `color` to (10, 200, 90): exact pixels; set `a` to 0.0/1.0 changes behaviour per the shader)
  - `custom_shader_samples_only_its_sprite` (two sprites sharing an atlas page; shader returns `sample(in.uv + vec2(0.2, 0.0))` near the edge: no pixel of the neighbour's colour)
  - `tint_applies_after_the_custom_shader` (identity shader + tint (130 on a 200 source) → CPU formula)
  - `material_wins_over_renderable_shader`
  - `render_gpu_sprites_can_use_custom_shaders` (`BatchCollector::add_custom` via `GpuRenderContext`)
  - `two_shaders_split_draw_runs` (crate-internal if `DebugStats` isn't public)
  - `broken_sprite_shader_falls_back` (syntax error: the sprite draws as plain; one dev-log error entry containing the shader id and `:2:` (its line); no panic; filter `dev_log::get_entries_since` by the unique shader id)
  - `replaced_source_changes_the_next_frame` (`handle.replace_source(...)` between steps → output changes)
- [ ] **Step 2:** watch them fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** run them + full e2e.
- [ ] **Step 5: Suite, lint, check, commit:** `feat(graphics): render custom sprite shaders`.

---

### Task 3: Custom post-process effects

**Files:**
- Modify: `src/core/shaders/manager.rs` (`EffectsManager` custom registry: `add_custom(name, handle) -> Result<(), EffectError>`, `set_custom_param(name, param, value) -> bool`, `set_custom_enabled(name, bool) -> bool`, `remove_custom(name) -> bool`, `has_custom(name)`; `has_active_effects` counts enabled custom effects; crate-internal iterator over enabled custom effects in insertion order with their params)
- Modify: `src/core/shaders/pipeline.rs` (custom effect pipelines cached by serial + generation, compiled with `custom::compile(ShaderKind::Effect, …)` + error scope; run after the built-ins in `apply_effects` (or a sibling the `post_process` pass calls), each a ping-pong pass: bind group = current input view, sampler, params uniform (packed bytes via `UniformPool`, sized `max(layout.size, 16)` rounded to 16), globals uniform (`time, width, height`); failed shaders skipped after `report_once`)
- Modify: `src/core/shaders/mod.rs` (export `EffectError`), `src/lib.rs` prelude if `EffectsManager` users need `EffectError`
- Modify: `src/core/render/passes/post_process.rs` if the pass must pass custom effects through
- Test: `tests/e2e/custom_shaders.rs` (effect tests)

- [ ] **Step 1: Write the failing tests:**
  - `custom_grayscale_effect` (scene with a coloured sprite; effect with `strength` param = 1.0 makes R≈G≈B (within 2); 0.0 leaves the frame unchanged)
  - `custom_effect_runs_after_built_ins` (a built-in vignette plus a custom effect that outputs `vec4(scene(uv).rgb * 0.0 + 0.5, 1.0)` (flat grey): the whole frame is flat grey - proving it ran last)
  - `disabled_and_removed_effects_do_not_run`
  - `duplicate_custom_effect_names_are_rejected`
  - `broken_effect_is_skipped` (the frame equals the frame without it; one dev-log error with the line; no panic)
- [ ] **Step 2:** watch them fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** run them + full e2e.
- [ ] **Step 5: Suite, lint, check, commit:** `feat(shaders): run custom post-process effects`.

---

### Task 4: Documentation and demos

**Files:** `src/cli/skills/ryo-renderer.md`, `src/cli/skills/ryo-api.md` + `.claude/skills/*/SKILL.md` copies (CRLF, identical); `src/core/shaders/AGENTS.md`; `src/core/ecs/material.rs` module docs (if Task 1 left anything stale); `examples/demos/sprite-shaders` (add `assets/shaders/dissolve.wgsl` loaded with `assets.load_shader`, applied with `SpriteShader::custom(..).with_param("amount", …)` animated over time); `examples/demos/post-processing` (add a custom effect `.wgsl`, toggled by a key, listed in its on-screen help).

- [ ] "Custom shaders" sections: the two templates; `SpriteIn`/`EffectIn` fields; `sample`/`scene`; params (types, limits, set by name, `Color` as sRGB/255); colour spaces (sprite code works in the sprite's sRGB values and the engine tints after; effect code sees linear scene colour); `load_shader` + hot reload; how errors appear (dev log line + fallback); custom effects run after built-ins in insertion order.
- [ ] Demos compile (`cargo check --workspace --all-targets`) and run headless-safe (no panics on load if the asset path is wrong - they should `match` the `Result`).
- [ ] Verify CRLF + `cmp`; `cargo test --doc`.
- [ ] Commit: `docs: describe custom sprite shaders and post-process effects`.
