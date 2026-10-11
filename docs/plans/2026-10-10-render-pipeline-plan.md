# Pass-Based Render Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every GPU frame is drawn by one ordered list of render passes (`RenderPipeline::default_2d()`), replacing the four hand-written render paths, with pixel-identical output.

**Architecture:** `src/core/render/` is rewritten: a public `RenderPass` trait, an ordered `RenderPipeline`, `FrameInfo` and a `RenderContext` with a small public surface and crate-internal fields. Twelve thin built-in passes call into the existing renderers. `WgpuRenderer::render_frame` runs the pipeline on one encoder; `present_frame` builds a `FrameScene` for it. The new path is built next to the old one behind a crate-internal switch, proven byte-identical by headless comparison tests, then the old paths are deleted.

**Tech Stack:** Rust 2024, wgpu, hecs, thiserror.

**Spec:** `docs/plans/2026-10-10-render-pipeline-design.md`

## Global Constraints

- Rust 2024, 2-space indentation, no `unwrap`/`expect`/`panic!` in library code (tests may).
- Format only lines you change; never plain-rustfmt a file declaring `mod x;` children (`src/lib.rs`, any `mod.rs`, `src/core/engine.rs` if it declares modules): `rustfmt --edition 2024 --check <file>` and fix only your hunks. Files you create from scratch may be fully formatted.
- Line endings: run `file <path>` (or compare `wc -l` with `grep -c $'\r$'`) before editing. CRLF today: `src/core/render/mod.rs`, `src/core/render/pass.rs`, `src/core/render/passes/{mod,clear,upload}.rs`, `src/core/scene.rs`, all skill files. Keep each existing file's endings on every line (a full rewrite of a CRLF file is written CRLF). New files are LF.
- Pass ids (exact strings): `upload`, `clear`, `raycast`, `gpu_sprites`, `cpu_blend`, `trails`, `lighting`, `unlit_trails`, `post_process`, `ui`, `composite`, `capture` - in that order in `default_2d()`.
- Frames stay pixel-identical; the only intended difference is a raycast frame without wall textures showing black (Task 4).
- Breaking demos/games is allowed; fix what breaks, don't shim.
- One commit per task with the given message; no trailer lines (no Co-Authored-By, no Claude-Session); don't push.
- Done = `cargo test --workspace` 0 failed; `cargo clippy --workspace --all-targets` no new warnings in touched files; `cargo check --workspace --all-targets` builds (examples included).

## Review Focus

1. A scene using lighting **and** effects **and** UI must look exactly as before - Task 3 comparison `lighting_effects_and_ui`.
2. A game that disables or removes a built-in pass mid-game must not crash or draw stale UI - Task 3 `composite` only blends UI the `ui` pass uploaded this frame; Task 5 `disabling_post_process_removes_the_effect`.
3. Raycast frames must keep their black background and effects/UI - Task 3 comparison `raycast_scene`; Task 4 `raycast_without_wall_textures_is_black`.
4. A resolution change must not leave the persistent lit texture the wrong size - Task 2 (lit texture recreated wherever the game texture is) + Task 3 comparison after the lighting scene.
5. Pipeline misconfiguration (duplicate id, unknown anchor) must return an error, never panic - Task 1 tests.

---

### Task 1: Pipeline core API

**Files:**
- Rewrite: `src/core/render/mod.rs` (CRLF), `src/core/render/pass.rs` (CRLF), `src/core/render/pipeline.rs` (LF), `src/core/render/context.rs` (LF)
- Create: `src/core/render/frame.rs`
- Delete: `src/core/render/passes/` (all files: `mod.rs`, `clear.rs`, `parallax.rs`, `upload.rs`, `lighting.rs`)
- Rewrite: `tests/integration/render_pipeline.rs`
- Modify: `src/lib.rs` prelude (add the render exports), `src/core/render/AGENTS.md` (replace the dependency-graph bullets with the ordered-list rules; keep "not wired into the engine yet")

**Interfaces:**
- Produces: `RenderPass` (`id`, `should_run`, `execute`), `RenderPipeline` (`new`, `Default`, `push`, `insert_before`, `insert_after`, `remove`, `set_enabled`, `is_enabled`, `contains`, `pass_ids`, `passes_to_run`), `RenderPipelineError`, `FrameInfo`, `RenderContext<'a>` (private fields `device`, `queue`, `encoder`, `frame`; accessors `device()`, `queue()`, `encoder()`, `frame()`, `game_size()`), `render::pass_ids` consts.

- [ ] **Step 1: Write the failing tests** — replace `tests/integration/render_pipeline.rs` with:

```rust
//! RenderPipeline configuration (no GPU needed)

use ryo2d::prelude::*;

/// A pass that does nothing, identified by name.
struct Named(&'static str);

impl RenderPass for Named {
  fn id(&self) -> &str {
    self.0
  }

  fn execute(&mut self, _ctx: &mut RenderContext<'_>) {}
}

/// A pass that only takes part in lit frames.
struct LitOnly;

impl RenderPass for LitOnly {
  fn id(&self) -> &str {
    "lit_only"
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.lighting
  }

  fn execute(&mut self, _ctx: &mut RenderContext<'_>) {}
}

fn pipeline(ids: &[&'static str]) -> RenderPipeline {
  let mut pipeline = RenderPipeline::new();
  for &id in ids {
    pipeline.push(Named(id)).unwrap();
  }
  pipeline
}

fn ids(pipeline: &RenderPipeline) -> Vec<&str> {
  pipeline.pass_ids().collect()
}

#[test]
fn passes_keep_the_order_they_were_added_in() {
  assert_eq!(ids(&pipeline(&["a", "b", "c"])), vec!["a", "b", "c"]);
}

#[test]
fn default_is_empty() {
  assert_eq!(ids(&RenderPipeline::default()), Vec::<&str>::new());
}

#[test]
fn passes_go_before_or_after_a_named_pass() {
  let mut pipeline = pipeline(&["a", "c"]);

  pipeline.insert_before("c", Named("b")).unwrap();
  pipeline.insert_after("c", Named("d")).unwrap();

  assert_eq!(ids(&pipeline), vec!["a", "b", "c", "d"]);
}

#[test]
fn duplicate_ids_are_rejected() {
  let mut pipeline = pipeline(&["a", "b"]);

  assert_eq!(
    pipeline.push(Named("b")),
    Err(RenderPipelineError::DuplicatePass("b".to_string()))
  );
  assert_eq!(
    pipeline.insert_after("a", Named("a")),
    Err(RenderPipelineError::DuplicatePass("a".to_string()))
  );
  assert_eq!(ids(&pipeline), vec!["a", "b"]);
}

#[test]
fn unknown_ids_are_rejected() {
  let mut pipeline = pipeline(&["a"]);
  let unknown = Err(RenderPipelineError::UnknownPass("missing".to_string()));

  assert_eq!(pipeline.insert_before("missing", Named("b")), unknown);
  assert_eq!(pipeline.insert_after("missing", Named("b")), unknown);
  assert_eq!(pipeline.set_enabled("missing", false), unknown);
  assert_eq!(pipeline.is_enabled("missing"), None);
  assert!(pipeline.remove("missing").is_none());
  assert_eq!(ids(&pipeline), vec!["a"]);
}

#[test]
fn disabled_passes_keep_their_place_but_do_not_run() {
  let mut pipeline = pipeline(&["a", "b", "c"]);

  pipeline.set_enabled("b", false).unwrap();

  assert_eq!(pipeline.is_enabled("b"), Some(false));
  assert_eq!(ids(&pipeline), vec!["a", "b", "c"]);
  assert_eq!(pipeline.passes_to_run(&FrameInfo::default()), vec!["a", "c"]);

  pipeline.set_enabled("b", true).unwrap();
  assert_eq!(pipeline.passes_to_run(&FrameInfo::default()), vec!["a", "b", "c"]);
}

#[test]
fn remove_hands_the_pass_back() {
  let mut pipeline = pipeline(&["a", "b"]);

  let removed = pipeline.remove("a").unwrap();

  assert_eq!(removed.id(), "a");
  assert!(!pipeline.contains("a"));
  assert_eq!(ids(&pipeline), vec!["b"]);
}

#[test]
fn should_run_decides_per_frame() {
  let mut pipeline = pipeline(&["a"]);
  pipeline.push(LitOnly).unwrap();
  let lit = FrameInfo {
    lighting: true,
    ..FrameInfo::default()
  };

  assert_eq!(pipeline.passes_to_run(&FrameInfo::default()), vec!["a"]);
  assert_eq!(pipeline.passes_to_run(&lit), vec!["a", "lit_only"]);
}

#[test]
fn errors_name_the_pass() {
  assert_eq!(
    RenderPipelineError::DuplicatePass("x".into()).to_string(),
    "a render pass with id `x` is already in the pipeline"
  );
  assert_eq!(
    RenderPipelineError::UnknownPass("x".into()).to_string(),
    "no render pass with id `x`"
  );
}
```

- [ ] **Step 2: Run to watch them fail**

Run: `cargo test --test integration render_pipeline 2>&1 | tail -5`
Expected: compile errors (`FrameInfo`, `RenderPipelineError`, `passes_to_run`, `should_run` don't exist).

- [ ] **Step 3: Implement**

`src/core/render/frame.rs` (new):

```rust
//! What a frame contains, decided before any pass runs

/// What this frame contains, decided before any render pass runs.
///
/// Passes read it in [`RenderPass::should_run`](super::RenderPass::should_run)
/// to decide whether they take part in the frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameInfo {
  /// Game resolution width in pixels
  pub game_width: u32,
  /// Game resolution height in pixels
  pub game_height: u32,
  /// The GPU raycaster draws this frame instead of the 2D scene
  pub raycast: bool,
  /// GPU sprite batches were submitted this frame
  pub gpu_sprites: bool,
  /// Trails or lines have geometry this frame
  pub trails: bool,
  /// 2D lighting is on
  pub lighting: bool,
  /// Post-process effects are active
  pub effects: bool,
  /// The UI buffer has something to draw
  pub ui: bool,
  /// A frame capture was requested for this frame
  pub capture: bool,
}
```

`src/core/render/pass.rs` (CRLF):

```rust
//! The render pass trait

use super::{FrameInfo, RenderContext};

/// One stage of the frame, run in order by a [`RenderPipeline`](super::RenderPipeline).
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// struct Flash;
///
/// impl RenderPass for Flash {
///   fn id(&self) -> &str {
///     "flash"
///   }
///
///   fn execute(&mut self, ctx: &mut RenderContext) {
///     // Record GPU work with ctx.device(), ctx.queue() and ctx.encoder()
///   }
/// }
///
/// ctx.render_pipeline.insert_after(pass_ids::POST_PROCESS, Flash)?;
/// ```
pub trait RenderPass: Send + Sync {
  /// Unique id, used to find, enable, disable and anchor passes.
  fn id(&self) -> &str;

  /// Whether this pass takes part in a frame. Only asked while the pass is
  /// enabled. Defaults to always.
  fn should_run(&self, _frame: &FrameInfo) -> bool {
    true
  }

  /// Record this pass's GPU work for the frame.
  fn execute(&mut self, ctx: &mut RenderContext<'_>);
}
```

`src/core/render/context.rs`:

```rust
//! What a render pass can use while it records its work

use super::FrameInfo;

/// What a [`RenderPass`](super::RenderPass) can use while it records its work.
pub struct RenderContext<'a> {
  device: &'a wgpu::Device,
  queue: &'a wgpu::Queue,
  encoder: &'a mut wgpu::CommandEncoder,
  frame: FrameInfo,
}

impl<'a> RenderContext<'a> {
  /// The GPU device.
  pub fn device(&self) -> &'a wgpu::Device {
    self.device
  }

  /// The GPU queue (buffer and texture writes land before the frame's commands).
  pub fn queue(&self) -> &'a wgpu::Queue {
    self.queue
  }

  /// The frame's command encoder.
  pub fn encoder(&mut self) -> &mut wgpu::CommandEncoder {
    self.encoder
  }

  /// What this frame contains.
  pub fn frame(&self) -> &FrameInfo {
    &self.frame
  }

  /// Game resolution in pixels (width, height).
  pub fn game_size(&self) -> (u32, u32) {
    (self.frame.game_width, self.frame.game_height)
  }
}
```

`src/core/render/pipeline.rs`:

```rust
//! An ordered list of render passes

use super::{FrameInfo, RenderPass};

/// A problem configuring a [`RenderPipeline`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderPipelineError {
  /// A pass with this id is already in the pipeline
  #[error("a render pass with id `{0}` is already in the pipeline")]
  DuplicatePass(String),
  /// No pass has this id
  #[error("no render pass with id `{0}`")]
  UnknownPass(String),
}

struct Entry {
  pass: Box<dyn RenderPass>,
  enabled: bool,
}

/// The frame's render passes, run in order.
///
/// Each enabled pass whose [`should_run`](RenderPass::should_run) holds for
/// the frame records its work, in list order, on one command encoder.
#[derive(Default)]
pub struct RenderPipeline {
  entries: Vec<Entry>,
}

impl RenderPipeline {
  /// An empty pipeline.
  pub fn new() -> Self {
    Self::default()
  }

  /// Add a pass at the end.
  pub fn push(&mut self, pass: impl RenderPass + 'static) -> Result<(), RenderPipelineError> {
    self.insert_at(self.entries.len(), Box::new(pass))
  }

  /// Add a pass just before the pass `anchor`.
  pub fn insert_before(
    &mut self,
    anchor: &str,
    pass: impl RenderPass + 'static,
  ) -> Result<(), RenderPipelineError> {
    let index = self.index_of(anchor)?;
    self.insert_at(index, Box::new(pass))
  }

  /// Add a pass just after the pass `anchor`.
  pub fn insert_after(
    &mut self,
    anchor: &str,
    pass: impl RenderPass + 'static,
  ) -> Result<(), RenderPipelineError> {
    let index = self.index_of(anchor)? + 1;
    self.insert_at(index, Box::new(pass))
  }

  /// Take a pass out of the pipeline.
  pub fn remove(&mut self, id: &str) -> Option<Box<dyn RenderPass>> {
    let index = self.position(id)?;
    Some(self.entries.remove(index).pass)
  }

  /// Turn a pass on or off without moving it.
  pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), RenderPipelineError> {
    let index = self.index_of(id)?;
    self.entries[index].enabled = enabled;
    Ok(())
  }

  /// Whether a pass is on, or `None` if there is no such pass.
  pub fn is_enabled(&self, id: &str) -> Option<bool> {
    self.position(id).map(|index| self.entries[index].enabled)
  }

  /// Whether the pipeline has a pass with this id.
  pub fn contains(&self, id: &str) -> bool {
    self.position(id).is_some()
  }

  /// Every pass id, in order.
  pub fn pass_ids(&self) -> impl Iterator<Item = &str> {
    self.entries.iter().map(|entry| entry.pass.id())
  }

  /// The ids of the passes that would run for `frame`, in order.
  pub fn passes_to_run(&self, frame: &FrameInfo) -> Vec<&str> {
    self
      .entries
      .iter()
      .filter(|entry| entry.enabled && entry.pass.should_run(frame))
      .map(|entry| entry.pass.id())
      .collect()
  }

  fn position(&self, id: &str) -> Option<usize> {
    self.entries.iter().position(|entry| entry.pass.id() == id)
  }

  fn index_of(&self, id: &str) -> Result<usize, RenderPipelineError> {
    self
      .position(id)
      .ok_or_else(|| RenderPipelineError::UnknownPass(id.to_string()))
  }

  fn insert_at(
    &mut self,
    index: usize,
    pass: Box<dyn RenderPass>,
  ) -> Result<(), RenderPipelineError> {
    if self.contains(pass.id()) {
      return Err(RenderPipelineError::DuplicatePass(pass.id().to_string()));
    }
    self.entries.insert(index, Entry { pass, enabled: true });
    Ok(())
  }
}
```

`src/core/render/mod.rs` (CRLF) — module docs describing the ordered pass list (each enabled pass whose `should_run` holds records its work in order on one encoder; insert before/after a named pass; note "The engine doesn't run this pipeline yet; it's being wired in"), then:

```rust
mod context;
mod frame;
mod pass;
mod pipeline;

pub use context::RenderContext;
pub use frame::FrameInfo;
pub use pass::RenderPass;
pub use pipeline::{RenderPipeline, RenderPipelineError};

/// Ids of the built-in passes, in the order `RenderPipeline::default_2d` runs them.
pub mod pass_ids {
  /// CPU pixel buffer upload
  pub const UPLOAD: &str = "upload";
  /// Game texture clear
  pub const CLEAR: &str = "clear";
  /// GPU raycaster (replaces the 2D scene passes while raycasting)
  pub const RAYCAST: &str = "raycast";
  /// GPU sprite batches
  pub const GPU_SPRITES: &str = "gpu_sprites";
  /// CPU-rendered content blended onto the game texture
  pub const CPU_BLEND: &str = "cpu_blend";
  /// Lit trails and lines
  pub const TRAILS: &str = "trails";
  /// 2D lighting
  pub const LIGHTING: &str = "lighting";
  /// Trails and lines that skip lighting
  pub const UNLIT_TRAILS: &str = "unlit_trails";
  /// Post-process effects
  pub const POST_PROCESS: &str = "post_process";
  /// UI upload
  pub const UI: &str = "ui";
  /// Final composite to the window or offscreen target
  pub const COMPOSITE: &str = "composite";
  /// Frame capture
  pub const CAPTURE: &str = "capture";
}
```

`src/lib.rs` prelude: add `pub use crate::core::render::{FrameInfo, RenderContext, RenderPass, RenderPipeline, RenderPipelineError, pass_ids};` next to the other `crate::core::` exports (byte-preserving edit; `rustfmt --check` only). Delete `src/core/render/passes/` and its `pub mod passes;`. Fix anything else that referenced the removed types (`grep -rn "core::render::\|render::passes\|TextureSlot\|RenderPassId" src tests examples`).

- [ ] **Step 4: Run the tests**

Run: `cargo test --test integration render_pipeline 2>&1 | grep -E "test result|FAILED"` — all pass. `cargo test --doc 2>&1 | grep -E "test result|FAILED"`.

- [ ] **Step 5: Suite, lint, commit**

`cargo test --workspace 2>&1 | grep -E "^test result|FAILED"`, `cargo clippy --workspace --all-targets 2>&1 | grep -E "^\s+--> (src/core/render|src/lib.rs|tests/integration/render_pipeline)"`, `cargo check --workspace --all-targets 2>&1 | tail -1`.

```bash
git add -A src/core/render src/lib.rs tests/integration/render_pipeline.rs
git commit -m "refactor(render): replace the experimental pass graph with an ordered pass list" -m "- RenderPass gains should_run; the pipeline runs passes in list order
- Insert before/after a named pass; duplicate or unknown ids are errors, not panics
- Remove the unused experimental passes"
```

---

### Task 2: Compositor and a persistent lit texture (no behaviour change)

**Files:**
- Create: `src/core/graphics/compositor.rs`
- Modify: `src/core/graphics/renderer.rs`, `src/core/graphics/mod.rs` (`mod compositor; pub(crate) use compositor::{Compositor, Viewport};`)

**Interfaces:**
- Produces: `pub(crate) struct Compositor` owning the final-composite resources (the REPLACE `render_pipeline`, the alpha `blend_pipeline`, `bind_group_layout`, `sampler`, `ui_texture` + `ui_texture_view`, the letterbox `uniform_buffer`, the `identity_uniform_buffer`), with `pub(crate) fn draw(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, source: &wgpu::TextureView, include_ui: bool, viewport: Viewport)` and `pub(crate) fn upload_ui(&self, queue: &wgpu::Queue, data: &[u8], width: u32, height: u32)`; `pub(crate) enum Viewport { Letterboxed, Identity }`. `WgpuRenderer` fields `compositor: Compositor`, `lit_texture: wgpu::Texture`, `lit_texture_view: wgpu::TextureView`.

- [ ] **Step 1: Baseline** — this task is a pure refactor; the existing GPU tests are its tests. Record them first: `cargo test --test e2e 2>&1 | grep -E "test result|FAILED"` and `cargo test --lib graphics:: 2>&1 | grep -E "test result|FAILED"`.

- [ ] **Step 2: Implement**
  - Move `draw_final_composite` and `composite_bind_group` (renderer.rs, "Draw the final frame" / "Bind group sampling") into `Compositor::draw` (+ a private bind-group helper), choosing the uniform from `viewport` (`Letterboxed` → `uniform_buffer`, `Identity` → `identity_uniform_buffer`). Move the UI `write_texture` blocks' body into `Compositor::upload_ui`. Construct the `Compositor` where `WgpuRenderer::new`/`new_offscreen` build those resources today (move the fields; every `self.render_pipeline`, `self.blend_pipeline`, `self.bind_group_layout`, `self.sampler`, `self.ui_texture*`, `self.uniform_buffer`, `self.identity_uniform_buffer` use becomes `self.compositor.…`, including resize and viewport-uniform updates). `encode_capture_pass` calls `self.compositor.draw(…, Viewport::Identity)`.
  - Lit texture: create `lit_texture` (label "Lit Scene Texture", game resolution, `Rgba8UnormSrgb`, `RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC` - exactly the descriptor `render_with_lighting` uses per frame today) wherever the game `texture` is created or recreated (grep the "Game Texture" descriptor and every place `self.texture =` is assigned). `render_with_lighting` uses `&self.lit_texture` / `&self.lit_texture_view` instead of allocating; keep its logic otherwise unchanged.
  - Bind groups that sample the game or lit texture and are created once must be recreated with them (check `cpu_bind_group` handling as the model).

- [ ] **Step 3: Run the tests**

Same commands as Step 1 - identical results (every GPU e2e test still passes, same counts).

- [ ] **Step 4: Suite, lint, commit**

As Task 1 Step 5 (clippy filter `src/core/graphics`).

```bash
git add src/core/graphics
git commit -m "refactor(graphics): move the final composite into a Compositor and keep the lit texture"
```

---

### Task 3: Built-in passes and `render_frame`, behind a switch, proven identical

**Files:**
- Modify: `src/core/render/frame.rs` (crate-internal `FrameImage`, `FrameScene`, `LightingFrame`), `src/core/render/context.rs` (crate-internal fields, `new`, `current_view`, `encoder_and_current_view`), `src/core/render/pipeline.rs` (`default_2d`, `pub(crate) fn execute`), `src/core/render/mod.rs` (`mod passes;`)
- Create: `src/core/render/passes/mod.rs` and one file per group: `scene.rs` (upload, clear, gpu_sprites, cpu_blend), `raycast.rs`, `trails.rs` (trails, unlit_trails), `lighting.rs`, `post_process.rs`, `output.rs` (ui, composite, capture)
- Modify: `src/core/graphics/renderer.rs` (`render_frame`, `last_frame_info`), `src/core/engine.rs` (`render_pipeline` field, `legacy_render` switch, `FrameScene` construction, `sync_raycast` split out of `sync_and_render_raycast`)
- Create: `src/core/testing/render_comparison.rs` (`#[cfg(test)] mod render_comparison;` in `src/core/testing/mod.rs`)
- Modify: `tests/integration/render_pipeline.rs` (default order and frame-kind tests)

**Interfaces:**
- Consumes: Task 1 types; Task 2 `Compositor`, `Viewport`, `lit_texture`.
- Produces:

```rust
// frame.rs (crate-internal)
/// Which texture holds the frame's latest image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameImage {
  Game,
  Lit,
  PostProcess(OutputBuffer), // crate::core::shaders OutputBuffer (A or B)
}

/// Lighting inputs for a frame (only built while lighting is enabled).
pub(crate) struct LightingFrame<'a> {
  pub(crate) renderer: &'a mut LightingRenderer,
  pub(crate) lights: &'a [(Transform, Light2D)],
  pub(crate) occluders: &'a [(Transform, LightOccluder2D)],
}

/// The engine subsystems and per-frame data the built-in passes draw from.
pub(crate) struct FrameScene<'a> {
  pub(crate) sprites: &'a SpriteGpuRenderer,
  pub(crate) batches: &'a BatchCollector,
  pub(crate) trails: Option<&'a TrailRenderer>,
  pub(crate) lighting: Option<LightingFrame<'a>>,
  pub(crate) raycast: Option<&'a GpuRaycastRenderer>, // only while raycasting is enabled
  pub(crate) effects: &'a EffectsManager,
  pub(crate) ui_buffer: &'a [u8],
  pub(crate) camera_pos: glam::Vec2,
  pub(crate) camera_zoom: f32,
  pub(crate) time: f32,
}

// context.rs (crate-internal)
pub(crate) struct FrameTargets<'a> {
  pub(crate) game: &'a wgpu::Texture,
  pub(crate) game_view: &'a wgpu::TextureView,
  pub(crate) lit: &'a wgpu::Texture,
  pub(crate) lit_view: &'a wgpu::TextureView,
  pub(crate) cpu_texture: &'a wgpu::Texture,
  pub(crate) pixel_buffer: &'a [u8],
  pub(crate) cpu_blend_pipeline: &'a wgpu::RenderPipeline,
  pub(crate) cpu_bind_group: &'a wgpu::BindGroup,
  pub(crate) compositor: &'a Compositor,
  pub(crate) output_view: &'a wgpu::TextureView,
  pub(crate) capture: Option<&'a wgpu::Texture>,
}
// RenderContext gains: pub(crate) targets: FrameTargets<'a>, pub(crate) scene: FrameScene<'a>,
// pub(crate) post_process: Option<&'a mut PostProcessPipeline>, pub(crate) image: FrameImage,
// pub(crate) ui_uploaded: bool; pub(crate) fn new(device, queue, encoder, frame, targets, scene, post_process)
// (image starts as Game); and public:
//   pub fn current_view(&self) -> &wgpu::TextureView
//   pub fn encoder_and_current_view(&mut self) -> (&mut wgpu::CommandEncoder, &wgpu::TextureView)

// renderer.rs
pub(crate) fn render_frame(&mut self, pipeline: &mut RenderPipeline, scene: FrameScene<'_>)
  -> Result<(), wgpu::SurfaceError>;
pub(crate) fn last_frame_info(&self) -> Option<FrameInfo>; // FrameInfo of the latest render_frame
```

- [ ] **Step 1: Write the failing tests**

`tests/integration/render_pipeline.rs` — add:

```rust
const ORDER: [&str; 12] = [
  pass_ids::UPLOAD,
  pass_ids::CLEAR,
  pass_ids::RAYCAST,
  pass_ids::GPU_SPRITES,
  pass_ids::CPU_BLEND,
  pass_ids::TRAILS,
  pass_ids::LIGHTING,
  pass_ids::UNLIT_TRAILS,
  pass_ids::POST_PROCESS,
  pass_ids::UI,
  pass_ids::COMPOSITE,
  pass_ids::CAPTURE,
];

#[test]
fn default_2d_runs_the_built_in_passes_in_order() {
  assert_eq!(ids(&RenderPipeline::default_2d()), ORDER.to_vec());
}

#[test]
fn a_plain_frame_runs_the_scene_passes() {
  assert_eq!(
    RenderPipeline::default_2d().passes_to_run(&FrameInfo::default()),
    vec!["upload", "clear", "cpu_blend", "composite"]
  );
}

#[test]
fn a_full_2d_frame_runs_every_scene_pass() {
  let frame = FrameInfo {
    gpu_sprites: true,
    trails: true,
    lighting: true,
    effects: true,
    ui: true,
    capture: true,
    ..FrameInfo::default()
  };
  assert_eq!(
    RenderPipeline::default_2d().passes_to_run(&frame),
    vec![
      "upload", "clear", "gpu_sprites", "cpu_blend", "trails", "lighting", "unlit_trails",
      "post_process", "ui", "composite", "capture",
    ]
  );
}

#[test]
fn a_raycast_frame_skips_the_2d_scene() {
  let frame = FrameInfo {
    raycast: true,
    gpu_sprites: true,
    trails: true,
    lighting: true,
    effects: true,
    ui: true,
    ..FrameInfo::default()
  };
  assert_eq!(
    RenderPipeline::default_2d().passes_to_run(&frame),
    vec!["raycast", "post_process", "ui", "composite"]
  );
}
```

`src/core/testing/render_comparison.rs` — the module can reach `HeadlessEngine`'s private `engine` field. Helper:

```rust
//! The pass pipeline draws exactly what the old render paths drew.

use super::HeadlessEngine;
use crate::core::engine::EngineBuilder;

/// Run the same scene through the old paths and the pipeline; every frame
/// must be byte-identical, and the pipeline must have run `expected` passes.
fn assert_paths_match(name: &str, make: impl Fn() -> EngineBuilder, steps: u32, expected: &[&str]) {
  let Some(mut legacy) = HeadlessEngine::new_or_skip(make()) else {
    return;
  };
  let Some(mut pipeline) = HeadlessEngine::new_or_skip(make()) else {
    return;
  };
  legacy.engine.legacy_render = true;
  pipeline.engine.legacy_render = false;

  for step in 1..=steps {
    legacy.step().unwrap();
    pipeline.step().unwrap();
    let (old, new) = (legacy.frame().unwrap(), pipeline.frame().unwrap());
    let differing = old.pixels().zip(new.pixels()).filter(|(a, b)| a != b).count();
    assert_eq!(differing, 0, "{name}: {differing} pixels differ at step {step}");
  }

  let frame = pipeline.engine.last_frame_info().expect("the pipeline rendered");
  assert_eq!(pipeline.engine.render_pipeline.passes_to_run(&frame), expected, "{name}: passes run");
}
```

(Add a `pub(crate) fn last_frame_info(&self) -> Option<FrameInfo>` on `Engine` forwarding to the renderer if the field isn't reachable directly.)

One `#[test]` per scene, each a small deterministic `Scene` (no particles - their RNG is unseeded; model scenes on `tests/e2e/headless_engine.rs` `Squares`/`LineScene` and the lighting e2e tests), run for 5 steps at 160x120:
- `cpu_sprites_and_ui` — two coloured sprite entities at different z, UI text/rect in `render_ui` → `["upload", "clear", "cpu_blend", "ui", "composite"]`
- `gpu_and_cpu_sprites` — sprite instances added in `render_gpu` (a solid texture via the `GpuRenderContext` API) plus a CPU sprite → `["upload", "clear", "gpu_sprites", "cpu_blend", "composite"]`
- `trails_without_lighting` — a moving `Trail2D`, a `Line2D` and a `Line2D::with_lighting(false)` → `["upload", "clear", "cpu_blend", "trails", "unlit_trails", "composite"]`
- `lighting_with_shadows` — `Light2D` (shadow-casting), `LightOccluder2D`, CPU sprites, GPU sprites, both line kinds → `[..., "gpu_sprites", "cpu_blend", "trails", "lighting", "unlit_trails", "composite"]`
- `effects_without_lighting` — a vignette and bloom (`ctx.effects`), CPU sprites, UI → `[..., "cpu_blend", "post_process", "ui", "composite"]`
- `lighting_effects_and_ui` — lights + an effect + UI → `[..., "lighting", "post_process", "ui", "composite"]`
- `raycast_scene` — modelled on `examples/demos/raycasting/main.rs` `on_enter`: a small `RaycastWorld` with solid-colour wall/floor/sprite textures from `ctx.textures().from_rgba(...)` + `create_array`, `set_map_info`, a `RaycastCamera` entity, one `RaycastSprite`, `ctx.enable_raycast()`, a vignette and UI → `["raycast", "post_process", "ui", "composite"]`

Run: `cargo test --lib render_comparison 2>&1 | tail -5` and `cargo test --test integration render_pipeline 2>&1 | tail -5` → compile errors (`legacy_render`, `default_2d`, `last_frame_info` don't exist).

- [ ] **Step 2: Implement the passes** (`src/core/render/passes/`, each a unit struct implementing `RenderPass`; `pub(crate) fn built_in() -> [Box<dyn RenderPass>; 12]` or equivalent used by `default_2d`). Bodies move today's code (renderer.rs `render_with_gpu_sprites` / `render_with_lighting` / `render_raycast`, engine.rs callbacks):

| Pass | `should_run` | `execute` |
|---|---|---|
| `upload` | `!f.raycast` | `queue.write_texture` of `targets.pixel_buffer` into `targets.cpu_texture` (game size) |
| `clear` | `!f.raycast` | empty render pass on `targets.game_view`, `LoadOp::Clear(DEFAULT_BACKGROUND_COLOR)`; `image = Game` |
| `raycast` | `f.raycast` | empty render pass clearing `targets.game_view` to `Color::BLACK`, then `raycast.render_walls(device, encoder, game_view)` and `raycast.render_sprites(device, encoder, game_view)`; `image = Game` |
| `gpu_sprites` | `!f.raycast && f.gpu_sprites` | `scene.sprites.render_batches(queue, encoder, game_view, scene.batches)` |
| `cpu_blend` | `!f.raycast` | the "CPU Content Blend Pass" (Load, `cpu_blend_pipeline`, `cpu_bind_group`, `draw(0..6, 0..1)`) on `game_view` |
| `trails` | `!f.raycast && f.trails` | `trails.render(queue, encoder, game_view, camera_pos, camera_zoom, time, TrailStyle::Solid)` |
| `lighting` | `!f.raycast && f.lighting` | `render_lights` (lights, camera), `render_shadows` with `generate_shadow_geometry` only if any light `casts_shadows()` and occluders aren't empty, then `compose_from_texture(device, queue, encoder, targets.game, targets.lit_view, time)`; `image = Lit` |
| `unlit_trails` | `!f.raycast && f.trails` | `trails.render_unlit(queue, encoder, <current view>, camera_pos, camera_zoom, time)` |
| `post_process` | `f.effects` | collect `effects.active_effects().map(\|e\| (e, effects.time()))`; if empty or no post-process pipeline, return; copy the current image's texture (game, lit, or the post-process output) to `post_process.texture_a()` (add a crate-internal accessor for buffer B's texture if needed); `image = PostProcess(post_process.apply_effects(device, queue, encoder, &active))` |
| `ui` | `f.ui` | `targets.compositor.upload_ui(queue, scene.ui_buffer, w, h)`; `ui_uploaded = true` |
| `composite` | always | `targets.compositor.draw(device, encoder, targets.output_view, <current view>, ui_uploaded, Viewport::Letterboxed)` |
| `capture` | `f.capture` | if `targets.capture` is `Some`: `compositor.draw(…, <capture view>, <current view>, ui_uploaded, Viewport::Identity)` |

Use field-level borrows inside `RenderContext` methods/passes to hold the encoder mutably while reading views.

- [ ] **Step 3: Implement `render_frame` and the engine wiring**

```rust
  /// Render a frame by running `pipeline`'s passes on one encoder.
  pub(crate) fn render_frame(
    &mut self,
    pipeline: &mut RenderPipeline,
    scene: FrameScene<'_>,
  ) -> Result<(), wgpu::SurfaceError> {
    let (output, view) = self.target.acquire()?;
    let capture_path = self.prepare_capture();
    let frame = FrameInfo {
      game_width: self.game_width,
      game_height: self.game_height,
      raycast: scene.raycast.is_some(),
      gpu_sprites: scene.batches.sprite_count() > 0,
      trails: scene.trails.is_some_and(|t| t.vertex_count() > 0),
      lighting: scene.lighting.is_some(),
      effects: self.post_process.is_some() && scene.effects.has_active_effects(),
      ui: scene.ui_buffer.iter().any(|&b| b != 0),
      capture: capture_path.is_some(),
    };
    self.last_frame = Some(frame);
    let mut encoder = self
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Frame Encoder"),
      });
    {
      let targets = FrameTargets { /* borrow self.texture, self.texture_view, self.lit_texture, … */ };
      let mut ctx = RenderContext::new(
        &self.device, &self.queue, &mut encoder, frame, targets, scene, self.post_process.as_mut(),
      );
      pipeline.execute(&mut ctx);
    }
    self.queue.submit(std::iter::once(encoder.finish()));
    if let Some(output) = output {
      output.present();
    }
    self.finish_capture(capture_path);
    Ok(())
  }
```

`RenderPipeline::execute(&mut self, ctx)` (crate-internal): copy `*ctx.frame()`, then for each entry `if enabled && pass.should_run(&frame) { pass.execute(ctx) }`.

Engine (`present_frame`): add fields `pub(crate) render_pipeline: RenderPipeline` (`RenderPipeline::default_2d()` in every constructor) and `pub(crate) legacy_render: bool` (default `true` in this task). Keep everything up to the branch. Then `if self.legacy_render { <today's branches, untouched> } else { <new> }`, where new: when raycasting is enabled call `sync_raycast(world, raycast, rend, time, texture_mgr)` (the first half of `sync_and_render_raycast`: flush, camera, sprites, lights - `sync_and_render_raycast` now calls it then renders, so the legacy path is unchanged); build `FrameScene` (`lighting: lighting.as_mut().filter(|l| l.is_enabled()).map(|renderer| LightingFrame { renderer, lights: &lights, occluders: &occluders })`, `raycast: raycast_renderer.as_ref().filter(|r| r.is_enabled())`, `trails: trail_renderer.as_ref()`, the rest from the existing locals and fields) and call `rend.render_frame(&mut self.render_pipeline, scene)`. The `match result` error handling stays shared.

- [ ] **Step 4: Run the tests**

`cargo test --lib render_comparison 2>&1 | grep -E "test result|FAILED|differ"` → all pass (0 differing pixels; expected passes ran). `cargo test --test integration render_pipeline`, `cargo test --test e2e` (unchanged results).

- [ ] **Step 5: Suite, lint, commit**

As Task 1 Step 5 (clippy filter `src/core/(render|graphics|engine|testing)`).

```bash
git add src/core tests/integration/render_pipeline.rs
git commit -m "feat(render): draw frames through the built-in pass pipeline behind a switch" -m "- Twelve built-in passes reproduce the four render paths
- Headless comparison tests prove byte-identical frames for every path"
```

---

### Task 4: Switch over and delete the old paths

**Files:**
- Modify: `src/core/engine.rs` (remove `legacy_render` and the legacy branches; `sync_and_render_raycast` → just `sync_raycast`), `src/core/graphics/renderer.rs` (delete `render`, `render_with_effects`, `render_with_gpu_sprites`, `render_with_lighting`, `render_raycast`, and tests tied only to them such as `document_lighting_effects_copy_invariant`), `src/core/testing/mod.rs` (drop `render_comparison`)
- Delete: `src/core/testing/render_comparison.rs`
- Create: `tests/e2e/render_pipeline.rs` (register in `tests/e2e.rs` like the other e2e modules)

- [ ] **Step 1: Write the failing test** (`tests/e2e/render_pipeline.rs`, public API only, `HeadlessEngine::new_or_skip`):
  - `raycast_without_wall_textures_is_black`: a scene that draws a CPU sprite for the first 2 steps (so the game texture holds the background colour and the sprite), then on step 3 calls `ctx.enable_raycast()` without setting wall textures; after step 5 every pixel is `[0, 0, 0, 255]` (no UI). RED today: the old raycast path leaves the previous frame in the game texture.
  - Run: `cargo test --test e2e render_pipeline 2>&1 | tail -5` → FAIL (non-black pixels).

- [ ] **Step 2: Switch and delete** — remove the switch (the pipeline path is the only path), the five renderer functions, the comparison module and anything only they used (`grep` for each name across `src`, `tests`, `examples`, `benches`; fix callers, e.g. dev windows). The `raycast` pass already clears to black (Task 3), so the new test passes.

- [ ] **Step 3: Lasting behaviour tests** (same file):
  - `ui_is_untouched_by_effects_and_lighting`: lighting on with a dark ambient, a vignette, and an opaque UI rectangle in the centre; the centre pixel equals the UI colour exactly.
  - `effects_apply_to_the_lit_image`: a white full-screen CPU sprite, lighting on with ambient ≈ 0.1 and no lights, and a luminance-preserving effect (pick one from `post_process_shaders`, e.g. grayscale, or a weak vignette probed at the centre); the centre pixel stays dark (each channel < 64). It would be bright if effects read the unlit image.

- [ ] **Step 4: Run the tests** — `cargo test --test e2e render_pipeline`, full `cargo test --test e2e` (all GPU e2e tests still pass), `cargo test --test integration`.

- [ ] **Step 5: Suite, lint, check examples, commit**

```bash
git add -A src/core tests/e2e.rs tests/e2e/render_pipeline.rs
git commit -m "refactor(render): render every frame through the pass pipeline" -m "- Remove the four hand-written render paths and their callbacks
- A raycast frame without wall textures is black instead of showing the previous frame"
```

---

### Task 5: Game access, game-pass tests and docs

**Files:**
- Modify: `src/core/scene.rs` (CRLF: `pub render_pipeline: &'a mut RenderPipeline` on `SceneContext`, with rustdoc), `src/core/engine.rs` (both `SceneContext { … }` constructions pass `&mut self.render_pipeline`; check borrows against fields already borrowed there), any other `SceneContext` construction (`grep -rn "SceneContext {" src tests examples`)
- Modify: `tests/e2e/render_pipeline.rs`
- Modify: `src/core/render/AGENTS.md` (rewrite), `src/core/graphics/AGENTS.md` (render path section), `src/core/render/mod.rs` docs (the engine runs this pipeline; how games use `ctx.render_pipeline`), `src/cli/skills/ryo-renderer.md` + `.claude/skills/ryo-renderer/SKILL.md`, `src/cli/skills/ryo-api.md` + `.claude/skills/ryo-api/SKILL.md` (CRLF, identical pairs; check the real skill file names with `ls`)

- [ ] **Step 1: Write the failing tests** (`tests/e2e/render_pipeline.rs`):
  - `a_game_pass_after_post_process_draws_on_the_frame`: a scene whose `on_enter` inserts, after `pass_ids::POST_PROCESS`, a pass that records a render pass on `ctx.encoder_and_current_view()` with `LoadOp::Clear(magenta)`; after 2 steps the frame's centre pixel is magenta (`[255, 0, 255, 255]`).
  - `disabling_post_process_removes_the_effect`: a scene with a uniform grey CPU background and a strong vignette; record a corner pixel (darker than the centre), then the scene calls `ctx.render_pipeline.set_enabled(pass_ids::POST_PROCESS, false)` (e.g. on a key tap or after N updates); the corner pixel now equals the centre pixel.
  - Run → compile error (`SceneContext` has no `render_pipeline`).

- [ ] **Step 2: Implement** the field and the constructions. A pass inserted by a scene persists across scenes (the pipeline is the engine's); say so in the field's rustdoc.

- [ ] **Step 3: Docs**
  - `render/AGENTS.md`: the ordered list and `default_2d` order; `should_run` vs enabled; one encoder, one submit; the latest-image tracking (`FrameImage`) and why the UI is only composited if the `ui` pass uploaded it this frame; thin passes (pipelines live in the subsystem renderers); `FrameScene` built in `present_frame`; gotcha: raycast replaces the 2D scene passes.
  - `graphics/AGENTS.md`: replace mentions of the four render functions/callbacks with the pipeline; keep the CPU/GPU layering notes (sub-project 2 changes them).
  - Skills: the frame's pass order, `ctx.render_pipeline` (toggle a built-in pass, insert a custom `RenderPass` before/after a named one, `encoder_and_current_view`), that custom shaders/effects APIs are coming.
  - Rustdoc: every public item in `render` documented (`#![warn(missing_docs)]` if the crate uses it).

- [ ] **Step 4: Run the tests** — `cargo test --test e2e render_pipeline`, `cargo test --doc`.

- [ ] **Step 5: Suite, lint, check examples, commit**

```bash
git add src/core tests/e2e/render_pipeline.rs src/cli/skills .claude/skills
git commit -m "feat(render): let scenes configure the render pipeline" -m "- SceneContext::render_pipeline toggles built-in passes and inserts game passes
- Document the pass pipeline in the renderer skill and AGENTS.md"
```
