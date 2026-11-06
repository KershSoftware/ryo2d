# Hexagonal Maps at Speed Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Meet the hexagonal map's held static-frame and edit targets, and bring ring-heavy edits under the edit target, without changing what collision, shadows or navigation produce.

**Architecture:**
- A crate-internal marker takes a map's still collider parts and shadow chains out of transform propagation's per-entity change scan. Their parents' walks still move them.
- Hexagonal shadow rebuilds keep what each chunk last built, and rebuild only the chains through the cells and rim edges that changed.
- Hexagonal rings move into collider parts of their own, so a side edit leaves them be.

**Tech Stack:** Rust 2024; bevy_ecs 0.20 as `ryo2d::ecs`; Rapier2d 0.36.1 / parry 0.31.1; criterion and the custom guards in `benches/tilemap.rs`.

**Spec:** `docs/specs/2026-11-05-hex-speed-design.md`

**Code map** (read-only recon, file:line evidence for every cost): `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/hex-speed-map.md`

## Global Constraints

- Branch `hex-speed`. `git rev-parse --abbrev-ref HEAD` prints `hex-speed` before every commit. No git worktrees.
- No public API change. `MapOccluder`, `LightOccluder2D`, `TileColliders`, `NavigationGrid` and `MapsView` keep their signatures and meaning. The new marker is `pub(crate)`.
- No new dependencies. `cargo check -p ryo2d --lib --target wasm32-unknown-unknown` stays green.
- `tests/tilemap_alloc.rs` stays green: a still frame allocates nothing on any grid.
- **Edges are invariant:** after any edits, a map's collider edges and shadow edges equal a fresh build's (spec, Goal 4 and Anti-goals).
- **Chain count:** a chunk's chain count never passes twice what a fresh build of its tiles makes.
- **Targets, unchanged:** the static frame 50 µs; each edit part 100 µs (chunk, loop, occluder); load 20 ms. No guard is loosened and no target moved. A held target that misses is reported, never hidden.
- **Measuring:**
  - release builds (`cargo bench --bench tilemap -- <guard>`, `cargo test --release ... -- --ignored --nocapture`);
  - a quiet machine with `uptime` recorded before each set, and nothing else building;
  - `df -h /` checked first, stopping below 40 GiB;
  - logs under `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/`, never /tmp.
- Profiling uses `xctrace` (Instruments' command line, headless) or temporary timers. Neither is committed.
- **Skills:** bump each skill once on the branch, by a minor version, in the first task that changes it: ryo-tilemap 1.6.0 → 1.7.0, ryo-ecs-internals 2.1.3 → 2.2.0, ryo-physics 3.7.0 → 3.8.0 only if Task 2 runs. No stage or task labels in user-facing text.
- **Commits:**
  - stage files by name;
  - no Co-Authored-By or Claude-Session lines;
  - "module notes", never the notes' file name, in messages.
- Failing-first runs of memory or allocation guards allocate a few hundred MB at most.
- Never start a shell loop that waits on a condition.

## Review Focus

1. **A map on a moving body.** A map root carrying a kinematic `RigidBody` moved by physics still carries its marked parts and chains. Physics writes its `Transform`, and propagation walks it. Task 1: `a_map_on_a_kinematic_body_carries_its_parts_and_chains`.
2. **An interpolated map.** A map under an `Interpolated` root whose chains are rebuilt by an edit draws them blended from the next walk. They must not be left without a `PreviousGlobalTransform` for good. Task 1: `an_interpolated_map_s_rebuilt_chains_get_previous_transforms`.
3. **Side-0 hex grids** (`hexsidelength` 0), where `hex_chains` gives `None` and chaining falls back to points. Edits there still match a fresh build, and the chain count stays bounded. Task 3: `a_side_zero_hex_map_edited_at_random_matches_a_fresh_build`.
4. **A reload after edits.** A hot reload (`MapEvent::Reloaded`) of a hex map after many edits drops every kept chunk state and chain, and casts what a fresh build casts. Task 3: `a_reloaded_hex_map_after_edits_casts_what_a_fresh_build_does`.
5. **A streamed hex chunk that unloads and lands again.** It keeps no stale state from its first life: its chains and parts equal a fresh build's, and the group keeps nothing for it while it's away. Task 3 (`a_streamed_hex_chunk_landed_again_keeps_no_stale_chains`) and Task 4 (`a_streamed_hex_chunk_landed_again_has_fresh_ring_parts`).

---

### Task 1: Still map children leave propagation's scan

**Files:**
- Modify: `src/core/ecs/transform.rs`: the marker, beside `Transform`.
- Modify: `src/core/ecs/propagate.rs`: `Placement`, at :20, `changed` at :70, and `Changes::placement` at :159.
- Modify: `src/core/physics/tiles.rs`: `TilePart`, at :2317, requires the marker.
- Modify: `src/core/tilemap/occluders.rs`. The spawn sites `spawn_chains` (:785) and `land` (:999-1037) insert the marker. `MapOccluder` is public, so insert the marker rather than require it, unless `#[require]` of a `pub(crate)` type compiles without a lint.
- Modify:
  - the docs: `src/core/ecs/AGENTS.md` (Propagation), `.claude/skills/ryo-ecs-internals/SKILL.md` (~:86, bump to 2.2.0), and `.claude/skills/ryo-tilemap/SKILL.md` (~:466, bump to 1.7.0);
  - the doc comments on `MapOccluder` and `TilePart`.
- Test: `src/core/ecs/propagate.rs` tests, `src/core/tilemap/occluders.rs` tests, `src/core/physics/tiles.rs` tests.

**Interfaces:**
- Produces: `pub(crate) struct ParentPlaced;` (a `Component`) in `crate::core::ecs`.
  - An entity carrying it is placed only by its parent's walk and by `Transform`'s insert hook. Its own `Transform`, `ChildOf` or `Interpolated` changes don't seed a walk.
  - Later tasks spawn parts and chains with it.

- [ ] **Step 1: Write the failing tests**
  - `a_parent_placed_child_s_own_transform_change_seeds_no_walk` (propagate.rs).
    - Spawn a parent at (10, 0) and a child with `ParentPlaced` at local (1, 0), and run propagation.
    - Set the child's `Transform` to (5, 0) and run propagation.
    - The child's `GlobalTransform` translation is still (11, 0).
    - It fails before the filter, which gives (15, 0).
  - `a_parent_placed_child_moves_with_its_parent_the_frame_it_moves` (propagate.rs). Move the parent to (20, 0) and run propagation: the child's global is (21, 0).
  - `a_moved_map_carries_its_hex_parts_and_chains` (occluders.rs tests, hex map helpers `hex_map`, `lit_edges`):
    - move the map root by (100, 50), turn it 30° and scale it 2 in one frame;
    - after that frame's propagation, every part's and chain's `GlobalTransform` equals the root's global times its local;
    - `lit_edges` equal the unmoved edges mapped by the root's transform;
    - repeat for a layer group with its own offset.
  - `a_part_spawned_the_step_its_map_moved_is_placed_where_the_map_is_now` (tiles.rs tests): move a colliding hex map and `set_tile` a new wall in the same update. The new part's `GlobalTransform` is under the moved map.
  - `a_map_on_a_kinematic_body_carries_its_parts_and_chains` (Review Focus 1): a kinematic body on the map root, moved by physics; its parts' Rapier positions and its chains' globals follow in the same step. If `owning_body` (`physics/sync.rs:174-187`) shows a body on a map root is unsupported, say so in the report and test a map root moved by a script instead.
  - `an_interpolated_map_s_rebuilt_chains_get_previous_transforms` (Review Focus 2): every chain carries `PreviousGlobalTransform` within one frame of its spawn.

- [ ] **Step 2: Run them to see the first fail**

  Run: `cargo test -p ryo2d --lib parent_placed`

  Expected: `a_parent_placed_child_s_own_transform_change_seeds_no_walk` FAILS (the child reads (15, 0)), or fails to compile with the marker missing. The map tests may pass already: they guard what must not break.

- [ ] **Step 3: Add `ParentPlaced`, filter `Placement`'s scan with `Without<ParentPlaced>`, and spawn parts and chains with it**

  - Keep `placed` (:71) unfiltered.
  - `Changes::is_seed` then returns false for marked children, which is correct, since they have no children of their own.

- [ ] **Step 4: Run the tests and the libraries' suites**

  Run:
  - `cargo test -p ryo2d --lib ecs::`
  - `cargo test -p ryo2d --lib tilemap::`
  - `cargo test -p ryo2d --lib physics::`
  - `cargo test --test tilemap_alloc`

  Expected: all PASS.

- [ ] **Step 5: Measure the static frame once, in release, on a quiet machine**

  Run: `RYO_TILEMAP_GRID=hexagonal cargo bench --bench tilemap -- static_frame 2>&1 | tee <scratchpad>/t1-static.log`

  Expected: the hexagonal map's figure in the grids' round. Report every map's figure against main's (`docs/backlog/tilemap.md`, the hexagonal section and "Every grid"). Task 2 runs only if the hexagonal map reads above 46 µs.

- [ ] **Step 6: Docs**
  - The propagation paragraph in `ecs/AGENTS.md` and in ryo-ecs-internals names the marker and what it costs a marked entity.
  - ryo-tilemap ~:466 says a chain's and a part's placement is the map's: a game's change to their `Transform` has no effect.

- [ ] **Step 7: Run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git add <the files above, by name>
  git commit -m "fix: transform propagation skips a map's still colliders and shadows"
  ```

### Task 2 (conditional): Collider parts leave physics' scans

Run only if Task 1's Step 5 reads the hexagonal static frame above 46 µs. Otherwise the controller ledgers a ruling that skips it.

**Files:**
- Modify: `src/core/physics/sync.rs`: `Resync` (:80-88), `reparented` (:496) and `sync_colliders` (:492-562).
- Modify: `src/core/physics/triggers.rs`: `Edited` (:39-44).
- Modify: `src/core/physics/tiles.rs`: `Moved` (:2447-2448) and `rebuild_tile_colliders`.
- Modify: `src/core/physics/AGENTS.md` (Sync Rules, Tile Colliders) and `.claude/skills/ryo-physics/SKILL.md` if it describes the sync (bump to 3.8.0).

**Interfaces:**
- Consumes: `ParentPlaced` (Task 1).
- Produces: every `TilePart` spawned or reshaped by `rebuild_tile_colliders` is pushed to `Physics::waiting`.
  - `sync_colliders` also pushes the `TilePart` children of any `TileColliders` entity whose `GlobalTransform` changed.

- [ ] **Step 1: Write the failing test** `a_map_moved_after_landing_moves_its_rapier_colliders`: move a landed colliding hex map, step physics, and Rapier's collider positions for its parts equal their new `GlobalTransform`s. Also `edit_schedules_keep_resyncing_rebuilt_parts`: the bench's standalone schedules still build a rebuilt part's Rapier shape.
- [ ] **Step 2: Add `Without<ParentPlaced>` to the four scans** (the tests above must then fail), **then add the explicit notices.** Run `cargo test -p ryo2d --lib physics::` and `tilemap::`: PASS.
- [ ] **Step 3: Measure** as in Task 1, Step 5.
- [ ] **Step 4: Run the gate and commit** with `fix: physics' change scans skip a map's still colliders`.

### Task 3: Hexagonal shadow edits rebuild only the chains that changed

**Files:**
- Modify: `src/core/tilemap/occluders.rs`:
  - `Group` (:158), which keeps each hex chunk's last build;
  - `hex_chunk_chains` (:552-716), which takes the incremental path;
  - `update` (:809-876), which despawns and spawns only the affected chains, newest first;
  - the neighbour marks (:681-715) for hex;
  - `Group::clear` (:245-252), newest first.
- Modify:
  - `src/core/physics/tiles.rs`, the `Boundary` hex helpers;
  - `src/core/physics/tiles/outline.rs`, `HexSides::across` (:452-529) made `pub(crate)` if needed.
- Modify: `src/core/tilemap/AGENTS.md` (hex occluders and their cost).
- Test: `src/core/tilemap/occluders.rs` tests.

**Interfaces:**
- Consumes:
  - `ParentPlaced` (Task 1): chains are spawned with it;
  - `Boundary::hex_chains(&self, placed: &[PlacedEdge], ..) -> Option<Vec<Chain>>`, which already chains any subset of edges.
- Produces: no new public items.
  - A hex chunk's kept state lives in the group, in a crate-private struct (for example `HexKept { solid: [u32; 32], shapes: FastMap<u16, ShapeKey>, chain_of: Vec<u16>, chains: Vec<Entity>, rings: FastMap<u16, Entity> }`). The names are the implementer's.
  - Task 5's bench reads only timings.

- [ ] **Step 1: Profile before building**
  - Profile with `xctrace record --template 'Time Profiler'` on the release bench, or with temporary `Instant` timers in a local copy, never committed:
    - one serpentine occluder rebuild: `RYO_TILEMAP_GRID=hexagonal cargo bench --bench tilemap -- edit`;
    - one ring-heavy edit: `cargo test --release -p ryo2d --lib hex_ring_heavy_edit_timing -- --ignored --nocapture`.
  - Split each into classify, full or rim scan, `hex_chains`, building `LightOccluder2D`s, and despawn plus spawn.
  - Test the despawn-order hypothesis: reverse the despawn loop locally and time the ring-heavy test again.
  - Write the split to the report.
  - If the split shows the incremental path can't bring the serpentine under 100 µs, stop and report BLOCKED with the figures.

- [ ] **Step 2: Write the failing tests**
  - `a_hex_edit_respawns_only_the_chains_through_its_cell`:
    - a dense chunk of full hex rows on an infinite map;
    - record each chain's `Entity` and vertices, then set one cell in its middle to empty;
    - every chain that doesn't touch the cell's six corners keeps its `Entity`;
    - `lit_edges` equal a fresh build's (`assert_same_edges`).
  - `a_hex_rim_edit_rebuilds_only_the_neighbour_chains_it_touches`: edit a cell in a chunk's first column. The chunk to its left keeps every chain `Entity` that doesn't touch the edited cell's corners.
  - `ring_chains_of_unchanged_cells_keep_their_entities`: a chunk of inset-hexagon shape tiles (as in `hex_ring_heavy_edit_timing`). Clear one shape cell, and only its ring chain goes. Turn one into another shape, and only its ring is respawned.
  - `hex_neighbour_marks_follow_side_ownership_on_every_variant`:
    - for each of `hex_variants()` (axis X and Y, odd and even);
    - edit each rim cell kind: the four corners, the first row's ends, the last column's odd and even rows;
    - a neighbour chunk keeps all its chain entities unless a side of the edited cell belongs to one of its cells.
  - `a_shape_cell_painted_on_a_chunk_edge_rebuilds_no_neighbour`: Empty → Shape(a) → Shape(b) on a first-column cell. The left chunk's chains keep their entities.
  - `hex_shadows_edited_at_random_match_a_fresh_build_on_bounded_maps_and_every_variant`:
    - like `hex_shadows_edited_at_random_round_chunk_corners_match_a_fresh_build` (:2653), on a bounded 40 x 40 map and an infinite one, for every variant;
    - random cells near (0, 0), (31..33, 31..33) and the bounds edge become Empty, Solid or one of two shapes;
    - after each edit, `lit_edges` equal a fresh build's.
  - `a_hex_chunk_edited_many_times_keeps_its_chain_count_near_a_fresh_build`: 500 seeded random edits in one dense chunk. After each, its chain count is at most twice a fresh build's.
  - `a_side_zero_hex_map_edited_at_random_matches_a_fresh_build` (Review Focus 3).
  - `a_reloaded_hex_map_after_edits_casts_what_a_fresh_build_does` (Review Focus 4): no chain entity from before the reload survives.
  - `a_streamed_hex_chunk_landed_again_keeps_no_stale_chains` (Review Focus 5): beside `a_streamed_hex_chunk_unloads_cleanly` (:2288).

- [ ] **Step 3: Run them to see them fail**

  Run: `cargo test -p ryo2d --lib tilemap::occluders`

  Expected: the entity-keeping tests and the neighbour-mark test FAIL (chains are respawned whole). The fresh-build and reload tests may pass: they guard correctness.

- [ ] **Step 4: Build the incremental hex path** (spec section 2)
  - **Keep, per hex chunk:**
    - its solidity bitset;
    - the shape identity of each shape cell;
    - a chain index per kept edge, aligned with `Group::edges[chunk]`;
    - each chain's entity;
    - each shape cell's ring entity.
  - **Changed cells:** diff the new classification against what was kept.
  - **Affected chains:** any chain with an edge touching a changed cell's corners, or a changed rim edge. Re-chain their edges together with the new edges using `hex_chains`.
  - **Entities:** despawn the affected chains' entities (newest first) and spawn theirs.
  - **Rings:** rebuild only for changed shape cells.
  - **Marks:** a neighbour is marked from `HexSides` ownership of a rim cell whose solidity changed.
  - **Fallbacks:**
    - chain the chunk whole when there is no kept state (a first build, a reload, a chunk landed again);
    - chain it whole when the chain count passes twice the last whole build's;
    - on side 0, where `hex_chains` gives `None`, chain whole by points.
  - Square, lattice and staggered paths are unchanged.

- [ ] **Step 5: Run the tests**

  Run:
  - `cargo test -p ryo2d --lib tilemap::`
  - `cargo test -p ryo2d --lib physics::`
  - `cargo test --test tilemap_alloc`
  - `cargo test -p tile-grids-demo`

  Expected: all PASS.

- [ ] **Step 6: Time it in release on a quiet machine**

  Run:
  - `RYO_TILEMAP_GRID=hexagonal cargo bench --bench tilemap -- edit`
  - `cargo test --release -p ryo2d --lib hex_ring_heavy_edit_timing -- --ignored --nocapture`

  Expected: the serpentine's occluder rebuild under 100 µs, and the ring-heavy occluder rebuild under 100 µs. Report the figures against main's.

- [ ] **Step 7: Update the docs, run the gate and commit**
  - The tilemap module notes' hex occluder paragraph says what an edit rebuilds.

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git commit -m "fix: a hexagonal shadow edit rebuilds only the chains through the cells it changed"
  ```

### Task 4: Hexagonal rings collide in parts of their own

**Files:**
- Modify: `src/core/physics/tiles/pack.rs`:
  - `PackKey::Hex` (:76);
  - `HexBox` (:325), which gains a sub-block level;
  - `HexBlock` (:347), which counts sides and rings apart;
  - `HexChunk` (:399), which keeps the ring parts' split beside the sides';
  - `boxes`, `HexParts::build` (:534), `apply` (:572) and `keys` (:684).
- Modify: `src/core/physics/tiles.rs`:
  - `hex_pack` (:1307) and `hex_repack` (:1348) pack a sides part or a rings part;
  - `TileColliders::trace` (:1641-1661) makes the same parts.
- Modify: `src/core/physics/AGENTS.md` (the hex parts paragraph) and the `HexParts` doc.
- Test: `src/core/physics/tiles.rs` tests.

**Interfaces:**
- Consumes: nothing from Tasks 1-3.
- Produces:
  - `PackKey::Hex { chunk: (i32, i32), part: HexBox, rings: bool }`;
  - `HexBox::Sub(u8)`: a 4 x 4-cell sub-block, by index row by row over the chunk (0..64), used for ring parts only.
  - The ring parts' levels are chunk, quarter, block, then sub-block, counted on ring edges alone against `HEX_PART_EDGES`. A sub-block may hold more than B, as a block may now.

- [ ] **Step 1: Write the failing tests**
  - `a_hex_shape_cell_s_rings_sit_in_a_ring_part_apart_from_the_sides`: one shape cell in a chunk gives one sides part and one rings part. The rings part holds exactly the cell's rings. Update `a_hex_shape_cell_is_a_closed_ring_in_its_own_part` (:3607) to match.
  - `scattered_shape_cells_share_one_ring_part_a_chunk`: 10 inset-hexagon cells spread over a chunk give one ring part (60 edges ≤ 128).
  - `ring_parts_split_by_their_own_budget`: a chunk full of inset hexagons (6 edges each) gives 4 x 4 sub-block ring parts of 96 edges each. A 12-gon shape fills a sub-block past B, which stays one part.
  - `a_side_edit_beside_ring_cells_rebuilds_no_ring_part`:
    - the ring-heavy chunk; turn (40, 40) whole;
    - every ring part except the one holding (40, 40) keeps its entity and its collider unchanged (compare `hex_parts` entries, :5490);
    - the rebuilt side parts' edges total under 200.
  - `a_landed_hex_map_s_prebuilt_parts_match_a_fresh_build_s`: the landing worker's `trace` gives the same keys and edges as `HexParts::build` on the same tiles.
  - `a_streamed_hex_chunk_landed_again_has_fresh_ring_parts` (Review Focus 5).
  - `hex_parts_edited_with_shape_cells_hold_what_a_fresh_build_does` (:3863) stays green. Extend its random edits with shape-to-shape changes.

- [ ] **Step 2: Run them to see them fail**

  Run: `cargo test -p ryo2d --lib physics::tiles`

  Expected: the ring-part tests FAIL (rings share their block's part).

- [ ] **Step 3: Split rings into their own parts** as the Interfaces say, in build, apply and trace.

- [ ] **Step 4: Run the tests**

  Run:
  - `cargo test -p ryo2d --lib physics::`
  - `cargo test -p ryo2d --lib tilemap::`
  - `cargo test --test tilemap_alloc`

  Expected: all PASS.

- [ ] **Step 5: Time it in release on a quiet machine**

  Run:
  - `cargo test --release -p ryo2d --lib hex_ring_heavy_edit_timing -- --ignored --nocapture`
  - `cargo test --release -p ryo2d --lib hex_static_part_counts -- --ignored --nocapture`

  Expected: the ring-heavy loop rebuild under 100 µs at the four-block corner and at the block's middle. Report the part counts against main's.

- [ ] **Step 6: Update the docs, run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git commit -m "fix: hexagonal rings collide in parts of their own, so a side edit leaves them be"
  ```

### Task 5: The ring-heavy guard, the measurements and the record

**Files:**
- Modify: `benches/tilemap.rs`: the hex edit guard's cases (`edit_cases`, :1492; `EditCase`, :1435; the tileset that `rock()`, :501, belongs to) gain a held ring-heavy case. Update the header comment.
- Modify:
  - `docs/backlog/tilemap.md`: a dated section in the style of "Hexagonal collision, shadows and paths (2026-11-05)", the targets table, and the Limits;
  - `src/core/tilemap/AGENTS.md`, `src/core/physics/AGENTS.md` and `src/core/physics/tiles/pack.rs`'s module doc (cost figures);
  - `.claude/skills/ryo-tilemap/SKILL.md` (cost lines; already 1.7.0);
  - `docs/backlog/follow-ups.md`: remove the lines this branch fixed.

**Interfaces:**
- Consumes: Tasks 1-4.

- [ ] **Step 1: Add the ring-heavy case**
  - A chunk of an inset-hexagon tile, the shape of `hex_ring_heavy_edit_timing`'s, on the hex map, kept apart from the islands.
  - Two held edits:
    - a block's middle cell cleared and set back;
    - a four-block corner cell turned whole and back.
  - Each edit's chunk, loop and occluder parts are held at 100 µs.
  - Check once that the chunk's cells are `TileCell::Shape` and its ring parts are sub-blocks, as `assert_whole_hexagon` checks the rock.

- [ ] **Step 2: Run it to see it count**

  Run: `RYO_TILEMAP_GRID=hexagonal cargo bench --bench tilemap -- edit`

  Expected: the ring-heavy lines print, held.

- [ ] **Step 3: Measure on a quiet machine, `uptime` before each set**
  - `RYO_TILEMAP_GRID=hexagonal cargo bench --bench tilemap -- static_frame`, three runs;
  - `-- edit`, three runs;
  - `-- load` and `-- stream`, once each;
  - the ignored timing tests in release: `hex_ring_heavy_edit_timing`, `hex_static_part_counts`, `hex_one_tile_edit_timing` and `navigation_window_timing`.

  Expected: every held static and edit target met on every grid, in every run. Any miss is reported with its figures.

- [ ] **Step 4: Write the record**
  - The dated section: method, before and after tables against the 2026-11-05 figures, and the part and chain counts.
  - Correct the targets table's hexagonal rows and verdicts.
  - **Limits:**
    - remove or rewrite "the hex static miss", "the serpentine's occluder miss" and "parts heavy with rings";
    - the propagation Limit says what is still scanned: every entity not marked;
    - the hex stream Limit keeps its proposal, with the new figure.
  - Every number a Limit quotes matches a row in the same file.
  - **Follow-ups:**
    - remove the diagonal-marks, full-kind rims and per-rebuild `hex_chains` allocation lines, if fixed;
    - add, under this work's own heading, the square grids' whole-chunk chain rebuild as a later lever.

- [ ] **Step 5: Run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo check -p ryo2d --lib --target wasm32-unknown-unknown && cargo test --workspace && cargo test --doc -p ryo2d tilemap`

  ```bash
  git commit -m "docs: hexagonal maps meet their static and edit targets, in the record and the tile-map docs"
  ```

  If a held target still misses, use `docs: hexagonal maps' static frame and edits are measured again and recorded` instead, and state the miss in the Limits.
