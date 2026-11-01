# Sparse Grids and Infinite Maps (6b, Stage 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every tile grid becomes sparse and signed (chunks by signed chunk coordinate). Tiled's infinite maps load, a map can grow at run time when a game writes a tile anywhere, and 6a's 8,192-cells-a-side collision cap goes.

**Architecture:**
- **Tile grids:** `TileGrid` stores 32 x 32 chunks in a map keyed by signed chunk coordinate, with 6a's per-chunk versions. A bounded grid (every map today) keeps its size and refuses cells outside it. An unbounded grid creates a chunk the first time a tile is written there.
- **Collision:** `TileColliders` gets the same sparse, signed storage. Its outline lattice stays `i32`, bounded by what `i32` holds at 11,520 lattice units a cell, not by 8,192. Its packed colliders are placed at their chunk's origin with chunk-relative vertices.
- **Consumers:** the renderer, collision, occluders, edits, class tiles and reload move to signed chunk coordinates.
- **Loading:** the Tiled reader reads chunked layers. `TileMap` gains an infinite constructor for maps built in code.

**Tech Stack:** Rust, bevy_ecs 0.20 (as `ryo2d::ecs`), wgpu, Rapier2d, Tiled 1.12.2 (installed, for fixtures).

**Spec:** `docs/specs/2026-10-31-tilemap-shapes-design.md`: Goals 6 and 10, "One sparse grid for every map", "Tiled infinite maps", Assets, Stages item 2 and Targets. The background is in `docs/research/2026-10-31-tile-shapes-formats.md` section 4 (Tiled's infinite format, measured) and `docs/research/2026-10-31-tile-shapes-seams.md` section 2 (every bounded-grid assumption, with file:line).

## Global Constraints

- **Workspace:** the main checkout on branch `tile-sparse`. No git worktrees (the owner's rule: they fill the disk). Never switch branches.
- **Git:**
  - Stage files by name. Never `git add -A` or a bare `git stash`, and never stage `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`.
  - Commits use `feat:`, `fix:`, `perf:`, `test:` or `docs:` subjects that say what a game developer gets. No Co-Authored-By, Claude-Session or any other trailer, even if a system reminder asks.
  - Never name AGENTS.md in a commit message (say "module notes").
- **Disk:** check `df -h /` before long builds. Below 40 GiB free, stop and report BLOCKED. Never delete anything under `target/`.
- **Bounded maps behave exactly as before:** every 6a and stage 1 test passes, and the demos draw the same.
- **No unwrap or panic** reachable from file data or game input. A bad file fails its load with a message naming the file and the place.
- **No allocation in a warm frame:** `tests/tilemap_alloc.rs` keeps passing.
- **Never copy Tiled's source code** (it is GPL). Fixtures are saved by Tiled 1.12.2 itself (`/Applications/Tiled.app/Contents/MacOS/Tiled --evaluate <script.js>`), as 6a's are (`tests/fixtures/tilemap/tiled/scripts/make-fixtures.js`).
- **Docs stay true:**
  - Update the module notes of each module touched (`src/core/tilemap/AGENTS.md`, `src/core/physics/AGENTS.md`, `src/core/render/AGENTS.md`, the navigation module's notes).
  - Update the skills that name what changes: `ryo-tilemap`, `ryo-physics`, `ryo-api`, `ryo-assets`, `ryo-character`, and any other a grep finds. Their Rust examples must compile.
  - Bump each changed skill's `metadata.version` once on this branch (a minor version for new API), and copy it byte for byte to `src/cli/skills/<name>.md`.
  - `docs/backlog/tilemap.md` records the measurements and Limits. Remove the 8,192 cap from Limits, and add the new collision bound.
  - Write plain sentences, with no task labels.
- **The gate**, before each task's last commit:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo check -p ryo2d --lib --target wasm32-unknown-unknown`
  - `cargo check -p skill-examples`
  - `cargo test -p ryo2d --lib cli::`
  - `cargo test --doc -p ryo2d tilemap`
  - `cargo test --workspace`
  - `cargo test --test tilemap_alloc`
- **The spec's targets for this stage:**
  - a static 1024 x 1024 four-layer map adds at most 50 µs a frame (now also with its layers unbounded);
  - one tile edit costs at most 100 µs per part;
  - a 256 x 256 four-layer map loads off the main thread within 20 ms, an infinite one included.

## Review Focus

1. **Cells at negative coordinates and on chunk edges.** At cells -1, 0, -32 and -33 on both axes, every consumer agrees on which chunk a cell is in and where it is: drawing, collision, shadows, class tiles, edits and the cell under the mouse. Chunk coordinates use `div_euclid`, never `/` (Task 3, `negative_and_chunk_edge_cells_agree_in_every_consumer`).
2. **A Tiled infinite map's own chunks.** Tiled's chunks are 16 cells (or any `editorsettings.chunksize`) against the engine's 32, layers have different chunk sets, and empty chunks are left out. Every cell lands where Tiled put it (Task 4, `an_infinite_map_s_chunks_land_where_tiled_put_them`).
3. **A hostile infinite file:** chunk coordinates near ±2^31, a chunk whose `data` length doesn't match its size, or a million chunks. It fails clearly, before allocating (Task 4, `hostile_infinite_chunks_are_refused_before_allocating`).
4. **Writing past the collision lattice's reach.** The tile draws. Its collision is refused with one dev-log warning, and nothing overflows (Task 2, `a_cell_past_the_lattice_s_reach_draws_but_does_not_collide`).
5. **Hot reload of an infinite map that gains and loses chunks.** The chunk sets are diffed, untouched chunks keep their version, and objects keep their entities (Task 4, `saving_an_infinite_map_that_moves_its_chunks_reloads_by_chunk`).

---

### Task 1: `TileGrid` stores signed chunks, bounded or unbounded

**Files:**
- Modify: `src/core/tilemap/grid.rs` (all of it, and its tests)
- Modify: `src/core/physics/tiles.rs` (add `CellRect`, see Interfaces), `src/lib.rs` (prelude: `CellRect`)
- Modify, only as far as compiling needs: every `TileGrid` caller. A caller that iterated `0..size` reads `extent()`. Tasks 3 and 4 do the real moves to signed chunks.

**Interfaces:**
- Produces, in `src/core/physics/tiles.rs` (physics is the lower layer; tilemap re-uses it):
  ```rust
  /// A rectangle of cells, `min` inclusive and `max` exclusive
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
  pub struct CellRect { pub min: IVec2, pub max: IVec2 }
  impl CellRect {
    pub fn new(min: IVec2, max: IVec2) -> CellRect;
    pub fn from_size(size: UVec2) -> CellRect; // (0, 0) to size
    pub fn size(&self) -> UVec2;               // zero when empty
    pub fn is_empty(&self) -> bool;
    pub fn contains(&self, cell: IVec2) -> bool;
    pub fn union(self, other: CellRect) -> CellRect; // an empty side is ignored
  }
  ```
- Produces, in `src/core/tilemap/grid.rs`:
  ```rust
  impl TileGrid {
    pub fn new(size: UVec2) -> TileGrid;                   // bounded: (0, 0) to size, as today
    pub fn unbounded() -> TileGrid;                        // any cell; chunks made on write
    pub fn bounds(&self) -> Option<CellRect>;              // None when unbounded
    pub fn extent(&self) -> CellRect;                      // bounded: bounds; else the chunks' cells
    pub fn get(&self, cell: IVec2) -> Option<Tile>;
    pub fn set(&mut self, cell: IVec2, tile: Option<Tile>) -> bool;
    pub fn fill(&mut self, min: IVec2, max: IVec2, tile: Option<Tile>) -> usize;
    pub fn clear(&mut self);
    pub fn chunk_coords(&self) -> impl Iterator<Item = IVec2> + '_; // present chunks, row order (y, then x)
    pub fn chunk_of(cell: IVec2) -> IVec2;                 // cell.div_euclid(CHUNK_SIZE)
    pub fn drain_dirty(&mut self, out: &mut Vec<IVec2>);   // row order
    pub fn chunk_version(&self, chunk: IVec2) -> u64;      // 0 for a chunk never written
    pub fn version(&self) -> u64;
    pub fn mark_all_dirty(&mut self);
    pub(crate) fn assign(&mut self, from: &TileGrid, also: impl Fn(IVec2) -> bool) -> bool;
    pub(crate) fn touch(&mut self, chunks: &[IVec2]) -> bool;
    pub(crate) fn chunk_cells(&self, chunk: IVec2) -> Option<&[u32; 1024]>; // CHUNK_SIZE² cells, row-major, for builders that walk a chunk without hashing per cell
  }
  ```
  `size()` and `chunks()` are removed, so no caller keeps assuming (0, 0) and a fixed size.

- [ ] **Step 1: Write the failing tests** in `grid.rs`'s test module, keeping every existing test (moved to the new signatures):

```rust
#[test]
fn an_unbounded_grid_takes_any_cell_and_makes_its_chunk() {
  let mut g = TileGrid::unbounded();
  assert!(g.set(IVec2::new(-33, -1), Some(Tile::new(0, 5))));
  assert_eq!(g.get(IVec2::new(-33, -1)), Some(Tile::new(0, 5)));
  assert_eq!(g.chunk_coords().collect::<Vec<_>>(), vec![IVec2::new(-2, -1)]);
  assert_eq!(g.extent(), CellRect::new(IVec2::new(-64, -32), IVec2::new(-32, 0)));
  assert_eq!(g.bounds(), None);
}

#[test]
fn clearing_an_absent_cell_makes_no_chunk() {
  let mut g = TileGrid::unbounded();
  assert!(!g.set(IVec2::new(100, 100), None));
  assert_eq!(g.chunk_coords().count(), 0);
}

#[test]
fn a_bounded_grid_refuses_cells_outside_it_as_before() {
  let mut g = TileGrid::new(UVec2::new(40, 20));
  assert!(!g.set(IVec2::new(-1, 0), Some(Tile::new(0, 1))));
  assert!(!g.set(IVec2::new(40, 0), Some(Tile::new(0, 1))));
  assert!(g.set(IVec2::new(39, 19), Some(Tile::new(0, 1))));
  assert_eq!(g.bounds(), Some(CellRect::from_size(UVec2::new(40, 20))));
}

#[test]
fn chunk_of_floors_toward_negative_infinity() {
  assert_eq!(TileGrid::chunk_of(IVec2::new(-1, 0)), IVec2::new(-1, 0));
  assert_eq!(TileGrid::chunk_of(IVec2::new(-32, 31)), IVec2::new(-1, 0));
  assert_eq!(TileGrid::chunk_of(IVec2::new(-33, 32)), IVec2::new(-2, 1));
}

#[test]
fn an_unbounded_fill_makes_the_chunks_it_writes_and_clears_only_what_exists() {
  let mut g = TileGrid::unbounded();
  assert_eq!(g.fill(IVec2::new(-40, -2), IVec2::new(-30, 1), Some(Tile::new(0, 2))), 11 * 4);
  assert_eq!(g.chunk_coords().count(), 4); // chunks (-2,-1) (-1,-1) (-2,0) (-1,0)
  assert_eq!(g.fill(IVec2::new(-1000, -1000), IVec2::new(1000, 1000), None), 11 * 4);
  assert_eq!(g.chunk_coords().count(), 4); // clearing leaves the chunks; only a streaming unload removes one
}

#[test]
fn drain_dirty_and_versions_work_on_signed_chunks() {
  let mut g = TileGrid::unbounded();
  g.set(IVec2::new(-1, -1), Some(Tile::new(0, 1)));
  g.set(IVec2::new(32, 0), Some(Tile::new(0, 1)));
  let mut out = Vec::new();
  g.drain_dirty(&mut out);
  assert_eq!(out, vec![IVec2::new(-1, -1), IVec2::new(1, 0)]); // row order
  assert!(g.chunk_version(IVec2::new(-1, -1)) > 0);
  assert_eq!(g.chunk_version(IVec2::new(5, 5)), 0);
}

#[test]
fn assign_diffs_two_unbounded_grids_by_their_chunk_sets() {
  // a has chunks A (unchanged in b) and B (dropped in b); b adds C
  // assert: after a.assign(&b, |_| false): A's version unchanged; B's cells all empty with a new
  // version; C present with b's cells and a new version; the result is true
}
```

- [ ] **Step 2: Run them**

Run: `cargo test -p ryo2d --lib tilemap::grid`
Expected: FAIL: `unbounded`, `chunk_of`, `extent` and `CellRect` don't exist yet.

- [ ] **Step 3: Implement the sparse grid.**
  - Chunks live in a `FastMap<IVec2, Chunk>`. A `Chunk` holds `Box<[u32; 1024]>` cells, its version, and its dirty mark.
  - A sorted list of chunk coordinates, kept on insert (inserts are rare), gives `chunk_coords` and `drain_dirty` their row order without sorting each call.
  - A bounded grid creates chunks lazily too: an all-empty chunk need not exist, and a chunk never written has version 0, as today.
  - Keep the process-wide `VERSIONS` counter and the cell encoding unchanged.
- [ ] **Step 4: Move every caller to the new signatures.** The compiler finds them. Where a caller looped `0..size`, use `extent()` (bounded maps get their old range back). Where it took a `UVec2` chunk, use `IVec2`.
- [ ] **Step 5: Run the tilemap tests and the e2e tilemap tests**

Run: `cargo test -p ryo2d --lib tilemap:: && cargo test --test e2e tilemap`
Expected: PASS, the new tests and every existing one.

- [ ] **Step 6: The gate, then commit**

```bash
git add src/core/tilemap/grid.rs src/core/physics/tiles.rs src/lib.rs <each caller file changed>
git commit -m "feat: tile grids store signed chunks, so a layer can be unbounded and grow where a tile is written"
```

### Task 2: `TileColliders` is sparse and signed, without the 8,192 cap

**Files:**
- Modify: `src/core/physics/tiles.rs` (storage, the public cell API, `MAX_SIDE`, the change list, `Cells`, part placement), `src/core/physics/tiles/outline.rs` (`Outlines::apply`'s dense index decode), `src/core/physics/tiles/pack.rs` (packing keys, already `div_euclid`)
- Modify: the callers of the public API: `src/core/navigation/grid.rs` (`block_tiles`), `src/core/physics/character.rs` tests, `src/core/gameplay/platformer.rs`, `src/core/particles/mod.rs` tests, `benches/physics.rs`, `examples/demos/character/src/lib.rs` and `tests/pages.rs`
- Modify: `src/core/physics/AGENTS.md`, the `ryo-physics` and `ryo-character` skills (and `ryo-api` where it lists the API)

**Interfaces:**
- Consumes: `CellRect` (Task 1).
- Produces, in `src/core/physics/tiles.rs`:
  ```rust
  impl TileColliders {
    pub fn new(width: u32, height: u32, tile_size: Vec2) -> Self;          // bounded, as today
    pub fn from_cells(width: u32, height: u32, tile_size: Vec2, cells: Vec<TileCell>) -> Self; // bounded, row by row, as today
    pub fn unbounded(tile_size: Vec2) -> Self;
    pub fn bounds(&self) -> Option<CellRect>;   // None when unbounded
    pub fn extent(&self) -> CellRect;           // the cells it holds
    pub fn get(&self, cell: IVec2) -> TileCell; // Empty outside
    pub fn set(&mut self, cell: IVec2, value: TileCell) -> bool;
  }
  /// How far from cell (0, 0) a grid's cells may be, on either axis, and still collide:
  /// what an i32 outline lattice of GRID units a cell holds, with room for a shape's reach
  pub(crate) const MAX_REACH: i32 = (i32::MAX / GRID) - 2;
  ```
  `width()` and `height()` are removed.

- [ ] **Step 1: Write the failing tests** (in `physics/tiles.rs`'s tests, with the existing ones moved to `IVec2`):

```rust
#[test]
fn an_unbounded_grid_collides_at_negative_cells() {
  // unbounded(16 px); a 5-cell floor at y = -3, x = -40..=-36; rebuild in a test world
  // assert: one TilePart; its collider's world AABB spans x -640..-560 px and y -48..-32 px (within 1e-3)
}

#[test]
fn a_grid_wider_than_8192_cells_collides() {
  // bounded 10_000 x 64 (6a refused it); a floor across row 63
  // assert: the parts' AABBs cover x 0..160_000 px; no warning
}

#[test]
fn a_cell_past_the_lattice_s_reach_draws_but_does_not_collide() {
  // unbounded; set (MAX_REACH + 1, 0) Solid -> false, dev log warns once ("past the reach");
  // set (MAX_REACH - 1, 0) Solid -> true and builds a part without overflow
}

#[test]
fn parts_sit_at_their_chunk_origin_with_chunk_relative_vertices() {
  // unbounded; a solid cell at (-1000, 2000) (chunk (-32, 62))
  // assert: its TilePart's Transform translation == chunk origin in px ((-1024 * 16, 1984 * 16));
  // the collider's vertices are within 32 * 16 px of the origin
}

#[test]
fn an_edit_on_an_unbounded_grid_retraces_only_its_loops() {
  // two islands in different chunks at negative cells; edit one; assert the other part is untouched
  // (same entity, same shape), the edited one reshaped in place
}
```

  Plus, in `navigation/grid.rs`: `block_tiles_reads_an_unbounded_grid_at_negative_cells` (a nav grid at origin (-320, -320) blocks the cells an unbounded `TileColliders` holds there).

- [ ] **Step 2: Run them**

Run: `cargo test -p ryo2d --lib physics::tiles navigation::grid`
Expected: FAIL to compile (`unbounded`, `IVec2` cells).

- [ ] **Step 3: Implement.**
  - Store cells by 32-cell chunk, as Task 1's grid does.
  - The change list holds `(IVec2, TileCell)`.
  - `Cells` answers `at(x, y)` for any signed cell, and `extent()` with a signed rectangle. `Outlines::apply` takes signed cells, not a dense index.
  - Replace `MAX_SIDE` with `MAX_REACH`, checked in `set` and `from_cells`. Check every multiplication in the tracer: none may multiply two absolute lattice coordinates. Deltas and coordinate times a small constant are fine.
  - Each packed collider's `TilePart` takes its pack chunk's origin as its `Transform` translation, and its shapes are built relative to that. Physics' prebuilt shapes still apply, since the part has no scale.
- [ ] **Step 4: Move the callers to the new API.** For `block_tiles`, iterate the colliders' chunks that overlap the navigation grid.
- [ ] **Step 5: Run the physics, navigation, character and particles tests, and the physics bench's guard if it has one**

Run: `cargo test -p ryo2d --lib physics:: navigation:: particles:: gameplay:: && cargo test -p character-demo`
Expected: PASS.

- [ ] **Step 6: Docs, the gate, commit**

```bash
git commit -m "feat: tile colliders take signed cells anywhere, so long levels and unbounded maps collide past 8,192 cells"
```

### Task 3: The renderer, collision, occluders, edits and class tiles work on signed chunks

**Files:**
- Modify: `src/core/render/tiles.rs` (`View::draw` culling and chunk loop, y-sorted rows), `src/core/render/batch/chunks.rs` (`ChunkKey.chunk: IVec2`, `ChunkRow.row: i32`), `src/core/render/draw_list.rs` (its test key)
- Modify: `src/core/tilemap/kinds.rs` (`Tracked`: versions by chunk in a map), `src/core/tilemap/collision.rs` (groups over present chunks, the group's extent, `TileColliders::unbounded` for unbounded layers, no `MAX_SIDE`), `src/core/tilemap/occluders.rs` (`Group`'s chunks and sides by chunk, `MapOccluder { chunk: IVec2 }`)
- Modify: `src/core/tilemap/edit.rs` and `view.rs` (writes on an unbounded layer create chunks; `clear_parts` over present chunks), `src/core/tilemap/spawn.rs` (`take_class_tiles` per present chunk)
- Modify: the render and tilemap module notes, and the `ryo-tilemap` skill

**Interfaces:**
- Consumes: Task 1's `TileGrid`, Task 2's `TileColliders`.
- Produces: `MapOccluder::chunk: IVec2` and `ChunkKey::chunk: IVec2` (crate). `MapsView` keeps its signatures; on an unbounded layer, `set_tile` and `fill` return true for any cell they change.

- [ ] **Step 1: Write the failing tests:**

```rust
// src/core/tilemap/spawn.rs tests (or a new tests module beside them)
#[test]
fn negative_and_chunk_edge_cells_agree_in_every_consumer() {
  // a code-built map whose tile layer grid is TileGrid::unbounded() (Task 5 adds TileMap::infinite;
  // build the layer data directly here), tiles at cells (-1,-1) (0,0) (-32,-32) (-33,-33), all solid
  // and occluding, one a class tile; spawn and land it
  // assert: TileColliders::get agrees at each cell; a MapOccluder exists for each of the chunks
  // (-1,-1), (0,0), (-2,-2) and no other; the class tile entity sits at cell (-33,-33)'s centre;
  // MapsView::world_to_cell of each cell's centre returns that cell
}

#[test]
fn writing_a_tile_far_away_on_an_unbounded_layer_collides_and_casts_shadows() {
  // spawned unbounded map; ctx.maps().set_tile(map, "ground", IVec2::new(-500, 300), Some(solid))
  // step the world; assert a TilePart now exists for chunk (-16, 9) and a MapOccluder for it
}
```

```rust
// tests/e2e/tilemap.rs
#[test]
fn an_unbounded_layer_draws_tiles_at_negative_cells_and_culls_by_signed_chunk() {
  // tiles at (-40, -3) and (40, 3) on an unbounded layer; camera centred on (-40, -3)'s pixel
  // assert: the tile's pixels are drawn; the chunk store holds no chunk for (1, 0) (culled)
}
```

- [ ] **Step 2: Run them**

Run: `cargo test -p ryo2d --lib tilemap:: && cargo test --test e2e tilemap`
Expected: FAIL. Unbounded layers don't collide, cast shadows or draw at negative cells yet.

- [ ] **Step 3: Implement.**
  - **Culling** takes the camera's box in layer pixels, `floor`ed to signed chunks. On a bounded layer it is clamped to the extent's chunks. On an unbounded layer only present chunks are drawn.
  - **Collision and occluders** walk present chunks. A bounded layer keeps its fixed group extent; an unbounded one uses `TileColliders::unbounded`.
  - **Every place that stored a dense per-chunk vector** keys by `IVec2`.
  - **Bounded maps** must build exactly what they built before.
- [ ] **Step 4: Run the tilemap, physics, render and e2e tests, and `tilemap_alloc`**

Run: `cargo test -p ryo2d --lib tilemap:: render:: physics:: && cargo test --test e2e tilemap && cargo test --test tilemap_alloc`
Expected: PASS.

- [ ] **Step 5: Docs, the gate, commit**

```bash
git commit -m "feat: unbounded tile layers draw, collide, cast shadows and spawn class tiles at any cell, negative ones included"
```

### Task 4: Tiled's infinite maps load, and reload by chunk

**Files:**
- Modify: `src/core/tilemap/tiled/raw.rs` (chunk fields, the layer's `startx`/`starty`), `src/core/tilemap/tiled/build.rs` (remove the infinite refusals; decode each chunk with the layer's encoding; place its cells; count the cell cap from decoded chunk cells), `src/core/tilemap/tiled/xml.rs` (`<chunk>` elements under `<data>`)
- Modify: `src/core/tilemap/map.rs` (`TileMap::infinite` field and `bounds()`), `src/core/tilemap/tiled/world_file.rs` (`map_size` from the chunks' bounds for an infinite map), `src/core/tilemap/reload.rs` (`LayerShape` for an unbounded layer is its kind, not its size; the grid diff is `assign` by chunk sets)
- Create: fixtures under `tests/fixtures/tilemap/tiled/maps/` and `xml/maps/`, saved by Tiled 1.12.2 from new jobs in `tests/fixtures/tilemap/tiled/scripts/make-fixtures.js`:
  - `infinite.tmj` and `infinite.tmx`: orthogonal, three tile layers with different chunk sets, tiles at negative coordinates, one layer with an empty chunk left out, objects at negative coordinates;
  - `infinite-chunks8.tmj`: `editorsettings.chunksize` 8 x 4;
  - one map per encoding (CSV, base64, zlib, gzip, zstd), in both formats.
- Modify: the tilemap and asset module notes, and the `ryo-tilemap` and `ryo-assets` skills

**Interfaces:**
- Consumes: Task 1's `TileGrid::unbounded` and `CellRect`.
- Produces, in `src/core/tilemap/map.rs`:
  ```rust
  pub struct TileMap { /* ... */ pub infinite: bool /* ... */ }
  impl TileMap {
    pub fn bounds(&self) -> CellRect; // bounded: (0, 0) to size; infinite: the union of its tile layers' extents
  }
  ```
  The `size` field's doc says "a bounded map's size; zero for an infinite map (use `bounds`)".

- [ ] **Step 1: Write the fixture jobs and save the fixtures with Tiled.** Run `make-fixtures.js` with `--evaluate`, then check them with `--export-map json` (the existing script's pattern).

- [ ] **Step 2: Write the failing tests** (`src/core/tilemap/tiled/mod.rs` tests, `xml.rs` tests, `reload/tests.rs`):

```rust
#[test]
fn an_infinite_map_s_chunks_land_where_tiled_put_them() {
  // load infinite.tmj and infinite.tmx
  // assert: map.infinite; each layer's grid is unbounded; every tile the fixture script placed reads back
  // at its cell (negative included); the omitted empty chunk has no engine chunk; map.bounds() is the
  // union of the layers' extents; objects at negative coordinates keep them; both formats load equal
}

#[test]
fn chunks_of_any_size_and_every_encoding_load() {
  // infinite-chunks8.tmj and the per-encoding fixtures, both formats; assert equal grids
}

#[test]
fn hostile_infinite_chunks_are_refused_before_allocating() {
  // hand-written JSON: a chunk at x = 2_147_483_600; a chunk whose data is shorter than width*height;
  // 2^20 chunks of 16x16 (past the map cell cap 2^27); a chunk of width 0
  // assert: each load fails naming the file, the layer and the chunk; the cap message reads
  // "layers hold N cells, past the M a map holds" (6a's wording)
}

#[test]
fn a_world_places_an_infinite_map_by_its_tile_origin() {
  // a .world listing infinite.tmj at (1000, 500) px with no width/height
  // assert: its rect's top-left == (1000 + bounds.min.x * tw, 500 + bounds.min.y * th), size == bounds.size() * tile
}

#[test]
fn saving_an_infinite_map_that_moves_its_chunks_reloads_by_chunk() {
  // spawn infinite.tmj; save a copy that drops one chunk, adds one far away, changes a cell of a third;
  // assert after reload: the dropped chunk's cells are empty, the new one is present, the changed
  // chunk's version moved, an untouched chunk's version did not, and objects keep their entities
}
```

- [ ] **Step 3: Run them**

Run: `cargo test -p ryo2d --lib tilemap::tiled tilemap::reload`
Expected: FAIL: infinite maps are refused (`..._naming_6b`). Delete those two refusal tests in this step, since 6b now loads what they refused.

- [ ] **Step 4: Implement the reader, `bounds`, the world size and the chunk-set reload.**
  - Every chunk's size comes from the chunk itself.
  - Validate the coordinates and lengths before decoding.
  - Count the map's cells from its chunks' sizes before allocating.
- [ ] **Step 5: Run the tilemap tests and `cargo test -p tilemap-demo`**
Expected: PASS.
- [ ] **Step 6: Docs, the gate, commit**

```bash
git commit -m "feat: Tiled infinite maps load in every encoding, place in worlds by their tile origin, and reload by chunk"
```

### Task 5: Maps grown at run time

**Files:**
- Modify: `src/core/tilemap/map.rs` (`TileMap::infinite(tile_size)`; `add_tile_layer` makes an unbounded grid on an infinite map), `src/core/tilemap/view.rs` (rustdoc: writes on an infinite map's layers create chunks)
- Test: `src/core/tilemap/spawn.rs` tests, `tests/e2e/tilemap.rs`
- Modify: the `ryo-tilemap` skill (a section on maps grown at run time, with a compiling example), `ryo-api`

**Interfaces:**
- Consumes: Tasks 1-4.
- Produces:
  ```rust
  impl TileMap {
    /// An empty infinite map of cells of `tile_size` pixels: its tile layers grow wherever a tile is written
    pub fn infinite(tile_size: UVec2) -> TileMap;
  }
  ```

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn a_map_built_infinite_in_code_grows_where_a_game_writes() {
  // TileMap::infinite(UVec2::splat(16)) with a tileset and an "ground" layer; spawn; land
  // ctx.maps().fill(map, "ground", IVec2::new(-100, 5), IVec2::new(-90, 5), Some(solid)) == 11
  // ctx.maps().set_tile(map, "ground", IVec2::new(5000, -5000), Some(solid)) == true
  // assert: map bounds now span both; a body dropped over (-95, 4)'s centre comes to rest on the floor
  // (step physics until it sleeps or 120 steps)
}
```

```rust
// tests/e2e/tilemap.rs
#[test]
fn a_tile_painted_far_from_the_origin_on_an_infinite_map_draws_with_a_camera_there() { /* pixel check */ }
```

- [ ] **Step 2: Run them.** Expected: FAIL (no `TileMap::infinite`).
- [ ] **Step 3: Implement `TileMap::infinite`** and the `add_tile_layer` change.
- [ ] **Step 4: Run the tilemap and e2e tests.** Expected: PASS.
- [ ] **Step 5: Docs (the skill section), the gate, commit**

```bash
git commit -m "feat: a map built infinite in code grows wherever a game writes a tile, and draws, collides and casts shadows there"
```

### Task 6: The speed guards hold, and an infinite map is measured

**Files:**
- Modify: `benches/tilemap.rs`:
  - `static_frame` gains a probe of the same map with unbounded layers, held to 50 µs like the bounded one;
  - `load` gains a 256 x 256 four-layer infinite TMX (zlib chunks), held to 20 ms;
  - the header comment describes both.
- Modify: `docs/backlog/tilemap.md` (the record: three runs of each guard; Limits: the 8,192 cap removed, the collision reach added)

**Interfaces:**
- Consumes: Tasks 1-5.

- [ ] **Step 1: Add the probe and the load case,** and run them before any tuning.

Run: `cargo bench --bench tilemap -- '^static_frame$'`, then `cargo bench --bench tilemap -- '^edit$'`, then `cargo bench --bench tilemap -- '^load$'`
Expected: every held verdict is `met`. If any misses, profile it (the guard prints the command) and fix the cost in the task's own code: the usual suspects are a hash lookup per cell in a chunk build, or a sort per frame. Commit the fix with its own `perf:` subject.

- [ ] **Step 2: Run each guard three times** and record the medians (95th percentile and maximum) in `docs/backlog/tilemap.md`'s target rows, as stage 1 did.
- [ ] **Step 3: The gate, commit**

```bash
git commit -m "test: the tile speed guards hold with sparse grids, and measure an infinite map's frame and load"
```

### Task 7: A new demo, `tile-grids`, with an infinite map and a paint-anywhere page

**Files:**
- Create: `examples/demos/tile-grids/`, a demo-kit demo following `examples/demos/AGENTS.md` (its `Cargo.toml`, `Ryo.toml`, `src/main.rs`, `src/lib.rs`, the pages, `tests/pages.rs`, `assets/README.md`). The workspace's `examples/demos/*` glob picks it up.
- Create: `examples/demos/tile-grids/assets/`:
  - Kenney's Pixel Platformer `tilemap_packed.png` and its `License.txt`, copied unchanged from `examples/demos/tilemap/assets/kenney-pixel-platformer/` (CC0 1.0), and credited in the README as 6a's demo does;
  - a Tiled infinite map, `maps/endless-meadow.tmx`, written by a `make-maps` script through Tiled's `--evaluate`, as 6a's demo does. It holds chunks at negative coordinates, platforms and coins.
- Pages:
  1. **"A Tiled infinite map":** the meadow, the camera following a character who runs left and right across chunks at negative coordinates. A debug overlay outlines the chunks present, and shows the map's bounds.
  2. **"Paint anywhere":** an empty infinite map. Clicking paints ground, right-clicking erases, and the arrow keys or a pad pan without limit. A bouncing ball collides with what is painted, and the overlay shows chunks being created. The page shows the cell under the mouse and the count of chunks.

**Interfaces:**
- Consumes: Tasks 4 and 5's API only (the demo is a game).

- [ ] **Step 1: Write the demo's page tests** (`tests/pages.rs`, headless, the demo-kit pattern):
  - each page loads and runs 300 frames;
  - the meadow's chunks at negative coordinates are present;
  - a scripted click (`RYO_INPUT` or the testing API) at a point paints a tile there;
  - the ball comes to rest on painted ground.
- [ ] **Step 2: Run them.** Expected: FAIL (no demo yet).
- [ ] **Step 3: Write the demo and its `make-maps`,** and generate the map with Tiled. Check it with `--export-map json`.
- [ ] **Step 4: Run the tests and the demo tour for it**

Run: `cargo test -p tile-grids-demo && cargo run -q -p demo-kit --bin demo-tour -- tile-grids`
Expected: PASS, and the two pages are captured. Look at the captures.
- [ ] **Step 5: Docs:** the demo's README (art, licence, regenerating), the demos list in `examples/demos/AGENTS.md`. Then the gate and commit.

```bash
git commit -m "feat: the tile-grids demo shows a Tiled infinite map and painting an infinite map anywhere"
```
