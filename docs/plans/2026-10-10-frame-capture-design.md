# Frame Capture - Design

## Overview

Engine-level frame capture: save the final composed frame (scene + UI, at game resolution) as a PNG. The primary user is an AI agent verifying rendering bugs without a human watching; developers also get a hotkey and a console command.

**Motivation:** a trails-vs-lighting regression (`d9fbbba`) could not be reproduced or verified automatically — `WgpuRenderer` needs a window surface and the engine had no way to get pixels out. Frame capture gives agents eyes on real engine output.

**Scope (sub-project 1 of 3):** capture core, env-var auto-capture, F12 hotkey, `capture` console command.

- Sub-project 2 (separate spec): headless rendering (offscreen present target + `HeadlessEngine` test harness) and unifying the engine's duplicated present paths. It reuses this spec's readback and final-composite pieces.
- Sub-project 3: the trails-vs-lighting fix, test-first on top of 1 and 2.

**Out of scope:** game-facing screenshot API, `[capture]` section in Ryo.toml, video/frame sequences, headless rendering (sub-project 2).

**Success criteria:**
- `RYO_CAPTURE_FRAMES=120 RYO_CAPTURE_EXIT=1` on any example writes `captures/frame-000120.png` matching what is on screen (minus letterboxing) and exits.
- Output is independent of window size and DPI (game resolution).
- Zero per-frame cost when no capture is pending.
- F12 (dev) and `capture [path]` (console) save the current frame.

## Architecture

### Module Structure

```
src/core/capture/
├── mod.rs       — CaptureRequest, PNG writer (background thread), re-exports
├── readback.rs  — read_texture_rgba(): texture → tightly packed RGBA8
└── config.rs    — AutoCapture: RYO_CAPTURE_* parsing and frame schedule
```

Exposed as `ryo2d::core::capture` (not in the prelude — dev tooling). `CaptureError` lives in `src/core/error.rs` with the other error enums.

### Responsibilities

| Unit | Knows about | Does not know about |
|------|-------------|---------------------|
| `readback` | wgpu device/queue/texture | windows, engine, files |
| `config` (`AutoCapture`) | env vars, frame numbers | GPU, files |
| `mod` (writer) | RGBA image, paths, `image` crate | GPU, engine |
| `WgpuRenderer` | one pending capture path; drawing the final composite | when or why to capture |
| `Engine` | frame counter, schedule, F12, console, exit | GPU details |

The renderer only holds a single pending request. All scheduling stays in the engine so sub-project 2's headless harness can drive the same renderer API.

## Readback (`readback.rs`)

```rust
pub fn read_texture_rgba(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  texture: &wgpu::Texture,
) -> Result<image::RgbaImage, CaptureError>;
```

- Copies the texture into a `MAP_READ` buffer with `bytes_per_row` padded to `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT` (256), submits, maps, and blocks with `device.poll(wgpu::Maintain::Wait)`.
- Strips row padding and converts to RGBA8: `Bgra8Unorm`/`Bgra8UnormSrgb` are swizzled; `Rgba8Unorm`/`Rgba8UnormSrgb` are copied; any other format returns `CaptureError::UnsupportedFormat`.
- Pure helpers `strip_row_padding()` and `bgra_to_rgba_in_place()` are separate functions so they can be unit-tested without a GPU.
- The texture must have `COPY_SRC` usage.

## Renderer Integration (`WgpuRenderer`)

### Final composite helper

The final pass (game/post-process source + optional UI overlay, fullscreen quad with the letterbox uniform) is currently duplicated in `render_with_effects`, `render_with_gpu_sprites` and `render_with_lighting`. It becomes one private helper:

```rust
fn draw_final_composite(
  &self,
  pass: &mut wgpu::RenderPass<'_>,
  source: &wgpu::BindGroup,
  ui: Option<&wgpu::BindGroup>,
);
```

All three render paths call it for the surface pass. This is a behavior-preserving extraction that the capture pass needs; it also removes one source of drift between paths.

### Capture pass

```rust
pub fn request_capture(&mut self, path: PathBuf);
pub fn take_capture_result(&mut self) -> Option<Result<CapturedFrame, CaptureError>>;
```

- `request_capture` stores one pending path (a later request in the same frame replaces it).
- When a capture is pending, the render path records a second pass that calls `draw_final_composite` into an offscreen **capture texture**: game resolution, surface format, `RENDER_ATTACHMENT | COPY_SRC`, drawn with a dedicated identity uniform (no letterbox scaling).
- After the frame is submitted, the renderer calls `read_texture_rgba` and stores the result for `take_capture_result`. `CapturedFrame` carries the `RgbaImage` and target path.
- The capture texture and identity uniform are created lazily on first capture and recreated in `resize_buffer` when the game resolution changes. Frames without a pending capture do no extra work.

## Engine Integration

- A presented-frame counter (1-based) increments only when a render call returns `Ok` (frames skipped for surface loss or minimisation are not counted). "Frame 60" therefore always means the 60th presented frame.
- Before each render: if `AutoCapture` schedules the next frame, or F12 / the console requested a capture, call `renderer.request_capture(path)`.
- After each render: `take_capture_result()`; on `Ok`, hand the image to the writer; on `Err`, log a failure.
- With `RYO_CAPTURE_EXIT=1`, once the last scheduled frame is captured the engine waits for pending writes and quits. If any scheduled capture failed, the process exits with code 1.
- Both present paths in `engine.rs` (`do_full_render_and_present` and the inline `RedrawRequested` handler) get the same before/after hooks via two small engine methods, so they cannot diverge. Unifying the paths themselves is sub-project 2.

## Triggers and Config

### Environment variables

Read once in `EngineBuilder::build()`. Honoured in dev **and** release builds (opt-in via environment; needed to verify release rendering). No cost when unset.

| Variable | Meaning | Default |
|----------|---------|---------|
| `RYO_CAPTURE_FRAMES` | Comma-separated presented-frame numbers, e.g. `60,120` (whitespace allowed) | unset (disabled) |
| `RYO_CAPTURE_DIR` | Output directory, relative to the working directory | `captures` |
| `RYO_CAPTURE_EXIT` | `1`/`true` (case-insensitive): quit after the last scheduled capture is written; `0`/`false`: off | off |

- Invalid `RYO_CAPTURE_FRAMES` values (non-numeric, `0`, empty list) produce one warning and disable auto-capture; the game runs normally. Duplicates are ignored and the list is sorted.
- Any other `RYO_CAPTURE_EXIT` value produces one warning and is treated as off.
- `RYO_CAPTURE_DIR` also applies to F12 and argument-less `capture`, even when `RYO_CAPTURE_FRAMES` is unset.
- `RYO_CAPTURE_EXIT` without `RYO_CAPTURE_FRAMES` has no effect.

### F12 hotkey

Handled next to F9/F10 and gated by `config.debug.dev_window_keys` (on in dev, off in release by default). Saves to `<RYO_CAPTURE_DIR or captures>/frame-NNNNNN.png`. The stale "F8-F9" comments in `config/debug.rs` are corrected to the actual keys (F9 logger, F10 profiler, F12 capture).

### Console command

Built-in `capture [path]`. Sets `CommandContext::capture_requested: Option<PathBuf>` (same pattern as `quit_requested`); with no argument it uses the default directory and name. A relative path is resolved against the working directory. The console prints the destination when the capture is written.

### File naming and feedback

- Default name: `frame-NNNNNN.png` (presented-frame number, zero-padded to 6). An explicit console path wins.
- Existing files are overwritten (deterministic paths are what agents want). Missing directories are created.
- Each capture logs one line to stdout and the dev log: `[capture] frame 60 → captures/frame-000060.png`; failures log `[capture] FAILED frame 60: <error>` to stderr and the dev log.

## PNG Writer

- Encodes with the existing `image` crate (`RgbaImage::save`) on a background thread so the game does not stall on encoding.
- The engine keeps the join handles of in-flight writes; exit-after-capture joins them before quitting and collects their results.

## Error Type

```rust
#[derive(Error, Debug)]
pub enum CaptureError {
  #[error("Texture format {0:?} is not supported for capture")]
  UnsupportedFormat(wgpu::TextureFormat),
  #[error("GPU readback failed: {0}")]
  Readback(String),
  #[error("Failed to create capture directory {path}: {source}")]
  CreateDir { path: PathBuf, source: std::io::Error },
  #[error("Failed to write {path}: {source}")]
  Write { path: PathBuf, source: image::ImageError },
}
```

Capture errors are never fatal: they are logged and the game continues (except for the exit code under `RYO_CAPTURE_EXIT`).

## Testing

**Unit (no GPU):**
- `AutoCapture` parsing: valid list, whitespace, duplicates/unsorted, empty, garbage, `0`, `RYO_CAPTURE_EXIT` values.
- Schedule logic: next frame due, "all done" detection for exit.
- File naming (`frame-000060.png`), default dir vs `RYO_CAPTURE_DIR`.
- `strip_row_padding` and `bgra_to_rgba_in_place` on synthetic buffers.

**GPU e2e (`tests/e2e/`, skipped without a GPU via `TestGpu::shared()`):**
- Render known colours into a 37-px-wide offscreen texture (forces row padding) in `Rgba8UnormSrgb` and `Bgra8UnormSrgb`; `read_texture_rgba` returns exact expected pixels.
- PNG round trip: write via the writer, reload with `image`, compare.

**Known gap:** the capture pass inside `WgpuRenderer` needs a window surface and is not covered automatically until sub-project 2. Manual acceptance until then:
- `RYO_CAPTURE_FRAMES=120 RYO_CAPTURE_EXIT=1` on the trails demo writes a PNG matching the screen and exits with code 0.
- F12 and `capture` work in a dev run; `capture /tmp/x.png` writes to that path.
- A capture of the trails demo with a `Light2D` present shows no trails (the known bug), providing sub-project 3's red evidence.

## Documentation

- Rustdoc on all public items.
- `src/core/capture/AGENTS.md`: readback contract (`COPY_SRC`, blocking), renderer/engine split, env vars.
- "Frame capture" note in the shipped `ryo-cli` skill (`src/cli/skills/ryo-cli.md` and `.claude/skills/ryo-cli/SKILL.md`) and in `ryo-engine-dev`, so agents in game projects discover the env vars.
- An mdbook page under `docs/src/` linked from `SUMMARY.md`.

## Design Decisions

- **Offscreen pass at game resolution, not surface copy:** deterministic across window sizes/DPI, no letterbox bars, no reliance on surface `COPY_SRC` support, and the same final-composite path sub-project 2 needs for headless output.
- **Blocking readback on capture frames:** captures are rare dev events; a one-frame hitch is acceptable and keeps the code synchronous and deterministic. PNG encoding moves to a thread because it is the slow part.
- **Presented-frame numbering:** skipped frames do not shift what "frame N" means.
- **Env vars over CLI flags:** they work unchanged through `./run`, `ryo2d dev`, `ryo2d run` and `cargo run`, because the game process inherits the environment.
- **Release builds honour env vars:** capture is opt-in and costs nothing when unset; agents need to verify release rendering.

## Future Extensions (Not in Scope)

- Game-facing `ctx.capture_frame(path)` for player screenshots.
- `[capture]` config section in Ryo.toml.
- Frame sequences / video export.
- Headless rendering and `HeadlessEngine` (sub-project 2).
