# Skeleton (stage 1: core) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the old debug-line skeleton module with a professional 2D skeletal animation core: bones, slots, region/point/bounding-box attachments, skins, keyframed animations with mixing and events, bone overrides, sockets, a hot-reloading `.skeleton.ron` asset, drawing that matches sprite entities, and a new demo.

**Architecture:**
- The data is an asset, `SkeletonData`, that follows Spine's model. Each entity holds a `Skeleton` (its pose) and an optional `SkeletonAnimation` (its tracks).
- One system in a new `FixedSet::Skeletons`, after `Transforms`, does all of a step's work in order: it advances the tracks, applies the timelines, applies the overrides, computes the world transforms, places the sockets, and sends the events.
- The entity drawer treats a skeleton entity (one whose `Renderable` has `RenderStyle::Skeleton`) as one drawable. It emits the skeleton's region quads in its draw order as entity-kind items, so the skeleton y-sorts, floors, lights and shadows like a sprite.

**Tech Stack:** Rust, bevy_ecs 0.20 (`ryo2d::ecs`), wgpu 30, RON (serde) for the file format.

**Spec:** `docs/specs/2026-11-12-skeleton-design.md`. Read it first. The research note is `docs/research/2026-11-12-skeleton-formats.md`, and the engine seam map is reproduced where tasks need it.

## Global Constraints

- **Clean room:** no task reads Esoteric's runtime source (spine-runtimes). Mixing and event rules come from Spine's public user guide and JSON format doc. Where those are silent, choose a rule, write it in the module notes (`src/core/skeleton/AGENTS.md`) and pin it with a test.
- **Coordinates:** y down, positive rotation clockwise. Files give angles in degrees; runtime data holds radians.
- **Limits:**
  - at most 255 bones and 255 slots a skeleton (`BoneId(u8)`, `SlotId(u8)`); more fails to load;
  - a non-finite value in a file fails to load;
  - in a bone override, a non-finite value is ignored with one warning.
- **Allocation:** no per-frame allocation in steady state. Names in messages are `Arc<str>` clones.
- **Drawing:** a skeleton draws exactly like a sprite entity (tint, alpha, flip, z, `YSort`, storey/`DrawLift`, lit, sprite shadows, interpolation).
- **Repository rules** (`AGENTS.md`):
  - no `unwrap` on reachable library paths;
  - LF line endings;
  - British spelling in docs;
  - skills hold no stage, task or history wording, and get exactly one minor-version bump per skill per branch;
  - every Rust example in a skill compiles under `tests/skill-examples`.
- **Commits:**
  - stage files by name, never `git add -A`, and never stage `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`;
  - no Co-Authored-By or Claude-Session trailers;
  - say "module notes", never `AGENTS.md`;
  - no test status in messages;
  - types are `feat`, `fix`, `refactor`, `docs`, `test` or `chore`.
- **Working conditions:**
  - work in the main checkout on branch `skeleton`, with no worktrees;
  - stop and report if free disk drops below 40 GiB (`df -h /`);
  - record `uptime` before any measurement and flag a load above 3;
  - logs go under `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/`.
- **Before each commit:** `cargo clippy --workspace --all-targets -- -D warnings`. For any wasm-touching code: `cargo clippy --target wasm32-unknown-unknown -p ryo2d -- -D warnings`.

## Review Focus

1. **A reload mid-crossfade that removes the bone a socket follows** (a modeller deletes a bone while the game runs): no panic. The socket warns once and stays where it was, the tracks keep their animations by name, and the mix completes. Test in Task 7: `a_reload_that_removes_a_socket_s_bone_warns_once_and_keeps_playing`.
2. **Zero-length animations and zero mix durations** (a one-frame pose used as a state, an instant switch): no division by zero or NaN in the pose, a looping zero-length animation's events fire once each time it is set, and a zero mix switches at once. Test in Task 5: `zero_length_animations_and_zero_mixes_are_finite`.
3. **A flipped skeleton** (`Renderable::flip_x`, or a negative scale on the entity, for a character turning round): regions mirror, draw order holds, `Skeleton::bone_world` and sockets mirror with it, and y-sort still uses the feet. Test in Task 9: `a_flipped_skeleton_mirrors_its_regions_and_sockets`.
4. **A skeleton spawned on a `load` handle that has not landed** (async loading): it draws nothing and warns nothing, an animation set before the data lands starts when it lands, and sockets wait. Test in Task 7: `a_skeleton_waits_for_its_data_and_keeps_the_animation_set_meanwhile`.
5. **Two skeletons sharing one `SkeletonData`** (a crowd): skins, attachment changes, overrides, colours and draw order are per entity and never leak between instances. Test in Task 7: `skeletons_sharing_data_keep_their_own_skin_overrides_and_attachments`.

---

### Task 1: Remove the old module; add the empty set

**Files:**
- Delete: `src/core/skeleton/*`, `src/core/systems/skeleton.rs`, `examples/demos/skeleton/` (the whole demo)
- Modify:
  - `src/core/mod.rs`, `src/core/systems/mod.rs` (drop `update_skeletons` and its test);
  - `src/lib.rs` (drop the 11 prelude names at :207-211);
  - `src/core/schedule.rs` (add `FixedSet::Skeletons` between `Transforms` and `Particles` in the enum and its `.chain()`; fix the `Juice` doc comment);
  - `src/core/engine/mod.rs` (configure the set);
  - `tests/e2e/gpu_world.rs` (delete the `Skeletal` scene and `draw_skeleton_draws_on_top`);
  - `tests/integration/support.rs:11` (the comment);
  - `docs/architecture/overview.md`, `src/core/systems/AGENTS.md`, `src/core/ecs/AGENTS.md`, `src/core/render/AGENTS.md` (drop the old mentions; the overview says skeletal animation is being rebuilt);
  - `.claude/skills/ryo-api`, `ryo-engine-dev`, `ryo-renderer` (remove the old sections; bump each once).
- Create: `src/core/skeleton/mod.rs` (an empty module with a doc comment), `src/core/skeleton/AGENTS.md` (a stub naming the spec).

**Interfaces:**
- Produces: `FixedSet::Skeletons` (`pub`), ordered `… Physics, Transforms, Skeletons, Particles, Trails`.

- [ ] Step 1: Write `the_skeleton_set_runs_after_transforms_and_before_particles` in `src/core/schedule.rs` tests. Build a schedule with three systems recording their order into a resource, in `Transforms`, `Skeletons` and `Particles`. Assert the order. Run it: it fails, because `Skeletons` does not exist.
- [ ] Step 2: Delete the old module and demo, and add the set. Run `cargo check --workspace --all-targets` and the test, and make both pass.
- [ ] Step 3: Confirm nothing refers to the old module: `git grep -n -i "SkeletalController\|SkeletonDef\|draw_skeleton\|update_skeletons" -- . ':!docs/research' ':!docs/specs' ':!docs/plans'` is empty.
- [ ] Step 4: Commit with `refactor: the old skeleton module and demo are removed, and skeletons get their own fixed set`.

### Task 2: The data model, builder and validation

**Files:**
- Create in `src/core/skeleton/`:
  - `data.rs`: `SkeletonData`, `BoneData`, `SlotData`, `Inherit`, `SlotBlend`, `Region`, `EventData`;
  - `attachment.rs`: `Attachment`, `RegionAttachment`, `PointAttachment`, `BoundingBoxAttachment`;
  - `skin.rs`: `Skin`;
  - `curve.rs`: `Curve`;
  - `animation.rs`: `Animation`, `Timeline`, its frame types;
  - `builder.rs`: `SkeletonDataBuilder`;
  - `validate.rs`.
- Modify: `src/core/skeleton/mod.rs`, `src/lib.rs` (prelude).

**Interfaces:**
- **Produces types:**
  - `pub struct BoneId(pub u8)` and `pub struct SlotId(pub u8)`, both `Copy` and `Eq`.
  - `pub enum Inherit { Normal, OnlyTranslation, NoRotationOrReflection, NoScale, NoScaleOrReflection }`.
  - `pub struct BoneData`, with pub fields:
    - `name: Arc<str>`, `parent: Option<BoneId>`, `length: f32`;
    - `x`, `y`, `rotation` (radians), `scale_x`, `scale_y`, `shear_x`, `shear_y` (radians);
    - `inherit: Inherit`.
  - `pub enum SlotBlend { Normal, Additive, Multiply, Screen }`.
  - `pub struct SlotData { name: Arc<str>, bone: BoneId, color: Color, attachment: Option<Arc<str>>, blend: SlotBlend }`.
  - `pub struct Region { name: Arc<str>, image: Handle<Image>, x: u32, y: u32, width: u32, height: u32 }`: a pixel rect within the image.
  - `pub enum Attachment { Region(RegionAttachment), Point(PointAttachment), BoundingBox(BoundingBoxAttachment) }`, where:
    - `RegionAttachment { region: usize /* index into regions */, x, y, rotation, scale_x, scale_y, width, height, color }`;
    - `PointAttachment { x, y, rotation }`;
    - `BoundingBoxAttachment { vertices: Vec<Vec2> }`.
  - `pub struct Skin { name: Arc<str>, … }`, with `fn attachment(&self, slot: SlotId, name: &str) -> Option<&Attachment>`.
  - `pub enum Curve { Linear, Stepped, Bezier { cx1, cy1, cx2, cy2 } }`.
  - `pub enum Timeline`, with variants:
    - `Rotate { bone, frames }`, `Translate { bone, frames }`, `Scale { bone, frames }`, `Shear { bone, frames }`;
    - `Attachment { slot, frames: Vec<(f32, Option<Arc<str>>)> }`;
    - `Color { slot, frames }`;
    - `DrawOrder { frames: Vec<(f32, Option<Vec<SlotId>>)> }`;
    - `Event { frames: Vec<(f32, usize /* event index */, EventValues)> }`.
    - Value frames hold `(time, value, Curve)`.
  - `pub struct Animation { name: Arc<str>, duration: f32, timelines: Vec<Timeline> }`.
  - `pub struct EventData { name: Arc<str>, int: i32, float: f32, string: Arc<str>, audio: Option<EventAudio> }`, with `pub struct EventAudio { path: Arc<str>, volume: f32, balance: f32 }`.
  - `pub struct SkeletonData`, with read accessors:
    - `bones()`, `slots()`, `skins()`, `default_skin()`, `animations()`, `events()`, `regions()`;
    - `bone(name) -> Option<BoneId>`, `slot(name) -> Option<SlotId>`, `animation(name) -> Option<&Animation>`;
    - `mix(from: &str, to: &str) -> f32` (the pair's mix, else the default mix).
  - `impl Asset for SkeletonData`.
- **Produces builder and validation:**
  - `SkeletonData::builder() -> SkeletonDataBuilder`. Its fluent methods are `bone`, `slot`, `region`, `skin`, `attachment`, `animation`, `event`, `mix`, `default_mix`, and `build() -> Result<SkeletonData, SkeletonError>`.
  - `pub struct SkeletonError { item: String, problem: String }`, which is `Display`.
  - Validation rules: names are unique per kind; parents come first; references resolve; frames are in time order; values are finite; at most 255 bones and 255 slots.

- [ ] Step 1: Write the tests in `src/core/skeleton/validate.rs`:
  - `a_built_skeleton_resolves_names` (`bone("arm") == Some(BoneId(1))`; `mix("idle","run")` returns the pair's mix, and the default otherwise);
  - and one test per rule: `a_duplicate_bone_name_fails`, `a_child_before_its_parent_fails`, `an_unknown_slot_bone_fails`, `frames_out_of_order_fail`, `a_non_finite_value_fails`, `two_hundred_fifty_six_bones_fail`.
  - Each failing case asserts that the error's `item` names the offending item.
  - Run them: they fail.
- [ ] Step 2: Implement the data, builder and validation. Make the tests pass.
- [ ] Step 3: Commit with `feat: skeleton data, built in code and checked`.

### Task 3: Bone world transforms

**Files:**
- Create: `src/core/skeleton/pose.rs`.

**Interfaces:**
- Consumes: `BoneData`, `Inherit`, `SkeletonData` (Task 2).
- Produces:
  - `pub struct BonePose { x, y, rotation, scale_x, scale_y, shear_x, shear_y }`, the local pose, with `fn setup(&BoneData) -> Self`;
  - `pub(crate) struct Pose { bones: Vec<BonePose>, world: Vec<Affine2>, slot_colors: Vec<Color>, slot_attachments: Vec<Option<Arc<str>>>, draw_order: Vec<SlotId> }`, with `fn setup(&SkeletonData) -> Self` and `fn compute_world(&mut self, data: &SkeletonData, root: Affine2)`.

- [ ] Step 1: Write the tests in `pose.rs`:
  - `normal_inherit_composes_parent_and_child`;
  - `only_translation_ignores_parent_rotation_and_scale`;
  - `no_rotation_or_reflection_keeps_world_angle_under_a_flipped_parent`;
  - `no_scale_keeps_unit_length_under_a_scaled_parent`;
  - `no_scale_or_reflection_ignores_a_negative_parent_scale`;
  - `shear_skews_the_child_axes`;
  - `the_root_affine_places_the_whole_skeleton`.
  - Each compares the world matrix against a hand-computed `Affine2` to 1e-4. Comment each expected value with how it was computed.
  - Run them: they fail.
- [ ] Step 2: Implement `compute_world`.
  - Bones are parent-first, so this is one pass.
  - Each inherit mode is derived from the parent's world matrix as Spine's user guide describes the modes: translation always inherited; rotation, scale and reflection removed per mode.
  - Write the formulas used into the module notes.
  - Make the tests pass.
- [ ] Step 3: Commit with `feat: skeleton bones compute their world transforms with Spine's inherit modes`.

### Task 4: Curves, sampling and applying one animation

**Files:**
- Create: `src/core/skeleton/apply.rs`.
- Modify: `curve.rs`, `animation.rs`.

**Interfaces:**
- Consumes: `Curve`, `Timeline`, `Animation` (Task 2); `Pose` (Task 3).
- Produces:
  - `Curve::value(&self, t: f32) -> f32`, mapping a 0–1 fraction to a 0–1 eased fraction (bezier solved for x, then y);
  - `pub(crate) fn apply(animation: &Animation, data: &SkeletonData, pose: &mut Pose, time: f32, alpha: f32, blend: MixBlend, direction: MixDirection)`.
    - `MixBlend` is `pub enum { Replace, Add }`, the public mix blend for tracks.
    - `MixDirection` is `pub(crate) enum { In, Out }`: attachment and draw-order keys apply only when mixing in.
  - `pub(crate) fn events_between(animation: &Animation, last: f32, now: f32, looping: bool, out: &mut Vec<(f32, usize, EventValues)>)`: the keys in (last, now], across loop boundaries, in time order.

- [ ] Step 1: Write the tests in `apply.rs`:
  - `linear_stepped_and_bezier_sample_known_values` (bezier `(0.25, 0.1, 0.25, 1.0)` at x = 0.5 ≈ 0.8024, to 1e-3; its ends are exactly 0 and 1);
  - `a_rotate_timeline_blends_by_alpha`;
  - `an_add_blend_adds_to_the_pose`;
  - `attachment_keys_apply_only_when_mixing_in`;
  - `draw_order_keys_reorder_slots`;
  - `color_keys_tint_the_slot`;
  - `events_between_crosses_loops_once_each`:
    - a 1 s loop, keys at 0.25 and 0.75, `last = 0.6`, `now = 2.3`;
    - expected `[0.75, 1.25, 1.75, 2.25]` as unwrapped times;
  - `an_event_at_zero_fires_on_the_first_step_and_each_loop`.
  - Run them: they fail.
- [ ] Step 2: Implement it. Make the tests pass.
- [ ] Step 3: Commit with `feat: skeleton animations sample curves and apply to a pose, and find their events`.

### Task 5: The animation state (tracks, queue, mixing)

**Files:**
- Create: `src/core/skeleton/state.rs`.
- Modify: `src/core/skeleton/AGENTS.md` (the rules).

**Interfaces:**
- Consumes: `apply`, `events_between`, `MixBlend` (Task 4).
- Produces:
  - `pub struct TrackEntryId(u32)`.
  - `pub struct TrackEntry`, with pub fields:
    - `speed: f32` (1), `alpha: f32` (1), `mix_duration: f32`, `mix_blend: MixBlend` (Replace);
    - `event_threshold: f32` (0), `reverse: bool`, `hold_previous: bool`, `looping: bool`, `delay: f32`;
    - and read-only `animation()`, `track_time()`, `is_complete()`.
  - `pub struct SkeletonAnimation`, with:
    - `fn set_animation(&mut self, track: usize, name: &str, looping: bool) -> Option<TrackEntryId>`;
    - `fn add_animation(&mut self, track: usize, name: &str, looping: bool, delay: f32) -> Option<TrackEntryId>`;
    - `fn set_empty(&mut self, track: usize, mix: f32)`, `fn clear(&mut self, track: usize)`;
    - `fn entry(&self, id) -> Option<&TrackEntry>`, `fn entry_mut(&mut self, id) -> Option<&mut TrackEntry>`;
    - `fn current(&self, track) -> Option<TrackEntryId>`;
    - `pub time_scale: f32`.
    - A name unknown to the data warns once per skeleton per name and returns `None`. Before the data lands, the call is kept and resolved when it lands (Task 7 tests this).
  - `pub(crate) fn advance_and_apply(&mut self, data: &SkeletonData, pose: &mut Pose, dt: f32, events: &mut Vec<FiredEvent>, track_events: &mut Vec<FiredTrackEvent>)`.
  - `pub enum TrackEventKind { Start, Interrupt, End, Complete, Dispose }`.

**Rules to write in the module notes and pin with tests:**
- Applying runs from the lowest track up. Track 0 replaces the setup pose; a higher track mixes over the tracks below by its alpha.
- A new entry on a track mixes from the previous one over `mix_duration`. The previous entry's time freezes or keeps advancing per its `hold_previous`: false means it keeps advancing, as Spine's default is.
- Events of a mixing-out entry fire while the mix's fraction is below `event_threshold`.
- `set_empty` mixes the track to the setup pose, or to nothing for higher tracks, and then clears it.
- `Complete` fires each time a looping entry passes its duration, and once for a non-looping one. `End` fires when an entry is replaced or cleared, and `Dispose` after it is gone. `Interrupt` fires when an entry is replaced before it ends.
- A queued entry starts after `delay`, measured from the previous entry's end when `delay <= 0` (Spine's convention: the previous duration plus the delay).

- [ ] Step 1: Write the tests in `state.rs`:
  - `a_crossfade_is_half_way_at_half_its_mix` (rotate 0 to 90° over a 0.2 s mix, sampled at 0.1 s, gives 45°);
  - `an_additive_track_adds_over_track_zero`;
  - `a_higher_track_at_half_alpha_blends_half`;
  - `set_empty_fades_a_layer_out_then_clears_it`;
  - `a_queued_entry_starts_after_its_delay`;
  - `events_fire_once_across_loops_large_steps_and_time_scales` (time scales 0 and 3; a step of 2.5 durations);
  - `a_mixing_out_entry_stops_firing_past_its_event_threshold`;
  - `track_events_follow_start_interrupt_end_complete_dispose`;
  - `zero_length_animations_and_zero_mixes_are_finite` (Review Focus 2).
  - Run them: they fail.
- [ ] Step 2: Implement the state and write the rules into the module notes. Make the tests pass.
- [ ] Step 3: Commit with `feat: skeletons mix animations on tracks, with queues, layers, additive blends and events`.

### Task 6: The `.skeleton.ron` file and its loader

**Files:**
- Create: `src/core/asset/loaders/skeleton.rs` (`SkeletonLoader`).
- Modify: `src/core/asset/loaders/mod.rs`, `src/core/asset/mod.rs` (register in `insert_registry`, beside particles), `src/lib.rs` (export `SkeletonLoader` as particles exports its loader).
- Test: in the loader file, plus `tests/skeleton_reload.rs`, modelled on `tests/reload.rs` for particles with a `MemorySource`.

**Interfaces:**
- Consumes: `SkeletonData::builder` and validation (Task 2).
- Produces:
  - The extension `"skeleton.ron"`.
  - A private serde file struct with `deny_unknown_fields`, angles in degrees, and images as paths loaded with `cx.load::<Image>` relative to the file.
  - Errors are `AssetError::decode(cx.path(), e)`, with line and column. A validation error names the file and the item.
- The file shape the loader reads (the example goes in the skill too):

```ron
(
  images: { "parts": "hero.png" },
  regions: { "head": (image: "parts", rect: (0, 0, 64, 64)), "torso": (image: "parts", rect: (64, 0, 48, 80)) },
  bones: [ (name: "root"), (name: "torso", parent: "root", y: -40, length: 40), (name: "head", parent: "torso", y: -40, rotation: 5) ],
  slots: [ (name: "torso", bone: "torso", attachment: "torso"), (name: "head", bone: "head", attachment: "head") ],
  skins: { "default": { "torso": { "torso": Region(region: "torso", y: -20) }, "head": { "head": Region(region: "head", y: -24) } } },
  events: { "step": (audio: Some((path: "audio/step.ogg", volume: 0.8))) },
  animations: { "idle": (duration: 1.0, bones: { "head": (rotate: [(0.0, 5.0), (0.5, -5.0, Bezier(0.25, 0, 0.75, 1)), (1.0, 5.0)]) }, events: [(0.5, "step")]) },
  mixes: { default: 0.15, pairs: [("idle", "run", 0.2)] },
)
```

- [ ] Step 1: Write the tests:
  - `the_documented_file_loads` (the example above, with a stub image in a `MemorySource`);
  - `a_typo_in_a_field_fails_with_its_line`;
  - `an_unknown_region_fails_naming_the_attachment`;
  - `angles_in_degrees_become_radians`.
  - In `tests/skeleton_reload.rs`: `a_reload_keeps_the_asset_id_and_a_broken_reload_keeps_the_last_good_data`.
  - Run them: they fail.
- [ ] Step 2: Implement the loader. Make the tests pass.
- [ ] Step 3: Commit with `feat: skeletons load from .skeleton.ron files that hot-reload`.

### Task 7: Skeletons in the world: components, the step, events, overrides, sockets

**Files:**
- Create in `src/core/skeleton/`:
  - `component.rs`: `Skeleton`, `BoneOverride`, `Space`;
  - `socket.rs`: `BoneSocket`;
  - `event.rs`: `SkeletonEvent`, `SkeletonTrackEvent`;
  - `system.rs`: `update_skeletons`.
- Modify: `src/core/engine/mod.rs` (register the system in `FixedSet::Skeletons`, and the messages), `src/lib.rs`.
- Test: `tests/skeleton.rs`, an integration test on `HeadlessEngine` or the integration support world.

**Interfaces:**
- Consumes: Tasks 2–6.
- **Produces `Skeleton`** (component), with:
  - `fn new(data: Handle<SkeletonData>) -> Self`;
  - `fn bone_world(&self, name: &str) -> Option<Affine2>`;
  - `fn set_skin(&mut self, name: &str)`, `fn set_skins(&mut self, names: &[&str])` (combined);
  - `fn set_attachment(&mut self, slot: &str, attachment: Option<&str>)`;
  - `fn override_bone(&mut self, bone: &str, o: BoneOverride)`, `fn clear_override(&mut self, bone: &str)`;
  - `fn point(&self, name: &str) -> Option<(Vec2, f32)>` and `fn bounding_box(&self, name: &str, out: &mut Vec<Vec2>) -> bool`, both in world space.
  - It keeps the last two steps' world matrices for drawing (`pub(crate)`).
- **Produces `BoneOverride`:** `pub struct BoneOverride { rotation: Option<f32>, translation: Option<Vec2>, scale: Option<Vec2>, space: Space, mix: f32 }`, with `pub enum Space { Local, World }`. World rotation is converted to the bone's parent space each step.
- **Produces `BoneSocket`** (component on a child entity of a skeleton entity): `{ bone: String, offset: Transform, inherit_scale: bool }`.
  - Each step, after world transforms are computed, its `Transform` (local to the skeleton entity) and its `GlobalTransform` are written.
  - Its descendants' `GlobalTransform`s are brought up to date in the same step, and propagation's change tracking stays correct. The builder chooses how, and states it in the module notes.
- **Produces the messages:**
  - `SkeletonEvent { entity, track: usize, animation: Arc<str>, name: Arc<str>, int, float, string: Arc<str>, audio: Option<EventAudio>, time: f32 }`;
  - `SkeletonTrackEvent { entity, track: usize, animation: Arc<str>, kind: TrackEventKind }`.
- **Produces `update_skeletons`**, in `FixedSet::Skeletons`. Its order per skeleton:
  1. advance by the game step × `time_scale`;
  2. apply;
  3. apply overrides;
  4. compute world from the entity's `GlobalTransform` (this step's);
  5. place sockets;
  6. send messages.
  - A paused game clock means `dt = 0` and nothing advances.
- **Hot reload:** a data version change rebuilds the pose by bone and slot name, keeps tracks by animation name (clearing a missing one with one warning), and keeps the skin and overrides by name. Use the particles precedent of caching per (`AssetId`, version).

- [ ] Step 1: Write the tests in `tests/skeleton.rs`:
  - `a_socket_is_where_its_bone_is_in_the_same_step` (the socket's `GlobalTransform` equals the bone world × the offset, to 1e-4, while the skeleton's entity moves this step);
  - `a_socket_child_s_children_follow_in_the_same_step`;
  - `events_reach_the_game_as_messages_in_time_order`;
  - `track_events_reach_the_game`;
  - `a_world_space_override_aims_a_bone_at_a_point`;
  - `a_paused_clock_freezes_animation_and_events`;
  - `a_reload_keeps_the_playing_animation_and_its_time`;
  - `a_reload_that_removes_the_playing_animation_clears_the_track_with_one_warning`;
  - `a_reload_that_removes_a_socket_s_bone_warns_once_and_keeps_playing` (Review Focus 1);
  - `a_skeleton_waits_for_its_data_and_keeps_the_animation_set_meanwhile` (Review Focus 4);
  - `skeletons_sharing_data_keep_their_own_skin_overrides_and_attachments` (Review Focus 5);
  - `a_despawned_skeleton_sends_nothing_more`.
  - Run them: they fail.
- [ ] Step 2: Implement it. Make the tests pass. Run `cargo test --workspace` to a log.
- [ ] Step 3: Commit with `feat: skeletons animate in the world, with events, bone overrides and sockets that keep pace`.

### Task 8: Drawing skeletons like sprites

**Files:**
- Modify:
  - `src/core/ecs/renderable.rs` (`RenderStyle::Skeleton`, `Renderable::skeleton() -> Self`, whose image is unused);
  - `src/core/render/entities.rs` (a skeleton drawable in `in_draw_order` and `collect_entity`);
  - `src/core/render/draw_list.rs` (if a run needs the `Entity` kind: `push_run_as(kind, …)` or equivalent);
  - `src/core/engine/extract.rs` (pass `Assets<SkeletonData>` and the skeleton query to the entity drawer).
- Create: `src/core/skeleton/draw.rs` (region quads from a pose), `src/core/skeleton/debug.rs` (`debug:skeletons`).
- Test: `tests/e2e/skeleton.rs` (new), and add it to the e2e harness's module list.

**Interfaces:**
- Consumes: `Skeleton`'s two world-matrix sets, `Pose`'s colours, attachments and draw order, `Region`.
- **Produces drawing:**
  - An entity with `Skeleton` and a `Renderable` whose style is `RenderStyle::Skeleton` sorts by its `DrawOrder` (z, `YSort` key from its feet as for sprites, storey) like any entity.
  - It emits its region quads, in the current draw order, as `DrawKind::Entity` items at its place in the entity order. Consecutive quads share one run until the texture or blend changes.
  - Each quad's colour is: slot colour × attachment colour × `Renderable` tint and alpha.
  - Bones are interpolated by the frame's overstep fraction, between the last two steps' matrices, with shortest-arc rotation as `render_global` does. Skipping interpolation on the entity skips it here too.
  - `Floor`/`DrawLift` lift it; `lit` follows the entity; `flip_x`/`flip_y` mirror about the entity's origin.
  - Drop, blob and contact shadows draw as for a sprite: per region for drop shadows, and at the skeleton's bounds for blob and contact.
- **Produces debugging:** `debug:skeletons` (a console command in dev-tools builds, and an overlay) draws bones as lines with joints, bounding boxes and points, unlit, on top.

- [ ] Step 1: Write the e2e pixel tests:
  - `a_region_on_a_rotated_bone_draws_where_expected`;
  - `a_draw_order_key_swaps_which_region_is_on_top`;
  - `a_y_sorted_skeleton_sorts_among_ysort_sprites_by_its_feet`;
  - `a_y_sorted_skeleton_sorts_within_a_y_sorted_tile_layer`;
  - `a_skeleton_is_lit_by_a_light_and_sits_on_its_floor_s_lift`;
  - `two_skeletons_at_different_z_put_a_sprite_between_them`;
  - `a_skeleton_casts_a_drop_shadow`;
  - `a_flipped_skeleton_mirrors_its_regions_and_sockets` (Review Focus 3);
  - `skeleton_bones_are_interpolated_between_steps` (pixels at an overstep of 0.5 sit half way).
  - Run them: they fail.
- [ ] Step 2: Implement it. Make the tests pass, then run `cargo test -p ryo2d --test e2e`.
- [ ] Step 3: Commit with `feat: skeletons draw like sprites: sorted, lit, floored, shadowed and interpolated`.

### Task 9: Allocation guard and bench

**Files:**
- Modify: `tests/tilemap_alloc.rs`, or the crate's allocation test home (follow where the lighting allocation case lives).
- Create: `benches/skeleton.rs` (registered in `Cargo.toml` `[[bench]]`), `docs/backlog/skeleton.md`.

**Interfaces:**
- Consumes everything above.
- The bench scene: 200 skeletons of 30 bones and 25 region slots, each playing two tracks with a crossfade in progress, lit, in view. It prints:
  - `skeletons_update` (µs per fixed step);
  - `skeletons_extract` (µs per frame, the drawing work in extract);
  - `one_skeleton_update` (µs).
- The guards (release, median of three runs): update ≤ 1000 µs, extract ≤ 600 µs, one skeleton ≤ 5 µs.

- [ ] Step 1: Write `fifty_animated_skeletons_allocate_nothing_per_frame` (lit, two tracks, events firing, a socket each). Run it: it fails if anything allocates (expected red until it is tightened; if green at once, say so in the report).
- [ ] Step 2: Write the bench. Run it three times with `uptime` first in each log. Record the figures and loads in `docs/backlog/skeleton.md` (Measured, Limits).
  - If a guard misses, profile, and fix within this task. Report what changed.
  - Do not loosen a guard.
- [ ] Step 3: Commit with `test: skeletons are guarded for allocations and speed, and their figures are recorded`.

### Task 10: The demo

**Files:**
- Create `examples/demos/skeleton/`, laid out as `examples/demos/particles`:
  - `Cargo.toml`, with the lib `skeleton_demo` and a bin;
  - `Ryo.toml`;
  - `src/lib.rs`, `src/main.rs`, `src/embedded.rs`;
  - `src/pages/*.rs`;
  - `assets/` with a `README.md`;
  - `index.html`, `Trunk.toml`;
  - `tests/pages.rs`.

**Interfaces:**
- Consumes the public API only, through `demo_kit::prelude::*` and `ryo2d::prelude::*`.

- [ ] Step 1: Fetch the art and check it.
  - Download rgsdev "Free CC0 Modular Animated Vector Characters 2D" (https://rgsdev.itch.io/free-cc0-modular-animated-vector-characters-2d).
  - Confirm the CC0 text in the download, and that its parts make a rig: head, torso, upper and lower arms, hands, upper and lower legs, feet.
  - If they don't, use Kenney "Modular Characters" (https://kenney.nl/assets/modular-characters, CC0).
  - Record the choice, the licence and the source in `assets/README.md`.
  - Pack the parts as one image with named regions. The tool is `magick`, and the README gives the command.
  - If neither pack works, stop and report BLOCKED with what is missing.
- [ ] Step 2: Write `tests/pages.rs`: every page builds and steps 120 frames headlessly with no warnings (`HeadlessEngine::new_or_skip`). Run it: it fails, because there are no pages yet.
- [ ] Step 3: Write `assets/hero.skeleton.ron`: a rig with idle, walk, run, jump, wave and aim animations, footstep events, and two skins plus a hat skin. Then write the pages:
  1. **Character:** idle, walk, run and jump on keys, crossfaded.
  2. **Layers:** walk with a wave or aim layer on track 1, with an alpha setting.
  3. **Events:** footsteps play a sound (the event's audio path, at the skeleton's position) and puff dust (`ctx.particles().play_at`).
  4. **Sockets:** a sword on the hand bone, with a trail and its hitbox drawn.
  5. **Skins:** outfit choice, and a hat toggle that combines skins.
  6. **Override:** the head and aiming arm follow the pointer (a world-space `BoneOverride`).
  7. **Crowd:** 200 heroes with varied animations and the frame cost shown.

  Follow `examples/demos/AGENTS.md` (reserved keys, settings, `content_area`). The sound can be a short CC0 or generated footstep, also listed in the README.
- [ ] Step 4: Run the page tests, `./dev.sh demos:tour skeleton`, and look at each PNG. Run `cargo clippy --target wasm32-unknown-unknown -p skeleton` (or the demo's package name).
- [ ] Step 5: Commit with `feat: a new skeleton demo with a cutout hero: mixing, layers, events, sockets, skins, aiming and a crowd`.

### Task 11: Docs

**Files:**
- Create: `.claude/skills/ryo-skeleton/SKILL.md` at version `1.0.0`, registered wherever the CLI lists skills (`src/build.rs`).
- Modify:
  - `.claude/skills/ryo-api/SKILL.md` (a skeletal animation section listing the new API);
  - `.claude/skills/ryo-assets/SKILL.md` (the new asset type in its list);
  - `.claude/skills/ryo-engine-dev/SKILL.md` (the module map and the `Skeletons` set);
  - `.claude/skills/ryo-renderer/SKILL.md` (skeletons in the entity drawer);
  - `src/core/skeleton/AGENTS.md` (complete);
  - `src/core/render/AGENTS.md`, `docs/architecture/overview.md`;
  - `docs/backlog/follow-ups.md` (remove only lines this branch fixed, after confirming each).
  - Bump each skill this branch touched exactly once against `main`.

- [ ] Step 1: Write the skill. It covers:
  - the workflow, from a file or from the builder;
  - the data model;
  - playing and mixing, with the rules;
  - events, including the audio pattern;
  - sockets, including the physics limit;
  - bone overrides, skins and hot reload;
  - drawing, which is like sprites;
  - `debug:skeletons`;
  - limits;
  - testing.
  Every Rust example compiles (`cargo test -p skill-examples`).
- [ ] Step 2: Update the other docs. Run:
  - `cargo test -p skill-examples`;
  - `cargo doc --no-deps -p ryo2d` (only the known `padded.rs` warnings are allowed);
  - `git grep -n -i "SkeletalController\|draw_skeleton\|SkeletonDef"` (empty outside `docs/research`, `docs/specs` and `docs/plans`);
  - `cargo test --workspace` to a log.
- [ ] Step 3: Commit with `docs: the skeleton skill, API listing, module notes and overview describe the new skeletons`.
