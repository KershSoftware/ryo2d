# Shadows Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace CPU shadow volumes with per-light GPU shadow maps in a shared depth atlas, real contact-hardening soft shadows, one light draw per camera, and an incremental occluder index.

**Architecture:** An engine-owned `OccluderIndex` holds each occluder's world bounds in a grid, updated only for what changed. Once a frame the CPU plans the shadows (the visible shadowed lights, at most 512, each a slot of four rows) and writes each casting edge as instances; one depth-tested draw fills the `Depth32Float` atlas before the camera passes. Each lit camera then draws every light in one instanced draw, the light shader sampling its slot (blocker search, then a filter whose width follows the light's size).

**Tech Stack:** Rust, bevy_ecs 0.20 (as `ryo2d::ecs`), wgpu 30 / WebGPU, WGSL.

**Spec:** `docs/specs/2026-11-11-shadows-design.md`

## Global Constraints

- Web is WebGPU only; the device asks for `Features::empty()` and `Limits::default()` (`src/core/graphics/renderer.rs` ~281-282): nothing in this plan may need more.
- Atlas: `Depth32Float`, `RENDER_ATTACHMENT | TEXTURE_BINDING`; four rows per shadowed light, rows in gather order; width from `ShadowQuality` (`Low` 256, `Medium` 512, `High` 1,024, `Ultra` 2,048), default `High`.
- Cap: `MAX_SHADOWED_LIGHTS = 512`, effective cap `min(512, max_texture_dimension_2d / 4)`; past it the lights nearest a lit camera's centre keep their shadows, the rest draw unshadowed, and the log warns once per run with the count.
- Stored value: distance from the light divided by its radius, written as the fragment's depth, in [0, 1]; the atlas is cleared to 1.0 (far).
- Only edges facing away from the light cast (today's `casts` rule, `src/core/lighting/shadow.rs:98-116`, including the spot cone test with its margin); chains cast as the polygons they bound; a closed occluder containing the light does not cast for it.
- Shadow strength (the shadow colour's alpha) applies once per light; coloured shadows let the shadow colour's colour through in proportion to the shadowed fraction.
- `Light2D::with_soft_shadows(size_px)` / `set_shadow_softness(size_px)` / `shadow_softness()`: the light source's radius in pixels, 0 = hard.
- Floors unchanged: `Floor::meets` filters occluders per light; maps are built in ground space; the light draws at its lift.
- No per-frame allocation in a lit frame over a static scene (`tests/tilemap_alloc.rs`); a frame where no occluder changed does no index work.
- `textureSampleCompareLevel` (never `textureSampleCompare`) in the light shader; the blocker search uses `textureLoad`.
- Engine code logs with `log::…!`; tests that assert on the log open `ryo2d::testing::capture_log()` before building the engine; no test lowers a log level default.
- Breaking changes are fine; update every caller in `examples/` and the skills' compiled examples.
- Commit messages: `type: description`, no Co-Authored-By or Claude-Session trailers, never name a notes file (say "module notes"), no test status.

## Review Focus

1. **A light standing exactly on an occluder's edge or vertex** (a torch placed on a wall line, a light following a character whose occluder passes through it): no NaN, no full-screen shadow, no division by zero; the light lights the side it faces. Test in Task 5 (`an_edge_through_the_light_writes_no_nan`) and Task 6 (`a_light_on_an_occluder_s_edge_lights_the_side_it_faces`).
2. **A huge occluder** (a map chain a chunk long, a 10,000 px polygon) reaching many index cells and crossing several of a light's views: found by every light it meets, drawn into each view it crosses, no gap where views meet. Tests in Task 3 (`a_big_occluder_is_found_from_every_cell_it_covers`) and Task 6 (`a_wall_across_two_views_casts_without_a_gap`).
3. **The atlas growing or changing quality mid-run** (more shadowed lights appear; the player changes shadow quality in an options menu): the next frame draws correct shadows at the new size, no stale rows, no panic, no allocation after it settles. Test in Task 7 (`the_atlas_grows_and_changes_quality_between_frames`).
4. **Occluders changing in the same frame as lights move** (a door opening while a torch passes; a map edit under a light): that frame's shadow uses the new geometry. Test in Task 8 (`an_occluder_moved_this_frame_casts_where_it_now_is`).
5. **Degenerate input** (a zero-length edge, coincident vertices, a non-finite vertex, a light with radius 0 or NaN position, an occluder scaled to zero): skipped quietly, no NaN in the light buffer. Test in Task 4 (`degenerate_edges_and_lights_plan_nothing`).

## Files

- Create: `src/core/lighting/index.rs` (the occluder index), `src/core/lighting/plan.rs` (choosing lights, slots and edges), `src/core/lighting/atlas.rs` (the atlas, its pipeline and its draw), `src/core/shaders/core/shadow_map.wgsl`, `benches/lighting.rs`, `docs/backlog/shadows.md` (the record), `examples/demos/shadows/scenes/{lantern.rs,many_lights.rs}`.
- Modify: `src/core/lighting/{mod.rs,light.rs,occluder.rs,batch.rs,renderer.rs,AGENTS.md}`, `src/core/render/batch/{gpu.rs,pipelines.rs}`, `src/core/render/passes/lighting.rs`, `src/core/shaders/core/light.wgsl`, `src/core/engine/{extract.rs,mod.rs}`, `src/core/settings/{mod.rs,file.rs,resource.rs}`, `src/lib.rs`, `tests/e2e/lighting.rs`, `tests/tilemap_alloc.rs`, `examples/demos/{lighting,shadows}`, `examples/games/leap-king`.
- Delete: `src/core/lighting/shadow.rs` (volumes), `src/core/shaders/core/shadow.wgsl`.

---

### Task 1: Measure today

**Files:** Create `benches/lighting.rs` (register in `Cargo.toml` like `benches/tilemap.rs`), `docs/backlog/shadows.md`.

**Interfaces:** Produces the bench names Task 10 guards: `one_light_over_cave`, `sixty_four_lights_quarter_visible`, `two_hundred_fifty_six_lights_in_view` (each a median frame time in µs over 120 frames after 30 warm-up, with the GPU wait, printed one per line as `name: X µs`), and the Before figures.

- [ ] **Step 1: Write the bench** on the 1024 x 1024 cave map `benches/tilemap.rs` builds (reuse its builder; move it to `benches/common/` if both need it): a 1920 x 1080 headless engine; `one_light_over_cave` (one `Light2D::point(.., 300.0).with_shadows()` following the camera, minus the same frame unlit), `sixty_four_lights_quarter_visible` (64 shadowed 200 px lights on a fixed grid over the map, the camera seeing 16), `two_hundred_fifty_six_lights_in_view` (256 shadowed 120 px lights inside the view). A header comment says what each measures and that `uptime` is recorded with every run.
- [ ] **Step 2: Run it three times** (`cargo bench --bench lighting`), `uptime` before each, to logs; flag any run at a load above 3.
- [ ] **Step 3: Write `docs/backlog/shadows.md`** with a "Before" table (bench, three medians, their median, load) and the record machine.
- [ ] **Step 4: Commit:** `test: today's shadows are measured over the cave map`

### Task 2: Shadow quality, a player graphics setting

**Files:** Modify `src/core/settings/{mod.rs,file.rs,resource.rs}`, `src/core/lighting/mod.rs`, `src/lib.rs`.

**Interfaces:**
- Produces: `pub enum ShadowQuality { Low, Medium, #[default] High, Ultra }` in `src/core/lighting/mod.rs` with `pub const fn texels(self) -> u32` (256/512/1024/2048) and `pub(crate) const fn taps(self) -> u32` (filter look-ups: 4/8/12/16); `Settings::shadow_quality(&self) -> ShadowQuality` and `Settings::set_shadow_quality(&mut self, ShadowQuality)`; `UserPreference::ShadowQuality`; saved in `system.json` under `display.shadow_quality` as `"low" | "medium" | "high" | "ultra"`; `ShadowQuality` in the prelude.

- [ ] **Step 1: Failing tests** (beside the settings tests): `shadow_quality_defaults_to_high`; `shadow_quality_round_trips_through_system_json` (set `Ultra`, write, read: `Ultra`); `an_unknown_shadow_quality_warns_and_keeps_the_default` (file value `"extreme"`: one warning in the capture, `High`); `resetting_shadow_quality_restores_the_game_s_default`.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib core::settings`: they fail to compile.
- [ ] **Step 3: Implement** following how `vsync` is stored, read (`file.rs` `take`), written and reset.
- [ ] **Step 4: Run** them: pass. Run `cargo test -p skill-examples`.
- [ ] **Step 5: Commit:** `feat: shadow quality is a player graphics setting`

### Task 3: The occluder index

**Files:** Create `src/core/lighting/index.rs`; modify `src/core/lighting/{mod.rs,occluder.rs}`, `src/core/engine/mod.rs` (insert the resource, add the system).

**Interfaces:**
- Produces (crate-internal): `#[derive(Resource, Default)] pub(crate) struct OccluderIndex`; `pub(crate) fn query(&self, min: Vec2, max: Vec2) -> impl Iterator<Item = Entity> + '_` (each entity once, in entity order, no allocation: a reused visited-mark per query generation); `pub(crate) fn bounds(&self, entity: Entity) -> Option<(Vec2, Vec2)>`; `#[cfg(test)] pub(crate) fn updates(&self) -> u64` (entries touched since creation); `pub(crate) const INDEX_CELL: f32 = 512.0`; `pub(crate) fn update_occluder_index(...)` (a system in `PostUpdate` after transform propagation); `LightOccluder2D::placed_bounds(&self, transform: &GlobalTransform) -> Option<(Vec2, Vec2)>` (`None` for non-finite).
- The update reads `Added`/`Changed<GlobalTransform>`/`Changed<LightOccluder2D>`, `RemovedComponents<LightOccluder2D>`, and both `Disabled` transitions (an observer of `Add<Disabled>` and `Remove<Disabled>` on occluder entities, queuing them); a disabled occluder (`is_enabled() == false` or the ECS's `Disabled`) has no entry. An interpolated occluder's bounds are the union of its placed box at its `PreviousGlobalTransform` and at its `GlobalTransform`.

- [ ] **Step 1: Failing tests:** `an_added_occluder_is_found_where_it_stands`; `a_moved_occluder_leaves_its_old_cells`; `a_removed_or_despawned_occluder_is_gone`; `disabling_and_re_enabling_an_occluder_is_followed` (both `set_enabled` and the ECS's `Disabled`); `a_big_occluder_is_found_from_every_cell_it_covers` (a 10,000 px polygon: found by a query box in each of its corners and its middle, once each); `a_static_scene_updates_nothing` (`updates()` does not change over 10 frames with nothing moving); `a_rotating_interpolated_occluder_is_found_along_its_swing` (a long thin occluder rotated 90° in one fixed step: found by a query at the midpoint of its swing); `map_chunk_chains_are_indexed_and_follow_an_edit` (a small map; an edit's rebuilt chunk is found that frame).
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib core::lighting::index`: fail.
- [ ] **Step 3: Implement.** A `HashMap<IVec2, Vec<Entity>>` of cells, an `EntityHashMap` of entries (bounds and covered cell range), cells' vectors reused.
- [ ] **Step 4: Run:** pass. Run `cargo test -p ryo2d --lib core::tilemap::occluders`.
- [ ] **Step 5: Commit:** `feat: occluders are indexed by where they are, updated only when they change`

### Task 4: Planning the shadows (CPU)

**Files:** Create `src/core/lighting/plan.rs`; modify `src/core/lighting/{mod.rs,light.rs}`.

**Interfaces:**
- Consumes: `OccluderIndex::query`, `LightOccluder2D::placed`, `Floor::meets`, `FrameLight` (`lighting/batch.rs`).
- Produces (crate-internal):
  - `#[repr(C)] #[derive(Pod, Zeroable, Clone, Copy)] pub(crate) struct ShadowEdge { a: [f32; 2], b: [f32; 2], light: [f32; 2], radius: f32, row: u32 }` (24 + 8 bytes; `row` is the atlas row: slot x 4 + view).
  - `pub(crate) struct ShadowPlan { pub(crate) slots: Vec<Option<u32>>, pub(crate) edges: Vec<ShadowEdge>, pub(crate) rows: u32 }` reused across frames; `slots[i]` is light `i`'s slot.
  - `pub(crate) fn plan_shadows(plan: &mut ShadowPlan, lights: &[FrameLight], views: &[(RenderLayers, Vec2 /*min*/, Vec2 /*max*/, Vec2 /*centre*/)], index: &OccluderIndex, occluders: &Query<(&GlobalTransform, &LightOccluder2D, Option<&Floor>, Option<&DrawLift>)>, cap: u32) -> u32` returning the count of shadowed lights dropped by the cap.
  - `pub(crate) const MAX_SHADOWED_LIGHTS: u32 = 512;`
  - Views: 0 right (-45°..45°), 1 down, 2 left, 3 up (y down, angles as `atan2(dy, dx)`), an edge entering each view it spans (from its end points' angles, wrapping across ±180°).
- Rules: a light gets a slot if it casts shadows, `light_instance` would draw it, and its box (position plus or minus radius, plus its lift) meets a view it shares layers with; past the cap, keep the lights with the smallest distance to the nearest sharing view's centre, ties by index. Its edges: index query of its box (ground space), then floor, then enabled and finite, then containment (closed occluders only, point-in-polygon on the placed vertices), then per edge `casts` (moved from `shadow.rs` with its cone margin), then a length above 1e-4.

- [ ] **Step 1: Failing tests:** `an_unseen_light_gets_no_slot`; `a_light_seen_by_two_cameras_gets_one_slot`; `only_edges_facing_away_cast`; `a_chain_casts_as_the_polygon_it_bounds` (same edges as the closed polygon minus its closing edge's); `a_closed_occluder_round_the_light_casts_nothing_for_it` (and a chain round it still casts); `an_edge_spanning_two_views_is_planned_in_both`; `a_spot_skips_edges_outside_its_cone`; `a_floor_light_reads_only_its_floor_s_occluders`; `past_the_cap_the_nearest_lights_keep_their_slots` (600 lights: 512 slots, the 88 farthest none, return value 88); `degenerate_edges_and_lights_plan_nothing` (zero-length edge, coincident vertices, non-finite vertex, radius 0, NaN position: no slot or no edge, no panic); `planning_a_static_frame_allocates_nothing` (the plan's vectors keep their capacity frame to frame: capacity unchanged after the second frame).
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib core::lighting::plan`: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run:** pass.
- [ ] **Step 5: Commit:** `feat: the visible shadowed lights are planned once a frame, each with its casting edges`

### Task 5: The atlas and the shadow map draw

**Files:** Create `src/core/lighting/atlas.rs`, `src/core/shaders/core/shadow_map.wgsl`; modify `src/core/lighting/renderer.rs`, `src/core/render/batch/gpu.rs` (encode before the camera passes), `src/core/engine/extract.rs`.

**Interfaces:**
- Produces: `pub(crate) struct ShadowAtlas` with `ensure(&mut self, device: &Device, quality: ShadowQuality, rows: u32)` (recreate when the width or needed height changes; height = rows rounded up to a multiple of 64), `encode(&mut self, encoder: &mut CommandEncoder, queue: &Queue, edges: &[ShadowEdge])` (clear depth to 1.0; one instanced draw, 6 vertices per instance), `view(&self) -> &TextureView`, `compare_sampler(&self)`, `width(&self) -> u32`; `#[cfg(test)] read_rows(&self, ...) -> Vec<f32>` (a readback for tests).
- Shader: per instance, the vertex stage computes the edge's two points in the instance's view (rotate by the view's angle, so the view looks along +x), maps each to `x = y_local / x_local` (the tangent, -1..1 across the view), with clip `w = x_local` and a near plane at `0.5` px so an edge through or behind the light is clipped, and draws a quad one texel tall at the row's centre (row `r` maps to clip y at its texel centre). The fragment writes `frag_depth = distance(world point, light) / radius` (clamped to [0, 1]); depth test `Less`, depth write on. Ends padded by 0.25 texel.

- [ ] **Step 1: Failing GPU tests** (beside the atlas, skip without a GPU, using the headless GPU helpers): `an_edge_writes_its_distance_in_its_view_s_row` (a light at origin radius 100, an edge from (50,-10) to (50,10): view 0's row has ~0.5 at its centre texels and 1.0 at its ends); `the_nearest_edge_wins` (two parallel edges at 30 and 60: 0.3); `an_edge_across_two_views_writes_both_rows` (an edge from (40,-60) to (40,60) at 45° boundaries: values in view 0 and the neighbouring views' ends, no texel left at 1.0 at the seam); `an_edge_through_the_light_writes_no_nan` (an edge through the origin: every value finite, in [0,1]); `each_light_writes_only_its_own_rows` (two slots: the second's rows untouched by the first's edges).
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib core::lighting::atlas`: fail.
- [ ] **Step 3: Implement**; the plan (Task 4) runs in `present_frame` after the lights are gathered and the atlas encodes once in batch preparation, before any camera pass, only when `plan.rows > 0`.
- [ ] **Step 4: Run:** pass. Run the WGSL validation test the shaders already have (`rg -n "fn .*shaders_validate|naga" src/core/shaders`).
- [ ] **Step 5: Commit:** `feat: shadowed lights' maps are drawn into a depth atlas once a frame`

### Task 6: Lights sample the atlas, in one draw

**Files:** Modify `src/core/shaders/core/light.wgsl`, `src/core/lighting/batch.rs`, `src/core/render/batch/{gpu.rs,pipelines.rs}`, `src/core/render/passes/lighting.rs`, `tests/e2e/lighting.rs`, `tests/e2e/render_pipeline.rs`. Delete `src/core/lighting/shadow.rs`'s volumes and `src/core/shaders/core/shadow.wgsl`.

**Interfaces:**
- Consumes: `ShadowAtlas` (Task 5), `ShadowPlan.slots` (Task 4).
- The light instance carries its slot in `uv_rect[1]` (`-1.0` for none), its size in pixels in `uv_rect[2]`, its radius in `uv_rect[3]`; the light pipeline's bind group gains the atlas view and the comparison sampler. `LightStep` keeps only `Add`; `light_blend`'s other steps go; `gpu.rs` draws each lit camera's lights in one instanced draw.
- Hard shadows in this task: one `textureSampleCompareLevel` at the pixel's view and row, compare value `distance / radius - bias` where `bias = 1.5 x (texel's world width at that distance) / radius`; `shadowed` = 1 - compare result; the light adds `light x (1 - strength x shadowed) + shadow_colour.rgb x light x strength x shadowed`.

- [ ] **Step 1: Failing e2e tests** (replace the volume-based ones; keep the rest of `tests/e2e/lighting.rs`): `a_hard_shadow_falls_behind_an_occluder` (behind: within 0.05 of ambient-only; beside the shadow: unchanged; edge width at most 2 px, 256 px light, zoom 1, `High`); `a_shadow_darkens_only_its_own_light` (kept); `strength_applies_once` (two 50% occluders in a row: behind both, the light is held back by 0.5 within 0.06; replaces `overlapping_shadows_combine_instead_of_cutting_twice`); `a_coloured_shadow_lets_its_colour_through` (kept, values re-derived); `a_carried_light_lights_round_its_bearer` (a light inside a closed 32 px occluder: lit all round at 40 px; a second light's shadow of that occluder still falls); `a_light_on_an_occluder_s_edge_lights_the_side_it_faces`; `a_wall_across_two_views_casts_without_a_gap`; `a_floor_light_and_its_shadows_draw_lifted` and `an_occluder_shadows_what_is_behind_it` (kept, `render_pipeline.rs`); `two_cameras_share_one_atlas` (two lit cameras: one atlas encode in the frame, a counter in the test build, both shadowed).
- [ ] **Step 2: Run** `cargo test -p ryo2d --test e2e lighting`: the new ones fail.
- [ ] **Step 3: Implement**; remove `push_shadow_volumes`, its triangles, the mask steps and their pipelines, `shadow.wgsl`, and `LightOccluder2D::transform_vertices`; move `casts` into `plan.rs` (Task 4 already did, if so delete the old copy).
- [ ] **Step 4: Run** `cargo test -p ryo2d --test e2e`, `cargo test -p ryo2d --lib core::lighting core::render core::tilemap::occluders`: pass.
- [ ] **Step 5: Commit:** `feat: every light draws in one pass and reads its shadow from the atlas`

### Task 7: Soft shadows with contact hardening

**Files:** Modify `src/core/shaders/core/light.wgsl`, `src/core/lighting/light.rs`, `tests/e2e/lighting.rs`, `src/core/lighting/atlas.rs` (quality changes).

**Interfaces:**
- `Light2D::with_soft_shadows(size_px: f32) -> Self` (turns shadows on), `set_shadow_softness(&mut self, size_px: f32)`, `shadow_softness(&self) -> f32`; a size that is not finite or below 0 is 0. Doc comments say: the radius of the light's source in pixels; 0 is a hard edge; a shadow's edge widens with distance from the occluder.
- Shader: blocker search with `taps(quality) / 2` `textureLoad`s across an arc of `size / distance_to_light` radians (capped at 0.35 rad), averaging the blockers nearer than the pixel; none: lit. Penumbra arc = `size x (d - d_blocker) / d_blocker / d`, capped at 0.35 rad; filter with `taps(quality)` `textureSampleCompareLevel`s spread evenly over it, each choosing its own view and row (wrapping view 3 to 0) and sampling at the row's centre. Size 0: one comparison (Task 6's path).

- [ ] **Step 1: Failing e2e tests:** `a_soft_shadow_hardens_where_it_meets_its_occluder` (size 12, a 256 px light: the 10%-90% penumbra width measured at 20 px behind the occluder is smaller than at 120 px behind, and the hard light's is at most 2 px at both); `a_soft_edge_moves_smoothly_with_its_light` (move the light 0.25 px eight times: the measured edge position changes monotonically, each step under 1 px, no pixel flips lit-dark-lit); `no_light_leaks_through_a_map_wall` (a row of touching map chunk chains and, separately, touching closed rectangles: along the far side, every pixel within 0.03 of fully shadowed, with the light at five positions including ones aligned with a joint); `no_acne_on_lit_ground` (lit open ground beside an occluder within 0.02 of the same ground with the occluder removed); `the_atlas_grows_and_changes_quality_between_frames` (1 shadowed light, then 300, then quality `Low` then `Ultra` across frames: each frame's shadow present and correct, no panic, no allocation in the frames after the last change).
- [ ] **Step 2: Run:** fail.
- [ ] **Step 3: Implement**; update `examples/` and skill examples that pass 0-to-1 softness (leap-king's `LAMP_SHADOW_SOFTNESS` becomes a size in pixels chosen by eye in Task 9; for now 6.0).
- [ ] **Step 4: Run** `cargo test -p ryo2d --test e2e lighting`, `cargo test -p skill-examples`: pass.
- [ ] **Step 5: Commit:** `feat: soft shadows have penumbrae that widen with distance from their occluder`

### Task 8: The frame without the old gather; the cap and allocations

**Files:** Modify `src/core/engine/extract.rs` (remove `occluders_to_draw` and its query; the plan reads the index), `tests/tilemap_alloc.rs`, `tests/e2e/lighting.rs`.

- [ ] **Step 1: Failing tests:** `an_occluder_moved_this_frame_casts_where_it_now_is` (move an occluder and a light in the same `update`: the frame's shadow is at the new place); `past_512_shadowed_lights_the_nearest_keep_their_shadows` (600 shadowed lights in view: 512 shadowed by pixel sampling of a known subset, the farthest 88 unshadowed, one warning in the capture over 10 frames); `a_lit_frame_with_64_shadowed_lights_allocates_nothing` (in `tests/tilemap_alloc.rs`, after warm-up); the existing `tests/tilemap_alloc.rs` lit cases kept.
- [ ] **Step 2: Run:** fail where expected.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace`: pass.
- [ ] **Step 5: Commit:** `refactor: the frame gathers no occluders, and shadows past the cap are dropped nearest-first`

### Task 9: Demos and leap-king

**Files:** Modify `examples/demos/shadows/scenes/{hard_shadows.rs,soft_shadows.rs,placed_occluders.rs,colored_shadows.rs,mod.rs}`, create `examples/demos/shadows/scenes/{lantern.rs,many_lights.rs}`, modify `examples/demos/lighting/main.rs`, `examples/games/leap-king/src/{scenes/game.rs,constants.rs}`, add `examples/demos/shadows/tests/pages.rs` if absent.

- [ ] **Step 1: Tests** in `examples/demos/shadows/tests/pages.rs`: every page builds and steps 120 headless frames with no warning in its capture; the soft page's size control changes `shadow_softness`; the lantern page's carried light is inside its bearer's occluder.
- [ ] **Step 2: Build the pages** to the demo standard (`examples/demos/AGENTS.md`): soft page with a size slider and a line saying what contact hardening is; lantern page (a character carrying a lantern among pillars lit by a second light); many-lights page (64 shadowed torches over a tile-map maze, the count on screen, the quality setting on a key); the lighting demo's lamp `.with_shadows()`.
- [ ] **Step 3: Leap-king:** platforms get `LightOccluder2D::rect` sized from `PLATFORM_OCCLUDER_HEIGHT`; its lamps' size chosen by eye; its tests pass.
- [ ] **Step 4: Run** `cargo test -p shadows-demo -p lighting-demo -p leap-king` (the packages' names as in their `Cargo.toml`), `./dev.sh demos:tour shadows lighting` to a log, and read each captured page.
- [ ] **Step 5: Commit:** `feat: the demos show soft shadows, a carried lantern and many shadowed lights, and leap-king's lamps cast`

### Task 10: Measure and guard

**Files:** Modify `benches/lighting.rs`, `docs/backlog/shadows.md`.

- [ ] **Step 1: Guards** (`benches/guards` pattern, as `benches/particles.rs` uses) with Task 1's medians as constants and a comment naming their source: `one_light_over_cave` no slower than Before; `sixty_four_lights_quarter_visible` at least 4x faster; `two_hundred_fifty_six_lights_in_view` under 16,600 µs.
- [ ] **Step 2: Run three times**, `uptime` first, to logs. On a miss report the figures and where the time goes; never loosen a guard.
- [ ] **Step 3: Record** "After" and "Limits" in `docs/backlog/shadows.md` (the envelope; the 512 cap; a light inside a map's wall; one light image per storey; no directional light; no adaptive resolution).
- [ ] **Step 4: Commit:** `test: shadows are guarded against today's figures and recorded`

### Task 11: Docs

**Files:** `.claude/skills/ryo-lighting/SKILL.md`, `.claude/skills/ryo-config-save/SKILL.md`, `.claude/skills/ryo-api/SKILL.md` (each one minor version bump), `src/core/lighting/AGENTS.md`, `src/core/render/AGENTS.md`, `docs/architecture/overview.md`, `docs/backlog/follow-ups.md`.

- [ ] **Step 1: Write** per the spec's Documentation section, checking every name and default against the code; correct "lighting stays on after a scene change" to what `SceneCleanup::LIGHTING` does; remove the follow-ups this branch fixed (true penumbrae, the doubled shadow's darkening, the lighting-stays-on doc bug, leap-king's unchecked shadows if checked in Task 9).
- [ ] **Step 2: Run** `cargo test -p skill-examples`, `cargo doc --no-deps -p ryo2d` (only the known `padded.rs` warnings), and `rg --hidden -i "push_shadow_volumes|shadow volume|transform_vertices|softness \(0" .claude src docs/architecture`: nothing stale.
- [ ] **Step 3: Commit:** `docs: skills, module notes and the overview describe shadow maps and soft shadows`
