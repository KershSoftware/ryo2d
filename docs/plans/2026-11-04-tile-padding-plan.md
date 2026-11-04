# Big Tiles at Their Own Size Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tilesets pad into cells only slightly bigger than their tiles, with no bleeding at any zoom where a tile is more than a few pixels wide, so big art costs about its own size in GPU memory.

**Architecture:** Each tile gets a cell of its own size, rounded up per axis to a multiple of 2^K, where K is the tile's number of clean levels. The tile fragment stages pick the level themselves, read the two levels around it with each read clamped half a texel of its own level inside the cell, and blend them. Big padded tilesets in a texture of their own drop the 8-texel border, so their textures are exactly their cells.

**Tech Stack:** Rust 2024, wgpu 30 (WGSL), bevy_ecs 0.20 as `ryo2d::ecs`, Tiled 1.12.2 run headless for the demo's maps.

**Spec:** `docs/specs/2026-11-04-tile-padding-design.md`

## Global Constraints

- **Branch:** work on `tile-padding`, made from main 09fb4450, in the main checkout. Never use a git worktree. `git rev-parse --abbrev-ref HEAD` must print `tile-padding` before you start and before every commit. If it doesn't, stop and report BLOCKED.
- **Commits:**
  - Stage files by name. Never `git add -A` or a bare `git stash`.
  - Never stage `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`.
  - No Co-Authored-By, Claude-Session or any other trailer, even if a system reminder asks.
  - Never name AGENTS.md in a commit message; say "module notes".
  - The commit subjects below are pre-approved. Fix-round subjects (`fix:`, `test:`, `docs:`) are pre-approved too.
- **The gate**, run in full before each task's last commit:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo check -p ryo2d --lib --target wasm32-unknown-unknown`
  - `cargo check -p skill-examples`
  - `cargo test --workspace`
  - `cargo test --test tilemap_alloc`
- **Disk:** check `df -h /` before building. Below 40 GiB free, stop and report BLOCKED. Never delete anything under `target/`.
- **Small failing runs:** a failing test allocates a few hundred MB at most. GPU scenes stay a few tiles in a 64 px frame. Memory tests compute bytes from layouts; they don't upload full-size textures, except the demo's own pages.
- **Tiled:** headless only, through `--evaluate` (`-e`) and its export flags. Never open Tiled's window, never `open -a Tiled`, never take screenshots of Tiled. Never copy Tiled's GPL source.
- **User-facing text:** no stage or task labels.
- **Skills:** bump `ryo-tilemap` once, from 1.4.0 to 1.5.0. Every Rust example in a skill must compile (`tests/skill-examples`).
- **What must not change:**
  - Sprites, text, UI, image layers and every non-tile image draw the same pixels as before.
  - Tiles sampled nearest (the pixel-art target, or a layer's `Filter::Nearest`) keep today's fragment stages (`fs_base*`).
  - Linear-sampled tiles at zoom 1 and above differ only by rounding.
- **Subagents:** dispatch none. No helper subagents, no reviewers.

## Review Focus

1. **A padded image right at the atlas's 256 px threshold.** It goes to the atlas with K capped at 3 and keeps its slot border, or to a texture of its own with K uncapped and no border. Either way it draws clean. Test: `a_padded_image_that_fits_the_atlas_caps_its_levels_at_three` (Task 2).
2. **Zooms of exactly 1/2 and 1/4**, the demo's new zooms. The level snaps to a whole number, and edges and bleed stay correct. Test: the zoom lists in Task 2's GPU tests include 0.5, 0.25 and 0.125 exactly.
3. **A layer scaled unevenly, and a turned camera.** The derivatives differ by axis, and the isotropic level must still keep reads inside the cell. Test: `a_squashed_layer_and_a_turned_camera_show_no_neighbour` (Task 2).
4. **Nearest sampling is untouched.** Pixel art and `Filter::Nearest` tile draws still use `fs_base*`. Test: `nearest_tile_draws_keep_the_sprite_fragment_stages` (Task 2).
5. **A tileset whose image changes size at run time** (hot reload), moving its padded copy between the atlas and a texture of its own. It keeps drawing clean. Test: `a_tileset_that_grows_past_the_atlas_pads_again_and_draws_clean` (Task 2).

---

### Task 1: Big padded tilesets drop the texture border

**Files:**
- Modify: `src/core/graphics/image.rs` (the `Image` struct, `prepare_upload`)
- Modify: `src/core/graphics/atlas.rs`: every standalone path that adds `BORDER`. That covers `add_standalone`'s slot UVs (~1258-1272), `Spread` (~400-440), the upload's size check and border (~755-800), `PreparedUpload` (`new`, `levels`, `fits`, `bytes`; ~1384-1475), and `standalone_levels` callers.
- Modify: `src/core/tilemap/padded.rs`: `texture_side_room`, the margin, `pad_tileset`'s image.
- Modify: `src/core/testing/gpu_world.rs`: the `max_side` assertion (~995-1001), and `a_large_tileset_draws_from_its_own_texture`'s comment and sizes (~1279-1283).
- Test: unit tests in `atlas.rs` and `padded.rs`.

**Interfaces:**
- Produces:
  - `pub(crate) fn Image::borderless(self) -> Image`: marks the image as one that, in a texture of its own, has no border.
  - `pub(crate) fn Image::is_borderless(&self) -> bool`.
  - `PreparedUpload::new(rgba: &[u8], width: u32, height: u32, border: u32) -> Option<PreparedUpload>`: the upload records its border, and `levels`, `fits` and `bytes` use it.
  - The rule: a borderless image in a texture of its own (big, or small and turned away by a full atlas) has border 0, slot UVs `[0, 0, 1, 1]`, a texture exactly its size, and levels `standalone_levels(width, height)`. In the shared atlas it keeps its slot border.
  - `padded::texture_side_room(max_texture_dimension_2d) -> u32` returns the dimension itself. Padded images are made with `.borderless()`.

- [ ] **Step 1: Write the failing tests**

In `atlas.rs` tests:

```rust
#[test]
fn a_borderless_image_of_its_own_texture_has_no_border() {
  // 300 x 40: over the atlas's 256 px, so a texture of its own
  // assert: slot.uv == [0.0, 0.0, 1.0, 1.0]
  // assert: the standalone texture is 300 x 40 with standalone_levels(300, 40) levels
  // assert: PreparedUpload::new(&rgba, 300, 40, 0).unwrap().bytes() == the chain of 300 x 40 alone
}

#[test]
fn a_borderless_image_in_the_atlas_keeps_its_slot_border() {
  // 64 x 64 marked borderless: placed on a page; slot.uv starts BORDER texels into its rect
}
```

In `padded.rs` tests:

```rust
#[test]
fn a_padded_image_of_its_own_texture_starts_its_cells_at_texel_zero() {
  // 100 tiles of 16 px: over 256 px, so its own texture
  // assert: the image is_borderless(); layout.margin == 0; cell_origin(0) == UVec2::ZERO
  // assert: layout.size == UVec2::new(10, 10) * layout.cell
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p ryo2d --lib -- borderless starts_its_cells_at_texel_zero`

Expected: compile errors (`borderless` and the new `PreparedUpload::new` argument don't exist), then assertion failures once stubbed.

- [ ] **Step 3: Implement it**

Thread a per-image border (`BORDER` or 0) through the standalone path. Each `Standalone` entry and `Spread` keeps the border its upload was made with. Write the margin as `(cell - border % cell) % cell` with the image's border, so cells start at texel 0 in a texture of their own. Set `PaddedTilesets::max_side` from `texture_side_room` (no border subtracted).

- [ ] **Step 4: Run the tests, the seam tests and the GPU tests**

Run:
- `cargo test -p ryo2d --lib -- borderless starts_its_cells_at_texel_zero`
- `cargo test -p ryo2d --lib tile_bleeding`
- `cargo test -p ryo2d --lib gpu_world`
- `cargo test --test tilemap_alloc`

Expected: PASS. Update `gpu_world.rs` so `max_side()` equals the device's side and `a_large_tileset_draws_from_its_own_texture`'s comment gives the new sizes. The 100 tiles of 16 px now make a 320 px image with no margin, still a texture of its own.

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/graphics/image.rs src/core/graphics/atlas.rs src/core/tilemap/padded.rs src/core/testing/gpu_world.rs
git commit -m "feat: a big tileset's padded copy has no texture border, so its texture is only as big as its cells"
```

---

### Task 2: Cells of their own size, read at the level the shader picks

**Files:**
- Modify: `src/core/tilemap/padded.rs`: the layout, its types, `paint`, `Unpadded`, the refusal warnings (~698-712), `PaddedTilesets` entries (a name for each), and the tests.
- Modify: `src/core/render/tiles.rs`: `instance`, `flipped_uv`, `object_instance`, the culling `reach` (~739), and the tests that read slots (~1748, ~1809, ~1960-1975). Also the module doc's lanes (~13-36).
- Modify: `src/core/shaders/include/sample.wgsl` (new `sample_cell`), `src/core/shaders/core/sprite.wgsl` (new `fs_tile`, `fs_tile_premultiplied`, `fs_tile_linear`), and `src/core/shaders/core/tile.wgsl` (`vs_tile` passes the levels).
- Modify: `src/core/render/batch/key.rs` (`BatchShader::TileObject`), `src/core/render/draw_list.rs` (`SpriteShading::TileObject` and its mapping to `BatchShader`), `src/core/render/batch/pipelines.rs` (`PipelineKey::sampling`, the tile entries), and `src/core/render/batch/gpu.rs` (where `PipelineKey`s are built). Add an arm to every exhaustive match on `BatchShader` or `SpriteShading`.
- Modify: `src/core/testing/tile_bleeding.rs` (new tests; the seam test's tile counts).
- Modify: `src/core/tilemap/AGENTS.md` (the `padded.rs` entry) and `src/core/render/AGENTS.md` (Tile chunks, Image layers and tile objects, the shader list).
- Modify: `.claude/skills/ryo-tilemap/SKILL.md`:
  - the "No bleeding at any zoom" bullet (~209);
  - "Keep big art small" (~110): its rule, here; the demo's numbers come in Task 4;
  - the "too big to pad" entry (~798): its rule, here;
  - `version: 1.5.0`.

**Interfaces:**
- Consumes (Task 1): `Image::borderless`, border 0 for padded textures of their own, and `max_side` with no border subtracted.
- Produces (in `padded.rs`; public and hidden, like the rest of the module):

```rust
/// Where a tile's picture sits: `size` texels from `origin`, both multiples of 2^levels,
/// so the cell is whole texels of every level up to `levels`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaddedCell { pub origin: UVec2, pub size: UVec2, pub levels: u32 }

pub struct PaddedTile {
  pub cell: PaddedCell,       // replaces `slot`
  pub size: UVec2,
  pub animation: Option<PaddedAnimation>,
  pub sort_offset: f32,
}

/// An animation's frames: equal cells (the largest frame's) row-major from `first`,
/// `columns` a row
pub struct PaddedAnimation { pub first: PaddedCell, pub columns: u32, pub frames: u32, pub frame_duration: f32 }
impl PaddedAnimation { pub fn frame(&self, k: u32) -> PaddedCell }

// PaddedLayout loses `cell`, `columns`, `margin` and `cell_origin`; keeps `size`, `offset`,
// `largest`, `tile_size`, `serial`, `tile(id)`; gains:
pub art_bytes: u64,                 // the tiles' drawn texels times 4, each picture once
pub largest_cell: UVec2,            // the widest and the tallest of its cells (the culling margin)
pub fn gpu_bytes(&self) -> u64      // the padded image with the levels its home keeps:
                                    // the atlas's 4, or a texture of its own's whole chain

pub(crate) fn clean_levels(size: UVec2, in_atlas: bool) -> u32
pub(crate) fn cell_for(size: UVec2, levels: u32) -> UVec2

/// Each kept padding's tileset name, padded bytes and art bytes (tests, the demo's tests)
pub struct PaddedSize { pub name: String, pub bytes: u64, pub art_bytes: u64 }
pub fn padded_sizes(world: &World) -> Vec<PaddedSize>
```

  - `Unpadded::TooBig { cells: u64, size: (u64, u64) }` (no `cell`). `Unpadded::TileTooBig { size: UVec2 }`: one tile's cell is over `max_side`.
  - Chunk instances carry the tile's levels in `transform[2][2]`. `vs_tile` passes them to the fragment stage as `effect_params.x`.
  - Tile objects are drawn with `SpriteShading::TileObject`, their levels in `effect_params[0]`. `vs_main` already passes `effect_params` through.
  - `PipelineKey` gains `sampling: Sampling`. It is the draw's sampling for `BatchShader::Tile` and `BatchShader::TileObject`, and `Sampling::Linear` for every other shader, so no other pipeline is built twice.
  - Entries, for `Tile` (the tile layout) and `TileObject` (the sprite layout):
    - nearest: `fs_base`, `fs_base_premultiplied` and `fs_base_linear`, as now;
    - linear: `fs_tile`, `fs_tile_premultiplied` and `fs_tile_linear`.

**The rule for K** (from the spec): `clean_levels` is the largest k for which, on each axis, `round_up(s, 2^k) - s <= s / 4` (integer: `4 * (round_up - s) <= s`). It is capped at 3 when `in_atlas`. `cell_for` rounds each axis up to a multiple of `2^levels`.

**Layout rules:**
- **Where the copy goes:** lay out first as a texture of its own (K uncapped). If both of the image's sides come out at 256 px or less, lay out again with K capped at 3. Capping K only shrinks cells, so the image stays in the atlas.
- **Tiles that share a size** (every tileset cut from one image, and uniform collections) go in a grid. Take the column count that gives the smallest image within `max_side`, the squarer one on a tie.
- **Mixed sizes:** pack by your own method, shelves by height for instance. Every cell starts on a multiple of its own 2^K, and the image's sides are multiples of the largest 2^K among its cells.
- **An animation's frames** take cells the size of `cell_for(largest frame, clean_levels(largest frame, ..))`. They sit in one block of `columns` columns, so the shader's frame step works with first column 0, first row 0 and the cell as its step. Its lanes are `[frames, seconds, step_u, step_v]` and `[0, 0, columns, 0]`. `uv_rect` and `uv_bounds` are frame 0's.
- **`paint`** centres a tile in its cell, which need not be square, and extrudes its edges, as now.

**Every reader of the old layout:**
- `flipped_uv` and `instance` read the tile's `PaddedCell`.
- The culling margin (`tiles.rs` ~739) reads `largest_cell`.
- `may_spill` keeps reading `largest`.

**The fragment stage's level and reads.** This is an algorithm the signature doesn't settle. In `sample_cell(t, s, uv, bounds, levels: f32, ddx, ddy)`:

```wgsl
// Transparent outside `bounds` (with sample_masked's hundredth-of-a-texel slack).
let dims = vec2<f32>(textureDimensions(t));
let rho = max(length(ddx * dims), length(ddy * dims));
var level = clamp(log2(max(rho, 1e-8)), 0.0, levels);
if abs(level - round(level)) < 1e-3 { level = round(level); }   // 1, 1/2, 1/4 read one level
let fine = floor(level);
let coarse = min(fine + 1.0, levels);
let a = textureSampleLevel(t, s, clamp(uv, bounds.xy + 0.5 * exp2(fine) / dims,
                                           max(bounds.xy + 0.5 * exp2(fine) / dims, bounds.zw - 0.5 * exp2(fine) / dims)), fine);
if level == fine { return a; }
let b = textureSampleLevel(t, s, clamp(uv, /* the same, with `coarse` */), coarse);
return mix(a, b, level - fine);
```

`fs_tile*` mirror `fs_base*`: they take the derivatives first and use `sample_cell` with `in.uv_bounds` and `in.effect_params.x`. `fs_tile_linear` reads a render-target page, as `fs_base_linear` does.

- [ ] **Step 1: Write the layout tests and watch them fail**

In `padded.rs`, replace the tests that pin the old rule:
- `cells_are_whole_texels_of_every_mip_level_behind_a_texture_s_border`
- `a_tile_of_any_size_gets_a_cell_with_room_for_every_mip_level`
- `a_collection_packs_each_tile_image_the_same_way`
- `an_animation_lays_its_frames_out_in_consecutive_cells`

with:

```rust
#[test]
fn clean_levels_keep_the_rounding_within_a_quarter() {
  let k = |x, y, atlas| clean_levels(UVec2::new(x, y), atlas);
  assert_eq!(k(16, 16, false), 4);  assert_eq!(k(16, 16, true), 3);
  assert_eq!(k(32, 32, false), 5);  assert_eq!(k(256, 256, false), 8);
  assert_eq!(k(129, 129, false), 5); assert_eq!(k(256, 352, false), 7);
  assert_eq!(k(120, 140, false), 5); assert_eq!(k(14, 14, false), 4);
  assert_eq!(k(14, 14, true), 3);   assert_eq!(k(1, 1, false), 0);
  assert_eq!(cell_for(UVec2::new(256, 352), 7), UVec2::new(256, 384));
  assert_eq!(cell_for(UVec2::new(120, 140), 5), UVec2::new(128, 160));
  assert_eq!(cell_for(UVec2::new(129, 129), 5), UVec2::new(160, 160));
  assert_eq!(cell_for(UVec2::new(14, 14), 3), UVec2::new(16, 16));
}

#[test]
fn every_cell_starts_on_its_own_alignment_and_none_overlap() {
  // tilesets: 14 of 256 x 352 (one tileset image); 112 of 120 x 140; a collection of one
  // 512 x 512, fifty 32 x 32 and three 100 x 60; an animation of frames 64 x 64 and 48 x 80
  // for every tile, still cell and animation frame cell:
  //   origin and size multiples of 2^levels; size >= the tile's; inside layout.size
  // no two cells overlap; layout.size a multiple of the largest 2^levels
}

#[test]
fn padded_bytes_stay_near_the_art() {
  // 14 of 256 x 352: gpu_bytes <= 2 * art_bytes (7 columns of 2: 1792 x 768)
  // 4 of 256 x 256: no rounding, so gpu_bytes <= art_bytes * 4 / 3 plus 0.1% (the art and its smaller copies)
  // the castle and fifty flowers: gpu_bytes <= 5 * art_bytes / 2
  // 64 of 16 px: every cell is 16 x 16
}

#[test]
fn an_animation_s_frames_take_cells_of_its_largest_frame_in_one_block() {
  // frames 64 x 64 and 48 x 80: every frame cell is cell_for((64, 80), clean_levels((64, 80), ..))
  // frame(k).origin == first.origin + ((k % columns) * w, (k / columns) * h)
}

#[test]
fn a_padded_image_that_fits_the_atlas_caps_its_levels_at_three() {
  // 9 tiles of 64 px -> 192 x 192: in the atlas, every cell's levels == 3, cells 64 px
  // 25 tiles of 64 px -> 320 x 320: a texture of its own, levels == 6, borderless
}
```

Keep `a_padded_tileset_extrudes_every_tile_into_its_own_cell`, rewritten for a non-square cell: the tile centred, its edge texels out to the cell's edges.

Run: `cargo test -p ryo2d --lib padded`

Expected: FAIL (the new names don't exist; the old layout's numbers).

- [ ] **Step 2: Write the GPU tests; they pass on the old code**

In `tile_bleeding.rs`, using the existing `seams` helper and `NEIGHBOURS` colours:

```rust
#[test]
fn big_tiles_and_mixed_collections_show_no_neighbour_at_any_zoom() {
  // linear target; tilesets: 30 tiles of 40 x 56 (cells 48 x 64, K 4, a texture of its own),
  // a mixed collection (one 72 x 72 among twenty 12 x 20), 9 tiles of 40 x 56 (the atlas)
  // zooms [1.0, 0.5, 0.25, 0.125, 0.0625, 0.7, 0.3] at quarter-pixel offsets
  // where a tile is >= 4 px on its shorter side: no pixel holds a neighbour's colour
}

#[test]
fn turned_flipped_and_animated_tiles_and_tile_objects_show_no_neighbour() {
  // linear target, zooms [1.0, 0.5, 0.25, 0.125]: a hex layer with HEX_ROTATE_60 and 120 tiles,
  // an orthogonal layer with H, V and diagonal flips, an animated tile of two frame sizes,
  // a y-sorted layer and a tile object of a big tile; no neighbour's colour
}

#[test]
fn a_squashed_layer_and_a_turned_camera_show_no_neighbour() {
  // a layer whose Transform scales (1.0, 0.25), and a camera turned 30 degrees, zooms [1.0, 0.5, 0.25]
}

#[test]
fn a_tile_zoomed_out_a_little_keeps_its_outer_texels_sharp() {
  // one 16 x 16 tile: outer ring texels R = [230, 20, 20, 255], inner G = [20, 200, 20, 255],
  // linear target, zoom 0.75, the tile's left edge on a whole pixel
  // the outermost pixel column is nearer R than G (squared distance in RGB)
}
```

In `no_seams_at_any_zoom_or_subpixel_camera`, raise the "own textures" cases' extra tiles so each padded image still passes 256 px under the new layout. Keep each case's purpose, and its `standalone_entity_count() == 2` assertion.

Run: `cargo test -p ryo2d --lib tile_bleeding`

Expected: PASS on the old code. The old layout is clean everywhere; say so in the report.

- [ ] **Step 3: Implement the layout and instances; watch the bleed tests fail**

Implement the layout rules above, `paint`, `padded_sizes`, the instances' cells and levels lanes, and `object_instance` with `SpriteShading::TileObject`. Leave the fragment stages as they are, so `BatchShader::TileObject` maps to `fs_base*` for now.

Run: `cargo test -p ryo2d --lib -- padded tile_bleeding`

Expected:
- the layout tests PASS;
- `big_tiles_and_mixed_collections_show_no_neighbour_at_any_zoom`, and the zoomed-out cases of the others, FAIL with neighbour colours: the new layout under the old sampling.

Put that output in the report. This is the spec's required failing run.

- [ ] **Step 4: Implement the tile fragment stages and pipelines; watch them pass**

Add `sample_cell`, `fs_tile*`, the levels in `vs_tile`, `PipelineKey::sampling` and the entry table. Add this test in `pipelines.rs`:

```rust
#[test]
fn nearest_tile_draws_keep_the_sprite_fragment_stages() {
  // the entry for (Tile, Quads, straight, image page, Nearest) is "fs_base"; with Linear "fs_tile";
  // the same for TileObject; Sprite with either sampling builds one pipeline (sampling normalised)
}
```

Run: `cargo test -p ryo2d --lib -- padded tile_bleeding pipelines`

Expected: PASS.

- [ ] **Step 5: Show the sharp-edge test has teeth**

Make `sample_cell` take one `textureSampleLevel` at the fractional level, inset for the coarser level. Run `a_tile_zoomed_out_a_little_keeps_its_outer_texels_sharp` and watch it fail, then restore the file with `git checkout -- src/core/shaders/include/sample.wgsl`. Put both runs in the report.

- [ ] **Step 6: The reload case**

```rust
#[test]
fn a_tileset_that_grows_past_the_atlas_pads_again_and_draws_clean() {
  // a code-built tileset of 9 tiles of 40 px (the atlas), drawn at zoom 0.25; its image
  // replaced by one of 36 tiles (a texture of its own); after the reload it draws with no
  // neighbour colour, and padded_sizes() shows the new bytes
}
```

Run it; expected PASS. If it fails, fix the cause and say so in the report.

- [ ] **Step 7: Docs**

Update the module notes and `ryo-tilemap` as listed under Files:
- the rule (cells a little bigger than their tiles, levels per tile, no border on a texture of its own, the shader's level and clamp);
- the guarantee (no neighbour colour while a tile is more than a few pixels wide; below that a slight shimmer);
- the new "too big to pad" capacities, computed from `max_side` 8,192 with no border.

Bump `version: 1.5.0`.

- [ ] **Step 8: Gate and commit**

Run the gate. Commit the code and tests, then the docs:

```bash
git add src/core/tilemap/padded.rs src/core/render/tiles.rs src/core/render/draw_list.rs \
  src/core/render/batch/key.rs src/core/render/batch/pipelines.rs src/core/render/batch/gpu.rs \
  src/core/shaders/include/sample.wgsl src/core/shaders/core/sprite.wgsl src/core/shaders/core/tile.wgsl \
  src/core/testing/tile_bleeding.rs
git commit -m "feat: each tile pads into a cell a little bigger than itself, and the tile shader reads the level it needs inside it"
git add src/core/tilemap/AGENTS.md src/core/render/AGENTS.md .claude/skills/ryo-tilemap/SKILL.md
git commit -m "docs: the tile-map docs give the padding's new rule and what it guarantees"
```

Stage any other file you had to touch (an exhaustive match elsewhere) by name with the first commit.

---

### Task 3: The demo draws Kenney's art at full size

**Files:**
- Modify: `examples/demos/tile-grids/scripts/derive-art.sh`:
  - Sketch Town: stop halving. Keep the cut to each used picture's bottom 256 rows (the 96 above are empty), so pictures are 256 x 256.
  - Hexagon Pack: cut the board's 24 hexagons at full size (120 x 140), six a row, into `hexagon-board.png` (720 x 560).
- Modify: `examples/demos/tile-grids/scripts/make-maps.js`:
  - isometric and staggered cells of 232 x 110 (`ISO_W`, `ISO_H`);
  - the Sketch Town tilesets' tile size and offsets doubled;
  - the hex board's 120 x 140 hexagons with side 70;
  - every position doubled (`player_start`, objects, the island's `centre()` steps, in whole pixels as now).
- Regenerate: `assets/kenney-sketch-town/*.png`, `assets/kenney-hexagon-pack/hexagon-board.png`, `assets/tilesets/sketch-town-ground.tsx`, `sketch-town-props.tsx`, `hexagon-board.tsx`, and `assets/maps/town.tmx`, `island-rows.tmx`, `island-columns.tmx`, `hex-board.tmx`.
- Modify: `examples/demos/tile-grids/src/isometric.rs` (`ZOOMS` becomes `[0.25, 0.5]`), `staggered.rs` (`ZOOM` 0.25) and `hexagonal.rs` (`ZOOM` 0.5). Also anything those pages and `walker.rs` or `shared.rs` place in world pixels: the knight's size and speed, `Text2d` sizes, outline widths. Each page should look on screen as it does now.
- Modify: `examples/demos/tile-grids/tests/pages.rs` and `tests/grids.rs`: world-pixel expectations doubled, `the_town_walls_stop_the_character` included.
- Modify: `examples/demos/tile-grids/assets/README.md`: the art rows, the derived files at full size, and the hand-play checklist. Remove the section on why the pictures are halved; the memory figures come in Task 4.

**Interfaces:**
- Consumes (Task 2): `ryo2d::core::tilemap::padded::padded_sizes(&World) -> Vec<PaddedSize>`.

- [ ] **Step 1: Write the failing tests**

In `tests/pages.rs`, with the existing headless page helpers in `tests/common`:

```rust
#[test]
fn the_town_draws_kenney_s_pictures_at_their_own_size() {
  // open the town page and wait for its maps to land
  // the ground tileset's tile_size == UVec2::new(256, 256)
  // padded_sizes(world): the ground and props tilesets (by their names in the .tsx files) each bytes <= 2 * art_bytes,
  // together <= 20 * 1024 * 1024
}

#[test]
fn the_hex_board_draws_its_hexagons_at_their_own_size() {
  // the hexagon-board tileset's tile_size == UVec2::new(120, 140);
  // its padded bytes <= 2 * art_bytes and <= 8 * 1024 * 1024
}
```

Run: `cargo test -p tile-grids-demo --test pages own_size`

Expected: FAIL. Tile sizes are 128 x 128 and 60 x 70.

- [ ] **Step 2: Remake the art and maps**

Run `scripts/derive-art.sh` as the README says, with the packs unzipped in the scratchpad folders recorded there. If they're gone, fetch them again into new, empty folders and check the sha256 the README records. Then run `cargo run -p tile-grids-demo --bin make-grid-maps` twice and confirm the same bytes both times. Tiled runs headless only.

- [ ] **Step 3: Scale the pages and their tests**

Change the zooms and every world-pixel size or speed on the town, island and hex pages so each looks as it does now. Update the page and grid tests' world-pixel expectations.

- [ ] **Step 4: Run the demo's tests and look at the pages**

Run: `cargo test -p tile-grids-demo`

Expected: PASS, including `every_page_plays_for_five_seconds_without_a_warning` and `the_town_walls_stop_the_character`.

Capture pages 4 to 6, both zooms of the town and both grids of the island, from the demo's folder. Use `RYO_HEADLESS=1`, `RYO_HEADLESS_HZ=60`, `RYO_CAPTURE_FRAMES=<n>`, `RYO_CAPTURE_EXIT=1`, `RYO_CAPTURE_DIR=<abs dir under the scratchpad>` and `RYO_INPUT` for the page keys. Look at each capture and describe it in the report against today's: where things stand, what is in front, whether outlines sit on their cells, and any seam or neighbour colour.

- [ ] **Step 5: README, gate and commit**

```bash
git add examples/demos/tile-grids/scripts/derive-art.sh examples/demos/tile-grids/scripts/make-maps.js \
  examples/demos/tile-grids/assets/kenney-sketch-town examples/demos/tile-grids/assets/kenney-hexagon-pack \
  examples/demos/tile-grids/assets/tilesets/sketch-town-ground.tsx examples/demos/tile-grids/assets/tilesets/sketch-town-props.tsx \
  examples/demos/tile-grids/assets/tilesets/hexagon-board.tsx examples/demos/tile-grids/assets/maps/town.tmx \
  examples/demos/tile-grids/assets/maps/island-rows.tmx examples/demos/tile-grids/assets/maps/island-columns.tmx \
  examples/demos/tile-grids/assets/maps/hex-board.tmx examples/demos/tile-grids/src examples/demos/tile-grids/tests \
  examples/demos/tile-grids/assets/README.md
git commit -m "feat: the grid pages draw Kenney's town and hexagons at their own size"
```

Check `git status` first. Stage only the files named here and any other file you changed in those folders, by name. Remove a halved picture no page uses any more with `git rm`.

---

### Task 4: The record and the Limits

**Files:**
- Modify: `docs/backlog/tilemap.md`:
  - Rewrite the Limit "Big tiles pad to a cell of about twice their side" (~471) into the new rule with measured figures.
  - Add a Limit: below its last clean level, a tile reads that level, which can shimmer slightly as the camera moves (a tile then a few pixels wide).
  - Add rows to the record: padded bytes and landing time for each of the demo's tilesets, before (the record's 577 MiB, 0.2 to 0.4 s) and after.
  - Re-run the held guards.
- Modify: `.claude/skills/ryo-tilemap/SKILL.md`: the demo's figures in the big-art paragraph. The version is already bumped.
- Modify: `examples/demos/tile-grids/assets/README.md`: the memory figures.
- Modify: `docs/backlog/follow-ups.md`: in "Tile grid shapes", remove the owner-decision line naming the big-tile padding as next, and the ruling that it was fixed in the demo. Then check the whole file for other lines this work fixed.
- Modify: `docs/backlog/tilemap-features.md`: drop "big tiles' padding" from the last bullet.

- [ ] **Step 1: Measure**
  - **Padded bytes:** from `padded_sizes` on the town, island and hex pages, in a release build.
  - **Landing time:** from opening each page to its maps landing, headless at `RYO_HEADLESS_HZ=60`, three runs each, measured the way the record's earlier landing figures were.
  - **The guards:** `static_frame`, `edit`, `load` and `stream` in `benches/tilemap.rs`, three runs each on a quiet machine, as the record's last runs. Note the machine's load. A held target that misses on a quiet machine is reported, never hidden.

- [ ] **Step 2: Write the record, the Limits and the docs**

Every number quoted in a Limit must match a record row in the same file.

- [ ] **Step 3: Gate and commit**

```bash
git add docs/backlog/tilemap.md docs/backlog/follow-ups.md docs/backlog/tilemap-features.md \
  .claude/skills/ryo-tilemap/SKILL.md examples/demos/tile-grids/assets/README.md
git commit -m "docs: big tiles' memory and landing times are in the record, and their Limit gives the new rule"
```

---

## After the tasks

The controller does the close-out: the whole-branch review, then the scripted tour comparison against main (spec, "No visible change"):
- every page that loads no tileset is byte-identical;
- pages with tiles differ only by rounding, every difference looked at;
- the owner hand-plays the grid pages.

Then come the merge question and the follow-ups.
