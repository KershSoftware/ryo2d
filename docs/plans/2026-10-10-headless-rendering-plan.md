# Headless Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let tests run real scenes through the engine's full update/render pipeline without a window — stepped deterministically, with simulated key/mouse input, frames read back as pixels — via a public `ryo2d::testing::HeadlessEngine`.

**Architecture:** `WgpuRenderer` gets a `PresentTarget` (window surface or offscreen texture) behind a shared constructor body. The engine's window-only setup, its duplicated present code and its per-frame logic are pulled into `init_gpu`, a single `present_frame` and a shared `step_frame`, so the window handler and `HeadlessEngine` run the same code. `TimeState::advance(dt)` makes steps deterministic.

**Tech Stack:** Rust 2024, wgpu 22, winit 0.30, hecs, image 0.25, thiserror, pollster.

**Spec:** `docs/plans/2026-10-10-headless-rendering-design.md`

## Global Constraints

- 2-space indentation; edition 2024. Format only the files you touch: `rustfmt --edition 2024 <file>` — never on `src/core/mod.rs` or `src/lib.rs` (rustfmt recurses into every submodule and reformats unrelated drift). Check `git diff --stat` after formatting; if unrelated files changed, `git checkout -- <them>`.
- Some files use CRLF line endings (e.g. `tests/e2e.rs`, skill files). Preserve them: edit those with the Edit tool or byte-preserving scripts, never by rewriting in text mode.
- No `unwrap`/`expect` in library code (tests may use them). The windowed `WgpuRenderer::new` keeps its existing `expect`s — out of scope.
- Public API path exactly `ryo2d::testing::{HeadlessEngine, HeadlessError, StepOutcome}`; method names exactly as in the spec (`new`, `new_or_skip`, `step`, `step_frames`, `frame_count`, `frame`, `world`, `world_mut`, `press_key`, `release_key`, `tap_key`, `press_mouse`, `release_mouse`, `mouse_move`).
- Headless mode: no audio manager, no gamepads, no hot reload, no dev windows, never reads/writes `system.json`, ignores `RYO_CAPTURE_*`, forces FPS / frame-timing / stats overlays off, honours `Ryo.toml`.
- Windowed behaviour must not change (refactor tasks are verified with before/after frame captures).
- `cargo clippy --workspace --all-targets` must add no new warnings in touched code (pre-existing warnings exist — compare file paths).
- Commits: `type: description` (+ bullets), no co-author lines, no mention of AGENTS.md. The repo owner authorises commits per task for this plan.

## Review Focus

1. A scene whose `audio` calls assume an audio manager exists (`ctx.audio.as_mut().unwrap()`) panics headless — reasonable users expect the harness docs to say audio is absent → Task 10 docs state it explicitly in the `HeadlessEngine` rustdoc ("`ctx.audio` is `None`").
2. Stepping after the scene quit must not run more frames or panic → Task 8 test `step_after_quit_stays_quit`.
3. `frame()` before any step must return an error, not stale/undefined pixels → Task 8 test `frame_before_first_step_is_an_error`.
4. Two `HeadlessEngine`s alive at once (or created in parallel tests) must share one GPU device without deadlock → Task 8 test `two_engines_share_the_gpu` (also the determinism test creates two).
5. A key held with `press_key` and later released must produce exactly one "just pressed" and one "just released" — not repeated each step → Task 9 tests `press_key_is_just_pressed_once` and `release_key_is_just_released_once`.

---

## File Map

| File | Change | Responsibility |
|------|--------|----------------|
| `src/core/timing.rs` | Modify | `advance(dt)`, `fixed_timestep()` |
| `src/core/graphics/renderer.rs` | Modify | `PresentTarget`, `RendererParts`/`from_parts`, `new_offscreen`, `read_frame`, `Option` instance/adapter |
| `src/core/engine.rs` | Modify | `init_gpu`, `present_frame`, `step_frame`, headless build mode, deferred user preferences, crate accessors |
| `src/core/error.rs` | Modify | `HeadlessError` |
| `src/core/testing/mod.rs` | Create | `HeadlessEngine`, `StepOutcome`, re-exports |
| `src/core/testing/gpu.rs` | Create | Process-wide shared device `shared_gpu()` |
| `src/core/testing/AGENTS.md` | Create | Agent notes |
| `src/core/mod.rs`, `src/lib.rs` | Modify | Module wiring (`pub mod testing;`, `pub use core::testing;`) — edit by hand, do not rustfmt |
| `tests/common/gpu.rs` | Modify | `TestGpu` reuses `shared_gpu()` |
| `tests/e2e/offscreen_renderer.rs`, `tests/e2e/headless_engine.rs` | Create | GPU e2e tests |
| `tests/e2e/engine_lifecycle.rs` | Rewrite | Replace empty `#[ignore]` stubs with real headless tests |
| `tests/e2e.rs` | Modify | Register new modules (CRLF) |
| `src/cli/skills/ryo-game-dev.md`, `.claude/skills/ryo-game-dev/SKILL.md`, `src/cli/skills/ryo-engine-dev.md`, `.claude/skills/ryo-engine-dev/SKILL.md` | Modify | Docs (CRLF) |

---

### Task 1: Deterministic time seam

**Files:** Modify `src/core/timing.rs`.

**Interfaces:**
- Produces: `TimeState::advance(&mut self, dt: Duration)`, `TimeState::fixed_timestep(&self) -> Duration`.

- [ ] **Step 1: Write the failing tests**

Add at the end of `src/core/timing.rs` (or inside the existing `#[cfg(test)] mod tests` if one exists — check with `grep -n "mod tests" src/core/timing.rs`):

```rust
#[cfg(test)]
mod advance_tests {
  use super::*;

  #[test]
  fn one_fixed_step_runs_exactly_one_update() {
    let mut time = TimeState::new(60, 60);
    let step = time.fixed_timestep();

    time.advance(step);

    assert!(time.should_update());
    time.consume_update();
    assert!(!time.should_update());
    assert_eq!(time.interpolation_alpha(), 0.0);
  }

  #[test]
  fn partial_steps_accumulate() {
    let mut time = TimeState::new(60, 60);
    let half = time.fixed_timestep() / 2;

    time.advance(half);
    assert!(!time.should_update());
    time.advance(half);
    assert!(time.should_update());
  }

  #[test]
  fn accumulator_is_capped_at_five_steps() {
    let mut time = TimeState::new(60, 60);
    let step = time.fixed_timestep();

    time.advance(step * 100);

    let mut updates = 0;
    while time.should_update() {
      time.consume_update();
      updates += 1;
    }
    assert_eq!(updates, 5);
  }

  #[test]
  fn advance_counts_frames() {
    let mut time = TimeState::new(60, 60);
    time.advance(Duration::ZERO);
    time.advance(Duration::ZERO);
    assert_eq!(time.frame_count(), 2);
  }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib core::timing`
Expected: compile errors `no method named advance` / `fixed_timestep`.

- [ ] **Step 3: Implement**

Replace `begin_frame` with:

```rust
  /// Begin a new frame using wall-clock time since the previous frame.
  pub fn begin_frame(&mut self) {
    let now = Instant::now();
    let dt = now.duration_since(self.frame_start);
    self.frame_start = now;
    self.advance(dt);
  }

  /// Begin a new frame that lasted `dt`.
  ///
  /// Headless runs pass exactly [`fixed_timestep`](Self::fixed_timestep) so
  /// each frame runs one fixed update, independent of the wall clock.
  pub fn advance(&mut self, dt: Duration) {
    self.delta_time = dt;

    // Add delta to accumulator, but cap it to prevent spiral of death
    // If we fall too far behind, we just drop updates rather than freezing
    self.accumulator += dt;
    let max_accumulator = self.fixed_timestep * 5; // Max 5 physics updates per frame
    if self.accumulator > max_accumulator {
      self.accumulator = max_accumulator;
    }

    self.frame_count += 1;
    // Update dev_log frame number for log entries
    set_frame(self.frame_count);
  }

  /// The fixed timestep used for logic and physics updates.
  pub fn fixed_timestep(&self) -> Duration {
    self.fixed_timestep
  }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib core::timing` → all pass.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/core/timing.rs
git add src/core/timing.rs
git commit -m "feat(timing): add TimeState::advance for deterministic stepping"
```

---

### Task 2: Renderer present target (window behaviour unchanged)

Behaviour-preserving refactor. No automated test can cover the window surface; verification is the full suite plus before/after frame captures.

**Files:** Modify `src/core/graphics/renderer.rs`, `src/core/engine.rs` (4 dev-window call sites).

**Interfaces:**
- Produces (private): `enum PresentTarget { Surface { surface, config }, Offscreen { texture } }` with `acquire()` and `format()`; `struct RendererParts`; `fn from_parts(parts: RendererParts) -> Self`.
- Changes (public): `WgpuRenderer::instance(&self) -> Option<&wgpu::Instance>`, `WgpuRenderer::adapter(&self) -> Option<&wgpu::Adapter>`.

- [ ] **Step 1: Capture the windowed baseline (before any edit)**

The acceptance runs need real windows; each opens briefly. For each demo in `lighting ui raycasting particles`:

```bash
for demo in lighting ui raycasting particles; do
  (cd examples/demos/$demo && RYO_CAPTURE_FRAMES=120 RYO_CAPTURE_EXIT=1 \
    RYO_CAPTURE_DIR="$PWD/../../../target/capture-baseline/$demo" \
    perl -e 'alarm shift; exec @ARGV' 300 \
    cargo run --manifest-path ../../../Cargo.toml --bin ryo2d -- dev --skip-opt-check) \
    > target/capture-baseline-$demo.log 2>&1; echo "$demo exit=$?"
done
ls target/capture-baseline/*/frame-000120.png
```

Expected: four PNGs, each demo `exit=0`. (These baselines are reused by Tasks 4, 5 and 6. `target/` is git-ignored.)

- [ ] **Step 2: Add `PresentTarget`**

Add above `pub struct WgpuRenderer`:

```rust
/// Where finished frames go: the window surface or an offscreen texture.
enum PresentTarget {
  Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
  },
  Offscreen {
    texture: wgpu::Texture,
  },
}

impl PresentTarget {
  /// The view to draw this frame into, plus the surface texture to present afterwards.
  fn acquire(
    &self,
  ) -> Result<(Option<wgpu::SurfaceTexture>, wgpu::TextureView), wgpu::SurfaceError> {
    match self {
      PresentTarget::Surface { surface, .. } => {
        let output = surface.get_current_texture()?;
        let view = output
          .texture
          .create_view(&wgpu::TextureViewDescriptor::default());
        Ok((Some(output), view))
      }
      PresentTarget::Offscreen { texture } => Ok((
        None,
        texture.create_view(&wgpu::TextureViewDescriptor::default()),
      )),
    }
  }

  fn format(&self) -> wgpu::TextureFormat {
    match self {
      PresentTarget::Surface { config, .. } => config.format,
      PresentTarget::Offscreen { texture } => texture.format(),
    }
  }
}

/// Construction inputs that differ between window and offscreen renderers.
struct RendererParts {
  instance: Option<wgpu::Instance>,
  adapter: Option<wgpu::Adapter>,
  device: Arc<wgpu::Device>,
  queue: Arc<wgpu::Queue>,
  target: PresentTarget,
  window_size: (u32, u32),
  presentation_blocks: bool,
  game_width: u32,
  game_height: u32,
  window_mode: WindowMode,
  max_width: Option<u32>,
  max_height: Option<u32>,
}
```

- [ ] **Step 3: Change the struct fields**

In `pub struct WgpuRenderer`, replace the four fields `instance: wgpu::Instance,`, `adapter: wgpu::Adapter,`, `surface: wgpu::Surface<'static>,` and `config: wgpu::SurfaceConfiguration,` with:

```rust
  // Only set for window renderers (dev windows create extra surfaces from them)
  instance: Option<wgpu::Instance>,
  adapter: Option<wgpu::Adapter>,
  target: PresentTarget,
```

- [ ] **Step 4: Split `new` into `new` + `from_parts`**

In `pub async fn new(...)`:
1. Keep everything up to and including `surface.configure(&device, &config);` unchanged.
2. Cut everything after it — from the `// Create the game texture` comment through the end of the function (the `WgpuRenderer { ... }` literal, `renderer.update_viewport_transform();` and `renderer`) — and replace it with:

```rust
    Self::from_parts(RendererParts {
      instance: Some(instance),
      adapter: Some(adapter),
      device,
      queue,
      target: PresentTarget::Surface { surface, config },
      window_size: (size.width, size.height),
      presentation_blocks: present_mode == wgpu::PresentMode::Fifo,
      game_width,
      game_height,
      window_mode,
      max_width,
      max_height,
    })
  }
```

3. Paste the cut code into a new method directly below `new`:

```rust
  /// Build the textures, pipelines and buffers shared by every target type.
  fn from_parts(parts: RendererParts) -> Self {
    let RendererParts {
      instance,
      adapter,
      device,
      queue,
      target,
      window_size,
      presentation_blocks,
      game_width,
      game_height,
      window_mode,
      max_width,
      max_height,
    } = parts;
    let target_format = target.format();

    // <the cut code goes here>
  }
```

and edit the pasted code:
- Replace both `format: surface_format,` (render pipeline and blend pipeline targets) with `format: target_format,`.
- Delete the line `let presentation_blocks = present_mode == wgpu::PresentMode::Fifo;` and its comment (the value now comes from `parts`).
- In the `WgpuRenderer { ... }` literal: replace `instance,` `adapter,` `surface,` with `instance,` `adapter,` `target,`; delete `config,`; replace `window_width: size.width,` / `window_height: size.height,` with `window_width: window_size.0,` / `window_height: window_size.1,`.

- [ ] **Step 5: Route the three render paths through the target**

In `render_with_gpu_sprites`, `render_with_lighting` and `render_raycast`, replace

```rust
    let output = self.surface.get_current_texture()?;
    let view = output
      .texture
      .create_view(&wgpu::TextureViewDescriptor::default());
```

with

```rust
    let (output, view) = self.target.acquire()?;
```

and replace each `    output.present();` with

```rust
    if let Some(output) = output {
      output.present();
    }
```

- [ ] **Step 6: Accessors and resize**

Replace `adapter()`, `instance()`, `surface_format()` and `resize()` with:

```rust
  /// Get a reference to the wgpu adapter (window renderers only).
  ///
  /// Used for creating additional surfaces (e.g., profiler window).
  pub fn adapter(&self) -> Option<&wgpu::Adapter> {
    self.adapter.as_ref()
  }

  /// Get a reference to the wgpu instance (window renderers only).
  ///
  /// Used for creating additional surfaces (e.g., profiler window).
  pub fn instance(&self) -> Option<&wgpu::Instance> {
    self.instance.as_ref()
  }
```

```rust
  /// Get the format frames are presented in (surface or offscreen texture).
  pub fn surface_format(&self) -> wgpu::TextureFormat {
    self.target.format()
  }
```

```rust
  /// Resize the surface when window size changes (no-op for offscreen targets).
  pub fn resize(&mut self, new_width: u32, new_height: u32) {
    if new_width == 0 || new_height == 0 {
      return;
    }
    let PresentTarget::Surface { surface, config } = &mut self.target else {
      // Offscreen targets always match the game resolution
      return;
    };
    config.width = new_width;
    config.height = new_height;
    surface.configure(&self.device, config);
    self.window_width = new_width;
    self.window_height = new_height;
    self.update_viewport_transform();
  }
```

In `prepare_capture`, replace `format: self.config.format,` with `format: self.target.format(),`.

Check: `grep -n "self.surface\|self.config" src/core/graphics/renderer.rs` → no output.

- [ ] **Step 7: Update the four dev-window call sites in `engine.rs`**

`grep -n "renderer.instance()" src/core/engine.rs` lists four `ProfilerWindow::new` / `LoggerWindow::new` calls, each inside an `if ... && let Some(ref renderer) = self.renderer` let-chain. For each, add a clause to the chain and pass the bound values:

```rust
      && let Some(ref renderer) = self.renderer
      && let (Some(instance), Some(adapter)) = (renderer.instance(), renderer.adapter())
    {
      // ...
        renderer.device_arc(),
        renderer.queue_arc(),
        instance,
        adapter,
```

- [ ] **Step 8: Verify**

Run: `cargo check --workspace` → no errors.
Run: `cargo clippy --workspace --all-targets 2>&1 | grep -E "^\s+--> src/core/(graphics/renderer|engine).rs"` → no new hits.
Run: `cargo test --workspace` → 0 failed.
Re-run Step 1's loop with `capture-check/task2` in place of `capture-baseline`, then compare each `target/capture-check/task2/<demo>/frame-000120.png` with its baseline by viewing both (agents: Read tool). Expected: same scene content (FPS digits and exact animation phase may differ).

- [ ] **Step 9: Format, commit**

```bash
rustfmt --edition 2024 src/core/graphics/renderer.rs src/core/engine.rs
git add src/core/graphics/renderer.rs src/core/engine.rs
git commit -m "refactor(renderer): draw frames through a present target"
```

---

### Task 3: Offscreen renderer

**Files:** Modify `src/core/graphics/renderer.rs`; create `tests/e2e/offscreen_renderer.rs`; modify `tests/e2e.rs`.

**Interfaces:**
- Consumes: `PresentTarget`, `RendererParts`, `from_parts` (Task 2); `read_texture_rgba` (frame capture).
- Produces: `WgpuRenderer::new_offscreen(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>, width: u32, height: u32) -> Self`; `WgpuRenderer::read_frame(&self) -> CaptureResult<RgbaImage>`.

- [ ] **Step 1: Write the failing e2e test**

Create `tests/e2e/offscreen_renderer.rs`:

```rust
//! E2E tests for the offscreen WgpuRenderer target.

use std::sync::Arc;

use crate::common::gpu::TestGpu;
use ryo2d::core::graphics::WgpuRenderer;

fn within_one(a: u8, b: u8) -> bool {
  a.abs_diff(b) <= 1
}

#[test]
fn offscreen_frame_matches_cpu_pixels() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping offscreen_frame_matches_cpu_pixels: no GPU available");
    return;
  };
  let (width, height) = (37, 9);
  let mut renderer =
    WgpuRenderer::new_offscreen(Arc::clone(&gpu.device), Arc::clone(&gpu.queue), width, height);

  // Opaque pattern: each pixel encodes its position
  let pixels = renderer.frame_mut();
  for (i, px) in pixels.chunks_exact_mut(4).enumerate() {
    let (x, y) = ((i as u32 % width) as u8, (i as u32 / width) as u8);
    px.copy_from_slice(&[x * 6, y * 20, 128, 255]);
  }
  let expected = renderer.frame_mut().to_vec();

  renderer.render().unwrap();
  let frame = renderer.read_frame().unwrap();

  assert_eq!(frame.dimensions(), (width, height));
  for (got, want) in frame.as_raw().chunks_exact(4).zip(expected.chunks_exact(4)) {
    assert!(
      got.iter().zip(want).all(|(g, w)| within_one(*g, *w)),
      "got {got:?}, want {want:?}"
    );
  }
}
```

Register in `tests/e2e.rs` (CRLF file — use the Edit tool) after the `capture_readback` module:

```rust
#[path = "e2e/offscreen_renderer.rs"]
mod offscreen_renderer;
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test e2e offscreen_renderer`
Expected: compile error `no function or associated item named new_offscreen` (and `read_frame`).

- [ ] **Step 3: Implement**

Add imports at the top of `renderer.rs` if missing: `use image::RgbaImage;`.

Add to `impl WgpuRenderer` (after `new`):

```rust
  /// Create a renderer that draws into an offscreen texture instead of a window.
  ///
  /// The target is `width × height` (the game resolution), so frames are never
  /// letterboxed. Read them back with [`read_frame`](Self::read_frame).
  pub fn new_offscreen(
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    width: u32,
    height: u32,
  ) -> Self {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Offscreen Target"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba8UnormSrgb,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
      view_formats: &[],
    });
    Self::from_parts(RendererParts {
      instance: None,
      adapter: None,
      device,
      queue,
      target: PresentTarget::Offscreen { texture },
      window_size: (width, height),
      presentation_blocks: false,
      game_width: width,
      game_height: height,
      window_mode: WindowMode::default(),
      max_width: None,
      max_height: None,
    })
  }

  /// Read the last frame drawn to an offscreen target.
  ///
  /// Window renderers can't read their surface back; use
  /// [`request_capture`](Self::request_capture) instead.
  pub fn read_frame(&self) -> CaptureResult<RgbaImage> {
    match &self.target {
      PresentTarget::Offscreen { texture } => read_texture_rgba(&self.device, &self.queue, texture),
      PresentTarget::Surface { .. } => Err(CaptureError::Readback(
        "window surfaces can't be read back; use request_capture".to_string(),
      )),
    }
  }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test e2e offscreen_renderer -- --nocapture` → passes, with no "Skipping" line on a machine with a GPU.
Run: `cargo test --workspace` → 0 failed.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/core/graphics/renderer.rs tests/e2e/offscreen_renderer.rs
git add src/core/graphics/renderer.rs tests/e2e/offscreen_renderer.rs tests/e2e.rs
git commit -m "feat(renderer): add offscreen render target with frame read-back"
```

---

### Task 4: `init_gpu` (window behaviour unchanged)

**Files:** Modify `src/core/engine.rs` (`resumed`, ~line 2401).

**Interfaces:**
- Produces: `pub(crate) fn init_gpu(&mut self, renderer: WgpuRenderer, scale_factor: f64)` on `Engine`.

- [ ] **Step 1: Extract**

Add to `impl Engine` (next to `fn on_frame_presented`):

```rust
  /// Finish GPU-dependent setup once a renderer exists (window or offscreen).
  ///
  /// Creates the sprite, lighting, trail and raycast renderers, the frame
  /// clock and the profiler, then enters the initial scene.
  pub(crate) fn init_gpu(&mut self, renderer: WgpuRenderer, scale_factor: f64) {
    // <moved code, see below>
  }
```

Move into it, in this order, the following blocks from `resumed`:
1. The four renderer constructions (`let sprite_gpu = SpriteGpuRenderer::new(...)` through `let raycast_renderer = GpuRaycastRenderer::new(...)`), unchanged.
2. `self.scale_factor = scale_factor;` (replacing `self.scale_factor = window.scale_factor();`) and the `self.ui_scale = ...` block, unchanged.
3. `self.renderer = Some(renderer);` `self.sprite_gpu = ...;` `self.lighting = ...;` `self.trail_renderer = ...;` `self.raycast_renderer = ...;` and the `self.time = Some(TimeState::new(...));` statement (but **not** `self.window = Some(window);`).
4. `super::profiler::init(self.config.debug.enable_profiler);` with its comment.
5. The whole `// Call on_enter for the initial scene` block.
6. The two `engine_info!` lines at the end of `resumed`.

`resumed` becomes, after the `let mut renderer = pollster::block_on(WgpuRenderer::new(...));` statement:

```rust
    // Present the first frame immediately to prevent white flash on startup
    // The renderer's pixel buffer is pre-filled with warm cream background
    let _ = renderer.render();

    // Now that the first frame is rendered, show the window
    window.set_visible(true);

    let scale_factor = window.scale_factor();
    self.window = Some(window);
    self.init_gpu(renderer, scale_factor);
```

followed by the unchanged profiler-window and logger-window blocks (`RYO_PROFILER_AUTO_OPEN` / `logger_window`). Those dev windows are now created after the initial scene's `on_enter` instead of before it; nothing in `on_enter` reads them.

- [ ] **Step 2: Verify**

Run: `cargo check --workspace`, `cargo clippy ...` (engine.rs hits), `cargo test --workspace` → clean / 0 failed.
Re-run Task 2 Step 1's loop into `target/capture-check/task4/` and compare with the baselines (same content expected).

- [ ] **Step 3: Format, commit**

```bash
rustfmt --edition 2024 src/core/engine.rs
git add src/core/engine.rs
git commit -m "refactor(engine): split GPU setup out of resumed into init_gpu"
```

---

### Task 5: One present path (window behaviour unchanged)

**Files:** Modify `src/core/engine.rs`.

**Interfaces:**
- Produces: `pub(crate) enum PresentOutcome { Presented, Skipped, Fatal(String) }`; `fn present_frame(&mut self) -> PresentOutcome` (replaces `do_full_render_and_present`).

- [ ] **Step 1: Turn `do_full_render_and_present` into `present_frame`**

Add above `impl Engine`:

```rust
/// Result of rendering and presenting one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PresentOutcome {
  /// The frame reached the screen (or offscreen target)
  Presented,
  /// Nothing was presented (surface outdated/lost, or no renderer yet)
  Skipped,
  /// Rendering failed in a way the window loop treats as fatal
  Fatal(String),
}
```

In `fn do_full_render_and_present(&mut self)`:
1. Rename it `fn present_frame(&mut self) -> PresentOutcome` and give it the doc comment `/// Render and present one frame — the single present path for window and headless runs.`
2. Inside the `// Render phase` block, right after `profile_scope!("cpu:render");`, add `let render_start = std::time::Instant::now();`; at the end of that block (before `} // render scope ends`), add `self.frame_timings.render = render_start.elapsed();`.
3. After `profile_scope!("gpu:present");` add `let present_start = std::time::Instant::now();`.
4. Replace `let mut presented = false;` with `let mut outcome = PresentOutcome::Skipped;`.
5. In `match result {`: `Ok(_) => presented = true,` → `Ok(_) => outcome = PresentOutcome::Presented,`; in the final `Err(e) => { ... }` arm, after the `engine_error!` line, add `outcome = PresentOutcome::Fatal(format!("{e:?}"));`.
6. Replace the tail
```rust
    if presented {
      self.on_frame_presented();
    }
  }
```
with
```rust
    self.frame_timings.present = present_start.elapsed();

    if outcome == PresentOutcome::Presented {
      self.on_frame_presented();
    }
    outcome
  }
```

Update the two `tick_frame` call sites (`grep -n "do_full_render_and_present()" src/core/engine.rs`) to `let _ = self.present_frame();` (unchanged behaviour: `tick_frame` only logged render errors).

- [ ] **Step 2: Delete the inline copy**

In the `WindowEvent::RedrawRequested` handler of `window_event`, delete everything from the comment `// Render with interpolation alpha` through the end of the block

```rust
        // Track first frame for potential future use
        if !self.first_frame_rendered {
          self.first_frame_rendered = true;
        }
```

and put in its place:

```rust
        match self.present_frame() {
          PresentOutcome::Fatal(_) => {
            event_loop.exit();
            return;
          }
          PresentOutcome::Presented | PresentOutcome::Skipped => {}
        }
```

(The deleted block is the duplicate of `present_frame`'s body: render scope, debug-font upload, `render_gpu`, stats, renderer take/put-back, callbacks, `match result`, the capture hooks and the present timing. Its `Err(e) => { ...; event_loop.exit(); return; }` arm is what the new `Fatal` arm preserves.)

- [ ] **Step 3: Verify**

Run: `grep -c "render_with_lighting(" src/core/engine.rs` → `1`; `grep -c "fn present_frame\|present_frame()" src/core/engine.rs` → `4` (definition + 2 in `tick_frame` + 1 in the handler).
Run: `cargo check --workspace`, clippy (engine.rs), `cargo test --workspace` → clean / 0 failed.
Re-run the capture loop into `target/capture-check/task5/` and compare with the baselines.

- [ ] **Step 4: Format, commit**

```bash
rustfmt --edition 2024 src/core/engine.rs
git add src/core/engine.rs
git commit -m "refactor(engine): use a single present path for every frame"
```

---

### Task 6: Shared frame step (window behaviour unchanged)

**Files:** Modify `src/core/engine.rs`.

**Interfaces:**
- Consumes: `TimeState::advance` (Task 1), `present_frame`/`PresentOutcome` (Task 5).
- Produces: `pub(crate) enum FrameOutcome { Presented, Skipped, Quit, Fatal(String) }`; `pub(crate) fn step_frame(&mut self, dt: Option<std::time::Duration>) -> FrameOutcome` (`None` = wall clock).

- [ ] **Step 1: Add the outcome type and move the frame body**

Add next to `PresentOutcome`:

```rust
/// Result of one engine frame (logic + render).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FrameOutcome {
  Presented,
  Skipped,
  /// The scene or console asked to quit; nothing was rendered
  Quit,
  /// Rendering failed fatally
  Fatal(String),
}
```

Add to `impl Engine`:

```rust
  /// Run one frame of game logic and rendering.
  ///
  /// Shared by the window loop (`dt = None`, wall-clock time) and headless runs
  /// (`dt = Some(fixed_timestep)`, exactly one fixed update per frame).
  pub(crate) fn step_frame(&mut self, dt: Option<std::time::Duration>) -> FrameOutcome {
    // <moved code, see below>
  }
```

Move into `step_frame`, from the `WindowEvent::RedrawRequested` handler, everything from `// Reset frame timings` through the quit check, with these edits:
- Delete `let frame_start = std::time::Instant::now();` (it stays in the handler — see Step 2).
- Replace
```rust
        if let Some(time) = &mut self.time {
          time.begin_frame();
        }
```
with
```rust
    if let Some(time) = &mut self.time {
      match dt {
        Some(dt) => time.advance(dt),
        None => time.begin_frame(),
      }
    }
```
- Replace the quit check body `event_loop.exit(); return;` with `return FrameOutcome::Quit;`.

Then, still in `step_frame`, append:

```rust
    let outcome = match self.present_frame() {
      PresentOutcome::Fatal(error) => return FrameOutcome::Fatal(error),
      PresentOutcome::Presented => FrameOutcome::Presented,
      PresentOutcome::Skipped => FrameOutcome::Skipped,
    };

    // Clear key repeat state every frame so held-key actions (backspace,
    // arrows) don't fire multiple times across consecutive no-update frames
    // when render_ui processes them on each frame.
    self.input.post_render_cleanup();

    // Clear all per-frame input state at the END of the frame, but ONLY if
    // we ran at least one update. This preserves input events for the next
    // frame when the fixed timestep accumulator isn't ready yet.
    if did_update {
      self.input.begin_frame();
    }

    outcome
```

and delete those two input-cleanup statements (with their comments) from the end of the `RedrawRequested` handler.

- [ ] **Step 2: The handler wraps `step_frame`**

The `WindowEvent::RedrawRequested` arm becomes: the unchanged profiler `new_frame` lines and surface-reconfigure block, then

```rust
        let frame_start = std::time::Instant::now();
        match self.step_frame(None) {
          FrameOutcome::Quit | FrameOutcome::Fatal(_) => {
            event_loop.exit();
            return;
          }
          FrameOutcome::Presented | FrameOutcome::Skipped => {}
        }
```

followed by the unchanged tail: profiler/logger window rendering, `self.frame_timings.total = frame_start.elapsed();`, the slow-frame warning, and the debug-output/frame-limiting block. (Input cleanup now happens inside `step_frame`, before frame limiting instead of after; nothing between them reads game input.)

- [ ] **Step 3: Verify**

Run: `cargo check --workspace`, clippy (engine.rs), `cargo test --workspace` → clean / 0 failed.
Run: `grep -n "time.begin_frame()" src/core/engine.rs` → only inside `step_frame` and `tick_frame`.
Re-run the capture loop into `target/capture-check/task6/` and compare with the baselines. Additionally run `./run.sh ui` (or the capture loop on `ui` with `RYO_CAPTURE_FRAMES=240`) to confirm keyboard-driven UI still responds — the windowed `ui` demo's frame should match the baseline.

- [ ] **Step 4: Format, commit**

```bash
rustfmt --edition 2024 src/core/engine.rs
git add src/core/engine.rs
git commit -m "refactor(engine): extract the per-frame step from the window handler"
```

---

### Task 7: Headless build mode and deferred user preferences

**Files:** Modify `src/core/engine.rs` (`EngineBuilder`, `Engine::new`).

**Interfaces:**
- Produces: `EngineBuilder::build_headless(self) -> Engine` (`pub(crate)`); `Engine::new(..., headless: bool)`; `with_user_preferences` now stores the filter and applies it in `build()`; crate accessors on `Engine`: `fixed_timestep() -> Duration`, `game_size() -> (u32, u32)`, `world() -> &World`, `world_mut() -> &mut World`, `input_mut() -> &mut InputState`, `read_frame() -> CaptureResult<RgbaImage>`.

- [ ] **Step 1: Write the failing unit tests**

In `engine.rs`'s `#[cfg(test)] mod tests` (it already contains `engine_builder_vsync_user_pref_blocks_builder`), add:

```rust
  struct EmptyScene;

  impl Scene for EmptyScene {
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }
  }

  #[test]
  fn user_preferences_are_deferred_until_build() {
    let builder = EngineBuilder::new()
      .with_user_preferences(crate::core::config::UserPreferenceFilter::all());
    assert!(builder.user_preferences.is_some());
    assert!(!builder.resolution_user_pref);
  }

  #[test]
  fn headless_build_has_no_audio_gamepads_or_wall_clock_overlays() {
    let engine = EngineBuilder::new()
      .show_fps(true)
      .start_scene(EmptyScene)
      .build_headless();

    assert!(engine.audio.is_none());
    assert!(engine.gilrs.is_none());
    assert!(!engine.config.debug.show_fps);
    assert!(!engine.config.debug.show_frame_timing);
    assert!(!engine.config.debug.show_stats);
  }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib core::engine::tests`
Expected: compile errors (`user_preferences` field, `build_headless`).

- [ ] **Step 3: Defer user preferences**

1. Add a field to `pub struct EngineBuilder`: `user_preferences: Option<super::config::UserPreferenceFilter>,` and `user_preferences: None,` in `EngineBuilder::new`.
2. Rename the existing `pub fn with_user_preferences(mut self, filter: impl Into<...>) -> Self` body into a private method `fn apply_user_preferences(mut self, filter: super::config::UserPreferenceFilter) -> Self` (drop its `let filter = filter.into();` line). Keep the public method's doc comment on the public method, which becomes:

```rust
  pub fn with_user_preferences(
    mut self,
    filter: impl Into<super::config::UserPreferenceFilter>,
  ) -> Self {
    // Read when the engine is built, so headless builds can skip it entirely
    self.user_preferences = Some(filter.into());
    self
  }
```

3. Rename `pub fn build(self) -> Engine` to `fn build_engine(mut self, headless: bool) -> Engine`, insert at its top:

```rust
    // User preferences have highest priority; headless runs never read them
    if let Some(filter) = self.user_preferences.take()
      && !headless
    {
      self = self.apply_user_preferences(filter);
    }
```

pass `headless` as a new last argument to the `Engine::new(...)` call at its end, and add:

```rust
  /// Build the engine (consumes the builder).
  pub fn build(self) -> Engine {
    self.build_engine(false)
  }

  /// Build an engine for `testing::HeadlessEngine`: no audio, gamepads, hot
  /// reload, user preferences, capture env vars or wall-clock overlays.
  pub(crate) fn build_headless(self) -> Engine {
    self.build_engine(true)
  }
```

(Move `build`'s existing doc comment, if any, onto the new `build`.)

- [ ] **Step 4: Headless `Engine::new`**

Add `headless: bool` as the last parameter of `fn new(...)`. Then:
- Wrap the `#[cfg(debug_assertions)]` hot-reload block's body in `if !headless { ... }`.
- Change audio init to `let mut audio = if headless { None } else { match AudioManager::new() { ... } };` (existing `match` unchanged inside).
- Change gamepad init to `let gilrs = if headless { None } else { match Gilrs::new() { ... } };`.
- Before `let initial_overlay_mode = OverlayMode::from_flags(`, add:

```rust
    let mut config = config;
    if headless {
      // Wall-clock overlays would make headless frames differ run to run
      config.debug.show_fps = false;
      config.debug.show_frame_timing = false;
      config.debug.show_stats = false;
      config.debug.logger_window = false;
    }
```

  (If `config` is already `mut`, drop the rebinding line.)
- Replace the capture env read

```rust
    let (capture_config, capture_warnings) = AutoCaptureConfig::from_env();
```

with

```rust
    let (capture_config, capture_warnings) = if headless {
      (AutoCaptureConfig::default(), Vec::new())
    } else {
      AutoCaptureConfig::from_env()
    };
```

- [ ] **Step 5: Crate accessors for the testing module**

Add to `impl Engine`:

```rust
  /// The fixed logic timestep (60 Hz until `init_gpu` creates the clock).
  pub(crate) fn fixed_timestep(&self) -> std::time::Duration {
    self
      .time
      .as_ref()
      .map(TimeState::fixed_timestep)
      .unwrap_or_else(|| std::time::Duration::from_secs_f64(1.0 / 60.0))
  }

  pub(crate) fn game_size(&self) -> (u32, u32) {
    (self.config.width, self.config.height)
  }

  pub(crate) fn world(&self) -> &World {
    &self.world
  }

  pub(crate) fn world_mut(&mut self) -> &mut World {
    &mut self.world
  }

  pub(crate) fn input_mut(&mut self) -> &mut InputState {
    &mut self.input
  }

  pub(crate) fn read_frame(&self) -> crate::core::error::CaptureResult<image::RgbaImage> {
    match &self.renderer {
      Some(renderer) => renderer.read_frame(),
      None => Err(crate::core::error::CaptureError::Readback(
        "renderer not initialized".to_string(),
      )),
    }
  }
```

- [ ] **Step 6: Run tests**

Run: `cargo test --lib core::engine::tests` → pass. Run: `cargo test --workspace` → 0 failed (the existing `engine_builder_*_user_pref_*` tests set flags directly and are unaffected).

- [ ] **Step 7: Format, lint, commit**

```bash
rustfmt --edition 2024 src/core/engine.rs
git add src/core/engine.rs
git commit -m "feat(engine): add headless build mode and defer user preferences to build"
```

---

### Task 8: `HeadlessEngine` core

**Files:** Modify `src/core/error.rs`; create `src/core/testing/mod.rs`, `src/core/testing/gpu.rs`; modify `src/core/mod.rs`, `src/lib.rs` (by hand — no rustfmt), `tests/common/gpu.rs`; create `tests/e2e/headless_engine.rs`; rewrite `tests/e2e/engine_lifecycle.rs`; modify `tests/e2e.rs`.

**Interfaces:**
- Consumes: `build_headless`, `init_gpu`, `step_frame`/`FrameOutcome`, accessors (Tasks 6-7); `new_offscreen` (Task 3).
- Produces: `ryo2d::testing::{HeadlessEngine, HeadlessError, StepOutcome, shared_gpu}`. `shared_gpu() -> Result<(Arc<wgpu::Device>, Arc<wgpu::Queue>), HeadlessError>`.

- [ ] **Step 1: Error type**

In `src/core/error.rs`, after `CaptureResult`:

```rust
/// Errors from running the engine headless (`ryo2d::testing`)
#[derive(Error, Debug)]
pub enum HeadlessError {
  /// No GPU adapter is available on this machine
  #[error("no GPU adapter available")]
  NoGpu,

  /// Rendering a frame failed
  #[error("render failed: {0}")]
  Render(String),

  /// Reading a frame back failed
  #[error(transparent)]
  Capture(#[from] CaptureError),
}
```

- [ ] **Step 2: Write the failing e2e tests**

Create `tests/e2e/headless_engine.rs`:

```rust
//! E2E tests for ryo2d::testing::HeadlessEngine.

use ryo2d::core::config::UserPreferenceFilter;
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, StepOutcome};

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// Draws a 20x20 red square at (100, 50) and a 30x30 green UI rect at (10, 10).
struct Squares;

impl Scene for Squares {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(20, 20, 255, 0, 0, 255), "red");
    ctx.world.spawn((Transform::new(100.0, 50.0), Renderable::sprite(sprite)));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    ui.draw_rect(Rect::new(10.0, 10.0, 30.0, 30.0), Color::rgb(0, 255, 0));
    SceneAction::Continue
  }
}

/// A square that moves 2px right per update, for determinism checks.
struct Mover {
  x: f32,
}

impl Scene for Mover {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.x += 2.0;
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    ui.draw_rect(Rect::new(self.x, 40.0, 8.0, 8.0), Color::rgb(255, 255, 0));
    SceneAction::Continue
  }
}

/// Quits on its third update.
struct QuitsOnThird {
  updates: u32,
}

impl Scene for QuitsOnThird {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.updates += 1;
    if self.updates == 3 {
      SceneAction::Quit
    } else {
      SceneAction::Continue
    }
  }
}

fn engine(scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(320, 180).start_scene(scene))
}

#[test]
fn sprite_and_ui_appear_in_frame() {
  let Some(mut engine) = engine(Squares) else { return };

  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();

  assert_eq!(frame.dimensions(), (320, 180));
  assert_eq!(frame.get_pixel(110, 60).0, RED);
  assert_eq!(frame.get_pixel(20, 20).0, GREEN);
}

#[test]
fn frame_before_first_step_is_an_error() {
  let Some(engine) = engine(Squares) else { return };
  assert!(engine.frame().is_err());
}

#[test]
fn same_steps_give_identical_frames() {
  let Some(mut a) = engine(Mover { x: 0.0 }) else { return };
  let Some(mut b) = engine(Mover { x: 0.0 }) else { return };

  a.step_frames(20).unwrap();
  b.step_frames(20).unwrap();

  assert_eq!(a.frame().unwrap().as_raw(), b.frame().unwrap().as_raw());
  assert_eq!(a.frame_count(), 20);
}

#[test]
fn two_engines_share_the_gpu() {
  let Some(mut a) = engine(Squares) else { return };
  let Some(mut b) = engine(Squares) else { return };
  a.step().unwrap();
  b.step().unwrap();
  drop(a);
  b.step().unwrap();
  assert_eq!(b.frame_count(), 2);
}

#[test]
fn quit_stops_stepping() {
  let Some(mut engine) = engine(QuitsOnThird { updates: 0 }) else { return };

  let outcome = engine.step_frames(10).unwrap();

  assert_eq!(outcome, StepOutcome::Quit);
  assert_eq!(engine.frame_count(), 2);
}

#[test]
fn step_after_quit_stays_quit() {
  let Some(mut engine) = engine(QuitsOnThird { updates: 0 }) else { return };
  engine.step_frames(10).unwrap();

  assert_eq!(engine.step().unwrap(), StepOutcome::Quit);
  assert_eq!(engine.frame_count(), 2);
}

#[test]
fn headless_ignores_user_preferences_and_fps_overlay() {
  let prefs = std::path::Path::new("data/system.json");
  let existed = prefs.exists();
  let Some(mut with_prefs) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(320, 180)
      .show_fps(true)
      .with_user_preferences(UserPreferenceFilter::all())
      .start_scene(Squares),
  ) else {
    return;
  };
  let created = !existed && prefs.exists();
  if created {
    let _ = std::fs::remove_dir_all("data");
  }
  assert!(!created, "headless build created data/system.json");

  let Some(mut plain) = engine(Squares) else { return };
  with_prefs.step_frames(2).unwrap();
  plain.step_frames(2).unwrap();
  assert_eq!(with_prefs.frame().unwrap().as_raw(), plain.frame().unwrap().as_raw());
}
```

Rewrite `tests/e2e/engine_lifecycle.rs`:

```rust
//! Engine lifecycle e2e tests, run headless.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

struct Solid([u8; 3]);

impl Scene for Solid {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let [r, g, b] = self.0;
    ui.draw_rect(Rect::new(0.0, 0.0, 64.0, 36.0), Color::rgb(r, g, b));
    SceneAction::Continue
  }
}

/// Pushes `Solid(blue)` on its second update; the pushed scene pops on its second update.
struct Pusher {
  updates: u32,
}

struct PopsOnSecond {
  updates: u32,
}

impl Scene for Pusher {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.updates += 1;
    if self.updates == 2 {
      SceneAction::Push(Box::new(PopsOnSecond { updates: 0 }))
    } else {
      SceneAction::Continue
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    ui.draw_rect(Rect::new(0.0, 0.0, 64.0, 36.0), Color::rgb(255, 0, 0));
    SceneAction::Continue
  }
}

impl Scene for PopsOnSecond {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.updates += 1;
    if self.updates == 2 {
      SceneAction::Pop
    } else {
      SceneAction::Continue
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    ui.draw_rect(Rect::new(0.0, 0.0, 64.0, 36.0), Color::rgb(0, 0, 255));
    SceneAction::Continue
  }
}

#[test]
fn engine_builds_with_defaults() {
  let Some(mut engine) =
    HeadlessEngine::new_or_skip(EngineBuilder::new().start_scene(Solid([0, 255, 0])))
  else {
    return;
  };
  engine.step().unwrap();
  let frame = engine.frame().unwrap();
  assert_eq!(frame.get_pixel(10, 10).0, [0, 255, 0, 255]);
}

#[test]
fn engine_can_be_dropped_and_rebuilt() {
  for _ in 0..3 {
    let Some(mut engine) = HeadlessEngine::new_or_skip(
      EngineBuilder::new()
        .resolution(64, 36)
        .start_scene(Solid([0, 0, 255])),
    ) else {
      return;
    };
    engine.step_frames(2).unwrap();
  }
}

#[test]
fn scene_push_and_pop_render_the_top_scene() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(64, 36)
      .start_scene(Pusher { updates: 0 }),
  ) else {
    return;
  };
  let colour = |engine: &HeadlessEngine| engine.frame().unwrap().get_pixel(10, 10).0;

  engine.step_frames(3).unwrap();
  assert_eq!(colour(&engine), [0, 0, 255, 255], "pushed scene on top");
  engine.step_frames(3).unwrap();
  assert_eq!(colour(&engine), [255, 0, 0, 255], "popped back to the first scene");
}
```

Register in `tests/e2e.rs` (CRLF; Edit tool) after `offscreen_renderer`:

```rust
#[path = "e2e/headless_engine.rs"]
mod headless_engine;
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --test e2e headless_engine`
Expected: compile errors (`ryo2d::testing` not found).

- [ ] **Step 4: Shared GPU**

Create `src/core/testing/gpu.rs`:

```rust
//! One GPU device for every headless engine in the process.
//!
//! Creating devices concurrently deadlocks on some platforms (seen on Windows),
//! so tests running in parallel share this one.

use std::sync::{Arc, OnceLock};

use crate::core::error::HeadlessError;

type Gpu = (Arc<wgpu::Device>, Arc<wgpu::Queue>);

static SHARED: OnceLock<Option<Gpu>> = OnceLock::new();

/// The process-wide headless device and queue, created on first use.
///
/// Returns [`HeadlessError::NoGpu`] when the machine has no usable adapter.
pub fn shared_gpu() -> Result<Gpu, HeadlessError> {
  SHARED
    .get_or_init(|| pollster::block_on(create()))
    .as_ref()
    .map(|(device, queue)| (Arc::clone(device), Arc::clone(queue)))
    .ok_or(HeadlessError::NoGpu)
}

async fn create() -> Option<Gpu> {
  let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
    backends: wgpu::Backends::all(),
    ..Default::default()
  });
  let adapter = instance
    .request_adapter(&wgpu::RequestAdapterOptions {
      power_preference: wgpu::PowerPreference::default(),
      compatible_surface: None,
      force_fallback_adapter: false,
    })
    .await?;
  let (device, queue) = adapter
    .request_device(
      &wgpu::DeviceDescriptor {
        label: Some("Ryo2D Headless Device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: Default::default(),
      },
      None,
    )
    .await
    .ok()?;
  Some((Arc::new(device), Arc::new(queue)))
}
```

In `tests/common/gpu.rs`, replace `fn new()` and the whole `async fn new_async()` with the version below, so the test binary has a single device (`pollster` stays imported: `read_pixels_sync` uses it):

```rust
  /// Create a headless GPU context on the engine's shared device.
  ///
  /// Returns `None` if no GPU adapter is available (e.g., in CI without GPU).
  /// Prefer `shared()` in tests.
  fn new() -> Option<Self> {
    let (device, queue) = ryo2d::testing::shared_gpu().ok()?;
    Some(Self {
      device,
      queue,
      format: wgpu::TextureFormat::Rgba8Unorm,
    })
  }
```

- [ ] **Step 5: `HeadlessEngine`**

Create `src/core/testing/mod.rs`:

```rust
//! Run the engine without a window, for tests.
//!
//! [`HeadlessEngine`] drives a real scene through the full update and render
//! pipeline at a fixed timestep and reads frames back as pixels:
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//! use ryo2d::testing::HeadlessEngine;
//!
//! #[test]
//! fn enemy_turns_red_when_hit() {
//!   // None (with a "skipping: no GPU" line) on machines without a GPU
//!   let Some(mut engine) = HeadlessEngine::new_or_skip(
//!     EngineBuilder::new().resolution(320, 180).start_scene(Arena::default()),
//!   ) else { return };
//!
//!   engine.step_frames(10).unwrap();
//!   engine.tap_key(KeyCode::Space);
//!   engine.step_frames(30).unwrap();
//!
//!   let frame = engine.frame().unwrap();
//!   assert_eq!(frame.get_pixel(160, 90).0, [255, 0, 0, 255]);
//! }
//! ```
//!
//! Headless runs have no audio (`ctx.audio` is `None`), no gamepads, no hot
//! reload and no dev windows; they never read or write `system.json` user
//! preferences, ignore `RYO_CAPTURE_*`, and keep the FPS, frame-timing and
//! stats overlays off. `Ryo.toml` is honoured as in a normal build. Seed any
//! randomness your scene uses if you compare frames exactly.

mod gpu;

use image::RgbaImage;
use winit::event::{ElementState, MouseButton};
use winit::keyboard::KeyCode;

use crate::core::ecs::World;
use crate::core::engine::{Engine, EngineBuilder, FrameOutcome};
use crate::core::graphics::WgpuRenderer;

pub use crate::core::error::HeadlessError;
pub use gpu::shared_gpu;

/// Whether the scene is still running after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
  Running,
  /// The scene (or console) asked to quit; further steps do nothing
  Quit,
}

/// The engine running without a window. See the [module docs](self).
pub struct HeadlessEngine {
  engine: Engine,
  presented: u64,
  quit: bool,
  pending_releases: Vec<KeyCode>,
}

impl HeadlessEngine {
  /// Build the engine headless and enter its initial scene.
  pub fn new(builder: EngineBuilder) -> Result<Self, HeadlessError> {
    let (device, queue) = shared_gpu()?;
    let mut engine = builder.build_headless();
    let (width, height) = engine.game_size();
    engine.init_gpu(WgpuRenderer::new_offscreen(device, queue, width, height), 1.0);
    Ok(Self {
      engine,
      presented: 0,
      quit: false,
      pending_releases: Vec::new(),
    })
  }

  /// Like [`new`](Self::new), but returns `None` (and prints a skip line) when
  /// there is no GPU, so tests can `return` early. Panics on any other error.
  pub fn new_or_skip(builder: EngineBuilder) -> Option<Self> {
    match Self::new(builder) {
      Ok(engine) => Some(engine),
      Err(HeadlessError::NoGpu) => {
        eprintln!("skipping: no GPU available for HeadlessEngine");
        None
      }
      Err(error) => panic!("failed to start HeadlessEngine: {error}"),
    }
  }

  /// Run one frame: one fixed update, then render.
  pub fn step(&mut self) -> Result<StepOutcome, HeadlessError> {
    if self.quit {
      return Ok(StepOutcome::Quit);
    }
    let dt = self.engine.fixed_timestep();
    let outcome = self.engine.step_frame(Some(dt));
    for key in self.pending_releases.drain(..) {
      self.engine.input_mut().handle_keyboard(key, ElementState::Released);
    }
    match outcome {
      FrameOutcome::Presented => {
        self.presented += 1;
        Ok(StepOutcome::Running)
      }
      FrameOutcome::Skipped => Ok(StepOutcome::Running),
      FrameOutcome::Quit => {
        self.quit = true;
        Ok(StepOutcome::Quit)
      }
      FrameOutcome::Fatal(error) => Err(HeadlessError::Render(error)),
    }
  }

  /// Run up to `frames` frames, stopping early if the scene quits.
  pub fn step_frames(&mut self, frames: u32) -> Result<StepOutcome, HeadlessError> {
    for _ in 0..frames {
      if self.step()? == StepOutcome::Quit {
        return Ok(StepOutcome::Quit);
      }
    }
    Ok(StepOutcome::Running)
  }

  /// Number of frames presented so far.
  pub fn frame_count(&self) -> u64 {
    self.presented
  }

  /// The last presented frame (scene + UI) at game resolution, origin top-left.
  pub fn frame(&self) -> Result<RgbaImage, HeadlessError> {
    if self.presented == 0 {
      return Err(HeadlessError::Render("no frame presented yet".to_string()));
    }
    Ok(self.engine.read_frame()?)
  }

  /// The ECS world, for inspecting entities between steps.
  pub fn world(&self) -> &World {
    self.engine.world()
  }

  /// The ECS world, for spawning or changing entities between steps.
  pub fn world_mut(&mut self) -> &mut World {
    self.engine.world_mut()
  }
}
```

(`FrameOutcome` and `EngineBuilder::build_headless` are `pub(crate)` from Tasks 6-7.)

Wire the module by hand (no rustfmt on these two files):
- `src/core/mod.rs`: add `pub mod testing;` after `pub mod systems;`.
- `src/lib.rs`: add `pub use core::testing;` directly after `pub mod core;`.

- [ ] **Step 6: Run tests**

Run: `cargo test --test e2e headless_engine -- --nocapture` and `cargo test --test e2e engine_lifecycle -- --nocapture` → all pass, no "skipping" lines on a GPU machine. If `scene_push_and_pop_render_the_top_scene` is off by one step (the push or pop takes effect a frame later than assumed), adjust only the step counts — never the expected colours — and note it in the ledger.
Run: `cargo test --workspace` → 0 failed. Confirm `git status --short data/` shows nothing.

- [ ] **Step 7: Format, lint, commit**

```bash
rustfmt --edition 2024 src/core/error.rs src/core/testing/mod.rs src/core/testing/gpu.rs tests/e2e/headless_engine.rs tests/e2e/engine_lifecycle.rs tests/common/gpu.rs
git diff --stat   # only intended files
git add src/core/error.rs src/core/testing src/core/mod.rs src/lib.rs tests/common/gpu.rs tests/e2e/headless_engine.rs tests/e2e/engine_lifecycle.rs tests/e2e.rs
git commit -m "feat(testing): add HeadlessEngine for windowless scene tests"
```

---

### Task 9: Input injection

**Files:** Modify `src/core/testing/mod.rs`, `tests/e2e/headless_engine.rs`.

**Interfaces:**
- Consumes: `HeadlessEngine` (Task 8); `InputState::{handle_keyboard, handle_mouse_button, handle_mouse_move}` (`pub(crate)`).
- Produces: `press_key`, `release_key`, `tap_key`, `press_mouse`, `release_mouse`, `mouse_move(x: f32, y: f32)`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/e2e/headless_engine.rs`:

```rust
/// Paints input state into the top-left 4 cells of a 4x1 strip:
/// [0] Space held, [1] Space just pressed, [2] Space just released, [3] left mouse held.
/// Also draws a 4x4 marker at the mouse position.
#[derive(Default)]
struct InputProbe {
  held: bool,
  pressed: bool,
  released: bool,
  mouse_down: bool,
  mouse: (f32, f32),
}

impl Scene for InputProbe {
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.held = ctx.input.key_down(KeyCode::Space);
    self.pressed = ctx.input.key_pressed(KeyCode::Space);
    self.released = ctx.input.key_released(KeyCode::Space);
    self.mouse_down = ctx.input.mouse_down(MouseButton::Left);
    self.mouse = ctx.input.mouse_position();
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    for (i, on) in [self.held, self.pressed, self.released, self.mouse_down]
      .into_iter()
      .enumerate()
    {
      if on {
        ui.draw_rect(Rect::new(i as f32 * 10.0, 0.0, 10.0, 10.0), Color::rgb(255, 255, 255));
      }
    }
    ui.draw_rect(Rect::new(self.mouse.0, self.mouse.1, 4.0, 4.0), Color::rgb(255, 0, 255));
    SceneAction::Continue
  }
}

/// Which of the four indicator cells are lit in the current frame.
fn cells(engine: &HeadlessEngine) -> [bool; 4] {
  let frame = engine.frame().unwrap();
  [0, 1, 2, 3].map(|i| frame.get_pixel(i * 10 + 5, 5).0 == [255, 255, 255, 255])
}

#[test]
fn press_key_is_just_pressed_once() {
  let Some(mut engine) = engine(InputProbe::default()) else { return };

  engine.press_key(KeyCode::Space);
  engine.step().unwrap();
  assert_eq!(cells(&engine), [true, true, false, false]);
  engine.step().unwrap();
  assert_eq!(cells(&engine), [true, false, false, false]);
}

#[test]
fn release_key_is_just_released_once() {
  let Some(mut engine) = engine(InputProbe::default()) else { return };
  engine.press_key(KeyCode::Space);
  engine.step_frames(2).unwrap();

  engine.release_key(KeyCode::Space);
  engine.step().unwrap();
  assert_eq!(cells(&engine), [false, false, true, false]);
  engine.step().unwrap();
  assert_eq!(cells(&engine), [false, false, false, false]);
}

#[test]
fn tap_key_presses_for_one_step() {
  let Some(mut engine) = engine(InputProbe::default()) else { return };

  engine.tap_key(KeyCode::Space);
  engine.step().unwrap();
  assert_eq!(cells(&engine), [true, true, false, false]);
  engine.step().unwrap();
  assert_eq!(cells(&engine), [false, false, true, false]);
}

#[test]
fn mouse_move_and_button_reach_the_scene() {
  let Some(mut engine) = engine(InputProbe::default()) else { return };

  engine.mouse_move(200.0, 100.0);
  engine.press_mouse(MouseButton::Left);
  engine.step().unwrap();
  let frame = engine.frame().unwrap();
  assert_eq!(frame.get_pixel(201, 101).0, [255, 0, 255, 255]);
  assert!(cells(&engine)[3]);

  engine.release_mouse(MouseButton::Left);
  engine.step().unwrap();
  assert!(!cells(&engine)[3]);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test e2e headless_engine`
Expected: compile errors (`press_key` etc. not found).

- [ ] **Step 3: Implement**

Add to `impl HeadlessEngine`:

```rust
  /// Hold `key` down from the next step until [`release_key`](Self::release_key).
  pub fn press_key(&mut self, key: KeyCode) {
    self.engine.input_mut().handle_keyboard(key, ElementState::Pressed);
  }

  /// Release `key`; the next step sees it as just released.
  pub fn release_key(&mut self, key: KeyCode) {
    self.engine.input_mut().handle_keyboard(key, ElementState::Released);
  }

  /// Press `key` for the next step only; it is released after that step.
  pub fn tap_key(&mut self, key: KeyCode) {
    self.press_key(key);
    self.pending_releases.push(key);
  }

  /// Hold a mouse button down from the next step.
  pub fn press_mouse(&mut self, button: MouseButton) {
    self
      .engine
      .input_mut()
      .handle_mouse_button(button, ElementState::Pressed);
  }

  /// Release a mouse button.
  pub fn release_mouse(&mut self, button: MouseButton) {
    self
      .engine
      .input_mut()
      .handle_mouse_button(button, ElementState::Released);
  }

  /// Move the mouse to `(x, y)` in game coordinates.
  pub fn mouse_move(&mut self, x: f32, y: f32) {
    self
      .engine
      .input_mut()
      .handle_mouse_move(f64::from(x), f64::from(y));
  }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test e2e headless_engine -- --nocapture` → all pass. Run: `cargo test --workspace` → 0 failed.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/core/testing/mod.rs tests/e2e/headless_engine.rs
git add src/core/testing/mod.rs tests/e2e/headless_engine.rs
git commit -m "feat(testing): simulate keyboard and mouse input in HeadlessEngine"
```

---

### Task 10: Documentation

**Files:** Create `src/core/testing/AGENTS.md`; modify the `ryo-game-dev` and `ryo-engine-dev` skill files (both copies each; CRLF — append with a byte-preserving edit).

- [ ] **Step 1: Module AGENTS.md**

Create `src/core/testing/AGENTS.md`:

```markdown
# AGENTS.md - Headless Testing

`HeadlessEngine` runs the real engine without a window: offscreen `WgpuRenderer`, one fixed update per `step`, pixels via `frame()`. Spec: `docs/plans/2026-10-10-headless-rendering-design.md`.

## How it fits

- `EngineBuilder::build_headless` → `Engine::new(.., headless: true)` → `init_gpu(WgpuRenderer::new_offscreen(..))`.
- `step()` calls the same `Engine::step_frame` as the window loop, with `dt = Some(fixed_timestep)`; `TimeState::advance` makes that exactly one update.
- Input goes through the same `InputState::handle_*` methods winit events use.

## Gotchas

- One process-wide device (`shared_gpu`) — concurrent device creation deadlocks on Windows. `tests/common/gpu.rs` reuses it.
- Headless never calls `TimeState::tick_fps_counter`: it resets `frame_count` on the wall clock, and `frame_count` drives trail/lighting animation time.
- Headless mode disables audio, gamepads, hot reload, dev windows, user preferences and `RYO_CAPTURE_*`, and forces FPS/timing/stats overlays off. Keep new wall-clock-dependent features out of headless frames the same way.
- `tick_frame` (unfocused window) is still a separate frame path; it shares `present_frame` but not `step_frame`.
```

- [ ] **Step 2: Skills**

Append to both `ryo-game-dev` skill files (`src/cli/skills/ryo-game-dev.md`, `.claude/skills/ryo-game-dev/SKILL.md`), preserving CRLF:

```markdown
## Visual Tests

Test what your scenes draw with `ryo2d::testing::HeadlessEngine` — no window, deterministic steps, pixels out:

```rust
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

#[test]
fn player_is_drawn_at_spawn() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new().resolution(320, 180).start_scene(Level::default()),
  ) else { return }; // no GPU: skip

  engine.step_frames(5).unwrap();
  engine.tap_key(KeyCode::Space); // press_key / release_key / press_mouse / mouse_move too
  engine.step_frames(10).unwrap();

  let frame = engine.frame().unwrap(); // RgbaImage, game resolution
  assert_eq!(frame.get_pixel(160, 90).0, [255, 0, 0, 255]);
}
```

- One fixed update per step; same scene + steps + input → identical frames (seed your RNG).
- Headless: no audio (`ctx.audio` is `None`), no gamepads, no user preferences; `Ryo.toml` is honoured.
- Save a failing frame with `engine.frame().unwrap().save("failed.png")`.
```

Append to both `ryo-engine-dev` skill files:

```markdown
## Headless Engine Tests

Regression tests for rendering go in `tests/e2e/` using `ryo2d::testing::HeadlessEngine` (see `src/core/testing/AGENTS.md`). Write the failing pixel assertion first, then fix.
```

- [ ] **Step 3: Verify and commit**

Run: `cargo test --workspace --doc` → 0 failed (the module doc example is `ignore`).

```bash
git add src/core/testing/AGENTS.md src/cli/skills/ryo-game-dev.md src/cli/skills/ryo-engine-dev.md .claude/skills/ryo-game-dev/SKILL.md .claude/skills/ryo-engine-dev/SKILL.md
git commit -m "docs: document headless engine tests"
```
