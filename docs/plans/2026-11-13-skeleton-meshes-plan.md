# Skeleton meshes (stage 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add mesh attachments to skeletons: textured triangle meshes whose vertices ride one bone or are weighted to several, with keyed per-vertex deformation, drawn exactly like the skeleton's region quads.

**Architecture:** The draw list gains a triangles item of the entity kind, drawn with a mesh shader built from the sprite shader's sampling and tint, so it sorts and looks like a sprite. The skeleton's drawer walks the draw order once, emitting region quads and CPU-skinned mesh triangles in order from the same interpolated bone matrices. Deform offsets live in the pose beside slot colours, are keyed per (slot, attachment name), and blend under the same mixing rules as every other timeline.

**Tech Stack:** Rust, bevy_ecs 0.20 (`ryo2d::ecs`), wgpu 30 and WGSL, RON (serde).

**Spec:** `docs/specs/2026-11-12-skeleton-design.md`. Its section "Meshes: details settled at planning (2026-11-13)" is this stage's design; read it first. The module notes `src/core/skeleton/AGENTS.md` record every rule of the merged core, each naming its test. An engine seam map for this stage is at `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/skeleton-mesh-seams.md`; it was made by reading, so verify each line before relying on it.

## Global Constraints

- **Clean room:** no task reads, recalls or reproduces Esoteric's Spine runtime source. Rules come from Spine's public user guide and format documents, geometry and first principles. Where those are silent, choose a rule, write it in the module notes and pin it with a test. Reviewers judge the same way.
- **Notes and tests:** every rule in `src/core/skeleton/AGENTS.md` names the test that pins it. Keep that true for every rule added or changed. For a test of behaviour that already holds, break the rule once to prove the test.
- **Coordinates:** y down, positive rotation clockwise. Files give angles in degrees; data holds radians.
- **Limits:** a mesh has at most 65,535 vertices (indices are `u16` in the data). A non-finite value in a file fails to load.
- **Allocation:** no per-frame allocation in steady state, with meshes drawn and deforms playing.
- **Drawing:** a mesh draws exactly like a region quad of the same image: tint, alpha, flip and mirroring, z, `YSort`, storey and `DrawLift`, the `lit` flag, drop shadow, interpolation.
- **Repository rules** (`AGENTS.md`): no `unwrap` on reachable library paths; LF line endings; British spelling in docs.
- **Skills:** no stage, task or history wording. Exactly one minor-version bump per skill per branch against `main` (`ryo-skeleton` goes 1.0.0 to 1.1.0). Every Rust example in a skill compiles under `tests/skill-examples`, and every file example loads.
- **Commits:**
  - Stage files by name. Never `git add -A`. Never stage `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`.
  - No Co-Authored-By or Claude-Session trailers.
  - Say "module notes", never `AGENTS.md`.
  - No test status in messages.
  - Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`.
- **Checkout:** work in the main checkout on branch `skeleton-meshes`. No worktrees.
- **Disk:** stop and report if free disk drops below 40 GiB (`df -h /`).
- **Measurements:** record `uptime` before any measurement and flag a load above 3.
- **Memory:** keep every test's memory small and bounded.
- **Logs** go under `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/`.
- **Before each commit:** `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo clippy --target wasm32-unknown-unknown -p ryo2d --lib -- -D warnings`. Do not commit unrelated formatting changes (`cargo fmt` touches `src/core/logging/mod.rs`; leave it).

## Review Focus

1. **A reload that changes a mesh's vertex count while a deform plays** (an artist re-cuts a mesh while the game runs): no panic and no read past a buffer. The mesh draws its setup shape, the deform is skipped with one warning, and the animation keeps playing. Test in Task 4: `a_reload_that_changes_a_mesh_s_vertex_count_skips_its_deform`.
2. **A malformed mesh** (a triangle index past the vertices, weights that sum to zero, UVs and vertices of different counts, a zero-area triangle, a UV outside 0 to 1): the first three fail to load naming the attachment; a zero-area triangle draws nothing and makes no NaN; a UV outside the region samples nothing from a neighbouring region. Tests in Task 2 (`a_mistake_in_a_mesh_names_its_attachment`) and Task 1 (`a_mesh_uv_outside_its_region_reads_no_neighbour`).
3. **A mesh on a flipped skeleton, and under a bone with negative scale** (a character turning round): the triangles' winding reverses, no face is culled, and the pixels mirror as the regions do. Test in Task 3: `a_mesh_mirrors_with_its_skeleton_and_under_a_mirrored_bone`.
4. **A skin swap in mid-deform** to a mesh of another vertex count, or to a region under the same attachment name: the deform is not applied to the wrong attachment, nothing is drawn wrong, and there is no warning on every step. Test in Task 4: `a_skin_swap_to_another_shape_skips_the_deform_without_repeating_the_warning`.
5. **A skeleton that is all meshes, and a mesh between two regions on different images**: the draw order holds across the run splits, the drop shadow covers every part, and culling uses the skinned bounds. Test in Task 3: `meshes_and_regions_keep_the_draw_order_across_images`.

---

### Task 1: Triangles in the entity order, with the sprite's look

**Files:**
- Modify:
  - `src/core/render/draw_list.rs`: `push_triangles_as`.
  - `src/core/engine/extract.rs`: clear the triangle stream before the entity drawer, and hand it to `EntityDrawer::collect`.
  - `src/core/render/entities.rs`: the `Emitter` gets the triangle stream.
  - `src/core/render/batch/{key.rs,pipelines.rs,gpu.rs,plan.rs}`: `BatchShader::Mesh`.
  - `src/core/shaders/compose.rs`.
  - `src/core/render/AGENTS.md`, `src/core/shaders/AGENTS.md`.
- Create: `src/core/shaders/core/mesh.wgsl`.
- Test: `src/core/testing/gpu_world.rs` (a crate-internal scene that pushes a mesh item) and `tests/e2e/gpu_world.rs`.

**Interfaces:**
- Produces:
  - `DrawList::push_triangles_as(&mut self, kind: DrawKind, lit: bool, z: i32, only_camera: Option<Entity>, triangles: DrawTriangles)`. `push_triangles` keeps its behaviour by calling it with `DrawKind::Trail`.
  - `BatchShader::Mesh`: a textured triangle shader over the existing `Vertex2D` stream, whose output for a quad of an image equals the sprite shader's for that image.
    - It composes the sprite path's sampling and tint includes (`sample_masked`, `finish` in `shaders/include/tint.wgsl`).
    - The vertex's `uv` is a fraction of the IMAGE.
    - `color` is the tint, as a sprite instance's tint is given.
    - `bounds` is set at `prepare` to the image's atlas slot, as the textured-line path sets it.
    - `params` carries the clamp rect of the region, as image fractions, so a UV outside it reads nothing.
    - State the lanes in `shaders/AGENTS.md`.
  - A crate-internal way for a drawer inside `EntityDrawer::collect` to append vertices and indices to the frame's triangle stream and push an entity-kind item for them. Task 3 uses it. Name it in the report.

- [ ] Step 1: Write the e2e pixel tests:
  - `a_two_triangle_mesh_draws_as_the_sprite_of_its_image`: every pixel equal to the sprite's, for an opaque image and one with soft alpha edges, tinted and half-transparent.
  - `a_mesh_item_sorts_between_two_entities_at_one_z`: by submission, not after them.
  - `a_lit_mesh_item_is_lit_and_an_unlit_one_is_not`.
  - `a_mesh_uv_outside_its_region_reads_no_neighbour` (Review Focus 2): a region inside a packed image, with a UV past its edge.

  Run them: they fail.
- [ ] Step 2: Implement it. Make the tests pass. Run `cargo test -p ryo2d --test e2e`, and check the tour is unchanged (`./dev.sh demos:tour`, frames compared with a run made before the change).
- [ ] Step 3: Commit with `feat: the draw list takes triangles in the entity order, drawn with the sprite's look`.

### Task 2: Mesh attachments in the data and the file

**Files:**
- Modify:
  - `src/core/skeleton/{attachment.rs,validate.rs,builder.rs,mod.rs,AGENTS.md}`.
  - `src/core/asset/loaders/skeleton.rs`.
  - `src/lib.rs` (prelude).
  - Every `match` on `Attachment`, so each compiles with a mesh ignored until Task 3: `draw.rs`, `debug.rs`, `component.rs`, `src/core/testing/gpu_world.rs`.

**Interfaces:**
- Produces, in-crate names (the prelude prefixes them `Skeleton…` as it does the others):
  - `Attachment::Mesh(MeshAttachment)`.
  - `pub struct MeshAttachment { region: usize, uvs: Vec<Vec2>, triangles: Vec<u16>, hull: u16, vertices: MeshVertices, color: Color }`.
  - `pub enum MeshVertices { Unweighted(Vec<Vec2>), Weighted(WeightedVertices) }`.
  - `WeightedVertices` holds a flat `Vec<Influence>` and each vertex's range in it. Its accessors are `vertex_count()`, `influences(vertex) -> &[Influence]`, and `positions()` (the total count of influence positions, which a deform key must match).
  - `pub struct Influence { bone: BoneId, x: f32, y: f32, weight: f32 }`.
  - `MeshAttachment::vertex_count()` and `MeshAttachment::positions()`. For an unweighted mesh, `positions()` is the vertex count.
- Validation, each rule naming the skin, slot and attachment in its error:
  - the region is in range;
  - there is one UV per vertex;
  - the triangles are a multiple of three, at least one, with every index below the vertex count;
  - `hull` is 0, or between 3 and the vertex count;
  - there are at most 65,535 vertices;
  - every value is finite;
  - every influence's bone exists and its weight is not negative;
  - every vertex's weights sum above zero (they are normalised when built).
- File format (angles do not occur; positions are in pixels):

```ron
"cape": Mesh(
  region: "cape",
  uvs: [(0, 0), (1, 0), (1, 1), (0, 1)],
  triangles: [0, 1, 2, 0, 2, 3],
  hull: 4,
  vertices: Weighted([
    [("torso", -20, 0, 1)],
    [("torso", 20, 0, 1)],
    [("cape_tip", 20, 0, 0.75), ("torso", 20, 60, 0.25)],
    [("cape_tip", -20, 0, 1)],
  ]),
),
"flag": Mesh(region: "flag", uvs: [..], triangles: [..], vertices: Unweighted([(0, 0), (64, 0), (64, 40), (0, 40)])),
```

- [ ] Step 1: Write the tests:
  - In `validate.rs`, one test per rule above, each asserting the full item and the reason.
  - In the loader:
    - `a_mesh_attachment_loads_weighted_and_unweighted`;
    - `a_mistake_in_a_mesh_names_its_attachment` (Review Focus 2: an index past the vertices, weights summing to zero, UVs and vertices of different counts, an unknown bone in an influence);
    - the every-feature file and its builder twin extended with both kinds of mesh.

  Run them: they fail.
- [ ] Step 2: Implement it. Make the tests pass. The documented example in the loader's rustdoc gains a mesh, and still loads.
- [ ] Step 3: Commit with `feat: skeleton data and files hold mesh attachments`.

### Task 3: Skinning and drawing meshes

**Files:**
- Modify:
  - `src/core/skeleton/{draw.rs,debug.rs,component.rs,AGENTS.md}`.
  - `src/core/render/entities.rs` (`collect_skeleton`).
  - `tests/tilemap_alloc.rs`: add the new drawer functions to the names the skeleton allocation test counts.
- Test: `tests/e2e/skeleton.rs`.

**Interfaces:**
- Consumes: Task 1's entity-kind triangles and its way to append to the triangle stream; Task 2's mesh data.
- Produces:
  - **One walk of the draw order** emits a skeleton's parts in order: region quads into runs as today, and each mesh as an entity-kind triangles item between them. A run ends before a mesh and a new one starts after it.
  - **Skinning** is on the CPU, from the bone matrices the regions use (`Skeleton::drawn_bone`), under the entity's `render_global`, flip and lift. Each bone a mesh uses is blended once per frame, not once per vertex.
  - **The item's culling box** is the skinned vertices' bounds.
  - **Colour** is slot × attachment × the entity's tint and alpha, clamped and converted as the regions' is.
  - **A drop shadow** draws each mesh's silhouette with the shadow's colour, offset and scale, as it draws each region's.
  - **A stale pose or a missing image** draws no mesh, as it draws no region.
  - **Under a game's own sprite shader**, regions take the shader and meshes draw plain. This is recorded in the notes and the rustdoc.
  - **`debug:skeletons`** draws a mesh's hull and its triangles.
  - `Skeleton::attachment(slot)` and the other readers keep working with a mesh shown.

- [ ] Step 1: Write the e2e pixel tests:
  - `an_unweighted_mesh_draws_where_its_bone_puts_it`.
  - `a_weighted_mesh_bends_between_two_bones`: a strip weighted from one bone to another, with the middle vertex half way.
  - `a_mesh_draws_as_the_region_of_its_image`: a two-triangle mesh against a region attachment of the same region, with equal pixels.
  - `meshes_and_regions_keep_the_draw_order_across_images` (Review Focus 5), with a skeleton that is all meshes among its cases.
  - `a_mesh_follows_ysort_light_and_floor_as_its_regions_do`.
  - `a_mesh_mirrors_with_its_skeleton_and_under_a_mirrored_bone` (Review Focus 3).
  - `a_mesh_is_interpolated_between_steps`.
  - `a_mesh_casts_the_drop_shadow_of_its_silhouette`.
  - `a_mesh_off_the_screen_is_culled_by_its_skinned_bounds`.

  Run them: they fail.
- [ ] Step 2: Implement it. Make the tests pass. Run `cargo test -p ryo2d --test e2e` and the skeleton allocation test.
- [ ] Step 3: Commit with `feat: skeletons draw meshes skinned to their bones`.

### Task 4: Deform keys

**Files:**
- Modify:
  - `src/core/skeleton/{animation.rs,validate.rs,builder.rs,apply.rs,state.rs,pose.rs,component.rs,draw.rs,AGENTS.md}`.
  - `src/core/asset/loaders/skeleton.rs`.
  - `src/lib.rs`.
- Test: in those files, plus `tests/skeleton.rs` and `tests/e2e/skeleton.rs`.

**Interfaces:**
- Consumes: Tasks 2 and 3.
- Produces:
  - **`Timeline::Deform { slot: SlotId, attachment: Arc<str>, frames: Vec<Frame<Vec<f32>>> }`.** Each key holds two floats per influence position (x, y), or is empty for the setup shape. The duplicate check and the mixing cover rule treat (slot, attachment name) as the property: two deform timelines for one slot and name fail, and two for different names do not clash.
  - **Validation.** The slot exists. Every key is empty or has the same length. Values are finite. Keys follow the time rules of other timelines. That the length matches the mesh is checked when applied, not at load, since skins resolve at run time.
  - **The pose** holds, per slot, the current deform offsets and the last step's (for drawing between steps), sized without allocating in a steady step.
  - **Applying.**
    - Replace moves the offsets toward the key's by alpha.
    - Add adds the key's times alpha.
    - Between two keys the offsets interpolate by the curve.
  - **When it applies.** Only while the slot shows that attachment name and the attachment is a mesh whose `positions() × 2` equals the key length. Otherwise it is skipped, with one warning per skeleton per animation.
  - **Reset.** The offsets return to zero when the slot's attachment changes.
  - **Skinning** (Task 3) adds the blended offsets to each influence position before weighting.
  - **File format**, in an animation's slot block:

```ron
slots: {
  "banner": (deform: { "flag": [(0.0, []), (0.5, [0, -6, 0, -6, 0, 6, 0, 6], Bezier(0.25, 0, 0.75, 1)), (1.0, [])] }),
},
```

- [ ] Step 1: Write the tests:
  - **Validation and loader:**
    - `a_deform_timeline_loads_and_matches_the_builder_s`;
    - `two_deform_timelines_for_one_slot_and_name_fail`;
    - `deform_keys_of_different_lengths_fail`.
  - **Applying and state:**
    - `a_deform_key_moves_its_vertices_by_alpha`;
    - `an_add_deform_adds_to_the_layer_below`;
    - `a_crossfade_blends_two_deforms_without_a_dip`;
    - `a_deform_resets_when_the_slot_s_attachment_changes`;
    - `an_empty_key_is_the_setup_shape`.
  - **In the world (`tests/skeleton.rs`):**
    - `a_reload_that_changes_a_mesh_s_vertex_count_skips_its_deform` (Review Focus 1);
    - `a_skin_swap_to_another_shape_skips_the_deform_without_repeating_the_warning` (Review Focus 4);
    - `skeletons_sharing_data_keep_their_own_deform`.
  - **Pixels (`tests/e2e/skeleton.rs`):**
    - `a_deform_key_moves_a_mesh_s_pixels`;
    - `a_deform_is_interpolated_between_steps`.

  Run them: they fail.
- [ ] Step 2: Implement it, with the rules written into the module notes. Make the tests pass. Run `cargo test --workspace` to a log.
- [ ] Step 3: Commit with `feat: skeleton animations key a mesh's deform`.

### Task 5: Guards and bench

**Files:**
- Modify: `tests/tilemap_alloc.rs`, `benches/skeleton.rs`, `docs/backlog/skeleton.md`.

**Interfaces:**
- Consumes everything above.
- **The allocation test** gains a case: fifty skeletons, each with a weighted mesh of 60 vertices and a deform playing, among their regions. A traced frame allocates nothing more than the same entities drawn as sprites.
- **The bench** gains `skeletons_extract_mesh` and `skeletons_update_mesh`: 200 skeletons of 25 regions plus one weighted mesh of 60 vertices (two influences a vertex) with a deform playing.
- **Guards** (release, median of three):
  - extract at most 600 µs;
  - update at most 1,000 µs;
  - the existing region-only cases within 5% of their figures in `docs/backlog/skeleton.md`'s second record.

- [ ] Step 1: Write the allocation case. Run it. If it is green at once, say so and prove it by planting an allocation in the skinning path.
- [ ] Step 2: Write the bench cases. Run the bench three times, with `uptime` first in each log. Record the figures and loads in `docs/backlog/skeleton.md`. If a guard misses, profile and fix within this task, and report what changed. Do not loosen a guard.
- [ ] Step 3: Commit with `test: skeleton meshes are guarded for allocations and speed`.

### Task 6: The demo's meshes page

**Files:**
- Modify: `examples/demos/skeleton/` (`src/lib.rs`, `src/pages/`, `src/embedded.rs`, `assets/hero.skeleton.ron`, `assets/hero.png` or a second image, `assets/README.md`, `tests/pages.rs`).

**Interfaces:**
- Consumes the public API only.

- [ ] Step 1: Extend `tests/pages.rs` for an eighth page: it steps with no warnings, and its own test asserts that a mesh is shown and that the banner's pixels change over a deform cycle. Run it: it fails.
- [ ] Step 2: Draw the art with `magick`: a cape and a banner. The pack has neither; record them as drawn for the demo, with the commands, in `assets/README.md`.
  - **The cape:** a weighted mesh on the hero's back, across the torso and two new cape bones keyed in idle, walk and run, so it swings.
  - **The banner:** on a pole beside the hero, an unweighted mesh rippled by a looping deform animation.
  - **The page "Meshes":** the hero walking with the cape, the banner beside it, a setting for the overlay (hull and triangles drawn), and a readout saying which part is weighted and which is deformed.
  - The other pages' captures must not change, except where the hero now wears the cape. Decide whether the cape is a skin (recommended: a `cape` skin worn only on this page) so the other seven pages are unchanged.
- [ ] Step 3: Run the page tests and `./dev.sh demos:tour skeleton`, and look at every PNG. Capture the meshes page in motion and look at the frames. Run the demo's wasm clippy.
- [ ] Step 4: Commit with `feat: the skeleton demo gets a meshes page: a cape and a banner`.

### Task 7: Docs

**Files:**
- Modify:
  - `.claude/skills/ryo-skeleton/SKILL.md` (1.0.0 to 1.1.0).
  - `.claude/skills/ryo-api/SKILL.md` and `.claude/skills/ryo-renderer/SKILL.md` (one bump each).
  - `src/core/skeleton/AGENTS.md` (complete).
  - `src/core/render/AGENTS.md`, `src/core/shaders/AGENTS.md`.
  - `docs/architecture/overview.md`.
  - `docs/backlog/skeleton.md` (limits).
  - `docs/backlog/follow-ups.md` (remove only lines this branch fixed, after confirming each in the code; add nothing).

- [ ] Step 1: Write the skill's mesh sections:
  - what a mesh is and when to use one;
  - the file format, with examples that load;
  - weights;
  - deform keys and how they mix;
  - what skips a deform;
  - drawing, which is like regions;
  - the limits (a game's sprite shader, pixel-art snapping);
  - the overlay.

  Every Rust example compiles.
- [ ] Step 2: Update the other docs. Run:
  - `cargo test -p skill-examples`;
  - `cargo doc --no-deps -p ryo2d` (only the two known `padded.rs` warnings);
  - `cargo test --workspace` to a log;
  - `cargo test --release -p ryo2d` to a log.
- [ ] Step 3: Commit with `docs: the skeleton skill, API listing and module notes describe meshes`.
