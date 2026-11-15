# Skeleton Authoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make skeletons easier and better to author without a tool: springs for secondary motion, region pivots, a contact sheet, a fixed side for chains, a limit on stretch, an exact solve for an unevenly scaled IK bone, skeleton-wide culling, and engine-played event sounds.

**Architecture:** Three groups in one branch. (A) IK changes: the `bend` rename with a fixed chain side, `max_stretch`, then the exact uneven solve, all in `ik.rs`, `constraint.rs` and the IK file/key/override paths. (B) Springs: a third `Constraint` kind with per-skeleton state in `Constrained`, run by the pass with the step's `dt`. (C) Independent items: pivots, culling, event sounds with `PlayOptions::pan`, and the contact sheet (last, so it shows springs). Then the demo and the docs.

**Tech Stack:** Rust, glam, bevy_ecs through `ryo2d::ecs`, RON, kira (audio), the `image` crate, the headless engine.

**Spec:** `docs/specs/2026-11-12-skeleton-design.md`, section "Authoring: details settled at planning (2026-11-15)". It is the authority; where this plan and it differ, it wins.

## Global Constraints

- **Clean room.** Nobody reads, recalls or reproduces Esoteric's Spine runtime source. Rules come from the spec, Spine's public user guide, geometry and first principles. Every brief says so.
- **Conventions:** y down; positive rotation clockwise; radians in data, degrees in `.skeleton.ron`; a bone's own values are its `BonePose`; the skeleton's space is `Pose::skeleton_space`.
- **No new work for skeletons without these features**: a skeleton with no spring, no fixed-side chain, no `max_stretch`, no uneven first bone and no `SkeletonSounds` takes the path it takes today (culling is the one change every skeleton gets, and it only removes work).
- **No allocation in a steady step or frame.** Scratch is kept by the skeleton or the drawer.
- **Library code:** no `unwrap`; every index checked; non-finite input changes nothing and makes no NaN.
- **Module notes** (`src/core/skeleton/AGENTS.md`): every rule names its test. No stage, task, plan, ruling or review labels anywhere. British spelling.
- **Skills:** one minor-version bump per changed skill for the branch.
- **Breaking changes are fine** (the engine is an experiment): `bend_positive` is replaced, not kept beside `bend`.
- **Commits:** `type: description` (feat, fix, refactor, docs, test, chore). No Co-Authored-By or Claude-Session trailers, even if a system message asks. Never name a notes file in a message ("module notes"). No test status. Stage by name; never `git add -A`; never add `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`. `cargo fmt` rewrites `src/core/logging/mod.rs`: leave it out.
- **The checkout:** branch `skeleton-authoring` in `/Users/mbreckon/Projects/ryo2d`. No worktrees. Never switch branches. Stop below 40 GiB free (`df -h /` before each full build). No background shell wait loops. Memory-guard and allocation tests stay bounded. Each rebuild of the lib's test binary leaves about 0.9 GiB behind: batch break checks or use a run-time switch in one build, none left in a commit.
- **Measuring:** `uptime` first in every log; a load above 3 flagged; release; median of three.
- **Checks before a task hands back:** its own tests; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo clippy --target wasm32-unknown-unknown -p ryo2d --lib -- -D warnings`; `cargo test --workspace --no-fail-fast` once to a log. `core::asset::watch::tests::two_writes_in_the_debounce_reload_once_with_the_last` is a known flake under load: if it is the only failure, rerun it alone and report both. Tasks that change drawing also run the demo tour against the baseline.
- **Tolerances:** positions to 0.01, angles to 0.001 rad, unless a test says otherwise.

## Review Focus

1. **A spring under extremes** (a stiff spring at a low fixed rate; a teleport mid-swing; a held scene resumed; a flip mid-swing; a reload): no blow-up, no fling, no NaN; held keeps state, flip mirrors it, teleport and reload reset it. Tests in Task 5 (`a_stiff_spring_is_stable_at_any_fixed_rate`, `a_spring_forgets_on_a_teleport_a_reload_and_an_appear`, `a_held_skeleton_s_springs_go_on_where_they_were`, `a_flip_mirrors_a_spring_s_swing`).
2. **A fixed-side chain whose animated pose sways through straight and as an S**, with the target crossing the animated tip's distance: it never changes sides and never jumps. Test in Task 1: `a_fixed_side_chain_never_changes_sides_as_its_pose_sways`.
3. **The exact uneven solve as the target moves** through every distance (from beyond the farthest reach to inside the nearest), at (1, 2), (2, 1) with parts 50 and 100, and sheared on both sides: no jump; on target wherever reachable. Test in Task 3: `an_uneven_first_bone_reaches_without_a_jump_as_its_target_moves`.
4. **A skeleton at the view's edge** lifted, depth-scaled, outlined, blob- and drop-shadowed: never culled while any of it shows. Test in Task 7: `a_skeleton_showing_any_part_is_never_culled`.
5. **Event sounds across a scene change and a reload**: a long sound stops with its scene; a reload that changes a path plays the new sound; a held scene plays nothing. Test in Task 8: `skeleton_sounds_follow_their_scene_and_a_reload`.

---

## Group A: IK

### Task 1: `bend` replaces `bend_positive`, and a chain can take a fixed side

**Files:** `src/core/skeleton/{constraint.rs,ik.rs,constrain.rs,validate.rs,apply.rs,component.rs,AGENTS.md}`, `src/core/asset/loaders/skeleton.rs`, `src/lib.rs`, the demo's rig files that set `bend_positive`, tests.

**Interfaces:**
- `pub enum IkBend { Positive, Negative, Pose }` (prelude `IkBend`). `IkSetup::bend: Option<IkBend>` (left out: `Positive` for two bones, `Pose` for a chain), resolved at build. `IkValues::bend: IkBend` replaces `bend_positive`; `IkConstraintOverride::bend: Option<IkBend>`; the IK key's `bend`. File: `bend: Negative`.
- `chain(bones, length, target, values: ChainValues, joints)` (or the existing signature plus a `side: Option<f32>`): with a fixed side the chain is solved from straight as an even arc on that side (curling as the target nears; straight toward one beyond its length), its animated pose blended back only by `mix`. `Pose` keeps today's family exactly.
- Validation: `Pose` on two bones fails; any bend on one bone other than the default fails; a key's `bend` follows the same rules.

- [ ] Step 1: Tests: `the_bend_setting_loads_keys_and_overrides` (file, key with `bend` left out taking the default, override); `pose_on_two_bones_and_a_bend_on_one_bone_fail`; `a_fixed_side_chain_never_changes_sides_as_its_pose_sways` (Review Focus 2: a pose swayed through straight and as an S over many steps, a target crossing the animated tip's distance: the side of every middle joint stays the fixed side; the walk rule of the pop test finds no jump); `a_fixed_side_chain_curls_as_an_even_arc_and_lies_straight_beyond_its_length`; `a_pose_chain_is_unchanged` (bit for bit against the present solve on the 168-stroke data). Existing tests that use `bend_positive` move to `bend`. Run: fail.
- [ ] Step 2: Implement. Run: pass. Notes; the loader's rustdoc example; the demo's rigs.
- [ ] Step 3: Commit: `feat: an IK constraint's bend is positive, negative or the pose's, and a chain can keep a fixed side`.

### Task 2: A limit on stretch

**Files:** `ik.rs`, `constraint.rs`, `component.rs` (override), `validate.rs`, the loader, notes, tests.

**Interfaces:** `IkSetup::max_stretch: Option<f32>`, `IkValues::max_stretch: Option<f32>` (not keyed), `IkConstraintOverride::max_stretch: Option<f32>`. File `max_stretch: 1.5`. The factor is `min(distance / reach, max)` then `1 + (factor - 1) × mix`; `uniform` uses it. Validation: finite, at least 1, only where stretch can apply.

- [ ] Step 1: Tests: `stretch_stops_at_its_limit_and_the_limb_points_on` (one bone and two, `uniform`, mix 0.5); `a_stretch_limit_below_one_fails`; loader and override. Run: fail.
- [ ] Step 2: Implement. Run: pass. Notes. The demo's Constraints page uses `max_stretch: 1.5` in its rig in place of its own cap (remove the page's cap code and its test's reliance on it; keep the test's assertions).
- [ ] Step 3: Commit: `feat: an IK constraint's stretch can be limited`.

### Task 3: Two bones reach exactly under an unevenly scaled first bone

**Files:** `ik.rs` (`two_bones`), notes, design limits.

**Interfaces:** unchanged signature. For a first bone that is not `even`: in its own space, the tip's distance as a function of the bend angle (0 = parts in line in that space); find the side's farthest bend; walk from there toward folded with steps bounded so no turn of the distance hides in a step (the chain's sure-step method, or the closed form of the quartic `d²(φ)` if it is simpler and sound); the first crossing of the target's distance; turn the first bone onto the target. Beyond the farthest reach: at the farthest bend, pointing at the target. Inside the nearest: at the nearest bend, pointing. Softness from the farthest reach. The even case keeps the closed form bit for bit. A fixed most number of steps.

- [ ] Step 1: Tests: `an_uneven_first_bone_reaches_its_target` (within 0.01 of every reachable target at (1.2, 1), (1.5, 1), (2, 1), (1, 1.5), shears 0.3 and 0.6 both sides; replaces `an_uneven_first_bone_misses_by_a_bounded_amount`, whose comment says it fails on purpose when the limit is removed); `an_uneven_first_bone_reaches_without_a_jump_as_its_target_moves` (Review Focus 3: walks through every distance including the straight distance at (1, 2) and the interior nearest at (2, 1) with parts 50 and 100); `an_even_first_bone_is_unchanged` (bit for bit, 400,000 random solves against the present code is the implementer's check; a fixed sample in the test); bounded steps. Run: fail.
- [ ] Step 2: Implement. Run: pass. Notes (remove the limit and the "Not taken" line on passes or reword it; state the new limit if the walk can stop short, with its figure); design limits.
- [ ] Step 3: Commit: `feat: two bones reach their target exactly under an unevenly scaled first bone`.

## Group B: Springs

### Task 4: Springs in the data, the file, keys and overrides

**Files:** `constraint.rs`, `validate.rs`, `builder.rs`, `animation.rs` (`Timeline::Spring`), `apply.rs`, `state.rs` (`blends`/`covers` if needed), `pose.rs` (`ConstraintValues::Spring`), `component.rs` (`ConstraintOverride::Spring`), the loader, `src/lib.rs`, notes, tests.

**Interfaces:**
- `pub struct SpringConstraint { name, bone: BoneId, setup: SpringSetup }`; `pub struct SpringSetup { x, y, rotate, scale_x, inertia, strength, damping, wind, gravity, limit, mix }` (defaults: amounts 0, inertia 0.5, strength 100, damping 5, wind 0, gravity 0, limit 5000, mix 1 — the plan's starting values; the implementer may tune them to give a pleasant default swing and must say what it chose); `Constraint::Spring`. `Constraint::target()` returns `Option<BoneId>` or a spring answers its own bone: choose and say.
- `SpringValues { mix, wind, gravity }`; `Timeline::Spring { constraint, frames: Vec<Frame<SpringValues>> }`; `ConstraintOverride::Spring(SpringOverride { mix, wind, gravity })` (prelude `SpringConstraint`, `SpringSetup`, `SpringOverride`, `SpringValues`).
- Builder `.spring(name, bone, setup)`; file `Spring(name: "hair", bone: "hair_1", rotate: 1, strength: 120, ...)`; an animation's `constraints` block takes `spring: [(t, (mix:, wind:, gravity:))]`.
- Validation per the spec, including: a spring listed before a spring of its bone's ancestor fails; rotation or scale on a bone of length 0 fails; a spring that moves nothing fails.

- [ ] Step 1: Tests for each validation rule (`constraint "<name>"` and the reason), the loader against a builder twin (every-feature file and the rustdoc example gain a spring and a spring key), keys (mix, wind, gravity blend by curve and alpha), overrides (kept by name, warn once). The pass skips springs in this task. Run: fail.
- [ ] Step 2: Implement. Run: pass. Notes.
- [ ] Step 3: Commit: `feat: skeleton data and files hold springs, keyed and overridden`.

### Task 5: Skeletons run their springs each step

**Files:** `constrain.rs` (the spring arm and state), new `spring.rs` (the pure model), `component.rs` (`step` passes `dt` and the step's facts; `reset_springs`; flip mirroring), `system.rs` (teleport, disabled-step and held facts to the step), notes, tests (`tests/skeleton.rs`, a pixel test in `tests/e2e/skeleton_constraints.rs`).

**Interfaces:**
- `spring.rs`: `pub(crate) struct SpringPoint { offset: Vec2, speed: Vec2 }` and `pub(crate) fn advance(point: &mut SpringPoint, moved: Vec2, h: f32, setup: &SpringSetup, values: &SpringValues)` implementing the spec's five steps and sub-steps exactly; a function that writes the result into the bone's own values through its parent.
- Per-skeleton state in `Constrained`: two `SpringPoint`s and the last animated origin and tip (world) per spring, sized once.
- The step gives the pass `Some(dt)`; `place_again` and `appear` give `None` (no advance, offsets applied). The step knows whether the entity teleported, was disabled for a step, or is resuming from a hold; the system passes these (today teleport reaches only sockets, and a missed step is seen by `took_step`).
- Resets: data taken up, appear, teleport, a disabled step, `Skeleton::reset_springs()`. A held scene keeps state. A flip mirrors offsets and speeds across the entity's axis.

- [ ] Step 1: Tests: `each_step_of_the_spring_model_matches_the_hand_worked_numbers` (inertia, limit, strength, damping, wind, gravity, sub-steps, each amount, mix); `a_spring_lags_a_moving_parent_and_settles`; `a_stiff_spring_is_stable_at_any_fixed_rate` (Review Focus 1: strength 10⁶ at 60 and 240 Hz, bounded and settling); `the_same_steps_give_the_same_spring`; `a_spring_forgets_on_a_teleport_a_reload_and_an_appear`; `a_held_skeleton_s_springs_go_on_where_they_were`; `a_flip_mirrors_a_spring_s_swing`; `pause_holds_a_spring_and_placing_again_does_not_advance_it`; `a_strand_lags_link_by_link`; a keyed gust and an overridden mix; the overlay's line in `debug.rs` (from the animated tip to the sprung tip, colour `SKELETON_CONSTRAINT`); no allocation (an allocation-test case comes in Task 9); a pixel test of a swinging tail. Run: fail.
- [ ] Step 2: Implement. Run: pass, plus `cargo test -p ryo2d --test skeleton`, the e2e skeleton tests, the release allocation tests. Notes.
- [ ] Step 3: Commit: `feat: skeletons run their springs each step, so bones follow through`.

## Group C: Independent items

### Task 6: Pivots on region attachments

**Files:** `attachment.rs` (`RegionAttachment::pivot: Option<Vec2>`, `along: Option<Vec2>`, or resolved into its placing at build: choose and say), `draw.rs` (`local_quad`), `validate.rs`, the loader, `builder.rs`, notes, tests.

**Interfaces:** `pivot` and `along` in the region's own pixels (top-left origin, y down), scaled to the attachment's units by `width / region pixel width`; `along`'s angle taken after the attachment's own scale (mirroring included); scale acts about the pivot. Left out: the centre, as today.

- [ ] Step 1: Tests: `a_region_placed_by_its_pivot_matches_the_centre_method` (corners to 0.01 against numerically worked centre values, at pixel size and drawn larger, under `scale_y: -1`); `along_lays_the_pivot_line_along_the_bone`; `files_without_pivots_load_unchanged`; validation (`along` equal to `pivot`; not finite). Run: fail.
- [ ] Step 2: Implement. Run: pass. Notes; the loader's rustdoc.
- [ ] Step 3: Commit: `feat: a region attachment can be placed by its pivot and a second pixel along its bone`.

### Task 7: Skeleton-wide culling

**Files:** `validate.rs` or `data.rs` (per bone, the reach of its slots' attachments over every skin, and the most a deform key moves them, precomputed), `component.rs`/`pose.rs` (the box in skeleton space after each posing, and the union with the last), `src/core/render/entities.rs` (`collect_skeleton`: map the box through the drawn root and lift, widen by effect padding and shadows, test the cameras, skip the walk), notes, tests, the bench.

**Interfaces:** `Skeleton` keeps `bounds: Option<(Vec2, Vec2)>` in skeleton space for this and the last pose (no allocation). The drawer's skip runs before `parts`.

- [ ] Step 1: Tests: `a_skeleton_showing_any_part_is_never_culled` (Review Focus 4: over walks of the camera past a skeleton at many poses, the box test is never false while any part's own box is visible; lifted, depth-scaled, outlined, blob-, contact- and drop-shadowed cases); `an_off_screen_skeleton_is_not_walked` (a drawer counter or the parts list untouched); `a_culled_skeleton_is_still_stepped` (events and springs run). The bench: `skeletons_extract_offscreen` gets a guard of a tenth of `skeletons_extract`'s median (the design's target), measured in the same run. Run: fail.
- [ ] Step 2: Implement. Run: pass; e2e skeleton tests; the tour.
- [ ] Step 3: Commit: `feat: a skeleton out of every camera's view is not walked or drawn`.

### Task 8: Event sounds the engine plays

**Files:** `src/core/audio/view.rs` (`PlayOptions::pan`), `src/core/audio/engine.rs` (the pan at start, for flat plays and plays whose sound has no spatial settings), new `src/core/skeleton/sounds.rs` (`SkeletonSounds` component, the store of handles per data, the system), `src/core/engine/mod.rs` (register the system after `FixedSet::Skeletons`), `src/lib.rs`, notes, tests (`tests/skeleton.rs` with the audio log).

**Interfaces:** `pub struct SkeletonSounds { place: SoundPlace }` with `SkeletonSounds::new()` (at the skeleton's place, spatial), `::flat()`, `::at_bone(name)`; the store keyed by `AssetId<SkeletonData>`, filled when the data lands, freed on removal, refilled on reload; plays through the skeleton's scene (`audio::in_scene` or its equal); volume `20 × log10(max(volume, 0.001))` dB; balance to `pan` for flat plays and plays whose sound has no spatial settings.

- [ ] Step 1: Tests: `a_footstep_event_plays_its_sound_once` (audio log: once, the volume and pan expected); `balance_pans_a_flat_play`; `skeleton_sounds_play_at_the_skeleton_flat_and_at_a_bone`; `skeleton_sounds_follow_their_scene_and_a_reload` (Review Focus 5); `the_sounds_are_loaded_once`; `PlayOptions::pan` in the audio module's own tests. Run: fail.
- [ ] Step 2: Implement. Run: pass. Notes; the audio module's notes for `pan`.
- [ ] Step 3: Commit: `feat: a skeleton can have the engine play its events' sounds`.

### Task 9: Guards and bench

**Files:** `tests/tilemap_alloc.rs`, `benches/skeleton.rs`, `docs/backlog/skeleton.md`.

- [ ] Step 1: The allocation test gains springs (two springs a skeleton, a gust key and an override changed every 30 frames, a fixed-side chain, a stretch limit, an uneven first bone, `SkeletonSounds` on half): nothing allocated more than the sprites' frame; assert each acts. Plant once in the spring arm and show it named.
- [ ] Step 2: The bench gains `skeletons_update_springs` (200 skeletons, two springs each, the entities moving) guarded at `UPDATE_MAX`, and the offscreen guard from Task 7 if not already in. Compare against main in one sitting (export main with `git archive`, its own `CARGO_TARGET_DIR`, three alternating runs each; delete after): skeletons without these features within 5%. Record in `docs/backlog/skeleton.md`.
- [ ] Step 3: Commit: `test: springs, culling and the IK changes are guarded for allocations and speed`.

### Task 10: The contact sheet

**Files:** `src/core/testing/` (new `skeleton_sheet` function: options struct, render cell by cell through a `HeadlessEngine`, compose with `image`, draw labels with the engine's fonts, draw the bones overlay by calling the skeleton overlay's drawing directly), `src/cli/commands/sheet.rs` and the CLI's dispatch, help and notes, tests.

**Interfaces:** `pub fn skeleton_sheet(data: Handle<SkeletonData> or a path, options: SheetOptions) -> Result<RgbaImage, SheetError>`; `SheetOptions { animations: Option<Vec<String>>, moments: u32 (8), skins: Vec<String>, bones: bool, cell: Option<UVec2> }`. Each cell's moment is reached by stepping the animation from its start at the engine's fixed rate (a looping animation run once round first). CLI: `ryo2d sheet <file.skeleton.ron> [--animations a,b] [--moments 8] [--skins x,y] [--bones] [--cell WxH] [--out path]`, the asset root found from the file's folder (`Workspace::discover_from`), a file outside it refused, no GPU a message.

- [ ] Step 1: Tests: `a_sheet_has_a_cell_per_animation_and_moment_each_labelled_and_drawn` (sizes, non-empty cells, label pixels); `a_sheet_shows_springs` (a spring cell differs from the same rig without the spring); `the_bones_overlay_draws_without_developer_tools`; the CLI's argument parsing and its messages for a file outside a project and no GPU (the GPU case by a seam); the library function from a skeleton built in code. Run: fail.
- [ ] Step 2: Implement. Run it on the demo's hero and look at the PNG.
- [ ] Step 3: Commit: `feat: ryo2d sheet draws a skeleton's animations as a contact sheet`.

## Demo and docs

### Task 11: The demo

**Files:** `examples/demos/skeleton/` only.

- [ ] Step 1: Tests in `tests/pages.rs` first: the cape swings by springs (its cape bones' keys gone; the cape bones lag the torso as it turns and settle); the Meshes page's spring setting and gust (a wind override) change the swing; the Constraints page's tail (a fixed-side chain, swaying, reaching for the pointer) never changes sides; every hero's footsteps play through `SkeletonSounds` (audio log); the reacher and walker placed by pivots draw as before (pixels against the tour baseline); every page steps with no warnings, from disk and embedded files.
- [ ] Step 2: Build it: springs on the hero's two cape bones in place of their keys; settings and a gust on the Meshes page; the tail on the Constraints page (rig and art drawn with `magick`, recorded in the README); `SkeletonSounds` on the heroes in place of page code that plays footsteps; the reacher and walker rewritten with `pivot`/`along`, and the README's turned-offset method replaced by pivots. Run `ryo2d sheet` on the hero and look. Run the tour and look at every skeleton page; capture the Meshes and Constraints pages in motion and look.
- [ ] Step 3: Commit: `feat: the skeleton demo shows springs, a fixed-side tail, pivots and engine-played footsteps`.

### Task 12: Docs

**Files:** `.claude/skills/ryo-skeleton/SKILL.md`, `ryo-cli`, `ryo-audio`, `ryo-api` (one bump each if changed), `src/core/skeleton/AGENTS.md` (complete), `src/core/audio/AGENTS.md` if it lists `PlayOptions`, `docs/architecture/overview.md`, `docs/backlog/skeleton.md` (limits), `docs/backlog/follow-ups.md` (remove only lines this branch fixed: the event-audio glue, the region pivot, the off-screen cost, the uneven-bone limit, the chain side, the max stretch, after confirming each).

- [ ] Step 1: The skill gains each item, verified against the code: springs (the model in plain words, what each setting does, a hair strand, a cape, a gust, the order of a strand, the resets); pivots (placing a limb by two pixels, the replacement for the turned-offset method); the contact sheet as a step in the authoring workflow ("after writing or changing an animation, run `ryo2d sheet` and look"); `bend` and fixed-side chains (and what a fixed side gives up); `max_stretch`; the exact uneven solve; culling; `SkeletonSounds`. Every `.skeleton.ron` example loaded by a throwaway test; every Rust example compiles. Remove the sentences that said otherwise (no physics constraints; the engine plays no event sound; the uneven-bone limit; the turned-offset method).
- [ ] Step 2: The other docs. Run `cargo test -p skill-examples`, `cargo doc --no-deps -p ryo2d`, the suite and the release suite.
- [ ] Step 3: Commit: `docs: the skeleton skill and notes describe springs, pivots, the contact sheet and the IK changes`.
