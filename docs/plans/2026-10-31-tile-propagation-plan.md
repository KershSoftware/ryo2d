# Transform Propagation (6b, Stage 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform propagation visits only the subtrees that hold a change, so a big static tile map costs nothing to propagate and 6a's static-map frame target (50 µs) is met.

**Architecture:** `propagate_transforms` (`src/core/ecs/propagate.rs`) keeps its early out and its walk, but seeds the walk from the changed entities instead of from every root.
- It collects the entities the `Placement` filter and the removal readers name.
- It drops those with a changed ancestor, since the ancestor's walk reaches them.
- It walks each remaining entity's subtree from its parent's stored `GlobalTransform`, with today's per-node rules.

When very many entities changed, it falls back to today's walk from every root.

**Tech Stack:** Rust, bevy_ecs 0.20 (as `ryo2d::ecs`), criterion benches.

**Spec:** `docs/specs/2026-10-31-tilemap-shapes-design.md` (the "Propagation design" call, Stages 1, and Goal 9). The research behind it is `docs/research/2026-10-31-tile-shapes-seams.md`, section 5.

## Global Constraints

- **Workspace:** work in the main checkout on branch `tile-propagation`. No git worktrees (the owner's rule: they fill the disk). Never switch branches.
- **Git:**
  - Stage files by name. Never `git add -A` or a bare `git stash`, and never stage `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`.
  - Commits use `fix:`, `feat:`, `test:`, `perf:` or `docs:` subjects. No Co-Authored-By, Claude-Session or any other trailer, even if a system reminder asks.
  - Never name AGENTS.md in a commit message (say "module notes").
- **Disk:** check `df -h /` before long builds. Below 40 GiB free, stop and report BLOCKED. Never delete anything under `target/`.
- **No allocation once warm:** a warm propagation run allocates nothing. Every scratch buffer is a `Local` that keeps its capacity, and `tests/tilemap_alloc.rs` guards it.
- **Behaviour is unchanged:** every existing propagation, transform, interpolation and physics test passes untouched. Only the cost changes.
- **Docs stay true:**
  - Update the ECS module notes (`src/core/ecs/AGENTS.md`, its "Propagation" bullet), the `ryo-ecs-internals` skill, and `docs/backlog/tilemap.md` (the record and Limits).
  - Copy a changed `.claude/skills/<name>/SKILL.md` byte for byte to `src/cli/skills/<name>.md`, and bump its `metadata.version` once (a patch).
  - Write plain sentences, with no task labels.
- **The gate**, before the last commit:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo check -p ryo2d --lib --target wasm32-unknown-unknown`
  - `cargo check -p skill-examples`
  - `cargo test -p ryo2d --lib cli::`
  - `cargo test --workspace`
  - `cargo test --test tilemap_alloc`

## Review Focus

1. **Interpolation markers.** Three cases must leave `PreviousGlobalTransform` exactly on the interpolated entities and their descendants, as today:
   - `Interpolated` added to an entity in the middle of a tree;
   - `Interpolated` removed from an entity that has another interpolated ancestor;
   - an entity reparented into or out of an interpolated subtree.

   A walk that starts mid-tree must take its interpolation from the ancestors (Task 1, the interpolation tests).
2. **A parent without a `Transform` in the middle of a chain.** Its children start from the identity and inherit no interpolation, as today. A walk up the ancestors must stop there (Task 1, `a_change_below_a_transform_less_parent_starts_from_the_identity`).
3. **A parent and its child changed in the same run.** The child must compose with the parent's new global, not the stale one (Task 1, `a_parent_and_its_child_changed_together_compose_with_the_new_parent`).
4. **An entity in the changed set that was despawned, or lost its parent, in the same frame.** Nothing panics, and it is placed as a root or skipped (Task 1, `a_changed_entity_despawned_before_the_run_is_skipped`).
5. **Physics moving many bodies at once.** "Every root moved" and "every leaf moved" must not get slower than today's walk (Task 1, the benches, with the fallback).

---

### Task 1: Propagation walks only the subtrees that hold a change

**Files:**
- Modify: `src/core/ecs/propagate.rs` (the system at lines 39-106, and its inline tests from line 133)
- Modify: `benches/foundation.rs` (add two benches beside `propagate 10k, nothing moved` and `every root moved`)
- Modify: `src/core/ecs/AGENTS.md` (the "Propagation" bullet, line 54), `.claude/skills/ryo-ecs-internals/SKILL.md` and its copy `src/cli/skills/ryo-ecs-internals.md`, `docs/backlog/tilemap.md` (the record's static-frame row, its notes, and Limits)

**Interfaces:**
- Consumes: today's `propagate_transforms` signature and queries (`RootItem`, `NodeItem`, `Placement`, `grouping`, the three `RemovedComponents` readers, the `Local` stack), `sync_previous`, and `GlobalTransform::mul_transform`.
- Produces: the same public `propagate_transforms` (it stays the system registered in `FixedSet::Transforms`, `PostUpdateSet::Transforms` and physics' schedule), with the same behaviour, at a cost that follows the changed subtrees. No new public items.

- [ ] **Step 1: Write the regression tests for the cases a mid-tree walk meets** (in `propagate.rs`'s test module, using its existing helpers and schedule set-up). Each passes on today's code; together they pin behaviour through the change.

```rust
#[test]
fn moving_one_leaf_of_a_big_tree_moves_only_that_leaf() {
  // 10 roots x 10 children x 9 grandchildren, settled; move one grandchild by (5, 0)
  // assert: its GlobalTransform == parent_global.mul_transform(&its_transform)
  // assert: every other entity's GlobalTransform is unchanged and not is_changed()
}

#[test]
fn a_child_changed_under_an_unchanged_parent_composes_with_the_parent_s_stored_global() {
  // root at (100, 0) rotated FRAC_PI_2 and scaled 2, settled; then move only the child to (10, 0)
  // assert: child global translation == (100, 20) (close within 1e-4)
}

#[test]
fn a_parent_and_its_child_changed_together_compose_with_the_new_parent() {
  // root -> child -> grandchild, settled; in one frame move the root to (50, 0) and the grandchild to (0, 7)
  // assert: grandchild global == new root global * child local * new grandchild local
}

#[test]
fn a_change_below_a_transform_less_parent_starts_from_the_identity() {
  // root(Transform at (100, 0), Interpolated) -> group (no Transform) -> child(Transform at (3, 4))
  // settled; move the child to (6, 8)
  // assert: child global translation == (6, 8); child has no PreviousGlobalTransform
}

#[test]
fn interpolated_added_mid_tree_marks_its_subtree_only() {
  // root -> a -> b -> c, settled, none interpolated; insert Interpolated on b
  // assert: b and c have PreviousGlobalTransform (skip: true); root and a do not
}

#[test]
fn removing_interpolated_keeps_previous_under_another_interpolated_ancestor() {
  // root(Interpolated) -> a(Interpolated) -> b, settled; remove Interpolated from a
  // assert: a and b keep PreviousGlobalTransform (root still covers them)
  // then remove Interpolated from root
  // assert: root, a and b have none
}

#[test]
fn reparenting_into_and_out_of_an_interpolated_subtree_syncs_previous() {
  // p(Interpolated) and q (plain) roots, settled; x -> y child of q
  // reparent x under p: assert x and y gain PreviousGlobalTransform, and their globals follow p
  // reparent x back under q: assert x and y lose it
}

#[test]
fn a_child_spawned_under_an_unchanged_parent_is_placed_and_marked() {
  // parent(Interpolated) at (10, 10), settled; spawn a child at (1, 0) under it
  // after a run: assert child global translation == (11, 10) and it has PreviousGlobalTransform
}

#[test]
fn a_changed_entity_despawned_before_the_run_is_skipped() {
  // root -> child, settled; move the child, then despawn it before the run; also move another
  // root and remove ChildOf from a third entity in the same frame
  // assert: the run does not panic; the moved root is placed; the detached entity is a root at its local
}
```

- [ ] **Step 2: Run them**

Run: `cargo test -p ryo2d --lib ecs::propagate`
Expected: PASS, every new and existing test. They are guards for the change, not REDs.

- [ ] **Step 3: Add the benches** to `benches/foundation.rs`, on `hierarchy_world()` (100 roots x 10 children x 9 grandchildren):
  - `propagate 10k, one leaf moved`: each iteration moves one grandchild (cycling through them) by `x += 0.001`, then runs the schedule and clears trackers;
  - `propagate 10k, every leaf moved`: moves all 9,000 grandchildren.

  Record today's medians of all four propagation benches as the baseline:

Run: `cargo bench --bench foundation -- propagate`
Expected: `one leaf moved` costs about the same as `every root moved` (today walks all 10,000 either way). Write the four medians into the report.

- [ ] **Step 4: Watch the spec's target miss (the RED)**

Run: `cargo bench --bench tilemap -- '^static_frame$'`
Expected: the guard prints `static_frame: ... missed` (6a recorded +84 to +89 µs against 50). Write the line into the report.

- [ ] **Step 5: Seed the walk from the changed entities** in `propagate_transforms`. Keep the early out, the removal readers, the grouping rule, `sync_previous` and the per-node loop exactly as they are. Replace the seeding (today's loops over every root and every grouping entity, lines 68-87) with this:

```text
1. Collect the run's changed set into a Local Vec<Entity> (cleared each run, capacity kept):
   - every entity the `changed` query (Placement) yields;
   - every entity in `detached`, and every entity whose Interpolated was removed;
   - the children of every entity in `untransformed` (they now start from the identity).
2. If the set holds more than FULL_WALK_AT entities, seed exactly as today (every root, every
   grouping entity) and walk. This keeps "every root moved" and "every leaf moved" at today's cost.
   FULL_WALK_AT is a const; start at 1,024 and set it from Step 6's benches. Say the chosen value
   and why in a comment.
3. Otherwise, for each entity e in the set (skipping any no longer alive, or without a Transform):
   walk up its ChildOf ancestors, stopping at the first parent without a Transform (today's
   grouping rule: children of a Transform-less parent start from the identity and inherit no
   interpolation). On the way:
   - if an ancestor is itself changed (its Ref<Transform> or Ref<ChildOf> is_changed(), it was
     Added<Interpolated>, or it is in detached, uninterpolated or untransformed's children),
     skip e: that ancestor's walk reaches it;
   - else note whether any ancestor has Interpolated.
4. Push each remaining e onto the existing stack as (e, parent_global, true, ancestors_interpolated).
   parent_global is the parent's stored GlobalTransform, or the identity when e has no parent or
   its parent has no Transform. A root's own global comes from its Transform, exactly as the
   roots loop does today, including the `detached.contains` case.
5. Run the existing `while let Some(...) = stack.pop()` loop unchanged, so each seeded subtree is
   recomputed and synced exactly as today.
```

  Roots need a path into step 4 that the node loop doesn't cover, since `nodes` is `With<ChildOf>`. Use the `roots` query for a seeded root, as the roots loop does today. The builder settles how to share that code between step 2 and step 4.

- [ ] **Step 6: Run the tests and the benches**

Run: `cargo test -p ryo2d --lib ecs:: && cargo test --test integration transform && cargo test -p ryo2d --lib physics::sync && cargo test --test tilemap_alloc`
Expected: PASS.

Run: `cargo bench --bench foundation -- propagate`
Expected:
- `one leaf moved` falls to a small fraction of its baseline: the run scans the changed filter and walks one leaf. Record the ratio.
- `nothing moved` is unchanged.
- `every root moved` and `every leaf moved` are within 5% of their baselines, through the fallback. Tune `FULL_WALK_AT` until both hold.

- [ ] **Step 7: Run the spec's guard**

Run: `cargo bench --bench tilemap -- '^static_frame$'`
Expected: `static_frame: ... Held, the median: N µs, met`, with N at most 50.
- If it is met, also run `cargo bench --bench tilemap -- '^edit$'` and record the edit frame's whole step beside 6a's record (no target).
- If it still misses, commit what is green, and do Task 2.

- [ ] **Step 8: Update the docs**
  - **The ECS module notes' "Propagation" bullet:** describe the seeded walk and the fallback. Replace the "Known cost" sentence with what remains: one tick check per entity per run, for the changed filter's scan.
  - **The `ryo-ecs-internals` skill:** update it where it describes propagation's cost, bump its version (a patch), and copy it to `src/cli/skills/`.
  - **`docs/backlog/tilemap.md`:** the static-frame row gets the new measurement and its verdict, and the propagation limit goes from Limits.

- [ ] **Step 9: Run the gate (Global Constraints), then commit**

```bash
git add src/core/ecs/propagate.rs benches/foundation.rs src/core/ecs/AGENTS.md .claude/skills/ryo-ecs-internals/SKILL.md src/cli/skills/ryo-ecs-internals.md docs/backlog/tilemap.md
git commit -m "perf: transform propagation walks only the subtrees that changed, so a still map costs nothing to propagate"
```

### Task 2 (only if Task 1's guard still misses): measure what is left, and report

**Files:**
- Modify: `docs/backlog/tilemap.md` (the record)
- Create: nothing in `src/` unless the controller rules it in. This task measures and reports.

**Interfaces:**
- Consumes: Task 1's propagation and the `static_frame` guard.
- Produces: a report the controller rules on, with the measurements below.

- [ ] **Step 1: Profile the map frame.** Use the guard's printed command (`target/release/deps/tilemap-<hash> --bench --profile-time 20 '^static_frame$'`). Record the shares of `propagate_transforms` (the changed filter's scan), the chunk drawer, physics' tile parts and anything else over 5% of the map's added cost.
- [ ] **Step 2: Measure the structural option the spec names: keeping static map parts out of the scanned set.** On a scratch commit (reverted afterwards, never merged), make collider parts and occluder chains carry no `Transform`, placed from their map instead. Run the guard once to see what it would buy. List the contracts it would change: physics reading a part's `GlobalTransform`, `MapOccluder`, and `TilePart`.
- [ ] **Step 3: Write both into `docs/backlog/tilemap.md`'s record, commit the docs, and report NEEDS_CONTEXT** with the numbers, so the controller can bring the choice to the owner.

```bash
git add docs/backlog/tilemap.md
git commit -m "docs: what the static map frame spends after the propagation change"
```
