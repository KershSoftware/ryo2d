# Tile Grid Shapes (6b, Stage 4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tiled's isometric, staggered, hexagonal and oblique maps load in both formats and behave as Tiled shows them:
- drawing in Tiled's order, hex rotations included;
- objects in place, edits, picking and grid tools;
- sorting with characters;
- terrain painting on isometric and staggered maps;
- worlds, infinite maps and streaming;
- collision and shadows on the grids whose cells form a square lattice.

Maps built in code take any of these grids. Hex layers that collide or occlude stay refused until hex collision exists.

**Architecture:**
- **One geometry:** a public `MapGrid` (the shape, the cell size and where cell (0, 0) sits) answers every cell-and-pixel question. A tile layer carries its `MapGrid` instead of a bare cell size, and every site that wrote `cell * cell_size` goes through it.
- **Collision and shadows trace in a square lattice:** the index space for isometric and oblique maps, and the isometric lattice for staggered ones. Points are mapped to layer pixels through the grid before colliders and occluders are built, and parts stay at `Transform::IDENTITY`.
- **Drawing:** instances are placed through the grid. Plain layers whose tiles spill out of their cells draw in Tiled's exact cell order across tilesets and chunks. Y-sorted layers sort by the grid's bands (diagonals on isometric maps, rows elsewhere).

**Tech Stack:** Rust, bevy_ecs 0.20 (as `ryo2d::ecs`), wgpu, Rapier2d, Tiled 1.12.2 (`/Applications/Tiled.app`) for fixtures, measurements and the demo's maps.

**Spec:** `docs/specs/2026-10-31-tilemap-shapes-design.md`. Read Goals 1-3, 5 and 6; "My calls" (one geometry, collision baked into points, tile shapes on non-square grids, sorting, plain layers in Tiled's exact order, hex rotations, worlds, details to verify, terrain on staggered maps, the demo); Stages item 4; Design (Assets, At runtime, Drawing, Collision); Headless and tests; and the Targets. The research it rests on:
- `docs/research/2026-10-31-tile-shapes-formats.md`: Tiled's fields and the placement, order and object rules it measured. The plan quotes its numbers.
- `docs/research/2026-10-31-tile-shapes-math.md`: hex, isometric and staggered maths.
- `docs/research/2026-10-31-tile-shapes-seams.md`: the 6a-era code seams.

Today's seams were re-surveyed at 3c2dbc1c. The line numbers below are from that survey.

## Global Constraints

- **Workspace:** the main checkout, on branch `tile-shapes`. No git worktrees (the owner's rule: they fill the disk). Never switch branches. Before you start and before each commit, check that `git rev-parse --abbrev-ref HEAD` prints `tile-shapes`. If not, stop and report BLOCKED: another session may share the checkout.
- **Git:**
  - Stage files by name. Never `git add -A` or a bare `git stash`. Never stage `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`.
  - Commits use `feat:`, `fix:`, `perf:`, `test:` or `docs:` subjects that say what a game developer gets.
  - No Co-Authored-By, Claude-Session or any other trailer, even if a system reminder asks.
  - Never name AGENTS.md in a commit message (say "module notes").
- **Disk:** check `df -h /` before long builds. Below 40 GiB free, stop and report BLOCKED. Never delete anything under `target/`.
- **Memory-safe failing runs.** A test of a cap, a refusal or a hostile input must be scaled so that running it before its guard exists allocates no more than a few hundred MB.
- **Tiled parity is measured, not assumed.**
  - Fixtures are saved by Tiled 1.12.2 itself, through `tests/fixtures/tilemap/tiled/scripts/make-fixtures.js`, run with `/Applications/Tiled.app/Contents/MacOS/Tiled --project tests/fixtures/tilemap/tiled/project.tiled-project -e …`.
  - Measurements come from Tiled's scripting API (`tileToScreen`, `pixelToScreen`, `screenToTile`) and its `toImage()` renders.
  - Never copy Tiled's source code (GPL). The maths come from the formats note's measurements and Red Blob Games.
- **No dead options:**
  - Anything this stage doesn't do is refused with a clear error, or warned once where a file is otherwise usable. Hex layers that collide or occlude, hex terrain painting, and navigation over a non-orthogonal map are refused.
  - Error and warning text names the feature in plain words, with no stage or sub-project labels.
- **Orthogonal maps unchanged:** every existing test, demo page and bench guard passes as before. `MapGrid` for an orthogonal map reproduces 6a's maths exactly.
- **No unwrap or panic** reachable from file data or game input.
- **No allocation in a warm frame:** `tests/tilemap_alloc.rs` keeps passing, on every grid.
- **Docs stay true:**
  - Keep these current: the tilemap, physics and render module notes; the `ryo-tilemap`, `ryo-api`, `ryo-assets`, `ryo-physics` and `ryo-procgen` skills; `docs/backlog/tilemap.md` (the record and Limits); and `docs/backlog/tilemap-features.md`.
  - Skills live only in `.claude/skills/`, which the CLI embeds. Bump each touched skill a minor version once on this branch, the first time it changes.
  - Plain sentences, with no stage or task labels. The skills' Rust examples compile (`tests/skill-examples`).
- **The gate**, before each task's last commit:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo check -p ryo2d --lib --target wasm32-unknown-unknown`
  - `cargo check -p skill-examples`
  - `cargo test -p ryo2d --lib cli::`
  - `cargo test --doc -p ryo2d tilemap`
  - `cargo test --workspace`
  - `cargo test --test tilemap_alloc`
- **The spec's targets on every grid** (Task 10 guards them):
  - **Static map:** a static 1024 x 1024 map of four layers adds at most 50 µs of CPU a frame over an empty frame, with a 1080p view of 16 px tiles.
  - **Edit:** an edit rebuilds only its chunk and the loops through it, within 100 µs for loops spanning up to four chunks.
  - **Load:** a 256 x 256 four-layer map of any grid, infinite or not, loads off the main thread within 20 ms.
  - **Streaming:** the streaming target holds on an isometric endless world.

## Review Focus

1. **Odd and degenerate grids Tiled saves:**
   - a negative skew;
   - `hexsidelength` 0 (a hex map that is a staggered one);
   - odd tile sizes, whose half-cell steps fall on half pixels;
   - an isometric tile ratio other than 2:1.

   Each loads, places tiles exactly where Tiled does, and draws without seams in pixel-art mode (Task 3, `odd_and_degenerate_grids_place_as_tiled_measures`; Task 5, `half_pixel_steps_draw_without_seams_in_pixel_art`).
2. **Tall tiles from two tilesets meeting at a chunk border** on a plain isometric layer draw in Tiled's order, not chunk by chunk (Task 5, `tall_tiles_across_a_chunk_border_draw_in_tileds_order`).
3. **Picking at shared edges and corners, and at negative cells.** `cell_at` gives exactly one cell, matching what draws there, and round-trips with `cell_center` for every cell in a range around the origin on every grid (Task 1, `cell_at_round_trips_and_splits_shared_edges_one_way`).
4. **A map whose grid changes on hot reload** (its orientation changed in Tiled and saved). Its layers respawn on the new grid and nothing of the old one survives: drawing, colliders, occluders and class tiles (Task 7, `a_map_saved_with_another_grid_respawns_its_layers`).
5. **A rotated or zoomed-out camera over an isometric infinite or streamed map.** Culling and streaming may take a few more chunks than they need, but never miss one on screen (Task 1, `chunks_over_covers_every_cell_a_rect_touches`; Task 7, `a_turned_camera_over_an_isometric_stream_shows_no_unfilled_chunk`).

---

### Task 1: `MapGrid`: one geometry for every grid

**Files:**
- Create: `src/core/tilemap/grid_shape.rs` (the types and their maths) and `src/core/tilemap/grid_shape/tests.rs`
- Modify: `src/core/tilemap/mod.rs`, `src/lib.rs` (prelude: `MapGrid`, `GridShape`, `StaggerAxis`, `StaggerIndex`, `CellOutline`)

**Interfaces:**
- Produces:
  ```rust
  /// How a map lays its cells out: Tiled's `orientation` and its parameters
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
  pub enum GridShape {
    #[default] Orthogonal,
    /// Diamonds; a cell's box is the diamond's bounding box
    Isometric,
    /// Diamonds in rows (axis Y) or columns (axis X), every other one shifted by half a cell
    Staggered { axis: StaggerAxis, index: StaggerIndex },
    /// Hexagons; `side` is the length in pixels of the side parallel to the stagger axis (Tiled's `hexsidelength`)
    Hexagonal { axis: StaggerAxis, index: StaggerIndex, side: u32 },
    /// Rectangles skewed: each row shifts x by `skew.x` pixels, each column shifts y by `skew.y`
    Oblique { skew: IVec2 },
  }
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub enum StaggerAxis { X, Y }
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub enum StaggerIndex { Odd, Even }

  /// A layer's grid in its own pixels (y down): the shape, a cell's size, and where the shape's (0, 0) sits
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct MapGrid { pub shape: GridShape, pub cell: UVec2, pub origin: Vec2 }

  /// A cell's outline, clockwise on screen from its top-most point (4 points, or 6 on hex maps)
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct CellOutline { /* [Vec2; 6] and a length */ }
  impl CellOutline { pub fn points(&self) -> &[Vec2]; }

  impl MapGrid {
    pub fn new(shape: GridShape, cell: UVec2) -> MapGrid;  // origin zero
    pub fn with_origin(self, origin: Vec2) -> MapGrid;
    /// Tiled's origin for a bounded map of `size` cells: on isometric maps the map's height
    /// times half a cell's width across (cell (0, 0)'s top vertex), zero for every other shape
    pub fn for_map(shape: GridShape, cell: UVec2, size: UVec2, infinite: bool) -> MapGrid;
    pub fn cell_center(&self, cell: IVec2) -> Vec2;
    pub fn cell_outline(&self, cell: IVec2) -> CellOutline;
    pub fn cell_bounds(&self, cell: IVec2) -> Rect;
    /// Where a tile image's bottom-left corner goes, before its tileset's offset (Tiled's rule on every grid)
    pub fn tile_anchor(&self, cell: IVec2) -> Vec2;
    /// The cell whose outline holds `point`; on hex maps the nearest centre among the candidates
    pub fn cell_at(&self, point: Vec2) -> IVec2;
    /// The cells sharing an edge with `cell`, clockwise from the top: 4, or 6 on hex maps
    pub fn neighbours(&self, cell: IVec2) -> impl ExactSizeIterator<Item = IVec2>;
    /// Steps between two cells through `neighbours`
    pub fn distance(&self, a: IVec2, b: IVec2) -> u32;
    /// `a` to `b`, both included, each step a neighbour (hex maps: Red Blob's cube line, nudged off edges)
    pub fn line(&self, a: IVec2, b: IVec2) -> Vec<IVec2>;
    /// Every cell within `radius` steps, `center` first
    pub fn range(&self, center: IVec2, radius: u32) -> Vec<IVec2>;
    /// The cells exactly `radius` steps away, clockwise; just `center` at radius 0
    pub fn ring(&self, center: IVec2, radius: u32) -> Vec<IVec2>;
    /// The bounding box of a rectangle of cells, outlines included
    pub fn bounds_of(&self, cells: CellRect) -> Rect;
    // crate-internal:
    pub(crate) fn chunk_bounds(&self, chunk: IVec2) -> Rect;
    /// The storage chunks (CHUNK_SIZE cells a side, by cell index) whose cells' outlines may meet `rect`: a superset, never missing one
    pub(crate) fn chunks_over(&self, rect: Rect) -> (IVec2, IVec2);
    /// The square lattice collision and shadows trace in: None on hex maps
    pub(crate) fn lattice(&self) -> Option<Lattice>;
    /// Tiled's order inside a plain layer: compare keys, smaller draws first
    pub(crate) fn paint_key(&self, cell: IVec2) -> (i32, i32, i32);
    /// Cells of one band share a sort point: c + r on isometric maps, the row elsewhere
    pub(crate) fn sort_band(&self, cell: IVec2) -> i32;
    /// The layer y a band sorts at: orthogonal maps the row's bottom edge (6a's rule); other grids its cells' centre
    pub(crate) fn band_y(&self, band: i32) -> f32;
  }
  /// A square lattice: lattice cell (x, y) is the unit square at (x, y) in lattice units
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub(crate) struct Lattice { pub to_pixel: Affine2 }
  impl Lattice {
    pub(crate) fn lattice_cell(&self, cell: IVec2) -> IVec2; // identity except on staggered maps
    pub(crate) fn grid_cell(&self, lattice: IVec2) -> IVec2;
  }
  ```

**The maths, from the formats note's measurements.** `tw`, `th` are the cell size, `(c, r)` the cell, and `shifted(i)` is `i` odd for `Odd` and `i` even for `Even`, Euclidean for negatives.
- **Orthogonal:** the cell box is `(c, r) * cell`.
- **Isometric:** the top vertex is at `((c - r) * tw/2 + origin.x, (c + r) * th/2)`.
- **Staggered Y:** the box top-left is at `(c*tw + (shifted(r) ? tw/2 : 0), r*th/2)`.
- **Staggered X:** the box top-left is at `(c*tw/2, r*th + (shifted(c) ? th/2 : 0))`.
- **Hexagonal Y:** the box top-left is at `(c*tw + (shifted(r) ? tw/2 : 0), r*(th + side)/2)`.
- **Hexagonal X:** the box top-left is at `(c*(tw + side)/2, r*th + (shifted(c) ? th/2 : 0))`.
- **Oblique:** a pixel of the unskewed grid `(px, py)` goes to `(px + py/th*skew.x, py + px/tw*skew.y)`.

Whether Tiled rounds half pixels on odd sizes is measured, not guessed: see Step 1's fixture note.

- **Tiled's order inside a layer** (`paint_key`):
  - rows top to bottom, columns left to right, on every grid;
  - on axis-X staggered and hex maps, within a row the unshifted columns come first, then the shifted ones. The key is `(r, shifted(c) as i32, c)`.
  - Oblique overlap order was never measured. Measure it in Step 1, and pin it.
- **The lattice:**
  - Orthogonal and isometric: the index space.
  - Oblique: the index space with the shear.
  - Staggered: the isometric lattice indexed differently, derived so that `cell_center` agrees.

- [ ] **Step 1: Measure what the research left open, with Tiled scripting.** Extend `make-fixtures.js` with a `measure` section that writes `tests/fixtures/tilemap/tiled/measured.json`. It holds `tileToScreen` for cells -3..3 on each of these sample grids:
  - isometric 64x32 and 48x32 (bounded and infinite);
  - staggered X/Y × Odd/Even;
  - hex X/Y × Odd/Even with side 12, and with side 0;
  - an odd-size grid of each shape (33x17 cells, hex side 13);
  - oblique with skew (8, 12) and (-10, 0).

  It also records the oblique render order of two overlapping tall tiles. Record anything surprising in the formats note.
- [ ] **Step 2: Write the failing tests** (`grid_shape/tests.rs`, pure, no world):

```rust
#[test]
fn tile_anchors_match_tileds_measurements() {
  // for every grid and cell in measured.json: tile_anchor(cell) == tileToScreen's box bottom-left (exact, f32)
  // and the formats note's table: iso 64x32 cell (1,2) anchor (64, 80); stag x odd (1,2) box top-left (32, 80);
  // hex y odd side 12 (2,1) box (80, 22); oblique skew (16, 0) cell (1,2) anchor (80, 96)
}
#[test]
fn an_orthogonal_grid_is_6a_s_rectangles() { /* center == (cell + 0.5) * cell; cell_at == floor(p / cell); band_y(r) == (r+1)*th */ }
#[test]
fn cell_at_round_trips_and_splits_shared_edges_one_way() {
  // every grid, cells -40..40 squared: cell_at(cell_center(c)) == c; a point on an edge shared by two cells
  // gives the same one of them however it is reached, and a point on a corner one cell; hex: nearest centre
}
#[test]
fn neighbours_share_an_edge_and_distance_counts_steps() {
  // each neighbour's outline shares exactly two points with the cell's; 4 on square-lattice grids, 6 on hex;
  // distance(a, b) == length of a BFS over neighbours for 200 random pairs per grid
}
#[test]
fn hex_lines_ranges_and_rings_match_red_blob() {
  // hex y odd: line((0,0),(4,-3)) is 5+ cells each a neighbour of the last; range(c, 2).len() == 19; ring(c, 2).len() == 12;
  // every grid: range(c, n) == the cells with distance <= n; ring(c, n) == distance exactly n, clockwise
}
#[test]
fn chunks_over_covers_every_cell_a_rect_touches() {
  // every grid: for 100 random rects (rotated-camera AABBs included, negative coordinates), every cell whose
  // outline meets the rect lies in a chunk in chunks_over(rect); and the range is at most one chunk wider than needed
}
#[test]
fn a_staggered_lattice_agrees_with_its_cells() {
  // staggered grids: lattice.to_pixel maps lattice cell lattice_cell(c)'s centre to cell_center(c); grid_cell inverts lattice_cell
}
#[test]
fn paint_keys_order_as_tiled_draws() { /* stag x odd: (2,2) before (3,2); stag x even: (3,2) before (2,2) and (4,2); oblique per measured.json */ }
#[test]
fn odd_and_degenerate_grids_place_as_tiled_measures() { /* the odd-size, side-0 and negative-skew rows of measured.json */ }
```

- [ ] **Step 3: Run them.** Expected: FAIL to compile.
- [ ] **Step 4: Implement `grid_shape.rs`.**
  - Hex picking takes the reference cell by division, then the nearest centre among the four candidates.
  - Hex lines, ranges and rings go through cube coordinates: odd-r/even-r for axis Y, odd-q/even-q for axis X.
  - Square-lattice lines are 4-connected, with a fixed tie-break.
- [ ] **Step 5: Run the tests.** Expected: PASS.
- [ ] **Step 6: Docs** (the tilemap module notes: the geometry and where it is used), the gate, and commit.

```bash
git commit -m "feat: MapGrid answers every grid's cell and pixel questions: placement, picking, neighbours, distance, lines, ranges and rings"
```

### Task 2: Maps and layers carry their grid

**Files:**
- Modify: `src/core/tilemap/map.rs`
  - `TileMap` gains `pub shape: GridShape` (default orthogonal), `TileMap::with_shape(self, GridShape) -> TileMap` and `TileMap::grid(&self) -> MapGrid` (through `MapGrid::for_map`).
  - `LayerKind::Tiles { cell_size }` becomes `grid: MapGrid`, and `add_tile_layer` copies the map's.
- Modify: `src/core/tilemap/spawn.rs` (`TileLayerData.cell_size` becomes `grid: MapGrid`, set in `plan_layer`), and every reader. The survey's "Where `cell_size`/`tile_size` live" list covers:
  - `edit.rs` (`set_cell`, `layer_space`, `world_to_cell`, `cell_to_world`);
  - `kinds.rs` (`chunk_kinds`);
  - `collision.rs` (`GroupKey::of`, `Inputs`);
  - `occluders.rs` (`Lit`);
  - `render/tiles.rs` (the stamp, `instance`, culling, `sort_y`);
  - `stream.rs` (`map_space`, `ChunkRequest`);
  - `reload.rs` (`LayerShape::Tiles`, the class-tile centre at :761);
  - `spawn.rs` (`spawn_class_tile` :734);
  - `ldtk/build.rs` (per-layer orthogonal grids).

  Each placement goes through `MapGrid`.
- Modify: `src/core/tilemap/stream.rs`. `ChunkRequest.tile_size` becomes `grid: MapGrid`.
- Test: beside each, and `src/core/tilemap/spawn/signed.rs`

**Interfaces:**
- Consumes: Task 1's `MapGrid`.
- Produces: `TileMap::shape`, `TileMap::with_shape`, `TileMap::grid`, `LayerKind::Tiles::grid`, `TileLayerData::grid` and `ChunkRequest::grid`. Every later task reads the grid from the layer.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn a_code_built_isometric_map_places_class_tiles_and_picks_through_its_grid() {
  // TileMap::new(8x8, 64x32).with_shape(GridShape::Isometric); a class tile at (2,1); land it
  // assert: the class tile's Transform == grid.cell_center((2,1)); MapsView::world_to_cell(grid.cell_center((5,3))) == (5,3);
  // cell_to_world((5,3)) == the centre through the layer's transform
}
#[test]
fn every_layer_copies_its_maps_grid() { /* add_tile_layer on an iso map: LayerKind::Tiles.grid == map.grid(); TileLayerData.grid the same once landed */ }
#[test]
fn a_streamed_request_carries_the_grid() { /* MapStream on an iso infinite map: the generator sees request.grid.shape == Isometric */ }
```

- [ ] **Step 2: Run them.** Expected: FAIL to compile.
- [ ] **Step 3: Replace `cell_size` with `grid` everywhere.**
  - Orthogonal behaviour must not change: the full workspace suite is the oracle.
  - **Through `MapGrid` now:** class-tile spawns (`spawn.rs`, `reload.rs`), `world_to_cell` and `cell_to_world`, and the stream's `ChunkRequest`.
  - **Type change only:** drawing, collision, occluders and the stream's areas read `grid.cell` and keep their orthogonal maths. Tasks 5, 7 and 8 move them onto the grid. Until then, a code-built non-orthogonal map draws and collides as if orthogonal, and files stay refused by the loader.
- [ ] **Step 4: Run the tilemap, physics and e2e tilemap tests, `tilemap_alloc` and both tile demos' tests.** Expected: PASS.
- [ ] **Step 5: Docs.**
  - The `ryo-tilemap` skill's "maps built in code", with a compiling `with_shape` example. This is the first change to `ryo-tilemap` and `ryo-api`, so bump each a minor version now.
  - The module notes.
  - Then the gate, and commit.

```bash
git commit -m "feat: a map and each of its layers carry their grid, and a map built in code takes any grid shape"
```

### Task 3: Tiled maps of every grid load, with hex rotations

**Files:**
- Modify:
  - `src/core/tilemap/tiled/raw.rs` (`RawMap` gains `staggeraxis`, `staggerindex`, `hexsidelength`, `skewx`, `skewy`), `tiled/xml.rs` (the same attributes) and `tiled/build.rs`:
    - replace the refusal at :1112-1122 with `GridShape` parsing; an unknown orientation or stagger value is an error naming it;
    - the `renderorder` warning (:1125) only on orthogonal maps, since Tiled ignores it elsewhere;
    - tile layers on hex maps default `collides` and `occludes` to false, and a hex layer that sets either property true is refused (Interfaces);
    - `check_chunk`'s pixel fit goes through the grid.
  - `src/core/tilemap/tile.rs`:
    - `TileFlip` gains `HEX_ROTATE_120 = 8`, and `HEX_ROTATE_60` as an alias of `DIAGONAL`'s bit;
    - `from_bits` masks `& 15`;
    - `rotated_hex_cw()` turns 60°.
  - `src/core/tilemap/grid.rs` (`decode` reads four flip bits), `tiled/data.rs` (`decode_gid` keeps bit 28 as `HEX_ROTATE_120`) and `tiled/build.rs` (clears `HEX_ROTATE_120` on non-hex maps, as Tiled ignores it there).
  - `src/core/tilemap/tiled/world_file.rs`: `map_rect` reads the grid from the raw header. A bounded level's rectangle is `grid.bounds_of(its cells)`, and an infinite one's is through `infinite_bounds`.
- Modify: `tests/fixtures/tilemap/tiled/scripts/make-fixtures.js`:
  - `shape-*` maps in both formats: every grid of Task 1's list, infinite variants of isometric, staggered, hex and oblique, a hex rotation map with every flag combination, a hex map with a colliding layer, and a world listing an isometric and an infinite hex map;
  - `renders/shape-*.png` via `toImage()`, for Task 5.
- Test: `src/core/tilemap/tiled/mod.rs`, `tiled/xml.rs`, `tile.rs`, `world.rs`. Replace `isometric_hexagonal_staggered_and_oblique_maps_fail_naming_6b`.

**Interfaces:**
- Consumes: Task 2's `TileMap::shape`.
- Produces:
  - Loaded maps carry their `GridShape`, and `TileFlip::HEX_ROTATE_120`/`HEX_ROTATE_60`.
  - **The refusal text:** `layer "{name}" on a hexagonal map collides or casts shadows; hexagonal collision and shadows aren't supported yet: set its collides and occludes properties to false`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn every_grid_loads_in_both_formats_with_its_parameters() {
  // each shape-* fixture, TMJ and TMX: map.shape equals the file's (axis, index, side, skew); tiles at their cells
}
#[test]
fn infinite_maps_of_every_grid_load_their_chunks() { /* negative chunks on iso, staggered, hex, oblique; bounds() tight */ }
#[test]
fn hex_rotation_flags_survive_loading_and_are_dropped_off_hex_maps() {
  // hex fixture: each cell's flip has the 0x2.., 0x1.. bits as HEX_ROTATE_60 / HEX_ROTATE_120 with H/V;
  // an orthogonal map with bit 28 set in a gid loads with HEX_ROTATE_120 cleared
}
#[test]
fn a_hex_layer_that_collides_is_refused_and_one_that_does_not_loads() { /* the refusal text above; the same map with collides=false loads, and with no property loads not colliding */ }
#[test]
fn a_world_places_isometric_and_infinite_hex_levels_by_their_grid_bounds() { /* rects == grid.bounds_of; positions from the .world */ }
#[test]
fn render_order_warns_only_on_orthogonal_maps() {}
#[test]
fn odd_and_degenerate_grids_load() { /* side-0 hex, negative skew, odd sizes, iso 48x32 */ }
#[test]
fn four_flip_bits_round_trip_through_a_cell() { /* tile.rs/grid.rs: 0..16 states encode/decode; rotated_hex_cw six times is the identity */ }
```

- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
  - Hex layers default to not colliding wherever a layer is built: the Tiled build and `TileMap::add_tile_layer` on a hex map.
- [ ] **Step 4: Run the tilemap tests and both tile demos' tests.** Expected: PASS.
- [ ] **Step 5: Docs.**
  - `ryo-tilemap`: the grids, the hex refusal, hex rotations and worlds. Its line 26, and `ryo-api` :1149, drop the "not supported" text.
  - Tiled can't load a world headless (the formats note §4), so where a world puts isometric and infinite levels is the engine's reading. A world places a map's tile origin at its listed position, with its rectangle from the grid. Say so in the skill.
  - `ryo-assets`: bump it now (its first change).
  - The module notes, and `docs/backlog/tilemap-features.md`.
  - Then the gate, and commit.

```bash
git commit -m "feat: Tiled's isometric, staggered, hexagonal and oblique maps load in both formats, hex rotations included"
```

### Task 4: Objects stand where Tiled shows them on every grid

**Files:**
- Modify:
  - `src/core/tilemap/tiled/objects.rs`:
    - `Alignment::parse` takes the grid, and unspecified means `Bottom` on isometric maps. Fix the call sites at :106, :111 and :329, and `tiled/build.rs:1214`.
    - `build_object` projects positions and points through the grid's object space.
  - `src/core/tilemap/spawn.rs` (`object_parts` keeps working for converted shapes).
- Test: `tiled/mod.rs` with new `shape-objects-*` fixtures from `make-fixtures.js`: a point, rectangle, ellipse, polygon, tile object and rotated rectangle on each grid.

**Interfaces:**
- Consumes: Task 3's loaded grid.
- Produces: object positions in layer pixels on every grid.

**The rules** (formats note §3):
- **Isometric:** a stored point `(px, py)` is in tile-height units along the diamond's axes. It projects to `((px - py)/th * tw/2 + origin.x, (px + py)/2)`.
  - Rectangles become their projected four corners (a `Polygon`). Ellipses become a 32-point polygon projected the same way.
  - Tile objects anchor at their alignment point after projection, bottom-centre by default.
- **Oblique:** stored points are unskewed pixels and skew through the grid. Rectangles become parallelograms, and ellipses become 32-point polygons.
- **Staggered and hex:** plain pixels, as on orthogonal maps.
- **Rotation on isometric and oblique maps:** measure in Step 1 how Tiled rotates a rotated rectangle there, and follow it. If it can't be measured headless, rotate about the projected anchor in screen space, and say so in the skill and the formats note.

- [ ] **Step 1: Measure** the rotated objects' screen corners with Tiled scripting into `measured.json`.
- [ ] **Step 2: Write the failing tests:**

```rust
#[test]
fn isometric_objects_project_from_tileds_stored_space() {
  // shape-objects-iso: the point at stored (80, 48) stands at (160, 64); the tile object at (64, 32) on a 64x64 image
  // covers x 64..128, y 16..80 (bottom-centre); the rectangle is a Polygon of its four projected corners
}
#[test]
fn oblique_objects_skew_and_staggered_and_hex_ones_do_not() { /* oblique_b's (64, 32) at (72, 56); stag and hex positions unchanged */ }
#[test]
fn a_rotated_object_matches_tileds_corners_on_every_grid() { /* measured.json */ }
#[test]
fn an_isometric_tile_object_honours_its_tilesets_alignment() { /* bottomleft: x 96..160, y 16..80; center: x 64..128, y 48..112 */ }
```

- [ ] **Step 3: Run them.** Expected: FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run the tilemap tests.** Expected: PASS.
- [ ] **Step 6: Docs** (`ryo-tilemap`'s objects section; the formats note's measured rotation), the gate, and commit.

```bash
git commit -m "feat: map objects stand where Tiled shows them on isometric, staggered, hexagonal and oblique maps"
```

### Task 5: Tiles draw where and in the order Tiled draws them

**Files:**
- Modify: `src/core/render/tiles.rs`:
  - `instance()`: placement through `grid.tile_anchor`. Hex rotations rotate the quad about the cell box's centre, flips first, through a general 2x2 in place of `axes`' two cases.
  - `chunk_instances`, `View::draw`'s culling: `grid.chunks_over` for the range, then each chunk's `chunk_bounds` tested against the camera box before the chunk is built.
  - The margin in cells.
  - The layer stamp gains the grid.
- Modify: `src/core/render/batch/chunks.rs` (rows as paint bands: below).
- Modify: `src/core/shaders/core/tile.wgsl` (the pixel-art snap), if Step 1's seam test needs it.
- Test: `tests/e2e/tilemap.rs` and `src/core/testing/tile_bleeding.rs`

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces:
  - `TileLayerData` and the draw path decide per layer whether it **spills**: a tileset it draws from has a tile whose drawn box, with offset and rotation, leaves its cell's box.
  - A spilling plain layer emits one plain draw per run of same-tileset instances in Tiled's order. The order is the grid's paint key, across tilesets and across chunks: for each paint band, the chunks left to right, then the next band.
    - A paint band is the first two parts of `paint_key`: the row, and on axis-X staggered and hex maps the shifted phase.
    - The chunk store's rows become these bands, still contiguous in a chunk's instance order.
  - A layer that doesn't spill keeps 6a's per-chunk, per-tileset draws.

- [ ] **Step 1: Write the failing tests** (GPU; skip without one):

```rust
#[test]
fn every_grid_draws_its_tiles_as_tileds_render_shows() {
  // each renders/shape-*.png fixture: the engine's frame of the same map (pixel art, zoom 1, solid-colour tiles,
  // camera on the map's bounds) equals Tiled's render on every covered pixel
}
// The render fixtures use tiles whose pixels are fully opaque or fully transparent, with no smoothed edges, on maps
// padded so no tile overhangs the render's edge (Tiled's toImage clips there).
#[test]
fn hex_rotations_and_flips_draw_as_tiled_shows_them() {
  // the hex rotation fixture: marker tiles with a coloured quadrant; at sample points well inside each quadrant,
  // the engine's colours equal Tiled's render (a rotated image is resampled, so its edges are not compared)
}
#[test]
fn tall_tiles_across_a_chunk_border_draw_in_tileds_order() {
  // a plain iso layer, tall tiles of two tilesets on both sides of a chunk border, overlapping: matches Tiled's render
}
#[test]
fn a_layer_whose_tiles_fit_their_cells_keeps_its_per_chunk_draws() { /* draw count per chunk = tilesets in it */ }
#[test]
fn culling_draws_only_chunks_a_camera_sees_on_every_grid() { /* engine.tile_chunks() for a camera over an iso and a hex map */ }
#[test]
fn half_pixel_steps_draw_without_seams_in_pixel_art() { /* odd-size staggered and hex maps, zooms 1..4 and sub-pixel cameras: no background pixel between tiles (tile_bleeding's method) */ }
```

- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
  - The pixel-art snap must hold every tile corner on a whole pixel. Snap each instance's anchor rather than only the slot's origin, if half-cell steps need it.
- [ ] **Step 4: Run the e2e and render tests, `tilemap_alloc`, and both tile demos' tests and tours.** Expected: PASS, with the orthogonal pages' captures unchanged.
- [ ] **Step 5: Docs** (the render and tilemap module notes; `ryo-tilemap`'s drawing section), the gate, and commit.

```bash
git commit -m "feat: tiles on every grid draw where Tiled places them and in its order, hex rotations included"
```

### Task 6: Characters sort with tiles on every grid

**Files:**
- Modify:
  - `src/core/render/tiles.rs`: y-sorted layers emit one item per (sort band, tileset run, chunk). `sort_y` is `grid.band_y(band)` through the layer's transform.
  - Tiles with a `sort_offset` property emit their own item at `band_y + sort_offset`.
- Modify: `src/core/tilemap/tiled/build.rs` (the reserved tile property `sort_offset`, in pixels, positive moving the sort point down) and `tileset.rs` (`TileData.sort_offset: f32`).
- Test: `tests/e2e/tilemap.rs`

**Interfaces:**
- Consumes: Task 5's draw path.
- Produces: `TileData::sort_offset`.

**The rule:**
- A tile or entity sorts by where it stands. On orthogonal maps that is a row's bottom edge (6a's rule). On every other grid it is a cell's centre, the point a character standing in the cell has its feet at.
- Isometric bands are the diagonals `c + r`. Staggered and hex bands are rows.
- Tall tiles sort at their base. Entities keep `y_sort_key` unchanged.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn a_character_walks_behind_and_in_front_of_a_tall_tile_on_every_grid() {
  // iso, staggered (both axes), hex (both axes), oblique: a y-sorted layer with a tall tree at cell T;
  // a character standing in the cell behind T draws under the tree; in the cell in front, over it (pixel samples)
}
#[test]
fn a_tiles_sort_offset_moves_its_sort_point() { /* a tall tile with sort_offset -16: a character 8 px in front of its band draws behind it */ }
#[test]
fn tile_objects_on_an_isometric_y_sorted_layer_sort_by_their_feet() {}
```

- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the e2e tests and `tilemap_alloc`.** Expected: PASS.
- [ ] **Step 5: Docs** (`ryo-tilemap`'s y-sorting section, with `sort_offset`), the gate, and commit.

```bash
git commit -m "feat: characters sort with tall tiles on every grid, and a tile's sort_offset moves its sort point"
```

### Task 7: Edits, picking, grid tools, streaming and hot reload on every grid

**Files:**
- Modify: `src/core/tilemap/view.rs`, which gains:
  ```rust
  /// A layer's grid and where its pixels are in the world
  #[derive(Clone, Copy, Debug)]
  pub struct LayerGrid { pub grid: MapGrid, pub to_world: Affine2 }
  impl LayerGrid {
    pub fn cell_at_world(&self, world: Vec2) -> IVec2;
    pub fn cell_center_world(&self, cell: IVec2) -> Vec2;
    pub fn outline_world(&self, cell: IVec2) -> CellOutline;
  }
  impl MapsView<'_, '_> { pub fn grid<'a>(&self, map: Entity, layer: impl Into<LayerRef<'a>>) -> Option<LayerGrid>; }
  ```
  `world_to_cell` and `cell_to_world` go through it.
- Modify: `src/core/tilemap/stream.rs`:
  - `map_space`, `area_of`, `chunk_range`, `nearness` and `stream_ready` go through the stream's grid (`chunks_over`, `chunk_bounds`).
  - A map whose unbounded layers have different grids streams by the first one's, and warns once.
- Modify: `benches/tilemap.rs` (the helpers `chunks_over`/`chunks_bound`, so the stream guard keeps its exact checks).
- Modify: `src/core/tilemap/reload.rs`. `LayerShape::Tiles` compares the grid, so a changed grid respawns the layer.
- Modify: `crates/ryo2d-procgen/src/tilemap.rs`. `to_tile_map` and `to_terrain_map` take a `GridShape`:
  - the procgen grid is the lattice, so for staggered maps each cell goes to `grid_cell(lattice)`;
  - a hex shape returns `None`, since the generators are square;
  - update the `ryo-procgen` skill and bump it.
- Test: `view.rs`, `stream/tests.rs`, `reload/tests.rs`, procgen's tests

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces: `MapsView::grid` and `LayerGrid`. Streaming and reload work on every grid. Procgen's `to_tile_map(&self, shape: GridShape, tile_size, tileset, tile_of) -> Option<TileMap>`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn picking_and_edits_land_on_the_cell_under_the_point_on_every_grid() {
  // for each grid: world_to_cell(outline_world(c)'s centroid) == c; set_tile there changes only c; negative cells on infinite maps
}
#[test]
fn hex_board_tools_work_through_the_layer() { /* MapsView::grid on a hex layer: neighbours, range, ring, line as Task 1 */ }
#[test]
fn an_isometric_stream_loads_the_chunks_around_a_moving_anchor() { /* resident chunks follow; stream_ready over a view */ }
#[test]
fn a_turned_camera_over_an_isometric_stream_shows_no_unfilled_chunk() { /* camera rotated 30°, zoom 0.5: stream_ready(view) once settled */ }
#[test]
fn a_map_saved_with_another_grid_respawns_its_layers() {
  // land an orthogonal fixture map, reload it saved as isometric: layers are new entities with the iso grid;
  // no colliders, occluders or class tiles from the old grid remain
}
#[test]
fn procgen_builds_isometric_and_staggered_maps_from_its_square_grid() { /* adjacent procgen cells are neighbours on the map; hex -> None */ }
```

- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the tilemap tests, the stream bench's checks (`cargo bench --bench tilemap -- --test`) and procgen's tests.** Expected: PASS.
- [ ] **Step 5: Docs** (`ryo-tilemap`'s grid tools, streaming and hot reload; `ryo-api`; `ryo-procgen`), the gate, and commit.

```bash
git commit -m "feat: picking, edits, grid tools, streaming and hot reload work on every grid, and procgen builds isometric and staggered maps"
```

### Task 8: Collision and shadows on isometric, staggered and oblique maps

**Files:**
- Modify: `src/core/physics/tiles.rs`. `TileColliders` maps lattice points to layer pixels through an affine:
  - `px`, `origin` and `at` (:1080-1120), and authored shapes (:1165);
  - a new `TileColliders::with_lattice(self, to_pixel: Affine2)`;
  - `new`/`unbounded` keep the orthogonal scale.
- Modify:
  - `src/core/tilemap/collision.rs`:
    - the group key holds the grid;
    - staggered layers write their cells at `lattice.lattice_cell(cell)`;
    - a streamed chunk's unload clears its cells through the lattice, not `unload_chunk`, since its lattice cells span several lattice chunks;
    - a hex layer that collides warns once and builds nothing (code-built maps; files are refused at load).
  - `src/core/tilemap/occluders.rs`: the same, with `px` (:410) through the lattice.
  - `src/core/tilemap/kinds.rs`: authored shapes go from the tile image's pixels into lattice-cell units through the inverse of the cell's placement, so a full diamond is `Solid`.
    - On oblique maps, shapes are taken in the unskewed cell, as on orthogonal maps, and skew with the grid. Check that this is what Tiled's collision editor shows, and record it in the formats note.
    - A `one_way` tile on a non-orthogonal map collides both ways, and the dev log warns once per map.
- Modify: `src/core/navigation/grid.rs`. `block_tiles` on colliders with a non-orthogonal lattice warns once (`navigation over isometric, staggered and hexagonal maps isn't supported yet`) and blocks nothing.
- Test: `src/core/tilemap/collision.rs` tests, `occluders.rs` tests, `src/core/physics/character.rs` (top-down on diamonds), `navigation/grid.rs`

**Interfaces:**
- Consumes: Task 1's `Lattice`; Task 2's layer grid.
- Produces: `TileColliders::with_lattice`.

- [ ] **Step 1: Measure** with Tiled scripting whether a tile's collision shapes are stored in image pixels when its tileset's grid is isometric (the spec's open detail). Record it, and convert accordingly.
- [ ] **Step 2: Write the failing tests:**

```rust
#[test]
fn an_isometric_wall_collides_along_its_diamonds() {
  // a row of full-diamond tiles: one closed polyline whose points are grid.lattice().to_pixel of the square-lattice outline
}
#[test]
fn a_staggered_map_collides_as_the_isometric_map_of_the_same_cells() {
  // the same tiles placed at lattice_cell(c) on an iso map: identical collider points (after both grids' to_pixel)
}
#[test]
fn an_oblique_map_collides_skewed() {}
#[test]
fn an_edit_on_an_isometric_map_retraces_only_its_loops() { /* other chunks' part entities unchanged */ }
#[test]
fn shadows_follow_the_same_outlines_on_every_square_lattice_grid() { /* MapOccluder chains == the collision outline's points, wound for lighting */ }
#[test]
fn a_one_way_tile_off_an_orthogonal_map_collides_both_ways_and_warns_once() {}
#[test]
fn a_code_built_hex_layer_that_collides_warns_once_and_builds_nothing() {}
#[test]
fn a_top_down_character_slides_along_a_diamond_wall_with_no_floor() { /* CharacterController::top_down against an iso wall: never grounded */ }
#[test]
fn navigation_refuses_a_non_orthogonal_map_with_one_warning() {}
#[test]
fn a_streamed_staggered_chunk_unloads_cleanly() { /* load/unload a chunk: colliders and occluders return to the state without it */ }
```

- [ ] **Step 3: Run them.** Expected: FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run the physics, tilemap, navigation and gameplay tests, and `tilemap_alloc`.** Expected: PASS.
- [ ] **Step 6: Docs.**
  - The physics module notes and `ryo-physics` (bump it).
  - `ryo-tilemap`'s collision and shadows, with the one-way and hex refusals.
  - Limits: navigation and hex collision.
  - Then the gate, and commit.

```bash
git commit -m "feat: isometric, staggered and oblique maps collide and cast shadows along their cells' outlines"
```

### Task 9: Terrain painting on isometric and staggered maps

**Files:**
- Modify: `src/core/tilemap/wang.rs`.
  - Isometric layers paint in index space, as orthogonal ones do.
  - Staggered layers paint in the lattice: `shared`, `plan`, `settle_next` and `reach` work on lattice cells, mapped through `lattice_cell` and `grid_cell`.
  - `set_terrain` on a hex layer returns false, and warns once: `terrain painting on hexagonal maps isn't supported`.
- Modify: `tests/fixtures/tilemap/tiled/scripts/paint-wang.js` (jobs on isometric and staggered X/Y × Odd/Even maps, painted by Tiled's own brush).
- Test: `wang.rs` tests (`assert_as_tiled`)

**Interfaces:**
- Consumes: Task 1's `Lattice`; Task 3's loaded grids.
- Produces: nothing new.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn isometric_terrain_paints_as_tileds_brush() { /* assert_as_tiled on the iso jobs: corner, edge, mixed */ }
#[test]
fn staggered_terrain_paints_as_tileds_brush_on_every_axis_and_index() {}
#[test]
fn hex_terrain_is_refused_with_one_warning() {}
```

- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the Wang tests.** Expected: PASS.
- [ ] **Step 5: Docs** (`ryo-tilemap`'s terrain section), the gate, and commit.

```bash
git commit -m "feat: terrain painting works on isometric and staggered maps as Tiled's brush paints"
```

### Task 10: The speed targets on every grid

**Files:**
- Modify: `benches/tilemap.rs`
  - `map_of` takes a `GridShape`. The guards run per grid: `static_frame` (orthogonal, isometric 32x16, staggered 32x16, hex 16x16 side 8 and oblique 16x16 with skew (4, 0)), `edit` (every grid except hex, which has no collision) and `load` (a generated 256 x 256 four-layer TMX per grid).
  - One isometric static case uses tall art (32x48 tiles) on a plain layer, the spilling path, measured and reported.
  - The stream guard's world is isometric too, as a second case.
- Modify: `docs/backlog/tilemap.md` (three runs per guard: medians, 99th percentiles and maxima, with the load averages; Limits).
- Test: the guards themselves.

**Interfaces:**
- Consumes: everything above.
- Produces: nothing new.

- [ ] **Step 1: Add the per-grid cases.** Each one is held to its target, with the `guards::miss` lines naming its grid.
- [ ] **Step 2: Run each guard once** to see it measure. On a held miss, profile it with the command the guard prints, and fix the cost where it lives, each fix its own `perf:` commit with its before and after.
- [ ] **Step 3: Record three runs** back to back, on a machine that is quiet: `uptime`, no VM, no game. If the machine isn't quiet, record what you have and say so.
- [ ] **Step 4: Docs** (the record and Limits; the spilling path's cost), the gate, and commit.

```bash
git commit -m "test: the static, edit, load and streaming targets are guarded on every grid"
```

### Task 11: The demo's grid pages, with Kenney's CC0 art

**Files:**
- Create, in `examples/demos/tile-grids/src/`: `isometric.rs`, `staggered.rs`, `hexagonal.rs`, `oblique.rs` and `speed.rs`. Register the pages after the existing three in `src/lib.rs`, and update its header.
- Modify: `examples/demos/tile-grids/scripts/make-maps.js` and `src/bin/make-grid-maps.rs` (the new maps, written by Tiled), `assets/README.md` (files, art, licences, regeneration and the hand-play checklist) and `tests/pages.rs`.
- Add: Kenney art under `assets/`: an isometric pack (tall walls and trees taller than their cells) and a hexagon pack. Download each from kenney.nl, check its licence file says CC0, and record the pack, version, URL and licence in the README. If a pack can't be fetched, use another CC0 source and record it the same way.

**The pages** (demo standard: `examples/demos/AGENTS.md`; public API only; world labels as `Text2d`):
- **"An isometric town":** tall walls and trees on a y-sorted layer. A character walks in front of and behind them. The hovered cell is outlined (`LayerGrid::outline_world`). The town's walls collide, and terrain paints with the mouse (`set_terrain`).
- **"A staggered map":** a staggered-Y island with a character. The hovered cell is outlined, and a setting switches to a staggered-X copy.
- **"A hex board":**
  - hover picks the nearest hex;
  - a click selects one;
  - the board shows the hovered hex's neighbours, the selected hex's range (a setting for the radius), its ring, and the line to the hovered hex, with the distance in the HUD;
  - hex rotations are visible in the art.
- **"An oblique map":** a skewed map, with a setting for the skew between the map's own and its mirror. A character walks on it and collides.
- **"Speed on every grid":** a `choice` setting picks the grid. The page shows the 1024 x 1024 four-layer map flown over, and the HUD shows the frame time.

- [ ] **Step 1: Write the failing page tests:**

```rust
#[test]
fn every_page_plays_for_five_seconds_without_a_warning() { /* existing test, now eight pages */ }
#[test]
fn the_town_character_draws_behind_a_wall_and_in_front_of_it() { /* headless, pixel samples at both sides */ }
#[test]
fn the_hex_board_picks_the_hex_under_the_pointer_and_shows_its_range() { /* scripted pointer; the HUD's distance and the highlighted cells from MapsView::grid */ }
#[test]
fn the_oblique_character_collides_with_a_skewed_wall() {}
```

- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Build the pages and the maps.** Run `make-grid-maps` twice, and confirm the files are the same bytes.
- [ ] **Step 4: Run the demo's tests and its tour.** Look at every page's capture yourself, and describe them in the report.
- [ ] **Step 5: Docs.**
  - The demo's README.
  - `ryo-tilemap`: a short "maps on other grids" walkthrough that names the pages.
  - `docs/backlog/tilemap-features.md`: reduced to what stays later.
  - Then the gate, and commit.

```bash
git commit -m "feat: the tile-grids demo shows isometric, staggered, hexagonal and oblique maps, and their speed"
```
