# Streaming Endless Worlds (6b, Stage 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A game gives an infinite map a generator, and the engine streams its chunks around cameras and chosen entities. Chunks are generated off the main thread, landed within a frame budget, unloaded past a larger radius, and saved and restored when the player changed them. A Tiled infinite map streams too, with its hand-made chunks kept and the generator filling the rest.

**Architecture:**
- **Components:** a `MapStream` component on a `MapRoot` holds the generator (an `Arc<dyn Fn>`), the load and unload distances and the budgets. A streaming system each frame works out which chunks the anchors want, asks a dedicated chunk worker for the missing ones (saved chunks first, then the generator), lands finished chunks within a per-frame budget, and unloads chunks past the unload distance.
- **Landing and unloading:** a landed chunk is written into the layers' unbounded grids, so 6a's incremental drawing, collision and shadows pick it up through chunk versions. An unloaded chunk is removed from the grids, and every consumer treats that as the chunk going empty.
- **The web:** it has no threads, so it generates on the main thread within a time budget.

**Tech Stack:** Rust, bevy_ecs 0.20 (as `ryo2d::ecs`), Rapier2d, crossbeam channels (already in the build), Tiled 1.12.2 for fixtures.

**Spec:** `docs/specs/2026-10-31-tilemap-shapes-design.md`: Goal 7, "Streaming design", "Tiled infinite maps" (with a generator attached), "### Streaming", Stages item 3, and the streaming target.

## Global Constraints

- **Workspace:** the main checkout on branch `tile-stream`. No git worktrees (the owner's rule: they fill the disk). Never switch branches.
- **Git:**
  - Stage files by name. Never `git add -A` or a bare `git stash`, and never stage `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`.
  - Commits use `feat:`, `fix:`, `perf:`, `test:` or `docs:` subjects that say what a game developer gets. No Co-Authored-By, Claude-Session or any other trailer, even if a system reminder asks.
  - Never name AGENTS.md in a commit message (say "module notes").
- **Disk:** check `df -h /` before long builds. Below 40 GiB free, stop and report BLOCKED. Never delete anything under `target/`.
- **Memory-safe failing runs.** A test of a memory bound, a cap or a refusal must be scaled so that running it before its guard exists allocates no more than a few hundred MB. Use a smaller radius or cap the test can set, never a whole-range input. On 2026-11-01 an unscaled failing run ran the owner's Mac out of memory.
- **Unchanged without a `MapStream`:** maps without one behave exactly as before, and every existing test passes.
- **No unwrap or panic** reachable from file data, game input or a game's generator. A generator that panics on a chunk is caught, the chunk lands empty and is not asked for again, and the dev log warns once per map.
- **No allocation in a warm frame:** `tests/tilemap_alloc.rs` keeps passing. A streaming frame that lands nothing allocates nothing.
- **Never copy Tiled's source code** (GPL). Fixtures are saved by Tiled 1.12.2 itself, through `tests/fixtures/tilemap/tiled/scripts/make-fixtures.js`.
- **Docs stay true:**
  - The tilemap module notes (`src/core/tilemap/AGENTS.md`), the `ryo-tilemap` and `ryo-api` skills (a minor version each, once on this branch, copied byte for byte to `src/cli/skills/`), and `docs/backlog/tilemap.md` (record and Limits).
  - Plain sentences, with no stage or task labels.
- **The gate**, before each task's last commit:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo check -p ryo2d --lib --target wasm32-unknown-unknown`
  - `cargo check -p skill-examples`
  - `cargo test -p ryo2d --lib cli::`
  - `cargo test --doc -p ryo2d tilemap`
  - `cargo test --workspace`
  - `cargo test --test tilemap_alloc`
- **The spec's target:** an anchor crossing an endless generated world at 2,000 px a second, with a 1080p view at zoom 1:
  - adds at most 0.5 ms of main-thread time to any frame on native platforms;
  - never shows an unfilled chunk on screen;
  - keeps resident chunks within the radii.

## Review Focus

1. **An anchor that jumps far** (a respawn or a teleport). The chunks around the old place unload, and the new place loads nearest first. A game can ask whether an area is ready, to hold a loading screen, instead of dropping a body into ungenerated space (Task 2, `a_teleported_anchor_s_area_reports_ready_once_landed`).
2. **A streamed map despawned, or its scene left, while chunk jobs are out.** Nothing lands into a gone map, finished results are dropped, and nothing leaks (Task 2, `a_despawned_streamed_map_takes_no_late_chunks`).
3. **An edit in a chunk the same frame it unloads.** The saved copy holds the edit (Task 3, `an_edit_in_the_unloading_frame_is_saved`).
4. **A generator that panics on one chunk.** The game keeps running, the chunk lands empty and is not asked for again, and one warning names the map and the chunk (Task 2, `a_panicking_generator_leaves_one_empty_chunk_and_one_warning`).
5. **Several anchors far apart** (two cameras, a player and a companion). Resident chunks are the union of their areas and no more, and an area unloads only when no anchor is near it (Task 2, `two_far_anchors_keep_both_areas_and_nothing_between`).

---

### Task 1: Chunks can be unloaded, and every consumer treats an unloaded chunk as gone

**Files:**
- Modify: `src/core/tilemap/grid.rs` (unloading a chunk, and a signal consumers read), `src/core/physics/tiles.rs` (removing a chunk's cells from `TileColliders`)
- Modify: the consumers: `src/core/tilemap/kinds.rs` (`Tracked`, `changed_chunks`: a chunk the grid no longer has counts as changed to empty, then is forgotten), `collision.rs`, `occluders.rs` (its occluder entities despawn), `spawn.rs` (class tiles in it despawn), `src/core/render/tiles.rs` (not drawn; the chunk store frees it as it frees any undrawn chunk)
- Test: beside each consumer, and `src/core/tilemap/spawn/signed.rs`

**Interfaces:**
- Produces:
  ```rust
  impl TileGrid {
    /// Takes `chunk` out of an unbounded grid, returning its cells (None if absent).
    /// Consumers see it as changed to empty on their next read, then forget it.
    pub(crate) fn unload_chunk(&mut self, chunk: IVec2) -> Option<Box<[u32; CELLS]>>;
    /// Puts a whole chunk's cells into an unbounded grid at once, as one change
    pub(crate) fn load_chunk(&mut self, chunk: IVec2, cells: Box<[u32; CELLS]>) -> bool;
  }
  impl TileColliders {
    pub(crate) fn unload_chunk(&mut self, chunk: IVec2); // its cells leave as empty; loops through it close at its edge
  }
  ```
  A bounded grid refuses both and returns false or None: only unbounded layers stream.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn an_unloaded_chunk_leaves_drawing_collision_shadows_and_class_tiles() {
  // an unbounded map, chunks A (-1, 0) and B (0, 0), solid, occluding, one class tile in A; land it
  // unload A from the layer grid (unload_chunk) and step the world
  // assert: TileColliders::get at A's cells is Empty and B's parts are untouched (same entities);
  // no MapOccluder for A remains, B's are the same entities; A's class tile entity is despawned;
  // the chunk store draws nothing for A (a camera over A draws only background)
}

#[test]
fn a_floor_across_an_unloaded_chunk_closes_at_its_edge() {
  // a floor spanning chunks -1, 0 and 1; unload chunk 1; assert the remaining outline is one closed loop
  // ending at x = 32 cells (no open edge, no part left over in chunk 1)
}

#[test]
fn loading_a_chunk_back_rebuilds_it_as_new() {
  // unload A, then load_chunk(A, same cells); assert collision, shadows and the class tile come back
  // (a new class tile entity), and B is untouched throughout
}

#[test]
fn a_bounded_grid_refuses_to_unload_or_load_chunks() { /* returns None / false, grid unchanged */ }
```

- [ ] **Step 2: Run them.** Expected: FAIL to compile (no `unload_chunk`/`load_chunk`).
- [ ] **Step 3: Implement.**
  - The grid keeps a list of unloaded chunks, each with the version of the unload, until every consumer has read past it. `chunk_versions` (or a sibling the consumers already call) reports them, so `changed_chunks` rebuilds them as empty and drops them from `Tracked`.
  - Unloading a chunk moves the grid's version on, like any change.
- [ ] **Step 4: Run the tilemap, physics and e2e tilemap tests, and `tilemap_alloc`.** Expected: PASS.
- [ ] **Step 5: Docs** (the tilemap and physics module notes: what unloading does), the gate, and commit.

```bash
git commit -m "feat: an unbounded layer's chunks can be unloaded, and drawing, collision, shadows and class tiles let them go"
```

### Task 2: `MapStream` generates chunks around anchors, off the main thread

**Files:**
- Create: `src/core/tilemap/stream.rs` (the component, the generator types, the streaming system and its worker), and its tests
- Modify: `src/core/tilemap/mod.rs` and `src/lib.rs` (exports), `spawn.rs` (register the system after the landing systems in `PreUpdate`), `view.rs` (`MapsView` edits on a streamed map; `stream_ready`)
- Modify: the tilemap module notes, and the `ryo-tilemap` skill (a "Streaming an endless world" section with a compiling example), plus `ryo-api`

**Interfaces:**
- Consumes: Task 1's `load_chunk`/`unload_chunk`; `TileMap::infinite`; `MapsView`.
- Produces:
  ```rust
  /// One chunk's tiles, layer by layer, as a generator fills it
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct ChunkTiles { /* layer name -> cells */ }
  impl ChunkTiles {
    pub fn set(&mut self, layer: &str, cell: UVec2, tile: Option<Tile>); // cell within the chunk, 0..32
    pub fn get(&self, layer: &str, cell: UVec2) -> Option<Tile>;
    pub fn is_empty(&self) -> bool;
  }
  /// What a generator is asked for
  #[derive(Clone, Copy, Debug)]
  pub struct ChunkRequest { pub chunk: IVec2, pub first_cell: IVec2, pub tile_size: UVec2 }
  /// Streams an infinite map's chunks: put it on the map's `MapRoot` entity
  #[derive(Component, Clone)]
  pub struct MapStream {
    pub generator: Arc<dyn Fn(ChunkRequest) -> ChunkTiles + Send + Sync>,
    /// Chunks within this many pixels of a camera's view, or of an anchor, load
    pub load_distance: f32,   // default 512.0
    /// Chunks farther than this from every camera's view and anchor unload; kept above load_distance
    pub unload_distance: f32, // default 1536.0
    /// Whether every camera is an anchor (its view, grown by the distances)
    pub cameras: bool,        // default true
    /// Landed chunks a frame, at most
    pub land_per_frame: u32,  // default 4
    /// Where there are no threads (the web): main-thread time a frame for generating
    pub web_budget: std::time::Duration, // default 2 ms
  }
  impl MapStream { pub fn new(generator: impl Fn(ChunkRequest) -> ChunkTiles + Send + Sync + 'static) -> MapStream; }
  /// Keeps the chunks near this entity loaded, on every streamed map
  #[derive(Component, Clone, Copy, Debug, Default)]
  pub struct StreamAnchor;
  /// A streamed map's chunk arriving or leaving (a `Message`)
  #[derive(Message, Clone, Debug)]
  pub enum ChunkEvent {
    Loaded { map: Entity, chunk: IVec2 },
    /// Sent before the chunk goes; `changed` holds its tiles when an edit changed it since it loaded
    Unloading { map: Entity, chunk: IVec2, changed: Option<ChunkTiles> },
  }
  impl MapsView<'_, '_> {
    /// Whether every chunk overlapping `area` (world pixels) is resident on streamed map `map`
    pub fn stream_ready(&self, map: Entity, area: Rect) -> bool;
  }
  ```

- [ ] **Step 1: Write the failing tests** (`stream.rs` tests, headless):

```rust
#[test]
fn chunks_load_around_an_anchor_and_unload_behind_it() {
  // an infinite map with one "ground" layer, a generator that fills row 0 of every chunk; an anchor at
  // (0, 0) with cameras off; run frames until idle
  // assert: every chunk within load_distance of the anchor is resident; none beyond unload_distance;
  // move the anchor 20 chunks right; run until idle; the old chunks are gone and the new ones in
}

#[test]
fn the_generator_runs_off_the_main_thread() {
  // a generator that records std::thread::current().id(); assert it differs from the test thread's
}

#[test]
fn chunks_land_nearest_first_and_within_the_frame_budget() {
  // land_per_frame 2; count chunks landed per frame <= 2; the first two are the anchor's own chunk and
  // a neighbour (by distance)
}

#[test]
fn loaded_and_unloading_messages_follow_the_chunks() { /* one Loaded per landing, one Unloading per unload */ }

#[test]
fn a_teleported_anchor_s_area_reports_ready_once_landed() {
  // anchor jumps 100 chunks; stream_ready(map, view-sized rect there) is false, then true after landing;
  // the old area unloads
}

#[test]
fn a_despawned_streamed_map_takes_no_late_chunks() {
  // a slow generator (it sleeps 50 ms); despawn the map while jobs are out; run frames; no panic, no
  // entities for the map, the worker's late results dropped
}

#[test]
fn a_panicking_generator_leaves_one_empty_chunk_and_one_warning() {
  // a generator that panics for chunk (3, 0); the chunk lands empty, is not requested again, and the dev
  // log has exactly one warning naming the map and chunk (3, 0)
}

#[test]
fn two_far_anchors_keep_both_areas_and_nothing_between() { /* the union of both areas, nothing more */ }

#[test]
fn an_edit_in_a_chunk_not_yet_loaded_on_a_streamed_map_is_refused() {
  // ctx.maps().set_tile at a far cell returns false and makes no chunk; in a resident chunk it returns true
}

#[test]
fn where_there_are_no_threads_generating_keeps_to_the_frame_budget() {
  // the crate's no-thread worker (as on the web); a generator that takes 1 ms; web_budget 2 ms;
  // assert at most ~2 chunks generated a frame and they all land eventually
}
```

- [ ] **Step 2: Run them.** Expected: FAIL to compile (no `MapStream`).
- [ ] **Step 3: Implement.**
  - **The chunk worker** is its own thread ("ryo2d chunk builder"), using the crate's `Worker` pattern. Generation never waits behind a whole map's landing on the map builder's thread.
  - **In flight:** at most 64 requests per map, nearest first.
  - **Landing:** a landed chunk goes through Task 1's `load_chunk` into each named layer. A layer name the map lacks is warned once and ignored.
  - **Validation:** `unload_distance` below `load_distance` is raised to it, with one warning.
  - **Edits:** an edit through `MapsView` into a chunk that isn't resident on a streamed map returns false.
  - **Generator panics** are caught on the worker (`catch_unwind`, as `jobs.rs` does).
- [ ] **Step 4: Run the stream tests, the tilemap tests and `tilemap_alloc`.** Expected: PASS.
- [ ] **Step 5: Docs** (the module notes; the skill section, with a compiling example of a noise-terrain generator and a `StreamAnchor` on the player), the gate, and commit.

```bash
git commit -m "feat: MapStream streams an infinite map's chunks around cameras and anchors, generated off the main thread"
```

### Task 3: Changed chunks are saved and come back, and chunk-owned entities unload with their chunk

**Files:**
- Modify: `src/core/tilemap/stream.rs` (`SavedChunks`, `ChunkOwned`), its tests, the skill section and the module notes

**Interfaces:**
- Consumes: Task 2's `MapStream`, `ChunkTiles`, `ChunkEvent`.
- Produces:
  ```rust
  /// Chunks a player changed, kept while they are unloaded; a clone shares the same store.
  /// Serialize it with a save game, and give it back to the stream when the game loads.
  #[derive(Clone, Default, Debug, serde::Serialize, serde::Deserialize)]
  pub struct SavedChunks { /* Arc<Mutex<FastMap<IVec2, ChunkTiles>>> inside; serde through a snapshot */ }
  impl SavedChunks {
    pub fn get(&self, chunk: IVec2) -> Option<ChunkTiles>;
    pub fn insert(&self, chunk: IVec2, tiles: ChunkTiles);
    pub fn remove(&self, chunk: IVec2) -> Option<ChunkTiles>;
    pub fn len(&self) -> usize;
  }
  impl MapStream { pub fn with_saved(self, saved: SavedChunks) -> MapStream; }
  /// An entity that belongs to a streamed chunk: despawned when the chunk unloads
  #[derive(Component, Clone, Copy, Debug)]
  pub struct ChunkOwned { pub map: Entity, pub chunk: IVec2 }
  ```
  The worker asks `SavedChunks` first, then the generator. On unload, a chunk changed since it landed goes into `SavedChunks` (when the stream has one), and is also carried in `ChunkEvent::Unloading::changed`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn an_edit_survives_leaving_and_coming_back() {
  // stream with SavedChunks; anchor at chunk 0; set_tile at a cell of chunk 0; move the anchor away until
  // chunk 0 unloads; assert saved.len() == 1 and Unloading carried Some(tiles) with the edit; move back;
  // assert the edited tile is there and the generator was not called for chunk 0 again
}

#[test]
fn an_unchanged_chunk_is_not_saved() { /* Unloading carries None; saved stays empty */ }

#[test]
fn an_edit_in_the_unloading_frame_is_saved() {
  // edit a cell of a chunk in the same frame (before the stream system) it falls past unload_distance
  // assert the saved copy holds the edit
}

#[test]
fn saved_chunks_round_trip_through_serde() { /* serde_json to string and back; equal contents */ }

#[test]
fn chunk_owned_entities_unload_with_their_chunk() {
  // on Loaded, spawn an entity with ChunkOwned { map, chunk }; unload the chunk; assert it is despawned,
  // and an entity owned by a chunk still resident stays
}
```

- [ ] **Step 2: Run them.** Expected: FAIL (no `SavedChunks`).
- [ ] **Step 3: Implement.** Track "changed since landed" per chunk through its version at landing.
- [ ] **Step 4: Run the stream and tilemap tests.** Expected: PASS.
- [ ] **Step 5: Docs** (the skill: saving with a save game; `ChunkOwned` for enemies and pickups spawned on `Loaded`), the gate, and commit.

```bash
git commit -m "feat: a streamed world keeps the chunks a player changed and restores them, and chunk-owned entities unload with their chunk"
```

### Task 4: A Tiled infinite map streams, its hand-made chunks kept

**Files:**
- Modify: `src/core/tilemap/stream.rs`, `src/core/tilemap/reload.rs` (a reload of a streamed map keeps generated chunks), the skill section and the module notes
- Test: `stream.rs` tests with the fixture `tests/fixtures/tilemap/tiled/maps/infinite.tmj` (Tiled-saved, chunks at negative coordinates)

**Interfaces:**
- Consumes: Tasks 1-3. The spec's rule, verbatim: "any engine chunk that any layer's Tiled chunk touches belongs wholly to the file. The generator never runs there, and the parts Tiled left empty stay empty… The file's chunks stay resident, because they are the asset's."

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn a_tiled_infinite_map_streams_around_its_hand_made_chunks() {
  // infinite.tmj with a MapStream whose generator fills every cell it is asked for with tile 1
  // assert: every engine chunk the file touches holds exactly the file's tiles (empty cells stay empty);
  // chunks the file never touched near the anchor hold the generator's tiles; the generator was never
  // asked for a file chunk
}

#[test]
fn the_file_s_chunks_stay_resident_when_the_anchor_leaves() { /* move far away: file chunks remain */ }

#[test]
fn saving_a_streamed_tiled_map_keeps_the_generated_chunks() {
  // stream, let generated chunks land, save a changed copy of the file (one file chunk changed);
  // assert after reload: the changed file chunk follows the file, generated chunks are untouched (same
  // versions), and the stream keeps going
}
```

- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
  - At landing, the stream records the set of file-owned engine chunks: every engine chunk any layer's Tiled chunk touches.
  - The streaming system never requests or unloads those chunks.
  - Reload's chunk-set diff applies to file-owned chunks only.
- [ ] **Step 4: Run the stream, reload and tilemap tests.** Expected: PASS.
- [ ] **Step 5: Docs, the gate, commit.**

```bash
git commit -m "feat: a Tiled infinite map streams endlessly around its hand-made chunks, and its saves keep the generated world"
```

### Task 5: The streaming speed guard

**Files:**
- Modify: `benches/tilemap.rs`: a `stream` guard and criterion bench, with the header comment describing them in the existing guards' pattern
- Modify: `docs/backlog/tilemap.md` (the record: three runs; Limits for streaming)

**Interfaces:**
- Consumes: Tasks 1-4.

- [ ] **Step 1: Write the guard:**
  - An engine at 1920 x 1080, zoom 1, 16 px tiles, with collision and shadows on, streams a generated noise-terrain world (two layers, one colliding and occluding).
  - Its camera, the anchor, crosses at 2,000 px a second for 2,000 frames, paced as the other guards are.
  - It is interleaved with an empty engine whose camera flies the same path.
  - **Held:** the 99th percentile of the frame's step over the empty frame's median is at most 0.5 ms (500 µs). The median and the maximum are printed beside it. The spec says "any frame", but a single maximum is the machine's noise, so the 99th percentile is held and the maximum reported.
  - **Asserted every frame:** every chunk overlapping the camera's view is resident (no unfilled chunk on screen), and the resident chunk count stays within the radii's bound (printed).
- [ ] **Step 2: Run it three times.**

Run: `cargo bench --bench tilemap -- '^stream$'`
Expected: `stream: ... Held, the 99th percentile: N µs, met`, with no unfilled frame.
- If it misses, profile it (the guard prints the command) and fix the cost in stream.rs or the incremental paths, with its own `perf:` commit.
- Reasonable levers, in order: the landing budget; trimming per-chunk landing work (the collision retrace for a whole new chunk); a larger prefetch so landing spreads out.
- Record the three runs.
- [ ] **Step 3: The gate, commit.**

```bash
git commit -m "test: a streaming guard crosses an endless world at 2,000 px/s and holds each frame's added cost"
```

### Task 6: The demo streams: the meadow goes on, and an endless world keeps your digging

**Files:**
- Modify: `examples/demos/tile-grids/` (pages, tests, README)

**Interfaces:**
- Consumes: the public API from Tasks 2-4 only.

- [ ] **Step 1: Write the page tests first:**
  - Page 1, the meadow, streams with a generator: the runner crosses past both of the meadow's former ends and chunks stream in, with the file's chunks unchanged.
  - The new page 3, "An endless world", lets the player dig and build. Leaving and coming back finds the edits (through `SavedChunks`).
  - The HUD shows resident chunks, requests in flight and saved chunks.
  - Run them; they fail.
- [ ] **Step 2: Build the pages.**
  - **Page 1:** the meadow's end walls go. Its title promises endlessness honestly again: "A hand-made meadow in an endless world". Its generator continues the meadow's ground: the same tileset, `ground_tile` rules and a gentle noise height, so the seam with the hand-made part reads as one world.
  - **Page 3:** a character or cursor that digs and builds with the mouse, with `StreamAnchor` on the character and `SavedChunks` on the stream.
  - **Generator:** use `ryo2d-procgen`'s noise if it offers 1D or 2D noise at a chunk's cells; otherwise a small hash noise in the demo. Say which in the README.
  - **World text:** labels in the world are `Text2d`, as page 1's chunk labels are.
- [ ] **Step 3: Run the tests and the demo tour** (`cargo run -q -p demo-kit --bin demo-tour -- tile-grids`), and look at the captures.
- [ ] **Step 4: README, the gate, commit.**

```bash
git commit -m "feat: the tile-grids demo streams the meadow into an endless world, and an endless page keeps what you dig and build"
```
