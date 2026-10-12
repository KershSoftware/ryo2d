# Renderer Core (2a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the renderer to wgpu 30, build the engine for the browser (WebGPU), give every texture an integer identity, separate screen space from physical pixels with an opt-in pixel-art mode, and turn the camera into entities with viewports and render-to-texture.

**Architecture:** In place, in ordered steps that each keep every test and demo green (spec approach A). Drawables are gathered once per frame in world space; each camera (an entity with `Camera2d`) culls them and runs the camera passes (`clear`, `world`, `lighting`, `unlit_world`) into its own image with its own view-projection uniform; frame passes (`cameras`, `transition`, `post_process`, `ui`, `composite`, `capture`) run once. One `ScreenMapping` converts between window pixels and screen space; one `TextureRegistry` keys every GPU texture by `TextureId`.

**Tech Stack:** Rust 2024, `wgpu`/`naga` 30.0.1, `winit` 0.30.13 (0.31 is beta), `bevy_ecs` 0.20, `web-time` 1.1, `wasm-bindgen` 0.2.129 and `wasm-bindgen-futures` 0.4.79 (wasm only), `getrandom` 0.4 with `wasm_js` (wasm only), `trunk` for serving the web build.

**Spec:** `docs/specs/2026-10-12-renderer-core-design.md`

## Global Constraints

- Branch `renderer-core` in a git worktree; `main` stays green until the final merge (spec: Migration).
- `wgpu` and `naga` are `30.0.1`; `winit` stays `0.30` (spec: Tech; Out of scope: winit 0.31).
- `Limits::default()` and no wgpu features; nothing native-only (spec: GPU foundation).
- No blocking GPU call (`pollster::block_on`, `device.poll(Wait)`) on any path that runs on the web; readback for capture and `HeadlessEngine` stays synchronous on native only (spec: GPU foundation).
- Resolution-independent is the default; pixel art is opt-in through `EngineBuilder::pixel_art(w, h)` or `pixel_art = true` in `Ryo.toml`; every bundled demo and game opts in (spec: Resolution model).
- Networking is not changed beyond `cfg` gates on its module declaration and prelude block (spec: Web build).
- Game code never names `bevy_ecs`; only `Cargo.toml`, `src/core/ecs/kernel.rs` and the hidden prelude re-export do (root `AGENTS.md`, foundation spec).
- No `unwrap`/`expect` in library code outside tests; LF line endings; two-space indentation (`rustfmt.toml`).
- Commit messages: `type: brief description`, no co-author lines, no mention of AGENTS.md files (root `AGENTS.md`).
- Every task ends with `cargo test --workspace` and `./dev.sh lint` green; from Task 4 on, `./dev.sh lint` includes the wasm check.

## Review Focus

- A window minimised to 0x0 in resolution-independent mode must skip frames without recreating zero-sized targets or panicking, and resume at the restored size. Test pinned to Task 9.
- `Camera2d` with zoom 0, negative, NaN or infinite, or `Projection::Fit { width: 0.0, .. }`, must produce a finite view (zoom clamped to 0.01..=100, a zero fit side treated as 1), never NaN vertices. Test pinned to Task 10.
- `RenderTargets::create(0, 0)` or a side above the device limit must clamp to 1..=`max_texture_dimension_2d` (and say so once) rather than fail validation. Test pinned to Task 17.
- Spawning and despawning cameras across frames must free their per-camera GPU uniforms and bind groups; after a despawn the renderer holds entries only for live cameras. Test pinned to Task 15.
- In pixel-art mode a window smaller than the target scales at 1 (never below) and the target is centred and cropped; the mouse still maps to target pixels. Test pinned to Task 8.

---

## Phase 0: branch and baseline

### Task 0: Worktree and reference captures

**Files:** none in the repo; references under `target/renderer-baseline/` (ignored).

- [ ] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/renderer-core -b renderer-core main
cd .claude/worktrees/renderer-core
```

All later tasks run inside this worktree.

- [ ] **Step 2: Baseline tests**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{p+=$4; f+=$6} END {print p, f}'`
Expected: `2540 0` (passed, failed; counts can be higher if `main` moved, failures must be 0).

- [ ] **Step 3: Reference captures**

```bash
./dev.sh demos:tour
mkdir -p target/renderer-baseline
rm -rf target/renderer-baseline/demo-tour && cp -R target/demo-tour target/renderer-baseline/demo-tour
for g in breakout bullet-hell flappy leap-king nigh_t; do
  (cd examples/games/$g && RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=60 RYO_CAPTURE_EXIT=1 \
    RYO_CAPTURE_DIR="$PWD/../../../target/renderer-baseline/games/$g" cargo run -q -p $g)
done
ls target/renderer-baseline/demo-tour | wc -l
```

Expected: 33 demo directories; each game directory holds `frame-000060.png`.

- [ ] **Step 4: Benchmark baseline**

Run: `cargo bench --bench foundation -- "headless frame" 2>&1 | grep -A1 '^headless frame' | grep time`
Expected: about `1.1-1.2 ms` for 10k sprites; record the line in the ledger (Task 19 compares against it).

- [ ] **Step 5: A comparison script used by later tasks**

Create `target/renderer-baseline/compare.sh` (not committed):

```bash
#!/usr/bin/env bash
# Usage: compare.sh  -> lists every demo-tour PNG that differs from the baseline
set -u
base=target/renderer-baseline/demo-tour
now=target/demo-tour
status=0
for f in $(cd "$base" && find . -name '*.png' | sort); do
  if [ ! -f "$now/$f" ]; then echo "missing $f"; status=1
  elif ! cmp -s "$base/$f" "$now/$f"; then echo "differs $f"; status=1; fi
done
exit $status
```

Run: `chmod +x target/renderer-baseline/compare.sh && ./dev.sh demos:tour >/dev/null && target/renderer-baseline/compare.sh; echo $?`
Expected: no lines, `0` (the tour is deterministic).

---

## Phase 1: wgpu 30

### Task 1: Move to wgpu 30 with no behaviour change

**Files:**
- Modify: `Cargo.toml` (`wgpu = "30.0.1"`, `naga = { version = "30.0.1", features = ["wgsl-in"] }`)
- Modify: every file that names `wgpu::` (find them with `grep -rln 'wgpu::' src tests examples crates benches`), chiefly `src/core/graphics/{renderer,sprite_gpu,compositor,atlas,parallax_gpu}.rs`, `src/core/lighting/renderer.rs`, `src/core/trail/renderer.rs`, `src/core/shaders/{pipeline,manager,effects}.rs`, `src/core/render/**`, `src/core/capture/readback.rs`, `src/core/testing/gpu.rs`, `src/core/windows/*.rs`, `src/core/engine/{extract,window}.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: the same public API on wgpu 30 (`ryo2d::wgpu` re-export now points at 30). `WgpuRenderer::render_frame` returns `Result<(), FrameError>` where `pub(crate) enum FrameError { Skip, Reconfigure, Recreate, Fatal(String) }` (replacing `wgpu::SurfaceError`, which no longer exists); Task 2 refines the mapping.

- [ ] **Step 1: Bump and list the breakage**

Edit `Cargo.toml` as above, then run: `cargo check -p ryo2d --all-targets 2>&1 | grep -E '^error' | sort | uniq -c | sort -rn | head -40`
Expected: errors from the renamed APIs below.

- [ ] **Step 2: Apply the wgpu 22 to 30 rules**

| wgpu 22 | wgpu 30 |
|---|---|
| `Instance::new(InstanceDescriptor { backends: Backends::all(), ..Default::default() })` | `InstanceDescriptor::default()` is gone: build it with `InstanceDescriptor::new_without_display_handle()` (headless, `testing/gpu.rs`) or `InstanceDescriptor::new_with_display_handle(Box::new(window.clone()))` (window renderer), set `backends = Backends::all()`, pass by value to `Instance::new`. Check the constructor names against docs.rs/wgpu/30.0.1 before editing. |
| `request_adapter(..).await` returns `Option<Adapter>` | returns `Result<Adapter, RequestAdapterError>` |
| `request_device(&desc, None)` | `request_device(&desc)`; `DeviceDescriptor` gains `experimental_features: wgpu::ExperimentalFeatures::default()` and `trace: wgpu::Trace::Off` |
| `surface.get_current_texture()` returns `Result<SurfaceTexture, SurfaceError>` | returns `CurrentSurfaceTexture::{Success(t), Suboptimal(t), Timeout, Occluded, Outdated, Lost, Validation}` |
| `output.present()` | `queue.present(output)` |
| `SurfaceConfiguration { .. }` | add `color_space: wgpu::SurfaceColorSpace::Auto` |
| `RenderPassColorAttachment { view, resolve_target, ops }` | add `depth_slice: None` |
| `RenderPassDescriptor { .. }` | add `multiview_mask: None` |
| `ImageCopyTexture`, `ImageCopyBuffer`, `ImageDataLayout` | `TexelCopyTextureInfo`, `TexelCopyBufferInfo`, `TexelCopyBufferLayout` |
| `device.poll(Maintain::Wait)` | `device.poll(PollType::wait_indefinitely())` returning `Result<PollStatus, PollError>` (map the error to `CaptureError::Readback`) |
| `VertexState { buffers: &[layout_a, layout_b], entry_point: "vs_main", .. }` | `buffers: &[Some(layout_a), Some(layout_b)]`, `entry_point: Some("vs_main")` (same for `FragmentState`) |
| `PipelineLayoutDescriptor { bind_group_layouts: &[&a, &b], push_constant_ranges: &[] }` | `bind_group_layouts: &[Some(&a), Some(&b)]`, `immediate_size: 0` |
| `RenderPipelineDescriptor { multiview: None, .. }` | `multiview_mask: None` |
| `PipelineCompilationOptions { constants: &HashMap::new(), .. }` / `Default::default()` | `Default::default()` (constants are `&[(&str, f64)]`) |
| `pass.set_bind_group(0, &group, &[])` | unchanged (takes `impl Into<Option<&BindGroup>>`) |
| `device.push_error_scope(f); ...; pollster::block_on(device.pop_error_scope())` | `let scope = device.push_error_scope(f); ...; pollster::block_on(scope.pop())` (Task 3 removes the blocking) |
| WGSL integer `@location` outputs | add `@interpolate(flat)` where missing |

`SurfaceError` call sites (`engine/extract.rs` `present_frame`, `graphics/renderer.rs` `PresentTarget::acquire`): map `CurrentSurfaceTexture` to `FrameError` exactly as today's behaviour: `Timeout` to `Skip`; `Outdated` and `Suboptimal` to `Reconfigure` (the old code reconfigured on `Outdated`; `Suboptimal` draws then reconfigures); `Lost` to `Recreate` (today: resize to the window size); `Occluded` to `Skip`; `Validation` to `Fatal`.

- [ ] **Step 3: Build, test, lint**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{p+=$4; f+=$6} END {print p, f}' && ./dev.sh lint 2>&1 | grep -E '^(warning|error)' | grep -v 'future version' | head`
Expected: the Task 0 count with `0` failed; no lint lines.

- [ ] **Step 4: Pixels unchanged**

Run: `./dev.sh demos:tour >/dev/null && target/renderer-baseline/compare.sh; echo $?`
Expected: `0` with no lines. Any difference is a migration bug; find it with `superpowers:systematic-debugging` before going on.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: move to wgpu 30"
```

### Task 2: GPU errors are returned, surfaces handled, non-sRGB output encoded

**Files:**
- Create: `src/core/graphics/gpu_error.rs`
- Modify: `src/core/graphics/{mod.rs,renderer.rs,compositor.rs}`, `src/core/shaders/core/fullscreen_quad.wgsl`, `src/core/error.rs`, `src/core/engine/{mod.rs,window.rs,extract.rs,frame.rs}`, `src/core/testing/gpu.rs`
- Test: `src/core/graphics/gpu_error.rs` (unit), `tests/e2e/gpu_basics.rs` (sRGB fallback)

**Interfaces:**
- Produces:
  - `pub enum GpuError { NoAdapter(String), RequestDevice(String), CreateSurface(String), DeviceLost(String) }` (`thiserror`), re-exported from `ryo2d::prelude`; `EngineError::Gpu(#[from] GpuError)`.
  - `pub async fn WgpuRenderer::new(window, settings: RendererSettings) -> Result<WgpuRenderer, GpuError>` (`RendererSettings { vsync: Option<bool> }` for now; Task 8 adds presentation fields).
  - `pub(crate) enum SurfaceState { Success, Suboptimal, Timeout, Occluded, Outdated, Lost, Validation }` and `pub(crate) fn surface_action(state: SurfaceState) -> SurfaceAction` with `pub(crate) enum SurfaceAction { Draw { then_reconfigure: bool }, Skip, Reconfigure, Recreate, Fatal }`.
  - `pub(crate) struct GpuFault(Arc<Mutex<Option<String>>>)` with `record(&self, message)` and `take(&self) -> Option<String>`; the device's uncaptured-error handler and lost callback record into it; `Engine::frame` returns `FrameOutcome::Fatal` when it holds a message.
  - `pub fn WgpuRenderer::new_offscreen_with_format(device, queue, width, height, format: wgpu::TextureFormat) -> WgpuRenderer` (test helper; `new_offscreen` keeps `Rgba8UnormSrgb`).

- [ ] **Step 1: Write the failing unit tests** (`src/core/graphics/gpu_error.rs`)

```rust
//! GPU setup and surface failures as values, not panics.

use std::sync::{Arc, Mutex};

/// Why the GPU could not be set up or kept running.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GpuError {
  #[error("no compatible GPU adapter: {0}")]
  NoAdapter(String),
  #[error("the GPU device could not be created: {0}")]
  RequestDevice(String),
  #[error("the window surface could not be created: {0}")]
  CreateSurface(String),
  #[error("the GPU device was lost: {0}")]
  DeviceLost(String),
}

/// What the surface reported when asked for the next frame's texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceState {
  Success,
  Suboptimal,
  Timeout,
  Occluded,
  Outdated,
  Lost,
  Validation,
}

/// What the frame does about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceAction {
  Draw { then_reconfigure: bool },
  Skip,
  Reconfigure,
  Recreate,
  Fatal,
}

pub(crate) fn surface_action(state: SurfaceState) -> SurfaceAction {
  todo!()
}

/// A GPU error reported by a callback, picked up by the next frame.
#[derive(Clone, Default)]
pub(crate) struct GpuFault(Arc<Mutex<Option<String>>>);

impl GpuFault {
  pub(crate) fn record(&self, message: impl Into<String>) {
    todo!()
  }

  pub(crate) fn take(&self) -> Option<String> {
    todo!()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn every_surface_state_has_an_action() {
    use SurfaceAction::*;
    assert_eq!(surface_action(SurfaceState::Success), Draw { then_reconfigure: false });
    assert_eq!(surface_action(SurfaceState::Suboptimal), Draw { then_reconfigure: true });
    assert_eq!(surface_action(SurfaceState::Timeout), Skip);
    assert_eq!(surface_action(SurfaceState::Occluded), Skip);
    assert_eq!(surface_action(SurfaceState::Outdated), Reconfigure);
    assert_eq!(surface_action(SurfaceState::Lost), Recreate);
    assert_eq!(surface_action(SurfaceState::Validation), Fatal);
  }

  #[test]
  fn a_fault_is_taken_once() {
    let fault = GpuFault::default();
    assert_eq!(fault.take(), None);
    fault.clone().record("device lost");
    assert_eq!(fault.take().as_deref(), Some("device lost"));
    assert_eq!(fault.take(), None);
  }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib -p ryo2d -- gpu_error 2>&1 | grep -E 'test result|panicked'`
Expected: both FAIL at `todo!()`.

- [ ] **Step 3: Implement**

```rust
pub(crate) fn surface_action(state: SurfaceState) -> SurfaceAction {
  match state {
    SurfaceState::Success => SurfaceAction::Draw { then_reconfigure: false },
    SurfaceState::Suboptimal => SurfaceAction::Draw { then_reconfigure: true },
    SurfaceState::Timeout | SurfaceState::Occluded => SurfaceAction::Skip,
    SurfaceState::Outdated => SurfaceAction::Reconfigure,
    SurfaceState::Lost => SurfaceAction::Recreate,
    SurfaceState::Validation => SurfaceAction::Fatal,
  }
}

impl GpuFault {
  pub(crate) fn record(&self, message: impl Into<String>) {
    if let Ok(mut slot) = self.0.lock() {
      slot.get_or_insert(message.into());
    }
  }

  pub(crate) fn take(&self) -> Option<String> {
    self.0.lock().ok().and_then(|mut slot| slot.take())
  }
}
```

Wire it in:
- `WgpuRenderer::new` returns `Result<Self, GpuError>`: `create_surface` errors to `CreateSurface`, `request_adapter` errors to `NoAdapter`, `request_device` errors to `RequestDevice`. No `unwrap`/`expect` remains in `renderer.rs`.
- After `request_device`: `device.on_uncaptured_error(..)` records `error.to_string()` into the renderer's `GpuFault`; the device-lost callback (`device.set_device_lost_callback`, check the 30.0.1 name) records `GpuError::DeviceLost(message).to_string()`.
- `PresentTarget::acquire` maps `CurrentSurfaceTexture` to `SurfaceState`, then `surface_action`; `present_frame` (`engine/extract.rs`) does `Skip` (no draw), `Reconfigure`, `Recreate`, `Fatal` and, for `Draw { then_reconfigure: true }`, draws then sets `surface_needs_reconfigure`.
- `Engine::frame` checks `renderer.fault().take()` first and returns `FrameOutcome::Fatal(message)`.
- `engine/window.rs` `resumed`: on `Err(error)` from `WgpuRenderer::new`, log it with `engine_error!`, store it in `self.startup_error`, and `event_loop.exit()`. `Engine::run` returns `Err(EngineError::Gpu(error))` when `startup_error` is set.
- `testing/gpu.rs`: `create()` maps failures to `None` as today, now through the `Result`s.

- [ ] **Step 4: The sRGB fallback test** (append to `tests/e2e/gpu_basics.rs`)

```rust
/// A mid-grey sprite must come out mid-grey on a target without an sRGB
/// format: the composite encodes to sRGB itself, so the output is not dark.
#[test]
fn a_non_srgb_target_still_shows_srgb_colours() {
  use ryo2d::testing::{HeadlessEngine, offscreen_with_format};
  struct Grey;
  impl Scene for Grey {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let grey = SpriteHandle::from_sprite(Sprite::solid_rect(32, 32, 128, 128, 128, 255), "grey");
      ctx.world.spawn((Transform::from_xy(16.0, 16.0), Renderable::sprite(grey)));
    }
  }
  let builder = EngineBuilder::new().resolution(32, 32).start_scene(Grey);
  let Some(mut engine) =
    HeadlessEngine::new_or_skip_with(builder, offscreen_with_format(wgpu::TextureFormat::Rgba8Unorm))
  else {
    return;
  };
  engine.step_frames(2).unwrap();
  let pixel = engine.frame().unwrap().get_pixel(16, 16).0;
  assert!((i32::from(pixel[0]) - 128).abs() <= 2, "{pixel:?}");
}
```

This needs a small test hook in `src/core/testing/headless.rs`: `pub struct OffscreenOptions { format: wgpu::TextureFormat }`, `pub fn offscreen_with_format(format) -> OffscreenOptions`, and `HeadlessEngine::new_or_skip_with(builder, options)` (`new_or_skip` uses `Rgba8UnormSrgb`). Export `offscreen_with_format` from `ryo2d::testing`.

Run: `cargo test -p ryo2d --test e2e -- a_non_srgb_target 2>&1 | grep -E 'test result|panicked' -A1`
Expected: FAIL (the pixel is about 55, linear written as is).

- [ ] **Step 5: Encode in the composite**

`fullscreen_quad.wgsl` gains `flags: vec4<f32>` after `transform` in `ViewportUniform`; `fs_main` returns `encode_srgb(color)` when `viewport.flags.x > 0.5`:

```wgsl
fn encode_srgb(c: vec4<f32>) -> vec4<f32> {
    let lo = c.rgb * 12.92;
    let hi = 1.055 * pow(c.rgb, vec3<f32>(1.0 / 2.4)) - 0.055;
    return vec4<f32>(select(hi, lo, c.rgb <= vec3<f32>(0.0031308)), c.a);
}
```

`Compositor`'s `ViewportUniform` becomes `{ transform: [f32; 4], flags: [f32; 4] }`; `flags[0] = 1.0` when `!target_format.is_srgb()`.

Run: `cargo test -p ryo2d --test e2e -- a_non_srgb_target 2>&1 | grep 'test result'`
Expected: PASS.

- [ ] **Step 6: Full suite, pixels, commit**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{f+=$6} END {print f}' && ./dev.sh lint 2>&1 | grep -cE '^(warning|error)'; ./dev.sh demos:tour >/dev/null && target/renderer-baseline/compare.sh; echo $?`
Expected: `0`, `1` (the future-incompat note only, or `0`), compare `0`.

```bash
git add -A
git commit -m "fix: GPU setup returns errors and surface states are handled

- GpuError for adapter, device, surface and device loss; run() returns it
- uncaptured GPU errors and device loss end the run cleanly
- every surface state has a defined action
- a non-sRGB output target is encoded in the composite"
```

### Task 3: Custom shaders never block on the GPU

**Files:**
- Create: `src/core/shaders/scope.rs`
- Modify: `src/core/graphics/sprite_gpu.rs` (`CustomPipelines::build`, about L438-485), `src/core/shaders/pipeline.rs` (about L249-282), `src/core/shaders/mod.rs`
- Test: `src/core/shaders/scope.rs` (unit), `tests/e2e/custom_shaders.rs`

**Interfaces:**
- Produces: `pub(crate) struct PendingCheck { future: Pin<Box<dyn Future<Output = Option<wgpu::Error>> + 'static>> }` (on wasm the future is not `Send`; keep the struct out of `Send` bounds) with `pub(crate) fn poll(&mut self) -> Poll<Option<String>>` using `std::task::Waker::noop()`. A custom pipeline is `Pending(PendingCheck)`, `Ready(pipeline)` or `Failed`; a sprite with a `Pending` or `Failed` shader draws plain; `Pending` resolves on a later frame's `prepare`.

- [ ] **Step 1: Write the failing unit test** (`src/core/shaders/scope.rs`)

```rust
//! A GPU error scope checked on later frames instead of waited on.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

pub(crate) struct PendingCheck {
  future: Pin<Box<dyn Future<Output = Option<String>>>>,
}

impl PendingCheck {
  pub(crate) fn new(future: impl Future<Output = Option<String>> + 'static) -> Self {
    Self { future: Box::pin(future) }
  }

  /// `Ready(None)`: the GPU accepted it. `Ready(Some(message))`: it did not.
  pub(crate) fn poll(&mut self) -> Poll<Option<String>> {
    todo!()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_ready_check_reports_without_blocking() {
    let mut ok = PendingCheck::new(async { None });
    assert_eq!(ok.poll(), Poll::Ready(None));
    let mut bad = PendingCheck::new(async { Some("invalid".to_string()) });
    assert_eq!(bad.poll(), Poll::Ready(Some("invalid".to_string())));
  }

  #[test]
  fn a_check_that_is_not_ready_stays_pending() {
    let mut never = PendingCheck::new(std::future::pending());
    assert_eq!(never.poll(), Poll::Pending);
  }
}
```

Run: `cargo test --lib -p ryo2d -- shaders::scope 2>&1 | grep -E 'test result|panicked'`
Expected: FAIL at `todo!()`.

- [ ] **Step 2: Implement**

```rust
  pub(crate) fn poll(&mut self) -> Poll<Option<String>> {
    let mut context = Context::from_waker(Waker::noop());
    self.future.as_mut().poll(&mut context)
  }
```

Run the same command. Expected: PASS.

- [ ] **Step 3: Pin the one-frame delay** (append to `tests/e2e/custom_shaders.rs`; it fails until Step 4)

The file's `render` helper already steps two frames, so the existing tests keep passing: frame 1 builds the pipeline and its check, frame 2's `prepare` sees it resolved. Add:

```rust
#[test]
fn a_custom_shader_applies_once_the_gpu_has_accepted_it() {
  let shader = ShaderHandle::from_source("t3-pending.wgsl", INVERT);
  let scene = Fixture::entities(move |ctx| {
    ctx.world.spawn((
      Transform::from_xy(10.0, 20.0),
      Renderable::sprite(solid("t3-pending-red", 20, RED))
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(shader.clone())),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  engine.step_frames(1).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(20, 30).0,
    RED,
    "plain while the GPU check is pending"
  );
  engine.step_frames(1).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(20, 30).0, [35, 225, 215, 255]);
}
```

A shader the device rejects after `naga` accepted it cannot be built through the engine's wrapper (naga catches every mismatch the wrapper allows), so that path is covered by the `PendingCheck` unit tests; the existing invalid-shader test keeps covering naga rejections.

Run: `cargo test -p ryo2d --test e2e -- custom 2>&1 | grep -E 'test result|panicked'`
Expected: the new test FAILS its first assertion (today the shader applies in frame 1); the others pass.

- [ ] **Step 4: Use it for custom sprite and effect pipelines**

In `CustomPipelines::build` (sprite) and the custom effect builder (`shaders/pipeline.rs`), replace `push_error_scope` ... `pollster::block_on(..pop..)` with:

```rust
let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
// create_shader_module and create_render_pipeline as today
let check = PendingCheck::new(async move { scope.pop().await.map(|error| error.to_string()) });
CompiledSource { ready: CustomState::Pending { pipeline, check }, last_used }
```

`CustomState { Pending { pipeline, check }, Ready(CustomPipeline), Failed }`. At the start of `prepare_world` (and of the effect chain), poll every `Pending`: `Ready(None)` becomes `Ready`; `Ready(Some(message))` becomes `Failed` and calls `custom::report_once` with a `ShaderError { shader, line: None, column: None, message }`. `shade_custom` returns `None` (draw plain) unless `Ready`. On native the scope resolves on the next `prepare`, so a valid custom shader appears one frame later than today.

Run: `cargo test -p ryo2d --test e2e -- custom 2>&1 | grep -E 'test result|panicked'`
Expected: PASS. `grep -rn 'pollster' src/core/graphics src/core/shaders` prints nothing.

- [ ] **Step 5: Full suite, pixels, commit**

Run the Task 2 Step 6 commands. Expected: the same. (The tour captures frame 60+ of each page, so the one-frame delay does not show.)

```bash
git add -A
git commit -m "fix: custom shaders are checked on later frames instead of blocking"
```

---

## Phase 2: the web build

### Task 4: `ryo2d` compiles for wasm32

**Files:**
- Modify: `Cargo.toml` (target-specific dependencies), `src/lib.rs`, `src/core/mod.rs`, every `std::time::{Instant, SystemTime}` user (`grep -rln 'std::time::Instant\|Instant::now\|SystemTime' src`: `time.rs`, `dev_log.rs`, `config/debug.rs`, `config/save.rs`, `scene/manager.rs`, `engine/{mod,frame,window,extract}.rs`, `ui/theme.rs`, `windows/*.rs`), `src/core/asset/{manager.rs,hot_reload.rs}`, `src/core/capture/{writer.rs,state.rs}`, `src/core/config/{save.rs,save_location.rs}`, `src/core/ui/retained/text_input.rs`, `src/core/engine/{mod.rs,window.rs,frame.rs}`, `dev.sh`, `dev.ps1`
- Test: `cargo check`/`clippy` for `wasm32-unknown-unknown` (no runtime tests for wasm in this task)

**Interfaces:**
- Produces: `#[cfg(target_arch = "wasm32")]` builds of `ryo2d` without `cli`, `network`, dev windows, profiler, hot reload, capture-to-disk, asset loader threads or disk saves. `SaveManager` operations return `SaveError::Unsupported` on wasm (add the variant: `#[error("saving to disk is not available on the web")] Unsupported`). `AssetManager::load_async` on wasm loads synchronously and returns an already-completed handle.

- [ ] **Step 1: Make the failure visible**

Run: `rustup target add wasm32-unknown-unknown && cargo check -p ryo2d --lib --target wasm32-unknown-unknown 2>&1 | grep -E '^error' | head -20`
Expected: errors (native-only crates such as `arboard`, `notify`, `pollster` usage, `getrandom` without a web backend).

- [ ] **Step 2: Split the dependencies by target** (`Cargo.toml`)

Move to `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`: `pollster`, `notify`, `dirs`, `arboard`, `renet`, `renet_netcode`, `zip`, `flate2`, `puffin` (keep `optional = true`), `matchbox_socket` (keep `optional = true`). Add:

```toml
web-time = "1.1"

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "=0.2.129"
wasm-bindgen-futures = "0.4.79"
web-sys = { version = "0.3.106", features = ["Window", "Document", "Element", "HtmlCanvasElement", "CssStyleDeclaration"] }
console_error_panic_hook = "0.1.7"
getrandom_02 = { package = "getrandom", version = "0.2", features = ["js"] }
gilrs = { version = "0.11", features = ["wasm-bindgen"] }
```

`getrandom` 0.2 arrives through `rand` 0.8; its `js` feature is its web backend. After the split, `cargo tree -p ryo2d --target wasm32-unknown-unknown -i getrandom@0.3` must print nothing (0.3 came only through `zip`).

- [ ] **Step 3: Clocks**

Replace `std::time::Instant` and `std::time::SystemTime` with `web_time::Instant` / `web_time::SystemTime` everywhere in `src/` (`web_time` is `std::time` on native). `Duration` stays `std::time::Duration`.

Run: `grep -rn 'std::time::Instant\|std::time::SystemTime\|time::{.*Instant' src | grep -v web_time`
Expected: nothing.

- [ ] **Step 4: Gate the native-only modules**

```rust
// src/lib.rs
#[cfg(not(target_arch = "wasm32"))]
pub mod cli;
```

```rust
// src/core/mod.rs
#[cfg(not(target_arch = "wasm32"))]
pub mod network;
#[cfg(not(target_arch = "wasm32"))]
pub mod windows;
```

In the prelude (`src/lib.rs`), put `#[cfg(not(target_arch = "wasm32"))]` on the network `pub use` block. Then, each with `#[cfg(not(target_arch = "wasm32"))]` on the item and a wasm counterpart where the API must exist:
- `asset/hot_reload.rs`: the whole module; `AssetManager::enable_hot_reload` is a no-op on wasm that logs once.
- `asset/manager.rs` async loading: on wasm, `load_async` runs the load inline and returns a completed `AsyncSpriteHandle`.
- `capture/writer.rs` (thread) and the env-driven `capture/config.rs` parsing: on wasm, capture requests are ignored with one warning.
- `config/save_location.rs` (`dirs`) and the file writes in `config/save.rs`: on wasm return `SaveError::Unsupported`.
- `ui/retained/text_input.rs` clipboard (`arboard`): on wasm, copy and paste do nothing.
- `engine/window.rs` dev windows, profiler window, `std::process::exit`; `engine/mod.rs` `run_offscreen` and the `RYO_HEADLESS` branch; `engine/frame.rs` `pace_frame` sleeping (on wasm the browser paces; `pace_frame` returns at once).
- `console/builtins.rs` network commands.

- [ ] **Step 5: Check both targets, and add the wasm check to the dev scripts**

Run: `cargo check -p ryo2d --lib --target wasm32-unknown-unknown 2>&1 | tail -2 && cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings 2>&1 | tail -2`
Expected: `Finished` for both, no warnings.

`dev.sh`:

```bash
    check)
        cargo check --workspace
        cargo check -p ryo2d --lib --target wasm32-unknown-unknown
        ;;
    lint)
        cargo clippy --workspace --all-targets
        cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown
        ;;
```

`dev.ps1`: the same two commands in its `check` and `lint` branches.

- [ ] **Step 6: Native suite, pixels, commit**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{f+=$6} END {print f}' && ./dev.sh lint 2>&1 | grep -E '^(warning|error)' | grep -v 'future version' | head -3; ./dev.sh demos:tour >/dev/null && target/renderer-baseline/compare.sh; echo $?`
Expected: `0`, no lint lines, compare `0`.

```bash
git add -A
git commit -m "feat: ryo2d builds for wasm32

- web-time replaces std::time clocks
- native-only parts are compiled out on wasm: CLI, networking, dev and
  profiler windows, hot reload, loader threads, capture to disk, disk
  saves, clipboard
- ./dev check and lint also check the wasm target"
```

### Task 5: The engine runs in the browser

**Files:**
- Modify: `src/core/engine/{mod.rs,window.rs,frame.rs}`, `src/core/graphics/renderer.rs`
- Create: `examples/demos/lighting/index.html`, `examples/demos/lighting/Trunk.toml`
- Modify: `dev.sh`, `dev.ps1` (the `web` command), `.gitignore` (`examples/demos/*/dist/`)

**Interfaces:**
- Consumes: `WgpuRenderer::new(window, RendererSettings) -> Result<_, GpuError>` (Task 2).
- Produces: `pub(crate) enum EngineEvent { GpuReady(Box<Result<WgpuRenderer, GpuError>>) }`; `impl ApplicationHandler<EngineEvent> for Engine`; `EngineBuilder::run()` on wasm returns `Ok(())` after `spawn_app`.

- [ ] **Step 1: Make the event loop carry a user event**

`Engine::run` builds `EventLoop::<EngineEvent>::with_user_event().build()` and stores `event_loop.create_proxy()` in `self.proxy`. Native keeps `run_app(&mut self)`; wasm calls:

```rust
#[cfg(target_arch = "wasm32")]
{
  use winit::platform::web::EventLoopExtWebSys;
  console_error_panic_hook::set_once();
  event_loop.spawn_app(self);
  return Ok(());
}
```

- [ ] **Step 2: Create the window and the renderer on the web**

In `resumed`, on wasm the window attributes add the canvas:

```rust
#[cfg(target_arch = "wasm32")]
let window_attrs = {
  use winit::platform::web::WindowAttributesExtWebSys;
  window_attrs.with_append(true).with_prevent_default(true)
};
```

Native keeps `pollster::block_on(WgpuRenderer::new(..))`. Wasm spawns the async setup and returns:

```rust
#[cfg(target_arch = "wasm32")]
{
  let proxy = self.proxy.clone();
  let settings = self.renderer_settings();
  let window = Arc::clone(&window);
  wasm_bindgen_futures::spawn_local(async move {
    let result = WgpuRenderer::new(window, settings).await;
    let _ = proxy.send_event(EngineEvent::GpuReady(Box::new(result)));
  });
  self.window = Some(window);
  return;
}
```

`user_event` receives `GpuReady`: `Ok(renderer)` goes through the same path native uses after `block_on` (blank frame, `set_visible(true)`, `init_gpu`); `Err(error)` logs with `engine_error!` and exits the loop. Until the GPU arrives, `RedrawRequested` does nothing.

- [ ] **Step 3: Let the browser pace frames**

In `about_to_wait`, wasm only requests a redraw (`window.request_redraw()`); the sleep and spin pacing in `engine/frame.rs` is native-only. Unfocused background-rate throttling is native-only too.

- [ ] **Step 4: Serve a demo**

`examples/demos/lighting/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Ryo2D: Lighting</title>
    <link data-trunk rel="rust" href="Cargo.toml" data-bin="lighting" />
    <style>
      html, body { margin: 0; height: 100%; background: #19192a; }
      canvas { display: block; width: 100vw; height: 100vh; outline: none; }
    </style>
  </head>
  <body></body>
</html>
```

`examples/demos/lighting/Trunk.toml`:

```toml
[build]
target = "index.html"
dist = "dist"
```

`dev.sh` (and the same in `dev.ps1`):

```bash
    web)
        demo="${2:?usage: ./dev.sh web <demo>}"
        command -v trunk >/dev/null || { echo "trunk is required: cargo install trunk --locked"; exit 1; }
        rustup target add wasm32-unknown-unknown >/dev/null
        (cd "examples/demos/$demo" && trunk serve --open)
        ;;
```

Add `web <demo>` to both scripts' help text.

- [ ] **Step 5: Run it in the browsers**

Run: `./dev.sh web lighting` (serves on `http://127.0.0.1:8080`).
Expected, in desktop Chrome, Safari 26 and Firefox: the lighting demo's page draws (sprites, the light, the demo-kit header and settings panel), the arrow keys change the setting, the frame keeps animating, and the console shows no panic. Record the three browser versions in the task's ledger line.

Then a scripted check with the Playwright tools (Chromium with WebGPU, `--enable-unsafe-webgpu` where needed): navigate to `http://127.0.0.1:8080`, wait two seconds, take a screenshot, and confirm the canvas is not blank (the header text and the lit scene are visible). If Playwright's Chromium has no WebGPU adapter, record that and rely on the manual checks.

- [ ] **Step 6: Native suite, pixels, commit**

Run the Task 4 Step 6 commands. Expected: the same.

```bash
git add -A
git commit -m "feat: the engine runs in the browser through WebGPU

- winit spawn_app with async GPU setup delivered as a user event
- the browser paces frames
- ./dev.sh web <demo> serves a demo with trunk; the lighting demo has a page"
```

---

## Phase 3: one texture registry

### Task 6: `TextureRegistry` and `TextureId` inside the renderer

**Files:**
- Create: `src/core/graphics/texture_registry.rs`
- Modify: `src/core/graphics/{mod.rs,atlas.rs,sprite_gpu.rs}`, `src/core/render/{draw_list.rs,entities.rs,mod.rs}`
- Test: `src/core/graphics/texture_registry.rs` (unit); existing `tests/e2e/sprite_atlas.rs`, `src/core/testing/atlas_bleeding.rs`

**Interfaces:**
- Produces (all `pub(crate)`):
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)] struct TextureId { index: u32, generation: u32 }`
  - `enum TextureSource { Sprite(SpriteHandle), Pixels(Arc<[u8]>) }` (Task 17 adds `Target`)
  - `struct TextureEntry { pub source: TextureSource, pub width: u32, pub height: u32, pub uploaded_content: Option<u64>, pub last_used: u64 }`
  - `struct TextureRegistry` with `new()`, `sprite(&mut self, sprite: &SpriteHandle, frame: u64) -> TextureId`, `pixels(&mut self, name: &str, width: u32, height: u32, data: &[u8], frame: u64) -> TextureId`, `get(&self, id: TextureId) -> Option<&TextureEntry>`, `get_mut(&mut self, id) -> Option<&mut TextureEntry>`, `needs_upload(&self, id) -> bool` (never uploaded, or a sprite whose `generation()` differs from `uploaded_content`), `mark_uploaded(&mut self, id)`, `prune(&mut self, frame: u64, ttl: u64) -> Vec<TextureId>` (removes entries unused for `ttl` frames and returns them so the caller frees their atlas space), `len(&self) -> usize`.
  - `AtlasKey` is replaced by `TextureId` in `atlas.rs` (`place`, `slot`, `lookup`, `remove`, `holds`, `get` take `TextureId`).
  - `render::TextureRef` (`Arc<str>`) is replaced by `TextureId` in `DrawList`, `BatchCollector` and `EntityDrawer`.

- [ ] **Step 1: Write the failing tests** (`src/core/graphics/texture_registry.rs`)

```rust
//! Every GPU texture the renderer draws has one `TextureId`.

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::asset::SpriteHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct TextureId {
  index: u32,
  generation: u32,
}

pub(crate) enum TextureSource {
  Sprite(SpriteHandle),
  Pixels(Arc<[u8]>),
}

pub(crate) struct TextureEntry {
  pub source: TextureSource,
  pub width: u32,
  pub height: u32,
  /// The sprite content generation (or 0 for pixels) last uploaded
  pub uploaded_content: Option<u64>,
  pub last_used: u64,
}

#[derive(Default)]
pub(crate) struct TextureRegistry {
  slots: Vec<(u32, Option<TextureEntry>)>,
  free: Vec<u32>,
  by_sprite: HashMap<usize, TextureId>,
  by_name: HashMap<String, TextureId>,
}

impl TextureRegistry {
  pub(crate) fn new() -> Self {
    Self::default()
  }

  pub(crate) fn sprite(&mut self, sprite: &SpriteHandle, frame: u64) -> TextureId {
    todo!()
  }

  pub(crate) fn pixels(&mut self, name: &str, width: u32, height: u32, data: &[u8], frame: u64) -> TextureId {
    todo!()
  }

  pub(crate) fn get(&self, id: TextureId) -> Option<&TextureEntry> {
    todo!()
  }

  pub(crate) fn get_mut(&mut self, id: TextureId) -> Option<&mut TextureEntry> {
    todo!()
  }

  pub(crate) fn needs_upload(&self, id: TextureId) -> bool {
    todo!()
  }

  pub(crate) fn mark_uploaded(&mut self, id: TextureId) {
    todo!()
  }

  pub(crate) fn prune(&mut self, frame: u64, ttl: u64) -> Vec<TextureId> {
    todo!()
  }

  pub(crate) fn len(&self) -> usize {
    todo!()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::Sprite;

  fn sprite(name: &str) -> SpriteHandle {
    SpriteHandle::from_sprite(Sprite::solid_rect(4, 2, 255, 0, 0, 255), name)
  }

  #[test]
  fn a_handle_and_its_clones_share_one_id() {
    let mut registry = TextureRegistry::new();
    let a = sprite("a");
    let id = registry.sprite(&a, 0);
    assert_eq!(registry.sprite(&a.clone(), 1), id);
    assert_ne!(registry.sprite(&sprite("a"), 1), id, "a different handle, even with the same name");
    let entry = registry.get(id).unwrap();
    assert_eq!((entry.width, entry.height, entry.last_used), (4, 2, 1));
  }

  #[test]
  fn named_pixels_share_one_id() {
    let mut registry = TextureRegistry::new();
    let data = [255u8; 16];
    let id = registry.pixels("ellipse", 2, 2, &data, 0);
    assert_eq!(registry.pixels("ellipse", 2, 2, &data, 3), id);
    assert_eq!(registry.get(id).unwrap().last_used, 3);
  }

  #[test]
  fn a_changed_sprite_needs_uploading_again() {
    let mut registry = TextureRegistry::new();
    let handle = sprite("hot");
    let id = registry.sprite(&handle, 0);
    assert!(registry.needs_upload(id), "never uploaded");
    registry.mark_uploaded(id);
    assert!(!registry.needs_upload(id));
    handle.update(Sprite::solid_rect(8, 8, 0, 255, 0, 255));
    assert!(registry.needs_upload(id), "hot reload bumped its generation");
    registry.sprite(&handle, 1);
    assert_eq!(registry.get(id).unwrap().width, 8, "the size follows the new content");
  }

  #[test]
  fn a_pruned_id_is_never_valid_again() {
    let mut registry = TextureRegistry::new();
    let old = registry.sprite(&sprite("old"), 0);
    let kept = registry.sprite(&sprite("kept"), 0);
    registry.sprite(&sprite("kept-again"), 0);
    registry.get_mut(kept).unwrap().last_used = 10;
    let pruned = registry.prune(10, 5);
    assert!(pruned.contains(&old));
    assert!(registry.get(old).is_none());
    assert!(registry.get(kept).is_some());
    let reused = registry.sprite(&sprite("new"), 11);
    assert_ne!(reused, old, "a reused slot gets a new generation");
    assert!(registry.get(old).is_none());
    assert_eq!(registry.len(), 2);
  }
}
```

Run: `cargo test --lib -p ryo2d -- texture_registry 2>&1 | grep -E 'test result|panicked'`
Expected: four FAIL at `todo!()`.

- [ ] **Step 2: Implement the registry**

```rust
impl TextureRegistry {
  fn insert(&mut self, entry: TextureEntry) -> TextureId {
    if let Some(index) = self.free.pop() {
      let slot = &mut self.slots[index as usize];
      slot.0 = slot.0.wrapping_add(1);
      slot.1 = Some(entry);
      TextureId { index, generation: slot.0 }
    } else {
      let index = self.slots.len() as u32;
      self.slots.push((0, Some(entry)));
      TextureId { index, generation: 0 }
    }
  }

  pub(crate) fn sprite(&mut self, sprite: &SpriteHandle, frame: u64) -> TextureId {
    if let Some(&id) = self.by_sprite.get(&sprite.serial())
      && let Some(entry) = self.get_mut(id)
    {
      entry.last_used = frame;
      if entry.uploaded_content != Some(sprite.generation()) {
        let (width, height) = sprite.with_sprite(|s| (s.width(), s.height()));
        entry.width = width;
        entry.height = height;
      }
      return id;
    }
    let (width, height) = sprite.with_sprite(|s| (s.width(), s.height()));
    let id = self.insert(TextureEntry {
      source: TextureSource::Sprite(sprite.clone()),
      width,
      height,
      uploaded_content: None,
      last_used: frame,
    });
    self.by_sprite.insert(sprite.serial(), id);
    id
  }

  pub(crate) fn pixels(&mut self, name: &str, width: u32, height: u32, data: &[u8], frame: u64) -> TextureId {
    if let Some(&id) = self.by_name.get(name)
      && let Some(entry) = self.get_mut(id)
    {
      entry.last_used = frame;
      return id;
    }
    let id = self.insert(TextureEntry {
      source: TextureSource::Pixels(Arc::from(data)),
      width,
      height,
      uploaded_content: None,
      last_used: frame,
    });
    self.by_name.insert(name.to_string(), id);
    id
  }

  pub(crate) fn get(&self, id: TextureId) -> Option<&TextureEntry> {
    let (generation, entry) = self.slots.get(id.index as usize)?;
    (*generation == id.generation).then_some(entry.as_ref()).flatten()
  }

  pub(crate) fn get_mut(&mut self, id: TextureId) -> Option<&mut TextureEntry> {
    let (generation, entry) = self.slots.get_mut(id.index as usize)?;
    (*generation == id.generation).then_some(entry.as_mut()).flatten()
  }

  fn content(entry: &TextureEntry) -> u64 {
    match &entry.source {
      TextureSource::Sprite(sprite) => sprite.generation(),
      TextureSource::Pixels(_) => 0,
    }
  }

  pub(crate) fn needs_upload(&self, id: TextureId) -> bool {
    self
      .get(id)
      .is_some_and(|entry| entry.uploaded_content != Some(Self::content(entry)))
  }

  pub(crate) fn mark_uploaded(&mut self, id: TextureId) {
    if let Some(entry) = self.get_mut(id) {
      entry.uploaded_content = Some(Self::content(entry));
    }
  }

  pub(crate) fn prune(&mut self, frame: u64, ttl: u64) -> Vec<TextureId> {
    let mut pruned = Vec::new();
    for (index, (generation, entry)) in self.slots.iter_mut().enumerate() {
      if entry.as_ref().is_some_and(|e| frame.saturating_sub(e.last_used) >= ttl) {
        *entry = None;
        pruned.push(TextureId { index: index as u32, generation: *generation });
        self.free.push(index as u32);
      }
    }
    self.by_sprite.retain(|_, id| !pruned.contains(id));
    self.by_name.retain(|_, id| !pruned.contains(id));
    pruned
  }

  pub(crate) fn len(&self) -> usize {
    self.slots.iter().filter(|(_, entry)| entry.is_some()).count()
  }
}
```

Run: `cargo test --lib -p ryo2d -- texture_registry 2>&1 | grep 'test result'`
Expected: PASS.

- [ ] **Step 3: Key the atlas, the sprite renderer and the draw list by `TextureId`**

- `atlas.rs`: delete `AtlasKey`; every method takes `TextureId`. A content change (`needs_upload` true for an id the atlas already holds) is `atlas.remove(id)` then `slot(..)` again.
- `sprite_gpu.rs`: `SpriteGpuRenderer` owns a `TextureRegistry`. Delete `CachedTexture`, `EntitySprite`, `textures: HashMap<String, CachedTexture>` and `entity_sprites`. `upload_entity_sprite(device, queue, sprite) -> Option<TextureId>` becomes `registry.sprite(sprite, frame)` plus residency: if `needs_upload`, read the pixels (`sprite.with_sprite(|s| s.data().to_vec())`), `atlas.slot(..)`, `mark_uploaded`. `upload_texture(.., id: &str, ..)` becomes `registry.pixels(..)` (kept for the shared shadow ellipse, `ELLIPSE_ID` stays a name). `make_resident` iterates `TextureId`s and re-uploads from `TextureEntry::source` after eviction exactly as today. `prune_textures`/`prune_entity_sprites` become one `registry.prune(frame, TEXTURE_TTL)` whose result is removed from the atlas; `MAX_STANDALONE_ENTITY_SPRITES` stays, applied to registry entries resident as standalone textures.
- `draw_list.rs`: `TextureRef` is deleted; `DrawPayload::Sprite { texture: TextureId, .. }`. Run planning compares ids instead of `Arc::ptr_eq`.
- `BatchCollector` keeps `HashMap<String, TextureRef>` only until Task 7; for now its `add(texture_id: &str, ..)` resolves names through `registry.pixels`/the sprite map when items are pushed in `push_to(&mut DrawList, &mut TextureRegistry, frame)`.

- [ ] **Step 4: Suite, pixels, commit**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{f+=$6} END {print f}' && ./dev.sh lint 2>&1 | grep -E '^(warning|error)' | grep -v 'future version' | head -3; ./dev.sh demos:tour >/dev/null && target/renderer-baseline/compare.sh; echo $?; grep -rn 'Arc<str>\|TextureRef\|AtlasKey' src/core/render src/core/graphics | head`
Expected: `0`, no lint lines, compare `0`, no grep lines.

```bash
git add -A
git commit -m "refactor: one texture registry keys every GPU texture by TextureId"
```

### Task 7: Games pass `SpriteHandle`; `TextureManager` goes

**Files:**
- Modify: `src/core/graphics/{gpu_context.rs,sprite_gpu.rs,parallax_gpu.rs}`, `src/core/particles/mod.rs`, `src/core/trail/renderer.rs`, `src/core/scene/context.rs`, `src/core/engine/mod.rs`, `src/lib.rs`, `src/core/mod.rs`, every example and test calling `add_sprite_instance`, `add_custom_sprite`, `has_texture`, `batch.add` or `ctx.textures()` (`grep -rln 'add_sprite_instance\|add_custom_sprite\|has_texture\|batch.add\|ctx.textures()' examples tests crates src`)
- Delete: `src/core/texture/` (module, `AGENTS.md`), the `TextureManager`/`TextureContext`/`TextureHandle`/`TextureArrayHandle` exports
- Test: `tests/e2e/gpu_world.rs` (existing `render_gpu` tests), a new test below

**Interfaces:**
- Consumes: `TextureRegistry`, `TextureId` (Task 6).
- Produces:
  - `GpuRenderContext::add_sprite_instance(&mut self, sprite: &SpriteHandle, instance: SpriteInstance, z_order: i32, has_effects: bool)`; `add_custom_sprite(&mut self, sprite: &SpriteHandle, shader: &ShaderHandle, params: &ShaderParams, instance: SpriteInstance, z_order: i32)`. `upload_sprite` and `has_texture` are removed (registration happens on first use).
  - `BatchCollector::add(&mut self, sprite: &SpriteHandle, has_effects: bool, instance: SpriteInstance, z_order: i32)` and `add_shaded(&mut self, sprite: &SpriteHandle, shading, instance, z_order)` (crate). The collector stores `HashMap<usize /* serial */, SpriteHandle>` plus items keyed by serial and resolves `TextureId`s in `push_to`.
  - `GpuParallaxLayer` stores `sprite: SpriteHandle` (its `texture_id: String` field is removed).
  - `TrailRenderer` takes textures from the `TextureRegistry` (its `texture_cache` keyed by `sprite.id()` is removed; a textured trail binds the registry texture's standalone view, uploading it as a standalone texture when not resident).

- [ ] **Step 1: Write the failing test** (append to `tests/e2e/gpu_world.rs`)

```rust
/// Two handles loaded under one name are two textures: the second does not
/// draw the first one's pixels.
#[test]
fn two_handles_with_one_name_draw_their_own_pixels() {
  let red = solid("same-name", 20, RED);
  let blue = solid("same-name", 20, BLUE);
  let scene = Fixture::new(|_| {})
    .with_gpu_sprite(red, 15.0, 15.0, 20.0, 0)
    .with_gpu_sprite(blue, 45.0, 15.0, 20.0, 0);
  let Some(frame) = frame_of(scene, (64, 32)) else {
    return;
  };
  assert_eq!(frame.get_pixel(15, 15).0, RED);
  assert_eq!(frame.get_pixel(45, 15).0, BLUE);
}
```

Update the file's `Fixture::render` to the new API: `gpu.add_sprite_instance(sprite, instance, *z, false);` (no `upload_sprite`, no `&sprite.id()`).

Run: `cargo test -p ryo2d --test e2e -- two_handles_with_one_name 2>&1 | grep -E 'error\[|test result'`
Expected: compile errors (the API still takes `&str`). Before changing the API, temporarily confirm the behaviour bug exists by running the test against the old API with `&sprite.id()` restored: the right-hand pixel is RED (both keyed by "same-name"). Then continue.

- [ ] **Step 2: Change the APIs and their callers**

Apply the Produces block. `particles/mod.rs` `collect_for_render` calls `collector.add(&emitter.sprite, ..)`. `parallax_gpu.rs` stores and passes the handle. Example and test call sites change `gpu.upload_sprite(&s); gpu.add_sprite_instance(&s.id(), i, z, fx)` to `gpu.add_sprite_instance(&s, i, z, fx)`.

- [ ] **Step 3: Delete `TextureManager` and its handles**

Remove `src/core/texture/`, `pub mod texture` in `src/core/mod.rs`, the prelude exports, the `TextureManager` resource insertion in `init_world_resources` (`engine/mod.rs`) and `SceneContext::textures()`. Run `grep -rn 'TextureManager\|TextureHandle\|TextureArrayHandle\|TextureContext\|textures()' src examples tests crates .claude/skills docs/architecture`; fix every hit (skills and AGENTS.md mentions are rewritten in Task 18; delete the sentences now so nothing points at removed types).

- [ ] **Step 4: Suite, pixels, commit**

Run the Task 6 Step 4 commands (without the last grep).
Expected: `0`, no lint lines, compare `0`.

```bash
git add -A
git commit -m "refactor: games pass SpriteHandle to the GPU batch; TextureManager removed

- add_sprite_instance and add_custom_sprite take the handle; textures
  register on first use, so two handles with one name no longer collide
- particles, parallax and trails key textures through the registry
- TextureManager, its handles and ctx.textures() are deleted"
```

---

## Phase 4: the resolution model

### Task 8: `Presentation`, `ScreenMapping` and the configuration; demos and games opt into pixel art

**Files:**
- Create: `src/core/graphics/screen.rs`
- Modify: `src/core/graphics/mod.rs`, `src/core/window.rs` (delete `WindowMode`), `src/core/engine/mod.rs` (`EngineConfig`, `EngineBuilder`), `src/core/config/{resolve.rs,workspace.rs}`, `src/build.rs` (embedded keys), `src/lib.rs` (prelude), `examples/demo-kit/src/*.rs` (`Demo::resolution`), every demo and game `main.rs`/builder (`grep -rln '\.resolution(' examples`)
- Test: `src/core/graphics/screen.rs` (unit), `src/core/config/resolve.rs` (unit)

**Interfaces:**
- Produces:
  - `pub enum Presentation { ResolutionIndependent, PixelArt { width: u32, height: u32 } }` (`Debug, Clone, Copy, PartialEq, Default` = `ResolutionIndependent`), in the prelude.
  - `pub struct ScreenMapping` (`Debug, Clone, Copy, PartialEq`) with `new(presentation: Presentation, physical: (u32, u32), scale_factor: f64) -> Self`, `screen_size(&self) -> Vec2`, `scale(&self) -> f32` (physical pixels per screen unit), `window_rect(&self) -> (Vec2, Vec2)` (top-left and size, physical pixels), `window_to_screen(&self, physical: Vec2) -> Vec2`, `screen_to_window(&self, screen: Vec2) -> Vec2`, `render_size(&self) -> (u32, u32)` (the size window-target images render at), `ndc_rect(&self) -> [f32; 4]` (scale_x, scale_y, offset_x, offset_y of the screen rectangle in the window's normalised device coordinates), `presentation(&self) -> Presentation`. In the prelude.
  - `pub(crate) fn initial_window_size(presentation: Presentation, resolution: (u32, u32), window_size: Option<(u32, u32)>) -> (u32, u32)` (logical).
  - `EngineConfig { presentation: Presentation, window_size: Option<(u32, u32)>, resizable: bool, .. }` replacing `window_mode`, `max_width`, `max_height`.
  - `EngineBuilder::pixel_art(self, width: u32, height: u32) -> Self` (sets `presentation` and the resolution), `window_size(self, width: u32, height: u32) -> Self`, `resizable(self, bool) -> Self`. `window_mode`, `max_width`, `max_height` are removed.
  - `Ryo.toml` `[system] pixel_art = true` makes `resolution` the pixel-art target (layers: project and embedded, like `vsync`; not a player preference, so not in `system.json`). Embedded key `RYO_EMBED_PIXEL_ART`.

- [ ] **Step 1: Write the failing tests** (`src/core/graphics/screen.rs`)

```rust
//! Screen space and the window: one mapping between them.

use glam::Vec2;

/// How the world reaches the window.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Presentation {
  /// Render at the window's physical resolution; screen space is logical pixels
  #[default]
  ResolutionIndependent,
  /// Render into a fixed target scaled by a whole number, centred with bars
  PixelArt { width: u32, height: u32 },
}

/// Converts between window pixels and screen space (UI, mouse, `screen_size`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenMapping {
  presentation: Presentation,
  physical: (u32, u32),
  scale_factor: f32,
}

impl ScreenMapping {
  pub fn new(presentation: Presentation, physical: (u32, u32), scale_factor: f64) -> Self {
    todo!()
  }
  pub fn presentation(&self) -> Presentation {
    self.presentation
  }
  pub fn screen_size(&self) -> Vec2 {
    todo!()
  }
  pub fn scale(&self) -> f32 {
    todo!()
  }
  pub fn window_rect(&self) -> (Vec2, Vec2) {
    todo!()
  }
  pub fn window_to_screen(&self, physical: Vec2) -> Vec2 {
    todo!()
  }
  pub fn screen_to_window(&self, screen: Vec2) -> Vec2 {
    todo!()
  }
  pub fn render_size(&self) -> (u32, u32) {
    todo!()
  }
  pub fn ndc_rect(&self) -> [f32; 4] {
    todo!()
  }
}

/// The window's initial logical size.
pub(crate) fn initial_window_size(
  presentation: Presentation,
  resolution: (u32, u32),
  window_size: Option<(u32, u32)>,
) -> (u32, u32) {
  todo!()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 1e-4
  }

  #[test]
  fn resolution_independent_screen_space_is_logical_pixels() {
    let m = ScreenMapping::new(Presentation::ResolutionIndependent, (2560, 1440), 2.0);
    assert_eq!(m.screen_size(), Vec2::new(1280.0, 720.0));
    assert_eq!(m.scale(), 2.0);
    assert_eq!(m.render_size(), (2560, 1440));
    assert_eq!(m.window_rect(), (Vec2::ZERO, Vec2::new(2560.0, 1440.0)));
    assert!(close(m.window_to_screen(Vec2::new(100.0, 50.0)), Vec2::new(50.0, 25.0)));
    assert_eq!(m.ndc_rect(), [1.0, 1.0, 0.0, 0.0]);
  }

  #[test]
  fn pixel_art_scales_by_a_whole_number_with_bars() {
    let art = Presentation::PixelArt { width: 320, height: 180 };
    let m = ScreenMapping::new(art, (1300, 740), 1.0);
    assert_eq!(m.screen_size(), Vec2::new(320.0, 180.0));
    assert_eq!(m.scale(), 4.0);
    assert_eq!(m.render_size(), (320, 180));
    assert_eq!(m.window_rect(), (Vec2::new(10.0, 10.0), Vec2::new(1280.0, 720.0)));
    assert!(close(m.window_to_screen(Vec2::new(10.0, 10.0)), Vec2::ZERO));
    assert!(close(m.window_to_screen(Vec2::new(1290.0, 730.0)), Vec2::new(320.0, 180.0)));
    assert!(
      close(m.window_to_screen(Vec2::new(6.0, 6.0)), Vec2::new(-1.0, -1.0)),
      "the bars still map, outside 0..size"
    );
    let [sx, sy, ox, oy] = m.ndc_rect();
    assert!((sx - 1280.0 / 1300.0).abs() < 1e-6 && (sy - 720.0 / 740.0).abs() < 1e-6);
    assert_eq!((ox, oy), (0.0, 0.0));
  }

  #[test]
  fn a_window_smaller_than_the_art_scales_at_one_and_crops() {
    let art = Presentation::PixelArt { width: 320, height: 180 };
    let m = ScreenMapping::new(art, (300, 170), 1.0);
    assert_eq!(m.scale(), 1.0);
    assert_eq!(m.window_rect(), (Vec2::new(-10.0, -5.0), Vec2::new(320.0, 180.0)));
    assert!(close(m.window_to_screen(Vec2::ZERO), Vec2::new(10.0, 5.0)));
  }

  #[test]
  fn mappings_round_trip() {
    for m in [
      ScreenMapping::new(Presentation::ResolutionIndependent, (1920, 1080), 1.5),
      ScreenMapping::new(Presentation::PixelArt { width: 256, height: 224 }, (1366, 768), 1.0),
      ScreenMapping::new(Presentation::PixelArt { width: 320, height: 180 }, (2560, 1440), 2.0),
    ] {
      let p = Vec2::new(123.0, 77.0);
      assert!(close(m.screen_to_window(m.window_to_screen(p)), p), "{m:?}");
    }
  }

  #[test]
  fn degenerate_inputs_stay_finite() {
    let m = ScreenMapping::new(Presentation::ResolutionIndependent, (0, 0), f64::NAN);
    assert_eq!(m.render_size(), (1, 1));
    assert!(m.screen_size().is_finite() && m.scale() > 0.0);
    let art = ScreenMapping::new(Presentation::PixelArt { width: 0, height: 0 }, (100, 100), 1.0);
    assert_eq!(art.render_size(), (1, 1));
  }

  #[test]
  fn the_initial_window_fits_the_art_within_720p() {
    let art = |w, h| Presentation::PixelArt { width: w, height: h };
    assert_eq!(initial_window_size(art(320, 180), (320, 180), None), (1280, 720));
    assert_eq!(initial_window_size(art(400, 300), (400, 300), None), (800, 600));
    assert_eq!(initial_window_size(art(2000, 1000), (2000, 1000), None), (2000, 1000));
    assert_eq!(initial_window_size(art(320, 180), (320, 180), Some((640, 360))), (640, 360));
    assert_eq!(
      initial_window_size(Presentation::ResolutionIndependent, (1024, 768), None),
      (1024, 768)
    );
  }
}
```

Run: `cargo test --lib -p ryo2d -- graphics::screen 2>&1 | grep -E 'test result|panicked'`
Expected: six FAIL at `todo!()`.

- [ ] **Step 2: Implement**

```rust
impl ScreenMapping {
  pub fn new(presentation: Presentation, physical: (u32, u32), scale_factor: f64) -> Self {
    let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 { scale_factor as f32 } else { 1.0 };
    let presentation = match presentation {
      Presentation::PixelArt { width, height } => Presentation::PixelArt { width: width.max(1), height: height.max(1) },
      other => other,
    };
    Self { presentation, physical: (physical.0.max(1), physical.1.max(1)), scale_factor }
  }

  fn physical(&self) -> Vec2 {
    Vec2::new(self.physical.0 as f32, self.physical.1 as f32)
  }

  pub fn screen_size(&self) -> Vec2 {
    match self.presentation {
      Presentation::ResolutionIndependent => self.physical() / self.scale_factor,
      Presentation::PixelArt { width, height } => Vec2::new(width as f32, height as f32),
    }
  }

  pub fn scale(&self) -> f32 {
    match self.presentation {
      Presentation::ResolutionIndependent => self.scale_factor,
      Presentation::PixelArt { .. } => {
        let fit = self.physical() / self.screen_size();
        fit.x.min(fit.y).floor().max(1.0)
      }
    }
  }

  pub fn window_rect(&self) -> (Vec2, Vec2) {
    let size = self.screen_size() * self.scale();
    ((self.physical() - size) / 2.0, size)
  }

  pub fn window_to_screen(&self, physical: Vec2) -> Vec2 {
    (physical - self.window_rect().0) / self.scale()
  }

  pub fn screen_to_window(&self, screen: Vec2) -> Vec2 {
    screen * self.scale() + self.window_rect().0
  }

  pub fn render_size(&self) -> (u32, u32) {
    match self.presentation {
      Presentation::ResolutionIndependent => self.physical,
      Presentation::PixelArt { width, height } => (width, height),
    }
  }

  pub fn ndc_rect(&self) -> [f32; 4] {
    let (origin, size) = self.window_rect();
    let physical = self.physical();
    let scale = size / physical;
    // Centre of the rectangle relative to the window centre, in NDC (y up)
    let centre = (origin + size / 2.0) / physical * 2.0 - Vec2::ONE;
    [scale.x, scale.y, centre.x, -centre.y]
  }
}

pub(crate) fn initial_window_size(
  presentation: Presentation,
  resolution: (u32, u32),
  window_size: Option<(u32, u32)>,
) -> (u32, u32) {
  if let Some(size) = window_size {
    return size;
  }
  match presentation {
    Presentation::ResolutionIndependent => resolution,
    Presentation::PixelArt { width, height } => {
      let (width, height) = (width.max(1), height.max(1));
      let scale = (1280 / width).min(720 / height).max(1);
      (width * scale, height * scale)
    }
  }
}
```

(A centred rectangle has offset `(0, 0)` because `origin + size / 2 == physical / 2`; offsets are non-zero only for camera viewports in Task 15, which reuse `ndc_rect`'s formula.)

Run the Step 1 command. Expected: PASS.

- [ ] **Step 3: The configuration**

`EngineConfig` loses `window_mode`, `max_width`, `max_height` and gains `presentation`, `window_size`, `resizable` (default `true`). `EngineBuilder::pixel_art(w, h)` sets `presentation = PixelArt { w, h }` and the resolution to `(w, h)`. `resolve.rs`: add `pixel_art: Option<bool>` to `ProjectLayer`, the embedded layer and `BuilderOverrides` wherever `vsync` appears, and `pixel_art: bool` (default `false`) to `ResolvedSettings`; it is not in `StoredPreferences` (the player layer). When resolved `pixel_art` is true, `presentation = PixelArt { resolution }`.

The player's stored `resolution` (`system.json`, `StoredPreferences::resolution`) now picks the **window size**, not the game resolution: `ResolvedSettings` gains `window_size: Option<(u32, u32)>` set from it, and `resolution` comes from the project, embedded and builder layers only (it is the pixel-art target or the main camera's design area). `WindowRequest` resolution changes likewise resize the window. Update the existing `resolve.rs` tests that expected a stored resolution to replace `s.resolution` so they expect `s.window_size` instead. `workspace.rs` parses `[system] pixel_art`; `src/build.rs` embeds it as `RYO_EMBED_PIXEL_ART`. `src/core/window.rs` deletes `WindowMode`; the renderer's `set_window_mode`/`set_max_*` go (the renderer gets a `Presentation` in Task 9; for now `WgpuRenderer::new` treats every presentation as today's `ResizableAspectRatio`). `engine/window.rs` `resumed` creates the window at `initial_window_size(..)` logical, `with_resizable(config.resizable)`.

Add to `resolve.rs` tests:

```rust
  #[test]
  fn the_project_picks_the_art_and_the_player_picks_the_window() {
    let project = ProjectLayer {
      resolution: Some((320, 180)),
      pixel_art: Some(true),
      ..Default::default()
    };
    let prefs = StoredPreferences {
      resolution: Some((1920, 1080)),
      ..Default::default()
    };
    let s = resolve_settings(&project, &BuilderOverrides::default(), Some(&prefs));
    assert!(s.pixel_art);
    assert_eq!(s.resolution, (320, 180), "the art's size");
    assert_eq!(s.window_size, Some((1920, 1080)), "the player's window");
  }
```

(`StoredPreferences` derives `Default` if it does not already.)

Run: `cargo test --lib -p ryo2d -- config 2>&1 | grep -E 'test result|panicked'`
Expected: PASS.

- [ ] **Step 4: Demos and games opt in**

Every bundled demo and game keeps its look by declaring pixel art:
- `examples/demo-kit`: `Demo::resolution(w, h)` calls `builder.pixel_art(w, h)`.
- Games: replace `.resolution(w, h)` with `.pixel_art(w, h)`; replace `.window_mode(..)`/`.max_width(..)` with nothing (the pixel-art mode letterboxes by integer scale).
- `examples/demos/y-sorting`: `.pixel_art(GAME_WIDTH, GAME_HEIGHT).window_size(GAME_WIDTH * 2, GAME_HEIGHT * 2)` and remove `ctx.camera.set_zoom(SCREEN_ZOOM)` and the `SCREEN_ZOOM` constant (the world is now the target; the window doubles it).
- `Ryo.toml` of every example: no change needed (builders win), except examples whose `Ryo.toml` sets only `resolution` and whose builder sets none: add `pixel_art = true` under `[system]`.

- [ ] **Step 5: Suite, pixels, commit**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{f+=$6} END {print f}' && ./dev.sh lint 2>&1 | grep -E '^(warning|error)' | grep -v 'future version' | head -3; ./dev.sh demos:tour >/dev/null && target/renderer-baseline/compare.sh`
Expected: `0`, no lint lines; compare lists only `y-sorting` pages (captured at 320x240 now, not 640x480). Open `target/demo-tour/y-sorting/page-1.png` and confirm the same scene at half the size.

```bash
git add -A
git commit -m "feat: presentation modes and one screen mapping

- Presentation::{ResolutionIndependent, PixelArt} replaces WindowMode and
  the max_width/max_height caps; EngineBuilder::pixel_art, window_size
  and resizable; pixel_art = true in Ryo.toml
- ScreenMapping converts window pixels to screen space and back
- the bundled demos and games opt into pixel art"
```

### Task 9: The renderer and the engine follow `ScreenMapping`

**Files:**
- Modify: `src/core/graphics/{renderer.rs,compositor.rs,sprite_gpu.rs}`, `src/core/lighting/renderer.rs`, `src/core/trail/renderer.rs`, `src/core/shaders/pipeline.rs`, `src/core/render/{frame.rs,context.rs,passes/output.rs}`, `src/core/engine/{mod.rs,window.rs,frame.rs,extract.rs}`, `src/core/scene/context.rs`, `src/core/testing/headless.rs`, `src/core/capture/*`
- Test: `tests/e2e/headless_engine.rs`, `tests/e2e/gpu_world.rs`, `src/core/engine/tests.rs`

**Interfaces:**
- Consumes: `Presentation`, `ScreenMapping`, `initial_window_size` (Task 8).
- Produces:
  - `WgpuRenderer` holds a `ScreenMapping`; `set_mapping(&mut self, mapping: ScreenMapping)` recreates the scene-sized targets when `render_size()` changes (scene/game texture, lit texture, post-process A/B and bloom scratch, transition copy) and updates the composite rectangle from `ndc_rect()`. Zero sizes cannot occur (the mapping clamps), and `Engine` skips frames while the window's physical size has a zero side.
  - The scene image renders at `render_size()`; the world's projection still spans `screen_size()` (sprites are placed in screen units until Task 12), so a resolution-independent window at scale factor 2 renders twice the pixels.
  - Sprite sampling: nearest in `PixelArt`, linear in `ResolutionIndependent` (one `wgpu::Sampler` chosen when the renderer is created).
  - The lighting renderer's light buffer and scene copy follow `render_size()`; its and the trail renderer's projections use `screen_size()` (split the `width`/`height` they use today into `render_size` for textures and `screen_size` for the matrix).
  - The UI buffer and UI texture are `screen_size()` (rounded up); the UI is composited over the window rectangle with nearest sampling. `EngineConfig::ui_scale` defaults to `Some(1)`; `None` (auto) still means the rounded scale factor.
  - `Input` receives `WindowEvent::CursorMoved` through `mapping.window_to_screen`, including over bars. `ctx.screen_size()` returns `mapping.screen_size()`.
  - `FrameInfo { screen_width, screen_height, render_width, render_height, .. }` replaces `game_width/game_height`; `RenderContext::game_size()` becomes `render_size()` plus `screen_size()`.
  - `HeadlessEngine::with_scale_factor(builder, scale_factor: f64) -> Result<HeadlessEngine, HeadlessError>` (and `new_or_skip_with_scale_factor`), `HeadlessEngine::window_frame(&self) -> Result<RgbaImage, HeadlessError>` (the presented window, bars included), `HeadlessEngine::mouse_move_window(&mut self, x: f32, y: f32)` (physical window pixels, mapped like a real cursor). `HeadlessEngine::frame()` returns the scene image before the letterbox (`render_size()`), like capture. `HeadlessEngine::resize(w, h)` resizes the physical window.
  - Capture reads the scene image before the letterbox at `render_size()`.
  - The old `Camera` resource's viewport follows `screen_size()` on every mapping change (position, zoom and shake are kept; today's resize replaced the whole camera).

- [ ] **Step 1: Write the failing e2e tests** (append to `tests/e2e/headless_engine.rs`)

```rust
fn red_square_at(x: f32, y: f32, size: u32) -> impl FnMut(&mut SceneContext) + Send + 'static {
  move |ctx| {
    let red = SpriteHandle::from_sprite(Sprite::solid_rect(size, size, 220, 30, 40, 255), "red");
    ctx.world.spawn((Transform::from_xy(x, y), Renderable::sprite(red)));
  }
}

struct Setup(Box<dyn FnMut(&mut SceneContext) + Send>);
impl Scene for Setup {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    (self.0)(ctx);
  }
}

#[test]
fn scale_factor_two_renders_twice_the_pixels() {
  let builder = EngineBuilder::new()
    .resolution(32, 32)
    .start_scene(Setup(Box::new(red_square_at(16.0, 16.0, 10))));
  let Some(mut engine) = HeadlessEngine::new_or_skip_with_scale_factor(builder, 2.0) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  assert_eq!(frame.dimensions(), (64, 64));
  // 10 x 10 screen units around (16, 16) is 20 x 20 pixels around (32, 32)
  assert_eq!(frame.get_pixel(32, 32).0, [220, 30, 40, 255]);
  assert_eq!(frame.get_pixel(23, 32).0, [220, 30, 40, 255]);
  assert_ne!(frame.get_pixel(21, 32).0, [220, 30, 40, 255]);
  assert_ne!(frame.get_pixel(43, 32).0, [220, 30, 40, 255]);
}

#[test]
fn pixel_art_letterboxes_by_a_whole_number() {
  let builder = EngineBuilder::new()
    .pixel_art(32, 32)
    .start_scene(Setup(Box::new(red_square_at(16.0, 16.0, 32))));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.resize(100, 80); // scale 2: a 64 x 64 image at (18, 8)
  engine.step_frames(2).unwrap();
  assert_eq!(engine.frame().unwrap().dimensions(), (32, 32));
  let window = engine.window_frame().unwrap();
  assert_eq!(window.dimensions(), (100, 80));
  assert_eq!(window.get_pixel(5, 40).0, [0, 0, 0, 255], "left bar");
  assert_eq!(window.get_pixel(50, 40).0, [220, 30, 40, 255], "the art");
  assert_eq!(window.get_pixel(50, 4).0, [0, 0, 0, 255], "top bar");
}

#[test]
fn the_pointer_over_a_bar_still_maps_to_screen_space() {
  let builder = EngineBuilder::new()
    .pixel_art(32, 32)
    .start_scene(Setup(Box::new(|_| {})));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.resize(100, 80);
  engine.mouse_move_window(10.0, 40.0);
  engine.step_frames(1).unwrap();
  let (x, y) = engine.world().resource::<Input>().mouse_position();
  assert_eq!((x, y), (-4.0, 16.0));
}

#[test]
fn a_minimised_window_skips_frames_and_comes_back() {
  let builder = EngineBuilder::new()
    .resolution(32, 32)
    .start_scene(Setup(Box::new(red_square_at(16.0, 16.0, 10))));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(1).unwrap();
  engine.resize(0, 0);
  engine.step_frames(3).unwrap();
  engine.resize(32, 32);
  engine.step_frames(2).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(16, 16).0, [220, 30, 40, 255]);
}
```

Run: `cargo test -p ryo2d --test e2e -- headless_engine 2>&1 | grep -E 'error\[|test result|panicked' | head`
Expected: compile errors for the new `HeadlessEngine` methods.

- [ ] **Step 2: Implement the Produces block**

Work through it in this order, keeping the suite compiling: the `HeadlessEngine` methods (a capture-style scene readback each frame; `window_frame` reads the offscreen target), then `ScreenMapping` in `WgpuRenderer` and `Compositor` (the composite draws the scene image into `ndc_rect()` and clears outside it to black; the identity capture path stays), then the engine (`resize_window`, `ScaleFactorChanged`, `CursorMoved`, the UI buffer, `ctx.screen_size()`), then the lighting and trail size split, then the sampler choice. Delete `Engine::window_to_game_coords`, `WgpuRenderer::calculate_viewport` and the `#[cfg(test)] calculate_viewport_scale` copy with its tests (`ScreenMapping`'s tests replace them).

Run: `cargo test -p ryo2d --test e2e -- headless_engine 2>&1 | grep -E 'test result|panicked'`
Expected: PASS.

- [ ] **Step 3: Existing tests under linear filtering**

Run: `cargo test --workspace 2>&1 | grep -E 'FAILED|panicked' | head -30`
e2e tests build resolution-independent engines, which now sample linearly. A test that fails only because a scaled or rotated sprite's edge is now filtered switches its builder to `.pixel_art(w, h)` with the same size (the exact-texel mode); any other failure is a bug to fix. Record each switched test in the ledger.

- [ ] **Step 4: Suite, pixels, commit**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{f+=$6} END {print f}' && ./dev.sh lint 2>&1 | grep -E '^(warning|error)' | grep -v 'future version' | head -3; ./dev.sh demos:tour >/dev/null && target/renderer-baseline/compare.sh`
Expected: `0`, no lint lines; compare lists only the `y-sorting` pages (Task 8). The demos run pixel art, whose path matches today's rendering.

```bash
git add -A
git commit -m "feat: render at the window's resolution unless the game asks for pixel art

- resolution-independent games render at physical resolution with
  logical screen space and linear sampling
- pixel-art games render a fixed target, integer-scaled with bars
- UI, mouse and capture use screen space; the pointer maps over bars
- HeadlessEngine gains a scale factor, window readback and window-pixel
  pointer moves"
```

---

## Phase 5: cameras as entities

### Task 10: Camera components and the view maths

**Files:**
- Create: `src/core/graphics/camera2d.rs` (components, `CameraView`), `src/core/graphics/render_layers.rs`
- Modify: `src/core/graphics/{mod.rs,camera.rs}` (`CameraShake` becomes a component here; the old `Camera` stays until Task 11), `src/core/ecs/components.rs` (`RenderLayer` renamed `ZLayer`), `src/lib.rs`, every `RenderLayer` user (`grep -rln 'RenderLayer\b' src examples tests crates .claude/skills`)
- Test: `src/core/graphics/camera2d.rs`, `src/core/graphics/render_layers.rs` (unit)

**Interfaces:**
- Consumes: `TextureId` (Task 6).
- Produces (all in the prelude):
  - `#[derive(Component, Debug, Clone, Copy, PartialEq)] #[require(Transform, CameraView, RenderLayers)] pub struct Camera2d { pub zoom: f32, pub projection: Projection, pub viewport: Viewport, pub order: i32, pub target: CameraTarget, pub clear: CameraClear }`; `Default` is zoom 1, `ScreenUnits`, `Viewport::FULL`, order 0, `Window`, `Background`.
  - `pub enum Projection { ScreenUnits, Fit { width: f32, height: f32 } }`
  - `pub struct Viewport { pub min: Vec2, pub max: Vec2 }` (fractions of the target) with `pub const FULL: Viewport` and `pub fn new(x: f32, y: f32, width: f32, height: f32) -> Viewport`
  - `pub enum CameraTarget { Window, Texture(RenderTarget) }`; `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub struct RenderTarget(pub(crate) TextureId)` (created by `RenderTargets` in Task 17)
  - `pub enum CameraClear { Background, Color(Color), None }`
  - `#[derive(Component, Default)] pub struct MainCamera;`
  - `#[derive(Component, Debug, Clone)] pub struct CameraShake` (today's trauma fields) with `add_trauma(f32)`, `configure(max_offset: f32, decay_rate: f32)`, `set_curve(Ease)`, `curve() -> Ease`, `trauma() -> f32`, `reset()`, `offset() -> Vec2`, `pub(crate) update(dt: f32)`
  - `#[derive(Component, Debug, Clone, Copy, PartialEq)] pub struct CameraView { pub view_proj: Mat4, pub center: Vec2, pub rotation: f32, pub scale: f32, pub shake: Vec2, pub rect_min: Vec2, pub rect_size: Vec2, pub bounds: CameraBounds }` with `compute(center: Vec2, rotation: f32, camera: &Camera2d, shake: Vec2, target_size: Vec2) -> CameraView`, `world_to_screen(Vec2) -> Vec2`, `screen_to_world(Vec2) -> Vec2`, `is_visible(min: Vec2, max: Vec2) -> bool`. `scale` is screen units (or target pixels) per world unit; `rect_*` is the camera's rectangle in its target's units.
  - `#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)] pub struct RenderLayers(u32)` with `layer(n: u8)`, `all()`, `none()`, `with(self, n: u8) -> Self`, `intersects(&self, other: &RenderLayers) -> bool`; `Default` is `layer(0)`.
  - `CameraBounds` (existing type) moves to `camera2d.rs`.
  - `ZLayer` (renamed from `RenderLayer`, same variants and `as_z_order`).

- [ ] **Step 1: Write the failing view tests** (`src/core/graphics/camera2d.rs`, with the types from the Produces block declared and `compute`, `world_to_screen`, `screen_to_world`, `is_visible` as `todo!()`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use std::f32::consts::FRAC_PI_2;

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 1e-3
  }

  fn camera(projection: Projection, zoom: f32) -> Camera2d {
    Camera2d { zoom, projection, ..Default::default() }
  }

  #[test]
  fn a_plain_camera_maps_its_centre_to_the_screen_centre() {
    let view = CameraView::compute(
      Vec2::new(400.0, 300.0),
      0.0,
      &Camera2d::default(),
      Vec2::ZERO,
      Vec2::new(800.0, 600.0),
    );
    assert!(close(view.world_to_screen(Vec2::new(400.0, 300.0)), Vec2::new(400.0, 300.0)));
    assert!(close(view.world_to_screen(Vec2::ZERO), Vec2::ZERO));
    assert_eq!((view.bounds.left, view.bounds.right), (0.0, 800.0));
    assert_eq!((view.bounds.top, view.bounds.bottom), (0.0, 600.0));
  }

  #[test]
  fn zoom_scales_around_the_centre() {
    let view = CameraView::compute(
      Vec2::new(400.0, 300.0),
      0.0,
      &camera(Projection::ScreenUnits, 2.0),
      Vec2::ZERO,
      Vec2::new(800.0, 600.0),
    );
    assert!(close(view.world_to_screen(Vec2::new(410.0, 300.0)), Vec2::new(420.0, 300.0)));
    assert_eq!(view.bounds.right - view.bounds.left, 400.0);
  }

  #[test]
  fn fit_keeps_the_design_area_visible() {
    let fit = camera(Projection::Fit { width: 320.0, height: 180.0 }, 1.0);
    let exact = CameraView::compute(Vec2::ZERO, 0.0, &fit, Vec2::ZERO, Vec2::new(1280.0, 720.0));
    assert_eq!(exact.scale, 4.0);
    assert!(close(exact.world_to_screen(Vec2::new(10.0, 0.0)), Vec2::new(680.0, 360.0)));
    let wide = CameraView::compute(Vec2::ZERO, 0.0, &fit, Vec2::ZERO, Vec2::new(1600.0, 720.0));
    assert_eq!(wide.scale, 4.0, "the shorter axis decides");
    assert_eq!(wide.bounds.right - wide.bounds.left, 400.0, "extra world on the long axis");
    assert_eq!(wide.bounds.bottom - wide.bounds.top, 180.0);
  }

  #[test]
  fn a_quarter_turn_shows_the_right_hand_side_above() {
    let view = CameraView::compute(
      Vec2::new(50.0, 50.0),
      FRAC_PI_2,
      &Camera2d::default(),
      Vec2::ZERO,
      Vec2::new(100.0, 100.0),
    );
    assert!(close(view.world_to_screen(Vec2::new(60.0, 50.0)), Vec2::new(50.0, 40.0)));
  }

  #[test]
  fn a_viewport_is_a_rectangle_of_the_target() {
    let half = Camera2d { viewport: Viewport::new(0.5, 0.0, 0.5, 1.0), ..Default::default() };
    let view = CameraView::compute(Vec2::new(10.0, 10.0), 0.0, &half, Vec2::ZERO, Vec2::new(800.0, 600.0));
    assert_eq!((view.rect_min, view.rect_size), (Vec2::new(400.0, 0.0), Vec2::new(400.0, 600.0)));
    assert!(close(view.world_to_screen(Vec2::new(10.0, 10.0)), Vec2::new(600.0, 300.0)));
  }

  #[test]
  fn conversions_round_trip_through_everything() {
    let camera = Camera2d {
      zoom: 1.7,
      projection: Projection::Fit { width: 300.0, height: 200.0 },
      viewport: Viewport::new(0.1, 0.2, 0.6, 0.7),
      ..Default::default()
    };
    let view = CameraView::compute(Vec2::new(-30.0, 75.0), 0.6, &camera, Vec2::new(3.0, -2.0), Vec2::new(1024.0, 768.0));
    for p in [Vec2::ZERO, Vec2::new(123.0, -45.0), Vec2::new(-300.0, 900.0)] {
      assert!(close(view.screen_to_world(view.world_to_screen(p)), p), "{p}");
    }
  }

  #[test]
  fn the_matrix_agrees_with_world_to_screen() {
    let camera = Camera2d { zoom: 1.3, ..Default::default() };
    let view = CameraView::compute(Vec2::new(20.0, 40.0), 0.4, &camera, Vec2::new(2.0, 1.0), Vec2::new(200.0, 100.0));
    let world = Vec2::new(35.0, 52.0);
    let ndc = view.view_proj.project_point3(world.extend(0.0)).truncate();
    let screen = view.rect_min + (Vec2::new(ndc.x, -ndc.y) + Vec2::ONE) / 2.0 * view.rect_size;
    assert!(close(screen, view.world_to_screen(world)));
  }

  #[test]
  fn degenerate_settings_stay_finite() {
    for zoom in [0.0, -1.0, f32::NAN, f32::INFINITY] {
      for projection in [Projection::ScreenUnits, Projection::Fit { width: 0.0, height: f32::NAN }] {
        let camera = Camera2d { zoom, projection, ..Default::default() };
        let view = CameraView::compute(Vec2::new(f32::NAN, 1.0), f32::NAN, &camera, Vec2::ZERO, Vec2::ZERO);
        assert!(view.view_proj.is_finite(), "{zoom} {projection:?}");
        assert!(view.world_to_screen(Vec2::new(5.0, 5.0)).is_finite());
      }
    }
  }

  #[test]
  fn a_rotated_view_sees_a_wider_box() {
    let view = CameraView::compute(Vec2::ZERO, std::f32::consts::FRAC_PI_4, &Camera2d::default(), Vec2::ZERO, Vec2::new(100.0, 100.0));
    assert!(view.bounds.right > 70.0, "{:?}", view.bounds);
    assert!(view.is_visible(Vec2::new(60.0, -1.0), Vec2::new(61.0, 1.0)));
    assert!(!view.is_visible(Vec2::new(80.0, 80.0), Vec2::new(90.0, 90.0)));
  }
}
```

`src/core/graphics/render_layers.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn layers_intersect_when_they_share_one() {
    assert_eq!(RenderLayers::default(), RenderLayers::layer(0));
    assert!(RenderLayers::layer(3).with(5).intersects(&RenderLayers::layer(5)));
    assert!(!RenderLayers::layer(3).intersects(&RenderLayers::layer(4)));
    assert!(RenderLayers::all().intersects(&RenderLayers::layer(31)));
    assert!(!RenderLayers::none().intersects(&RenderLayers::all()));
    assert_eq!(RenderLayers::layer(40), RenderLayers::layer(31), "out-of-range layers clamp to 31");
  }
}
```

Run: `cargo test --lib -p ryo2d -- camera2d render_layers 2>&1 | grep -E 'test result|panicked' | head`
Expected: FAIL at `todo!()`.

- [ ] **Step 2: Implement**

```rust
impl CameraView {
  pub fn compute(center: Vec2, rotation: f32, camera: &Camera2d, shake: Vec2, target_size: Vec2) -> Self {
    let finite = |v: Vec2| if v.is_finite() { v } else { Vec2::ZERO };
    let positive = |x: f32| if x.is_finite() && x > 0.0 { x } else { 1.0 };
    let target_size = finite(target_size).max(Vec2::ONE);
    let rect_min = camera.viewport.min.clamp(Vec2::ZERO, Vec2::ONE) * target_size;
    let rect_size = ((camera.viewport.max - camera.viewport.min).clamp(Vec2::ZERO, Vec2::ONE) * target_size)
      .max(Vec2::ONE);
    let zoom = if camera.zoom.is_finite() { camera.zoom.clamp(0.01, 100.0) } else { 1.0 };
    let base = match camera.projection {
      Projection::ScreenUnits => 1.0,
      Projection::Fit { width, height } => (rect_size.x / positive(width)).min(rect_size.y / positive(height)),
    };
    let scale = base * zoom;
    let rotation = if rotation.is_finite() { rotation } else { 0.0 };
    let (center, shake) = (finite(center), finite(shake));
    let view_proj = Mat4::from_scale(Vec3::new(2.0 / rect_size.x, -2.0 / rect_size.y, 1.0))
      * Mat4::from_translation(shake.extend(0.0))
      * Mat4::from_scale(Vec3::new(scale, scale, 1.0))
      * Mat4::from_rotation_z(-rotation)
      * Mat4::from_translation((-center).extend(0.0));
    let half = rect_size / (2.0 * scale);
    let (sin, cos) = rotation.sin_cos();
    let extent = Vec2::new(
      half.x * cos.abs() + half.y * sin.abs(),
      half.x * sin.abs() + half.y * cos.abs(),
    );
    let bounds = CameraBounds {
      left: center.x - extent.x,
      right: center.x + extent.x,
      top: center.y - extent.y,
      bottom: center.y + extent.y,
    };
    Self { view_proj, center, rotation, scale, shake, rect_min, rect_size, bounds }
  }

  pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
    let local = Vec2::from_angle(-self.rotation).rotate(world - self.center) * self.scale + self.shake;
    self.rect_min + self.rect_size / 2.0 + local
  }

  pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
    let local = screen - self.rect_min - self.rect_size / 2.0 - self.shake;
    Vec2::from_angle(self.rotation).rotate(local / self.scale) + self.center
  }

  pub fn is_visible(&self, min: Vec2, max: Vec2) -> bool {
    max.x > self.bounds.left && min.x < self.bounds.right && max.y > self.bounds.top && min.y < self.bounds.bottom
  }
}

impl Default for CameraView {
  fn default() -> Self {
    Self::compute(Vec2::ZERO, 0.0, &Camera2d::default(), Vec2::ZERO, Vec2::ONE)
  }
}

impl RenderLayers {
  pub const fn layer(n: u8) -> Self {
    let n = if n > 31 { 31 } else { n };
    Self(1 << n)
  }
  pub const fn all() -> Self {
    Self(u32::MAX)
  }
  pub const fn none() -> Self {
    Self(0)
  }
  pub const fn with(self, n: u8) -> Self {
    Self(self.0 | Self::layer(n).0)
  }
  pub const fn intersects(&self, other: &RenderLayers) -> bool {
    self.0 & other.0 != 0
  }
}

impl Default for RenderLayers {
  fn default() -> Self {
    Self::layer(0)
  }
}
```

`CameraShake` moves from `camera.rs` unchanged apart from deriving `Component` and the method names in the Produces block (`configure`, `set_curve`, `curve`, `reset`, `offset() -> Vec2`). Rename `RenderLayer` to `ZLayer` everywhere.

Run the Step 1 command. Expected: PASS.

- [ ] **Step 3: Suite, commit**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{f+=$6} END {print f}' && ./dev.sh lint 2>&1 | grep -E '^(warning|error)' | grep -v 'future version' | head -3`
Expected: `0`, no lint lines.

```bash
git add -A
git commit -m "feat: camera components and the view maths

Camera2d, Projection, Viewport, CameraTarget, CameraClear, MainCamera,
CameraShake and CameraView, with rotation, zoom, Fit projection,
viewports and shake; RenderLayers for per-camera visibility. The z-order
constants RenderLayer are renamed ZLayer."
```

### Task 11: The main camera entity and `ctx.camera()`; the `Camera` resource goes

**Files:**
- Create: `src/core/graphics/camera_mut.rs`
- Modify: `src/core/graphics/{camera.rs,mod.rs,gpu_context.rs,parallax_gpu.rs}`, `src/core/engine/{mod.rs,frame.rs,extract.rs}`, `src/core/scene/{context.rs,cleanup.rs}`, `src/core/ecs/world_ext.rs`, `src/core/render/entities.rs`, `src/core/particles/mod.rs`, `src/core/config/debug.rs`, `src/lib.rs`; every `ctx.camera` user (27 sites: `grep -rn 'ctx\.camera\b' examples tests crates src`)
- Test: `src/core/graphics/camera_mut.rs` (unit), `src/core/scene/cleanup.rs` (unit), `tests/e2e/gpu_world.rs`

**Interfaces:**
- Consumes: `Camera2d`, `CameraView`, `CameraShake`, `MainCamera`, `RenderLayers` (Task 10); `ScreenMapping` (Task 8).
- Produces:
  - `ScreenMapping` derives `Resource`; the engine inserts it and replaces it on every resize and scale-factor change.
  - `pub(crate) fn spawn_main_camera(world: &mut World, design: Vec2) -> Entity` spawns `(Transform::from_xy(design.x / 2, design.y / 2), Camera2d { projection: Projection::Fit { width: design.x, height: design.y }, ..default }, MainCamera, CameraShake::default(), Interpolated)`. `design` is the configured resolution (the pixel-art target in pixel-art mode). Called from `init_world_resources`.
  - `pub(crate) fn camera_shake_system(time: Res<Time>, shakes: Query<&mut CameraShake>)` (real delta) and `pub(crate) fn camera_view_system(..)` in `PostUpdateSet::Camera`, chained: for every `Camera2d`, `CameraView::compute(render_global(global, previous, alpha) translation, its rotation, camera, shake offset, target size)` where the target size is `ScreenMapping::screen_size()` for `CameraTarget::Window` (texture targets arrive in Task 17).
  - `pub struct CameraMut<'w> { world: &'w mut World, entity: Entity }` with `entity()`, `position() -> Vec2`, `set_position(x, y)`, `zoom() -> f32`, `set_zoom(f32)` (clamped 0.01..=100), `rotation() -> f32`, `set_rotation(radians)`, `set_projection(Projection)`, `set_viewport(Viewport)`, `follow(target: Vec2, lerp: f32)`, `snap_to(target: Vec2)` (also skips interpolation), `clamp_to_bounds(min: Vec2, max: Vec2)`, `add_trauma(f32)`, `configure_shake(max_offset, decay_rate)`, `set_shake_curve(Ease)`, `trauma() -> f32`, `view() -> CameraView` (computed now from the current, uninterpolated transform, so gameplay conversions see this step's camera), `world_to_screen(Vec2) -> Vec2`, `screen_to_world(Vec2) -> Vec2`, `visible_bounds() -> CameraBounds`.
  - `SceneContext::camera(&mut self) -> CameraMut<'_>` (the `camera` field is removed); `GpuRenderContext::camera(&self) -> &CameraView` (the main camera's view; the `camera` field is removed); `screen_size()` there returns `ScreenMapping::screen_size()`.
  - `despawn_game_entities` adds `Without<MainCamera>` to its filter; `SceneCleanup::CAMERA` resets the main camera's transform (to the design centre), zoom, rotation, projection, viewport and shake, and skips its interpolation.
  - The engine's CPU paths (`render/entities.rs`, `particles/mod.rs`, `config/debug.rs` overlays, `engine/extract.rs` god rays and trail/light matrices, `parallax_gpu.rs`) read the main camera's `CameraView` (`world_to_screen`, `scale`, `center`, `bounds`) instead of the resource; behaviour is unchanged at rotation 0 (Task 12 moves sprites to the GPU).
  - Deleted: `Camera` (the resource struct) and its tests; `CameraBounds` lives in `camera2d.rs`.

Migration rules for `ctx.camera` call sites:

| Before | After |
|---|---|
| `ctx.camera.set_position(x, y)` | `ctx.camera().set_position(x, y)` |
| `ctx.camera.x`, `ctx.camera.y` | `ctx.camera().position().x`, `.y` |
| `ctx.camera.zoom` / `set_zoom(z)` | `ctx.camera().zoom()` / `set_zoom(z)` |
| `ctx.camera.follow(x, y, f)` | `ctx.camera().follow(Vec2::new(x, y), f)` |
| `ctx.camera.snap_to(x, y)` | `ctx.camera().snap_to(Vec2::new(x, y))` |
| `ctx.camera.world_to_screen(x, y)` returning `(f32, f32)` | `ctx.camera().world_to_screen(Vec2::new(x, y))` returning `Vec2` |
| `ctx.camera.screen_to_world(x, y)` | `ctx.camera().screen_to_world(Vec2::new(x, y))` |
| `ctx.camera.clamp_to_bounds(x0, y0, x1, y1)` | `ctx.camera().clamp_to_bounds(Vec2::new(x0, y0), Vec2::new(x1, y1))` |
| `ctx.camera.add_trauma(a)`, `configure_shake(o, d)`, `set_shake_curve(c)`, `trauma()` | the same on `ctx.camera()` |
| `ctx.camera.visible_bounds()` | `ctx.camera().visible_bounds()` |
| `gpu.camera().x` (in `render`) | `gpu.camera().center.x` |

- [ ] **Step 1: Write the failing tests** (`src/core/graphics/camera_mut.rs`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::World;
  use crate::core::graphics::{Presentation, ScreenMapping};

  fn world_with_main_camera() -> (World, Entity) {
    let mut world = World::new();
    world.insert_resource(ScreenMapping::new(Presentation::ResolutionIndependent, (800, 600), 1.0));
    let camera = spawn_main_camera(&mut world, Vec2::new(800.0, 600.0));
    (world, camera)
  }

  #[test]
  fn the_main_camera_starts_with_world_equal_to_screen() {
    let (mut world, _) = world_with_main_camera();
    let camera = CameraMut::main(&mut world).unwrap();
    assert_eq!(camera.position(), Vec2::new(400.0, 300.0));
    assert!((camera.world_to_screen(Vec2::new(10.0, 20.0)) - Vec2::new(10.0, 20.0)).length() < 1e-3);
  }

  #[test]
  fn moving_and_zooming_change_the_view_at_once() {
    let (mut world, entity) = world_with_main_camera();
    let mut camera = CameraMut::main(&mut world).unwrap();
    camera.set_position(100.0, 100.0);
    camera.set_zoom(2.0);
    assert!((camera.world_to_screen(Vec2::new(110.0, 100.0)) - Vec2::new(420.0, 300.0)).length() < 1e-3);
    camera.set_zoom(-3.0);
    assert_eq!(camera.zoom(), 0.01);
    assert_eq!(world.get::<Transform>(entity).unwrap().translation, Vec2::new(100.0, 100.0));
  }

  #[test]
  fn follow_moves_part_of_the_way() {
    let (mut world, _) = world_with_main_camera();
    let mut camera = CameraMut::main(&mut world).unwrap();
    camera.follow(Vec2::new(500.0, 300.0), 0.5);
    assert_eq!(camera.position(), Vec2::new(450.0, 300.0));
  }

  #[test]
  fn clamping_keeps_the_view_inside_the_bounds() {
    let (mut world, _) = world_with_main_camera();
    let mut camera = CameraMut::main(&mut world).unwrap();
    camera.set_zoom(2.0); // 400 x 300 world units visible
    camera.set_position(0.0, 0.0);
    camera.clamp_to_bounds(Vec2::ZERO, Vec2::new(1000.0, 1000.0));
    assert_eq!(camera.position(), Vec2::new(200.0, 150.0));
  }
}
```

`CameraMut::main(world: &mut World) -> Option<CameraMut<'_>>` finds the `MainCamera` entity (used by `SceneContext::camera`, which unwraps it with a documented invariant: the engine always has one).

Append to `src/core/scene/cleanup.rs` tests:

```rust
  #[test]
  fn world_cleanup_keeps_the_main_camera_and_camera_cleanup_resets_it() {
    let (mut world, mut audio) = cleanup_world(); // the file's existing helper
    world.insert_resource(ScreenMapping::new(Presentation::ResolutionIndependent, (800, 600), 1.0));
    let entity = spawn_main_camera(&mut world, Vec2::new(800.0, 600.0));
    CameraMut::main(&mut world).unwrap().set_zoom(3.0);
    execute_cleanup(SceneCleanup::WORLD, &mut world, &mut audio);
    assert!(world.get_entity(entity).is_ok(), "the main camera survives a world cleanup");
    execute_cleanup(SceneCleanup::CAMERA, &mut world, &mut audio);
    assert_eq!(world.get::<Camera2d>(entity).unwrap().zoom, 1.0);
  }
```

(If `cleanup.rs` has no world helper, build the world with `crate::core::scene::init_world_resources_for_tests` and `Audio::new(None)` as `scene/manager.rs` tests do.)

Run: `cargo test --lib -p ryo2d -- camera_mut cleanup 2>&1 | grep -E 'error\[|test result|panicked' | head`
Expected: compile errors (`CameraMut`, `spawn_main_camera` missing).

- [ ] **Step 2: Implement the Produces block and migrate the call sites**

Implement `camera_mut.rs`, the systems and the main camera spawn; remove the resource scope for `Camera` in `SceneContext::scoped_parts`; switch every engine reader to the main camera's `CameraView` (a helper `pub(crate) fn main_view(world: &World) -> CameraView` returning the main camera's `CameraView` or the default); apply the migration rules to examples and tests.

Run: `cargo test --lib -p ryo2d -- camera_mut cleanup 2>&1 | grep -E 'test result|panicked'`
Expected: PASS.

- [ ] **Step 3: Suite, pixels, commit**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{f+=$6} END {print f}' && ./dev.sh lint 2>&1 | grep -E '^(warning|error)' | grep -v 'future version' | head -3; ./dev.sh demos:tour >/dev/null && target/renderer-baseline/compare.sh`
Expected: `0`, no lint lines; compare lists only the `y-sorting` pages (Task 8). The camera-shake demo captures may differ only if shake is mid-flight at the capture frame: compare by eye and ledger it.

```bash
git add -A
git commit -m "feat: the camera is an entity; ctx.camera() replaces the resource

- the engine spawns a MainCamera with Camera2d, CameraShake and
  Interpolated; world cleanup keeps it, camera cleanup resets it
- CameraView is computed every frame from the interpolated transform
- CameraMut offers position, zoom, rotation, follow, shake and
  conversions on the main camera"
```

### Task 12: Sprites draw in world space through each camera's matrix

**Files:**
- Create: `src/core/graphics/camera_gpu.rs`
- Modify: `src/core/graphics/sprite_gpu.rs` (`SpriteGlobals` becomes `CameraGlobals`, per-camera bind groups), `src/core/shaders/core/sprite.wgsl`, `src/core/shaders/custom.rs` (the sprite wrapper reads `globals.game_time`), `src/core/render/entities.rs` (world-space instances, per-camera culling), `src/core/render/passes/world.rs`, `src/core/engine/extract.rs`
- Test: `tests/e2e/gpu_world.rs`, `src/core/graphics/camera_gpu.rs` (unit)

**Interfaces:**
- Consumes: `CameraView` (Tasks 10-11).
- Produces:
  - `#[repr(C)] #[derive(Pod, Zeroable, Clone, Copy)] pub(crate) struct CameraGlobals { view_proj: [[f32; 4]; 4], resolution: [f32; 2], game_time: f32, real_time: f32, snap: f32, _pad: [f32; 3] }` (96 bytes) with `pub(crate) fn new(view: &CameraView, image_size: (u32, u32), game_time: f32, real_time: f32, snap: bool) -> Self`.
  - `pub(crate) struct CameraUniforms` keyed by camera `Entity`: `sync(&mut self, device, queue, cameras: &[(Entity, CameraGlobals)])` creates or updates one uniform buffer per camera and drops entries whose camera is gone; `buffer(&self, camera: Entity) -> Option<&wgpu::Buffer>`; `len()`.
  - `SpriteGpuRenderer` builds its group-0 bind group per camera from that buffer (`HashMap<Entity, wgpu::BindGroup>`, pruned with the uniforms) and `draw_world_run(&self, pass, run, camera: Entity)`.
  - Sprite instances carry world transforms: `render/entities.rs` computes each sprite's world-space quad centre (anchor pivot offset rotated with the sprite), size (`texture size * scale`), rotation and flips; no `Placement`, no `as i32`, no screen-space `instance_for`. Each camera culls by `CameraView::is_visible` on the quad's world AABB (centre plus or minus the half-diagonal).
  - Pixel-art mode sets `snap`: the vertex shader rounds an axis-aligned instance's vertices to whole pixels of the camera image.

- [ ] **Step 1: Write the failing pixel tests** (append to `tests/e2e/gpu_world.rs`)

```rust
#[test]
fn a_rotated_camera_turns_the_world() {
  // A red square 10 px right of the camera centre shows 10 px above it
  // when the camera turns a quarter clockwise
  let scene = Fixture::new(|ctx| {
    ctx.camera().set_position(32.0, 32.0);
    ctx.camera().set_rotation(std::f32::consts::FRAC_PI_2);
    ctx.world.spawn((
      Transform::from_xy(42.0, 32.0),
      Renderable::sprite(red_rect(4, 4, "turned")),
    ));
  });
  let Some(frame) = frame_of(scene, (64, 64)) else {
    return;
  };
  assert_eq!(frame.get_pixel(32, 22).0, ANCHOR_RED);
  assert_ne!(frame.get_pixel(42, 32).0, ANCHOR_RED);
}

#[test]
fn a_fractional_position_is_not_truncated() {
  // At scale factor 2, a sprite whose left edge is at x = 10.5 starts at
  // physical pixel 21, not 20
  let scene = Fixture::new(|ctx| {
    ctx.world.spawn((
      Transform::from_xy(10.5, 10.0),
      Renderable::sprite(red_rect(10, 10, "half")).with_anchor(Anchor::TopLeft),
    ));
  });
  let builder = EngineBuilder::new().resolution(32, 32).start_scene(scene);
  let Some(mut engine) = HeadlessEngine::new_or_skip_with_scale_factor(builder, 2.0) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  assert_ne!(frame.get_pixel(20, 25).0, ANCHOR_RED);
  assert_eq!(frame.get_pixel(21, 25).0, ANCHOR_RED);
  assert_eq!(frame.get_pixel(40, 25).0, ANCHOR_RED);
  assert_ne!(frame.get_pixel(41, 25).0, ANCHOR_RED);
}

#[test]
fn pixel_art_sprites_land_on_whole_pixels() {
  // The camera sits a third of a pixel off; every pixel of the sprite's row
  // is fully red or fully background, never a blend
  let scene = Fixture::new(|ctx| {
    ctx.camera().set_position(16.33, 16.0);
    ctx.world.spawn((
      Transform::from_xy(16.0, 16.0),
      Renderable::sprite(red_rect(9, 9, "snap")),
    ));
  });
  let builder = EngineBuilder::new().pixel_art(32, 32).start_scene(scene);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let background = frame.get_pixel(0, 16).0;
  let reds = (0..32).filter(|&x| frame.get_pixel(x, 16).0 == ANCHOR_RED).count();
  assert_eq!(reds, 9);
  for x in 0..32 {
    let pixel = frame.get_pixel(x, 16).0;
    assert!(pixel == ANCHOR_RED || pixel == background, "blend at x = {x}: {pixel:?}");
  }
}
```

(`Fixture::new` takes a `fn(&mut SceneContext)`; `ctx.camera()` is the Task 11 API.)

Run: `cargo test -p ryo2d --test e2e -- rotated_camera fractional_position whole_pixels 2>&1 | grep -E 'test result|panicked' -A1 | head`
Expected: `a_rotated_camera_turns_the_world` and `a_fractional_position_is_not_truncated` FAIL (the CPU path ignores rotation and truncates); `pixel_art_sprites_land_on_whole_pixels` may pass (truncation also snaps); keep it as a guard.

Unit test (`src/core/graphics/camera_gpu.rs`, no GPU: the bookkeeping is separated from buffer creation through a small trait or by testing with `shared_gpu()` and skipping without one):

```rust
#[test]
fn uniforms_follow_the_live_cameras() {
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let mut world = crate::core::ecs::World::new();
  let (a, b) = (world.spawn_empty().id(), world.spawn_empty().id());
  let globals = CameraGlobals::new(&CameraView::default(), (8, 8), 0.0, 0.0, false);
  let mut uniforms = CameraUniforms::default();
  uniforms.sync(&device, &queue, &[(a, globals), (b, globals)]);
  assert_eq!(uniforms.len(), 2);
  uniforms.sync(&device, &queue, &[(b, globals)]);
  assert_eq!(uniforms.len(), 1);
  assert!(uniforms.buffer(a).is_none() && uniforms.buffer(b).is_some());
}
```

- [ ] **Step 2: Implement**

`sprite.wgsl`'s `Globals` becomes:

```wgsl
struct Globals {
    view_proj: mat4x4<f32>,
    resolution: vec2<f32>,
    game_time: f32,
    real_time: f32,
    snap: f32,
}
```

and `vs_main` ends with:

```wgsl
    var clip = globals.view_proj * vec4<f32>(world_pos.xy, 0.0, 1.0);
    // Pixel art: axis-aligned quads put their corners on whole pixels
    let axis_aligned = abs(instance.transform0.y) < 1e-5 && abs(instance.transform1.x) < 1e-5;
    if globals.snap > 0.5 && axis_aligned {
        let pixel = (clip.xy * vec2<f32>(0.5, -0.5) + 0.5) * globals.resolution;
        let snapped = round(pixel) / globals.resolution;
        clip = vec4<f32>((snapped - 0.5) * vec2<f32>(2.0, -2.0), clip.z, clip.w);
    }
    out.clip_position = clip;
```

The custom sprite wrapper in `shaders/custom.rs` reads `globals.game_time` where it read `globals.time_resolution.x`. `extract.rs` builds one `CameraGlobals` per camera from its `CameraView` (only the main camera is drawn until Task 15) and calls `CameraUniforms::sync`; the world pass binds the main camera's group 0.

Run the Step 1 commands. Expected: PASS.

- [ ] **Step 3: Suite, pixels, commit**

Run the Task 11 Step 3 commands.
Expected: `0`, no lint lines. The tour may now differ where pixel-art sprites used to be truncated toward zero and are now rounded to the nearest pixel (a 1 px shift for sprites at fractional positions); open each differing page beside its baseline and confirm the shift is at most 1 px; record the list in the ledger.

```bash
git add -A
git commit -m "feat: sprites draw in world space through each camera's matrix

- a per-camera uniform holds the view-projection, time and snapping
- instances carry world transforms; no CPU screen transform or
  truncation; culling is per camera and rotation-aware
- pixel-art mode snaps axis-aligned sprites to whole pixels on the GPU"
```

### Task 13: Particles, parallax, `Scene::render` sprites and debug overlays in world space

**Files:**
- Modify: `src/core/particles/mod.rs` (`collect_for_render`), `src/core/graphics/{parallax_gpu.rs,gpu_context.rs}`, `src/core/config/debug.rs` (overlays), `src/core/engine/extract.rs`; examples using `Scene::render` instances (`grep -rln 'fn render(&mut self, gpu' examples`)
- Test: `src/core/particles/mod.rs`, `src/core/graphics/parallax_gpu.rs`, `src/core/config/debug.rs` (unit); `tests/e2e/gpu_world.rs`

**Interfaces:**
- Consumes: `CameraView` and `GpuRenderContext::camera()` (Task 11); world-space sprite drawing (Task 12).
- Produces:
  - `ParticleSystem::collect_for_render(&self, world: &World, collector: &mut BatchCollector)`: instances centred on the particle's world position, sized in world units (no camera argument).
  - `GpuRenderContext`: instances added in `render` are world-space; `camera()` is the main camera's view for games that place things relative to it.
  - `GpuParallaxBackground::render` lays each layer out in the main camera's screen space from its `center.x`, `center.y` and `scale` (scroll speed applies to both axes; `TileMode::Horizontal`/`Vertical`/`Both` tile across the camera's rectangle) and converts every instance to world space with `screen_to_world` (centre) and `1 / scale` (size).
  - Plan ruling (deviation from the spec's "parallax per camera"): parallax follows the **main** camera. `GpuParallaxBackground` is drawn from `Scene::render`, which runs once per frame, so per-camera parallax needs a parallax component; that belongs with the shared batch in 2b. Other cameras see the background where it sits in the world.
  - `overlay_lines(world, flags)` builds world-space `Line2D`s (collider outlines at the collider's world bounds, velocity arrows from the origin, origin crosses), with widths of `1 / scale` world units so they stay one screen pixel wide (it takes the main camera's `scale`).

- [ ] **Step 1: Write the failing unit tests**

`src/core/particles/mod.rs`:

```rust
  #[test]
  fn particles_are_centred_on_their_world_position() {
    let mut world = World::new();
    let mut system = ParticleSystem::new();
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(2, 2, 255, 255, 255, 255), "dot");
    let emitter = world
      .spawn((Transform::from_xy(100.0, 50.0), ParticleEmitter::new(sprite).with_size(8.0, 8.0)))
      .id();
    system.spawn_test_particle(emitter, Vec2::new(100.0, 50.0), 8.0); // crate-private test helper
    let mut collector = BatchCollector::new();
    system.collect_for_render(&world, &mut collector);
    let instance = collector.instances().next().unwrap();
    assert_eq!((instance.transform[2][0], instance.transform[2][1]), (100.0, 50.0));
  }
```

(`spawn_test_particle(emitter, position, size)` and `BatchCollector::instances()` are `#[cfg(test)]` helpers added in this step; read `transform` as `with_transform` writes it: the third column holds the centre.)

`src/core/graphics/parallax_gpu.rs`:

```rust
  #[test]
  fn a_layer_scrolls_with_the_camera_on_both_axes() {
    let layer = GpuParallaxLayer::test_layer(64, 64).with_scroll_speed(0.5);
    let near = CameraView::compute(Vec2::new(100.0, 100.0), 0.0, &Camera2d::default(), Vec2::ZERO, Vec2::new(64.0, 64.0));
    let far = CameraView::compute(Vec2::new(120.0, 140.0), 0.0, &Camera2d::default(), Vec2::ZERO, Vec2::new(64.0, 64.0));
    let a = layer.world_instances(&near, 0.0)[0].transform[2];
    let b = layer.world_instances(&far, 0.0)[0].transform[2];
    // Half the camera's movement in world space: 10 right and 20 down
    assert_eq!((b[0] - a[0], b[1] - a[1]), (10.0, 20.0));
  }
```

(`world_instances(view, elapsed)` replaces `compute_instances(screen_width, screen_height, camera_x, elapsed)`; `test_layer(w, h)` is a `#[cfg(test)]` constructor with a solid sprite.)

`src/core/config/debug.rs` (replace the existing overlay test's camera construction):

```rust
  #[test]
  fn collider_outlines_are_drawn_where_the_collider_is() {
    let mut world = World::new();
    world.spawn((Transform::from_xy(100.0, 50.0), Collider::box_collider(20.0, 10.0)));
    let colliders = OverlayFlags { colliders: true, ..Default::default() };
    let lines = overlay_lines(&mut world, colliders, 1.0);
    let points: Vec<Vec2> = lines.iter().flat_map(|line| line.points().iter().copied()).collect();
    assert!(points.contains(&Vec2::new(90.0, 45.0)) && points.contains(&Vec2::new(110.0, 55.0)), "{points:?}");
  }
```

(`overlay_lines(world, flags, camera_scale)`; `Line2D::points()` exists or is added as a read accessor.)

Run: `cargo test --lib -p ryo2d -- particles_are_centred scrolls_with_the_camera collider_outlines 2>&1 | grep -E 'error\[|test result|panicked' | head`
Expected: compile errors for the new signatures.

- [ ] **Step 2: Write the failing e2e test** (append to `tests/e2e/gpu_world.rs`)

```rust
#[test]
fn render_gpu_sprites_are_placed_in_the_world() {
  // A render_gpu sprite at world (40, 32) shows 10 px further left when the
  // camera moves 10 px right
  let scene = Fixture::new(|ctx| ctx.camera().set_position(42.0, 32.0))
    .with_gpu_sprite(solid("gpu-world", 4, RED), 40.0, 32.0, 4.0, 0);
  let Some(frame) = frame_of(scene, (64, 64)) else {
    return;
  };
  assert_eq!(frame.get_pixel(30, 32).0, RED);
  assert_ne!(frame.get_pixel(40, 32).0, RED);
}
```

Run: `cargo test -p ryo2d --test e2e -- render_gpu_sprites_are_placed 2>&1 | grep -E 'test result|panicked'`
Expected: FAIL (today `render_gpu` instances are screen pixels).

- [ ] **Step 3: Implement** the Produces block; update `Scene::render` users in `examples/` (instances become world-space; for screen-fixed drawing use `gpu.camera().screen_to_world(..)`).

Run the Step 1 and Step 2 commands. Expected: PASS.

- [ ] **Step 4: Suite, pixels, commit**

Run the Task 11 Step 3 commands. Expected: `0`, no lint lines; tour differences limited to the Task 12 ledger list plus particle demo pages (particles move by half their size, the convention fix): confirm by eye and ledger.

```bash
git add -A
git commit -m "feat: particles, parallax, render_gpu sprites and overlays in world space

- particles are centred on their position
- parallax scrolls with the main camera on both axes and with its zoom
- render_gpu instances are world-space
- debug overlays are world-space lines"
```

### Task 14: Trails, lines, lights, shadows and god rays on the camera's matrix

**Files:**
- Modify: `src/core/trail/renderer.rs`, `src/core/shaders/core/{trail.wgsl,trail_textured.wgsl,light.wgsl,shadow.wgsl}`, `src/core/lighting/renderer.rs`, `src/core/render/{frame.rs,passes/lighting.rs,passes/world.rs}`, `src/core/engine/extract.rs`
- Test: `tests/e2e/gpu_world.rs`, `tests/e2e/render_pipeline.rs`, `src/core/testing/gpu_world.rs`

**Interfaces:**
- Consumes: `CameraUniforms`, `CameraGlobals` (Task 12).
- Produces:
  - `TrailGlobals`, `LightGlobals` and `ShadowGlobals` are replaced by the `CameraGlobals` layout in their shaders (`time` reads `real_time`).
  - `TrailRenderer::prepare(&mut self, queue)` uploads world-space vertices only; `draw_group(&self, pass, lit: bool, z: i32, camera: Entity)` binds that camera's group 0 (built from `CameraUniforms::buffer(camera)`, cached per camera and pruned with the uniforms).
  - `LightingRenderer::render_lights(&mut self, device, queue, encoder, lights, camera: Entity, uniforms: &CameraUniforms)` and `render_shadows(.., camera, uniforms)`; the light and shadow bind groups are per camera (the light one also holds the light storage buffer and is rebuilt when that buffer grows). `camera_pos`/`camera_zoom` parameters and `FrameScene::{camera_pos, camera_zoom}` are removed.
  - `LightingRenderer::update_god_rays(&mut self, effects, lights, view: &CameraView)` places each ray at `view.world_to_screen(light) / view.rect_size` (UV).

- [ ] **Step 1: Write the failing tests**

`tests/e2e/gpu_world.rs`:

```rust
#[test]
fn shake_moves_sprites_and_lines_together() {
  let scene = Fixture::new(|ctx| {
    let mut camera = ctx.camera();
    camera.configure_shake(12.0, 0.0);
    camera.add_trauma(1.0);
    spawn_line(ctx, 0); // green across y 32, rows 30..34
    ctx.world.spawn((
      Transform::from_xy(40.0, 32.0),
      Renderable::sprite(solid("shake-red", 10, RED)).with_anchor(Anchor::TopLeft),
    ));
  });
  let builder = EngineBuilder::new().resolution(64, 64).start_scene(scene);
  let Some(mut engine) = start(builder) else {
    return;
  };
  // Step until the shake has moved the view by at least 3 px vertically
  let mut shaken = false;
  for _ in 0..120 {
    engine.step_frames(1).unwrap();
    let mut views = engine.world_mut().query::<&CameraView>();
    let shake = views.iter(engine.world()).next().unwrap().shake;
    if shake.y.abs() >= 3.0 {
      shaken = true;
      break;
    }
  }
  assert!(shaken, "the shake never moved the view");
  let frame = engine.frame().unwrap();
  let first = |x: u32, hit: &dyn Fn([u8; 4]) -> bool| (0..64).find(|&y| hit(frame.get_pixel(x, y).0));
  let line_top = first(10, &is_line).unwrap();
  let sprite_top = first(45, &|p| p == RED).unwrap();
  assert_eq!(sprite_top as i32 - line_top as i32, 2, "unshaken they are 2 rows apart");
}

#[test]
fn a_rotated_camera_turns_lines_too() {
  let scene = Fixture::new(|ctx| {
    ctx.camera().set_position(32.0, 32.0);
    ctx.camera().set_rotation(std::f32::consts::FRAC_PI_2);
    spawn_line(ctx, 0); // horizontal through the centre
  });
  let Some(frame) = frame_of(scene, (64, 64)) else {
    return;
  };
  assert!(is_line(frame.get_pixel(32, 5).0), "now vertical through the centre");
  assert!(!is_line(frame.get_pixel(5, 32).0));
}
```

`src/core/testing/gpu_world.rs`:

```rust
#[test]
fn an_interpolated_camera_draws_between_its_last_two_steps() {
  use std::time::Duration;
  /// Pans the main camera 20 px right every fixed step over a still sprite.
  struct Pan;
  impl Scene for Pan {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 220, 30, 40, 255), "still");
      ctx.world.spawn((
        Transform::from_xy(58.0, 10.0),
        Renderable::sprite(sprite).with_anchor(Anchor::TopLeft),
      ));
    }
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      let mut camera = ctx.camera();
      let p = camera.position();
      camera.set_position(p.x + 20.0, p.y);
      SceneAction::Continue
    }
  }
  let builder = EngineBuilder::new().resolution(120, 30).fixed_hz(50).start_scene(Pan);
  let Some(mut engine) = engine(builder) else {
    return;
  };
  // The camera starts at x = 60; two steps take it to 100 (from 80)
  engine.step_frames(2).unwrap();
  // One more step (to 120) and half a step left over: drawn at 110, so the
  // sprite at world 58 shows at screen 58 - 110 + 60 = 8
  engine.engine.frame(Some(Duration::from_millis(30)));
  let frame = engine.frame().unwrap();
  assert_eq!(frame.get_pixel(9, 11).0, RED, "drawn from the interpolated camera");
}
```

`tests/e2e/render_pipeline.rs` (it has the `engine`, `white_sprite` and lit-scene helpers):

```rust
/// A white scene under dark ambient with one light 16 px right of the
/// camera centre; the camera turns a quarter clockwise.
struct LitTurned;

impl Scene for LitTurned {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
    ctx.world.spawn((
      Transform::from_xy(48.0, 32.0),
      Light2D::point(Color::rgb(255, 255, 255), 8.0),
    ));
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.02));
    ctx.camera().set_position(32.0, 32.0);
    ctx.camera().set_rotation(std::f32::consts::FRAC_PI_2);
  }
}

#[test]
fn lights_turn_with_the_camera() {
  let Some(mut engine) = engine(LitTurned) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let frame = engine.frame().unwrap();
  // 16 right of the centre shows 16 above it: (32, 16)
  let lit = frame.get_pixel(32, 16).0;
  let dark = frame.get_pixel(48, 32).0;
  assert!(lit[0] > dark[0].saturating_add(60), "{lit:?} vs {dark:?}");
}
```

Run: `cargo test -p ryo2d --test e2e -- shake_moves rotated_camera_turns_lines lights_turn_with_the_camera 2>&1 | grep -E 'test result|panicked'; cargo test --lib -p ryo2d -- interpolated_camera 2>&1 | grep -E 'test result|panicked'`
Expected: the three e2e tests FAIL (trails and lights ignore shake and rotation); the interpolation test PASSES already if Task 11's `CameraView` uses `render_global` (keep it as the guard it is; if it fails, fix `camera_view_system`).

- [ ] **Step 2: Implement** the Produces block.

Run the Step 1 commands. Expected: PASS.

- [ ] **Step 3: Suite, pixels, commit**

Run the Task 11 Step 3 commands. Expected: `0`, no lint lines; tour differences limited to the earlier ledger lists, plus god-ray pages if their positions moved by sub-pixel amounts (compare by eye).

```bash
git add -A
git commit -m "feat: trails, lines, lights, shadows and god rays use the camera's matrix

They now rotate and shake with the sprites; one per-camera uniform layout
serves sprites, trails, lights and shadows."
```

---

## Phase 6: rendering per camera

### Task 15: Camera passes run once per camera; window cameras composite into viewports

**Files:**
- Create: `src/core/render/passes/cameras.rs`, `src/core/graphics/camera_images.rs`
- Modify: `src/core/render/{pass.rs,pipeline.rs,context.rs,frame.rs,mod.rs,draw_list.rs,entities.rs}`, `src/core/render/passes/{mod.rs,scene.rs,world.rs,lighting.rs}`, `src/core/graphics/{renderer.rs,sprite_gpu.rs,compositor.rs}`, `src/core/trail/renderer.rs`, `src/core/engine/extract.rs`, `src/lib.rs`
- Test: `tests/e2e/render_pipeline.rs`, `tests/e2e/gpu_world.rs`, `src/core/testing/gpu_world.rs`

**Interfaces:**
- Consumes: `Camera2d`, `CameraView`, `RenderLayers` (Task 10); `CameraUniforms`, `CameraGlobals` (Task 12); per-camera trail and light bind groups (Task 14).
- Produces:
  - `pub enum PassScope { Frame, Camera }` and `fn scope(&self) -> PassScope { PassScope::Frame }` on `RenderPass` (in the prelude). The built-in `clear`, `world`, `lighting` and `unlit_world` return `PassScope::Camera`.
  - `pass_ids::CAMERAS = "cameras"`: a frame pass after `unlit_world` that composites the window cameras' images into the scene image.
  - `RenderPipeline::execute` runs, for each camera in render order (texture targets, then window targets, each by `order`, ties by entity), every enabled camera-scope pass with `RenderContext` pointing at that camera's image; then every enabled frame-scope pass once.
  - `RenderContext::camera(&self) -> Option<&CameraView>` (`Some` inside camera-scope passes), `RenderContext::camera_entity(&self) -> Option<Entity>`; `current_view()`/`encoder_and_current_view()` return the camera image (or its lit image) inside camera-scope passes and the scene image in frame-scope passes.
  - `pub(crate) struct CameraImages`: one texture per window camera that does not draw straight into the scene image, sized to its viewport in render pixels (`render_size * viewport fraction`), recreated when that size changes, dropped when its camera goes. The first window camera in render order whose viewport is `Viewport::FULL` and whose `clear` is not `CameraClear::None` draws straight into the scene image; every other window camera gets an image.
  - The `clear` pass clears to `CameraClear::Background` (today's background colour), `Color(c)` or not at all (`None`, cleared to transparent for a separate image).
  - Drawables carry their `RenderLayers` (sprites through `DrawPayload`, trails and lines through their group key `(lit, z, layers)`); each camera draws only items whose layers intersect its own and whose world bounds intersect its `CameraView::bounds`. `SpriteGpuRenderer::prepare_world(device, queue, list, cameras: &[CameraDraw])` (`pub(crate) struct CameraDraw { entity: Entity, view: CameraView, layers: RenderLayers }`) writes each camera's visible instances to its own range of the instance buffer; `world_runs(camera: Entity) -> &[WorldRun]`.
  - Lighting in this task runs for cameras that draw straight into the scene image; another lit camera draws unlit and warns once (Task 16 lifts this).
  - Per-camera GPU state (uniforms, bind groups, camera images) exists only for live cameras: `pub(crate) fn camera_gpu_entries(&self) -> usize` on the engine (test hook) counts uniforms plus images.

- [ ] **Step 1: Write the failing tests**

`tests/e2e/gpu_world.rs`:

```rust
/// A second camera spawned by a test scene.
fn spawn_camera(ctx: &mut SceneContext, at: Vec2, camera: Camera2d) -> Entity {
  ctx.world.spawn((Transform::from_xy(at.x, at.y), camera)).id()
}

#[test]
fn two_cameras_split_the_window() {
  let scene = Fixture::new(|ctx| {
    // Red at (-1000, 0), blue at (1000, 0); each half looks at one
    ctx.world.spawn((Transform::from_xy(-1000.0, 0.0), Renderable::sprite(solid("split-red", 16, RED))));
    ctx.world.spawn((Transform::from_xy(1000.0, 0.0), Renderable::sprite(solid("split-blue", 16, BLUE))));
    ctx.camera().set_position(-1000.0, 0.0);
    ctx.camera().set_viewport(Viewport::new(0.0, 0.0, 0.5, 1.0));
    spawn_camera(ctx, Vec2::new(1000.0, 0.0), Camera2d {
      viewport: Viewport::new(0.5, 0.0, 0.5, 1.0),
      order: 1,
      ..Default::default()
    });
  });
  let Some(frame) = frame_of(scene, (64, 32)) else {
    return;
  };
  assert_eq!(frame.get_pixel(16, 16).0, RED, "left half centre");
  assert_eq!(frame.get_pixel(48, 16).0, BLUE, "right half centre");
}

#[test]
fn an_overlay_camera_draws_over_the_main_view() {
  let scene = Fixture::new(|ctx| {
    ctx.world.spawn((Transform::from_xy(32.0, 32.0), Renderable::sprite(solid("main-red", 64, RED))));
    ctx.world.spawn((Transform::from_xy(500.0, 500.0), Renderable::sprite(solid("far-green", 8, GREEN))));
    spawn_camera(ctx, Vec2::new(500.0, 500.0), Camera2d {
      viewport: Viewport::new(0.75, 0.0, 0.25, 0.25),
      order: 1,
      clear: CameraClear::None,
      ..Default::default()
    });
  });
  let Some(frame) = frame_of(scene, (64, 64)) else {
    return;
  };
  assert_eq!(frame.get_pixel(56, 8).0, GREEN, "the overlay's centre");
  assert_eq!(frame.get_pixel(50, 2).0, RED, "the overlay does not clear: the main view shows through");
  assert_eq!(frame.get_pixel(20, 40).0, RED, "the main view");
}

#[test]
fn render_layers_choose_what_a_camera_sees() {
  let scene = Fixture::new(|ctx| {
    ctx.world.spawn((
      Transform::from_xy(16.0, 16.0),
      Renderable::sprite(solid("layer-one", 8, GREEN)),
      RenderLayers::layer(1),
    ));
    ctx.camera().set_viewport(Viewport::new(0.0, 0.0, 0.5, 1.0));
    let right = spawn_camera(ctx, Vec2::new(16.0, 16.0), Camera2d {
      viewport: Viewport::new(0.5, 0.0, 0.5, 1.0),
      order: 1,
      ..Default::default()
    });
    ctx.world.entity_mut(right).insert(RenderLayers::layer(1));
  });
  let Some(frame) = frame_of(scene, (64, 32)) else {
    return;
  };
  assert_ne!(frame.get_pixel(16, 16).0, GREEN, "the main camera sees layer 0 only");
  assert_eq!(frame.get_pixel(48, 16).0, GREEN, "the second camera sees layer 1");
}
```

The main camera here is at its default position (32, 16 for a 64x32 window), and `Fit` keeps 64x32 world units visible, so in the half-width viewport the scale is 0.5. Each test above places what it checks at the viewport centres, where scale does not matter.

`tests/e2e/render_pipeline.rs`:

```rust
struct CountPerCamera(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl RenderPass for CountPerCamera {
  fn id(&self) -> &str {
    "count_per_camera"
  }
  fn scope(&self) -> PassScope {
    PassScope::Camera
  }
  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    assert!(ctx.camera().is_some());
    self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
  }
}

struct TwoCameras(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl Scene for TwoCameras {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), Camera2d { order: 1, ..Default::default() }));
    ctx
      .render_pipeline()
      .insert_after(pass_ids::WORLD, CountPerCamera(self.0.clone()))
      .unwrap();
  }
}

#[test]
fn a_camera_pass_runs_once_per_camera() {
  let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
  let Some(mut engine) = engine(TwoCameras(count.clone())) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 6);
}
```

`src/core/testing/gpu_world.rs`:

```rust
#[test]
fn despawned_cameras_free_their_gpu_state() {
  /// Spawns three cameras, then despawns them on the fourth update.
  struct Churn(u32, Vec<Entity>);
  impl Scene for Churn {
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      self.0 += 1;
      if self.0 == 1 {
        for i in 0..3 {
          let camera = Camera2d { viewport: Viewport::new(0.0, 0.0, 0.5, 0.5), order: i + 1, ..Default::default() };
          self.1.push(ctx.world.spawn((Transform::IDENTITY, camera)).id());
        }
      } else if self.0 == 4 {
        for entity in self.1.drain(..) {
          ctx.world.despawn(entity);
        }
      }
      SceneAction::Continue
    }
  }
  let Some(mut engine) = engine(EngineBuilder::new().resolution(32, 32).start_scene(Churn(0, Vec::new()))) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let busy = engine.engine.camera_gpu_entries();
  engine.step_frames(3).unwrap();
  let calm = engine.engine.camera_gpu_entries();
  assert!(busy > calm, "{busy} then {calm}");
  assert_eq!(calm, 1, "only the main camera's uniform remains");
}
```

Run: `cargo test -p ryo2d --test e2e -- split_the_window overlay_camera render_layers_choose a_camera_pass_runs 2>&1 | grep -E 'error\[|test result|panicked' | head`
Expected: compile errors (`PassScope`, `set_viewport` exists from Task 11 but extra cameras are not drawn).

- [ ] **Step 2: Implement** the Produces block. Order of work: `PassScope` and the per-camera loop in `RenderPipeline::execute` (with only the main camera, all existing tests stay green), then `CameraImages` and the `cameras` pass, then per-camera instance ranges and layer filtering, then trail group keys with layers, then GPU-state pruning.

Run the Step 1 commands plus `cargo test --lib -p ryo2d -- despawned_cameras 2>&1 | grep -E 'test result|panicked'`.
Expected: PASS.

- [ ] **Step 3: Suite, pixels, commit**

Run the Task 11 Step 3 commands. Expected: `0`, no lint lines, no new tour differences (every demo has one full-window camera, which draws straight into the scene image).

```bash
git add -A
git commit -m "feat: every camera renders its own view

- camera-scope passes run once per camera into its image; frame passes
  run once
- window cameras composite into their viewports in order; a camera that
  does not clear draws over the ones below
- RenderLayers decide what each camera draws
- per-camera GPU state goes with its camera"
```

### Task 16: Lighting for every camera

**Files:**
- Modify: `src/core/lighting/renderer.rs`, `src/core/render/passes/lighting.rs`, `src/core/graphics/camera_images.rs`
- Test: `tests/e2e/render_pipeline.rs`

**Interfaces:**
- Consumes: per-camera images and passes (Task 15).
- Produces: `LightingRenderer` keeps light targets (accumulation, scene copy, lit output) per camera, keyed by camera `Entity` and sized to that camera's image, created on first use and dropped with the camera (counted by `camera_gpu_entries`). The `lighting` pass composes into the camera's own lit image, so any number of cameras can be lit. The Task 15 "draws unlit and warns" path is removed.

- [ ] **Step 1: Write the failing test** (`tests/e2e/render_pipeline.rs`)

```rust
/// Two lit halves: each camera looks at its own light over white.
struct LitSplit;

impl Scene for LitSplit {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    for x in [-1000.0, 1000.0] {
      let white = SpriteHandle::from_sprite(Sprite::solid_rect(64, 64, 255, 255, 255, 255), "white-half");
      ctx.world.spawn((Transform::from_xy(x, 0.0), Renderable::sprite(white)));
      ctx.world.spawn((Transform::from_xy(x, 0.0), Light2D::point(Color::rgb(255, 255, 255), 6.0)));
    }
    ctx.lighting().set_ambient(AmbientLight::with_intensity(0.02));
    ctx.camera().set_position(-1000.0, 0.0);
    ctx.camera().set_viewport(Viewport::new(0.0, 0.0, 0.5, 1.0));
    ctx.world.spawn((
      Transform::from_xy(1000.0, 0.0),
      Camera2d { viewport: Viewport::new(0.5, 0.0, 0.5, 1.0), order: 1, ..Default::default() },
    ));
  }
}

#[test]
fn every_camera_is_lit() {
  let Some(mut engine) = engine(LitSplit) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let frame = engine.frame().unwrap();
  for (lit_x, dark_x) in [(16, 2), (48, 34)] {
    let (lit, dark) = (frame.get_pixel(lit_x, 32).0, frame.get_pixel(dark_x, 32).0);
    assert!(lit[0] > dark[0].saturating_add(60), "x = {lit_x}: {lit:?} vs {dark:?}");
  }
}
```

(`engine(scene)` in this file builds a 64x64 resolution-independent engine; each half is 32 pixels wide with its light at the half's centre.)

Run: `cargo test -p ryo2d --test e2e -- every_camera_is_lit 2>&1 | grep -E 'test result|panicked'`
Expected: FAIL (the second camera is unlit).

- [ ] **Step 2: Implement** the Produces block.

Run the Step 1 command. Expected: PASS.

- [ ] **Step 3: Suite, pixels, commit**

Run the Task 11 Step 3 commands. Expected: `0`, no lint lines, no new tour differences.

```bash
git add -A
git commit -m "feat: lighting renders for every camera"
```

### Task 17: Render targets, texture cameras and sprites that show them

**Files:**
- Create: `src/core/graphics/render_targets.rs`
- Modify: `src/core/graphics/{texture_registry.rs,sprite_gpu.rs,renderer.rs,camera2d.rs,mod.rs}`, `src/core/ecs/renderable.rs`, `src/core/render/{entities.rs,pipeline.rs}`, `src/core/engine/{mod.rs,frame.rs,extract.rs}`, `src/core/scene/context.rs`, `src/lib.rs`
- Test: `src/core/graphics/render_targets.rs` (unit), `tests/e2e/gpu_world.rs`

**Interfaces:**
- Consumes: `RenderTarget` (Task 10), `TextureRegistry` (Task 6), per-camera rendering (Tasks 15-16).
- Produces:
  - `TextureSource::Target { width: u32, height: u32 }` in the registry: GPU-only, never evicted, never in the atlas.
  - `#[derive(Resource, Default)] pub struct RenderTargets` with `create(&mut self, width: u32, height: u32) -> RenderTarget` (sides clamped to `1..=8192`, WebGPU's default `max_texture_dimension_2d`, with one warning when clamped), `resize(&mut self, target: RenderTarget, width: u32, height: u32) -> bool`, `remove(&mut self, target: RenderTarget) -> bool`, `size(&self, target: RenderTarget) -> Option<(u32, u32)>`, `contains(&self, target) -> bool`. It records requests; the renderer creates, resizes and frees the GPU textures (`Rgba8UnormSrgb`, `RENDER_ATTACHMENT | TEXTURE_BINDING`) at the next frame. `ctx.render_targets() -> Mut<'_, RenderTargets>`.
  - `CameraTarget::Texture(target)` cameras render into that texture (their image is the target; their `CameraView` target size is `RenderTargets::size`). A camera whose target was removed draws nothing and warns once.
  - `Renderable::render_target(target: RenderTarget) -> Renderable` (anchor, tint, alpha, scale, rotation, flips as for sprites); `pub target: Option<RenderTarget>` on `Renderable` (drawn instead of `sprite` when set; its size comes from `RenderTargets`).
  - Render order puts texture cameras first, so a sprite shows this frame's image. A camera skips any drawable showing its own target, warning once per drawable entity.

- [ ] **Step 1: Write the failing unit tests** (`src/core/graphics/render_targets.rs`)

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sizes_are_clamped_to_what_a_gpu_can_hold() {
    let mut targets = RenderTargets::default();
    let tiny = targets.create(0, 0);
    let huge = targets.create(100_000, 64);
    assert_eq!(targets.size(tiny), Some((1, 1)));
    assert_eq!(targets.size(huge), Some((8192, 64)));
  }

  #[test]
  fn a_removed_target_is_gone() {
    let mut targets = RenderTargets::default();
    let target = targets.create(32, 32);
    assert!(targets.resize(target, 64, 16));
    assert_eq!(targets.size(target), Some((64, 16)));
    assert!(targets.remove(target));
    assert!(!targets.contains(target) && targets.size(target).is_none());
    assert!(!targets.remove(target));
  }
}
```

Run: `cargo test --lib -p ryo2d -- render_targets 2>&1 | grep -E 'error\[|test result|panicked'`
Expected: compile errors (`RenderTargets` missing).

- [ ] **Step 2: Write the failing pixel tests** (append to `tests/e2e/gpu_world.rs`)

```rust
#[test]
fn a_sprite_shows_what_a_texture_camera_saw_this_frame() {
  struct Mirror;
  impl Scene for Mirror {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      // Far away: a green square, seen by a camera that renders to a texture
      ctx.world.spawn((Transform::from_xy(1000.0, 1000.0), Renderable::sprite(solid("rt-green", 16, GREEN))));
      let target = ctx.render_targets().create(16, 16);
      ctx.world.spawn((
        Transform::from_xy(1000.0, 1000.0),
        Camera2d { target: CameraTarget::Texture(target), ..Default::default() },
      ));
      // Near: a sprite showing that texture
      ctx.world.spawn((Transform::from_xy(32.0, 32.0), Renderable::render_target(target)));
    }
  }
  let builder = EngineBuilder::new().resolution(64, 64).start_scene(Mirror);
  let Some(mut engine) = start(builder) else {
    return;
  };
  engine.step_frames(1).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(32, 32).0, GREEN, "on the first frame");
}

#[test]
fn a_camera_skips_a_sprite_showing_its_own_target() {
  struct Loop;
  impl Scene for Loop {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let target = ctx.render_targets().create(16, 16);
      // The texture camera looks at the very sprite that shows its texture
      ctx.world.spawn((
        Transform::from_xy(32.0, 32.0),
        Camera2d { target: CameraTarget::Texture(target), ..Default::default() },
      ));
      ctx.world.spawn((Transform::from_xy(32.0, 32.0), Renderable::render_target(target)));
    }
  }
  let builder = EngineBuilder::new().resolution(64, 64).start_scene(Loop);
  let Some(mut engine) = start(builder) else {
    return;
  };
  // Reading and writing one texture in a pass would be a validation error
  engine.step_frames(3).unwrap();
}
```

Run: `cargo test -p ryo2d --test e2e -- texture_camera_saw own_target 2>&1 | grep -E 'error\[|test result|panicked' | head`
Expected: compile errors (`render_targets`, `CameraTarget::Texture` drawing, `Renderable::render_target`).

- [ ] **Step 3: Implement** the Produces block.

Run the Step 1 and Step 2 commands. Expected: PASS; `a_camera_skips_a_sprite_showing_its_own_target` logs one warning.

- [ ] **Step 4: Suite, pixels, commit**

Run the Task 11 Step 3 commands. Expected: `0`, no lint lines, no new tour differences.

```bash
git add -A
git commit -m "feat: render targets; cameras render into them and sprites show them

- RenderTargets creates, resizes and removes GPU textures (sides clamped
  to 1..=8192)
- texture cameras render first, so a sprite shows this frame's image
- a camera skips sprites showing its own target"
```

---

## Phase 7: documentation and verification

### Task 18: Documentation, skills, the architecture overview and the `init` template

**Files:**
- Modify: `.claude/skills/{ryo-api,ryo-renderer,ryo-game-dev,ryo-engine-dev,ryo-cli,ryo-lighting,ryo-particles,ryo-config-save,ryo-ecs-internals,ryo-ui}/SKILL.md`, `src/cli/skills/*.md` (verbatim copies), `src/cli/commands/install_skills.rs` (only if a skill description changed), `src/core/{AGENTS.md,render/AGENTS.md,graphics/AGENTS.md,ecs/AGENTS.md,lighting/AGENTS.md}`, `docs/architecture/overview.md`, `src/cli/commands/init.rs` (templates), `README.md`

- [ ] **Step 1: Find every stale statement**

Run: `grep -rnE 'ctx\.camera\b|ctx\.camera\.|Camera::centered_at|CameraBounds|WindowMode|max_width|max_height|TextureManager|TextureHandle|ctx\.textures|RenderLayer\b|upload_sprite|has_texture|time_resolution|game_width|game_height|SurfaceError|calculate_viewport|world_to_screen\(.*,.*\)' .claude/skills docs/architecture src/core --include='*.md' README.md src/cli/commands/init.rs`
Expected: a list to rewrite. Every hit is either rewritten to the new API or deleted.

- [ ] **Step 2: Rewrite**

- `ryo-api`: sections for `Presentation`/`EngineBuilder::{pixel_art, window_size, resizable}`/`ScreenMapping`; cameras (`Camera2d`, `Projection`, `Viewport`, `CameraTarget`, `CameraClear`, `CameraShake`, `CameraView`, `MainCamera`, `CameraMut` via `ctx.camera()`, `RenderLayers`, `ZLayer`); render targets (`RenderTargets`, `RenderTarget`, `Renderable::render_target`); `RenderPass::scope`/`PassScope`/`RenderContext::camera`; `GpuError`; `GpuRenderContext` with handles and world-space instances; removals listed in the spec's Public API section.
- `ryo-renderer`: the per-camera frame (camera passes, frame passes, `CameraImages`, the `cameras` pass), `CameraGlobals`, `CameraUniforms`, the texture registry, `ScreenMapping`, pixel snapping, non-blocking shader checks, the web build.
- `ryo-game-dev`: cameras as entities (follow in `update`, `Interpolated`), split screen and minimap recipes, render-to-texture recipe, choosing pixel art vs resolution-independent.
- `ryo-cli`: `./dev.sh web <demo>` (and that `ryo2d` itself has no web command yet).
- `ryo-lighting`: lights render per camera and turn with it. `ryo-particles`: particles are centred and world-space. `ryo-config-save`: `pixel_art` in `Ryo.toml`; a player's stored resolution sets the window size. `ryo-ecs-internals`: `MainCamera` survives world cleanup; camera systems in `PostUpdateSet::Camera`. `ryo-ui`: UI is in screen space.
- `docs/architecture/overview.md`: the frame diagram becomes the per-camera frame (gather once, cameras in order, frame passes); the resolution model and the web build get a paragraph each.
- `src/cli/commands/init.rs`: the generated `main.rs` uses `.resolution(1280, 720)` (resolution-independent) and a comment pointing at `.pixel_art(w, h)`; it uses `ctx.camera()` where it touched the camera.
- Copy every changed skill verbatim to `src/cli/skills/<name>.md`; regenerate `EMBEDDED_SKILLS` descriptions only if a frontmatter description changed.

- [ ] **Step 3: Check, commit**

Run: `for d in .claude/skills/*/; do n=$(basename $d); cmp -s $d/SKILL.md src/cli/skills/$n.md || echo "copy differs: $n"; done; cargo test -p ryo2d --lib -- cli 2>&1 | grep 'test result'`
Expected: no `copy differs`; tests pass. Build the `init` template in a scratch directory as the foundation did: `cargo run -q -- init /tmp/ryo2d-init-check && (cd /tmp/ryo2d-init-check && cargo check -q)` prints nothing.

```bash
git add -A
git commit -m "docs: cameras, render targets, the resolution model and the web build"
```

### Task 19: Full verification, review and merge

**Files:** none new.

- [ ] **Step 1: Everything green**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{p+=$4; f+=$6} END {print p, f}' && ./dev.sh lint 2>&1 | grep -E '^(warning|error)' | grep -v 'future version' | head -3`
Expected: passed count at least the Task 0 count plus the new tests, `0` failed; no lint lines (the wasm clippy included).

- [ ] **Step 2: Demos and games against the baseline**

Run: `./dev.sh demos:tour >/dev/null && target/renderer-baseline/compare.sh`
Expected: only pages already explained in the ledger (y-sorting size, 1 px rounding, particle centring, shake timing). Open each one beside its baseline once more.

Run the Task 0 Step 3 game loop into `target/renderer-final/games/` and compare each `frame-000060.png` by eye with the baseline.
Expected: same scene; differences only of the ledgered kinds.

- [ ] **Step 3: The web demo again**

Run: `./dev.sh web lighting` and check Chrome, Safari and Firefox as in Task 5.
Expected: as in Task 5.

- [ ] **Step 4: Benchmark**

Run: `cargo bench --bench foundation -- "headless frame" 2>&1 | grep -A1 '^headless frame' | grep time`
Expected: within about 20% of the Task 0 baseline (one full-window camera: instances move to world space and the CPU transform goes, so it should not be slower). A larger regression is investigated before merge.

- [ ] **Step 5: Whole-branch review**

Per `superpowers:executing-plans` (or `subagent-driven-development`): build the review package from `git merge-base main HEAD`, dispatch the reviewer on the most capable model with the spec, this plan, the Review Focus section verbatim and the ledger's `Ruling:` lines. Re-grade findings by effect; one fix pass for Critical and Important, each test-first; minors to the ledger.

- [ ] **Step 6: Finish**

Use `superpowers:finishing-a-development-branch`. Merging into `main` is the owner's call. After merge, delete this plan (`docs/plans/2026-10-12-renderer-core-plan.md`) per the project's rule; the spec stays.

---

## Self-review notes

- Spec coverage: GPU foundation (Tasks 1-3), web build (4-5), texture registry (6-7), resolution model (8-9), cameras (10-11), draw paths (12-14), per-camera rendering and render targets (15-17), public API changes (7, 8, 10, 11, 17), migration order (phases follow the spec's seven steps; demos opt into pixel art in Task 8 so Task 9's renderer change is checked against identical captures), testing (each task), documentation (18).
- Plan rulings against the spec, for the owner: parallax follows the main camera only (Task 13; per-camera parallax needs a component, 2b); a player's stored resolution now sets the window size, not the game resolution (Task 8; the spec's resolution model left it unstated); custom shaders apply one frame after first use, because the GPU check is no longer waited on (Task 3).
- Review Focus items are pinned: minimised window (Task 9 `a_minimised_window_skips_frames_and_comes_back`), degenerate camera settings (Task 10 `degenerate_settings_stay_finite`), render-target clamping (Task 17 `sizes_are_clamped_to_what_a_gpu_can_hold`), camera GPU state (Task 15 `despawned_cameras_free_their_gpu_state`), small pixel-art windows (Task 8 `a_window_smaller_than_the_art_scales_at_one_and_crops`).
