# GPU World Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every world element is drawn on the GPU from one z-sorted draw list (entities, shadows, tiles, particles, parallax, `render_gpu` sprites, trails, lines) using a texture atlas; the CPU entity renderer is retired.

**Architecture:** A `TextureAtlas` (etagere-allocated `Rgba8Unorm` pages) backs every sprite texture. The sprite shader shades in sRGB space and outputs linear, masks effect samples to the sprite's own rect and enlarges quads for growing effects. A per-frame `DrawList` sorted by `(z, kind, sequence)` feeds a `world` pass (one render pass, instanced runs + trail groups) and an `unlit_world` pass. Entities become screen-space `SpriteInstance`s collected on the CPU. Built next to the CPU path behind a switch, proven exact for opaque axis-aligned content, then the CPU entity path is deleted.

**Tech Stack:** Rust 2024, wgpu 22, hecs, etagere (new dependency).

**Spec:** `docs/plans/2026-10-10-gpu-world-design.md`

## Global Constraints

- Rust 2024, 2-space indentation, no `unwrap`/`expect`/`panic!` in library code (tests may).
- Format only lines you change; never plain-rustfmt a file declaring `mod x;` children (`src/lib.rs`, any `mod.rs`): `rustfmt --edition 2024 --check` and fix only your hunks. New files may be fully formatted.
- Line endings: check `file <path>` before editing; keep each file's endings on every line (CRLF today include `src/core/scene.rs`, `src/core/render/mod.rs`, `src/core/render/pass.rs`, all skill files). New files LF.
- Equal-z order: `Entity` < `Sprite` (`render_gpu`/particles/parallax) < `Trail` (trails and lines).
- Atlas: 2048×2048 `Rgba8Unorm` pages, 1-px extruded border, images > 256 px on either side standalone, free entries unused ≥ 300 frames when full, up to 8 pages, then LRU eviction.
- Built-in pass ids after Task 5: `clear`, `raycast`, `world`, `lighting`, `unlit_world`, `transition`, `post_process`, `ui`, `composite`, `capture`.
- Fidelity: opaque pixels of unrotated sprites at whole-pixel positions and integer scales (incl. tint) match the CPU exactly.
- Breaking demos/games allowed: fix them, don't shim. One commit per task, no trailer lines, don't push.
- Done = `cargo test --workspace` 0 failed; `cargo clippy --workspace --all-targets` no new warnings in touched files; `cargo check --workspace --all-targets` builds.

## Review Focus

1. A particle (or `render_gpu` sprite) at a z between two entities draws between them — Task 5 `particle_between_entities`.
2. Many sprites from a few textures don't explode the draw-call count — Task 5 `ten_thousand_sprites_few_draws`.
3. Sprites sharing an atlas page never show a neighbour's pixels when scaled or outlined — Task 1 `no_bleeding_under_scaling`, Task 2 `outline_ignores_atlas_neighbours`.
4. A sprite whose pixels change at runtime (hot reload, regenerated sprite) shows the new pixels — Task 1 `changed_sprite_reuploads`, Task 4 keying by generation.
5. Fades (transitions) cover the whole world including trails but never the UI — Task 4 `transition_covers_trails_not_ui`.

---

### Task 1: Texture atlas

**Files:**
- Modify: `Cargo.toml` (`etagere = "0.2"`; if it can't be fetched offline, implement a shelf allocator in `atlas.rs` instead and report it)
- Create: `src/core/graphics/atlas.rs`; Modify: `src/core/graphics/mod.rs` (`mod atlas; pub(crate) use atlas::{…};`)

**Interfaces (crate-internal):**

```rust
/// What an atlas entry is keyed by.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum AtlasKey {
  /// An entity sprite: the handle's identity plus its content generation
  Sprite { handle: usize, generation: u64 },
  /// A texture uploaded by id (render_gpu, particles, parallax)
  Id(String),
}

/// Where an image lives: a page index and its rect in normalized page UVs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AtlasSlot {
  pub(crate) page: usize,
  /// u_min, v_min, u_max, v_max of the image (inside the extruded border)
  pub(crate) uv: [f32; 4],
}

/// CPU-side allocation bookkeeping (testable without a GPU).
pub(crate) struct AtlasLayout { /* pages of etagere::BucketedAtlasAllocator, entries, last-used frame */ }

impl AtlasLayout {
  pub(crate) fn new(page_size: u32, max_pages: usize) -> Self;
  /// Find a slot for `key`, allocating `width`x`height` (+2 px border) if new.
  /// `None` when the image must be standalone (larger than 256 px) or nothing can be freed.
  pub(crate) fn place(&mut self, key: &AtlasKey, width: u32, height: u32, frame: u64) -> Option<Placement>;
  /// Mark `key` used this frame.
  pub(crate) fn touch(&mut self, key: &AtlasKey, frame: u64);
}
pub(crate) struct Placement { pub(crate) slot: AtlasSlot, /// pixel origin of the bordered rect
  pub(crate) x: u32, pub(crate) y: u32, pub(crate) newly_allocated: bool }

/// GPU pages + layout + standalone textures.
pub(crate) struct TextureAtlas { /* layout, pages: Vec<wgpu::Texture + view + bind group>, standalone: HashMap<AtlasKey, (texture, view, bind group)> */ }

impl TextureAtlas {
  pub(crate) fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler) -> Self;
  /// Upload (if needed) and return where `rgba` (sRGB bytes) lives.
  pub(crate) fn slot(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, key: AtlasKey,
    width: u32, height: u32, rgba: impl FnOnce() -> Vec<u8>, frame: u64) -> Option<AtlasSlot>;
  pub(crate) fn bind_group(&self, page: usize) -> Option<&wgpu::BindGroup>;
  pub(crate) fn end_frame(&mut self, frame: u64); // housekeeping
}
```

Pages are `Rgba8Unorm` (raw sRGB bytes), usage `TEXTURE_BINDING | COPY_DST`. A placed image is written with a 1-px border that repeats its edge pixels (corners repeat the corner pixel). Standalone textures use the same format and slot semantics (`page` indexes past the atlas pages, or a separate enum - your choice, documented).

- [ ] **Step 1: Write the failing tests** (unit tests in `atlas.rs`; GPU ones use `crate::core::testing::shared_gpu()` and return early on error):
  - `same_key_gets_the_same_slot` / `different_keys_get_disjoint_rects` (rects of 50 random-sized placements never overlap, borders included)
  - `large_images_are_standalone` (257×10 → `None` from `place`)
  - `full_page_frees_entries_unused_for_300_frames` (fill a small layout, e.g. `new(128, 1)`; entries last touched ≥ 300 frames ago are reused; recently touched ones keep their slot)
  - `new_pages_before_eviction_then_lru` (with `max_pages = 2`: second page used before any eviction; when both are full of recent entries, the least recently touched is evicted)
  - `changed_sprite_reuploads` (same handle, generation 1 → 2 gives a new placement, `newly_allocated`)
  - GPU `border_repeats_edge_pixels` (upload a 2×2 image, read the page back with `crate::core::capture::read_texture_rgba` or a copy, check the 4×4 bordered block)
  - GPU `no_bleeding_under_scaling` is in Task 2 (needs the shader).
- [ ] **Step 2:** run, watch them fail (compile errors).
- [ ] **Step 3: Implement** `AtlasLayout` (etagere `BucketedAtlasAllocator` per page; entries map key → (page, alloc id, rect, last_used); eviction as specified) and `TextureAtlas`.
- [ ] **Step 4:** run the tests; `cargo test --lib graphics::atlas`.
- [ ] **Step 5: Suite, lint, check, commit:** `feat(graphics): add a texture atlas for sprite textures`.

---

### Task 2: The sprite renderer on the atlas

**Files:**
- Modify: `src/core/graphics/sprite_gpu.rs` (textures move into `TextureAtlas`; `upload_texture`/`upload_sprite`/`has_texture`/`get_texture_bind_group` keep their meaning via `AtlasKey::Id`; instance UVs remapped into the slot at draw time; `SpriteInstance` gains `uv_bounds: [f32; 4]` + vertex attribute; batches key by atlas page instead of texture id)
- Modify: `src/core/shaders/core/sprite.wgsl` (sRGB-space shading, linear output, `uv_bounds` masking, quad enlargement for growing effects, `DirectionalOutline` as effect 11)
- Modify: `src/core/ecs/sprite_effects.rs` if the GPU parameter packing for `DirectionalOutline` lives there
- Test: `tests/e2e/sprite_atlas.rs` (register in `tests/e2e.rs`)

**Behaviour:**
- Sampling: the atlas is `Rgba8Unorm`, so the shader sees sRGB bytes; tint (`instance.tint`, straight alpha in [0,1] of the sRGB colour) and effects run in that space, then the RGB result is converted to linear (`srgb_to_linear`) before output, alpha unchanged; blending stays standard alpha onto the `Rgba8UnormSrgb` game texture.
- Masking: every texture sample at UV outside `uv_bounds` returns transparent.
- Growing effects (Outline, Glow, DirectionalOutline): the CPU path grew the image by the effect's padding (`SpriteEffect::apply_cpu` returns the new size) and shifted it by half the growth; the instance's quad and UVs are enlarged by the same padding (in texels → atlas UV) so the effect draws outside the sprite, centred as the CPU did.
- `DirectionalOutline`: port the CPU implementation's semantics (`sprite_effects.rs`) into the shader.

- [ ] **Step 1: Write the failing tests** (`tests/e2e/sprite_atlas.rs`, headless scenes using `render_gpu` + `GpuRenderContext::upload_sprite` / `add_sprite_instance`):
  - `no_bleeding_under_scaling`: two small solid sprites of different colours uploaded (they share a page), one drawn at 3× scale; every pixel inside its drawn rect is its own colour.
  - `outline_ignores_atlas_neighbours`: an outlined sprite next (in the atlas) to a solid opaque one: outline pixels appear around the sprite (outside its rect) and none of the neighbour's colour appears.
  - `tint_matches_the_cpu`: an opaque white sprite tinted (200, 100, 50) shows exactly `[200, 100, 50, 255]`.
  - `directional_outline_draws`: the effect produces outline pixels on the configured side only.
- [ ] **Step 2:** watch them fail.
- [ ] **Step 3: Implement** as above. Particles and parallax (they use texture ids) keep working through the atlas; their tint math now matches the CPU (sRGB space) - expected look change, note it.
- [ ] **Step 4:** run the new tests and the whole e2e suite (particles/parallax/lighting e2e must still pass; if a pixel-exact assertion depended on linear-space tinting, update it and say why).
- [ ] **Step 5: Suite, lint, check, commit:** `feat(graphics): draw GPU sprites from the atlas with CPU-matching shading` (bullets: atlas-backed textures; sRGB-space tint/effects; effects masked to the sprite and allowed to grow; DirectionalOutline on the GPU).

---

### Task 3: The z-sorted draw list and world passes

**Files:**
- Create: `src/core/render/draw_list.rs` (`DrawList`, `DrawItem`, `DrawKind`)
- Modify: `src/core/graphics/sprite_gpu.rs` (`BatchCollector` records per-instance items with `z` and submission sequence - keep `add(texture_id, has_effects, instance, z_order)`, `sprite_count`, `clear`; add a method to draw a run of instances inside an existing `wgpu::RenderPass`)
- Modify: `src/core/trail/renderer.rs` + `trail.rs` (`Trail2D.z_order: i32`, default 0, `with_z_order`; geometry grouped by `(lit, z)`; `draw_group(&self, pass: &mut wgpu::RenderPass, lit: bool, z: i32)` after a pre-pass `prepare(queue, camera…)` upload)
- Create: `src/core/render/passes/world.rs` (`WorldPass` id `world`, `UnlitWorldPass` id `unlit_world`)
- Modify: `src/core/render/frame.rs` (`FrameScene` gains `draw_list: &DrawList`), `src/core/render/mod.rs` (`pass_ids::WORLD`, `UNLIT_WORLD`, `TRANSITION` added now; old ids stay until Task 5), `src/core/render/pipeline.rs` (`RenderPipeline::world_2d()` - the new built-in list: `clear`, `raycast`, `world`, `lighting`, `unlit_world`, `post_process`, `ui`, `composite`, `capture`; `transition` arrives in Task 4)
- Modify: `src/core/engine.rs` (`pub(crate) gpu_world: bool`, default `false`; when true, `present_frame` builds the `DrawList` from the collector and trails and renders with `world_2d()`; CPU entities are not drawn on this path yet)

**Interfaces:**

```rust
/// Kind order at equal z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DrawKind { Entity, Sprite, Trail }

pub(crate) enum DrawPayload {
  /// Index into the frame's sprite instances (atlas page + effects flag resolved at draw time)
  Sprite(usize),
  /// A trail/line geometry group
  Trail { z: i32 },
}

pub(crate) struct DrawItem { pub(crate) z: i32, pub(crate) kind: DrawKind, pub(crate) sequence: u32, pub(crate) payload: DrawPayload }

pub(crate) struct DrawList { /* items (lit), unlit items, sprite instances */ }
impl DrawList {
  pub(crate) fn clear(&mut self);
  pub(crate) fn push_sprite(&mut self, kind: DrawKind, z: i32, texture: TextureRef, has_effects: bool, instance: SpriteInstance);
  pub(crate) fn push_trail_group(&mut self, lit: bool, z: i32);
  /// Items sorted by (z, kind, sequence); stable.
  pub(crate) fn sorted(&mut self) -> &[DrawItem];
}
```

(`TextureRef` = the atlas key or texture id the sprite renderer resolves; choose the smallest workable type and document it.)

- [ ] **Step 1: Write the failing tests**
  - Unit (`draw_list.rs`): `sorts_by_z_then_kind_then_sequence` (entity z1, trail z0, sprite z1, entity z1 again → trail0, entity, entity, sprite); `stable_within_kind`.
  - Unit: `consecutive_sprites_on_one_page_merge_into_one_run` (a pure helper that splits sorted items into draw runs; a trail between two sprite runs splits them).
  - Integration (`tests/integration/render_pipeline.rs`): `world_2d_order` (exact ids).
  - Headless crate-internal test (a `#[cfg(test)]` child module of `src/core/testing`, engine `gpu_world = true`): `trail_between_gpu_sprites` - two `render_gpu` sprites at z 0 and 2 overlapping, a `Line2D` at z 1 crossing them; the line shows over the z-0 sprite and under the z-2 sprite.
- [ ] **Step 2:** watch them fail.
- [ ] **Step 3: Implement.** One render pass in `WorldPass`: upload instance and trail buffers first, then iterate runs: sprite run → set sprite pipeline (effects or plain) + page bind group + draw instanced range; trail group → set trail pipeline + draw group. `UnlitWorldPass` draws unlit trail groups (sorted by z) onto the current image. `DebugStats.draw_calls` = number of draws issued by both passes on this path.
- [ ] **Step 4:** run the tests; the legacy path (switch off) must be unchanged: `cargo test --test e2e` all pass.
- [ ] **Step 5: Suite, lint, check, commit:** `feat(render): draw GPU sprites and trails from one z-sorted list`.

---

### Task 4: Entities, shadows, overlays and transitions on the GPU

**Files:**
- Create: `src/core/render/entities.rs` (collect entities into the `DrawList`: port `Engine::render`'s entity loop at `src/core/engine.rs` ~2027-2560 - render cache order, `Inactive`/`visible`, `WorldTransform`, interpolation, offset, YSort depth scale, pivot maths, `world_to_screen`, culling, whole-pixel snapping, flip, tint, alpha, effects; `DropShadow` (+ blur copies as `draw_rotated_tinted` did), `BlobShadow`, `ContactShadow` (a cached solid-ellipse atlas texture, scaled), `Tiled`, `CompositeTile` at its `z_order`)
- Modify: `src/core/asset/manager.rs` (`SpriteHandle` exposes a stable identity and a content `generation` bumped on every write to its sprite)
- Create: `src/core/render/passes/transition.rs` (`TransitionPass`, id `transition`: full-screen quad of `scene_manager.transition_overlay_color()` × `transition_overlay_alpha()`; `FrameScene` gains the overlay colour+alpha; add to `world_2d()` after `unlit_world`)
- Modify: `src/core/debug.rs` (or wherever `render_colliders`/`render_velocities`/`render_origins` live): emit lines into the unlit trail geometry at `i32::MAX` z instead of drawing into the pixel buffer, on the GPU path
- Modify: `src/core/engine.rs` (on the `gpu_world` path: collect entities into the `DrawList`, skip the CPU entity drawing and the pixel-buffer overlay)
- Create: comparison module `src/core/testing/gpu_world_comparison.rs` (`#[cfg(test)]`)

**Mapping notes:** match `Sprite::draw_rotated_tinted` / `draw_scaled_xy_flipped_tinted` placement exactly for unrotated sprites: top-left = `(screen_x as i32, screen_y as i32)`, size = `ceil(w * scale)` × `ceil(h * scale)`, nearest sampling, flip mirrors UVs; rotated sprites rotate about `top_left + (scaled_w / 2, scaled_h / 2)` (integer halves, as the CPU). Shadow before its entity (sequence), all as `DrawKind::Entity`.

- [ ] **Step 1: Write the failing tests**
  - Comparison (CPU path vs `gpu_world`, byte-identical over 3 steps, 160×120): `opaque_sprites_at_several_z`, `tiled_horizontal_and_vertical`, `composite_tile_parts` (z equal to neighbours so order matches the old "after everything" only where intended - document), `drop_shadow_without_blur`, `tinted_opaque_sprites`, `flipped_sprites`, `integer_scaled_sprites` (2×, 3×), `camera_offset_and_zoom_2x`.
  - Lasting headless (public API, `tests/e2e/gpu_world.rs`, run with the GPU path once it is default in Task 5 - write them now against the crate-internal switch in the comparison module if needed, then move): `transition_covers_trails_not_ui`, `debug_colliders_draw`, `blob_shadow_under_its_entity`.
- [ ] **Step 2:** watch them fail.
- [ ] **Step 3: Implement.** Any comparison that can't be byte-identical: find the cause; only the documented fidelity exceptions (translucent overlaps, rotation edges, fractional scale) may differ - never loosen an opaque axis-aligned comparison.
- [ ] **Step 4:** run them; legacy path unchanged.
- [ ] **Step 5: Suite, lint, check, commit:** `feat(render): draw entities, shadows and transitions on the GPU behind a switch`.

---

### Task 5: Switch over, delete the CPU entity path, fix demos

**Files:**
- Modify: `src/core/engine.rs` (remove `gpu_world` and the CPU entity drawing, the scene pixel buffer and its overlay; `Engine::render` keeps UI drawing into the UI buffer), `src/core/graphics/renderer.rs` (remove the CPU staging texture, `cpu_bind_group`, `game_blend_pipeline` if unused), `src/core/render/passes/` (delete `upload`, `gpu_sprites`, `cpu_blend`, `trails`, `unlit_trails` passes), `src/core/render/pipeline.rs` (`default_2d()` = the new list; drop `world_2d`), `src/core/render/mod.rs` (`pass_ids` = the 10 ids), `src/core/render/frame.rs` (`FrameInfo` fields that no longer apply: replace `gpu_sprites`/`trails` with `world: bool` meaning "the draw list has items"), `tests/integration/render_pipeline.rs` (orders/passes_to_run), the Task 3/4 comparison modules (delete)
- Keep `Sprite`'s CPU drawing functions that the UI still uses (`src/core/ui/immediate/context.rs`); delete only what nothing uses (`cargo check` + clippy dead-code tell you).
- Create: `tests/e2e/gpu_world.rs` (register; move Task 4's lasting tests here)
- Modify demos/games that assumed "GPU content is behind": `examples/games/bullet-hell` (parallax z already negative - verify), `examples/games/leap-king` (rain z comment now true - verify), others found by grepping `render_gpu`/`ParticleEmitter`/`GpuParallax` with entity z ≤ their z.

- [ ] **Step 1: Write the failing tests** (`tests/e2e/gpu_world.rs`, public API):
  - `particle_between_entities` (`render_gpu` sprite at z 5 between entities at z 0 and 10, all overlapping: centre shows the z-10 entity; a pixel covered only by the z-0 entity and the sprite shows the sprite)
  - `trail_between_entities`, `equal_z_order_entity_sprite_trail`, `ysort_lower_draws_in_front`, `composite_tile_honours_z`
  - `ten_thousand_sprites_few_draws` (10,000 entities from 3 textures, all visible: `DebugStats.draw_calls` ≤ 8 - read via the engine's stats API used by the overlay, or a crate-internal test if not public)
  - Plus Task 4's lasting tests moved here.
- [ ] **Step 2:** watch the ones that need the switch-over fail (on the old default).
- [ ] **Step 3: Switch, delete, fix demos.** Grep for every removed item across `src`, `tests`, `examples`, `benches` and fix callers.
- [ ] **Step 4:** full e2e + integration + lib.
- [ ] **Step 5: Suite, lint, check (examples!), commit:** `refactor(render): render the world entirely on the GPU` (bullets: one z-order for entities, sprites, particles and trails; CPU entity renderer and frame upload removed; pass list changes).

---

### Task 6: Documentation

**Files:** `src/cli/skills/{ryo-renderer,ryo-ecs-internals,ryo-particles,ryo-lighting,ryo-game-dev,ryo-api,ryo-engine-dev}.md` + their `.claude/skills/*/SKILL.md` copies (CRLF, identical pairs), `src/core/graphics/AGENTS.md`, `src/core/render/AGENTS.md`, `src/core/trail/AGENTS.md`, rustdoc on new public items (`Trail2D::with_z_order`).

- [ ] Describe: one z-order for everything with the equal-z tie rule; `Trail2D`/`Line2D` z; the atlas (games keep texture ids; big images standalone); fidelity notes (translucent overlaps blend in linear space); the new pass list; that `render_gpu` sprites no longer sit behind entities automatically (give them a lower z). Remove every "GPU sprites always sit behind CPU entities" statement (grep).
- [ ] Verify CRLF + `cmp`; `cargo test --doc`.
- [ ] Commit: `docs: describe GPU world rendering and its single z-order`.
