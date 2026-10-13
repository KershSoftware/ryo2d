# Renderer Batch (2b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put everything the world draws through one 2D batch (instanced quads plus indexed triangles), give drawables blend and filter modes with mipmapped textures, rebuild trails and lines as joined polylines, make parallax per camera, add a public debug draw, share WGSL through includes, and stop creating bind groups every frame.

**Architecture:** In place, in ordered steps that keep every test and the pixel baseline green (spec: Migration). A new `render/batch` module owns the per-frame quad and triangle streams, the pipeline cache and the draws each camera replays; `SpriteGpuRenderer` keeps the textures (registry, atlas, custom-shader compilation); the trail renderer goes. Bind groups are created with their resources; WGSL is composed in Rust from shared includes.

**Tech Stack:** Rust 2024, `wgpu`/`naga` 30.0.1 (unchanged), `bevy_ecs` 0.20 behind `ryo2d::ecs`, `etagere` (atlas), WGSL. No new crates.

**Spec:** `docs/specs/2026-10-13-renderer-batch-design.md`

## Global Constraints

- Branch `renderer-batch` in the worktree `.claude/worktrees/renderer-batch`; `main` stays green until the final merge, which the owner approves.
- `wgpu` and `naga` stay `30.0.1`; no new crates: shared WGSL is composed in Rust (spec: Bind groups, lighting buffers and shared WGSL).
- `Limits::default()` and no wgpu features; every shader stays WebGPU-valid (derivatives such as `dpdx` only in uniform control flow).
- Lighting keeps its own pipelines; only its bind groups and buffer uploads change (spec: owner decisions).
- Under `BlendMode::Alpha` the demo tour stays byte-identical to the Task 0 baseline; sampling at scale 1 with mipmaps stays byte-identical (spec: Risks).
- The tour stays byte-identical after every task except the pages a task names (trail and line pages in Task 10, debug overlay pages in Task 11, parallax pages in Task 12); each such page is checked by eye and the refresh recorded in the ledger.
- No `unwrap`/`expect` in library code outside tests; LF line endings; two-space indentation (`rustfmt.toml`).
- Game code never names `bevy_ecs`; only `Cargo.toml`, `src/core/ecs/kernel.rs` and the hidden prelude re-export do. Networking is not changed.
- Breaking public APIs is allowed; demos, games, tests and docs are updated in the task that breaks them.
- Commit messages: `type: brief description` plus bullets if needed, no co-author lines, no mention of AGENTS.md files.
- **The gate** (every task ends with it):

  ```bash
  cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{p+=$4; f+=$6} END {print p, f}'
  ./dev.sh lint 2>&1 | grep -E '^(warning|error)' | grep -v 'future version' | head -3
  ./dev.sh demos:tour >/dev/null && target/batch-baseline/compare.sh; echo "compare $?"
  ```

  Expected: failed count `0`; no lint lines (the wasm clippy included); `compare 0`, or only the pages the task names.

## Review Focus

- A `ParallaxLayer` with scale 0, negative, NaN, or so small that its tiles would number millions must draw nothing or a bounded number of tiles (`MAX_PARALLAX_TILES`), never stall the frame. Test pinned to Task 12.
- A polyline that doubles back on itself (a U-turn) or zigzags through very short segments must fall back from miter to bevel without spikes past 4x the width and without NaN vertices. Test pinned to Task 9.
- Resizing the window between frames while effects are active must rebuild the cached bind groups once (no bind group pointing at a destroyed texture, which is a validation error) and then create none per frame again. Test pinned to Task 2.
- A sprite whose pixels change (hot reload, `SpriteHandle::update`) after it was drawn zoomed out must show the new pixels at every mip level. Test pinned to Task 8.
- A custom sprite shader drawn with two blend modes in one frame must draw both, each with its own pipeline. Test pinned to Task 6.

---

## Phase 0: branch and baseline

### Task 0: Worktree and reference captures

**Files:** none in the repo; references under `target/batch-baseline/` (ignored).

- [ ] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/renderer-batch -b renderer-batch main
cd .claude/worktrees/renderer-batch
```

All later tasks run inside this worktree.

- [ ] **Step 2: Baseline tests**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{p+=$4; f+=$6} END {print p, f}'`
Expected: `2560 0` (failures must be 0).

- [ ] **Step 3: Reference captures and the comparison script**

```bash
./dev.sh demos:tour
mkdir -p target/batch-baseline
rm -rf target/batch-baseline/demo-tour && cp -R target/demo-tour target/batch-baseline/demo-tour
for g in breakout bullet-hell flappy leap-king nigh_t; do
  (cd examples/games/$g && RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=60 RYO_CAPTURE_EXIT=1 \
    RYO_CAPTURE_DIR="$PWD/../../../target/batch-baseline/games/$g" cargo run -q -p $g)
done
cat > target/batch-baseline/compare.sh <<'EOF'
#!/usr/bin/env bash
# Lists every demo-tour PNG that differs from the baseline (particles and
# steering draw random numbers, so they are checked by eye)
set -u
base=target/batch-baseline/demo-tour
now=target/demo-tour
status=0
for f in $(cd "$base" && find . -name "*.png" | grep -v -e "^./particles/" -e "^./steering/" | sort); do
  if [ ! -f "$now/$f" ]; then echo "missing $f"; status=1
  elif ! cmp -s "$base/$f" "$now/$f"; then echo "differs $f"; status=1; fi
done
exit $status
EOF
chmod +x target/batch-baseline/compare.sh
./dev.sh demos:tour >/dev/null && target/batch-baseline/compare.sh; echo $?
```

Expected: 33 demo directories in the baseline, `frame-000060.png` in each game directory, and `0`.

- [ ] **Step 4: Benchmark baseline**

Run: `cargo bench --bench foundation -- "headless frame" 2>&1 | grep -A1 '^headless frame' | grep time`
Expected: about `0.96 ms`; record the line in the ledger (Task 15 compares against it).

---

## Phase 1: shared WGSL and bind groups

### Task 1: Shared WGSL includes

**Files:**
- Create: `src/core/shaders/include/camera.wgsl`, `src/core/shaders/include/color.wgsl`, `src/core/shaders/include/sample.wgsl`, `src/core/shaders/include/fullscreen.wgsl`, `src/core/shaders/compose.rs`
- Modify: `src/core/shaders/mod.rs`; `src/core/shaders/core/{sprite,light,shadow,raymarch_shadow,trail,trail_textured}.wgsl`; the 11 `src/core/shaders/post-process/*.wgsl` effect files and `post-process/lighting.wgsl`; `src/core/shaders/custom.rs` (the sprite head and `EFFECT_PRELUDE`); `src/core/graphics/sprite_gpu.rs` (`SPRITE_SHADER`); `src/core/lighting/renderer.rs`; `src/core/trail/renderer.rs`; `src/core/shaders/pipeline.rs`
- Test: `src/core/shaders/compose.rs` (unit)

**Interfaces:**
- Produces (all `pub(crate)`, in `shaders::compose`):
  - `const CAMERA: &str`, `const COLOR: &str`, `const SAMPLE: &str`, `const FULLSCREEN: &str` (the include files)
  - `fn compose(parts: &[&str]) -> String`: the parts joined with `"\n"`
  - one function per built-in module, used by its creation site: `sprite_module()`, `light_module()`, `shadow_module()`, `raymarch_shadow_module()`, `trail_module()`, `trail_textured_module()`, `lighting_compose_module()`, `post_process_module(name: &str) -> Option<String>` (the 11 effect files by their `post_process_shaders` names)
  - `fn built_in() -> Vec<(&'static str, String)>`: every composed built-in module, named
- Include contents:
  - `camera.wgsl`: `struct CameraGlobals { view_proj: mat4x4<f32>, resolution: vec2<f32>, game_time: f32, real_time: f32, snap: f32, }` and `@group(0) @binding(0) var<uniform> globals: CameraGlobals;`. The six shaders that declare their own copy (`ShadowGlobals`, `LightGlobals`, `Globals`, `TrailGlobals`) drop it and read `globals`.
  - `color.wgsl`: `fn srgb_to_linear(c: vec3<f32>) -> vec3<f32>` (moved from `sprite.wgsl`, same body), `fn linear_to_srgb(c: vec3<f32>) -> vec3<f32>`, `fn premultiply(c: vec4<f32>) -> vec4<f32> { return vec4<f32>(c.rgb * c.a, c.a); }`.
  - `sample.wgsl`: `fn sample_masked(t: texture_2d<f32>, s: sampler, uv: vec2<f32>, bounds: vec4<f32>) -> vec4<f32>`: `sprite.wgsl`'s body with the texture and sampler passed in (`textureDimensions(t)` for the texel size).
  - `fullscreen.wgsl`: `struct FullscreenVertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32>, }` and `@vertex fn vs_main(@builtin(vertex_index) vertex_index: u32) -> FullscreenVertex` with the six-vertex position and uv arrays copied verbatim from `post-process/fade.wgsl`.
- The 11 effect shaders, `post-process/lighting.wgsl` and the custom-effect `EFFECT_PRELUDE` use `FULLSCREEN` (their fragment inputs become `FullscreenVertex` and `.uv`). The compositor's scaled quad (`core/fullscreen_quad.wgsl`), `core/camera_image.wgsl` and `core/transition.wgsl` keep their own vertex stage (different geometry and draw counts).

- [ ] **Step 1: Write the failing test** (create `src/core/shaders/compose.rs` with stubs and the test; add `pub(crate) mod compose;` to `shaders/mod.rs`)

```rust
//! Shared WGSL: include files composed in front of each shader body.

pub(crate) const CAMERA: &str = include_str!("include/camera.wgsl");
pub(crate) const COLOR: &str = include_str!("include/color.wgsl");
pub(crate) const SAMPLE: &str = include_str!("include/sample.wgsl");
pub(crate) const FULLSCREEN: &str = include_str!("include/fullscreen.wgsl");

/// `parts` as one module source.
pub(crate) fn compose(parts: &[&str]) -> String {
  parts.join("\n")
}

/// Every built-in module, composed, by name.
pub(crate) fn built_in() -> Vec<(&'static str, String)> {
  Vec::new()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn validate(name: &str, source: &str) {
    let module = naga::front::wgsl::parse_str(source)
      .unwrap_or_else(|e| panic!("{name} does not parse: {}", e.emit_to_string(source)));
    naga::valid::Validator::new(
      naga::valid::ValidationFlags::all(),
      naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{name} does not validate: {e:?}"));
  }

  #[test]
  fn every_built_in_shader_parses_and_validates() {
    let shaders = built_in();
    assert!(shaders.len() >= 18, "{} built-in shaders", shaders.len());
    for (name, source) in &shaders {
      validate(name, source);
    }
  }

  #[test]
  fn the_camera_struct_is_declared_once() {
    for (name, source) in built_in() {
      assert!(source.matches("view_proj: mat4x4").count() <= 1, "{name}");
    }
    assert!(CAMERA.contains("view_proj: mat4x4"));
  }
}
```

The include files exist (empty bodies are fine for the first run).

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --lib -p ryo2d -- shaders::compose 2>&1 | grep -E 'test result|panicked'`
Expected: FAIL, `0 built-in shaders`.

- [ ] **Step 3: Implement**

Write the four includes; move the duplicated code out of every body; add the per-module functions and list them in `built_in()`; point every creation site (`sprite_gpu.rs`, `lighting/renderer.rs` x5, `trail/renderer.rs` x2, `pipeline.rs` for the effects, `custom.rs` for the sprite head and the effect prelude) at the composed source. A custom game sprite shader is `compose(&[CAMERA, COLOR, SAMPLE, sprite body, SPRITE_PRELUDE, game source, ...])`, so game shaders get the includes too.

- [ ] **Step 4: Run it to see it pass, then the gate**

Run: `cargo test --lib -p ryo2d -- shaders::compose 2>&1 | grep -E 'test result|panicked'`
Expected: PASS (2 tests). Then the gate: `compare 0` (no pixel changes).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: shared WGSL includes for the camera, colour, sampling and fullscreen code"
```

### Task 2: Bind groups created with their resources; a counter proves it

**Files:**
- Create: `src/core/graphics/bind_groups.rs`
- Modify: every file that calls `create_bind_group` (`graphics/{atlas,camera_images,compositor,renderer,sprite_gpu}.rs`, `lighting/renderer.rs`, `render/passes/{transition,output}.rs`, `shaders/pipeline.rs`, `trail/renderer.rs`, `windows/dev_window.rs`), `graphics/mod.rs`
- Test: `src/core/graphics/bind_groups.rs` (unit), `src/core/testing/gpu_world.rs`

**Interfaces:**
- Produces:
  - `pub(crate) fn create(device: &wgpu::Device, desc: &wgpu::BindGroupDescriptor<'_>) -> wgpu::BindGroup`: the only place that calls `device.create_bind_group`; in tests it counts per thread
  - `#[cfg(test)] pub(crate) fn created_on_this_thread() -> u64`
  - `pub(crate) enum CompositeSource { Game, Lit, PostA, PostB }`; `Compositor::bind_sources(&mut self, device: &wgpu::Device, sources: &CompositeViews<'_>)` with `pub(crate) struct CompositeViews<'a> { pub(crate) game: &'a wgpu::TextureView, pub(crate) lit: &'a wgpu::TextureView, pub(crate) post: Option<(&'a wgpu::TextureView, &'a wgpu::TextureView)> }`; `Compositor::draw(&self, encoder, target: &wgpu::TextureView, source: CompositeSource, include_ui: bool, viewport: CompositeViewport)`. The renderer calls `bind_sources` when it is built, after `recreate_scene_targets`, and when the post-process targets are recreated; the UI bind group is made in `resize_ui`.
  - `PostProcessPipeline` keeps its bind groups in a map keyed by `(pass, input)` (`pass` = which uniform pool and slot, `input` = A or B; bloom's internal passes keyed the same way); the map is cleared when the targets are recreated or the effect chain changes, so after one rebuilding frame no pass creates a bind group.
  - `RenderContext` maps its `FrameImage` to a `CompositeSource` for `composite` and `capture`.

- [ ] **Step 1: Write the failing tests**

`src/core/graphics/bind_groups.rs` (the counter stubbed to return 0 is enough for the first run; the guard fails while other files call `create_bind_group`):

```rust
//! Every bind group is made here, so tests can count them.

#[cfg(test)]
thread_local! {
  static CREATED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Create a bind group (counted per thread in tests).
pub(crate) fn create(device: &wgpu::Device, desc: &wgpu::BindGroupDescriptor<'_>) -> wgpu::BindGroup {
  #[cfg(test)]
  CREATED.with(|created| created.set(created.get() + 1));
  device.create_bind_group(desc)
}

/// Bind groups created on this thread so far.
#[cfg(test)]
pub(crate) fn created_on_this_thread() -> u64 {
  CREATED.with(std::cell::Cell::get)
}

#[cfg(test)]
mod tests {
  #[test]
  fn every_bind_group_is_created_through_the_counter() {
    fn visit(dir: &std::path::Path, offenders: &mut Vec<String>) {
      for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
          visit(&path, offenders);
        } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("bind_groups.rs") {
          let text = std::fs::read_to_string(&path).unwrap();
          if text.contains(".create_bind_group(") {
            offenders.push(path.display().to_string());
          }
        }
      }
    }
    let mut offenders = Vec::new();
    visit(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut offenders);
    assert!(offenders.is_empty(), "{offenders:?}");
  }
}
```

`src/core/testing/gpu_world.rs`:

```rust
use crate::core::graphics::bind_groups::created_on_this_thread;

/// Sprites, a line, a batch sprite and two effects (one of them bloom).
struct Busy;

impl Scene for Busy {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 220, 30, 40, 255), "busy");
    for i in 0..20 {
      ctx.world.spawn((
        Transform::from_xy(10.0 + i as f32 * 5.0, 30.0),
        Renderable::sprite(sprite.clone()),
      ));
    }
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Line2D::horizontal(50.0, 5.0, 100.0, Color::WHITE, 2.0),
    ));
    ctx.effects().set_vignette(0.4);
    ctx.effects().set_bloom(0.6);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 30, 200, 60, 255), "busy-batch");
    let instance = SpriteInstance::new().with_transform(60.0, 60.0, 0.0, 4.0, 4.0);
    gpu.add_sprite_instance(&sprite, instance, 0, false);
  }
}

#[test]
fn steady_frames_create_no_bind_groups() {
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(Busy)) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let before = created_on_this_thread();
  engine.step_frames(3).unwrap();
  assert_eq!(created_on_this_thread() - before, 0);
}

#[test]
fn a_resize_rebuilds_bind_groups_once() {
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(Busy)) else {
    return;
  };
  engine.step_frames(3).unwrap();
  engine.resize(200, 150);
  engine.step_frames(1).unwrap();
  let before = created_on_this_thread();
  engine.step_frames(3).unwrap();
  assert_eq!(created_on_this_thread() - before, 0, "none after the rebuilding frame");
  assert!(engine.frame().unwrap().pixels().any(|p| p.0[0] > 100), "it still draws");
}
```

(`GpuRenderContext::add_sprite_instance` is renamed in Task 6, which updates this test.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib -p ryo2d -- every_bind_group_is_created steady_frames_create a_resize_rebuilds 2>&1 | grep -E 'test result|panicked'`
Expected: FAIL (the guard lists ~10 files; the steady test counts bind groups once the files are switched over).

- [ ] **Step 3: Implement**

Route every `create_bind_group` through `bind_groups::create`. Then remove the per-frame creations the counter reports: the compositor's source and UI bind groups (built in `bind_sources` and `resize_ui`), and the post-process chain's per-pass bind groups (cached by pass and input). Lighting's per-frame bind groups are Task 3's; `Busy` has no light, so lighting creates none here.

- [ ] **Step 4: Run them to see them pass, then the gate**

Run the Step 2 command. Expected: PASS (3 tests). Gate: `compare 0`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "perf: the compositor and post-process chain make their bind groups once

- every bind group goes through one helper, counted in tests"
```

### Task 3: Lighting: bind groups per camera and one upload per frame

**Files:**
- Modify: `src/core/lighting/renderer.rs`, `src/core/render/passes/lighting.rs`, `src/core/render/frame.rs` (`LightingFrame`), `src/core/engine/extract.rs`
- Test: `src/core/testing/gpu_world.rs`

**Interfaces:**
- Consumes: `bind_groups::create` (Task 2); `CameraUniforms::buffer(camera)` (2a).
- Produces (all `pub(crate)` on `LightingRenderer`):
  - `sync_cameras(&mut self, device: &wgpu::Device, cameras: &[CameraDraw], uniforms: &CameraUniforms)`: `LightTargets` gain `light_bind_group` (camera uniform plus the light buffer), `shadow_bind_group` (camera uniform) and `compose_bind_group` (scene copy, light view, sampler, compose uniforms), made with the targets and remade only with them
  - `prepare_frame(&mut self, queue: &wgpu::Queue, lights: &[(Vec2, Light2D)], occluders: &[(GlobalTransform, &LightOccluder2D)])`: builds the light instances and the shadow geometry once and uploads both buffers once
  - `draw_for_camera(&self, encoder: &mut wgpu::CommandEncoder, camera: Entity)`: clears the camera's light buffer, draws the lights, subtracts the shadows, with its cached bind groups
  - `compose_for_camera(..)` keeps its signature and uses the cached compose bind group
- The lighting pass calls `draw_for_camera` and `compose_for_camera` only; `present_frame` calls `prepare_frame` before the pipeline runs. The standalone API (`render_lights`, `render_lights_with_shadows`, `compose`, `compose_from_texture`) keeps working as in 2a.

- [ ] **Step 1: Write the failing test** (`src/core/testing/gpu_world.rs`)

```rust
/// `Busy` plus a shadow-casting light, an occluder, a second lit camera and a
/// long fade.
struct BusyLit {
  started: bool,
}

impl Scene for BusyLit {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    Busy.on_enter(ctx);
    ctx.world.spawn((
      Transform::from_xy(60.0, 20.0),
      Light2D::point(Color::WHITE, 80.0).with_shadows(),
    ));
    ctx.world.spawn((Transform::from_xy(60.0, 45.0), LightOccluder2D::rect(10.0, 10.0)));
    ctx.world.spawn((
      Transform::from_xy(60.0, 45.0),
      Camera2d {
        viewport: Viewport::new(0.5, 0.0, 0.5, 0.5),
        order: 1,
        ..Default::default()
      },
    ));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if std::mem::replace(&mut self.started, true) {
      return SceneAction::Continue;
    }
    SceneAction::PushWith(
      Box::new(Idle),
      Transition::fade(std::time::Duration::from_secs(10)),
    )
  }
}

/// A scene that does nothing (pushed over `BusyLit`, which keeps drawing).
struct Idle;

impl Scene for Idle {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn lit_split_frames_create_no_bind_groups() {
  let scene = BusyLit { started: false };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(scene)) else {
    return;
  };
  engine.step_frames(4).unwrap();
  let before = created_on_this_thread();
  engine.step_frames(3).unwrap();
  assert_eq!(created_on_this_thread() - before, 0);
}
```

(If a pushed scene stops the scene below from drawing, push nothing and start the fade with `SceneAction::SwitchWith` to a second `BusyLit`; the fade must be running during the measured frames.)

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --lib -p ryo2d -- lit_split_frames 2>&1 | grep -E 'test result|panicked' -A2`
Expected: FAIL with a non-zero count (light, shadow and compose bind groups per lit camera per frame).

- [ ] **Step 3: Implement** the Produces block.

- [ ] **Step 4: Run it to see it pass, then the gate**

Run the Step 2 command, then `cargo test -p ryo2d --test e2e -- every_camera_is_lit lights_turn 2>&1 | grep 'test result'`.
Expected: PASS. Gate: `compare 0`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "perf: lighting uploads its lights and shadows once per frame

- each lit camera keeps its light, shadow and compose bind groups"
```

---

## Phase 2: the batch

### Task 4: Batch keys and per-camera draw planning (CPU)

**Files:**
- Create: `src/core/render/batch/mod.rs`, `src/core/render/batch/key.rs`, `src/core/render/batch/plan.rs`
- Modify: `src/core/render/{mod.rs,draw_list.rs}`, `src/core/graphics/sprite_gpu.rs` (uses `plan_draws` in place of `plan_runs`), `src/core/render/passes/world.rs`, `src/lib.rs`
- Test: `src/core/render/batch/key.rs`, `src/core/render/batch/plan.rs` (unit)

**Interfaces:**
- Produces:
  - `pub enum BlendMode { Alpha, Premultiplied, Additive, Multiply, Screen }` (`#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]`, default `Alpha`) and `pub enum Filter { Default, Nearest, Linear }` (same derives, default `Default`), exported in the prelude.
  - `pub(crate) enum Sampling { Nearest, Linear }` and `impl Filter { pub(crate) fn resolve(self, presentation: Presentation) -> Sampling }` (`Default` is nearest for pixel art, linear otherwise).
  - `pub(crate) enum BatchShader { Sprite, SpriteEffect, Custom(u64), Line, LineTextured }`.
  - `pub(crate) struct BatchKey { pub(crate) texture: Option<TextureId>, pub(crate) shader: BatchShader, pub(crate) blend: BlendMode, pub(crate) filter: Filter, pub(crate) scissor: Option<PixelRect> }`.
  - `pub(crate) enum Stream { Quads, Triangles }`.
  - `pub(crate) struct DrawState { pub(crate) page: Option<SlotPage>, pub(crate) shader: BatchShader, pub(crate) blend: BlendMode, pub(crate) sampling: Sampling, pub(crate) scissor: Option<PixelRect> }` (`PartialEq`, `Copy`): a key resolved for drawing.
  - `pub(crate) struct Draw { pub(crate) stream: Stream, pub(crate) state: DrawState, pub(crate) range: std::ops::Range<u32> }` (instances for quads, indices for triangles).
  - `pub(crate) enum Planned { Draw(Draw), External { z: i32, layers: RenderLayers } }` (`External` carries 2a's trail groups until Task 10 removes it).
  - `pub(crate) enum Placed { Quad(DrawState), Triangles { state: DrawState, indices: std::ops::Range<u32> }, External { z: i32, layers: RenderLayers } }`.
  - `pub(crate) fn plan_draws<'i>(items: impl IntoIterator<Item = &'i DrawItem>, out: &mut Vec<Planned>, first_quad: u32, place: impl FnMut(&DrawItem) -> Option<Placed>)`: quads are numbered from `first_quad` in draw order; consecutive quads with equal states merge; consecutive triangle items with equal states and adjacent index ranges merge; `External` and any state or stream change split; `None` drops the item without a gap.
  - `DrawSprite` gains `pub(crate) blend: BlendMode`, `pub(crate) filter: Filter` (`Alpha` and `Default` until Task 6). `WorldRun` and `plan_runs` go; `SpriteGpuRenderer::world_runs(camera)` becomes `draws(camera) -> &[Planned]`.

- [ ] **Step 1: Write the failing tests** (`plan.rs`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::{RenderLayers, SlotPage, SpriteInstance, TextureId};
  use crate::core::render::{DrawKind, DrawList, SpriteShading};

  fn state(page: usize, blend: BlendMode) -> DrawState {
    DrawState {
      page: Some(SlotPage::Atlas(page)),
      shader: BatchShader::Sprite,
      blend,
      sampling: Sampling::Nearest,
      scissor: None,
    }
  }

  /// A list of `n` sprites, sorted.
  fn list(n: u32) -> DrawList {
    let mut list = DrawList::default();
    for i in 0..n {
      let texture = TextureId::for_test(i);
      list.push_sprite(DrawKind::Sprite, 0, texture, SpriteShading::Plain, SpriteInstance::new());
    }
    list.sorted();
    list
  }

  fn plan(list: &DrawList, mut place: impl FnMut(usize) -> Option<Placed>) -> Vec<Planned> {
    let mut out = Vec::new();
    let mut i = 0;
    plan_draws(list.items(), &mut out, 0, |_| {
      i += 1;
      place(i - 1)
    });
    out
  }

  fn quads(out: &[Planned]) -> Vec<(DrawState, std::ops::Range<u32>)> {
    out
      .iter()
      .map(|p| match p {
        Planned::Draw(d) => (d.state, d.range.clone()),
        Planned::External { .. } => panic!("external"),
      })
      .collect()
  }

  #[test]
  fn equal_states_merge_into_one_draw() {
    let out = plan(&list(3), |_| Some(Placed::Quad(state(0, BlendMode::Alpha))));
    assert_eq!(quads(&out), [(state(0, BlendMode::Alpha), 0..3)]);
  }

  #[test]
  fn a_blend_or_page_change_splits() {
    let states = [
      state(0, BlendMode::Alpha),
      state(0, BlendMode::Additive),
      state(1, BlendMode::Additive),
    ];
    let out = plan(&list(3), |i| Some(Placed::Quad(states[i])));
    assert_eq!(
      quads(&out),
      [(states[0], 0..1), (states[1], 1..2), (states[2], 2..3)]
    );
  }

  #[test]
  fn dropped_items_leave_no_gap() {
    let out = plan(&list(3), |i| (i != 1).then_some(Placed::Quad(state(0, BlendMode::Alpha))));
    assert_eq!(quads(&out), [(state(0, BlendMode::Alpha), 0..2)]);
  }

  #[test]
  fn adjacent_triangle_ranges_merge_and_external_splits() {
    let tri = DrawState { shader: BatchShader::Line, page: None, ..state(0, BlendMode::Alpha) };
    let placed = [
      Placed::Triangles { state: tri, indices: 0..6 },
      Placed::Triangles { state: tri, indices: 6..12 },
      Placed::External { z: 0, layers: RenderLayers::default() },
      Placed::Triangles { state: tri, indices: 12..18 },
    ];
    let out = plan(&list(4), |i| Some(placed[i].clone()));
    assert_eq!(out.len(), 3);
    assert!(matches!(&out[0], Planned::Draw(d) if d.stream == Stream::Triangles && d.range == (0..12)));
    assert!(matches!(&out[1], Planned::External { .. }));
    assert!(matches!(&out[2], Planned::Draw(d) if d.range == (12..18)));
  }
}
```

`key.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::Presentation;

  #[test]
  fn the_default_filter_follows_the_presentation() {
    let pixel_art = Presentation::PixelArt { width: 320, height: 180 };
    assert_eq!(Filter::Default.resolve(pixel_art), Sampling::Nearest);
    assert_eq!(Filter::Default.resolve(Presentation::ResolutionIndependent), Sampling::Linear);
    assert_eq!(Filter::Linear.resolve(pixel_art), Sampling::Linear);
    assert_eq!(Filter::Nearest.resolve(Presentation::ResolutionIndependent), Sampling::Nearest);
  }
}
```

(`Placed` derives `Clone`; `Draw` and `Stream` derive `Debug, Clone, PartialEq`.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib -p ryo2d -- render::batch 2>&1 | grep -E 'error\[|test result|panicked' | head`
Expected: compile errors (the types do not exist yet).

- [ ] **Step 3: Implement** the Produces block; `prepare_world` and the `world` pass use `plan_draws` and `Planned` with today's two pipelines (`Sprite` for plain, `SpriteEffect` for effects).

- [ ] **Step 4: Run them to see them pass, then the gate**

Run the Step 2 command. Expected: PASS (5 tests). Gate: `compare 0`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: batch keys and per-camera draw planning replace sprite runs"
```

### Task 5: `Batch2D` draws the quads; shaders output premultiplied colour

**Files:**
- Create: `src/core/render/batch/gpu.rs`, `src/core/render/batch/pipelines.rs`
- Modify: `src/core/graphics/sprite_gpu.rs` (pipelines, instance buffer and camera bind groups move to the batch; it keeps the registry, atlas, names, custom-shader sources and `shade_custom`), `src/core/shaders/core/sprite.wgsl`, `src/core/render/passes/world.rs`, `src/core/render/frame.rs` (`FrameScene.batch`), `src/core/engine/{mod.rs,extract.rs}`, `src/core/testing/gpu_world.rs` (draw counts through the batch)
- Test: `src/core/render/batch/pipelines.rs` (unit); the e2e suite and the tour

**Interfaces:**
- Consumes: `plan_draws`, `DrawState`, `Planned` (Task 4); `bind_groups::create` (Task 2); `compose::sprite_module` (Task 1).
- Produces:
  - `pub(crate) fn blend_state(mode: BlendMode) -> wgpu::BlendState` (Task 5 fills `Alpha`; Task 6 the rest)
  - `pub(crate) struct PipelineKey { pub(crate) shader: BatchShader, pub(crate) blend: BlendMode, pub(crate) stream: Stream, pub(crate) format: wgpu::TextureFormat }`
  - `pub(crate) struct Batch2D` with `new(device: &wgpu::Device, format: wgpu::TextureFormat, presentation: Presentation) -> Self`, `sync_cameras(&mut self, device, uniforms: &CameraUniforms)`, `prepare(&mut self, device, queue, list: &DrawList, cameras: &[CameraDraw], textures: &mut SpriteGpuRenderer)`, `draws(&self, camera: Entity) -> &[Planned]`, `draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, draw: &Draw, camera: Entity, textures: &'a SpriteGpuRenderer)`, `draw_count(&self) -> usize`
  - Pipelines are created lazily per `PipelineKey` (custom ones through 2a's `Checked<P>`), stored in a map; a failed custom shader draws with `Sprite`.
  - `sprite.wgsl` `finish` returns `vec4<f32>(srgb_to_linear(rgb) * alpha, alpha)`; every sprite pipeline uses `blend_state(Alpha)` = `BlendState::PREMULTIPLIED_ALPHA_BLENDING`.
  - `DebugStats::draw_calls` = `batch.draw_count()` plus 2a's trail draws (until Task 10).

- [ ] **Step 1: Write the failing test** (`pipelines.rs`)

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn alpha_blends_premultiplied_colour() {
    // Same result as straight alpha once the shader multiplies by alpha
    assert_eq!(blend_state(BlendMode::Alpha), wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
  }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --lib -p ryo2d -- alpha_blends_premultiplied 2>&1 | grep -E 'error\[|test result' | head -3`
Expected: compile error (`blend_state` missing).

- [ ] **Step 3: Implement** the Produces block.

- [ ] **Step 4: Run it, then the gate**

Run the Step 2 command (PASS), then the gate. Expected: `compare 0`: premultiplied output is byte-identical to straight alpha. If pages differ, check them per pixel (`magick compare -metric AE`): differences of at most 1 in any channel are rounding in the blend unit; record a ruling and refresh only those pages; anything larger is a bug.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: a 2D batch draws the world's quads in premultiplied colour"
```

### Task 6: Blend and filter modes on the drawables

**Files:**
- Modify: `src/core/render/batch/{key.rs,pipelines.rs,gpu.rs}`, `src/core/shaders/core/sprite.wgsl` (an entry per texture convention), `src/core/graphics/atlas.rs` (a bind group per page and sampling), `src/core/graphics/sprite_gpu.rs` (`BatchCollector`), `src/core/graphics/gpu_context.rs`, `src/core/ecs/renderable.rs`, `src/core/particles/{emitter.rs,mod.rs}`, `src/core/render/{draw_list.rs,entities.rs,passes/world.rs}`, `src/lib.rs`; callers of `add_sprite_instance`/`add_custom_sprite`/`BatchCollector::add` (`src/core/graphics/parallax_gpu.rs`, `src/core/testing/{atlas_bleeding.rs,gpu_world.rs}`, `tests/e2e/{gpu_world.rs,sprite_atlas.rs,custom_shaders.rs}`, `src/core/particles/mod.rs`, `src/core/graphics/sprite_gpu.rs` tests)
- Test: `src/core/render/batch/pipelines.rs` (unit), `tests/e2e/gpu_world.rs`

**Interfaces:**
- Consumes: `BlendMode`, `Filter`, `Sampling` (Task 4); `Batch2D` (Task 5).
- Produces:
  - `blend_state` for every mode (spec table): `Alpha`/`Premultiplied` = (`One`, `OneMinusSrcAlpha`) for colour and alpha; `Additive` colour (`One`, `One`); `Multiply` colour (`Dst`, `OneMinusSrcAlpha`); `Screen` colour (`OneMinusDst`, `One`); the last three keep the destination alpha (`Zero`, `One`).
  - `sprite.wgsl`: `fs_base`/`fs_effect` premultiply the sampled straight colour; `fs_base_premultiplied`/`fs_effect_premultiplied` take it as is; the pipeline for `BlendMode::Premultiplied` uses the second pair; custom shaders get the same choice through their generated entry.
  - Two samplers (nearest; linear) created once; the atlas makes a bind group per page and `Sampling`, and the batch picks it from `DrawState::sampling`.
  - `Renderable` gains `pub blend: BlendMode`, `pub filter: Filter`, `pub lighting: bool` (default `true`) and `with_blend`, `with_filter`, `with_lighting`; `Renderable::render_target` sets `BlendMode::Premultiplied`.
  - `ParticleEmitter` gains `pub blend: BlendMode`, `pub filter: Filter`, `pub lighting: bool` and their `with_*` builders.
  - `#[derive(Debug, Clone, Copy, PartialEq)] pub struct SpriteDraw { pub z: i32, pub blend: BlendMode, pub filter: Filter, pub lit: bool, pub effects: bool }` with a hand-written `Default` (`z` 0, `Alpha`, `Filter::Default`, `lit` true, `effects` false), in the prelude.
  - `GpuRenderContext::add_sprite(&mut self, sprite: &SpriteHandle, instance: SpriteInstance, draw: SpriteDraw)` replaces `add_sprite_instance`; `add_custom_sprite(&mut self, sprite, shader, params, instance, draw: SpriteDraw)`; `BatchCollector::add(&mut self, sprite: &SpriteHandle, instance: SpriteInstance, draw: SpriteDraw)`.
  - A drawable with `lit == false` goes to the unlit list and draws in `unlit_world` (sprites included); drop, blob and contact shadows draw with `Alpha` whatever their entity's blend.

- [ ] **Step 1: Write the failing tests**

`pipelines.rs`:

```rust
#[test]
fn every_blend_mode_has_its_state() {
  use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as Op};
  let over = C { src_factor: F::One, dst_factor: F::OneMinusSrcAlpha, operation: Op::Add };
  let keep_alpha = C { src_factor: F::Zero, dst_factor: F::One, operation: Op::Add };
  let colour = |mode| blend_state(mode).color;
  assert_eq!(blend_state(BlendMode::Premultiplied), wgpu::BlendState { color: over, alpha: over });
  assert_eq!(colour(BlendMode::Additive), C { src_factor: F::One, dst_factor: F::One, operation: Op::Add });
  assert_eq!(colour(BlendMode::Multiply), C { src_factor: F::Dst, dst_factor: F::OneMinusSrcAlpha, operation: Op::Add });
  assert_eq!(colour(BlendMode::Screen), C { src_factor: F::OneMinusDst, dst_factor: F::One, operation: Op::Add });
  for mode in [BlendMode::Additive, BlendMode::Multiply, BlendMode::Screen] {
    assert_eq!(blend_state(mode).alpha, keep_alpha, "{mode:?}");
  }
}
```

`tests/e2e/gpu_world.rs`:

```rust
/// A 16x16 colour square drawn with `blend` over a 32x32 grey one.
struct Over(BlendMode);

impl Scene for Over {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.spawn((
      Transform::from_xy(16.0, 16.0),
      Renderable::sprite(solid("under-grey", 32, [100, 100, 100, 255])),
    ));
    ctx.world.spawn((
      Transform::from_xy(16.0, 16.0),
      Renderable::sprite(solid("over-colour", 16, [120, 60, 30, 255]))
        .with_z_order(1)
        .with_blend(self.0),
    ));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn centre_with(blend: BlendMode) -> Option<[u8; 4]> {
  let mut engine = start(EngineBuilder::new().resolution(32, 32).start_scene(Over(blend)))?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap().get_pixel(16, 16).0)
}

#[test]
fn blend_modes_combine_as_named() {
  let Some(alpha) = centre_with(BlendMode::Alpha) else {
    return;
  };
  let additive = centre_with(BlendMode::Additive).unwrap();
  let multiply = centre_with(BlendMode::Multiply).unwrap();
  let screen = centre_with(BlendMode::Screen).unwrap();
  assert_eq!(alpha, [120, 60, 30, 255], "alpha covers");
  for c in 0..3 {
    assert!(additive[c] > 100, "additive brightens: {additive:?}");
    assert!(multiply[c] < 100, "multiply darkens: {multiply:?}");
    assert!(screen[c] >= 100 && screen[c] <= additive[c], "screen: {screen:?} vs {additive:?}");
  }
}

#[test]
fn a_transparent_render_target_shows_without_dark_edges() {
  struct Glass;
  impl Scene for Glass {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      // White ground; a half-transparent red square seen two ways
      ctx.world.spawn((Transform::from_xy(32.0, 16.0), Renderable::sprite(solid("glass-white", 64, [255, 255, 255, 255]))));
      let red = solid("glass-red", 8, [220, 30, 40, 128]);
      ctx.world.spawn((Transform::from_xy(16.0, 16.0), Renderable::sprite(red.clone()).with_z_order(1)));
      ctx.world.spawn((Transform::from_xy(500.0, 500.0), Renderable::sprite(red)));
      let target = ctx.render_targets().create(8, 8);
      ctx.world.spawn((
        Transform::from_xy(500.0, 500.0),
        Camera2d { target: CameraTarget::Texture(target), clear: CameraClear::None, ..Default::default() },
      ));
      ctx.world.spawn((Transform::from_xy(48.0, 16.0), Renderable::render_target(target).with_z_order(1)));
    }
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }
  }
  let Some(mut engine) = start(EngineBuilder::new().resolution(64, 32).start_scene(Glass)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let (direct, shown) = (frame.get_pixel(16, 16).0, frame.get_pixel(48, 16).0);
  for c in 0..3 {
    assert!(direct[c].abs_diff(shown[c]) <= 2, "{direct:?} vs {shown:?}");
  }
}

#[test]
fn nearest_and_linear_differ_when_magnified() {
  struct Checker(Filter);
  impl Scene for Checker {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let texels = vec![0, 0, 0, 255, 255, 255, 255, 255];
      let sprite = SpriteHandle::from_sprite(Sprite::from_rgba(texels, 2, 1), "checker-2x1");
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0).with_scale(Vec2::splat(16.0)),
        Renderable::sprite(sprite).with_filter(self.0),
      ));
    }
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }
  }
  let middle = |filter| {
    let mut engine = start(EngineBuilder::new().resolution(32, 32).start_scene(Checker(filter)))?;
    engine.step_frames(2).unwrap();
    Some(engine.frame().unwrap().get_pixel(15, 16).0)
  };
  let Some(nearest) = middle(Filter::Nearest) else {
    return;
  };
  let linear = middle(Filter::Linear).unwrap();
  assert!(nearest[0] == 0 || nearest[0] == 255, "nearest is one texel: {nearest:?}");
  assert!(linear[0] > 10 && linear[0] < 245, "linear mixes near the edge: {linear:?}");
}

#[test]
fn an_unlit_sprite_keeps_full_brightness_in_the_dark() {
  let scene = Fixture::new(|ctx| {
    ctx.lighting().set_ambient(AmbientLight::dark());
    ctx.lighting().set_enabled(true);
    let red = solid("dark-red", 8, RED);
    ctx.world.spawn((Transform::from_xy(8.0, 8.0), Renderable::sprite(red.clone())));
    ctx.world.spawn((Transform::from_xy(24.0, 8.0), Renderable::sprite(red).with_lighting(false)));
  });
  let Some(frame) = frame_of(scene, (32, 16)) else {
    return;
  };
  assert!(frame.get_pixel(8, 8).0[0] < 60, "lit and dark");
  assert_eq!(frame.get_pixel(24, 8).0, RED, "unlit");
}

#[test]
fn a_custom_shader_draws_with_two_blend_modes_in_one_frame() {
  struct TwoBlends;
  impl Scene for TwoBlends {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let shader = ShaderHandle::from_source(
        "two-blends",
        "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv); }",
      );
      let grey = solid("two-grey", 64, [100, 100, 100, 255]);
      ctx.world.spawn((Transform::from_xy(32.0, 16.0), Renderable::sprite(grey)));
      let colour = solid("two-colour", 8, [120, 60, 30, 255]);
      for (x, blend) in [(16.0, BlendMode::Alpha), (48.0, BlendMode::Additive)] {
        ctx.world.spawn((
          Transform::from_xy(x, 16.0),
          Renderable::sprite(colour.clone()).with_z_order(1).with_blend(blend),
          Material::custom(shader.clone()),
        ));
      }
    }
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }
  }
  let Some(mut engine) = start(EngineBuilder::new().resolution(64, 32).start_scene(TwoBlends)) else {
    return;
  };
  // Custom pipelines are used from the frame after their check
  engine.step_frames(4).unwrap();
  let frame = engine.frame().unwrap();
  assert_eq!(frame.get_pixel(16, 16).0, [120, 60, 30, 255]);
  assert!(frame.get_pixel(48, 16).0[0] > 120, "additive brightens");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --test e2e -- blend_modes transparent_render_target nearest_and_linear unlit_sprite two_blend 2>&1 | grep -E 'error\[|test result' | head`
Expected: compile errors (`with_blend`, `with_filter`, `with_lighting`, `BlendMode` in scope).

- [ ] **Step 3: Implement** the Produces block, migrating every caller in the Files list.

- [ ] **Step 4: Run them to see them pass, then the gate**

Run the Step 2 command and `cargo test --lib -p ryo2d -- every_blend_mode 2>&1 | grep 'test result'`.
Expected: PASS. Gate: `compare 0`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: blend and filter modes for sprites, particles and batch sprites

- Alpha, Premultiplied, Additive, Multiply and Screen; nearest or linear
  per drawable, the presentation's choice by default
- a sprite showing a render target is premultiplied by default
- any sprite can skip lighting"
```

### Task 7: Pixel-art snapping keeps a quad's size; the shadow ellipse is looked up once

**Files:**
- Modify: `src/core/shaders/core/sprite.wgsl`, `src/core/render/entities.rs`
- Test: `tests/e2e/gpu_world.rs`

**Interfaces:**
- Produces:
  - In `vs_main`, an axis-aligned quad under `snap` takes its two opposite corners to target pixels, rounds the top-left and the size once, and places each vertex from them:

    ```wgsl
    fn to_pixel(world: vec2<f32>) -> vec2<f32> {
        let clip = globals.view_proj * vec4<f32>(world, 0.0, 1.0);
        return (clip.xy * vec2<f32>(0.5, -0.5) + 0.5) * globals.resolution;
    }

    fn from_pixel(pixel: vec2<f32>) -> vec4<f32> {
        let ndc = (pixel / globals.resolution - 0.5) * vec2<f32>(2.0, -2.0);
        return vec4<f32>(ndc, 0.0, 1.0);
    }

    // In vs_main, replacing 2a's per-vertex snap
    if globals.snap > 0.5 && axis_aligned {
        let p0 = to_pixel((transform * vec3<f32>(-0.5, -0.5, 1.0)).xy);
        let p1 = to_pixel((transform * vec3<f32>(0.5, 0.5, 1.0)).xy);
        let origin = round(min(p0, p1));
        let size = round(abs(p1 - p0));
        // A corner that maps to the larger pixel coordinate counts from the far side
        let along = select(vertex.position + 0.5, 0.5 - vertex.position, p1 < p0);
        clip = from_pixel(origin + along * size);
    }
    ```
  - `EntityDrawer` keeps `ellipse: Option<TextureId>`, reused while `gpu.is_texture(id)` holds; `Emitter::ellipse_texture` no longer hashes the name per shadow.

- [ ] **Step 1: Write the failing test** (`tests/e2e/gpu_world.rs`)

```rust
#[test]
fn a_fractional_quad_keeps_its_width_as_it_moves_in_pixel_art() {
  /// A 3-texel-wide red bar scaled by 1.5 at `x`.
  struct Bar(f32);
  impl Scene for Bar {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let bar = SpriteHandle::from_sprite(Sprite::solid_rect(3, 4, 220, 30, 40, 255), "frac-bar");
      ctx.world.spawn((
        Transform::from_xy(self.0, 8.0).with_scale(Vec2::splat(1.5)),
        Renderable::sprite(bar),
      ));
    }
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }
  }
  let width_at = |x: f32| {
    let builder = EngineBuilder::new().pixel_art(32, 16).start_scene(Bar(x));
    let mut engine = start(builder)?;
    engine.step_frames(2).unwrap();
    let frame = engine.frame().unwrap();
    Some((0..32).filter(|&px| frame.get_pixel(px, 8).0 == RED).count())
  };
  let Some(a) = width_at(10.2) else {
    return;
  };
  let b = width_at(10.7).unwrap();
  assert_eq!(a, b, "the bar is {a} then {b} pixels wide");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p ryo2d --test e2e -- fractional_quad_keeps 2>&1 | grep -E 'test result|panicked' -A2`
Expected: FAIL (4 then 5 pixels: each corner rounds on its own).

- [ ] **Step 3: Implement** the Produces block.

- [ ] **Step 4: Run it to see it pass, then the gate**

Expected: PASS. Gate: `compare 0` (integer-sized sprites snap as before); if a pixel-art page changes, it holds a fractionally scaled sprite: check it by eye and refresh it with a ruling.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "fix: pixel-art sprites keep their size as they move; shadows reuse the ellipse id"
```

---

## Phase 3: mipmaps

### Task 8: Mipmapped atlas pages and standalone textures

**Files:**
- Modify: `src/core/graphics/atlas.rs`, `src/core/graphics/sprite_gpu.rs` (samplers), `src/core/shaders/include/sample.wgsl`, `src/core/shaders/core/sprite.wgsl`, `src/core/shaders/custom.rs` (the generated entry passes derivatives)
- Test: `src/core/graphics/atlas.rs` (unit), `tests/e2e/gpu_world.rs`

**Interfaces:**
- Produces (in `atlas.rs`):
  - `const BORDER: u32 = 8`, `const CELL: u32 = 8`, `pub(crate) const MIP_LEVELS: u32 = 4`
  - `AtlasLayout` allocates in `CELL`-pixel units (an etagere allocator over `page_size / CELL`), so every bordered rect starts on and spans multiples of 8 pixels; the image sits `BORDER` pixels inside it.
  - `fn extrude_border(rgba: &[u8], width: u32, height: u32, border: u32) -> Option<Vec<u8>>` (edge pixels repeated `border` deep)
  - `pub(crate) fn mip_chain(rgba: &[u8], width: u32, height: u32, levels: u32) -> Vec<(u32, u32, Vec<u8>)>`: level 0 is the input; each next level averages 2x2 texels in linear light (sRGB bytes decoded, averaged, encoded; alpha averaged linearly), halving each side (an odd side keeps its last row or column)
  - Pages are created with `mip_level_count: MIP_LEVELS`; a slot writes every level at `(x >> level, y >> level)`; standalone textures carry a full chain (`32 - max(w, h).leading_zeros()` levels).
  - Samplers: nearest (`mipmap_filter: Nearest`, `lod_max_clamp: 0.0`) and linear (`mipmap_filter: Linear`).
  - `sample_masked(t, s, uv, bounds, ddx, ddy)` samples with `textureSampleGrad(t, s, inner, ddx, ddy)`; fragment entries take `let ddx = dpdx(in.uv); let ddy = dpdy(in.uv);` as their first statements (uniform control flow).

- [ ] **Step 1: Write the failing tests**

`atlas.rs`:

```rust
#[test]
fn mip_levels_average_in_linear_light() {
  // A black and white 2x2 checker averages to linear 0.5: sRGB 188
  let rgba = [0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255];
  let chain = mip_chain(&rgba, 2, 2, 2);
  assert_eq!(chain.len(), 2);
  let (w, h, pixels) = &chain[1];
  assert_eq!((*w, *h), (1, 1));
  assert!((187..=189).contains(&pixels[0]), "{pixels:?}");
  assert_eq!(pixels[3], 255);
}

#[test]
fn slots_line_up_with_the_smallest_mip_level() {
  let mut layout = AtlasLayout::new(PAGE_SIZE, MAX_PAGES);
  for (i, (w, h)) in [(5, 9), (16, 16), (31, 3), (1, 1)].into_iter().enumerate() {
    let placed = layout.place(TextureId::for_test(i as u32), w, h, 0).unwrap();
    assert_eq!((placed.x % CELL, placed.y % CELL), (0, 0), "{w}x{h}");
    let [u0, v0, ..] = placed.slot.uv;
    let inset = ((u0 * PAGE_SIZE as f32).round() as u32 - placed.x, (v0 * PAGE_SIZE as f32).round() as u32 - placed.y);
    assert_eq!(inset, (BORDER, BORDER));
  }
}

#[test]
fn borders_repeat_edge_pixels_eight_deep() {
  let rgba = [10, 20, 30, 255];
  let bordered = extrude_border(&rgba, 1, 1, BORDER).unwrap();
  let side = (1 + 2 * BORDER) as usize;
  assert_eq!(bordered.len(), side * side * 4);
  assert!(bordered.chunks(4).all(|p| p == [10, 20, 30, 255]));
}
```

`tests/e2e/gpu_world.rs`:

```rust
/// A 64x64 one-texel checker drawn at an eighth of its size.
struct Shrunk(Filter);

impl Scene for Shrunk {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let mut data = Vec::with_capacity(64 * 64 * 4);
    for i in 0..64 * 64 {
      let v = if (i % 64 + i / 64) % 2 == 0 { 0 } else { 255 };
      data.extend_from_slice(&[v, v, v, 255]);
    }
    let checker = SpriteHandle::from_sprite(Sprite::from_rgba(data, 64, 64), "shrunk-checker");
    ctx.world.spawn((
      Transform::from_xy(16.0, 16.0).with_scale(Vec2::splat(0.125)),
      Renderable::sprite(checker).with_filter(self.0),
    ));
  }
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn a_shrunk_checker_averages_to_grey_with_mipmaps() {
  let centre = |filter| {
    let mut engine = start(EngineBuilder::new().resolution(32, 32).start_scene(Shrunk(filter)))?;
    engine.step_frames(2).unwrap();
    Some(engine.frame().unwrap().get_pixel(16, 16).0)
  };
  let Some(linear) = centre(Filter::Linear) else {
    return;
  };
  let nearest = centre(Filter::Nearest).unwrap();
  assert!((160..=215).contains(&linear[0]), "linear with mips is grey: {linear:?}");
  assert!(nearest[0] < 10 || nearest[0] > 245, "nearest is one texel: {nearest:?}");
}

#[test]
fn a_reloaded_sprite_is_new_at_every_mip_level() {
  let scene = Fixture::new(|ctx| {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(64, 64, 220, 30, 40, 255), "reload-mips");
    ctx.world.spawn((
      Transform::from_xy(16.0, 16.0).with_scale(Vec2::splat(0.125)),
      Renderable::sprite(sprite).with_filter(Filter::Linear),
    ));
  });
  let Some(mut engine) = start(EngineBuilder::new().resolution(32, 32).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let handle = {
    let world = engine.world_mut();
    let mut renderables = world.query::<&Renderable>();
    renderables.iter(world).next().unwrap().sprite.clone()
  };
  handle.update(Sprite::solid_rect(64, 64, 30, 200, 60, 255));
  engine.step_frames(2).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(16, 16).0, GREEN);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib -p ryo2d -- mip_levels slots_line_up borders_repeat 2>&1 | grep -E 'error\[|test result' | head -3` and `cargo test -p ryo2d --test e2e -- shrunk_checker reloaded_sprite_is_new 2>&1 | grep -E 'test result|panicked' -A2`
Expected: compile errors in the unit tests (`mip_chain`, `CELL`, the `border` argument); the shrunk checker FAILS (black or white at the centre with a linear filter and no mips).

- [ ] **Step 3: Implement** the Produces block.

- [ ] **Step 4: Run them to see them pass, then the gate**

Expected: PASS. Gate: `compare 0` (level 0 at scale 1; pixel art samples nearest without mips). Also run `cargo test --lib -p ryo2d -- atlas` (the eviction and placement tests keep passing with 8-pixel cells).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: mipmapped sprite textures, so zoomed-out sprites stop shimmering

- atlas slots get 8-pixel borders on an 8-pixel grid and four mip levels
- levels average in linear light when a texture is uploaded"
```

---

## Phase 4: trails, lines and debug drawing on the batch

### Task 9: The polyline builder (CPU)

**Files:**
- Create: `src/core/render/batch/polyline.rs`
- Modify: `src/core/render/batch/mod.rs`, `src/core/trail/trail.rs` (`LineJoin` loses `None`, default `Miter`; `Trail2D` gains `join` with `with_join`, `join`, `set_join`), `src/core/trail/line.rs` (default join `Miter`), `src/core/trail/renderer.rs` (its `LineJoin::None` arms go; the old renderer keeps drawing until Task 10), `examples/demos/trails/main.rs` (its `JOINS` array loses `None`)
- Test: `src/core/render/batch/polyline.rs` (unit), the existing `src/core/trail/*` tests

**Interfaces:**
- Produces (all `pub(crate)`):
  - `#[repr(C)] #[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)] struct Vertex2D { position: [f32; 2], uv: [f32; 2], color: [f32; 4], params: [f32; 4], across: [f32; 2] }` (56 bytes; the spec's "48" miscounted)
  - `#[derive(Clone, Copy, Debug)] struct PolyPoint { pos: Vec2, half_width: f32, color: [f32; 4], u: f32 }` (`u` is precomputed: 0..1 when stretched, distance / tile length when tiled)
  - `#[derive(Clone, Copy, Debug)] struct PolyStyle { join: LineJoin, cap: LineCap, closed: bool, params: [f32; 4] }` (`params` as 2a's `build_effect_params`: effect and style bits in `[0]`)
  - `const MITER_LIMIT: f32 = 4.0` (a miter longer than `MITER_LIMIT` times the width becomes a bevel)
  - `#[derive(Default)] struct Triangles { vertices: Vec<Vertex2D>, indices: Vec<u32> }` with `clear(&mut self)` and `polyline(&mut self, points: &[PolyPoint], style: &PolyStyle) -> std::ops::Range<u32>` (the index range it added; empty for nothing drawn)
- Rules: non-finite points are skipped, consecutive points closer than `1e-4` collapse, fewer than two points or a non-positive width draws nothing; the strip shares edge vertices and each join adds one wedge on its outside, so no area is covered twice; `v` is 0 on one edge and 1 on the other; caps extend `u` beyond the end point by their length over the line's length scale; `across` is the unit perpendicular (zero at cap and join centres).

- [ ] **Step 1: Write the failing tests** (in `polyline.rs`, with `Triangles::polyline` stubbed to `0..0`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::trail::{LineCap, LineJoin};
  use glam::Vec2;

  fn build(points: &[(f32, f32)], width: f32, join: LineJoin, cap: LineCap, closed: bool) -> Triangles {
    let pts: Vec<PolyPoint> = points
      .iter()
      .enumerate()
      .map(|(i, &(x, y))| PolyPoint {
        pos: Vec2::new(x, y),
        half_width: width / 2.0,
        color: [1.0; 4],
        u: i as f32,
      })
      .collect();
    let mut t = Triangles::default();
    t.polyline(&pts, &PolyStyle { join, cap, closed, params: [0.0; 4] });
    t
  }

  fn area(t: &Triangles) -> f32 {
    t.indices
      .chunks(3)
      .map(|tri| {
        let [a, b, c] = [0, 1, 2].map(|k| Vec2::from(t.vertices[tri[k] as usize].position));
        (b - a).perp_dot(c - a).abs() / 2.0
      })
      .sum()
  }

  const L: [(f32, f32); 3] = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];

  #[test]
  fn a_straight_segment_covers_its_rectangle() {
    let t = build(&[(0.0, 0.0), (10.0, 0.0)], 2.0, LineJoin::Miter, LineCap::Butt, false);
    assert!((area(&t) - 20.0).abs() < 1e-3, "{}", area(&t));
  }

  #[test]
  fn a_mitered_corner_covers_its_outline_once() {
    // Outline (0,-1) (11,-1) (11,10) (9,10) (9,1) (0,1): area 40
    let t = build(&L, 2.0, LineJoin::Miter, LineCap::Butt, false);
    assert!((area(&t) - 40.0).abs() < 1e-3, "{}", area(&t));
  }

  #[test]
  fn a_bevelled_corner_cuts_the_miter_triangle() {
    let t = build(&L, 2.0, LineJoin::Bevel, LineCap::Butt, false);
    assert!((area(&t) - 39.5).abs() < 1e-3, "{}", area(&t));
  }

  #[test]
  fn a_round_corner_approaches_a_quarter_circle() {
    let t = build(&L, 2.0, LineJoin::Round, LineCap::Butt, false);
    let expected = 39.0 + std::f32::consts::FRAC_PI_4;
    assert!((area(&t) - expected).abs() < 0.05, "{} vs {expected}", area(&t));
  }

  #[test]
  fn caps_extend_the_ends() {
    let square = build(&[(0.0, 0.0), (10.0, 0.0)], 2.0, LineJoin::Miter, LineCap::Square, false);
    assert!((area(&square) - 24.0).abs() < 1e-3, "{}", area(&square));
    let round = build(&[(0.0, 0.0), (10.0, 0.0)], 2.0, LineJoin::Miter, LineCap::Round, false);
    assert!((area(&round) - (20.0 + std::f32::consts::PI)).abs() < 0.1, "{}", area(&round));
  }

  #[test]
  fn a_closed_square_joins_at_the_seam() {
    let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let t = build(&square, 2.0, LineJoin::Miter, LineCap::Butt, true);
    assert!((area(&t) - 80.0).abs() < 1e-3, "outer 144 minus inner 64: {}", area(&t));
  }

  #[test]
  fn a_u_turn_falls_back_to_a_bevel() {
    let t = build(&[(0.0, 0.0), (10.0, 0.0), (0.0, 0.1)], 2.0, LineJoin::Miter, LineCap::Butt, false);
    for v in &t.vertices {
      let p = Vec2::from(v.position);
      assert!(p.is_finite(), "{p}");
      assert!(p.distance(Vec2::new(10.0, 0.0)) < 10.0 + MITER_LIMIT * 2.0, "spike at {p}");
    }
    assert!(!t.indices.is_empty());
  }

  #[test]
  fn degenerate_input_draws_nothing() {
    for points in [vec![], vec![(1.0, 1.0)], vec![(1.0, 1.0), (1.0, 1.0)], vec![(f32::NAN, 0.0), (1.0, 1.0)]] {
      let t = build(&points, 2.0, LineJoin::Miter, LineCap::Round, false);
      assert!(t.indices.is_empty(), "{points:?}");
    }
    let zero = build(&[(0.0, 0.0), (10.0, 0.0)], 0.0, LineJoin::Miter, LineCap::Butt, false);
    assert!(zero.indices.is_empty());
  }

  #[test]
  fn uv_runs_along_and_across() {
    let t = build(&[(0.0, 0.0), (10.0, 0.0)], 2.0, LineJoin::Miter, LineCap::Butt, false);
    for v in &t.vertices {
      assert!(v.uv[1] == 0.0 || v.uv[1] == 1.0, "{:?}", v.uv);
      assert!((0.0..=1.0).contains(&v.uv[0]), "{:?}", v.uv);
    }
  }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib -p ryo2d -- render::batch::polyline 2>&1 | grep -E 'error\[|test result|panicked' | head -5`
Expected: FAIL (areas 0; the degenerate test passes).

- [ ] **Step 3: Implement** the builder and the `LineJoin` change.

- [ ] **Step 4: Run them to see them pass, then the gate**

Expected: PASS (9 tests), and `cargo test --lib -p ryo2d -- trail` green. Gate: line pages may change where 2a drew `LineJoin::None` by default (`Line2D` defaulted to `None`); those are Task 10's refresh, so if `compare` lists line pages here, leave the baseline alone and note them for Task 10.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: a polyline builder with miter, bevel and round joins

- LineJoin::None goes; lines and trails default to Miter"
```

### Task 10: Trails and lines draw on the batch; the trail renderer goes

**Files:**
- Create: `src/core/shaders/core/line.wgsl`, `src/core/render/batch/lines.rs`
- Modify: `src/core/render/batch/{gpu.rs,pipelines.rs,plan.rs,mod.rs}`, `src/core/shaders/{compose.rs,include/sample.wgsl}`, `src/core/render/{draw_list.rs,frame.rs,passes/world.rs}`, `src/core/engine/{mod.rs,extract.rs}`, `src/core/trail/{mod.rs,trail.rs,line.rs}` (delete `renderer.rs`; add `blend`, `filter` with `with_blend`, `with_filter` to `Trail2D` and `Line2D`), `src/lib.rs` (prelude loses `TrailRenderer`), `tests/e2e.rs` (drop the `trail_renderer` module), `docs` mentions are Task 14's
- Delete: `src/core/trail/renderer.rs`, `src/core/shaders/core/trail.wgsl`, `src/core/shaders/core/trail_textured.wgsl`, `tests/e2e/trail_renderer.rs`
- Test: `tests/e2e/gpu_world.rs`

**Interfaces:**
- Consumes: `Triangles`, `PolyPoint`, `PolyStyle` (Task 9); `Batch2D`, `Planned` (Tasks 4-5); `blend_state`, `Sampling` (Task 6); atlas slots and `sample_masked` (Task 8).
- Produces:
  - `pub(crate) fn trail_points(trail: &Trail2D, out: &mut Vec<PolyPoint>) -> Option<PolyStyle>` and `pub(crate) fn line_points(line: &Line2D, at: Vec2, out: &mut Vec<PolyPoint>) -> Option<PolyStyle>` (in `lines.rs`): 2a's per-point rules ported exactly (trail: width from `width_at(t)`, colour from `color_at` by fade distance or `t`, age alpha when not fading by alpha or distance, `u` by texture mode and effect as `push_trail` did; line: `width_at(t)`, `color_at(t)`, `u = t`), the effect and style bits as `build_effect_params`
  - `DrawPayload::Triangles(usize)` (an index into the list's triangle items: key, layers, bounds, index range) replaces `DrawPayload::Trail`; `DrawList::push_triangles(lit, z, key, layers, bounds, indices)`; `Planned::External` and `Placed::External` go
  - `Batch2D` gains the triangle stream (`Triangles` built during extraction, uploaded once, both buffers doubling) and `Line`/`LineTextured` pipelines per blend (`TriangleList`, `Vertex2D` layout at locations 0..4)
  - `line.wgsl` (composed with `CAMERA`, `COLOR`, `SAMPLE`): 2a's `trail.wgsl` vertex stage (displacement by `across` for Lightning, Noise, Ribbon; uv scroll) and fragment styles (Solid, Soft, Glow), with `trail.wgsl`'s Noise and Ribbon formulas for both entries (the textured copy differed); `fs_main` and `fs_textured` output premultiplied colour; the textured entry decodes the atlas's sRGB bytes and samples with `sample_wrapped`
  - `fn sample_wrapped(t, s, uv: vec2<f32>, bounds: vec4<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec4<f32>` in `sample.wgsl`: `fract` of `uv.x` mapped into the slot's u range, `uv.y` clamped into the slot, `textureSampleGrad` with the unwrapped gradients scaled into the slot
  - Each trail and line is one item: texture slot (textured trails), `BatchShader::Line` or `LineTextured`, blend, filter, `lit` from `lighting()`, layers from the entity, world bounds = the points' box grown by the widest half width plus the effect amplitude
  - `DebugStats::draw_calls` = `batch.draw_count()`
- Tests in `tests/e2e/trail_renderer.rs` that are kept move to `tests/e2e/gpu_world.rs` through the engine: every style draws, a line draws, separate trails do not connect.

- [ ] **Step 1: Write the failing tests** (`tests/e2e/gpu_world.rs`)

```rust
#[test]
fn a_translucent_corner_is_no_darker_than_its_segments() {
  let scene = Fixture::new(|ctx| {
    let line = Line2D::new(
      vec![Vec2::new(4.0, 8.0), Vec2::new(40.0, 8.0), Vec2::new(40.0, 28.0)],
      Color::new(255, 255, 255, 128),
      6.0,
    )
    .with_join(LineJoin::Miter)
    .with_lighting(false);
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), line));
  });
  let Some(frame) = frame_of(scene, (48, 32)) else {
    return;
  };
  let (segment, corner) = (frame.get_pixel(20, 8).0, frame.get_pixel(40, 8).0);
  for c in 0..3 {
    assert!(segment[c].abs_diff(corner[c]) <= 2, "segment {segment:?} corner {corner:?}");
  }
}

#[test]
fn a_tiled_trail_repeats_its_texture_without_seams() {
  let scene = Fixture::new(|ctx| {
    // 8x2 texture: left half red, right half blue
    let mut data = Vec::new();
    for _ in 0..2 {
      for x in 0..8 {
        data.extend_from_slice(if x < 4 { &RED } else { &BLUE });
      }
    }
    let texture = SpriteHandle::from_sprite(Sprite::from_rgba(data, 8, 2), "tile-stripe");
    let mut trail = Trail2D::new(Color::WHITE, 6.0)
      .with_length(32)
      .with_fade(false)
      .with_texture(texture)
      .with_texture_mode(TextureMode::Tile)
      .with_tile_length(8.0)
      .with_filter(Filter::Nearest)
      .with_lighting(false);
    trail.populate_line(4.0, 8.0, 60.0, 8.0, 32);
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), trail));
  });
  let Some(frame) = frame_of(scene, (64, 16)) else {
    return;
  };
  let row: Vec<[u8; 4]> = (8..56).map(|x| frame.get_pixel(x, 8).0).collect();
  assert!(row.iter().all(|p| *p == RED || *p == BLUE), "every pixel is a texel: {row:?}");
  let changes = row.windows(2).filter(|w| w[0] != w[1]).count();
  assert!((10..=13).contains(&changes), "a stripe every 4 pixels: {changes} changes");
}

#[test]
fn an_additive_line_brightens_what_is_under_it() {
  let scene = Fixture::new(|ctx| {
    ctx.world.spawn((
      Transform::from_xy(16.0, 8.0),
      Renderable::sprite(solid("line-under", 32, [100, 100, 100, 255])),
    ));
    let line = Line2D::horizontal(8.0, 0.0, 32.0, Color::new(120, 60, 30, 255), 4.0)
      .with_blend(BlendMode::Additive)
      .with_z_order(1)
      .with_lighting(false);
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), line));
  });
  let Some(frame) = frame_of(scene, (32, 16)) else {
    return;
  };
  let p = frame.get_pixel(16, 8).0;
  assert!(p[0] > 120 && p[1] > 100 && p[2] > 100, "{p:?}");
}

#[test]
fn every_trail_style_draws() {
  let scene = Fixture::new(|ctx| {
    for (i, style) in [TrailStyle::Solid, TrailStyle::Soft, TrailStyle::Glow].into_iter().enumerate() {
      let y = 6.0 + i as f32 * 10.0;
      let mut trail = Trail2D::new(Color::new(30, 200, 60, 255), 6.0)
        .with_style(style)
        .with_fade(false)
        .with_lighting(false);
      trail.populate_line(4.0, y, 40.0, y, 8);
      ctx.world.spawn((Transform::from_xy(0.0, 0.0), trail));
    }
  });
  let Some(frame) = frame_of(scene, (48, 32)) else {
    return;
  };
  for y in [6, 16, 26] {
    assert!(frame.get_pixel(20, y).0[1] > 100, "style at row {y}");
  }
}

#[test]
fn separate_trails_do_not_connect() {
  let scene = Fixture::new(|ctx| {
    for y in [4.0, 28.0] {
      let mut trail = Trail2D::new(Color::new(30, 200, 60, 255), 4.0).with_fade(false).with_lighting(false);
      trail.populate_line(4.0, y, 40.0, y, 8);
      ctx.world.spawn((Transform::from_xy(0.0, 0.0), trail));
    }
  });
  let Some(frame) = frame_of(scene, (48, 32)) else {
    return;
  };
  assert!((10..22).all(|y| frame.get_pixel(20, y).0[1] < 100), "nothing between the trails");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --test e2e -- translucent_corner tiled_trail additive_line every_trail_style separate_trails 2>&1 | grep -E 'error\[|test result|panicked' | head`
Expected: compile errors (`with_blend`, `with_filter` on `Line2D`/`Trail2D`).

- [ ] **Step 3: Implement** the Produces block; delete the trail renderer, its shaders, its group machinery and its e2e file.

- [ ] **Step 4: Run them to see them pass, then the gate**

Expected: PASS. Gate: `compare` lists the pages with trails, lines, circles and debug overlays (corners joined, translucent overlaps even, premultiplied blending). Open each listed page beside its baseline; the only differences may be at corners, joins and overlaps. Refresh exactly those pages in `target/batch-baseline/demo-tour/` and record the list in the ledger. Rerun `compare` for `0`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: trails and lines draw through the batch as joined polylines

- trails get joins; no corner gaps or darker overlaps; no vertex cap
- trails and lines take blend and filter modes
- the separate trail renderer and its texture cache are gone"
```

### Task 11: `DebugDraw`

**Files:**
- Create: `src/core/render/debug_draw.rs`
- Modify: `src/core/render/mod.rs`, `src/core/engine/{mod.rs,extract.rs}` (resource, systems, per-camera build, end of frame), `src/core/scene/{mod.rs,context.rs}`, `src/core/config/debug.rs` (overlays become shapes), `src/core/skeleton/render.rs` (`draw_skeleton`), `src/lib.rs`, `examples/demos/skeleton/scenes/mod.rs`, `tests/e2e/gpu_world.rs` (`skeleton_debug_lines_draw_on_top`)
- Test: `src/core/render/debug_draw.rs` (unit), `tests/e2e/gpu_world.rs`

**Interfaces:**
- Produces:
  - `#[derive(Resource, Default)] pub struct DebugDraw` with `line(&mut self, a: Vec2, b: Vec2, color: Color)`, `polyline(&mut self, points: &[Vec2], color: Color)`, `rect(&mut self, center: Vec2, size: Vec2, color: Color)`, `circle(&mut self, center: Vec2, radius: f32, color: Color)`, `arrow(&mut self, from: Vec2, to: Vec2, color: Color)`, `cross(&mut self, at: Vec2, size: f32, color: Color)`, `width(&mut self, pixels: f32) -> DebugPen<'_>`, `layers(&mut self, layers: RenderLayers) -> DebugPen<'_>`; shapes are stored in two lists (fixed-step and frame) over one shared point buffer, no allocation per shape after warm-up
  - `pub struct DebugPen<'a>` with the same shape methods plus `width` and `layers` (chainable); defaults 1 screen pixel and `RenderLayers::default()`
  - `pub(crate)`: `begin_fixed_step(&mut self)` (clears the fixed-step list, marks fixed mode), `end_fixed_step(&mut self)`, `end_frame(&mut self)` (clears the frame list), `len(&self) -> usize`
  - Systems `begin_debug_fixed_step` (`FixedSet::Snapshot`, first) and `end_debug_fixed_step` (`FixedSet::Trails`, after `record_trail_points`); `present_frame` calls `end_frame` after drawing
  - `SceneContext::debug_draw(&mut self) -> Mut<'_, DebugDraw>`; the resource is inserted in `init_world_resources` and `init_world_resources_for_tests`
  - Per camera, each shape on its layers becomes a polyline with half width `pixels / 2 / view.scale`, `TrailStyle::Solid`, `LineCap::Butt`, `LineJoin::Miter`, unlit, z `i32::MAX`, `only_camera` = that camera; for odd pixel widths, line, rect, arrow and cross points are moved to that camera's pixel centres (through `world_to_screen`, `floor + 0.5`, `screen_to_world`); circles use `clamp(radius_px / 2, 12, 64)` segments
  - `DrawItem` gains `only_camera: Option<Entity>`, honoured by `DrawList::sees`
  - `overlay_shapes(world: &mut World, flags: OverlayFlags, debug: &mut DebugDraw)` replaces `overlay_lines`; `pub fn draw_skeleton(debug: &mut DebugDraw, def: &SkeletonDef, world_transforms: &[BoneWorldTransform], bone_color: Color, joint_color: Color, width: f32, joint_radius: f32)` replaces `skeleton_debug_lines`

- [ ] **Step 1: Write the failing tests**

`debug_draw.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn fixed_step_shapes_last_until_the_next_step() {
    let mut debug = DebugDraw::default();
    debug.begin_fixed_step();
    debug.line(Vec2::ZERO, Vec2::X, Color::WHITE);
    debug.end_fixed_step();
    assert_eq!(debug.len(), 1);
    debug.end_frame();
    assert_eq!(debug.len(), 1, "a frame without a step still draws it");
    debug.begin_fixed_step();
    assert_eq!(debug.len(), 0, "the next step starts afresh");
  }

  #[test]
  fn other_shapes_last_one_frame() {
    let mut debug = DebugDraw::default();
    debug.circle(Vec2::ZERO, 4.0, Color::WHITE);
    assert_eq!(debug.len(), 1);
    debug.end_frame();
    assert_eq!(debug.len(), 0);
  }

  #[test]
  fn a_pen_sets_width_and_layers() {
    let mut debug = DebugDraw::default();
    debug.width(3.0).layers(RenderLayers::layer(2)).rect(Vec2::ZERO, Vec2::ONE, Color::WHITE);
    let shape = debug.shapes().next().unwrap();
    assert_eq!((shape.width, shape.layers), (3.0, RenderLayers::layer(2)));
  }
}
```

`tests/e2e/gpu_world.rs`:

```rust
/// Draws a horizontal debug line every update (camera zoomed out 4x).
struct DebugLine;

impl Scene for DebugLine {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.camera().set_zoom(0.25);
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let centre = ctx.camera().position();
    ctx.debug_draw().line(centre - Vec2::new(40.0, 0.0), centre + Vec2::new(40.0, 0.0), Color::new(30, 200, 60, 255));
    SceneAction::Continue
  }
}

#[test]
fn a_debug_line_is_one_pixel_wide_when_zoomed_out() {
  let Some(mut engine) = start(EngineBuilder::new().resolution(64, 32).start_scene(DebugLine)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let rows = (0..32).filter(|&y| frame.get_pixel(32, y).0[1] > 150).count();
  assert_eq!(rows, 1);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib -p ryo2d -- render::debug_draw 2>&1 | grep -E 'error\[|test result' | head -3` and `cargo test -p ryo2d --test e2e -- debug_line_is_one_pixel 2>&1 | grep -E 'error\[|test result' | head -3`
Expected: compile errors (`DebugDraw`, `ctx.debug_draw`).

- [ ] **Step 3: Implement** the Produces block; migrate the overlays, the skeleton helper, its demo and its test.

- [ ] **Step 4: Run them to see them pass, then the gate**

Expected: PASS, with `debug_colliders_draw` and `skeleton_debug_lines_draw_on_top` (renamed for `draw_skeleton`) green. Gate: pages showing debug overlays or skeleton lines may change (pixel-centred, solid style); check each by eye, refresh them and record the list.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: ctx.debug_draw() for lines, rects, circles, arrows and crosses

- one screen pixel wide in every camera; fixed-step shapes last to the
  next step
- the engine's overlays and skeleton lines use it"
```

---

## Phase 5: parallax per camera

### Task 12: `ParallaxLayer` components

**Files:**
- Modify: `src/core/graphics/parallax.rs` (the component, its layout and its system next to `TileMode`), `src/core/graphics/mod.rs`, `src/lib.rs`, `src/core/engine/{mod.rs,extract.rs}`, `src/core/render/draw_list.rs` (parallax quads tagged `only_camera`), `examples/games/bullet-hell/src/scenes/{menu.rs,game/mod.rs}`
- Delete: `src/core/graphics/parallax_gpu.rs`
- Test: `src/core/graphics/parallax.rs` (unit), `tests/e2e/gpu_world.rs`

**Interfaces:**
- Produces:
  - `#[derive(Component, Debug, Clone)] pub struct ParallaxLayer { pub sprite: SpriteHandle, pub scroll: Vec2, pub auto_scroll: Vec2, pub offset: Vec2, pub tile: TileMode, pub scale: f32, pub tint: Color, pub z_order: i32, pub blend: BlendMode, pub filter: Filter, pub lighting: bool, pub scrolled: Vec2 }`
  - `ParallaxLayer::new(sprite: &SpriteHandle) -> Self` (scroll `Vec2::ZERO`, scale 1, tint white, lit) and `with_scroll(Vec2)`, `with_auto_scroll(Vec2)`, `with_offset(Vec2)`, `with_tile(TileMode)`, `with_scale(f32)`, `with_tint(Color)`, `with_z_order(i32)`, `with_blend(BlendMode)`, `with_filter(Filter)`, `with_lighting(bool)`
  - `pub(crate) const MAX_PARALLAX_TILES: usize = 1024`
  - `pub(crate) fn layout(layer: &ParallaxLayer, texels: Vec2, view: &CameraView, out: &mut Vec<SpriteInstance>)`: 2a's `GpuParallaxLayer::world_instances` with `camera = view.center * layer.scroll`, `total_offset_x = (camera.x + layer.scrolled.x + layer.offset.x) * scale`, `y = (layer.offset.y - camera.y - layer.scrolled.y) * scale`; it returns nothing when the scaled size is not finite and positive, and stops at `MAX_PARALLAX_TILES`
  - `pub(crate) fn advance_parallax(time: Res<Time>, mut layers: Query<&mut ParallaxLayer>)` in `PostUpdateSet::Extract` (before `update_effects_system`): `scrolled += auto_scroll * time.game.delta()`
  - At extraction each layer is laid out for every camera on its layers (`RenderLayers` on its entity, default layer 0); its quads are `DrawKind::Sprite` items with `only_camera` = that camera

- [ ] **Step 1: Write the failing tests**

`parallax.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::{Camera2d, CameraView};

  fn view(center: Vec2) -> CameraView {
    CameraView::compute(center, 0.0, &Camera2d::default(), Vec2::ZERO, Vec2::new(64.0, 64.0))
  }

  fn layer() -> ParallaxLayer {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(16, 16, 255, 255, 255, 255), "plx");
    ParallaxLayer::new(&sprite).with_scroll(Vec2::splat(0.5)).with_tile(TileMode::InfiniteScroll)
  }

  #[test]
  fn each_camera_lays_the_layer_out_from_its_own_view() {
    let (mut a, mut b) = (Vec::new(), Vec::new());
    layout(&layer(), Vec2::splat(16.0), &view(Vec2::new(32.0, 32.0)), &mut a);
    layout(&layer(), Vec2::splat(16.0), &view(Vec2::new(1032.0, 32.0)), &mut b);
    assert!(!a.is_empty() && !b.is_empty());
    // Every quad stays inside its own camera's view
    for (quads, centre) in [(&a, 32.0), (&b, 1032.0)] {
      for q in quads {
        assert!((q.transform[2][0] - centre).abs() <= 32.0 + 16.0, "{}", q.transform[2][0]);
      }
    }
  }

  #[test]
  fn a_degenerate_scale_draws_nothing_or_a_bounded_count() {
    for scale in [0.0, -1.0, f32::NAN, 1e-9] {
      let mut out = Vec::new();
      layout(&layer().with_scale(scale), Vec2::splat(16.0), &view(Vec2::splat(32.0)), &mut out);
      assert!(out.len() <= MAX_PARALLAX_TILES, "{scale}: {}", out.len());
    }
  }
}
```

`tests/e2e/gpu_world.rs`:

```rust
#[test]
fn split_screen_cameras_each_see_their_own_parallax() {
  let scene = Fixture::new(|ctx| {
    // A screen-fixed layer (scroll 0) sits at each viewport's top-left
    let green = solid("plx-green", 8, GREEN);
    ctx.world.spawn(ParallaxLayer::new(&green));
    ctx.camera().set_viewport(Viewport::new(0.0, 0.0, 0.5, 1.0));
    ctx.world.spawn((
      Transform::from_xy(1000.0, 0.0),
      Camera2d { viewport: Viewport::new(0.5, 0.0, 0.5, 1.0), order: 1, ..Default::default() },
    ));
  });
  let Some(frame) = frame_of(scene, (64, 32)) else {
    return;
  };
  assert_eq!(frame.get_pixel(2, 2).0, GREEN, "left camera");
  assert_eq!(frame.get_pixel(34, 2).0, GREEN, "right camera");
}
```

(With the main camera's `Projection::Fit` over 64x32 in a 32-wide viewport the layer may be scaled; sample inside its top-left texel at `(1, 1)` and `(33, 1)` if `(2, 2)` misses.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib -p ryo2d -- graphics::parallax 2>&1 | grep -E 'error\[|test result' | head -3`
Expected: compile errors (`ParallaxLayer`, `layout`).

- [ ] **Step 3: Implement** the Produces block; migrate bullet-hell (its builders become spawned layers in `on_enter`, its `update(dt)` and `render(gpu)` calls go).

- [ ] **Step 4: Run them to see them pass, then the gate**

Expected: PASS. Gate: `compare 0` (no bundled demo uses parallax); check bullet-hell's frame 60 against its baseline by eye.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: parallax layers are components laid out for every camera

- GpuParallaxBackground, its builder and its layer type go
- auto-scroll runs on game time; tiles are capped per layer"
```

---

## Phase 6: leftovers, documentation and verification

### Task 13: Extraction reuses its scratch space

**Files:**
- Modify: `src/core/render/entities.rs` (the drawables vector), `src/core/graphics/sprite_gpu.rs` (`BatchCollector::push_to`'s texture list), `src/core/engine/{mod.rs,extract.rs}` (camera globals, occluders, parallax instances, polyline points), `src/core/lighting/renderer.rs` (light instances), `src/core/particles/mod.rs` (`collect_for_render`'s emitter map)
- Test: `benches/foundation.rs` ("headless frame, 10k sprites"), `src/core/testing/gpu_world.rs` (`ten_thousand_sprites_few_draws`)

**Interfaces:**
- Produces: each owner keeps its scratch `Vec`/`HashMap` as a field, cleared and refilled each frame (`EntityDrawer::items`, `BatchCollector::textures`, `Engine::{camera_globals, occluders, parallax, poly_points}`, `LightingRenderer::instances`, `ParticleSystem::looks`). Iteration order and results are unchanged.

- [ ] **Step 1: Measure before**

Run: `cargo bench --bench foundation -- "headless frame" 2>&1 | grep -A1 '^headless frame' | grep time`
Expected: a line to compare with Task 0's.

- [ ] **Step 2: Implement** the Produces block.

- [ ] **Step 3: Measure after, then the gate**

Run the Step 1 command and `cargo test --lib -p ryo2d -- ten_thousand_sprites_few_draws 2>&1 | grep 'test result'`.
Expected: within 20% of the Task 0 line (lower is fine); the draw-count test passes. Gate: `compare 0`.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "perf: frame extraction reuses its buffers instead of allocating"
```

### Task 14: Documentation

**Files:**
- Modify: `.claude/skills/{ryo-api,ryo-renderer,ryo-game-dev,ryo-particles,ryo-engine-dev}/SKILL.md` and their verbatim copies in `src/cli/skills/`; `src/core/{render,graphics,shaders,trail}/AGENTS.md`; `src/core/AGENTS.md`; `docs/architecture/overview.md`; `README.md`
- Test: the copy check and the CLI tests

- [ ] **Step 1: Find every stale statement**

Run: `grep -rnE 'GpuParallax|add_sprite_instance|TrailRenderer|LineJoin::None|skeleton_debug_lines|overlay_lines|ALPHA_BLENDING|MAX_TRAIL_VERTICES|trail group|65536|sample_masked\(in' .claude/skills docs/architecture src/core README.md --include='*.md'`
Expected: a list to rewrite.

- [ ] **Step 2: Rewrite**

- `ryo-api`: `BlendMode` and `Filter` (the table of modes, the default filter by presentation); `Renderable::{blend, filter, lighting}` and builders; `ParticleEmitter`'s; `Trail2D`/`Line2D` `with_blend`/`with_filter` and `Trail2D::with_join`; `SpriteDraw`, `GpuRenderContext::add_sprite`, `add_custom_sprite(.., SpriteDraw)`, `BatchCollector::add(.., SpriteDraw)`; `ParallaxLayer`; `DebugDraw` and `DebugPen` (`ctx.debug_draw()`), lifetimes; `draw_skeleton`; removals (`GpuParallaxBackground`, `GpuParallaxBuilder`, `GpuParallaxLayer`, `add_sprite_instance`, `TrailRenderer`, `LineJoin::None`, `skeleton_debug_lines`).
- `ryo-renderer`: the batch (`render/batch`): keys, the two streams, planning per camera, the pipeline cache, bind groups made with their resources and the counter, mipmapped atlas slots (8-pixel borders and grid, four levels, linear-light averaging), the two samplers, wrapped tiling, the polyline builder, the line shader, the includes and `compose`.
- `ryo-game-dev`: recipes for additive particles (`with_blend(BlendMode::Additive)`), a debug overlay from a system, parallax that works in split screen, crisp versus smooth sprites (`with_filter`).
- `ryo-particles`: `blend`, `filter`, `lighting` on emitters. `ryo-engine-dev`: the frame diagram's batch and the module map.
- AGENTS.md files and `overview.md`: the batch in the frame, the removed trail renderer, the includes, the bind-group rule.
- Copy each changed skill verbatim to `src/cli/skills/<name>.md`.

- [ ] **Step 3: Check, commit**

Run: `for d in .claude/skills/*/; do n=$(basename $d); cmp -s $d/SKILL.md src/cli/skills/$n.md || echo "copy differs: $n"; done; cargo test -p ryo2d --lib -- cli 2>&1 | grep 'test result'`
Expected: no `copy differs`; tests pass. Then the Step 1 grep finds only current API.

```bash
git add -A
git commit -m "docs: the batch, blend and filter modes, polylines, parallax and debug draw"
```

### Task 15: Verification, review and merge

**Files:** none new.

- [ ] **Step 1: Everything green**

Run the gate. Expected: `0` failed (more tests than Task 0's 2560), no lint lines, `compare 0` against the baseline as refreshed in Tasks 10-12.

- [ ] **Step 2: Games and the web**

Capture the five games at frame 60 into `target/batch-final/games/` (Task 0's loop) and compare each with its baseline by eye; differences only of the kinds the ledger records (corners and overlaps of trails and lines, overlays, parallax). Build the lighting demo with trunk (`cd examples/demos/lighting && trunk build`), serve `dist/` and check it in Chromium, WebKit and Firefox through the Playwright tools: it draws, animates, takes arrow keys, the console has no engine errors.

- [ ] **Step 3: Benchmark**

Run: `cargo bench --bench foundation -- "headless frame" 2>&1 | grep -A1 '^headless frame' | grep time`
Expected: within 20% of the Task 0 line.

- [ ] **Step 4: Whole-branch review**

Build the review package from `git merge-base main HEAD` and dispatch the reviewer on the most capable model with the spec, this plan, the Review Focus section verbatim and the ledger's `Ruling:` lines. Re-grade findings by effect; one fix pass for Critical and Important, each test-first; minors to the ledger.

- [ ] **Step 5: Finish**

Use `superpowers:finishing-a-development-branch`. Merging into `main` is the owner's call. After the merge, delete this plan (the spec stays).
