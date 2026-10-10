# Frame Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Save the final composed frame (scene + UI, game resolution) as a PNG, triggered by `RYO_CAPTURE_*` env vars, the F12 dev key, or the `debug:capture` console command.

**Architecture:** A new `src/core/capture/` module holds GPU-free pieces (env config, scheduling state machine, PNG writer) plus a window-independent GPU readback helper. `WgpuRenderer` gains one pending-capture slot and records an extra game-resolution pass (identity uniform, no letterbox) through a shared `draw_final_composite` helper. `Engine` owns a `CaptureState` that decides which presented frame to capture and when to quit.

**Tech Stack:** Rust 2024, wgpu 22, image 0.25 (PNG), thiserror, tempfile (dev).

**Spec:** `docs/plans/2026-10-10-frame-capture-design.md`

## Global Constraints

- 2-space indentation (`rustfmt.toml`: `tab_spaces = 2`), edition 2024. Format only the files you touch: `rustfmt --edition 2024 <files>` (the repo has unrelated formatting drift; do not run a workspace-wide `cargo fmt`).
- No `unwrap`/`expect` in library code (tests may use them).
- Env vars exactly: `RYO_CAPTURE_FRAMES`, `RYO_CAPTURE_DIR` (default `captures`), `RYO_CAPTURE_EXIT` (`1`/`true` case-insensitive; `0`/`false` off).
- Default file name exactly `frame-NNNNNN.png` (presented-frame number, zero-padded to 6).
- Log lines exactly `[capture] frame 60 → captures/frame-000060.png` (stdout) and `[capture] FAILED frame 60: <error>` (stderr), each also sent to the dev log.
- Console command name: `debug:capture [path]`.
- F12 is gated by `config.debug.dev_window_keys`, like F9/F10.
- No per-frame cost when no capture is pending.
- `cargo clippy --workspace --all-targets` must add no new warnings in touched code (pre-existing warnings exist elsewhere — compare file paths, not totals).
- Commits: `type: description` (+ bullets if needed), no co-author lines, no mention of AGENTS.md. The repo owner confirms each commit and message before it is made.

## Review Focus

1. A scheduled capture whose PNG write fails (unwritable `RYO_CAPTURE_DIR`) must be reported, must not crash, and must make `RYO_CAPTURE_EXIT` exit with code 1 → Task 4 `write_failure_marks_scheduled_failed`.
2. Writes still in flight when the window closes must complete before the process exits → Task 4 `written_capture_is_reported_and_exit_follows` (exercises `finish()` joining) + Task 7 `run()` calls `finish()`.
3. A non-8-bit surface format must produce `CaptureError::UnsupportedFormat`, not a panic → Task 1 e2e `readback_rejects_unsupported_format`.
4. The capture must include the UI overlay (console, HUD) composited exactly as on screen → Task 9 acceptance step "open the console, press F12, PNG shows the console".
5. Output must not depend on window size → Task 9 acceptance step "resize the window, press F12, PNG is still game resolution".

---

## File Map

| File | Change | Responsibility |
|------|--------|----------------|
| `src/core/error.rs` | Modify | `CaptureError`, `CaptureResult` |
| `src/core/mod.rs` | Modify | `pub mod capture;` |
| `src/core/capture/mod.rs` | Create | `CapturedFrame`, `CaptureTarget`, re-exports |
| `src/core/capture/readback.rs` | Create | `read_texture_rgba` + padding/swizzle helpers |
| `src/core/capture/config.rs` | Create | `AutoCaptureConfig` env parsing |
| `src/core/capture/writer.rs` | Create | `default_file_name`, `write_png`, `spawn_write` |
| `src/core/capture/state.rs` | Create | `CaptureState`, `CaptureReport` |
| `src/core/capture/AGENTS.md` | Create | Agent notes for the module |
| `src/core/graphics/renderer.rs` | Modify | `draw_final_composite`, capture pass, request/take API |
| `src/core/engine.rs` | Modify | `capture` field, env load, hooks, F12, exit, banner, console forwarding |
| `src/core/config/debug.rs` | Modify | Correct dev key docs (F9/F10/F12) |
| `src/core/console/commands.rs` | Modify | `CommandContext::capture_requested` |
| `src/core/console/builtins.rs` | Modify | `debug:capture` builtin |
| `tests/e2e/capture_readback.rs` | Create | GPU readback tests |
| `tests/e2e.rs` | Modify | Register the new e2e module |
| `src/cli/skills/ryo-cli.md`, `.claude/skills/ryo-cli/SKILL.md` | Modify | "Frame capture" section |
| `src/cli/skills/ryo-engine-dev.md`, `.claude/skills/ryo-engine-dev/SKILL.md` | Modify | "Frame capture" section |
| `docs/src/concepts/frame-capture.md`, `docs/src/SUMMARY.md` | Create/Modify | mdbook page |

---

### Task 1: CaptureError and GPU readback

**Files:**
- Modify: `src/core/error.rs` (add after the `InventoryResult` type alias, before `#[cfg(test)]`)
- Modify: `src/core/mod.rs` (add `pub mod capture;` next to `pub mod console;`)
- Create: `src/core/capture/mod.rs`, `src/core/capture/readback.rs`
- Create: `tests/e2e/capture_readback.rs`
- Modify: `tests/e2e.rs`

**Interfaces:**
- Produces: `ryo2d::core::error::{CaptureError, CaptureResult}`; `ryo2d::core::capture::{read_texture_rgba, CapturedFrame, CaptureTarget}`; `read_texture_rgba(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> CaptureResult<image::RgbaImage>`.

- [ ] **Step 1: Add the error type**

In `src/core/error.rs`, after `pub type InventoryResult<T> = Result<T, InventoryError>;`:

```rust
/// Errors that can occur while capturing a frame
#[derive(Error, Debug)]
pub enum CaptureError {
  /// The texture's format can't be converted to RGBA8
  #[error("Texture format {0:?} is not supported for capture")]
  UnsupportedFormat(wgpu::TextureFormat),

  /// Copying or mapping the texture on the GPU failed
  #[error("GPU readback failed: {0}")]
  Readback(String),

  /// The output directory could not be created
  #[error("Failed to create capture directory {path}: {source}")]
  CreateDir {
    path: PathBuf,
    source: std::io::Error,
  },

  /// Encoding or writing the PNG failed
  #[error("Failed to write {path}: {source}")]
  Write {
    path: PathBuf,
    source: image::ImageError,
  },
}

/// Result type for capture operations
pub type CaptureResult<T> = Result<T, CaptureError>;
```

In the `#[cfg(test)] mod tests` of the same file, add:

```rust
  #[test]
  fn capture_error_display() {
    let err = CaptureError::UnsupportedFormat(wgpu::TextureFormat::R8Unorm);
    assert_eq!(
      format!("{}", err),
      "Texture format R8Unorm is not supported for capture"
    );
  }
```

- [ ] **Step 2: Write the failing unit tests for the readback helpers**

Create `src/core/capture/readback.rs` with only the tests for now:

```rust
//! GPU texture readback into tightly packed RGBA8 images.

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn padded_row_rounds_up_to_256() {
    assert_eq!(padded_bytes_per_row(64), 256);
    assert_eq!(padded_bytes_per_row(37), 256);
    assert_eq!(padded_bytes_per_row(65), 512);
  }

  #[test]
  fn strip_row_padding_keeps_only_pixel_bytes() {
    // 2x2 image, rows padded to 12 bytes (8 pixel bytes + 4 padding)
    let data = [
      1, 2, 3, 4, 5, 6, 7, 8, 0xEE, 0xEE, 0xEE, 0xEE, //
      9, 10, 11, 12, 13, 14, 15, 16, 0xEE, 0xEE, 0xEE, 0xEE,
    ];
    let out = strip_row_padding(&data, 2, 2, 12);
    assert_eq!(out, (1..=16).collect::<Vec<u8>>());
  }

  #[test]
  fn bgra_to_rgba_swaps_red_and_blue() {
    let mut px = [10, 20, 30, 40, 1, 2, 3, 4];
    bgra_to_rgba_in_place(&mut px);
    assert_eq!(px, [30, 20, 10, 40, 3, 2, 1, 4]);
  }
}
```

Create `src/core/capture/mod.rs`:

```rust
//! Frame capture: save the final composed frame (scene + UI) as a PNG.
//!
//! Captures are taken at game resolution (no letterboxing) and triggered by
//! `RYO_CAPTURE_*` environment variables, the F12 dev key, or the
//! `debug:capture` console command.

mod readback;

use std::path::PathBuf;

use image::RgbaImage;

pub use readback::read_texture_rgba;

/// A frame read back from the GPU, ready to be written.
#[derive(Debug)]
pub struct CapturedFrame {
  /// Pixels at game resolution
  pub image: RgbaImage,
  /// Destination requested for this frame
  pub path: PathBuf,
}

/// Where a manual capture should be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTarget {
  /// `<capture dir>/frame-NNNNNN.png`
  Default,
  /// An explicit path; relative paths resolve against the working directory
  Path(PathBuf),
}
```

Add `pub mod capture;` to `src/core/mod.rs` on the line after `pub mod console;`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib core::capture`
Expected: compile errors `cannot find function padded_bytes_per_row` / `strip_row_padding` / `bgra_to_rgba_in_place` (and `read_texture_rgba` unresolved in `mod.rs`).

- [ ] **Step 4: Implement the readback module**

Insert above the `#[cfg(test)]` block in `src/core/capture/readback.rs`:

```rust
use image::RgbaImage;

use crate::core::error::{CaptureError, CaptureResult};

/// Bytes per row padded to wgpu's copy alignment (256).
pub fn padded_bytes_per_row(width: u32) -> u32 {
  let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
  (width * 4).div_ceil(align) * align
}

/// Remove per-row padding from a buffer laid out with `padded_row` bytes per row.
pub fn strip_row_padding(data: &[u8], width: u32, height: u32, padded_row: u32) -> Vec<u8> {
  let row = (width * 4) as usize;
  let padded_row = padded_row as usize;
  let mut out = Vec::with_capacity(row * height as usize);
  for y in 0..height as usize {
    let start = y * padded_row;
    out.extend_from_slice(&data[start..start + row]);
  }
  out
}

/// Swap the red and blue channels of BGRA8 pixels in place.
pub fn bgra_to_rgba_in_place(pixels: &mut [u8]) {
  for px in pixels.chunks_exact_mut(4) {
    px.swap(0, 2);
  }
}

/// Read a texture back to the CPU as an RGBA8 image.
///
/// Blocks until the GPU has finished the copy. The texture needs `COPY_SRC`
/// usage and an 8-bit RGBA or BGRA format; BGRA is converted to RGBA.
pub fn read_texture_rgba(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  texture: &wgpu::Texture,
) -> CaptureResult<RgbaImage> {
  let swizzle = match texture.format() {
    wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => false,
    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => true,
    other => return Err(CaptureError::UnsupportedFormat(other)),
  };

  let width = texture.width();
  let height = texture.height();
  let padded_row = padded_bytes_per_row(width);

  let buffer = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("Capture Readback Buffer"),
    size: u64::from(padded_row) * u64::from(height),
    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
    mapped_at_creation: false,
  });

  let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
    label: Some("Capture Readback Encoder"),
  });
  encoder.copy_texture_to_buffer(
    wgpu::ImageCopyTexture {
      texture,
      mip_level: 0,
      origin: wgpu::Origin3d::ZERO,
      aspect: wgpu::TextureAspect::All,
    },
    wgpu::ImageCopyBuffer {
      buffer: &buffer,
      layout: wgpu::ImageDataLayout {
        offset: 0,
        bytes_per_row: Some(padded_row),
        rows_per_image: Some(height),
      },
    },
    wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
  );
  queue.submit(std::iter::once(encoder.finish()));

  let slice = buffer.slice(..);
  let (tx, rx) = std::sync::mpsc::channel();
  slice.map_async(wgpu::MapMode::Read, move |result| {
    let _ = tx.send(result);
  });
  device.poll(wgpu::Maintain::Wait);
  rx.recv()
    .map_err(|e| CaptureError::Readback(e.to_string()))?
    .map_err(|e| CaptureError::Readback(e.to_string()))?;

  let mut pixels = {
    let mapped = slice.get_mapped_range();
    strip_row_padding(&mapped, width, height, padded_row)
  };
  buffer.unmap();

  if swizzle {
    bgra_to_rgba_in_place(&mut pixels);
  }

  RgbaImage::from_raw(width, height, pixels)
    .ok_or_else(|| CaptureError::Readback("pixel buffer size mismatch".to_string()))
}
```

- [ ] **Step 5: Run the unit tests**

Run: `cargo test --lib core::capture` and `cargo test --lib core::error`
Expected: PASS (3 readback tests, `capture_error_display`).

- [ ] **Step 6: Write the GPU e2e tests**

Create `tests/e2e/capture_readback.rs`:

```rust
//! E2E tests for frame capture GPU readback.

use crate::common::gpu::TestGpu;
use ryo2d::core::capture::read_texture_rgba;
use ryo2d::core::error::CaptureError;

fn upload_texture(
  gpu: &TestGpu,
  format: wgpu::TextureFormat,
  bytes_per_pixel: u32,
  width: u32,
  height: u32,
  bytes: &[u8],
) -> wgpu::Texture {
  let size = wgpu::Extent3d {
    width,
    height,
    depth_or_array_layers: 1,
  };
  let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Capture Test Texture"),
    size,
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format,
    usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
    view_formats: &[],
  });
  gpu.queue.write_texture(
    wgpu::ImageCopyTexture {
      texture: &texture,
      mip_level: 0,
      origin: wgpu::Origin3d::ZERO,
      aspect: wgpu::TextureAspect::All,
    },
    bytes,
    wgpu::ImageDataLayout {
      offset: 0,
      bytes_per_row: Some(width * bytes_per_pixel),
      rows_per_image: Some(height),
    },
    size,
  );
  texture
}

/// Each pixel's channels encode its position so row/column mix-ups show up.
fn pattern(width: u32, height: u32) -> Vec<u8> {
  let mut bytes = Vec::with_capacity((width * height * 4) as usize);
  for y in 0..height {
    for x in 0..width {
      bytes.extend_from_slice(&[x as u8, y as u8, (x + y) as u8, 255]);
    }
  }
  bytes
}

#[test]
fn readback_rgba_texture_with_row_padding() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping readback_rgba_texture_with_row_padding: no GPU available");
    return;
  };
  // 37 px wide = 148 bytes per row, padded to 256 for the copy
  let (width, height) = (37, 5);
  let bytes = pattern(width, height);
  let texture = upload_texture(gpu, wgpu::TextureFormat::Rgba8UnormSrgb, 4, width, height, &bytes);

  let image = read_texture_rgba(&gpu.device, &gpu.queue, &texture).unwrap();

  assert_eq!(image.dimensions(), (width, height));
  assert_eq!(image.into_raw(), bytes);
}

#[test]
fn readback_bgra_texture_is_swizzled_to_rgba() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping readback_bgra_texture_is_swizzled_to_rgba: no GPU available");
    return;
  };
  let (width, height) = (37, 5);
  let bgra = pattern(width, height);
  let texture = upload_texture(gpu, wgpu::TextureFormat::Bgra8UnormSrgb, 4, width, height, &bgra);

  let image = read_texture_rgba(&gpu.device, &gpu.queue, &texture).unwrap();

  let expected: Vec<u8> = bgra
    .chunks_exact(4)
    .flat_map(|px| [px[2], px[1], px[0], px[3]])
    .collect();
  assert_eq!(image.into_raw(), expected);
}

#[test]
fn readback_rejects_unsupported_format() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping readback_rejects_unsupported_format: no GPU available");
    return;
  };
  let texture = upload_texture(gpu, wgpu::TextureFormat::R8Unorm, 1, 4, 4, &[0u8; 16]);

  let result = read_texture_rgba(&gpu.device, &gpu.queue, &texture);

  assert!(matches!(
    result,
    Err(CaptureError::UnsupportedFormat(wgpu::TextureFormat::R8Unorm))
  ));
}
```

Register it in `tests/e2e.rs`, after the `gpu_basics` module:

```rust
#[path = "e2e/capture_readback.rs"]
mod capture_readback;
```

- [ ] **Step 7: Run the e2e tests**

Run: `cargo test --test e2e capture_readback`
Expected: 3 passed (or each prints "Skipping…" and passes when no GPU is available — report which happened).

- [ ] **Step 8: Format, lint, commit**

Run: `rustfmt --edition 2024 src/core/error.rs src/core/mod.rs src/core/capture/mod.rs src/core/capture/readback.rs tests/e2e/capture_readback.rs tests/e2e.rs`
Run: `cargo clippy --workspace --all-targets 2>&1 | grep -E "capture|error.rs"` → no output.

```bash
git add src/core/error.rs src/core/mod.rs src/core/capture tests/e2e/capture_readback.rs tests/e2e.rs
git commit -m "feat(capture): add GPU texture readback and CaptureError"
```

---

### Task 2: Auto-capture env config

**Files:**
- Create: `src/core/capture/config.rs`
- Modify: `src/core/capture/mod.rs`

**Interfaces:**
- Produces: `AutoCaptureConfig { frames: Vec<u64>, dir: PathBuf, exit_after: bool }` (derives `Debug, Clone, PartialEq, Eq`, `Default`); `AutoCaptureConfig::from_env() -> (AutoCaptureConfig, Vec<String>)`; `AutoCaptureConfig::parse(frames: Option<&str>, dir: Option<&str>, exit: Option<&str>) -> (AutoCaptureConfig, Vec<String>)`; `DEFAULT_CAPTURE_DIR: &str = "captures"`.

- [ ] **Step 1: Write the failing tests**

Create `src/core/capture/config.rs`:

```rust
//! `RYO_CAPTURE_*` environment configuration.

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn unset_is_disabled() {
    let (config, warnings) = AutoCaptureConfig::parse(None, None, None);
    assert!(config.frames.is_empty());
    assert_eq!(config.dir, PathBuf::from("captures"));
    assert!(!config.exit_after);
    assert!(warnings.is_empty());
  }

  #[test]
  fn frames_are_sorted_and_deduplicated() {
    let (config, warnings) = AutoCaptureConfig::parse(Some(" 120, 60,60 "), None, None);
    assert_eq!(config.frames, vec![60, 120]);
    assert!(warnings.is_empty());
  }

  #[test]
  fn invalid_frames_disable_auto_capture() {
    for raw in ["", "abc", "60,x", "0", "-5", "60,,120"] {
      let (config, warnings) = AutoCaptureConfig::parse(Some(raw), None, Some("1"));
      assert!(config.frames.is_empty(), "{raw:?} should be rejected");
      assert!(!config.exit_after, "{raw:?}: exit needs frames");
      assert_eq!(warnings.len(), 1, "{raw:?} should warn once");
    }
  }

  #[test]
  fn exit_accepts_one_and_true() {
    for raw in ["1", "true", "TRUE", " True "] {
      let (config, warnings) = AutoCaptureConfig::parse(Some("5"), None, Some(raw));
      assert!(config.exit_after, "{raw:?}");
      assert!(warnings.is_empty());
    }
  }

  #[test]
  fn exit_off_values_and_invalid_values() {
    for raw in ["0", "false", ""] {
      let (config, warnings) = AutoCaptureConfig::parse(Some("5"), None, Some(raw));
      assert!(!config.exit_after, "{raw:?}");
      assert!(warnings.is_empty(), "{raw:?}");
    }
    let (config, warnings) = AutoCaptureConfig::parse(Some("5"), None, Some("yes"));
    assert!(!config.exit_after);
    assert_eq!(warnings.len(), 1);
  }

  #[test]
  fn exit_without_frames_has_no_effect() {
    let (config, warnings) = AutoCaptureConfig::parse(None, None, Some("1"));
    assert!(!config.exit_after);
    assert!(warnings.is_empty());
  }

  #[test]
  fn custom_dir_applies_without_frames() {
    let (config, _) = AutoCaptureConfig::parse(None, Some(" shots "), None);
    assert_eq!(config.dir, PathBuf::from("shots"));
  }
}
```

In `src/core/capture/mod.rs` add `mod config;` above `mod readback;` and `pub use config::{AutoCaptureConfig, DEFAULT_CAPTURE_DIR};` above the readback re-export.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib core::capture::config`
Expected: compile errors (`AutoCaptureConfig` not found).

- [ ] **Step 3: Implement**

Insert above `#[cfg(test)]` in `config.rs`:

```rust
use std::path::PathBuf;

/// Default output directory for captures.
pub const DEFAULT_CAPTURE_DIR: &str = "captures";

/// Auto-capture settings read from `RYO_CAPTURE_*` environment variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoCaptureConfig {
  /// Presented-frame numbers to capture (1-based, sorted, no duplicates)
  pub frames: Vec<u64>,
  /// Output directory for default-named captures
  pub dir: PathBuf,
  /// Quit once the last scheduled capture is written
  pub exit_after: bool,
}

impl Default for AutoCaptureConfig {
  fn default() -> Self {
    Self {
      frames: Vec::new(),
      dir: PathBuf::from(DEFAULT_CAPTURE_DIR),
      exit_after: false,
    }
  }
}

impl AutoCaptureConfig {
  /// Read `RYO_CAPTURE_FRAMES`, `RYO_CAPTURE_DIR` and `RYO_CAPTURE_EXIT`.
  ///
  /// Returns the config and a warning for each invalid value.
  pub fn from_env() -> (Self, Vec<String>) {
    Self::parse(
      std::env::var("RYO_CAPTURE_FRAMES").ok().as_deref(),
      std::env::var("RYO_CAPTURE_DIR").ok().as_deref(),
      std::env::var("RYO_CAPTURE_EXIT").ok().as_deref(),
    )
  }

  /// Parse raw variable values (`None` means unset).
  pub fn parse(
    frames: Option<&str>,
    dir: Option<&str>,
    exit: Option<&str>,
  ) -> (Self, Vec<String>) {
    let mut config = Self::default();
    let mut warnings = Vec::new();

    if let Some(dir) = dir.map(str::trim).filter(|d| !d.is_empty()) {
      config.dir = PathBuf::from(dir);
    }

    if let Some(raw) = frames {
      match parse_frames(raw) {
        Some(list) => config.frames = list,
        None => warnings.push(format!(
          "RYO_CAPTURE_FRAMES={raw:?} is invalid (expected positive frame numbers like \"60,120\"); auto-capture disabled"
        )),
      }
    }

    if let Some(raw) = exit {
      match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => config.exit_after = true,
        "0" | "false" | "" => {}
        _ => warnings.push(format!(
          "RYO_CAPTURE_EXIT={raw:?} is invalid (expected 1/true or 0/false); ignoring"
        )),
      }
    }

    // Exiting only makes sense once scheduled captures exist
    if config.frames.is_empty() {
      config.exit_after = false;
    }

    (config, warnings)
  }
}

fn parse_frames(raw: &str) -> Option<Vec<u64>> {
  let mut frames = Vec::new();
  for part in raw.split(',') {
    let frame: u64 = part.trim().parse().ok()?;
    if frame == 0 {
      return None;
    }
    frames.push(frame);
  }
  frames.sort_unstable();
  frames.dedup();
  Some(frames)
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::capture::config`
Expected: 7 passed.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/core/capture/config.rs src/core/capture/mod.rs
git add src/core/capture
git commit -m "feat(capture): parse RYO_CAPTURE_* environment variables"
```

---

### Task 3: PNG writer

**Files:**
- Create: `src/core/capture/writer.rs`
- Modify: `src/core/capture/mod.rs`

**Interfaces:**
- Produces: `default_file_name(frame: u64) -> String`; `write_png(image: &RgbaImage, path: &Path) -> CaptureResult<()>`; `spawn_write(image: RgbaImage, path: PathBuf) -> JoinHandle<CaptureResult<PathBuf>>`.

- [ ] **Step 1: Write the failing tests**

Create `src/core/capture/writer.rs`:

```rust
//! PNG output for captured frames.

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_name_is_zero_padded() {
    assert_eq!(default_file_name(60), "frame-000060.png");
    assert_eq!(default_file_name(1_234_567), "frame-1234567.png");
  }

  #[test]
  fn write_png_round_trips_and_creates_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/out.png");
    let mut image = RgbaImage::new(3, 2);
    image.put_pixel(2, 1, image::Rgba([10, 20, 30, 40]));

    write_png(&image, &path).unwrap();

    let loaded = image::open(&path).unwrap().to_rgba8();
    assert_eq!(loaded, image);
  }

  #[test]
  fn write_png_reports_unwritable_directory() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("not-a-dir");
    std::fs::write(&blocker, b"x").unwrap();

    let err = write_png(&RgbaImage::new(1, 1), &blocker.join("out.png")).unwrap_err();

    assert!(matches!(err, CaptureError::CreateDir { .. }));
  }

  #[test]
  fn spawn_write_returns_written_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bg.png");

    let written = spawn_write(RgbaImage::new(2, 2), path.clone())
      .join()
      .unwrap()
      .unwrap();

    assert_eq!(written, path);
    assert!(path.exists());
  }
}
```

In `mod.rs` add `mod writer;` (after `mod readback;`) and `pub use writer::{default_file_name, write_png};`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib core::capture::writer`
Expected: compile errors (`default_file_name`, `write_png`, `spawn_write` not found).

- [ ] **Step 3: Implement**

Insert above `#[cfg(test)]` in `writer.rs`:

```rust
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use image::RgbaImage;

use crate::core::error::{CaptureError, CaptureResult};

/// Default file name for a capture of presented frame `frame`.
pub fn default_file_name(frame: u64) -> String {
  format!("frame-{frame:06}.png")
}

/// Encode `image` as PNG at `path`, creating missing parent directories.
///
/// Existing files are overwritten.
pub fn write_png(image: &RgbaImage, path: &Path) -> CaptureResult<()> {
  if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
    std::fs::create_dir_all(parent).map_err(|source| CaptureError::CreateDir {
      path: parent.to_path_buf(),
      source,
    })?;
  }
  image
    .save_with_format(path, image::ImageFormat::Png)
    .map_err(|source| CaptureError::Write {
      path: path.to_path_buf(),
      source,
    })
}

/// Write the PNG on a background thread so encoding never stalls a frame.
pub fn spawn_write(image: RgbaImage, path: PathBuf) -> JoinHandle<CaptureResult<PathBuf>> {
  std::thread::spawn(move || write_png(&image, &path).map(|()| path))
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::capture::writer`
Expected: 4 passed.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/core/capture/writer.rs src/core/capture/mod.rs
git add src/core/capture
git commit -m "feat(capture): add PNG writer"
```

---

### Task 4: Capture scheduling state

**Files:**
- Create: `src/core/capture/state.rs`
- Modify: `src/core/capture/mod.rs`

**Interfaces:**
- Consumes: `AutoCaptureConfig` (Task 2), `default_file_name`, `spawn_write` (Task 3), `CapturedFrame`, `CaptureTarget` (Task 1).
- Produces:
  - `CaptureState::new(config: AutoCaptureConfig) -> CaptureState`
  - `request(&mut self, target: CaptureTarget)`
  - `path_for_next_frame(&mut self) -> Option<PathBuf>` — call before rendering
  - `on_presented(&mut self, result: Option<CaptureResult<CapturedFrame>>) -> Vec<CaptureReport>` — call after a frame reached the screen
  - `poll_writes(&mut self) -> Vec<CaptureReport>`, `finish(&mut self) -> Vec<CaptureReport>`
  - `exit_requested(&self) -> bool`, `exit_with_failure(&self) -> bool`
  - `enum CaptureReport { Written { frame: u64, path: PathBuf }, Failed { frame: u64, error: String } }` (derives `Debug, PartialEq, Eq`)

- [ ] **Step 1: Write the failing tests**

Create `src/core/capture/state.rs`:

```rust
//! Engine-side capture scheduling: which presented frame to capture, where,
//! and when auto-capture has finished.

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::error::CaptureError;
  use image::RgbaImage;
  use std::path::Path;

  fn config(frames: &[u64], dir: &Path, exit_after: bool) -> AutoCaptureConfig {
    AutoCaptureConfig {
      frames: frames.to_vec(),
      dir: dir.to_path_buf(),
      exit_after,
    }
  }

  fn captured(path: PathBuf) -> Option<CaptureResult<CapturedFrame>> {
    Some(Ok(CapturedFrame {
      image: RgbaImage::new(2, 2),
      path,
    }))
  }

  #[test]
  fn nothing_due_without_schedule_or_request() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[], dir.path(), false));
    for _ in 0..3 {
      assert_eq!(state.path_for_next_frame(), None);
      assert!(state.on_presented(None).is_empty());
    }
    assert!(!state.exit_requested());
  }

  #[test]
  fn scheduled_frames_use_default_names() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[2], dir.path(), false));
    assert_eq!(state.path_for_next_frame(), None);
    state.on_presented(None);
    assert_eq!(
      state.path_for_next_frame(),
      Some(dir.path().join("frame-000002.png"))
    );
  }

  #[test]
  fn unpresented_frame_keeps_its_number() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[1], dir.path(), false));
    let first = state.path_for_next_frame();
    // Frame skipped (surface lost): on_presented is not called
    assert_eq!(state.path_for_next_frame(), first);
  }

  #[test]
  fn manual_request_is_used_once() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("shot.png");
    let mut state = CaptureState::new(config(&[], dir.path(), false));
    state.request(CaptureTarget::Path(target.clone()));

    assert_eq!(state.path_for_next_frame(), Some(target.clone()));
    state.on_presented(captured(target));
    assert_eq!(state.path_for_next_frame(), None);
    state.finish();
  }

  #[test]
  fn default_manual_request_uses_capture_dir() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[], dir.path(), false));
    state.on_presented(None);
    state.request(CaptureTarget::Default);
    assert_eq!(
      state.path_for_next_frame(),
      Some(dir.path().join("frame-000002.png"))
    );
  }

  #[test]
  fn scheduled_frame_takes_priority_over_manual() {
    let dir = tempfile::tempdir().unwrap();
    let manual = dir.path().join("manual.png");
    let scheduled = dir.path().join("frame-000001.png");
    let mut state = CaptureState::new(config(&[1], dir.path(), false));
    state.request(CaptureTarget::Path(manual.clone()));

    assert_eq!(state.path_for_next_frame(), Some(scheduled.clone()));
    state.on_presented(captured(scheduled));
    assert_eq!(state.path_for_next_frame(), Some(manual));
    state.finish();
  }

  #[test]
  fn written_capture_is_reported_and_exit_follows() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[1], dir.path(), true));
    let path = state.path_for_next_frame().unwrap();
    assert!(!state.exit_requested());

    assert!(state.on_presented(captured(path.clone())).is_empty());
    let reports = state.finish();

    assert_eq!(
      reports,
      vec![CaptureReport::Written {
        frame: 1,
        path: path.clone()
      }]
    );
    assert!(path.exists());
    assert!(state.exit_requested());
    assert!(!state.exit_with_failure());
  }

  #[test]
  fn readback_failure_on_scheduled_frame_fails_exit() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[1], dir.path(), true));
    state.path_for_next_frame();

    let reports = state.on_presented(Some(Err(CaptureError::Readback("lost device".into()))));

    assert!(matches!(
      reports.as_slice(),
      [CaptureReport::Failed { frame: 1, .. }]
    ));
    assert!(state.exit_requested());
    assert!(state.exit_with_failure());
  }

  #[test]
  fn missing_capture_result_counts_as_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[1], dir.path(), false));
    state.path_for_next_frame();

    let reports = state.on_presented(None);

    assert!(matches!(
      reports.as_slice(),
      [CaptureReport::Failed { frame: 1, .. }]
    ));
  }

  #[test]
  fn write_failure_marks_scheduled_failed() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("not-a-dir");
    std::fs::write(&blocker, b"x").unwrap();
    let mut state = CaptureState::new(config(&[1], &blocker, true));
    let path = state.path_for_next_frame().unwrap();

    state.on_presented(captured(path));
    let reports = state.finish();

    assert!(matches!(
      reports.as_slice(),
      [CaptureReport::Failed { frame: 1, .. }]
    ));
    assert!(state.exit_with_failure());
  }
}
```

In `mod.rs` add `mod state;` (after `mod readback;`) and `pub use state::{CaptureReport, CaptureState};`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib core::capture::state`
Expected: compile errors (`CaptureState` not found).

- [ ] **Step 3: Implement**

Insert above `#[cfg(test)]` in `state.rs`:

```rust
use std::path::PathBuf;
use std::thread::JoinHandle;

use super::config::AutoCaptureConfig;
use super::writer::{default_file_name, spawn_write};
use super::{CaptureTarget, CapturedFrame};
use crate::core::error::CaptureResult;

/// Outcome of a capture, for logging.
#[derive(Debug, PartialEq, Eq)]
pub enum CaptureReport {
  /// The PNG was written
  Written { frame: u64, path: PathBuf },
  /// Reading back or writing the frame failed
  Failed { frame: u64, error: String },
}

/// The capture requested for the upcoming frame.
struct Pending {
  frame: u64,
  scheduled: bool,
}

struct PendingWrite {
  frame: u64,
  scheduled: bool,
  handle: JoinHandle<CaptureResult<PathBuf>>,
}

/// Tracks presented frames and decides which ones to capture.
///
/// Call [`path_for_next_frame`](Self::path_for_next_frame) before rendering and
/// [`on_presented`](Self::on_presented) once the frame reached the screen.
/// Frames that are never presented do not advance the counter, so "frame N"
/// is always the Nth presented frame.
pub struct CaptureState {
  config: AutoCaptureConfig,
  presented: u64,
  next_scheduled: usize,
  manual: Option<CaptureTarget>,
  pending: Option<Pending>,
  writes: Vec<PendingWrite>,
  scheduled_failed: bool,
}

impl CaptureState {
  /// Create a state with the given auto-capture schedule.
  pub fn new(config: AutoCaptureConfig) -> Self {
    Self {
      config,
      presented: 0,
      next_scheduled: 0,
      manual: None,
      pending: None,
      writes: Vec::new(),
      scheduled_failed: false,
    }
  }

  /// Request a capture of the next presented frame (F12, console).
  pub fn request(&mut self, target: CaptureTarget) {
    self.manual = Some(target);
  }

  /// Where to capture the upcoming frame, if a capture is due.
  ///
  /// A scheduled frame takes priority; a manual request then waits one frame.
  pub fn path_for_next_frame(&mut self) -> Option<PathBuf> {
    let frame = self.presented + 1;
    self.pending = None;

    if self.config.frames.get(self.next_scheduled) == Some(&frame) {
      self.pending = Some(Pending {
        frame,
        scheduled: true,
      });
      return Some(self.config.dir.join(default_file_name(frame)));
    }

    let path = match self.manual.as_ref()? {
      CaptureTarget::Default => self.config.dir.join(default_file_name(frame)),
      CaptureTarget::Path(path) => path.clone(),
    };
    self.pending = Some(Pending {
      frame,
      scheduled: false,
    });
    Some(path)
  }

  /// Record that a frame reached the screen, with the renderer's capture result.
  ///
  /// Successful captures are written on a background thread and reported by
  /// [`poll_writes`](Self::poll_writes); immediate failures are returned here.
  pub fn on_presented(
    &mut self,
    result: Option<CaptureResult<CapturedFrame>>,
  ) -> Vec<CaptureReport> {
    self.presented += 1;
    let Some(pending) = self.pending.take() else {
      return Vec::new();
    };

    if pending.scheduled {
      self.next_scheduled += 1;
    } else {
      self.manual = None;
    }

    match result {
      Some(Ok(captured)) => {
        self.writes.push(PendingWrite {
          frame: pending.frame,
          scheduled: pending.scheduled,
          handle: spawn_write(captured.image, captured.path),
        });
        Vec::new()
      }
      Some(Err(error)) => vec![self.failed(pending.frame, pending.scheduled, error.to_string())],
      None => vec![self.failed(
        pending.frame,
        pending.scheduled,
        "renderer produced no capture".to_string(),
      )],
    }
  }

  /// Collect background writes that have finished.
  pub fn poll_writes(&mut self) -> Vec<CaptureReport> {
    self.collect_writes(false)
  }

  /// Wait for every in-flight write (call before the process exits).
  pub fn finish(&mut self) -> Vec<CaptureReport> {
    self.collect_writes(true)
  }

  /// True once `RYO_CAPTURE_EXIT` is set and every scheduled capture is done.
  pub fn exit_requested(&self) -> bool {
    self.config.exit_after
      && self.next_scheduled >= self.config.frames.len()
      && self.writes.is_empty()
  }

  /// True if `RYO_CAPTURE_EXIT` is set and a scheduled capture failed.
  pub fn exit_with_failure(&self) -> bool {
    self.config.exit_after && self.scheduled_failed
  }

  fn collect_writes(&mut self, wait: bool) -> Vec<CaptureReport> {
    let mut reports = Vec::new();
    let mut still_running = Vec::new();
    for write in std::mem::take(&mut self.writes) {
      if !wait && !write.handle.is_finished() {
        still_running.push(write);
        continue;
      }
      let report = match write.handle.join() {
        Ok(Ok(path)) => CaptureReport::Written {
          frame: write.frame,
          path,
        },
        Ok(Err(error)) => self.failed(write.frame, write.scheduled, error.to_string()),
        Err(_) => self.failed(
          write.frame,
          write.scheduled,
          "capture writer thread panicked".to_string(),
        ),
      };
      reports.push(report);
    }
    self.writes = still_running;
    reports
  }

  fn failed(&mut self, frame: u64, scheduled: bool, error: String) -> CaptureReport {
    if scheduled {
      self.scheduled_failed = true;
    }
    CaptureReport::Failed { frame, error }
  }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::capture`
Expected: all capture tests pass (3 readback + 7 config + 4 writer + 10 state).

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/core/capture/state.rs src/core/capture/mod.rs
git add src/core/capture
git commit -m "feat(capture): add capture scheduling state"
```

---

### Task 5: Extract `draw_final_composite` in the renderer

Behaviour-preserving refactor that the capture pass needs. `WgpuRenderer` needs a window surface, so there is no automated test for the surface pass; verification is the full suite + Task 9's visual acceptance (captures go through the same helper).

**Files:**
- Modify: `src/core/graphics/renderer.rs` — the three `// Final render pass to screen` blocks in `render_with_gpu_sprites` (~line 1014), `render_with_lighting` (~line 1416) and `render_raycast` (~line 1660), plus two new private methods.

**Interfaces:**
- Produces (private): `fn draw_final_composite(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, source: &wgpu::TextureView, has_ui_overlay: bool, uniform: &wgpu::Buffer)` and `fn composite_bind_group(&self, label: &str, view: &wgpu::TextureView, uniform: &wgpu::Buffer) -> wgpu::BindGroup`.

- [ ] **Step 1: Add the helpers**

Add to `impl WgpuRenderer` (directly above `/// Set the window mode.`):

```rust
  /// Draw the final frame (scene source + optional UI overlay) into `target`.
  ///
  /// `uniform` selects the viewport transform: the letterboxing uniform for the
  /// window surface, or the identity uniform for game-resolution captures.
  fn draw_final_composite(
    &self,
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    source: &wgpu::TextureView,
    has_ui_overlay: bool,
    uniform: &wgpu::Buffer,
  ) {
    let source_bind_group = self.composite_bind_group("Final Source Bind Group", source, uniform);
    let ui_bind_group = has_ui_overlay
      .then(|| self.composite_bind_group("UI Overlay Bind Group", &self.ui_texture_view, uniform));

    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("Final Composite Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: target,
        resolve_target: None,
        ops: wgpu::Operations {
          // Clear to black (letterbox color)
          load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      occlusion_query_set: None,
      timestamp_writes: None,
    });

    render_pass.set_pipeline(&self.render_pipeline);
    render_pass.set_bind_group(0, &source_bind_group, &[]);
    render_pass.draw(0..6, 0..1);

    if let Some(ui_bind_group) = &ui_bind_group {
      render_pass.set_pipeline(&self.blend_pipeline);
      render_pass.set_bind_group(0, ui_bind_group, &[]);
      render_pass.draw(0..6, 0..1);
    }
  }

  /// Bind group sampling `view` with the given viewport uniform.
  fn composite_bind_group(
    &self,
    label: &str,
    view: &wgpu::TextureView,
    uniform: &wgpu::Buffer,
  ) -> wgpu::BindGroup {
    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some(label),
      layout: &self.bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&self.sampler),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: uniform.as_entire_binding(),
        },
      ],
    })
  }
```

- [ ] **Step 2: Replace the final pass in `render_with_gpu_sprites`**

Replace everything from `    // Final render pass to screen` through the closing `    }` of that block (the line before `    // Submit commands`) with:

```rust
    // Final render pass to screen
    let source_view = match use_post_process {
      Some(output_buffer) => self
        .post_process
        .as_ref()
        .map(|pp| pp.get_output_view(output_buffer))
        .unwrap_or(&self.texture_view),
      None => &self.texture_view,
    };
    self.draw_final_composite(
      &mut encoder,
      &view,
      source_view,
      has_ui_overlay,
      &self.uniform_buffer,
    );
```

- [ ] **Step 3: Replace the final pass in `render_with_lighting`**

Same block boundaries; replacement:

```rust
    // Final render pass to screen
    let source_view = match (use_post_process, lit_texture_view.as_ref()) {
      (Some(output_buffer), _) => self
        .post_process
        .as_ref()
        .map(|pp| pp.get_output_view(output_buffer))
        .unwrap_or(&self.texture_view),
      (None, Some(lit_view)) => lit_view,
      (None, None) => &self.texture_view,
    };
    self.draw_final_composite(
      &mut encoder,
      &view,
      source_view,
      has_ui_overlay,
      &self.uniform_buffer,
    );
```

(`lit_texture_view` is `Some` exactly when `lighting_applied`, so this matches the old `else if lighting_applied` branch. `lighting_applied` is still read by the post-process branch above.)

- [ ] **Step 4: Replace the final pass in `render_raycast`**

Use the Step 2 replacement verbatim.

- [ ] **Step 5: Verify nothing else changed behaviour**

Run: `cargo check --workspace` → no errors.
Run: `cargo clippy --workspace --all-targets 2>&1 | grep -A5 "renderer.rs"` → no new warnings.
Run: `cargo test --workspace` → same pass counts as before (0 failed).
Run: `grep -n "Final render pass to screen" -A3 src/core/graphics/renderer.rs` → three hits, each followed by `let source_view`.

- [ ] **Step 6: Format, commit**

```bash
rustfmt --edition 2024 src/core/graphics/renderer.rs
git add src/core/graphics/renderer.rs
git commit -m "refactor(renderer): share the final composite pass across render paths"
```

---

### Task 6: Renderer capture pass

**Files:**
- Modify: `src/core/graphics/renderer.rs`

**Interfaces:**
- Consumes: `read_texture_rgba`, `CapturedFrame` (Task 1), `draw_final_composite` (Task 5).
- Produces: `WgpuRenderer::request_capture(&mut self, path: PathBuf)`, `WgpuRenderer::take_capture_result(&mut self) -> Option<CaptureResult<CapturedFrame>>`.

- [ ] **Step 1: Imports and fields**

Add imports at the top of `renderer.rs`:

```rust
use std::path::PathBuf;

use crate::core::capture::{CapturedFrame, read_texture_rgba};
use crate::core::error::{CaptureError, CaptureResult};
```

Add fields at the end of `struct WgpuRenderer` (after `presentation_blocks: bool,`):

```rust
  // Identity viewport uniform (no letterboxing) for game-resolution passes
  identity_uniform_buffer: wgpu::Buffer,
  // Offscreen target for frame capture, created on first capture
  capture_texture: Option<wgpu::Texture>,
  // Destination for the capture requested for the next presented frame
  pending_capture: Option<PathBuf>,
  // Result of the latest capture, collected by the engine
  capture_result: Option<CaptureResult<CapturedFrame>>,
```

In `new()`, the local `identity_uniform_buffer` already exists (used for `cpu_bind_group`). Add to the `Self { ... }` literal:

```rust
      identity_uniform_buffer,
      capture_texture: None,
      pending_capture: None,
      capture_result: None,
```

- [ ] **Step 2: Capture API and helpers**

Add to `impl WgpuRenderer` (next to `draw_final_composite`):

```rust
  /// Capture the next presented frame to `path` at game resolution.
  ///
  /// Replaces any pending request. Collect the outcome with
  /// [`take_capture_result`](Self::take_capture_result) after rendering.
  pub fn request_capture(&mut self, path: PathBuf) {
    self.pending_capture = Some(path);
  }

  /// Take the result of the latest capture, if one ran since the last call.
  pub fn take_capture_result(&mut self) -> Option<CaptureResult<CapturedFrame>> {
    self.capture_result.take()
  }

  /// Take the pending request and make sure the capture texture matches the game resolution.
  fn prepare_capture(&mut self) -> Option<PathBuf> {
    let path = self.pending_capture.take()?;
    let up_to_date = self
      .capture_texture
      .as_ref()
      .is_some_and(|t| t.width() == self.game_width && t.height() == self.game_height);
    if !up_to_date {
      self.capture_texture = Some(self.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Frame Capture Texture"),
        size: wgpu::Extent3d {
          width: self.game_width,
          height: self.game_height,
          depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Must match the surface format: the final-composite pipelines target it
        format: self.config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
      }));
    }
    Some(path)
  }

  /// Draw the final composite into the capture texture (no letterbox).
  fn encode_capture_pass(
    &self,
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::TextureView,
    has_ui_overlay: bool,
  ) {
    if let Some(texture) = &self.capture_texture {
      let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
      self.draw_final_composite(
        encoder,
        &view,
        source,
        has_ui_overlay,
        &self.identity_uniform_buffer,
      );
    }
  }

  /// Read the capture texture back once the frame's commands were submitted.
  fn finish_capture(&mut self, path: Option<PathBuf>) {
    let Some(path) = path else {
      return;
    };
    let result = match &self.capture_texture {
      Some(texture) => read_texture_rgba(&self.device, &self.queue, texture)
        .map(|image| CapturedFrame { image, path }),
      None => Err(CaptureError::Readback("capture texture missing".to_string())),
    };
    self.capture_result = Some(result);
  }
```

- [ ] **Step 3: Wire it into the three render paths**

In each of `render_with_gpu_sprites`, `render_with_lighting` and `render_raycast`:

1. Directly after the `let view = output.texture.create_view(...)` statement, add:

```rust
    let capture_path = self.prepare_capture();
```

(Placed after `get_current_texture()?` so an early `SurfaceError` return never drops a request — the engine re-requests the same frame next time.)

2. Directly after the `self.draw_final_composite(...)` call added in Task 5, add:

```rust
    if capture_path.is_some() {
      self.encode_capture_pass(&mut encoder, source_view, has_ui_overlay);
    }
```

3. Directly after `output.present();`, add:

```rust
    self.finish_capture(capture_path);
```

- [ ] **Step 4: Drop the capture texture when the game resolution changes**

At the end of `resize_buffer` (before its closing brace), add:

```rust
    // Recreated at the new size on the next capture
    self.capture_texture = None;
```

- [ ] **Step 5: Verify**

Run: `cargo check --workspace` → no errors.
Run: `cargo clippy --workspace --all-targets 2>&1 | grep -A5 "renderer.rs"` → no new warnings.
Run: `cargo test --workspace` → 0 failed.
Run: `grep -c "self.prepare_capture()\|self.encode_capture_pass(\|self.finish_capture(" src/core/graphics/renderer.rs` → `9` (3 per path).

- [ ] **Step 6: Format, commit**

```bash
rustfmt --edition 2024 src/core/graphics/renderer.rs
git add src/core/graphics/renderer.rs
git commit -m "feat(renderer): capture the final frame at game resolution on request"
```

---

### Task 7: Engine wiring — env schedule, F12, reporting, exit

**Files:**
- Modify: `src/core/engine.rs`
- Modify: `src/core/config/debug.rs` (doc comments only)

**Interfaces:**
- Consumes: `AutoCaptureConfig::from_env`, `CaptureState`, `CaptureReport`, `CaptureTarget` (Tasks 1-4); `WgpuRenderer::request_capture`, `take_capture_result` (Task 6).
- Produces (private): `Engine::on_frame_presented(&mut self)`, `Engine::report_captures(&mut self, reports: Vec<CaptureReport>)`; field `Engine::capture: CaptureState` (used by Task 8).

- [ ] **Step 1: Import, field, construction**

Add with the other `use super::...` imports:

```rust
use super::capture::{AutoCaptureConfig, CaptureReport, CaptureState, CaptureTarget};
```

In `pub struct Engine`, after `console_quit_requested: bool,`:

```rust
  // Frame capture scheduling (RYO_CAPTURE_*, F12, debug:capture)
  capture: CaptureState,
```

In `Engine::new`, directly before the `Engine {` struct literal:

```rust
    let (capture_config, capture_warnings) = AutoCaptureConfig::from_env();
    for warning in &capture_warnings {
      eprintln!("[capture] {warning}");
      engine_warn!(module: "Capture", "{}", warning);
    }
```

and in the literal, after `console_quit_requested: false,`:

```rust
      capture: CaptureState::new(capture_config),
```

- [ ] **Step 2: Reporting and post-present helpers**

Add to `impl Engine` (directly above `fn do_full_render_and_present`):

```rust
  /// Advance the capture schedule after a frame reached the screen.
  fn on_frame_presented(&mut self) {
    let result = self
      .renderer
      .as_mut()
      .and_then(WgpuRenderer::take_capture_result);
    let reports = self.capture.on_presented(result);
    self.report_captures(reports);
  }

  /// Log capture outcomes to stdout/stderr, the dev log and the console.
  fn report_captures(&mut self, reports: Vec<CaptureReport>) {
    for report in reports {
      match report {
        CaptureReport::Written { frame, path } => {
          let message = format!("frame {frame} → {}", path.display());
          println!("[capture] {message}");
          engine_info!(module: "Capture", "{}", message);
          self.console_state.print_system(format!("Captured {message}"));
        }
        CaptureReport::Failed { frame, error } => {
          let message = format!("FAILED frame {frame}: {error}");
          eprintln!("[capture] {message}");
          engine_warn!(module: "Capture", "{}", message);
          self.console_state.print_error(format!("Capture {message}"));
        }
      }
    }
  }
```


- [ ] **Step 3: Hook `do_full_render_and_present`**

1. Replace

```rust
    if let (Some(rend), Some(gpu)) = (&mut renderer, &mut sprite_gpu) {
      // Collect trails and lines from the world (inside renderer block for device/queue access)
```

(the occurrence at ~line 990, 4-space indent) with

```rust
    let mut presented = false;
    if let (Some(rend), Some(gpu)) = (&mut renderer, &mut sprite_gpu) {
      if let Some(path) = self.capture.path_for_next_frame() {
        rend.request_capture(path);
      }

      // Collect trails and lines from the world (inside renderer block for device/queue access)
```

2. In the `match result {` of the same function, change `Ok(_) => {}` to `Ok(_) => presented = true,`.

3. At the end of the function, after the `// Track first frame` block, add:

```rust

    if presented {
      self.on_frame_presented();
    }
```

- [ ] **Step 4: Hook the `RedrawRequested` present path**

Same three edits on the second copy (~line 3090, 8-space indent):

1. Insert `let mut presented = false;` on the line before `if let (Some(rend), Some(gpu)) = (&mut renderer, &mut sprite_gpu) {`, and as the first statement inside that block:

```rust
          if let Some(path) = self.capture.path_for_next_frame() {
            rend.request_capture(path);
          }
```

2. In its `match result {` (the one whose `Err(e)` arm calls `event_loop.exit()`), change `Ok(_) => {}` to `Ok(_) => presented = true,`.

3. Directly after the block that starts `// Put sprite_gpu, renderer, lighting, trail_renderer, and raycast_renderer back` (after `self.raycast_renderer = raycast_renderer;`), add:

```rust
        if presented {
          self.on_frame_presented();
        }
```

- [ ] **Step 5: F12, exit, final flush, banner**

1. Inside `if self.config.debug.dev_window_keys {` (the F9/F10 block), after the F10 `if` block:

```rust

          // F12: Capture the next frame to a PNG
          if self.input.key_pressed(KeyCode::F12) {
            self.capture.request(CaptureTarget::Default);
            self.input.consume_key(KeyCode::F12);
          }
```

2. In `about_to_wait`, directly after `event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);`:

```rust

    // Report finished captures and honour RYO_CAPTURE_EXIT
    let reports = self.capture.poll_writes();
    self.report_captures(reports);
    if self.capture.exit_requested() {
      event_loop.exit();
      return;
    }
```

3. In `pub fn run`, replace

```rust
    event_loop
      .run_app(&mut self)
      .map_err(|e| EngineError::Window(format!("Event loop error: {}", e)))
```

with

```rust
    let result = event_loop
      .run_app(&mut self)
      .map_err(|e| EngineError::Window(format!("Event loop error: {}", e)));

    // Make sure in-flight captures are on disk before the process exits
    let reports = self.capture.finish();
    self.report_captures(reports);
    if self.capture.exit_with_failure() {
      std::process::exit(1);
    }

    result
```

4. In `print_startup_banner`, after the `println!("  F10    Profiler");` line:

```rust
    println!("  F12    Capture frame");
```

- [ ] **Step 6: Correct the dev key docs**

In `src/core/config/debug.rs` replace

```rust
//! **Dev Window Keys (F8-F9)** - Dev-only by default, requires `dev_window_keys = true` in release:
//!
//! | Key | Action |
//! |-----|--------|
//! | F8 | Toggle profiler window (requires `profiler` feature) |
//! | F9 | Toggle logger window |
//!
//! Set `disable_debug_keys = true` in `[debug]` to disable F1-F4 toggles.
//! Set `dev_window_keys = true` in `[debug]` to enable F8-F9 in release builds.
```

with

```rust
//! **Dev Keys (F9, F10, F12)** - Dev-only by default, requires `dev_window_keys = true` in release:
//!
//! | Key | Action |
//! |-----|--------|
//! | F9 | Toggle logger window |
//! | F10 | Toggle profiler window (requires `profiler` feature) |
//! | F12 | Capture the next frame to `captures/` (or `RYO_CAPTURE_DIR`) |
//!
//! Set `disable_debug_keys = true` in `[debug]` to disable F1-F4 toggles.
//! Set `dev_window_keys = true` in `[debug]` to enable F9, F10 and F12 in release builds.
```

and replace `  /// Whether to enable dev window hotkeys (F8=profiler, F9=logger).` with `  /// Whether to enable dev hotkeys (F9=logger, F10=profiler, F12=frame capture).`

- [ ] **Step 7: Verify**

Run: `cargo check --workspace` → no errors.
Run: `cargo clippy --workspace --all-targets 2>&1 | grep -B2 -A6 "engine.rs\|debug.rs"` → no warnings on lines you touched.
Run: `cargo test --workspace` → 0 failed.

- [ ] **Step 8: Format, commit**

```bash
rustfmt --edition 2024 src/core/engine.rs src/core/config/debug.rs
git add src/core/engine.rs src/core/config/debug.rs
git commit -m "feat(engine): schedule frame captures from RYO_CAPTURE_* and F12"
```

---

### Task 8: `debug:capture` console command

**Files:**
- Modify: `src/core/console/commands.rs` (struct `CommandContext`, test helper `make_test_context`)
- Modify: `src/core/console/builtins.rs` (new builtin, test helper, tests)
- Modify: `src/core/engine.rs` (`CommandContext` literal ~line 2910 and the code after `execute`)

**Interfaces:**
- Consumes: `CaptureTarget` (Task 1), `Engine::capture` (Task 7).
- Produces: `CommandContext::capture_requested: Option<CaptureTarget>`.

- [ ] **Step 1: Write the failing tests**

In `src/core/console/builtins.rs` `mod tests`, add (the module already has `use super::*;` and `use crate::core::config::DebugConfig;`):

```rust
  #[test]
  fn test_capture_command_uses_default_target() {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry);
    let mut console = super::super::ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    let result = registry.execute("debug:capture", &mut ctx);

    assert_eq!(result, CommandResult::Ok);
    assert_eq!(ctx.capture_requested, Some(CaptureTarget::Default));
  }

  #[test]
  fn test_capture_command_accepts_path_with_spaces() {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry);
    let mut console = super::super::ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    registry.execute("debug:capture shots/my shot.png", &mut ctx);

    assert_eq!(
      ctx.capture_requested,
      Some(CaptureTarget::Path(PathBuf::from("shots/my shot.png")))
    );
  }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib core::console`
Expected: compile errors (`capture_requested` field missing, `CaptureTarget`/`PathBuf` not in scope).

- [ ] **Step 3: Implement**

`src/core/console/commands.rs` — add import `use crate::core::capture::CaptureTarget;` and, in `pub struct CommandContext`, after `pub quit_requested: bool,`:

```rust
  /// Frame capture requested by `debug:capture` (handled by the engine)
  pub capture_requested: Option<CaptureTarget>,
```

Add `capture_requested: None,` after `quit_requested: false,` in `make_test_context` in both `commands.rs` and `builtins.rs`, and in the `CommandContext { ... }` literal in `src/core/engine.rs` (~line 2910).

`src/core/console/builtins.rs` — add imports at the top:

```rust
use std::path::PathBuf;

use crate::core::capture::CaptureTarget;
```

Register the command in the `debug:` section, after `debug:fps`:

```rust
  // Frame capture (F12 does the same with the default path)
  registry.register(
    "debug:capture",
    "Save the next frame as a PNG (debug:capture [path])",
    |args, ctx| {
      let target = if args.is_empty() {
        CaptureTarget::Default
      } else {
        CaptureTarget::Path(PathBuf::from(args.join(" ")))
      };
      ctx.capture_requested = Some(target);
      ctx.console.print("Capturing next frame...");
      CommandResult::Ok
    },
  );
```

`src/core/engine.rs` — after the existing block

```rust
            if matches!(result, CommandResult::Quit) || cmd_ctx.quit_requested {
              self.console_quit_requested = true;
            }
```

add

```rust
            if let Some(target) = cmd_ctx.capture_requested.take() {
              self.capture.request(target);
            }
```

(`cmd_ctx` borrows other `Engine` fields; `self.capture` is a disjoint field, so this compiles while `cmd_ctx` is alive.)

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::console` → all pass including the 2 new tests.
Run: `cargo test --workspace` → 0 failed.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/core/console/commands.rs src/core/console/builtins.rs src/core/engine.rs
cargo clippy --workspace --all-targets 2>&1 | grep -A5 "console/"
git add src/core/console/commands.rs src/core/console/builtins.rs src/core/engine.rs
git commit -m "feat(console): add debug:capture command"
```

---

### Task 9: Acceptance run

Manual end-to-end checks with a real window (the capture pass is not covered by automated tests until sub-project 2). A window will open briefly on the desktop. First set `CHECK` to an absolute, empty scratch directory (agents: the session scratchpad), e.g. `CHECK="$(mktemp -d)"`, and inspect PNGs with an image viewer (agents: the Read tool on the PNG).

**Files:** none changed (temporary demo edits are reverted).

- [ ] **Step 1: Auto-capture and exit**

```bash
cd examples/demos/trails
RYO_CAPTURE_FRAMES=60,180 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR="$CHECK/auto" \
  cargo run --manifest-path ../../../Cargo.toml --bin ryo2d -- dev --skip-opt-check
```

Expected:
- stdout contains `[capture] frame 60 → <CHECK>/auto/frame-000060.png` and the same for 180.
- The game exits on its own.
- Both PNGs are 1920×1080 (the demo's game resolution — check with `file "$CHECK"/auto/*.png`) and show the trails demo with no black letterbox bars.

- [ ] **Step 2: Failure path exit code**

```bash
touch "$CHECK/blocker"
RYO_CAPTURE_FRAMES=30 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR="$CHECK/blocker/out" \
  cargo run --manifest-path ../../../Cargo.toml --bin ryo2d -- dev --skip-opt-check; echo "exit=$?"
```

Expected: stderr `[capture] FAILED frame 30: Failed to create capture directory …`; the game exits; record the printed exit code. If `ryo2d dev` does not propagate the game's exit code, note it in the report (the game process itself exits 1 — confirm by running the built game binary directly from `target/` with the same env).

- [ ] **Step 3: Invalid env value**

`RYO_CAPTURE_FRAMES=abc` → one `[capture] RYO_CAPTURE_FRAMES="abc" is invalid…` warning on stderr; the game runs normally (close it manually).

- [ ] **Step 4: F12, console, UI overlay, window size** (Review Focus 4 and 5)

Run the demo without env vars. Then:
1. Press F12 → `captures/frame-NNNNNN.png` under `examples/demos/trails/`.
2. Open the console (backtick), run `debug:capture <CHECK>/console.png` → PNG shows the console overlay.
3. Resize the window to roughly half size, press F12 → PNG is still 1920×1080.

Delete `examples/demos/trails/captures/` afterwards.

- [ ] **Step 5: Red evidence for the trails-vs-lighting bug (sub-project 3)**

The working tree contains a temporary `Light2D` spawn in `examples/demos/trails/main.rs` (`on_enter`, marked `TEMP repro`). With it in place, repeat Step 1 with `RYO_CAPTURE_DIR="$CHECK/lit"`. Expected: lit scene, **no trails** (the known bug). Keep both PNG sets as before/after evidence for sub-project 3. Do not commit the temporary light.

- [ ] **Step 6: Report**

Summarise each step's result (pass/fail, exit codes, PNG dimensions). No commit for this task.

---

### Task 10: Documentation

**Files:**
- Create: `src/core/capture/AGENTS.md`
- Modify: `src/cli/skills/ryo-cli.md` and `.claude/skills/ryo-cli/SKILL.md` (keep both in sync)
- Modify: `src/cli/skills/ryo-engine-dev.md` and `.claude/skills/ryo-engine-dev/SKILL.md`
- Create: `docs/src/concepts/frame-capture.md`; Modify: `docs/src/SUMMARY.md`

- [ ] **Step 1: Module AGENTS.md**

Create `src/core/capture/AGENTS.md`:

```markdown
# AGENTS.md - Frame Capture

Saves the final composed frame (scene + UI) as a PNG at game resolution. Spec: `docs/plans/2026-10-10-frame-capture-design.md`.

## Split

- `CaptureState` (engine) decides *when* and *where*: presented-frame counter, `RYO_CAPTURE_*` schedule, F12/console requests, background PNG writes, exit.
- `WgpuRenderer` only knows one pending path: `request_capture(path)` before a render call, `take_capture_result()` after. The capture pass reuses `draw_final_composite` with the identity uniform (no letterbox).
- Keep scheduling out of the renderer so a headless harness can drive the same API.

## Gotchas

- `read_texture_rgba` blocks (`Maintain::Wait`) and needs `COPY_SRC` plus an 8-bit RGBA/BGRA format; anything else is `CaptureError::UnsupportedFormat`.
- The capture texture uses the **surface** format (the final-composite pipelines target it), so readback must handle BGRA.
- "Frame N" counts presented frames only; a frame lost to `SurfaceError` is retried with the same number.
- Both present paths in `engine.rs` (`do_full_render_and_present` and `RedrawRequested`) must call `path_for_next_frame` before rendering and `on_frame_presented` after `Ok`.
- With `RYO_CAPTURE_EXIT=1`, `run()` joins in-flight writes and exits with code 1 if a scheduled capture failed.
```

- [ ] **Step 2: Skills**

Append to both `ryo-cli` skill files:

```markdown
## Frame Capture

Save frames as PNGs to check rendering without watching the window. Works with `ryo2d dev`, `ryo2d run`, `./run` and `cargo run` (the game inherits the environment):

| Variable | Meaning |
|----------|---------|
| `RYO_CAPTURE_FRAMES=60,120` | Presented-frame numbers to capture |
| `RYO_CAPTURE_DIR=captures` | Output directory (default `captures/`) |
| `RYO_CAPTURE_EXIT=1` | Quit after the last capture; exit code 1 if one failed |

Files are named `frame-000060.png` at game resolution (no letterboxing). In dev builds, F12 or the console command `debug:capture [path]` saves the next frame.

    RYO_CAPTURE_FRAMES=120 RYO_CAPTURE_EXIT=1 ryo2d dev
```

Append to both `ryo-engine-dev` skill files:

```markdown
## Verifying Rendering Changes

Use frame capture (`src/core/capture/`) instead of guessing: run an example with `RYO_CAPTURE_FRAMES=<n> RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=<abs dir>` and inspect the PNG. Capture before and after a renderer change to prove it.
```

- [ ] **Step 3: mdbook page**

Create `docs/src/concepts/frame-capture.md`:

```markdown
# Frame Capture

Ryo2D can save the final frame — scene, lighting, post-processing and UI — as a PNG at your game's resolution. It is meant for checking rendering (by you, CI or an AI agent) without watching the window.

## Automatic captures

Set environment variables when launching the game:

| Variable | Meaning | Default |
|----------|---------|---------|
| `RYO_CAPTURE_FRAMES` | Comma-separated presented-frame numbers, e.g. `60,120` | off |
| `RYO_CAPTURE_DIR` | Output directory, relative to the working directory | `captures` |
| `RYO_CAPTURE_EXIT` | `1`/`true`: quit after the last capture is written | off |

    RYO_CAPTURE_FRAMES=60,120 RYO_CAPTURE_EXIT=1 ryo2d dev

Files are named `frame-000060.png`. "Frame 60" is the 60th frame actually shown; frames skipped while the window is minimised don't count. With `RYO_CAPTURE_EXIT=1` the process exits with code 1 if a capture failed.

Invalid values print a warning and are ignored; the game still runs.

## Manual captures

- **F12** (dev builds, or `dev_window_keys = true` in `[debug]`) saves the next frame to the capture directory.
- The console command `debug:capture [path]` does the same, optionally to a specific path.

Captures never include letterbox bars and are the same size regardless of window size.
```

In `docs/src/SUMMARY.md`, after `- [Debugging](./concepts/debugging.md)` add `- [Frame Capture](./concepts/frame-capture.md)`.

- [ ] **Step 4: Verify docs build**

Run: `mdbook build docs` (if `mdbook` is installed; otherwise note it was skipped).
Expected: no broken-link errors for the new page.

- [ ] **Step 5: Commit**

```bash
git add src/core/capture/AGENTS.md src/cli/skills/ryo-cli.md src/cli/skills/ryo-engine-dev.md .claude/skills/ryo-cli/SKILL.md .claude/skills/ryo-engine-dev/SKILL.md docs/src/concepts/frame-capture.md docs/src/SUMMARY.md
git commit -m "docs: document frame capture"
```

(Auto mode may block commits that touch agent instruction files; if so, hand the commit command to the repo owner.)
