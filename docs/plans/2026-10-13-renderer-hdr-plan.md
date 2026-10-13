# Renderer HDR (2c) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render the world in linear HDR (`Rgba16Float`), turn it into the display image with a tonemap pass that applies the frame's `Look` (exposure, auto-exposure, curve, grading, LUT), split effects into an HDR and a display stage, make every `Color` mean sRGB, let drawables glow above white, and draw lights through the 2D batch with shadows that darken only their own light.

**Architecture:** In place, in the spec's migration order, with every task green. The images the world draws into (scene, lit, camera images, render targets, the lighting copy) become `Rgba16Float`. A new `tonemap` pass writes the display image, the first buffer of the display effect stage's `Rgba8UnormSrgb` ping-pong pair; the transition, display effects, UI and composite then run on it as today. Effects run in two `PostProcessPipeline` instances, one per stage. Lights become quads in the batch's quad stream and shadow volumes become triangles in its triangle stream; they are drawn per lit camera into its light buffer, whose alpha holds each shadowed light's mask. `LightingRenderer` keeps the per-camera targets and the compose pass.

**Tech Stack:** Rust 2024, `wgpu`/`naga` 30.0.1 (unchanged), `bevy_ecs` 0.20 behind `ryo2d::ecs`, `glam`, WGSL (render and compute). No new crates.

**Spec:** `docs/specs/2026-10-13-renderer-hdr-design.md`

## Global Constraints

- Work on branch `renderer-hdr` in the worktree `.claude/worktrees/renderer-hdr`. `main` stays green until the final merge, which the owner approves.
- `wgpu` and `naga` stay at `30.0.1`, and no new crates are added. The CPU never writes f16 texels in library code: float images are filled by render passes, and tests that need f16 texels build the bits by hand.
- Use `Limits::default()` and no optional wgpu features. In core WebGPU, `rgba16float` is renderable, blendable and filterable. Compute shaders use storage buffers and atomics only. Every shader stays WebGPU-valid: derivatives only in uniform control flow, no `f16` in WGSL.
- The scene format is one constant, `crate::core::render::SCENE_FORMAT` (`Rgba16Float`), and the display stage stays `Rgba8UnormSrgb`. Unchanged: atlas and standalone pages (`Rgba8Unorm`, raw sRGB bytes), the UI texture, the surface, the offscreen target and the capture readback.
- Every shader writes colour premultiplied by alpha, as in 2b.
- After Task 3, every `Color` that reaches the GPU as a colour goes through `Color::to_linear`. The two exceptions are sRGB by design: sprite tints and sprite-effect colours, which the sprite shader applies in sRGB space.
- No `unwrap`/`expect` in library code outside tests. LF line endings; two-space indentation (`rustfmt.toml`).
- Game code never names `bevy_ecs`; only `Cargo.toml`, `src/core/ecs/kernel.rs` and the hidden prelude re-export do. Networking is not changed.
- Breaking public APIs is allowed. Demos, games, tests and docs are updated in the task that breaks them.
- Commit messages: `type: brief description` plus bullets if needed; no co-author lines; no mention of AGENTS.md files.
- **The gate** (every task ends with it):

  ```bash
  cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{p+=$4; f+=$6} END {print p, f}'
  ./dev.sh lint 2>&1 | grep -E '^(warning|error)' | grep -v 'future version' | head -3
  ./dev.sh demos:tour >/dev/null && target/hdr-baseline/compare.sh; echo "compare $?"
  ```

  Expected:
  - the failed count is `0`;
  - there are no lint lines (the wasm clippy included);
  - `compare 0`, or only the pages the task names.

  `compare.sh` passes a page whose largest channel difference is at most `TOL` levels (default 1). Float images drop the 8-bit rounding between draws, so translucent overlaps may move by one level.

  When a task names pages that change:
  1. Check each listed page by eye.
  2. Run `target/hdr-baseline/refresh.sh`, so the next task compares against the accepted look.
  3. Record the list in the ledger.

## Review Focus

- **Resizing the window while everything is on.** Bloom (HDR chain), a vignette (display chain), a LUT and auto-exposure are all active when the window is resized. Every bind group that points at a scene, HDR or display image (tonemap, both chains, exposure) must be rebuilt once. None may point at a destroyed texture (a validation error), and steady frames must create none. Test pinned to Task 11, Step 1.
- **Non-finite HDR values.** A pile-up past f16's range (emission `f32::INFINITY`, or thousands of additive sprites) must show white where it is. It must never turn into NaN or black elsewhere: not through the tonemap, not through bloom, and auto-exposure must ignore it. Tests pinned to Task 8 (tonemap and bloom) and Task 11 (exposure).
- **Degenerate look parameters.** A `Soft` knee ≥ 1 or negative, `white` ≤ `knee`, NaN exposure, gamma 0, or NaN in any grading field is sanitized on the CPU. The frame stays finite and within 0..1. Tests pinned to Task 4 (curve parameters) and Task 5 (grading uniforms).
- **A LUT of the wrong size.** `ColorLut::from_sprite` errors on a wrong-size strip. A LUT sprite hot-reloaded to a wrong size keeps the last good LUT, warns once and never panics. Test pinned to Task 5.
- **Degenerate lights.** A light with radius 0 or NaN, an intensity that is negative, NaN or infinite, or a NaN position draws nothing. It must never put NaN in the light buffer, which compose and bloom would spread over the frame. Test pinned to Task 9.

---

## Phase 0: branch and baseline

### Task 0: Worktree, reference captures and the comparison script

**Files:** none in the repo. References go under `target/hdr-baseline/`, which git ignores.

- [ ] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/renderer-hdr -b renderer-hdr main
cd .claude/worktrees/renderer-hdr
```

All later tasks run inside this worktree.

- [ ] **Step 2: Baseline tests**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:' | awk '{p+=$4; f+=$6} END {print p, f}'`

Expected: failures `0`. Record the passed count in the ledger.

- [ ] **Step 3: Reference captures, the comparison script and the refresh script**

```bash
./dev.sh demos:tour
mkdir -p target/hdr-baseline
rm -rf target/hdr-baseline/demo-tour && cp -R target/demo-tour target/hdr-baseline/demo-tour
# The look before 2c, never refreshed: Tasks 13 and 15 compare against it
mkdir -p target/hdr-baseline-task0
rm -rf target/hdr-baseline-task0/demo-tour && cp -R target/demo-tour target/hdr-baseline-task0/demo-tour
for g in breakout bullet-hell flappy leap-king nigh_t; do
  (cd examples/games/$g && RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=60 RYO_CAPTURE_EXIT=1 \
    RYO_CAPTURE_DIR="$PWD/../../../target/hdr-baseline/games/$g" cargo run -q -p $g)
done
cat > target/hdr-baseline/compare.sh <<'EOF'
#!/usr/bin/env bash
# Lists every demo-tour PNG whose largest channel difference from the
# baseline is more than TOL levels (default 1). Particles and steering draw
# random numbers, so they are checked by eye
set -u
tol=${TOL:-1}
base=target/hdr-baseline/demo-tour
now=target/demo-tour
status=0
for f in $(cd "$base" && find . -name "*.png" | grep -v -e "^./particles/" -e "^./steering/" | sort); do
  if [ ! -f "$now/$f" ]; then echo "missing $f"; status=1; continue; fi
  cmp -s "$base/$f" "$now/$f" && continue
  out=$(magick compare -metric PAE "$base/$f" "$now/$f" null: 2>&1)
  levels=$(printf '%s\n' "$out" | sed -n 's/.*(\([0-9.e+-]*\)).*/\1/p' | awk '{printf "%d", $1 * 255 + 0.5}')
  if [ -z "$levels" ]; then echo "differs $f ($out)"; status=1
  elif [ "$levels" -gt "$tol" ]; then echo "differs $f by $levels"; status=1; fi
done
exit $status
EOF
cat > target/hdr-baseline/refresh.sh <<'EOF'
#!/usr/bin/env bash
# Accept the current tour as the baseline, once its changed pages were checked by eye
set -eu
rm -rf target/hdr-baseline/demo-tour
cp -R target/demo-tour target/hdr-baseline/demo-tour
EOF
chmod +x target/hdr-baseline/compare.sh target/hdr-baseline/refresh.sh
./dev.sh demos:tour >/dev/null && TOL=0 target/hdr-baseline/compare.sh; echo $?
```

Expected:
- 33 demo directories in the baseline;
- `frame-000060.png` in each game directory;
- `0` printed.

`magick compare -metric PAE` prints `<absolute> (<normalized>)`; the script reads the normalized value. It needs the true-colour RGBA PNGs the engine writes.

- [ ] **Step 4: Benchmark baseline**

Run: `cargo bench --bench foundation -- "headless frame" 2>&1 | grep -A1 '^headless frame' | grep time`

Expected: about `1.02 ms`. Record the line in the ledger; Task 15 compares against it.

---

## Phase 1: the float world and two stages (spec migration step 1)

### Task 1: The tonemap pass and the display image

The display image becomes the first buffer of the display effect stage. A new `tonemap` pass after `cameras` writes it from the latest scene image. Scene images are still 8-bit here, so the pass only clamps, and the tour stays byte-identical.

**Files:**
- Create: `src/core/shaders/core/tonemap.wgsl`, `src/core/render/passes/tonemap.rs`
- Modify:
  - `src/core/shaders/compose.rs` (`TONEMAP`, `built_in()`)
  - `src/core/render/mod.rs` (`pass_ids::TONEMAP`)
  - `src/core/render/frame.rs` (`FrameImage`)
  - `src/core/render/context.rs` (`EffectStages`, `image_view`, `image_texture`)
  - `src/core/render/passes/mod.rs`, `src/core/render/passes/post_process.rs`, `src/core/render/passes/transition.rs`
  - `src/core/graphics/renderer.rs` (`display_effects`)
  - `src/core/render/AGENTS.md` (the pass table)
- Test: `tests/integration/render_pipeline.rs`, `tests/e2e/render_pipeline.rs`

**Interfaces:**
- Produces:
  - `pass_ids::TONEMAP: &str = "tonemap"`. It runs every frame, between `cameras` and `transition`.
  - `FrameImage::Display(OutputBuffer)` replaces `FrameImage::PostProcess(OutputBuffer)`. After `tonemap` the latest image is `Display(OutputBuffer::A)`.
  - `pub(crate) struct EffectStages<'a> { pub(crate) display: &'a mut PostProcessPipeline }` in `render::context`. It becomes the field `RenderContext.stages`, replacing `post_process: Option<&'a mut PostProcessPipeline>`.
  - `RenderContext::new(device, queue, encoder, frame, targets, scene, stages: EffectStages<'a>)`.
  - `context::image_view(targets, &EffectStages, FrameImage) -> &TextureView` and `context::image_texture(targets, &EffectStages, FrameImage) -> &Texture`.
  - `WgpuRenderer.display_effects: PostProcessPipeline`, replacing the always-`Some` `post_process: Option<PostProcessPipeline>`.
  - `passes::tonemap::Tonemap` (`#[derive(Default)]`), which holds `Option<TonemapGpu>`. Tasks 4, 5 and 11 extend `TonemapGpu`.
  - `shaders::compose::TONEMAP` (fullscreen + colour includes + `core/tonemap.wgsl`).

- [ ] **Step 1: Write the failing tests**

In `tests/integration/render_pipeline.rs`, the order gains the pass:

```rust
const ORDER: [&str; 11] = [
  pass_ids::CLEAR,
  pass_ids::WORLD,
  pass_ids::LIGHTING,
  pass_ids::UNLIT_WORLD,
  pass_ids::CAMERAS,
  pass_ids::TONEMAP,
  pass_ids::TRANSITION,
  pass_ids::POST_PROCESS,
  pass_ids::UI,
  pass_ids::COMPOSITE,
  pass_ids::CAPTURE,
];
```

In `a_plain_frame_runs_the_always_on_passes`, expect `vec!["clear", "unlit_world", "cameras", "tonemap", "composite"]`. In `a_full_2d_frame_runs_every_scene_pass`, insert `"tonemap",` after `"cameras",`.

In `tests/e2e/render_pipeline.rs`, give `Magenta` the slot it records into, so two tests never share one:

```rust
/// A game pass that clears the current image to magenta and records the
/// format it saw.
struct Magenta(&'static Mutex<Option<ryo2d::wgpu::TextureFormat>>);

/// The format `RenderContext::format` reported to the pass after post-processing.
static SEEN_FORMAT: Mutex<Option<ryo2d::wgpu::TextureFormat>> = Mutex::new(None);
/// The format it reported to the pass before the tonemap.
static SEEN_BEFORE_TONEMAP: Mutex<Option<ryo2d::wgpu::TextureFormat>> = Mutex::new(None);
```

In `execute`, change `SEEN_FORMAT.lock()` to `self.0.lock()`, and in `WithGamePass` insert `Magenta(&SEEN_FORMAT)`. Then add:

```rust
struct WithEarlyPass;

impl Scene for WithEarlyPass {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
    let _ = ctx
      .render_pipeline()
      .insert_before(pass_ids::TONEMAP, Magenta(&SEEN_BEFORE_TONEMAP));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn a_frame_pass_before_the_tonemap_reaches_the_frame() {
  let Some(mut engine) = engine(WithEarlyPass) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(32, 32).0,
    [255, 0, 255, 255]
  );
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test --test integration render_pipeline 2>&1 | tail -5; cargo test --test e2e render_pipeline 2>&1 | tail -5`

Expected: compile errors: no `pass_ids::TONEMAP`.

- [ ] **Step 3: The shader and the pass id**

`src/core/shaders/core/tonemap.wgsl`:

```wgsl
// The look: the latest scene image (linear light, premultiplied, may exceed
// 1) into the display image (Rgba8UnormSrgb, which encodes sRGB on store),
// one texel per pixel.

@group(0) @binding(0) var scene: texture_2d<f32>;

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    let c = textureLoad(scene, vec2<i32>(in.position.xy), 0);
    return clamp(c, vec4<f32>(0.0), vec4<f32>(1.0));
}
```

In `src/core/shaders/compose.rs`, after `CAMERA_IMAGE`:

```rust
/// The tonemap pass: the scene image into the display image
pub(crate) const TONEMAP: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("include/color.wgsl"),
  "\n",
  include_str!("core/tonemap.wgsl"),
);
```

Add `("tonemap", TONEMAP),` to `built_in()` after `("camera_image", CAMERA_IMAGE),`.

In `src/core/render/mod.rs`, add after `CAMERAS` in `pass_ids`:

```rust
  /// The look: the scene image into the display image (exposure, curve,
  /// grading)
  pub const TONEMAP: &str = "tonemap";
```

- [ ] **Step 4: The display image and the effect stages**

In `src/core/render/frame.rs`, replace the `PostProcess` variant of `FrameImage`:

```rust
  /// A buffer of the display effect stage: `A` is the display image the
  /// `tonemap` pass writes
  Display(OutputBuffer),
```

In `src/core/render/context.rs`:
- Add the struct below.
- Replace the `post_process` field with `pub(crate) stages: EffectStages<'a>,`.
- Make `RenderContext::new` take `stages: EffectStages<'a>` in place of `post_process`.
- Pass `&self.stages` in `format`, `current_view` and `encoder_and_current_view`.

```rust
/// The effect stages' pipelines and their ping-pong images.
pub(crate) struct EffectStages<'a> {
  /// The display stage: its buffer `A` is the display image the `tonemap`
  /// pass writes
  pub(crate) display: &'a mut PostProcessPipeline,
}
```

Rewrite the two lookups:

```rust
/// The view of the texture that holds `image`.
pub(super) fn image_view<'t>(
  targets: &'t FrameTargets<'_>,
  stages: &'t EffectStages<'_>,
  image: FrameImage,
) -> &'t wgpu::TextureView {
  match image {
    FrameImage::Game => targets.game_view,
    FrameImage::Lit => targets.lit_view,
    FrameImage::Display(buffer) => stages.display.get_output_view(buffer),
    FrameImage::Camera(camera) => targets
      .camera_images
      .view(camera)
      .unwrap_or(targets.game_view),
  }
}

/// The texture that holds `image`.
pub(super) fn image_texture<'t>(
  targets: &'t FrameTargets<'_>,
  stages: &'t EffectStages<'_>,
  image: FrameImage,
) -> &'t wgpu::Texture {
  match image {
    FrameImage::Game => targets.game,
    FrameImage::Lit => targets.lit,
    FrameImage::Display(OutputBuffer::A) => stages.display.texture_a(),
    FrameImage::Display(OutputBuffer::B) => stages.display.texture_b(),
    FrameImage::Camera(camera) => targets
      .camera_images
      .texture(camera)
      .unwrap_or(targets.game),
  }
}
```

In `src/core/render/passes/transition.rs`, destructure `stages` in place of `post_process`, and read the source with `image_texture(targets, stages, *image)`.

In `src/core/render/passes/post_process.rs`, replace everything from the `let RenderContext {` destructuring to the end of `execute`:

```rust
    let RenderContext {
      encoder,
      stages,
      image,
      ..
    } = ctx;
    // Display effects work on the display image the tonemap pass wrote
    let FrameImage::Display(buffer) = *image else {
      return;
    };
    if active.is_empty() && custom.is_empty() {
      return;
    }
    let display = &mut *stages.display;
    // The chain reads texture A
    if buffer == OutputBuffer::B {
      encoder.copy_texture_to_texture(
        display.texture_b().as_image_copy(),
        display.texture_a().as_image_copy(),
        wgpu::Extent3d {
          width,
          height,
          depth_or_array_layers: 1,
        },
      );
    }
    let output = display.apply_chain(device, queue, encoder, &active, &custom, time);
    *image = FrameImage::Display(output);
```

It no longer needs the `image_texture` import.

- [ ] **Step 5: The tonemap pass**

`src/core/render/passes/tonemap.rs`:

```rust
//! The look: the scene image tonemapped into the display image

use crate::core::graphics::bind_groups::{BindGroupCache, Bound};
use crate::core::render::context::image_view;
use crate::core::render::{FrameImage, RenderContext, RenderPass, pass_ids};
use crate::core::shaders::OutputBuffer;

/// The tonemap pipeline for one display format and render size, and the
/// bind groups of the scene images it has read.
struct TonemapGpu {
  format: wgpu::TextureFormat,
  size: (u32, u32),
  layout: wgpu::BindGroupLayout,
  pipeline: wgpu::RenderPipeline,
  bind_groups: BindGroupCache,
}

impl TonemapGpu {
  fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> Self {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Tonemap Bind Group Layout"),
      entries: &[wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
          sample_type: wgpu::TextureSampleType::Float { filterable: false },
          view_dimension: wgpu::TextureViewDimension::D2,
          multisampled: false,
        },
        count: None,
      }],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Tonemap Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::TONEMAP.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Tonemap Pipeline Layout"),
      bind_group_layouts: &[Some(&layout)],
      immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Tonemap Pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: Some("vs_main"),
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: Some("fs_main"),
        targets: &[Some(wgpu::ColorTargetState {
          format,
          blend: None,
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState::default(),
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      multiview_mask: None,
      cache: None,
    });
    Self {
      format,
      size,
      layout,
      pipeline,
      bind_groups: BindGroupCache::default(),
    }
  }
}

/// Writes the display image (the display stage's buffer `A`) from the
/// latest scene image; the transition, display effects, UI and composite
/// work on it.
#[derive(Default)]
pub(super) struct Tonemap {
  gpu: Option<TonemapGpu>,
}

impl RenderPass for Tonemap {
  fn id(&self) -> &str {
    pass_ids::TONEMAP
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    // Already the display image: nothing to map (and a pass can't read the
    // image it writes)
    if matches!(ctx.image, FrameImage::Display(_)) {
      return;
    }
    let device = ctx.device();
    let size = ctx.render_size();
    let format = ctx.stages.display.texture_a().format();
    // A resize recreates the images the cached bind groups point at
    let gpu = match &mut self.gpu {
      Some(gpu) if gpu.format == format && gpu.size == size => gpu,
      slot => slot.insert(TonemapGpu::new(device, format, size)),
    };
    let RenderContext {
      encoder,
      targets,
      stages,
      image,
      ..
    } = ctx;
    let source = image_view(targets, stages, *image).clone();
    let group = gpu.bind_groups.get(
      device,
      "Tonemap Bind Group",
      &gpu.layout,
      &[Bound::View(source)],
    );
    {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Tonemap Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: stages.display.get_output_view(OutputBuffer::A),
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      pass.set_pipeline(&gpu.pipeline);
      pass.set_bind_group(0, &group, &[]);
      pass.draw(0..6, 0..1);
    }
    *image = FrameImage::Display(OutputBuffer::A);
  }
}
```

In `src/core/render/passes/mod.rs`:
- Add `mod tonemap;`.
- `built_in()` returns `[Box<dyn RenderPass>; 11]`, with `Box::new(tonemap::Tonemap::default()),` after `Box::new(cameras::Cameras),`.

- [ ] **Step 6: The renderer owns the display stage**

In `src/core/graphics/renderer.rs`:
- Rename the field `post_process: Option<PostProcessPipeline>` to `display_effects: PostProcessPipeline` (doc: `// The display effect stage; its buffer A is the display image`). Construct it without `Some`.
- In `recreate_scene_targets`, call `self.display_effects.resize(&self.device, new_width, new_height);` directly.
- In `render_frame`, set `effects: scene.effects.has_active_effects(),`. Pass `EffectStages { display: &mut self.display_effects }` to `RenderContext::new`. `context` is a private module, so re-export the type in `src/core/render/mod.rs` with `pub(crate) use context::{EffectStages, FrameTargets};`.
- The test near line 951 reads `let post = &renderer.display_effects;`.

Run `grep -rn 'FrameImage::PostProcess\|\.post_process\b\|post_process:' src` and update whatever it lists the same way.

- [ ] **Step 7: Run the tests**

Run: `cargo test --test integration render_pipeline 2>&1 | grep 'test result'; cargo test --test e2e render_pipeline 2>&1 | grep 'test result'`

Expected: both pass.

- [ ] **Step 8: Document the pass**

In `src/core/render/AGENTS.md`, add a `tonemap` row to the pass table after `cameras`: "Writes the display image (the display stage's buffer A) from the latest scene image; runs every frame". Note that the transition, `post_process` and the composite work on the display image.

- [ ] **Step 9: The gate and commit**

Run the gate, with `TOL=0 target/hdr-baseline/compare.sh` for the compare step.

Expected: `<N> 0`; no lint; `compare 0` (byte-identical: the 8-bit scene goes through the clamp unchanged).

```bash
git add -A
git commit -m "feat: tonemap pass writing the display image

- the display image is the display effect stage's first buffer
- post-processing and the transition work on it"
```

### Task 2: Float scene images and linear render-target pages

Every image the world draws into becomes `Rgba16Float`. Render targets hold linear premultiplied colour, so sprites showing them sample through new linear-page shader entries.

**Files:**
- Modify:
  - `src/core/render/mod.rs` (`SCENE_FORMAT`)
  - `src/core/graphics/renderer.rs` (game and lit textures, camera images, the first clear, `game_texture_format`)
  - `src/core/lighting/renderer.rs` (the scene copy follows the renderer's format)
  - `src/core/graphics/atlas.rs` (render-target pages)
  - `src/core/render/batch/{key,pipelines,gpu,mod}.rs` (`PageKind`)
  - `src/core/shaders/core/sprite.wgsl` (linear entries)
  - `src/core/shaders/custom.rs` (`fs_custom_linear`)
  - `src/core/render/context.rs` (the `format` docs)
- Test: `tests/e2e/render_pipeline.rs`, `tests/e2e/gpu_world.rs`, `tests/e2e/custom_shaders.rs`, `src/core/render/batch/key.rs`

**Interfaces:**
- Consumes: Task 1's `Tonemap` pass, which clamps the float image into the display image.
- Produces:
  - `pub(crate) const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;` in `crate::core::render`.
  - `pub(crate) enum PageKind { Srgb8, Linear }` with `PageKind::of(page: Option<SlotPage>) -> PageKind` in `render::batch` (`Target` pages are `Linear`).
  - `PipelineKey { shader, blend, stream, page: PageKind, format }`.
  - In `sprite.wgsl`:
    - entries `fs_base_linear` and `fs_effect_linear`;
    - `var<private> linear_page: bool`, which `sample_sprite` reads;
    - `fn straight_srgb(c: vec4<f32>) -> vec4<f32>`;
    - `fn finish_linear(color: vec4<f32>, tint: vec4<f32>) -> vec4<f32>`.
  - Custom sprite shaders gain the entry `fs_custom_linear`.
  - `LightingRenderer::new(device, width, height, format)` makes its scene copies in `format`.

- [ ] **Step 1: Write the failing tests**

In `tests/e2e/render_pipeline.rs`, extend `a_frame_pass_before_the_tonemap_reaches_the_frame` after its pixel assertion:

```rust
  assert_eq!(
    *SEEN_BEFORE_TONEMAP.lock().unwrap(),
    Some(ryo2d::wgpu::TextureFormat::Rgba16Float),
    "frame passes before the tonemap draw on the HDR scene image"
  );
```

`a_game_pass_after_post_process_draws_on_the_frame` keeps expecting `Rgba8UnormSrgb`.

In `tests/e2e/gpu_world.rs`:

```rust
/// Two additive white squares (2.0 in linear light), halved by a multiply.
struct PileUp;

impl Scene for PileUp {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let white = solid("pile-white", 32, [255, 255, 255, 255]);
    for z in 0..2 {
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0),
        Renderable::sprite(white.clone())
          .with_z_order(z)
          .with_blend(BlendMode::Additive),
      ));
    }
    // sRGB 188 is half in linear light
    ctx.world.spawn((
      Transform::from_xy(16.0, 16.0),
      Renderable::sprite(solid("pile-half", 32, [188, 188, 188, 255]))
        .with_z_order(2)
        .with_blend(BlendMode::Multiply),
    ));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn light_above_white_survives_until_the_tonemap() {
  let Some(mut engine) = start(EngineBuilder::new().resolution(32, 32).start_scene(PileUp)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let pixel = engine.frame().unwrap().get_pixel(16, 16).0;
  // 2.0 halved is 1.0 (white); an image clamped at 1.0 shows 188
  assert!(pixel[0] > 230, "{pixel:?}");
}
```

In `tests/e2e/custom_shaders.rs` add two guards. They pass today and must still pass once targets are float.

```rust
/// A red square seen by a texture camera, shown by a sprite with `shader`.
fn red_through_a_target(shader: SpriteShader) -> Fixture {
  Fixture::entities(move |ctx| {
    ctx.world.spawn((
      Transform::from_xy(1000.0, 1000.0),
      Renderable::sprite(solid("rt-red", 16, RED)),
    ));
    let target = ctx.render_targets().create(16, 16);
    ctx.world.spawn((
      Transform::from_xy(1000.0, 1000.0),
      Camera2d {
        target: CameraTarget::Texture(target),
        ..Default::default()
      },
    ));
    ctx.world.spawn((
      Transform::from_xy(32.0, 32.0),
      Renderable::render_target(target).with_shader(shader.clone()),
    ));
  })
}

fn assert_near(pixel: [u8; 4], want: [u8; 4]) {
  for (got, want) in pixel.iter().zip(want) {
    assert!(got.abs_diff(want) <= 1, "{pixel:?} vs {want:?}");
  }
}

#[test]
fn a_custom_shader_sees_a_render_target_as_straight_srgb() {
  let shader = ShaderHandle::from_source("t2-rt-invert.wgsl", INVERT);
  let Some(mut engine) = start(red_through_a_target(SpriteShader::custom(shader))) else {
    return;
  };
  assert_near(render(&mut engine).get_pixel(32, 32).0, [35, 225, 215, 255]);
}

#[test]
fn a_built_in_effect_sees_a_render_target_as_straight_srgb() {
  let invert = SpriteShader::BuiltIn(SpriteEffect::Invert);
  let Some(mut engine) = start(red_through_a_target(invert)) else {
    return;
  };
  assert_near(render(&mut engine).get_pixel(32, 32).0, [35, 225, 215, 255]);
}
```

In `src/core/render/batch/key.rs` tests:

```rust
  #[test]
  fn render_targets_are_linear_pages() {
    use crate::core::graphics::{RenderTargets, SlotPage};
    let target = RenderTargets::default().create(4, 4);
    assert_eq!(PageKind::of(Some(SlotPage::Target(target))), PageKind::Linear);
    assert_eq!(PageKind::of(Some(SlotPage::Atlas(0))), PageKind::Srgb8);
    assert_eq!(PageKind::of(Some(SlotPage::Standalone(2))), PageKind::Srgb8);
    assert_eq!(PageKind::of(None), PageKind::Srgb8);
  }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test e2e -- light_above_white a_frame_pass_before_the_tonemap 2>&1 | grep -E 'panicked|test result'; cargo test --lib render_targets_are_linear_pages 2>&1 | tail -3`

Expected:
- `light_above_white` fails with a pixel near 188;
- the format assertion fails with `Rgba8UnormSrgb`;
- the unit test does not compile (no `PageKind`).

- [ ] **Step 3: The scene format**

In `src/core/render/mod.rs`:

```rust
/// The format of every image the world draws into: the scene, lit and
/// camera images, render targets and the lighting copy. Linear light,
/// premultiplied by alpha, with values above 1.
pub(crate) const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
```

In `src/core/graphics/renderer.rs`:

1. Both game texture descriptors (in `new` and `recreate_scene_targets`) use `format: crate::core::render::SCENE_FORMAT`.
2. `create_lit_texture` uses `crate::core::render::SCENE_FORMAT`.
3. `CameraImages::new(&device, crate::core::render::SCENE_FORMAT)`.
4. `game_texture_format()` returns `crate::core::render::SCENE_FORMAT`; its doc reads "The format the world renders in: linear HDR".
5. Replace the charcoal `pixel_buffer` and `queue.write_texture` block with a clear pass (f16 texels are not written from the CPU):

```rust
    // Fill the game texture with a dark charcoal background, which is presented
    // immediately to prevent a flash on startup
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Initial Game Texture Clear"),
    });
    {
      let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Initial Game Texture Clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &texture_view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            // sRGB 45 grey in linear light
            load: wgpu::LoadOp::Clear(wgpu::Color {
              r: 0.0262,
              g: 0.0262,
              b: 0.0262,
              a: 1.0,
            }),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
    }
    queue.submit(std::iter::once(encoder.finish()));
```

In `src/core/lighting/renderer.rs`:
- Store the `format` passed to `LightingRenderer::new` in a new field, `format: wgpu::TextureFormat`.
- `LightTargets::new` takes `format: wgpu::TextureFormat` and uses it for `scene_copy_texture`.
- Pass `self.format` (or the constructor's `format`) at its three call sites.

The engine passes `game_texture_format()`, so its copies are float. The standalone tests in `tests/e2e/lighting_renderer.rs` keep `Rgba8UnormSrgb` until Task 10 removes them.

In `src/core/graphics/atlas.rs` `sync_target`, make the target texture float and bind its own view:

```rust
      format: crate::core::render::SCENE_FORMAT,
      // Lighting copies the camera's image before lighting it in place
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_SRC,
      view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_groups = self.bind(device, &view, "Render Target");
```

The `sampled` view goes. Atlas pages keep `PAGE_FORMAT`.

- [ ] **Step 4: The page kind in the pipeline key**

In `src/core/render/batch/key.rs`:

```rust
/// What a sampled page holds: sRGB bytes (atlas and standalone pages) or
/// linear colour premultiplied by alpha (render targets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PageKind {
  Srgb8,
  Linear,
}

impl PageKind {
  /// The kind of `page`; untextured draws count as sRGB.
  pub(crate) fn of(page: Option<SlotPage>) -> Self {
    match page {
      Some(SlotPage::Target(_)) => Self::Linear,
      _ => Self::Srgb8,
    }
  }
}
```

Export it from `render/batch/mod.rs` (`pub(crate) use key::{BatchShader, DrawState, PageKind, Sampling, Stream};`).

`DrawState` needs no new field: its `page` already says which kind it samples.

In `src/core/render/batch/pipelines.rs`, `PipelineKey` gains `pub(crate) page: PageKind,` after `stream`. `ensure` picks the linear entries:

```rust
    // Premultiplied textures take their colour as it is
    let premultiplied = key.blend == BlendMode::Premultiplied;
    let linear = key.page == PageKind::Linear;
    let (module, layout, fragment) = match (key.shader, key.stream, premultiplied, linear) {
      // Render targets hold linear premultiplied colour, whatever the blend
      (BatchShader::Sprite, Stream::Quads, _, true) => (&self.sprite, &self.layout, "fs_base_linear"),
      (BatchShader::Sprite, Stream::Quads, false, false) => (&self.sprite, &self.layout, "fs_base"),
      (BatchShader::Sprite, Stream::Quads, true, false) => {
        (&self.sprite, &self.layout, "fs_base_premultiplied")
      }
      (BatchShader::SpriteEffect, Stream::Quads, _, true) => {
        (&self.sprite, &self.layout, "fs_effect_linear")
      }
      (BatchShader::SpriteEffect, Stream::Quads, false, false) => {
        (&self.sprite, &self.layout, "fs_effect")
      }
      (BatchShader::SpriteEffect, Stream::Quads, true, false) => {
        (&self.sprite, &self.layout, "fs_effect_premultiplied")
      }
      (BatchShader::Line, Stream::Triangles, _, _) => (&self.line, &self.camera_only, "fs_main"),
      (BatchShader::LineTextured, Stream::Triangles, _, _) => {
        (&self.line, &self.layout, "fs_textured")
      }
      (BatchShader::Custom(source), Stream::Quads, _, _) => {
```

In the custom arm, `fragment` becomes:

```rust
        let fragment = if linear {
          "fs_custom_linear"
        } else if premultiplied {
          "fs_custom_premultiplied"
        } else {
          "fs_custom"
        };
```

In `src/core/render/batch/gpu.rs`:
- The sprite key in `prepare` gets `page: PageKind::of(Some(slot.page)),`. `slot` is resolved before the key; move the key below it if needed.
- The line key gets `page: PageKind::Srgb8,`. Textured trails always sample as sRGB pages: a trail textured by a render target is not supported.
- In `draw`, build the key with:

```rust
    let page = match draw.stream {
      Stream::Quads => PageKind::of(draw.state.page),
      Stream::Triangles => PageKind::Srgb8,
    };
```

- [ ] **Step 5: The linear-page shader entries**

In `src/core/shaders/core/sprite.wgsl`, replace `sample_sprite` and add the helpers:

```wgsl
// Set by the entries for render-target pages, which hold linear colour
// premultiplied by alpha: samples come back as straight sRGB, like any
// other texture's
var<private> linear_page: bool;

// The sprite texture at `uv`, transparent outside `bounds`
fn sample_sprite(uv: vec2<f32>, bounds: vec4<f32>) -> vec4<f32> {
    let texel = sample_masked(sprite_texture, sprite_sampler, uv, bounds, uv_ddx, uv_ddy);
    if linear_page {
        return straight_srgb(texel);
    }
    return texel;
}

// Linear colour premultiplied by alpha as straight sRGB (0 to 1)
fn straight_srgb(c: vec4<f32>) -> vec4<f32> {
    if c.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    let rgb = clamp(c.rgb / c.a, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(linear_to_srgb(rgb), min(c.a, 1.0));
}

// A linear premultiplied texel tinted in linear light (the tint decoded from
// sRGB); values above 1 are kept
fn finish_linear(color: vec4<f32>, tint: vec4<f32>) -> vec4<f32> {
    let a = clamp(tint.a, 0.0, 1.0);
    return vec4<f32>(color.rgb * srgb_to_linear(tint.rgb) * a, color.a * a);
}
```

Below `fs_effect_premultiplied`, add:

```wgsl
// The same for render-target pages
@fragment
fn fs_base_linear(in: VertexOutput) -> @location(0) vec4<f32> {
    take_derivatives(in.uv);
    let color = sample_masked(sprite_texture, sprite_sampler, in.uv, in.uv_bounds, uv_ddx, uv_ddy);
    return finish_linear(color, in.tint);
}

// Effects work on straight sRGB, so the page reads back as it
@fragment
fn fs_effect_linear(in: VertexOutput) -> @location(0) vec4<f32> {
    take_derivatives(in.uv);
    linear_page = true;
    return finish(effect_color(in), in.tint);
}
```

Update the header comment: atlas pages are sampled as raw sRGB bytes; render-target pages hold linear premultiplied colour; output goes to the linear float scene image.

In `src/core/shaders/custom.rs` `sprite_entry`, add after `fs_custom_premultiplied`:

```wgsl
@fragment
fn fs_custom_linear(in: VertexOutput) -> @location(0) vec4<f32> {{
  take_derivatives(in.uv);
  linear_page = true;
  return finish(ry_custom(in), in.tint);
}}
```

- [ ] **Step 6: The format docs**

`RenderContext::format` in `src/core/render/context.rs`:

```rust
  /// The format of [`current_view`](Self::current_view), for the render
  /// pipelines a pass builds: `Rgba16Float` (linear light, may exceed 1) in
  /// camera passes and frame passes before `tonemap`, `Rgba8UnormSrgb` (the
  /// display image) after it.
```

Update `current_view`'s doc to say the same: before `tonemap` it is a scene image, after it the display image.

- [ ] **Step 7: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:|panicked' | head -20`

Expected: everything passes.

Exact-colour asserts on render-target pages (`a_sprite_shows_what_a_texture_camera_saw_this_frame` and friends) may move by one level from f16 rounding. Loosen such an assert to ±1 only after reading the pixel, and record a ruling. A difference of more than one level is a bug.

- [ ] **Step 8: The gate and commit**

Expected:
- `compare 0` at `TOL=1`.
- A page differing by more than 1 is investigated. If it is explained by removed 8-bit rounding (an alpha pile-up, bloom over a lit scene), it is checked by eye, listed in the ledger and the baseline refreshed. Anything else is a bug.

```bash
git add -A
git commit -m "feat: the world renders into float images

- scene, lit and camera images, render targets and the lighting copy are Rgba16Float
- render targets hold linear premultiplied colour, sampled through linear-page entries"
```

---

## Phase 2: the colour rule (spec migration step 2)

### Task 3: `Color::to_linear` and decoded colours

**Files:**
- Modify:
  - `src/core/ui/types.rs` (`Color::to_linear`)
  - `src/core/lighting/renderer.rs` (light, ambient and atmosphere colours)
  - `src/core/render/batch/lines.rs` (`line_color`)
  - `src/core/shaders/effects.rs` (fade, tint and flash colours)
  - `src/core/render/passes/scene.rs` (`premultiplied_linear`)
- Create: `tests/e2e/lighting.rs` (registered in `tests/e2e.rs` as `#[path = "e2e/lighting.rs"] mod lighting;`)
- Test: `src/core/ui/types.rs`, `tests/e2e/gpu_world.rs`, `tests/e2e/post_process.rs`, `tests/e2e/lighting.rs`

**Interfaces:**
- Produces:
  - `pub fn Color::to_linear(self) -> [f32; 4]`: each channel decoded with the exact sRGB transfer function, alpha straight (0 to 1).
  - `line_color(color: Color) -> [f32; 4]` now decodes; Task 8 adds an emission argument.
  - `tests/e2e/lighting.rs` with the `lit(setup)` helper: a 128x64 window with a white floor, two frames stepped, the frame returned. Tasks 9 and 10 add to it.

- [ ] **Step 1: Write the failing tests**

In `src/core/ui/types.rs` (add a `#[cfg(test)] mod tests` if the file has none):

```rust
  #[test]
  fn to_linear_decodes_srgb_and_keeps_alpha_straight() {
    assert_eq!(Color::BLACK.to_linear(), [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(Color::WHITE.to_linear(), [1.0, 1.0, 1.0, 1.0]);
    let [grey, _, _, alpha] = Color::new(128, 128, 128, 128).to_linear();
    assert!((grey - 0.215_861).abs() < 1e-5, "{grey}");
    assert!((alpha - 128.0 / 255.0).abs() < 1e-6, "{alpha}");
    // The linear segment near black
    let [dark, _, _, _] = Color::rgb(10, 10, 10).to_linear();
    assert!((dark - 10.0 / 255.0 / 12.92).abs() < 1e-7, "{dark}");
  }
```

In `tests/e2e/gpu_world.rs`:

```rust
#[test]
fn a_line_shows_the_colour_it_names() {
  let scene = Fixture::new(|ctx| {
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Line2D::horizontal(16.0, 0.0, 32.0, Color::rgb(128, 128, 128), 8.0),
    ));
  });
  let Some(frame) = frame_of(scene, (32, 32)) else {
    return;
  };
  let pixel = frame.get_pixel(16, 16).0;
  // Read as linear (the old rule) it showed about 188
  assert!(pixel[0].abs_diff(128) <= 1, "{pixel:?}");
}
```

In `tests/e2e/post_process.rs`:

```rust
struct GreyFade;

impl Scene for GreyFade {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.effects().set_fade_color(1.0, Color::rgb(128, 128, 128));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn a_fade_reaches_the_colour_it_names() {
  let Some(mut engine) = engine(GreyFade) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let pixel = engine.frame().unwrap().get_pixel(32, 32).0;
  // Read as linear (the old rule) it showed about 188
  assert!(pixel[0].abs_diff(128) <= 1, "{pixel:?}");
}
```

Create `tests/e2e/lighting.rs`:

```rust
//! 2D lighting through the engine: lights, shadows, ambient light and
//! atmosphere, read back as display bytes.
//!
//! Tests skip when no GPU is available.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

type Setup = Box<dyn FnMut(&mut SceneContext) + Send>;

/// A white floor over the whole 128x64 window, then `setup`.
struct Lit(Setup);

impl Scene for Lit {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let floor = SpriteHandle::from_sprite(Sprite::solid_rect(128, 64, 255, 255, 255, 255), "lit-floor");
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::sprite(floor).with_anchor(Anchor::TopLeft),
    ));
    (self.0)(ctx);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// The frame after two steps of the floor lit by `setup`.
fn lit(setup: impl FnMut(&mut SceneContext) + Send + 'static) -> Option<image::RgbaImage> {
  let scene = Lit(Box::new(setup));
  let builder = EngineBuilder::new().resolution(128, 64).start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap())
}

#[test]
fn ambient_mid_grey_lights_white_to_mid_grey() {
  let Some(frame) = lit(|ctx| {
    let mut lighting = ctx.lighting();
    lighting.set_ambient(AmbientLight::new(Color::rgb(128, 128, 128), 1.0));
    lighting.set_enabled(true);
  }) else {
    return;
  };
  let pixel = frame.get_pixel(64, 32).0;
  // Read as linear (the old rule) it showed about 188
  assert!(pixel[0].abs_diff(128) <= 1, "{pixel:?}");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib to_linear 2>&1 | tail -3; cargo test --test e2e -- a_line_shows a_fade_reaches ambient_mid_grey 2>&1 | grep -E 'panicked|test result'`

Expected: the unit test does not compile (no `to_linear`); the three e2e tests fail with values near 188.

- [ ] **Step 3: The decoder**

In `src/core/ui/types.rs`, next to `to_normalized`:

```rust
  /// The colour in linear light, for the GPU: each channel decoded with the
  /// exact sRGB transfer function, alpha straight (0 to 1).
  pub fn to_linear(self) -> [f32; 4] {
    let decode = |channel: u8| {
      let c = f32::from(channel) / 255.0;
      if c <= 0.04045 {
        c / 12.92
      } else {
        ((c + 0.055) / 1.055).powf(2.4)
      }
    };
    [
      decode(self.r),
      decode(self.g),
      decode(self.b),
      f32::from(self.a) / 255.0,
    ]
  }
```

Change `to_normalized`'s doc to "the bytes over 255, for colours used as sRGB; colours reaching the GPU as light use [`to_linear`](Self::to_linear)".

- [ ] **Step 4: Every colour that reaches the GPU as light**

In `src/core/lighting/renderer.rs`:

`light_instances` builds `color_intensity` with a helper, in both arms:

```rust
/// A light's colour in linear light, intensity in alpha.
fn light_color(color: Color, intensity: f32) -> [f32; 4] {
  let [r, g, b, _] = color.to_linear();
  [r, g, b, intensity]
}
```

`compose_targets` decodes the ambient and atmosphere colours:

```rust
    let [ar, ag, ab, _] = self.ambient.color.to_linear();
    let [fr, fg, fb, _] = self.atmosphere.color.to_linear();
    let compose_uniform = LightingParams {
      color: [ar, ag, ab, 1.0],
      params: [
        self.ambient.intensity,
        self.atmosphere.density,
        self.atmosphere.scattering,
        0.0,
      ],
      time_resolution: [time, targets.width as f32, targets.height as f32, 0.0],
      atmosphere_color: [fr, fg, fb, 1.0],
    };
```

In `src/core/render/batch/lines.rs`:

```rust
/// A colour decoded to linear light, for a vertex.
pub(crate) fn line_color(color: Color) -> [f32; 4] {
  color.to_linear()
}
```

In `src/core/shaders/effects.rs` `EffectParams::from_effect`, the `Fade`, `ColorTint` and `ScreenFlash` arms use `color.to_linear()` in place of `color.to_normalized()`. Their shaders mix in linear light on the display image.

In `src/core/render/passes/scene.rs`:

```rust
/// `color` (sRGB) as the linear, premultiplied colour a scene image holds.
fn premultiplied_linear(color: Color) -> wgpu::Color {
  let [r, g, b, a] = color.to_linear().map(f64::from);
  wgpu::Color {
    r: r * a,
    g: g * a,
    b: b * a,
    a,
  }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:|panicked' | head -20`

Expected: all pass. Some existing tests measured light, ambient, line or effect colours computed from bytes read as linear (for example compose values in `tests/e2e/lighting_renderer.rs`, which Task 9 deletes). Update their expected values to the decoded colour and record each as a ruling. Never change what a test checks to make it pass.

- [ ] **Step 6: The gate and commit**

Expected pages in `compare`:
- pages with lights (lighting, lighting-volumetric, every lit shadows page): mid-tones darken, warm and cool lights saturate;
- pages with trails, lines or debug shapes: they now match sprites of their colour;
- pages with a coloured fade, tint or flash.

Check each by eye, refresh, and ledger the list. Task 13 retunes the demos.

```bash
git add -A
git commit -m "feat: every Color is sRGB on the GPU

- Color::to_linear decodes light, ambient, atmosphere, line, debug and effect colours
- lines match sprites of the same colour"
```

---

## Phase 3: the look (spec migration step 3)

### Task 4: `Look`: exposure and tonemap curves

**Files:**
- Create: `src/core/render/look.rs`, `tests/e2e/look.rs` (registered in `tests/e2e.rs`)
- Modify:
  - `src/core/render/mod.rs` (`pub(crate) mod look; pub use look::{Look, Tonemap};`)
  - `src/lib.rs` (prelude: `Look`, `Tonemap`)
  - `src/core/render/frame.rs` (`FrameScene.look`)
  - `src/core/engine/extract.rs` (pass the resource)
  - `src/core/engine/mod.rs`, `src/core/scene/mod.rs` (insert `Look::default()` next to `EffectsManager::new()`, at both sites)
  - `src/core/scene/context.rs` (`look()`)
  - `src/core/scene/cleanup.rs`, `src/core/scene/mod.rs` (`SceneCleanup::EFFECTS` resets the look)
  - `src/core/shaders/core/tonemap.wgsl`, `src/core/render/passes/tonemap.rs` (uniforms)

**Interfaces:**
- Consumes: Task 1's `TonemapGpu` and `tonemap.wgsl`.
- Produces:
  - `#[derive(Resource, Debug, Clone, PartialEq)] pub struct Look { pub exposure: f32, pub tonemap: Tonemap }` (`Default`: 0 EV, `Tonemap::default()`). Task 5 adds `grading`; Task 11 adds `auto_exposure`.
  - `#[derive(Debug, Clone, Copy, PartialEq)] pub enum Tonemap { Soft { knee: f32, white: f32 }, Clamp, Aces, AgX }`. `Default` is `Soft { knee: 0.8, white: 4.0 }`.
  - `pub fn Tonemap::apply(self, rgb: Vec3) -> Vec3`.
  - `pub(crate) fn Tonemap::sanitized(self) -> Tonemap`.
  - `#[repr(C)] pub(crate) struct LookUniforms { pub(crate) curve: [f32; 4] }` (Task 5 extends it) and `pub(crate) fn Look::uniforms(&self) -> LookUniforms`.
  - `FrameScene { .., pub(crate) look: &'a Look }`.
  - `pub fn SceneContext::look(&mut self) -> Mut<'_, Look>`.

- [ ] **Step 1: Write the failing tests**

Create `src/core/render/look.rs` holding only this tests module; it fails to compile until Step 3 adds the types above it:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  fn grey(v: f32) -> Vec3 {
    Vec3::splat(v)
  }

  const CURVES: [Tonemap; 4] = [
    Tonemap::Soft {
      knee: 0.8,
      white: 4.0,
    },
    Tonemap::Clamp,
    Tonemap::Aces,
    Tonemap::AgX,
  ];

  #[test]
  fn soft_is_identity_below_the_knee() {
    for v in [0.0, 0.05, 0.4, 0.79, 0.8] {
      assert_eq!(Tonemap::default().apply(grey(v)), grey(v));
    }
    let colour = Vec3::new(0.7, 0.2, 0.05);
    assert_eq!(Tonemap::default().apply(colour), colour);
  }

  #[test]
  fn soft_leaves_the_knee_at_slope_one() {
    let at = |v: f32| Tonemap::default().apply(grey(v)).x;
    let h = 1e-3;
    let slope = (at(0.8 + h) - at(0.8)) / h;
    assert!((slope - 1.0).abs() < 0.01, "{slope}");
  }

  #[test]
  fn soft_reaches_one_at_white_and_clamps_beyond() {
    let at = |v: f32| Tonemap::default().apply(grey(v)).x;
    assert!((at(4.0) - 1.0).abs() < 1e-5, "{}", at(4.0));
    assert_eq!(at(100.0), 1.0);
  }

  #[test]
  fn soft_shows_white_at_about_0_9() {
    let v = Tonemap::default().apply(grey(1.0)).x;
    assert!((v - 0.9004).abs() < 1e-3, "{v}");
  }

  #[test]
  fn soft_keeps_hues() {
    let c = Tonemap::default().apply(Vec3::new(2.0, 1.0, 0.5));
    assert!((c.x / c.y - 2.0).abs() < 1e-4, "{c:?}");
    assert!((c.y / c.z - 2.0).abs() < 1e-4, "{c:?}");
  }

  #[test]
  fn every_curve_maps_black_to_black_and_never_falls() {
    for tonemap in CURVES {
      assert!(tonemap.apply(Vec3::ZERO).max_element() < 1e-4, "{tonemap:?}");
      let mut last = 0.0;
      for i in 0..=400 {
        let v = tonemap.apply(grey(i as f32 * 0.04)).x;
        assert!(v >= last - 1e-6 && v <= 1.0, "{tonemap:?} at step {i}: {v} after {last}");
        last = v;
      }
    }
  }

  #[test]
  fn degenerate_soft_parameters_stay_finite_and_monotonic() {
    let cases = [
      Tonemap::Soft { knee: 1.5, white: 0.2 },
      Tonemap::Soft { knee: -1.0, white: 4.0 },
      Tonemap::Soft { knee: f32::NAN, white: f32::NAN },
      Tonemap::Soft { knee: 0.8, white: 0.8 },
    ];
    for tonemap in cases {
      let mut last = 0.0;
      for i in 0..=200 {
        let v = tonemap.apply(grey(i as f32 * 0.05)).x;
        assert!(v.is_finite() && (0.0..=1.0).contains(&v), "{tonemap:?}: {v}");
        assert!(v >= last - 1e-6, "{tonemap:?}: {v} after {last}");
        last = v;
      }
    }
  }

  #[test]
  fn non_finite_light_shows_as_black_or_white() {
    for tonemap in CURVES {
      assert!(tonemap.apply(grey(f32::NAN)).max_element() < 1e-4, "{tonemap:?}");
      assert!(tonemap.apply(grey(f32::INFINITY)).min_element() > 0.99, "{tonemap:?}");
    }
  }

  #[test]
  fn uniforms_are_sanitized() {
    let look = Look {
      exposure: f32::NAN,
      tonemap: Tonemap::Soft {
        knee: 2.0,
        white: f32::NAN,
      },
      ..Look::default()
    };
    let curve = look.uniforms().curve;
    assert_eq!(curve[0], 0.0, "NaN exposure is 0 EV");
    assert!(curve[2] <= 0.99 && curve[3] > curve[2], "{curve:?}");
  }
}
```

Create `tests/e2e/look.rs`:

```rust
//! The look: exposure, tonemap curves, grading, LUTs and auto-exposure,
//! read back as display bytes.
//!
//! Tests skip when no GPU is available.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

type Setup = Box<dyn FnMut(&mut SceneContext) + Send>;

/// A square of `color` filling the 32x32 window, under `look`, then `setup`.
struct Square {
  color: [u8; 4],
  look: Look,
  setup: Option<Setup>,
}

impl Scene for Square {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let [r, g, b, a] = self.color;
    let sprite = SpriteHandle::from_sprite(
      Sprite::solid_rect(32, 32, r, g, b, a),
      format!("look-square-{r}-{g}-{b}-{a}"),
    );
    ctx
      .world
      .spawn((Transform::from_xy(16.0, 16.0), Renderable::sprite(sprite)));
    *ctx.look() = self.look.clone();
    if let Some(setup) = self.setup.as_mut() {
      setup(ctx);
    }
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn square(color: [u8; 4], look: Look) -> Square {
  Square {
    color,
    look,
    setup: None,
  }
}

fn start(scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(32, 32).start_scene(scene))
}

/// The centre pixel after two frames.
fn shown(scene: impl Scene + 'static) -> Option<[u8; 4]> {
  let mut engine = start(scene)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap().get_pixel(16, 16).0)
}

/// Linear light as the sRGB byte the display image stores.
fn encode(linear: f32) -> u8 {
  let c = linear.clamp(0.0, 1.0);
  let srgb = if c <= 0.003_130_8 {
    c * 12.92
  } else {
    1.055 * c.powf(1.0 / 2.4) - 0.055
  };
  (srgb * 255.0).round() as u8
}

#[test]
fn the_default_look_keeps_colours_below_the_knee() {
  let Some(pixel) = shown(square([200, 100, 50, 255], Look::default())) else {
    return;
  };
  assert_eq!(pixel, [200, 100, 50, 255]);
}

#[test]
fn white_shows_at_about_243_under_the_default_look() {
  let Some(pixel) = shown(square([255, 255, 255, 255], Look::default())) else {
    return;
  };
  for c in &pixel[..3] {
    assert!((242..=244).contains(c), "{pixel:?}");
  }
}

#[test]
fn clamp_shows_white_as_255() {
  let look = Look {
    tonemap: Tonemap::Clamp,
    ..Look::default()
  };
  let Some(pixel) = shown(square([255, 255, 255, 255], look)) else {
    return;
  };
  assert_eq!(pixel, [255, 255, 255, 255]);
}

#[test]
fn a_fade_to_white_still_reaches_255() {
  // Display effects run after the tonemap, so the shoulder cannot hold them back
  let scene = Square {
    color: [255, 255, 255, 255],
    look: Look::default(),
    setup: Some(Box::new(|ctx: &mut SceneContext| {
      ctx.effects().set_fade_color(1.0, Color::WHITE);
    })),
  };
  let Some(pixel) = shown(scene) else {
    return;
  };
  assert_eq!(pixel, [255, 255, 255, 255]);
}

#[test]
fn one_stop_of_exposure_doubles_the_light() {
  let look = Look {
    exposure: 1.0,
    ..Look::default()
  };
  let Some(pixel) = shown(square([128, 128, 128, 255], look)) else {
    return;
  };
  // sRGB 128 is 0.2159 in linear light; doubled, 0.4317
  assert!(pixel[0].abs_diff(encode(0.4317)) <= 1, "{pixel:?}");
}

#[test]
fn every_curve_on_the_gpu_matches_tonemap_apply() {
  let curves = [Tonemap::default(), Tonemap::Clamp, Tonemap::Aces, Tonemap::AgX];
  for tonemap in curves {
    for exposure in [-1.0_f32, 0.0, 1.0, 2.0, 3.0] {
      let look = Look {
        exposure,
        tonemap,
        ..Look::default()
      };
      let Some(pixel) = shown(square([255, 255, 255, 255], look)) else {
        return;
      };
      let want = encode(tonemap.apply(Vec3::splat(exposure.exp2())).x);
      assert!(
        pixel[0].abs_diff(want) <= 1,
        "{tonemap:?} at {exposure} EV: {pixel:?}, want {want}"
      );
    }
  }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib look:: 2>&1 | tail -3; cargo test --test e2e look:: 2>&1 | tail -3`

Expected: compile errors (`Look`, `Tonemap` missing).

- [ ] **Step 3: The look and its curves**

`src/core/render/look.rs`, above the tests:

```rust
//! The look: how the frame's linear HDR image becomes the picture on screen

use glam::{Mat3, Vec3};

use crate::core::ecs::Resource;

/// How the frame's linear HDR image becomes the display image, once per
/// frame after every camera has drawn (`ctx.look()`). The tonemap pass
/// applies the exposure, then the curve.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Look {
  /// Exposure in EV (stops): +1 doubles the light, -1 halves it (default 0)
  pub exposure: f32,
  /// How light near and above white rolls off (default
  /// [`Tonemap::Soft`] with knee 0.8 and white 4)
  pub tonemap: Tonemap,
}

impl Default for Look {
  fn default() -> Self {
    Self {
      exposure: 0.0,
      tonemap: Tonemap::default(),
    }
  }
}

/// The curve that maps linear light, which may exceed 1, to the display's
/// 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tonemap {
  /// Identity up to `knee`, then a smooth shoulder reaching 1 at `white`
  /// (both linear light) and clamped beyond. It scales all three channels by
  /// what it does to the brightest, so hues stay. The default (knee 0.8,
  /// white 4) keeps colours below sRGB 231 exact and shows pure white at
  /// about sRGB 243
  Soft { knee: f32, white: f32 },
  /// Clips at 1: the look before HDR
  Clamp,
  /// The fitted ACES filmic curve: contrasty, highlights desaturate
  Aces,
  /// AgX with its base contrast: soft, hue-faithful highlights
  AgX,
}

impl Default for Tonemap {
  fn default() -> Self {
    Tonemap::Soft {
      knee: 0.8,
      white: 4.0,
    }
  }
}

impl Tonemap {
  /// What the curve does to a linear colour (after exposure): the display
  /// colour in linear light, each channel 0 to 1. NaN counts as black,
  /// anything past f16's range as its largest value.
  pub fn apply(self, rgb: Vec3) -> Vec3 {
    let rgb = finite(rgb);
    match self.sanitized() {
      Tonemap::Soft { knee, white } => soft(rgb, knee, white),
      Tonemap::Clamp => rgb.clamp(Vec3::ZERO, Vec3::ONE),
      Tonemap::Aces => aces(rgb),
      Tonemap::AgX => agx(rgb),
    }
  }

  /// `Soft` with its knee in 0..=0.99 and white above it; NaN parts take the
  /// default's.
  pub(crate) fn sanitized(self) -> Self {
    match self {
      Tonemap::Soft { knee, white } => {
        let knee = if knee.is_nan() { 0.8 } else { knee.clamp(0.0, 0.99) };
        let white = if white.is_nan() { 4.0 } else { white };
        Tonemap::Soft {
          knee,
          white: white.max(knee + 0.01),
        }
      }
      other => other,
    }
  }
}

/// The largest value an f16 scene image holds.
const F16_MAX: f32 = 65504.0;

/// NaN and negative channels as 0, anything larger than f16 holds as its
/// largest value.
fn finite(rgb: Vec3) -> Vec3 {
  let channel = |c: f32| if c.is_nan() { 0.0 } else { c.clamp(0.0, F16_MAX) };
  Vec3::new(channel(rgb.x), channel(rgb.y), channel(rgb.z))
}

/// Extended Reinhard on the excess of the largest channel over the knee.
fn soft(rgb: Vec3, knee: f32, white: f32) -> Vec3 {
  let m = rgb.max_element();
  if m <= knee {
    return rgb;
  }
  let t = (m - knee) / (1.0 - knee);
  let tw = (white - knee) / (1.0 - knee);
  let g = t * (1.0 + t / (tw * tw)) / (1.0 + t);
  let mapped = (knee + (1.0 - knee) * g).min(1.0);
  rgb * (mapped / m)
}

/// Narkowicz's fit of the ACES curve.
fn aces(x: Vec3) -> Vec3 {
  ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(Vec3::ZERO, Vec3::ONE)
}

/// AgX's inset and outset matrices (columns) and its log2 range.
const AGX_INSET: Mat3 = Mat3::from_cols_array(&[
  0.842_479_1, 0.042_328_24, 0.042_375_65,
  0.078_433_6, 0.878_468_6, 0.078_433_6,
  0.079_223_74, 0.079_166_13, 0.879_143,
]);
const AGX_OUTSET: Mat3 = Mat3::from_cols_array(&[
  1.196_879, -0.052_896_85, -0.052_971_64,
  -0.098_020_88, 1.151_903, -0.098_043_45,
  -0.099_029_74, -0.098_961_18, 1.151_073_7,
]);
const AGX_MIN_EV: f32 = -12.473_93;
const AGX_MAX_EV: f32 = 4.026_069;

/// AgX with its base contrast: log2 encoding, a polynomial sigmoid, then
/// back to linear light (the fit's output is gamma 2.2).
fn agx(rgb: Vec3) -> Vec3 {
  let v = AGX_INSET * rgb.max(Vec3::splat(1e-10));
  let v = Vec3::new(v.x.log2(), v.y.log2(), v.z.log2())
    .clamp(Vec3::splat(AGX_MIN_EV), Vec3::splat(AGX_MAX_EV));
  let v = (v - AGX_MIN_EV) / (AGX_MAX_EV - AGX_MIN_EV);
  let v2 = v * v;
  let v4 = v2 * v2;
  let v = 15.5 * v4 * v2 - 40.14 * v4 * v + 31.96 * v4 - 6.868 * v2 * v + 0.4298 * v2
    + 0.1191 * v
    - 0.00232;
  let v = (AGX_OUTSET * v).clamp(Vec3::ZERO, Vec3::ONE);
  Vec3::new(v.x.powf(2.2), v.y.powf(2.2), v.z.powf(2.2))
}

/// The look as the tonemap shader reads it (group 0, binding 1).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LookUniforms {
  /// x: exposure (EV), y: curve (0 soft, 1 clamp, 2 ACES, 3 AgX), z: knee,
  /// w: white
  pub(crate) curve: [f32; 4],
}

impl Look {
  /// What the tonemap shader reads, every parameter sanitized.
  pub(crate) fn uniforms(&self) -> LookUniforms {
    let exposure = if self.exposure.is_finite() {
      self.exposure.clamp(-16.0, 16.0)
    } else {
      0.0
    };
    let (curve, knee, white) = match self.tonemap.sanitized() {
      Tonemap::Soft { knee, white } => (0.0, knee, white),
      Tonemap::Clamp => (1.0, 0.0, 1.0),
      Tonemap::Aces => (2.0, 0.0, 1.0),
      Tonemap::AgX => (3.0, 0.0, 1.0),
    };
    LookUniforms {
      curve: [exposure, curve, knee, white],
    }
  }
}
```

Clippy may ask for fewer digits in the AgX constants (`excessive_precision`). Take its suggestion: the values above are the published matrices rounded to f32.

- [ ] **Step 4: The shader**

Replace `src/core/shaders/core/tonemap.wgsl`:

```wgsl
// The look: the latest scene image (linear light, premultiplied, may exceed
// 1) into the display image (Rgba8UnormSrgb, which encodes sRGB on store),
// one texel per pixel: exposure, then the curve. `Tonemap::apply` in
// render/look.rs is the same maths on the CPU; keep them together.

struct Look {
    // x: exposure (EV), y: curve (0 soft, 1 clamp, 2 ACES, 3 AgX), z: knee, w: white
    curve: vec4<f32>,
}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var<uniform> look: Look;

// Negative light as none, infinity as the largest value f16 holds
fn finite(c: vec3<f32>) -> vec3<f32> {
    return min(max(c, vec3<f32>(0.0)), vec3<f32>(65504.0));
}

// Identity up to the knee, then extended Reinhard on the excess of the
// largest channel, scaling all three so hues stay
fn soft(c: vec3<f32>, knee: f32, white: f32) -> vec3<f32> {
    let m = max(max(c.r, c.g), c.b);
    if m <= knee {
        return c;
    }
    let t = (m - knee) / (1.0 - knee);
    let tw = (white - knee) / (1.0 - knee);
    let g = t * (1.0 + t / (tw * tw)) / (1.0 + t);
    let mapped = min(knee + (1.0 - knee) * g, 1.0);
    return c * (mapped / m);
}

// Narkowicz's fit of the ACES curve
fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

// AgX with its base contrast
fn agx(c: vec3<f32>) -> vec3<f32> {
    let inset = mat3x3<f32>(
        vec3<f32>(0.8424791, 0.04232824, 0.04237565),
        vec3<f32>(0.0784336, 0.8784686, 0.0784336),
        vec3<f32>(0.07922374, 0.07916613, 0.879143),
    );
    let outset = mat3x3<f32>(
        vec3<f32>(1.196879, -0.05289685, -0.05297164),
        vec3<f32>(-0.09802088, 1.151903, -0.09804345),
        vec3<f32>(-0.09902974, -0.09896118, 1.1510737),
    );
    let min_ev = -12.47393;
    let max_ev = 4.026069;
    var v = inset * max(c, vec3<f32>(1e-10));
    v = clamp(log2(v), vec3<f32>(min_ev), vec3<f32>(max_ev));
    v = (v - min_ev) / (max_ev - min_ev);
    let v2 = v * v;
    let v4 = v2 * v2;
    v = 15.5 * v4 * v2 - 40.14 * v4 * v + 31.96 * v4 - 6.868 * v2 * v + 0.4298 * v2 + 0.1191 * v - 0.00232;
    v = clamp(outset * v, vec3<f32>(0.0), vec3<f32>(1.0));
    return pow(v, vec3<f32>(2.2));
}

fn curve(c: vec3<f32>) -> vec3<f32> {
    switch i32(look.curve.y) {
        case 1: {
            return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case 2: {
            return aces(c);
        }
        case 3: {
            return agx(c);
        }
        default: {
            return soft(c, look.curve.z, look.curve.w);
        }
    }
}

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    let texel = textureLoad(scene, vec2<i32>(in.position.xy), 0);
    let exposed = finite(texel.rgb) * exp2(look.curve.x);
    return vec4<f32>(curve(exposed), clamp(texel.a, 0.0, 1.0));
}
```

- [ ] **Step 5: The pass uploads the look**

In `src/core/render/passes/tonemap.rs`:
- `TonemapGpu` gains `uniforms: wgpu::Buffer`, created with `size: std::mem::size_of::<LookUniforms>() as u64` and `usage: UNIFORM | COPY_DST`.
- The layout gains binding 1:

```rust
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
          },
          count: None,
        },
```

In `execute`, before the bind group:

```rust
    ctx.queue().write_buffer(&gpu.uniforms, 0, bytemuck::bytes_of(&ctx.scene.look.uniforms()));
```

Take `queue` and the uniforms before destructuring `ctx`. The bind group binds `[Bound::View(source), Bound::Buffer(gpu.uniforms.clone())]`.

- [ ] **Step 6: The resource and its plumbing**

1. `src/core/render/frame.rs`: `FrameScene` gains `/// The frame's look (the tonemap pass reads it)` `pub(crate) look: &'a Look,`.
2. `src/core/engine/extract.rs`: set `look: world.resource::<Look>(),` where `FrameScene` is built. Any other `FrameScene { .. }` literal (search `grep -rn 'FrameScene {' src`) binds a `Look::default()` made just before it.
3. `src/core/engine/mod.rs` and `src/core/scene/mod.rs`: `world.insert_resource(crate::core::render::Look::default());` next to `EffectsManager::new()`.
4. `src/core/scene/context.rs`, after `effects()`:

```rust
  /// The frame's look: exposure, the tonemap curve and colour grading.
  pub fn look(&mut self) -> Mut<'_, Look> {
    self.world.resource_mut::<Look>()
  }
```

5. `src/core/scene/cleanup.rs`, in the `EFFECTS` branch:

```rust
  if cleanup.contains(SceneCleanup::EFFECTS) {
    if let Some(mut effects) = world.get_resource_mut::<EffectsManager>() {
      effects.clear();
    }
    if let Some(mut look) = world.get_resource_mut::<Look>() {
      *look = Look::default();
    }
  }
```

6. The doc of `SceneCleanup::EFFECTS` reads "Clear all post-processing effects and reset the look."
7. Export `Look` and `Tonemap` from `crate::core::render` and the prelude in `src/lib.rs` (next to `RenderPipeline`).

- [ ] **Step 7: Run the tests**

Run: `cargo test --lib look:: 2>&1 | grep 'test result'; cargo test --test e2e look:: 2>&1 | grep -E 'panicked|test result'`

Expected: all pass. If `every_curve_on_the_gpu_matches_tonemap_apply` fails for one curve, the WGSL and the Rust disagree: diff the two functions. Never widen the tolerance.

Then run the whole suite. Existing tests that assert exact world bytes at or above sRGB 231 will fail under the default `Soft` curve. Known cases:
- white and `[0, 0, 255, 255]` in `custom_shaders.rs`;
- `debug_colliders_draw` in `gpu_world.rs`;
- `sprite_effects.rs`'s `WHITE`;
- `tint_matches_the_cpu` in `sprite_atlas.rs`;
- the two lighting-off tests in `render_pipeline.rs`, and its `WithEarlyPass` (Task 1's magenta, cleared before the tonemap);
- the white cells `cells()` reads in `headless_engine.rs`.

These tests check drawing, not the look, so their scenes set `ctx.look().tonemap = Tonemap::Clamp;` in `on_enter` (in the shared fixture where one exists), and their assertions stay as they are. Tests whose point is the look (`tests/e2e/look.rs`) keep the default. Record the list of scenes changed as one ruling.

- [ ] **Step 8: The gate and commit**

Expected pages: almost every page whose world has channels at or above sRGB 231. Whites now read about 243, colours below it are unchanged, and the UI is untouched. Check a sample of them by eye, refresh, and ledger the list.

```bash
git add -A
git commit -m "feat: the look: exposure and tonemap curves

- Look resource (ctx.look()) with exposure and Soft, Clamp, ACES and AgX curves
- the default Soft curve keeps colours below the knee and rolls off above it"
```

### Task 5: Colour grading and the LUT

**Files:**
- Modify:
  - `src/core/render/look.rs` (`ColorGrading`, `ColorLut`, `LutError`, `Look.grading`, the uniforms)
  - `src/core/render/mod.rs`, `src/lib.rs` (exports)
  - `src/core/shaders/core/tonemap.wgsl`
  - `src/core/render/passes/tonemap.rs` (`LutTexture`, bindings 2 and 3)
- Test: `src/core/render/look.rs`, `src/core/render/passes/tonemap.rs`, `tests/e2e/look.rs`

**Interfaces:**
- Consumes: Task 4's `Look`, `LookUniforms`, `TonemapGpu`.
- Produces:
  - `#[derive(Debug, Clone, PartialEq)] pub struct ColorGrading { pub temperature: f32, pub tint: f32, pub contrast: f32, pub saturation: f32, pub lift: Vec3, pub gamma: Vec3, pub gain: Vec3, pub lut: Option<ColorLut>, pub lut_strength: f32 }`. `Default` is neutral: 0, 0, 1, 1, 0, 1, 1, `None`, 1.
  - `Look { exposure, tonemap, grading: ColorGrading }`.
  - `#[derive(Debug, Clone)] pub struct ColorLut` with:
    - `pub fn from_sprite(&SpriteHandle) -> Result<ColorLut, LutError>`;
    - `pub fn size(&self) -> u32`;
    - `pub fn identity_strip(size: u32) -> Sprite`.
  - `PartialEq` for `ColorLut`: the same sprite (`serial()`) and size.
  - `#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)] pub enum LutError { WrongSize { width: u32, height: u32 } }`.
  - `LookUniforms { curve, balance, grade, lift, inv_gamma, gain }` (six `[f32; 4]`, 96 bytes).
  - `Look::uniforms(&self, lut_size: u32) -> LookUniforms`: `lut_size` is the N of the LUT the GPU holds, 0 for none.

- [ ] **Step 1: Write the failing tests**

In `src/core/render/look.rs` tests:

```rust
  #[test]
  fn a_lut_strip_is_n_tiles_of_n_by_n() {
    let lut = |w: u32, h: u32| {
      let sprite = crate::core::asset::SpriteHandle::from_sprite(
        crate::core::graphics::Sprite::solid_rect(w, h, 0, 0, 0, 255),
        format!("lut-{w}x{h}"),
      );
      ColorLut::from_sprite(&sprite).map(|lut| lut.size())
    };
    assert_eq!(lut(256, 16), Ok(16));
    assert_eq!(lut(1024, 32), Ok(32));
    assert_eq!(lut(16, 16), Err(LutError::WrongSize { width: 16, height: 16 }));
    assert_eq!(lut(1, 1), Err(LutError::WrongSize { width: 1, height: 1 }));
  }

  #[test]
  fn the_identity_strip_is_a_lut() {
    let strip = ColorLut::identity_strip(4);
    assert_eq!((strip.width(), strip.height()), (16, 4));
    // Tile b = 3, red 2, green 1: x = 3 * 4 + 2, y = 1
    let at = (1 * 16 + 3 * 4 + 2) * 4;
    assert_eq!(&strip.data()[at..at + 4], &[170, 85, 255, 255]);
  }

  #[test]
  fn neutral_grading_is_flagged_off() {
    assert_eq!(Look::default().uniforms(0).grade[3], 0.0);
    let warm = Look {
      grading: ColorGrading {
        temperature: 0.2,
        ..ColorGrading::default()
      },
      ..Look::default()
    };
    assert_eq!(warm.uniforms(0).grade[3], 1.0);
  }

  #[test]
  fn grading_uniforms_are_finite_whatever_the_settings() {
    let grading = ColorGrading {
      temperature: f32::NAN,
      tint: f32::INFINITY,
      contrast: 0.0,
      saturation: -3.0,
      lift: Vec3::splat(f32::NAN),
      gamma: Vec3::ZERO,
      gain: Vec3::splat(f32::INFINITY),
      lut: None,
      lut_strength: f32::NAN,
    };
    let u = Look {
      grading,
      ..Look::default()
    }
    .uniforms(0);
    let all = [u.curve, u.balance, u.grade, u.lift, u.inv_gamma, u.gain].concat();
    assert!(all.iter().all(|v| v.is_finite()), "{u:?}");
    assert_eq!(u.grade[1], 0.0, "no LUT, no strength");
  }
```

In `src/core/render/passes/tonemap.rs`, add a `#[cfg(test)] mod tests`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::SpriteHandle;
  use crate::core::graphics::Sprite;
  use crate::core::render::ColorLut;

  #[test]
  fn a_lut_reloaded_at_the_wrong_size_keeps_the_last_good_one() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let handle = SpriteHandle::from_sprite(ColorLut::identity_strip(4), "lut-reload");
    let Ok(lut) = ColorLut::from_sprite(&handle) else {
      panic!("a 16x4 strip is a LUT");
    };
    let mut texture = LutTexture::new(&device, &queue);
    assert_eq!(texture.sync(&device, &queue, Some(&lut)), 4);
    let before = texture.view.clone();
    handle.update(Sprite::solid_rect(5, 5, 0, 0, 0, 255));
    assert_eq!(texture.sync(&device, &queue, Some(&lut)), 4, "keeps the last good LUT");
    assert_eq!(texture.view, before);
    assert_eq!(texture.sync(&device, &queue, None), 0, "no LUT, nothing applied");
  }
}
```

In `tests/e2e/look.rs`:

```rust
fn graded(grading: ColorGrading) -> Look {
  Look {
    grading,
    ..Look::default()
  }
}

#[test]
fn zero_saturation_turns_red_grey() {
  let look = graded(ColorGrading {
    saturation: 0.0,
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(square([220, 30, 40, 255], look)) else {
    return;
  };
  assert!(pixel[0].abs_diff(pixel[1]) <= 1 && pixel[1].abs_diff(pixel[2]) <= 1, "{pixel:?}");
}

#[test]
fn a_warm_temperature_warms_grey() {
  let look = graded(ColorGrading {
    temperature: 0.5,
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(square([128, 128, 128, 255], look)) else {
    return;
  };
  assert!(pixel[0] > pixel[2] + 10, "{pixel:?}");
}

#[test]
fn lift_raises_black() {
  let look = graded(ColorGrading {
    lift: Vec3::splat(0.1),
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(square([0, 0, 0, 255], look)) else {
    return;
  };
  assert!(pixel[0].abs_diff(encode(0.1)) <= 1, "{pixel:?}");
}

/// A LUT that inverts every sRGB channel.
fn inverting_lut() -> ColorLut {
  let mut strip = ColorLut::identity_strip(16);
  let inverted: Vec<u8> = strip
    .data()
    .chunks(4)
    .flat_map(|p| [255 - p[0], 255 - p[1], 255 - p[2], 255])
    .collect();
  strip = Sprite::from_rgba(inverted, strip.width(), strip.height());
  let Ok(lut) = ColorLut::from_sprite(&SpriteHandle::from_sprite(strip, "lut-invert")) else {
    panic!("a 256x16 strip is a LUT");
  };
  lut
}

#[test]
fn an_inverting_lut_inverts() {
  let look = graded(ColorGrading {
    lut: Some(inverting_lut()),
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(square([200, 100, 50, 255], look)) else {
    return;
  };
  for (got, want) in pixel.iter().zip([55u8, 155, 205]) {
    assert!(got.abs_diff(want) <= 2, "{pixel:?}");
  }
}

#[test]
fn an_identity_lut_changes_nothing() {
  let strip = SpriteHandle::from_sprite(ColorLut::identity_strip(16), "lut-identity");
  let look = graded(ColorGrading {
    lut: ColorLut::from_sprite(&strip).ok(),
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(square([200, 100, 50, 255], look)) else {
    return;
  };
  for (got, want) in pixel.iter().zip([200u8, 100, 50]) {
    assert!(got.abs_diff(want) <= 1, "{pixel:?}");
  }
}

#[test]
fn half_strength_blends_halfway() {
  let look = graded(ColorGrading {
    lut: Some(inverting_lut()),
    lut_strength: 0.5,
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(square([200, 100, 50, 255], look)) else {
    return;
  };
  assert!(pixel[0].abs_diff(128) <= 2, "{pixel:?}");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib look 2>&1 | tail -3; cargo test --test e2e look:: 2>&1 | tail -3`

Expected: compile errors (`ColorGrading`, `ColorLut`, `LutTexture` missing).

- [ ] **Step 3: Grading and the LUT type**

In `src/core/render/look.rs`:
- `Look` gains `/// Colour grading (default neutral)` `pub grading: ColorGrading,` (and `grading: ColorGrading::default()` in `Default`).
- Its doc lists the pass order: "exposure; white balance, contrast and saturation in linear HDR; the curve; lift, gamma and gain on the display colour; the LUT in sRGB".

```rust
/// Colour grading, applied by the tonemap pass. White balance, contrast and
/// saturation work on the HDR light before the curve; lift, gamma and gain
/// on the display colour after it; the LUT last, in sRGB. The default
/// changes nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorGrading {
  /// Warmer (positive) or cooler (negative), -1 to 1
  pub temperature: f32,
  /// Toward magenta (positive) or green (negative), -1 to 1
  pub tint: f32,
  /// Contrast about mid-grey (0.18) in log2 space; 1 is neutral
  pub contrast: f32,
  /// 0 is grey, 1 neutral, above 1 more vivid
  pub saturation: f32,
  /// Raises the shadows of the display colour, per channel; 0 is neutral
  pub lift: Vec3,
  /// Bends the mid-tones, per channel; 1 is neutral, above 1 brightens
  pub gamma: Vec3,
  /// Scales the display colour, per channel; 1 is neutral
  pub gain: Vec3,
  /// A colour lookup table, applied last
  pub lut: Option<ColorLut>,
  /// How much of the LUT's result shows, 0 to 1
  pub lut_strength: f32,
}

impl Default for ColorGrading {
  fn default() -> Self {
    Self {
      temperature: 0.0,
      tint: 0.0,
      contrast: 1.0,
      saturation: 1.0,
      lift: Vec3::ZERO,
      gamma: Vec3::ONE,
      gain: Vec3::ONE,
      lut: None,
      lut_strength: 1.0,
    }
  }
}

/// A colour lookup table: a strip of N tiles of N x N side by side (1024x32
/// for N = 32, 256x16 for N = 16). Tile `b` holds blue `b / (N - 1)`; within
/// a tile red grows to the right and green downward. Lookups are in sRGB,
/// where LUTs are authored. It follows its sprite's hot reload.
#[derive(Debug, Clone)]
pub struct ColorLut {
  sprite: SpriteHandle,
  size: u32,
}

impl PartialEq for ColorLut {
  fn eq(&self, other: &Self) -> bool {
    self.size == other.size && self.sprite.serial() == other.sprite.serial()
  }
}

/// Why a sprite is not a [`ColorLut`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LutError {
  /// A strip is N x N tiles side by side: N * N wide, N high, N from 2 to 64
  #[error("a LUT strip is N*N wide and N high (N from 2 to 64); this sprite is {width}x{height}")]
  WrongSize { width: u32, height: u32 },
}

/// N for a `width` x `height` strip, if it is one.
pub(crate) fn strip_size(width: u32, height: u32) -> Option<u32> {
  ((2..=64).contains(&height) && width == height * height).then_some(height)
}

impl ColorLut {
  /// The LUT `sprite` holds as a strip.
  pub fn from_sprite(sprite: &SpriteHandle) -> Result<Self, LutError> {
    let (width, height) = (sprite.width(), sprite.height());
    let size = strip_size(width, height).ok_or(LutError::WrongSize { width, height })?;
    Ok(Self {
      sprite: sprite.clone(),
      size,
    })
  }

  /// N: the strip's height.
  pub fn size(&self) -> u32 {
    self.size
  }

  /// The strip's sprite.
  pub(crate) fn sprite(&self) -> &SpriteHandle {
    &self.sprite
  }

  /// The strip that changes nothing, N = `size` (2 to 64): a starting point
  /// for LUTs built in code.
  pub fn identity_strip(size: u32) -> Sprite {
    let n = size.clamp(2, 64);
    let level = |i: u32| ((i * 255 + (n - 1) / 2) / (n - 1)) as u8;
    let mut data = Vec::with_capacity((n * n * n * 4) as usize);
    for g in 0..n {
      for b in 0..n {
        for r in 0..n {
          data.extend_from_slice(&[level(r), level(g), level(b), 255]);
        }
      }
    }
    Sprite::from_rgba(data, n * n, n)
  }
}
```

Import `crate::core::asset::SpriteHandle` and `crate::core::graphics::Sprite` (use the paths `src/core` already uses).

Extend `LookUniforms` and `uniforms`:

```rust
/// The look as the tonemap shader reads it (group 0, binding 1).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LookUniforms {
  /// x: exposure (EV), y: curve (0 soft, 1 clamp, 2 ACES, 3 AgX), z: knee,
  /// w: white
  pub(crate) curve: [f32; 4],
  /// White balance multipliers (rgb), w: contrast
  pub(crate) balance: [f32; 4],
  /// x: saturation, y: LUT strength (0 without one), z: the LUT's N, w: 1
  /// when the grading is not neutral
  pub(crate) grade: [f32; 4],
  pub(crate) lift: [f32; 4],
  /// 1 / gamma
  pub(crate) inv_gamma: [f32; 4],
  pub(crate) gain: [f32; 4],
}

/// `v` if finite, else `default`, then clamped.
fn num(v: f32, default: f32, min: f32, max: f32) -> f32 {
  if v.is_finite() { v.clamp(min, max) } else { default }
}

fn per_channel(v: Vec3, default: f32, min: f32, max: f32) -> [f32; 4] {
  [
    num(v.x, default, min, max),
    num(v.y, default, min, max),
    num(v.z, default, min, max),
    0.0,
  ]
}

/// Luminance weights of linear sRGB (Rec. 709).
const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

/// Multipliers that warm or cool and tint, keeping a grey's luminance.
fn white_balance(temperature: f32, tint: f32) -> Vec3 {
  let m = Vec3::new(1.0 + 0.25 * temperature, 1.0 - 0.25 * tint, 1.0 - 0.25 * temperature);
  m / m.dot(LUMA)
}

impl Look {
  /// What the tonemap shader reads, every parameter sanitized; `lut_size`
  /// is the N of the LUT the GPU holds (0 for none).
  pub(crate) fn uniforms(&self, lut_size: u32) -> LookUniforms {
    let exposure = num(self.exposure, 0.0, -16.0, 16.0);
    let (curve, knee, white) = match self.tonemap.sanitized() {
      Tonemap::Soft { knee, white } => (0.0, knee, white),
      Tonemap::Clamp => (1.0, 0.0, 1.0),
      Tonemap::Aces => (2.0, 0.0, 1.0),
      Tonemap::AgX => (3.0, 0.0, 1.0),
    };
    let g = &self.grading;
    let temperature = num(g.temperature, 0.0, -1.0, 1.0);
    let tint = num(g.tint, 0.0, -1.0, 1.0);
    let contrast = num(g.contrast, 1.0, 0.01, 10.0);
    let saturation = num(g.saturation, 1.0, 0.0, 4.0);
    let lift = per_channel(g.lift, 0.0, -1.0, 1.0);
    let gamma = per_channel(g.gamma, 1.0, 0.01, 10.0);
    let gain = per_channel(g.gain, 1.0, 0.0, 10.0);
    let neutral = temperature == 0.0
      && tint == 0.0
      && contrast == 1.0
      && saturation == 1.0
      && lift[..3] == [0.0; 3]
      && gamma[..3] == [1.0; 3]
      && gain[..3] == [1.0; 3];
    let balance = white_balance(temperature, tint);
    let strength = if lut_size > 0 {
      num(g.lut_strength, 1.0, 0.0, 1.0)
    } else {
      0.0
    };
    LookUniforms {
      curve: [exposure, curve, knee, white],
      balance: [balance.x, balance.y, balance.z, contrast],
      grade: [
        saturation,
        strength,
        lut_size as f32,
        if neutral { 0.0 } else { 1.0 },
      ],
      lift,
      inv_gamma: [1.0 / gamma[0], 1.0 / gamma[1], 1.0 / gamma[2], 0.0],
      gain,
    }
  }
}
```

Task 4's `uniforms_are_sanitized` test calls `uniforms(0)`.

- [ ] **Step 4: The shader**

In `tonemap.wgsl`, the `Look` struct mirrors `LookUniforms`:

```wgsl
struct Look {
    // x: exposure (EV), y: curve (0 soft, 1 clamp, 2 ACES, 3 AgX), z: knee, w: white
    curve: vec4<f32>,
    // White balance multipliers (rgb), w: contrast
    balance: vec4<f32>,
    // x: saturation, y: LUT strength (0 without one), z: the LUT's N, w: 1 when grading is on
    grade: vec4<f32>,
    lift: vec4<f32>,
    inv_gamma: vec4<f32>,
    gain: vec4<f32>,
}

@group(0) @binding(2) var lut: texture_3d<f32>;
@group(0) @binding(3) var lut_sampler: sampler;
```

Add the two grading stages and use them in `fs_main`:

```wgsl
// White balance, contrast about mid-grey in log2 space and saturation, on
// the HDR light
fn balanced(c: vec3<f32>) -> vec3<f32> {
    if look.grade.w < 0.5 {
        return c;
    }
    let x = c * look.balance.rgb;
    let contrasted = select(
        vec3<f32>(0.0),
        0.18 * pow(x / 0.18, vec3<f32>(look.balance.w)),
        x > vec3<f32>(0.0),
    );
    let luma = dot(contrasted, vec3<f32>(0.2126, 0.7152, 0.0722));
    return max(mix(vec3<f32>(luma), contrasted, look.grade.x), vec3<f32>(0.0));
}

// Lift, gamma and gain on the display colour, then the LUT in sRGB
fn graded(display: vec3<f32>) -> vec3<f32> {
    var d = display;
    if look.grade.w > 0.5 {
        d = look.gain.rgb * (d + look.lift.rgb * (vec3<f32>(1.0) - d));
        d = clamp(pow(max(d, vec3<f32>(0.0)), look.inv_gamma.rgb), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if look.grade.y > 0.0 {
        let s = linear_to_srgb(d);
        let n = look.grade.z;
        let uvw = (s * (n - 1.0) + 0.5) / n;
        let mapped = textureSampleLevel(lut, lut_sampler, uvw, 0.0).rgb;
        d = srgb_to_linear(mix(s, mapped, look.grade.y));
    }
    return d;
}

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    let texel = textureLoad(scene, vec2<i32>(in.position.xy), 0);
    let exposed = finite(texel.rgb) * exp2(look.curve.x);
    let display = graded(curve(balanced(exposed)));
    return vec4<f32>(display, clamp(texel.a, 0.0, 1.0));
}
```

Neutral grading skips both stages, so Task 4's byte-exact tests hold.

- [ ] **Step 5: The LUT on the GPU**

In `src/core/render/passes/tonemap.rs`:

```rust
/// The LUT on the GPU: a 3D texture of the strip's sRGB colours, and which
/// sprite version it holds.
struct LutTexture {
  view: wgpu::TextureView,
  /// N of the LUT held (2 while it is the identity placeholder)
  size: u32,
  /// The sprite and generation uploaded, or `None` for the placeholder
  source: Option<(usize, u64)>,
  /// The sprite and generation already warned about
  warned: Option<(usize, u64)>,
}

impl LutTexture {
  /// An identity placeholder, so the bind group always has a LUT.
  fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
    let strip = crate::core::render::ColorLut::identity_strip(2);
    Self {
      view: upload_lut(device, queue, strip.data(), 2),
      size: 2,
      source: None,
      warned: None,
    }
  }

  /// Follow `lut`: upload a new or reloaded strip; keep the last good one
  /// (warning once) if it is no longer a strip. Returns the N to apply, 0
  /// without a LUT.
  fn sync(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    lut: Option<&crate::core::render::ColorLut>,
  ) -> u32 {
    let Some(lut) = lut else {
      return 0;
    };
    let handle = lut.sprite();
    let key = (handle.serial(), handle.generation());
    if self.source != Some(key) && self.warned != Some(key) {
      let sprite = handle.sprite();
      match crate::core::render::look::strip_size(sprite.width(), sprite.height()) {
        Some(n) => {
          self.view = upload_lut(device, queue, sprite.data(), n);
          self.size = n;
          self.source = Some(key);
        }
        None => {
          crate::engine_warn!(
            module: "Render",
            "LUT sprite {} is {}x{}, not a strip; keeping the last good LUT",
            handle.path().display(),
            sprite.width(),
            sprite.height()
          );
          self.warned = Some(key);
        }
      }
    }
    if self.source.is_some() { self.size } else { 0 }
  }
}

/// A 3D texture of the `n` x `n` x `n` LUT a strip's `rgba` bytes hold.
fn upload_lut(device: &wgpu::Device, queue: &wgpu::Queue, rgba: &[u8], n: u32) -> wgpu::TextureView {
  let size = wgpu::Extent3d {
    width: n,
    height: n,
    depth_or_array_layers: n,
  };
  let texture = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Look LUT"),
    size,
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D3,
    format: wgpu::TextureFormat::Rgba8Unorm,
    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    view_formats: &[],
  });
  // Slice b of the volume is tile b of the strip
  let row = (n * 4) as usize;
  let mut volume = Vec::with_capacity(row * (n * n) as usize);
  for b in 0..n as usize {
    for g in 0..n as usize {
      let start = (g * n as usize * n as usize + b * n as usize) * 4;
      volume.extend_from_slice(&rgba[start..start + row]);
    }
  }
  queue.write_texture(
    texture.as_image_copy(),
    &volume,
    wgpu::TexelCopyBufferLayout {
      offset: 0,
      bytes_per_row: Some(n * 4),
      rows_per_image: Some(n),
    },
    size,
  );
  texture.create_view(&wgpu::TextureViewDescriptor::default())
}
```

`strip_size` is `pub(crate)` in `look.rs` (a `pub(crate) mod` since Task 4). `handle.sprite()` holds a read lock: read the size and bytes, then drop it before uploading if the borrow checker asks. `SpriteHandle::serial()` and `generation()` are `pub(crate)`.

`TonemapGpu` gains `lut: LutTexture` and a `lut_sampler`:

```rust
    let lut_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Look LUT Sampler"),
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      ..Default::default()
    });
```

`TonemapGpu::new` takes `queue` too. The layout gains binding 2 (`Texture { sample_type: Float { filterable: true }, view_dimension: D3, multisampled: false }`) and binding 3 (`Sampler(Filtering)`).

In `execute`:
- `let lut_size = gpu.lut.sync(device, queue, ctx.scene.look.grading.lut.as_ref());`
- write `ctx.scene.look.uniforms(lut_size)`;
- bind `[View(source), Buffer(uniforms), View(gpu.lut.view.clone()), Sampler(gpu.lut_sampler.clone())]`.

A new LUT view makes one new cached bind group, not one per frame.

- [ ] **Step 6: Export and run the tests**

Export `ColorGrading`, `ColorLut`, `LutError` from `crate::core::render` and the prelude.

Run: `cargo test --lib look 2>&1 | grep 'test result'; cargo test --lib a_lut_reloaded 2>&1 | grep 'test result'; cargo test --test e2e look:: 2>&1 | grep -E 'panicked|test result'`

Expected: all pass.

- [ ] **Step 7: The gate and commit**

Expected: `compare 0` (grading is neutral by default).

```bash
git add -A
git commit -m "feat: colour grading and LUTs in the look

- white balance, contrast and saturation in HDR; lift, gamma and gain after the curve
- ColorLut from a strip sprite, applied in sRGB and following hot reload"
```

---

## Phase 4: effect stages (spec migration step 4)

### Task 6: The HDR and display effect stages

**Files:**
- Modify:
  - `src/core/shaders/effects.rs` (`EffectStage`, `PostProcessEffect::stage`)
  - `src/core/shaders/manager.rs` (stage per custom effect, `add_custom_in`, per-stage queries)
  - `src/core/shaders/mod.rs`, `src/lib.rs` (export `EffectStage`)
  - `src/core/render/mod.rs` (`pass_ids::HDR_EFFECTS`)
  - `src/core/render/frame.rs` (`FrameInfo.hdr_effects`, `FrameImage::Hdr`)
  - `src/core/render/context.rs` (`EffectStages.hdr`, the lookups)
  - `src/core/render/passes/{mod,post_process}.rs`
  - `src/core/graphics/renderer.rs` (`hdr_effects` pipeline)
- Test: `tests/integration/render_pipeline.rs`, `tests/e2e/post_process.rs`, `src/core/shaders/manager.rs`

**Interfaces:**
- Consumes: Task 1's `EffectStages`, `FrameImage::Display`, the `PostProcess` pass.
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum EffectStage { Hdr, Display }`.
  - `pub fn PostProcessEffect::stage(&self) -> EffectStage`: `Bloom`, `GodRays` and `AmbientOcclusion` are `Hdr`; the rest are `Display`.
  - `pub fn EffectsManager::add_custom_in(&mut self, name: impl Into<String>, shader: ShaderHandle, stage: EffectStage) -> Result<(), EffectError>`. `add_custom` is `add_custom_in(.., EffectStage::Display)`.
  - `pub fn EffectsManager::active_effects_in(&self, stage: EffectStage) -> impl Iterator<Item = &PostProcessEffect>`.
  - `pub fn EffectsManager::has_active_effects_in(&self, stage: EffectStage) -> bool`.
  - `pub(crate) fn EffectsManager::enabled_custom_in(&self, stage: EffectStage) -> impl Iterator<Item = &CustomEffect>`.
  - `pass_ids::HDR_EFFECTS: &str = "hdr_effects"`, between `cameras` and `tonemap`.
  - `pub hdr_effects: bool` on `FrameInfo` ("HDR-stage effects are active"). `effects` now means display-stage effects.
  - `FrameImage::Hdr(OutputBuffer)`.
  - `EffectStages { hdr: &'a mut PostProcessPipeline, display: .. }`.
  - `WgpuRenderer.hdr_effects: PostProcessPipeline` in `SCENE_FORMAT`.

- [ ] **Step 1: Write the failing tests**

In `tests/integration/render_pipeline.rs`:
- `ORDER` becomes 12 entries, with `pass_ids::HDR_EFFECTS` between `CAMERAS` and `TONEMAP`.
- `a_full_2d_frame_runs_every_scene_pass` sets `hdr_effects: true` and expects `"hdr_effects"` between `"cameras"` and `"tonemap"`.
- The always-on list is unchanged.

In `src/core/shaders/manager.rs` tests:

```rust
  #[test]
  fn effects_report_their_stage() {
    let mut manager = EffectsManager::new();
    manager.set_bloom(1.0);
    manager.set_vignette(0.5);
    let hdr: Vec<_> = manager.active_effects_in(EffectStage::Hdr).collect();
    let display: Vec<_> = manager.active_effects_in(EffectStage::Display).collect();
    assert!(matches!(hdr[..], [PostProcessEffect::Bloom { .. }]));
    assert!(matches!(display[..], [PostProcessEffect::Vignette { .. }]));
    assert!(manager.has_active_effects_in(EffectStage::Hdr));
  }

  #[test]
  fn custom_effects_run_in_the_stage_they_were_added_to() {
    let mut manager = EffectsManager::new();
    let shader = ShaderHandle::from_source("stage.wgsl", "fn effect(in: EffectIn, params: Params) -> vec4<f32> { return scene(in.uv); }");
    manager.add_custom("late", shader.clone()).unwrap();
    manager.add_custom_in("early", shader, EffectStage::Hdr).unwrap();
    let names = |stage| manager.enabled_custom_in(stage).map(|c| c.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(EffectStage::Hdr), ["early"]);
    assert_eq!(names(EffectStage::Display), ["late"]);
  }
```

In `tests/e2e/post_process.rs`:

```rust
/// `BrightLine` with bloom (threshold 0.5) and the display stage switched off.
struct BloomWithoutPostProcess;

impl Scene for BloomWithoutPostProcess {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    BrightLine { bloom: false }.on_enter(ctx);
    ctx.effects().set_bloom_custom(0.5, 1.0, 5.0, 2);
    let _ = ctx.render_pipeline().set_enabled(pass_ids::POST_PROCESS, false);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn bloom_survives_switching_post_process_off() {
  let (Some(plain), Some(mut engine)) = (pixel_right_of_line(false), engine(BloomWithoutPostProcess)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let bloomed = engine.frame().unwrap().get_pixel(38, 32).0[0];
  assert!(bloomed > plain + 5, "plain {plain}, bloomed {bloomed}");
}

const QUARTER: &str = "
fn effect(in: EffectIn, params: Params) -> vec4<f32> {
  return vec4<f32>(scene(in.uv).rgb * 0.25, 1.0);
}
";

/// Two additive white squares (2.0 in linear light) under the clamp curve,
/// a quarter of them kept by a custom effect in `stage`.
struct Quartered(EffectStage);

impl Scene for Quartered {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let white = SpriteHandle::from_sprite(Sprite::solid_rect(64, 64, 255, 255, 255, 255), "quarter-white");
    for z in 0..2 {
      ctx.world.spawn((
        Transform::from_xy(32.0, 32.0),
        Renderable::sprite(white.clone())
          .with_z_order(z)
          .with_blend(BlendMode::Additive),
      ));
    }
    ctx.look().tonemap = Tonemap::Clamp;
    let shader = ShaderHandle::from_source("quarter.wgsl", QUARTER);
    let _ = ctx.effects().add_custom_in("quarter", shader, self.0);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn quartered(stage: EffectStage) -> Option<u8> {
  let mut engine = engine(Quartered(stage))?;
  // Custom effects compile asynchronously: give them a few frames
  engine.step_frames(4).unwrap();
  Some(engine.frame().unwrap().get_pixel(32, 32).0[0])
}

#[test]
fn an_hdr_custom_effect_sees_light_above_white() {
  let (Some(hdr), Some(display)) = (quartered(EffectStage::Hdr), quartered(EffectStage::Display)) else {
    return;
  };
  // About 2.03 x 0.25 = 0.51 in HDR (sRGB 189); 1.0 x 0.25 after the clamp (sRGB 137)
  assert!(hdr.abs_diff(189) <= 3, "hdr {hdr}");
  assert!(display.abs_diff(137) <= 3, "display {display}");
}
```

If custom effects already settle within two frames in `tests/e2e/custom_shaders.rs`, keep their convention for the frame count.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test integration render_pipeline 2>&1 | tail -3; cargo test --lib effects_report 2>&1 | tail -3`

Expected: compile errors (`HDR_EFFECTS`, `EffectStage` missing).

- [ ] **Step 3: Stages on effects**

In `src/core/shaders/effects.rs`:

```rust
/// Where an effect runs: on the HDR scene before the tonemap, or on the
/// display image after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectStage {
  /// Linear light that may exceed 1: bloom, god rays, ambient occlusion
  Hdr,
  /// The display image, linear 0 to 1: fade, tint, vignette, flash, CRT,
  /// chromatic aberration
  Display,
}

impl PostProcessEffect {
  /// The stage this effect runs in.
  pub fn stage(&self) -> EffectStage {
    match self {
      Self::Bloom { .. } | Self::GodRays { .. } | Self::AmbientOcclusion { .. } => EffectStage::Hdr,
      _ => EffectStage::Display,
    }
  }
}
```

Put `stage` inside the existing `impl PostProcessEffect`.

In `src/core/shaders/manager.rs`:

```rust
  /// The active effects that run in `stage`, in order.
  pub fn active_effects_in(&self, stage: EffectStage) -> impl Iterator<Item = &PostProcessEffect> {
    self.active_effects().filter(move |effect| effect.stage() == stage)
  }

  /// Whether any effect (built-in or custom) runs in `stage`.
  pub fn has_active_effects_in(&self, stage: EffectStage) -> bool {
    self.active_effects_in(stage).next().is_some() || self.enabled_custom_in(stage).next().is_some()
  }

  /// Add a game effect that runs in `stage`: `Hdr` sees linear light that
  /// may exceed 1, `Display` the tonemapped image.
  pub fn add_custom_in(
    &mut self,
    name: impl Into<String>,
    shader: ShaderHandle,
    stage: EffectStage,
  ) -> Result<(), EffectError> {
    let name = name.into();
    if self.has_custom(&name) {
      return Err(EffectError::DuplicateName(name));
    }
    self.custom.push(CustomEffect {
      name,
      shader,
      params: ShaderParams::new(),
      enabled: true,
      stage,
    });
    Ok(())
  }

  /// The enabled game effects of `stage`, in insertion order.
  pub(crate) fn enabled_custom_in(&self, stage: EffectStage) -> impl Iterator<Item = &CustomEffect> {
    self.custom.iter().filter(move |c| c.enabled && c.stage == stage)
  }
```

Also:
- `CustomEffect` gains `stage: EffectStage`.
- `add_custom` becomes `self.add_custom_in(name, shader, EffectStage::Display)` (its doc says it runs on the display image).
- `enabled_custom` is removed if nothing else uses it.
- Export `EffectStage` from `shaders` and the prelude.

- [ ] **Step 4: The HDR stage in the frame**

1. `src/core/render/mod.rs`, after `CAMERAS`:

```rust
  /// HDR effects (bloom, god rays, ambient occlusion, HDR custom effects)
  /// on the scene image
  pub const HDR_EFFECTS: &str = "hdr_effects";
```

   `POST_PROCESS`'s doc becomes "Display effects (fade, tint, vignette, flash, CRT, chromatic aberration, display custom effects) on the display image".
2. `src/core/render/frame.rs`:
   - `FrameInfo` gains `/// HDR-stage effects are active` `pub hdr_effects: bool,`, and `effects`' doc becomes "Display-stage effects are active".
   - `FrameImage` gains `/// A buffer of the HDR effect stage` `Hdr(OutputBuffer),`.
3. `src/core/render/context.rs`:
   - `EffectStages` gains `/// The HDR stage, on the scene image` `pub(crate) hdr: &'a mut PostProcessPipeline,`.
   - `image_view` gains `FrameImage::Hdr(buffer) => stages.hdr.get_output_view(buffer),`.
   - `image_texture` gains the `Hdr(A)`/`Hdr(B)` arms reading `stages.hdr.texture_a()`/`texture_b()`.
4. `src/core/graphics/renderer.rs`:
   - Add `hdr_effects: PostProcessPipeline` (doc: `// The HDR effect stage, in the scene format`), built with `PostProcessPipeline::new(&device, game_width, game_height, crate::core::render::SCENE_FORMAT)`, and resized with `display_effects`.
   - `render_frame` sets `hdr_effects: scene.effects.has_active_effects_in(EffectStage::Hdr)` and `effects: scene.effects.has_active_effects_in(EffectStage::Display)`.
   - Pass `EffectStages { hdr: &mut self.hdr_effects, display: &mut self.display_effects }`.
5. `src/core/render/passes/post_process.rs`:
   - `PostProcess` collects `effects.active_effects_in(EffectStage::Display)` and `effects.enabled_custom_in(EffectStage::Display)`.
   - Add the HDR pass:

```rust
/// Runs the HDR-stage effects on the scene image (the lit one when lighting
/// ran); the chain's output becomes the latest image.
pub(super) struct HdrEffects;

impl RenderPass for HdrEffects {
  fn id(&self) -> &str {
    pass_ids::HDR_EFFECTS
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.hdr_effects
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (device, queue) = (ctx.device(), ctx.queue());
    let (width, height) = ctx.render_size();
    let effects = ctx.scene.effects;
    let time = effects.time();
    let active: Vec<_> = effects
      .active_effects_in(EffectStage::Hdr)
      .map(|effect| (effect, time))
      .collect();
    let custom: Vec<_> = effects.enabled_custom_in(EffectStage::Hdr).collect();
    if active.is_empty() && custom.is_empty() {
      return;
    }
    let RenderContext {
      encoder,
      targets,
      stages,
      image,
      ..
    } = ctx;
    match *image {
      // Already in the chain's input
      FrameImage::Hdr(OutputBuffer::A) => {}
      // Past the tonemap: no HDR image to work on
      FrameImage::Display(_) => return,
      source => {
        let texture = image_texture(targets, stages, source);
        encoder.copy_texture_to_texture(
          texture.as_image_copy(),
          stages.hdr.texture_a().as_image_copy(),
          wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
          },
        );
      }
    }
    let output = stages.hdr.apply_chain(device, queue, encoder, &active, &custom, time);
    *image = FrameImage::Hdr(output);
  }
}
```

6. `src/core/render/passes/mod.rs`: `built_in()` returns 12 passes, with `Box::new(post_process::HdrEffects),` between `Cameras` and `Tonemap`.

- [ ] **Step 5: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:|panicked' | head -20`

Expected: all pass, including `steady_frames_create_no_bind_groups` and `a_resize_rebuilds_bind_groups_once` in `src/core/testing/gpu_world.rs`. Their `Busy` scene runs bloom (HDR) and a vignette (display), so they cover both chains.

- [ ] **Step 6: The gate and commit**

Expected pages: bloom, god-ray and ambient-occlusion pages (bloom, post-processing, lighting-volumetric, the shadows demo's ambient occlusion page). These effects now run on the float image before the tonemap. Check them by eye, refresh, and ledger the list.

```bash
git add -A
git commit -m "feat: HDR and display effect stages

- bloom, god rays and ambient occlusion run on the HDR scene before the tonemap
- fade, tint, vignette, flash, CRT and chromatic aberration on the display image
- add_custom_in chooses a custom effect's stage"
```

### Task 7: Bloom and god rays in HDR, and god rays placed on the whole frame

**Files:**
- Create: `src/core/lighting/god_rays.rs`
- Modify:
  - `src/core/shaders/post-process/bloom_extract.wgsl`, `src/core/shaders/post-process/god_rays.wgsl`
  - `src/core/shaders/effects.rs` (thresholds and docs)
  - `src/core/shaders/manager.rs` (docs)
  - `src/core/lighting/atmosphere.rs` (threshold doc)
  - `src/core/lighting/mod.rs`, `src/core/lighting/renderer.rs` (`update_god_rays` moves out)
  - `src/core/engine/extract.rs`
- Test: `src/core/lighting/god_rays.rs`, `tests/e2e/post_process.rs`, `src/core/shaders/effects.rs`

**Interfaces:**
- Consumes: Task 6's HDR stage.
- Produces:
  - `pub(crate) fn god_ray_positions<'a>(lights: impl Iterator<Item = (Vec2, &'a Light2D)>, view: &CameraView, screen: Vec2) -> Vec<(f32, f32)>`.
  - `pub(crate) fn update_god_rays<'a>(effects: &mut EffectsManager, lights: impl Iterator<Item = (Vec2, &'a Light2D)>, view: &CameraView, screen: Vec2, atmosphere: &Atmosphere)`.
  - Both live in `lighting::god_rays` and replace `LightingRenderer::update_god_rays`.
  - `PostProcessEffect::bloom(intensity)` has threshold 1.0. Bloom and god-ray thresholds are `max(0.0)`, never clamped to 1.

- [ ] **Step 1: Write the failing tests**

`src/core/lighting/god_rays.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::{Camera2d, Viewport};

  #[test]
  fn a_light_lands_where_a_half_window_camera_shows_it() {
    // The main camera fills the left half of a 200x100 screen, centred on (0, 0)
    let camera = Camera2d {
      viewport: Viewport::new(0.0, 0.0, 0.5, 1.0),
      ..Default::default()
    };
    let screen = Vec2::new(200.0, 100.0);
    let view = CameraView::compute(Vec2::ZERO, 0.0, &camera, Vec2::ZERO, screen);
    let light = Light2D::point(Color::WHITE, 50.0).with_god_rays();
    let at = god_ray_positions([(Vec2::ZERO, &light)].into_iter(), &view, screen);
    assert_eq!(at.len(), 1);
    assert!((at[0].0 - 0.25).abs() < 1e-5 && (at[0].1 - 0.5).abs() < 1e-5, "{at:?}");
  }

  #[test]
  fn only_god_ray_lights_count_and_at_most_four() {
    let screen = Vec2::new(100.0, 100.0);
    let view = CameraView::compute(Vec2::ZERO, 0.0, &Camera2d::default(), Vec2::ZERO, screen);
    let rays = Light2D::point(Color::WHITE, 50.0).with_god_rays();
    let plain = Light2D::point(Color::WHITE, 50.0);
    let lights = [(Vec2::ZERO, &plain)]
      .into_iter()
      .chain(std::iter::repeat_n((Vec2::ZERO, &rays), 6));
    assert_eq!(god_ray_positions(lights, &view, screen).len(), 4);
  }
}
```

Use the crate paths `Camera2d`, `Viewport`, `CameraView` and `Color` already have (`crate::core::graphics::...`, `crate::core::ui::Color`).

In `tests/e2e/post_process.rs`:

```rust
/// A black frame with a 2-px white vertical line at x = 32 drawn `layers`
/// times additively, under default bloom.
struct GlowingLine {
  layers: u32,
}

impl Scene for GlowingLine {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let black = SpriteHandle::from_sprite(Sprite::solid_rect(64, 64, 0, 0, 0, 255), "glow-black");
    ctx
      .world
      .spawn((Transform::from_xy(32.0, 32.0), Renderable::sprite(black)));
    let line = SpriteHandle::from_sprite(Sprite::solid_rect(2, 64, 255, 255, 255, 255), "glow-line");
    for z in 1..=self.layers as i32 {
      ctx.world.spawn((
        Transform::from_xy(33.0, 32.0),
        Renderable::sprite(line.clone())
          .with_z_order(z)
          .with_blend(BlendMode::Additive),
      ));
    }
    ctx.effects().set_bloom(1.0);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// The red byte 3 px right of the line.
fn glow_beside(scene: impl Scene + 'static) -> Option<u8> {
  let mut engine = engine(scene)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap().get_pixel(37, 32).0[0])
}

#[test]
fn plain_white_does_not_bloom_by_default() {
  let Some(glow) = glow_beside(GlowingLine { layers: 1 }) else {
    return;
  };
  assert!(glow <= 2, "white is not above the default threshold: {glow}");
}

#[test]
fn light_above_white_blooms_by_default() {
  let Some(glow) = glow_beside(GlowingLine { layers: 3 }) else {
    return;
  };
  assert!(glow > 20, "three layers of white glow: {glow}");
}
```

In `src/core/shaders/effects.rs` tests, add what `tests/e2e/lighting_renderer.rs` checked about packing (that file goes in Task 9):

```rust
  #[test]
  fn god_rays_pack_their_light_positions() {
    let effect = PostProcessEffect::god_rays(0.8, vec![(0.2, 0.3), (0.7, 0.8)]);
    let params = GodRaysParams::from_effect(&effect, 0.0, 64, 64);
    assert_eq!(params.light_pos_01, [0.2, 0.3, 0.7, 0.8]);
  }

  #[test]
  fn bloom_thresholds_are_unbounded_above() {
    let PostProcessEffect::Bloom { threshold, .. } = PostProcessEffect::bloom(1.0) else {
      unreachable!()
    };
    assert_eq!(threshold, 1.0);
    let PostProcessEffect::Bloom { threshold, .. } = PostProcessEffect::bloom_custom(3.0, 1.0, 5.0, 2) else {
      unreachable!()
    };
    assert_eq!(threshold, 3.0);
  }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib god_ray 2>&1 | tail -3; cargo test --test e2e -- plain_white_does_not_bloom light_above_white_blooms 2>&1 | grep -E 'panicked|test result'`

Expected:
- `god_rays` does not compile;
- `plain_white_does_not_bloom_by_default` fails (threshold 0.7 blooms white);
- `bloom_thresholds_are_unbounded_above` fails (0.7, and 3.0 clamped to 1).

- [ ] **Step 3: Bloom extracts light above its threshold**

In `bloom_extract.wgsl`, replace `bright`:

```wgsl
// The bright part of one scene texel: light whose luminance passes
// `threshold`, eased in over the half threshold above it, never clamped
fn bright(uv: vec2<f32>, threshold: f32) -> vec3<f32> {
    let color = min(textureSampleLevel(t_input, s_input, uv, 0.0).rgb, vec3<f32>(65504.0));
    let knee = max(threshold * 0.5, 1e-4);
    return color * smoothstep(threshold, threshold + knee, luminance(color));
}
```

Update the header comment to match.

In `effects.rs`:
- `bloom()` uses `threshold: 1.0`.
- `bloom_custom` uses `threshold: threshold.max(0.0)`.
- `god_rays_custom` uses `threshold: threshold.max(0.0)`.
- The `Bloom`/`GodRays` field docs and the constructors' docs read "luminance in linear light, 0 or more; 1.0 is white, so only light brighter than white passes".

Make the same doc change to `set_bloom_custom`/`set_god_rays_custom` in `manager.rs` and `Atmosphere::god_rays_threshold`.

- [ ] **Step 4: God rays keep their light**

In `god_rays.wgsl`'s sampling loop, replace the threshold block:

```wgsl
        // Light above the threshold, eased in over the half threshold above it
        let knee = max(threshold * 0.5, 1e-4);
        let contribution = smoothstep(threshold, threshold + knee, brightness);
        accumulated += min(sample_color.rgb, vec3<f32>(65504.0)) * decay_factor * weight * contribution;
```

The end of `fs_main` adds the rays unclamped:

```wgsl
    // Added to the HDR scene; the tonemap rolls it off
    return vec4<f32>(original.rgb + god_rays * intensity, original.a);
```

- [ ] **Step 5: God rays placed on the frame**

`src/core/lighting/god_rays.rs`:

```rust
//! Where god rays come from: the god-ray lights in frame coordinates

use glam::Vec2;

use super::{Atmosphere, Light2D};
use crate::core::graphics::CameraView;
use crate::core::shaders::EffectsManager;

/// Frame coordinates (0 to 1 across the whole frame) of up to four god-ray
/// lights, where `view` shows them on a screen of `screen` units.
pub(crate) fn god_ray_positions<'a>(
  lights: impl Iterator<Item = (Vec2, &'a Light2D)>,
  view: &CameraView,
  screen: Vec2,
) -> Vec<(f32, f32)> {
  lights
    .filter(|(_, light)| light.contributes_to_god_rays())
    .take(4)
    .map(|(position, _)| {
      let at = view.world_to_screen(position) / screen.max(Vec2::ONE);
      (at.x, at.y)
    })
    .collect()
}

/// Keep the god-ray effect on the lights that cast god rays: added with the
/// atmosphere's settings when the first one appears, its positions updated
/// every frame, removed when none is left. A game that set god rays itself
/// keeps its settings; only the positions follow the lights.
pub(crate) fn update_god_rays<'a>(
  effects: &mut EffectsManager,
  lights: impl Iterator<Item = (Vec2, &'a Light2D)>,
  view: &CameraView,
  screen: Vec2,
  atmosphere: &Atmosphere,
) {
  let positions = god_ray_positions(lights, view, screen);
  if positions.is_empty() {
    if effects.has_god_rays() {
      effects.disable_god_rays();
    }
  } else if effects.has_god_rays() {
    effects.update_god_rays_lights(positions);
  } else {
    effects.set_god_rays_custom(
      atmosphere.god_rays_intensity,
      atmosphere.god_rays_samples,
      atmosphere.god_rays_decay,
      1.0,
      atmosphere.god_rays_threshold,
      positions,
    );
  }
}
```

In `src/core/lighting/mod.rs`, add `mod god_rays; pub(crate) use god_rays::update_god_rays;`. Delete `LightingRenderer::update_god_rays`.

In `src/core/engine/extract.rs`, replace the call:

```rust
        let mut effects = self.world.resource_mut::<EffectsManager>();
        let lights_at = lights.iter().map(|(position, light)| (*position, light));
        crate::core::lighting::update_god_rays(
          &mut effects,
          lights_at,
          &camera,
          rend.screen_size(),
          lighting.atmosphere(),
        );
```

`lighting.atmosphere()` is the atmosphere `set_atmosphere` just copied from the settings.

The spec says frame coordinates are `world_to_screen` divided by the render size. `world_to_screen` returns screen units, though, and the frame image spans the screen, so dividing by the screen size is what the spec means. The two agree at scale 1, and a hi-DPI window would be wrong with the render size.

- [ ] **Step 6: Existing bloom tests keep their meaning**

In `tests/e2e/post_process.rs`, `BrightLine` blooms a plain white line: with the new default it would not glow. Change its `ctx.effects().set_bloom(1.0);` to `ctx.effects().set_bloom_custom(0.5, 1.0, 5.0, 2);`. Bloom's own default is now covered by `GlowingLine`.

- [ ] **Step 7: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:|panicked' | head -20`

Expected: all pass.

- [ ] **Step 8: The gate and commit**

Expected pages:
- bloom pages glow less: only light above white glows now;
- god-ray pages: rays are no longer capped;
- the god-ray page of lighting-volumetric if its main camera is not full-window.

Check them by eye, refresh, and ledger the list. Task 13 retunes them.

```bash
git add -A
git commit -m "feat: bloom and god rays in linear light

- bloom glows only above its threshold (default 1.0, white), unclamped
- god rays drop their clamps and sit on their lights whatever the main camera's viewport
- engine-added god rays take the atmosphere's settings"
```

---

## Phase 5: emission and linear pages (spec migration step 5)

Linear render-target pages landed in Task 2, where float targets needed them.

### Task 8: Emission

**Files:**
- Modify:
  - `src/core/graphics/sprite_gpu.rs` (`SpriteInstance::with_emission`, `emission`, `with_transform`, `SpriteDraw.emission`, `add_shaded`)
  - `src/core/shaders/core/sprite.wgsl`, `src/core/shaders/custom.rs` (entries pass emission)
  - `src/core/ecs/renderable.rs` (`emission`, `with_emission`)
  - `src/core/render/entities.rs` (`Shading.emission`)
  - `src/core/particles/emitter.rs`, `src/core/particles/mod.rs`
  - `src/core/trail/trail.rs`, `src/core/trail/line.rs`
  - `src/core/render/batch/lines.rs`, `src/core/render/debug_draw.rs` (`line_color` takes emission)
- Test: `src/core/graphics/sprite_gpu.rs`, `tests/e2e/look.rs`, `tests/e2e/post_process.rs`

**Interfaces:**
- Consumes: Task 7's bloom (threshold 1.0) and `GlowingLine`.
- Produces:
  - `pub fn SpriteInstance::with_emission(self, emission: f32) -> Self` and `pub fn SpriteInstance::emission(&self) -> f32`. The value is stored as `emission - 1` in `transform[0][3]`. NaN counts as 1; it is clamped to `0..=65504`.
  - `SpriteInstance::with_transform` keeps that lane.
  - `pub emission: f32` (default 1.0) and `with_emission` on `Renderable`, `SpriteDraw` and `ParticleEmitter`.
  - `with_emission(f32)` and `emission() -> f32` on `Trail2D` and `Line2D` (private field, default 1.0).
  - `line_color(color: Color, emission: f32) -> [f32; 4]`.
  - In WGSL:
    - `VertexOutput.emission` (`@location(5) @interpolate(flat)`);
    - `finish(color, tint, emission)`;
    - `finish_premultiplied(color, tint, emission)`;
    - `finish_linear(color, tint, emission)`.

- [ ] **Step 1: Write the failing tests**

In `src/core/graphics/sprite_gpu.rs` tests:

```rust
  #[test]
  fn emission_lives_in_the_unused_transform_lane() {
    let zeroed: SpriteInstance = bytemuck::Zeroable::zeroed();
    assert_eq!(zeroed.emission(), 1.0, "a zeroed lane means no glow");
    let glowing = SpriteInstance::new()
      .with_emission(3.0)
      .with_transform(10.0, 20.0, 0.5, 2.0, 2.0);
    assert_eq!(glowing.emission(), 3.0, "with_transform keeps it");
    assert_eq!(SpriteInstance::new().with_emission(f32::NAN).emission(), 1.0);
    assert_eq!(SpriteInstance::new().with_emission(-2.0).emission(), 0.0);
    assert_eq!(SpriteInstance::new().with_emission(f32::INFINITY).emission(), 65504.0);
  }
```

In `tests/e2e/look.rs`:

```rust
/// A square of `color` with `emission`, under `look`.
fn emitting(color: [u8; 4], emission: f32, look: Look) -> Square {
  Square {
    color: [0, 0, 0, 255],
    look,
    setup: Some(Box::new(move |ctx: &mut SceneContext| {
      let [r, g, b, a] = color;
      let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(32, 32, r, g, b, a), "look-emitting");
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0),
        Renderable::sprite(sprite).with_z_order(1).with_emission(emission),
      ));
    })),
  }
}

#[test]
fn emission_multiplies_a_sprites_light() {
  let look = Look {
    tonemap: Tonemap::Clamp,
    ..Look::default()
  };
  let Some(pixel) = shown(emitting([128, 128, 128, 255], 2.0, look)) else {
    return;
  };
  assert!(pixel[0].abs_diff(encode(0.4317)) <= 1, "{pixel:?}");
}

/// A 4-px white square with `emission` at the left of a 128x32 frame, with bloom.
struct Blazing(f32);

impl Scene for Blazing {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 255, 255, 255), "look-blazing");
    ctx.world.spawn((
      Transform::from_xy(8.0, 16.0),
      Renderable::sprite(sprite).with_emission(self.0),
    ));
    ctx.effects().set_bloom(1.0);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn blazing(emission: f32) -> Option<image::RgbaImage> {
  let builder = EngineBuilder::new().resolution(128, 32).start_scene(Blazing(emission));
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap())
}

#[test]
fn infinite_emission_shows_white_and_leaves_the_rest_alone() {
  let (Some(frame), Some(plain)) = (blazing(f32::INFINITY), blazing(1.0)) else {
    return;
  };
  let centre = frame.get_pixel(8, 16).0;
  assert!(centre[..3].iter().all(|&c| c >= 242), "{centre:?}");
  // Far from it the background is what it is without the glow
  let (far, background) = (frame.get_pixel(124, 16).0, plain.get_pixel(124, 16).0);
  for (got, want) in far.iter().zip(background) {
    assert!(got.abs_diff(want) <= 1, "{far:?} vs {background:?}");
  }
  // NaN would show as black
  assert!(frame.pixels().all(|p| p.0[..3] != [0, 0, 0]), "no black (NaN) pixels");
}
```

In `tests/e2e/post_process.rs`:
- `GlowingLine` gains `emission: f32`, and each line takes `.with_emission(self.emission)`;
- the two Task 7 tests pass `emission: 1.0`;
- add:

```rust
#[test]
fn an_emissive_sprite_blooms_where_plain_white_does_not() {
  let (Some(plain), Some(glowing)) = (
    glow_beside(GlowingLine { layers: 1, emission: 1.0 }),
    glow_beside(GlowingLine { layers: 1, emission: 3.0 }),
  ) else {
    return;
  };
  assert!(plain <= 2 && glowing > 20, "plain {plain}, emissive {glowing}");
}

/// An emissive line seen by a texture camera, shown by a sprite: the target
/// keeps light above white.
struct GlowThroughTarget;

impl Scene for GlowThroughTarget {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let black = SpriteHandle::from_sprite(Sprite::solid_rect(64, 64, 0, 0, 0, 255), "rt-glow-black");
    let line = SpriteHandle::from_sprite(Sprite::solid_rect(2, 64, 255, 255, 255, 255), "rt-glow-line");
    ctx.world.spawn((Transform::from_xy(1000.0, 1000.0), Renderable::sprite(black)));
    ctx.world.spawn((
      Transform::from_xy(1001.0, 1000.0),
      Renderable::sprite(line).with_z_order(1).with_emission(3.0),
    ));
    let target = ctx.render_targets().create(64, 64);
    ctx.world.spawn((
      Transform::from_xy(1000.0, 1000.0),
      Camera2d {
        target: CameraTarget::Texture(target),
        ..Default::default()
      },
    ));
    ctx.world.spawn((Transform::from_xy(32.0, 32.0), Renderable::render_target(target)));
    ctx.effects().set_bloom(1.0);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn an_emissive_sprite_seen_through_a_render_target_still_blooms() {
  let Some(glow) = glow_beside(GlowThroughTarget) else {
    return;
  };
  assert!(glow > 20, "{glow}");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib emission_lives 2>&1 | tail -3; cargo test --test e2e emission 2>&1 | tail -3`

Expected: compile errors (`with_emission` missing).

- [ ] **Step 3: Emission on the instance**

In `src/core/graphics/sprite_gpu.rs`:

```rust
  /// Multiply the sprite's light by `emission` (default 1): above 1 it
  /// glows past white and feeds bloom. Kept as `emission - 1` in
  /// `transform[0][3]`, so a zeroed instance means 1. NaN counts as 1; the
  /// value is clamped to 0..=65504 (the most a scene image holds).
  pub fn with_emission(mut self, emission: f32) -> Self {
    let emission = if emission.is_nan() {
      1.0
    } else {
      emission.clamp(0.0, 65504.0)
    };
    self.transform[0][3] = emission - 1.0;
    self
  }

  /// The instance's emission (see [`with_emission`](Self::with_emission)).
  pub fn emission(&self) -> f32 {
    self.transform[0][3] + 1.0
  }
```

`with_transform` keeps the lane:

```rust
    let emission = self.transform[0][3];
    self.transform[0] = [scale_x * cos, scale_x * sin, 0.0, emission];
```

Update the `transform` field doc: "the last lane of row 0 holds emission − 1; the others are unused".

`SpriteDraw` gains:

```rust
  /// Multiplies the instance's emission (default 1)
  pub emission: f32,
```

`Default` sets it to `1.0`. In `add_shaded`, first thing: `let instance = instance.with_emission(instance.emission() * draw.emission);`. Update the `SpriteDraw` literal in `src/core/particles/mod.rs` (`emission: emitter.emission`) and any other literal `grep -rn 'SpriteDraw {' src tests examples` finds without `..Default::default()`.

- [ ] **Step 4: Emission in the sprite shader**

In `sprite.wgsl`:
- `VertexOutput` gains `@location(5) @interpolate(flat) emission: f32,`.
- `vs_main` sets `out.emission = instance.transform0.w + 1.0;`.

The finishing functions take it:

```wgsl
fn finish(color: vec4<f32>, tint: vec4<f32>, emission: f32) -> vec4<f32> {
    let source = round(clamp(color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0);
    let rgb = floor(min(source * tint.rgb, vec3<f32>(255.0))) / 255.0;
    let a = clamp(color.a * tint.a, 0.0, 1.0);
    let alpha = select(floor(a * 255.0) / 255.0, 1.0, a >= 1.0);
    return vec4<f32>(srgb_to_linear(rgb) * alpha * emission, alpha);
}

fn finish_premultiplied(color: vec4<f32>, tint: vec4<f32>, emission: f32) -> vec4<f32> {
    let rgb = srgb_to_linear(clamp(color.rgb * tint.rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    let alpha = clamp(color.a * tint.a, 0.0, 1.0);
    return vec4<f32>(rgb * tint.a * emission, alpha);
}

fn finish_linear(color: vec4<f32>, tint: vec4<f32>, emission: f32) -> vec4<f32> {
    let a = clamp(tint.a, 0.0, 1.0);
    return vec4<f32>(color.rgb * srgb_to_linear(tint.rgb) * a * emission, color.a * a);
}
```

Every entry passes `in.emission`. In `custom.rs` `sprite_entry`, the three custom entries pass `in.emission` too. Emission 1.0 multiplies exactly, so unlit art stays byte-identical.

- [ ] **Step 5: Emission on the drawables**

1. `Renderable` (`src/core/ecs/renderable.rs`):
   - field `/// Multiplies the sprite's light; above 1 it glows past white and feeds bloom (default 1). A lit sprite is still darkened by lighting: to glow in the dark it also takes \`with_lighting(false)\`` `pub emission: f32,`;
   - `1.0` in every constructor;
   - `pub fn with_emission(mut self, emission: f32) -> Self { self.emission = emission; self }`.
2. `src/core/render/entities.rs`:
   - `Shading` gains `emission: f32` (`1.0` in `PLAIN` and `tinted`), with `fn emitting(self, emission: f32) -> Self { Self { emission, ..self } }`.
   - The entity's own sprite and tiles take `.emitting(renderable.emission)` after `.blended(renderable.blend)` (the two sites); shadows keep 1.0.
   - In the emitter, the instance takes `.with_emission(shading.emission)` after `.with_tint(..)`.
3. `ParticleEmitter` (`src/core/particles/emitter.rs`):
   - `/// Multiplies each particle's light (default 1)` `pub emission: f32,`, `1.0` in its default;
   - `pub fn with_emission(mut self, emission: f32) -> Self`;
   - `collect_for_render` sets `emission: emitter.emission` in its `SpriteDraw`.
4. `Trail2D` and `Line2D` gain a private `emission: f32` (`1.0` in their constructors), and:

```rust
  /// Multiply the line's light by `emission` (default 1): above 1 it glows
  /// past white and feeds bloom. NaN counts as 1; clamped to 0..=65504.
  pub fn with_emission(mut self, emission: f32) -> Self {
    self.emission = if emission.is_nan() { 1.0 } else { emission.clamp(0.0, 65504.0) };
    self
  }

  /// How much the line's light is multiplied by.
  pub fn emission(&self) -> f32 {
    self.emission
  }
```

5. `src/core/render/batch/lines.rs`:

```rust
/// A colour decoded to linear light for a vertex, its light multiplied by
/// `emission`.
pub(crate) fn line_color(color: Color, emission: f32) -> [f32; 4] {
  let [r, g, b, a] = color.to_linear();
  [r * emission, g * emission, b * emission, a]
}
```

   `trail_points` passes `trail.emission()` and `line_points` passes `line.emission()`; `debug_draw.rs` passes `1.0`.

- [ ] **Step 6: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:|panicked' | head -20`

Expected: all pass.

- [ ] **Step 7: The gate and commit**

Expected: `compare 0` (emission defaults to 1).

```bash
git add -A
git commit -m "feat: emission on sprites, particles, trails and lines

- emission multiplies a drawable's linear light, so it can pass white and bloom
- sprites keep it in the instance's unused transform lane"
```

---

## Phase 6: lighting on the batch (spec migration step 6)

### Task 9: Lights through the batch; the standalone lighting API and ray-marched shadows removed

**Files:**
- Create: `src/core/lighting/batch.rs`
- Rewrite: `src/core/shaders/core/light.wgsl`
- Delete: `src/core/shaders/core/raymarch_shadow.wgsl`, `tests/e2e/lighting_renderer.rs` (and its `mod` line in `tests/e2e.rs`)
- Rename: `examples/demos/shadows/scenes/raymarched_shadows.rs` to `placed_occluders.rs`
- Modify:
  - `src/core/render/draw_list.rs` (`DrawLight`, `push_light`, `lights_for`)
  - `src/core/render/mod.rs` (export `DrawLight` to the crate)
  - `src/core/render/batch/{key,pipelines,gpu,mod}.rs` (`LightStep`, `BatchShader::Light`, the light module, `light_draws`)
  - `src/core/render/passes/lighting.rs`
  - `src/core/render/frame.rs` (`LightingFrame` doc)
  - `src/core/lighting/renderer.rs` (lights, standalone API and ray marching out)
  - `src/core/lighting/light.rs` (`with_raymarched_shadows` and its sample count out)
  - `src/core/lighting/mod.rs` (exports)
  - `src/core/lighting/AGENTS.md`
  - `src/core/shaders/compose.rs` (`RAYMARCH_SHADOW` out)
  - `src/core/engine/extract.rs` (`FrameLight`, lights into the draw list)
  - `src/core/engine/mod.rs` (`LightingRenderer::new(device, format)`)
  - `examples/demos/shadows/scenes/mod.rs`, `examples/demos/shadows/main.rs`
- Test: `tests/e2e/lighting.rs`, `src/core/lighting/batch.rs`, `src/core/render/draw_list.rs`, `src/core/render/batch/pipelines.rs`

**Interfaces:**
- Consumes:
  - Task 2's `PageKind` and `PipelineKey.page`;
  - Task 3's `Color::to_linear`;
  - Task 7's `update_god_rays(effects, lights: impl Iterator<Item = (Vec2, &Light2D)>, ..)`.
- Produces:
  - `pub(crate) struct FrameLight { pub(crate) position: Vec2, pub(crate) light: Light2D, pub(crate) layers: RenderLayers }`.
  - `pub(crate) fn light_instance(position: Vec2, light: &Light2D) -> Option<SpriteInstance>`.
  - `pub(crate) fn light_item(light: &FrameLight) -> Option<DrawLight>`. Task 10 adds occluder and triangle arguments.
  - These three are in `lighting::batch` and re-exported from `lighting`.
  - `pub(crate) struct DrawLight { pub(crate) instance: SpriteInstance, pub(crate) layers: RenderLayers, pub(crate) min: Vec2, pub(crate) max: Vec2 }`. Task 10 adds `shadows` and `coloured`.
  - `DrawList::push_light(&mut self, light: DrawLight)` and `DrawList::lights_for(&self, camera: &CameraDraw) -> impl Iterator<Item = &DrawLight>`.
  - `pub(crate) enum LightStep { Add }` (Task 10 adds `ResetMask`, `Shadow`, `Masked`, `Coloured`), with `fn fragment(self) -> &'static str` and `fn stream(self) -> Stream`.
  - `BatchShader::Light(LightStep)` and `pub(crate) fn light_blend(step: LightStep) -> (wgpu::BlendState, wgpu::ColorWrites)` in `batch::pipelines`.
  - `Batch2D::light_draws(&self, camera: Entity) -> &[Draw]`.
  - `pub(crate) struct LightingRenderer` with:
    - `new(device, format)`;
    - the ambient, atmosphere, enabled and UI-affected accessors;
    - `sync_cameras`;
    - `light_view(camera) -> Option<&TextureView>`;
    - `prepare_shadows(queue, lights: &[FrameLight], occluders)`;
    - `draw_shadows_for_camera(device, encoder, camera, uniforms)`;
    - `compose_for_camera(..)`.
  - Removed: `LightingRenderer` from the public API and its standalone methods (`resize`, `render_lights`, `render_lights_with_shadows`, `compose`, `compose_from_texture`, `light_texture_view`), `LightingParams` and `MAX_LIGHTS` exports, ray-marched shadows (`Light2D::with_raymarched_shadows`, `raymarch_samples`, `set_raymarch_samples`, `uses_raymarched_shadows`, the variant field `raymarch_samples`, `raymarch_shadow.wgsl`).

- [ ] **Step 1: Write the failing tests**

Add to `tests/e2e/lighting.rs`:

```rust
/// No ambient light, and `light` at the centre of the floor.
fn centre_light(light: Light2D) -> impl FnMut(&mut SceneContext) + Send + 'static {
  move |ctx| {
    ctx.lighting().set_ambient(AmbientLight::with_intensity(0.0));
    ctx.world.spawn((Transform::from_xy(64.0, 32.0), light.clone()));
  }
}

#[test]
fn a_light_lights_what_it_covers() {
  let Some(frame) = lit(centre_light(Light2D::point(Color::WHITE, 30.0))) else {
    return;
  };
  let (centre, corner) = (frame.get_pixel(64, 32).0, frame.get_pixel(4, 4).0);
  assert!(centre[0] > 200, "{centre:?}");
  assert!(corner[0] < 10, "{corner:?}");
}

#[test]
fn zero_ambient_without_lights_is_black() {
  let Some(frame) = lit(|ctx| {
    let mut lighting = ctx.lighting();
    lighting.set_ambient(AmbientLight::with_intensity(0.0));
    lighting.set_enabled(true);
  }) else {
    return;
  };
  assert!(frame.get_pixel(64, 32).0[0] < 3, "{:?}", frame.get_pixel(64, 32).0);
}

#[test]
fn disabled_lighting_leaves_the_scene_unlit() {
  let Some(frame) = lit(|ctx| {
    ctx.world.spawn((Transform::from_xy(64.0, 32.0), Light2D::point(Color::rgb(40, 40, 40), 10.0)));
    let mut lighting = ctx.lighting();
    lighting.set_ambient(AmbientLight::with_intensity(0.0));
    lighting.set_enabled(false);
  }) else {
    return;
  };
  assert!(frame.get_pixel(4, 4).0[0] > 230, "{:?}", frame.get_pixel(4, 4).0);
}

/// The red byte inside a dim spotlight's cone, with or without its beam.
fn in_the_beam(volumetric: bool) -> Option<u8> {
  let frame = lit(move |ctx| {
    ctx.lighting().set_ambient(AmbientLight::with_intensity(0.0));
    let spot = Light2D::spot(Color::WHITE, 120.0, 40.0, 0.0).with_intensity(0.2);
    let spot = if volumetric { spot.with_volumetric() } else { spot };
    ctx.world.spawn((Transform::from_xy(8.0, 32.0), spot));
  })?;
  Some(frame.get_pixel(60, 32).0[0])
}

#[test]
fn a_volumetric_spotlight_brightens_its_beam() {
  let (Some(plain), Some(beam)) = (in_the_beam(false), in_the_beam(true)) else {
    return;
  };
  assert!(beam > plain + 3, "plain {plain}, beam {beam}");
}

#[test]
fn a_light_on_layer_one_does_not_light_a_layer_zero_camera() {
  let Some(frame) = lit(|ctx| {
    ctx.lighting().set_ambient(AmbientLight::with_intensity(0.0));
    ctx.world.spawn((
      Transform::from_xy(64.0, 32.0),
      Light2D::point(Color::WHITE, 30.0),
      RenderLayers::layer(1),
    ));
  }) else {
    return;
  };
  assert!(frame.get_pixel(64, 32).0[0] < 3, "{:?}", frame.get_pixel(64, 32).0);
}

#[test]
fn lights_past_the_old_cap_all_draw() {
  let Some(frame) = lit(|ctx| {
    ctx.lighting().set_ambient(AmbientLight::with_intensity(0.0));
    // 300 faint lights in a corner, then the one that lights the centre
    for _ in 0..300 {
      ctx.world.spawn((
        Transform::from_xy(2.0, 2.0),
        Light2D::point(Color::WHITE, 1.0).with_intensity(0.001),
      ));
    }
    ctx.world.spawn((Transform::from_xy(64.0, 32.0), Light2D::point(Color::WHITE, 30.0)));
  }) else {
    return;
  };
  assert!(frame.get_pixel(64, 32).0[0] > 200, "{:?}", frame.get_pixel(64, 32).0);
}

#[test]
fn degenerate_lights_draw_nothing() {
  let Some(frame) = lit(|ctx| {
    ctx.lighting().set_ambient(AmbientLight::with_intensity(0.5));
    ctx.effects().set_bloom(1.0);
    let mut lights = vec![Light2D::point(Color::WHITE, 30.0); 4];
    if let Light2D::Point { radius, .. } = &mut lights[0] {
      *radius = 0.0;
    }
    if let Light2D::Point { radius, .. } = &mut lights[1] {
      *radius = f32::NAN;
    }
    if let Light2D::Point { intensity, .. } = &mut lights[2] {
      *intensity = f32::NAN;
    }
    if let Light2D::Point { intensity, .. } = &mut lights[3] {
      *intensity = f32::INFINITY;
    }
    for light in lights {
      ctx.world.spawn((Transform::from_xy(64.0, 32.0), light));
    }
    ctx.world.spawn((Transform::from_xy(f32::NAN, 32.0), Light2D::point(Color::WHITE, 30.0)));
  }) else {
    return;
  };
  // Half the ambient light (sRGB 188) everywhere: no light, no NaN spread by bloom
  let corner = frame.get_pixel(4, 4).0;
  assert!(corner[0].abs_diff(188) <= 1, "{corner:?}");
  assert!(frame.pixels().all(|p| p.0 == corner), "the floor is lit evenly");
}
```

`src/core/lighting/batch.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::Color;

  #[test]
  fn a_light_is_a_quad_of_its_reach_with_its_decoded_colour() {
    let light = Light2D::point(Color::rgb(128, 255, 0), 40.0).with_intensity(3.0);
    let Some(instance) = light_instance(Vec2::new(10.0, 20.0), &light) else {
      panic!("a usable light");
    };
    assert_eq!(instance.transform[2][..2], [10.0, 20.0]);
    assert_eq!(instance.transform[0][0], 80.0, "two radii wide");
    assert!((instance.tint[0] - 0.2159).abs() < 1e-3, "{:?}", instance.tint);
    assert_eq!(instance.tint[1], 1.0);
    assert_eq!(instance.tint[3], 3.0, "intensity, unbounded");
  }

  #[test]
  fn lights_that_cannot_light_anything_are_dropped() {
    let light = Light2D::point(Color::WHITE, 40.0);
    assert!(light_instance(Vec2::new(f32::NAN, 0.0), &light).is_none());
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
      let mut light = light.clone();
      if let Light2D::Point { radius, .. } = &mut light {
        *radius = bad;
      }
      assert!(light_instance(Vec2::ZERO, &light).is_none(), "radius {bad}");
      let mut light = Light2D::point(Color::WHITE, 40.0);
      if let Light2D::Point { intensity, .. } = &mut light {
        *intensity = bad;
      }
      assert!(light_instance(Vec2::ZERO, &light).is_none(), "intensity {bad}");
    }
  }

  #[test]
  fn nan_shape_parameters_fall_back_to_usable_ones() {
    let mut light = Light2D::spot(Color::WHITE, 40.0, 45.0, 0.0).with_volumetric();
    if let Light2D::Spot { falloff, angle, direction, softness, .. } = &mut light {
      *falloff = f32::NAN;
      *angle = f32::NAN;
      *direction = f32::INFINITY;
      *softness = f32::NAN;
    }
    let Some(instance) = light_instance(Vec2::ZERO, &light) else {
      panic!("still a light");
    };
    let shape = instance.effect_params.iter().chain(&instance.effect_extra);
    assert!(shape.copied().all(f32::is_finite), "{instance:?}");
  }
}
```

In `src/core/render/draw_list.rs` tests:

```rust
  #[test]
  fn a_camera_sees_the_lights_on_its_layers_in_its_view() {
    use crate::core::ecs::World;
    use crate::core::graphics::{Camera2d, CameraView, RenderLayers};
    let mut world = World::new();
    let entity = world.spawn_empty().id();
    let camera = Camera2d::default();
    let view = CameraView::compute(Vec2::new(50.0, 50.0), 0.0, &camera, Vec2::ZERO, Vec2::new(100.0, 100.0));
    let (mut cameras, mut targetless) = (Vec::new(), Vec::new());
    super::super::frame::plan_cameras(
      [(entity, &camera, &view, None)].into_iter(),
      (100, 100),
      true,
      None,
      &mut cameras,
      &mut targetless,
    );
    let light = |at: Vec2, layers: RenderLayers| DrawLight {
      instance: SpriteInstance::new(),
      layers,
      min: at - 5.0,
      max: at + 5.0,
    };
    let mut list = DrawList::default();
    list.push_light(light(Vec2::new(50.0, 50.0), RenderLayers::default()));
    list.push_light(light(Vec2::new(500.0, 50.0), RenderLayers::default()));
    list.push_light(light(Vec2::new(50.0, 50.0), RenderLayers::layer(1)));
    assert_eq!(list.lights_for(&cameras[0]).count(), 1);
  }
```

Use the path that reaches `plan_cameras` from `draw_list.rs` (`crate::core::render::frame::plan_cameras` if `frame` is a private sibling module).

In `src/core/render/batch/pipelines.rs` tests:

```rust
  #[test]
  fn lights_add_up_and_leave_the_mask_alone() {
    let (blend, writes) = light_blend(LightStep::Add);
    assert_eq!(blend.color.src_factor, wgpu::BlendFactor::One);
    assert_eq!(blend.color.dst_factor, wgpu::BlendFactor::One);
    assert_eq!(writes, wgpu::ColorWrites::COLOR);
  }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test e2e lighting:: 2>&1 | grep -E 'panicked|test result'; cargo test --lib light 2>&1 | tail -3`

Expected:
- `lights_past_the_old_cap_all_draw` fails (the 301st light is past `MAX_LIGHTS`);
- the layer test fails (lights ignore layers);
- `degenerate_lights_draw_nothing` fails or is flaky (NaN radius);
- the unit tests do not compile.

- [ ] **Step 3: The light shader as a batch shader**

Replace `src/core/shaders/core/light.wgsl`:

```wgsl
// Lights as batch quads, drawn into a camera's light buffer.
//
// A light's instance (`lighting::light_instance`) carries:
// - the transform rows: its quad, centred on it, two radii wide
// - uv_rect.x: 0 for a point light, 1 for a spot
// - tint: its linear colour (rgb) and intensity (a)
// - effect_params: falloff, cone angle and direction (degrees), cone softness
// - effect_extra: volumetric intensity, density, scattering, god rays
// - uv_bounds: its shadow colour (linear; alpha is the shadow's strength)

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
}

struct InstanceInput {
    @location(2) transform0: vec4<f32>,
    @location(3) transform1: vec4<f32>,
    @location(4) transform2: vec4<f32>,
    @location(5) uv_rect: vec4<f32>,
    @location(6) tint: vec4<f32>,
    @location(7) effect_params: vec4<f32>,
    @location(8) effect_extra: vec4<f32>,
    @location(9) uv_bounds: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    // From the light's centre, in world units
    @location(0) offset: vec2<f32>,
    // Colour (rgb) and intensity (a)
    @location(1) @interpolate(flat) color: vec4<f32>,
    // Radius, type, falloff
    @location(2) @interpolate(flat) shape: vec3<f32>,
    // Cone angle, direction, softness
    @location(3) @interpolate(flat) spot: vec3<f32>,
    // Intensity, density, scattering, god rays
    @location(4) @interpolate(flat) volumetric: vec4<f32>,
    @location(5) @interpolate(flat) shadow_color: vec4<f32>,
}

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    let transform = mat3x3<f32>(
        instance.transform0.xyz,
        instance.transform1.xyz,
        instance.transform2.xyz,
    );
    let world = (transform * vec3<f32>(vertex.position, 1.0)).xy;
    var out: VertexOutput;
    out.position = globals.view_proj * vec4<f32>(world, 0.0, 1.0);
    out.offset = world - instance.transform2.xy;
    out.color = instance.tint;
    out.shape = vec3<f32>(instance.transform0.x * 0.5, instance.uv_rect.x, instance.effect_params.x);
    out.spot = instance.effect_params.yzw;
    out.volumetric = instance.effect_extra;
    out.shadow_color = instance.uv_bounds;
    return out;
}

// The light reaching this fragment: point or spot falloff, a volumetric
// beam for spots and a faint glow for points
fn light_at(in: VertexOutput) -> vec3<f32> {
    let radius = in.shape.x;
    let dist = length(in.offset);
    if dist > radius {
        return vec3<f32>(0.0);
    }
    let normalized_dist = dist / radius;
    let attenuation = pow(1.0 - normalized_dist, in.shape.z);
    let vol_intensity = in.volumetric.x;
    let vol_density = in.volumetric.y;
    let vol_scattering = in.volumetric.z;
    if in.shape.y > 0.5 {
        let half_angle = radians(in.spot.x * 0.5);
        let direction = radians(in.spot.y);
        let light_dir = vec2<f32>(cos(direction), sin(direction));
        let to_pixel = select(in.offset / dist, light_dir, dist < 1e-6);
        let pixel_angle = acos(clamp(dot(light_dir, to_pixel), -1.0, 1.0));
        let inner = half_angle * (1.0 - in.spot.z);
        let cone = 1.0 - smoothstep(inner, half_angle, pixel_angle);
        var color = in.color.rgb * attenuation * cone * in.color.a;
        if vol_intensity > 0.001 {
            // A visible beam in dusty air, with a soft scatter past the cone
            let beam_falloff = 1.0 - pow(normalized_dist, 2.0 + vol_density);
            let beam_core = vol_intensity * vol_density * cone * beam_falloff;
            let scatter_cone = 1.0 - smoothstep(half_angle * 0.7, half_angle * 1.2, pixel_angle);
            let edge_scatter = vol_scattering * scatter_cone * beam_falloff * 0.5;
            color += in.color.rgb * (beam_core + edge_scatter);
        }
        return color;
    }
    var color = in.color.rgb * attenuation * in.color.a;
    if vol_intensity > 0.001 {
        let glow_falloff = pow(1.0 - normalized_dist, 0.3 + vol_density * 0.5);
        color += in.color.rgb * vol_intensity * vol_density * glow_falloff * 0.2;
    }
    return color;
}

@fragment
fn fs_light(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(light_at(in), 0.0);
}
```

`compose::LIGHT` (camera include + this file) is unchanged.

- [ ] **Step 4: Light items**

`src/core/lighting/batch.rs`:

```rust
//! Lights as batch items: one quad per light, drawn into the light buffer
//! of each lit camera that sees it

use glam::Vec2;

use super::Light2D;
use crate::core::graphics::{RenderLayers, SpriteInstance};
use crate::core::render::DrawLight;

/// A light where it is drawn this frame, and the layers that see it.
#[derive(Debug, Clone)]
pub(crate) struct FrameLight {
  pub(crate) position: Vec2,
  pub(crate) light: Light2D,
  pub(crate) layers: RenderLayers,
}

/// `v` if finite, else `default`.
fn or(v: f32, default: f32) -> f32 {
  if v.is_finite() { v } else { default }
}

/// `light` at `position` as a quad instance for `light.wgsl`, or `None`
/// when it cannot light anything: a radius or intensity that is not
/// positive and finite, or a position that is not finite. Shape parameters
/// that are not finite take usable defaults.
pub(crate) fn light_instance(position: Vec2, light: &Light2D) -> Option<SpriteInstance> {
  let usable = |v: f32| v.is_finite() && v > 0.0;
  let (radius, intensity) = (light.radius(), light.intensity());
  if !(usable(radius) && usable(intensity) && position.is_finite()) {
    return None;
  }
  let [r, g, b, _] = light.color().to_linear();
  let (kind, params) = match light {
    Light2D::Point { falloff, .. } => (0.0, [or(*falloff, 2.0).max(0.0), 0.0, 0.0, 0.0]),
    Light2D::Spot {
      falloff,
      angle,
      direction,
      softness,
      ..
    } => (
      1.0,
      [
        or(*falloff, 2.0).max(0.0),
        or(*angle, 45.0),
        or(*direction, 0.0),
        or(*softness, 0.2).clamp(0.0, 1.0),
      ],
    ),
  };
  let volumetric = light
    .volumetric()
    .map(|v| v.to_gpu_vec4().map(|x| or(x, 0.0)))
    .unwrap_or([0.0; 4]);
  let mut instance = SpriteInstance::new().with_transform(
    position.x,
    position.y,
    0.0,
    radius * 2.0,
    radius * 2.0,
  );
  instance.uv_rect = [kind, 0.0, 0.0, 0.0];
  instance.tint = [r, g, b, intensity];
  instance.effect_params = params;
  instance.effect_extra = volumetric;
  instance.uv_bounds = light.shadow_color().to_linear();
  Some(instance)
}

/// `light`'s draw-list item, if it can light anything.
pub(crate) fn light_item(light: &FrameLight) -> Option<DrawLight> {
  let instance = light_instance(light.position, &light.light)?;
  let reach = Vec2::splat(light.light.radius());
  Some(DrawLight {
    instance,
    layers: light.layers,
    min: light.position - reach,
    max: light.position + reach,
  })
}
```

`src/core/lighting/mod.rs`: `mod batch; pub(crate) use batch::{FrameLight, light_instance, light_item};`.

`src/core/render/draw_list.rs`:

```rust
/// A light waiting to be drawn into the light buffers of the lit cameras
/// that see it.
#[derive(Debug, Clone)]
pub(crate) struct DrawLight {
  pub(crate) instance: SpriteInstance,
  /// The cameras on these layers draw it
  pub(crate) layers: RenderLayers,
  /// The world box it lights
  pub(crate) min: Vec2,
  pub(crate) max: Vec2,
}
```

`DrawList` gains `lights: Vec<DrawLight>`, cleared in `clear`, plus:

```rust
  /// Add a light for the lit cameras that see it.
  pub(crate) fn push_light(&mut self, light: DrawLight) {
    self.lights.push(light);
  }

  /// The lights `camera` draws: those on its layers whose box meets its view.
  pub(crate) fn lights_for<'s>(
    &'s self,
    camera: &'s CameraDraw,
  ) -> impl Iterator<Item = &'s DrawLight> + 's {
    self
      .lights
      .iter()
      .filter(|light| light.layers.intersects(&camera.layers) && camera.view.is_visible(light.min, light.max))
  }
```

Export `DrawLight` from `src/core/render/mod.rs` as `pub(crate)`.

- [ ] **Step 5: Lights in the batch**

`src/core/render/batch/key.rs`:

```rust
/// One draw of a light into a light buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LightStep {
  /// Light added up (lights without shadows)
  Add,
}

impl LightStep {
  /// The entry point that draws it.
  pub(crate) fn fragment(self) -> &'static str {
    match self {
      LightStep::Add => "fs_light",
    }
  }

  /// The stream it draws from.
  pub(crate) fn stream(self) -> Stream {
    Stream::Quads
  }
}
```

`BatchShader` gains `/// A light (or its shadow) into a light buffer` `Light(LightStep),`. Export `LightStep` from `render/batch/mod.rs`.

`src/core/render/batch/pipelines.rs`:
- `Pipelines` gains `light: wgpu::ShaderModule` (`module("Light Shader", crate::core::shaders::compose::LIGHT)`).
- `ensure` gains the arm `(BatchShader::Light(step), Stream::Quads, _, _) => (&self.light, &self.camera_only, step.fragment()),` before the catch-all.
- `batch_pipeline` takes its blend and writes from the key:

```rust
  let (blend, write_mask) = match key.shader {
    BatchShader::Light(step) => light_blend(step),
    _ => (blend_state(key.blend), wgpu::ColorWrites::ALL),
  };
```

`blend: Some(blend), write_mask` replace the fixed values in the colour target. Add:

```rust
/// The blend state and colour writes of a draw into a light buffer.
pub(crate) fn light_blend(step: LightStep) -> (wgpu::BlendState, wgpu::ColorWrites) {
  use wgpu::BlendFactor::{One, Zero};
  let add = |src_factor, dst_factor| wgpu::BlendComponent {
    src_factor,
    dst_factor,
    operation: wgpu::BlendOperation::Add,
  };
  match step {
    // Light adds up; the alpha, a shadowed light's mask, is left alone
    LightStep::Add => (
      wgpu::BlendState {
        color: add(One, One),
        alpha: add(Zero, One),
      },
      wgpu::ColorWrites::COLOR,
    ),
  }
}
```

`src/core/render/batch/gpu.rs`:
- `CameraDraws` gains `lights: Vec<Draw>`.
- In `prepare`, after both `plan_draws` calls (end the `place` closure's borrow of `placed` first: wrap the closure and the two calls in a block), add:

```rust
      // A lit camera's lights, into its light buffer: those without
      // shadows add up in one draw
      drawn.lights.clear();
      if camera.lit {
        let key = light_key(LightStep::Add, format);
        pipelines.ensure(device, key, None);
        let start = placed.len() as u32;
        placed.extend(list.lights_for(camera).map(|light| light.instance));
        let end = placed.len() as u32;
        if end > start {
          drawn.lights.push(light_draw(LightStep::Add, start..end));
        }
      }
```

with:

```rust
/// The pipeline key of a light-buffer draw.
fn light_key(step: LightStep, format: wgpu::TextureFormat) -> PipelineKey {
  PipelineKey {
    shader: BatchShader::Light(step),
    blend: BlendMode::Additive,
    stream: step.stream(),
    page: PageKind::Srgb8,
    format,
  }
}

/// A light-buffer draw of `range` (instances or indices, by the step's stream).
fn light_draw(step: LightStep, range: std::ops::Range<u32>) -> Draw {
  Draw {
    stream: step.stream(),
    state: DrawState {
      page: None,
      shader: BatchShader::Light(step),
      blend: BlendMode::Additive,
      sampling: Sampling::Nearest,
      scissor: None,
    },
    range,
  }
}
```

Then:
- Add `pub(crate) fn light_draws(&self, camera: Entity) -> &[Draw]` beside `draws` (empty for an unknown camera).
- `draw_count` adds `drawn.lights.len()`.
- In `draw`, untextured quads are allowed for lights: `None if draw.stream == Stream::Quads && !matches!(draw.state.shader, BatchShader::Light(_)) => return,`.

The light buffer is `Rgba16Float`, the batch's format since Task 2, so light pipelines share `format`.

- [ ] **Step 6: The renderer keeps compose; the lighting pass draws the lights**

In `src/core/lighting/renderer.rs`, remove:
- the standalone targets field `targets` and its `width`/`height` constructor arguments (`LightingRenderer::new(device, format)` now);
- `instances`, `frame_lights`, `light_pipeline`, `light_buffer`, `light_bind_group_layout`;
- every `raymarch_*` field, `RaymarchParams` and `RaymarchEdge`;
- the methods `resize`, `render_lights`, `standalone_globals`, `draw_lights`, `draw_shadows` (the standalone one), `render_raymarched_shadow`, `render_lights_with_shadows`, `compose`, `light_texture_view`, `compose_from_texture`;
- the functions `light_instances` and `light_color`, the struct `LightInstance`, and `MAX_LIGHTS`.

What stays or changes:
- `LightingParams` stays, as `pub(crate)`. `pub struct LightingRenderer` becomes `pub(crate) struct LightingRenderer`.
- `prepare_frame` becomes `prepare_shadows`, uploading only the shadow vertices:

```rust
  /// Build this frame's shadow geometry and upload it once; every lit
  /// camera then cuts it from its light buffer
  /// ([`draw_shadows_for_camera`](Self::draw_shadows_for_camera)).
  pub(crate) fn prepare_shadows(
    &mut self,
    queue: &wgpu::Queue,
    lights: &[FrameLight],
    occluders: &[(GlobalTransform, &LightOccluder2D)],
  ) {
    self.frame_shadow_vertices = 0;
    if occluders.is_empty() || !lights.iter().any(|l| l.light.casts_shadows()) {
      return;
    }
    let occluders: Vec<(&GlobalTransform, &LightOccluder2D)> =
      occluders.iter().map(|(t, o)| (t, *o)).collect();
    let lights = lights.iter().map(|l| (&l.position, &l.light));
    let geometry = generate_shadow_geometry(lights, &occluders);
    if !geometry.is_empty() {
      queue.write_buffer(&self.shadow_vertex_buffer, 0, bytemuck::cast_slice(&geometry.vertices));
      self.frame_shadow_vertices = geometry.vertex_count();
    }
  }
```

- `draw_for_camera` becomes `draw_shadows_for_camera`: only its shadow pass (`LoadOp::Load`), without the light accumulation pass.
- Add:

```rust
  /// `camera`'s light buffer, which the batch draws its lights into.
  pub(crate) fn light_view(&self, camera: Entity) -> Option<&wgpu::TextureView> {
    self.cameras.get(&camera).map(|targets| &targets.light_view)
  }
```

In `src/core/lighting/mod.rs`, `pub use renderer::{LightingParams, LightingRenderer, MAX_LIGHTS};` becomes `pub(crate) use renderer::LightingRenderer;`.

In `src/core/lighting/light.rs`:
- remove `raymarch_samples` from both variants, every constructor, and the methods `with_raymarched_shadows`, `raymarch_samples`, `set_raymarch_samples` and `uses_raymarched_shadows`;
- fix whatever else the compiler then lists.

In `src/core/shaders/compose.rs`, remove `RAYMARCH_SHADOW` and its `built_in()` entry, and delete `raymarch_shadow.wgsl`.

In `src/core/engine/mod.rs`: `LightingRenderer::new(renderer.device(), format)`.

`src/core/render/passes/lighting.rs` `execute`:

```rust
  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let Some(camera) = ctx.current_camera().filter(|camera| camera.lit) else {
      return;
    };
    let (device, queue) = (ctx.device(), ctx.queue());
    let RenderContext {
      encoder,
      targets,
      scene,
      image,
      ..
    } = ctx;
    let Some(lighting) = scene.lighting.as_ref() else {
      return;
    };
    let renderer = &*lighting.renderer;
    let (entity, uniforms) = (camera.entity, scene.camera_uniforms);
    let Some(light_view) = renderer.light_view(entity) else {
      return;
    };
    // The frame's lights, through the batch, into the camera's light buffer
    {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Light Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: light_view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      if let Some(batch) = scene.batch {
        for draw in batch.light_draws(entity) {
          batch.draw(&mut pass, draw, entity, scene.sprites);
        }
      }
    }
    renderer.draw_shadows_for_camera(device, encoder, entity, uniforms);
    // The camera drawing straight into the scene image is lit into the lit
    // image; a camera with an image of its own is lit in place
    let (scene_texture, output) = if camera.direct {
      (targets.game, targets.lit_view)
    } else {
      let images = targets.camera_images;
      let (Some(texture), Some(view)) = (images.texture(entity), images.view(entity)) else {
        return;
      };
      (texture, view)
    };
    renderer.compose_for_camera(device, queue, encoder, entity, scene_texture, output, scene.time);
    if camera.direct {
      *image = FrameImage::Lit;
    }
  }
```

If `compose_for_camera` takes `&mut self`, change it to `&self` (it writes through the queue only).

- [ ] **Step 7: The engine gathers lights with their layers**

In `src/core/engine/extract.rs`:
- `self.lights` becomes `Vec<FrameLight>`.
- `lights_to_draw` reads `Option<&RenderLayers>` too:

```rust
fn lights_to_draw(world: &mut World, alpha: f32, out: &mut Vec<FrameLight>) {
  out.clear();
  out.extend(
    world
      .query::<(
        &GlobalTransform,
        Option<&PreviousGlobalTransform>,
        &Light2D,
        Option<&RenderLayers>,
      )>()
      .iter(world)
      .map(|(global, previous, light, layers)| FrameLight {
        position: render_global(global, previous, alpha).translation(),
        light: light.clone(),
        layers: layers.copied().unwrap_or_default(),
      }),
  );
}
```

- The god-ray call maps `lights.iter().map(|l| (l.position, &l.light))`.
- `prepare_frame` becomes `lighting.prepare_shadows(rend.queue(), &lights, &occluders)`.
- Right after `self.draw_list.clear();`:

```rust
      // Lights, for the lit cameras whose layers and view reach them
      if lit {
        for light in &lights {
          if let Some(item) = crate::core::lighting::light_item(light) {
            self.draw_list.push_light(item);
          }
        }
      }
```

The `extract.rs` tests that call `lights_to_draw` adapt to `FrameLight`'s fields.

In `src/core/render/frame.rs`, `LightingFrame`'s doc reads "With this frame's shadows uploaded (`prepare_shadows`)".

- [ ] **Step 8: Migrate the standalone tests; the demo page**

Delete `tests/e2e/lighting_renderer.rs` and its `mod lighting_renderer;` in `tests/e2e.rs`. Its checks now live in:
- `tests/e2e/lighting.rs` (lights, ambient, disabled lighting, volumetric beams);
- `src/core/shaders/effects.rs` (god-ray packing, Task 7);
- `tests/e2e/render_pipeline.rs` (effects on the lit scene).

`git mv examples/demos/shadows/scenes/raymarched_shadows.rs examples/demos/shadows/scenes/placed_occluders.rs`, then in it:
- the module doc becomes `//! Placed occluders page` / `//!` / `//! Place and remove occluders around a light that casts soft shadow volumes.`;
- `RaymarchedShadowsPage` becomes `PlacedOccludersPage`;
- the field `raymarch_samples: i32` (default 32) becomes `softness: f32` (default 0.4);
- `create_light` ends with `.with_soft_shadows(self.softness)` instead of `.with_raymarched_shadows(..)`;
- the occluder sprite names start `po_occluder_`;
- `name()` returns `"Placed occluders"`;
- the settings row becomes `s.float("Softness", &mut self.softness, 0.0..=1.0, 0.05);`;
- any description text naming ray marching says shadow volumes.

`scenes/mod.rs` declares `mod placed_occluders;` and `pub use placed_occluders::PlacedOccludersPage;`. `main.rs` registers `.page(PlacedOccludersPage::new)`.

In `src/core/lighting/AGENTS.md`, replace the standalone-API sentence with: "Lights are batch quads (`lighting::batch`), drawn by the `lighting` pass into each lit camera's light buffer; `LightingRenderer` is engine-internal and keeps the per-camera light buffers, the scene copies and the compose pass."

- [ ] **Step 9: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:|panicked' | head -20`

Expected: all pass.

- [ ] **Step 10: The gate and commit**

Expected pages:
- lit pages may move by a level or two (lights now cull per camera and draw through the batch; the formulas are the same);
- the shadows demo's renamed page (now `Placed occluders`) shows shadow volumes.

Check them by eye, refresh, and ledger the list.

```bash
git add -A
git commit -m "feat: lights draw through the 2D batch

- lights are batch quads per lit camera, culled by view and RenderLayers, with no cap
- LightingRenderer is engine-internal; its standalone API goes
- ray-marched shadows are removed; the shadows demo page uses shadow volumes"
```

### Task 10: Shadows that darken only their own light

**Files:**
- Rewrite: `src/core/shaders/core/shadow.wgsl`, `src/core/lighting/shadow.rs`
- Modify:
  - `src/core/shaders/core/light.wgsl` (`fs_mask_reset`, `fs_light_coloured`)
  - `src/core/lighting/batch.rs` (`light_item` builds the shadow volumes)
  - `src/core/render/draw_list.rs` (`DrawLight.shadows`, `coloured`)
  - `src/core/render/batch/{key,pipelines,gpu,mod}.rs` (the remaining light steps, the shadow module, `Vertex2D` exported)
  - `src/core/lighting/renderer.rs` (shadow pipeline out)
  - `src/core/lighting/mod.rs` (exports)
  - `src/core/render/passes/lighting.rs`
  - `src/core/engine/extract.rs` (occluders gathered before the draw list is laid out)
- Test: `tests/e2e/lighting.rs`, `src/core/lighting/shadow.rs`, `src/core/render/batch/pipelines.rs`

**Interfaces:**
- Consumes: Task 9's `light_item`, `DrawLight`, `LightStep`, `light_blend`, `light_key`, `light_draw`.
- Produces:
  - `pub(crate) fn push_shadow_volumes(position: Vec2, light: &Light2D, occluders: &[(GlobalTransform, &LightOccluder2D)], triangles: &mut Triangles) -> Range<u32>`, the index range.
  - `light_item(light: &FrameLight, occluders: &[(GlobalTransform, &LightOccluder2D)], triangles: &mut Triangles) -> Option<DrawLight>`.
  - `DrawLight { .., pub(crate) shadows: Range<u32>, pub(crate) coloured: bool }`.
  - `LightStep::{Add, ResetMask, Shadow, Masked, Coloured}`.
  - `Vertex2D` re-exported from `render::batch`.
  - Removed: `ShadowVertex`, `ShadowGeometry`, `generate_shadow_geometry`, `MAX_SHADOW_VERTICES` (public), the renderer's shadow pipeline, `prepare_shadows` and `draw_shadows_for_camera`.

- [ ] **Step 1: Write the failing tests**

In `tests/e2e/lighting.rs`:

```rust
/// The red byte at (96, 32), lit by light B; with `with_a`, light A on the
/// left casts an occluder's shadow over all of B.
fn light_b(with_a: bool) -> Option<u8> {
  let frame = lit(move |ctx| {
    ctx.lighting().set_ambient(AmbientLight::with_intensity(0.0));
    if with_a {
      ctx.world.spawn((
        Transform::from_xy(24.0, 32.0),
        Light2D::point(Color::WHITE, 100.0).with_shadows(),
      ));
      ctx.world.spawn((Transform::from_xy(40.0, 32.0), LightOccluder2D::rect(4.0, 16.0)));
    }
    ctx.world.spawn((
      Transform::from_xy(96.0, 32.0),
      Light2D::point(Color::WHITE, 20.0).with_intensity(0.3),
    ));
  })?;
  Some(frame.get_pixel(96, 32).0[0])
}

#[test]
fn a_shadow_darkens_only_its_own_light() {
  let (Some(in_shadow), Some(alone)) = (light_b(true), light_b(false)) else {
    return;
  };
  assert!(in_shadow.abs_diff(alone) <= 2, "B alone {alone}, in A's shadow {in_shadow}");
}

/// The red byte at (80, 32) behind `occluders` stacked occluders, under a
/// light whose shadow keeps half its light.
fn behind(occluders: usize) -> Option<u8> {
  let frame = lit(move |ctx| {
    ctx.lighting().set_ambient(AmbientLight::with_intensity(0.0));
    let light = Light2D::point(Color::WHITE, 100.0)
      .with_intensity(0.5)
      .with_shadows()
      .with_shadow_color(Color::new(0, 0, 0, 128));
    ctx.world.spawn((Transform::from_xy(24.0, 32.0), light));
    for i in 0..occluders {
      ctx.world.spawn((
        Transform::from_xy(40.0 + i as f32 * 6.0, 32.0),
        LightOccluder2D::rect(4.0, 16.0),
      ));
    }
  })?;
  Some(frame.get_pixel(80, 32).0[0])
}

#[test]
fn overlapping_shadows_combine_instead_of_cutting_twice() {
  let (Some(open), Some(one), Some(two)) = (behind(0), behind(1), behind(2)) else {
    return;
  };
  let linear = |byte: u8| {
    let c = f32::from(byte) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
  };
  let (open, one, two) = (linear(open), linear(one), linear(two));
  // Alpha 128 keeps about half the light behind one occluder, a quarter behind two
  assert!((one / open - 0.5).abs() < 0.06, "one {one} of {open}");
  assert!((two / open - 0.25).abs() < 0.06, "two {two} of {open}");
}

#[test]
fn a_coloured_shadow_lets_its_colour_through() {
  let Some(frame) = lit(|ctx| {
    ctx.lighting().set_ambient(AmbientLight::with_intensity(0.0));
    let light = Light2D::point(Color::WHITE, 100.0)
      .with_intensity(0.5)
      .with_shadows()
      .with_shadow_color(Color::rgb(255, 0, 0));
    ctx.world.spawn((Transform::from_xy(24.0, 32.0), light));
    ctx.world.spawn((Transform::from_xy(40.0, 32.0), LightOccluder2D::rect(4.0, 16.0)));
  }) else {
    return;
  };
  let pixel = frame.get_pixel(80, 32).0;
  assert!(pixel[0] > 40 && pixel[1] < 10 && pixel[2] < 10, "{pixel:?}");
}
```

The coloured-shadow test already passes (the old colour mask also let red through). It guards the rewrite; the first two fail today.

In `src/core/lighting/shadow.rs`, replace the tests module (keep `project_vertex_extends_to_radius`):

```rust
  #[test]
  fn an_occluder_casts_one_quad_per_back_edge() {
    let light = Light2D::point(crate::core::ui::Color::WHITE, 100.0).with_shadows();
    let occluder = LightOccluder2D::rect(4.0, 16.0);
    let at = GlobalTransform::from_transform(&crate::core::ecs::Transform::from_xy(40.0, 0.0));
    let mut triangles = Triangles::default();
    let range = push_shadow_volumes(Vec2::ZERO, &light, &[(at, &occluder)], &mut triangles);
    assert!(!range.is_empty() && range.len() % 6 == 0, "{range:?}");
    assert_eq!(triangles.vertices.len() * 6, range.len() * 4, "four corners per quad");
    // Every vertex knows its light and is fully strong (black shadow)
    assert!(triangles.vertices.iter().all(|v| v.uv == [0.0, 0.0] && v.color[3] == 1.0));
  }

  #[test]
  fn a_light_without_shadows_casts_none() {
    let light = Light2D::point(crate::core::ui::Color::WHITE, 100.0);
    let occluder = LightOccluder2D::rect(4.0, 16.0);
    let at = GlobalTransform::from_transform(&crate::core::ecs::Transform::from_xy(40.0, 0.0));
    let mut triangles = Triangles::default();
    assert!(push_shadow_volumes(Vec2::ZERO, &light, &[(at, &occluder)], &mut triangles).is_empty());
  }
```

In `src/core/render/batch/pipelines.rs` tests:

```rust
  #[test]
  fn shadowed_lights_mask_their_own_light() {
    use wgpu::BlendFactor as F;
    let (reset, reset_writes) = light_blend(LightStep::ResetMask);
    assert_eq!((reset.alpha.src_factor, reset.alpha.dst_factor), (F::One, F::Zero));
    assert_eq!(reset_writes, wgpu::ColorWrites::ALPHA);
    let (shadow, shadow_writes) = light_blend(LightStep::Shadow);
    assert_eq!((shadow.alpha.src_factor, shadow.alpha.dst_factor), (F::Zero, F::OneMinusSrcAlpha));
    assert_eq!(shadow_writes, wgpu::ColorWrites::ALPHA);
    let (masked, _) = light_blend(LightStep::Masked);
    assert_eq!((masked.color.src_factor, masked.color.dst_factor), (F::DstAlpha, F::One));
    let (coloured, writes) = light_blend(LightStep::Coloured);
    assert_eq!((coloured.color.src_factor, coloured.color.dst_factor), (F::OneMinusDstAlpha, F::One));
    assert_eq!(writes, wgpu::ColorWrites::COLOR);
  }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test e2e -- a_shadow_darkens_only overlapping_shadows 2>&1 | grep -E 'panicked|test result'; cargo test --lib shadow 2>&1 | tail -3`

Expected:
- `a_shadow_darkens_only_its_own_light` fails (B darkened by A's shadow);
- `overlapping_shadows_combine..` fails (both cut to black);
- the unit tests do not compile.

- [ ] **Step 3: Shadow volumes as batch triangles**

Rewrite `src/core/lighting/shadow.rs`, keeping `project_vertex`:

```rust
//! Shadow volumes: the quads each occluder edge facing away from a light
//! casts, as triangles in the batch's triangle stream

use std::ops::Range;

use glam::Vec2;

use super::{Light2D, LightOccluder2D};
use crate::core::ecs::GlobalTransform;
use crate::core::render::batch::{Triangles, Vertex2D};

/// Push the shadow volumes `light` at `position` casts from `occluders`
/// into `triangles`; returns their index range (empty when it casts none).
///
/// Each vertex carries what `shadow.wgsl` needs: the light's position (in
/// `uv`), its radius and shadow softness (in `params`), and its shadow
/// colour in linear light, whose alpha is the shadow's strength.
pub(crate) fn push_shadow_volumes(
  position: Vec2,
  light: &Light2D,
  occluders: &[(GlobalTransform, &LightOccluder2D)],
  triangles: &mut Triangles,
) -> Range<u32> {
  let start = triangles.indices.len() as u32;
  if !light.casts_shadows() {
    return start..start;
  }
  let radius = light.radius();
  let spot = match light {
    Light2D::Point { .. } => None,
    Light2D::Spot { angle, direction, .. } => Some((*angle, *direction)),
  };
  let vertex = |at: Vec2| Vertex2D {
    position: at.into(),
    uv: position.into(),
    color: light.shadow_color().to_linear(),
    params: [radius, light.shadow_softness(), 0.0, 0.0],
    across: [0.0; 2],
    bounds: [0.0; 4],
  };
  for (transform, occluder) in occluders {
    if !occluder.is_enabled() {
      continue;
    }
    let center = transform.translation();
    // Too far for its shadow to reach the light's radius
    if center.distance(position) > radius * 2.0 {
      continue;
    }
    let corners = occluder.transform_vertices(center.x, center.y, transform.rotation());
    if corners.len() < 3 {
      continue;
    }
    for i in 0..corners.len() {
      let (v0, v1) = (corners[i], corners[(i + 1) % corners.len()]);
      if !casts(v0, v1, position, spot) {
        continue;
      }
      let (p0, p1) = (
        project_vertex(v0, position, radius),
        project_vertex(v1, position, radius),
      );
      let first = triangles.vertices.len() as u32;
      triangles
        .vertices
        .extend([vertex(v0), vertex(v1), vertex(p1), vertex(p0)]);
      triangles
        .indices
        .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }
  }
  start..triangles.indices.len() as u32
}

/// Whether the edge `v0`-`v1` faces away from a light at `light` (and, for
/// a spot of `(angle, direction)` in degrees, lies near its cone).
fn casts(v0: Vec2, v1: Vec2, light: Vec2, spot: Option<(f32, f32)>) -> bool {
  let to_edge = (v0 + v1) * 0.5 - light;
  let edge = v1 - v0;
  // Edge normal for counter-clockwise winding: positive means it faces away
  if to_edge.dot(Vec2::new(-edge.y, edge.x)) <= 0.0 {
    return false;
  }
  let Some((angle, direction)) = spot else {
    return true;
  };
  let length = to_edge.length();
  if length <= 0.001 {
    return true;
  }
  let facing = Vec2::from_angle(direction.to_radians());
  let off_axis = facing.dot(to_edge / length).clamp(-1.0, 1.0).acos();
  // A margin past the cone keeps soft edges shadowed
  off_axis <= (angle / 2.0).to_radians() + 0.2
}
```

Delete `ShadowVertex`, `ShadowGeometry`, `generate_shadow_geometry`, `generate_shadow_quads_for_occluder` and `MAX_SHADOW_VERTICES`.

In `src/core/render/batch/mod.rs`, export `Vertex2D` (`pub(crate) use polyline::{PolyPoint, PolyStyle, Triangles, Vertex2D};`). In `src/core/lighting/mod.rs`, replace the shadow exports with `pub(crate) use shadow::push_shadow_volumes;`. Remove them from `src/lib.rs` if listed.

- [ ] **Step 4: The shadow and mask shaders**

Replace `src/core/shaders/core/shadow.wgsl`:

```wgsl
// Shadow volumes as batch triangles (`lighting::push_shadow_volumes`): each
// keeps 1 - strength of its light's mask in the light buffer's alpha.
//
// A vertex carries the light's position (uv), its radius and shadow
// softness (params.xy) and the shadow colour (linear; alpha its strength).

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
    @location(4) across: vec2<f32>,
    @location(5) bounds: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) @interpolate(flat) light: vec2<f32>,
    @location(2) @interpolate(flat) strength: f32,
    // Radius, softness
    @location(3) @interpolate(flat) shape: vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = globals.view_proj * vec4<f32>(in.position, 0.0, 1.0);
    out.world = in.position;
    out.light = in.uv;
    out.strength = in.color.a;
    out.shape = in.params.xy;
    return out;
}

@fragment
fn fs_shadow(in: VertexOutput) -> @location(0) vec4<f32> {
    let reach = clamp(distance(in.world, in.light) / max(in.shape.x, 1e-3), 0.0, 1.0);
    // Soft shadows fade with distance from the light; hard ones stay whole
    let softness = clamp(in.shape.y, 0.0, 1.0);
    let edge = 1.0 - smoothstep(0.0, max(softness * 0.5, 1e-3), reach) * softness;
    return vec4<f32>(0.0, 0.0, 0.0, clamp(in.strength * edge, 0.0, 1.0));
}
```

`compose::SHADOW` (camera include + this file) stays.

Add to `light.wgsl`:

```wgsl
// A shadowed light's mask starts full inside its quad
@fragment
fn fs_mask_reset(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

// The light through a coloured shadow: tinted, where the mask held it back
@fragment
fn fs_light_coloured(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(light_at(in) * in.shadow_color.rgb, 0.0);
}
```

- [ ] **Step 5: Light steps and planning**

`LightStep` in `key.rs`:

```rust
/// One draw of a light into a light buffer. A shadowed light draws, inside
/// its quad: `ResetMask`, its `Shadow` volumes, `Masked`, and `Coloured` if
/// its shadow is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LightStep {
  /// Light added up (lights without shadow volumes)
  Add,
  /// The light buffer's alpha set to 1 in the light's quad
  ResetMask,
  /// Each shadow volume keeps 1 - strength of the alpha
  Shadow,
  /// The light added where the alpha lets it through
  Masked,
  /// The light, tinted by its shadow colour, where the alpha held it back
  Coloured,
}

impl LightStep {
  /// The entry point that draws it.
  pub(crate) fn fragment(self) -> &'static str {
    match self {
      LightStep::Add | LightStep::Masked => "fs_light",
      LightStep::ResetMask => "fs_mask_reset",
      LightStep::Shadow => "fs_shadow",
      LightStep::Coloured => "fs_light_coloured",
    }
  }

  /// The stream it draws from: shadow volumes are triangles, the rest quads.
  pub(crate) fn stream(self) -> Stream {
    match self {
      LightStep::Shadow => Stream::Triangles,
      _ => Stream::Quads,
    }
  }
}
```

`pipelines.rs`:
- `Pipelines` gains `shadow: wgpu::ShaderModule` (`compose::SHADOW`).
- `ensure` gains `(BatchShader::Light(LightStep::Shadow), Stream::Triangles, _, _) => (&self.shadow, &self.camera_only, "fs_shadow"),` before the quad arm.
- `light_blend` covers every step:

```rust
pub(crate) fn light_blend(step: LightStep) -> (wgpu::BlendState, wgpu::ColorWrites) {
  use wgpu::BlendFactor::{DstAlpha, One, OneMinusDstAlpha, OneMinusSrcAlpha, Zero};
  let add = |src_factor, dst_factor| wgpu::BlendComponent {
    src_factor,
    dst_factor,
    operation: wgpu::BlendOperation::Add,
  };
  let keep = add(Zero, One);
  let (color, alpha, writes) = match step {
    // Light adds up; the alpha, a shadowed light's mask, is left alone
    LightStep::Add => (add(One, One), keep, wgpu::ColorWrites::COLOR),
    LightStep::ResetMask => (keep, add(One, Zero), wgpu::ColorWrites::ALPHA),
    LightStep::Shadow => (keep, add(Zero, OneMinusSrcAlpha), wgpu::ColorWrites::ALPHA),
    LightStep::Masked => (add(DstAlpha, One), keep, wgpu::ColorWrites::COLOR),
    LightStep::Coloured => (add(OneMinusDstAlpha, One), keep, wgpu::ColorWrites::COLOR),
  };
  (wgpu::BlendState { color, alpha }, writes)
}
```

`DrawLight` gains:

```rust
  /// Its shadow volumes in the batch's triangle indices (empty: none)
  pub(crate) shadows: std::ops::Range<u32>,
  /// Its shadow is coloured: the light shows through it, tinted
  pub(crate) coloured: bool,
```

`light_item` in `lighting/batch.rs` builds them:

```rust
/// `light`'s draw-list item with its shadow volumes from `occluders` in
/// `triangles`, if it can light anything.
pub(crate) fn light_item(
  light: &FrameLight,
  occluders: &[(GlobalTransform, &LightOccluder2D)],
  triangles: &mut Triangles,
) -> Option<DrawLight> {
  let instance = light_instance(light.position, &light.light)?;
  let reach = Vec2::splat(light.light.radius());
  let shadows = super::push_shadow_volumes(light.position, &light.light, occluders, triangles);
  let [r, g, b, _] = light.light.shadow_color().to_linear();
  Some(DrawLight {
    instance,
    layers: light.layers,
    min: light.position - reach,
    max: light.position + reach,
    shadows,
    coloured: r + g + b > 0.0,
  })
}
```

Update the Task 9 `draw_list.rs` test's `DrawLight` literal with `shadows: 0..0, coloured: false`.

In `gpu.rs` `prepare`, the lit camera's light planning becomes:

```rust
      drawn.lights.clear();
      if camera.lit {
        for step in [
          LightStep::Add,
          LightStep::ResetMask,
          LightStep::Shadow,
          LightStep::Masked,
          LightStep::Coloured,
        ] {
          pipelines.ensure(device, light_key(step, format), None);
        }
        // Lights without shadow volumes add up in one draw
        let start = placed.len() as u32;
        placed.extend(
          list
            .lights_for(camera)
            .filter(|light| light.shadows.is_empty())
            .map(|light| light.instance),
        );
        let end = placed.len() as u32;
        if end > start {
          drawn.lights.push(light_draw(LightStep::Add, start..end));
        }
        // Each shadowed light masks itself: full in its quad, cut by its
        // shadows, then let through (and through a coloured shadow, tinted)
        for light in list.lights_for(camera).filter(|light| !light.shadows.is_empty()) {
          let at = placed.len() as u32;
          placed.push(light.instance);
          drawn.lights.push(light_draw(LightStep::ResetMask, at..at + 1));
          drawn.lights.push(light_draw(LightStep::Shadow, light.shadows.clone()));
          drawn.lights.push(light_draw(LightStep::Masked, at..at + 1));
          if light.coloured {
            drawn.lights.push(light_draw(LightStep::Coloured, at..at + 1));
          }
        }
      }
```

- [ ] **Step 6: The renderer and the engine**

In `src/core/lighting/renderer.rs`:
- remove the shadow pipeline, `shadow_vertex_buffer`, `shadow_bind_group_layout`, `frame_shadow_vertices`, `prepare_shadows` and `draw_shadows_for_camera`;
- what is left is the per-camera targets, the compose pipeline and layout, the sampler, the bind-group cache, ambient, atmosphere, the enabled and UI flags, and the format.

In `src/core/render/passes/lighting.rs`, drop the `draw_shadows_for_camera` call.

In `src/core/engine/extract.rs`:
1. Delete the occluder gathering and `prepare_shadows` from the `resource_scope` closure.
2. Push the lights after the frame's triangles are cleared, with their shadows. Move the `lit` light block from Task 9 to just after `triangles.clear();`:

```rust
      // Lights, each with its shadow volumes, for the lit cameras whose
      // layers and view reach them
      let mut occluders = recycle(std::mem::take(&mut self.occluders));
      if lit {
        let mut query = self.world.query::<OccluderItem>();
        occluders_to_draw(&self.world, &mut query, alpha, &mut occluders);
        for light in &lights {
          if let Some(item) = crate::core::lighting::light_item(light, &occluders, triangles) {
            self.draw_list.push_light(item);
          }
        }
      }
      self.occluders = recycle(occluders);
```

If the borrow checker objects to `self.world.query` while `triangles` borrows `batch`, take the query before `let triangles = batch.triangles();`.

- [ ] **Step 7: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E '^test result:|panicked' | head -20`

Expected: all pass, including `src/core/testing/gpu_world.rs`'s `BusyLit` bind-group tests (a shadow-casting light and an occluder).

- [ ] **Step 8: The gate and commit**

Expected pages: every page with shadow-casting lights (the shadows demo's hard, soft, coloured and placed-occluder pages, and lighting pages with occluders). One light's shadow no longer darkens others, and overlapping shadows no longer go black. Check them by eye, refresh, and ledger the list.

```bash
git add -A
git commit -m "feat: shadows darken only their own light

- each shadowed light masks itself in the light buffer's alpha
- overlapping shadows combine instead of cutting twice; coloured shadows tint the light
- shadow volumes are batch triangles, with no vertex cap"
```

---

## Phase 7: auto-exposure (spec migration step 7)

### Task 11: Auto-exposure

**Files:**
- Create: `src/core/render/exposure.rs`, `src/core/shaders/core/exposure.wgsl`
- Modify:
  - `src/core/render/look.rs` (`AutoExposure`, `Look.auto_exposure`, `LookUniforms.auto`, `num` made `pub(crate)`)
  - `src/core/render/passes/tonemap.rs` (measure, then bind the stored exposure)
  - `src/core/shaders/core/tonemap.wgsl`
  - `src/core/shaders/compose.rs` (`EXPOSURE`, `built_in()`)
  - `src/core/graphics/bind_groups.rs` (`MAX_BINDINGS` 5)
  - `src/core/render/mod.rs`, `src/lib.rs` (exports)
  - `src/core/testing/gpu_world.rs` (`Busy` gets a LUT and auto-exposure)
- Test: `src/core/render/exposure.rs`, `tests/e2e/look.rs`, `src/core/testing/gpu_world.rs`

**Interfaces:**
- Consumes: Task 5's `TonemapGpu`, `Look::uniforms(lut_size)`, `num`.
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq)] pub struct AutoExposure { pub min_ev: f32, pub max_ev: f32, pub compensation: f32, pub speed_up: f32, pub speed_down: f32 }`. `Default` is -4, 4, 0, 3, 1.
  - `Look.auto_exposure: Option<AutoExposure>` (default `None`).
  - `LookUniforms.auto: [f32; 4]` (x: 1 when on; 112 bytes in all).
  - `pub(crate) struct ExposureGpu` with `new(device)`, `measure(device, queue, encoder, scene: &TextureView, size: (u32, u32), settings: Option<&AutoExposure>, dt: f32)` and `state(&self) -> &wgpu::Buffer`.
  - `shaders::compose::EXPOSURE`.

- [ ] **Step 1: Write the failing tests**

`src/core/render/exposure.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  /// The f16 bits of 2^e, a normal power of two.
  fn f16_power_of_two(e: i32) -> u16 {
    ((e + 15) as u16) << 10
  }

  /// A 16x16 float image: the left half grey 2^-6, the right half 2^-4.
  fn two_greys(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let mut texels: Vec<u16> = Vec::new();
    for _ in 0..16 {
      for x in 0..16 {
        let v = f16_power_of_two(if x < 8 { -6 } else { -4 });
        texels.extend([v, v, v, f16_power_of_two(0)]);
      }
    }
    let size = wgpu::Extent3d {
      width: 16,
      height: 16,
      depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Two Greys"),
      size,
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba16Float,
      usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
      view_formats: &[],
    });
    queue.write_texture(
      texture.as_image_copy(),
      bytemuck::cast_slice(&texels),
      wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(16 * 8),
        rows_per_image: Some(16),
      },
      size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
  }

  /// What `average_main` computes, on the CPU: the target EV of `lumas`.
  fn cpu_target(lumas: &[f32], settings: &AutoExposure) -> f32 {
    let mut bins = [0u32; 64];
    for &luma in lumas {
      let t = ((luma.max(1e-12).log2() + 12.0) / 16.0).clamp(0.0, 1.0);
      bins[((t * 64.0) as usize).min(63)] += 1;
    }
    let total = lumas.len() as f32;
    let (low, high) = (total * 0.05, total * 0.95);
    let (mut seen, mut sum, mut weight) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (i, &count) in bins.iter().enumerate() {
      let count = count as f32;
      let kept = ((seen + count).min(high) - seen.max(low)).max(0.0);
      sum += kept * (-12.0 + (i as f32 + 0.5) * 16.0 / 64.0);
      weight += kept;
      seen += count;
    }
    (0.18_f32.log2() - sum / weight + settings.compensation).clamp(settings.min_ev, settings.max_ev)
  }

  /// The exposure the GPU stored.
  fn read_ev(device: &wgpu::Device, queue: &wgpu::Queue, gpu: &ExposureGpu) -> f32 {
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Exposure Readback"),
      size: 16,
      usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(gpu.state(), 0, &staging, 0, 16);
    queue.submit([encoder.finish()]);
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bytes = slice.get_mapped_range();
    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
  }

  fn measure_once(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    gpu: &mut ExposureGpu,
    scene: &wgpu::TextureView,
    settings: &AutoExposure,
    dt: f32,
  ) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    gpu.measure(device, queue, &mut encoder, scene, (16, 16), Some(settings), dt);
    queue.submit([encoder.finish()]);
  }

  #[test]
  fn the_first_measure_is_the_histogram_target() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let scene = two_greys(&device, &queue);
    let settings = AutoExposure::default();
    let mut gpu = ExposureGpu::new(&device);
    measure_once(&device, &queue, &mut gpu, &scene, &settings, 0.0);
    let lumas: Vec<f32> = (0..256).map(|i| if i % 16 < 8 { 2f32.powi(-6) } else { 2f32.powi(-4) }).collect();
    let ev = read_ev(&device, &queue, &gpu);
    assert!((ev - cpu_target(&lumas, &settings)).abs() < 1e-3, "{ev}");
    // The exact log average is 2^-5: within half a bin of its target
    assert!((ev - (0.18_f32.log2() + 5.0)).abs() <= 0.13, "{ev}");
  }

  #[test]
  fn later_measures_move_at_the_speed_limit() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let scene = two_greys(&device, &queue);
    let mut gpu = ExposureGpu::new(&device);
    let low = AutoExposure {
      compensation: -2.0,
      ..AutoExposure::default()
    };
    measure_once(&device, &queue, &mut gpu, &scene, &low, 0.0);
    let start = read_ev(&device, &queue, &gpu);
    // Two stops up wanted; 3 EV/s for a tenth of a second allows 0.3
    measure_once(&device, &queue, &mut gpu, &scene, &AutoExposure::default(), 0.1);
    let moved = read_ev(&device, &queue, &gpu) - start;
    assert!((moved - 0.3).abs() < 1e-3, "{moved}");
  }
}
```

In `tests/e2e/look.rs`:

```rust
fn auto() -> Look {
  Look {
    auto_exposure: Some(AutoExposure::default()),
    ..Look::default()
  }
}

#[test]
fn auto_exposure_starts_at_its_target() {
  let Some(pixel) = shown(square([39, 39, 39, 255], auto())) else {
    return;
  };
  // 0.0203 linear, brought to mid-grey (0.18, sRGB 118) on the first frame,
  // within the histogram's quarter-stop bins
  assert!(pixel[0].abs_diff(118) <= 4, "{pixel:?}");
}

#[test]
fn auto_exposure_stops_at_max_ev() {
  let Some(pixel) = shown(square([3, 3, 3, 255], auto())) else {
    return;
  };
  // 0.00091 linear wants 7.6 EV; at the 4 EV limit it shows 0.0146 (sRGB 32)
  assert!(pixel[0].abs_diff(32) <= 3, "{pixel:?}");
}

#[test]
fn auto_exposure_eases_toward_a_darker_scene() {
  // A dark square under a mid-grey one, which is hidden at frame 10
  let scene = Square {
    color: [39, 39, 39, 255],
    look: auto(),
    setup: Some(Box::new(|ctx: &mut SceneContext| {
      let grey = SpriteHandle::from_sprite(Sprite::solid_rect(32, 32, 118, 118, 118, 255), "look-auto-grey");
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0),
        Renderable::sprite(grey).with_z_order(1),
      ));
    })),
  };
  let Some(mut engine) = start(scene) else {
    return;
  };
  engine.step_frames(10).unwrap();
  let world = engine.world_mut();
  let mut query = world.query::<&mut Renderable>();
  for mut renderable in query.iter_mut(world) {
    if renderable.z_order == 1 {
      renderable.visible = false;
    }
  }
  engine.step_frames(1).unwrap();
  let soon = engine.frame().unwrap().get_pixel(16, 16).0[0];
  engine.step_frames(90).unwrap();
  let later = engine.frame().unwrap().get_pixel(16, 16).0[0];
  // One frame moves 3 EV/s x 1/60 s: still about as dark as the square is
  assert!(soon < 50, "after a frame: {soon}");
  assert!(later.abs_diff(118) <= 4, "after a second and a half: {later}");
}

#[test]
fn auto_exposure_ignores_overflowed_light() {
  // The dark square plus an 8x8 pile-up past f16's range (6% of the frame)
  let scene = Square {
    color: [39, 39, 39, 255],
    look: auto(),
    setup: Some(Box::new(|ctx: &mut SceneContext| {
      let blaze = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 255, 255, 255, 255), "look-auto-blaze");
      for z in 1..=2 {
        ctx.world.spawn((
          Transform::from_xy(4.0, 4.0),
          Renderable::sprite(blaze.clone())
            .with_z_order(z)
            .with_blend(BlendMode::Additive)
            .with_emission(65504.0),
        ));
      }
    })),
  };
  // Away from the blaze
  let Some(pixel) = shown_at(scene, 24, 24) else {
    return;
  };
  assert!(pixel[0].abs_diff(118) <= 4, "{pixel:?}");
}

/// The pixel at (`x`, `y`) after two frames.
fn shown_at(scene: impl Scene + 'static, x: u32, y: u32) -> Option<[u8; 4]> {
  let mut engine = start(scene)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap().get_pixel(x, y).0)
}
```

In `src/core/testing/gpu_world.rs`, `Busy::on_enter` adds a LUT and auto-exposure, so the steady-frame and resize tests cover the tonemap's and exposure's bind groups:

```rust
    let lut = SpriteHandle::from_sprite(ColorLut::identity_strip(16), "busy-lut");
    let mut look = ctx.look();
    look.grading.lut = ColorLut::from_sprite(&lut).ok();
    look.auto_exposure = Some(AutoExposure::default());
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib exposure 2>&1 | tail -3; cargo test --test e2e auto_exposure 2>&1 | tail -3`

Expected: compile errors (`AutoExposure`, `ExposureGpu` missing).

- [ ] **Step 3: The settings**

In `src/core/render/look.rs`:

```rust
/// Exposure that follows the scene's brightness, measured on the GPU each
/// frame: the average luminance, without the darkest and brightest 5%, is
/// brought to mid-grey within `min_ev..=max_ev`, plus `compensation`. It
/// starts at its target and then moves at its speeds, in real time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoExposure {
  /// The least exposure it uses, in EV (default -4)
  pub min_ev: f32,
  /// The most, in EV (default 4)
  pub max_ev: f32,
  /// Added to what it measures, in EV: positive brightens (default 0)
  pub compensation: f32,
  /// How fast it brightens, in EV per second (default 3)
  pub speed_up: f32,
  /// How fast it darkens, in EV per second (default 1)
  pub speed_down: f32,
}

impl Default for AutoExposure {
  fn default() -> Self {
    Self {
      min_ev: -4.0,
      max_ev: 4.0,
      compensation: 0.0,
      speed_up: 3.0,
      speed_down: 1.0,
    }
  }
}
```

Then:
- `Look` gains `/// Exposure that follows the scene (default off); added to \`exposure\`` `pub auto_exposure: Option<AutoExposure>,` (`None` in `Default`).
- `LookUniforms` gains `/// x: 1 when auto-exposure adds its stored EV` `pub(crate) auto: [f32; 4],`, which `uniforms` fills with `[if self.auto_exposure.is_some() { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0]`.
- Make `num` `pub(crate)`.
- Export `AutoExposure` from `render` and the prelude.

- [ ] **Step 4: The measuring shader**

`src/core/shaders/core/exposure.wgsl`:

```wgsl
// Auto-exposure on the GPU: a histogram of the scene's log2 luminance, then
// its average without the darkest and brightest 5%, turned into the
// exposure that brings it to mid-grey and eased toward it. Nothing is read
// back: the tonemap pass reads `state.ev`.

const BINS: u32 = 64u;
const MIN_LOG: f32 = -12.0;
const MAX_LOG: f32 = 4.0;

struct Params {
    // Least EV, most EV, compensation, unused
    ev_range: vec4<f32>,
    // EV per second brightening, EV per second darkening, seconds since the last frame, unused
    speed: vec4<f32>,
}

struct State {
    ev: f32,
    // 0 until the first measure, which starts at its target
    ready: u32,
    pad0: u32,
    pad1: u32,
}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> histogram: array<atomic<u32>, 64>;
@group(0) @binding(2) var<storage, read_write> state: State;
@group(0) @binding(3) var<uniform> params: Params;

var<workgroup> local_bins: array<atomic<u32>, 64>;

// A pixel's bin, or BINS for light that is not finite or reached f16's limit
fn bin_of(c: vec3<f32>) -> u32 {
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    // NaN fails both comparisons; overflowed light the second
    if !(luma >= 0.0 && luma < 60000.0) {
        return BINS;
    }
    let t = (log2(max(luma, 1e-12)) - MIN_LOG) / (MAX_LOG - MIN_LOG);
    return min(u32(clamp(t, 0.0, 1.0) * f32(BINS)), BINS - 1u);
}

@compute @workgroup_size(16, 16)
fn histogram_main(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
) {
    if local < BINS {
        atomicStore(&local_bins[local], 0u);
    }
    workgroupBarrier();
    let size = textureDimensions(scene);
    if id.x < size.x && id.y < size.y {
        let bin = bin_of(textureLoad(scene, vec2<i32>(id.xy), 0).rgb);
        if bin < BINS {
            atomicAdd(&local_bins[bin], 1u);
        }
    }
    workgroupBarrier();
    if local < BINS {
        atomicAdd(&histogram[local], atomicLoad(&local_bins[local]));
    }
}

@compute @workgroup_size(1)
fn average_main() {
    var total = 0u;
    for (var i = 0u; i < BINS; i++) {
        total += atomicLoad(&histogram[i]);
    }
    if total == 0u {
        return;
    }
    let low = f32(total) * 0.05;
    let high = f32(total) * 0.95;
    var seen = 0.0;
    var sum = 0.0;
    var weight = 0.0;
    for (var i = 0u; i < BINS; i++) {
        let count = f32(atomicLoad(&histogram[i]));
        // The part of this bin between the dropped tails
        let kept = max(min(seen + count, high) - max(seen, low), 0.0);
        let log_luma = MIN_LOG + (f32(i) + 0.5) * (MAX_LOG - MIN_LOG) / f32(BINS);
        sum += kept * log_luma;
        weight += kept;
        seen += count;
    }
    let average = sum / max(weight, 1e-6);
    let target_ev = clamp(log2(0.18) - average + params.ev_range.z, params.ev_range.x, params.ev_range.y);
    if state.ready == 0u {
        state.ev = target_ev;
        state.ready = 1u;
        return;
    }
    let delta = target_ev - state.ev;
    let step = select(params.speed.y, params.speed.x, delta > 0.0) * params.speed.z;
    state.ev += clamp(delta, -step, step);
}
```

`compose.rs`: `pub(crate) const EXPOSURE: &str = include_str!("core/exposure.wgsl");` and `("exposure", EXPOSURE),` in `built_in()`, like its other entries.

- [ ] **Step 5: The GPU side**

`src/core/render/exposure.rs`:

```rust
//! Auto-exposure on the GPU: measure the scene image, keep the exposure the
//! tonemap pass reads

use crate::core::graphics::bind_groups::{BindGroupCache, Bound};
use crate::core::render::look::{AutoExposure, num};

/// `exposure.wgsl`'s parameters.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ExposureParams {
  ev_range: [f32; 4],
  speed: [f32; 4],
}

/// `settings` for the shader, sanitized, with `dt` seconds since the last frame.
fn params(settings: &AutoExposure, dt: f32) -> ExposureParams {
  let min_ev = num(settings.min_ev, -4.0, -16.0, 16.0);
  let max_ev = num(settings.max_ev, 4.0, -16.0, 16.0).max(min_ev);
  ExposureParams {
    ev_range: [min_ev, max_ev, num(settings.compensation, 0.0, -16.0, 16.0), 0.0],
    speed: [
      num(settings.speed_up, 3.0, 0.0, 100.0),
      num(settings.speed_down, 1.0, 0.0, 100.0),
      num(dt, 0.0, 0.0, 0.25),
      0.0,
    ],
  }
}

/// The histogram and stored exposure, and the two compute pipelines that
/// fill them.
pub(crate) struct ExposureGpu {
  layout: wgpu::BindGroupLayout,
  histogram_pipeline: wgpu::ComputePipeline,
  average_pipeline: wgpu::ComputePipeline,
  histogram: wgpu::Buffer,
  state: wgpu::Buffer,
  params: wgpu::Buffer,
  bind_groups: BindGroupCache,
  /// It measured last frame: off and on again, it starts at its target
  was_on: bool,
}

impl ExposureGpu {
  pub(crate) fn new(device: &wgpu::Device) -> Self {
    let storage = |binding| wgpu::BindGroupLayoutEntry {
      binding,
      visibility: wgpu::ShaderStages::COMPUTE,
      ty: wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only: false },
        has_dynamic_offset: false,
        min_binding_size: None,
      },
      count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Exposure Bind Group Layout"),
      entries: &[
        wgpu::BindGroupLayoutEntry {
          binding: 0,
          visibility: wgpu::ShaderStages::COMPUTE,
          ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
          },
          count: None,
        },
        storage(1),
        storage(2),
        wgpu::BindGroupLayoutEntry {
          binding: 3,
          visibility: wgpu::ShaderStages::COMPUTE,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
          },
          count: None,
        },
      ],
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Exposure Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::EXPOSURE.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Exposure Pipeline Layout"),
      bind_group_layouts: &[Some(&layout)],
      immediate_size: 0,
    });
    let pipeline = |entry: &str| {
      device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
      })
    };
    let buffer = |label, size, usage| {
      device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: false,
      })
    };
    use wgpu::BufferUsages as U;
    Self {
      histogram_pipeline: pipeline("histogram_main"),
      average_pipeline: pipeline("average_main"),
      histogram: buffer("Exposure Histogram", 64 * 4, U::STORAGE | U::COPY_DST),
      state: buffer("Exposure State", 16, U::STORAGE | U::COPY_DST | U::COPY_SRC),
      params: buffer(
        "Exposure Params",
        std::mem::size_of::<ExposureParams>() as u64,
        U::UNIFORM | U::COPY_DST,
      ),
      layout,
      bind_groups: BindGroupCache::default(),
      was_on: false,
    }
  }

  /// The stored exposure (`exposure.wgsl`'s `State`), which the tonemap
  /// pass reads.
  pub(crate) fn state(&self) -> &wgpu::Buffer {
    &self.state
  }

  /// Measure `scene` and move the stored exposure toward its target; with
  /// `settings` off, forget it so it starts at its target when back on.
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn measure(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    scene: &wgpu::TextureView,
    (width, height): (u32, u32),
    settings: Option<&AutoExposure>,
    dt: f32,
  ) {
    let Some(settings) = settings else {
      if std::mem::replace(&mut self.was_on, false) {
        queue.write_buffer(&self.state, 0, &[0u8; 16]);
      }
      return;
    };
    self.was_on = true;
    queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params(settings, dt)));
    encoder.clear_buffer(&self.histogram, 0, None);
    let group = self.bind_groups.get(
      device,
      "Exposure Bind Group",
      &self.layout,
      &[
        Bound::View(scene.clone()),
        Bound::Buffer(self.histogram.clone()),
        Bound::Buffer(self.state.clone()),
        Bound::Buffer(self.params.clone()),
      ],
    );
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
      label: Some("Exposure Pass"),
      timestamp_writes: None,
    });
    pass.set_bind_group(0, &group, &[]);
    pass.set_pipeline(&self.histogram_pipeline);
    pass.dispatch_workgroups(width.div_ceil(16), height.div_ceil(16), 1);
    pass.set_pipeline(&self.average_pipeline);
    pass.dispatch_workgroups(1, 1, 1);
  }
}
```

Add `pub(crate) mod exposure;` to `src/core/render/mod.rs`. If `ComputePassDescriptor` or `ComputePipelineDescriptor` field names differ in wgpu 30, follow the compiler: the shapes above are wgpu 30's.

- [ ] **Step 6: The tonemap reads the stored exposure**

In `src/core/graphics/bind_groups.rs`, `MAX_BINDINGS` becomes `5` (the tonemap binds five resources).

In `tonemap.wgsl`:
- `Look` gains `// x: 1 when auto-exposure adds its stored EV` `auto: vec4<f32>,` after `gain`.
- Add:

```wgsl
struct Exposure {
    ev: f32,
    ready: u32,
    pad0: u32,
    pad1: u32,
}

@group(0) @binding(4) var<storage, read> exposure: Exposure;
```

`fs_main` exposes with:

```wgsl
    let ev = look.curve.x + select(0.0, exposure.ev, look.auto.x > 0.5);
    let exposed = finite(texel.rgb) * exp2(ev);
```

In `passes/tonemap.rs`:
- `TonemapGpu` gains `exposure: ExposureGpu` (made in `new`). The layout gains binding 4 (`Buffer { ty: Storage { read_only: true }, .. }`, `FRAGMENT`), and the bind group adds `Bound::Buffer(gpu.exposure.state().clone())`.
- The pass struct gains `last_time: Option<f32>`. `execute` computes `dt` from `ctx.scene.time` (real time) as `(time - last).clamp(0.0, 0.25)`, 0 on the first frame, and stores the time.
- Before the render pass:

```rust
    gpu.exposure.measure(
      device,
      queue,
      encoder,
      &source,
      size,
      look.auto_exposure.as_ref(),
      dt,
    );
```

  Here `source` is the scene view already cloned for the bind group, and `look` is `ctx.scene.look` read before destructuring.

- [ ] **Step 7: Run the tests**

Run: `cargo test --lib exposure 2>&1 | grep 'test result'; cargo test --test e2e auto_exposure 2>&1 | grep -E 'panicked|test result'; cargo test --lib steady_frames a_resize_rebuilds 2>&1 | grep 'test result'`

Expected: all pass.

- [ ] **Step 8: The gate and commit**

Expected: `compare 0` (auto-exposure is off by default).

```bash
git add -A
git commit -m "feat: auto-exposure measured on the GPU

- a histogram of the HDR image sets the exposure that brings its average to mid-grey
- it starts at its target and moves at its own speeds, with no readback"
```

---

## Phase 8: the demo, retuning, documentation and verification (spec migration step 8)

### Task 12: The `look` demo

**Files:**
- Create: `examples/demos/look/Cargo.toml`, `examples/demos/look/Ryo.toml`, `examples/demos/look/Trunk.toml`, `examples/demos/look/index.html`, `examples/demos/look/main.rs`
- Test: the demo tour (it scans `examples/demos`) and `cargo build -p look-demo`

**Interfaces:**
- Consumes: `Look`, `Tonemap`, `ColorGrading`, `ColorLut`, `AutoExposure`, `with_emission`, `set_bloom_custom` (Tasks 4–11); demo-kit's `Demo`, `DemoPage`, `Settings`.

- [ ] **Step 1: The crate files**

`examples/demos/look/Cargo.toml`:

```toml
[package]
name = "look-demo"
version = "0.1.0"
edition = "2024"
description = "Tonemapping, grading, LUTs and auto-exposure demo for Ryo2D engine"

[[bin]]
name = "look-demo"
path = "main.rs"

[dependencies]
ryo2d = { path = "../../.." }
demo-kit = { path = "../../demo-kit" }
```

`examples/demos/look/Ryo.toml`:

```toml
[project]
name = "Look Demo"

[paths]
entry_point = "main.rs"
save_data = "data"
assets = "assets"

[system]
resolution = "960x640"
vsync = true
display_mode = "windowed"
show_fps = true

[debug]
show_colliders = false
show_velocities = false
show_origins = false
```

`examples/demos/look/Trunk.toml` and `index.html`: copies of `examples/demos/lighting/`'s, with the title `Ryo2D: Look` and `data-bin="look-demo"`.

- [ ] **Step 2: The pages**

`examples/demos/look/main.rs`:

```rust
//! The look: emission and bloom, tonemap curves, colour grading, LUTs made
//! in code and auto-exposure. Loads no files, so it runs on the web too.

use demo_kit::prelude::*;

fn main() {
  Demo::new("Look")
    .resolution(960, 640)
    .page(GlowPage::default)
    .page(CurvesPage::default)
    .page(GradingPage::default)
    .page(LutPage::default)
    .page(ExposurePage::default)
    .run();
}

/// A grey backdrop over the whole window.
fn backdrop(ctx: &mut SceneContext, grey: u8) {
  let sprite = SpriteHandle::from_sprite(
    Sprite::solid_rect(960, 640, grey, grey, grey, 255),
    format!("look-backdrop-{grey}"),
  );
  ctx.world.spawn((
    Transform::from_xy(480.0, 320.0),
    Renderable::sprite(sprite).with_z_order(-10),
  ));
}

/// A square of `color`, `size` wide, at `at`, glowing by `emission`.
fn square(ctx: &mut SceneContext, at: (f32, f32), size: u32, color: Color, emission: f32) -> Entity {
  let sprite = SpriteHandle::from_sprite(
    Sprite::solid_rect(size, size, color.r, color.g, color.b, 255),
    format!("look-square-{size}-{}-{}-{}", color.r, color.g, color.b),
  );
  ctx
    .world
    .spawn((
      Transform::from_xy(at.0, at.1),
      Renderable::sprite(sprite).with_emission(emission),
    ))
    .id()
}

/// A colour chart left of the settings panel: six hues at three
/// brightnesses, and a grey ramp.
fn palette(ctx: &mut SceneContext) {
  backdrop(ctx, 30);
  let hues = [
    (230, 60, 50),
    (240, 170, 40),
    (230, 220, 60),
    (70, 190, 80),
    (60, 130, 230),
    (170, 80, 210),
  ];
  for (i, &(r, g, b)) in hues.iter().enumerate() {
    for (j, scale) in [1.0, 0.6, 0.3].into_iter().enumerate() {
      let shade = |c: u8| (f32::from(c) * scale) as u8;
      let at = (70.0 + i as f32 * 70.0, 180.0 + j as f32 * 70.0);
      square(ctx, at, 56, Color::rgb(shade(r), shade(g), shade(b)), 1.0);
    }
  }
  for i in 0..7u32 {
    let v = (i * 255 / 6) as u8;
    square(ctx, (70.0 + i as f32 * 60.0, 430.0), 48, Color::rgb(v, v, v), 1.0);
  }
}

// ---------------------------------------------------------------------------
// Glow
// ---------------------------------------------------------------------------

const EMISSIONS: [f32; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];

/// Squares from 1 to 16 times white, under bloom.
struct GlowPage {
  bloom: f32,
  threshold: f32,
  boost: f32,
  squares: Vec<(Entity, f32)>,
}

impl Default for GlowPage {
  fn default() -> Self {
    Self {
      bloom: 1.0,
      threshold: 1.0,
      boost: 1.0,
      squares: Vec::new(),
    }
  }
}

impl DemoPage for GlowPage {
  fn name(&self) -> &str {
    "Glow"
  }

  fn description(&self) -> &str {
    "Emission lifts light past white; bloom glows only from light above its threshold."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Bloom", &mut self.bloom, 0.0..=3.0, 0.1);
    s.float("Threshold", &mut self.threshold, 0.0..=4.0, 0.1);
    s.float("Emission x", &mut self.boost, 0.0..=4.0, 0.25);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    backdrop(ctx, 20);
    self.squares = EMISSIONS
      .iter()
      .enumerate()
      .map(|(i, &emission)| {
        let at = (80.0 + i as f32 * 100.0, 320.0);
        (square(ctx, at, 48, Color::rgb(255, 200, 120), emission), emission)
      })
      .collect();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    ctx.effects().set_bloom_custom(self.threshold, self.bloom, 6.0, 3);
    for &(entity, emission) in &self.squares {
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(entity) {
        renderable.emission = emission * self.boost;
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    for (i, emission) in EMISSIONS.iter().enumerate() {
      let x = 66.0 + i as f32 * 100.0;
      ui.draw_text(&format!("x{}", emission * self.boost), x, 370.0, Color::WHITE);
    }
  }
}

// ---------------------------------------------------------------------------
// Curves
// ---------------------------------------------------------------------------

const CURVES: [&str; 4] = ["Soft", "Clamp", "ACES", "AgX"];

/// Ramps of light from a quarter of white to 64 times it.
struct CurvesPage {
  curve: usize,
  exposure: f32,
  knee: f32,
  white: f32,
}

impl Default for CurvesPage {
  fn default() -> Self {
    Self {
      curve: 0,
      exposure: 0.0,
      knee: 0.8,
      white: 4.0,
    }
  }
}

impl DemoPage for CurvesPage {
  fn name(&self) -> &str {
    "Curves"
  }

  fn description(&self) -> &str {
    "The tonemap curve rolls light above white off to the display: ramps from 1/4 to 64 times white."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Curve", &mut self.curve, &CURVES);
    s.float("Exposure (EV)", &mut self.exposure, -4.0..=4.0, 0.25);
    if self.curve == 0 {
      s.float("Knee", &mut self.knee, 0.0..=0.95, 0.05);
      s.float("White", &mut self.white, 1.0..=16.0, 0.5);
    }
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    backdrop(ctx, 10);
    let rows = [
      Color::rgb(255, 255, 255),
      Color::rgb(255, 140, 40),
      Color::rgb(60, 120, 255),
    ];
    for (row, color) in rows.into_iter().enumerate() {
      for step in 0..9 {
        let emission = 0.25 * 2f32.powi(step);
        let at = (50.0 + step as f32 * 52.0, 220.0 + row as f32 * 80.0);
        square(ctx, at, 40, color, emission);
      }
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let tonemap = match self.curve {
      0 => Tonemap::Soft {
        knee: self.knee,
        white: self.white,
      },
      1 => Tonemap::Clamp,
      2 => Tonemap::Aces,
      _ => Tonemap::AgX,
    };
    let mut look = ctx.look();
    look.tonemap = tonemap;
    look.exposure = self.exposure;
  }
}

// ---------------------------------------------------------------------------
// Grading
// ---------------------------------------------------------------------------

/// White balance, contrast, saturation, lift, gamma and gain on a colour chart.
struct GradingPage {
  temperature: f32,
  tint: f32,
  contrast: f32,
  saturation: f32,
  lift: f32,
  gamma: f32,
  gain: f32,
}

impl Default for GradingPage {
  fn default() -> Self {
    Self {
      temperature: 0.0,
      tint: 0.0,
      contrast: 1.0,
      saturation: 1.0,
      lift: 0.0,
      gamma: 1.0,
      gain: 1.0,
    }
  }
}

impl DemoPage for GradingPage {
  fn name(&self) -> &str {
    "Grading"
  }

  fn description(&self) -> &str {
    "White balance, contrast and saturation on the light; lift, gamma and gain on the display colour."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Temperature", &mut self.temperature, -1.0..=1.0, 0.05);
    s.float("Tint", &mut self.tint, -1.0..=1.0, 0.05);
    s.float("Contrast", &mut self.contrast, 0.25..=3.0, 0.05);
    s.float("Saturation", &mut self.saturation, 0.0..=2.0, 0.05);
    s.float("Lift", &mut self.lift, -0.2..=0.2, 0.01);
    s.float("Gamma", &mut self.gamma, 0.25..=3.0, 0.05);
    s.float("Gain", &mut self.gain, 0.0..=2.0, 0.05);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    palette(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    ctx.look().grading = ColorGrading {
      temperature: self.temperature,
      tint: self.tint,
      contrast: self.contrast,
      saturation: self.saturation,
      lift: Vec3::splat(self.lift),
      gamma: Vec3::splat(self.gamma),
      gain: Vec3::splat(self.gain),
      ..ColorGrading::default()
    };
  }
}

// ---------------------------------------------------------------------------
// LUT
// ---------------------------------------------------------------------------

const LUTS: [&str; 4] = ["Warm film", "Cool night", "Sepia", "Invert"];

/// A 16-step LUT made by mapping every colour of the identity strip.
fn make_lut(name: &str, map: impl Fn([f32; 3]) -> [f32; 3]) -> Option<ColorLut> {
  let strip = ColorLut::identity_strip(16);
  let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
  let data: Vec<u8> = strip
    .data()
    .chunks(4)
    .flat_map(|p| {
      let [r, g, b] = map([p[0], p[1], p[2]].map(|c| f32::from(c) / 255.0));
      [byte(r), byte(g), byte(b), 255]
    })
    .collect();
  let sprite = Sprite::from_rgba(data, strip.width(), strip.height());
  ColorLut::from_sprite(&SpriteHandle::from_sprite(sprite, format!("look-lut-{name}"))).ok()
}

fn lut(index: usize) -> Option<ColorLut> {
  match index {
    0 => make_lut("warm", |[r, g, b]| [r * 1.08 + 0.03, g + 0.01, b * 0.85]),
    1 => make_lut("cool", |[r, g, b]| [r * 0.8, g * 0.92 + 0.02, b * 1.1 + 0.05]),
    2 => make_lut("sepia", |[r, g, b]| {
      let y = 0.299 * r + 0.587 * g + 0.114 * b;
      [y * 1.07 + 0.05, y * 0.95 + 0.02, y * 0.75]
    }),
    _ => make_lut("invert", |[r, g, b]| [1.0 - r, 1.0 - g, 1.0 - b]),
  }
}

/// LUTs built in code, blended by strength.
struct LutPage {
  choice: usize,
  strength: f32,
  luts: Vec<Option<ColorLut>>,
}

impl Default for LutPage {
  fn default() -> Self {
    Self {
      choice: 0,
      strength: 1.0,
      luts: Vec::new(),
    }
  }
}

impl DemoPage for LutPage {
  fn name(&self) -> &str {
    "LUT"
  }

  fn description(&self) -> &str {
    "Colour lookup tables made in code from the identity strip, blended by strength."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("LUT", &mut self.choice, &LUTS);
    s.float("Strength", &mut self.strength, 0.0..=1.0, 0.05);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    palette(ctx);
    self.luts = (0..LUTS.len()).map(lut).collect();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let mut look = ctx.look();
    look.grading.lut = self.luts.get(self.choice).cloned().flatten();
    look.grading.lut_strength = self.strength;
  }
}

// ---------------------------------------------------------------------------
// Auto-exposure
// ---------------------------------------------------------------------------

/// A dark cave opening onto daylight; the view pans between them.
struct ExposurePage {
  auto: bool,
  compensation: f32,
  speed_up: f32,
  speed_down: f32,
  time: f32,
}

impl Default for ExposurePage {
  fn default() -> Self {
    Self {
      auto: true,
      compensation: 0.0,
      speed_up: 3.0,
      speed_down: 1.0,
      time: 0.0,
    }
  }
}

impl DemoPage for ExposurePage {
  fn name(&self) -> &str {
    "Auto-exposure"
  }

  fn description(&self) -> &str {
    "Exposure follows the scene as the view pans between a dark cave and daylight."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Auto-exposure", &mut self.auto);
    s.float("Compensation (EV)", &mut self.compensation, -2.0..=2.0, 0.25);
    s.float("Brightening (EV/s)", &mut self.speed_up, 0.25..=8.0, 0.25);
    s.float("Darkening (EV/s)", &mut self.speed_down, 0.25..=8.0, 0.25);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // Rock from x -600 to 600, sky from 600 to 1500
    let rock = SpriteHandle::from_sprite(Sprite::solid_rect(1200, 640, 40, 34, 30, 255), "look-cave");
    ctx
      .world
      .spawn((Transform::from_xy(0.0, 320.0), Renderable::sprite(rock).with_z_order(-1)));
    let sky = SpriteHandle::from_sprite(Sprite::solid_rect(900, 640, 200, 220, 255, 255), "look-sky");
    ctx.world.spawn((
      Transform::from_xy(1050.0, 320.0),
      Renderable::sprite(sky).with_z_order(-1).with_emission(4.0),
    ));
    for i in 0..6 {
      square(ctx, (-400.0 + i as f32 * 120.0, 420.0), 40, Color::rgb(90, 70, 50), 1.0);
    }
    for i in 0..5 {
      square(ctx, (800.0 + i as f32 * 110.0, 400.0), 40, Color::rgb(60, 140, 60), 4.0);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.time += dt;
    let x = 450.0 + 500.0 * (self.time * 0.25).sin();
    ctx.camera().set_position(x, 320.0);
    ctx.look().auto_exposure = self.auto.then_some(AutoExposure {
      compensation: self.compensation,
      speed_up: self.speed_up,
      speed_down: self.speed_down,
      ..AutoExposure::default()
    });
  }
}
```

- [ ] **Step 3: Build and tour it**

Run: `cargo build -p look-demo 2>&1 | tail -2 && ./dev.sh demos:tour look 2>&1 | tail -5`

Expected: it builds; the tour captures `target/demo-tour/look/page-1.png` to `page-5.png` with no failures.

Open each PNG and check:
- Glow: squares from x1 to x16 glow more as emission rises, and x1 does not glow.
- Curves: the Soft ramp rolls off without banding.
- Grading and LUT: the colour chart is neutral at their defaults.
- Auto-exposure: the cave is lit to mid-grey.

- [ ] **Step 4: The gate and commit**

Expected: `compare 0` (`compare.sh` walks only the baseline's files, so the new `look/` pages are not compared yet). Refresh the baseline so they are from now on.

```bash
git add -A
git commit -m "feat: look demo

- emission and bloom, tonemap curves, grading, LUTs made in code and auto-exposure"
```

### Task 13: Retune the demos and check the games

The colour rule (Task 3) and the look (Tasks 4–7, 10) changed how tuned scenes read. Each changed page now reads as intended again, or is recorded as better by design (bloom only above white, shadows no longer darkening other lights).

**Files (what the ledger lists as changed, at least):**
- `examples/demos/lighting/main.rs`, `examples/demos/lighting-volumetric/main.rs`, `examples/demos/bloom/main.rs`, `examples/demos/post-processing/main.rs`, `examples/demos/shadows/scenes/*.rs`
- `examples/games/leap-king/src/scenes/{menu,game}.rs`, `examples/games/nigh_t/scenes/main_menu.rs`, `examples/games/bullet-hell/src/scenes/game/*.rs`, `examples/games/flappy/src/scenes/menu.rs`

- [ ] **Step 1: Measure every changed page against its original look**

```bash
for f in $(cd target/hdr-baseline/demo-tour && ls */page-*.png); do
  [ -f "target/demo-tour/$f" ] || continue
  before=$(magick "target/hdr-baseline-task0/demo-tour/$f" -colorspace Gray -format '%[fx:mean]' info:)
  now=$(magick "target/demo-tour/$f" -colorspace Gray -format '%[fx:mean]' info:)
  awk -v f="$f" -v b="$before" -v n="$now" 'BEGIN { r = (b > 0) ? n / b : 1; if (r < 0.9 || r > 1.1) printf "%s %.2f\n", f, r }'
done
```

Expected: a list of pages whose mean brightness moved by more than 10%, with the ratio.

- [ ] **Step 2: Retune by these rules**

1. **Light, ambient and atmosphere colours tuned under the old rule** (the bytes were read as linear):
   - Where a page or game scene got darker than 0.9 of its original, keep its look by re-encoding each channel: the new byte is the sRGB encoding of the old byte read as linear.
   - Compute it with `python3 -c 'import sys; c=int(sys.argv[1])/255; print(round(255*(1.055*c**(1/2.4)-0.055 if c>0.0031308 else 12.92*c)))' <byte>` (for example 128 becomes 188, 200 becomes 231).
   - Apply it in `lighting/main.rs`, `lighting-volumetric/main.rs` (light presets, ambient, atmosphere), the shadows pages' `set_ambient`/light colours, `leap-king` and `nigh_t`.
2. **Line, trail and debug colours** keep their named colours: they now match sprites, which is the fix. Check `trails`, `pathfinding`, `steering` and `bullet-hell` by eye.
3. **Effect colours** (fade, tint, flash in `post-processing`, `knockback`, `sprite-shaders`, `bullet-hell`, `flappy`, `lighting-volumetric`) keep their named colours. A flash or tint that now reads too weak, because a mid-colour decodes darker, gets a higher intensity, not a different colour.
4. **Bloom** (`bloom/main.rs`):
   - read `BloomPage`'s presets;
   - every preset threshold below 1.0 becomes 1.0;
   - the sprites meant to glow take `.with_emission(3.0)`, the brightest `.with_emission(6.0)`;
   - the description says bloom glows from light above white.
5. **God rays** (`lighting-volumetric`): if rays now overpower the frame without their old clamp, lower the page's god-ray intensity until the sun's rays read as before.
6. **Shadows:** pages with several shadow-casting lights are now correct (one light's shadow no longer darkens another). Leave them unless a page's description says otherwise.

After each demo's edit, run `./dev.sh demos:tour <demo>` and look at the pages. For the games:

```bash
for g in breakout bullet-hell flappy leap-king nigh_t; do
  (cd examples/games/$g && RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=60 RYO_CAPTURE_EXIT=1 \
    RYO_CAPTURE_DIR="$PWD/../../../target/hdr-now/games/$g" cargo run -q -p $g)
done
```

Compare each `frame-000060.png` with `target/hdr-baseline/games/<game>/frame-000060.png` by eye.

- [ ] **Step 3: Record and run the gate**

Ledger: every changed page and game, the rule applied, and pages left changed on purpose (with why). Run the gate.

Expected: tests and lint clean. `compare` lists the retuned pages; check them by eye and refresh.

```bash
git add -A
git commit -m "chore: retune demos and games for the HDR look

- light, ambient and atmosphere colours re-encoded where scenes were tuned under the old colour rule
- the bloom demo glows from emissive sprites"
```

### Task 14: Documentation

**Files:**
- Modify:
  - `src/core/render/AGENTS.md`, `src/core/lighting/AGENTS.md`, `src/core/shaders/AGENTS.md`, `src/core/graphics/AGENTS.md`
  - `docs/architecture/overview.md`
  - `.claude/skills/{ryo-api,ryo-renderer,ryo-lighting,ryo-game-dev}/SKILL.md`, then copied to `src/cli/skills/<name>.md`
  - `src/cli/commands/install_skills.rs` (only if a skill's frontmatter description changes)
  - `examples/demos/AGENTS.md` (if it lists the demos)

- [ ] **Step 1: Module docs**

`src/core/render/AGENTS.md`:
1. **The pass table** in order: `clear`, `world`, `lighting`, `unlit_world` (per camera), `cameras`, `hdr_effects`, `tonemap`, `transition`, `post_process`, `ui`, `composite`, `capture`. One line each, as in the code's `pass_ids` docs.
2. **Images and formats:**
   - `SCENE_FORMAT` (`Rgba16Float`, linear, premultiplied, may exceed 1) for the game, lit and camera images, render targets, the lighting copy and the HDR ping-pong pair;
   - the display image (the display stage's buffer A, `Rgba8UnormSrgb`) and its pair;
   - `FrameImage::{Game, Lit, Camera, Hdr, Display}`;
   - `RenderContext::format()` by stage.
3. **The look:**
   - the tonemap pass's order (exposure and auto EV, white balance, contrast and saturation, the curve, lift/gamma/gain, sRGB and the LUT);
   - `Tonemap::apply` in `look.rs` mirrors `tonemap.wgsl`: change them together; `every_curve_on_the_gpu_matches_tonemap_apply` pins them;
   - the LUT as a 3D texture following its sprite's generation;
   - `exposure.rs`'s two compute dispatches and the stored EV the tonemap binds.
4. **Lights in the batch:**
   - `DrawList::lights_for`;
   - each lit camera's light draws: `Add` for unshadowed lights in one draw; per shadowed light `ResetMask`, `Shadow` (triangles), `Masked`, and `Coloured` when the shadow is coloured;
   - the blend of each (`light_blend`), and why the mask lives in the light buffer's alpha (no stencil, core WebGPU only).

`src/core/lighting/AGENTS.md`:
- lights are batch quads (`batch.rs`), with layers and view culling and no cap;
- shadow volumes are batch triangles per light (`shadow.rs`); each shadow darkens only its own light;
- strength is the shadow colour's alpha times the softness edge term; overlapping shadows multiply;
- compose is `lit = scene × (ambient + light)` plus fog, in float, with no clamp;
- every colour decodes through `Color::to_linear`;
- god rays are placed in frame coordinates by `god_rays.rs` and take the atmosphere's settings;
- `LightingRenderer` is engine-internal.

`src/core/shaders/AGENTS.md`:
- effect stages (`EffectStage`, two `PostProcessPipeline`s by format, `add_custom_in`);
- bloom and god-ray thresholds on linear luminance with the knee above the threshold;
- display effects' colours decoded;
- the sprite shader's linear-page entries (`fs_*_linear`, `linear_page`, `straight_srgb`) and emission (`transform0.w` holds emission − 1; `finish*` multiply by it).

`src/core/graphics/AGENTS.md`: render-target pages are `SCENE_FORMAT` and sampled as linear premultiplied colour (`PageKind::Linear`); the instance's emission lane.

`docs/architecture/overview.md`: the frame section shows the spec's diagram (per camera into `Rgba16Float`; per frame `cameras → hdr_effects → tonemap → transition → post_process → ui → composite → capture`), and says where the look and the effect stages live.

- [ ] **Step 2: Skills**

`.claude/skills/ryo-api/SKILL.md`:
- add `Look` (fields and defaults), `Tonemap` (variants, `apply`), `ColorGrading` (fields, defaults), `ColorLut` (`from_sprite`, `size`, `identity_strip`, the strip layout), `LutError`, `AutoExposure` (fields, defaults) and `ctx.look()`;
- `emission`/`with_emission` on `Renderable`, `SpriteDraw`, `SpriteInstance` (with `emission()`), `ParticleEmitter`, `Trail2D` and `Line2D`;
- `RenderLayers` on lights; `Color::to_linear`;
- `EffectStage`, `PostProcessEffect::stage`, `add_custom_in`, `active_effects_in` and `has_active_effects_in`;
- bloom's threshold (linear luminance, default 1.0, unbounded);
- `pass_ids::HDR_EFFECTS` and `pass_ids::TONEMAP`;
- removed: `with_raymarched_shadows`, `LightingRenderer`, `LightingParams`, `MAX_LIGHTS` and the shadow geometry exports.
- Fix the `SpriteDraw` line (it gains `emission`).

`ryo-renderer`:
- the two stages and their formats;
- what a game pass sees before and after `tonemap` (`RenderContext::format()`);
- the tonemap pass and its order;
- linear render-target pages;
- emission and bloom;
- effect stages and where custom effects run.

`ryo-lighting`:
- lights through the batch (layers, culling, no cap);
- per-light shadows and coloured shadows (stained glass);
- colours decoded from sRGB (a light of `Color(128, 128, 128)` is mid-grey light);
- intensity above 1 now shows (rolled off by the look);
- the compose formula in HDR; god rays on the whole frame;
- ray-marched shadows removed (use `with_soft_shadows`).

`ryo-game-dev` gains three recipes:

```rust
// A glowing pickup: emission above 1, unlit so it glows in the dark, and bloom
ctx.world.spawn((
  Transform::from_xy(x, y),
  Renderable::sprite(gem).with_emission(4.0).with_lighting(false),
));
ctx.effects().set_bloom(1.0);

// A moody level: cooler, less saturated, crushed blacks, and a LUT
let night = ctx.assets.load_sprite("luts/night.png").ok();
ctx.look().grading = ColorGrading {
  temperature: -0.3,
  saturation: 0.8,
  contrast: 1.2,
  lift: Vec3::splat(-0.02),
  lut: night.as_ref().and_then(|strip| ColorLut::from_sprite(strip).ok()),
  lut_strength: 0.6,
  ..ColorGrading::default()
};

// A cave: exposure follows the player from the dark into daylight
ctx.look().auto_exposure = Some(AutoExposure {
  compensation: -0.5,
  ..AutoExposure::default()
});
```

Copy each edited skill to `src/cli/skills/<name>.md`:

```bash
for s in ryo-api ryo-renderer ryo-lighting ryo-game-dev; do cp .claude/skills/$s/SKILL.md src/cli/skills/$s.md; done
```

- [ ] **Step 3: The gate and commit**

Expected: tests and lint clean; `compare 0`.

```bash
git add -A
git commit -m "docs: HDR rendering, the look, effect stages and lighting on the batch"
```

### Task 15: Verification

**Files:** none, unless a check fails (then fix test-first in the task the failure belongs to, and record a ruling).

- [ ] **Step 1: The whole suite and lint**

Run: the gate's first two commands.

Expected: failures `0`; no lint lines.

- [ ] **Step 2: The tour against the original look**

Run: `./dev.sh demos:tour >/dev/null; for f in $(cd target/hdr-baseline-task0/demo-tour && find . -name '*.png' | sort); do cmp -s "target/hdr-baseline-task0/demo-tour/$f" "target/demo-tour/$f" || echo "$f"; done`

Expected: a list of every page that differs from before 2c. Open each one beside its original and confirm the difference is one the ledger explains. Record the full list.

- [ ] **Step 3: The games**

Capture all five at frame 60 (Task 13's loop) and compare with the baseline by eye. Record the result.

- [ ] **Step 4: The web**

```bash
./dev.sh web lighting
```

Run it in the background; trunk serves on `http://127.0.0.1:8080`. With the Playwright MCP servers (Chromium, WebKit, Firefox):
1. Open the page in each browser.
2. Wait for the first frame.
3. Take a screenshot, and read the console for errors.
4. Repeat with `./dev.sh web look` on all five pages (`]` moves to the next page).

Expected: all three browsers draw both demos with no WebGPU validation errors. Then delete the repo root's `.playwright-mcp/`.

- [ ] **Step 5: The benchmark**

Run: `cargo bench --bench foundation -- "headless frame" 2>&1 | grep -A1 '^headless frame' | grep time`

Expected: within 20% of Task 0's figure (about 1.02 ms, so at most about 1.22 ms). Record it.

- [ ] **Step 6: Final review and finishing**

Final review of the whole branch, then `superpowers:finishing-a-development-branch`. The merge into `main` is the owner's (`! git -C /Users/mbreckon/Projects/ryo2d merge --ff-only renderer-hdr`). After the merge:
- delete this plan (`docs: remove the executed renderer HDR plan`), with the owner's approval of the commit;
- remove the worktree and branch;
- update the overhaul memory.
