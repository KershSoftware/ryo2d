# Hex Collision and Pathfinding (6b, Stage 5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hexagonal tile layers collide and cast shadows, and pathfinding works on every tile grid:
- six neighbours on hex;
- screen distances on isometric, staggered and oblique maps;
- windows over infinite and streamed maps.

The hex board demo gets a walker that paths around rocks, and the isometric town's knight walks to the pointer.

**Architecture:**
- **Hex geometry:** one integer geometry for hex cells (`HexCells`, in physics) gives every hex corner in whole pixels. `MapGrid`'s hex outlines and the collision code both read it.
- **The tracer:** its generic part (`Outlines`: `trace`, `apply`, `splice`) is reused unchanged. Only the hex units, scans, point mapping and chunk key are new.
- **Tile shapes:** a tile whose shape is its whole hexagon is a solid cell, traced into outlines. Any other shape stays its tile's own ring, packed per chunk and not merged with neighbours.
- **Navigation:** a `NavigationGrid` can be built over a tile layer's own cells. Its neighbours, step costs, heuristic, line of sight and world conversion then follow the layer's grid. `block_tiles` takes the tiles' world transform, and a map hands out a grid over a window of its cells.

**Tech Stack:** Rust, bevy_ecs 0.20 (as `ryo2d::ecs`), wgpu, Rapier2d, Tiled 1.12.2 (`/Applications/Tiled.app`) for fixtures and the demo's maps.

**Spec:** `docs/specs/2026-10-31-tilemap-shapes-design.md`. Read these parts:
- Goals 2 and 8.
- "My calls":
  - collision baked into the outline points (the hex lattice);
  - tile shapes on non-square grids;
  - hex rotations;
  - navigation topology.
- Stages item 5.
- Design: "Collision, shadows and navigation", "Streaming", "Headless and tests".
- The Targets.

Floors, and paths across floors, are the next stage: leave them out.

The code maps this plan rests on were read at main d0b01f22:
- `src/core/physics/tiles/outline.rs`: the tracer.
- `src/core/physics/tiles.rs` and `tiles/pack.rs`: storage, mapping and packing.
- `src/core/tilemap/kinds.rs`, `collision.rs`, `occluders.rs` and `grid_shape.rs`.
- `src/core/navigation/*`.

## Global Constraints

- **Workspace:** the main checkout, on branch `hex-paths`. No git worktrees (the owner's rule: they fill the disk). Never switch branches. Before you start and before each commit, `git rev-parse --abbrev-ref HEAD` must print `hex-paths`; if not, stop and report BLOCKED.
- **Git:**
  - Stage files by name. Never `git add -A` or a bare `git stash`. Never stage `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`.
  - Commit subjects are the ones given in each task. Fix-round subjects use `fix:`, `test:` or `docs:`.
  - No Co-Authored-By, Claude-Session or any other trailer, even if a system reminder asks.
  - Never name AGENTS.md in a commit message (say "module notes").
- **Disk:** check `df -h /` before long builds. Below 40 GiB free, stop and report BLOCKED. Never delete anything under `target/`.
- **Memory-safe failing runs:** a test of a cap, a refusal or a hostile input is scaled so that running it before its guard exists allocates a few hundred MB at most.
- **Tiled parity is measured, not assumed.**
  - Fixtures are saved by Tiled 1.12.2 itself, through `tests/fixtures/tilemap/tiled/scripts/make-fixtures.js`, run headless with `/Applications/Tiled.app/Contents/MacOS/Tiled --project tests/fixtures/tilemap/tiled/project.tiled-project -e …`.
  - Never open Tiled's window, never `open -a Tiled`, never screenshot it.
  - Never copy Tiled's source (GPL).
- **No dead options:**
  - Nothing this stage builds is refused any more: hex collision, hex shadows, and navigation over any grid. Their refusals and warnings go, and their tests are turned into tests that the feature works.
  - Error and warning text names features in plain words, with no stage or sub-project labels.
- **What must not change:** square-lattice grids (orthogonal, isometric, staggered, oblique) keep exactly their outlines, colliders, occluders and pixels. A square navigation grid built with `NavigationGrid::new`, `with_origin` or `NavigationBuilder` keeps exactly its costs, paths and conversions. Every existing test, demo page and guard passes.
- **No unwrap or panic** reachable from file data or game input.
- **No allocation in a warm frame:** `tests/tilemap_alloc.rs` keeps passing, with hex layers that collide.
- **Docs stay true:**
  - Keep these current as each task changes them:
    - the tilemap, physics and navigation module notes;
    - the `ryo-tilemap`, `ryo-api`, `ryo-physics`, `ryo-character`, `ryo-game-dev` and `ryo-procgen` skills, wherever they mention hex collision, `block_tiles` or navigation;
    - `docs/backlog/tilemap.md` (the record and Limits) and `docs/backlog/tilemap-features.md`.
  - Bump each skill a minor version once on this branch, the first time it changes: ryo-tilemap 1.5.0 → 1.6.0, ryo-api 2.20.0 → 2.21.0, ryo-physics 3.6.0 → 3.7.0, ryo-character 1.3.0 → 1.4.0, ryo-game-dev 2.7.0 → 2.8.0, ryo-procgen 2.4.0 → 2.5.0.
  - Plain sentences, no stage or task labels. The skills' Rust examples compile (`tests/skill-examples`).
- **The gate**, before each task's last commit:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo check -p ryo2d --lib --target wasm32-unknown-unknown`
  - `cargo check -p skill-examples`
  - `cargo test --doc -p ryo2d tilemap`
  - `cargo test --workspace`
  - `cargo test --test tilemap_alloc`
- **The spec's targets on hex grids** (Task 8 guards and records them):
  - **Edit:** an edit rebuilds only its chunk and the loops through it, within 100 µs for loops spanning up to four chunks.
  - **Static map:** a static 1024 x 1024 four-layer map adds at most 50 µs of CPU a frame, its layers colliding.
  - **Load:** a 256 x 256 four-layer map loads off the main thread within 20 ms.

## Review Focus

1. **Degenerate hex grids Tiled saves.**
   - A `hexsidelength` of 0: four cells meet at a corner, as on a staggered map.
   - A side longer than the tile: the slants are 0, so the outlines are rectangles.
   - Odd tile sizes, whose half-width truncates.

   Each traces closed outlines with no zero-length edge and no panic. Regions touching only at a point stay apart. (Task 2, `degenerate_hex_grids_trace_closed_outlines_without_zero_length_edges`.)
2. **Flat-topped hexes and even stagger indices at negative cells.** Outlines, neighbours and paths agree with `MapGrid`'s own cells across rows and columns below zero (Task 1, `hex_cells_agree_with_cell_outlines_on_every_variant`; Task 5, `hex_paths_cross_negative_rows_and_columns_on_every_variant`).
3. **A turned or flipped hex tile with a shape of its own.**
   - Its ring turns with its art, about the pivot the drawing uses.
   - A whole-hexagon shape on a turned tile still collides as the whole cell.
   - On hex maps bit 29 is the 60° turn, never a diagonal flip.

   (Task 3, `a_turned_hex_tile_s_shape_turns_with_its_art`.)
4. **A navigation window crossing the edge of a streamed map's loaded area.** Cells in chunks that aren't loaded block, and no path routes through them. A grid taken again after a `ChunkEvent` opens them. (Task 6, `a_streamed_map_s_navigation_blocks_unloaded_chunks_until_they_land`.)
5. **A map whose root is moved, turned or scaled.** A layer navigation grid converts through the layer's whole world transform, and `block_tiles` blocks through it, not just its translation. (Task 5, `a_turned_map_s_navigation_follows_its_transform`.)

---

### Task 1: One integer geometry for hexagonal cells

**Files:**
- Create: `src/core/physics/tiles/hex.rs` (declared from `src/core/physics/tiles.rs`)
- Modify: `src/core/tilemap/grid_shape.rs` (the hex arm of `cell_outline`, and `Lines::outline`, read their corners from `HexCells`); `src/core/tilemap/grid_shape/tests.rs`

**Interfaces:**
- Produces:
  ```rust
  /// A hexagonal grid's cells in whole pixels from the grid's origin
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub(crate) struct HexCells { /* axis, index, half, slant, side: integers */ }
  impl HexCells {
    /// `None` when a cell is wider or taller than 16,384 px
    pub(crate) fn new(axis: StaggerAxis, index: StaggerIndex, cell: UVec2, side: u32) -> Option<HexCells>;
    /// Clockwise on screen from the top-most corner, as `CellOutline` gives them; a side of length 0 repeats its corner
    pub(crate) fn corners(&self, cell: IVec2) -> [IVec2; 6];
    /// The cell across side `side` (corner `side` to corner `side + 1`), 0..6
    pub(crate) fn across(&self, cell: IVec2, side: usize) -> IVec2;
    /// Whether every corner of `cell` and of its neighbours fits the tracer's i32 points
    pub(crate) fn reaches(&self, cell: IVec2) -> bool;
  }
  impl MapGrid { pub(crate) fn hex_cells(&self) -> Option<HexCells>; } // None off hex
  ```
  `StaggerAxis` and `StaggerIndex` move to physics or are mirrored there, whichever keeps the dependency pointing from tilemap to physics. Physics must not import tilemap.

- [ ] **Step 1: Write the failing tests** in `grid_shape/tests.rs`:
  - `hex_cells_agree_with_cell_outlines_on_every_variant`: for both axes, both indices, and the cell sizes and sides (16x16, 8), (120x140, 70), (15x17, 7), (16x16, 0) and (16x16, 24), each at origins (0, 0) and (5, -3). For every cell in -40..40 squared, `hex.corners(cell)` plus the origin equals `grid.cell_outline(cell).points()` exactly.
  - `each_hex_side_is_its_neighbours_side_reversed`: side `s` of cell `c` runs corner `s` to `s + 1`, and is side `(s + 3) % 6` of `hex.across(c, s)` with its ends swapped. Check every cell in -20..20 squared on every variant above with a side above 0.
  - `hex_cells_refuse_cells_past_16384_px`: `HexCells::new(.., UVec2::new(16_385, 16), 8)` is `None`, and a 16,384 px cell is `Some`.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib hex_cells each_hex_side`. Expected: it fails to compile, because `HexCells` doesn't exist yet.
- [ ] **Step 3: Implement `HexCells`** from the integer rules `Lines` uses today: `half = across / 2`, `slant = max(0, (along - side) / 2)`, `step = (along - side) / 2 + side`, and the shift by stagger index. Integer division truncates toward zero, as `Lines` does. Then make `MapGrid`'s hex outlines read `HexCells::corners`, so the hex corner formula lives in one place.
- [ ] **Step 4: Run** the new tests and every existing `grid_shape` test: `cargo test -p ryo2d --lib grid_shape`. Expected: PASS. Every existing hex outline, picking and placement test passes unchanged.
- [ ] **Step 5: Gate, then commit**
  ```bash
  git add src/core/physics/tiles.rs src/core/physics/tiles/hex.rs src/core/tilemap/grid_shape.rs src/core/tilemap/grid_shape/tests.rs
  git commit -m "refactor: hexagonal cell corners come from one integer geometry the collision code shares"
  ```

### Task 2: The tracer traces hexagonal outlines

**Files:**
- Modify: `src/core/physics/tiles.rs` (a hex variant beside `lattice`; `at`, `origin`, `point`, `pack_all` and `cells_in` for hex; `Boundary::scan`); `src/core/physics/tiles/outline.rs` (hex units, `unit_edges`, `boundary_edges`, `units_of`); `src/core/physics/tiles/pack.rs` (the point-to-chunk key for hex); `src/core/physics/AGENTS.md` ("Outlines")
- Test: `src/core/physics/tiles/outline.rs` and `src/core/physics/tiles.rs` test modules

**Interfaces:**
- Consumes: `HexCells` (Task 1).
- Produces:
  - `TileColliders::with_hex(self, hex: HexCells, origin: Vec2) -> Self` (crate-internal).
  - `TileColliders::hex(&self) -> Option<(HexCells, Vec2)>` (crate-internal).
  - `TileColliders::cell_polygon(&self, cell: IVec2) -> CellPolygon`, with `CellPolygon { points: [Vec2; 6], len: u8 }`. It gives a cell's corners in the colliders' own pixels: 4 corners on square lattices, 6 on hex. Tasks 5 and 6 read it.
- **Rules the code must keep:**
  - **Points:** a hex point is a corner's whole-pixel position relative to the grid origin, from `HexCells::corners`, so points are exact integers and edges are clockwise on screen.
  - **Units:** each of a cell's six sides is owned by whichever of its two cells comes first in row-major order (smaller y, then smaller x). An edit's units are the changed cells' six sides.
  - **Edges:** a side with exactly one solid cell is a boundary edge, clockwise around that cell. A side of length 0 is no edge.
  - **Scans:** bounded groups emit the sides on their outer border too, owned by cells outside the bounds. Unbounded groups scan the chunks next to existing ones for the sides they own, as the square scan does.
  - **Packing:** a loop's pack is the chunk of its first corner, `div_euclid` of the point by the chunk box (32 cells across by 32 lines along, in the stagger frame). A part sits at the grid origin plus its chunk box's corner, with chunk-relative vertices.
  - **The rest stays as it is:** `Outlines::{trace, apply, splice}`, `sharper` and `merge_straight_runs`.
  - **Reach:** cells whose corners don't fit `i32` points (`!hex.reaches(cell)`) draw but don't collide, as square cells past `MAX_REACH` do.

- [ ] **Step 1: Write the failing tests:**
  - `a_hex_cell_traces_one_six_corner_loop`: one solid cell at (3, -2) on (16x16, 8, axis Y, odd). There is one loop, whose ring equals `corners((3, -2))` from its top-most corner, clockwise.
  - `hex_outlines_are_the_sides_with_one_solid_cell`: 200 random seeded sparse sets of 30 cells in -40..40 squared, on each variant from Task 1 with a side above 0. The traced edges are exactly the sides with one solid cell, oriented as above, with each loop closed.
  - `edits_on_a_hex_grid_splice_to_what_a_whole_trace_gives`: a seeded random walk of 400 one-cell edits on an unbounded grid crossing four chunks, negative ones included. After each edit, `apply`'s loops equal a fresh `Outlines::build` as sets of rings. At least one splice is taken, counted through the test-only `splices` counter.
  - `a_bounded_full_hex_grid_is_one_loop_round_its_border`: a 7 x 5 grid, all solid, gives one loop whose corners are the union's border.
  - `hex_parts_sit_at_their_chunk_box_with_chunk_relative_vertices`: cells in chunks (0, 0) and (-1, 2). Each part's `Transform` is the chunk box's corner plus the origin, and its vertices lie within the chunk box's size plus one cell.
  - `degenerate_hex_grids_trace_closed_outlines_without_zero_length_edges`:
    - side 0: two cells meeting only at a corner give two loops;
    - side 24 on 16 px cells: rectangles;
    - 15 x 17 cells with side 7: a region's loop closes.

    No ring repeats a point consecutively.
  - `a_hex_cell_past_the_reach_draws_but_does_not_collide`: a cell at x = 40,000,000 on 64 px cells builds no loop and no panic.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib hex_ -- --test-threads 4`. Expected: it fails to compile (`with_hex` is missing).
- [ ] **Step 3: Implement** the hex variant as the rules above say. The square paths stay byte-for-byte as they are.
- [ ] **Step 4: Run** the new tests, then `cargo test -p ryo2d --lib physics::tiles`. Expected: PASS, with every square tracer test unchanged.
- [ ] **Step 5: Gate, then commit**
  ```bash
  git add src/core/physics/tiles.rs src/core/physics/tiles/outline.rs src/core/physics/tiles/pack.rs src/core/physics/AGENTS.md
  git commit -m "feat: tile collision traces outlines on hexagonal grids"
  ```

### Task 3: Hexagonal tile layers collide

**Files:**
- Modify:
  - `src/core/tilemap/kinds.rs`: a hex `Placement` in place of `Nothing`, the whole-hexagon test, and hex placement in `placed()` and `chunk_kinds`.
  - `src/core/tilemap/collision.rs`: plan hex layers with `with_hex`, and drop the hex warning.
  - `src/core/tilemap/map.rs`: `add_tile_layer` no longer turns collision off on hex, and its doc changes.
  - `src/core/tilemap/tiled/build.rs`: drop the load refusal.
  - `src/core/tilemap/tiled/mod.rs` and `tiled/xml.rs` tests.
  - `src/core/render/tiles.rs`: `quad_turn` moves to `src/core/tilemap/tile.rs` as `pub(crate) fn TileFlip::quad_turn(self, hex: bool) -> Mat2`, used by both drawing and shapes.
  - `src/core/physics/tiles.rs`: a hex cell's own shapes as rings, `PackKey::Shapes`.
  - `src/core/physics/tiles/pack.rs`.
  - `src/core/physics/character.rs` tests.
  - `src/core/tilemap/reload/tests.rs`.
  - `tests/tilemap_alloc.rs`.
  - Docs: `src/core/tilemap/AGENTS.md` and the `ryo-tilemap` skill: hex layers collide, shapes on hex maps, and the reserved properties text.
- Fixtures: `tests/fixtures/tilemap/tiled/scripts/make-fixtures.js` writes the hex collision maps and tilesets below. Regenerate the fixtures, and run the script twice to confirm the same bytes.

**Interfaces:**
- Consumes: `TileColliders::with_hex`, `HexCells` (Tasks 1-2).
- Produces:
  - Hex `TileColliders` built by maps.
  - `PackKey::Shapes(i32, i32)`: the own-shape rings of one chunk's hex cells, rebuilt whole when one of that chunk's cells gains, loses or changes a shape.
- **Rules:**
  - **Placing a tile's shapes on hex:** image pixels go to cell pixels from `MapGrid::tile_anchor`. Then the horizontal and vertical flips apply, then the hex turn: `TileFlip::quad_turn(hex: true)` about the tile quad's middle, the pivot drawing uses. Bit 29 is the 60° turn here, never a diagonal flip.
  - **Whole hexagon:** after the flips and before the turn, the shape's cleaned ring has the cell's distinct corners, each within 0.5 px, and no other point. Such a tile is `TileCell::Solid`, whatever its turn.
  - **Any other shape** is `TileCell::Shape(i)`: its polygons, turned and flipped, are closed rings in its cell's chunk pack. They are not traced and not merged with neighbours. `shape_blocks(i)` is whether a polygon contains the cell's centre.
  - **One-way** tiles collide both ways, with the existing once-per-map warning.

- [ ] **Step 1: Write the fixtures.** Add these maps in both formats to `make-fixtures.js`:
  - `collide-hex-y-odd`: 120 x 140 cells, side 70, axis Y, odd index.
  - `collide-hex-x-even`: 140 x 120, side 70, axis X, even index.

  Each uses one tileset with three tiles:
  - a tile whose collision shape is its whole hexagon;
  - a tile with a rock-sized triangle off-centre;
  - a tile with no shape.

  Paint a 3-cell clump of the whole-hexagon tile, a lone triangle tile placed plain, 60° turned and H-flipped, and blank tiles around them. Run the script twice and compare `shasum` output.
- [ ] **Step 2: Write the failing tests:**
  - `a_hex_layer_collides_along_its_hexagons` (collision.rs), on both fixtures and both formats:
    - the clump is one loop, equal to its cells' union border from `cell_outline`;
    - each triangle tile is one ring of 3 points;
    - blank tiles add nothing.
  - `a_turned_hex_tile_s_shape_turns_with_its_art`: the turned and flipped triangles' rings equal the plain triangle's points put through the same `quad_turn` and flip the drawing uses. A whole-hexagon tile turned 60° and 120° is still `Solid`.
  - `a_hex_layer_that_collides_loads` replaces `a_hex_layer_that_collides_is_refused_and_one_that_does_not_loads`, in tiled/mod.rs and tiled/xml.rs. It also checks that `add_tile_layer` on a hex `TileMap` gives `collides == true`.
  - `an_edit_on_a_hex_map_retraces_only_its_loops`: an infinite hex map with three clumps in chunks (0, 0), (1, 0) and (-1, -1). Painting one cell changes only the parts of its chunk and the loops through it, and the other parts keep their entities.
  - `a_streamed_hex_chunk_unloads_cleanly` (beside the staggered one): a loop crossing into an unloaded chunk closes at its edge, and reloading the chunk restores the original loop.
  - `a_top_down_character_slides_along_a_hex_wall_with_no_floor` (character.rs): a top-down character pushed into a row of 10 solid hexes (120 x 140, side 70) never sits inside them. It ends `walled`, never `on_floor` or `on_ceiling`, and it slid along.
  - `a_hex_tileset_saved_with_a_new_shape_rebuilds_its_colliders` (reload tests): saving the fixture's tileset with the triangle tile's shape made a whole hexagon turns that tile's cells into part of the outlines, and the ring pack goes.
  - The existing `a_code_built_hex_layer_that_collides_warns_once_and_builds_nothing` becomes `a_code_built_hex_layer_collides`.
  - `tests/tilemap_alloc.rs` gains a hex map whose layer collides and casts shadows, so its warm frame is held to no allocation.
- [ ] **Step 3: Run** `cargo test -p ryo2d --lib hex`. Expected: FAIL. Today hex layers are refused or build nothing.
- [ ] **Step 4: Implement** as the rules say. Drawing must not change: moving `quad_turn` keeps every pixel, as the tile-bleeding and hex-turn tests show.
- [ ] **Step 5: Run** `cargo test -p ryo2d --lib tilemap physics`, then the existing hex render tests: `cargo test -p ryo2d --lib hex_turn shape_hex`. Expected: PASS.
- [ ] **Step 6: Gate, then commit**, staging every file listed above and the regenerated fixtures by name:
  ```bash
  git commit -m "feat: hexagonal tile layers collide, from whole hexagons and their tiles' own shapes"
  ```

### Task 4: Hexagonal tile layers cast shadows

**Files:**
- Modify:
  - `src/core/tilemap/occluders.rs`: plan hex layers, drop the hex warning, map hex points to pixels, and chain own-shape rings.
  - `src/core/physics/tiles.rs`: `Boundary` over hex units.
  - Docs: `src/core/tilemap/AGENTS.md` ("Boundary chains", "Other grids") and the `ryo-lighting` skill's tile-shadows lines, if they say hex casts none. If you touch `ryo-lighting`, bump it a minor version once.

**Interfaces:**
- Consumes: the hex units and `Boundary` scan (Task 2), and the hex kinds (Task 3).
- Produces: `MapOccluder` chains for hex layers. The whole-hexagon regions' borders are chains from `Boundary`, and each own-shape ring is a closed chain. Lighting's winding is the reverse of the edges, as today.

- [ ] **Step 1: Write the failing tests** (occluders.rs):
  - `shadows_follow_hex_outlines`: on both Task 3 fixtures, the occluder chains, mapped to world points, equal the collision loops and rings as point sets, up to each chain's start.
  - `an_edit_rebuilds_only_its_chunk_s_occluders_on_a_hex_map`: as the square test of that name, on the infinite hex map from Task 3.
  - `occluder_marked_shapes_cast_the_shadow_on_hex`: a tile with one collision triangle and one `occluder` square casts the square.
  - Extend `a_streamed_hex_chunk_unloads_cleanly` (Task 3) to assert the occluders as well.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib occluders`. Expected: FAIL (hex casts none).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p ryo2d --lib occluders lighting`. Expected: PASS. The square occluder tests are unchanged.
- [ ] **Step 5: Gate, then commit**
  ```bash
  git add src/core/tilemap/occluders.rs src/core/physics/tiles.rs src/core/tilemap/AGENTS.md
  git commit -m "feat: hexagonal tile layers cast shadows along their outlines"
  ```

### Task 5: Navigation grids over any tile grid's own cells

**Files:**
- Modify: `src/core/navigation/grid.rs`, `pathfinder.rs` (heuristic and tie-break by the grid), `path.rs` (no change expected: smoothing uses `line_of_sight`), `mod.rs`, `src/core/navigation/AGENTS.md`
- Test: `src/core/navigation/grid.rs` and `pathfinder.rs` test modules, `tests/integration/navigation.rs`
- Docs: the `ryo-api` (Navigation), `ryo-character` (:259), `ryo-physics` (:136) and `ryo-game-dev` (:762) skills.

**Interfaces:**
- Consumes: `LayerGrid` and `MapGrid` (`neighbours`, `line`, `lattice`, `cell_at`, `cell_center`), `CellRect`, and `TileColliders::{cell_polygon, hex, lattice, cells_in}`.
- Produces (public, and in the prelude where noted):
  ```rust
  impl NavigationGrid {
    /// A grid over `window` (max exclusive) of a tile layer's own cells: `GridCoord`s are the layer's cells, signed
    pub fn for_layer(grid: LayerGrid, window: CellRect) -> NavigationGrid;
    /// The cells it covers: (0, 0)..(width, height) for a grid made with `new`
    pub fn window(&self) -> CellRect;
    /// The layer it was made over, `None` for a square world grid
    pub fn layer(&self) -> Option<LayerGrid>;
    /// Breaking: was `origin: Vec2`. `to_world` is the tiles' entity's whole world transform
    pub fn block_tiles(&mut self, tiles: &TileColliders, to_world: Affine2);
  }
  impl From<IVec2> for GridCoord { .. }  impl From<GridCoord> for IVec2 { .. }
  ```
- **Rules:**
  - **Neighbours on a layer grid:**
    - hex: `MapGrid::neighbours`, six;
    - every other grid: the eight neighbours in its square lattice (`Lattice::lattice_cell`, then `grid_cell` back for staggered maps). A diagonal needs both lattice cardinals beside it walkable, as today.
  - **Step cost:** the world distance between the two cells' centres divided by the grid's shortest step, times the entered cell's cost. Per-direction lengths are computed once (the grids are affine in lattice or axial coordinates).
  - **Heuristic:** the world distance between centres divided by the shortest step, times `heuristic_weight`.
  - **Tie-break:** from the world centres divided by the shortest step.
  - **The square world grid:** unchanged, with octile costs and its integer tie-break.
  - **World conversion:** `world_to_grid` is `LayerGrid::cell_at_world`, and `grid_to_world` is `cell_center_world`. `grid_to_world_corner` is the cell box's top-left. `cell_size()` on a layer grid is the shortest step in world units.
  - **Bounds and window:** `in_bounds` means inside the window, and outside it reads `Blocked`. A window over 4,194,304 cells shrinks about its centre to 2048 x 2048, warned once.
  - **Line of sight:**
    - hex: `MapGrid::line(from, to)`, every cell after the start walkable;
    - other grids: today's Bresenham walk in lattice cells.
  - **`block_tiles`:**
    - **Same cells:** when the tiles' cells are this layer grid's cells (same `MapGrid`, and `to_world` within 1e-4 per element), it blocks cell for cell. Staggered lattice cells map back through `grid_cell`. A cell blocks when it is `Solid` or `Slope`, or a `Shape` whose `shape_blocks` is true.
    - **Otherwise:** each blocking cell's world polygon (`cell_polygon`, then `to_world`) blocks every grid cell whose polygon overlaps its interior (convex SAT, strict). The upright square fast path stays.
    - The non-orthogonal warning is gone.
  - **`block_rect` and `block_collider` on a layer grid** block every cell whose polygon meets the rect, edges included.
  - **`rebuild_from_world`** passes each `TileColliders` entity's `GlobalTransform` affine.

- [ ] **Step 1: Write the failing tests:**
  - `an_orthogonal_layer_grid_paths_as_the_square_grid_does`: a 30 x 20 orthogonal layer (16 px cells, origin (0, 0)) and `NavigationGrid::new(480.0, 320.0, 16.0)`, with the same walls. 50 seeded start and goal pairs give paths of equal total cost (within 1e-3) and equal reachability.
  - `isometric_steps_cost_their_screen_distance`: on 116 x 55 isometric cells, a path from (0, 0) to (6, 6) runs along the lattice diagonal. Its cost is 6 × 55 / step_min within 1e-3, and `step_min` is 55.
  - `hex_cells_have_six_neighbours_and_path_around_a_wall`: on (120 x 140, 70, axis Y, odd), a wall of 7 blocked hexes with one gap. The path goes through the gap, and its cost equals a Dijkstra over `MapGrid::neighbours` with the same step costs.
  - `hex_paths_cross_negative_rows_and_columns_on_every_variant`: on every Task 1 variant with a side above 0, in a window of (-12, -12)..(12, 12), each path step is a `MapGrid::neighbours` pair, and the path's cost equals the Dijkstra's.
  - `a_staggered_layer_grid_paths_in_its_own_cells`: path cells are the map's grid cells. Each step is a lattice neighbour (eight-way), and no step cuts a blocked corner.
  - `line_of_sight_follows_the_grid`: a hex line blocked by one cell is blocked; the same on an isometric lattice. `smooth_line_of_sight` on a hex path keeps every remaining waypoint pair in sight.
  - `block_tiles_blocks_a_layer_s_own_cells_cell_for_cell`: hex `TileColliders` from Task 3's builder with 5 solid cells and one off-centre shape cell whose polygon misses the centre. Exactly the 5 cells block.
  - `a_square_grid_reads_isometric_and_hex_tiles_by_overlap`: a square 8 px grid over one solid diamond, then one solid hexagon. The blocked cells are exactly those whose square overlaps the polygon's interior, computed independently in the test by sampling 16 points per cell.
  - `a_turned_map_s_navigation_follows_its_transform`: a layer turned 30° and scaled 2x. `grid_to_world` → `world_to_grid` round-trips every cell, and `block_tiles` with that affine blocks the turned cells.
  - `a_window_past_four_million_cells_shrinks_and_warns_once`: a 3000 x 3000 window (9 M cells, about 72 MB before the guard) gives `dimensions() == (2048, 2048)`, centred, and one warning.
  - Rename `navigation_refuses_a_non_orthogonal_map_with_one_warning` to `navigation_reads_a_staggered_map_s_tiles`, asserting the blocked cells.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib navigation`. Expected: it fails to compile (`for_layer` is missing).
- [ ] **Step 3: Implement** as the rules say. The square grid's code path stays as it is.
- [ ] **Step 4: Run** `cargo test -p ryo2d --lib navigation` and `cargo test --test integration navigation`. Expected: PASS, with every existing navigation test unchanged except the renamed one.
- [ ] **Step 5: Gate, then commit**
  ```bash
  git add src/core/navigation/grid.rs src/core/navigation/pathfinder.rs src/core/navigation/mod.rs src/core/navigation/AGENTS.md tests/integration/navigation.rs .claude/skills/ryo-api/SKILL.md .claude/skills/ryo-character/SKILL.md .claude/skills/ryo-physics/SKILL.md .claude/skills/ryo-game-dev/SKILL.md
  git commit -m "feat: navigation grids path over any tile grid's own cells, six neighbours on hex and screen distances elsewhere"
  ```

### Task 6: A map hands out a navigation grid over a window of its cells

**Files:**
- Modify: `src/core/tilemap/view.rs` (`MapsView::navigation`); `src/core/tilemap/stream.rs` (a crate-internal read of the resident chunks); `src/core/tilemap/AGENTS.md`; the `ryo-tilemap` skill (Navigation, and the streamed-map paragraph at ~632) and `ryo-api` (MapsView)
- Test: `src/core/tilemap/view.rs` or `src/core/tilemap/spawn/signed.rs` test modules

**Interfaces:**
- Consumes: `NavigationGrid::for_layer` and `block_tiles` (Task 5).
- Produces:
  ```rust
  impl MapsView<'_> {
    /// A navigation grid over `window` of `layer`'s cells: every colliding layer of `map` blocks through
    /// `block_tiles`, and on a streamed map every cell of a chunk not resident is `Blocked`.
    /// `None` until the map lands, or when `layer` isn't a tile layer
    pub fn navigation<'a>(&self, map: Entity, layer: impl Into<LayerRef<'a>>, window: CellRect) -> Option<NavigationGrid>;
  }
  ```
  The game adds its own colliders (`block_collider`) and costs (`set`), and takes the grid again after `ChunkEvent`s or edits.

- [ ] **Step 1: Write the failing tests:**
  - `a_map_s_navigation_blocks_its_colliding_layers`: a bounded hex map with a colliding wall layer and a plain floor layer. `navigation(map, "floor", bounds)` blocks exactly the wall's solid cells, and a path goes round them.
  - `a_streamed_map_s_navigation_blocks_unloaded_chunks_until_they_land`: a streamed orthogonal world. The window spans resident and non-resident chunks.
    - Every cell of a non-resident chunk reads `Blocked`, and a path between two resident cells across that gap is `NoPath`, or goes round it.
    - After the anchor moves and the `ChunkEvent`s arrive, a grid taken again opens those cells.
  - `navigation_on_an_isometric_map_matches_its_collision`: an isometric map built in code with a colliding wall layer, as the collision tests' `lattice_map` builds one. The blocked cells equal the cells whose `TileColliders` kind blocks.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib navigation`. Expected: it fails to compile.
- [ ] **Step 3: Implement.** A grid over a 256 x 256 window must be taken in under 1 ms on a release build. Task 8 records the time.
- [ ] **Step 4: Run** the new tests. Expected: PASS.
- [ ] **Step 5: Gate, then commit**
  ```bash
  git add src/core/tilemap/view.rs src/core/tilemap/stream.rs src/core/tilemap/AGENTS.md .claude/skills/ryo-tilemap/SKILL.md .claude/skills/ryo-api/SKILL.md
  git commit -m "feat: a map hands out a navigation grid over a window of its cells, its collision and unloaded chunks blocked"
  ```

### Task 7: The demo paths on hex and isometric maps

**Files:**
- Modify:
  - `examples/demos/tile-grids/scripts/make-maps.js`:
    - in `hexagon-board.tsx`, the rock and stone tiles (`grass_14`, `grass_15`, `grass_16`, `dirt_16`, `dirt_18`, `dirt_17`, `stone_07`) get a whole-hexagon collision shape;
    - the tree and forest tiles (`grass_10` to `grass_13`, `dirt_11` to `dirt_14`) get a `cost` float property of 2;
    - the board layer collides and casts shadows;
    - the header comments stop saying hex layers don't collide.
  - Regenerate `hex-board.tmx` and `hexagon-board.tsx` (headless Tiled, twice, same bytes).
  - `src/hexagonal.rs`, `src/isometric.rs`, `src/walker.rs` (shared walking along a path, if both pages need it).
  - `tests/grids.rs`, `tests/pages.rs`.
  - `assets/README.md` (pages and hand-play items).
- **Hex board:**
  - **The walker:** a walker (`spawn_walker`) starts on the board's hex (1, 4). WASD walks it top-down, and the rocks' hexagons stop it.
  - **The path:** hovering a hex shows the cheapest path from the walker's hex as a line through hex centres, drawn as the outlines are. Trees cost 2 (from `cost`), and rocks and stone block (from collision, through `ctx.maps().navigation`).
  - **Right-click** walks the walker along that path.
  - **HUD lines:** `Path: {n} hexes, cost {c}` or `No path to hex {x}, {y}`.
  - Click-to-select, range, ring, neighbours and line stay as today.
  - **Controls:** `Control::mouse("Point at a hex, click to select it, right-click to walk there")`, plus WASD.
- **Town:**
  - Space walks the knight to the cell under the pointer, along an isometric path round walls and water.
  - After painting, the path reads the new water: the grid is taken again when the town's tiles change.
  - The control bar adds `Space Walk to the pointer`.

- [ ] **Step 1: Write the failing tests:**
  - `the_hex_board_s_walker_paths_around_the_rocks` (grids.rs): hover a hex behind a rock clump. The HUD shows a path, every path hex is walkable, and none is a rock or stone hex. Right-click, step 600 frames: the walker's hex is the goal.
  - `the_hex_board_s_rocks_stop_the_walker`: hold D toward a rock hex for 120 frames. The walker never enters it.
  - `the_town_knight_walks_a_path_around_the_walls`: point at a cell inside the walls from outside, press Space, step 900 frames. The knight is at that cell and never crossed a wall cell.
  - Update `every_page_plays_for_five_seconds_without_a_warning`'s expectations if it lists warnings.
- [ ] **Step 2: Run** `cargo test -p tile-grids-demo`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p tile-grids-demo`. Expected: PASS.
- [ ] **Step 5: Look at the pages.** Capture pages 4 and 6 headless (`RYO_HEADLESS=1`, `RYO_HEADLESS_HZ=60`, `RYO_CAPTURE_FRAMES`, `RYO_CAPTURE_EXIT=1`, `RYO_CAPTURE_DIR` under the scratchpad, `RYO_INPUT` for the keys and pointer). Describe the path line, the walker and the knight in the report.
- [ ] **Step 6: Gate, then commit**, staging the files above and the regenerated map and tileset by name:
  ```bash
  git commit -m "feat: the hex board's walker paths around rocks, and the town's knight walks to the pointer"
  ```

### Task 8: The guards and the record

**Files:**
- Modify:
  - `benches/tilemap.rs`:
    - `Grid::collides()` is true for hex;
    - `rock()` gives a hexagon on hex;
    - the `edit` guard runs on hex (16 x 16 cells, side 8, odd rows);
    - `RYO_TILEMAP_GRID=hexagonal` no longer maps `edit` to orthogonal;
    - the header comments say every grid.
  - `docs/backlog/tilemap.md`:
    - the record gains hex rows for edit, static and load, with the map's layers colliding;
    - a measured row for path times: a 256 x 256 window, 100 seeded paths on orthogonal, isometric and hex layer grids, median and 99th percentile, no held target;
    - a row for `MapsView::navigation` on a 256 x 256 window;
    - Limits: the hex own-shape rule ("not merged with neighbours"), and any target missed.
  - `docs/backlog/tilemap-features.md`: drop hex collision and navigation topology.
  - `docs/backlog/follow-ups.md`: remove the lines this stage fixed. Check the whole file: the navigation warning naming hex maps, hex layers never reaching `block_tiles`, and "stage 5 owns infinite navigation windows".
  - The skills' and module notes' figures, if any quote them.
- [ ] **Step 1: Measure** on a quiet machine, recording `uptime` before each set:
  - `cargo bench --bench tilemap` with `RYO_TILEMAP_GRID=hexagonal`: static, edit and load, three runs each;
  - a hex stream run, measured and recorded but not held;
  - the path times and the `navigation` time from a release build.

  A held target that misses is reported, never hidden.
- [ ] **Step 2: Write the record, the Limits and the backlog changes.** Every number quoted in a Limit matches a record row in the same file.
- [ ] **Step 3: Gate, then commit**
  ```bash
  git add benches/tilemap.rs docs/backlog/tilemap.md docs/backlog/tilemap-features.md docs/backlog/follow-ups.md
  git commit -m "docs: hexagonal collision and pathfinding are in the record and the tile-map docs"
  ```

---

## After the tasks

The controller does the close-out:
- the whole-branch review;
- the scripted tour against main: pages without tile maps byte-identical, square-grid tile pages byte-identical (collision and navigation don't draw), the hex board and the town compared by eye;
- the owner's hand-play of the hex board and the town;
- the merge question and the follow-ups.
