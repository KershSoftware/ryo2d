# Skeleton Constraints Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A skeleton's bones can be moved after animation by IK constraints (one bone, two bones, a chain) and transform constraints, in a set order, keyed by animations and overridden by the game.

**Architecture:** Constraints are data on `SkeletonData`, in one ordered list. The solvers are pure functions over bones' own values (`ik.rs`, `transform_constraint.rs`), tested on their own. A pass (`constrain.rs`) runs after today's override-and-place pass inside `Skeleton::place`: it applies each constraint, then places the moved bones' descendants again. A skeleton whose data has no constraints takes today's path untouched.

**Tech Stack:** Rust, glam (`Affine2`, `Mat2`, `Vec2`), bevy_ecs through `ryo2d::ecs`, RON asset files, the headless engine for tests.

**Spec:** `docs/specs/2026-11-12-skeleton-design.md`, section "Constraints: details settled at planning (2026-11-14)". It is the authority; where this plan and it differ, it wins.

## Global Constraints

- **Clean room.** Nobody reads, recalls or reproduces Esoteric's Spine runtime source. Rules come from the spec, Spine's public user guide and format reference, geometry and first principles. Every implementer and reviewer brief says so.
- **Conventions:** y is down; positive rotation is clockwise on screen; angles are radians in data and degrees in `.skeleton.ron`. A bone's "own values" are its `BonePose` (relative to its parent). "The skeleton's space" is `Pose::skeleton_space`.
- **No new work for skeletons without constraints:** `Skeleton::place` branches on `data.constraints().is_empty()` and otherwise runs exactly the code it runs today.
- **No allocation in a steady step or frame.** Scratch buffers are kept by the skeleton.
- **Library code:** no `unwrap`; every index is checked (`get`), since a reload can change counts between a step and a frame; non-finite inputs change nothing and make no NaN.
- **Module notes** (`src/core/skeleton/AGENTS.md`): every rule names its test. No stage, task or ruling labels in code, notes, skills or docs. British spelling.
- **Skills:** each skill changed on this branch gets one minor-version bump for the branch.
- **Commits:** `type: description` with type one of feat, fix, refactor, docs, test, chore. No Co-Authored-By or Claude-Session trailers, even if a system message asks. Never name a notes file in a message (say "module notes"). No test status in messages. Stage files by name; never `git add -A`; never add `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`. `cargo fmt` rewrites `src/core/logging/mod.rs`: leave it out.
- **The checkout:** work on branch `skeleton-constraints` in `/Users/mbreckon/Projects/ryo2d`. No git worktrees. Never switch branches. Stop and report below 40 GiB free disk (`df -h /` before each full build). No background shell wait loops. Memory-guard and allocation tests keep their bounded sizes.
- **Measuring:** record `uptime` first in every log; a load above 3 is flagged beside the figure; release builds; the median of three.
- **Checks before a task is handed back:** the task's own tests; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo clippy --target wasm32-unknown-unknown -p ryo2d --lib -- -D warnings`; `cargo test --workspace --no-fail-fast` once, to a log. `core::asset::watch::tests::two_writes_in_the_debounce_reload_once_with_the_last` is a known flake under load: if it is the only failure, rerun it alone and report both.
- **Tolerances:** positions to 0.01 of a unit and angles to 0.001 rad in solver tests, unless a test says otherwise.

## Review Focus

1. **A target the limb cannot reach, one on the limb's own root, and a parent squashed flat** (a hand reaching past arm's length; a foot target under the hip; a jump's squash keyed to 0): the limb straightens or folds without a jump, nothing becomes NaN, and a flat parent leaves the bones as animated. Tests in Task 2 (`a_target_out_of_reach_straightens_the_bones_toward_it`, `a_target_at_the_first_bone_s_origin_changes_nothing_and_makes_no_nan`) and Task 4 (`a_flat_parent_leaves_the_bones_as_animated`).
2. **A character turned round** (a flipped entity, a mirrored ancestor): its knees and elbows bend on the same side of its body as before. Test in Task 4: `a_flipped_entity_and_a_mirrored_ancestor_keep_the_bend_side`.
3. **A game override near a constraint** (a foot held level under an IK shin; an override on a bone IK also turns; an override moving the target): the foot stays level, the constraint has the last word on its own bone, and the target is reached in the same step. Tests in Task 4: `a_world_override_below_an_ik_bone_still_holds`, `a_constraint_has_the_last_word_over_an_override_on_its_own_bone`, `a_target_moved_by_a_world_override_is_reached_in_the_same_step`.
4. **A hot reload that removes, reorders or retargets constraints** while one is overridden and another is keyed: the skeleton keeps playing, overrides follow their names, a gone name warns once, and nothing indexes out of range. Test in Task 6: `a_reload_that_removes_or_reorders_constraints_keeps_playing`.
5. **A crossfade from an animation with IK on to one with it off**: the limb eases from the solved pose to the animated one over the mix, with no pop at either end. Test in Task 6: `a_keyed_mix_fades_with_its_animation_without_a_pop`.

---

### Task 1: Constraints in the data and the file

**Files:**
- Create: `src/core/skeleton/constraint.rs`.
- Modify: `src/core/skeleton/{mod.rs,data.rs,builder.rs,validate.rs,AGENTS.md}`, `src/core/asset/loaders/skeleton.rs`, `src/lib.rs`.

**Interfaces:**
- Produces, in `constraint.rs` (all `Debug, Clone, PartialEq`):
  - `pub struct ConstraintId(pub u8)` with `index()`.
  - `pub enum Constraint { Ik(IkConstraint), Transform(TransformConstraint) }` with `name(&self) -> &Arc<str>`, `bones(&self) -> &[BoneId]`, `target(&self) -> BoneId`.
  - `pub struct IkConstraint { pub name: Arc<str>, pub bones: Vec<BoneId>, pub target: BoneId, pub setup: IkSetup }`.
  - `pub struct IkSetup { pub mix: f32, pub softness: f32, pub bend_positive: bool, pub compress: bool, pub stretch: bool, pub uniform: bool }`; `Default` is mix 1, softness 0, `bend_positive` true, the rest false.
  - `pub struct TransformConstraint { pub name: Arc<str>, pub bones: Vec<BoneId>, pub target: BoneId, pub setup: TransformSetup }`.
  - `pub struct TransformSetup { pub mix: TransformMix, pub offset: TransformOffset, pub local: bool, pub relative: bool }` (`Default`).
  - `pub struct TransformMix { pub rotate: f32, pub x: f32, pub y: f32, pub scale_x: f32, pub scale_y: f32, pub shear_y: f32 }`: `Default` is all 0; `TransformMix::ALL` is all 1.
  - `pub struct TransformOffset { pub rotation: f32, pub x: f32, pub y: f32, pub scale_x: f32, pub scale_y: f32, pub shear_y: f32 }`: `Default` is all 0; angles in radians.
  - `pub const MAX_CONSTRAINTS: usize = 255` and `pub const MAX_IK_BONES: usize = 16`.
- Produces elsewhere:
  - `SkeletonData::constraints(&self) -> &[Constraint]` (application order) and `SkeletonData::constraint(&self, name: &str) -> Option<ConstraintId>`.
  - `SkeletonDataBuilder::ik(self, name: impl Into<Arc<str>>, bones: &[&str], target: &str, setup: IkSetup) -> Self` and `SkeletonDataBuilder::transform_constraint(self, name: impl Into<Arc<str>>, bones: &[&str], target: &str, setup: TransformSetup) -> Self`. Constraints apply in the order these are called. Names resolve at `build`.
  - Prelude: `SkeletonConstraint` (the enum), `SkeletonConstraintId`, `IkConstraint`, `IkSetup`, `TransformConstraint`, `TransformSetup`, `TransformMix`, `TransformOffset`.
- **Validation** (in `validate::build`, after bones, before animations; item text `constraint "leg"`). Each rule fails with the constraint named and the reason:
  - more than 255 constraints; a name used twice (across both kinds);
  - a bone or the target is not a bone; a bone listed twice; the target is one of the bones or a descendant of one;
  - IK: no bones, or more than 16; a bone after the first is not an immediate child of the one before; a bone that does not inherit normally; `mix` outside 0 to 1; `softness` not finite or negative;
  - IK settings that cannot apply: `bend_positive: false` or `softness > 0` on other than two bones; `compress` on other than one; `stretch` or `uniform` on three or more;
  - IK lengths: with two or more bones the last bone's `length` is not above 0; with one bone and `stretch` or `compress` its `length` is not above 0;
  - transform: no bones; a mix or an offset not finite.
- **File format** (top level of `.skeleton.ron`, after `slots`; `#[serde(default)]`, `deny_unknown_fields`; angles in degrees):

```ron
constraints: [
  Ik(name: "leg", bones: ["thigh", "shin"], target: "foot_target", bend_positive: false, softness: 6),
  Ik(name: "look", bones: ["head"], target: "look_target", mix: 0.5, stretch: true, uniform: true),
  Transform(name: "pelvis", bones: ["pelvis"], target: "torso", mix: (rotate: 0.6), offset: (rotation: 90), local: true),
],
```

- [ ] Step 1: Write the tests.
  - `validate.rs`: one test per rule above, in the module's style (`fails(builder, item, reason)`), each asserting the item `constraint "<name>"` and the reason's text. Names: `a_constraint_name_used_twice_fails`, `a_constraint_s_unknown_bone_or_target_fails`, `a_constraint_s_target_among_or_below_its_bones_fails`, `an_ik_bone_that_is_not_its_predecessor_s_child_fails`, `an_ik_bone_that_does_not_inherit_normally_fails`, `an_ik_mix_or_softness_out_of_range_fails`, `an_ik_setting_that_cannot_apply_fails`, `an_ik_tip_without_a_length_fails`, `a_transform_constraint_without_bones_or_with_a_value_not_finite_fails`, `too_many_constraints_or_ik_bones_fail`. Plus `constraints_keep_the_order_they_were_added_in`.
  - Loader: `a_file_s_constraints_load_and_match_the_builder_s` (the three examples above against their builder twins, with `assert_eq!` on `data.constraints()`); extend `a_file_using_every_feature_loads_as_the_builder_builds_it` and the rustdoc example with one IK and one transform constraint; extend `a_mistake_in_the_file_names_the_file_and_the_item` with an unknown target and a misspelt field.
  - Run `cargo test -p ryo2d --lib skeleton::validate loaders::skeleton`: the new tests fail to compile or fail.
- [ ] Step 2: Implement the types, the builder methods, validation and the loader. A bone's descendants are found from `parent` links (parents come first).
- [ ] Step 3: Run the tests: pass. Update the module notes (each rule with its test).
- [ ] Step 4: Commit: `feat: skeleton data and files hold IK and transform constraints`.

### Task 2: One bone and two bones solved toward a target

**Files:**
- Create: `src/core/skeleton/ik.rs`.
- Modify: `src/core/skeleton/{mod.rs,pose.rs,AGENTS.md}`.

**Interfaces:**
- Consumes: `BonePose` (`pose.rs`), `apply::shortest`.
- Produces, in `ik.rs` (crate-internal, pure: no pose, no data):
  - `#[derive(Debug, Clone, Copy, PartialEq)] pub struct IkValues { pub mix: f32, pub softness: f32, pub bend_positive: bool, pub compress: bool, pub stretch: bool }` (public: Task 6 keys it) with `IkValues::of(setup: &IkSetup) -> Self`.
  - `pub(crate) fn one_bone(bone: &mut BonePose, length: f32, target: Vec2, values: IkValues, uniform: bool)`.
  - `pub(crate) fn two_bones(first: &mut BonePose, second: &mut BonePose, length: f32, target: Vec2, values: IkValues, uniform: bool)`.
  - `target` is in the space of the first bone's parent. `length` is the last bone's data length. Both change only the bones' own rotation and scale.
  - `BonePose::affine(&self) -> Affine2` (its own transform in its parent's space), if `pose.rs` has none.
- **Rules** (the spec's "IK constraints"; all in the first bone's parent's space):
  - `mix <= 0`, or any input not finite, changes nothing.
  - **One bone:** the rotation that points its x axis at the target (less its `shear_x`; the other way round under a negative `scale_x`, as `apply_override` does), blended the short way by `mix`. A target on the bone's origin changes nothing. `stretch` (target beyond `length × |scale_x|`) and `compress` (nearer) multiply `scale_x` by `1 + (distance / reach - 1) × mix`, and `scale_y` too with `uniform`.
  - **Two bones:** part one is the vector from the first bone's origin to the second's (the second's `x, y` under the first's scale and shear), part two is `length × |second.scale_x|` under the first's scale, both at their lengths in the pose passed in. Solve the triangle by the law of cosines for the distance to the target after softness; the bend side by `bend_positive` as the spec defines it (the turn from part one to part two is positive); blend each bone's rotation the short way by `mix`.
  - Softness: `s = softness.min(m)`; a distance `d > m - s` becomes `(m - s) + s × (1 - exp(-(d - (m - s)) / s))`.
  - Out of reach: straight toward the target. Nearer than `|l1 - l2|`: folded. A part of length 0 (the second bone on the first's origin): the first bone is left as it is and the second points at the target.
  - Stretch (softness 0, the first bone's `|scale_x| == |scale_y|` within 1e-4 and no shear): as one bone, on the first bone.

- [ ] Step 1: Write the tests in `ik.rs`, with a helper `tip(first, second, length) -> Vec2` built from `BonePose::affine`:
  - `one_bone_points_at_its_target_at_each_mix` (mix 0, 0.5, 1; the half mix is half the short-way turn);
  - `one_bone_stretches_and_compresses_to_its_target` (with and without `uniform`, and at mix 0.5);
  - `one_bone_under_a_negative_scale_or_a_shear_still_points_at_its_target`;
  - `two_bones_reach_a_target_on_each_bend_side` (tip within 0.01; the joint on the side `bend_positive` says, by the sign of the cross product of part one and part two);
  - `two_bones_reach_with_the_second_bone_off_the_first_s_axis` (second bone at `(40, 12)`);
  - `a_target_out_of_reach_straightens_the_bones_toward_it` (Review Focus 1);
  - `a_target_too_near_folds_the_bones`;
  - `softness_slows_the_tip_as_the_bones_straighten` (the tip's distance equals the formula at five distances across `m - s`, and never reaches `m`; softness above `m` is taken as `m`);
  - `a_half_mix_turns_each_bone_half_way_the_short_way`;
  - `two_bones_stretch_to_a_target_out_of_reach` (exact on the axis; exact with `uniform` off it; not applied with softness or an uneven first bone);
  - `a_first_bone_mirrored_by_its_own_scale_keeps_the_bend_side` (scale `(1, -1)` and `(-1, 1)`);
  - `a_target_at_the_first_bone_s_origin_changes_nothing_and_makes_no_nan` (Review Focus 1; also a second bone on the first's origin, a zero scale, and a NaN target);
  - `an_uneven_first_bone_misses_by_a_bounded_amount` (scale `(1.2, 1)`: the result is finite and the miss is under a tenth of the reach; pins the recorded limit).
  - Run `cargo test -p ryo2d --lib skeleton::ik`: fail.
- [ ] Step 2: Implement `one_bone` and `two_bones`.
- [ ] Step 3: Run: pass. Module notes: the rules with their tests, and the uneven-first-bone limit.
- [ ] Step 4: Commit: `feat: one bone and two bones are solved toward a target`.

### Task 3: A chain reaches for a target

**Files:**
- Modify: `src/core/skeleton/{ik.rs,AGENTS.md}`.

**Interfaces:**
- Produces: `pub(crate) fn chain(bones: &mut [BonePose], length: f32, target: Vec2, mix: f32, joints: &mut Vec<Vec2>)`.
  - `bones` are first to last, each the child of the one before, in their own values; `target` is in the first bone's parent's space; `joints` is scratch the caller keeps (cleared and filled here, never shrunk).
  - `pub(crate) const CHAIN_PASSES: usize = 16` and `CHAIN_NEAR: f32 = 0.001` (a pass stops when the tip is within `CHAIN_NEAR ×` the chain's length of the target).
- **Rules:** joint positions come from the pose passed in (each part rigid, at its length there). Forward and backward reaching from that pose, at most `CHAIN_PASSES` passes. A target beyond the chain's length lays the chain straight toward it without iterating. Each bone's rotation is then turned by the angle its part turned relative to its parent's part (so only rotations change), blended the short way by `mix`. Parts of length 0 are skipped. Non-finite input, `mix <= 0` or fewer than two bones changes nothing.

- [ ] Step 1: Write the tests in `ik.rs`:
  - `a_chain_reaches_a_target_in_range` (four and sixteen bones; tip within `CHAIN_NEAR ×` length);
  - `a_chain_out_of_reach_lies_straight_toward_its_target`;
  - `a_chain_gives_the_same_result_every_run` (two runs from the same pose are bit-equal);
  - `a_chain_starts_from_the_animated_pose` (two poses bent opposite ways reach one target with their bends kept on their own sides);
  - `a_chain_at_half_mix_turns_each_bone_half_way`;
  - `a_chain_changes_only_rotations` and `a_chain_takes_no_more_room_on_a_second_run` (the scratch's pointer and capacity);
  - `a_chain_with_a_part_of_no_length_or_a_value_not_finite_makes_no_nan`.
  - Run: fail.
- [ ] Step 2: Implement `chain`.
- [ ] Step 3: Run: pass. Module notes.
- [ ] Step 4: Commit: `feat: a chain of bones reaches for a target`.

### Task 4: Skeletons apply their IK constraints each step, in order

**Files:**
- Create: `src/core/skeleton/constrain.rs`.
- Modify: `src/core/skeleton/{mod.rs,pose.rs,component.rs,data.rs,validate.rs,system.rs,AGENTS.md}`.
- Test: in those files, `tests/skeleton.rs`, and a new `tests/e2e/skeleton_constraints.rs` (registered in `tests/e2e.rs`).

**Interfaces:**
- Consumes: Tasks 1 to 3.
- Produces:
  - **In the pose:** `Pose::constraints: Vec<ConstraintValues>`, one per constraint in order, with `pub(crate) enum ConstraintValues { Ik(IkValues), Transform(TransformMix) }`; `Pose::setup` and `Pose::reset` put each back to its constraint's setup without allocating.
  - **In the data** (built once in `validate::build`, kept in `SkeletonIndex`): for each constraint, the bones to place again after it (its bones' descendants in bone order, and for IK its bones too); and the set of bones any IK constraint changes.
  - **The pass:** `pub(crate) fn constrain(pose: &mut Pose, data: &SkeletonData, root: Affine2, overrides: &[SetOverride], state: &mut Constrained)` in `constrain.rs`, called from `Skeleton::place` after its override-and-place loop and before `apply_root`, only when `!data.constraints().is_empty()`.
  - `pub(crate) struct Constrained` (a field of `Skeleton`): the animated own values of the bones constraints change, which bones a constraint has changed so far in this pass, and the chain's `joints` scratch. Sized once per data.
  - Transform constraints are skipped by the pass in this task (Task 5 fills them in).
- **Rules** (the spec's "Order and the step"):
  - For each constraint in order: bring the target (`skeleton_space[target].translation`) into the first bone's parent's space (`Pose::parent_space`; not invertible: skip the constraint); call `one_bone`, `two_bones` or `chain` with `pose.constraints[i]`'s values and the data's `uniform`; mark the bones changed; place the constraint's listed bones again with `Pose::place_bone`.
  - A bone placed again that has a game override and has not been changed by a constraint gets its override applied afresh (from its animated own values, through `apply_override`) before it is placed. A bone a constraint has changed keeps the constraint's values.
  - `Skeleton::place_again` and `Skeleton::appear` give the pose a step gives: before the pass, the bones constraints change are put back to their animated own values.
  - `rebuild` sizes `Constrained` for the new data.

- [ ] Step 1: Write the tests.
  - `constrain.rs` / `component.rs` (with `component::tests::posed`-style rigs built by the builder):
    - `a_one_bone_two_bone_and_chain_constraint_each_reach_their_target_in_a_step` (tip from `bone_world`);
    - `constraints_apply_in_their_order` (two constraints on one bone, in each order, give the later one's result);
    - `a_constraint_places_the_descendants_of_its_bones_again` (a hand below an IK arm follows it in the same step);
    - `a_target_moved_by_a_world_override_is_reached_in_the_same_step` (Review Focus 3);
    - `a_world_override_below_an_ik_bone_still_holds` (Review Focus 3: a foot with a world rotation override under an IK shin keeps its world angle to 0.001 rad);
    - `a_constraint_has_the_last_word_over_an_override_on_its_own_bone` (Review Focus 3);
    - `a_flat_parent_leaves_the_bones_as_animated` (Review Focus 1: a parent scaled to 0);
    - `a_flipped_entity_and_a_mirrored_ancestor_keep_the_bend_side` (Review Focus 2: the joint's side relative to the limb, in the skeleton's own space, is the same flipped and not);
    - `a_skeleton_placed_again_has_its_step_s_pose` (`place_again` twice more leaves `skeleton_space` equal);
    - `a_skeleton_posed_in_the_frame_it_appears_has_its_constraints_applied` (`appear`);
    - `the_pose_s_constraint_values_return_to_setup_each_step_without_allocating` (pointer and capacity);
    - `a_skeleton_without_constraints_keeps_no_constraint_state` (`Constrained` empty, its buffers unallocated).
  - `tests/skeleton.rs`: `a_socket_on_a_constrained_bone_follows_it_in_the_same_step` (the probe after `FixedSet::Skeletons`).
  - `tests/e2e/skeleton_constraints.rs` (helpers shared from `skeleton.rs` as `skeleton_mesh.rs` does): `a_two_bone_arm_reaches_a_moved_target` (pixels of the hand at the target, at two targets); `a_constrained_skeleton_flips_sorts_and_draws_between_steps_as_any_other` (flip mirrors the pixels; a y-sorted pair keeps its order; a frame between steps draws between the two poses).
  - Run: fail.
- [ ] Step 2: Implement the pose's values, the data's lists, `Constrained`, the pass and the calls. Keep `Skeleton::place`'s present code as the path for data without constraints.
- [ ] Step 3: Run the task's tests, `cargo test -p ryo2d --test skeleton`, `cargo test -p ryo2d --test e2e skeleton`: pass. Module notes.
- [ ] Step 4: Commit: `feat: skeletons apply their IK constraints each step, in order`.

### Task 5: Transform constraints

**Files:**
- Create: `src/core/skeleton/transform_constraint.rs`.
- Modify: `src/core/skeleton/{mod.rs,constrain.rs,data.rs,validate.rs,AGENTS.md}`.
- Test: in those files and `tests/e2e/skeleton_constraints.rs`.

**Interfaces:**
- Consumes: Task 4's pass and lists.
- Produces, in `transform_constraint.rs` (crate-internal, pure):
  - `pub(crate) fn in_the_world(bone: &mut Affine2, target: &Affine2, mix: &TransformMix, offset: &TransformOffset, relative: bool)`: both in the skeleton's space.
  - `pub(crate) fn in_its_own_values(bone: &mut BonePose, target: &BonePose, mix: &TransformMix, offset: &TransformOffset, relative: bool)`.
- **Rules** (the spec's "Transform constraints"; a mix of 0 for a property leaves it bit-equal):
  - World, not relative: place moves toward `target.transform_point2((offset.x, offset.y))` by `mix.x` and `mix.y`; the x axis turns the short way toward the target's x axis's angle plus `offset.rotation`, the whole matrix with it, by `mix.rotate`; each axis's length moves toward the target's axis's length plus the offset; shear (how far the y axis is from square to the x axis, on the bone's own side: the sign of its determinant) moves toward the target's plus `offset.shear_y`, turning the y axis alone.
  - World, relative: place adds `target.transform_point2(offset xy) × mix`; the matrix turns by `(target's x-axis angle + offset.rotation) × mix.rotate`; each axis's length is multiplied by `1 + (target's length - 1 + offset) × mix`; shear adds `(target's shear + offset.shear_y) × mix.shear_y`.
  - Under a target whose determinant is negative, `offset.rotation` and `offset.shear_y` change sign in the world modes. A bone's own determinant never changes sign.
  - Own values, not relative: `value += (target's + offset - value) × mix` for rotation (plain numbers, not the short way), x, y, scale x, scale y and shear y.
  - Own values, relative: rotation, x, y and shear y add `(target's + offset) × mix`; each scale is multiplied by `1 + (target's - 1 + offset) × mix`.
  - A length under 1e-6 (a flat bone or target) leaves rotation, scale and shear alone in the world modes; non-finite input changes nothing.
- **In the pass:** a transform constraint's bones are taken in bone order. A world mode sets `pose.skeleton_space[bone]` and places the bone's descendants again; an own-values mode changes `pose.bones[bone]`, marks it changed, and places it and its descendants again. Each bone is finished before the next is read. The data's lists gain the transform constraints (the bones an own-values mode changes join the set that is put back to animated values before a pass).

- [ ] Step 1: Write the tests.
  - `transform_constraint.rs`: for each of the four modes, `..._copies_each_property_by_its_own_mix` (each of the six mixes alone at 1 leaves the other five properties bit-equal and moves its own to the expected value), with offsets; `mixes_of_minus_one_a_half_and_two_go_where_the_rule_says`; `a_mirrored_target_mirrors_the_rotation_and_shear_offsets`; `a_mirrored_bone_stays_mirrored_and_an_unmirrored_one_unmirrored`; `an_own_values_relative_half_mix_turns_smoothly_through_its_target_s_full_turn` (the target's own rotation from 0 to 2π in 64 steps: no step of the bone exceeds π/32 + 1e-4); `a_world_half_mix_jumps_when_its_target_passes_half_a_turn` (pins the recorded limit); `a_flat_bone_or_target_changes_nothing_it_cannot_and_makes_no_nan`.
  - `constrain.rs`: `a_world_mode_result_is_lost_when_a_later_constraint_moves_an_ancestor`; `an_own_values_result_is_kept_when_a_later_constraint_moves_an_ancestor`; `a_bone_and_its_descendant_in_one_constraint_are_both_constrained`; `a_world_mode_ignores_the_bone_s_inherit_mode`; `an_own_values_mode_reads_the_target_as_animation_overrides_and_earlier_constraints_left_it`; `a_transform_constraint_then_an_ik_constraint_and_the_other_way_round` (each order gives what the order rule says); `a_skeleton_placed_again_has_its_step_s_pose` extended with both modes.
  - `tests/e2e/skeleton_constraints.rs`: `a_part_held_level_stays_level_while_its_arm_turns` (pixels).
  - Run: fail.
- [ ] Step 2: Implement the two functions and the pass's transform arm.
- [ ] Step 3: Run: pass. Module notes, with the world-mode fraction limit.
- [ ] Step 4: Commit: `feat: transform constraints copy a target bone's transform`.

### Task 6: Animations key constraints, and a game overrides them

**Files:**
- Modify: `src/core/skeleton/{animation.rs,apply.rs,state.rs,validate.rs,component.rs,constrain.rs,pose.rs,AGENTS.md}`, `src/core/asset/loaders/skeleton.rs`, `src/lib.rs`.
- Test: in those files and `tests/skeleton.rs`.

**Interfaces:**
- Consumes: Tasks 1, 4 and 5.
- Produces:
  - `Timeline::Ik { constraint: ConstraintId, frames: Vec<Frame<IkValues>> }` (`kind()` "ik") and `Timeline::TransformMix { constraint: ConstraintId, frames: Vec<Frame<TransformMix>> }` (`kind()` "transform mix"); `Target::Constraint(ConstraintId)`; one property each, so two timelines for one constraint clash and a crossfade's cover rule treats a constraint as one property.
  - **Applying** (into `pose.constraints`): `mix`, `softness` and the six mixes sample by the key's curve and blend as bone values do (`Replace` toward the key from what the layers below left; `Add` adds the key's difference from setup, times alpha). `bend_positive`, `compress` and `stretch` are stepped and apply only in `MixDirection::In`, as attachment keys do. After all tracks, an IK `mix` is clamped to 0 to 1 and `softness` to 0 or more.
  - **Validation:** the constraint exists and is of the timeline's kind (item `animation "walk" timeline 3 (ik)`); values finite; IK `mix` 0 to 1 and `softness` not negative; a key sets no switch its constraint could not have (the Task 1 rules).
  - **File format**, in an animation (a value a key leaves out is the constraint's setup value):

```ron
constraints: {
  "leg": (ik: [(0.0, (mix: 1)), (0.3, (mix: 0, bend_positive: true), Bezier(0.3, 0, 0.7, 1))]),
  "pelvis": (transform: [(0.0, (rotate: 0.6)), (0.5, (rotate: 0.9, x: 0.2))]),
},
```

  - Prelude: `IkValues` (a key's value), beside the names below.
  - **The game's side** (prelude `SkeletonConstraintOverride`, `IkOverride`, `TransformMixOverride`):
    - `pub enum ConstraintOverride { Ik(IkOverride), Transform(TransformMixOverride) }`;
    - `pub struct IkOverride { pub mix: Option<f32>, pub softness: Option<f32>, pub bend_positive: Option<bool>, pub compress: Option<bool>, pub stretch: Option<bool> }` (`Default`: all `None`);
    - `pub struct TransformMixOverride { pub rotate: Option<f32>, pub x: Option<f32>, pub y: Option<f32>, pub scale_x: Option<f32>, pub scale_y: Option<f32>, pub shear_y: Option<f32> }` (`Default`: all `None`);
    - `Skeleton::override_constraint(&mut self, name: &str, o: ConstraintOverride)` and `Skeleton::clear_constraint_override(&mut self, name: &str)`.
  - Overrides are applied to `pose.constraints` in `Skeleton::finish` after the tracks and before `place`; they are kept by name through `rebuild`. A non-finite value is dropped with one warning (as bone overrides); an IK `mix` is clamped. An unknown name, or an override of the other kind, warns once per name (the skeleton's existing warned-names list and its cap) and does nothing. Before the data lands the call is kept.

- [ ] Step 1: Write the tests.
  - `apply.rs` / `state.rs`: `an_ik_key_sets_its_constraint_s_values_by_alpha`; `a_transform_key_sets_its_mixes_by_alpha`; `an_add_layer_adds_a_constraint_s_difference_from_setup`; `a_constraint_s_switches_change_at_their_key_and_follow_the_attachment_rule_in_a_crossfade`; `a_keyed_mix_fades_with_its_animation_without_a_pop` (Review Focus 5: a crossfade from mix 1 to an animation with no IK timeline over setup mix 0: the tip's distance from the target grows monotonically from 0 and no step moves it more than a twentieth of the whole way plus 0.01); `a_crossfade_between_two_keyed_mixes_has_no_dip`; `an_ik_mix_is_clamped_after_the_tracks`.
  - `validate.rs`: `a_constraint_timeline_for_an_unknown_constraint_or_the_other_kind_fails`; `two_timelines_for_one_constraint_fail`; `a_constraint_key_out_of_range_or_with_a_switch_it_cannot_have_fails`.
  - Loader: `constraint_timelines_load_and_match_the_builder_s` (a key's left-out values are the setup's); the every-feature file and the rustdoc example gain both kinds.
  - `component.rs`: `an_override_beats_the_animation_and_clearing_it_gives_the_animation_back`; `an_override_forces_a_switch`; `an_override_for_an_unknown_constraint_or_of_the_other_kind_warns_once`; `an_override_value_not_finite_is_dropped_with_one_warning`.
  - `tests/skeleton.rs`: `a_reload_that_removes_or_reorders_constraints_keeps_playing` (Review Focus 4: a file with two IK constraints and one transform constraint, one overridden and one keyed; saves that reorder them, retarget one, remove one and bring it back; after each the animation's time advances, the override still applies to its name, a gone name warns once, and no warning repeats); `turning_foot_ik_off_with_an_override_frees_the_leg_in_the_same_step`.
  - Run: fail.
- [ ] Step 2: Implement the timelines at every exhaustive match on `Timeline` (`animation.rs`, `apply.rs`, `validate.rs`, `state.rs`'s `blends` and `covers`, the loader), the overrides and the reload.
- [ ] Step 3: Run the task's tests and `cargo test -p ryo2d --test skeleton --test skeleton_reload`: pass. Module notes.
- [ ] Step 4: Commit: `feat: animations key constraints, and a game overrides them`.

### Task 7: The overlay marks constraints

**Files:**
- Modify: `src/core/skeleton/{debug.rs,AGENTS.md}`, `src/core/diagnostics/overlays.rs`.

**Interfaces:**
- Produces: under `debug:skeletons`, for each IK constraint a cross at its target and a line from the last bone's tip to it; for each transform constraint a line from each of its bones to its target. One new colour, `colors::SKELETON_CONSTRAINT`. Positions come from `drawn_bone`, so the marks sit on the drawn pose between steps.

- [ ] Step 1: Write `debug.rs` tests: `the_overlay_marks_an_ik_constraint_s_target_and_reach` and `the_overlay_links_a_transform_constraint_s_bones_to_its_target` (the lines' ends and colour), and extend `tests/e2e/skeleton_constraints.rs` with `the_overlay_draws_its_marks_at_the_target_and_the_tip` (pixels of the colour at both places, through `console_run("debug:skeletons")`). Run: fail.
- [ ] Step 2: Implement. Run: pass.
- [ ] Step 3: Commit: `feat: the skeleton overlay marks constraints and their targets`.

### Task 8: Guards and bench

**Files:**
- Modify: `tests/tilemap_alloc.rs`, `benches/skeleton.rs`, `docs/backlog/skeleton.md`.

**Interfaces:**
- Consumes everything above.
- **The allocation test** gains `fifty_skeletons_with_constraints_allocate_nothing_per_frame`: the crowd rig with two two-bone IK constraints, a four-bone chain, one transform constraint of each mode, a keyed IK mix and a constraint override changed every 30 frames, target bones moved by world overrides each frame. A traced frame allocates nothing more than the same entities drawn as sprites.
- **The bench** gains `skeletons_update_constraints`: 200 skeletons of 30 bones, each with two two-bone IK constraints, one one-bone IK constraint and one transform constraint, targets moved each step. Guard: `UPDATE_MAX` (1,000 µs).
- **Against the build before constraints:** main's bench and this tree's, built apart (export main with `git archive` into a new scratch directory with its own `CARGO_TARGET_DIR`; no branch switch, no worktree; delete both after), run alternately, three each. The cases without constraints must not be more than 5% over. If they are on a quiet machine, profile and fix within this task. Record every set, with loads, in `docs/backlog/skeleton.md`.

- [ ] Step 1: Write the allocation case. Run it in release. If it is green at once, plant an allocation in `constrain` and in `ik::chain` once each, show each failing with the function named, and remove both.
- [ ] Step 2: Write the bench case; assert in the bench that the constraints really move bones in the measured steps (the tips are on their targets). Run the bench three times and the comparison with main. Record the figures.
- [ ] Step 3: Commit: `test: skeleton constraints are guarded for allocations and speed`.

### Task 9: The demo's constraints page, and the hero's pelvis by constraint

**Files:**
- Modify: `examples/demos/skeleton/` (`src/lib.rs`, `src/pages/mod.rs`, new `src/pages/constraints.rs`, `src/embedded.rs`, `assets/hero.skeleton.ron`, new `assets/reacher.skeleton.ron`, `assets/walker.skeleton.ron` and their images, `assets/README.md`, `tests/pages.rs`), and the page descriptions that count pages.

**Interfaces:**
- Consumes the public API only (`ryo2d::prelude` and the demo kit). If the page needs something the public API lacks, stop and report.
- **The page "Constraints"** (ninth):
  - **A reaching arm** (`reacher.skeleton.ron`): two parts and a hand, a two-bone IK constraint whose target the page moves to the pointer with a world-space `override_bone`, and a transform constraint that holds the hand level. Settings: Bend (positive or negative), Softness, Stretch, Mix (through `override_constraint`), and Level hand (the transform constraint's rotate mix).
  - **A walker** (`walker.skeleton.ron`): a body on two two-part legs crossing uneven ground. The page moves the body, keeps each foot's target planted on the ground until the body has passed it, then steps it ahead. Setting: Speed.
  - A readout naming which constraint does what, and a line pointing at the console's `debug:skeletons`.
- **The art** is drawn with `magick` in the demo's palette; `assets/README.md` records the commands and that these parts are drawn for the demo.
- **The hero's pelvis:** `hero.skeleton.ron` gains `Transform(name: "pelvis", bones: ["pelvis"], target: "torso", mix: (rotate: 0.6), offset: (rotation: 90), local: true)` and loses its three hand-keyed pelvis timelines. Run, jump and slash draw as before; idle and walk now turn the pelvis a little (about 1 and 2.4 degrees).

- [ ] Step 1: Extend `tests/pages.rs`: nine pages; the page steps with no warnings from disk and from the embedded files; `the_reaching_arm_s_hand_is_on_the_pointer` (in range: within 2 px; out of range with Stretch off: the arm points at it); `the_bend_setting_moves_the_elbow_to_the_other_side`; `the_walker_s_planted_feet_stay_put_while_its_body_moves` (a planted foot's world position moves less than 1 px over the frames it is planted) and `the_walker_s_feet_are_on_the_ground` (each foot within 2 px of the ground's height under it); `the_level_hand_keeps_its_angle_while_the_arm_turns`; `the_hero_s_pelvis_turns_six_tenths_of_the_way_with_its_trunk` (in run). Run: fail.
- [ ] Step 2: Draw the art, write the two rigs and the page, move the pelvis to the constraint.
- [ ] Step 3: Run the page tests and `./dev.sh demos:tour skeleton`. Look at every PNG of the skeleton demo with the Read tool. Capture the Constraints page in motion (at least eight frames across a reach and a walk cycle, overlay off, and through a harness with `console_run("debug:skeletons")` on) and look at each frame: the elbow and knees bend the right way, no part detaches, the feet do not slide or sink, the hand is level. Compare the hero's other pages with the tour before this task and say what changed. Run the demo's wasm clippy.
- [ ] Step 4: Commit: `feat: the skeleton demo gets a constraints page, and the hero's pelvis follows its trunk by constraint`.

### Task 10: Docs

**Files:**
- Modify: `.claude/skills/ryo-skeleton/SKILL.md` (one minor bump), `.claude/skills/ryo-api/SKILL.md` (one minor bump), any other skill that constraints make stale (one bump each, only if changed), `src/core/skeleton/AGENTS.md` (complete), `docs/architecture/overview.md`, `docs/backlog/skeleton.md` (limits), the root `README.md` if it counts the demo's pages, `docs/backlog/follow-ups.md` (remove only lines this branch fixed, such as the hand-keyed pelvis; add nothing).

- [ ] Step 1: Write the skill's constraints sections, each rule checked against the code or a test before it is written: what a constraint is and when to use each kind; target bones and moving one from a game; the IK settings, with a worked leg; chains; transform constraints, the four modes in a table, with the pelvis and a gear as examples; the order rule; keys and overrides; what fails to load, with each message; the limits; the overlay; the demo's page. Every `.skeleton.ron` example is loaded once (a throwaway test, not committed) and every Rust example compiles through `tests/skill-examples`.
- [ ] Step 2: Update the other docs. Run `cargo test -p skill-examples`, `cargo doc --no-deps -p ryo2d` (only the two known `padded.rs` warnings), `cargo test --workspace --no-fail-fast` to a log, and `cargo test --release -p ryo2d` to a log.
- [ ] Step 3: Commit: `docs: the skeleton skill, API listing and module notes describe constraints`.
