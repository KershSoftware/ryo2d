# Hexagonal maps at speed: still children out of the scans, and edits that rebuild what changed

**Status:** design, 2026-11-05. It follows hexagonal collision and pathfinding (merged at main 9b6cc834), whose record left two held targets missed, and comes before floors. The owner chose this pass at that merge, and asked for the design without a round of questions. Every decision marked **(my call)** is the designer's and is open to the owner's review.

## Why

Hexagonal maps now collide and cast shadows. The record, in `docs/backlog/tilemap.md` under "Hexagonal collision, shadows and paths (2026-11-05)" and its Limits, measured them in release on a quiet machine. The figures:

| Case | Measured | Target |
|---|---|---|
| Static frame, 1024² four-layer hex map, colliding and casting shadows | +53.7 to +54.9 µs | 50, held: **missed** |
| Serpentine edit, occluder rebuild | 121 to 144 µs | 100, held: **missed** |
| Ring-heavy chunk (tiles whose own shape is an inset hexagon), occluder rebuild | 343.6 to 348.8 µs | 100, unheld |
| Ring-heavy chunk, loop rebuild where four 8x8 blocks meet | 134.0 to 135.1 µs | 100, unheld |
| Hex stream, 99th percentile | 550 to 598 µs | 500, unheld |

The edit guard and the grids' static round always include hex, so these two misses fail the `static_frame` and `edit` guards' verdicts on every run. (A full `cargo bench --bench tilemap` also fails on the landing guard's known first-landing miss, and at times on the orthogonal stream's spread. Both are recorded Limits, outside this work.)

A code map (2026-11-05, read-only) traced each cost:

- **Static frame.**
  - About 4.5 µs is drawing: 79 tile draws against the orthogonal map's 59.
  - About 8.7 µs is change-tick scans over the map's still children. There are 5,255 of them on hex (2,109 collider parts and 3,142 shadow chains) against 2,882 on the orthogonal map.
  - Transform propagation runs three times a frame (`ecs/propagate.rs`, the `Placement` query). Each run checks two change ticks on every entity with a `Transform` or a parent, about 0.7 ns each.
  - Physics' collider sync, triggers and tile rebuild queries check about six more ticks on every collider part each fixed step (`physics/sync.rs` `Resync` and `reparented`, `physics/triggers.rs` `Edited`, `physics/tiles.rs` `Moved`).
  - Rapier's own idle step costs about nothing per fixed collider.
- **Serpentine occluder rebuild.** Every chunk an edit marks is rebuilt whole (`tilemap/occluders.rs` `hex_chunk_chains`, `update`):
  - the chunk is classified;
  - every edge is chained again;
  - every ring is rebuilt;
  - every chain entity is despawned and spawned again.

  An edit in a dense chunk's first column also marks the dense chunk to its left. Only one or two of that chunk's edges change, but it is rebuilt whole.
- **Ring-heavy occluder rebuild.**
  - A cell's rings depend only on that cell, yet all 1,024 of a chunk's rings are rebuilt, and their entities respawned, on every rebuild.
  - A suspected extra: a chunk's chains are despawned in spawn order. Bevy removes each from its parent's `Children` vector with a search from the back and a shift, so the loop may be quadratic. This is unprofiled.
- **Ring-heavy loop rebuild.** A collider part holds its block's sides and rings together. An edit that changes one cell's sides rebuilds four blocks' parts with all their rings: 1,536 edges at about 80 ns each.
- **Hex stream.** Each landed chunk is classified, packed, shaped, chained and spawned on the main thread. Only the generator runs on the chunk worker.

## Goals

1. **The static-frame and edit guards pass again.** Every held target of `benches/tilemap.rs`'s `static_frame` and `edit` guards is met on every grid, so each guard's verdict passes, in three quiet release runs (my call: the landing guard's known miss and the stream guard's spread are recorded Limits this work does not touch; the `load` and `stream` guards are run once to show they read as before):
   - the hex static frame under 50 µs;
   - every hex edit case under 100 µs for each part, the serpentine's occluder rebuild included.
   - The guards and their targets stay as they are.
2. **Ring-heavy edits meet the edit target.**
   - Both the loop rebuild and the occluder rebuild of a ring-heavy chunk are under 100 µs: the cell in a block's middle, and the four-block corner.
   - The ring-heavy case becomes a held case of the hex edit guard **(my call: a target nothing guards regresses silently)**.
3. **Every grid gains, none loses.** Taking still map children out of the per-frame scans helps the orthogonal, isometric, staggered and oblique maps' static frames too, by an estimated 6 to 7 µs. No grid's held figure gets worse.
4. **Behaviour unchanged.** Collision, shadows and navigation give the same results as now on every grid:
   - the same collider edges;
   - the same shadow edges;
   - the same blocked cells.

   Only the cost changes, and the way chains are split into entities may differ from a fresh build (section 2).

## Non-goals

- **The hex stream's 99th percentile** (my call). Its lever is to prepare a landed chunk's classification, edges, rings, chains and shapes on the chunk worker against a snapshot of its neighbours' rims. That is a large change touching every grid, and it stays a recorded Limit with that proposal. The edits below make a landing's neighbour rebuilds cheaper, and the record measures what that buys.
- **One entity per chunk of chains.** A chunk's chains would sit in one component instead of one `LightOccluder2D` entity each. This would cut lit frames' occluder gathering and the entity count, but it changes `MapOccluder`'s meaning in the public API. Once the scans skip still children, its unlit saving is about nothing. Left for later.
- **Incremental chains on square and lattice grids.** Their `chunk_chains` has the same whole-chunk rebuild, but they meet their targets. Left for later, noted in the follow-ups.
- **Coarser hex packing at load.** Once parts are out of the scans, a still part costs about nothing, and coarser parts would make first edits miss.
- **Drawing.** The hex map's extra 20 draws a frame stay as they are. Merged contiguous chunk draws remain the Limits' proposal.

## Anti-goals

These would make the work a failure even if the numbers drop:

- **A wrong shadow or collider after an edit.** After any sequence of edits, a map's collider edges and shadow edges must equal a fresh build's, as the existing randomized fresh-build tests check on hex.
- **Any moving map that fails to carry its colliders and shadows.** A map, a layer group with its own offset, or a scaled or turned map that moves must still move them, the frame it moves, as it does now.
- **Chain fragmentation.** A chunk edited many times must not end with far more chain entities than a fresh build of the same tiles would make, which would make every lit frame dearer.
- **A loosened guard, or a moved target.**

## The design

### 1. Still map children leave the per-frame scans

A crate-internal marker component in `ecs` (named by the builder) marks entities whose placement only their parents' walk changes. Transform propagation's change scan (`Placement`) excludes it with an archetype filter (`Without<...>`), so its tables are never visited.

The map's collider parts and shadow chains carry the marker from spawn: `TilePart` and `MapOccluder` require it, or their spawn bundles include it. Nothing else does.

**How they are still placed:**
- **At spawn:** `Transform`'s insert hook places a new child from its parent's global transform, as it does now.
- **When the map, a layer or a group root moves:** propagation walks that ancestor's subtree through `Children`, so marked children move with it, the frame it moves.
- A marked child's own `Transform`, parent or interpolation changes no longer seed a walk.
  - The engine never changes them: hex parts keep their corner, square parts that move are re-inserted, and chains are respawned.
  - The ryo-tilemap skill already says a game's change to a chain lasts only until its chunk changes. It will now say that a chain's and a part's placement is the map's.

**Physics' scans (conditional, my call).**
- Exempting parts from physics' per-collider scans as well would save about 5.5 µs more on the hex static frame. But it means physics must be told explicitly of parts that are spawned, reshaped, or moved by their map.
- It is built only if the hex static frame, after the propagation change, is still above 46 µs in a quiet release run, which leaves less than 4 µs of room.
- If built, the tile rebuild queues every part it spawns or reshapes for the collider sync, and the sync queues a moved map's parts.

### 2. Hex shadow edits rebuild only the chains that changed

Today a chunk's rebuild is whole-chunk. After this change, a rebuild works from what changed:

- **What changed.** Each chunk keeps what its last build read:
  - its cells' solidity, as a bitset;
  - the shape identity of each shape cell;
  - its chains, with a chain index for each of its edges and an entity for each chain;
  - its rings, by cell.

  A rebuild classifies the chunk, as now, and compares the result with what it kept, which gives the cells whose solidity or shape changed.
- **Rings by cell.** Only cells whose shape or kind changed have their rings rebuilt, despawned and spawned. Other cells' ring chains stay as they are.
- **Chains through changed sides.** A chain is affected when one of its edges touches a vertex of a changed cell's hexagon, or touches a rim edge that changed. A merge can join a chain to its neighbours, so both chains' ends touch the changed cell.
  - The affected chains' remaining edges, and the new edges, are chained again together with `hex_chains`, which already takes any subset of edges.
  - Only the affected chains' entities are despawned and spawned.
  - Unaffected chains and their entities stay.
- **Neighbour marks follow ownership and solidity.**
  - A neighbour chunk is marked only when a rim cell's solidity changed and the side it touches belongs to a cell of that neighbour (`HexSides` ownership). This replaces the hand-written left, above, above-left, above-right and right marks, which ignore the stagger index, and the comparison of whole `TileCell`s.
  - A marked neighbour rebuilds only its chains through the rim edges that changed.
- **Fragmentation.** Re-chaining the affected chains' whole edge sets, not just the new edges, keeps a chunk's chain count close to a fresh build's.
  - **(My call:)** If a chunk's chain count ever passes twice what its edges would chain to fresh, the chunk is chained again whole, once.
  - A test edits a chunk many times and checks the count.
- **Despawn order.** A chunk's chain entities are despawned newest first, so each removal from the parent's `Children` comes off its end. If the profile (section 5) shows the suspected quadratic cost elsewhere too, the same applies to gone collider parts.

### 3. Hex rings get collider parts of their own

A block's rings move out of its side part into ring parts of their own:

- An edit that changes a cell's sides rebuilds the side parts it touches, which are small.
- Only a cell whose shape changed rebuilds the ring part it sits in.
- Ring parts follow the side parts' levels, counted on ring edges alone (my call): a chunk's rings are one part while they hold at most B = 128 edges, else one part per 16 x 16 quarter that holds them, else per 8 x 8 block, and a block whose rings still pass B is split into smaller sub-blocks (a size the builder picks, for example 4 x 4 cells). Scattered shape tiles so add about one ring part a chunk, not one a block.
- A fresh build makes the same parts, the landing worker's prebuilt trace (`TileColliders::trace`) included, so a first edit in a ring-heavy block is as cheap as later ones.
- The side parts' levels and their one-shot split are as now.

Goal 4 still holds: the colliders' edges are the same as now, split differently among parts.

### 4. What stays as it is

- The edge budget B = 128 and the side parts' three levels.
- Square, isometric, staggered and oblique shadow building.
- The public API: `MapOccluder`, `LightOccluder2D`, `TileColliders` and `NavigationGrid`.
- What a fresh build's colliders and shadows cover: the same edges. Only the hex collider parts' split changes (section 3).
- The landing job and the streaming path, apart from what their neighbour rebuilds share with sections 2 and 3.

### 5. Profile first

Before section 2 is built, one profile of the serpentine edit and one of the ring-heavy edit split their cost into these steps:
- classify;
- scan;
- chaining;
- building occluders;
- entity churn, with the despawn-order hypothesis tested.

The builder uses Instruments' command line (`xctrace`, headless) or temporary timers, never committed. If the split shows section 2 can't reach 100 µs, the builder says so with the figures before building it.

## How we know it works

- **Correctness:**
  - The existing randomized hex tests that compare edits with a fresh build pass unchanged. They compare collider edges and shadow edges after random edits at chunk corners.
  - New tests:
    - bounded maps (no hex shadow test edits a bounded map yet);
    - both stagger indices and both axes;
    - shape cells turning into other shapes;
    - the chain-count bound after many edits.
- **Moving maps:**
  - Moving a map, a layer group with its own offset, or a scaled or turned map moves its parts and chains the frame it moves.
  - A part spawned in the step its map moved is placed correctly.
  - Navigation on a moved map still blocks the right cells.
- **The guards:**
  - `cargo bench --bench tilemap -- static_frame` and `-- edit` pass their verdicts in three quiet release runs, with `uptime` recorded; `-- load` and `-- stream` once each, reading as before.
  - The hex edit guard gains the ring-heavy case.
  - The record (`docs/backlog/tilemap.md`) gets a dated section with the before and after tables. The Limits whose misses are fixed are corrected, and the propagation Limit says what is left.
- **The timing tests:** the ring-heavy timing test and the part-count timing test are run again in release, and their figures recorded.
- **The tour:** the scripted demo tour against main shows every page byte-identical, apart from the pages known to differ between runs. Nothing draws differently.

## Constraints

- No public API change. The marker is crate-internal.
- No new dependencies.
- The web build (`wasm32-unknown-unknown`) keeps compiling.
- No allocation in a still frame: `tests/tilemap_alloc.rs` stays green on every grid.
- Measurements in release on a quiet machine, three runs, logs under the session scratchpad.
- **Docs say what is true:**
  - the ryo-tilemap skill: the chains' and parts' placement, and the cost lines;
  - the ryo-ecs-internals skill's propagation paragraph;
  - the ecs, physics and tilemap module notes;
  - the Limits.

  Each skill touched is bumped once on the branch. No stage or task labels go in user-facing text.

## Left to the builder

- The marker's name and where it is required.
- Whether the kept per-chunk state lives in `Group` or beside it.
- The exact affected-chain rule, provided goal 4 and the fragmentation bound hold.
- The ring-part sub-block size.
- The profiling tool.
- Whether the despawn-order fix applies to collider parts too, which the profile decides.
