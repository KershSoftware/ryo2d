# Headless Rendering - Design

## Overview

Run real scenes through the engine's full update and render pipeline without a window, step them deterministically, simulate basic input, and read frames back as pixels — so engine tests and game projects can write `cargo test` visual tests.

**Sub-project 2 of 3** (see `2026-10-10-frame-capture-design.md`). Sub-project 1 (frame capture) is done. Sub-project 3 — the trails-vs-lighting fix — is written test-first on top of this.

**Audience:** engine tests in this repo **and** game developers (and their agents). The harness is public API: `ryo2d::testing::HeadlessEngine`.

**Success criteria:**
- A test can build a scene, step N frames, press keys / move the mouse, and assert on pixels of the composed frame (scene + lighting + post-processing + UI) at game resolution.
- Same scene + same steps + same input → byte-identical frames.
- Runs under plain `cargo test` on any thread; skips (does not fail) when no GPU is available.
- Headless and windowed runs share one render path and one frame path — no duplicated code to drift.
- No side effects on the developer's machine (audio, gamepads, user preferences, files).

**Out of scope:** unifying `tick_frame` (unfocused-window path) with the frame step; the trails-vs-lighting fix (sub-project 3); golden-image diff tooling; scroll/text/gamepad input injection; seeded RNG helpers; the Ryo.toml-overrides-builder-debug-flags precedence bug.

## Approach

**Chosen:** an offscreen present target inside `WgpuRenderer`, plus one shared `step_frame()` used by both the window event handler and `HeadlessEngine`.

**Rejected:**
- A separate headless renderer/engine — duplicates render paths, the exact drift that caused the trails-vs-lighting regression.
- A hidden winit window — winit allows one event loop per process and requires the main thread on macOS, so it cannot run inside `cargo test`.

## Renderer: Present Target

Today the window surface is used only for: adapter selection (`compatible_surface`), surface caps (format, present mode, alpha), `config`/`resize`, the three `get_current_texture`/`present` pairs (`render_with_gpu_sprites`, `render_with_lighting`, `render_raycast`), `presentation_blocks`, and the format of the two final-composite pipelines (`render_pipeline`, `blend_pipeline`) and the capture texture. Every other renderer (sprite GPU, lighting, trails, raycast, post-process) targets the game texture format (`Rgba8UnormSrgb`).

```rust
enum PresentTarget {
  Surface { surface: wgpu::Surface<'static>, config: wgpu::SurfaceConfiguration },
  Offscreen { texture: wgpu::Texture },
}
```

- `WgpuRenderer::new` (windowed) builds `Surface` as today; its existing panics on adapter/device failure are unchanged.
- New `WgpuRenderer::new_offscreen(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>, width: u32, height: u32) -> Self`:
  - offscreen texture at `width × height`, format `Rgba8UnormSrgb`, usage `RENDER_ATTACHMENT | COPY_SRC`;
  - window size = game size, so the viewport uniform is the identity (no letterbox);
  - `instance` and `adapter` become `Option` (only dev windows use them; their getters return `Option`);
  - `presentation_blocks = false`.
- The composite pipelines and capture texture use `target_format()` (surface format or `Rgba8UnormSrgb`).
- Each render path acquires its output view through the target; for `Offscreen`, acquire returns the texture's view and present is a no-op. `resize` is a no-op for `Offscreen`.
- `read_frame(&self) -> CaptureResult<RgbaImage>` reads the offscreen texture back via `read_texture_rgba` (`CaptureError::Readback` for a `Surface` target). Headless frames therefore need no capture pass.

## Engine: One Init, One Present, One Frame

### `init_gpu`

Extract from `resumed()` everything that needs only device/queue into:

```rust
fn init_gpu(&mut self, renderer: WgpuRenderer, scale_factor: f64)
```

It creates the sprite-GPU, lighting, trail and raycast renderers, sets `scale_factor`/`ui_scale`, stores the renderer, creates `TimeState`, initialises the profiler and runs the initial scene's `on_enter`. `resumed()` keeps window creation, fullscreen/IME/icon setup, the anti-flash first render, `set_visible`, and dev windows, and calls `init_gpu` in between.

### `present_frame`

`do_full_render_and_present` becomes the single present path:

```rust
enum PresentOutcome { Presented, Skipped, Fatal(String) }
fn present_frame(&mut self) -> PresentOutcome
```

- It records `frame_timings.render` and `frame_timings.present` itself (today only the inline copy does).
- `Outdated`/`Lost` → `Skipped` (handled as today); any other `SurfaceError` → `Fatal`.
- The inline render/present copy in the `RedrawRequested` handler is deleted. `tick_frame` already calls this function.
- Capture hooks (`path_for_next_frame` before, `on_frame_presented` on `Presented`) live here once.

### `step_frame`

The body of the `RedrawRequested` handler becomes:

```rust
enum FrameOutcome { Presented, Skipped, Quit, Fatal(String) }
fn step_frame(&mut self) -> FrameOutcome
```

In order: `time.begin_frame()` (or the headless advance, below), debug keys, console toggle, F9/F10/F12, gamepad events, console update/commands, cursor request, interpolation snapshot, the fixed-update loop, hitstop decay, quit check (`Quit`), `present_frame()`, `post_render_cleanup`, `input.begin_frame` when an update ran.

The window handler keeps only window work around it: pre-frame surface reconfigure, dev-window rendering, slow-frame warning, FPS counter, frame limiting, and `event_loop.exit()` on `Quit`/`Fatal`.

### Time seam

`TimeState` gains:

```rust
pub fn advance(&mut self, dt: Duration)
```

`begin_frame()` becomes `advance(now - last_frame)` (keeping the 5× accumulator cap). Headless steps call `advance(fixed_timestep)` instead, which yields exactly one fixed update per step with interpolation alpha 0, and never call `tick_fps_counter` (wall-clock), so `frame_count` — which feeds trail/lighting animation time — advances deterministically.

## Public API: `ryo2d::testing`

```rust
pub struct HeadlessEngine { /* Engine in headless mode */ }

#[derive(Debug, thiserror::Error)]
pub enum HeadlessError {
  #[error("no GPU adapter available")]
  NoGpu,
  #[error("render failed: {0}")]
  Render(String),
  #[error(transparent)]
  Capture(#[from] CaptureError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome { Running, Quit }

impl HeadlessEngine {
  pub fn new(builder: EngineBuilder) -> Result<Self, HeadlessError>;
  /// `None` (with a "skipping: no GPU" line on stderr) when no GPU; panics on other errors.
  pub fn new_or_skip(builder: EngineBuilder) -> Option<Self>;

  pub fn step(&mut self) -> Result<StepOutcome, HeadlessError>;
  /// Stops early and returns `Quit` if the scene quits.
  pub fn step_frames(&mut self, frames: u32) -> Result<StepOutcome, HeadlessError>;
  pub fn frame_count(&self) -> u64;

  /// Last presented frame (scene + UI) at game resolution, origin top-left.
  /// Errors if no frame has been presented yet.
  pub fn frame(&self) -> Result<RgbaImage, HeadlessError>;

  pub fn world(&self) -> &World;
  pub fn world_mut(&mut self) -> &mut World;

  pub fn press_key(&mut self, key: KeyCode);
  pub fn release_key(&mut self, key: KeyCode);
  /// Pressed for the next step, released after it.
  pub fn tap_key(&mut self, key: KeyCode);
  pub fn press_mouse(&mut self, button: MouseButton);
  pub fn release_mouse(&mut self, button: MouseButton);
  /// Game coordinates.
  pub fn mouse_move(&mut self, x: f32, y: f32);
}
```

### Input semantics

Injected events call the same `InputState` handlers winit events use (`handle_keyboard`, `handle_mouse_button`, `handle_mouse_move`, all `pub(crate)`), so they behave exactly like events arriving between frames: visible to the next step's update; "just pressed"/"just released" last one step (cleared by `input.begin_frame` after the update). `tap_key` presses immediately and queues the release to be applied after the next step.

### Headless mode (isolation defaults)

`HeadlessEngine::new` builds the engine with a headless flag that:

- creates no audio manager (scenes' audio calls are no-ops, as when no output device exists) and no gamepad input;
- disables hot reload and dev windows; ignores cursor and window/display-mode requests;
- never reads or writes `system.json` user preferences and ignores `RYO_CAPTURE_*`;
- forces off overlays that draw wall-clock values (FPS, frame timing, stats); collider/velocity/origin overlays are honoured;
- honours `Ryo.toml` exactly as a normal build does (a game's tests use its real config; `cargo test` runs from the package root so asset paths resolve as in the game).

Randomness inside scenes is the scene's responsibility (seed it in tests); documented.

### GPU device

One lazily created, process-wide device (`OnceLock`): adapter requested with `compatible_surface: None`, empty features, default limits (identical to `WgpuRenderer::new`). Concurrent device creation deadlocks on Windows, so every `HeadlessEngine` shares it; each gets its own `new_offscreen` renderer. No adapter → `HeadlessError::NoGpu`.

## Error Handling

- `new_offscreen` and the shared-device helper never panic.
- Offscreen targets cannot produce surface errors; any `Fatal` present outcome becomes `HeadlessError::Render`.
- `frame()` before the first presented frame returns `HeadlessError::Render("no frame presented yet")`.
- Windowed behaviour is unchanged: `Fatal` still logs and exits the event loop; `Skipped` still reconfigures/resizes.

## Testing

**Unit (no GPU):**
- `TimeState::advance`: one fixed timestep → exactly one `should_update`; sub-step deltas accumulate; the 5× cap holds; interpolation alpha after an exact step is 0.

**E2E (GPU; skip without):**
- Offscreen renderer: a known CPU pixel-buffer pattern rendered with `render()` reads back identically via `read_frame()` (allowing ±1 per channel for sRGB round-trips).
- `HeadlessEngine` with small test scenes:
  - a sprite at a known position appears at that pixel;
  - UI drawn in `render_ui` appears in `frame()`;
  - determinism: two engines, same scene and steps → byte-identical frames;
  - input: `press_key` held across steps, `tap_key` visible for exactly one step's "just pressed", mouse move/button reach the scene;
  - a scene returning `SceneAction::Quit` makes `step` return `Quit`;
  - isolation: no `system.json` is created; the FPS overlay stays off when the workspace enables it.
- Replace the three empty `#[ignore]` stubs in `tests/e2e/engine_lifecycle.rs` with real headless tests.

**Present-path unification (behaviour-preserving):** full suite; the headless e2e tests (which now exercise the single `present_frame`); windowed before/after captures (`RYO_CAPTURE_FRAMES`) of the lighting, trails and UI demos compared visually (FPS text and exact animation phase may differ).

## Documentation

- Rustdoc on everything in `ryo2d::testing`, with the example from this spec.
- `src/core/testing/AGENTS.md`: headless-mode flags, the shared device, the time seam, why `tick_fps_counter` is skipped.
- "Visual tests" section in the shipped `ryo-game-dev` skill (`src/cli/skills/` and `.claude/skills/` copies); a pointer from `ryo-engine-dev`.
- No mdbook pages (being retired).

## Design Decisions

- **One present path and one frame path:** headless tests are only trustworthy if they run the code players run; duplicated paths are how the trails-vs-lighting regression happened.
- **Offscreen read-back, not the capture pass:** the offscreen target already holds the composed frame at game resolution.
- **Time seam at `TimeState::advance`:** the smallest change that makes stepping deterministic without touching the fixed-update loop.
- **Shared process-wide device:** avoids the Windows concurrent-initialisation deadlock already documented in `tests/common/gpu.rs`.
- **Honour Ryo.toml, ignore user preferences:** game tests should match the game's configuration but never depend on, or modify, one developer's saved settings.
