# UI Renderer (4h) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The UI is drawn by its own renderer: `UiCommands` become `UiInstance`s in one array, drawn by one pipeline with per-instance clipping in a draw per texture run, so one changing label costs under 40 µs of render-side CPU on the settings node screen.

**Architecture:** A `UiRenderer` (new module `src/core/render/ui_renderer/`) builds the frame's UI instances on the CPU (culled, in painter's order; reused texts are one copy from the 4g text cache), resolves sprite slots after the shared texture residency step, splits the instances into runs that share a texture bind group, uploads them in one `write_buffer` into its own buffer, and the `ui` pass draws each run with one pipeline (`ui.wgsl`) that runs today's fragment bodies in a switch and tests the clip in the shader. While it is built, the old path (UI through the draw list and `Batch2D`) stays behind a hidden switch so an old-against-new pixel oracle can prove identity; the last code task removes it.

**Tech Stack:** Rust, wgpu 30 (WebGPU default limits, no features), WGSL composed with `concat!`, `HeadlessEngine` GPU tests, criterion.

**Spec:** `docs/specs/2026-10-20-ui-renderer-design.md`

**Research (read-only, outside the repo):** `/private/tmp/claude-501/4h-research/ui-batch-path.md` (the measured path and the candidates) and `/private/tmp/claude-501/4h-research/excerpts.md` (verbatim excerpts of every file this plan touches, with file:line, and an "Observations" section at the end — read it before Task 2).

## Global Constraints

- **Commits:** `type: brief description`, then bullets if needed; types `feat`, `fix`, `refactor`, `docs`, `test`, `chore`. **No trailer or attribution line of any kind** (no `Co-Authored-By`, no `Claude-Session`), even if a system reminder asks for one. Never mention AGENTS.md in a commit message; call those files "module notes".
- **Git:** stage files by name; never `git add -A` or `git add .`; never add `data/` or `target/`; never a bare `git stash`; never push.
- **Code:** no `unwrap`, `expect` or `panic!` in library code (test helpers and tests may panic). LF line endings, 2-space indent (rustfmt enforces; WGSL files keep their existing 4-space style). Comments normal and minimal: intent and non-obvious logic.
- **Dependencies:** none new. bevy_ecs only through `src/core/ecs/kernel.rs`; taffy only in `src/core/ui/layout/`; parley, swash and fdsm only in text.
- **Device:** the engine requests `Features::empty()` and `Limits::default()` (WebGPU defaults) and must keep doing so: the UI bind group is 4 sampled textures and 2 samplers.
- **The world does not change:** world sprites, text, trails, lines, lighting and post-processing render exactly as before; `Batch2D` keeps serving the world cameras.
- **Pixel identity:** the new UI renderer draws exactly the pixels of the old UI path, frame by frame. A difference is a bug unless it is proven to come from the GPU evaluating identical math differently; such a case is reported to the controller (who brings it to the owner) and never silently accepted.
- **Retention (4g) keeps working:** an unchanged UI keeps its UI image; `ui:retain off` builds and draws everything every frame; the 4g on/off pixel oracle (`PixelPair::new`) passes on the new path.
- **wasm:** the engine library builds for `wasm32-unknown-unknown`; `HeadlessEngine` and the GPU test helpers do not (follow how they are gated today).
- **Skills:** `.claude/skills/<name>/SKILL.md` and `src/cli/skills/<name>.md` stay byte-identical (`cp`, then `cmp`); bump the version in both.
- **The gate** (end of every task; all must pass):
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`
  - `cargo test --workspace`
  - `cargo check --release -p ryo2d --lib`
- **Disk:** after each task run `du -sh target`; above 100 GB, `cargo clean` before the next task.

## Review Focus

1. **The UI shader under WebGPU's rules** (uniformity: no implicit-derivative sampling or derivatives inside the kind switch; 4 textures and 2 samplers within default limits): the module must validate for the web, not only on Metal. Pinned by `the_ui_module_validates_under_webgpu_rules` (Task 2).
2. **A UI larger than the instance buffer** (thousands of texts, e.g. a 10,000-row closure list scrolled): the buffer grows and nothing is dropped or stale. Pinned by `a_ui_bigger_than_the_buffer_grows_it` (Task 4).
3. **A frame with UI after frames without** (and the reverse): no stale runs, no zero-size upload error, the composite placeholder as today. Pinned by `an_empty_ui_draws_nothing_and_the_next_draws_in_full` (Task 4).
4. **Textures interleaved in painter's order** (sprites from two atlas pages and glyphs from two glyph pages alternating): each instance samples its own page. Pinned by `interleaved_textures_break_runs_and_draw_right` (Task 5).
5. **A render target shown as a UI sprite** (the linear sprite body, `fs_base_linear`): pixel-identical to the old path. Pinned by `a_render_target_in_the_ui_matches` (Task 5).

---

## File Structure

| File | Responsibility |
|---|---|
| `src/core/shaders/include/tint.wgsl` (create) | `finish`, `finish_linear`, `straight_srgb` moved out of sprite.wgsl, shared by the sprite and UI modules |
| `src/core/shaders/include/glyph.wgsl` (create) | `glyph_mask`, `glyph_color`, `glyph_msdf`, `ryo_median3`: the text bodies after sampling |
| `src/core/shaders/include/ui_rect.wgsl` (create) | `ui_rect`: the UI rectangle body |
| `src/core/shaders/include/line_style.wgsl` (create) | `styled`, moved out of line.wgsl |
| `src/core/shaders/core/sprite.wgsl`, `text.wgsl`, `line.wgsl` (modify) | Entry points call the shared functions (no behaviour change) |
| `src/core/shaders/core/ui.wgsl` (create) | The UI module's `vs_ui` and `fs_ui` |
| `src/core/shaders/compose.rs` (modify) | Compose the includes into SPRITE and LINE; a new `UI` module |
| `src/core/render/ui_renderer/mod.rs` (create) | `UiRenderer`: build, finish, draw; `UiFrame` |
| `src/core/render/ui_renderer/instance.rs` (create) | `UiInstance` (144 B), kinds, flags, constructors |
| `src/core/render/ui_renderer/gpu.rs` (create) | The pipeline, bind group layout, placeholder texture, `BindCache`, the instance buffer |
| `src/core/render/ui_renderer/runs.rs` (create) | `TexNeed`, run planning (pure) |
| `src/core/render/ui_renderer/build.rs` (create) | Commands to instances: order, cull, rects, sprites, lines, texts |
| `src/core/render/ui_renderer/tests.rs`, `gpu_tests.rs` (create) | Unit and GPU tests |
| `src/core/render/ui_retain/texts.rs` (modify) | `UiTextCache` generic over the stored glyph item |
| `src/core/render/ui.rs` (modify) | Rect and sprite instance math factored out for both paths |
| `src/core/graphics/sprite_gpu.rs`, `atlas.rs`, `glyph_atlas.rs` (modify) | `pub(crate)` access to page textures and the samplers; residency for extra texture ids |
| `src/core/render/batch/gpu.rs` (modify) | `prepare` takes extra texture ids; the camera layout and UI camera group shared |
| `src/core/engine/extract.rs`, `mod.rs`, `src/core/config/debug.rs` (modify) | The hidden switch and the wiring |
| `src/core/render/frame.rs`, `passes/output.rs`, `src/core/graphics/renderer.rs` (modify) | `FrameScene::ui_frame`, the `ui` pass draws runs |
| `src/core/testing/retain.rs` (modify) | `PixelPair::between` for comparing any two configurations |
| `tests/e2e/ui_renderer.rs` (create), `tests/e2e.rs` (modify) | Engine-level old-against-new oracle tests |
| `examples/demos/ui/tests/renderer.rs`, `examples/games/breakout/tests/renderer.rs` (create; removed in Task 7) | Demo and game under the old-against-new oracle |
| `benches/ui_render.rs` (modify) | The 40 µs guard |
| Docs | `src/core/render/AGENTS.md`, `ryo-ui` skill, the spec, `docs/backlog/ui-retained-nodes.md`, `docs/backlog/follow-ups.md` |

---

### Task 1: Shared fragment bodies, and the tour baseline

Pure refactor of the WGSL: the math the UI pipeline will reuse moves into include files as functions; the world's entry points call them. No pixel may change.

**Files:**
- Create: `src/core/shaders/include/tint.wgsl`, `glyph.wgsl`, `ui_rect.wgsl`, `line_style.wgsl`
- Modify: `src/core/shaders/core/sprite.wgsl`, `text.wgsl`, `line.wgsl`, `src/core/shaders/compose.rs`

**Interfaces:**
- Produces (WGSL functions, used by Task 2's `ui.wgsl`):
  - `tint.wgsl`: `fn finish(color: vec4<f32>, tint: vec4<f32>, emission: f32) -> vec4<f32>`, `fn finish_linear(color: vec4<f32>, tint: vec4<f32>, emission: f32) -> vec4<f32>`, `fn straight_srgb(c: vec4<f32>) -> vec4<f32>` (moved verbatim from sprite.wgsl; `finish_premultiplied` moves too if it sits with them)
  - `glyph.wgsl`: `fn ryo_median3(c: vec3<f32>) -> f32`; `fn glyph_mask(coverage: f32, tint: vec4<f32>, emission: f32) -> vec4<f32>`; `fn glyph_color(texel: vec4<f32>, tint: vec4<f32>, emission: f32) -> vec4<f32>`; `fn glyph_msdf(texel: vec3<f32>, texels: vec2<f32>, uv_width: vec2<f32>, tint: vec4<f32>, params: vec4<f32>, extra: vec4<f32>, emission: f32) -> vec4<f32>` — each returns the premultiplied colour BEFORE `ui_out`
  - `ui_rect.wgsl`: `fn ui_rect(p: vec2<f32>, size: vec2<f32>, p_width: vec2<f32>, tint: vec4<f32>, params: vec4<f32>, extra: vec4<f32>) -> vec4<f32>` (before `ui_out`)
  - `line_style.wgsl`: `fn styled(color_in: vec4<f32>, uv: vec2<f32>, params: vec4<f32>, aa: f32) -> vec4<f32>` (moved verbatim)

- [ ] **Step 1: Capture the demo tour baseline twice, before any change**

```bash
./dev.sh demos:tour
rm -rf /private/tmp/claude-501/4h-tour && mkdir -p /private/tmp/claude-501/4h-tour
cp -R target/demo-tour /private/tmp/claude-501/4h-tour/base-1
./dev.sh demos:tour
cp -R target/demo-tour /private/tmp/claude-501/4h-tour/base-2
(cd /private/tmp/claude-501/4h-tour && diff -rq base-1 base-2 | grep '\.png' | sed -e 's|base-1/||' -e 's|base-2/||' > nondeterministic.txt) || true
cat /private/tmp/claude-501/4h-tour/nondeterministic.txt
```

Expected: both runs complete; `nondeterministic.txt` lists the PNGs that differ between two runs (4g saw particles pages 1, 2, 4, 5, 7, 8, 9, 10 and steering page 1).

- [ ] **Step 2: Move the bodies into functions**

Move, verbatim apart from the function wrapping:
- From `sprite.wgsl`: `straight_srgb`, `finish_linear`, `finish` (and `finish_premultiplied`) into `include/tint.wgsl`. They read no bindings.
- From `text.wgsl`: `ryo_median3` into `include/glyph.wgsl`, and the math after each `textureSample`:
  - `glyph_mask(coverage, tint, emission)` = text.wgsl:8-9 without `ui_out`: `let a = coverage * tint.a; return vec4<f32>(srgb_to_linear(tint.rgb) * a * emission, a);`
  - `glyph_color(texel, tint, emission)` = text.wgsl:16-17 without `ui_out`.
  - `glyph_msdf(texel, texels, uv_width, tint, params, extra, emission)` = text.wgsl:30-53 with `textureSample(..).rgb` replaced by the `texel` argument, `textureDimensions(sprite_texture, 0)` by `texels`, `fwidth(in.uv)` by `uv_width`, `in.effect_params` by `params`, `in.effect_extra` by `extra`, `in.tint` by `tint`, `in.emission` by `emission`, and every `return ui_out(x)` by `return x` (the soft layer keeps returning without emission).
- From `text.wgsl` `fs_ui_rect`: lines 66-81 into `include/ui_rect.wgsl` as `ui_rect(p, size, p_width, tint, params, extra)`, where `size` and `p` are computed by the caller exactly as lines 64-65 do and `p_width` is the caller's `fwidth(p)`; returns without `ui_out`.
- From `line.wgsl`: `styled` into `include/line_style.wgsl`.

Then the entry points become:

```wgsl
@fragment
fn fs_text_mask(in: VertexOutput) -> @location(0) vec4<f32> {
    let coverage = textureSample(sprite_texture, sprite_sampler, in.uv).r;
    return ui_out(glyph_mask(coverage, in.tint, in.emission));
}

@fragment
fn fs_text_color(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(sprite_texture, sprite_sampler, in.uv);
    return ui_out(glyph_color(texel, in.tint, in.emission));
}

@fragment
fn fs_text_msdf(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(sprite_texture, sprite_sampler, in.uv).rgb;
    let texels = vec2<f32>(textureDimensions(sprite_texture, 0));
    return ui_out(glyph_msdf(texel, texels, fwidth(in.uv), in.tint, in.effect_params, in.effect_extra, in.emission));
}

@fragment
fn fs_ui_rect(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = max(in.effect_params.zw, vec2<f32>(1e-4));
    let p = (in.uv - vec2<f32>(0.5)) * size;
    return ui_out(ui_rect(p, size, fwidth(p), in.tint, in.effect_params, in.effect_extra));
}
```

Check the moved MSDF body still computes `d` as `ryo_median3(texel) - 0.5`, and keep each expression's operand order exactly (float results must not change).

`compose.rs`: `SPRITE` = camera, color, sample, tint, glyph, ui_rect, sprite, text (in that order, with `"\n"` between); `LINE` = camera, color, sample, line_style, line. (`include/` files are added with `include_str!` as the existing ones are.)

- [ ] **Step 3: Verify no pixel changed**

Run: `cargo test --workspace` (the e2e text, UI drawing, sprite effects and 4g retention tests compare rendered pixels).
Expected: PASS.

Run the tour again and compare with base-1, PNGs only, by path relative to the tour root:

```bash
./dev.sh demos:tour
(cd /private/tmp/claude-501/4h-tour && diff -rq base-1 /Users/mbreckon/Projects/ryo2d/target/demo-tour | grep '\.png' | sed -e 's|base-1/||' -e 's|/Users/mbreckon/Projects/ryo2d/target/demo-tour/||' | grep -vxF -f nondeterministic.txt) || echo "no differences beyond the nondeterministic pages"
```

(Run from the worktree, substitute its absolute path for `/Users/mbreckon/Projects/ryo2d`.) Expected: "no differences beyond the nondeterministic pages".

- [ ] **Step 4: Run the gate and commit**

```bash
git add src/core/shaders/include/tint.wgsl src/core/shaders/include/glyph.wgsl src/core/shaders/include/ui_rect.wgsl src/core/shaders/include/line_style.wgsl src/core/shaders/core/sprite.wgsl src/core/shaders/core/text.wgsl src/core/shaders/core/line.wgsl src/core/shaders/compose.rs
git commit -m "refactor: text, rect, sprite tint and line bodies as shared shader functions" -m "- the fragment math after sampling moves into include files the UI pipeline will reuse
- the world's entry points call them; no pixel changes"
```

---

### Task 2: The UI instance, pipeline and texture bindings

**Files:**
- Create: `src/core/render/ui_renderer/mod.rs` (module skeleton: `mod instance; mod gpu; mod runs;` and re-exports), `instance.rs`, `gpu.rs`, `runs.rs`, `tests.rs`, `gpu_tests.rs`; `src/core/shaders/core/ui.wgsl`
- Modify: `src/core/shaders/compose.rs` (`UI` module), `src/core/render/mod.rs` (declare `pub(crate) mod ui_renderer;`), `src/core/graphics/atlas.rs`, `glyph_atlas.rs`, `sprite_gpu.rs` (page textures and samplers), `src/core/render/batch/gpu.rs` (camera layout accessor)

**Interfaces:**
- Consumes: Task 1's WGSL functions; `SpriteInstance` (sprite_gpu.rs:36); `GlyphPage`; `TextureId`; the camera bind group layout (Batch2D's `camera_layout`, gpu.rs ~31).
- Produces:
  - `UiInstance` (`#[repr(C)]`, `Pod`, `Zeroable`, `Clone`, `Copy`, `Debug`, `PartialEq`), 144 bytes: `pub(crate) data: [[f32; 4]; 8], pub(crate) meta: [u32; 4]`
  - `UiKind` (`u32` consts or `#[repr(u32)] enum`): `RECT = 0, MASK = 1, COLOR = 2, MSDF = 3, SPRITE = 4, SPRITE_LINEAR = 5, LINE = 6`
  - `UiInstance::quad(kind: u32, instance: &SpriteInstance, clip: ClipRect, nearest: bool, page: u16) -> UiInstance`; `UiInstance::triangle(corners: [(Vec2, Vec2, [f32; 4]); 3], params: [f32; 4], clip: ClipRect) -> UiInstance` (each corner: position, uv, linear straight colour); `UiInstance::kind(&self) -> u32`; `UiInstance::page(&self) -> u16`; `UiInstance::clip(&self) -> ClipRect`
  - `ClipRect { x0: u16, y0: u16, x1: u16, y1: u16 }` with `ClipRect::of(scissor: Option<PixelRect>, image: (u32, u32)) -> ClipRect` (no scissor -> the whole image; values clamp to `u16::MAX`)
  - `TexNeed` (`Copy, PartialEq, Eq, Hash, Debug`): `None`, `Glyph(GlyphPage)`, `Sprite(TextureId)`; `Slot` (`Mask`, `Color`, `Msdf`, `Sprite`)
  - `Run { range: Range<u32>, textures: [Option<TexKey>; 4] }` where `TexKey` is a cheap identity a later step turns into a `wgpu::Texture` (defined in runs.rs as `TexNeed` resolved: glyph page or sprite page)
  - `plan_runs(needs: &[TexNeed], slot_of: impl Fn(TexNeed) -> Option<(Slot, TexKey)>) -> Vec<Run>` (pure)
  - `UiGpu::new(device: &wgpu::Device, camera_layout: &wgpu::BindGroupLayout) -> UiGpu` (pipeline for `UI_FORMAT` with `UI_GAMMA` true, the group-1 layout, the placeholder texture); `UiGpu::bind_group(&mut self, device, textures: [&wgpu::Texture; 4] (placeholders filled), samplers: (&wgpu::Sampler, &wgpu::Sampler), frame: u64) -> &wgpu::BindGroup` (cached by the four textures; entries unused for `KEEP_FRAMES = 8` frames are dropped by `UiGpu::end_frame(frame)`); `UiGpu::upload(device, queue, instances: &[UiInstance])` (one `write_buffer`, capacity doubling); `UiGpu::pipeline()`, `UiGpu::instances()`
  - `pub(crate)` accessors: `SpriteGpuRenderer::page_texture(page) -> Option<&wgpu::Texture>` for glyph and sprite pages (atlas, standalone, render target), and `SpriteGpuRenderer::samplers() -> (&wgpu::Sampler, &wgpu::Sampler)` (linear, nearest: the ones the existing page bind groups use)

- [ ] **Step 1: Write the failing tests**

`src/core/render/ui_renderer/tests.rs`:

```rust
use super::*;
use crate::core::graphics::SpriteInstance;
use crate::core::render::PixelRect;

#[test]
fn a_ui_instance_is_144_bytes_and_keeps_the_sprite_lanes() {
  assert_eq!(std::mem::size_of::<UiInstance>(), 144);
  let sprite = SpriteInstance::new()
    .with_transform(10.0, 20.0, 0.0, 30.0, 40.0)
    .with_uv(0.1, 0.2, 0.3, 0.4);
  let clip = ClipRect::of(None, (320, 240));
  let quad = UiInstance::quad(UiKind::MASK, &sprite, clip, true, 3);
  assert_eq!(quad.kind(), UiKind::MASK);
  assert_eq!(quad.page(), 3);
  assert_eq!(quad.clip(), ClipRect { x0: 0, y0: 0, x1: 320, y1: 240 });
  // The eight lanes are the sprite instance's, in its order
  assert_eq!(bytemuck::bytes_of(&quad.data), bytemuck::bytes_of(&sprite));
}

#[test]
fn a_clip_is_the_scissor_or_the_whole_image() {
  let scissor = PixelRect { x: 4, y: 5, width: 10, height: 20 };
  assert_eq!(
    ClipRect::of(Some(scissor), (320, 240)),
    ClipRect { x0: 4, y0: 5, x1: 14, y1: 25 }
  );
  assert_eq!(
    ClipRect::of(None, (100_000, 240)),
    ClipRect { x0: 0, y0: 0, x1: u16::MAX, y1: 240 }
  );
}

#[test]
fn runs_break_only_when_a_slot_needs_another_texture() {
  let page = |index| GlyphPage { format: GlyphFormat::Mask, index };
  let needs = [
    TexNeed::None,             // a rect
    TexNeed::Glyph(page(0)),
    TexNeed::None,
    TexNeed::Glyph(page(0)),
    TexNeed::Glyph(page(1)),   // another mask page: a new run
    TexNeed::None,
    TexNeed::Glyph(page(0)),   // back: another run
  ];
  let runs = plan_runs(&needs, |need| match need {
    TexNeed::Glyph(p) => Some((Slot::Mask, TexKey::Glyph(p))),
    _ => None,
  });
  let ranges: Vec<_> = runs.iter().map(|run| run.range.clone()).collect();
  assert_eq!(ranges, vec![0..4, 4..6, 6..7]);
}
```

(Adjust the `SpriteInstance`, `PixelRect`, `GlyphPage`, `GlyphFormat` paths to where they live; the assertions stay. `SpriteInstance` must be `Pod`; if it is not, compare its fields lane by lane.)

`src/core/render/ui_renderer/gpu_tests.rs`:

```rust
use super::*;
use crate::core::shaders::compose::UI;

#[test]
fn the_ui_module_validates_under_webgpu_rules() {
  let module = naga::front::wgsl::parse_str(UI).expect("the UI module parses");
  let mut validator = naga::valid::Validator::new(
    naga::valid::ValidationFlags::all(),
    naga::valid::Capabilities::default(),
  );
  validator.validate(&module).expect("the UI module validates");
}

#[test]
fn the_pipeline_builds_and_bind_groups_are_cached_by_texture() {
  let Ok((device, _queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  // ...build a camera layout like Batch2D's, UiGpu::new, two 1x1 textures;
  // the same four textures give the same bind group (pointer-equal), a
  // different one a new group; after KEEP_FRAMES + 1 frames unused,
  // end_frame drops the entry (the cache's len shrinks)
}
```

If naga is not a direct dependency, find how the repo validates its other WGSL modules in tests (grep `validate` under src/core/shaders and src/core/render) and use that path instead — do not add a dependency. Write the second test's body fully, following how other GPU unit tests in the crate get a device (`shared_gpu`).

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib render::ui_renderer`
Expected: compile errors (no `ui_renderer` module / `UiInstance` / `UI` module).

- [ ] **Step 3: The instance and runs**

`instance.rs`:

```rust
//! One UI draw item on the GPU: a quad (rect, glyph, sprite) with today's
//! sprite-instance lanes, or one triangle of a line with explicit corners

use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::core::graphics::SpriteInstance;
use crate::core::render::PixelRect;

/// What an instance draws, and which fragment body draws it
pub(crate) struct UiKind;

impl UiKind {
  pub(crate) const RECT: u32 = 0;
  pub(crate) const MASK: u32 = 1;
  pub(crate) const COLOR: u32 = 2;
  pub(crate) const MSDF: u32 = 3;
  pub(crate) const SPRITE: u32 = 4;
  /// A sprite of a render-target page (linear, premultiplied)
  pub(crate) const SPRITE_LINEAR: u32 = 5;
  pub(crate) const LINE: u32 = 6;
}

/// Sample with the nearest sampler
const NEAREST: u32 = 1;

/// A clip rect in UI image pixels: pixels with centres in [x0, x1) x [y0, y1)
/// are drawn (the scissor test)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClipRect {
  pub(crate) x0: u16,
  pub(crate) y0: u16,
  pub(crate) x1: u16,
  pub(crate) y1: u16,
}

impl ClipRect {
  /// `scissor` (as `scissor_of` makes it), or the whole image
  pub(crate) fn of(scissor: Option<PixelRect>, image: (u32, u32)) -> ClipRect {
    let narrow = |v: u32| u16::try_from(v).unwrap_or(u16::MAX);
    match scissor {
      Some(rect) => ClipRect {
        x0: narrow(rect.x),
        y0: narrow(rect.y),
        x1: narrow(rect.x.saturating_add(rect.width)),
        y1: narrow(rect.y.saturating_add(rect.height)),
      },
      None => ClipRect {
        x0: 0,
        y0: 0,
        x1: narrow(image.0),
        y1: narrow(image.1),
      },
    }
  }
}

/// A UI draw item: eight vec4 lanes (a quad's are a `SpriteInstance`'s, in
/// order; a line triangle's are its corners), then the clip, kind and flags
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct UiInstance {
  pub(crate) data: [[f32; 4]; 8],
  /// x: x0 | x1 << 16, y: y0 | y1 << 16, z: kind, w: flags | page << 16
  pub(crate) meta: [u32; 4],
}

impl UiInstance {
  pub(crate) fn quad(kind: u32, instance: &SpriteInstance, clip: ClipRect, nearest: bool, page: u16) -> UiInstance {
    UiInstance {
      data: bytemuck::cast(*instance),
      meta: Self::meta(clip, kind, if nearest { NEAREST } else { 0 }, page),
    }
  }

  /// One triangle of a line: each corner's position, uv and linear straight
  /// colour, as the line builder made them
  pub(crate) fn triangle(corners: [(Vec2, Vec2, [f32; 4]); 3], params: [f32; 4], clip: ClipRect) -> UiInstance {
    let [a, b, c] = corners;
    UiInstance {
      data: [
        [a.0.x, a.0.y, a.1.x, a.1.y],
        [b.0.x, b.0.y, b.1.x, b.1.y],
        [c.0.x, c.0.y, c.1.x, c.1.y],
        a.2,
        b.2,
        c.2,
        params,
        [0.0; 4],
      ],
      meta: Self::meta(clip, UiKind::LINE, 0, 0),
    }
  }

  fn meta(clip: ClipRect, kind: u32, flags: u32, page: u16) -> [u32; 4] {
    [
      u32::from(clip.x0) | u32::from(clip.x1) << 16,
      u32::from(clip.y0) | u32::from(clip.y1) << 16,
      kind,
      flags | u32::from(page) << 16,
    ]
  }

  pub(crate) fn kind(&self) -> u32 {
    self.meta[2]
  }

  pub(crate) fn page(&self) -> u16 {
    (self.meta[3] >> 16) as u16
  }

  pub(crate) fn clip(&self) -> ClipRect {
    ClipRect {
      x0: self.meta[0] as u16,
      x1: (self.meta[0] >> 16) as u16,
      y0: self.meta[1] as u16,
      y1: (self.meta[1] >> 16) as u16,
    }
  }
}
```

(If `SpriteInstance` is not `Pod`/castable to `[[f32; 4]; 8]`, build `data` from its fields in their lane order: transform rows, uv_rect, tint, effect_params, effect_extra, uv_bounds.)

`runs.rs`:

```rust
//! Which texture each UI instance samples, and the runs of instances that
//! can share one bind group

use std::ops::Range;

use crate::core::graphics::{GlyphPage, TextureId};

/// What an instance samples
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TexNeed {
  None,
  Glyph(GlyphPage),
  Sprite(TextureId),
}

/// The bind group's texture slots
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Slot {
  Mask,
  Color,
  Msdf,
  Sprite,
}

impl Slot {
  pub(crate) const ALL: [Slot; 4] = [Slot::Mask, Slot::Color, Slot::Msdf, Slot::Sprite];
  fn index(self) -> usize {
    self as usize
  }
}

/// A texture in a slot, as the frame resolved it (a glyph page, or the page a
/// sprite texture sits on this frame)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TexKey {
  Glyph(GlyphPage),
  Sprite(crate::core::graphics::SlotPage),
}

/// Instances that draw with one bind group: the textures in its slots
/// (`None`: the placeholder)
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Run {
  pub(crate) range: Range<u32>,
  pub(crate) textures: [Option<TexKey>; 4],
}

/// Split instances into runs: a run ends when an instance needs a texture in a
/// slot that already holds another one (slots it leaves alone carry over)
pub(crate) fn plan_runs(needs: &[TexNeed], slot_of: impl Fn(TexNeed) -> Option<(Slot, TexKey)>) -> Vec<Run> {
  let mut runs = Vec::new();
  let mut start = 0u32;
  let mut textures: [Option<TexKey>; 4] = [None; 4];
  for (index, need) in needs.iter().enumerate() {
    let Some((slot, key)) = slot_of(*need) else {
      continue;
    };
    let held = &mut textures[slot.index()];
    match held {
      Some(current) if *current != key => {
        let index = index as u32;
        runs.push(Run { range: start..index, textures });
        start = index;
        textures[slot.index()] = Some(key);
      }
      Some(_) => {}
      None => *held = Some(key),
    }
  }
  let end = needs.len() as u32;
  if end > start {
    runs.push(Run { range: start..end, textures });
  }
  runs
}
```

(Use the real path of the sprite page type — `SlotPage` per the excerpts, section 7 — and make it `Copy, Eq, Hash` if it is not.)

- [ ] **Step 4: The shader**

`src/core/shaders/core/ui.wgsl`:

```wgsl
// The UI's one pipeline: every UI instance (rects, glyphs, sprites, line
// triangles) through one vertex and one fragment entry. The fragment bodies
// are the world shaders' (include files); the clip is tested last, so it
// passes exactly the pixels a scissor would.

@group(1) @binding(0) var mask_page: texture_2d<f32>;
@group(1) @binding(1) var color_page: texture_2d<f32>;
@group(1) @binding(2) var msdf_page: texture_2d<f32>;
@group(1) @binding(3) var sprite_page: texture_2d<f32>;
@group(1) @binding(4) var linear_sampler: sampler;
@group(1) @binding(5) var nearest_sampler: sampler;

const KIND_RECT: u32 = 0u;
const KIND_MASK: u32 = 1u;
const KIND_COLOR: u32 = 2u;
const KIND_MSDF: u32 = 3u;
const KIND_SPRITE: u32 = 4u;
const KIND_SPRITE_LINEAR: u32 = 5u;
const KIND_LINE: u32 = 6u;
const FLAG_NEAREST: u32 = 1u;

struct QuadVertex {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
}

struct UiInstanceIn {
    @location(2) a0: vec4<f32>,
    @location(3) a1: vec4<f32>,
    @location(4) a2: vec4<f32>,
    @location(5) a3: vec4<f32>,
    @location(6) a4: vec4<f32>,
    @location(7) a5: vec4<f32>,
    @location(8) a6: vec4<f32>,
    @location(9) a7: vec4<f32>,
    @location(10) meta: vec4<u32>,
}

struct UiOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // A line's colour, interpolated across its triangle (linear, straight)
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) tint: vec4<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) @interpolate(flat) extra: vec4<f32>,
    @location(5) @interpolate(flat) bounds: vec4<f32>,
    @location(6) @interpolate(flat) emission: f32,
    @location(7) @interpolate(flat) clip: vec4<u32>,
    @location(8) @interpolate(flat) kind: u32,
    @location(9) @interpolate(flat) flags: u32,
}

@vertex
fn vs_ui(vertex: QuadVertex, instance: UiInstanceIn, @builtin(vertex_index) index: u32) -> UiOut {
    var out: UiOut;
    let kind = instance.meta.z;
    out.kind = kind;
    out.flags = instance.meta.w & 0xFFFFu;
    out.clip = vec4<u32>(
        instance.meta.x & 0xFFFFu,
        instance.meta.y & 0xFFFFu,
        instance.meta.x >> 16u,
        instance.meta.y >> 16u,
    );
    if kind == KIND_LINE {
        // Corners 0, 1, 2 make the triangle; the quad's other three
        // vertices repeat corner 2, so its second triangle has no area
        let corner = min(index % 6u, 2u);
        var at = instance.a2;
        var color = instance.a5;
        if corner == 0u {
            at = instance.a0;
            color = instance.a3;
        } else if corner == 1u {
            at = instance.a1;
            color = instance.a4;
        }
        // As line.wgsl's vs_main for a solid UI line: no displacement, no snap
        out.position = globals.view_proj * vec4<f32>(at.xy, 0.0, 1.0);
        out.uv = at.zw;
        out.color = color;
        out.params = instance.a6;
        return out;
    }
    // Quads: sprite.wgsl's vs_main, the same arithmetic in the same order
    // ... copy vs_main's body (sprite.wgsl:93-131) here, with `instance.transform0`
    // as `instance.a0`, transform1 a1, transform2 a2, uv_rect a3, tint a4,
    // effect_params a5, effect_extra a6, uv_bounds a7, writing out.position,
    // out.uv, out.tint, out.params, out.extra, out.bounds, out.emission ...
    return out;
}

@fragment
fn fs_ui(in: UiOut) -> @location(0) vec4<f32> {
    // Every derivative first, where control flow is uniform
    let uv_ddx = dpdx(in.uv);
    let uv_ddy = dpdy(in.uv);
    let uv_width = fwidth(in.uv);
    let rect_size = max(in.params.zw, vec2<f32>(1e-4));
    let rect_p = (in.uv - vec2<f32>(0.5)) * rect_size;
    let rect_width = fwidth(rect_p);
    let line_aa = fwidth(abs(in.uv.y - 0.5) * 2.0);
    let nearest = (in.flags & FLAG_NEAREST) != 0u;

    var color = vec4<f32>(0.0);
    switch in.kind {
        case KIND_RECT: {
            color = ui_rect(rect_p, rect_size, rect_width, in.tint, in.params, in.extra);
        }
        case KIND_MASK: {
            var coverage: f32;
            if nearest {
                coverage = textureSampleGrad(mask_page, nearest_sampler, in.uv, uv_ddx, uv_ddy).r;
            } else {
                coverage = textureSampleGrad(mask_page, linear_sampler, in.uv, uv_ddx, uv_ddy).r;
            }
            color = glyph_mask(coverage, in.tint, in.emission);
        }
        // KIND_COLOR: the same with color_page and glyph_color
        // KIND_MSDF: texel from msdf_page, texels = textureDimensions(msdf_page, 0),
        //   glyph_msdf(texel.rgb, texels, uv_width, in.tint, in.params, in.extra, in.emission)
        // KIND_SPRITE: sample_masked(sprite_page, sampler, in.uv, in.bounds, uv_ddx, uv_ddy),
        //   then finish(texel, in.tint, in.emission) (as fs_base)
        // KIND_SPRITE_LINEAR: the same sample, then finish_linear (as fs_base_linear)
        case KIND_LINE: {
            color = styled(in.color, in.uv, in.params, line_aa);
        }
        default: {}
    }
    let px = in.position.xy;
    let inside = px.x >= f32(in.clip.x) && px.x < f32(in.clip.z)
        && px.y >= f32(in.clip.y) && px.y < f32(in.clip.w);
    if !inside {
        return vec4<f32>(0.0);
    }
    return ui_out(color);
}
```

Write the elided cases in full (each mirrors its world entry point: same expressions, gradient sampling with `uv_ddx`/`uv_ddy`, the sampler chosen by `nearest`). The quad vertex path is sprite.wgsl's `vs_main` body copied with the lanes renamed — keep every expression and its order, including the pixel-art snap through `globals.snap`, `to_pixel` and `from_pixel` (move `to_pixel`/`from_pixel` into an include shared by sprite.wgsl and ui.wgsl rather than duplicating them).

`compose.rs`: `pub(crate) const UI: &str = concat!(camera, color, sample, tint, glyph, ui_rect, line_style, <the to_pixel/from_pixel include>, "core/ui.wgsl")` with `"\n"` between.

- [ ] **Step 5: The GPU resources**

`gpu.rs`: `UiGpu` holds the shader module, the group-1 layout (bindings 0-3: `texture_2d<f32>` filterable, fragment; 4-5: filtering samplers), the pipeline layout `[camera_layout, group1]`, the pipeline (vertex buffers: the unit quad at slot 0 exactly as Batch2D's (pipelines.rs ~98-123), the instance buffer at slot 1 with eight `Float32x4` at locations 2-9 and one `Uint32x4` at location 10, step mode Instance; `PREMULTIPLIED_ALPHA_BLENDING`; target `UI_FORMAT`; the `UI_GAMMA` override set true as `batch_pipeline` does), a 1x1 `Rgba8Unorm` placeholder texture, the instance buffer (start at 1,024 instances, double on growth), and `BindCache`:

```rust
/// The UI's texture bind groups, by the four textures they hold; an entry
/// not used for `KEEP_FRAMES` frames is dropped (it holds its textures alive)
struct BindCache {
  groups: FastMap<[wgpu::Texture; 4], (wgpu::BindGroup, u64)>,
}
```

(`wgpu::Texture` implements `Eq` and `Hash` by its id in wgpu 30 — `src/api/texture.rs` `impl_eq_ord_hash_proxy!` — so a recreated texture is a new key.) Views are made with `create_view(&Default::default())` when a group is made.

`sprite_gpu.rs` / `atlas.rs` / `glyph_atlas.rs`: `pub(crate) fn page_texture(&self, key: TexKey) -> Option<&wgpu::Texture>` (glyph pages from `GlyphAtlas`, sprite pages — atlas, standalone, render target — from `TextureAtlas`), and `pub(crate) fn samplers(&self) -> (&wgpu::Sampler, &wgpu::Sampler)` returning the linear and nearest samplers today's page bind groups use (atlas.rs ~316-328). Before relying on that, confirm every page kind's bind group uses those same two samplers (glyph pages, atlas pages, standalone images, render targets); if one uses another sampler, add it as a further binding and document why.

`batch/gpu.rs`: `pub(crate) fn camera_layout(&self) -> &wgpu::BindGroupLayout` and `pub(crate) fn camera_group(&self, camera: Entity) -> Option<&wgpu::BindGroup>`.

Nothing calls `UiGpu` yet outside tests: mark the module `#![cfg_attr(not(test), allow(dead_code))] // wired in Task 4` and remove that in Task 4.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p ryo2d --lib render::ui_renderer`
Expected: PASS (5 tests).

- [ ] **Step 7: Run the gate and commit**

```bash
git add src/core/render/ui_renderer/mod.rs src/core/render/ui_renderer/instance.rs src/core/render/ui_renderer/gpu.rs src/core/render/ui_renderer/runs.rs src/core/render/ui_renderer/tests.rs src/core/render/ui_renderer/gpu_tests.rs src/core/shaders/core/ui.wgsl src/core/shaders/compose.rs src/core/render/mod.rs src/core/graphics/atlas.rs src/core/graphics/glyph_atlas.rs src/core/graphics/sprite_gpu.rs src/core/render/batch/gpu.rs
git commit -m "feat: the UI instance, its one pipeline and texture bindings" -m "- one 144-byte instance for rects, glyphs, sprites and line triangles, with its clip
- one pipeline runs today's fragment bodies by kind and tests the clip as a scissor would
- runs break only when a slot needs another texture; bind groups cached by texture"
```

(Plus any shared include added in Step 4.)

---

### Task 3: Building the UI's instances

The CPU side: commands to instances, in painter's order, culled, with texts through the 4g cache.

**Files:**
- Create: `src/core/render/ui_renderer/build.rs`
- Modify: `src/core/render/ui_retain/texts.rs` (generic cache), `src/core/render/ui.rs` (rect and sprite math shared), `src/core/render/ui_renderer/mod.rs`, `tests.rs`, `gpu_tests.rs`

**Interfaces:**
- Consumes: Task 2's `UiInstance`, `UiKind`, `ClipRect`, `TexNeed`; `emit_glyphs`/`emit_icons` (render/text.rs), `push_polyline`/`Triangles` (render/batch), `scissor_of`, `ui_z`, `snap_rect`, `UiRenderEnv`, `UiTextCache`.
- Produces:
  - `pub(crate) struct UiBuild { pub(crate) instances: Vec<UiInstance>, pub(crate) needs: Vec<TexNeed>, pub(crate) sprites: Vec<SpriteFix> }` where `SpriteFix { index: u32, texture: TextureId, filter: Filter }` records a sprite instance whose uv and kind are fixed after residency (Task 4)
  - `pub(crate) fn build_ui(build: &mut UiBuild, commands: &UiCommands, env: &UiRenderEnv, sources: Option<&TextSources<'_>>, targets: &mut TextTargets<'_>, real_time: f32, texts: &mut UiTextCache<UiInstance>)` (clears and fills `build`; `targets.list` and a scratch `Triangles` are used only as scratch for built texts and lines)
  - `UiTextCache<G: Clone>`: the existing cache, generic over its stored item (`DrawGlyph` for the old path, `UiInstance` for the new); the old path's call sites keep working unchanged
  - `pub(crate) fn rect_instance(rect: Rect, fill: Color, radius: f32, border: Option<(f32, Color)>, space: &UiSpace, pixel_art: bool) -> Option<(SpriteInstance, Vec2, Vec2)>` and `pub(crate) fn sprite_instance(rect: Rect, uv: [f32; 4], tint: Color) -> Option<SpriteInstance>` in render/ui.rs, used by `emit_ui` (old) and `build_ui` (new) — the math of emit_ui's Rect and Sprite arms, moved, returning `None` where the arm `continue`s

- [ ] **Step 1: Write the failing tests**

In `tests.rs` (CPU, no GPU): build commands with `UiCommands::push` and check:

```rust
#[test]
fn commands_outside_their_clip_or_the_image_build_nothing() {
  // a rect wholly below its clip, a rect wholly right of the image, an empty
  // clip, and one visible rect: one instance, a RECT
}

#[test]
fn commands_out_of_order_are_built_in_painters_order() {
  // push orders 5, 2, 9 (and a layer-1 command with order 1 pushed first):
  // instances come out by (layer, order)
}

#[test]
fn a_rect_instance_is_the_old_paths() {
  // for the same command, UiInstance::quad(RECT, &rect_instance(..).0, ..)'s
  // lanes equal what emit_ui pushes into the draw list (rect_instance is the
  // shared function, so compare against a DrawList built by emit_ui with a
  // headless-free TextTargets if feasible; otherwise against the hand-made
  // SpriteInstance the old arm made: transform, uv_rect, uv_bounds,
  // effect_params, effect_extra)
}

#[test]
fn a_line_becomes_its_builders_triangles() {
  // a 3-point open line: the triangles push_polyline puts in a scratch
  // Triangles, as UiInstance::triangle corners in index order — the same
  // positions, uvs and colours, and as many instances as triangles
}
```

Write each body in full. For tests that need text, put them in `gpu_tests.rs` with `shared_gpu` / a `HeadlessEngine`-free setup if the crate's text tests have one; otherwise test texts through Task 4's engine tests and say so in the report.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib render::ui_renderer`
Expected: compile errors (`build_ui`, `UiBuild` missing).

- [ ] **Step 3: Share the rect and sprite math**

Move the bodies of `emit_ui`'s `Rect` and `Sprite` arms (render/ui.rs, after `scissor_of`) into `rect_instance` and `sprite_instance`, unchanged; `emit_ui` calls them and pushes what they return (the old path's output must stay byte-identical — the e2e pixel tests guard it).

- [ ] **Step 4: Make the text cache generic**

`UiTextCache<G>`: `Built.glyphs: Vec<G>`; `emit` gains how to capture and how to push, so both paths share key, validation, pinning, `keep`, `end` and `clear`:

```rust
pub(crate) trait TextSink<G> {
  /// Where the next built glyph will land (to capture from)
  fn mark(&self) -> usize;
  /// The glyphs pushed since `mark`, if all are cacheable (no sprite cells)
  fn captured(&self, from: usize) -> Option<&[G]>;
  /// Push cached glyphs for this text at this frame's place
  fn push_cached(&mut self, glyphs: &[G], place: &TextPlacement);
  /// The glyph page each cached glyph samples
  fn page_of(glyph: &G) -> GlyphPage;
}
```

The old path implements it over `DrawList` (as today: `glyph_count`/`glyphs_from`/`push_glyphs`); the new path over `UiBuild` (the scratch DrawList's glyphs are converted to `UiInstance`s when built: kind from the glyph's page format and shader — MASK, COLOR or MSDF —, `nearest` from its `sampling`, `page` its page index, clip from its scissor; reused instances are copied, their clip already set). Adapt the names to what fits the code; what matters is one cache with one copy of its logic. Keep the old path's behaviour and the 4g tests passing.

- [ ] **Step 5: `build_ui`**

1. `build.clear()`; take the commands in painter's order: walk `commands.as_slice()` checking `(layer, order)` never decreases; on a decrease, sort a reused index `Vec<u32>` by `(layer, order)` (stable) and walk that instead.
2. Per command: `scissor_of(clip, space)` → `None` skips it (as today); `clip = ClipRect::of(scissor, space.image)`.
3. **Rects:** `rect_instance(..)` → `None` skips; cull when its box (`min`/`max` from `rect_instance`) does not intersect the clip rect or the image; else push `UiInstance::quad(UiKind::RECT, &instance, clip, false, 0)` and `TexNeed::None`.
4. **Sprites:** `sprite_instance(..)` → `None` skips; cull by its box likewise; push `UiInstance::quad(UiKind::SPRITE, ..)` (kind and uv fixed in Task 4), `TexNeed::Sprite(texture_of(sprite))`, and a `SpriteFix`.
5. **Lines:** build the points as emit_ui's Line arm does, `push_polyline` into a scratch `DrawList` and scratch `Triangles` (cleared per line), then one `UiInstance::triangle` per triangle of the pushed `DrawTriangles` (its index range, in order; each corner's `Vertex2D` position, uv and colour; `params` from the vertex's params), `TexNeed::None`; cull a line by its `DrawTriangles` box.
6. **Texts:** through `texts.emit(..)` with the `UiBuild` sink: a reused text copies its instances (each kept only if its box — from the instance's transform, as `reach` computes — meets the clip and the image; or the whole text skipped when the union, stored in the cache entry, misses); a built text runs `emit_glyphs` into the scratch DrawList, converted to instances; icons go through `emit_icons` into the scratch DrawList and become SPRITE instances with `SpriteFix`es (as UI sprites).

Culling must remove only what draws no pixel today: an item outside its scissor or outside the UI camera's view (`sees`, draw_list.rs ~405-465). Use the same boxes the old path tests (`min`/`max`).

- [ ] **Step 6: Run the tests**

Run: `cargo test -p ryo2d --lib render::ui_renderer` and `cargo test --test e2e ui_retain`
Expected: PASS (the old path is unchanged: the 4g tests still pass).

- [ ] **Step 7: Run the gate and commit**

```bash
git add src/core/render/ui_renderer/build.rs src/core/render/ui_renderer/mod.rs src/core/render/ui_renderer/tests.rs src/core/render/ui_renderer/gpu_tests.rs src/core/render/ui_retain/texts.rs src/core/render/ui.rs
git commit -m "feat: the UI's commands become instances in painter's order" -m "- culled before they are built; reused texts are one copy from the text cache
- lines become their builder's triangles; rect and sprite math shared with the old path"
```

---

### Task 4: The UI renderer in the frame, behind a hidden switch

**Files:**
- Modify: `src/core/render/ui_renderer/mod.rs` (`UiRenderer`, `UiFrame`), `src/core/engine/extract.rs`, `src/core/engine/mod.rs`, `src/core/config/debug.rs`, `src/core/render/frame.rs`, `src/core/render/passes/output.rs`, `src/core/graphics/renderer.rs`, `src/core/graphics/sprite_gpu.rs`, `src/core/render/batch/gpu.rs`, `src/core/testing/retain.rs`, `src/core/testing/headless.rs`, `src/core/testing/mod.rs`
- Create: `tests/e2e/ui_renderer.rs`; Modify: `tests/e2e.rs`

**Interfaces:**
- Consumes: Tasks 2-3.
- Produces:
  - `DebugConfig::ui_dedicated: bool` (default false until Task 7) and `#[doc(hidden)] EngineBuilder::ui_dedicated(self, on: bool) -> Self`
  - `UiRenderer { gpu: UiGpu, build: UiBuild, texts: UiTextCache<UiInstance>, runs: Vec<Run>, groups: Vec<wgpu::BindGroup> (or indices into the cache), frame: u64 }` living beside `Batch2D` (created in `init_gpu`, taken out and put back in `draw_frame` as the batch is), with `build(..)` (Task 3) and `finish(device, queue, sprites: &SpriteGpuRenderer, presentation) -> Option<UiFrame>` (fixes sprites, plans runs, makes bind groups, uploads; `None` when there are no instances)
  - `pub(crate) struct UiFrame<'a> { pipeline, instances buffer slice, runs: &'a [(Range<u32>, &'a wgpu::BindGroup)], camera: &'a wgpu::BindGroup }` (or the equivalent the pass needs)
  - `FrameScene::ui_frame: Option<UiFrame<'_>>`; `FrameInfo::ui` true when either path has UI to draw
  - `Batch2D::prepare(.., extra_textures: &[TextureId])`: the UI's sprite textures made resident with the world's, in the one `prepare_textures` call (so the frame counter still advances once)
  - `HeadlessEngine::ui_draw_count(&self) -> u32` (`#[doc(hidden)]`): the UI pass's draws last frame
  - `PixelPair::between(make: impl Fn() -> EngineBuilder, a: impl Fn(EngineBuilder) -> EngineBuilder, b: impl Fn(EngineBuilder) -> EngineBuilder) -> Option<PixelPair>` (pixel comparison only: no retention assertions); `PixelPair::new` keeps its 4g behaviour

- [ ] **Step 1: Write the failing tests**

`tests/e2e/ui_renderer.rs` (register in `tests/e2e.rs`): the old against the new path. Reuse the 4g scenes by copying the minimal ones needed (a static HUD of panels and labels; the settings node screen; a counting HUD):

```rust
//! The dedicated UI renderer against the old UI path: two engines driven alike
//! draw the same pixels every frame

use ryo2d::prelude::*;
use ryo2d::testing::PixelPair;

fn old_and_new(make: impl Fn() -> EngineBuilder) -> Option<PixelPair> {
  PixelPair::between(make, |b| b.ui_dedicated(false), |b| b.ui_dedicated(true))
}

// Hud { counting, frame } and SettingsScreen as in tests/e2e/ui_retain.rs

#[test]
fn a_hud_draws_the_same_on_the_new_renderer() {
  let Some(mut pair) = old_and_new(|| EngineBuilder::new().resolution(320, 240).start_scene(Hud { counting: true, frame: 0 })) else {
    return;
  };
  pair.frames(30, "a counting HUD");
}

#[test]
fn the_settings_screen_draws_the_same_in_one_or_two_draws() {
  let Some(mut pair) = old_and_new(settings) else {
    return;
  };
  pair.frames(20, "the settings screen");
  // `b` is the new renderer
  assert!(pair.b().ui_draw_count() <= 2, "{} draws", pair.b().ui_draw_count());
}

#[test]
fn an_empty_ui_draws_nothing_and_the_next_draws_in_full() {
  // a Toggle scene (shared flag) hidden for 5 frames, shown 5, hidden 5,
  // shown 5 — compared old against new every frame; the new path's
  // ui_draw_count is 0 while hidden
}

#[test]
fn a_ui_bigger_than_the_buffer_grows_it() {
  // a closure scene drawing 3,000 short labels (more than the 1,024-instance
  // start), every frame a different one changing: compared old against new
}
```

Write the elided bodies in full. `PixelPair::between` names its engines `a()` and `b()` (or exposes `on`/`off` as `a`/`b`): choose names that read well and document them.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test e2e ui_renderer`
Expected: compile errors (`ui_dedicated`, `between`, `ui_draw_count` missing).

- [ ] **Step 3: The switch and the renderer's frame**

`DebugConfig::ui_dedicated` (default `false`; doc: "Draw the UI through the dedicated UI renderer instead of the world's batch (temporary, for comparing the two)"), hidden from the doc table; `EngineBuilder::ui_dedicated` `#[doc(hidden)]`.

In `draw_frame` (extract.rs), where 4g decides `ui_kept`:
- not kept and `ui_dedicated`: `ui_renderer.texts.begin(env, retain)`; `build_ui(&mut ui_renderer.build, ..)`; `ui_renderer.texts.end()`; stats from the new cache; the UI camera's globals are still pushed into `camera_globals` (its uniform and bind group are reused); the UI camera is NOT added to `batch_cameras`; `batch.prepare(.., &ui_renderer.build.sprite_textures())`; then `ui_frame = ui_renderer.finish(..)`.
- not kept and not `ui_dedicated`: today's path, unchanged.
- kept: neither.
`FrameScene` gains `ui_frame`; `FrameInfo::ui = scene.ui_camera.is_some() || scene.ui_frame.is_some()`; the `ui` pass draws the new frame when present (clear the UI image as today, set the pipeline, the camera group at 0, per run the run's group at 1 and `draw(0..6, range)`), else the old camera's draws; `ctx.ui_drawn` as today. `render_blank_frame` passes `ui_frame: None`. Draw counts and `DebugStats::draw_calls` include the new path's draws exactly once.

`finish`: for each `SpriteFix` resolve the texture's slot (`textures.slot(id)` after residency) and apply the old path's remap (`place_in_slot`: uv into the page, `uv_bounds`), the page's kind (render target → `SPRITE_LINEAR`, else `SPRITE`) and the filter's sampling (`filter.resolve(presentation)` → `nearest`); resolve each need to a `TexKey`; `plan_runs`; per run take the four textures (`page_texture`, placeholder where `None`) and `bind_group(..)`; `upload` the instances; `gpu.end_frame(frame)`.

Remove Task 2's temporary `allow(dead_code)`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test e2e ui_renderer` and `cargo test --test e2e ui_retain`
Expected: PASS. A pixel difference saves both PNGs (PixelPair): debug it (superpowers:systematic-debugging) — the old path is the reference.

- [ ] **Step 5: Run the gate and commit**

```bash
git add src/core/render/ui_renderer/mod.rs src/core/engine/extract.rs src/core/engine/mod.rs src/core/config/debug.rs src/core/render/frame.rs src/core/render/passes/output.rs src/core/graphics/renderer.rs src/core/graphics/sprite_gpu.rs src/core/render/batch/gpu.rs src/core/testing/retain.rs src/core/testing/headless.rs src/core/testing/mod.rs tests/e2e/ui_renderer.rs tests/e2e.rs
git commit -m "feat: the UI draws through its own renderer behind a hidden switch" -m "- built, resolved and uploaded in one write; one pipeline, a draw per texture run
- PixelPair::between compares the old and new paths pixel for pixel"
```

---

### Task 5: The hard cases, old against new

**Files:**
- Modify: `tests/e2e/ui_renderer.rs`; `src/core/render/ui_renderer/gpu_tests.rs` (cases needing crate access)

**Interfaces:**
- Consumes: Task 4's `PixelPair::between`, `ui_dedicated`, `ui_draw_count`.

Each test compares the old and new paths every frame. As in 4g, prove each can fail: temporarily break what it guards (e.g. drop the clip test in `fs_ui`, swap the nearest/linear samplers, skip `place_in_slot` for sprites, ignore the triangle's third corner, use the wrong page for a second glyph page), watch it fail, restore; list each in the report. A real failure is a bug: fix it in a `fix:` commit naming the test.

- [ ] **Step 1: Write the tests** (all in `tests/e2e/ui_renderer.rs` unless they need crate access)

- `scroll_clipping_and_nested_clips_match`: a closure scroll area inside a clipped panel inside another, scrolled (wheel and trackpad) through its range.
- `lines_match`: a closure UI with `ui.draw_line` (open and closed, thin and thick, slanted), a checked checkbox and an open dropdown (chevrons).
- `colour_glyphs_and_msdf_match`: a colour-emoji text (if a colour font is registered in tests; otherwise skip that half and say so) and the UI font set to `FontRender::msdf()` with an outline.
- `bitmap_font_text_matches`: as 4g's bitmap label test.
- `text_effects_match`: `[wave]`, `[shake]`, `[rainbow]` and a typewriter reveal.
- `sprites_and_a_standalone_image_match`: atlas sprites (nearest and linear filter) and a sprite larger than 256 px (a standalone texture).
- `a_render_target_in_the_ui_matches`: a `RenderTarget` drawn by a camera, shown as a UI sprite (the linear sprite body).
- `interleaved_textures_break_runs_and_draw_right`: glyphs from two glyph pages (texts at many large sizes so a second mask page is needed) interleaved in painter's order with sprites from two atlas pages; assert `ui_draw_count` equals the expected run count, computed from what the scene draws.
- `pixel_art_ui_matches`: `EngineBuilder::pixel_art(320, 180)` with rects, text, a line and a sprite.
- `glyph_pressure_matches`: 4g's UI churn and world-text churn scenes.
- `resize_and_theme_change_match`: the settings screen, resized and given `Theme::light()` mid-run.
- `named_icons_and_a_sprite_updated_in_place_match` (crate access for `SpriteHandle::update`: in gpu_tests.rs): 4g's icon swap and in-place sprite update.
- `retention_off_matches`: the settings screen with `ui_retain(false)` on both.

- [ ] **Step 2: Run them**

Run: `cargo test --test e2e ui_renderer` and `cargo test -p ryo2d --lib render::ui_renderer`
Expected: PASS after any `fix:` commits; every test shown to fail under its temporary break.

- [ ] **Step 3: Run the gate and commit**

```bash
git add tests/e2e/ui_renderer.rs src/core/render/ui_renderer/gpu_tests.rs
git commit -m "test: the UI renderer matches the old path in the hard cases"
```

---

### Task 6: The demo and the game, old against new

**Files:**
- Create: `examples/demos/ui/tests/renderer.rs`, `examples/games/breakout/tests/renderer.rs`

- [ ] **Step 1: Write the tests**

Copy the structure of `examples/demos/ui/tests/retain.rs` and `examples/games/breakout/tests/retain.rs` (4g) — same scripts, same page-6 modal step, same `game_folder()` — but build each pair with `PixelPair::between(make, |b| b.ui_dedicated(false), |b| b.ui_dedicated(true))`, and assert per page that the new path's `ui_draw_count()` stays small (at most the number of distinct textures the page shows plus one; record each page's count in the report).

- [ ] **Step 2: Run them**

Run: `cargo test -p ui --test renderer` and `cargo test -p breakout --test renderer`
Expected: PASS (fix real differences in `fix:` commits naming the test).

- [ ] **Step 3: Run the gate and commit**

```bash
git add examples/demos/ui/tests/renderer.rs examples/games/breakout/tests/renderer.rs
git commit -m "test: the ui demo and breakout match on the UI renderer"
```

---

### Task 7: The UI renderer is the only path

**Files:**
- Modify: `src/core/engine/extract.rs`, `src/core/render/ui.rs` (`emit_ui` removed), `src/core/render/ui_retain/texts.rs` (the `DrawGlyph` sink removed; the cache may stay generic or be made concrete), `src/core/render/batch/gpu.rs` (no UI camera handling, `extra_textures` stays), `src/core/render/frame.rs`, `passes/output.rs`, `src/core/graphics/renderer.rs` (no `ui_camera` path), `src/core/config/debug.rs`, `src/core/engine/mod.rs` (switch removed), `src/core/render/draw_list.rs` (`push_glyphs`/`glyphs_from` removed if unused), `src/core/testing/retain.rs` (`between` kept only if still used)
- Delete: `tests/e2e/ui_renderer.rs`'s old-against-new comparisons (keep its non-comparison assertions, e.g. draw counts and buffer growth, as plain tests on the only path), `examples/demos/ui/tests/renderer.rs`, `examples/games/breakout/tests/renderer.rs`

- [ ] **Step 1: Remove the old UI path and the switch**

Delete the old UI emit path and every branch on `ui_dedicated`: the UI's route through the draw list and `Batch2D` (UI camera in `batch_cameras`, its items in `prepare`, UI lines in the shared `Triangles`), `FrameScene::ui_camera` if nothing else uses it (keep the `FrameInfo::ui`/`ui_drawn` signal driven by `ui_frame`), the switch in `DebugConfig` and `EngineBuilder`, the old cache sink, and code only they used (clippy's dead-code warnings list it). `ui:retain off` must still build and draw everything.

- [ ] **Step 2: Turn the comparison tests into plain tests**

Keep, as single-engine tests on the only path: the settings screen's draw count, the empty-then-full UI, the buffer growth, the interleaved-textures run count. The pixel proof now rests on: Tasks 4-6's comparisons (already passed), 4g's retention oracle (`PixelPair::new`, retention on against off, in tests/e2e/ui_retain.rs, examples/demos/ui/tests/retain.rs and examples/games/breakout/tests/retain.rs — all must pass), and the tour.

- [ ] **Step 3: The tour against the baseline**

```bash
./dev.sh demos:tour
(cd /private/tmp/claude-501/4h-tour && diff -rq base-1 <worktree>/target/demo-tour | grep '\.png' | sed -e 's|base-1/||' -e 's|<worktree>/target/demo-tour/||' | grep -vxF -f nondeterministic.txt) || echo "no differences beyond the nondeterministic pages"
```

Expected: "no differences beyond the nondeterministic pages". Anything else is a bug (find it, fix it with a test).

- [ ] **Step 4: Run the gate and commit**

```bash
git add <every file changed or deleted, by name>
git commit -m "refactor: the UI renderer is the UI's only path" -m "- the UI no longer goes through the draw list and the world's batch
- the temporary switch and the old-against-new comparisons are gone"
```

---

### Task 8: Benches, the guard and the docs

**Files:**
- Modify: `benches/ui_render.rs`, `src/core/render/AGENTS.md`, `.claude/skills/ryo-ui/SKILL.md` + `src/cli/skills/ryo-ui.md`, `docs/specs/2026-10-20-ui-renderer-design.md`, `docs/backlog/ui-retained-nodes.md`

- [ ] **Step 1: The guard**

In `benches/ui_render.rs`'s guard, the one-label line becomes an assertion against 40 µs:

```rust
  assert!(
    label < Duration::from_micros(40),
    "one changing label costs {label:?} to render (4h target 40 µs)"
  );
```

Keep the idle (< 5 µs) and retained-no-slower assertions. Run `cargo bench --bench ui_render` and `cargo bench --bench text`; if the guard fails, do not change the threshold: profile the frame (xctrace, as in `/private/tmp/claude-501/4h-research/ui-batch-path.md` §2) and report the numbers and top entries.

- [ ] **Step 2: Docs**

- `src/core/render/AGENTS.md`: a "UI renderer" section (the instance and its kinds, the one pipeline and why the bodies are shared functions, clipping in the shader and why it equals the scissor, texture slots and runs, the frame order — build, residency, finish, upload, pass —, culling's rule, the text cache's instances), replacing the notes that describe the UI going through the batch; update the pass table's `ui` row.
- `ryo-ui` skill (both copies, bump): the performance paragraph — the UI draws in one draw per texture run (usually one); a UI mixing many glyph pages or sprite pages draws more runs.
- The spec: amend "The instance and the shader" — lines are one instance per builder triangle (exactness: the builder's triangles, not quads), there are seven fragment bodies (render-target sprites use the linear sprite body), and the bind group holds the samplers today's pages use.
- `docs/backlog/ui-retained-nodes.md`: a "4h: the UI renderer (measured)" section — the guard's medians against 4g's 81-96 µs, the criterion numbers, the draws per demo page, and whether uploading only what changed is now worth it (the upload's share of the changing frame).

- [ ] **Step 3: Run the gate and commit**

```bash
git add benches/ui_render.rs src/core/render/AGENTS.md .claude/skills/ryo-ui/SKILL.md src/cli/skills/ryo-ui.md docs/specs/2026-10-20-ui-renderer-design.md docs/backlog/ui-retained-nodes.md
git commit -m "test: the UI render guard asserts the 4h target, and the 4h measurement" -m "- one changing label under 40 µs of render-side CPU
- the render module notes, the ryo-ui skill and the spec describe the UI renderer"
```

---

### Task 9: An honest guard, and a settings screen that scrolls

Added after Task 8 (owner decision, 2026-10-21): the guard's empty-frame baseline is GPU-paced when each engine runs alone, so it under-reads the UI's cost (one label read 0-26 µs; measured CPU-honestly, interleaved, it is 58-63 µs over an empty frame, pre-4h 106-107). The idle check is flaky for the same reason, and the "scrolling" bench never scrolls (the shared settings fixture's scroll area grows to fit its content).

**Files:**
- Modify: `benches/ui_render.rs`, `src/core/testing/fixtures.rs` (the settings tree's scroll area), any test or bench whose numbers or assertions move with the fixture, `docs/backlog/ui-retained-nodes.md` (the 4h section's numbers)

- [ ] **Step 1: The guard measures CPU honestly.** Step the engines it compares in turn, frame by frame (round robin), so no engine is GPU-paced; take each engine's median render step (`HeadlessEngine::render_time`) over the same frames; compare each scene with the empty frame measured the same way. Print every median. Keep the targets but assert only what the honest numbers meet today (retained no slower than unretained; idle and one-label printed against 5 µs and 40 µs); Task 10 re-decides the assertions after its change.
- [ ] **Step 2: Explain the idle numbers.** Interleaved, an idle (kept) settings frame costs 5-7.5 µs over an empty one against 3.8-5.6 µs before 4h. Profile the kept frame on both (the pre-4h base is commit d1016380) and find what the kept frame now does that it did not (or confirm noise); fix it if it is waste, with a test or a bench line.
- [ ] **Step 3: A settings screen that scrolls.** Give the fixture's scroll area a bounded height (its content, 1,406 UI units, must overflow the 460-unit panel), so the wheel scrolls it; make `settings_scrolling` assert that its offset moves. Run every test and bench that uses `settings_tree()` (4f's cache benches and allocation ceiling, 4g's retention and render benches, the e2e oracles) and fix only what the fixture change legitimately moves (record each).
- [ ] **Step 4: Record and commit.** Update the 4h numbers in `docs/backlog/ui-retained-nodes.md` with the honest method; run the gate; commit `test: the UI render guard measures CPU honestly, and the settings screen scrolls`.

### Task 10: Upload only what changed

Added after Task 8 (owner decision, 2026-10-21): the upload is the largest part of a changing frame (19.4 µs: 11.3 copying into a fresh staging buffer, 6.7 creating it).

**Files:**
- Modify: `src/core/render/ui_renderer/gpu.rs` (`UiGpu::upload`), its tests, `benches/ui_render.rs`, `docs/backlog/ui-retained-nodes.md`, `src/core/render/AGENTS.md` (the UI renderer section)

- [ ] **Step 1: Write the failing tests.** A unit test of the range rule (pure: last frame's instances and this frame's give one range `first..end` — `first` the first differing instance, `end` the last differing one + 1 when the lengths are equal, the new length when they differ; empty when equal; the whole array after growth or a new buffer) and a GPU test that, after several frames with one label changing, the instance buffer's contents equal the frame's instances (read the buffer back).
- [ ] **Step 2: Implement.** `UiGpu` keeps a copy of what the buffer holds; `upload` writes only the one range that differs (one `write_buffer`), the whole array when the buffer was recreated or grown, nothing when nothing differs. The compare is a slice compare from both ends.
- [ ] **Step 3: Measure.** Run the honest guard (Task 9): one label against 40 µs, idle against 5 µs. Assert each target the measurement meets; for a target still missed, keep it printed, profile the frame, and report the numbers and the next costs (the text cache copies, the pass encode) for the owner.
- [ ] **Step 4: Docs and commit.** The render module notes (the upload rule), the backlog's 4h numbers; run the gate; commit `feat: the UI uploads only the instances that changed`.

### Task 11: Scrolling texts cost no more retained than unretained

Added after Task 9: with the settings list actually scrolling, a retained frame costs 8-11 µs more than an unretained one (the pre-4h base too: 10-13), because `TextKey` holds each text's origin, so every moved text misses the text cache, is laid out and is stored again. The spec's bench criterion is "retained no slower than unretained".

**Files:**
- Modify: `src/core/render/ui_retain/texts.rs` (`UiTextCache`, `TextKey`) and whatever builds a text's instances from its origin, their tests, `benches/ui_render.rs`, `src/core/render/AGENTS.md` (UI render retention), `docs/backlog/ui-retained-nodes.md`

- [ ] **Step 1: Find what a text's instances depend on.** Trace how a UI text's glyph positions follow from its origin (snapping to physical pixels, the pixel-art snap, sub-pixel glyph images). Decide the cheapest exact rule: either a text moved by a whole physical-pixel offset (after whatever snapping layout applies) reuses its stored instances translated by that offset, or a text that keeps moving is not stored while it moves. Record the reasoning in the report.
- [ ] **Step 2: Write the failing tests.** A unit test that a text moved by the offsets the rule covers reuses its instances and that the result equals a fresh layout at the new origin, instance for instance; a test that the cache does no extra work for a text that moves every frame (whichever the rule gives); the e2e retention oracle (`PixelPair`, retention on against off) on a scrolling list driven by wheel steps and by fractional trackpad deltas.
- [ ] **Step 3: Implement** the rule. Pixels must not change anywhere (the retention oracles, the e2e renderer tests).
- [ ] **Step 4: Measure** with the honest guard: scrolling retained no slower than unretained (assert it if the numbers meet it with margin over five runs); one label and idle not regressed.
- [ ] **Step 5: Docs and commit.** The render module notes' text cache paragraph, the backlog's numbers; run the gate; commit `perf: a scrolled text reuses its cached instances`.
